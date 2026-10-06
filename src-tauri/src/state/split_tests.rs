//! p1: the split, as tests. The shape is described in `docs/split-contract.md`; these pin it.
//!
//! Four things are pinned here and nowhere else:
//! * the two shapes **cannot disagree** about the same state;
//! * view state never produces a row action, and row actions are numbered by a watermark that
//!   is not the local `rev`;
//! * the reads that serve one surface (`search`, `feed`, `tasks`) change nothing at all;
//! * `org_id`, `owner_id` and `me` are recorded and survive the store, though nothing reads
//!   them yet.

use super::*;
use crate::state::tests::{agent_row, at, check_golden, ctx, flatten, golden_pending_state, golden_state, now, run, seen, state, AGENT};

fn at_min(minutes: i64) -> Ctx {
    let n = now();
    at(Now { at: n.at + minutes * 60_000, today: n.today })
}

/// The split's fixtures live in `fixtures/split/`, **not** beside the classic ones: the window's
/// preview loads every `fixtures/*.json` as a classic snapshot, and a split message dropped
/// there renders as a broken scenario. (Found the hard way: it failed two window tests.)
fn fixture(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A message as a file: `rev` is a process-wide counter and would differ between runs, so it
/// is pinned. Everything else is exactly what the core produced.
fn golden(mut v: Value) -> String {
    v["rev"] = json!(1);
    serde_json::to_string_pretty(&v).unwrap() + "\n"
}

/// The golden workspace after one more stage change, and the update that change produced.
fn golden_after_a_move() -> (AppState, Value) {
    let mut st = golden_state();
    let before = st.db();
    let deal = st.db().by_handle("acme-pilot").unwrap().1;
    let out = st.command(
        &json!({ "cmd": "move", "id": deal, "to": "qualified" }),
        Some(AGENT),
        &at_min(60),
    );
    assert_eq!(out.resp["ok"], true, "{:?}", out.resp);
    let ops = st.stamp(&before);
    let update = st.update(now(), &ops);
    (st, update)
}

// MARK: - Golden fixtures: the new shape, beside the old

#[test]
fn golden_split_bootstrap_matches_the_committed_fixture() {
    check_golden(&fixture("split/bootstrap.json"), &golden(golden_state().bootstrap(now())));
}

#[test]
fn golden_split_bootstrap_with_a_parked_question_matches_the_committed_fixture() {
    check_golden(&fixture("split/bootstrap-pending.json"), &golden(golden_pending_state().bootstrap(now())));
}

#[test]
fn golden_split_update_after_a_move_matches_the_committed_fixture() {
    check_golden(&fixture("split/update-move.json"), &golden(golden_after_a_move().1));
}

/// What the window is handed must carry ids only under keys it never displays, in the new
/// shape as in the old: a ULID may be a key and may sit in `id`/`…Id`/`…Ids`/`links`, and is
/// never prose.
#[test]
fn the_split_keeps_ids_under_keys_that_are_never_displayed() {
    fn walk(v: &Value, key: &str, bad: &mut Vec<String>) {
        match v {
            Value::String(s) => {
                let is_id = crate::model::Ulid::parse(s).is_some();
                let ok_key = key == "id" || key.ends_with("Id") || key.ends_with("Ids") || key == "links" || key == "dealIds";
                if is_id && !ok_key {
                    bad.push(format!("{key} = {s}"));
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, key, bad)),
            Value::Object(o) => o.iter().for_each(|(k, x)| walk(x, k, bad)),
            _ => {}
        }
    }
    let (st, update) = golden_after_a_move();
    for msg in [st.bootstrap(now()), update, golden_pending_state().bootstrap(now())] {
        let mut bad = Vec::new();
        // The object keys of `cards` and the `cards`' children are ids by design.
        walk(&msg, "", &mut bad);
        assert!(bad.is_empty(), "an id outside an id key: {bad:?}");
    }
}

#[test]
fn nothing_secret_and_no_install_identity_is_in_the_split() {
    let text = golden_state().bootstrap(now()).to_string();
    assert!(!text.contains("\"origin\""), "origin is this install's, not the org's");
    assert!(!text.to_lowercase().contains("token"));
}

// MARK: - The board and the data are one picture

/// Every deal on the board is a deal in the data, with the same stage, status, value and move time.
#[test]
fn every_card_on_the_board_is_a_deal_in_the_data() {
    for st in [golden_state(), golden_pending_state()] {
        let boot = st.bootstrap(now());
        let cards = boot["view"]["cards"].as_object().unwrap();
        assert!(!cards.is_empty());
        for (id, card) in cards {
            let deal = boot["data"]["deals"].as_array().unwrap().iter().find(|d| d["id"] == id.as_str()).expect("a card with no deal");
            for (a, b) in [("stage", "stage"), ("status", "status"), ("value", "value"), ("movedAt", "movedAt")] {
                assert_eq!(deal[a], card[b], "{id}: {a}");
            }
        }
    }
}

/// And in the other direction: the totals the board shows per column and the pipeline totals
/// are the same money, grouped the same way — and never summed across currencies.
#[test]
fn pipeline_totals_are_per_currency_core_formatted_and_never_summed() {
    let st = golden_state();
    let boot = st.bootstrap(now());
    let p = &boot["derived"]["pipeline"];
    // Open: Hollis renewal 45,000.00 USD + Acme pilot 8,000.00 USD; Northwind expansion 12,000.00 EUR.
    let open = p["open"].as_array().unwrap();
    assert_eq!(open.len(), 2, "two currencies, two totals: {open:?}");
    let usd = open.iter().find(|m| m["currency"] == "USD").unwrap();
    let eur = open.iter().find(|m| m["currency"] == "EUR").unwrap();
    assert_eq!(usd["amount"], 5_300_000);
    assert_eq!(eur["amount"], 1_200_000);
    for m in open {
        assert!(m["formatted"].as_str().is_some_and(|f| !f.is_empty()), "the core formats it: {m}");
    }
    assert_eq!(p["won"].as_array().unwrap().len(), 1);
    assert_eq!(p["won"][0]["amount"], 250_000);
    assert_eq!(p["lost"].as_array().unwrap().len(), 0, "a lost deal with no value adds nothing");

    // The board's open columns are the same money, split by stage.
    let columns = boot["view"]["board"]["columns"].as_array().unwrap();
    let column_usd: i64 = columns
        .iter()
        .filter(|c| !["won", "lost"].contains(&c["key"].as_str().unwrap()))
        .flat_map(|c| c["totals"].as_array().unwrap())
        .filter(|m| m["currency"] == "USD")
        .map(|m| m["amount"].as_i64().unwrap())
        .sum();
    assert_eq!(column_usd, 5_300_000);
}

#[test]
fn pipeline_totals_leave_out_archived_deals() {
    let mut st = golden_state();
    let deal = st.db().by_handle("acme-pilot").unwrap().1;
    st.archive(&deal, &at_min(70)).unwrap();
    let usd = st.bootstrap(now())["derived"]["pipeline"]["open"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["currency"] == "USD")
        .unwrap()["amount"]
        .as_i64()
        .unwrap();
    assert_eq!(usd, 4_500_000);
}

// MARK: - Per seat, per org

#[test]
fn the_view_holds_only_what_a_seat_looks_at_and_the_data_holds_only_rows() {
    let boot = golden_state().bootstrap(now());
    let view: Vec<&str> = boot["view"].as_object().unwrap().keys().map(String::as_str).collect();
    for key in ["focus", "list", "board", "cards", "pending", "focused"] {
        assert!(view.contains(&key), "the view is missing {key}");
    }
    let data: Vec<&str> = boot["data"].as_object().unwrap().keys().map(String::as_str).collect();
    for key in ["companies", "contacts", "deals", "tasks", "pipeline"] {
        assert!(data.contains(&key), "the data is missing {key}");
    }
    for seat_only in ["focus", "list", "board", "pending", "focused", "cards"] {
        assert!(!data.contains(&seat_only), "`{seat_only}` is a seat's, and is in the org's data");
    }
}

/// The data holds **stored** fields and nothing derived: a company's deal count would be
/// stale the moment one of its deals moved, without the company changing.
#[test]
fn the_data_carries_no_derived_facts_that_a_delta_could_leave_stale() {
    let boot = golden_state().bootstrap(now());
    for company in boot["data"]["companies"].as_array().unwrap() {
        for derived in ["dealCount", "openValue", "contactCount"] {
            assert!(company.get(derived).is_none(), "{derived} is derived: it belongs on the view's rows");
        }
    }
    let row = &boot["view"]["list"]["rows"].as_array().unwrap().iter().find(|r| r["kind"] == "company").unwrap().clone();
    for derived in ["dealCount", "openValue", "contactCount"] {
        assert!(row.get(derived).is_some(), "the list's company rows carry {derived}");
    }
}

/// **Another seat's navigation never crosses.** Two states over the same records: one looks at
/// a company, one at a deal. Their data is identical byte for byte; only the view differs.
#[test]
fn two_seats_over_the_same_records_share_the_data_and_not_the_view() {
    let a = golden_state();
    let mut b = AppState::with_db(a.db());
    let acme = b.db().by_handle("acme-corp").unwrap().1;
    b.show(&acme).unwrap();
    b.find("northwind", false);

    let (ba, bb) = (a.bootstrap(now()), b.bootstrap(now()));
    assert_eq!(ba["data"], bb["data"], "record data is per org");
    assert_ne!(ba["view"]["focus"], bb["view"]["focus"], "view state is per seat");
    assert_ne!(ba["view"]["list"]["query"], bb["view"]["list"]["query"]);
}

#[test]
fn a_navigation_takes_no_watermark_and_produces_no_row_action() {
    let mut st = golden_state();
    let w = st.watermark();
    let before = st.db();
    let acme = st.db().by_handle("acme-corp").unwrap().1;
    for req in [
        json!({ "cmd": "show", "kind": "company", "id": acme }),
        json!({ "cmd": "find", "query": "northwind" }),
        json!({ "cmd": "board", "stage": "proposal" }),
    ] {
        let out = run(&mut st, req.clone());
        assert_eq!(out.resp["ok"], true, "{req}: {:?}", out.resp);
    }
    assert_eq!(st.stamp(&before), vec![], "looking is not a row action");
    assert_eq!(st.watermark(), w, "and it takes no number");
}

// MARK: - The watermark: two counters, two jobs

#[test]
fn every_row_action_takes_the_next_number_in_the_order_it_was_committed() {
    let mut st = state();
    let before = st.db();
    let acme = st.add_company("Acme Corp", &ctx());
    let deal = st.add_deal("Acme renewal", Some(&acme), None, None, &ctx());
    st.log(ActivityKind::Note, "hello", vec![deal.clone()], None, &ctx());
    let ops = st.stamp(&before);

    let kinds: Vec<&str> = ops.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, ["company", "deal", "activity"], "stable order: kind, then creation");
    let seqs: Vec<u64> = ops.iter().map(|o| o.seq).collect();
    assert_eq!(seqs, [1, 2, 3], "strictly increasing, no gaps");
    assert_eq!(st.watermark(), 3);

    // The next commit continues from there.
    let before = st.db();
    st.complete_task(&st.db().tasks.first().map(|t| t.id.clone()).unwrap_or_default(), &ctx()).ok();
    st.add_task("Call", Date::new(2027, 1, 1), vec![deal], None, &ctx());
    let ops = st.stamp(&before);
    assert_eq!(ops.iter().map(|o| o.seq).collect::<Vec<_>>(), [4]);
}

