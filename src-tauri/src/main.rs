//! One binary, two roles, over one state.
//!
//! `crm app` is the window Clatch launches through the manifest's `launch`; `crm <verb>`
//! is the agent's CLI. `clappkit::role::main_dispatch` decides which at startup, so this
//! clapp ships one executable and the two surfaces cannot be built from different code.
//!
//! Everything here is transport and wiring. The rules live in [`state`], which is pure;
//! the vocabulary lives in [`model`]; the disk lives in [`store`]; and the agent's manual
//! lives in [`cli`].

mod cli;
mod model;
mod state;
mod store;

use clappkit::app::Reply;
use clappkit::window::WindowPolicy;
use model::{Ctx, Date, InstanceId, Now};
use serde_json::{json, Value};
use state::split::Op;
use state::AppState;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use store::{load_or_mint_instance_id, load_or_mint_person, CrmStore, JsonStore, SaveQueue};
use tauri::Manager;
use tokio::sync::Mutex;

pub(crate) const APP_ID: &str = "com.breksos.crm";
pub(crate) const CLI: &str = "crm";

/// How long the dataset must go unchanged before it is written. Long enough to swallow a
/// card dragged across a board — one save, not sixty — short enough that a crash costs one
/// gesture and not an afternoon.
const SAVE_QUIET: Duration = Duration::from_millis(400);

/// How long the process's last act — flushing the writer — may take. Under Clatch the
/// shutdown hook has one second before clappkit exits anyway, so this stays inside it.
const FLUSH_WAIT: Duration = Duration::from_millis(800);

/// How often the due-task timer looks. A due date changes at most once a day, so a few
/// minutes is ample and anything tighter is a bug (`docs/architecture.md` §8, §11). The
/// number lives in the core, which says it to the person; this is only the loop's use of it.
const SWEEP_EVERY: Duration = Duration::from_secs(state::SWEEP_EVERY_MINUTES * 60);

/// The wait before the launch sweep. Clatch pushes the agent roster a moment after the app
/// registers, and a sweep that ran first would find nobody to tell and hold the catch-up
/// back for a whole interval. A few seconds is the cheapest way to let it land.
const LAUNCH_GRACE: Duration = Duration::from_secs(3);

/// The app's own mark, for the Dock (macOS) and the taskbar (Windows/Linux). The bytes are
/// ours because they *are* our identity; clappkit insets a full-bleed tile to the native
/// grid at runtime.
const ICON: &[u8] = include_bytes!("../../assets/icon.png");

fn main() {
    clappkit::role::main_dispatch(APP_ID, CLI, cli::run, gui);
}

/// Everything both surfaces share: the one state, the live control pipe, and the writer.
struct Core {
    state: Mutex<AppState>,
    control: clappkit::Control,
    saves: SaveQueue,
    /// Which install this is. Minted once on first run and stamped onto every mutable
    /// record this process writes.
    origin: InstanceId,
    shape: Shape,
    /// Standalone has no roster, so a test that needs an agent connected supplies one.
    #[cfg(test)]
    test_roster: std::sync::Mutex<Option<Vec<clappkit::AgentRow>>>,
}

/// Which shape the window is pushed. **A flag, because p1 publishes both** (`p1-the-split.md`
/// §8): `classic` is today's whole snapshot and is the default; `split` pushes only an
/// [`update`](state::AppState::update) — the seat's view and the row actions since — and
/// answers every command with the same update under `split`. Chosen with `CRM_SHAPE=split`.
/// The classic fields stay in every *response* either way, because the CLI reads them; the flag
/// decides what the **window** is handed. It is deleted with the classic shape, in its own
/// commit, once nothing reads that shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Shape {
    #[default]
    Classic,
    Split,
}

impl Shape {
    fn from_env() -> Shape {
        match std::env::var("CRM_SHAPE").ok().as_deref().map(str::trim) {
            Some("split") => Shape::Split,
            _ => Shape::Classic,
        }
    }
}

/// What [`Core::commit`] hands back: the mutation's result, the dataset as it now stands, and
/// the row actions the commit took (empty for a read, a navigation or a refusal).
struct Committed<T> {
    value: Result<T, NotSaved>,
    snapshot: Value,
    update: Option<Value>,
    ops: Vec<Op>,
    watermark: u64,
}

/// Put the numbers a commit took into the update the core built before it knew them. The view
/// does not depend on them, so the (expensive) projection is built once and **patched**.
fn patch_update(update: &mut Value, watermark: u64, ops: &[Op]) {
    if !update.is_object() {
        return;
    }
    update["watermark"] = json!(watermark);
    update["delta"] = json!({
        "from": watermark.saturating_sub(ops.len() as u64),
        "to": watermark,
        "ops": ops.iter().map(Op::json).collect::<Vec<_>>(),
    });
}

/// Why a change could not be committed. The only thing a caller may say about it.
#[derive(Debug)]
struct NotSaved;

const NOT_SAVED: &str =
    "not saved — the write did not reach the disk, so nothing was changed (check the disk's space and permissions)";

impl Core {
    fn roster(&self) -> Vec<clappkit::AgentRow> {
        #[cfg(test)]
        if let Some(r) = self.test_roster.lock().unwrap().clone() {
            return r;
        }
        self.control.roster()
    }

    /// **The commit point.** Every change to the dataset — a CLI verb, a window edit, a
    /// reminder's mark — passes through here, and nothing else touches the state.
    ///
    /// 1. It runs `mutate` under the one lock, against a copy kept to roll back to.
    /// 2. It decides *whether anything changed* by comparing the dataset with that copy
    ///    ([`Db::change_from`]) — never by the shape of the answer, which is how `import`
    ///    (a read-shaped answer over a bulk write) was once acknowledged and never saved.
    /// 3. A change to the data is written **through** before this returns, and the lock is
    ///    held across the write, so two commands cannot reach the disk out of order. If the
    ///    write fails the change is rolled back and the caller gets `Err(NotSaved)`: the
    ///    acknowledgement is *derived from* the persist result, so no door, verb or answer
    ///    type can opt out of it.
    /// 4. Navigation alone (focus, the shared list) is kept on the debounced writer: it is
    ///    not data, and a failing disk must not refuse a `find`.
    ///
    /// Returns what `mutate` returned plus the dataset's snapshot as it stands *after* the
    /// outcome — the rolled-back one on failure.
    async fn commit<T>(
        &self,
        now: Now,
        mutate: impl FnOnce(&mut AppState) -> T,
    ) -> Committed<T> {
        let mut state = self.state.lock().await;
        let before = state.db();
        let out = mutate(&mut state);
        let mut ops = Vec::new();
        match state.db_ref().change_from(&before) {
            model::Change::None => {}
            model::Change::View => self.saves.save(state.db()),
            model::Change::Data => {
                // **Numbered inside the commit, before the write**, so the watermark reaches
                // the disk with the rows it numbers.
                ops = state.stamp(&before);
                if !self.saves.write_through(state.db(), FLUSH_WAIT).await {
                    let _ = writeln!(std::io::stderr(), "{CLI}: could not confirm a write reached the disk; rolled it back");
                    state.roll_back_to(before);
                    let update = Some(state.update(now, &[]));
                    return Committed {
                        value: Err(NotSaved),
                        snapshot: state.snapshot(now),
                        update,
                        ops: Vec::new(),
                        watermark: state.watermark(),
                    };
                }
                // Visible to a client asking what it missed only once it is safely on the disk.
                state.publish(ops.clone());
            }
        }
        let update = (self.shape == Shape::Split).then(|| state.update(now, &ops));
        Committed { value: Ok(out), snapshot: state.snapshot(now), update, ops, watermark: state.watermark() }
    }
}

impl Core {
    /// Apply one command from either surface.
    ///
    /// `caller` is the agent id for a CLI call and `None` for the person at the window.
    /// The response and the snapshot come out of **one** lock, so they cannot describe
    /// different moments.
    async fn command(&self, req: Value, caller: Option<String>) -> Reply {
        // The roster is Clatch's, not ours, and it rides the snapshot because the window
        // draws it. Handing it to the core keeps the core free of anything it would have
        // to reach out to fetch — and the same goes for the clock.
        let roster = self.roster();
        let (now, offset_secs) = clock();
        let ctx = Ctx { now, entropy: entropy(), origin: self.origin.clone(), offset_secs };

        let committed = self
            .commit(now, |state| {
                state.set_agents(roster);
                state.command(&req, caller.as_deref(), &ctx)
            })
            .await;
        let (snapshot, update) = (committed.snapshot, committed.update);

        match committed.value {
            // Persisted (or nothing to persist). Only now is it acknowledged — and only a
            // human's action signals: an agent is never told about its own write.
            Ok(out) => {
                if caller.is_none() {
                    self.control.emit_all(out.emits);
                }
                let mut resp = out.resp;
                // The core built the update before the commit numbered it: put the numbers in.
                patch_update(&mut resp["split"], committed.watermark, &committed.ops);
                // Under the flag the window is pushed the update, never the whole state.
                let push = match update {
                    Some(update) => {
                        resp["split"] = update.clone();
                        update
                    }
                    None => out.snapshot,
                };
                Reply::new(resp, push)
            }
            // A refusal, in the shape every other refusal has, over the state as rolled back.
            Err(NotSaved) => {
                let mut resp = json!({ "ok": false, "error": NOT_SAVED });
                // The state as rolled back: the refusal says what the surfaces still hold.
                let update = update.expect("a refusal always carries the rolled-back update");
                resp["split"] = update.clone();
                let push = if self.shape == Shape::Split { update } else { snapshot };
                Reply::new(resp, push)
            }
        }
    }
}

