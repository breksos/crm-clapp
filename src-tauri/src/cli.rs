//! The agent's hands, and its only manual.
//!
//! **`crm -h` is the whole of the documentation an agent gets.** A verb missing from it
//! does not exist as far as the agent is concerned, and a verb declared in `clatch.json`
//! but unimplemented is a granted permission that fails at runtime. `clatch validate`
//! reads the manifest and nothing reads this file, so the two are held together here
//! instead: [`VERBS`] is the single table that the manual prints and [`plan`] dispatches
//! on, and a test pins it against the manifest.
//!
//! **M2 lands the rest of the pipeline.** All nineteen declared verbs answer now — the
//! seventeen agent verbs below plus `focus`/`close`, which clappkit answers itself before
//! a request ever reaches [`crate::state::AppState::command`]. Every write verb here turns
//! what an agent typed into the exact same envelope shape the window's own controls send
//! (`docs/work-orders/round-3-snapshot.md` §5, extended by `m2-cli.md`), so the core has
//! one implementation of every rule and the two surfaces cannot drift.
//!
//! **Nothing in this file ever prints an `id`.** A record's id is a ULID: stored,
//! referenced and keyed on, and unusable by anybody reading it. What goes to stdout is the
//! `handle` — `acme`, `acme-2` — because the only question that matters for a printed
//! string is whether the reader could type it back into a verb. A test scans every verb's
//! rendered output for anything that parses as a ULID and fails if one appears.
//!
//! **Exit codes, precisely.** `0` the app answered. `1` the app is not running, or a
//! *valid* request the core declined — a handle that matches nothing, a stage that does
//! not exist, a field this kind of record does not have, archiving something already
//! archived. `2` the command line itself was wrong — an unknown verb or option, the wrong
//! number of arguments, a date that does not parse. The dividing line is deliberate: shape
//! is checked here, before a single byte reaches the socket; everything else is the
//! core's vocabulary, and only it can say whether a word in that vocabulary is good.

use crate::model::{ActivityKind, Kind, MoveTarget, Stage, Status};
use serde_json::{json, Map, Value};

/// This app's own manifest, embedded at compile time. The manual's verb table and the
/// parity test both read it, so neither can drift from the surface the PM froze.
const MANIFEST: &str = include_str!("../../clatch.json");

use crate::{APP_ID, CLI};

/// Every verb this build answers, with the `about` line it carries in
/// `connector.commands` — verbatim, so a test can pin the two descriptions identical. In
/// manifest order, which is the order the manual lists them in.
const VERBS: &[(&str, &str)] = &[
    ("find", "search contacts, companies and deals — results land in the shared list"),
    ("show", "one record in full, and open it in the window"),
    ("board", "the pipeline: every stage, its deals and its totals"),
    ("stages", "the stage vocabulary, in order"),
    ("due", "next steps that are overdue, due today, or due this week"),
    ("status", "what both surfaces are looking at, plus counts and connected agents"),
    ("export", "write records as CSV or JSON; prints the file path"),
    ("add", "create a company, contact or deal"),
    ("set", "edit fields on a record"),
    ("log", "record a call, email, meeting or note against a record"),
    ("move", "change a deal's stage, or close it won or lost"),
    ("task", "set a next step with a due date"),
    ("done", "complete a next step"),
    ("link", "associate a contact, company and deal"),
    ("archive", "retire a record, or restore one — never a delete"),
    ("select", "answer a pending question, or open result N"),
    ("import", "read records from a CSV or vCard file"),
    ("focus", "focus the app window"),
    ("close", "quit the app"),
];

/// Every verb's usage line, exactly as `m2-cli.md` § the argument grammar freezes it,
/// minus the leading `crm `. `add` is three lines, because it is really three verbs
/// wearing one name — [`usage_lines`] is what the manual and `-h` actually print.
const USAGE_LINES: &[(&str, &str)] = &[
    ("find", "find [<query>] [--kind company|contact|deal|all] [--sort updated|name|value] [--page N] [--archived] [-n N]"),
    ("show", "show <handle>"),
    ("board", "board [--stage <stage>]"),
    ("stages", "stages"),
    ("due", "due [--overdue | --today | --week]"),
    ("status", "status"),
    ("export", "export <kind> [--format csv|json] [--out <path>]"),
    ("set", "set <handle> <field> <value>"),
    ("log", "log <call|email|meeting|note> <handle> <body> [--at <date>]"),
    ("move", "move <handle> <stage | won | lost>"),
    ("task", "task <handle> <what> --due <date>"),
    ("done", "done <task-handle>"),
    ("link", "link <handle> <handle>"),
    ("archive", "archive <handle> [--restore]"),
    ("select", "select <n>"),
    ("import", "import <path> [--kind companies|contacts|deals]"),
    ("focus", "focus"),
    ("close", "close"),
];

/// One or more usage lines for a verb — more than one only for `add`, whose three kinds
/// each take different flags.
fn usage_lines(verb: &str) -> Vec<String> {
    if verb == "add" {
        return vec![
            "add company <name> [--domain D] [--tag T]...".to_string(),
            "add contact <name> [--company <handle>] [--email E] [--phone P] [--title T]".to_string(),
            "add deal <title> [--company <handle>] [--value N] [--currency ISO] [--stage <stage>]"
                .to_string(),
        ];
    }
    let line = USAGE_LINES.iter().find(|(v, _)| *v == verb).map(|(_, l)| l.to_string()).unwrap_or_else(|| verb.to_string());
    vec![line]
}

/// What `crm <verb> -h` says beyond the usage line, for the few verbs whose arguments are
/// not something an agent could be expected to already have. A next step's handle is the
/// case: nothing said a task *had* one, or where to read it back.
fn verb_note(verb: &str) -> Option<&'static str> {
    match verb {
        "task" => Some(
            "The new next step gets a handle of its own, made from its text\n\
             (`Send the contract` -> `send-the-contract`). `crm show <record>` and\n\
             `crm due` list open ones with their handles; `crm done <handle>` finishes one.\n",
        ),
        "done" => Some(
            "<task-handle> is a next step's own handle, not the record it is on. Open ones\n\
             are listed, handle first, by `crm show <record>` and by `crm due`.\n",
        ),
        "due" => Some(
            "Lists open next steps that are overdue, due today or due within a week, each\n\
             with the handle `crm done` takes and the record it is on.\n",
        ),
        _ => None,
    }
}

/// The primary usage line, for embedding in a one-line error. `verb` may be a compound
/// name like `"add company"` — [`tokenize`] and [`wrong_count`] are handed that exact
/// string as `verb` when parsing one of `add`'s three forms, so their own error messages
/// stay specific rather than falling back to bare `add`'s three-line usage.
fn usage(verb: &str) -> String {
    if verb.starts_with("add ") {
        let line = usage_lines("add").into_iter().find(|l| l.starts_with(verb)).unwrap_or_else(|| verb.to_string());
        return format!("{CLI} {line}");
    }
    format!("{CLI} {}", usage_lines(verb).first().cloned().unwrap_or_default())
}

