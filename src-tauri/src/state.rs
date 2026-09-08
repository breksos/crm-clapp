//! The core. **Pure state and logic**: no files, no sockets, no platform code, and
//! nothing here reaches for the clock or the network. That is what makes the rules
//! testable without a window server, and it is why the tests are where the rules
//! actually live (`docs/architecture.md` §2).
//!
//! Both surfaces call the same methods on the same [`AppState`], so they cannot drift.
//! Persistence will reach it through the `CrmStore` port and never the other way round.
//!
//! **M0 is the shell.** It answers `status` and nothing else, which is the whole of what
//! M0 has to prove: that a command entered on either surface reaches one state and comes
//! back stamped. M1 fills in the records, the pipeline, the shared view state and the
//! rest of the snapshot — see `docs/work-orders/m0-m1-backend.md`.

use clappkit::control::Emit;
use clappkit::AgentRow;
use serde_json::{json, Value};

/// What one command produced: the answer for the caller who asked, the snapshot for
/// everyone who is looking, and the signals the app owes its agents.
///
/// The response and the snapshot are taken from **one** call so they leave the same
/// critical section — an app that re-locks to take a snapshot after answering has a
/// window in which the two describe different moments, and then the window is dragged
/// backwards by its own reply (`clappkit::snapshot`).
pub struct Outcome {
    pub resp: Value,
    pub snapshot: Value,
    /// Only ever non-empty for a **human** action. The agent already knows about its own
    /// writes, and telling it is the loop that makes an app talk to itself.
    pub emits: Vec<Emit>,
}

/// The whole of the app's state.
#[derive(Debug, Default)]
pub struct AppState {
    /// The agents bound to this app, as Clatch last described them. Not our data — the
    /// launcher's — but it rides the snapshot because the window draws it, so the core
    /// is handed it rather than fetching it. Keyed on the immutable `id`; `name` is a
    /// re-pointable label we only ever display.
    agents: Vec<AgentRow>,
}

impl AppState {
    pub fn new() -> AppState {
        AppState::default()
    }

    /// Replace the roster with Clatch's latest snapshot of it. A rename arrives as a
    /// fresh roster with the same id, so replacing wholesale is the correct read of the
    /// protocol — there is nothing to merge.
    pub fn set_agents(&mut self, agents: Vec<AgentRow>) {
        self.agents = agents;
    }

    /// One stamped view of everything both surfaces agree on.
    ///
    /// Stamped **here and nowhere else** (`clappkit::snapshot::with_rev`), so a response
    /// and the pushed `state` event carry the same `rev` when they describe the same
    /// moment.
    ///
    /// **Nothing secret ever enters this structure.** No credentials exist in v1, so that
    /// costs nothing today; it is written down because a snapshot goes everywhere and
    /// absence has to be by construction, not by redaction.
    ///
    /// M1 adds `pipeline`, `board`, `list` and `due` — the shape is frozen in that order,
    /// and M3 builds the window against it.
    pub fn snapshot(&self) -> Value {
        clappkit::snapshot::with_rev(json!({
            "ok": true,
            "focus": Value::Null,
            "pending": Value::Null,
            "counts": self.counts(),
            "agents": self.agents,
        }))
    }

    /// Counts of **active** records. Archived ones are excluded here, from the board and
    /// from default `find` results — `show` still loads them, and `archive --restore`
    /// brings them back. Zero across the board until M1 gives them something to count.
    fn counts(&self) -> Value {
        json!({
            "companies": 0,
            "contacts": 0,
            "deals": 0,
            "activities": 0,
            "tasks": 0,
        })
    }

    /// Apply one command envelope from either surface.
    ///
    /// `caller` is the agent id Clatch injected into the calling shell, or `None` for the
    /// person at the window. M1 turns it into an `Actor` so every logged line records who
    /// wrote it; M0 only has to carry it, so that the seam exists before anything depends
    /// on it.
    ///
    /// The window verbs (`focus`, `close`, `ping`) never arrive here — `clappkit`'s
    /// `window_cmd` answers those itself, because they are the app process rather than
    /// its state.
    pub fn command(&mut self, req: &Value, caller: Option<&str>) -> Outcome {
        let _ = caller; // M1: becomes `Actor::Agent { id }` / `Actor::Human`.
        let cmd = req.get("cmd").and_then(Value::as_str).unwrap_or("");
        let snapshot = self.snapshot();
        match cmd {
            // `status` is the agent's; `state` is the window asking for its first paint.
            // One answer, because there is one state.
            "status" | "state" => Outcome {
                resp: snapshot.clone(),
                snapshot,
                emits: Vec::new(),
            },
            other => Outcome {
                resp: unknown(other),
                snapshot,
                emits: Vec::new(),
            },
        }
    }
}