impl Core {
    /// One pass of the due-task timer, and the only place `task.due` leaves the app.
    ///
    /// A task coming due is the clock's doing rather than a person's, which is why this is
    /// the one emit outside `command` — the sanctioned exception in `docs/architecture.md`
    /// §8. Everything it decides is the core's; this reads the clock, sends what the core
    /// hands back, and tells the window the sweep ran.
    async fn sweep(&self, app: &tauri::AppHandle) {
        let (now, offset_secs) = clock();
        let sweep = self.sweep_once(self.roster(), now, offset_secs).await;
        clappkit::app::push_state(app, sweep.snapshot);
    }

    /// The whole of a sweep short of the window: the roster and the clock come in, the
    /// marks are queued for the disk, and the signal goes out. Split from [`sweep`] so a
    /// test can run it without a webview.
    async fn sweep_once(&self, roster: Vec<clappkit::AgentRow>, now: Now, offset_secs: i32) -> state::Sweep {
        let ctx = Ctx { now, entropy: entropy(), origin: self.origin.clone(), offset_secs };

        // **Write-ahead, through the one commit point.** The mark that says "sent" reaches
        // the disk *before* the signal leaves, so a stop at any instant — even one that
        // gives the process no time to flush — can never send it twice.
        let committed = self
            .commit(now, |state| {
                state.set_agents(roster);
                // Refusals are read at the top of the sweep, before the core decides what is
                // still waiting: a refused batch goes back to waiting and is retried in this
                // very pass. Not a poll of anything — the loop was already running.
                for (signal, agent, reason) in take_refusals(&self.control) {
                    state.note_refusal(&signal, &agent, &reason, &ctx);
                }
                state.sweep(&ctx)
            })
            .await;
        let update = committed.update;

        match committed.value {
            Ok(mut sweep) => {
                self.control.emit_all(sweep.emits.clone());
                // What the window is pushed after a sweep: the update, under the flag.
                if let Some(update) = update {
                    sweep.snapshot = update;
                }
                sweep
            }
            Err(NotSaved) => {
                // The mark could not be recorded, so the signal is withheld: sending without
                // it is how a reminder arrives twice. One late, never two.
                let _ = writeln!(std::io::stderr(), "{CLI}: could not record a reminder; not sending it");
                let mut state = self.state.lock().await;
                state.unsend();
                let update = state.update(now, &[]);
                let snapshot = if self.shape == Shape::Split { update.clone() } else { state.snapshot(now) };
                state::Sweep { emits: Vec::new(), update, snapshot }
            }
        }
    }
}

/// The refusals Clatch has reported since the last look: `(signal id, agent id, reason)`.
///
/// **Wired to nothing, on purpose, until clappkit gives it something to read.**
/// `app.toAgentRefused` is in the protocol and the core handles it
/// ([`state::AppState::note_refusal`], tested), but `clappkit::Control` — which does not use
/// the reference `clapp_pipe::Client` at all — drops that notification: its serve loop
/// matches only `app.agents`, and its comment says "the roster; refusals" over a branch that
/// exists for the roster alone. Checked again at pin `2cde169`. There is no workaround that
/// detects a refusal: `Control` retains nothing, and a second control connection is
/// impossible (one endpoint per instance, one-time token). The day `Control` exposes an
/// accessor, this returns it and nothing else changes.
///
/// **So the app does not pretend.** It records that a reminder was *sent*, never that it
/// was delivered; `crm status` says the platform does not confirm delivery; and the person
/// — who knows whether their agent acted — can send it again (`{cmd: "resend"}`).
fn take_refusals(_control: &clappkit::Control) -> Vec<(String, String, String)> {
    Vec::new()
}

/// Eighty bits of OS randomness, for the ids one command mints.
///
/// One draw per command, not per record: [`model::Ulid::next_after`] makes the second and
/// later ids of a command monotonic rather than identical, which is both cheaper and the
/// behaviour ULID specifies.
///
/// A failing entropy source is not survivable by carrying on — ids would stop being
/// unique, and a CRM that quietly reuses an identity loses records. It is also not a thing
/// that fails on a working machine.
fn entropy() -> [u8; 10] {
    let mut bytes = [0u8; 10];
    getrandom::fill(&mut bytes).expect("the OS must be able to provide randomness");
    bytes
}

/// The clock, read here so the core never has to — and read **once**.
///
/// One read gives both the instant and the zone offset in effect at that instant, and the
/// date is derived from those two. Reading them separately is how a command at
/// 23:59:59.999 ends up carrying today's instant and tomorrow's date — a snapshot nobody
/// can reproduce, and a due bucket that disagrees with its own timestamp.
///
/// The offset comes back too, folded into [`Ctx`] beside `now` — it is what lets the core
/// place a *different* date (`crm log … --at 2026-09-10`) on the same calendar without a
/// clock or a zone of its own.
fn clock() -> (Now, i32) {
    let now = chrono::Local::now();
    let at = now.timestamp_millis();
    let offset_secs = now.offset().local_minus_utc();
    (Now { at, today: local_date(at, offset_secs) }, offset_secs)
}

/// The civil date an instant falls on, `offset_secs` east of UTC.
///
/// A due date is a human calendar concept: a task due "today" in Istanbul is not due on
/// UTC's today, and a reminder that fires on the wrong day reads as the app being
/// unreliable — the worst kind of bug for a CRM.
///
/// **Pure on purpose.** The offset is handed in rather than looked up, so the tests below
/// pin `+14:00`, `-12:00` and the 23:00-local-is-tomorrow-in-UTC case on any machine —
/// including a CI runner set to UTC, where a test that read the real zone could never
/// fail.
fn local_date(at_ms: i64, offset_secs: i32) -> Date {
    // Floor-divide throughout, so an instant before the epoch, or a negative offset, is
    // not rounded towards zero into the wrong day.
    let local_secs = at_ms.div_euclid(1000) + offset_secs as i64;
    civil_from_days(local_secs.div_euclid(86_400))
}

/// Howard Hinnant's `civil_from_days` — the inverse of `model::Date::days_until`'s
/// arithmetic. Pure integers: the zone is what `local_date` adds, not the calendar.
fn civil_from_days(days: i64) -> Date {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    Date { y: (if m <= 2 { y + 1 } else { y }) as i32, m: m as u32, d: d as u32 }
}

/// The window's one call into the core. The person is never an agent, so the caller id is
/// `None` and their actions are the ones that signal.
#[tauri::command]
async fn run_cmd(core: tauri::State<'_, Arc<Core>>, req: Value) -> Result<Value, String> {
    // Bound rather than chained: the future must not borrow a temporary that ends at the
    // semicolon.
    let core = core.inner().clone();
    Ok(core.command(req, None).await.resp)
}

/// A roster avatar as a `data:` URI, because a webview cannot open `file://` and Clatch
/// hands out paths rather than bytes.
///
/// **Only a path the roster itself published is read.** The webview is not trusted to name a
/// file: binding this straight to a path-taking reader would make it "base64 me any file on
/// this machine" — which is what it was before clappkit's K1, when this called
/// `clappkit::app::asset`. `avatar_uri` builds its allow-list from the live roster.
#[tauri::command]
fn asset(path: String, core: tauri::State<'_, Arc<Core>>) -> Option<String> {
    clappkit::app::avatar_uri(&path, &core.control)
}

