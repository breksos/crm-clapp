//! Tests for the agent's hands.
//!
//! Two things this file exists to prove beyond the individual verbs: **no `id` ever
//! reaches stdout**, checked against real core output wherever possible, and **every
//! declared verb is implemented and described identically** in the manual and the
//! manifest. The rest pins each verb's shape — its grammar, its exit codes, and the one
//! render bug a live run against the real app actually caught (`move`/`archive` reading
//! the wrong row back out of a response that never changes `focus`).

use super::*;
use crate::model::Ulid;

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// A ULID, exactly as the core mints one — real fixtures, not readable strings that only
/// *look* like the shape the code meets.
fn an_id(seed: u8) -> String {
    Ulid::from_parts(1_788_861_600_000, [seed; 10]).to_string()
}

/// Any run of Crockford base32 long enough to be a ULID, anywhere in a string.
fn first_ulid_in(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric()).find(|word| Ulid::parse(word).is_some()).map(str::to_string)
}

// MARK: - The manifest and the manual agree, for all nineteen

#[test]
fn every_verb_the_manual_offers_is_declared_in_the_manifest() {
    let declared = declared();
    assert!(!declared.is_empty(), "the embedded manifest must parse");
    for (name, _) in VERBS {
        assert!(declared.iter().any(|(d, _)| d == name), "`{name}` is in the manual but not in clatch.json");
    }
}

#[test]
fn every_declared_verb_is_in_the_manual_too() {
    let declared = declared();
    for (name, _) in &declared {
        assert!(VERBS.iter().any(|(v, _)| v == name), "`{name}` is declared but M2 never built it");
    }
    assert_eq!(declared.len(), VERBS.len(), "a verb exists on only one side of the contract");
}

#[test]
fn the_manual_and_the_manifest_describe_each_verb_identically() {
    let declared = declared();
    for (name, about) in VERBS {
        let (_, manifest_about) = declared.iter().find(|(d, _)| d == name).unwrap();
        assert_eq!(manifest_about, about, "`{name}` is described two different ways");
    }
}

#[test]
fn the_manual_names_every_verb_and_fits_eighty_columns() {
    let m = manual();
    for (name, about) in VERBS {
        assert!(m.contains(name), "`{name}` is missing from the manual");
        assert!(m.contains(about), "`{name}`'s description is missing from the manual");
    }
    assert!(m.contains(APP_ID));
    for line in m.lines() {
        assert!(line.chars().count() <= 80, "{} chars: {line}", line.chars().count());
    }
}

#[test]
fn every_exit_code_the_manual_documents_is_a_real_one() {
    let m = manual();
    for code in [exit::OK, exit::FAILED, exit::USAGE] {
        assert!(m.contains(&format!("  {code}  ")), "the manual never names {code}");
    }
}