/// **Not the local `rev`.** They are different counters: the same data position can be seen
/// at any number of revisions, and a revision says nothing about the data.
#[test]
fn the_watermark_and_the_local_rev_are_two_counters_with_two_jobs() {
    let st = golden_state();
    let a = st.bootstrap(now());
    let b = st.bootstrap(now());
    assert_eq!(a["watermark"], b["watermark"], "nothing happened to the data between the two reads");
    assert!(b["rev"].as_u64() > a["rev"].as_u64(), "but the window drew a later view, so the rev moved");

    // And a row action moves the watermark without the rev having to be the same number.
    let (st2, update) = golden_after_a_move();
    assert_eq!(a["watermark"], 0, "the golden workspace was built in memory, never committed");
    assert_eq!(update["watermark"], 2, "a move is two row actions: the deal and the line in the history");
    assert_ne!(update["rev"], update["watermark"], "two numbers, two jobs");
    let _ = st2;
}

#[test]
fn the_watermark_persists_with_the_data_and_continues_after_a_restart() {
    let mut st = state();
    let before = st.db();
    st.add_company("Acme Corp", &ctx());
    st.stamp(&before);

    let json = serde_json::to_string(&st.db()).unwrap();
    let mut again = AppState::with_db(serde_json::from_str(&json).unwrap());
    assert_eq!(again.watermark(), 1, "it came back with the rows it numbers");
    let before = again.db();
    again.add_company("Hooli", &ctx());
    assert_eq!(again.stamp(&before)[0].seq, 2, "and the next action continues the sequence");
}