fn gui() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![run_cmd, asset])
        .setup(|app| {
            let handle = app.handle().clone();

            // `setup` is the main thread, which is where AppKit will accept this.
            clappkit::app::apply_icon(&handle, ICON);

            // Read the dataset before anything can ask for it. A load failure here is
            // fatal on purpose: opening a window onto an empty CRM when the person's data
            // is sitting on disk, unreadable, would look exactly like losing it.
            let store: Arc<dyn CrmStore> = Arc::new(JsonStore::in_data_dir());
            let db = match store.load() {
                Ok(db) => db,
                Err(e) => {
                    eprintln!("{CLI}: cannot open your data: {e:#}");
                    std::process::exit(1);
                }
            };

            // This install's identity, in its own file beside the data — never inside it,
            // or a synced copy would claim to be the install that wrote it.
            let origin = match load_or_mint_instance_id(
                &clappkit::paths::data_file(CLI, "instance"),
                || {
                    let mut b = [0u8; 16];
                    getrandom::fill(&mut b).expect("the OS must be able to provide randomness");
                    b
                },
            ) {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("{CLI}: cannot establish this install's identity: {e:#}");
                    std::process::exit(1);
                }
            };

            // Tauri's runtime, not a bare `tokio::spawn`: `setup` is the main thread and
            // has no tokio context of its own. Made **before** the control pipe, because
            // the shutdown hook below needs to hold it.
            let (saves, writer) = SaveQueue::channel(store, SAVE_QUIET);
            tauri::async_runtime::spawn(writer);

            // Register on the control pipe, on Tauri's own runtime so the reactive loop
            // shares it. Fatal on failure: an app that cannot reach Clatch has no agent
            // half, and a window pretending otherwise is worse than no window.
            //
            // **`clatch stop` ends the process from here**: clappkit answers
            // `app.shutdown`, runs this hook (one second at most), and exits. The writer
            // is debounced, so without this an acknowledged write that is under
            // `SAVE_QUIET` old dies with the process.
            let hook_saves = saves.clone();
            let control = tauri::async_runtime::block_on(clappkit::connect_or_die_with(
                CLI,
                Arc::new(move |cause| {
                    on_shutdown(&hook_saves, cause, &mut std::io::stderr());
                }),
            ));

            // The timer arrived after the data did: the first open of an older dataset
            // marks what was already overdue as told, quietly (see `AppState::open`).
            // Who is sitting here: its own file beside the data, for the same reason as the
            // install id. Named after the OS user until there is a Settings control for it.
            let me = match load_or_mint_person(
                &clappkit::paths::data_file(CLI, "seat.json"),
                || {
                    let mut b = [0u8; 16];
                    getrandom::fill(&mut b).expect("the OS must be able to provide randomness");
                    b
                },
                &std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default(),
            ) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{CLI}: cannot establish who is at this seat: {e:#}");
                    std::process::exit(1);
                }
            };
            let (state, migrated) = {
                let (now, offset_secs) = clock();
                AppState::open_as(db, &Ctx { now, entropy: entropy(), origin: origin.clone(), offset_secs }, me)
            };
            if migrated {
                // Written through, like every other change: the marks say "nobody needs telling",
                // and a launch that forgot them would wake the agent about the backlog next time.
                saves.flush(Some(state.db()), FLUSH_WAIT);
            }

            // Held in Tauri's state for `RunEvent::Exit`, which is how `crm close` and a
            // quit from the window end the process.
            app.manage(ExitFlush(saves.clone()));

            let core = Arc::new(Core {
                state: Mutex::new(state),
                control,
                saves,
                origin,
                shape: Shape::from_env(),
                #[cfg(test)]
                test_roster: Default::default(),
            });
            app.manage(core.clone());
            let timer_core = core.clone();
            let timer_handle = handle.clone();

            // The agent's channel: our own socket, which Clatch never sees. clappkit
            // answers the window verbs itself and pushes the snapshot we return.
            clappkit::app::spawn_ipc(handle, CLI, WindowPolicy::default(), move |req, caller| {
                let core = core.clone();
                async move { core.command(req, caller).await }
            });

            // The due-task timer: the single loop this app owns, and it exists only while
            // the app does — Clatch starts nothing at boot and ships no scheduler, so
            // anything that came due while we were closed is reported by the first sweep,
            // as one signal. `Skip` so a laptop that slept does not replay the ticks it
            // missed.
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + LAUNCH_GRACE, SWEEP_EVERY);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    timer_core.sweep(&timer_handle).await;
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("{CLI}: the window failed to start: {e}");
            std::process::exit(1)
        })
        .run(|app, event| {
            // `crm close` answers, waits its grace (150 ms — shorter than the writer's
            // quiet period) and calls `exit`, as does a quit from the window. Both arrive
            // here. This is the guarantee the whole store rests on: **a write the app
            // acknowledged is on the disk before the process is gone.**
            if let tauri::RunEvent::Exit = event {
                if let Some(saves) = app.try_state::<ExitFlush>() {
                    saves.0.flush(None, FLUSH_WAIT);
                }
            }
        });
}

/// What the process does when Clatch says stop: **flush first**, then say so.
///
/// The order is the whole point. When Clatch stops an app it closes the app's stderr pipe,
/// and `eprintln!` *panics* on a failed write — so a hook that logged first died before it
/// saved, and every write inside the quiet period was lost while the log line was, at best,
/// cut off. Logging is a `writeln!` whose result is thrown away, and it comes last.
fn on_shutdown(saves: &SaveQueue, cause: impl std::fmt::Display, log: &mut impl std::io::Write) -> bool {
    let saved = saves.flush(None, FLUSH_WAIT);
    let _ = writeln!(log, "{CLI}: shutting down — {cause} (saved: {saved})");
    saved
}

