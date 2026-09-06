//! The agent's hands, and its only manual.
//!
//! **`crm -h` is the whole of the documentation an agent gets.** A verb missing from it
//! does not exist as far as the agent is concerned, and a verb declared in `clatch.json`
//! but unimplemented is a granted permission that fails at runtime. `clatch validate`
//! reads the manifest and nothing reads this file, so the two are held together here
//! instead: [`VERBS`] is the single table that the manual prints and `run` dispatches on,
//! and a test pins it against the manifest.
//!
//! **M0 ships three verbs.** `status` is the round-trip that proves the two surfaces are
//! wired to one state; `focus` and `close` are the window verbs clappkit answers itself.
//! The other sixteen are declared in the manifest and land in M2 — the manual says so
//! plainly rather than letting an agent discover it by being refused.

use serde_json::{json, Value};

/// This app's own manifest, embedded at compile time. The manual's "not yet" list and the
/// parity test both read it, so neither can drift from the surface the PM froze.
const MANIFEST: &str = include_str!("../../clatch.json");

const APP_ID: &str = "com.breksos.crm";
const CLI: &str = "crm";

/// The verbs this build actually answers, with the `about` line each one carries in
/// `connector.commands`. One table: the manual prints it and [`run`] dispatches on it, so
/// the manual cannot describe a verb the binary does not have.
const VERBS: &[(&str, &str)] = &[
    ("status", "what both surfaces are looking at, plus counts and connected agents"),
    ("focus", "focus the app window"),
    ("close", "quit the app"),
];

/// Exit codes, so an agent can branch without parsing prose.
mod exit {
    /// The app answered.
    pub const OK: i32 = 0;
    /// The app is not running, or the answer said `ok: false`.
    pub const FAILED: i32 = 1;
    /// The command line was wrong — an unknown verb. `crm -h` is the fix.
    pub const USAGE: i32 = 2;
}

/// The agent's CLI. Never returns: every path prints and exits, so the exit code is the
/// verb's answer.
pub async fn run(args: Vec<String>) -> ! {
    let verb = args.first().map(String::as_str).unwrap_or("");
    match verb {
        "-h" | "--help" | "help" => {
            print!("{}", manual());
            std::process::exit(exit::OK)
        }
        v if VERBS.iter().any(|(name, _)| *name == v) => ask(v).await,
        "" => {
            eprintln!("{CLI}: no verb — see `{CLI} -h`");
            std::process::exit(exit::USAGE)
        }
        other => {
            eprintln!("{}", refusal(other));
            std::process::exit(exit::USAGE)
        }
    }
}

/// Send one verb to the running app and print what comes back.
async fn ask(verb: &str) -> ! {
    // Clatch injects `CLATCH_AGENT_ID` into the calling agent's shell, so the app knows
    // who asked. Absent means the person ran it themselves. `clappkit::app::spawn_ipc`
    // reads it back out of `agent` and hands it to the core.
    let mut req = json!({ "cmd": verb });
    if let Some(id) = std::env::var("CLATCH_AGENT_ID").ok().filter(|s| !s.is_empty()) {
        req["agent"] = Value::from(id);
    }

    match clappkit::ipc::request(CLI, &req).await {
        Ok(resp) => {
            if resp.get("ok").and_then(Value::as_bool) == Some(false) {
                let msg = resp.get("error").and_then(Value::as_str).unwrap_or("the app refused");
                eprintln!("{CLI}: {msg}");
                std::process::exit(exit::FAILED)
            }
            print!("{}", render(verb, &resp));
            std::process::exit(exit::OK)
        }
        Err(e) => {
            eprintln!("{}", transport_error(&e.to_string()));
            std::process::exit(exit::FAILED)
        }
    }
}

/// What the agent reads when a verb succeeds.
fn render(verb: &str, resp: &Value) -> String {
    match verb {
        "status" => status_lines(resp),
        // The window verbs answer with a sentence and no state; that sentence is the
        // whole of their output.
        _ => match resp.get("message").and_then(Value::as_str) {
            Some(m) => format!("{m}\n"),
            None => String::new(),
        },
    }
}