/// Exit codes, so an agent can branch without parsing prose.
mod exit {
    /// The app answered.
    pub const OK: i32 = 0;
    /// The app is not running, or the request was refused — including a verb that is
    /// declared in the manifest but has not landed yet, or a *valid* request the core
    /// declined (an unknown handle, an unknown field, an already-archived record). That
    /// is a granted request that failed, not a malformed command line.
    pub const FAILED: i32 = 1;
    /// The command line was wrong: an unknown verb or option, the wrong number of
    /// arguments, an unparseable date. `crm <verb> -h` is the fix.
    pub const USAGE: i32 = 2;
}

/// What one invocation resolves to, before a single byte goes near the socket.
///
/// Split out from [`run`] so that every exit code and every message is decided by a pure
/// function a test can call — the alternative is a dispatch whose behaviour can only be
/// checked by spawning the binary, which is how the wrong exit code survived a milestone.
#[derive(Debug, PartialEq)]
enum Plan {
    /// Print the manual and stop.
    Manual,
    /// Print one verb's own usage and stop — `crm show -h`.
    VerbHelp(String),
    /// Send this verb, with this envelope already built, to the running app.
    Ask(String, Value),
    /// Refuse, with this message on stderr and this exit code.
    Refuse(String, i32),
}

// MARK: - Splitting a command line into positionals and flags

/// One flag a verb's grammar accepts.
struct Flag {
    name: &'static str,
    takes_value: bool,
    /// `--tag` may appear more than once; everything else may not.
    repeatable: bool,
}

const fn flag(name: &'static str, takes_value: bool) -> Flag {
    Flag { name, takes_value, repeatable: false }
}

const fn repeatable_flag(name: &'static str) -> Flag {
    Flag { name, takes_value: true, repeatable: true }
}

/// Positionals in order, and every flag actually given, in the order given — so a
/// handler can require exactly one occurrence of a flag, or read all of a repeatable
/// one's values.
struct Tokens {
    positionals: Vec<String>,
    flags: Vec<(&'static str, Option<String>)>,
}

impl Tokens {
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags.iter().find(|(n, _)| *n == name).and_then(|(_, v)| v.as_deref())
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| *n == name)
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.flags.iter().filter(|(n, _)| *n == name).filter_map(|(_, v)| v.as_deref()).collect()
    }
}

/// Split `args` into positionals and flags against one verb's grammar. Nothing is
/// silently dropped: an option not in `grammar`, a flag given twice when it may only
/// appear once, or a value-taking flag with nothing after it are all refused by name,
/// pointing at the verb's own `-h`.
fn tokenize(verb: &str, args: &[String], grammar: &[Flag]) -> Result<Tokens, (String, i32)> {
    let mut positionals = Vec::new();
    let mut flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with('-') && a.len() > 1 {
            let Some(spec) = grammar.iter().find(|f| f.name == a) else {
                return Err(unknown_option(verb, a));
            };
            if !spec.repeatable && flags.iter().any(|(n, _)| *n == spec.name) {
                return Err((
                    format!("{CLI}: `{verb} {}` was given more than once — `{}`", spec.name, usage(verb)),
                    exit::USAGE,
                ));
            }
            if spec.takes_value {
                let Some(v) = args.get(i + 1) else {
                    return Err((
                        format!("{CLI}: `{verb} {}` needs a value — `{}`", spec.name, usage(verb)),
                        exit::USAGE,
                    ));
                };
                flags.push((spec.name, Some(v.clone())));
                i += 2;
            } else {
                flags.push((spec.name, None));
                i += 1;
            }
        } else {
            positionals.push(a.clone());
            i += 1;
        }
    }
    Ok(Tokens { positionals, flags })
}

fn unknown_option(verb: &str, opt: &str) -> (String, i32) {
    (format!("{CLI}: `{verb}` has no option `{opt}` — `{}`, see `{CLI} {verb} -h`", usage(verb)), exit::USAGE)
}

fn wrong_count(verb: &str, wanted: &[&str], given: &[String]) -> (String, i32) {
    if given.len() < wanted.len() {
        let missing = wanted[given.len()];
        return (
            format!("{CLI}: `{verb}` needs {missing} — `{}`, see `{CLI} {verb} -h`", usage(verb)),
            exit::USAGE,
        );
    }
    let extra = &given[wanted.len()];
    (
        format!(
            "{CLI}: `{verb}` takes {}, and `{extra}` was given as well — `{}`",
            wanted.join(" "),
            usage(verb)
        ),
        exit::USAGE,
    )
}

/// `--due`/`--at` on the command line: a date is a **shape** question (does this look
/// like `YYYY-MM-DD`), so it is refused here, at exit 2, before anything is sent — never
/// left for the core to refuse at 1. A stage word, by contrast, is core vocabulary and
/// stays core-validated; the two look similar and are deliberately not treated the same.
fn parse_date_arg(verb: &str, flag_name: &str, raw: &str) -> Result<String, (String, i32)> {
    crate::model::Date::parse(raw)
        .map(|_| raw.to_string())
        .ok_or_else(|| {
            (
                format!("{CLI}: `{verb} {flag_name}` wants a date as YYYY-MM-DD, not `{raw}`"),
                exit::USAGE,
            )
        })
}

/// A whole, non-negative number on the command line — `select <n>`, `--page N`, `-n N`.
/// Also a shape question, at exit 2.
fn parse_uint_arg(verb: &str, what: &str, raw: &str) -> Result<u64, (String, i32)> {
    raw.parse::<u64>().map_err(|_| {
        (format!("{CLI}: `{verb}` wants {what} as a whole number, not `{raw}`"), exit::USAGE)
    })
}

// MARK: - Deciding what an invocation means

/// Decide what an invocation means: which verb, and — for a verb this build answers —
/// the fully-built envelope to send, or the refusal to print instead.
fn plan(args: &[String]) -> Plan {
    let verb = args.first().map(String::as_str).unwrap_or("");
    match verb {
        "-h" | "--help" | "help" => Plan::Manual,
        "" => Plan::Refuse(format!("{CLI}: no verb — see `{CLI} -h`"), exit::USAGE),
        v if VERBS.iter().any(|(name, _)| *name == v) => plan_verb(v, &args[1..]),
        other => {
            let declared_but_unbuilt = declared().iter().any(|(name, _)| name == other);
            let code = if declared_but_unbuilt { exit::FAILED } else { exit::USAGE };
            Plan::Refuse(refusal(other), code)
        }
    }
}

