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
        let roster = self.control.roster();
        let ctx = Ctx { now: clock(), entropy: entropy(), origin: self.origin.clone() };

        let (out, db) = {
            let mut state = self.state.lock().await;
            state.set_agents(roster);
            let out = state.command(&req, caller.as_deref(), &ctx);
            let db = out.dirty.then(|| state.db());
            (out, db)
        };

        // The core says the dataset changed; the writer decides when. Debounced, so a drag
        // is one save.
        if let Some(db) = db {
            self.saves.save(db);
        }

        // Only human actions signal. An agent is never told about its own write — that is
        // the loop that makes an app talk to itself.
        if caller.is_none() {
            self.control.emit_all(out.emits);
        }
        Reply::new(out.resp, out.snapshot)
    }
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
fn clock() -> Now {
    let now = chrono::Local::now();
    let at = now.timestamp_millis();
    Now { at, today: local_date(at, now.offset().local_minus_utc()) }
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

/// An absolute file path — a roster avatar — as a `data:` URI, because a webview cannot
/// open `file://` and Clatch hands out paths rather than bytes.
#[tauri::command]
fn asset(path: String) -> Option<String> {
    clappkit::app::asset(&path)
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

            // Register on the control pipe, on Tauri's own runtime so the reactive loop
            // shares it. Fatal on failure: an app that cannot reach Clatch has no agent
            // half, and a window pretending otherwise is worse than no window.
            let control = tauri::async_runtime::block_on(clappkit::connect_or_die(CLI));

            // Tauri's runtime, not a bare `tokio::spawn`: `setup` is the main thread and
            // has no tokio context of its own.
            let (saves, writer) = SaveQueue::channel(store, SAVE_QUIET);
            tauri::async_runtime::spawn(writer);

            let core = Arc::new(Core {
                state: Mutex::new(AppState::with_db(db)),
                control,
                saves,
                origin,
            });
            app.manage(core.clone());

            // The agent's channel: our own socket, which Clatch never sees. clappkit
            // answers the window verbs itself and pushes the snapshot we return.
            clappkit::app::spawn_ipc(handle, CLI, WindowPolicy::default(), move |req, caller| {
                let core = core.clone();
                async move { core.command(req, caller).await }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("{CLI}: the window failed to start: {e}");
            std::process::exit(1)
        });
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let now = clock();
        let offset = {
            use chrono::TimeZone;
            chrono::Local
                .timestamp_millis_opt(now.at)
                .single()
                .expect("an unambiguous local instant")
                .offset()
                .local_minus_utc()
        };
        assert_eq!(now.today, local_date(now.at, offset));
        assert!(now.today.y >= 2026, "{:?}", now.today);
        assert_eq!(Date::parse(&now.today.to_string_iso()), Some(now.today));
    }
}