/// The refusal for a verb this build does not have. It names the manual rather than
/// listing the verbs, because `crm -h` is the manual and a second list would be a second
/// thing to keep true.
fn unknown(cmd: &str) -> Value {
    let what = if cmd.is_empty() { "a command with no verb".to_string() } else { format!("`{cmd}`") };
    json!({
        "ok": false,
        "error": format!("{what} is not a verb this build answers — see `crm -h`"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(name: &str) -> Value {
        json!({ "cmd": name })
    }

    #[test]
    fn status_answers_with_the_state_and_a_revision() {
        let mut st = AppState::new();
        let out = st.command(&cmd("status"), None);
        assert_eq!(out.resp["ok"], true);
        assert!(out.resp["rev"].as_u64().unwrap() > 0, "0 is reserved for an unstamped snapshot");
    }

    /// The race `clappkit::snapshot` exists to settle: an answer and the snapshot pushed
    /// alongside it describe ONE moment, so they must carry one revision. Two calls to
    /// `snapshot()` would silently give two.
    #[test]
    fn the_answer_and_the_pushed_snapshot_share_one_revision() {
        let mut st = AppState::new();
        let out = st.command(&cmd("status"), None);
        assert_eq!(out.resp["rev"], out.snapshot["rev"]);
    }

    #[test]
    fn revisions_only_ever_go_up() {
        let mut st = AppState::new();
        let first = st.command(&cmd("status"), None).snapshot["rev"].as_u64().unwrap();
        let second = st.command(&cmd("status"), None).snapshot["rev"].as_u64().unwrap();
        assert!(second > first, "{second} must follow {first}");
    }

    /// The window's first paint asks `state`; the agent asks `status`. They are the same
    /// question, and an app that answered only one of them would have a window that never
    /// filled in or a CLI that could not see.
    #[test]
    fn the_window_and_the_cli_ask_the_same_question() {
        let mut st = AppState::new();
        let window = st.command(&cmd("state"), None).resp;
        let agent = st.command(&cmd("status"), Some("agent-1")).resp;
        assert_eq!(window["ok"], true);
        assert_eq!(agent["ok"], true);
        assert_eq!(window["counts"], agent["counts"], "one state, one answer");
    }

    #[test]
    fn an_unknown_verb_is_refused_and_points_at_the_manual() {
        let mut st = AppState::new();
        let out = st.command(&cmd("move"), None);
        assert_eq!(out.resp["ok"], false);
        let err = out.resp["error"].as_str().unwrap();
        assert!(err.contains("`move`"), "{err}");
        assert!(err.contains("crm -h"), "the manual is the only place to send them: {err}");
    }

    /// A refusal must still leave the surfaces agreeing, so the snapshot rides along.
    #[test]
    fn a_refusal_still_carries_the_state() {
        let mut st = AppState::new();
        let out = st.command(&cmd("nonsense"), None);
        assert_eq!(out.snapshot["ok"], true);
        assert!(out.snapshot["rev"].as_u64().unwrap() > 0);
    }

    /// M0 has nothing to signal about, and M4 must not discover that the plumbing was
    /// never there. The rule it will be built on: only human actions signal.
    #[test]
    fn m0_emits_no_signals_from_either_surface() {
        let mut st = AppState::new();
        assert!(st.command(&cmd("status"), None).emits.is_empty());
        assert!(st.command(&cmd("status"), Some("agent-1")).emits.is_empty());
    }

    #[test]
    fn the_roster_rides_the_snapshot_and_a_rename_keeps_the_id() {
        let mut st = AppState::new();
        assert_eq!(st.snapshot()["agents"], json!([]));

        let row = |name: &str| AgentRow {
            id: "a-1".into(),
            name: name.into(),
            backend: Some("claude".into()),
            model: None,
            avatar: None,
        };
        st.set_agents(vec![row("Scout")]);
        assert_eq!(st.snapshot()["agents"][0]["name"], "Scout");

        // A rename is a fresh roster carrying the same id — the label changes, the key
        // does not, and nothing keyed on the id is dropped and re-created.
        st.set_agents(vec![row("Ranger")]);
        let snap = st.snapshot();
        assert_eq!(snap["agents"][0]["id"], "a-1");
        assert_eq!(snap["agents"][0]["name"], "Ranger");
    }

    /// A snapshot goes everywhere. v1 holds no credentials, so this passes trivially
    /// today — which is the point of writing it now rather than the day it can fail.
    #[test]
    fn nothing_secret_is_in_the_snapshot() {
        let st = AppState::new();
        let snap = st.snapshot().to_string().to_lowercase();
        for word in ["password", "secret", "token", "apikey", "api_key", "credential"] {
            assert!(!snap.contains(word), "`{word}` reached a snapshot");
        }
    }
}