fn plan_verb(verb: &str, given: &[String]) -> Plan {
    // `add company -h` asks about `add`'s help exactly as much as `add -h` does — the
    // kind is not a flag position, so `-h` can legitimately sit one token further in.
    let is_help = |s: Option<&String>| matches!(s.map(String::as_str), Some("-h" | "--help"));
    if is_help(given.first()) || (verb == "add" && is_help(given.get(1))) {
        return Plan::VerbHelp(verb.to_string());
    }
    let built = match verb {
        "find" => build_find(given),
        "show" => build_show(given),
        "board" => build_board(given),
        "stages" => build_no_args(verb, given, json!({ "cmd": "stages" })),
        "due" => build_due(given),
        "status" => build_no_args(verb, given, json!({ "cmd": "status" })),
        "export" => build_export(given),
        "add" => build_add(given),
        "set" => build_set(given),
        "log" => build_log(given),
        "move" => build_move(given),
        "task" => build_task(given),
        "done" => build_done(given),
        "link" => build_link(given),
        "archive" => build_archive(given),
        "select" => build_select(given),
        "import" => build_import(given),
        "focus" => build_no_args(verb, given, json!({ "cmd": "focus" })),
        "close" => build_no_args(verb, given, json!({ "cmd": "close" })),
        _ => unreachable!("plan() only calls plan_verb for a verb in VERBS"),
    };
    match built {
        Ok(req) => Plan::Ask(verb.to_string(), req),
        Err((msg, code)) => Plan::Refuse(msg, code),
    }
}

fn build_no_args(verb: &str, given: &[String], req: Value) -> Result<Value, (String, i32)> {
    if let Some(extra) = given.first() {
        return Err((
            format!("{CLI}: `{verb}` takes no arguments, and `{extra}` was given — see `{CLI} -h`"),
            exit::USAGE,
        ));
    }
    Ok(req)
}

/// `crm find [<query>] [--kind …] [--sort …] [--page N] [--archived] [-n N]`.
///
/// A **present but empty** positional (`crm find ""`) clears the query; an **absent**
/// one omits the field entirely, which is what keeps it at whatever it already was —
/// `find`'s whole "an omitted option keeps its current value" rule turns on that
/// distinction, so it is made once, here, rather than by every caller.
fn build_find(given: &[String]) -> Result<Value, (String, i32)> {
    let grammar =
        [flag("--kind", true), flag("--sort", true), flag("--page", true), flag("--archived", false), flag("-n", true)];
    let t = tokenize("find", given, &grammar)?;
    if t.positionals.len() > 1 {
        return Err(wrong_count("find", &["<query>"], &t.positionals));
    }

    let mut req = json!({ "cmd": "find" });
    if let Some(q) = t.positionals.first() {
        req["query"] = json!(q);
    }
    if let Some(k) = t.flag("--kind") {
        req["kind"] = if k.eq_ignore_ascii_case("all") { Value::Null } else { json!(k) };
    }
    if let Some(s) = t.flag("--sort") {
        req["sort"] = json!(s);
    }
    if let Some(p) = t.flag("--page") {
        let n = parse_uint_arg("find", "--page", p)?;
        if n == 0 {
            return Err((format!("{CLI}: `find --page` counts from 1, not 0"), exit::USAGE));
        }
        req["page"] = json!(n - 1); // 1-based at the edge, 0-based on the wire
    }
    if t.has("--archived") {
        req["archived"] = json!(true);
    }
    // `-n` is a **local** display limit. It is read back out by `render`, from the
    // envelope this function returned — never sent to the core, and never anywhere
    // near `view.list.page_size`.
    if let Some(n) = t.flag("-n") {
        let n = parse_uint_arg("find", "-n", n)?;
        req["__limit"] = json!(n);
    }
    Ok(req)
}

/// `crm show <handle>`. Sent over the socket as `open`, not `show` — see the note on
/// [`crate::state::AppState::command`] on why `show` is a name clappkit's own relay would
/// answer itself.
fn build_show(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("show", given, &[])?;
    if t.positionals.len() != 1 {
        return Err(wrong_count("show", &["<handle>"], &t.positionals));
    }
    Ok(json!({ "cmd": "open", "handle": t.positionals[0] }))
}

/// `crm board [--stage <stage>]`.
fn build_board(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("board", given, &[flag("--stage", true)])?;
    if !t.positionals.is_empty() {
        return Err(wrong_count("board", &[], &t.positionals));
    }
    let mut req = json!({ "cmd": "board" });
    if let Some(s) = t.flag("--stage") {
        req["stage"] = json!(s);
    }
    Ok(req)
}

/// `crm due [--overdue | --today | --week]`. Which buckets to print is a **local**
/// rendering choice — the core answers with all three counts either way — so the flags
/// ride the envelope under a `__`-prefixed key `render` reads back, same as `find`'s `-n`.
fn build_due(given: &[String]) -> Result<Value, (String, i32)> {
    let grammar = [flag("--overdue", false), flag("--today", false), flag("--week", false)];
    let t = tokenize("due", given, &grammar)?;
    if !t.positionals.is_empty() {
        return Err(wrong_count("due", &[], &t.positionals));
    }
    let mut only: Vec<&str> = Vec::new();
    if t.has("--overdue") {
        only.push("overdue");
    }
    if t.has("--today") {
        only.push("today");
    }
    if t.has("--week") {
        only.push("week");
    }
    Ok(json!({ "cmd": "due", "__only": only }))
}

/// `crm export <kind> [--format csv|json] [--out <path>]`. `--format`/`--out` never reach
/// the core — they say how *this CLI process* renders the answer to disk, which is a
/// question the core, with no filesystem of its own, cannot be asked.
fn build_export(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("export", given, &[flag("--format", true), flag("--out", true)])?;
    if t.positionals.len() != 1 {
        return Err(wrong_count("export", &["<kind>"], &t.positionals));
    }
    let plural = &t.positionals[0];
    let kind = plural_kind(plural)
        .ok_or_else(|| (format!("{CLI}: `export` wants companies, contacts or deals, not `{plural}`"), exit::USAGE))?;
    let format = match t.flag("--format") {
        Some("csv") | None => "csv",
        Some("json") => "json",
        Some(other) => {
            return Err((format!("{CLI}: `export --format` wants csv or json, not `{other}`"), exit::USAGE))
        }
    };
    let mut req = json!({ "cmd": "export", "kind": kind.word(), "__format": format, "__kindPlural": plural });
    if let Some(out) = t.flag("--out") {
        req["__out"] = json!(out);
    }
    Ok(req)
}

