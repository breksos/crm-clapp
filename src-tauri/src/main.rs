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
use serde_json::Value;
use state::AppState;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use store::{load_or_mint_instance_id, CrmStore, JsonStore, SaveQueue};
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
}

/// Which door a command came in by. The durability promise is made **to the surface that
/// gets the answer**, so it follows the channel, not the caller's identity: a person typing
/// `crm add` in a terminal is answered over the same socket as an agent and has been told
/// just as much.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Via {
    /// The window's own invocation: its edits are visible on screen and arrive in bursts (a
    /// card dragged across a board), so they are debounced and made durable by the exit flush.
    Window,
    /// The `crm` CLI's socket: each command is discrete and is answered to a caller that
    /// cannot see a lost write, so it is written before it is answered.
    Cli,
}

impl Core {
    /// Apply one command from either surface.
    ///
    /// `caller` is the agent id for a CLI call and `None` for the person at the window.
    /// The response and the snapshot come out of **one** lock, so they cannot describe
    /// different moments.
    async fn command(&self, req: Value, caller: Option<String>, via: Via) -> Reply {
        // The roster is Clatch's, not ours, and it rides the snapshot because the window
        // draws it. Handing it to the core keeps the core free of anything it would have
        // to reach out to fetch — and the same goes for the clock.
        let roster = self.control.roster();
        let (now, offset_secs) = clock();
        let ctx = Ctx { now, entropy: entropy(), origin: self.origin.clone(), offset_secs };

        let (out, db) = {
            let mut state = self.state.lock().await;
            state.set_agents(roster);
            let out = state.command(&req, caller.as_deref(), &ctx);
            let db = out.dirty.then(|| state.db());
            (out, db)
        };

        // The core says the dataset changed; the writer decides when.
        //
        // **The window's edits are debounced** — a drag is one save, not sixty — and made
        // durable by the exit flush. **The CLI's are written before it is answered.** Its
        // caller cannot see a lost write and will build on the "added" it was told, and
        // `clatch stop` does not give the app time to flush: it is gone in ~30 ms with no hook
        // and no exit event. CLI commands are sporadic and discrete, so a few milliseconds
        // each is the price of the answer being true.
        if let Some(db) = db {
            if via == Via::Cli {
                if !self.saves.write_through(db, FLUSH_WAIT).await {
                    let _ = writeln!(std::io::stderr(), "{CLI}: could not confirm a write reached the disk");
                }
            } else {
                self.saves.save(db);
            }
        }

        // Only human actions signal. An agent is never told about its own write — that is
        // the loop that makes an app talk to itself.
        if caller.is_none() {
            self.control.emit_all(out.emits);
        }
        Reply::new(out.resp, out.snapshot)
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
        let sweep = self.sweep_once(self.control.roster(), now, offset_secs).await;
        clappkit::app::push_state(app, sweep.snapshot);
    }

