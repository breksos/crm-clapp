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

/// The clock, read here so the core never has to.
///
/// Reading the time is platform state, and a pure core has no business doing it — so it is
/// read once per command and handed in. Once, not twice: an instant and a date taken
/// separately can straddle midnight, and a snapshot that does is a snapshot nobody can
/// reproduce.
fn clock() -> Now {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Now { at, today: local_today() }
}

/// Today, as a civil date, in the machine's own timezone.
///
/// A due date is a human calendar concept: a task due "today" in Istanbul is not due on
/// UTC's today, and a reminder that fires on the wrong day reads as the app being
/// unreliable — the worst kind of bug for a CRM. So this is local, and `chrono` owns it,
/// because "what day is it here" is a timezone-database question (including the day the
/// clocks change) rather than arithmetic.
///
/// This is the **only** place the app reads local time. The core takes the date as a
/// value, which is why fixing it here fixed it everywhere.
fn local_today() -> Date {
    use chrono::Datelike;
    let d = chrono::Local::now().date_naive();
    Date { y: d.year(), m: d.month(), d: d.day() }
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

    /// The clock the core is handed and the calendar the core does its own arithmetic on
    /// must be the same calendar, or "due today" lands a day out — and only for people in
    /// some timezones, which is the worst way to find a bug.
    #[test]
    fn the_clock_and_the_calendar_agree_about_today() {
        let today = clock().today;
        assert_eq!(today.days_until(today), 0);

        // `model::Date` and `chrono` must place today at the same distance from a fixed
        // point. Two calendars would be two answers.
        let via_model = Date::new(1970, 1, 1).days_until(today);
        let via_chrono = (chrono::Local::now().date_naive()
            - chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
        .num_days();
        assert_eq!(via_model, via_chrono);
    }

    /// The fix QA asked for: this is the person's day, not UTC's. West of UTC after
    /// 00:00Z, and east of it before, the two differ — and the local one is the one a due
    /// date means.
    #[test]
    fn today_is_the_local_day_not_the_utc_one() {
        let now = clock();
        let utc_day = now.at.div_euclid(86_400_000);
        let local_day = Date::new(1970, 1, 1).days_until(now.today);
        assert!(
            (local_day - utc_day).abs() <= 1,
            "local and UTC can differ by at most a day: {local_day} vs {utc_day}"
        );

        let offset = chrono::Local::now().offset().local_minus_utc() as i64;
        let expected = (now.at / 1000 + offset).div_euclid(86_400);
        assert_eq!(local_day, expected, "today must follow this machine's UTC offset");
    }

    #[test]
    fn the_clock_produces_a_date_the_core_can_read_back() {
        let today = clock().today;
        assert!(today.y >= 2026, "{today:?}");
        assert!((1..=12).contains(&today.m), "{today:?}");
        assert!((1..=31).contains(&today.d), "{today:?}");
        assert_eq!(Date::parse(&today.to_string_iso()), Some(today));
    }
}