/// `crm add <company|contact|deal> <name> [flags]` — really three verbs sharing one
/// name, each with its own flags, so it is parsed on its own rather than through the
/// shared single-grammar path every other verb uses.
fn build_add(given: &[String]) -> Result<Value, (String, i32)> {
    let Some(kind_word) = given.first() else {
        return Err((
            format!("{CLI}: `add` needs a kind — company, contact or deal — see `{CLI} add -h`"),
            exit::USAGE,
        ));
    };
    let kind = Kind::parse(kind_word).ok_or_else(|| {
        (format!("{CLI}: `add` wants company, contact or deal, not `{kind_word}` — see `{CLI} add -h`"), exit::USAGE)
    })?;
    let rest = &given[1..];

    let (name_word, grammar): (&str, Vec<Flag>) = match kind {
        Kind::Company => ("<name>", vec![flag("--domain", true), repeatable_flag("--tag")]),
        Kind::Contact => {
            ("<name>", vec![flag("--company", true), flag("--email", true), flag("--phone", true), flag("--title", true)])
        }
        Kind::Deal => (
            "<title>",
            vec![flag("--company", true), flag("--value", true), flag("--currency", true), flag("--stage", true)],
        ),
    };
    let t = tokenize(&format!("add {kind_word}"), rest, &grammar)?;
    if t.positionals.len() != 1 {
        let verb = format!("add {}", kind.word());
        return Err(wrong_count(&verb, &[name_word], &t.positionals));
    }

    let mut fields = Map::new();
    match kind {
        Kind::Company => {
            if let Some(d) = t.flag("--domain") {
                fields.insert("domain".into(), json!(d));
            }
            let tags = t.all("--tag");
            if !tags.is_empty() {
                fields.insert("tags".into(), json!(tags));
            }
        }
        Kind::Contact => {
            for (key, name) in [("company", "--company"), ("email", "--email"), ("phone", "--phone"), ("title", "--title")]
            {
                if let Some(v) = t.flag(name) {
                    fields.insert(key.into(), json!(v));
                }
            }
        }
        Kind::Deal => {
            if let Some(v) = t.flag("--company") {
                fields.insert("company".into(), json!(v));
            }
            if let Some(v) = t.flag("--value") {
                fields.insert("value".into(), json!(v));
            }
            if let Some(v) = t.flag("--currency") {
                fields.insert("currency".into(), json!(v));
            }
            if let Some(v) = t.flag("--stage") {
                fields.insert("stage".into(), json!(v));
            }
        }
    }
    Ok(json!({ "cmd": "add", "kind": kind.word(), "name": t.positionals[0], "fields": Value::Object(fields) }))
}

/// `crm set <handle> <field> <value>`.
fn build_set(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("set", given, &[])?;
    if t.positionals.len() != 3 {
        return Err(wrong_count("set", &["<handle>", "<field>", "<value>"], &t.positionals));
    }
    Ok(json!({ "cmd": "set", "handle": t.positionals[0], "field": t.positionals[1], "value": t.positionals[2] }))
}

/// `crm log <call|email|meeting|note> <handle> <body> [--at <date>]`.
fn build_log(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("log", given, &[flag("--at", true)])?;
    if t.positionals.len() != 3 {
        return Err(wrong_count("log", &["<kind>", "<handle>", "<body>"], &t.positionals));
    }
    let kind_word = &t.positionals[0];
    if ActivityKind::parse(kind_word).is_none() {
        let all: Vec<&str> = ActivityKind::ALL.iter().map(|k| k.word()).collect();
        return Err((
            format!("{CLI}: `log` wants one of {} for its kind, not `{kind_word}`", all.join(", ")),
            exit::USAGE,
        ));
    }
    let mut req = json!({ "cmd": "log", "kind": kind_word, "handle": t.positionals[1], "body": t.positionals[2] });
    if let Some(at) = t.flag("--at") {
        req["at"] = json!(parse_date_arg("log", "--at", at)?);
    }
    Ok(req)
}

/// `crm move <handle> <stage | won | lost>`.
fn build_move(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("move", given, &[])?;
    if t.positionals.len() != 2 {
        return Err(wrong_count("move", &["<handle>", "<stage|won|lost>"], &t.positionals));
    }
    Ok(json!({ "cmd": "move", "handle": t.positionals[0], "to": t.positionals[1] }))
}

/// `crm task <handle> <what> --due <date>`. `--due` has no brackets in the grammar: it is
/// required, and a missing one is a missing-argument shape error, exit 2, decided here.
fn build_task(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("task", given, &[flag("--due", true)])?;
    if t.positionals.len() != 2 {
        return Err(wrong_count("task", &["<handle>", "<what>"], &t.positionals));
    }
    let Some(due) = t.flag("--due") else {
        return Err((format!("{CLI}: `task` needs `--due <date>` — `{}`", usage("task")), exit::USAGE));
    };
    let due = parse_date_arg("task", "--due", due)?;
    Ok(json!({ "cmd": "task", "handle": t.positionals[0], "what": t.positionals[1], "due": due }))
}

/// `crm done <task-handle>`.
fn build_done(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("done", given, &[])?;
    if t.positionals.len() != 1 {
        return Err(wrong_count("done", &["<task-handle>"], &t.positionals));
    }
    Ok(json!({ "cmd": "done", "handle": t.positionals[0] }))
}

/// `crm link <handle> <handle>`. Which of the two is the deal is the core's to decide —
/// it is the one that already has both records to check.
fn build_link(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("link", given, &[])?;
    if t.positionals.len() != 2 {
        return Err(wrong_count("link", &["<handle>", "<handle>"], &t.positionals));
    }
    Ok(json!({ "cmd": "link", "handle": t.positionals[0], "toHandle": t.positionals[1] }))
}

/// `crm archive <handle> [--restore]`.
fn build_archive(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("archive", given, &[flag("--restore", false)])?;
    if t.positionals.len() != 1 {
        return Err(wrong_count("archive", &["<handle>"], &t.positionals));
    }
    let mut req = json!({ "cmd": "archive", "handle": t.positionals[0] });
    if t.has("--restore") {
        req["restore"] = json!(true);
    }
    Ok(req)
}

/// `crm select <n>`. The number is the one printed beside a candidate — a shape question,
/// so it is checked here, not left for the core to explain what a non-number means.
fn build_select(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("select", given, &[])?;
    if t.positionals.len() != 1 {
        return Err(wrong_count("select", &["<n>"], &t.positionals));
    }
    let n = parse_uint_arg("select", "<n>", &t.positionals[0])?;
    Ok(json!({ "cmd": "select", "n": n }))
}