/// `status` as a person or an agent reads it: what both surfaces are looking at, the
/// counts, and who else is connected.
fn status_lines(snap: &Value) -> String {
    let n = |key: &str| snap.pointer(&format!("/counts/{key}")).and_then(Value::as_u64).unwrap_or(0);
    let mut out = String::new();
    out.push_str("Breksos CRM — running\n");
    out.push_str(&format!(
        "  companies {}   contacts {}   deals {}   activities {}   tasks {}\n",
        n("companies"),
        n("contacts"),
        n("deals"),
        n("activities"),
        n("tasks"),
    ));

    let focus = match snap.get("focus") {
        Some(f) if !f.is_null() => {
            let kind = f.get("kind").and_then(Value::as_str).unwrap_or("record");
            let id = f.get("id").and_then(Value::as_str).unwrap_or("?");
            format!("{kind} {id}")
        }
        _ => "nothing open".to_string(),
    };
    out.push_str(&format!("  looking at: {focus}\n"));

    let agents = snap.get("agents").and_then(Value::as_array).cloned().unwrap_or_default();
    if agents.is_empty() {
        out.push_str("  agents: none connected\n");
    } else {
        out.push_str("  agents:\n");
        for a in agents {
            let name = a.get("name").and_then(Value::as_str).unwrap_or("?");
            let id = a.get("id").and_then(Value::as_str).unwrap_or("?");
            match a.get("backend").and_then(Value::as_str) {
                Some(b) => out.push_str(&format!("    {name} ({id}) — {b}\n")),
                None => out.push_str(&format!("    {name} ({id})\n")),
            }
        }
    }
    out
}

/// clappkit's connect errors are already agent-actionable; the one thing it cannot know
/// is our app id, so it writes `<id>` as a placeholder. Fill it in — an instruction the
/// agent can paste is worth more than a correct sentence it has to finish itself.
fn transport_error(msg: &str) -> String {
    msg.replace("<id>", APP_ID)
}

/// The refusal for a verb that is declared but not yet built, and for one that is neither.
/// Both name the manual; only the first can promise it is coming, and it says which
/// milestone rather than "soon".
fn refusal(verb: &str) -> String {
    if declared().iter().any(|(name, _)| name == verb) {
        format!(
            "{CLI}: `{verb}` is declared in clatch.json but lands in M2 — \
             `{CLI} -h` lists what this build answers"
        )
    } else {
        format!("{CLI}: `{verb}` is not a verb — see `{CLI} -h`")
    }
}

/// Every `{name, about}` in the manifest's `connector.commands`, in manifest order.
///
/// A parse failure here is a packaging bug, not a runtime condition — the manifest is
/// embedded from the repository at compile time — so an empty list is returned and the
/// manual simply stops advertising what is coming, rather than the CLI dying.
fn declared() -> Vec<(String, String)> {
    serde_json::from_str::<Value>(MANIFEST)
        .ok()
        .and_then(|m| m.pointer("/connector/commands").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| {
            Some((
                c.get("name")?.as_str()?.to_string(),
                c.get("about").and_then(Value::as_str).unwrap_or("").to_string(),
            ))
        })
        .collect()
}

/// The manual. Everything an agent knows about this app, and nothing that is not true of
/// this build.
fn manual() -> String {
    let mut out = String::new();
    out.push_str("crm — Breksos CRM\n\n");
    out.push_str(
        "A sales pipeline you and your agent share. Whatever either of you opens, logs\n\
         or moves, the other is looking at it too: `crm show acme` opens that record in\n\
         the person's window, and their drag of a card reaches you as a signal.\n\n",
    );
    out.push_str("usage:\n  crm <verb> [args]\n\nverbs:\n");
    let width = VERBS.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    for (name, about) in VERBS {
        out.push_str(&format!("  {name:<width$}  {about}\n"));
    }

    let coming: Vec<String> = declared()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !VERBS.iter().any(|(v, _)| v == name))
        .collect();
    if !coming.is_empty() {
        out.push_str("\nnot in this build — M2 lands the rest of the pipeline verbs:\n");
        for line in wrap(&coming.join(", "), 68) {
            out.push_str(&format!("  {line}\n"));
        }
        out.push_str(
            "Each is declared in clatch.json, so the grant already exists; calling one\n\
             today is refused. This list is generated from the manifest, so a verb\n\
             appears above the day it works and never before.\n",
        );
    }

    out.push_str("\nexit codes:\n");
    out.push_str("  0  the app answered\n");
    out.push_str("  1  the app is not running, or it refused\n");
    out.push_str("  2  the command line was wrong — see this page\n");
    out.push_str(&format!("\nThe app must be running: `clatch run {APP_ID}`.\n"));
    out
}