#[test]
fn a_client_asks_for_what_it_missed_and_is_told_to_bootstrap_when_the_log_no_longer_reaches() {
    let mut st = state();
    let commit = |st: &mut AppState, name: &str| {
        let before = st.db();
        st.add_company(name, &ctx());
        let ops = st.stamp(&before);
        st.publish(ops);
    };
    commit(&mut st, "One");
    commit(&mut st, "Two");
    commit(&mut st, "Three");

    assert_eq!(st.changes(3), Some(vec![]), "up to date");
    let missed = st.changes(1).expect("the log reaches back to 1");
    assert_eq!(missed.iter().map(|o| o.seq).collect::<Vec<_>>(), [2, 3], "everything after, in order");
    assert_eq!(st.changes(0).unwrap().len(), 3);
    assert_eq!(st.changes(9), None, "a client ahead of the data holds something that is gone");

    // After a restart the watermark is there and the log is not: bootstrap again.
    let restarted = AppState::with_db(st.db());
    assert_eq!(restarted.changes(restarted.watermark()), Some(vec![]));
    assert_eq!(restarted.changes(1), None);
}

#[test]
fn the_log_is_bounded_and_a_client_older_than_it_must_bootstrap() {
    let mut log = DeltaLog::default();
    let op = |seq| Op { seq, kind: "company", id: format!("c{seq}"), record: json!({}) };
    log.push((1..=(LOG_CAP as u64 + 10)).map(op).collect());
    let now_seq = LOG_CAP as u64 + 10;
    assert_eq!(log.since(now_seq, 5), None, "5 fell off the front");
    assert_eq!(log.since(now_seq, 10).unwrap().len(), LOG_CAP, "10 is exactly the oldest it still covers");
}

#[test]
fn the_changes_envelope_answers_in_order_or_says_stale() {
    let mut st = state();
    let before = st.db();
    st.add_company("One", &ctx());
    let ops = st.stamp(&before);
    st.publish(ops);

    let ok = run(&mut st, json!({ "cmd": "changes", "since": 0 }));
    assert_eq!(ok.resp["changes"]["ops"][0]["kind"], "company");
    assert_eq!(ok.resp["changes"]["from"], 0);
    assert_eq!(ok.resp["changes"]["to"], 1);

    let stale = run(&mut st, json!({ "cmd": "changes", "since": 5 }));
    assert_eq!(stale.resp["changes"]["stale"], true);
    assert_eq!(stale.resp["changes"]["watermark"], 1);

    let bad = run(&mut st, json!({ "cmd": "changes" }));
    assert_eq!(bad.resp["ok"], false);
}

// MARK: - Deltas: row actions, not whole state

