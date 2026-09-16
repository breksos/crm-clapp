//! The agent's hands, and its only manual.
//!
//! **`crm -h` is the whole of the documentation an agent gets.** A verb missing from it
//! does not exist as far as the agent is concerned, and a verb declared in `clatch.json`
//! but unimplemented is a granted permission that fails at runtime. `clatch validate`
//! reads the manifest and nothing reads this file, so the two are held together here
//! instead: [`VERBS`] is the single table that the manual prints and `run` dispatches on,
//! and a test pins it against the manifest.
//!
//! **Three verbs so far.** `status` is the round-trip that proves the two surfaces are
//! wired to one state; `focus` and `close` are the window verbs clappkit answers itself.
//! The other sixteen are declared in the manifest and land in M2 — the manual says so
//! plainly rather than letting an agent discover it by being refused.
//!
//! M1 added no verbs and one section: the pipeline vocabulary, generated from the core's
//! own list so that the word the board draws, the word `crm move` accepts and the word
//! this page names cannot drift apart.
//!
//! **Nothing in this file ever prints an `id`.** A record's id is a ULID: stored,
//! referenced and keyed on, and unusable by anybody reading it. What goes to stdout is the
//! `handle` — `acme`, `acme-2` — because the only question that matters for a printed
//! string is whether the reader could type it back into a verb. A test at the bottom of
//! this file scans the output for anything that parses as a ULID and fails if one appears.

use crate::model::{MoveTarget, Stage, Status};
use serde_json::{json, Value};

/// This app's own manifest, embedded at compile time. The manual's "not yet" list and the
/// parity test both read it, so neither can drift from the surface the PM froze.
const MANIFEST: &str = include_str!("../../clatch.json");

use crate::{APP_ID, CLI};

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
    /// The app is not running, or the request was refused — including a verb that is
    /// declared in the manifest but has not landed yet. That is a **valid** request the
    /// build declined, not a malformed command line.
    pub const FAILED: i32 = 1;
    /// The command line was wrong: an unknown verb, or an argument to a verb that takes
    /// none. `crm -h` is the fix.
    pub const USAGE: i32 = 2;
}

/// What one invocation resolves to, before a single byte goes near the socket.
///
/// Split out from [`run`] so that every exit code and every message is decided by a pure
/// function a test can call — the alternative is a dispatch whose behaviour can only be
/// checked by spawning the binary, which is how the wrong exit code survived a milestone.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// Print the manual and stop.
    Manual,
    /// Send this verb to the running app.
    Ask(String),
    /// Refuse, with this message on stderr and this exit code.
    Refuse(String, i32),
}

/// Decide what an invocation means.
fn plan(args: &[String]) -> Plan {
    let verb = args.first().map(String::as_str).unwrap_or("");
    match verb {
        "-h" | "--help" | "help" => Plan::Manual,
        "" => Plan::Refuse(format!("{CLI}: no verb — see `{CLI} -h`"), exit::USAGE),
        v if VERBS.iter().any(|(name, _)| *name == v) => match args.get(1) {
            // All three of this build's verbs take no arguments, and they still will when
            // M2 gives the rest a grammar. Silently dropping one is worse than refusing
            // it: `crm status --json` that exits 0 having ignored the flag reads exactly
            // like `--json` worked.
            Some(extra) => Plan::Refuse(
                format!("{CLI}: `{v}` takes no arguments, and `{extra}` was given — see `{CLI} -h`"),
                exit::USAGE,
            ),
            None => Plan::Ask(v.to_string()),
        },
        other => {
            let declared_but_unbuilt = declared().iter().any(|(name, _)| name == other);
            let code = if declared_but_unbuilt { exit::FAILED } else { exit::USAGE };
            Plan::Refuse(refusal(other), code)
        }
    }
}