/// The writer, in Tauri's state, for the exit event to reach.
struct ExitFlush(SaveQueue);

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    const HOUR: i32 = 3600;

    /// An instant, from a civil date and a UTC hour — built with the core's own calendar,
    /// so the tests below do not depend on a hand-computed epoch number being right.
    fn utc(date: Date, hour: i64) -> i64 {
        let days = Date::new(1970, 1, 1).days_until(date);
        (days * 86_400 + hour * 3600) * 1000
    }

    #[test]
    fn the_calendar_round_trips_through_its_inverse() {
        for date in [
            Date::new(1970, 1, 1),
            Date::new(1969, 12, 31),
            Date::new(1999, 12, 31),
            Date::new(2026, 9, 16),
            Date::new(2028, 2, 29),
            Date::new(2100, 3, 1),
        ] {
            let days = Date::new(1970, 1, 1).days_until(date);
            assert_eq!(civil_from_days(days), date, "{date:?} did not survive the round trip");
        }
    }

    /// The case QA asked for. 23:00 on the 15th in New York is already 04:00 on the 16th in
    /// UTC. The task due "today" is due on the 15th. **Reintroduce UTC and this fails** —
    /// on any machine, including one whose own zone is UTC.
    #[test]
    fn eleven_at_night_local_is_still_today_even_when_utc_is_tomorrow() {
        let at = utc(Date::new(2026, 9, 16), 4);
        assert_eq!(local_date(at, -5 * HOUR), Date::new(2026, 9, 15));
        assert_ne!(local_date(at, 0), Date::new(2026, 9, 15), "the fixture must actually cross midnight");
    }

    /// The far edges of the offsets in use. Kiritimati is a whole day ahead of Baker
    /// Island at the same instant.
    #[test]
    fn the_extreme_offsets_land_on_different_days() {
        let at = utc(Date::new(2026, 9, 15), 10);
        assert_eq!(local_date(at, 14 * HOUR), Date::new(2026, 9, 16), "+14:00 is already tomorrow");
        assert_eq!(local_date(at, -12 * HOUR), Date::new(2026, 9, 14), "-12:00 is still yesterday");
        assert_eq!(local_date(at, 0), Date::new(2026, 9, 15));
    }

    #[test]
    fn a_half_hour_offset_and_the_exact_edge_of_midnight_are_handled() {
        // 18:29:59.999 UTC is 23:59:59.999 in India (+05:30); one millisecond later is
        // tomorrow there.
        let edge = utc(Date::new(2026, 9, 15), 18) + 29 * 60_000 + 59_999;
        assert_eq!(local_date(edge, 5 * HOUR + 1800), Date::new(2026, 9, 15));
        assert_eq!(local_date(edge + 1, 5 * HOUR + 1800), Date::new(2026, 9, 16));
    }

    #[test]
    fn an_instant_before_the_epoch_floors_rather_than_truncating() {
        assert_eq!(local_date(-1, 0), Date::new(1969, 12, 31), "one millisecond before the epoch");
        assert_eq!(local_date(0, 0), Date::new(1970, 1, 1));
        assert_eq!(local_date(0, -HOUR), Date::new(1969, 12, 31), "an hour west of it");
    }

    /// The date `clock()` carries is the one its own instant falls on. Two reads could
    /// disagree across midnight; one read derived both.
    #[test]
    fn the_clock_derives_its_date_from_its_own_instant() {
        let (now, offset_secs) = clock();
        let real_offset = {
            use chrono::TimeZone;
            chrono::Local
                .timestamp_millis_opt(now.at)
                .single()
                .expect("an unambiguous local instant")
                .offset()
                .local_minus_utc()
        };
        assert_eq!(offset_secs, real_offset, "clock() must hand back the offset it actually used");
        assert_eq!(now.today, local_date(now.at, offset_secs));
        assert!(now.today.y >= 2026, "{:?}", now.today);
        assert_eq!(Date::parse(&now.today.to_string_iso()), Some(now.today));
    }

    /// [`model::Date::to_timestamp_ms`] is the exact inverse of `local_date`: a civil date
    /// placed back on the calendar at this offset must fall on that same date again.
    #[test]
    fn a_dates_local_midnight_round_trips_through_local_date() {
        for (date, offset) in [
            (Date::new(2026, 9, 10), -5 * HOUR),
            (Date::new(2026, 9, 10), 5 * HOUR + 1800),
            (Date::new(2026, 1, 1), 14 * HOUR),
            (Date::new(1969, 12, 31), -12 * HOUR),
        ] {
            let at = date.to_timestamp_ms(offset);
            assert_eq!(local_date(at, offset), date, "{date:?} at offset {offset}");
        }
    }

    // MARK: - An acknowledged write survives the process
    //
    // Round 5's most serious finding: `crm add` answered "added", the app was stopped
    // inside the writer's 400 ms quiet period, and the record was gone. These run the real
    // `Core` — the same `command` both surfaces call — against a real file, and end the
    // "process" the only way a test can: by flushing, as the exit paths now do, and
    // reading the file back through a fresh state, as a relaunch does.

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        // A counter as well as the clock: two tests starting in the same microsecond with the
        // same tag shared one directory, and read each other's file (an intermittent failure).
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("crm-main-{}-{nanos}-{n}-{tag}", std::process::id()))
    }

    /// A core over a real file, on a runtime of its own. `quiet` is far longer than any
    /// test, so nothing reaches the disk unless something flushes it.
    fn a_core(path: &std::path::Path) -> (Arc<Core>, tokio::runtime::Runtime) {
        a_core_over(Arc::new(JsonStore::at(path)))
    }

    fn a_core_over(store: Arc<dyn CrmStore>) -> (Arc<Core>, tokio::runtime::Runtime) {
        a_core_shaped(store, Shape::Classic)
    }

    fn a_core_shaped(store: Arc<dyn CrmStore>, shape: Shape) -> (Arc<Core>, tokio::runtime::Runtime) {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let (saves, writer) = SaveQueue::channel(store, Duration::from_secs(30));
        rt.spawn(writer);
        // Outside Clatch there is no control pipe, and `connect` answers with a no-op one.
        let control = rt.block_on(clappkit::Control::connect(Vec::new())).expect("the standalone control");
        let core = Arc::new(Core {
            state: Mutex::new(AppState::new()),
            control,
            saves,
            origin: InstanceId::from_bytes([0xA1; 16]),
            shape,
            test_roster: Default::default(),
        });
        (core, rt)
    }

    /// A relaunch: what is on the disk, opened as the app opens it.
    fn relaunched(path: &std::path::Path) -> AppState {
        AppState::with_db(JsonStore::at(path).load().expect("the file reads back"))
    }

    fn there_is(st: &AppState, handle: &str) -> bool {
        st.db().by_handle(handle).is_some()
    }

    /// **The three commands from the report**: `crm add`, stop, relaunch — with no flush,
    /// no hook and no exit event, because measured against a real Clatch `clatch stop`
    /// gives the process none of them (it is gone within ~30 ms). The only thing that can
    /// make the acknowledgement true is that the write was done *before* it was given.
    #[test]
    fn a_write_the_agent_was_told_succeeded_is_on_the_disk_before_it_was_told() {
        let dir = scratch_dir("add");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);

        let reply = rt.block_on(core.command(
            json!({ "cmd": "add", "kind": "company", "name": "Flush Test" }),
            Some("agent-1".to_string()),
        ));
        assert_eq!(reply.resp["ok"], true, "acknowledged: {:?}", reply.resp);

        // The process is killed right here — nothing runs. `quiet` is 30 s, so the debounce
        // cannot have done it.
        assert!(there_is(&relaunched(&path), "flush-test"), "acknowledged, then lost");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every one of an agent's writes, not just the first — each is acknowledged, so each
    /// is on the disk when it is.
    #[test]
    fn each_of_an_agents_writes_is_durable_when_it_is_acknowledged() {
        let dir = scratch_dir("agent-many");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);
        for (n, name) in ["Acme Corp", "Hooli", "Initech"].iter().enumerate() {
            rt.block_on(core.command(
                json!({ "cmd": "add", "kind": "company", "name": name }),
                Some("agent-1".to_string()),
            ));
            let back = relaunched(&path);
            assert_eq!(back.db().companies.len(), n + 1, "after write {} the file already holds it", n + 1);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A read is not a write: nothing is written for a question.
    #[test]
    fn an_agents_read_writes_nothing() {
        let dir = scratch_dir("agent-read");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);
        rt.block_on(core.command(json!({ "cmd": "status" }), Some("agent-1".to_string())));
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A person's write from the window takes the same road: **there is no second door.**
    /// It used to ride a 400 ms debounce "made durable by the exit flush", and `clatch stop`,
    /// `kill -9` and a crash run no exit flush — QA round 6b measured every one of them losing
    /// it. The window's answer is as true as the agent's.
    #[test]
    fn a_write_from_the_window_is_on_the_disk_before_it_is_shown() {
        let dir = scratch_dir("window");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);

        let reply = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Window Co" }), None));
        assert_eq!(reply.resp["ok"], true);
        // No flush, no exit event: the process is killed right here.
        assert!(there_is(&relaunched(&path), "window-co"), "shown as done, then lost");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Several writes, then the exit: the file holds all of them, not just the first.
    #[test]
    fn every_write_before_the_exit_is_kept_not_only_the_first() {
        let dir = scratch_dir("many");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);

        for name in ["Acme Corp", "Hooli", "Initech"] {
            rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": name }), None));
        }
        assert!(core.saves.flush(None, FLUSH_WAIT));

        let back = relaunched(&path);
        for handle in ["acme-corp", "hooli", "initech"] {
            assert!(there_is(&back, handle), "{handle} was acknowledged and then lost");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scout() -> clappkit::AgentRow {
        clappkit::AgentRow { id: "1789126979".into(), name: "Scout".into(), backend: None, model: None, avatar: None }
    }

    /// The same bug in another costume — QA met it as a reminder delivered twice. The mark
    /// that says "sent" is a write like any other, and it must outlive a stop that comes
    /// straight after the send.
    #[test]
    fn a_reminder_sent_once_stays_once_across_an_immediate_stop_and_relaunch() {
        let dir = scratch_dir("reminder");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);

        // A person's already-due next step: not born told, so the first sweep sends it.
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "deal", "name": "Acme renewal" }), None));
        rt.block_on(core.command(
            json!({ "cmd": "task", "handle": "acme-renewal", "what": "Chase Maya", "due": "2026-09-01" }),
            None,
        ));

        let (now, offset) = (Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) }, 0);
        let first = rt.block_on(core.sweep_once(vec![scout()], now, offset));
        assert_eq!(first.emits.len(), 1, "sent once");

        // Killed the instant it was sent — **no flush**: the mark was written before the
        // signal left (write-ahead), which is the only order that is at-most-once when the
        // process is not given a chance to say goodbye.

        // The relaunch's own sweep, with the same agent bound: nothing to say.
        let mut again = relaunched(&path);
        // Not vacuous: an empty file would also "send nothing", so the task must be there,
        // and carry the mark.
        let task = again.db().task_by_handle("chase-maya").cloned().expect("the task was lost with the exit");
        assert!(task.due_sent_at.is_some(), "the task came back, but not marked sent");
        again.set_agents(vec![scout()]);
        let ctx = Ctx { now, entropy: [7; 10], origin: InstanceId::from_bytes([0xA1; 16]), offset_secs: 0 };
        assert!(again.sweep(&ctx).emits.is_empty(), "delivered twice: the mark did not survive the exit");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What Clatch does to a stopping app's stderr: closes the other end.
    struct BrokenPipe;
    impl std::io::Write for BrokenPipe {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    /// **The bug the first version of the fix had**, found only against a real Clatch: the
    /// shutdown hook printed before it saved, `eprintln!` panics on a closed pipe, and every
    /// write inside the quiet period was lost anyway. With stderr gone, the write is still
    /// kept and nothing panics.
    #[test]
    fn the_shutdown_hook_saves_even_when_stderr_is_a_closed_pipe() {
        let dir = scratch_dir("hook");
        let path = dir.join("crm.json");
        let (core, _rt) = a_core(&path);
        // A dataset waiting out the debounce (what a drag leaves behind). Writes no longer
        // wait — they go through the commit point — so this is put there directly.
        let mut st = AppState::new();
        st.add_company("Hook Co", &Ctx {
            now: Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) },
            entropy: [1; 10],
            origin: InstanceId::from_bytes([0xA1; 16]),
            offset_secs: 0,
        });
        core.saves.save(st.db());
        assert!(!path.exists(), "inside the quiet period");

        let saved = on_shutdown(&core.saves, clappkit::ShutdownCause::Requested, &mut BrokenPipe);

        assert!(saved, "it saved, and it did not panic on the log line");
        assert!(there_is(&relaunched(&path), "hook-co"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_shutdown_hook_says_what_it_did_when_stderr_is_open() {
        let dir = scratch_dir("hook-log");
        let (core, _rt) = a_core(&dir.join("crm.json"));
        let mut log = Vec::new();
        on_shutdown(&core.saves, clappkit::ShutdownCause::PipeClosed, &mut log);
        let line = String::from_utf8(log).unwrap();
        assert!(line.contains("shutting down") && line.contains("saved: true"), "{line}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A store that can be made to fail — a full disk, a revoked directory. Real file
    /// underneath, so "what is on the disk" is a fact and not a stand-in.
    struct Flaky {
        inner: JsonStore,
        failing: std::sync::atomic::AtomicBool,
    }

    impl Flaky {
        fn new(path: &std::path::Path) -> Arc<Flaky> {
            Arc::new(Flaky { inner: JsonStore::at(path), failing: false.into() })
        }
        fn fail(&self, on: bool) {
            self.failing.store(on, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl CrmStore for Flaky {
        fn load(&self) -> anyhow::Result<crate::model::Db> {
            self.inner.load()
        }
        fn save(&self, db: &crate::model::Db) -> anyhow::Result<()> {
            if self.failing.load(std::sync::atomic::Ordering::SeqCst) {
                anyhow::bail!("cannot write crm.json: Permission denied (os error 13)");
            }
            self.inner.save(db)
        }
    }

    /// **The disk refuses: the write is refused, not acknowledged.** QA round 6b: the agent
    /// was told "added", the log said "could not confirm", and `find` showed a record the next
    /// launch did not have. Now the answer is derived from the persist result, the change is
    /// rolled back, and the memory agrees with the disk.
    #[test]
    fn a_write_the_disk_refuses_is_refused_and_rolled_back() {
        let dir = scratch_dir("refused");
        let path = dir.join("crm.json");
        let store = Flaky::new(&path);
        let (core, rt) = a_core_over(store.clone());

        let ok = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Before" }), Some("a".into())));
        assert_eq!(ok.resp["ok"], true);

        store.fail(true);
        for caller in [Some("agent-1".to_string()), None] {
            let r = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Doomed" }), caller));
            assert_eq!(r.resp["ok"], false, "a write the disk refused must not be acknowledged");
            let msg = r.resp["error"].as_str().unwrap();
            assert!(msg.contains("not saved") && msg.contains("did not reach the disk"), "{msg}");
            // Rolled back: what the surfaces see is what the disk has.
            assert_eq!(r.resp["split"]["derived"]["counts"]["companies"], 1, "memory kept a record the disk refused");
            assert_eq!(relaunched(&path).db().companies.len(), 1);
        }
        let found = rt.block_on(core.command(json!({ "cmd": "find", "query": "" }), Some("a".into())));
        assert_eq!(found.resp["list"]["total"], 1, "`find` must agree with the disk");

        // And it recovers: the disk comes back, the same write goes through.
        store.fail(false);
        let again = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Doomed" }), None));
        assert_eq!(again.resp["ok"], true);
        assert!(there_is(&relaunched(&path), "doomed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused write signals nobody: the person's window edit that did not happen must not
    /// tell the agent it did.
    #[test]
    fn a_refused_window_write_emits_nothing_and_leaves_the_pending_question_alone() {
        let dir = scratch_dir("refused-pending");
        let path = dir.join("crm.json");
        let store = Flaky::new(&path);
        let (core, rt) = a_core_over(store.clone());
        for n in ["Acme Corp", "Acme Industries"] {
            rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": n }), None));
        }
        store.fail(true);
        // Parking a question is a change too (a promise to finish a write): refused, not half-kept.
        let r = rt.block_on(core.command(
            json!({ "cmd": "log", "kind": "note", "handle": "acme", "body": "x" }),
            Some("a".into()),
        ));
        assert_eq!(r.resp["ok"], false);
        assert!(r.resp["split"]["view"]["pending"].is_null(), "a question the disk never saw must not be on screen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Navigation is not data: a failing disk must not refuse a `find` or a `show`.
    #[test]
    fn a_failing_disk_does_not_refuse_navigation() {
        let dir = scratch_dir("nav");
        let path = dir.join("crm.json");
        let store = Flaky::new(&path);
        let (core, rt) = a_core_over(store.clone());
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Acme Corp" }), None));
        store.fail(true);
        for req in [json!({ "cmd": "find", "query": "acme" }), json!({ "cmd": "open", "handle": "acme-corp" }), json!({ "cmd": "status" })] {
            let r = rt.block_on(core.command(req.clone(), Some("a".into())));
            assert_eq!(r.resp["ok"], true, "{req}: {:?}", r.resp);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// At-most-once means the mark is durable *before* the signal. If it cannot be made
    /// durable the signal is withheld — one late, never two — and the task goes back to
    /// waiting rather than sitting marked in memory and forgotten.
    #[test]
    fn a_reminder_whose_mark_cannot_be_written_is_withheld_and_tried_again() {
        let dir = scratch_dir("reminder-fail");
        let path = dir.join("crm.json");
        let store = Flaky::new(&path);
        let (core, rt) = a_core_over(store.clone());
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "deal", "name": "Acme renewal" }), None));
        rt.block_on(core.command(
            json!({ "cmd": "task", "handle": "acme-renewal", "what": "Chase Maya", "due": "2026-09-01" }),
            None,
        ));

        store.fail(true);
        let now = Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) };
        let sweep = rt.block_on(core.sweep_once(vec![scout()], now, 0));
        assert!(sweep.emits.is_empty(), "sending without a recorded mark is how a reminder arrives twice");
        assert_eq!(sweep.update["seat"]["reminders"]["awaiting"], 1, "still waiting, not silently forgotten");
        let again = rt.block_on(core.sweep_once(vec![scout()], now, 0));
        assert!(again.emits.is_empty(), "and it keeps refusing while the disk does");
        assert_eq!(again.update["seat"]["reminders"]["awaiting"], 1);

        store.fail(false);
        let recovered = rt.block_on(core.sweep_once(vec![scout()], now, 0));
        assert_eq!(recovered.emits.len(), 1, "one late, never two");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // MARK: - A question parked behind an ambiguity survives a restart (QA round 6)
    //
    // `Pending.resume` was `#[serde(skip)]` while `resuming` was saved, so after any restart
    // the prompt promised "picking one finishes it" and `select` opened the record instead —
    // exit 0, the write gone. These park a write through the CLI's own door, end the process
    // two ways, and answer the question on the relaunch.

    /// How the process ended. Graceful: `crm close` (the exit flush runs). Hard: `kill -9`
    /// — nothing runs, so only what was durable *before the answer* is there.
    #[derive(Clone, Copy, Debug)]
    enum End {
        Graceful,
        Kill9,
    }

    /// Two companies that answer to "acme" and two deals that answer to "globex", written by agent.
    fn an_ambiguous_workspace(path: &std::path::Path) -> (Arc<Core>, tokio::runtime::Runtime) {
        let (core, rt) = a_core(path);
        for req in [
            json!({ "cmd": "add", "kind": "company", "name": "Acme Corp" }),
            json!({ "cmd": "add", "kind": "company", "name": "Acme Industries" }),
            json!({ "cmd": "add", "kind": "deal", "name": "Globex renewal" }),
            json!({ "cmd": "add", "kind": "deal", "name": "Globex pilot" }),
        ] {
            let r = rt.block_on(core.command(req, Some("agent-1".into())));
            assert_eq!(r.resp["ok"], true, "{:?}", r.resp);
        }
        (core, rt)
    }

    fn park(core: &Core, rt: &tokio::runtime::Runtime, req: Value) -> Value {
        let r = rt.block_on(core.command(req, Some("agent-1".into())));
        assert_eq!(r.resp["answer"], "ambiguous", "{:?}", r.resp);
        assert_eq!(r.resp["pending"]["resuming"], true);
        r.resp
    }

    fn end(core: &Core, how: End) {
        if let End::Graceful = how {
            assert!(core.saves.flush(None, FLUSH_WAIT));
        }
    }

    fn select_after_restart(path: &std::path::Path, n: u64) -> (AppState, crate::state::Outcome) {
        let mut st = relaunched(path);
        let ctx = Ctx {
            now: Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) },
            entropy: [9; 10],
            origin: InstanceId::from_bytes([0xA1; 16]),
            offset_secs: 0,
        };
        let out = st.command(&json!({ "cmd": "select", "n": n }), Some("agent-1"), &ctx);
        (st, out)
    }

    fn the_write_lands_across_a_restart(how: End, req: Value, check: impl Fn(&AppState, &Value)) {
        let dir = scratch_dir("resume");
        let path = dir.join("crm.json");
        let (core, rt) = an_ambiguous_workspace(&path);
        park(&core, &rt, req);
        end(&core, how);

        let (st, out) = select_after_restart(&path, 1);
        assert_eq!(out.resp["ok"], true, "{how:?}: {:?}", out.resp);
        assert!(out.resp["resumed"].is_object(), "{how:?}: it must say it completed a write: {:?}", out.resp);
        assert!(out.resp["pending"].is_null(), "{how:?}: the question is answered");
        check(&st, &out.resp);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_parked_log_lands_after_a_restart_graceful_or_kill_9() {
        for how in [End::Graceful, End::Kill9] {
            the_write_lands_across_a_restart(
                how,
                json!({ "cmd": "log", "kind": "note", "handle": "acme", "body": "cross-restart test" }),
                |st, resp| {
                    assert_eq!(resp["resumed"]["cmd"], "log");
                    let db = st.db();
                    assert_eq!(db.activities.len(), 1, "{how:?}: the note was never written");
                    assert_eq!(db.activities[0].body, "cross-restart test");
                },
            );
        }
    }

    #[test]
    fn a_parked_set_lands_after_a_restart_graceful_or_kill_9() {
        for how in [End::Graceful, End::Kill9] {
            the_write_lands_across_a_restart(
                how,
                json!({ "cmd": "set", "handle": "acme", "field": "domain", "value": "acme.example" }),
                |st, resp| {
                    assert_eq!(resp["resumed"]["cmd"], "set");
                    let db = st.db();
                    assert_eq!(
                        db.companies.iter().filter(|c| c.domain.as_deref() == Some("acme.example")).count(),
                        1,
                        "{how:?}: the domain was never written"
                    );
                },
            );
        }
    }

    #[test]
    fn a_parked_move_lands_after_a_restart_graceful_or_kill_9() {
        for how in [End::Graceful, End::Kill9] {
            the_write_lands_across_a_restart(
                how,
                json!({ "cmd": "move", "handle": "globex", "to": "proposal" }),
                |st, resp| {
                    assert_eq!(resp["resumed"]["cmd"], "move");
                    let db = st.db();
                    let moved = db.deals.iter().filter(|d| d.stage == crate::model::Stage::Proposal).count();
                    assert_eq!(moved, 1, "{how:?}: the deal never moved");
                },
            );
        }
    }

    /// The promise must be derived, not stored: after a restart the snapshot still says
    /// the question completes a write, because it still can — and never leaks the request.
    #[test]
    fn the_promise_survives_a_restart_because_the_write_does_and_the_request_never_reaches_a_snapshot() {
        let dir = scratch_dir("resume-snapshot");
        let path = dir.join("crm.json");
        let (core, rt) = an_ambiguous_workspace(&path);
        let parked = park(&core, &rt, json!({ "cmd": "log", "kind": "note", "handle": "acme", "body": "private body" }));
        assert!(!parked.to_string().contains("private body"), "the parked request leaked into a snapshot");

        let st = relaunched(&path);
        let now = Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) };
        let snap = crate::state::tests::seen(&st, now);
        assert_eq!(snap["pending"]["resuming"], true);
        assert!(snap["pending"].get("lost").is_none());
        assert!(!snap.to_string().contains("private body"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A data file written by the build that had the bug: `resuming: true`, no `resume`.
    /// The write cannot be completed, so `select` must refuse — never open the record and
    /// say it worked.
    #[test]
    fn a_question_parked_by_the_old_build_is_refused_not_turned_into_a_navigation() {
        let dir = scratch_dir("resume-legacy");
        let path = dir.join("crm.json");
        let (core, rt) = an_ambiguous_workspace(&path);
        park(&core, &rt, json!({ "cmd": "log", "kind": "note", "handle": "acme", "body": "x" }));
        assert!(core.saves.flush(None, FLUSH_WAIT));

        // Rewrite the file the way the old build wrote it.
        let mut db: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let pending = db["view"]["pending"].as_object_mut().unwrap();
        pending.remove("resume");
        pending.insert("resuming".into(), json!(true));
        std::fs::write(&path, serde_json::to_vec(&db).unwrap()).unwrap();

        let st = relaunched(&path);
        let now = Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) };
        assert_eq!(crate::state::tests::seen(&st, now)["pending"]["lost"], true, "the surfaces can say so");
        let (_, out) = select_after_restart(&path, 1);
        assert_eq!(out.resp["ok"], false, "{:?}", out.resp);
        assert!(out.resp["error"].as_str().unwrap().contains("write was lost — run it again"), "{:?}", out.resp);
        assert!(out.emits.is_empty());

        // And again: a retry must not fall through to "open result N".
        let mut st = relaunched(&path);
        let ctx = Ctx { now, entropy: [3; 10], origin: InstanceId::from_bytes([0xA1; 16]), offset_secs: 0 };
        for _ in 0..2 {
            let again = st.command(&json!({ "cmd": "select", "n": 1 }), Some("agent-1"), &ctx);
            assert_eq!(again.resp["ok"], false, "{:?}", again.resp);
        }
        // Running the write again parks a fresh, completable question in its place.
        let r = st.command(&json!({ "cmd": "log", "kind": "note", "handle": "acme", "body": "again" }), Some("agent-1"), &ctx);
        assert_eq!(r.resp["pending"]["resuming"], true);
        assert!(r.resp["pending"].get("lost").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A plain `show` ambiguity never carried a write and still opens the record.
    #[test]
    fn a_plain_show_question_still_opens_the_record_after_a_restart() {
        let dir = scratch_dir("resume-plain");
        let path = dir.join("crm.json");
        let (core, rt) = an_ambiguous_workspace(&path);
        let r = rt.block_on(core.command(json!({ "cmd": "open", "handle": "acme" }), Some("agent-1".into())));
        assert_eq!(r.resp["answer"], "ambiguous");
        assert_eq!(r.resp["pending"]["resuming"], false);
        let (_, out) = select_after_restart(&path, 1);
        assert_eq!(out.resp["ok"], true, "{:?}", out.resp);
        assert!(out.resp.get("resumed").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // MARK: - The sweep: every envelope, performed, killed, relaunched (QA round 6b)
    //
    // Durability was decided per path, so each path got it wrong separately: `import`
    // answered like a read and was never saved; a refused disk write was answered with
    // success; the window's door was debounced. The commit point closes the category, and
    // this is what keeps it closed: **for every envelope the dispatcher accepts**, perform it
    // through the core, end the process with nothing flushed (the file is read exactly as a
    // `kill -9` leaves it), and assert the disk already holds everything the memory does.
    // A verb that is not in one of the two tables below fails the build.

    type Setup = fn(&Core, &tokio::runtime::Runtime);

    fn run_cli(core: &Core, rt: &tokio::runtime::Runtime, req: Value) -> Value {
        let r = rt.block_on(core.command(req.clone(), Some("agent-1".into())));
        assert_eq!(r.resp["ok"], true, "setup {req}: {:?}", r.resp);
        r.resp
    }

    /// Two companies, a contact, a deal and an open step: enough to act on.
    fn seeded(core: &Core, rt: &tokio::runtime::Runtime) {
        for req in [
            json!({ "cmd": "add", "kind": "company", "name": "Acme Corp" }),
            json!({ "cmd": "add", "kind": "company", "name": "Hooli" }),
            json!({ "cmd": "add", "kind": "contact", "name": "Ada Lovelace" }),
            json!({ "cmd": "add", "kind": "deal", "name": "Globex pilot" }),
            json!({ "cmd": "task", "handle": "globex-pilot", "what": "Call Maya", "due": "2027-01-01" }),
        ] {
            run_cli(core, rt, req);
        }
    }

    fn id(st: &AppState, handle: &str) -> String {
        st.db().by_handle(handle).unwrap_or_else(|| panic!("no `{handle}`")).1
    }

    fn task_id(st: &AppState, handle: &str) -> String {
        st.db().task_by_handle(handle).unwrap_or_else(|| panic!("no task `{handle}`")).id.clone()
    }

    /// One way of performing one envelope: what to set up, and the request, built from the
    /// state as set up — so a window-shaped request can carry real ids.
    struct Case {
        what: &'static str,
        setup: Setup,
        act: fn(&AppState) -> Value,
    }

    fn park_a_log(core: &Core, rt: &tokio::runtime::Runtime) {
        seeded(core, rt);
        run_cli(core, rt, json!({ "cmd": "add", "kind": "company", "name": "Acme Industries" }));
        let r = rt.block_on(core.command(
            json!({ "cmd": "log", "kind": "note", "handle": "acme", "body": "parked" }),
            Some("agent-1".into()),
        ));
        assert_eq!(r.resp["answer"], "ambiguous");
    }

    fn after_a_reminder_was_sent(core: &Core, rt: &tokio::runtime::Runtime) {
        seeded(core, rt);
        // A person's already-due step is the one the first sweep sends.
        run_cli(core, rt, json!({ "cmd": "task", "handle": "globex-pilot", "what": "Chase", "due": "2026-09-01" }));
        // `task` above went in as an agent's, which is born told — so make it a person's.
        let r = rt.block_on(core.command(
            json!({ "cmd": "task", "handle": "globex-pilot", "what": "Chase Maya", "due": "2026-09-01" }),
            None,
        ));
        assert_eq!(r.resp["ok"], true);
        *core.test_roster.lock().unwrap() = Some(vec![scout()]);
        let now = Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) };
        let sweep = rt.block_on(core.sweep_once(vec![scout()], now, 0));
        assert_eq!(sweep.emits.len(), 1, "the fixture must have sent a reminder");
    }

    /// Every write, in the shapes the two doors send: handles from the CLI, ids from the window.
    fn the_writes() -> Vec<(&'static str, Vec<Case>)> {
        vec![
            ("add", vec![
                Case { what: "company", setup: seeded, act: |_| json!({ "cmd": "add", "kind": "company", "name": "Initech" }) },
                Case { what: "contact with a company", setup: seeded, act: |_| json!({ "cmd": "add", "kind": "contact", "name": "Bob", "fields": { "company": "acme-corp" } }) },
                Case { what: "deal", setup: seeded, act: |_| json!({ "cmd": "add", "kind": "deal", "name": "Umbrella", "fields": { "value": 5000, "currency": "USD" } }) },
            ]),
            ("set", vec![
                Case { what: "by handle (CLI)", setup: seeded, act: |_| json!({ "cmd": "set", "handle": "acme-corp", "field": "domain", "value": "acme.example" }) },
                Case { what: "by id (window)", setup: seeded, act: |st| json!({ "cmd": "set", "id": id(st, "acme-corp"), "field": "domain", "value": "acme.example" }) },
                Case { what: "--clear", setup: |c, r| { seeded(c, r); run_cli(c, r, json!({ "cmd": "set", "handle": "acme-corp", "field": "domain", "value": "x.example" })); }, act: |_| json!({ "cmd": "set", "handle": "acme-corp", "field": "domain", "clear": true }) },
            ]),
            ("log", vec![
                Case { what: "by handle (CLI)", setup: seeded, act: |_| json!({ "cmd": "log", "kind": "call", "handle": "acme-corp", "body": "rang" }) },
                Case { what: "by id (window)", setup: seeded, act: |st| json!({ "cmd": "log", "kind": "note", "id": id(st, "acme-corp"), "body": "hi" }) },
            ]),
            ("move", vec![
                Case { what: "by handle (CLI)", setup: seeded, act: |_| json!({ "cmd": "move", "handle": "globex-pilot", "to": "proposal" }) },
                Case { what: "by id (window)", setup: seeded, act: |st| json!({ "cmd": "move", "id": id(st, "globex-pilot"), "to": "won" }) },
            ]),
            ("task", vec![
                Case { what: "by handle (CLI)", setup: seeded, act: |_| json!({ "cmd": "task", "handle": "acme-corp", "what": "Send deck", "due": "2027-02-01" }) },
                Case { what: "by id (window)", setup: seeded, act: |st| json!({ "cmd": "task", "id": id(st, "acme-corp"), "what": "Send deck", "due": "2027-02-01" }) },
            ]),
            ("done", vec![
                Case { what: "by handle (CLI)", setup: seeded, act: |_| json!({ "cmd": "done", "handle": "call-maya" }) },
                Case { what: "by id (window)", setup: seeded, act: |st| json!({ "cmd": "done", "id": task_id(st, "call-maya") }) },
            ]),
            ("link", vec![
                Case { what: "by handle (CLI)", setup: seeded, act: |_| json!({ "cmd": "link", "handle": "globex-pilot", "toHandle": "ada-lovelace" }) },
                Case { what: "by id (window)", setup: seeded, act: |st| json!({ "cmd": "link", "id": id(st, "globex-pilot"), "toId": id(st, "acme-corp") }) },
            ]),
            ("archive", vec![
                Case { what: "by handle (CLI)", setup: seeded, act: |_| json!({ "cmd": "archive", "handle": "hooli" }) },
                Case { what: "restore by id (window)", setup: |c, r| { seeded(c, r); run_cli(c, r, json!({ "cmd": "archive", "handle": "hooli" })); }, act: |st| json!({ "cmd": "archive", "id": id(st, "hooli"), "restore": true }) },
            ]),
            // The one QA round 6b found: answered like a read, so never marked as a change.
            ("import", vec![
                Case { what: "contacts", setup: seeded, act: |_| json!({ "cmd": "import", "kind": "contact", "rows": [{ "name": "Ada" }, { "name": "Bob" }] }) },
                Case { what: "companies", setup: seeded, act: |_| json!({ "cmd": "import", "kind": "company", "rows": [{ "name": "Initech", "domain": "initech.example" }] }) },
                Case { what: "deals", setup: seeded, act: |_| json!({ "cmd": "import", "kind": "deal", "rows": [{ "title": "Umbrella pilot" }] }) },
            ]),
            // Answering a parked question completes a write; and *parking* is itself a change.
            ("select", vec![
                Case { what: "completes the parked write", setup: park_a_log, act: |_| json!({ "cmd": "select", "n": 1 }) },
            ]),
            ("resend", vec![
                Case { what: "the last reminder, again", setup: after_a_reminder_was_sent, act: |_| json!({ "cmd": "resend" }) },
            ]),
        ]
    }

    /// Read-only or navigation: they may move where the surfaces are looking, never the data.
    type Navigate = (&'static str, fn() -> Value);
    const NAVIGATION: &[Navigate] = &[
        ("status", || json!({ "cmd": "status" })),
        ("state", || json!({ "cmd": "state" })),
        ("show", || json!({ "cmd": "show", "kind": "company", "id": "placeholder" })),
        ("open", || json!({ "cmd": "open", "handle": "acme-corp" })),
        ("board", || json!({ "cmd": "board" })),
        ("stages", || json!({ "cmd": "stages" })),
        ("due", || json!({ "cmd": "due" })),
        ("export", || json!({ "cmd": "export", "kind": "company" })),
        ("find", || json!({ "cmd": "find", "query": "acme" })),
        // The split's reads (p1): they serve one surface and must touch nothing shared.
        ("bootstrap", || json!({ "cmd": "bootstrap" })),
        ("changes", || json!({ "cmd": "changes", "since": 0 })),
        ("search", || json!({ "cmd": "search", "q": "acme" })),
        ("feed", || json!({ "cmd": "feed" })),
        ("tasks", || json!({ "cmd": "tasks" })),
    ];

    /// What `dispatch` accepts, read from its own source. A verb added to one place and not
    /// the other fails here, before any behaviour is even compared.
    fn dispatch_arms() -> Vec<String> {
        let src = include_str!("state.rs");
        let body = &src[src.find("fn dispatch(").expect("dispatch exists")..];
        let body = &body[..body.find("other => Err(unknown(other))").expect("the catch-all arm")];
        let mut names = Vec::new();
        for line in body.lines() {
            let t = line.trim_start();
            if t.starts_with('"') && t.contains("=>") {
                let head = &t[..t.find("=>").unwrap()];
                names.extend(head.split('|').map(|n| n.trim().trim_matches('"').to_string()));
            }
        }
        names
    }

    #[test]
    fn the_list_of_envelopes_is_exactly_what_dispatch_accepts() {
        let mut arms = dispatch_arms();
        arms.sort();
        let mut listed: Vec<String> = state::ENVELOPES.iter().map(|s| s.to_string()).collect();
        listed.sort();
        assert_eq!(arms, listed, "an envelope was added to `dispatch` or to `ENVELOPES` but not both");
    }

    #[test]
    fn every_envelope_is_either_swept_as_a_write_or_declared_navigation() {
        let writes: Vec<&str> = the_writes().iter().map(|(n, _)| *n).collect();
        let nav: Vec<&str> = NAVIGATION.iter().map(|(n, _)| *n).collect();
        for env in state::ENVELOPES {
            assert!(
                writes.contains(env) || nav.contains(env),
                "`{env}` is an envelope with no durability test: add it to `the_writes` (perform, kill, relaunch) or to `NAVIGATION`"
            );
            assert!(!(writes.contains(env) && nav.contains(env)), "`{env}` is in both tables");
        }
        for name in writes.iter().chain(nav.iter()) {
            assert!(state::ENVELOPES.contains(name), "`{name}` is tested but is not an envelope");
        }
    }

    /// **The sweep.** Perform each write, kill without flushing, relaunch, and the disk holds
    /// what the memory does — for every verb, in both doors' shapes.
    #[test]
    fn every_write_is_on_the_disk_the_instant_it_is_acknowledged() {
        for (envelope, cases) in the_writes() {
            for case in cases {
                let label = format!("{envelope} ({})", case.what);
                let dir = scratch_dir("sweep");
                let path = dir.join("crm.json");
                let (core, rt) = a_core(&path);
                (case.setup)(&core, &rt);

                let (before, req) = {
                    let st = rt.block_on(core.state.lock());
                    (st.db(), (case.act)(&st))
                };
                // The window's door and the CLI's are one: both answer through `command`.
                let reply = rt.block_on(core.command(req.clone(), None));
                assert_eq!(reply.resp["ok"], true, "{label}: {req} -> {:?}", reply.resp);

                let after = rt.block_on(core.state.lock()).db();
                assert_eq!(
                    after.change_from(&before),
                    crate::model::Change::Data,
                    "{label}: the fixture must actually change the data, or this proves nothing"
                );
                // kill -9 here: nothing is flushed, nothing runs. The file is what a relaunch reads.
                let on_disk = JsonStore::at(&path).load().expect("the file reads back");
                assert_eq!(on_disk, after, "{label}: acknowledged, then not on the disk");
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }

    /// Navigation never changes the data, and (above) never refuses on a failing disk.
    #[test]
    fn navigation_never_changes_the_data() {
        for (envelope, req) in NAVIGATION {
            let dir = scratch_dir("nav-sweep");
            let (core, rt) = a_core(&dir.join("crm.json"));
            seeded(&core, &rt);
            let before = rt.block_on(core.state.lock()).db();
            let mut req = req();
            if *envelope == "show" {
                req["id"] = json!(id(&rt.block_on(core.state.lock()), "acme-corp"));
            }
            let reply = rt.block_on(core.command(req.clone(), Some("agent-1".into())));
            assert_eq!(reply.resp["ok"], true, "{envelope}: {:?}", reply.resp);
            let after = rt.block_on(core.state.lock()).db();
            assert_ne!(after.change_from(&before), crate::model::Change::Data, "`{envelope}` changed data");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// QA round 6b, case A/B/C: `crm import` said "imported 2" and saved nothing, because its
    /// answer is read-shaped. Now it is saved because the dataset differs.
    #[test]
    fn an_import_is_saved_even_though_its_answer_looks_like_a_read() {
        let dir = scratch_dir("import");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);
        let r = rt.block_on(core.command(
            json!({ "cmd": "import", "kind": "contact", "rows": [{ "name": "Ada" }, { "name": "Bob" }] }),
            Some("agent-1".into()),
        ));
        assert_eq!(r.resp["answer"], "read", "the premise: it answers like a read");
        assert_eq!(relaunched(&path).db().contacts.len(), 2, "imported 2, and lost them");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two commands at once cannot reach the disk out of order: the lock is held across the
    /// write, so the last one written is the last one applied.
    #[test]
    fn concurrent_commands_never_leave_an_older_dataset_on_the_disk() {
        let dir = scratch_dir("concurrent");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);
        rt.block_on(async {
            let mut tasks = Vec::new();
            for n in 0..40 {
                let core = core.clone();
                tasks.push(tokio::spawn(async move {
                    core.command(json!({ "cmd": "add", "kind": "company", "name": format!("Co {n}") }), Some("a".into())).await
                }));
            }
            for t in tasks {
                assert_eq!(t.await.unwrap().resp["ok"], true);
            }
        });
        assert_eq!(relaunched(&path).db().companies.len(), 40, "an older dataset overwrote a newer one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // MARK: - p1: both shapes published, behind a flag; the watermark commits with its rows

    fn split_core(path: &std::path::Path) -> (Arc<Core>, tokio::runtime::Runtime) {
        a_core_shaped(Arc::new(JsonStore::at(path)), Shape::Split)
    }

    /// The default is today's behaviour, untouched: the window is pushed the whole snapshot and
    /// no response grows a key. (`CRM_SHAPE=split` is the only way to see the other.)
    #[test]
    fn classic_is_the_default_and_pushes_the_whole_snapshot_exactly_as_before() {
        assert_eq!(Shape::default(), Shape::Classic);
        let dir = scratch_dir("classic");
        let (core, rt) = a_core(&dir.join("crm.json"));
        let r = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Acme" }), None));
        assert!(r.snapshot.get("type").is_none(), "no update message in the classic shape");
        for key in ["board", "cards", "list", "counts", "due", "agents", "reminders"] {
            assert!(r.snapshot.get(key).is_some(), "classic is missing {key}");
        }
        assert_eq!(r.resp["split"]["type"], "update", "the response carries the update the CLI reads");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Under the flag the window is pushed **an update** — never the whole state — and every
    /// response carries the same update under `split`. The classic fields stay in the response
    /// because the CLI reads them; they are not what the window is handed.
    #[test]
    fn split_pushes_only_an_update_and_answers_with_the_same_one() {
        let dir = scratch_dir("split-push");
        let (core, rt) = split_core(&dir.join("crm.json"));
        let r = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Acme" }), None));
        assert_eq!(r.snapshot["type"], "update");
        assert_eq!(r.snapshot["shape"], "split");
        for whole in ["board", "cards", "list", "counts", "data", "focused"] {
            assert!(r.snapshot.get(whole).is_none(), "`{whole}` is whole state; nothing pushes it: {}", r.snapshot);
        }
        assert_eq!(r.resp["split"], r.snapshot, "the response and the push are one moment");
        assert_eq!(r.resp["ok"], true);
        assert!(r.resp.get("board").is_some(), "the CLI's fields are still in the response");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **No code path pushes everything** — every write verb, in both doors' shapes, under the
    /// flag: the push is an update whose delta is exactly the rows the write changed, numbered
    /// from the watermark the client already held, and **the watermark that reached the disk is
    /// the one it was told** (they are committed together).
    #[test]
    fn every_write_pushes_an_update_numbered_from_the_previous_watermark_and_it_is_on_the_disk() {
        for (envelope, cases) in the_writes() {
            for case in cases {
                let label = format!("{envelope} ({})", case.what);
                let dir = scratch_dir("split-sweep");
                let path = dir.join("crm.json");
                let (core, rt) = split_core(&path);
                (case.setup)(&core, &rt);

                let (before_mark, req) = {
                    let st = rt.block_on(core.state.lock());
                    (st.watermark(), (case.act)(&st))
                };
                let reply = rt.block_on(core.command(req.clone(), None));
                assert_eq!(reply.resp["ok"], true, "{label}: {:?}", reply.resp);

                let push = &reply.snapshot;
                assert_eq!(push["type"], "update", "{label}");
                assert!(push.get("data").is_none() && push.get("cards").is_none(), "{label}: pushed whole state");
                let delta = &push["delta"];
                assert_eq!(delta["from"], before_mark, "{label}: it must start where the client was");
                let ops = delta["ops"].as_array().unwrap();
                assert!(!ops.is_empty() || envelope == "select", "{label}: a write that changed data has row actions");
                for (i, op) in ops.iter().enumerate() {
                    assert_eq!(op["seq"], before_mark + 1 + i as u64, "{label}: numbered in order, no gaps");
                }
                assert_eq!(delta["to"], push["watermark"]);

                // On the disk, in the same atomic write as the rows it numbers.
                let on_disk = JsonStore::at(&path).load().unwrap();
                assert_eq!(on_disk.watermark, push["watermark"], "{label}: the watermark and its rows disagree on disk");
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }

    /// A refused write took no number and published nothing: a client that asks what it missed
    /// is told nothing happened, because nothing did.
    #[test]
    fn a_refused_write_takes_no_watermark_and_publishes_no_row_action() {
        let dir = scratch_dir("split-refused");
        let path = dir.join("crm.json");
        let store = Flaky::new(&path);
        let (core, rt) = a_core_shaped(store.clone(), Shape::Split);
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Before" }), None));
        let held = rt.block_on(core.state.lock()).watermark();
        assert_eq!(held, 1);

        store.fail(true);
        let r = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Doomed" }), None));
        assert_eq!(r.resp["ok"], false);
        assert_eq!(r.snapshot["type"], "update", "even a refusal pushes the rolled-back state as an update");
        assert_eq!(r.snapshot["delta"]["ops"], json!([]), "no row action: the write did not happen");
        assert_eq!(r.snapshot["watermark"], held);

        let st = rt.block_on(core.state.lock());
        assert_eq!(st.watermark(), held, "the number was rolled back with the rows");
        assert_eq!(st.changes(held), Some(vec![]), "and nothing was published for a client to find");
        drop(st);

        store.fail(false);
        let again = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Doomed" }), None));
        assert_eq!(again.snapshot["delta"]["ops"][0]["seq"], held + 1, "the number it would have had is the one it gets");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A client that fell behind asks for what it missed, through the same door as everything.
    #[test]
    fn the_changes_envelope_returns_what_was_committed_after_a_watermark() {
        let dir = scratch_dir("split-changes");
        let (core, rt) = split_core(&dir.join("crm.json"));
        for n in ["One", "Two", "Three"] {
            rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": n }), None));
        }
        let r = rt.block_on(core.command(json!({ "cmd": "changes", "since": 1 }), None));
        let ops = r.resp["changes"]["ops"].as_array().unwrap();
        assert_eq!(ops.iter().map(|o| o["record"]["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Two", "Three"]);
        assert_eq!(r.snapshot["delta"]["ops"], json!([]), "a read is an update with nothing in it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Looking is not a row action, through the whole commit path: no number, no op, no write.
    #[test]
    fn navigating_under_the_flag_pushes_an_update_with_an_empty_delta() {
        let dir = scratch_dir("split-nav");
        let path = dir.join("crm.json");
        let (core, rt) = split_core(&path);
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Acme" }), None));
        let held = rt.block_on(core.state.lock()).watermark();
        let r = rt.block_on(core.command(json!({ "cmd": "find", "query": "acme" }), Some("agent-1".into())));
        assert_eq!(r.snapshot["watermark"], held);
        assert_eq!(r.snapshot["delta"]["ops"], json!([]));
        assert_eq!(r.snapshot["view"]["list"]["query"], "acme");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The timer's mark is a row action too — a task's `sentAt` — numbered, persisted before the
    /// signal leaves, and pushed as an update.
    #[test]
    fn a_reminders_mark_is_a_row_action_with_a_watermark_and_is_pushed_as_an_update() {
        let dir = scratch_dir("split-sweep-mark");
        let path = dir.join("crm.json");
        let (core, rt) = split_core(&path);
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "deal", "name": "Acme renewal" }), None));
        rt.block_on(core.command(
            json!({ "cmd": "task", "handle": "acme-renewal", "what": "Chase Maya", "due": "2026-09-01" }),
            None,
        ));
        let held = rt.block_on(core.state.lock()).watermark();
        let now = Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) };
        let sweep = rt.block_on(core.sweep_once(vec![scout()], now, 0));
        assert_eq!(sweep.emits.len(), 1);
        assert_eq!(sweep.snapshot["type"], "update");
        let ops = sweep.snapshot["delta"]["ops"].as_array().unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["kind"], "task");
        assert_eq!(ops[0]["record"]["sentAt"], now.at);
        assert_eq!(ops[0]["seq"], held + 1);
        assert_eq!(JsonStore::at(&path).load().unwrap().watermark, held + 1, "on the disk before the signal left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The window and the CLI share a seat, so they share the view — and neither door can
    /// reach another seat's, because there is only the one.
    #[test]
    fn the_person_and_their_agent_see_one_view_through_both_doors() {
        let dir = scratch_dir("split-seat");
        let (core, rt) = split_core(&dir.join("crm.json"));
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Acme Corp" }), None));
        let agent = rt.block_on(core.command(json!({ "cmd": "open", "handle": "acme-corp" }), Some("agent-1".into())));
        let window = rt.block_on(core.command(json!({ "cmd": "state" }), None));
        assert_eq!(agent.snapshot["view"]["focus"], window.snapshot["view"]["focus"]);
        assert_eq!(window.snapshot["view"]["focus"]["handle"], "acme-corp", "the agent opened it in the person's window");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