    /// The whole of a sweep short of the window: the roster and the clock come in, the
    /// marks are queued for the disk, and the signal goes out. Split from [`sweep`] so a
    /// test can run it without a webview.
    async fn sweep_once(&self, roster: Vec<clappkit::AgentRow>, now: Now, offset_secs: i32) -> state::Sweep {
        let ctx = Ctx { now, entropy: entropy(), origin: self.origin.clone(), offset_secs };

        let (sweep, db) = {
            let mut state = self.state.lock().await;
            state.set_agents(roster);

            // Refusals are read at the top of the sweep, before the core decides what is
            // still waiting: a refused batch goes back to waiting and is retried in this
            // very pass. Not a poll of anything — the loop was already running.
            for (signal, agent, reason) in take_refusals(&self.control) {
                state.note_refusal(&signal, &agent, &reason, &ctx);
            }
            let sweep = state.sweep(&ctx);
            let db = sweep.dirty.then(|| state.db());
            (sweep, db)
        };

        // **Write-ahead.** The mark that says "sent" reaches the disk *before* the signal
        // leaves, so a stop at any instant — even one that gives the process no time to
        // flush — can never send it twice. The other order is at-least-once.
        if let Some(db) = db {
            if !self.saves.write_through(db, FLUSH_WAIT).await {
                let _ = writeln!(std::io::stderr(), "{CLI}: could not confirm a reminder was recorded; not sending it");
                let mut state = self.state.lock().await;
                state.unsend();
                // Rebuilt after the rollback: the snapshot taken inside the sweep still says
                // the tasks were sent.
                return state::Sweep { emits: Vec::new(), dirty: sweep.dirty, snapshot: state.snapshot(now) };
            }
        }
        self.control.emit_all(sweep.emits.clone());
        sweep
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
    Ok(core.command(req, None, Via::Window).await.resp)
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
            let (state, migrated) = {
                let (now, offset_secs) = clock();
                AppState::open(db, &Ctx { now, entropy: entropy(), origin: origin.clone(), offset_secs })
            };
            if migrated {
                saves.save(state.db());
            }

            // Held in Tauri's state for `RunEvent::Exit`, which is how `crm close` and a
            // quit from the window end the process.
            app.manage(ExitFlush(saves.clone()));

            let core = Arc::new(Core {
                state: Mutex::new(state),
                control,
                saves,
                origin,
            });
            app.manage(core.clone());
            let timer_core = core.clone();
            let timer_handle = handle.clone();

            // The agent's channel: our own socket, which Clatch never sees. clappkit
            // answers the window verbs itself and pushes the snapshot we return.
            clappkit::app::spawn_ipc(handle, CLI, WindowPolicy::default(), move |req, caller| {
                let core = core.clone();
                async move { core.command(req, caller, Via::Cli).await }
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
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("crm-main-{}-{nanos}-{tag}", std::process::id()))
    }

    /// A core over a real file, on a runtime of its own. `quiet` is far longer than any
    /// test, so nothing reaches the disk unless something flushes it.
    fn a_core(path: &std::path::Path) -> (Arc<Core>, tokio::runtime::Runtime) {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let store: Arc<dyn CrmStore> = Arc::new(JsonStore::at(path));
        let (saves, writer) = SaveQueue::channel(store, Duration::from_secs(30));
        rt.spawn(writer);
        // Outside Clatch there is no control pipe, and `connect` answers with a no-op one.
        let control = rt.block_on(clappkit::Control::connect(Vec::new())).expect("the standalone control");
        let core = Arc::new(Core {
            state: Mutex::new(AppState::new()),
            control,
            saves,
            origin: InstanceId::from_bytes([0xA1; 16]),
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
            Via::Cli,
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
                Via::Cli,
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
        rt.block_on(core.command(json!({ "cmd": "status" }), Some("agent-1".to_string()), Via::Cli));
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A person's write from the window takes the same road.
    #[test]
    fn a_write_from_the_window_survives_an_immediate_exit_too() {
        let dir = scratch_dir("window");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);

        let reply = rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Window Co" }), None, Via::Window));
        assert_eq!(reply.resp["ok"], true);
        // The person's edits keep the debounce — a drag is one save — so this is inside it…
        assert!(!path.exists(), "the fixture must really be inside the debounce window");
        // …and the exit (`crm close`, a quit from the window) is what makes it durable.
        assert!(core.saves.flush(None, FLUSH_WAIT));
        assert!(there_is(&relaunched(&path), "window-co"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Several writes, then the exit: the file holds all of them, not just the first.
    #[test]
    fn every_write_before_the_exit_is_kept_not_only_the_first() {
        let dir = scratch_dir("many");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);

        for name in ["Acme Corp", "Hooli", "Initech"] {
            rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": name }), None, Via::Window));
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
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "deal", "name": "Acme renewal" }), None, Via::Window));
        rt.block_on(core.command(
            json!({ "cmd": "task", "handle": "acme-renewal", "what": "Chase Maya", "due": "2026-09-01" }),
            None,
            Via::Window,
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
        let (core, rt) = a_core(&path);
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Hook Co" }), None, Via::Window));
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

    /// A store that cannot write — a full disk, a revoked directory.
    struct FailingStore;
    impl CrmStore for FailingStore {
        fn load(&self) -> anyhow::Result<crate::model::Db> {
            Ok(crate::model::Db::default())
        }
        fn save(&self, _: &crate::model::Db) -> anyhow::Result<()> {
            anyhow::bail!("disk full")
        }
    }

    /// At-most-once means the mark is durable *before* the signal. If it cannot be made
    /// durable the signal is withheld — one late, never two — and the task goes back to
    /// waiting rather than sitting marked in memory and forgotten.
    #[test]
    fn a_reminder_whose_mark_cannot_be_written_is_withheld_and_tried_again() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let (saves, writer) = SaveQueue::channel(Arc::new(FailingStore), Duration::from_secs(30));
        rt.spawn(writer);
        let control = rt.block_on(clappkit::Control::connect(Vec::new())).unwrap();
        let core = Core {
            state: Mutex::new(AppState::new()),
            control,
            saves,
            origin: InstanceId::from_bytes([0xA1; 16]),
        };
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "deal", "name": "Acme renewal" }), None, Via::Window));
        rt.block_on(core.command(
            json!({ "cmd": "task", "handle": "acme-renewal", "what": "Chase Maya", "due": "2026-09-01" }),
            None,
            Via::Window,
        ));

        let now = Now { at: 1_790_000_000_000, today: Date::new(2026, 9, 22) };
        let sweep = rt.block_on(core.sweep_once(vec![scout()], now, 0));
        assert!(sweep.emits.is_empty(), "sending without a recorded mark is how a reminder arrives twice");
        assert_eq!(sweep.snapshot["reminders"]["awaiting"], 1, "still waiting, not silently forgotten");
        let again = rt.block_on(core.sweep_once(vec![scout()], now, 0));
        assert!(again.emits.is_empty(), "and it keeps refusing while the disk does");
        assert_eq!(again.snapshot["reminders"]["awaiting"], 1);
    }