#[test]
fn an_update_after_a_move_carries_the_deal_and_the_move_and_nothing_else() {
    let (_, update) = golden_after_a_move();
    assert_eq!(update["type"], "update");
    assert!(update.get("data").is_none(), "an update never carries the whole state");
    let ops = update["delta"]["ops"].as_array().unwrap();
    let kinds: Vec<&str> = ops.iter().map(|o| o["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["deal", "move"], "the deal's new row, and the line in the history");
    assert_eq!(ops[0]["record"]["stage"], "qualified");
    assert_eq!(ops[1]["record"]["from"], "lead");
    assert_eq!(ops[1]["record"]["to"], "qualified");
    assert_eq!(update["delta"]["to"], update["watermark"]);
    assert_eq!(update["delta"]["from"].as_u64().unwrap() + 2, update["delta"]["to"].as_u64().unwrap());
}

#[test]
fn an_update_without_a_row_action_has_an_empty_delta_at_the_same_watermark() {
    let st = golden_state();
    let u = st.update(now(), &[]);
    assert_eq!(u["delta"]["ops"], json!([]));
    assert_eq!(u["delta"]["from"], u["delta"]["to"]);
    assert_eq!(u["delta"]["to"], u["watermark"]);
}

#[test]
fn a_row_that_did_not_change_is_not_in_the_delta() {
    let mut st = golden_state();
    let before = st.db();
    let acme = st.db().by_handle("acme-corp").unwrap().1;
    st.set_field(&acme, "domain", "acme.example", &at_min(80)).unwrap();
    let ops = st.stamp(&before);
    assert_eq!(ops.len(), 1, "one company changed, and only it: {ops:?}");
    assert_eq!(ops[0].record["domain"], "acme.example");
}

#[test]
fn applying_the_bootstrap_and_then_the_deltas_reproduces_the_state() {
    // The property the whole contract rests on: bootstrap, then deltas, equals bootstrap-later.
    let mut st = golden_state();
    let boot = st.bootstrap(now());

    let mut rows: HashMap<(String, String), Value> = HashMap::new();
    for (kind, key) in [("company", "companies"), ("contact", "contacts"), ("deal", "deals"), ("task", "tasks")] {
        for r in boot["data"][key].as_array().unwrap() {
            rows.insert((kind.to_string(), r["id"].as_str().unwrap().to_string()), r.clone());
        }
    }

    // A handful of commits of every kind.
    let mut apply = |st: &mut AppState, req: Value, minutes: i64| {
        let before = st.db();
        let out = st.command(&req, None, &at_min(minutes));
        assert_eq!(out.resp["ok"], true, "{req}: {:?}", out.resp);
        for op in st.stamp(&before) {
            if ["activity", "move"].contains(&op.kind) {
                continue; // feed items, not entity rows
            }
            rows.insert((op.kind.to_string(), op.id.clone()), op.record.clone());
        }
    };
    let deal = st.db().by_handle("acme-pilot").unwrap().1;
    apply(&mut st, json!({ "cmd": "move", "id": deal, "to": "negotiation" }), 90);
    apply(&mut st, json!({ "cmd": "add", "kind": "company", "name": "Initech" }), 91);
    apply(&mut st, json!({ "cmd": "task", "handle": "acme-pilot", "what": "Call back", "due": "2027-01-04" }), 92);
    apply(&mut st, json!({ "cmd": "archive", "handle": "northwind-traders" }), 93);
    apply(&mut st, json!({ "cmd": "set", "handle": "hollis-partners", "field": "domain", "value": "hollis.example" }), 94);

    let later = st.bootstrap(now());
    let mut fresh: HashMap<(String, String), Value> = HashMap::new();
    for (kind, key) in [("company", "companies"), ("contact", "contacts"), ("deal", "deals"), ("task", "tasks")] {
        for r in later["data"][key].as_array().unwrap() {
            fresh.insert((kind.to_string(), r["id"].as_str().unwrap().to_string()), r.clone());
        }
    }
    assert_eq!(rows, fresh, "bootstrap + deltas must equal a later bootstrap, row for row");
}

// MARK: - Gap 4: a search that touches nothing

#[test]
fn a_search_changes_nothing_a_surface_is_looking_at() {
    let mut st = golden_state();
    st.find("northwind", false);
    let before = st.db();
    let view_before = st.update(now(), &[])["view"].clone();

    let out = run(&mut st, json!({ "cmd": "search", "q": "acme" }));
    assert_eq!(out.resp["ok"], true);
    assert!(out.resp["search"]["total"].as_u64().unwrap() > 0, "it found something, or this proves nothing");

    assert_eq!(st.db(), before, "the dataset — view included — is exactly as it was");
    assert_eq!(view_before, st.update(now(), &[])["view"], "both surfaces see the same thing they saw");
    assert_eq!(st.db().view.list.query, "northwind", "the shared list still says what it said");
}

#[test]
fn a_search_returns_rows_and_a_count_per_kind_for_everything_that_matched() {
    let st = golden_state();
    let r = st.search_json("acme", &[], 8);
    assert_eq!(r["counts"]["company"], 1);
    assert_eq!(r["counts"]["deal"], 2, "Acme pilot and Acme onboarding");
    assert_eq!(r["counts"]["contact"], 0);
    assert_eq!(r["total"], 3);
    assert_eq!(r["rows"].as_array().unwrap().len(), 3);
    assert!(r["rows"][0].get("handle").is_some() && r["rows"][0].get("updatedAt").is_some());
}

#[test]
fn a_search_can_be_narrowed_capped_and_never_offers_an_archived_record() {
    let mut st = golden_state();
    let r = st.search_json("acme", &[Kind::Deal], 1);
    assert_eq!(r["rows"].as_array().unwrap().len(), 1, "capped");
    assert_eq!(r["total"], 2, "but it says how many there were");
    assert_eq!(r["counts"]["company"], 0, "counts follow the kinds asked for");

    let acme = st.db().by_handle("acme-corp").unwrap().1;
    st.archive(&acme, &at_min(100)).unwrap();
    assert_eq!(st.search_json("acme", &[Kind::Company], 8)["total"], 0);
}

#[test]
fn a_search_does_not_use_the_lists_sort_or_kind_because_those_are_somebody_elses() {
    let mut st = golden_state();
    run(&mut st, json!({ "cmd": "find", "query": "", "kind": "company", "sort": "name" }));
    let r = st.search_json("acme", &[], 8);
    assert_eq!(r["counts"]["deal"], 2, "the list is narrowed to companies; the search is not");
}

#[test]
fn the_search_envelope_validates_its_arguments() {
    let mut st = golden_state();
    assert_eq!(run(&mut st, json!({ "cmd": "search", "q": 7 })).resp["ok"], false);
    assert_eq!(run(&mut st, json!({ "cmd": "search", "q": "a", "kinds": ["people"] })).resp["ok"], false);
    assert_eq!(run(&mut st, json!({ "cmd": "search", "q": "a", "kinds": "deal" })).resp["ok"], false);
    assert_eq!(run(&mut st, json!({ "cmd": "search", "q": "northwind", "kinds": ["company", "deal"] })).resp["ok"], true);
}

// MARK: - Gap 1: next steps across records

#[test]
fn tasks_are_listed_across_every_record_soonest_due_first_with_what_they_are_on() {
    let mut st = golden_state();
    let hooli = st.add_company("Hooli", &at_min(110));
    st.add_task("Intro call", Date::new(2026, 9, 8), vec![hooli], None, &at_min(111));

    let t = st.tasks_json(now(), 100, false);
    assert_eq!(t["total"], 3, "two on the renewal, one on Hooli — the done one is not open");
    let open = t["open"].as_array().unwrap();
    let whats: Vec<&str> = open.iter().map(|x| x["what"].as_str().unwrap()).collect();
    assert_eq!(whats, ["Send the contract", "Intro call", "Call Maya about the term"], "soonest due first");
    assert_eq!(open[0]["record"]["handle"], "hollis-renewal");
    assert_eq!(open[0]["record"]["label"], "Hollis renewal");
    assert_eq!(open[1]["record"]["kind"], "company");
    assert_eq!(open[0]["bucket"], "overdue");
    assert_eq!(open[1]["bucket"], "today");
    assert_eq!(open[2]["bucket"], "week");
    assert!(open[0]["handle"].as_str().is_some(), "the handle `crm done` takes");
}

#[test]
fn tasks_can_be_capped_and_can_include_what_is_done() {
    let st = golden_state();
    let t = st.tasks_json(now(), 1, true);
    assert_eq!(t["open"].as_array().unwrap().len(), 1);
    assert_eq!(t["total"], 2, "the cap limits what is carried, not what is counted");
    assert_eq!(t["doneTotal"], 1);
    assert_eq!(t["done"][0]["what"], "Book the kickoff");
    assert_eq!(t["done"][0]["bucket"], "done");
}

/// QA 6c: "a task carries no `sentAt`", so a surface could only send the whole batch again.
#[test]
fn a_task_says_when_the_timer_sent_it_so_a_surface_can_pick_one_to_send_again() {
    let mut st = golden_state();
    st.set_agents(vec![agent_row()]);
    let sweep = st.sweep(&at_min(120));
    assert_eq!(sweep.emits.len(), 1, "the person's overdue step fires");
    let t = st.tasks_json(now(), 100, false);
    let sent: Vec<&Value> = t["open"].as_array().unwrap().iter().filter(|x| !x["sentAt"].is_null()).collect();
    assert!(!sent.is_empty(), "the step that was sent says so");
    assert_eq!(sent[0]["sentAt"], at_min(120).at());
}

// MARK: - Gap 2: the feed and the move log

#[test]
fn a_stage_change_is_kept_with_where_it_was_and_where_it_went() {
    let st = golden_state();
    let moves = &st.db().moves;
    assert_eq!(moves.len(), 5, "renewal, expansion, onboarding closed, trial qualified, trial closed");
    let renewal = moves.iter().find(|m| m.to == "negotiation").unwrap();
    assert_eq!(renewal.from, "lead");
    let closed = moves.iter().find(|m| m.to == "won").unwrap();
    assert_eq!(closed.from, "lead", "a close remembers where the deal was");
    let lost = moves.iter().find(|m| m.to == "lost").unwrap();
    assert_eq!(lost.from, "qualified");
    assert_eq!(lost.by, Actor::Agent { id: AGENT.into() });
}

#[test]
fn a_move_to_where_it_already_is_is_not_a_line_in_the_history() {
    let mut st = state();
    let deal = st.add_deal("Acme", None, None, None, &ctx());
    st.move_deal(&deal, MoveTarget::To(Stage::Lead), None, &ctx()).unwrap();
    assert!(st.db().moves.is_empty());
    st.move_deal(&deal, MoveTarget::To(Stage::Proposal), None, &ctx()).unwrap();
    st.move_deal(&deal, MoveTarget::Close(Status::Won), None, &ctx()).unwrap();
    st.move_deal(&deal, MoveTarget::To(Stage::Qualified), None, &ctx()).unwrap(); // reopens
    let words: Vec<(String, String)> = st.db().moves.iter().map(|m| (m.from.clone(), m.to.clone())).collect();
    assert_eq!(
        words,
        [("lead".into(), "proposal".into()), ("proposal".into(), "won".into()), ("won".into(), "qualified".into())]
    );
}

#[test]
fn the_feed_merges_the_timeline_and_the_moves_newest_first() {
    let st = golden_state();
    let f = st.feed_json(None, None, 0, 100);
    assert_eq!(f["total"], 8, "three timeline lines and five moves");
    let items = f["items"].as_array().unwrap();
    let ats: Vec<i64> = items.iter().map(|i| i["at"].as_i64().unwrap()).collect();
    let mut sorted = ats.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(ats, sorted, "newest first");
    let kinds: std::collections::BTreeSet<&str> = items.iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds.into_iter().collect::<Vec<_>>(), ["call", "email", "note", "stage"]);
    let stage = items.iter().find(|i| i["kind"] == "stage").unwrap();
    assert!(stage["from"].is_string() && stage["to"].is_string());
    assert_eq!(stage["record"]["kind"], "deal");
    let note = items.iter().find(|i| i["kind"] == "note").unwrap();
    assert_eq!(note["record"]["handle"], "hollis-renewal", "a line is about the deal it names, first");
    assert_eq!(note["links"].as_array().unwrap().len(), 2, "and says everything it names");
}

#[test]
fn the_feed_filters_by_actor_and_kind_and_pages() {
    let st = golden_state();
    let agent = st.feed_json(Some(&Actor::Agent { id: AGENT.into() }), None, 0, 100);
    assert!(agent["items"].as_array().unwrap().iter().all(|i| i["by"]["id"] == AGENT));
    let person = st.feed_json(Some(&Actor::Human), None, 0, 100);
    assert_eq!(agent["total"].as_u64().unwrap() + person["total"].as_u64().unwrap(), 8);

    let stages = st.feed_json(None, Some("stage"), 0, 100);
    assert_eq!(stages["total"], 5);
    assert_eq!(st.feed_json(None, Some("call"), 0, 100)["total"], 1);

    let p0 = st.feed_json(None, None, 0, 3);
    let p1 = st.feed_json(None, None, 1, 3);
    assert_eq!(p0["items"].as_array().unwrap().len(), 3);
    assert_ne!(p0["items"][0]["id"], p1["items"][0]["id"]);
    assert_eq!(st.feed_json(None, None, 99, 3)["items"], json!([]), "past the end is empty, not an error");
}

#[test]
fn the_feed_envelope_writes_nothing_and_validates() {
    let mut st = golden_state();
    let before = st.db();
    let ok = run(&mut st, json!({ "cmd": "feed", "actor": "human", "kind": "stage" }));
    assert_eq!(ok.resp["feed"]["total"], 3, "the person made three of the five moves");
    assert_eq!(st.db(), before);
    assert_eq!(run(&mut st, json!({ "cmd": "feed", "kind": "tweet" })).resp["ok"], false);
    assert_eq!(run(&mut st, json!({ "cmd": "feed", "actor": 5 })).resp["ok"], false);
    let by_agent = run(&mut st, json!({ "cmd": "feed", "actor": { "kind": "agent", "id": AGENT } }));
    assert_eq!(by_agent.resp["ok"], true);
}

#[test]
fn a_new_line_and_a_new_move_arrive_as_feed_items_in_the_delta() {
    let mut st = golden_state();
    let before = st.db();
    let deal = st.db().by_handle("acme-pilot").unwrap().1;
    st.log(ActivityKind::Call, "rang", vec![deal.clone()], None, &at_min(130));
    st.move_deal(&deal, MoveTarget::To(Stage::Proposal), None, &at_min(131)).unwrap();
    let ops = st.stamp(&before);
    let kinds: Vec<&str> = ops.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, ["deal", "activity", "move"]);
    assert_eq!(ops[1].record["kind"], "call");
    assert_eq!(ops[1].record["record"]["handle"], "acme-pilot");
}