/// `crm import <path> [--kind companies|contacts|deals]`. The file is read **here**, in
/// the CLI process, because it lives in the agent's own working directory — the app has
/// no reason to know where that is, and no business reading outside its own data
/// directory. What reaches the core is only the already-parsed rows.
fn build_import(given: &[String]) -> Result<Value, (String, i32)> {
    let t = tokenize("import", given, &[flag("--kind", true)])?;
    if t.positionals.len() != 1 {
        return Err(wrong_count("import", &["<path>"], &t.positionals));
    }
    let path = &t.positionals[0];
    let content = std::fs::read_to_string(path)
        .map_err(|e| (format!("{CLI}: cannot read {path}: {e}"), exit::USAGE))?;

    let kind_hint = match t.flag("--kind") {
        Some(k) => Some(
            plural_kind(k).ok_or_else(|| (format!("{CLI}: `import --kind` wants companies, contacts or deals, not `{k}`"), exit::USAGE))?,
        ),
        None => None,
    };

    let is_vcard = path.to_ascii_lowercase().ends_with(".vcf") || content.trim_start().starts_with("BEGIN:VCARD");
    if is_vcard {
        if let Some(k) = kind_hint {
            if k != Kind::Contact {
                return Err((format!("{CLI}: a vCard file imports contacts only, not {}", k.word()), exit::USAGE));
            }
        }
        return Ok(json!({ "cmd": "import", "kind": "contact", "rows": parse_vcard(&content) }));
    }

    let (kind, rows) = parse_csv_import(&content, kind_hint)
        .map_err(|e| (format!("{CLI}: {path}: {e}"), exit::USAGE))?;
    Ok(json!({ "cmd": "import", "kind": kind.word(), "rows": rows }))
}

fn plural_kind(word: &str) -> Option<Kind> {
    match word {
        "companies" => Some(Kind::Company),
        "contacts" => Some(Kind::Contact),
        "deals" => Some(Kind::Deal),
        _ => None,
    }
}

// MARK: - Running

/// The agent's CLI. Never returns: every path prints and exits, so the exit code is the
/// verb's answer.
pub async fn run(args: Vec<String>) -> ! {
    match plan(&args) {
        Plan::Manual => {
            print!("{}", manual());
            std::process::exit(exit::OK)
        }
        Plan::VerbHelp(verb) => {
            let about = VERBS.iter().find(|(name, _)| *name == verb).map(|(_, a)| *a).unwrap_or("");
            let mut out = String::new();
            for line in usage_lines(&verb) {
                out.push_str(&format!("usage: {CLI} {line}\n"));
            }
            out.push('\n');
            out.push_str(about);
            out.push('\n');
            if let Some(note) = verb_note(&verb) {
                out.push('\n');
                out.push_str(note);
            }
            print!("{out}");
            std::process::exit(exit::OK)
        }
        Plan::Ask(verb, req) => ask(&verb, req).await,
        Plan::Refuse(message, code) => {
            eprintln!("{message}");
            std::process::exit(code)
        }
    }
}

/// Send one envelope to the running app and print what comes back.
async fn ask(verb: &str, mut req: Value) -> ! {
    // CLI-local directives — how *this* process should render or write the answer.
    // Never sent over the wire; stripped here and read back after the round trip.
    let limit = take_u64(&mut req, "__limit");
    let only = take_str_array(&mut req, "__only");
    let out_path = take_string(&mut req, "__out");
    let format = take_string(&mut req, "__format").unwrap_or_else(|| "csv".to_string());
    let kind_plural = take_string(&mut req, "__kindPlural");

    // Clatch injects `CLATCH_AGENT_ID` into the calling agent's shell, so the app knows
    // who asked. Absent means the person ran it themselves. `clappkit::app::spawn_ipc`
    // reads it back out of `agent` and hands it to the core.
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
            match verb {
                "export" => match finish_export(&resp, kind_plural.as_deref().unwrap_or("records"), out_path.as_deref(), &format) {
                    Ok(msg) => {
                        print!("{msg}");
                        std::process::exit(exit::OK)
                    }
                    Err(e) => {
                        eprintln!("{CLI}: {e}");
                        std::process::exit(exit::FAILED)
                    }
                },
                "find" => {
                    print!("{}", find_lines(&resp, limit));
                    std::process::exit(exit::OK)
                }
                "due" => {
                    print!("{}", due_lines(&resp, &only));
                    std::process::exit(exit::OK)
                }
                _ => {
                    print!("{}", render(verb, &req, &resp));
                    std::process::exit(exit::OK)
                }
            }
        }
        Err(e) => {
            eprintln!("{}", transport_error(&e.to_string()));
            std::process::exit(exit::FAILED)
        }
    }
}

fn take_string(req: &mut Value, key: &str) -> Option<String> {
    req.as_object_mut().and_then(|m| m.remove(key)).and_then(|v| v.as_str().map(str::to_string))
}

fn take_u64(req: &mut Value, key: &str) -> Option<u64> {
    req.as_object_mut().and_then(|m| m.remove(key)).and_then(|v| v.as_u64())
}