    /// A write the agent is told about, when the disk cannot take it, is still answered —
    /// the core did change; persistence is best-effort — and the failure is not silent.
    #[test]
    fn an_agents_write_on_a_failing_disk_is_still_answered() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let (saves, writer) = SaveQueue::channel(Arc::new(FailingStore), Duration::from_secs(30));
        rt.spawn(writer);
        let control = rt.block_on(clappkit::Control::connect(Vec::new())).unwrap();
        let core = Core { state: Mutex::new(AppState::new()), control, saves, origin: InstanceId::from_bytes([0xA1; 16]) };
        let reply = rt.block_on(core.command(
            json!({ "cmd": "add", "kind": "company", "name": "Acme" }),
            Some("agent-1".to_string()),
            Via::Cli,
        ));
        assert_eq!(reply.resp["ok"], true, "the app does not fall over because the disk did");
    }

    /// The regression the live run caught. The first version keyed durability on *who* was
    /// calling, and a `crm add` typed in a plain terminal carries no agent id — so the app
    /// took it for the window and debounced it, and a kill lost a write it had answered.
    /// The promise follows the channel the answer goes back on.
    #[test]
    fn a_crm_command_typed_in_a_plain_terminal_is_durable_before_it_is_answered_too() {
        let dir = scratch_dir("terminal");
        let path = dir.join("crm.json");
        let (core, rt) = a_core(&path);

        // The CLI socket, and no CLATCH_AGENT_ID: `caller` is None, exactly as for the window.
        let reply = rt.block_on(core.command(
            json!({ "cmd": "add", "kind": "company", "name": "Typed By Hand" }),
            None,
            Via::Cli,
        ));
        assert_eq!(reply.resp["ok"], true);
        assert!(there_is(&relaunched(&path), "typed-by-hand"), "answered, then lost");

        // …while the same command from the window stays inside the debounce.
        rt.block_on(core.command(json!({ "cmd": "add", "kind": "company", "name": "Dragged" }), None, Via::Window));
        assert!(!there_is(&relaunched(&path), "dragged"), "the window's edits keep the debounce");
        assert!(core.saves.flush(None, FLUSH_WAIT));
        assert!(there_is(&relaunched(&path), "dragged"), "and the exit flush keeps them");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