// MARK: - Gap 5: more find filters

fn find(st: &mut AppState, req: Value) -> Value {
    let mut req = req;
    req["cmd"] = json!("find");
    let out = run(st, req);
    // A refusal is the response as it is; an answer is what this seat is now shown.
    if out.resp["ok"] == true { flatten(&out.resp["split"]) } else { out.resp }
}

fn no_filters(v: &Value) -> bool {
    v["list"]["filters"].as_object().is_some_and(|f| f.values().all(Value::is_null))
}

fn handles(resp: &Value) -> Vec<String> {
    resp["list"]["rows"].as_array().unwrap().iter().map(|r| r["handle"].as_str().unwrap().to_string()).collect()
}

#[test]
fn find_filters_deals_by_status() {
    let mut st = golden_state();
    let won = find(&mut st, json!({ "query": "", "status": "won" }));
    assert_eq!(handles(&won), ["acme-onboarding"]);
    let open = find(&mut st, json!({ "status": "open" }));
    assert_eq!(open["list"]["total"], 3, "the three still open");
    assert!(open["list"]["rows"].as_array().unwrap().iter().all(|r| r["kind"] == "deal"), "a filter on deals is a list of deals");
}

#[test]
fn find_filters_deals_by_stage_and_by_who_touched_them() {
    let mut st = golden_state();
    let proposal = find(&mut st, json!({ "query": "", "status": null, "stage": "proposal" }));
    assert_eq!(handles(&proposal), ["northwind-expansion"]);

    let agent = find(&mut st, json!({ "stage": null, "touchedBy": { "kind": "agent", "id": AGENT } }));
    let mut got = handles(&agent);
    got.sort();
    assert_eq!(got, ["acme-pilot", "hollis-renewal", "northwind-trial"], "the three whose latest move was the agent's");

    let me = find(&mut st, json!({ "touchedBy": "human" }));
    let mut got = handles(&me);
    got.sort();
    assert_eq!(got, ["acme-onboarding", "northwind-expansion"]);
}