/// A verb that never existed is a usage error; the manifest's own `about` text can never
/// collide with "not a verb" wording, so this also guards against a stray substring match.
#[test]
fn an_unknown_verb_is_a_usage_error() {
    let (msg, code) = refusal_for(&["teleport"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("not a verb"), "{msg}");
    assert!(msg.contains("crm -h"), "{msg}");
}

#[test]
fn no_verb_at_all_is_a_usage_error() {
    assert_eq!(refusal_for(&[]).1, exit::USAGE);
    assert_eq!(refusal_for(&[""]).1, exit::USAGE);
}

fn refusal_for(v: &[&str]) -> (String, i32) {
    match plan(&argv(v)) {
        Plan::Refuse(msg, code) => (msg, code),
        other => panic!("{v:?} was not refused: {other:?}"),
    }
}

fn ask_of(v: &[&str]) -> Value {
    match plan(&argv(v)) {
        Plan::Ask(_, req) => req,
        other => panic!("{v:?} was not accepted: {other:?}"),
    }
}

// MARK: - The manual, and each verb's own -h

#[test]
fn every_verb_answers_its_own_help_including_adds_three_forms() {
    for (verb, _) in VERBS {
        assert_eq!(plan(&argv(&[verb, "-h"])), Plan::VerbHelp(verb.to_string()));
        assert_eq!(plan(&argv(&[verb, "--help"])), Plan::VerbHelp(verb.to_string()));
    }
    assert_eq!(plan(&argv(&["add", "company", "-h"])), Plan::VerbHelp("add".to_string()));
    assert_eq!(usage_lines("add").len(), 3, "add is three verbs wearing one name");
}

// MARK: - status / show / focus / close — unchanged from M0/round-3

#[test]
fn status_and_window_verbs_take_no_arguments() {
    assert_eq!(ask_of(&["status"]), json!({ "cmd": "status" }));
    assert_eq!(ask_of(&["focus"]), json!({ "cmd": "focus" }));
    assert_eq!(ask_of(&["close"]), json!({ "cmd": "close" }));
    for line in [vec!["status", "--json"], vec!["close", "--please"], vec!["focus", "now"]] {
        let (msg, code) = refusal_for(&line);
        assert_eq!(code, exit::USAGE, "{line:?} → {msg}");
        assert!(msg.contains(line[0]) && msg.contains(line[1]) && msg.contains("crm -h"), "{msg}");
    }
}

#[test]
fn show_sends_the_typed_handle_as_open_never_as_show() {
    // `open`, not `show`: clappkit's IPC relay answers `show` itself, as `focus`, before
    // the core ever sees a request — a domain verb named `show` on the wire is silently
    // eaten. Checked against the relay's own classifier below.
    assert_eq!(ask_of(&["show", "acme"]), json!({ "cmd": "open", "handle": "acme" }));
    let (msg, code) = refusal_for(&["show"]);
    assert_eq!(code, exit::USAGE);
    assert!(msg.contains("<handle>"), "{msg}");
    let (msg, code) = refusal_for(&["show", "a", "b"]);
    assert_eq!(code, exit::USAGE);
    assert!(msg.contains('b'), "{msg}");
}

/// **The collision that shipped once**, now checked for every verb this build sends —
/// not only `show`. Every request this file can build must dodge clappkit's own
/// window-verb names, or the app silently answers the wrong thing.
#[test]
fn no_request_this_build_sends_uses_a_name_the_ipc_relay_answers_itself() {
    use clappkit::window::{classify_req, WindowPolicy};
    let policy = WindowPolicy::default();
    let samples: &[(&str, &[&str])] = &[
        ("find", &[]),
        ("show", &["acme"]),
        ("board", &[]),
        ("stages", &[]),
        ("due", &[]),
        ("status", &[]),
        ("export", &["companies"]),
        ("add", &["company", "Acme"]),
        ("set", &["acme", "domain", "acme.com"]),
        ("log", &["call", "acme", "hi"]),
        ("move", &["acme", "won"]),
        ("task", &["acme", "call back", "--due", "2026-09-30"]),
        ("done", &["acme"]),
        ("link", &["acme", "hooli"]),
        ("archive", &["acme"]),
        ("select", &["1"]),
        ("focus", &[]),
        ("close", &[]),
    ];
    for (verb, args) in samples {
        let req = ask_of(&[std::iter::once(*verb).chain(args.iter().copied()).collect::<Vec<_>>()].concat());
        let caught = classify_req(&req, &policy).is_some();
        let is_window_verb = matches!(*verb, "focus" | "close");
        assert_eq!(caught, is_window_verb, "`crm {verb}` sends {req}, relay {}", if caught { "eats it" } else { "ignores it" });
    }
}

// MARK: - find

#[test]
fn find_omits_query_when_absent_and_sends_it_empty_when_given_as_empty() {
    assert_eq!(ask_of(&["find"]), json!({ "cmd": "find" }), "no positional at all — query stays whatever it was");
    assert_eq!(ask_of(&["find", ""]), json!({ "cmd": "find", "query": "" }), "an explicit empty string clears it");
    assert_eq!(ask_of(&["find", "acme"]), json!({ "cmd": "find", "query": "acme" }));
}

#[test]
fn find_kind_all_clears_the_filter_with_a_null() {
    assert_eq!(ask_of(&["find", "--kind", "deal"]), json!({ "cmd": "find", "kind": "deal" }));
    assert_eq!(ask_of(&["find", "--kind", "all"]), json!({ "cmd": "find", "kind": null }));
}

#[test]
fn find_page_is_one_based_at_the_edge_and_zero_based_on_the_wire() {
    assert_eq!(ask_of(&["find", "--page", "1"]), json!({ "cmd": "find", "page": 0 }));
    assert_eq!(ask_of(&["find", "--page", "3"]), json!({ "cmd": "find", "page": 2 }));
    let (msg, code) = refusal_for(&["find", "--page", "0"]);
    assert_eq!(code, exit::USAGE, "page 0 does not exist to a person reading 1-based pages");
    assert!(msg.contains("counts from 1"), "{msg}");
}

/// The whole rule, checked structurally: `-n` never appears as anything the core would
/// read, and specifically never under a key that could be mistaken for `pageSize`.
#[test]
fn find_dash_n_never_reaches_the_wire() {
    let req = ask_of(&["find", "acme", "-n", "3"]);
    // `ask()` strips every `__`-prefixed key before the request is sent; this fixture
    // checks the same envelope `ask()` would send by re-doing that strip here.
    let mut wire = req.clone();
    wire.as_object_mut().unwrap().retain(|k, _| !k.starts_with("__"));
    assert_eq!(wire, json!({ "cmd": "find", "query": "acme" }));
    assert!(req.get("n").is_none());
    assert!(req.get("pageSize").is_none());
    assert_eq!(req["__limit"], 3);
}

#[test]
fn find_archived_is_not_sticky_on_the_wire_either() {
    assert_eq!(ask_of(&["find", "--archived"]), json!({ "cmd": "find", "archived": true }));
    assert_eq!(ask_of(&["find"]).get("archived"), None, "omitted, not false — it applies to one search only");
}

#[test]
fn find_rejects_a_second_query_and_a_bad_sort_word_is_left_to_the_core() {
    let (msg, code) = refusal_for(&["find", "a", "b"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    // A sort word is core vocabulary, exactly like a stage word — the CLI passes it
    // through unvalidated and the core is the one that may refuse it, at exit 1.
    assert_eq!(ask_of(&["find", "--sort", "bogus"]), json!({ "cmd": "find", "sort": "bogus" }));
}

// MARK: - board / stages / due

#[test]
fn board_takes_an_optional_stage_the_core_validates() {
    assert_eq!(ask_of(&["board"]), json!({ "cmd": "board" }));
    assert_eq!(ask_of(&["board", "--stage", "proposal"]), json!({ "cmd": "board", "stage": "proposal" }));
    let (_, code) = refusal_for(&["board", "extra"]);
    assert_eq!(code, exit::USAGE);
}

#[test]
fn stages_lines_prints_the_pipeline_in_order() {
    let resp = shown(json!({ "pipeline": { "stages": ["lead", "qualified", "proposal", "negotiation"] } }));
    assert_eq!(stages_lines(&resp), "lead\nqualified\nproposal\nnegotiation\n");
}

#[test]
fn due_only_flags_are_local_and_filter_what_is_printed() {
    let req = ask_of(&["due", "--overdue"]);
    let mut wire = req.clone();
    wire.as_object_mut().unwrap().retain(|k, _| !k.starts_with("__"));
    assert_eq!(wire, json!({ "cmd": "due" }), "the core answers with all three either way");

    let resp = shown(json!({ "due": { "overdue": 2, "today": 1, "week": 5 } }));
    assert_eq!(due_lines(&resp, &[]), "overdue 2\ntoday 1\nweek 5\n");
    assert_eq!(due_lines(&resp, &["today".to_string()]), "today 1\n");
}

// MARK: - export

#[test]
fn export_builds_the_kind_word_and_carries_local_format_out_directives() {
    let req = ask_of(&["export", "companies"]);
    assert_eq!(req["cmd"], "export");
    assert_eq!(req["kind"], "company");
    assert_eq!(req["__format"], "csv");
    assert_eq!(req["__kindPlural"], "companies");
    assert!(req.get("__out").is_none());

    let req = ask_of(&["export", "deals", "--format", "json", "--out", "d.json"]);
    assert_eq!(req["kind"], "deal");
    assert_eq!(req["__format"], "json");
    assert_eq!(req["__out"], "d.json");

    let (msg, code) = refusal_for(&["export", "widgets"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    let (msg, code) = refusal_for(&["export", "deals", "--format", "xml"]);
    assert_eq!(code, exit::USAGE, "{msg}");
}

#[test]
fn csv_export_round_trips_through_the_import_reader() {
    let rows = json!([
        [["handle", "acme-corp"], ["name", "Acme, Inc \"The Best\""], ["domain", "acme.com"]],
        [["handle", "hooli"], ["name", "Hooli"], ["domain", ""]],
    ]);
    let csv = render_export_csv(rows.as_array().unwrap());
    assert!(csv.contains("\"Acme, Inc \"\"The Best\"\"\""), "a comma and a quote must be escaped: {csv}");

    let parsed = parse_csv(&csv);
    assert_eq!(parsed[0], vec!["handle", "name", "domain"]);
    assert_eq!(parsed[1], vec!["acme-corp", "Acme, Inc \"The Best\"", "acme.com"]);
    assert_eq!(parsed[2], vec!["hooli", "Hooli", ""]);
}

#[test]
fn json_export_is_one_object_per_row() {
    let rows = json!([[["handle", "acme-corp"], ["name", "Acme Corp"]]]);
    let text = render_export_json(rows.as_array().unwrap());
    let back: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(back, json!([{ "handle": "acme-corp", "name": "Acme Corp" }]));
}

/// **Round 1's guard, still true here.** `amount` being an integer means there is no
/// negative zero to produce — but `export`'s CSV cell is `Money::decimal()`, a second
/// formatter, and it gets its own check.
#[test]
fn an_empty_pipeline_s_zero_deal_value_exports_as_0_00_never_negative() {
    assert_eq!(crate::model::Money::new(0, "USD").decimal(), "0.00");
    assert!(!crate::model::Money::new(0, "USD").decimal().contains('-'));
}

// MARK: - add

#[test]
fn add_company_builds_fields_from_repeated_tags() {
    let req = ask_of(&["add", "company", "Acme Corp", "--domain", "acme.com", "--tag", "vip", "--tag", "b2b"]);
    assert_eq!(req["cmd"], "add");
    assert_eq!(req["kind"], "company");
    assert_eq!(req["name"], "Acme Corp");
    assert_eq!(req["fields"]["domain"], "acme.com");
    assert_eq!(req["fields"]["tags"], json!(["vip", "b2b"]));
}

#[test]
fn add_contact_and_deal_build_their_own_fields() {
    let req = ask_of(&["add", "contact", "Ada", "--company", "acme-corp", "--email", "ada@acme.com"]);
    assert_eq!(req["fields"]["company"], "acme-corp");
    assert_eq!(req["fields"]["email"], "ada@acme.com");
    assert!(req["fields"].get("phone").is_none());

    let req = ask_of(&["add", "deal", "Acme renewal", "--value", "45000.5", "--currency", "eur", "--stage", "proposal"]);
    assert_eq!(req["kind"], "deal");
    assert_eq!(req["name"], "Acme renewal");
    assert_eq!(req["fields"]["value"], "45000.5");
    assert_eq!(req["fields"]["currency"], "eur");
    assert_eq!(req["fields"]["stage"], "proposal");
}

#[test]
fn add_refuses_a_missing_or_unknown_kind_and_a_flag_from_the_wrong_kind() {
    let (msg, code) = refusal_for(&["add"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("company, contact or deal"), "{msg}");

    let (msg, code) = refusal_for(&["add", "widget", "x"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("widget"), "{msg}");

    // `--domain` is a company flag; a contact does not have one.
    let (msg, code) = refusal_for(&["add", "contact", "Ada", "--domain", "x"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("--domain"), "{msg}");
}

// MARK: - set / log / move / task / done / link / archive / select

#[test]
fn set_takes_exactly_three_positionals() {
    assert_eq!(
        ask_of(&["set", "acme", "domain", "acme.io"]),
        json!({ "cmd": "set", "handle": "acme", "field": "domain", "value": "acme.io" })
    );
    let (_, code) = refusal_for(&["set", "acme", "domain"]);
    assert_eq!(code, exit::USAGE);
}

#[test]
fn log_validates_its_kind_word_locally_and_the_date_shape_locally() {
    assert_eq!(
        ask_of(&["log", "call", "acme", "rang them"]),
        json!({ "cmd": "log", "kind": "call", "handle": "acme", "body": "rang them" })
    );
    let (msg, code) = refusal_for(&["log", "chat", "acme", "hi"]);
    assert_eq!(code, exit::USAGE, "an activity kind is a fixed, small vocabulary the CLI already knows: {msg}");
    assert!(msg.contains("call, email, meeting, note"), "{msg}");

    let req = ask_of(&["log", "note", "acme", "hi", "--at", "2026-09-10"]);
    assert_eq!(req["at"], "2026-09-10");
    let (msg, code) = refusal_for(&["log", "note", "acme", "hi", "--at", "10 sept"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("YYYY-MM-DD"), "{msg}");
}

#[test]
fn move_sends_the_stage_word_unvalidated_for_the_core_to_judge() {
    assert_eq!(ask_of(&["move", "acme", "won"]), json!({ "cmd": "move", "handle": "acme", "to": "won" }));
    // A bad stage word is core vocabulary — passed through, refused at exit 1 by the
    // core, never checked here.
    assert_eq!(ask_of(&["move", "acme", "orbit"]), json!({ "cmd": "move", "handle": "acme", "to": "orbit" }));
}

#[test]
fn task_requires_due_as_a_flag_not_a_positional() {
    let req = ask_of(&["task", "acme", "call back", "--due", "2026-09-30"]);
    assert_eq!(req, json!({ "cmd": "task", "handle": "acme", "what": "call back", "due": "2026-09-30" }));

    let (msg, code) = refusal_for(&["task", "acme", "call back"]);
    assert_eq!(code, exit::USAGE, "`--due` has no brackets in the grammar — it is required: {msg}");
    assert!(msg.contains("--due"), "{msg}");

    let (msg, code) = refusal_for(&["task", "acme", "call back", "--due", "next tuesday"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("YYYY-MM-DD"), "{msg}");
}

#[test]
fn done_link_and_archive_take_exactly_the_handles_the_grammar_names() {
    assert_eq!(ask_of(&["done", "call-back"]), json!({ "cmd": "done", "handle": "call-back" }));
    assert_eq!(
        ask_of(&["link", "acme-renewal", "ada-lovelace"]),
        json!({ "cmd": "link", "handle": "acme-renewal", "toHandle": "ada-lovelace" })
    );
    assert_eq!(ask_of(&["archive", "acme"]), json!({ "cmd": "archive", "handle": "acme" }));
    assert_eq!(ask_of(&["archive", "acme", "--restore"]), json!({ "cmd": "archive", "handle": "acme", "restore": true }));
    assert!(ask_of(&["archive", "acme"]).get("restore").is_none(), "omitted, not false, when not given");
}

#[test]
fn select_wants_a_whole_number_and_converts_it() {
    assert_eq!(ask_of(&["select", "2"]), json!({ "cmd": "select", "n": 2 }));
    let (msg, code) = refusal_for(&["select", "two"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("whole number"), "{msg}");
}

// MARK: - import: file reading, sniffing, both formats

#[test]
fn import_reads_the_file_from_the_agent_s_own_working_directory() {
    let dir = std::env::temp_dir().join(format!("crm-cli-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("companies.csv");
    std::fs::write(&path, "handle,name,domain\nacme,Acme Corp,acme.com\n").unwrap();

    let req = ask_of(&["import", path.to_str().unwrap()]);
    assert_eq!(req["cmd"], "import");
    assert_eq!(req["kind"], "company");
    assert_eq!(req["rows"][0]["name"], "Acme Corp");
    assert_eq!(req["rows"][0]["domain"], "acme.com");

    // A path that names nothing is a valid request the app declined (exit 1), not a command
    // line of the wrong shape (exit 2) — and it says why in words, not the OS's number.
    let (msg, code) = refusal_for(&["import", dir.join("missing.csv").to_str().unwrap()]);
    assert_eq!(code, exit::FAILED, "{msg}");
    assert!(msg.contains("cannot read") && msg.contains("no such file"), "{msg}");
    assert!(!msg.contains("os error"), "the platform's number leaked: {msg}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_sniffs_kind_from_the_header_and_title_alone_never_reads_as_a_deal() {
    assert_eq!(sniff_kind(&["handle".into(), "domain".into(), "notes".into()]), Some(Kind::Company));
    assert_eq!(sniff_kind(&["handle".into(), "email".into(), "title".into()]), Some(Kind::Contact));
    assert_eq!(sniff_kind(&["handle".into(), "stage".into(), "status".into()]), Some(Kind::Deal));
    // The bug a live run caught: a contacts export also has a `title` column (a job
    // title), which must never be mistaken for a deal's own `title` (its name).
    assert_eq!(sniff_kind(&["name".into(), "email".into(), "phone".into(), "title".into()]), Some(Kind::Contact));
    assert_eq!(sniff_kind(&["name".into(), "title".into()]), Some(Kind::Contact), "title alone still leans contact");
    assert_eq!(sniff_kind(&["name".into(), "id".into()]), None, "no distinctive column at all");
}

#[test]
fn import_kind_flag_overrides_sniffing_and_rejects_a_bad_word() {
    let dir = std::env::temp_dir().join(format!("crm-cli-import2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("people.csv");
    std::fs::write(&path, "name,email\nAda,ada@acme.com\n").unwrap();
    let req = ask_of(&["import", path.to_str().unwrap(), "--kind", "contacts"]);
    assert_eq!(req["kind"], "contact");

    let (msg, code) = refusal_for(&["import", path.to_str().unwrap(), "--kind", "widgets"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_blank_trailing_csv_line_is_not_imported_as_a_row() {
    let (kind, rows) = parse_csv_import("handle,name,domain\nacme,Acme,acme.com\n\n", None).unwrap();
    assert_eq!(kind, Kind::Company);
    assert_eq!(rows.as_array().unwrap().len(), 1);
}

#[test]
fn vcard_parses_the_fields_this_app_has_somewhere_to_put_and_skips_a_card_with_no_name() {
    let content = "BEGIN:VCARD\nFN:Ada Lovelace\nEMAIL:ada@acme.com\nTEL:555-1\nTITLE:Engineer\nORG:Acme Corp;Sales\nEND:VCARD\nBEGIN:VCARD\nEMAIL:noname@x.com\nEND:VCARD\n";
    let rows = parse_vcard(content);
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1, "the card with no FN is skipped: {rows:?}");
    assert_eq!(rows[0]["name"], "Ada Lovelace");
    assert_eq!(rows[0]["email"], "ada@acme.com");
    assert_eq!(rows[0]["phone"], "555-1");
    assert_eq!(rows[0]["title"], "Engineer");
    assert_eq!(rows[0]["company"], "Acme Corp", "ORG's own subfields are dropped, only the org name is used");
}

#[test]
fn a_vcf_file_forces_vcard_parsing_regardless_of_kind_flag_value() {
    let dir = std::env::temp_dir().join(format!("crm-cli-vcf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("c.vcf");
    std::fs::write(&path, "BEGIN:VCARD\nFN:Ada\nEND:VCARD\n").unwrap();
    let req = ask_of(&["import", path.to_str().unwrap()]);
    assert_eq!(req["kind"], "contact");

    let (msg, code) = refusal_for(&["import", path.to_str().unwrap(), "--kind", "deals"]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("vCard"), "{msg}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A response as the CLI now receives one: the **update** the core attached, with this seat's
/// `view`, the `seat` and the org's `derived`. These hand-made fixtures are written with the keys
/// grouped the way a person thinks about them (`focus`, `list`, `counts`, …) and nested here, so
/// each test still says what it is about while the CLI is only ever given the one shape.
fn shown(parts: Value) -> Value {
    let mut parts = parts;
    let mut take = |keys: &[&str]| {
        let mut out = serde_json::Map::new();
        for k in keys {
            if let Some(v) = parts.as_object_mut().and_then(|o| o.remove(*k)) {
                out.insert((*k).to_string(), v);
            }
        }
        Value::Object(out)
    };
    let view = take(&["focus", "focused", "list", "board", "cards", "pending"]);
    let seat = take(&["agents", "reminders"]);
    let derived = take(&["counts", "due"]);
    let stages = take(&["pipeline"]);
    let mut out = parts;
    out["split"] = json!({ "type": "update", "shape": "split", "view": view, "seat": seat, "derived": derived });
    if let Some(p) = stages.get("pipeline") {
        out["stages"] = p["stages"].clone();
    }
    out
}

// MARK: - Rendering: the bug a live run caught, and the guard that would have too

/// A snapshot with real ULIDs everywhere an id could leak — every render function below
/// is run against this once.
fn a_snapshot() -> Value {
    shown(json!({
        "ok": true, "rev": 9, "answer": "changed",
        "focus": { "kind": "deal", "id": an_id(0x10), "handle": "acme-renewal" },
        "focused": {
            "row": {
                "kind": "deal", "id": an_id(0x10), "handle": "acme-renewal", "label": "Acme renewal",
                "stage": "negotiation", "status": "open", "archived": false,
            },
            "fields": [{ "label": "Value", "value": "$45,000.00" }],
        },
        "board": { "columns": [
            { "key": "lead", "label": "Lead", "count": 1, "totals": [{ "amount": 100, "currency": "USD", "formatted": "$1.00" }] },
            { "key": "won", "label": "Won", "count": 0, "totals": [] },
        ]},
        "pipeline": { "stages": ["lead", "qualified", "proposal", "negotiation"] },
        "due": { "overdue": 1, "today": 0, "week": 2 },
        "list": { "total": 1, "page": 0, "rows": [
            { "kind": "deal", "id": an_id(0x20), "handle": "hooli-deal", "label": "Hooli deal", "value": { "amount": 5, "currency": "USD", "formatted": "$0.05" } },
        ]},
        "counts": { "companies": 1, "contacts": 1, "deals": 2, "activities": 0, "tasks": 0 },
        "agents": [{ "id": an_id(0x30), "name": "Scout", "backend": "claude" }],
        "import": { "created": 1, "skipped": [{ "row": {"a": an_id(0x40)}, "reason": "no name" }] },
    }))
}

fn an_ambiguous_snapshot() -> Value {
    an_ambiguous_snapshot_resuming(false)
}

/// `resuming: true` is what a write verb's ambiguity carries — `crm select N` completes
/// the interrupted write, and `crm show <handle>` will not.
fn an_ambiguous_snapshot_resuming(resuming: bool) -> Value {
    shown(json!({
        "ok": true, "rev": 10, "answer": "ambiguous", "focus": null,
        "pending": { "prompt": "which “acme”?", "resuming": resuming, "candidates": [
            { "kind": "company", "id": an_id(1), "handle": "acme-corp", "label": "Acme Corp" },
            { "kind": "deal", "id": an_id(2), "handle": "acme-renewal-2", "label": "Acme renewal" },
        ]},
    }))
}

/// Every request this build can construct, real handles and stage words in place —
/// what `render` is handed alongside a response.
fn a_request(verb: &str) -> Value {
    match verb {
        "move" => json!({ "cmd": "move", "handle": "acme-renewal", "to": "won" }),
        "archive" => json!({ "cmd": "archive", "handle": "acme-renewal" }),
        _ => json!({ "cmd": verb }),
    }
}

/// **The regression guard**, extended to every verb this build answers rather than only
/// `status`/`show` as it was before M2: an id reaching stdout from *any* of them is the
/// same defect, wherever it leaks from.
#[test]
fn no_id_ever_reaches_stdout_from_any_verb_against_either_kind_of_answer() {
    for snap in [a_snapshot(), an_ambiguous_snapshot()] {
        for (verb, _) in VERBS {
            let out = render(verb, &a_request(verb), &snap);
            assert!(first_ulid_in(&out).is_none(), "`{verb}` printed an id:\n{out}");
        }
        assert!(first_ulid_in(&board_lines(&snap)).is_none());
        assert!(first_ulid_in(&stages_lines(&snap)).is_none());
        assert!(first_ulid_in(&due_lines(&snap, &[])).is_none());
        assert!(first_ulid_in(&find_lines(&snap, None)).is_none());
        assert!(first_ulid_in(&import_lines(&snap)).is_none(), "a skipped row can carry an id — it must not print");
    }
    // …and the real core's own golden bytes, not only fixtures written in this file.
    // (A bootstrap carries the same `view`, `seat` and `derived` an update does, which is all
    // the CLI reads, so it stands in for the update every response carries.)
    let golden: Value = serde_json::from_str(include_str!("../fixtures/split/bootstrap.json")).unwrap();
    let pending: Value = serde_json::from_str(include_str!("../fixtures/split/bootstrap-pending.json")).unwrap();
    for fixture in [golden, pending] {
        let snap = json!({ "split": fixture });
        for (verb, _) in VERBS {
            let out = render(verb, &a_request(verb), &snap);
            assert!(first_ulid_in(&out).is_none(), "`{verb}` printed an id from the golden fixture:\n{out}");
        }
        // Not vacuous: the fixture really is what the CLI reads, and it prints something.
        assert!(status_lines(&snap).contains("looking at:"), "the CLI could not read the golden update");
    }
}

/// **The bug itself.** `move` and `archive` do not change `focus`, so their confirmation
/// must come from what was asked for, not from whatever happens to be open. Built from a
/// response whose `focused.row` names a *different* record than the one being moved —
/// exactly the shape that printed "moved acme-renewal to proposal" while actually having
/// moved a company that was not even a deal, the first time this ran against a real app.
#[test]
fn move_and_archive_confirmations_describe_the_record_that_was_actually_acted_on() {
    let snap = a_snapshot(); // focused.row is acme-renewal, stage negotiation
    let req = json!({ "cmd": "move", "handle": "some-other-deal", "to": "proposal" });
    assert_eq!(render("move", &req, &snap), "moved some-other-deal to proposal\n");

    let req = json!({ "cmd": "archive", "handle": "some-other-record" });
    assert_eq!(render("archive", &req, &snap), "archived\n");
    let req = json!({ "cmd": "archive", "handle": "some-other-record", "restore": true });
    assert_eq!(render("archive", &req, &snap), "restored\n");
}

#[test]
fn show_prints_the_record_then_its_fields() {
    let out = render("show", &json!({}), &a_snapshot());
    assert!(out.starts_with("deal acme-renewal — Acme renewal\n"), "{out}");
    assert!(out.contains("Value"), "{out}");
}

/// **QA finding:** the ambiguity block never mentioned `crm select N`, the documented
/// resolver, and offered only `crm show <handle>` — which does not complete a write.
/// `crm select 1`/`crm select 2` must be the numbered choices, always; `crm show` is
/// offered too, but its wording must change once a write is actually being resumed.
#[test]
fn ambiguity_leads_with_select_and_only_offers_show_as_a_plain_look() {
    let out = render("show", &json!({}), &an_ambiguous_snapshot());
    assert!(out.contains("crm select 1"), "{out}");
    assert!(out.contains("crm select 2"), "{out}");
    assert!(out.contains("crm show acme-corp"), "show is still offered, for a plain look: {out}");

    let out = render("log", &a_request("log"), &an_ambiguous_snapshot_resuming(true));
    assert!(out.contains("crm select 1") && out.contains("crm select 2"), "{out}");
    assert!(out.contains("Picking one finishes it"), "{out}");
    assert!(
        out.contains("will not") && !out.contains("crm show acme-corp"),
        "a resumed write must not offer `show` as if it were equivalent: {out}"
    );
}

#[test]
fn every_write_verb_prints_the_shared_ambiguity_block_when_the_core_parked_one() {
    for resuming in [false, true] {
        let snap = an_ambiguous_snapshot_resuming(resuming);
        for verb in ["set", "log", "move", "task", "done", "link", "archive"] {
            let out = render(verb, &a_request(verb), &snap);
            assert!(out.contains("More than one record matches"), "`{verb}` (resuming={resuming}): {out}");
            assert!(out.contains("crm select 1"), "`{verb}` (resuming={resuming}): {out}");
        }
    }
}

#[test]
fn status_falls_back_to_the_kind_alone_when_a_focus_has_no_handle() {
    let snap = shown(json!({ "focus": { "kind": "deal", "id": an_id(5) }, "counts": {}, "agents": [] }));
    let out = status_lines(&snap);
    assert!(out.contains("looking at: deal\n"), "{out}");
    assert!(first_ulid_in(&out).is_none(), "{out}");
}

#[test]
fn find_lines_reports_the_shared_total_and_trims_to_the_local_limit() {
    let snap = a_snapshot();
    let full = find_lines(&snap, None);
    assert!(full.contains("hooli-deal"), "{full}");
    assert!(full.contains("1 of 1 (page 1)"), "{full}");
    let limited = find_lines(&snap, Some(0));
    assert!(limited.contains("no results"), "{limited}");
}

/// **QA finding.** A sticky `--kind` filter (correct, shared, persists across calls per
/// `m2-cli.md`) was invisible: `crm find acme` after `crm find --kind contact` printed
/// "no results" with nothing saying a filter was even in force. Named now, on the
/// zero-result path and the ordinary one — a filtered "2 of 2" is the same silence, just
/// quieter.
#[test]
fn find_names_a_sticky_kind_filter_on_both_the_empty_and_the_ordinary_path() {
    let mut filtered_empty = a_snapshot();
    filtered_empty["split"]["view"]["list"]["kind"] = json!("contact");
    filtered_empty["split"]["view"]["list"]["rows"] = json!([]);
    filtered_empty["split"]["view"]["list"]["total"] = json!(0);
    let out = find_lines(&filtered_empty, None);
    assert!(out.contains("no results"), "{out}");
    assert!(out.contains("filtered to contact"), "{out}");
    assert!(out.contains("crm find --kind all"), "{out}");

    let mut filtered_hit = a_snapshot();
    filtered_hit["split"]["view"]["list"]["kind"] = json!("deal");
    let out = find_lines(&filtered_hit, None);
    assert!(out.contains("hooli-deal"), "{out}");
    assert!(out.contains("filtered to deal"), "a milder version of the same silence: {out}");

    // No filter at all: no note, on either path.
    let out = find_lines(&a_snapshot(), None);
    assert!(!out.contains("filtered to"), "{out}");
}

/// **QA finding.** `status` claims to say "what both surfaces are looking at" and said
/// nothing about the shared list — the one place a sticky filter, an active search or a
/// non-default sort would actually be visible.
#[test]
fn status_reports_the_shared_list_s_query_kind_sort_and_page() {
    let mut snap = a_snapshot();
    snap["split"]["view"]["list"] = json!({ "query": "acme", "kind": "contact", "sort": "name", "page": 1, "total": 7 });
    let out = status_lines(&snap);
    assert!(out.contains("list:"), "{out}");
    assert!(out.contains("“acme”"), "{out}");
    assert!(out.contains("kind contact"), "{out}");
    assert!(out.contains("sort name"), "{out}");
    assert!(out.contains("page 2"), "1-based, matching everywhere else a page is shown: {out}");
    assert!(out.contains("7 total"), "{out}");
}

#[test]
fn status_names_an_empty_query_and_no_filter_plainly() {
    let mut snap = a_snapshot();
    snap["split"]["view"]["list"] = json!({ "query": "", "kind": null, "sort": "updated", "page": 0, "total": 0 });
    let out = status_lines(&snap);
    assert!(out.contains("(none set)"), "{out}");
    assert!(out.contains("kind all"), "{out}");
}

#[test]
fn board_lines_names_the_stage_word_first() {
    let out = board_lines(&a_snapshot());
    assert!(out.contains("lead"), "{out}");
    assert!(out.contains("1 deal"), "{out}");
    assert!(out.contains("$1.00"), "{out}");
    assert!(out.contains("0 deals"), "won prints its zero too: {out}");
}

#[test]
fn import_lines_names_the_reason_and_never_the_offending_row() {
    let out = import_lines(&a_snapshot());
    assert!(out.contains("imported 1"), "{out}");
    assert!(out.contains("no name"), "{out}");
    assert!(first_ulid_in(&out).is_none(), "{out}");
}

#[test]
fn the_not_running_error_names_the_app_id_to_start() {
    let raw = "crm: app is not running — start it with `clatch run <id>`";
    let shown = transport_error(raw);
    assert!(shown.contains(APP_ID), "{shown}");
    assert!(!shown.contains("<id>"), "{shown}");
}

// MARK: - The exit-code split, spot-checked across the board

/// Shape (bad command line) is 2; a valid request the core would have to weigh in on is
/// left to the core, which answers `ok:false` and becomes 1 through `ask`'s own path —
/// this file cannot exercise that half without a running app, but it can prove the CLI
/// never pre-empts it: a stage word, a sort word, and a field name are all passed
/// through unexamined.
#[test]
fn core_vocabulary_is_never_pre_validated_by_the_command_line_parser() {
    assert!(matches!(plan(&argv(&["move", "acme", "not-a-real-stage"])), Plan::Ask(..)));
    assert!(matches!(plan(&argv(&["find", "--sort", "not-a-real-sort"])), Plan::Ask(..)));
    assert!(matches!(plan(&argv(&["set", "acme", "not-a-real-field", "x"])), Plan::Ask(..)));
    assert!(matches!(plan(&argv(&["log", "call", "acme", "hi"])), Plan::Ask(..)));
}

#[test]
fn shape_problems_are_always_exit_two() {
    for line in [
        vec!["select", "abc"],
        vec!["find", "--page", "abc"],
        vec!["task", "h", "w", "--due", "abc"],
        vec!["log", "not-a-kind", "h", "b"],
        vec!["move", "h"],
        vec!["set", "h", "f"],
        vec!["add", "notakind", "n"],
        vec!["show", "a", "b"],
        vec!["export", "notakind"],
    ] {
        let (msg, code) = refusal_for(&line);
        assert_eq!(code, exit::USAGE, "{line:?} → {msg}");
    }
}

// MARK: - QA round 4: next steps are findable, and `select` says what it finished
//
// These run the CLI's own render functions over the **real core's** answers — a real
// `AppState`, real envelopes, real handles — not snapshots written by hand. Round 2's
// blocker and this round's both survived because a test pinned a shape the code no longer
// had; a test that asks the core cannot drift from it.

mod real {
    use super::*;
    use crate::model::{Ctx, Date, InstanceId, Now};
    use crate::state::AppState;

    pub fn ctx() -> Ctx {
        Ctx {
            now: Now { at: 1_788_861_600_000, today: Date::new(2026, 9, 8) },
            entropy: [0x5A; 10],
            origin: InstanceId::from_bytes([0xA1; 16]),
            offset_secs: 0,
        }
    }

    /// What one CLI invocation does end to end, minus the socket: parse the command line,
    /// hand the envelope to the core, render the answer. The `Err` is what the agent
    /// would read on stderr (exit 1).
    pub fn crm(st: &mut AppState, line: &[&str]) -> Result<String, String> {
        crm_reading(st, line, false)
    }

    /// The top-level keys the retired snapshot shape carried. A response with them **removed**
    /// is what proves the CLI reads none of them.
    pub const RETIRED_KEYS: &[&str] =
        &["rev", "focus", "focused", "list", "board", "cards", "pending", "due", "counts", "agents", "reminders", "pipeline"];

    /// [`crm`], optionally rendering from the response with [`RETIRED_KEYS`] deleted first.
    pub fn crm_reading(st: &mut AppState, line: &[&str], strip_retired_keys: bool) -> Result<String, String> {
        let args: Vec<String> = line.iter().map(|s| s.to_string()).collect();
        let (verb, req) = match plan(&args) {
            Plan::Ask(verb, req) => (verb, req),
            other => panic!("{line:?} did not parse into a request: {other:?}"),
        };
        let mut wire = req.clone();
        wire.as_object_mut().unwrap().retain(|k, _| !k.starts_with("__"));
        let out = st.command(&wire, Some("agent-1"), &ctx());
        if out.resp["ok"] == false {
            return Err(out.resp["error"].as_str().unwrap_or("").to_string());
        }
        let mut resp = out.resp;
        if strip_retired_keys {
            resp.as_object_mut().unwrap().retain(|k, _| !RETIRED_KEYS.contains(&k.as_str()));
        }
        let only = take_str_array(&mut req.clone(), "__only");
        Ok(match verb.as_str() {
            "due" => due_lines(&resp, &only),
            "find" => find_lines_for(&resp, None, &wire),
            _ => render(&verb, &wire, &resp),
        })
    }

    pub fn seeded_deal_with_tasks() -> AppState {
        let mut st = AppState::new();
        crm(&mut st, &["add", "deal", "Acme renewal"]).unwrap();
        crm(&mut st, &["task", "acme-renewal", "Send the signed order form", "--due", "2026-10-01"]).unwrap();
        crm(&mut st, &["task", "acme-renewal", "Call Maya", "--due", "2026-09-07"]).unwrap();
        crm(&mut st, &["task", "acme-renewal", "Book kickoff", "--due", "2026-09-01"]).unwrap();
        crm(&mut st, &["done", "book-kickoff"]).unwrap();
        st
    }
}

// -- `crm show` lists open next steps, and the handle it prints is the one `done` takes ------

#[test]
fn show_lists_open_next_steps_with_their_handles_and_leaves_finished_ones_out() {
    let mut st = real::seeded_deal_with_tasks();
    let out = real::crm(&mut st, &["show", "acme-renewal"]).unwrap();

    assert!(out.contains("Next steps"), "{out}");
    assert!(out.contains("send-the-signed-order-form"), "{out}");
    assert!(out.contains("call-maya"), "{out}");
    assert!(out.contains("due 2026-10-01") && out.contains("Send the signed order form"), "{out}");
    assert!(!out.contains("book-kickoff"), "a finished next step is not still to do: {out}");
    assert!(first_ulid_in(&out).is_none(), "an id reached `crm show`: {out}");
}

/// **The regression itself.** QA ran `crm done acme-renewal`, was told "`crm show` lists
/// it", ran `crm show acme-renewal`, and found no task in the output — advice that taught
/// something false. Here the whole loop runs: read the handle out of what `show` printed,
/// and `done` accepts it.
#[test]
fn the_handle_show_prints_is_one_done_accepts() {
    let mut st = real::seeded_deal_with_tasks();

    // The refusal points at `show`…
    let err = real::crm(&mut st, &["done", "acme-renewal"]).unwrap_err();
    assert!(err.contains("`crm show acme-renewal`"), "{err}");

    // …and `show` really does carry what it points at: take the handle from its output.
    let shown = real::crm(&mut st, &["show", "acme-renewal"]).unwrap();
    let line = shown.lines().find(|l| l.contains("Send the signed order form")).expect("the task is listed");
    let handle = line.split_whitespace().next().unwrap().to_string();
    assert_eq!(handle, "send-the-signed-order-form");

    assert_eq!(real::crm(&mut st, &["done", &handle]).unwrap(), "done\n");
    let after = real::crm(&mut st, &["show", "acme-renewal"]).unwrap();
    assert!(!after.contains("send-the-signed-order-form"), "completed, so no longer listed: {after}");
    assert!(after.contains("call-maya"), "the other one still is: {after}");
}

#[test]
fn a_record_with_no_open_next_steps_prints_no_next_steps_section() {
    let mut st = crate::state::AppState::new();
    real::crm(&mut st, &["add", "company", "Acme Corp"]).unwrap();
    let out = real::crm(&mut st, &["show", "acme-corp"]).unwrap();
    assert!(!out.contains("Next steps"), "{out}");
}

// -- `crm due` lists ---------------------------------------------------------------------------

#[test]
fn due_lists_each_next_step_under_its_bucket_with_its_handle_and_record() {
    let mut st = real::seeded_deal_with_tasks();
    let out = real::crm(&mut st, &["due"]).unwrap();

    // 2026-09-07 is overdue against a fixed today of 2026-09-08; 2026-10-01 is beyond a week.
    assert!(out.contains("overdue 1\n  call-maya  2026-09-07  Call Maya  (on acme-renewal)\n"), "{out}");
    assert!(out.contains("today 0\n"), "{out}");
    assert!(out.contains("week 0\n"), "{out}");
    assert!(!out.contains("send-the-signed-order-form"), "not due within a week: {out}");
    assert!(!out.contains("book-kickoff"), "finished: {out}");
    assert!(first_ulid_in(&out).is_none(), "{out}");
}

#[test]
fn due_flags_still_choose_which_buckets_print_and_the_list_follows_them() {
    let mut st = real::seeded_deal_with_tasks();
    real::crm(&mut st, &["task", "acme-renewal", "Ring Ada", "--due", "2026-09-09"]).unwrap();

    let out = real::crm(&mut st, &["due", "--week"]).unwrap();
    assert!(out.starts_with("week 1\n  ring-ada"), "{out}");
    assert!(!out.contains("call-maya") && !out.contains("overdue"), "only the asked-for bucket: {out}");
}

#[test]
fn a_handle_due_prints_completes_the_task_when_typed_into_done() {
    let mut st = real::seeded_deal_with_tasks();
    let out = real::crm(&mut st, &["due"]).unwrap();
    let handle = out.lines().find(|l| l.contains("Call Maya")).unwrap().split_whitespace().next().unwrap().to_string();
    assert_eq!(real::crm(&mut st, &["done", &handle]).unwrap(), "done\n");
    assert!(real::crm(&mut st, &["due"]).unwrap().starts_with("overdue 0\n"));
}

// -- `crm select` names the write it completed ---------------------------------------------------

fn two_acmes() -> crate::state::AppState {
    let mut st = crate::state::AppState::new();
    real::crm(&mut st, &["add", "company", "Acme Corp"]).unwrap();
    real::crm(&mut st, &["add", "company", "Acme Industries"]).unwrap();
    st
}

/// **The regression itself.** `crm log note acme "…"` → `crm select 1` printed "opened
/// company acme-corp" — word for word what a plain `show` prints — though the note *was*
/// logged. An agent that cannot tell its write landed writes it again.
#[test]
fn select_that_resumed_a_log_says_it_logged_and_where() {
    let mut st = two_acmes();
    let parked = real::crm(&mut st, &["log", "note", "acme", "both acmes now exist"]).unwrap();
    assert!(parked.contains("crm select 1"), "{parked}");

    let out = real::crm(&mut st, &["select", "1"]).unwrap();
    assert_eq!(out, "logged — acme-corp\n");
    assert!(!out.contains("opened"), "must not read like a plain look: {out}");

    // And it really was logged, once.
    assert!(real::crm(&mut st, &["status"]).unwrap().contains("activities 1"));
}

#[test]
fn select_that_resumed_each_other_write_names_that_write() {
    let cases: &[(&[&str], &str)] = &[
        (&["set", "acme", "domain", "a.com"], "updated — acme-industries\n"),
        (&["task", "acme", "call back", "--due", "2026-09-30"], "task set — acme-industries\n"),
        (&["archive", "acme"], "archived — acme-industries\n"),
    ];
    for (line, want) in cases {
        let mut st = two_acmes();
        real::crm(&mut st, line).unwrap();
        let out = real::crm(&mut st, &["select", "2"]).unwrap();
        assert_eq!(&out, want, "after {line:?}");
    }
}

#[test]
fn select_that_resumed_a_move_names_the_deal_and_where_it_went() {
    let mut st = crate::state::AppState::new();
    real::crm(&mut st, &["add", "deal", "Acme renewal"]).unwrap();
    real::crm(&mut st, &["add", "deal", "Acme pilot"]).unwrap();
    real::crm(&mut st, &["move", "acme", "proposal"]).unwrap();
    let out = real::crm(&mut st, &["select", "2"]).unwrap();
    assert!(out.starts_with("moved acme-") && out.ends_with(" to proposal\n"), "{out}");
    assert!(!out.contains("opened"), "{out}");
}

/// The plain case is unchanged: a `show` question resumed nothing, so "opened" is true.
#[test]
fn select_that_only_answers_a_show_still_says_it_opened_the_record() {
    let mut st = two_acmes();
    real::crm(&mut st, &["show", "acme"]).unwrap();
    assert_eq!(real::crm(&mut st, &["select", "1"]).unwrap(), "opened company acme-corp\n");
}

#[test]
fn a_resumed_write_confirmation_names_no_id() {
    let mut st = two_acmes();
    real::crm(&mut st, &["log", "call", "acme", "rang"]).unwrap();
    let out = real::crm(&mut st, &["select", "1"]).unwrap();
    assert!(first_ulid_in(&out).is_none(), "{out}");
}

// -- the manual and `-h` say a next step has a handle ---------------------------------------------

#[test]
fn the_manual_and_the_task_verbs_say_a_next_step_has_a_handle_and_where_to_read_it() {
    let m = manual();
    assert!(m.contains("next steps:") && m.contains("crm due") && m.contains("crm done <task-handle>"), "{m}");
    for verb in ["task", "done", "due"] {
        let note = verb_note(verb).unwrap_or_else(|| panic!("`crm {verb} -h` says nothing about handles"));
        assert!(note.contains("handle"), "{verb}: {note}");
    }
    assert!(verb_note("done").unwrap().contains("crm show"), "names where the handle is listed");
}

// MARK: - M4: `crm status` says what the timer is doing, and what it cannot do

mod reminders {
    use super::*;
    use crate::state::AppState;
    use clappkit::AgentRow;

    const SCOUT: &str = "1789126979";

    /// What `crm status` is handed: a response carrying the update the core built.
    fn of(update: Value) -> Value {
        json!({ "split": update })
    }

    fn scout() -> AgentRow {
        AgentRow { id: SCOUT.into(), name: "Scout".into(), backend: Some("claude".into()), model: None, avatar: None }
    }

    /// The core, driven to a chosen clock. Day `n` after 2026-09-08 10:00 UTC.
    fn ctx_at(days: i64, minutes: i64) -> crate::model::Ctx {
        let mut c = real::ctx();
        c.now.at += days * 86_400_000 + minutes * 60_000;
        c.now.today = crate::local_date(c.now.at, 0);
        c
    }

    fn state_with_a_task_due_on(due: &str) -> AppState {
        let mut st = AppState::new();
        st.set_agents(vec![scout()]);
        let deal = st.add_deal("Acme renewal", None, None, None, &real::ctx());
        st.add_task("Call Maya", crate::model::Date::parse(due).unwrap(), vec![deal], None, &real::ctx());
        st
    }

    #[test]
    fn status_says_when_it_last_checked_and_when_a_reminder_was_sent() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        let sweep = st.sweep(&ctx_at(3, 0));
        let now = ctx_at(3, 7).at();
        let out = status_lines_at(&of(sweep.update), now, 0);
        assert!(out.contains("reminders: checked 7 min ago, every 5 min\n"), "{out}");
        assert!(
            out.contains("reminder sent 10:00 — the platform does not confirm delivery (1 next step, 1 still open)"),
            "{out}"
        );
        assert!(!out.contains("REFUSED") && !out.contains("waiting"), "nothing is wrong: {out}");
    }

    // -- round 5: sent is not delivered ------------------------------------------------------

    /// **The regression.** `crm status` used to say "last sent just now" for a reminder the
    /// launcher had dropped, and the person had no reason to doubt it. Sent, not delivered —
    /// and the platform's silence is said out loud.
    #[test]
    fn status_never_calls_a_sent_reminder_delivered() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        let out = status_lines_at(&of(st.sweep(&ctx_at(3, 0)).update), ctx_at(3, 1).at(), 0);
        assert!(out.contains("does not confirm delivery"), "{out}");
        assert!(!out.contains("last sent") && !out.contains("delivered to"), "{out}");
    }

    #[test]
    fn the_time_a_reminder_was_sent_is_the_readers_local_time_not_utc() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        let snap = of(st.sweep(&ctx_at(3, 0)).update); // 10:00 UTC
        let now = ctx_at(3, 30).at();
        assert!(status_lines_at(&snap, now, 3 * 3600).contains("reminder sent 13:00"), "Istanbul");
        assert!(status_lines_at(&snap, now, -5 * 3600).contains("reminder sent 05:00"), "New York");
        assert!(status_lines_at(&snap, now, 0).contains("reminder sent 10:00"));
    }

    #[test]
    fn a_reminder_sent_on_another_day_carries_its_date() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        let snap = of(st.sweep(&ctx_at(3, 0)).update); // 2026-09-11
        let out = status_lines_at(&snap, ctx_at(5, 0).at(), 0);
        assert!(out.contains("reminder sent 2026-09-11 10:00 —"), "{out}");
    }

    #[test]
    fn status_says_who_can_send_it_again_while_a_step_is_still_open_and_stops_when_done() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        st.sweep(&ctx_at(3, 0));
        let open = status_lines_at(&of(st.update(ctx_at(3, 1).now, &[])), ctx_at(3, 1).at(), 0);
        assert!(open.contains("the person can send it again from the window"), "{open}");
        assert!(open.contains("not something the app can see"), "{open}");

        let id = st.db().task_by_handle("call-maya").unwrap().id.clone();
        st.complete_task(&id, &ctx_at(3, 2)).unwrap();
        let done = status_lines_at(&of(st.update(ctx_at(3, 3).now, &[])), ctx_at(3, 3).at(), 0);
        assert!(done.contains("reminder sent") && !done.contains("still open"), "{done}");
        assert!(!done.contains("send it again"), "nothing left to remind anybody of: {done}");
    }

    /// The caveat is on the surface the agent actually reads, and it is the real behaviour.
    #[test]
    fn status_always_says_reminders_only_fire_while_the_app_runs() {
        let st = state_with_a_task_due_on("2026-09-10");
        let out = status_lines_at(&of(st.update(ctx_at(0, 0).now, &[])), ctx_at(0, 0).at(), 0);
        assert!(out.contains("not checked yet"), "{out}");
        assert!(out.contains("nothing sent yet"), "{out}");
        assert!(out.contains("only fire while this app runs"), "{out}");
        assert!(out.contains("closed") && out.contains("next launch"), "{out}");
        for line in out.lines() {
            assert!(line.chars().count() <= 100, "a status line that will wrap badly: {line}");
        }
    }

    #[test]
    fn a_refusal_is_in_status_with_who_why_and_that_nobody_was_told() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        st.sweep(&ctx_at(3, 0));
        let refused = st.note_refusal("task.due", SCOUT, "inbox_full", &ctx_at(3, 0));
        let out = status_lines_at(&of(refused.update), ctx_at(3, 1).at(), 0);
        assert!(out.contains("reminders REFUSED: Scout would not take the last one — its inbox is full"), "{out}");
        assert!(out.contains("Nobody was told"), "{out}");
        assert!(out.contains("1 next step will be tried again at the next check"), "{out}");
        assert!(first_ulid_in(&out).is_none(), "{out}");
    }

    #[test]
    fn a_full_context_queue_is_worded_as_one_and_an_unknown_reason_is_quoted() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        st.sweep(&ctx_at(3, 0));
        let a = st.note_refusal("task.due", SCOUT, "queue_full", &ctx_at(3, 0));
        assert!(status_lines_at(&of(a.update), 0, 0).contains("its context queue is full"));
        let b = st.note_refusal("task.due", SCOUT, "something_new", &ctx_at(3, 0));
        assert!(status_lines_at(&of(b.update), 0, 0).contains("it said something_new"));
    }

    #[test]
    fn due_tasks_held_for_want_of_an_agent_are_said_to_be_waiting() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        st.set_agents(Vec::new());
        let sweep = st.sweep(&ctx_at(3, 0));
        let out = status_lines_at(&of(sweep.update), ctx_at(3, 0).at(), 0);
        assert!(out.contains("reminders waiting: 1 due next step, and no agent is connected to tell"), "{out}");
    }

    /// The backlog is on the surface the agent reads too: it was not woken for these, so
    /// the one place it can learn they exist is here.
    #[test]
    fn status_reports_the_quiet_backlog_and_says_nobody_was_woken() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        st.set_agents(vec![scout()]);
        let mut json = serde_json::to_value(st.db()).unwrap();
        for t in json["tasks"].as_array_mut().unwrap() {
            t["due"] = json!({ "y": 2026, "m": 9, "d": 1 });
            t.as_object_mut().unwrap().remove("dueSentAt");
        }
        json.as_object_mut().unwrap().remove("reminders");
        let (old, _) = AppState::open(serde_json::from_value(json).unwrap(), &ctx_at(0, 0));
        let out = status_lines_at(&of(old.update(ctx_at(0, 0).now, &[])), ctx_at(0, 0).at(), 0);
        assert!(out.contains("reminders backlog: 1 next step was already overdue when reminders began"), "{out}");
        assert!(out.contains("nobody was woken for it") && out.contains("`crm due`"), "{out}");
        assert!(!out.contains("waiting"), "and nothing is queued to fire: {out}");
    }

    #[test]
    fn a_snapshot_from_before_the_timer_prints_nothing_about_it() {
        let out = status_lines_at(&shown(json!({ "counts": {}, "agents": [] })), 0, 0);
        assert!(!out.contains("reminders"), "{out}");
    }

    #[test]
    fn ago_is_coarse_and_never_negative() {
        assert_eq!(ago(1_000, 1_000), "just now");
        assert_eq!(ago(2_000, 1_000), "just now", "a clock that stepped back is not the future");
        assert_eq!(ago(0, 59_999), "just now");
        assert_eq!(ago(0, 60_000), "1 min ago");
        assert_eq!(ago(0, 59 * 60_000), "59 min ago");
        assert_eq!(ago(0, 60 * 60_000), "1 h ago");
        assert_eq!(ago(0, 23 * 3_600_000), "23 h ago");
        assert_eq!(ago(0, 24 * 3_600_000), "1 d ago");
    }

    /// `crm -h` is the agent's only manual. A `task.due` that wakes it must be explained
    /// there, or the agent meets it with no idea what it is for.
    #[test]
    fn the_manual_explains_what_a_task_due_wake_up_is() {
        let m = manual();
        assert!(m.contains("reminders:") && m.contains("task.due"), "{m}");
        assert!(m.contains("crm due") && m.contains("once, ever"), "{m}");
        assert!(m.contains("not confirmed") && m.contains("send"), "the manual must not promise delivery: {m}");
        assert!(m.contains("only checks while it runs"), "{m}");
        for line in m.lines() {
            assert!(line.chars().count() <= 80, "{} chars: {line}", line.chars().count());
        }
    }
}

// MARK: - Round 5: `find` says what it is filtering by, `set` cannot erase by accident,
// and the refusals point at things an agent can do

mod round5 {
    use super::*;
    use crate::state::AppState;

    /// A workspace with three records, driven the way an agent drives it: through `crm`.
    fn a_workspace() -> AppState {
        let mut st = AppState::new();
        for args in [
            &["add", "company", "Acme Corp"][..],
            &["add", "contact", "Ada Whitlock"],
            &["add", "deal", "Acme renewal", "--value", "45000"],
        ] {
            real::crm(&mut st, args).unwrap();
        }
        st
    }

    // -- find: every filter in force is named, and how to clear it ------------------------

    /// **The QA finding, step by step as QA met it.** `crm find deal` — "deal" is a query
    /// here, not a kind — found nothing and said nothing about why; the only hint blamed a
    /// filter that was not the cause, and following it changed nothing.
    #[test]
    fn an_empty_find_names_the_query_that_caused_it_and_how_to_clear_it() {
        let mut st = a_workspace();
        let out = real::crm(&mut st, &["find", "zzz"]).unwrap();
        assert!(out.starts_with("no results (page 1 of 0 total)\n"), "{out}");
        assert!(out.contains("filtered by query “zzz” — `crm find \"\"` clears the query"), "{out}");
    }

    #[test]
    fn following_the_kind_hint_does_not_hide_the_query_that_is_still_in_force() {
        let mut st = a_workspace();
        real::crm(&mut st, &["find", "zzz", "--kind", "deal"]).unwrap();
        // The advice that used to lead nowhere: clear the kind…
        let out = real::crm(&mut st, &["find", "--kind", "all"]).unwrap();
        // …and the query, which nothing ever mentioned, is now the thing on the page.
        assert!(out.contains("filtered by query “zzz”"), "the cause must appear: {out}");
        assert!(!out.contains("filtered to"), "the kind is cleared: {out}");
    }

    #[test]
    fn an_empty_find_names_every_filter_in_force_the_query_and_the_kind() {
        let mut st = a_workspace();
        let out = real::crm(&mut st, &["find", "zzz", "--kind", "deal"]).unwrap();
        assert!(out.contains("filtered by query “zzz”") && out.contains("`crm find \"\"` clears the query"), "{out}");
        assert!(out.contains("filtered to deal") && out.contains("`crm find --kind all` searches everything"), "{out}");
    }

    /// And the fix nothing ever suggested actually works.
    #[test]
    fn find_with_an_empty_string_clears_the_query_and_the_workspace_is_back() {
        let mut st = a_workspace();
        real::crm(&mut st, &["find", "deal"]).unwrap();
        let out = real::crm(&mut st, &["find", ""]).unwrap();
        assert!(out.contains("3 of 3"), "{out}");
        assert!(!out.contains("filtered by query"), "no filter left to name: {out}");
    }

    /// A query inherited from earlier is named on a *non-empty* result too — "2 of 2" while
    /// filtered is the same silence, quieter. A query typed this time is not news.
    #[test]
    fn a_query_left_over_from_earlier_is_named_on_a_result_and_a_typed_one_is_not() {
        let mut st = a_workspace();
        let typed = real::crm(&mut st, &["find", "acme"]).unwrap();
        assert!(!typed.contains("filtered by query"), "they just typed it: {typed}");

        // Same query still in force, not retyped: this one is a filter the reader did not ask for now.
        let inherited = real::crm(&mut st, &["find", "--sort", "name"]).unwrap();
        assert!(inherited.contains("filtered by query “acme” — `crm find \"\"` clears the query"), "{inherited}");
    }

    #[test]
    fn a_find_with_nothing_in_force_stays_quiet() {
        let mut st = a_workspace();
        let out = real::crm(&mut st, &["find"]).unwrap();
        assert!(out.contains("3 of 3 (page 1)") && !out.contains("filtered"), "{out}");
    }

    // -- set: erasing is its own act -----------------------------------------------------

    #[test]
    fn set_with_an_empty_value_is_refused_and_the_value_survives() {
        let mut st = a_workspace();
        let err = real::crm(&mut st, &["set", "acme-renewal", "value", ""]).unwrap_err();
        assert!(err.contains("would erase value"), "{err}");
        assert!(err.contains("`crm set acme-renewal value --clear`"), "it names the way to mean it: {err}");
        let shown = real::crm(&mut st, &["show", "acme-renewal"]).unwrap();
        assert!(shown.contains("45,000") || shown.contains("45000"), "the value must still be there: {shown}");
    }

    #[test]
    fn set_with_only_spaces_is_the_same_refusal() {
        let mut st = a_workspace();
        assert!(real::crm(&mut st, &["set", "acme-renewal", "value", "   "]).unwrap_err().contains("would erase"));
    }

    #[test]
    fn set_clear_erases_on_purpose() {
        let mut st = a_workspace();
        assert_eq!(real::crm(&mut st, &["set", "acme-renewal", "value", "--clear"]).unwrap(), "updated\n");
        let shown = real::crm(&mut st, &["show", "acme-renewal"]).unwrap();
        assert!(!shown.contains("45,000") && !shown.contains("45000"), "cleared: {shown}");
    }

    /// Every field, not only the one QA tripped: a domain or an email is as destructive.
    #[test]
    fn the_same_rule_holds_for_every_field() {
        let mut st = a_workspace();
        real::crm(&mut st, &["set", "acme-corp", "domain", "acme.com"]).unwrap();
        assert!(real::crm(&mut st, &["set", "acme-corp", "domain", ""]).unwrap_err().contains("would erase domain"));
        assert!(real::crm(&mut st, &["show", "acme-corp"]).unwrap().contains("acme.com"));
        real::crm(&mut st, &["set", "acme-corp", "domain", "--clear"]).unwrap();
        assert!(!real::crm(&mut st, &["show", "acme-corp"]).unwrap().contains("acme.com"));
    }

    #[test]
    fn set_clear_takes_no_value_and_a_bare_set_still_takes_three_words() {
        assert_eq!(
            ask_of(&["set", "acme", "value", "--clear"]),
            json!({ "cmd": "set", "handle": "acme", "field": "value", "clear": true })
        );
        let (msg, code) = refusal_for(&["set", "acme", "value", "1", "--clear"]);
        assert_eq!(code, exit::USAGE, "{msg}");
        assert!(msg.contains("takes no value"), "{msg}");
        assert_eq!(refusal_for(&["set", "acme", "--clear"]).1, exit::USAGE, "a field is still needed");
        assert_eq!(refusal_for(&["set", "acme", "value"]).1, exit::USAGE, "and so is a value, or --clear");
        assert_eq!(ask_of(&["set", "acme", "domain", "a.com"])["value"], "a.com");
    }

    /// The window's envelope: a person's clear is an explicit `clear`, never an empty commit.
    #[test]
    fn the_window_envelope_erases_only_when_it_says_clear() {
        let mut st = a_workspace();
        let deal = st.db().by_handle("acme-renewal").unwrap().1;
        let out = st.command(&json!({ "cmd": "set", "id": deal, "field": "value", "value": "" }), None, &real::ctx());
        assert_eq!(out.resp["ok"], false, "{:?}", out.resp);
        let out = st.command(&json!({ "cmd": "set", "id": deal, "field": "value", "clear": true }), None, &real::ctx());
        assert_eq!(out.resp["ok"], true, "{:?}", out.resp);
    }

    #[test]
    fn set_help_and_the_manual_say_how_to_erase() {
        assert_eq!(usage_lines("set").len(), 2);
        assert!(usage_lines("set")[1].contains("--clear"));
        assert!(verb_note("set").unwrap().contains("--clear"));
        for line in manual().lines() {
            assert!(line.chars().count() <= 80, "{} chars: {line}", line.chars().count());
        }
    }

    // -- the refusals that send an agent somewhere it can go ------------------------------

    /// `m2-cli.md` gives the bar in so many words: `try `crm find acme``. The old message
    /// sent an agent to a window it cannot see.
    #[test]
    fn a_handle_that_matches_nothing_points_at_find_not_at_the_window() {
        let mut st = a_workspace();
        let err = real::crm(&mut st, &["show", "zzqx"]).unwrap_err();
        assert!(err.contains("try `crm find zzqx`"), "{err}");
        assert!(!err.contains("window"), "an agent cannot see the window: {err}");
    }

    #[test]
    fn a_multi_word_miss_is_quoted_so_the_hint_can_be_pasted() {
        let mut st = a_workspace();
        let err = real::crm(&mut st, &["show", "no such thing"]).unwrap_err();
        assert!(err.contains("try `crm find \"no such thing\"`"), "{err}");
    }

    #[test]
    fn done_on_a_finished_next_step_is_refused_like_archive_on_an_archived_record() {
        let mut st = a_workspace();
        real::crm(&mut st, &["task", "acme-renewal", "Call Maya", "--due", "2026-10-01"]).unwrap();
        assert_eq!(real::crm(&mut st, &["done", "call-maya"]).unwrap(), "done\n");
        let before = st.db();
        let err = real::crm(&mut st, &["done", "call-maya"]).unwrap_err();
        assert!(err.contains("already done"), "{err}");
        assert_eq!(st.db(), before, "and the finish time was not overwritten");
    }

    #[test]
    fn a_bad_find_kind_names_all_because_the_grammar_accepts_it() {
        let mut st = a_workspace();
        let err = real::crm(&mut st, &["find", "--kind", "widget"]).unwrap_err();
        assert!(err.contains("company, contact, deal or all"), "{err}");
    }
}

// MARK: - Round 6b minors: export is atomic, and a closed pipe is not a panic

mod export_and_pipes {
    use super::*;
    use std::path::{Path, PathBuf};

    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("crm-export-{}-{nanos}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp."))
            .collect()
    }

    #[test]
    fn a_new_file_is_written_whole_and_leaves_nothing_beside_it() {
        let dir = scratch("new");
        let path = dir.join("companies.csv");
        write_file_atomically(&path, b"name\nAcme\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "name\nAcme\n");
        assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replacing_a_file_swaps_it_whole() {
        let dir = scratch("replace");
        let path = dir.join("c.csv");
        std::fs::write(&path, "old old old old old old\n").unwrap();
        write_file_atomically(&path, b"new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n", "no tail of the longer old file");
        assert!(leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The finding.** A write that fails part-way must leave the previous file exactly as it
    /// was — `fs::write` truncated it first, so a failure (or a kill) left a stub.
    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_the_previous_file_untouched_and_no_temp_behind() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("fail");
        let path = dir.join("c.csv");
        std::fs::write(&path, "precious\n").unwrap();
        // The directory refuses new files (what a full or read-only volume looks like).
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let refuses = std::fs::File::create(dir.join("probe")).is_err(); // root is not refused
        let result = write_file_atomically(&path, b"replacement\n");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        if refuses {
            // The write goes via a new file, so it is refused whole — an in-place write would
            // have happily truncated `c.csv` (it is writable) and left a stub on failure.
            assert!(result.is_err(), "it wrote anyway");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "precious\n", "the old export was damaged");
        }
        assert!(leftovers(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Works on any platform, root or not: a target whose directory is gone.
    #[test]
    fn a_missing_directory_is_an_error_not_something_to_create() {
        let dir = scratch("missing");
        let path = dir.join("no-such-dir").join("c.csv");
        let err = write_file_atomically(&path, b"x").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(!dir.join("no-such-dir").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_or_other_non_file_is_refused_not_renamed_over() {
        let dir = scratch("nonfile");
        let err = write_file_atomically(&dir, b"x").unwrap_err();
        assert!(err.to_string().contains("not a regular file"), "{err}");
        assert!(dir.is_dir(), "the directory must still be there");
        #[cfg(unix)]
        {
            let err = write_file_atomically(Path::new("/dev/null"), b"x").unwrap_err();
            assert!(err.to_string().contains("not a regular file"), "{err}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_followed_so_the_file_it_names_is_the_one_replaced() {
        let dir = scratch("link");
        let real = dir.join("real.csv");
        let link = dir.join("link.csv");
        std::fs::write(&real, "old\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_file_atomically(&link, b"new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new\n");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link must survive");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_files_permissions_are_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("mode");
        let path = dir.join("c.csv");
        std::fs::write(&path, "old\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_file_atomically(&path, b"new\n").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `finish_export` is the one caller, and keeps its report: rows, path, and the reason.
    #[test]
    fn finish_export_reports_what_it_wrote_and_why_it_could_not() {
        let dir = scratch("finish");
        let resp = json!({ "export": [[["name", "Acme"]], [["name", "Hooli"]]] });
        let path = dir.join("c.csv");
        let msg = finish_export(&resp, "companies", path.to_str(), "csv").unwrap();
        assert!(msg.starts_with("wrote 2 companies to "), "{msg}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "name\nAcme\nHooli\n");

        let bad = dir.join("nope").join("c.csv");
        let err = finish_export(&resp, "companies", bad.to_str(), "csv").unwrap_err();
        assert!(err.contains("cannot write") && err.contains("nope"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `print!` and `eprintln!` **panic** when the reader has gone. Nothing in the CLI may use
    /// them; `say` and `warn` are the way out. (Doc comments may name them.)
    #[test]
    fn the_cli_never_uses_a_macro_that_panics_on_a_closed_pipe() {
        let src = include_str!("cli.rs");
        let code: Vec<&str> = src
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && (t.contains("print!(") || t.contains("println!(") || t.contains("eprint!(") || t.contains("eprintln!("))
            })
            .collect();
        assert!(code.is_empty(), "these panic when stdout or stderr is a closed pipe: {code:#?}");
    }
}

// MARK: - p1: filters set in the window are named to the agent

mod p1_filters {
    use super::*;
    use crate::state::AppState;

    fn a_workspace() -> AppState {
        let mut st = AppState::new();
        for args in [&["add", "deal", "Acme renewal"][..], &["add", "deal", "Acme pilot"], &["add", "company", "Acme Corp"]] {
            real::crm(&mut st, args).unwrap();
        }
        st
    }

    /// The window set a saved view's filter (the CLI has no flag for one), and the agent ran
    /// `crm find`. It must say why the company is missing and how to get it back.
    #[test]
    fn a_filter_the_window_set_is_named_on_a_result_and_on_an_empty_one_and_in_status() {
        let mut st = a_workspace();
        let set = st.command(
            &json!({ "cmd": "find", "query": "", "status": "won", "touchedBy": "human" }),
            None,
            &real::ctx(),
        );
        assert_eq!(set.resp["ok"], true);

        let empty = real::crm(&mut st, &["find"]).unwrap();
        assert!(empty.starts_with("no results"), "{empty}");
        assert!(empty.contains("deals only, filtered by status won, last moved by a person (set in the window)"), "{empty}");
        assert!(empty.contains("`crm find --kind all` clears them"), "{empty}");

        let status = real::crm(&mut st, &["status"]).unwrap();
        assert!(status.contains("filters: deals only, filtered by status won"), "{status}");
    }

    #[test]
    fn the_hint_is_true_kind_all_clears_every_deal_filter() {
        let mut st = a_workspace();
        st.command(&json!({ "cmd": "find", "query": "", "stage": "proposal", "since": "2026-01-01" }), None, &real::ctx());
        let named = real::crm(&mut st, &["find"]).unwrap();
        assert!(named.contains("stage proposal") && named.contains("moved since 2026-01-01"), "{named}");

        let all = real::crm(&mut st, &["find", "--kind", "all"]).unwrap();
        assert!(all.contains("3 of 3"), "everything again: {all}");
        assert!(!all.contains("filtered by"), "{all}");
        assert!(!real::crm(&mut st, &["status"]).unwrap().contains("filters:"));
    }

    #[test]
    fn with_no_filter_in_force_nothing_is_said() {
        let mut st = a_workspace();
        let out = real::crm(&mut st, &["find"]).unwrap();
        assert!(out.contains("3 of 3") && !out.contains("deals only"), "{out}");
    }
}

// MARK: - p1 step 3: the CLI reads nothing from the retired shape

mod nothing_reads_the_old_shape {
    use super::*;
    use crate::state::AppState;

    /// A day of an agent's work, covering every verb that renders from a response. Run twice
    /// over identical workspaces: once as the core answers, once with every top-level key the
    /// old snapshot carried **deleted from the response first**. If any output differs, the CLI
    /// was still reading the old shape — and it fails here, loudly, instead of falling back.
    const A_DAY: &[&[&str]] = &[
        &["status"],
        &["add", "company", "Acme Corp"],
        &["add", "company", "Acme Industries"],
        &["add", "contact", "Ada Whitlock", "--company", "acme-corp"],
        &["add", "deal", "Acme renewal", "--company", "acme-corp", "--value", "45000"],
        &["status"],
        &["find"],
        &["find", "acme", "--kind", "deal"],
        &["find", "zzz"],
        &["find", "--kind", "all"],
        &["show", "acme-renewal"],
        &["board"],
        &["stages"],
        &["move", "acme-renewal", "proposal"],
        &["task", "acme-renewal", "Call Maya", "--due", "2026-12-01"],
        &["show", "acme-renewal"],
        &["due"],
        &["due", "--week"],
        &["set", "acme-corp", "domain", "acme.example"],
        &["log", "note", "acme-renewal", "rang"],
        &["link", "acme-renewal", "ada-whitlock"],
        &["done", "call-maya"],
        &["archive", "acme-industries"],
        &["archive", "acme-industries", "--restore"],
        &["log", "note", "acme", "which acme?"],
        &["select", "1"],
        &["show", "acme"],
        &["select", "2"],
        &["status"],
        &["export", "companies"],
    ];

    #[test]
    fn every_verb_prints_the_same_thing_with_the_retired_keys_removed_from_its_response() {
        let (mut a, mut b) = (AppState::new(), AppState::new());
        for line in A_DAY {
            // `export` writes a file in the CLI; the core's answer is all `render` reads.
            let (plain, stripped) = (real::crm_reading(&mut a, line, false), real::crm_reading(&mut b, line, true));
            assert_eq!(plain, stripped, "`crm {}` read a key the old shape carried", line.join(" "));
        }
    }

    /// The guard is not vacuous: it really does remove the keys, and a response with them gone
    /// would print something else if the CLI still looked for them.
    #[test]
    fn the_retired_keys_are_the_ones_the_core_still_attaches_today_and_the_cli_would_miss() {
        let mut st = AppState::new();
        real::crm(&mut st, &["add", "company", "Acme Corp"]).unwrap();
        let out = st.command(&json!({ "cmd": "status" }), Some("agent-1"), &real::ctx());
        let mut gone = out.resp.clone();
        gone.as_object_mut().unwrap().retain(|k, _| !real::RETIRED_KEYS.contains(&k.as_str()));
        gone.as_object_mut().unwrap().remove("split");
        assert!(status_lines(&gone).contains("nothing open"), "without the update the CLI has nothing to say");
        assert!(status_lines(&out.resp).contains("company acme-corp"), "with it, the CLI reads it");
    }
}

// MARK: - p2: identity is the window's, never the agent's

mod identity_is_not_the_agents {
    use super::*;
    use crate::account::tests::{friendly, scratch, INVITE, PASSWORD, TOKEN};
    use crate::account::Account;
    use crate::state::AppState;
    use identity::{Person, Secret};

    /// The words the CLI prints for each verb, from the core's real response, with a signed-in
    /// account holding a token, a password and an invite. None may appear.
    #[test]
    fn no_credential_reaches_what_the_cli_prints() {
        let path = scratch("cli-leak");
        let account = Account::open(path.clone(), Box::new(friendly()));
        account.accept_invite("https://x", &Secret::new(INVITE), "ada@example.com", &Secret::new(PASSWORD), "Ada").unwrap();
        account.ensure_agent("agent-1", "Scout");
        let mut st = AppState::new();
        st.set_account(account.identity(), &Person::default());
        st.set_workspace_agents(account.agent_ids());

        let mut printed = String::new();
        for line in [
            &["add", "company", "Acme Corp"][..],
            &["add", "deal", "Pilot", "--company", "acme-corp"],
            &["status"],
            &["find"],
            &["show", "pilot"],
            &["board"],
            &["stages"],
            &["due"],
            &["task", "pilot", "Call", "--due", "2026-12-01"],
            &["log", "note", "pilot", "hi"],
        ] {
            printed.push_str(&real::crm(&mut st, line).unwrap_or_else(|e| e));
        }
        printed.push_str(&manual());
        assert!(printed.len() > 500);
        for secret in [TOKEN, PASSWORD, INVITE, "SECRET"] {
            assert!(!printed.contains(secret), "`{secret}` reached the CLI's output");
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Sign-in, sign-up, sign-out, invites: none is a verb, none is in the manual, none is an
    /// envelope the core answers — so none is reachable from the agent's socket either.
    #[test]
    fn the_cli_has_no_way_to_sign_in_out_or_touch_a_credential() {
        for word in ["sign-in", "signin", "sign-up", "sign-out", "login", "logout", "account", "invite", "token", "session", "password"] {
            match plan(&argv(&[word])) {
                Plan::Refuse(msg, code) => assert_eq!(code, exit::USAGE, "{word}: {msg}"),
                other => panic!("`crm {word}` is a verb: {other:?}"),
            }
        }
        let manual = manual().to_lowercase();
        for word in ["password", "sign in", "sign-in", "token", "credential"] {
            assert!(!manual.contains(word), "the manual mentions `{word}` — the agent's only manual must not");
        }
        let mut st = AppState::new();
        for cmd in ["sign_in", "signin", "sign_up", "sign_out", "account", "account_status", "create_invite", "token", "session", "login", "logout"] {
            let out = st.command(&json!({ "cmd": cmd, "email": "a@b.c", "password": "x" }), Some("agent-1"), &real::ctx());
            assert_eq!(out.resp["ok"], false, "`{cmd}` is an envelope the core answers");
        }
    }
}

// MARK: - W1 part A: every call an agent makes is a line in the feed

mod w1_feed {
    use super::*;
    use crate::model::{ActivityKind, Date, Kind};
    use crate::state::AppState;

    /// The records every verb below acts on, made through the core's own API so **no feed entry**
    /// exists before the call under test.
    fn a_workspace() -> AppState {
        let mut st = AppState::new();
        let ctx = real::ctx();
        let acme = st.add_company("Acme Corp", &ctx);
        st.add_company("Hooli", &ctx);
        st.add_contact("Ada Whitlock", Some(&acme), &ctx);
        let deal = st.add_deal("Acme renewal", Some(&acme), None, None, &ctx);
        st.add_task("Call back", Date::new(2027, 1, 1), vec![deal.clone()], None, &ctx);
        st.log(ActivityKind::Note, "seed", vec![deal], None, &ctx);
        st
    }

    fn feed(st: &AppState) -> Vec<Value> {
        st.update(real::ctx().now, &[])["view"]["feed"].as_array().unwrap().clone()
    }

    /// (the command line as the agent types it, the line the feed shows, the record it is about)
    type Expect = (&'static str, &'static [&'static str], &'static str, Option<(&'static str, &'static str)>);
    const EVERY_VERB: &[Expect] = &[
        ("find", &["find", "acme", "--kind", "deal"], "find acme --kind deal", None),
        ("show", &["show", "acme-renewal"], "show acme-renewal", Some(("deal", "acme-renewal"))),
        ("board", &["board", "--stage", "proposal"], "board --stage proposal", None),
        ("stages", &["stages"], "stages", None),
        ("due", &["due"], "due", None),
        ("status", &["status"], "status", None),
        ("export", &["export", "companies"], "export companies", None),
        ("add", &["add", "company", "Initech", "--domain", "initech.example"], "add company Initech --domain initech.example", Some(("company", "initech"))),
        ("set", &["set", "acme-corp", "domain", "acme.example"], "set acme-corp domain acme.example", Some(("company", "acme-corp"))),
        ("log", &["log", "note", "acme-renewal", "hello there"], "log note acme-renewal 'hello there'", Some(("deal", "acme-renewal"))),
        ("move", &["move", "acme-renewal", "proposal"], "move acme-renewal proposal", Some(("deal", "acme-renewal"))),
        ("task", &["task", "acme-renewal", "Call Maya", "--due", "2027-02-01"], "task acme-renewal 'Call Maya' --due 2027-02-01", Some(("deal", "acme-renewal"))),
        ("done", &["done", "call-back"], "done call-back", Some(("deal", "acme-renewal"))),
        ("link", &["link", "acme-renewal", "ada-whitlock"], "link acme-renewal ada-whitlock", Some(("deal", "acme-renewal"))),
        ("archive", &["archive", "hooli"], "archive hooli", Some(("company", "hooli"))),
    ];

    /// **The sweep**: every verb in `crm -h`, one call, exactly one entry, with the right verb, line,
    /// outcome and target.
    #[test]
    fn every_verb_in_crm_h_is_exactly_one_line_in_the_feed() {
        let mut covered: Vec<&str> = EVERY_VERB.iter().map(|v| v.0).collect();
        covered.extend(["select", "import"]); // below: they need a parked question / parsed rows
        let mut declared: Vec<&str> = VERBS.iter().map(|(n, _)| *n).filter(|n| !["focus", "close"].contains(n)).collect();
        covered.sort();
        declared.sort();
        assert_eq!(covered, declared, "a verb in `crm -h` has no feed test (focus and close never reach the app)");

        for (verb, line, shown, target) in EVERY_VERB {
            let mut st = a_workspace();
            assert!(feed(&st).is_empty());
            real::crm(&mut st, line).unwrap_or_else(|e| panic!("`{}` was refused: {e}", line.join(" ")));
            let f = feed(&st);
            assert_eq!(f.len(), 1, "`{verb}` produced {} entries", f.len());
            let e = &f[0];
            assert_eq!(e["verb"], *verb);
            assert_eq!(e["line"], *shown, "the line must be what the agent typed, in handles");
            assert_eq!(e["outcome"], "ok");
            assert_eq!(e["actor"], json!({ "kind": "agent", "id": "agent-1" }));
            match target {
                Some((kind, handle)) => {
                    assert_eq!(e["target"]["kind"], *kind, "{verb}");
                    assert_eq!(e["target"]["handle"], *handle, "{verb}");
                    assert!(e["target"]["id"].as_str().is_some(), "an id, so the window can open it");
                }
                None => assert!(e.get("target").is_none(), "`{verb}` is about no one record"),
            }
        }

        // select: answers a parked question, and says which record it opened.
        let mut st = a_workspace();
        st.add_company("Acme Industries", &real::ctx());
        real::crm(&mut st, &["show", "acme"]).unwrap();
        let before = feed(&st).len();
        real::crm(&mut st, &["select", "1"]).unwrap();
        let f = feed(&st);
        assert_eq!(f.len(), before + 1);
        assert_eq!((f[0]["verb"].as_str(), f[0]["line"].as_str(), f[0]["outcome"].as_str()), (Some("select"), Some("select 1"), Some("ok")));
        assert_eq!(f[0]["target"]["kind"], "company");

        // import: the CLI reads the file; the core is handed rows. One line, naming how many.
        let mut st = a_workspace();
        let out = st.command(&json!({ "cmd": "import", "kind": "contact", "rows": [{ "name": "Ada" }, { "name": "Bob" }] }), Some("agent-1"), &real::ctx());
        assert_eq!(out.resp["ok"], true);
        let f = feed(&st);
        assert_eq!((f.len(), f[0]["line"].as_str(), f[0]["outcome"].as_str()), (1, Some("import contacts (2 rows)"), Some("ok")));
        assert!(f[0].get("target").is_none());
    }

    #[test]
    fn a_refused_call_is_a_refused_line_carrying_the_exact_words_the_agent_saw() {
        let mut st = a_workspace();
        let said = real::crm(&mut st, &["move", "acme-renewal", "nonsense"]).unwrap_err();
        let said_missing = real::crm(&mut st, &["show", "no-such-record"]).unwrap_err();
        let f = feed(&st);
        assert_eq!(f.len(), 2);
        assert_eq!(f[1]["outcome"], "refused");
        assert_eq!(f[1]["message"], said.as_str(), "verbatim");
        assert_eq!(f[1]["line"], "move acme-renewal nonsense");
        assert_eq!(f[1]["target"]["handle"], "acme-renewal", "a refusal about a real record can open it");
        assert_eq!(f[0]["outcome"], "refused");
        assert_eq!(f[0]["message"], said_missing.as_str());
        assert!(f[0].get("target").is_none(), "a record that does not exist cannot be opened");
        assert!(f[0]["message"].as_str().unwrap().contains("no-such-record"));
    }

    #[test]
    fn an_ambiguity_is_an_asked_line_with_no_target() {
        let mut st = a_workspace();
        st.add_company("Acme Industries", &real::ctx());
        real::crm(&mut st, &["log", "note", "acme", "which acme?"]).unwrap();
        let f = feed(&st);
        assert_eq!((f.len(), f[0]["outcome"].as_str(), f[0]["verb"].as_str()), (1, Some("asked"), Some("log")));
        assert!(f[0].get("target").is_none() && f[0].get("message").is_none());
    }

    #[test]
    fn a_window_action_is_no_entry_and_an_agents_call_still_signals_nothing() {
        let mut st = a_workspace();
        let person = st.command(&json!({ "cmd": "add", "kind": "company", "name": "Person Co" }), None, &real::ctx());
        assert_eq!(person.resp["ok"], true);
        assert!(!person.emits.is_empty(), "the person's write signals, as it always did");
        assert!(feed(&st).is_empty(), "the window does not log itself to itself");

        let agent = st.command(&json!({ "cmd": "add", "kind": "company", "name": "Agent Co" }), Some("agent-1"), &real::ctx());
        assert!(agent.emits.is_empty(), "the agent is never told about its own calls");
        assert_eq!(feed(&st).len(), 1);
        // The window's own envelopes are not an agent's verbs, even if an agent sends them.
        st.command(&json!({ "cmd": "search", "q": "a" }), Some("agent-1"), &real::ctx());
        st.command(&json!({ "cmd": "bootstrap" }), Some("agent-1"), &real::ctx());
        assert_eq!(feed(&st).len(), 1);
    }

    #[test]
    fn follow_on_moves_the_focus_to_what_the_agent_touched_and_follow_off_does_not() {
        let mut st = a_workspace();
        assert_eq!(st.update(real::ctx().now, &[])["view"]["follow"], true, "on by default");
        real::crm(&mut st, &["show", "acme-corp"]).unwrap();
        real::crm(&mut st, &["log", "note", "acme-renewal", "followed"]).unwrap();
        assert_eq!(seen_focus(&st), "acme-renewal", "follow on: the focus went where the agent went");

        let off = st.command(&json!({ "cmd": "follow", "on": false }), None, &real::ctx());
        assert_eq!(off.resp["ok"], true);
        real::crm(&mut st, &["show", "acme-corp"]).unwrap(); // show still moves focus, as it always did
        real::crm(&mut st, &["log", "note", "acme-renewal", "not followed"]).unwrap();
        assert_eq!(seen_focus(&st), "acme-corp", "follow off: a write elsewhere leaves the focus alone");
        assert_eq!(feed(&st)[0]["target"]["handle"], "acme-renewal", "…but it is still a line, one click away");

        // It is the person's choice: an agent cannot flip it.
        let by_agent = st.command(&json!({ "cmd": "follow", "on": true }), Some("agent-1"), &real::ctx());
        assert_eq!(by_agent.resp["ok"], false);
        assert_eq!(st.update(real::ctx().now, &[])["view"]["follow"], false);
    }

    fn seen_focus(st: &AppState) -> String {
        st.update(real::ctx().now, &[])["view"]["focus"]["handle"].as_str().unwrap_or("").to_string()
    }

    #[test]
    fn follow_never_emits_a_signal_to_the_agent_about_its_own_action() {
        let mut st = a_workspace();
        let out = st.command(&json!({ "cmd": "log", "kind": "note", "handle": "acme-renewal", "body": "x" }), Some("agent-1"), &real::ctx());
        assert!(out.emits.is_empty(), "following is done in the core, not by a window `show` that would emit deal.opened");
        assert_eq!(seen_focus(&st), "acme-renewal");
    }

    #[test]
    fn page_reads_send_the_window_to_their_page_and_the_same_page_twice_is_a_new_instruction() {
        let mut st = a_workspace();
        let page = |st: &AppState| st.update(real::ctx().now, &[])["view"]["page"].clone();
        assert!(page(&st).is_null());
        real::crm(&mut st, &["board"]).unwrap();
        let first = page(&st);
        assert_eq!(first["name"], "pipeline");
        real::crm(&mut st, &["board"]).unwrap();
        assert_eq!(page(&st)["name"], "pipeline");
        assert!(page(&st)["seq"].as_u64() > first["seq"].as_u64(), "so the window navigates again");
        real::crm(&mut st, &["due"]).unwrap();
        assert_eq!(page(&st)["name"], "inbox");
        real::crm(&mut st, &["find", "acme", "--kind", "deal"]).unwrap();
        assert_eq!(page(&st)["name"], "deals");
        real::crm(&mut st, &["find", "ada", "--kind", "contact"]).unwrap();
        assert_eq!(page(&st)["name"], "contacts");
        real::crm(&mut st, &["find", "--kind", "company"]).unwrap();
        assert_eq!(page(&st)["name"], "contacts", "companies live on the contacts-and-companies page");

        st.command(&json!({ "cmd": "follow", "on": false }), None, &real::ctx());
        let before = page(&st);
        real::crm(&mut st, &["board"]).unwrap();
        assert_eq!(page(&st), before, "follow off: the window stays where the person is");
        assert_eq!(feed(&st)[0]["verb"], "board", "…and the line is still there");
    }

    #[test]
    fn no_id_and_no_secret_is_ever_in_a_line_even_when_the_caller_sent_ids_and_junk() {
        let mut st = a_workspace();
        for line in EVERY_VERB.iter().map(|v| v.1) {
            let _ = real::crm(&mut st, line);
        }
        // A raw-socket caller that names records by id, and tacks on a field the contract does not have.
        let id = st.db().by_handle("acme-renewal").unwrap().1;
        let other = st.db().by_handle("ada-whitlock").unwrap().1;
        st.command(&json!({ "cmd": "move", "id": id, "to": "won", "token": "sekret-token-123" }), Some("agent-1"), &real::ctx());
        st.command(&json!({ "cmd": "link", "id": id, "toId": other, "password": "hunter2hunter2" }), Some("agent-1"), &real::ctx());
        st.command(&json!({ "cmd": "set", "id": id, "field": "value", "value": "1", "authorization": "Bearer abc" }), Some("agent-1"), &real::ctx());
        let f = feed(&st);
        assert!(f.len() > 15);
        for e in &f {
            let line = e["line"].as_str().unwrap();
            assert!(first_ulid_in(line).is_none(), "an id in a line: {line}");
            for secret in ["sekret", "hunter2", "Bearer", "abc"] {
                assert!(!line.contains(secret) && !e.get("message").is_some_and(|m| m.as_str().unwrap().contains(secret)), "`{secret}` reached the feed: {line}");
            }
        }
        assert!(f.iter().any(|e| e["line"] == "move acme-renewal won"), "an id is printed as the handle it names");
        assert!(f.iter().any(|e| e["line"] == "link acme-renewal ada-whitlock"));
    }

    #[test]
    fn the_ring_holds_a_hundred_newest_first_and_seq_keeps_climbing_across_the_wrap() {
        let mut st = a_workspace();
        for _ in 0..130 {
            real::crm(&mut st, &["status"]).unwrap();
        }
        let f = feed(&st);
        assert_eq!(f.len(), 100);
        let seqs: Vec<u64> = f.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
        assert_eq!(seqs[0], 130, "newest first");
        assert_eq!(*seqs.last().unwrap(), 31, "the oldest 30 fell off");
        assert!(seqs.windows(2).all(|w| w[0] == w[1] + 1), "monotonic, no gaps, across the wrap");
        real::crm(&mut st, &["status"]).unwrap();
        assert_eq!(feed(&st)[0]["seq"], 131);
    }

    /// Not persisted: it is view state that lives while the app runs.
    #[test]
    fn the_feed_is_not_persisted() {
        let mut st = a_workspace();
        real::crm(&mut st, &["show", "acme-renewal"]).unwrap();
        let text = serde_json::to_string(&st.db()).unwrap();
        assert!(!text.contains("show acme-renewal") && !text.contains("\"outcome\""), "the feed reached the data file");
        let back = AppState::with_db(serde_json::from_str(&text).unwrap());
        assert!(feed(&back).is_empty(), "a restart empties it");
        assert_eq!(back.update(real::ctx().now, &[])["view"]["follow"], true, "…but the person's follow choice is theirs and stays");
    }

    #[test]
    fn crm_h_tells_the_agent_it_is_being_watched() {
        let m = manual();
        assert!(m.contains("Everything you run shows, step by step, in the person's window."), "{m}");
        for line in m.lines() {
            assert!(line.chars().count() <= 80, "{} chars: {line}", line.chars().count());
        }
        let _ = Kind::Deal;
    }

    fn rows(st: &AppState) -> (Vec<String>, u64) {
        let l = st.update(real::ctx().now, &[])["view"]["list"].clone();
        let handles = l["rows"].as_array().unwrap().iter().map(|r| r["handle"].as_str().unwrap().to_string()).collect();
        (handles, l["total"].as_u64().unwrap())
    }

    #[test]
    fn a_fresh_dataset_with_no_query_shows_every_record_not_zero() {
        let mut st = AppState::new();
        assert_eq!(rows(&st).1, 0);
        real::crm(&mut st, &["add", "company", "Acme Corp"]).unwrap();
        real::crm(&mut st, &["add", "company", "Zed Ltd"]).unwrap();
        // The list was never searched; it is still the whole of what exists.
        let (handles, total) = rows(&st);
        assert_eq!(total, 2);
        assert_eq!(handles.len(), 2);
        let seeded = a_workspace();
        assert_eq!(rows(&seeded).1 as usize, seeded.update(real::ctx().now, &[])["view"]["list"]["rows"].as_array().unwrap().len().max(rows(&seeded).1 as usize));
        assert!(rows(&seeded).1 > 0, "a dataset nobody has searched is not \"0 total\"");
    }

    #[test]
    fn the_shared_list_is_live_an_add_a_rename_an_archive_and_a_restore_show_in_the_next_update() {
        let mut st = a_workspace();
        real::crm(&mut st, &["find", "quill"]).unwrap();
        assert_eq!(rows(&st), (vec![], 0));
        real::crm(&mut st, &["add", "contact", "Pia Quill"]).unwrap();
        let (handles, total) = rows(&st);
        assert_eq!(total, 1, "{handles:?}");
        let handle = handles[0].clone();
        // The person's own write, not only the agent's.
        st.command(&json!({ "cmd": "add", "kind": "contact", "name": "Quill Two" }), None, &real::ctx());
        assert_eq!(rows(&st).1, 2);
        real::crm(&mut st, &["archive", &handle]).unwrap();
        assert_eq!(rows(&st).1, 1, "archived leaves the list");
        real::crm(&mut st, &["archive", &handle, "--restore"]).unwrap();
        assert_eq!(rows(&st).1, 2, "restored comes back");
        assert_eq!(st.db().view.list.results.len(), 2, "stored in the same commit, not only drawn");
        real::crm(&mut st, &["find", "zedq"]).unwrap();
        assert_eq!(rows(&st).1, 0);
        real::crm(&mut st, &["set", &handle, "name", "Pia Zedq"]).unwrap();
        assert_eq!(rows(&st).1, 1, "renamed into the query");
    }

    #[test]
    fn a_call_with_a_target_lands_on_the_page_where_the_record_lives() {
        let mut st = a_workspace();
        let page = |st: &AppState| st.update(real::ctx().now, &[])["view"]["page"]["name"].clone();
        real::crm(&mut st, &["add", "contact", "Pia Quill"]).unwrap();
        assert_eq!(page(&st), "contacts");
        real::crm(&mut st, &["log", "note", "acme-renewal", "x"]).unwrap();
        assert_eq!(page(&st), "pipeline", "a deal lives on the pipeline");
        real::crm(&mut st, &["set", "acme-corp", "domain", "acme.io"]).unwrap();
        assert_eq!(page(&st), "contacts", "a company is on the contacts-and-companies page");
        st.command(&json!({ "cmd": "follow", "on": false }), None, &real::ctx());
        let before = page(&st);
        real::crm(&mut st, &["log", "note", "acme-renewal", "y"]).unwrap();
        assert_eq!(page(&st), before, "follow off: the window stays put");
    }
}

// MARK: - R9: edit a next step; an ambiguity's list header

mod r9_small_fixes {
    use super::*;
    use crate::state::AppState;

    fn workspace() -> AppState {
        let mut st = AppState::new();
        real::crm(&mut st, &["add", "company", "Acme Corp"]).unwrap();
        real::crm(&mut st, &["task", "acme-corp", "Call back", "--due", "2026-09-30"]).unwrap();
        st
    }

    fn task(st: &AppState) -> crate::model::Task {
        st.db().tasks[0].clone()
    }

    #[test]
    fn crm_set_edits_a_next_steps_due_and_what_and_its_handle_stays() {
        let mut st = workspace();
        let handle = task(&st).handle;
        assert_eq!(handle, "call-back");
        real::crm(&mut st, &["set", &handle, "due", "2026-10-15"]).unwrap();
        assert_eq!(task(&st).due.to_string_iso(), "2026-10-15");
        real::crm(&mut st, &["set", &handle, "what", "Call back about pricing"]).unwrap();
        let t = task(&st);
        assert_eq!(t.what, "Call back about pricing");
        assert_eq!(t.handle, "call-back", "`crm done` must still find it");
        real::crm(&mut st, &["done", &handle]).unwrap();
    }

    #[test]
    fn a_next_step_edit_says_what_it_refuses() {
        let mut st = workspace();
        for (args, want) in [
            (&["set", "call-back", "due", "soon"][..], "not a date"),
            (&["set", "call-back", "colour", "red"][..], "you can set: due, what"),
            (&["set", "call-back", "what", ""][..], "would erase"),
        ] {
            let err = real::crm(&mut st, args).unwrap_err();
            assert!(format!("{err:?}").contains(want), "{args:?}: {err:?}");
        }
        assert_eq!(task(&st).what, "Call back", "a refused edit changes nothing");
        real::crm(&mut st, &["done", "call-back"]).unwrap();
        let err = real::crm(&mut st, &["set", "call-back", "due", "2026-11-01"]).unwrap_err();
        assert!(format!("{err:?}").contains("already done"), "{err:?}");
    }

    #[test]
    fn the_window_edits_a_next_step_by_id_and_a_new_due_date_is_told_afresh() {
        let mut st = workspace();
        let id = task(&st).id;
        let out = st.command(&json!({ "cmd": "set", "id": id, "field": "due", "value": "2026-10-20" }), None, &real::ctx());
        assert_eq!(out.resp["ok"], true, "{}", out.resp);
        assert_eq!(task(&st).due.to_string_iso(), "2026-10-20");
        assert!(task(&st).due_sent_at.is_none(), "the reminder is about the new date");
        // An agent moving it to a day already past is not woken about its own write.
        let out = st.command(&json!({ "cmd": "set", "handle": "call-back", "field": "due", "value": "2000-01-01" }), Some("agent-1"), &real::ctx());
        assert_eq!(out.resp["ok"], true);
        assert!(task(&st).due_sent_at.is_some());
        assert!(out.emits.is_empty());
    }

    #[test]
    fn show_of_an_ambiguous_word_leaves_the_lists_header_agreeing_with_its_rows() {
        let mut st = AppState::new();
        for args in [
            &["add", "company", "Acme Corp"][..],
            &["add", "company", "Acme Inc"],
            &["add", "deal", "Acme renewal"],
            &["add", "deal", "Acme pilot"],
        ] {
            real::crm(&mut st, args).unwrap();
        }
        real::crm(&mut st, &["find", "--kind", "deal"]).unwrap();
        let out = real::crm(&mut st, &["show", "acme"]).unwrap();
        assert!(out.contains("More than one record matches"), "{out}");
        let list = st.update(real::ctx().now, &[])["view"]["list"].clone();
        assert!(list["kind"].is_null(), "rows of several kinds under a `deal` header: {list}");
        assert_eq!(list["query"], "acme");
        let kinds: std::collections::BTreeSet<&str> =
            list["rows"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap()).collect();
        assert!(kinds.len() > 1, "{kinds:?}");
        let status = real::crm(&mut st, &["status"]).unwrap();
        assert!(!status.contains("kind deal"), "{status}");
    }
}

// MARK: - p3: the link to the team's server, and what a write was finally called

mod p3_sync_words {
    use super::*;

    fn snap(sync: Value) -> Value {
        json!({ "split": { "view": {}, "seat": { "sync": sync }, "derived": { "counts": {} } } })
    }

    #[test]
    fn status_says_how_the_link_to_the_team_is_and_says_nothing_in_local_mode() {
        let now = 1_000_000_000_000_i64;
        let line = |s: Value| status_lines_at(&snap(s), now, 0);
        assert!(!line(json!({ "state": "local", "lastSeen": null })).contains("sync:"), "local mode prints what it always printed");
        assert!(line(json!({ "state": "online", "lastSeen": now - 3000 })).contains("sync: online (last heard just now)"));
        assert!(line(json!({ "state": "syncing", "lastSeen": null })).contains("sync: catching up"));
        let off = line(json!({ "state": "offline", "lastSeen": null }));
        assert!(off.contains("sync: offline — reads work, writes are refused"), "{off}");
    }

    #[test]
    fn add_prints_what_the_record_was_finally_called() {
        let resp = json!({ "ok": true, "handles": [{ "kind": "company", "proposed": "acme", "final": "acme-2" }], "split": { "view": {} } });
        assert_eq!(confirm_add(&resp), "added company acme-2\n");
        let local = json!({ "ok": true, "split": { "view": { "focused": { "row": { "kind": "deal", "handle": "pilot" } } } } });
        assert_eq!(confirm_add(&local), "added deal pilot\n", "an answer with no handles still reads the focused row");
    }
}
