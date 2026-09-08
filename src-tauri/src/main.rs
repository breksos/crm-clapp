//! One binary, two roles, over one state.
//!
//! `crm app` is the window Clatch launches through the manifest's `launch`; `crm <verb>`
//! is the agent's CLI. `clappkit::role::main_dispatch` decides which at startup, so this
//! clapp ships one executable and the two surfaces cannot be built from different code.
//!
//! Everything below is transport. The rules live in [`state`], which is pure, and the
//! agent's manual lives in [`cli`].

mod cli;
mod state;

use clappkit::app::Reply;
use clappkit::window::WindowPolicy;
use serde_json::Value;
use state::AppState;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

const APP_ID: &str = "com.breksos.crm";
const CLI: &str = "crm";

/// The app's own mark, for the Dock (macOS) and the taskbar (Windows/Linux). The bytes
/// are ours because they *are* our identity; clappkit insets a full-bleed tile to the
/// native grid at runtime.
const ICON: &[u8] = include_bytes!("../../assets/icon.png");

fn main() {
    clappkit::role::main_dispatch(APP_ID, CLI, cli::run, gui);
}

/// Everything both surfaces share: the one state, and the live control pipe.
struct Core {
    state: Mutex<AppState>,
    control: clappkit::Control,
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
        // to reach out to fetch.
        let roster = self.control.roster();

        let out = {
            let mut state = self.state.lock().await;
            state.set_agents(roster);
            state.command(&req, caller.as_deref())
        };

        // Only human actions signal. An agent is never told about its own write — that is
        // the loop that makes an app talk to itself.
        if caller.is_none() {
            self.control.emit_all(out.emits);
        }
        Reply::new(out.resp, out.snapshot)
    }
}

/// The window's one call into the core. The person is never an agent, so the caller id is
/// `None` and their actions are the ones that signal.
#[tauri::command]
async fn run_cmd(core: tauri::State<'_, Arc<Core>>, req: Value) -> Result<Value, String> {
    Ok(core.inner().clone().command(req, None).await.resp)
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

            // Register on the control pipe, on Tauri's own runtime so the reactive loop
            // shares it. Fatal on failure: an app that cannot reach Clatch has no agent
            // half, and a window pretending otherwise is worse than no window.
            let control = tauri::async_runtime::block_on(clappkit::connect_or_die(CLI));

            let core = Arc::new(Core { state: Mutex::new(AppState::new()), control });
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