#[test]
fn find_filters_deals_by_when_they_last_moved_in_the_callers_calendar() {
    let mut st = golden_state();
    // The golden deals moved at +7..+31 minutes after 2026-09-08T10:00Z. "Stalled" = moved before a date.
    let none = find(&mut st, json!({ "query": "", "touchedBy": null, "before": "2026-09-08" }));
    assert_eq!(none["list"]["total"], 0, "nothing moved before the 8th");
    let all = find(&mut st, json!({ "before": "2026-09-09" }));
    assert_eq!(all["list"]["total"], 5, "every deal moved on the 8th");
    let since = find(&mut st, json!({ "before": null, "since": "2026-09-09" }));
    assert_eq!(since["list"]["total"], 0, "and none since the 9th");

    // The date is the person's: at +14:00 the 8th began 14 hours earlier in absolute time.
    let mut ahead = golden_state();
    let out = ahead.command(
        &json!({ "cmd": "find", "since": "2026-09-09" }),
        None,
        &crate::state::tests::at_offset(now(), 14 * 3600),
    );
    assert_eq!(
        flatten(&out.resp["split"])["list"]["total"], 5,
        "local midnight of the 9th at +14:00 is 10:00Z on the 8th, and every deal moved after it"
    );
}

#[test]
fn a_filter_is_sticky_named_in_the_view_and_cleared_by_null_or_by_all() {
    let mut st = golden_state();
    assert!(no_filters(&seen(&st, now())), "none set: every filter is null");

    let r = find(&mut st, json!({ "status": "won", "stage": "lead" }));
    assert_eq!(r["list"]["filters"]["status"], "won");
    assert_eq!(r["list"]["filters"]["stage"], "lead");

    // Sticky: the next find that does not mention it keeps it.
    let again = find(&mut st, json!({ "query": "" }));
    assert_eq!(again["list"]["filters"]["status"], "won");

    // `null` clears one.
    let one = find(&mut st, json!({ "status": null }));
    assert!(one["list"]["filters"]["status"].is_null());
    assert_eq!(one["list"]["filters"]["stage"], "lead");

    // `kind: null` — "all" — clears every one.
    let all = find(&mut st, json!({ "kind": null }));
    assert!(no_filters(&all), "searching everything searches everything: {all}");
    assert_eq!(all["list"]["total"], 11, "three companies, three contacts and five deals again");
}