/// Wrap on spaces at `width`. A manual that runs off an 80-column terminal is a manual
/// somebody has to scroll sideways to read.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule that has no other enforcement (playbook §4). `clatch validate` checks the
    /// manifest; nothing checks that the code agrees with it — so this does.
    ///
    /// M0 implements a subset on purpose, and the manual says so. What is never allowed
    /// in either milestone is the other direction: a verb the manual offers that the
    /// manifest does not declare is a command with no grant behind it, which fails in
    /// front of an agent that was told it would work.
    #[test]
    fn every_verb_the_manual_offers_is_declared_in_the_manifest() {
        let declared = declared();
        assert!(!declared.is_empty(), "the embedded manifest must parse");
        for (name, _) in VERBS {
            assert!(
                declared.iter().any(|(d, _)| d == name),
                "`{name}` is in the manual but not in clatch.json's connector.commands"
            );
        }
    }

    /// The manifest's `about` is what a person reads when granting the verb; the manual is
    /// what the agent reads before using it. Two descriptions of one verb is two things to
    /// keep true, so they are the same string.
    #[test]
    fn the_manual_and_the_manifest_describe_each_verb_identically() {
        let declared = declared();
        for (name, about) in VERBS {
            let (_, manifest_about) = declared.iter().find(|(d, _)| d == name).unwrap();
            assert_eq!(manifest_about, about, "`{name}` is described two different ways");
        }
    }

    /// The manual is the agent's only documentation, so anything true of this build has to
    /// be on this page: how to run it, what it answers, and what it does not.
    #[test]
    fn the_manual_names_every_verb_this_build_answers() {
        let m = manual();
        for (name, about) in VERBS {
            assert!(m.contains(name), "`{name}` is missing from the manual");
            assert!(m.contains(about), "`{name}`'s description is missing from the manual");
        }
        assert!(m.contains(APP_ID), "the manual must say how to start the app");
    }

    /// M0's honesty requirement: the sixteen verbs that are declared and unbuilt are named
    /// as not-yet rather than silently absent, and the list comes from the manifest so it
    /// empties itself as M2 lands them.
    #[test]
    fn the_manual_says_which_declared_verbs_are_not_in_this_build() {
        let m = manual();
        assert!(m.contains("M2"), "the manual must say when the rest arrives");
        for (name, _) in declared() {
            assert!(m.contains(&name), "`{name}` is declared but the manual never mentions it");
        }
    }

    /// A declared-but-unbuilt verb and a verb that never existed are different mistakes,
    /// and an agent can act on the difference: one is worth retrying after an update.
    #[test]
    fn a_declared_verb_and_a_typo_are_refused_differently() {
        let declared_but_unbuilt = refusal("move");
        assert!(declared_but_unbuilt.contains("M2"), "{declared_but_unbuilt}");

        let typo = refusal("mvoe");
        assert!(!typo.contains("M2"), "{typo}");
        assert!(typo.contains("not a verb"), "{typo}");
    }

    /// Both refusals send the reader to the one place that is kept true.
    #[test]
    fn every_refusal_points_at_the_manual() {
        for verb in ["move", "mvoe", "board"] {
            assert!(refusal(verb).contains("crm -h"), "{verb}: {}", refusal(verb));
        }
    }

    /// The negative smoke test's sentence (playbook §9). clappkit writes `<id>` because it
    /// cannot know ours; an agent should never be handed a placeholder to fill in.
    #[test]
    fn the_not_running_error_names_the_app_id_to_start() {
        let raw = "crm: app is not running — start it with `clatch run <id>`";
        let shown = transport_error(raw);
        assert!(shown.contains(APP_ID), "{shown}");
        assert!(!shown.contains("<id>"), "the placeholder reached the agent: {shown}");
    }

    #[test]
    fn status_reads_as_prose_and_survives_a_snapshot_with_nothing_in_it() {
        let out = status_lines(&json!({
            "ok": true, "rev": 1, "focus": null,
            "counts": { "companies": 0, "contacts": 0, "deals": 0, "activities": 0, "tasks": 0 },
            "agents": []
        }));
        assert!(out.contains("companies 0"), "{out}");
        assert!(out.contains("nothing open"), "{out}");
        assert!(out.contains("none connected"), "{out}");

        // A snapshot missing keys entirely must not panic — the CLI and the core version
        // in front of a user are not always the same build.
        let bare = status_lines(&json!({ "ok": true }));
        assert!(bare.contains("deals 0"), "{bare}");
    }

    #[test]
    fn status_names_what_is_open_and_who_is_connected() {
        let out = status_lines(&json!({
            "focus": { "kind": "deal", "id": "acme" },
            "counts": { "deals": 3 },
            "agents": [ { "id": "a-1", "name": "Scout", "backend": "claude" } ]
        }));
        assert!(out.contains("deal acme"), "{out}");
        assert!(out.contains("deals 3"), "{out}");
        // The name is what a person reads; the id is what everything is keyed on, so
        // both are shown.
        assert!(out.contains("Scout"), "{out}");
        assert!(out.contains("a-1"), "{out}");
    }

    #[test]
    fn a_window_verb_prints_its_sentence_and_nothing_else() {
        assert_eq!(render("close", &json!({ "ok": true, "message": "bye" })), "bye\n");
        assert_eq!(render("focus", &json!({ "ok": true })), "");
    }

    #[test]
    fn the_manual_fits_an_eighty_column_terminal() {
        for line in manual().lines() {
            assert!(line.chars().count() <= 80, "{} chars: {line}", line.chars().count());
        }
    }
}
