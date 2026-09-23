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
    let resp = json!({ "pipeline": { "stages": ["lead", "qualified", "proposal", "negotiation"] } });
    assert_eq!(stages_lines(&resp), "lead\nqualified\nproposal\nnegotiation\n");
}

#[test]
fn due_only_flags_are_local_and_filter_what_is_printed() {
    let req = ask_of(&["due", "--overdue"]);
    let mut wire = req.clone();
    wire.as_object_mut().unwrap().retain(|k, _| !k.starts_with("__"));
    assert_eq!(wire, json!({ "cmd": "due" }), "the core answers with all three either way");

    let resp = json!({ "due": { "overdue": 2, "today": 1, "week": 5 } });
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

    let (msg, code) = refusal_for(&["import", dir.join("missing.csv").to_str().unwrap()]);
    assert_eq!(code, exit::USAGE, "{msg}");
    assert!(msg.contains("cannot read"), "{msg}");
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

// MARK: - Rendering: the bug a live run caught, and the guard that would have too

/// A snapshot with real ULIDs everywhere an id could leak — every render function below
/// is run against this once.
fn a_snapshot() -> Value {
    json!({
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
    })
}

fn an_ambiguous_snapshot() -> Value {
    an_ambiguous_snapshot_resuming(false)
}

/// `resuming: true` is what a write verb's ambiguity carries — `crm select N` completes
/// the interrupted write, and `crm show <handle>` will not.
fn an_ambiguous_snapshot_resuming(resuming: bool) -> Value {
    json!({
        "ok": true, "rev": 10, "answer": "ambiguous", "focus": null,
        "pending": { "prompt": "which “acme”?", "resuming": resuming, "candidates": [
            { "kind": "company", "id": an_id(1), "handle": "acme-corp", "label": "Acme Corp" },
            { "kind": "deal", "id": an_id(2), "handle": "acme-renewal-2", "label": "Acme renewal" },
        ]},
    })
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
    let golden: Value = serde_json::from_str(include_str!("../fixtures/snapshot.json")).unwrap();
    let pending: Value = serde_json::from_str(include_str!("../fixtures/snapshot-pending.json")).unwrap();
    for snap in [golden, pending] {
        for (verb, _) in VERBS {
            let out = render(verb, &a_request(verb), &snap);
            assert!(first_ulid_in(&out).is_none(), "`{verb}` printed an id from the golden fixture:\n{out}");
        }
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
    let snap = json!({ "focus": { "kind": "deal", "id": an_id(5) }, "counts": {}, "agents": [] });
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
    filtered_empty["list"]["kind"] = json!("contact");
    filtered_empty["list"]["rows"] = json!([]);
    filtered_empty["list"]["total"] = json!(0);
    let out = find_lines(&filtered_empty, None);
    assert!(out.contains("no results"), "{out}");
    assert!(out.contains("filtered to contact"), "{out}");
    assert!(out.contains("crm find --kind all"), "{out}");

    let mut filtered_hit = a_snapshot();
    filtered_hit["list"]["kind"] = json!("deal");
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
    snap["list"] = json!({ "query": "acme", "kind": "contact", "sort": "name", "page": 1, "total": 7 });
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
    snap["list"] = json!({ "query": "", "kind": null, "sort": "updated", "page": 0, "total": 0 });
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
        let only = take_str_array(&mut req.clone(), "__only");
        Ok(match verb.as_str() {
            "due" => due_lines(&out.resp, &only),
            "find" => find_lines(&out.resp, None),
            _ => render(&verb, &wire, &out.resp),
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
    fn status_says_when_it_last_checked_and_when_it_last_sent() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        let sweep = st.sweep(&ctx_at(3, 0));
        let now = ctx_at(3, 7).at();
        let out = status_lines_at(&sweep.snapshot, now);
        assert!(out.contains("reminders: checked 7 min ago, every 5 min; last sent 7 min ago (1 next step)"), "{out}");
        assert!(!out.contains("REFUSED") && !out.contains("waiting"), "nothing is wrong: {out}");
    }

    /// The caveat is on the surface the agent actually reads, and it is the real behaviour.
    #[test]
    fn status_always_says_reminders_only_fire_while_the_app_runs() {
        let st = state_with_a_task_due_on("2026-09-10");
        let out = status_lines_at(&st.snapshot(ctx_at(0, 0).now), ctx_at(0, 0).at());
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
        let out = status_lines_at(&refused.snapshot, ctx_at(3, 1).at());
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
        assert!(status_lines_at(&a.snapshot, 0).contains("its context queue is full"));
        let b = st.note_refusal("task.due", SCOUT, "something_new", &ctx_at(3, 0));
        assert!(status_lines_at(&b.snapshot, 0).contains("it said something_new"));
    }

    #[test]
    fn due_tasks_held_for_want_of_an_agent_are_said_to_be_waiting() {
        let mut st = state_with_a_task_due_on("2026-09-10");
        st.set_agents(Vec::new());
        let sweep = st.sweep(&ctx_at(3, 0));
        let out = status_lines_at(&sweep.snapshot, ctx_at(3, 0).at());
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
            t.as_object_mut().unwrap().remove("dueSignalledAt");
        }
        json.as_object_mut().unwrap().remove("reminders");
        let (old, _) = AppState::open(serde_json::from_value(json).unwrap(), &ctx_at(0, 0));
        let out = status_lines_at(&old.snapshot(ctx_at(0, 0).now), ctx_at(0, 0).at());
        assert!(out.contains("reminders backlog: 1 next step was already overdue when reminders began"), "{out}");
        assert!(out.contains("nobody was woken for it") && out.contains("`crm due`"), "{out}");
        assert!(!out.contains("waiting"), "and nothing is queued to fire: {out}");
    }

    #[test]
    fn a_snapshot_from_before_the_timer_prints_nothing_about_it() {
        let out = status_lines_at(&json!({ "counts": {}, "agents": [] }), 0);
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
        assert!(m.contains("only checks while it runs"), "{m}");
        for line in m.lines() {
            assert!(line.chars().count() <= 80, "{} chars: {line}", line.chars().count());
        }
    }
}