#[test]
fn a_bad_filter_leaves_the_list_exactly_as_it_was() {
    let mut st = golden_state();
    find(&mut st, json!({ "query": "acme", "status": "open" }));
    let before = st.db().view.list.clone();
    for (req, needle) in [
        (json!({ "status": "pending" }), "not a status"),
        (json!({ "stage": "closing" }), "not a stage"),
        (json!({ "since": "last week" }), "is a date"),
        (json!({ "before": 7 }), "must be text"),
        (json!({ "touchedBy": 5 }), "an actor is"),
    ] {
        let r = find(&mut st, req.clone());
        assert_eq!(r["ok"], false, "{req}");
        assert!(r["error"].as_str().unwrap().contains(needle), "{req} → {}", r["error"]);
        assert_eq!(st.db().view.list, before, "{req} half-applied");
    }
}

#[test]
fn the_filters_survive_a_restart_with_the_rest_of_the_view() {
    let mut st = golden_state();
    find(&mut st, json!({ "status": "open", "touchedBy": "human", "since": "2026-09-01" }));
    let back: Db = serde_json::from_str(&serde_json::to_string(&st.db()).unwrap()).unwrap();
    assert_eq!(back.view.list.status, Some(Status::Open));
    assert_eq!(back.view.list.touched_by, Some(Actor::Human));
    assert_eq!(back.view.list.since, Some(Date::new(2026, 9, 1)));
}

// MARK: - Gaps 6 and 11: facts the core stored and never sent

#[test]
fn a_deal_row_says_when_it_opened_closed_moved_and_was_updated() {
    let st = golden_state();
    let won = st.row_v2(&st.db().by_handle("acme-onboarding").unwrap().1).unwrap();
    assert_eq!(won["openedAt"], at_min(11).at());
    assert_eq!(won["closedAt"], at_min(25).at(), "a closed deal says when");
    assert_eq!(won["movedAt"], at_min(25).at());
    assert_eq!(won["updatedAt"], at_min(25).at());
    assert_eq!(won["companyHandle"], "acme-corp", "a handle, so it can be a link");
    let open = st.row_v2(&st.db().by_handle("hollis-renewal").unwrap().1).unwrap();
    assert!(open["closedAt"].is_null());
    assert_eq!(open["by"]["id"], AGENT);
}

#[test]
fn a_company_row_carries_its_domain_tags_counts_and_open_value_per_currency() {
    let mut st = golden_state();
    let hollis = st.db().by_handle("hollis-partners").unwrap().1;
    st.set_field(&hollis, "domain", "hollis.example", &at_min(140)).unwrap();
    let row = st.row_v2(&hollis).unwrap();
    assert_eq!(row["domain"], "hollis.example");
    assert_eq!(row["dealCount"], 1);
    assert_eq!(row["contactCount"], 2);
    assert_eq!(row["openValue"][0]["currency"], "USD");
    assert_eq!(row["openValue"][0]["amount"], 4_500_000);
    assert!(row["updatedAt"].as_i64().unwrap() >= at_min(140).at());

    let acme = st.row_v2(&st.db().by_handle("acme-corp").unwrap().1).unwrap();
    assert_eq!(acme["dealCount"], 2, "the pilot and the won onboarding");
    assert_eq!(acme["openValue"].as_array().unwrap().len(), 1, "only the open one counts as open value");
    assert_eq!(acme["openValue"][0]["amount"], 800_000);
}

#[test]
fn a_contact_row_carries_its_title_email_phone_and_its_companys_handle() {
    let mut st = golden_state();
    let maya = st.db().by_handle("maya-hollis").unwrap().1;
    st.set_field(&maya, "title", "Head of Procurement", &at_min(150)).unwrap();
    st.set_field(&maya, "email", "maya@hollis.example", &at_min(151)).unwrap();
    let row = st.row_v2(&maya).unwrap();
    assert_eq!(row["title"], "Head of Procurement");
    assert_eq!(row["email"], "maya@hollis.example");
    assert_eq!(row["companyHandle"], "hollis-partners");
    assert!(row["phone"].is_null());
}

// MARK: - Org, owner and me: recorded, not enforced