/// The agent's CLI. Never returns: every path prints and exits, so the exit code is the
/// verb's answer.
pub async fn run(args: Vec<String>) -> ! {
    match plan(&args) {
        Plan::Manual => {
            print!("{}", manual());
            std::process::exit(exit::OK)
        }
        Plan::Ask(verb) => ask(&verb).await,
        Plan::Refuse(message, code) => {
            eprintln!("{message}");
            std::process::exit(code)
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

    // The **handle**, never the id. Whatever is printed here is what the reader types
    // next — `crm show acme-renewal` — and a ULID is not something anybody can type.
    // There is deliberately no fall back to `id`: a snapshot with no handle prints the
    // kind alone, because reintroducing the id quietly is the bug this replaced.
    let focus = match snap.get("focus") {
        Some(f) if !f.is_null() => {
            let kind = f.get("kind").and_then(Value::as_str).unwrap_or("record");
            match f.get("handle").and_then(Value::as_str).filter(|h| !h.is_empty()) {
                Some(handle) => format!("{kind} {handle}"),
                None => kind.to_string(),
            }
        }
        _ => "nothing open".to_string(),
    };
    out.push_str(&format!("  looking at: {focus}\n"));

    // An agent's id is immutable and is what attribution is keyed on — and it is also not
    // something anybody types at this CLI, so the name is the whole of what is shown
    // (`docs/architecture.md` §4, the keying rule).
    let agents = snap.get("agents").and_then(Value::as_array).cloned().unwrap_or_default();
    if agents.is_empty() {
        out.push_str("  agents: none connected\n");
    } else {
        out.push_str("  agents:\n");
        for a in agents {
            let name = a.get("name").and_then(Value::as_str).unwrap_or("?");
            match a.get("backend").and_then(Value::as_str) {
                Some(b) => out.push_str(&format!("    {name} — {b}\n")),
                None => out.push_str(&format!("    {name}\n")),
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
pub(crate) fn manual() -> String {
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

    out.push_str(&pipeline_section());

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

/// The pipeline vocabulary, written out of the core's own list rather than typed here.
///
/// **Any enum a surface shows lives in the core.** The board draws a "Negotiation" column,
/// so `crm move` takes that word and this page names it — one list, three surfaces, and a
/// test in `state_tests.rs` pins all three against it. A vocabulary the manual spells out
/// by hand is one an agent learns wrong the first time somebody edits the core.
fn pipeline_section() -> String {
    let stages: Vec<&str> = Stage::ALL.iter().map(|s| s.word()).collect();
    let mut out = String::from("\nthe pipeline:\n");
    out.push_str(&format!("  {}\n", stages.join(" → ")));
    out.push_str(&format!(
        "  then {} or {} — those two are STATUSES, not stages.\n",
        Status::Won.word(),
        Status::Lost.word()
    ));
    out.push_str("  `crm move <deal> <word>` takes any of:\n");
    for line in wrap(&MoveTarget::vocabulary().join(", "), 66) {
        out.push_str(&format!("    {line}\n"));
    }
    out.push_str(
        "  Closing a deal leaves its stage where it was, so a won deal still\n\
        \x20 remembers the stage it was won out of. Moving a closed deal to a\n\
        \x20 stage reopens it.\n",
    );
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
    use crate::model::Ulid;

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

    /// A ULID, exactly as the core mints one — so these fixtures are the shape the code
    /// actually meets. The previous fixture wrote `"id": "acme"`, which was true before
    /// ids became ULIDs and made this test green against a model the app no longer had.
    fn an_id(seed: u8) -> String {
        Ulid::from_parts(1_788_861_600_000, [seed; 10]).to_string()
    }

    /// A realistic post-revision snapshot: ULIDs in every `id`, handles beside them.
    fn a_snapshot() -> Value {
        json!({
            "ok": true,
            "rev": 12,
            "focus": { "kind": "deal", "id": an_id(0x5A), "handle": "acme-renewal" },
            "counts": { "companies": 2, "contacts": 4, "deals": 3, "activities": 9, "tasks": 1 },
            "agents": [ { "id": an_id(0x11), "name": "Scout", "backend": "claude" } ],
        })
    }

    #[test]
    fn status_names_what_is_open_by_the_handle_somebody_could_type() {
        let out = status_lines(&a_snapshot());
        assert!(out.contains("deal acme-renewal"), "{out}");
        assert!(out.contains("deals 3"), "{out}");
        assert!(out.contains("Scout"), "the agent's name is what a person reads: {out}");
    }

    /// **The regression guard.** An id reaching stdout is the defect QA found, and it
    /// survived a whole revision because the test above pinned the pre-revision model.
    /// This one does not care which field leaked: it scans the output for anything that
    /// parses as a ULID.
    #[test]
    fn no_id_ever_reaches_stdout() {
        let snap = a_snapshot();
        let out = status_lines(&snap);
        assert!(
            first_ulid_in(&out).is_none(),
            "an id reached stdout: {:?} in\n{out}",
            first_ulid_in(&out)
        );

        // …and the same page really did have ids in it to leak.
        assert!(Ulid::parse(snap["focus"]["id"].as_str().unwrap()).is_some());
        assert!(Ulid::parse(snap["agents"][0]["id"].as_str().unwrap()).is_some());
    }

    /// The window verbs answer with a sentence; nothing there can carry an id either.
    #[test]
    fn no_id_reaches_stdout_from_any_verb_this_build_answers() {
        for (verb, _) in VERBS {
            let out = render(verb, &a_snapshot());
            assert!(first_ulid_in(&out).is_none(), "`{verb}` printed an id:\n{out}");
        }
    }

    /// Any run of Crockford base32 long enough to be a ULID. Deliberately not "does it
    /// equal the id we put in": a leak through a different field is the same defect.
    fn first_ulid_in(text: &str) -> Option<String> {
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .find(|word| Ulid::parse(word).is_some())
            .map(str::to_string)
    }

    /// A snapshot whose focus somehow carries no handle must not fall back to the id. The
    /// kind alone is worse to read and correct to print.
    #[test]
    fn a_focus_without_a_handle_prints_the_kind_rather_than_the_id() {
        let out = status_lines(&json!({
            "focus": { "kind": "deal", "id": an_id(7) },
            "counts": {},
            "agents": []
        }));
        assert!(out.contains("looking at: deal"), "{out}");
        assert!(first_ulid_in(&out).is_none(), "{out}");
    }

    // MARK: - What an invocation means

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn refusal_for(v: &[&str]) -> (String, i32) {
        match plan(&argv(v)) {
            Plan::Refuse(msg, code) => (msg, code),
            other => panic!("{v:?} was not refused: {other:?}"),
        }
    }

    #[test]
    fn a_verb_this_build_answers_is_sent_as_it_is() {
        assert_eq!(plan(&argv(&["status"])), Plan::Ask("status".into()));
        assert_eq!(plan(&argv(&["close"])), Plan::Ask("close".into()));
        assert_eq!(plan(&argv(&["-h"])), Plan::Manual);
        assert_eq!(plan(&argv(&["--help"])), Plan::Manual);
        assert_eq!(plan(&argv(&["help"])), Plan::Manual);
    }

    /// A declared verb that has not landed is a **valid request the build refused** — the
    /// `1` the manual already documents. Exit 2 means the command line was wrong, and
    /// `crm add` is not wrong, it is early. An agent branching on the code has to be able
    /// to tell those apart without parsing prose.
    #[test]
    fn a_declared_but_unbuilt_verb_is_a_refusal_not_a_usage_error() {
        let (msg, code) = refusal_for(&["add"]);
        assert_eq!(code, exit::FAILED, "{msg}");
        assert!(msg.contains("M2"), "{msg}");
    }

    #[test]
    fn a_verb_that_never_existed_is_a_usage_error() {
        let (msg, code) = refusal_for(&["teleport"]);
        assert_eq!(code, exit::USAGE, "{msg}");
        assert!(msg.contains("not a verb"), "{msg}");
    }

    #[test]
    fn no_verb_at_all_is_a_usage_error() {
        assert_eq!(refusal_for(&[]).1, exit::USAGE);
        assert_eq!(refusal_for(&[""]).1, exit::USAGE);
    }

    /// An argument that is quietly dropped reads exactly like an argument that worked.
    /// `crm status --json` exiting 0 with human text is the worst of both.
    #[test]
    fn an_argument_to_a_verb_that_takes_none_is_refused_rather_than_dropped() {
        for line in [
            vec!["status", "--json"],
            vec!["status", "extra", "args", "here"],
            vec!["close", "--please"],
            vec!["focus", "now"],
        ] {
            let (msg, code) = refusal_for(&line);
            assert_eq!(code, exit::USAGE, "{line:?} → {msg}");
            assert!(msg.contains(line[0]), "the refusal must name the verb: {msg}");
            assert!(msg.contains(line[1]), "…and what it could not use: {msg}");
            assert!(msg.contains("crm -h"), "…and where to look: {msg}");
        }
    }

    /// The three codes the manual documents, and nothing else.
    #[test]
    fn every_exit_code_this_file_can_produce_is_one_the_manual_names() {
        let manual = manual();
        for code in [exit::OK, exit::FAILED, exit::USAGE] {
            assert!(manual.contains(&format!("  {code}  ")), "the manual never names {code}");
        }
        for line in [vec![], vec!["status"], vec!["add"], vec!["teleport"], vec!["status", "-x"]] {
            if let Plan::Refuse(_, code) = plan(&argv(&line)) {
                assert!([exit::FAILED, exit::USAGE].contains(&code), "{line:?} → {code}");
            }
        }
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