fn take_str_array(req: &mut Value, key: &str) -> Vec<String> {
    req.as_object_mut()
        .and_then(|m| m.remove(key))
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

// MARK: - Rendering an answer

/// What the agent reads when a verb succeeds and needed no special local handling.
/// `find`, `due` and `export` are handled directly in [`ask`], because they need
/// something beyond the response alone (a local limit, a local filter, a file write).
///
/// Takes `req` as well as `resp` because two confirmations cannot be built from the
/// response alone: [`AppState::move_deal`](crate::state::AppState::move_deal) and
/// `archive`/`restore` do **not** change `focus` — moving or archiving something from
/// the CLI must not silently redirect the shared window to it — so there is no row in
/// the response naming what just happened. What was *asked for* still is, in `req`.
fn render(verb: &str, req: &Value, resp: &Value) -> String {
    if resp.get("answer").and_then(Value::as_str) == Some("ambiguous") {
        return ambiguous_lines(resp);
    }
    match verb {
        "status" => status_lines(resp),
        "show" => show_lines(resp),
        "board" => board_lines(resp),
        "stages" => stages_lines(resp),
        "add" => confirm_add(resp),
        "set" => "updated\n".to_string(),
        "log" => "logged\n".to_string(),
        "move" => confirm_move(req),
        "task" => "task set\n".to_string(),
        "done" => "done\n".to_string(),
        "link" => "linked\n".to_string(),
        "archive" => confirm_archive(req),
        "select" => confirm_select(resp),
        "import" => import_lines(resp),
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

    // **The shared list is state `status` was already promising to report** — "what both
    // surfaces are looking at" — and it did not, which is how a sticky `--kind` filter
    // went unnoticed: nothing here said one was in force. `query`/`kind` default to "none
    // set" / "all" only when the key is genuinely absent, never when it holds an actual
    // empty string or null the core sent on purpose.
    let list = snap.get("list").cloned().unwrap_or(Value::Null);
    let query = match list.get("query").and_then(Value::as_str) {
        Some(q) if !q.is_empty() => format!("“{q}”"),
        _ => "(none set)".to_string(),
    };
    let kind = list.get("kind").and_then(Value::as_str).unwrap_or("all");
    let sort = list.get("sort").and_then(Value::as_str).unwrap_or("updated");
    let page = list.get("page").and_then(Value::as_u64).unwrap_or(0);
    let total = list.get("total").and_then(Value::as_u64).unwrap_or(0);
    out.push_str(&format!(
        "  list: query {query}   kind {kind}   sort {sort}   page {} ({total} total)\n",
        page + 1
    ));

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

/// `show` as a person or an agent reads it: the record, then its fields — the very list
/// the window's record panel draws, from the same snapshot, in the same order.
fn show_lines(resp: &Value) -> String {
    let Some(focused) = resp.get("focused") else {
        return "nothing is open\n".to_string();
    };
    let row = &focused["row"];
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    // The handle, never the id — the same rule as `status`, and no fallback to the id.
    let mut header = format!("{} {} — {}", text(row, "kind"), text(row, "handle"), text(row, "label"));
    if row.get("archived").and_then(Value::as_bool) == Some(true) {
        header.push_str("  (archived)");
    }
    let mut out = header;
    out.push('\n');

    let fields = focused.get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let width = fields.iter().map(|f| text(f, "label").chars().count()).max().unwrap_or(0);
    for field in &fields {
        let label = text(field, "label");
        let pad = width - label.chars().count();
        out.push_str(&format!("  {label}{}  {}\n", " ".repeat(pad), text(field, "value")));
    }
    out.push_str(&next_steps_lines(focused));
    out
}

/// The record's **open** next steps, each led by its handle — the one thing `crm done`
/// accepts. Until a QA round found `crm done`'s own refusal pointing here, no read verb
/// surfaced a task's handle at all, and the only way to complete one was to guess how it
/// had been slugged. Done ones are left to the window: this is the list of what is still
/// to do.
fn next_steps_lines(focused: &Value) -> String {
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let open: Vec<&Value> = focused
        .get("tasks")
        .and_then(Value::as_array)
        .map(|ts| ts.iter().filter(|t| t.get("doneAt").is_none_or(Value::is_null)).collect())
        .unwrap_or_default();
    if open.is_empty() {
        return String::new();
    }
    let width = open.iter().map(|t| text(t, "handle").chars().count()).max().unwrap_or(0);
    let mut out = format!("  Next steps  (finish one with `{CLI} done <handle>`)\n");
    for t in open {
        let handle = text(t, "handle");
        out.push_str(&format!("    {handle:<width$}  due {}  {}\n", text(t, "due"), text(t, "what")));
    }
    out
}

/// If the handle matched more than one record, the core has parked a question instead —
/// shared by every write verb that resolves a handle, since any of them can trip it, not
/// only `show`. This is exit **0**: ambiguity is a question, not a failure.
///
/// **`crm select N` is *the* resolver** (`m2-cli.md`), and it used to go unmentioned here
/// in favour of `crm show <handle>` alone — a QA finding, and a real gap for a *write*
/// verb's ambiguity: `crm show <handle>` only opens a record, so on `crm log note acme
/// "…"` it would silently skip the note that answering was supposed to finish. `select` is
/// what completes that write (`Pending::resuming`, computed by the core); `show` is offered
/// only alongside it, as a second way to look before choosing, never as an equal
/// alternative for a resumed write.
fn ambiguous_lines(resp: &Value) -> String {
    let candidates = resp.pointer("/pending/candidates").and_then(Value::as_array).cloned().unwrap_or_default();
    let resuming = resp.pointer("/pending/resuming").and_then(Value::as_bool).unwrap_or(false);
    let mut out = if resuming {
        String::from("More than one record matches — pick the one you meant to finish this:\n")
    } else {
        String::from("More than one record matches — pick the one you meant:\n")
    };
    let selects: Vec<String> = (1..=candidates.len()).map(|n| format!("{CLI} select {n}")).collect();
    let width = selects.iter().map(String::len).max().unwrap_or(0);
    for (select, c) in selects.iter().zip(&candidates) {
        let label = c.get("label").and_then(Value::as_str).unwrap_or("");
        let kind = c.get("kind").and_then(Value::as_str).unwrap_or("record");
        out.push_str(&format!("  {select:<width$}  {label} ({kind})\n"));
    }
    if resuming {
        out.push_str("Picking one finishes it. `crm show <handle>` only opens a record — it will not.\n");
    } else {
        out.push_str("Or open one directly, e.g. ");
        let first = candidates.first().and_then(|c| c.get("handle")).and_then(Value::as_str).unwrap_or("<handle>");
        out.push_str(&format!("`{CLI} show {first}`.\n"));
    }
    out.push_str("The window is showing the same question; picking there works too.\n");
    out
}

/// `board`: every column, its deals and its totals — the stage word first, because that
/// is what `crm move` and `crm board --stage` both take.
fn board_lines(resp: &Value) -> String {
    let columns = resp.pointer("/board/columns").and_then(Value::as_array).cloned().unwrap_or_default();
    let width = columns.iter().filter_map(|c| c.get("key").and_then(Value::as_str)).map(str::len).max().unwrap_or(0);
    let mut out = String::new();
    for c in &columns {
        let key = c.get("key").and_then(Value::as_str).unwrap_or("?");
        let count = c.get("count").and_then(Value::as_u64).unwrap_or(0);
        let noun = if count == 1 { "deal" } else { "deals" };
        let totals: Vec<String> = c
            .get("totals")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t.get("formatted").and_then(Value::as_str).map(str::to_string))
            .collect();
        let mut line = format!("  {key:<width$}  {count} {noun}");
        if !totals.is_empty() {
            line.push_str(&format!("   {}", totals.join(", ")));
        }
        line.push('\n');
        out.push_str(&line);
    }
    if out.is_empty() {
        out.push_str("  no stages\n");
    }
    out
}

fn stages_lines(resp: &Value) -> String {
    let stages = resp.pointer("/pipeline/stages").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = String::new();
    for s in &stages {
        if let Some(word) = s.as_str() {
            out.push_str(word);
            out.push('\n');
        }
    }
    out
}

/// `due`, filtered to whichever buckets were asked for — all three when none were.
fn due_lines(resp: &Value, only: &[String]) -> String {
    let n = |key: &str| resp.pointer(&format!("/due/{key}")).and_then(Value::as_u64).unwrap_or(0);
    let buckets: Vec<&str> = if only.is_empty() {
        vec!["overdue", "today", "week"]
    } else {
        only.iter().map(String::as_str).collect()
    };
    let tasks = resp.get("dueTasks").and_then(Value::as_array).cloned().unwrap_or_default();
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let mut out = String::new();
    for b in buckets {
        out.push_str(&format!("{b} {}\n", n(b)));
        // The count says how many; these say *which*, by the handle `crm done` takes.
        let in_bucket: Vec<&Value> = tasks.iter().filter(|t| text(t, "bucket") == b).collect();
        let width = in_bucket.iter().map(|t| text(t, "handle").chars().count()).max().unwrap_or(0);
        for t in in_bucket {
            let handle = text(t, "handle");
            let on: Vec<String> =
                t.get("on").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            let mut line = format!("  {handle:<width$}  {}  {}", text(t, "due"), text(t, "what"));
            if !on.is_empty() {
                line.push_str(&format!("  (on {})", on.join(", ")));
            }
            line.push('\n');
            out.push_str(&line);
        }
    }
    out
}

/// **`--kind` is sticky.** It rides the shared `view.list.kind` exactly like the query,
/// the sort and the page do — omitted, it keeps whatever it already was. That is correct
/// per `m2-cli.md`, and it was also the QA finding: `crm find --kind contact` narrows the
/// list *for every later `find`, from either surface, until something clears it* — and
/// nothing said so. `crm find acme` after that silently searched contacts alone, and "no
/// results" gave no reason to suspect a filter at all. Every place `find`'s answer is
/// printed now names the active filter and how to drop it, on both the empty and the
/// ordinary path — a milder version of the same silence ("2 of 2" while filtered) is still
/// the bug, just quieter.
fn active_filter_note(resp: &Value) -> Option<String> {
    let kind = resp.pointer("/list/kind").and_then(Value::as_str)?;
    Some(format!("filtered to {kind} — `crm find --kind all` searches everything"))
}

/// `find`: the shared page, trimmed to `-n` rows for **this terminal's own printed
/// output** — the page itself, and everyone else's view of it, is untouched.
fn find_lines(resp: &Value, limit: Option<u64>) -> String {
    let total = resp.pointer("/list/total").and_then(Value::as_u64).unwrap_or(0);
    let page = resp.pointer("/list/page").and_then(Value::as_u64).unwrap_or(0);
    let mut rows = resp.pointer("/list/rows").and_then(Value::as_array).cloned().unwrap_or_default();
    if let Some(n) = limit {
        rows.truncate(n as usize);
    }
    let filter_note = active_filter_note(resp);
    if rows.is_empty() {
        let mut out = format!("no results (page {} of {total} total)\n", page + 1);
        if let Some(note) = &filter_note {
            out.push_str(&format!("  {note}\n"));
        }
        return out;
    }
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let width = rows.iter().map(|r| text(r, "handle").len()).max().unwrap_or(0);
    let mut out = String::new();
    for r in &rows {
        let handle = text(r, "handle");
        let label = text(r, "label");
        let kind = text(r, "kind");
        let value = r.get("value").and_then(|v| v.get("formatted")).and_then(Value::as_str);
        let mut line = format!("  {handle:<width$}  {label} ({kind})");
        if let Some(v) = value {
            line.push_str(&format!("  {v}"));
        }
        line.push('\n');
        out.push_str(&line);
    }
    out.push_str(&format!("{} of {total} (page {})", rows.len(), page + 1));
    if let Some(note) = &filter_note {
        out.push_str(&format!(" — {note}"));
    }
    out.push('\n');
    out
}

fn confirm_add(resp: &Value) -> String {
    let row = resp.pointer("/focused/row");
    match row {
        Some(row) => {
            let kind = row.get("kind").and_then(Value::as_str).unwrap_or("record");
            let handle = row.get("handle").and_then(Value::as_str).unwrap_or("?");
            format!("added {kind} {handle}\n")
        }
        None => "added\n".to_string(),
    }
}

/// `req` carries exactly what was typed — `to` is either a stage word or `won`/`lost` —
/// so the confirmation is just that, echoed back; the deal need not be (and is not)
/// the one in focus.
fn confirm_move(req: &Value) -> String {
    let handle = req.get("handle").and_then(Value::as_str).unwrap_or("?");
    let to = req.get("to").and_then(Value::as_str).unwrap_or("?");
    format!("moved {handle} to {to}\n")
}

fn confirm_archive(req: &Value) -> String {
    let restored = req.get("restore").and_then(Value::as_bool).unwrap_or(false);
    if restored {
        "restored\n".to_string()
    } else {
        "archived\n".to_string()
    }
}

fn confirm_select(resp: &Value) -> String {
    // **A `select` that resumed a write must say which write.** `answer` is "changed" for
    // that and for a plain select alike, so it cannot tell them apart; the core says what
    // it resumed, and this reads it. Without it every resumed `log`/`set`/`task`/… answered
    // "opened company acme-corp" — indistinguishable from having only looked — and an
    // agent that cannot tell its note landed writes it again.
    if let Some(resumed) = resp.get("resumed") {
        return resumed_lines(resp, resumed);
    }
    let Some(row) = resp.pointer("/focused/row") else {
        return "selected\n".to_string();
    };
    let kind = row.get("kind").and_then(Value::as_str).unwrap_or("record");
    let handle = row.get("handle").and_then(Value::as_str).unwrap_or("?");
    format!("opened {kind} {handle}\n")
}

/// The confirmation for a write that `select` completed: the very sentence the verb would
/// have printed had it not been ambiguous — built by [`render`] from the request the core
/// kept, so the two cannot say different things — plus the record the choice landed on,
/// since which of three "acme"s got the note is exactly what somebody picking from a list
/// needs to read back. `move` already names its deal.
fn resumed_lines(resp: &Value, resumed: &Value) -> String {
    let cmd = resumed.get("cmd").and_then(Value::as_str).unwrap_or("");
    let handle = resumed.get("handle").and_then(Value::as_str).unwrap_or("?");
    let req = resumed.get("req").cloned().unwrap_or(Value::Null);
    let base = render(cmd, &req, resp);
    let base = base.trim_end();
    if cmd == "move" {
        format!("{base}\n")
    } else {
        format!("{base} — {handle}\n")
    }
}

fn import_lines(resp: &Value) -> String {
    let created = resp.pointer("/import/created").and_then(Value::as_u64).unwrap_or(0);
    let skipped = resp.pointer("/import/skipped").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = format!("imported {created}\n");
    if !skipped.is_empty() {
        out.push_str(&format!("skipped {}:\n", skipped.len()));
        for s in &skipped {
            let reason = s.get("reason").and_then(Value::as_str).unwrap_or("unknown reason");
            out.push_str(&format!("  {reason}\n"));
        }
    }
    out
}

// MARK: - Export: rendering rows to CSV or JSON, and writing them

/// `resp["export"]` is an array of rows, each an array of `[column, value]` pairs — see
/// `AppState::export_rows`. Rendered here, in the CLI process, and written to the
/// agent's own working directory: the app has no reason to resolve a path that belongs
/// to whoever is running it.
fn finish_export(resp: &Value, kind_plural: &str, out_path: Option<&str>, format: &str) -> Result<String, String> {
    let rows = resp.get("export").and_then(Value::as_array).cloned().unwrap_or_default();
    let (text, ext) = if format == "json" { (render_export_json(&rows), "json") } else { (render_export_csv(&rows), "csv") };
    let path = out_path.map(str::to_string).unwrap_or_else(|| format!("{kind_plural}.{ext}"));
    std::fs::write(&path, text).map_err(|e| format!("cannot write {path}: {e}"))?;
    Ok(format!("wrote {} {kind_plural} to {path}\n", rows.len()))
}

fn render_export_csv(rows: &[Value]) -> String {
    let cell = |pair: &Value, i: usize| pair.get(i).and_then(Value::as_str).unwrap_or("").to_string();
    let mut out = String::new();
    if let Some(first) = rows.first().and_then(Value::as_array) {
        let headers: Vec<String> = first.iter().map(|pair| csv_escape(&cell(pair, 0))).collect();
        out.push_str(&headers.join(","));
        out.push('\n');
    }
    for row in rows {
        let Some(row) = row.as_array() else { continue };
        let cells: Vec<String> = row.iter().map(|pair| csv_escape(&cell(pair, 1))).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn render_export_json(rows: &[Value]) -> String {
    let objects: Vec<Value> = rows
        .iter()
        .filter_map(Value::as_array)
        .map(|row| {
            let map: Map<String, Value> = row
                .iter()
                .filter_map(|pair| {
                    let k = pair.get(0)?.as_str()?.to_string();
                    let v = pair.get(1)?.clone();
                    Some((k, v))
                })
                .collect();
            Value::Object(map)
        })
        .collect();
    serde_json::to_string_pretty(&Value::Array(objects)).unwrap_or_default() + "\n"
}

// MARK: - Import: reading rows back out of a CSV or a vCard file

/// A minimal RFC 4180 reader: quoted fields, doubled quotes inside them, `\r\n` or `\n`
/// line endings. Rows may have different lengths; nothing downstream assumes otherwise.
fn parse_csv(content: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() => in_quotes = true,
                ',' => row.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

fn parse_csv_import(content: &str, kind_hint: Option<Kind>) -> Result<(Kind, Value), String> {
    let rows = parse_csv(content);
    let Some(header) = rows.first() else {
        return Err("the file is empty".to_string());
    };
    let header: Vec<String> = header.iter().map(|h| h.trim().to_string()).collect();

    let kind = match kind_hint {
        Some(k) => k,
        None => sniff_kind(&header)
            .ok_or_else(|| "cannot tell what kind of records this file holds — pass --kind".to_string())?,
    };

    let mut out = Vec::new();
    for data_row in rows.iter().skip(1) {
        if data_row.iter().all(|c| c.trim().is_empty()) {
            continue; // a trailing blank line is not a record
        }
        let mut obj = Map::new();
        for (i, col) in header.iter().enumerate() {
            if let Some(v) = data_row.get(i) {
                if !v.trim().is_empty() {
                    obj.insert(col.clone(), json!(v));
                }
            }
        }
        out.push(Value::Object(obj));
    }
    Ok((kind, json!(out)))
}

/// `title` alone is not distinctive: a deal's export column and a contact's job title
/// share the word. Checked last, and only as a tiebreaker, so a contacts file that
/// happens to have a `title` column is never mistaken for a deal export — a deal's own
/// columns (`stage`, `status`, `value`) are unique to it and checked first.
fn sniff_kind(header: &[String]) -> Option<Kind> {
    let has = |c: &str| header.iter().any(|h| h.eq_ignore_ascii_case(c));
    if has("email") || has("phone") {
        Some(Kind::Contact)
    } else if has("stage") || has("status") || has("value") {
        Some(Kind::Deal)
    } else if has("domain") || has("notes") {
        Some(Kind::Company)
    } else if has("title") {
        Some(Kind::Contact)
    } else {
        None
    }
}

/// A small, deliberately partial vCard 3/4 reader: `BEGIN:VCARD` … `END:VCARD` blocks,
/// `FN`/`EMAIL`/`TEL`/`TITLE`/`ORG` — the fields an address-book export actually carries
/// for a person, and the only ones `crm add contact` has anywhere to put. `ORG` becomes
/// a company **name**, not a handle — resolved fuzzily by the core exactly like a typed
/// search, since a vCard has no notion of our handles at all.
fn parse_vcard(content: &str) -> Value {
    let mut cards = Vec::new();
    let mut current: Option<Map<String, Value>> = None;
    for raw_line in content.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.eq_ignore_ascii_case("BEGIN:VCARD") {
            current = Some(Map::new());
            continue;
        }
        if line.eq_ignore_ascii_case("END:VCARD") {
            if let Some(card) = current.take() {
                if card.contains_key("name") {
                    cards.push(Value::Object(card));
                }
            }
            continue;
        }
        let Some(card) = current.as_mut() else { continue };
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.split(';').next().unwrap_or(key).trim().to_ascii_uppercase();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "FN" => {
                card.insert("name".into(), json!(value));
            }
            "EMAIL" if !card.contains_key("email") => {
                card.insert("email".into(), json!(value));
            }
            "TEL" if !card.contains_key("phone") => {
                card.insert("phone".into(), json!(value));
            }
            "TITLE" => {
                card.insert("title".into(), json!(value));
            }
            "ORG" => {
                card.insert("company".into(), json!(value.split(';').next().unwrap_or(value)));
            }
            _ => {}
        }
    }
    json!(cards)
}

// MARK: - The manual, and the refusals that point at it

/// clappkit's connect errors are already agent-actionable; the one thing it cannot know
/// is our app id, so it writes `<id>` as a placeholder. Fill it in — an instruction the
/// agent can paste is worth more than a correct sentence it has to finish itself.
fn transport_error(msg: &str) -> String {
    msg.replace("<id>", APP_ID)
}

/// The refusal for a verb that is declared but not yet built, and for one that is neither.
/// M2 ships every declared verb, so the first branch is now unreachable in practice — kept
/// because a future milestone may add a new declared-but-unbuilt one, and the distinction
/// still matters: a typo is a usage error, a not-yet verb is a valid request declined.
fn refusal(verb: &str) -> String {
    if declared().iter().any(|(name, _)| name == verb) {
        format!("{CLI}: `{verb}` is declared in clatch.json but is not implemented — `{CLI} -h` lists what this build answers")
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
    out.push_str(&format!("usage:\n  {CLI} <verb> [args]\n  {CLI} <verb> -h      what one verb takes\n\nverbs:\n"));
    // One space, not two, between the verb and its `about`: with nineteen verbs the
    // widest name (`archive`) plus the longest `about` no longer fits an 80-column
    // terminal at the old two-space gap.
    let width = VERBS.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    for (name, about) in VERBS {
        out.push_str(&format!("  {name:<width$} {about}\n"));
    }

    out.push_str(&pipeline_section());

    out.push_str(
        "\nnext steps:\n\
         \x20 `crm task <handle> <what> --due <date>` gives a record a next step, with a\n\
         \x20 handle of its own. `crm show <handle>` and `crm due` list the open ones with\n\
         \x20 their handles; `crm done <task-handle>` completes one.\n",
    );

    out.push_str("\nexit codes:\n");
    out.push_str("  0  the app answered\n");
    out.push_str("  1  the app is not running, or it refused a valid request\n");
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
#[path = "cli_tests.rs"]
mod tests;