#[test]
fn every_row_is_born_with_the_org_and_the_owner_it_was_made_by() {
    let mut st = state();
    let acme = st.add_company("Acme", &ctx());
    let ada = st.add_contact("Ada", Some(&acme), &ctx());
    let deal = st.add_deal("Pilot", Some(&acme), None, Some("agent-1"), &ctx());
    st.log(ActivityKind::Note, "hi", vec![deal.clone()], Some("agent-1"), &ctx());
    st.add_task("Call", Date::new(2027, 1, 1), vec![deal], None, &ctx());
    let _ = ada;
    let db = st.db();

    let me = Person::default().id;
    assert!(db.companies.iter().all(|r| r.org_id == LOCAL_ORG && r.owner_id == me));
    assert!(db.contacts.iter().all(|r| r.org_id == LOCAL_ORG && r.owner_id == me));
    assert!(db.activities.iter().all(|r| r.org_id == LOCAL_ORG && r.owner_id == me));
    assert!(db.tasks.iter().all(|r| r.org_id == LOCAL_ORG && r.owner_id == me));
    assert_eq!(db.deals[0].owner_id, me, "an agent's deal is owned by the person whose seat the agent is at");
    assert_eq!(db.deals[0].moved_by, Actor::Agent { id: "agent-1".into() }, "…while who acted is still who acted");
}

#[test]
fn org_owner_and_me_round_trip_through_the_store() {
    use crate::store::{CrmStore, JsonStore};
    let dir = std::env::temp_dir().join(format!("crm-p1-{}-{}", std::process::id(), now().at));
    let path = dir.join("crm.json");
    let store = JsonStore::at(&path);

    let me = Person { id: "person-ayse".into(), name: "Ayşe".into() };
    let mut st = AppState::open_as(Db::default(), &ctx(), me.clone()).0;
    st.add_company("Acme", &ctx());
    st.add_deal("Pilot", None, None, None, &ctx());
    let mut db = st.db();
    db.org = Org { id: "org-7".into(), name: "Breksos".into() };
    for c in &mut db.companies {
        c.org_id = "org-7".into();
    }
    store.save(&db).unwrap();

    let back = store.load().unwrap();
    assert_eq!(back.org, Org { id: "org-7".into(), name: "Breksos".into() });
    assert_eq!(back.companies[0].org_id, "org-7");
    assert_eq!(back.companies[0].owner_id, "person-ayse");
    assert_eq!(back.deals[0].owner_id, "person-ayse");
    assert_eq!(back.deals[0].org_id, LOCAL_ORG);

    // And the person at the seat is in the snapshot, by id and name (gap 8).
    let boot = AppState::open_as(back, &ctx(), me).0.bootstrap(now());
    assert_eq!(boot["seat"]["me"], json!({ "id": "person-ayse", "name": "Ayşe" }));
    assert_eq!(boot["org"], json!({ "id": "org-7", "name": "Breksos" }));
    assert_eq!(boot["data"]["companies"][0]["orgId"], "org-7");
    assert_eq!(boot["data"]["companies"][0]["ownerId"], "person-ayse");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A dataset written before any of this existed: no org, no owner, no watermark, no moves.
#[test]
fn an_older_dataset_opens_and_every_row_gets_the_org_and_an_owner() {
    let mut st = golden_state();
    let before = st.db();
    st.command(&json!({ "cmd": "status" }), None, &ctx());
    let mut json: Value = serde_json::to_value(before).unwrap();
    for key in ["companies", "contacts", "deals", "activities", "tasks"] {
        for row in json[key].as_array_mut().unwrap() {
            let o = row.as_object_mut().unwrap();
            o.remove("orgId");
            o.remove("ownerId");
        }
    }
    for key in ["org", "watermark", "moves"] {
        json.as_object_mut().unwrap().remove(key);
    }
    let old: Db = serde_json::from_value(json).expect("an old file still opens");
    assert!(old.companies.iter().all(|c| c.org_id == LOCAL_ORG), "the org defaults on read");
    assert!(old.companies.iter().all(|c| c.owner_id.is_empty()), "an owner nobody recorded is empty");
    assert_eq!(old.watermark, 0);

    let me = Person { id: "person-berk".into(), name: "Berk".into() };
    let (opened, changed) = AppState::open_as(old, &ctx(), me);
    assert!(changed, "the stamps owe a save, or the next launch would migrate again");
    let db = opened.db();
    assert!(db.companies.iter().all(|r| r.owner_id == "person-berk"));
    assert!(db.contacts.iter().all(|r| r.owner_id == "person-berk"));
    assert!(db.deals.iter().all(|r| r.owner_id == "person-berk"));
    assert!(db.activities.iter().all(|r| r.owner_id == "person-berk"));
    assert!(db.tasks.iter().all(|r| r.owner_id == "person-berk"));
    // …and opened a second time there is nothing left to do.
    assert!(!AppState::open_as(db, &ctx(), Person { id: "someone-else".into(), name: "X".into() }).1);
}

#[test]
fn nothing_reads_org_or_owner_to_decide_anything_yet() {
    // Rows of a different org and a different owner are listed, searched, moved and counted
    // exactly like any other: recorded now, enforced at p2.
    let mut st = golden_state();
    let mut db = st.db();
    for d in &mut db.deals {
        d.org_id = "another-org".into();
        d.owner_id = "another-person".into();
    }
    st = AppState::with_db(db);
    assert_eq!(st.search_json("acme", &[Kind::Deal], 8)["total"], 2);
    let deal = st.db().by_handle("acme-pilot").unwrap().1;
    st.move_deal(&deal, MoveTarget::To(Stage::Qualified), None, &at_min(160)).unwrap();
    assert_eq!(st.db().deal(&deal).unwrap().stage, Stage::Qualified);
    assert_eq!(st.bootstrap(now())["derived"]["counts"]["deals"], 5);
}

#[test]
fn the_seat_holds_the_person_the_agents_and_the_reminders_and_none_of_it_is_the_views() {
    let boot = golden_state().bootstrap(now());
    let seat = boot["seat"].as_object().unwrap();
    for key in ["me", "agents", "reminders"] {
        assert!(seat.contains_key(key), "{key}");
    }
    assert_eq!(boot["seat"]["me"]["name"], "You", "an unconfigured seat has a person and a default name");
}
