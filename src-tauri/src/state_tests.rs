//! The rules, as tests.
//!
//! These are not coverage theatre. Each one pins a decision from `docs/architecture.md`
//! that is cheap to break by accident and expensive to notice — a stage that stops being a
//! stage, a page size that follows the caller, an archived record that keeps showing up.
//!
//! In its own file because `state.rs` is long enough already, and because a reader looking
//! for "what does this app actually guarantee" should be able to open one file and read it
//! top to bottom.

use super::*;
use crate::store::{CrmStore, JsonStore};

// MARK: - Fixtures

/// A fixed instant, so nothing here depends on when it runs: 2026-09-08T10:00:00Z, and
/// the same day as a civil date. The two halves are supplied independently because that is
/// exactly how the core receives them — which is what lets a test put a task three days
/// overdue without waiting three days.
fn now() -> Now {
    Now { at: 1_788_861_600_000, today: Date::new(2026, 9, 8) }
}

/// A later instant on the same day. Used where only the ordering of timestamps matters —
/// `today` deliberately does not move, so the due buckets stay pinned.
fn later(days: i64) -> Now {
    let n = now();
    Now { at: n.at + days * 86_400_000, today: n.today }
}

/// This install, fixed. Every record written below is stamped with it.
fn origin() -> InstanceId {
    InstanceId::from_bytes([0xA1; 16])
}

/// The impure half of one command, fixed so that every id minted below is reproducible
/// and every test reads the same on the tenth run as on the first.
fn ctx() -> Ctx {
    at(now())
}

fn at(now: Now) -> Ctx {
    // UTC by default — a fixed offset would be one more number every existing test would
    // have to carry for no reason. `--at` tests that care about the zone use `at_offset`.
    at_offset(now, 0)
}

fn at_offset(now: Now, offset_secs: i32) -> Ctx {
    Ctx { now, entropy: [0x5A; 10], origin: origin(), offset_secs }
}

fn state() -> AppState {
    AppState::new()
}

/// What a person or an agent would type to reach this record. Ids are ULIDs and nobody
/// types one, so every assertion about "what you type" goes through here.
fn handle(st: &AppState, id: &str) -> String {
    st.db().handle_of(id).unwrap_or_default().to_string()
}

/// A board with one deal per stage plus one won and one lost, all in USD.
fn a_full_board() -> AppState {
    let mut st = state();
    for (title, stage) in [
        ("Acme renewal", Stage::Lead),
        ("Hooli platform", Stage::Qualified),
        ("Initech rollout", Stage::Proposal),
        ("Umbrella expansion", Stage::Negotiation),
    ] {
        let id = st.add_deal(title, None, Some(Money::new(1_000_000, "USD")), None, &ctx());
        st.move_deal(&id, MoveTarget::To(stage), None, &ctx()).unwrap();
    }
    let won = st.add_deal("Globex pilot", None, Some(Money::new(2_000_000, "USD")), None, &ctx());
    st.move_deal(&won, MoveTarget::Close(Status::Won), None, &ctx()).unwrap();
    let lost = st.add_deal("Soylent trial", None, Some(Money::new(3_000_000, "USD")), None, &ctx());
    st.move_deal(&lost, MoveTarget::Close(Status::Lost), None, &ctx()).unwrap();
    st
}

fn column<'a>(snap: &'a Value, key: &str) -> &'a Value {
    snap["board"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == key)
        .unwrap_or_else(|| panic!("no `{key}` column on the board"))
}

// MARK: - 1. One vocabulary, three surfaces
//
// "Any enum a surface shows lives in the core." The board draws a Negotiation column, so
// `crm move` takes that word and `crm -h` names it. Three places, one list — and if they
// ever disagree, an agent learns a word from a screenshot that its CLI will refuse.

#[test]
fn the_stage_vocabulary_is_one_list_and_all_three_surfaces_read_it() {
    let source: Vec<&str> = Stage::ALL.iter().map(|s| s.word()).collect();

    // 1. what the snapshot carries — the board and the pipeline
    let snap = a_full_board().snapshot(now());
    let in_pipeline: Vec<&str> =
        snap["pipeline"]["stages"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    assert_eq!(in_pipeline, source, "the pipeline's stages are the vocabulary");

    let columns: Vec<&str> =
        snap["board"]["columns"].as_array().unwrap().iter().map(|c| c["key"].as_str().unwrap()).collect();
    assert_eq!(&columns[..4], &source[..], "the board's first four columns are the stages, in order");
    assert_eq!(columns[4..].to_vec(), vec!["won", "lost"], "then the two closing ones");

    // 2. what `crm move` accepts
    for word in &source {
        assert_eq!(
            MoveTarget::parse(word),
            Some(MoveTarget::To(Stage::parse(word).unwrap())),
            "`crm move … {word}` must be accepted"
        );
    }

    // 3. what `crm -h` names
    let manual = crate::cli::manual();
    for word in &source {
        assert!(manual.contains(word), "`crm -h` never names the stage `{word}`");
    }
}

/// The other half of the same rule: nothing is in the manual's vocabulary that the core
/// would refuse. A manual naming a word `crm move` rejects is worse than one naming none.
#[test]
fn every_word_the_manual_offers_move_is_a_word_move_accepts() {
    let manual = crate::cli::manual();
    for word in MoveTarget::vocabulary() {
        assert!(manual.contains(word), "`crm -h` never names `{word}`");
        assert!(MoveTarget::parse(word).is_some(), "`{word}` is named but not accepted");
    }
}

// MARK: - 2. The store seam
//
// `pipeline_id` is a field nothing reads in v1. The full round-trip lives in `store.rs`;
// this is the core's half — that a deal the core creates carries one at all.

#[test]
fn a_deal_the_core_creates_carries_the_pipeline_it_belongs_to() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    let db = st.db();
    assert_eq!(db.deal(&id).unwrap().pipeline_id, "sales");
    assert_eq!(db.pipeline().id, "sales", "one pipeline, and the deal is in it");
}

/// The seam end to end, through the real store: the core's dataset, written and read back.
#[test]
fn the_core_s_dataset_survives_the_store() {
    let mut st = a_full_board();
    st.find("acme", false);
    st.set_page_size(3);

    let path = std::env::temp_dir()
        .join(format!("crm-state-{}", std::process::id()))
        .join("crm.json");
    let store = JsonStore::at(&path);
    store.save(&st.db()).unwrap();
    let back = store.load().unwrap();

    assert_eq!(back, st.db(), "the whole dataset, including the shared view");
    assert_eq!(back.view.list.page_size, 3, "the shared page size is state, so it persists");
    assert!(back.deals.iter().all(|d| d.pipeline_id == "sales"));
    // The identity layer rides along: the ULID, the handle beside it, and which install
    // wrote this version.
    assert!(back.deals.iter().all(|d| Ulid::parse(&d.id).is_some()), "ids are ULIDs");
    assert!(back.deals.iter().all(|d| !d.handle.is_empty()), "every record keeps a handle");
    assert!(back.deals.iter().all(|d| d.origin == origin()), "and its origin");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// MARK: - 3. Pagination is shared state, never the caller's
//
// An agent asking for three results must not repaginate the person's table to three rows.
// The bug reads as "why does searching Acme return one result?" and it is invisible from
// the side that caused it.

#[test]
fn a_search_never_touches_the_shared_page_size() {
    let mut st = a_full_board();
    let before = st.db().view.list.page_size;
    assert_eq!(before, DEFAULT_PAGE_SIZE);

    // `find` takes a query and one flag. There is no page-size parameter to pass, which
    // is the point: `-n` limits what the agent's terminal prints, and the core is never
    // told about it.
    let hits = st.find("", false);
    assert_eq!(hits, 6);
    assert_eq!(st.db().view.list.page_size, before, "a caller's -n must not reach the page");

    let snap = st.snapshot(now());
    assert_eq!(snap["list"]["pageSize"], DEFAULT_PAGE_SIZE);
    assert_eq!(snap["list"]["total"], 6, "both surfaces say N of the same TOTAL");
}

#[test]
fn changing_the_page_size_is_a_deliberate_act_and_moves_both_surfaces() {
    let mut st = a_full_board();
    st.find("", false);
    st.set_page_size(2);

    let snap = st.snapshot(now());
    assert_eq!(snap["list"]["pageSize"], 2);
    assert_eq!(snap["list"]["rows"].as_array().unwrap().len(), 2, "the page is two rows");
    assert_eq!(snap["list"]["total"], 6, "…of six");
}

/// A page that has fallen off the end of a shrinking result set shows the last page rather
/// than an error or an empty table.
#[test]
fn a_page_past_the_end_is_clamped_not_refused() {
    let mut st = a_full_board();
    st.find("", false);
    st.set_page_size(2);
    st.set_page(99);
    assert_eq!(st.db().view.list.page, 2, "six results, two per page, last page is index 2");
    assert_eq!(st.snapshot(now())["list"]["rows"].as_array().unwrap().len(), 2);
}

/// Sort is state for the same reason paging is: reordering only the page you hold is a lie
/// about the data underneath it.
#[test]
fn sort_is_shared_state_and_re_pages_the_whole_result_set() {
    let mut st = state();
    st.add_deal("Zeta", None, Some(Money::new(100, "USD")), None, &ctx());
    st.add_deal("Alpha", None, Some(Money::new(900, "USD")), None, &ctx());
    st.find("", false);

    st.set_sort(Sort::Name);
    let by_name = st.snapshot(now());
    assert_eq!(by_name["list"]["sort"], "name");
    assert_eq!(by_name["list"]["rows"][0]["label"], "Alpha");

    st.set_sort(Sort::Value);
    let by_value = st.snapshot(now());
    assert_eq!(by_value["list"]["sort"], "value");
    assert_eq!(by_value["list"]["rows"][0]["label"], "Alpha", "900 outranks 100");
    assert_eq!(by_value["list"]["page"], 0, "a re-sort starts at the first page");
}

// MARK: - 4. Stage and status are different axes
//
// The part most likely to be got wrong. Closing a deal must leave the stage alone, or
// "how many did we win out of Negotiation" becomes unanswerable.

#[test]
fn winning_a_deal_sets_the_status_and_leaves_the_stage_exactly_where_it_was() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Negotiation), None, &ctx()).unwrap();

    st.move_deal(&id, MoveTarget::Close(Status::Won), None, &at(later(1))).unwrap();

    let db = st.db();
    let deal = db.deal(&id).unwrap();
    assert_eq!(deal.status, Status::Won);
    assert_eq!(deal.stage, Stage::Negotiation, "the stage is what conversion reporting reads");
    assert_eq!(deal.closed_at, Some(later(1).at));
    assert_eq!(deal.updated_at, later(1).at);
    assert_eq!(deal.origin, origin(), "a write records which install made it");
}

#[test]
fn losing_a_deal_behaves_the_same_way() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Proposal), None, &ctx()).unwrap();
    st.move_deal(&id, MoveTarget::Close(Status::Lost), None, &ctx()).unwrap();

    let db = st.db();
    assert_eq!(db.deal(&id).unwrap().status, Status::Lost);
    assert_eq!(db.deal(&id).unwrap().stage, Stage::Proposal);
}

#[test]
fn moving_a_closed_deal_to_a_stage_reopens_it() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.move_deal(&id, MoveTarget::Close(Status::Lost), None, &ctx()).unwrap();

    st.move_deal(&id, MoveTarget::To(Stage::Proposal), None, &at(later(1))).unwrap();

    let db = st.db();
    let deal = db.deal(&id).unwrap();
    assert_eq!(deal.status, Status::Open, "a deal cannot be in Proposal and lost at once");
    assert_eq!(deal.stage, Stage::Proposal);
    assert_eq!(deal.closed_at, None, "reopening clears the closing date it no longer has");
}

/// A closed deal is drawn under how it closed, whatever stage it stopped at — which is why
/// six columns cover four stages.
#[test]
fn a_won_deal_leaves_its_stage_column_for_the_won_one() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Negotiation), None, &ctx()).unwrap();
    assert_eq!(column(&st.snapshot(now()), "negotiation")["count"], 1);

    st.move_deal(&id, MoveTarget::Close(Status::Won), None, &ctx()).unwrap();
    let snap = st.snapshot(now());
    assert_eq!(column(&snap, "negotiation")["count"], 0);
    assert_eq!(column(&snap, "won")["count"], 1);
}

#[test]
fn moving_a_deal_that_does_not_exist_is_an_error_not_a_silent_no_op() {
    let mut st = state();
    assert!(st.move_deal("nope", MoveTarget::To(Stage::Lead), None, &ctx()).is_err());
}

// MARK: - 5. Archive, never delete
//
// An agent holding a delete verb and a bad fuzzy match is an unrecoverable afternoon. So
// archiving hides a record everywhere it would clutter, and hides it nowhere it would lose
// it.

#[test]
fn an_archived_record_leaves_the_board_the_counts_and_the_default_search() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, Some(Money::new(500, "USD")), None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Proposal), None, &ctx()).unwrap();

    let before = st.snapshot(now());
    assert_eq!(before["counts"]["deals"], 1);
    assert_eq!(column(&before, "proposal")["count"], 1);
    assert_eq!(st.find("acme", false), 1);

    st.archive(&id, &at(later(1))).unwrap();

    let after = st.snapshot(now());
    assert_eq!(after["counts"]["deals"], 0, "counts are of active records");
    assert_eq!(column(&after, "proposal")["count"], 0, "and it leaves the board");
    assert!(
        column(&after, "proposal")["totals"].as_array().unwrap().is_empty(),
        "an archived deal's value must not still be counted in the pipeline"
    );
    assert_eq!(st.find("acme", false), 0, "…and the default search");
}

#[test]
fn an_archived_record_is_still_loadable_by_show() {
    let mut st = state();
    let id = st.add_company("Acme Corp", &ctx());
    st.archive(&id, &ctx()).unwrap();

    assert!(st.show(&id).is_ok(), "archiving is not deleting");
    let snap = st.snapshot(now());
    assert_eq!(snap["focus"]["id"], id.as_str());
    assert_eq!(snap["focus"]["kind"], "company");
    assert_eq!(snap["focus"]["handle"], "acme-corp", "…and still names what you would type");
    assert!(st.is_archived(&id));
}

#[test]
fn an_archived_record_can_be_found_on_purpose_and_says_that_it_is_archived() {
    let mut st = state();
    let id = st.add_company("Acme Corp", &ctx());
    st.archive(&id, &ctx()).unwrap();

    assert_eq!(st.find("acme", true), 1, "asked for explicitly, it is there");
    assert_eq!(st.snapshot(now())["list"]["rows"][0]["archived"], true);
}

#[test]
fn restoring_brings_a_record_all_the_way_back() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.archive(&id, &ctx()).unwrap();
    st.restore(&id, &at(later(1))).unwrap();

    assert!(!st.is_archived(&id));
    assert_eq!(st.snapshot(now())["counts"]["deals"], 1);
    assert_eq!(st.find("acme", false), 1);
}

// MARK: - 6. Attribution is the feature
//
// A shared log where you cannot tell who said what is a log two parties stop trusting.

#[test]
fn an_activity_logged_by_an_agent_records_that_agent_s_id() {
    let mut st = state();
    // The link is a real record's **id**, not its handle: the fixture used to read
    // `vec!["acme"]`, which was an id before the revision and is a handle after it.
    let acme = st.add_company("Acme Corp", &ctx());
    st.log(ActivityKind::Call, "Rang about the renewal", vec![acme.clone()], Some("agent-7"), &ctx());

    let db = st.db();
    assert_eq!(db.activities[0].by, Actor::Agent { id: "agent-7".into() });
    assert_eq!(db.activities[0].kind, ActivityKind::Call);
    assert_eq!(db.activities[0].links, vec![acme]);
}

#[test]
fn an_activity_logged_at_the_window_records_the_person() {
    let mut st = state();
    st.log(ActivityKind::Note, "Left a voicemail", vec![], None, &ctx());
    assert_eq!(st.db().activities[0].by, Actor::Human);
    assert!(st.db().activities[0].by.is_human());
}

/// The id is immutable; the name is a re-pointable label. Storing the name would silently
/// re-attribute every line an agent ever wrote the day somebody renamed it.
#[test]
fn attribution_is_keyed_on_the_agent_id_and_never_on_its_display_name() {
    let mut st = state();
    st.log(ActivityKind::Email, "Sent the quote", vec![], Some("a-1"), &ctx());
    st.set_agents(vec![AgentRow {
        id: "a-1".into(),
        name: "Scout".into(),
        backend: Some("claude".into()),
        model: None,
        avatar: None,
    }]);

    let by = &st.db().activities[0].by;
    assert_eq!(by, &Actor::Agent { id: "a-1".into() });
    assert_eq!(serde_json::to_value(by).unwrap()["id"], "a-1");
    assert!(
        !serde_json::to_string(by).unwrap().contains("Scout"),
        "the display name must not be what the log is keyed on"
    );

    // A rename is a fresh roster with the same id. The line stays attributed.
    st.set_agents(vec![AgentRow {
        id: "a-1".into(),
        name: "Ranger".into(),
        backend: Some("claude".into()),
        model: None,
        avatar: None,
    }]);
    assert_eq!(st.db().activities[0].by, Actor::Agent { id: "a-1".into() });
    assert_eq!(st.snapshot(now())["agents"][0]["name"], "Ranger");
}

#[test]
fn a_task_records_who_set_it_too() {
    let mut st = state();
    st.add_task("Follow up", Date::new(2026, 9, 10), vec![], Some("a-2"), &ctx());
    assert_eq!(st.db().tasks[0].by, Actor::Agent { id: "a-2".into() });
}

/// The log is append-only. Two lines written in the same millisecond must both survive —
/// an id collision here would silently drop one side of a conversation.
#[test]
fn two_activities_in_the_same_millisecond_are_both_kept() {
    let mut st = state();
    st.log(ActivityKind::Note, "first", vec![], None, &ctx());
    st.log(ActivityKind::Note, "second", vec![], None, &ctx());
    let db = st.db();
    assert_eq!(db.activities.len(), 2);
    assert_ne!(db.activities[0].id, db.activities[1].id);
}

// MARK: - 7. Totals group by currency
//
// We hold no rate source, and this app reaches nothing outside the machine. Two honest
// numbers beat one invented one.

#[test]
fn a_column_s_totals_are_grouped_by_currency_and_never_summed_across_them() {
    let mut st = state();
    for value in [Money::new(4_500_000, "USD"), Money::new(1_000_000, "EUR"), Money::new(500_000, "USD")] {
        st.add_deal(&format!("Deal {}", value.currency), None, Some(value), None, &ctx());
    }

    let totals = column(&st.snapshot(now()), "lead")["totals"].clone();
    assert_eq!(
        totals,
        // Round 3 §4: every money value on the wire carries its formatted string beside
        // the raw amount, which stays for sorting and comparison.
        json!([
            { "currency": "EUR", "amount": 1_000_000, "formatted": "€10,000.00" },
            { "currency": "USD", "amount": 5_000_000, "formatted": "$50,000.00" },
        ]),
        "two currencies, two numbers, in a stable order"
    );
}

#[test]
fn an_empty_pipeline_totals_nothing_rather_than_negative_zero() {
    let snap = state().snapshot(now());
    for column in snap["board"]["columns"].as_array().unwrap() {
        assert_eq!(column["count"], 0);
        assert!(column["totals"].as_array().unwrap().is_empty(), "no deals, no totals");
    }
    // And when there is a zero to print, it prints as one. `amount` is an integer, so
    // `-0.00` is not a bug that was fixed — it is a state that cannot be represented.
    // (Round 3 made `format()` the display form, so the zero is `$0.00`.)
    assert_eq!(Money::new(0, "USD").format(), "$0.00");
    assert!(!Money::new(0, "USD").format().contains('-'));
}

#[test]
fn a_deal_with_no_value_is_counted_but_contributes_no_total() {
    let mut st = state();
    st.add_deal("Unpriced", None, None, None, &ctx());
    st.add_deal("Priced", None, Some(Money::new(100, "USD")), None, &ctx());

    let snap = st.snapshot(now());
    let lead = column(&snap, "lead");
    assert_eq!(lead["count"], 2, "a deal without a price is still a deal");
    assert_eq!(lead["totals"], json!([{ "currency": "USD", "amount": 100, "formatted": "$1.00" }]));
}

// MARK: - 8. Ambiguity is a state, not a guess and not an error

#[test]
fn two_close_matches_park_a_question_rather_than_picking_one() {
    let mut st = state();
    st.add_company("Acme Corp", &ctx());
    st.add_company("Acme Industries", &ctx());

    let resolved = st.resolve("acme", false);
    let Resolved::Ambiguous(candidates) = resolved else {
        panic!("two equally good matches must not be resolved silently: {resolved:?}");
    };
    assert_eq!(candidates.len(), 2);

    st.park("which Acme?", candidates);
    let snap = st.snapshot(now());
    assert_eq!(snap["pending"]["prompt"], "which Acme?");
    assert_eq!(snap["pending"]["candidates"].as_array().unwrap().len(), 2);
    // Each candidate says what to type, not what it is keyed on.
    assert_eq!(snap["pending"]["candidates"][0]["handle"], "acme-corp");
    assert_eq!(snap["pending"]["candidates"][1]["handle"], "acme-industries");
    // The candidates go into the list both surfaces already render — there is no second
    // list to drift.
    assert_eq!(snap["list"]["total"], 2);
    assert_eq!(snap["list"]["rows"].as_array().unwrap().len(), 2);
}

#[test]
fn an_exact_handle_match_is_decisive_and_parks_nothing() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    st.add_company("Acme Industries", &ctx());
    assert_eq!(handle(&st, &acme), "acme-corp");

    // What an agent types is the handle — and it resolves to the id, decisively, even
    // though another record matches "acme" just as well by name.
    assert_eq!(st.resolve("acme-corp", false), Resolved::One(Kind::Company, acme.clone()));
    // Case-folded, because an agent that types `Acme-Corp` means the same record.
    assert_eq!(st.resolve("ACME-CORP", false), Resolved::One(Kind::Company, acme.clone()));
    // And an id, for a caller that already holds one — a click on a row, or a value read
    // straight back out of a snapshot.
    assert_eq!(st.resolve(&acme, false), Resolved::One(Kind::Company, acme.clone()));
    assert_eq!(st.snapshot(now())["pending"], Value::Null, "no question was asked");
}

/// A clear winner is not ambiguity. "Acme Corp" matches its own name exactly; "Acme
/// Industries" does not match at all.
#[test]
fn a_clear_winner_is_not_ambiguity() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    st.add_company("Acme Industries", &ctx());
    assert_eq!(st.resolve("Acme Corp", false), Resolved::One(Kind::Company, acme));
}

#[test]
fn nothing_matching_is_an_answer_not_a_question() {
    let mut st = state();
    st.add_company("Acme Corp", &ctx());
    assert_eq!(st.resolve("hooli", false), Resolved::None);
    assert_eq!(st.resolve("", false), Resolved::None, "an empty lookup names nothing");
}

/// Either surface answers the same question: `crm select 2`, or a click on the row.
#[test]
fn either_surface_answers_a_parked_question_and_it_clears() {
    let mut st = state();
    st.add_company("Acme Corp", &ctx());
    st.add_company("Acme Industries", &ctx());
    let Resolved::Ambiguous(candidates) = st.resolve("acme", false) else {
        panic!("expected two candidates");
    };
    st.park("which Acme?", candidates);

    let chosen = st.select(2).unwrap();
    assert_eq!(handle(&st, &chosen), "acme-industries");

    let snap = st.snapshot(now());
    assert_eq!(snap["pending"], Value::Null, "answering clears the question");
    assert_eq!(snap["focus"]["id"], chosen.as_str(), "…and opens what was chosen");
    assert_eq!(snap["focus"]["handle"], "acme-industries");
}

#[test]
fn selecting_outside_the_candidates_says_what_the_choices_were() {
    let mut st = state();
    st.add_company("Acme Corp", &ctx());
    st.add_company("Acme Industries", &ctx());
    let Resolved::Ambiguous(c) = st.resolve("acme", false) else { panic!() };
    st.park("which Acme?", c);

    let err = st.select(5).unwrap_err();
    assert!(err.contains('2'), "the refusal must say how many there were: {err}");
    assert!(st.db().view.pending.is_some(), "a bad answer leaves the question standing");
}

/// With nothing parked, `select N` opens result N of the shared list — the second thing
/// the verb is documented to do.
#[test]
fn select_with_no_question_pending_opens_result_n_of_the_shared_list() {
    let mut st = state();
    st.add_company("Acme Corp", &ctx());
    st.add_company("Hooli", &ctx());
    st.find("", false);
    st.set_sort(Sort::Name);

    let chosen = st.select(2).unwrap();
    assert_eq!(handle(&st, &chosen), "hooli");
    assert_eq!(st.snapshot(now())["focus"]["id"], chosen.as_str());
}

// MARK: - Identity: a ULID that nobody types, a handle that everybody does
//
// Name-derived ids do not survive a second machine: two installs both create "Acme Corp",
// both mint `acme`, and on sync nothing can say whether that is one company or two. So the
// identity is a ULID and the typable name is a separate, non-referencing handle.

/// The acceptance case, and the reason the change was made at all.
#[test]
fn two_records_from_the_same_name_get_different_ids_and_different_handles() {
    let mut st = state();
    let first = st.add_company("Acme Corp.", &ctx());
    let second = st.add_company("Acme Corp.", &ctx());

    assert_ne!(first, second, "two records are two identities, whatever they are called");
    assert!(Ulid::parse(&first).is_some(), "`{first}` is not a ULID");
    assert!(Ulid::parse(&second).is_some(), "`{second}` is not a ULID");

    assert_eq!(handle(&st, &first), "acme-corp");
    assert_eq!(handle(&st, &second), "acme-corp-2", "the typable name is uniquified too");
}

/// Handles are unique across the whole dataset, not per type — `crm show acme` must not
/// have to be told which kind of thing `acme` is.
#[test]
fn a_handle_is_unique_across_every_record_type() {
    let mut st = state();
    let company = st.add_company("Acme Corp.", &ctx());
    let deal = st.add_deal("Acme Corp.", None, None, None, &ctx());
    assert_eq!(handle(&st, &company), "acme-corp");
    assert_eq!(handle(&st, &deal), "acme-corp-2");
}

/// The other acceptance case. A rename moves the label and nothing else: the id is what
/// every reference stores, and the handle is what somebody has already written down.
#[test]
fn a_rename_changes_neither_the_id_nor_the_handle() {
    let mut st = state();
    let id = st.add_company("Acme Corp", &ctx());
    let before = handle(&st, &id);

    st.rename(&id, "Acme Industries GmbH", &at(later(1))).unwrap();

    assert_eq!(handle(&st, &id), before, "a handle somebody wrote down still works");
    assert_eq!(st.db().company(&id).unwrap().name, "Acme Industries GmbH");
    assert_eq!(st.resolve("acme-corp", false), Resolved::One(Kind::Company, id.clone()));
    // …and the rename is a write like any other.
    assert_eq!(st.db().company(&id).unwrap().updated_at, later(1).at);
    assert_eq!(st.db().company(&id).unwrap().origin, origin());
}

#[test]
fn a_rename_finds_the_record_under_its_new_name_too() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.rename(&id, "Acme expansion", &ctx()).unwrap();

    st.find("expansion", false);
    assert_eq!(st.snapshot(now())["list"]["rows"][0]["label"], "Acme expansion");
    assert_eq!(st.snapshot(now())["list"]["rows"][0]["handle"], "acme-renewal");
}

#[test]
fn renaming_something_that_does_not_exist_or_to_nothing_is_an_error() {
    let mut st = state();
    let id = st.add_company("Acme Corp", &ctx());
    assert!(st.rename("nobody", "Anything", &ctx()).is_err());
    assert!(st.rename(&id, "   ", &ctx()).is_err(), "a nameless record is not an edit");
}

/// Ids are minted, not derived — but they still have to be *ids*: distinct, and in the
/// order the records were made, even when a whole board is built inside one millisecond.
#[test]
fn every_record_in_one_command_still_gets_its_own_id() {
    let st = a_full_board();
    let db = st.db();
    let ids: Vec<&str> = db.deals.iter().map(|d| d.id.as_str()).collect();

    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "an id was minted twice: {ids:?}");

    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, ids, "ids must sort in the order the records were created");
}

/// Nothing stores a handle. That is what lets a workspace that merges with another
/// re-handle a collision without orphaning a single pointer.
#[test]
fn every_reference_stores_an_id_and_never_a_handle() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let ada = st.add_contact("Ada Lovelace", Some(&acme), &ctx());
    let deal = st.add_deal("Acme renewal", Some(&acme), None, None, &ctx());
    st.link(&deal, &ada, &ctx()).unwrap();
    st.log(ActivityKind::Call, "Rang", vec![acme.clone()], None, &ctx());
    st.add_task("Follow up", Date::new(2026, 9, 10), vec![deal.clone()], None, &ctx());

    let db = st.db();
    assert_eq!(db.contact(&ada).unwrap().company_id, Some(acme.clone()));
    assert_eq!(db.deal(&deal).unwrap().company_id, Some(acme.clone()));
    assert_eq!(db.deal(&deal).unwrap().contact_ids, vec![ada]);
    assert_eq!(db.activities[0].links, vec![acme]);
    assert_eq!(db.tasks[0].links, vec![deal]);

    for reference in db.references() {
        assert!(Ulid::parse(&reference).is_some(), "`{reference}` is a handle, not an id");
    }
}

// MARK: - The shared view

#[test]
fn showing_a_record_opens_it_in_the_other_surface() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.show(&id).unwrap();
    let snap = st.snapshot(now());
    assert_eq!(snap["focus"], json!({ "kind": "deal", "id": id, "handle": "acme-renewal" }));
}

#[test]
fn a_stage_filter_narrows_the_board_for_both_surfaces() {
    let mut st = a_full_board();
    st.set_stage_filter(Some(Stage::Proposal));

    let snap = st.snapshot(now());
    assert_eq!(snap["board"]["stageFilter"], "proposal");
    let columns = snap["board"]["columns"].as_array().unwrap();
    assert_eq!(columns.len(), 1, "a filter one surface knew about would be drift");
    assert_eq!(columns[0]["key"], "proposal");

    st.set_stage_filter(None);
    let all = st.snapshot(now());
    assert_eq!(all["board"]["stageFilter"], Value::Null);
    assert_eq!(all["board"]["columns"].as_array().unwrap().len(), 6);
}

#[test]
fn linking_is_idempotent_so_a_retry_cannot_corrupt_the_graph() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let ada = st.add_contact("Ada Lovelace", Some(&acme), &ctx());
    let deal = st.add_deal("Acme renewal", None, None, None, &ctx());

    st.link(&deal, &ada, &ctx()).unwrap();
    st.link(&deal, &ada, &ctx()).unwrap();
    st.link(&deal, &acme, &ctx()).unwrap();

    let db = st.db();
    assert_eq!(db.deal(&deal).unwrap().contact_ids, vec![ada]);
    assert_eq!(db.deal(&deal).unwrap().company_id, Some(acme));
}

#[test]
fn linking_something_that_does_not_exist_is_an_error() {
    let mut st = state();
    let deal = st.add_deal("Acme renewal", None, None, None, &ctx());
    assert!(st.link(&deal, "nobody", &ctx()).is_err());
    assert!(st.link("nothing", &deal, &ctx()).is_err());
}

// MARK: - Next steps and the due buckets

#[test]
fn due_buckets_are_disjoint_so_the_three_numbers_can_be_read_side_by_side() {
    let mut st = state();
    st.add_task("Late one", Date::new(2026, 9, 1), vec![], None, &ctx());
    st.add_task("Late two", Date::new(2026, 9, 7), vec![], None, &ctx());
    st.add_task("Today", Date::new(2026, 9, 8), vec![], None, &ctx());
    st.add_task("Thursday", Date::new(2026, 9, 10), vec![], None, &ctx());
    st.add_task("Next week", Date::new(2026, 9, 30), vec![], None, &ctx());

    let snap = st.snapshot(now());
    assert_eq!(snap["due"], json!({ "overdue": 2, "today": 1, "week": 1 }));
    assert_eq!(snap["counts"]["tasks"], 5);
}

#[test]
fn a_completed_task_stops_being_due_and_stops_being_counted() {
    let mut st = state();
    let id = st.add_task("Follow up", Date::new(2026, 9, 1), vec![], None, &ctx());
    assert_eq!(st.snapshot(now())["due"]["overdue"], 1);

    st.complete_task(&id, &at(later(1))).unwrap();
    let snap = st.snapshot(now());
    assert_eq!(snap["due"]["overdue"], 0);
    assert_eq!(snap["counts"]["tasks"], 0);
    assert_eq!(st.db().tasks[0].done_at, Some(later(1).at), "…but the task is still on record");
}

#[test]
fn completing_a_task_that_does_not_exist_is_an_error() {
    assert!(state().complete_task("nope", &ctx()).is_err());
}

// MARK: - The snapshot as a whole

/// The frozen shape. M3 builds the window against these keys, so a rename here is a
/// breaking change to another milestone's work — which is exactly what this test is for.
#[test]
fn the_snapshot_carries_every_key_the_surfaces_were_promised() {
    let mut st = a_full_board();
    st.find("", false); // the list has to hold something for its row shape to be checked
    let snap = st.snapshot(now());
    for key in ["ok", "rev", "pipeline", "board", "focus", "list", "pending", "due", "counts", "agents"] {
        assert!(snap.get(key).is_some(), "the snapshot lost `{key}`");
    }
    for key in ["id", "name", "stages"] {
        assert!(snap["pipeline"].get(key).is_some(), "pipeline lost `{key}`");
    }
    for key in ["pipelineId", "stageFilter", "columns"] {
        assert!(snap["board"].get(key).is_some(), "board lost `{key}`");
    }
    for key in ["key", "label", "dealIds", "count", "totals"] {
        assert!(snap["board"]["columns"][0].get(key).is_some(), "a column lost `{key}`");
    }
    for key in ["query", "sort", "page", "pageSize", "total", "rows"] {
        assert!(snap["list"].get(key).is_some(), "list lost `{key}`");
    }
    // Additive only: `kind` and `id` are exactly what M3 was given, and `handle` is new
    // beside them. Nothing was removed, renamed or retyped.
    for key in ["kind", "id", "label", "detail", "stage", "status", "value", "archived", "handle"] {
        assert!(snap["list"]["rows"][0].get(key).is_some(), "a row lost `{key}`");
    }
    for key in ["overdue", "today", "week"] {
        assert!(snap["due"].get(key).is_some(), "due lost `{key}`");
    }
    for key in ["companies", "contacts", "deals", "activities", "tasks"] {
        assert!(snap["counts"].get(key).is_some(), "counts lost `{key}`");
    }
}

/// A row has one shape whether it is a company, a contact or a deal, so a table has one
/// thing to draw. The deal-only fields are null rather than absent.
#[test]
fn every_row_has_the_same_shape() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    st.add_contact("Ada Lovelace", Some(&acme), &ctx());
    st.add_deal("Acme renewal", Some(&acme), Some(Money::new(100, "USD")), None, &ctx());
    st.find("", false);

    let snap = st.snapshot(now());
    let rows = snap["list"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        for key in ["kind", "id", "handle", "label", "detail", "stage", "status", "value", "archived"] {
            assert!(row.get(key).is_some(), "a row lost `{key}`: {row}");
        }
    }
    let deal = rows.iter().find(|r| r["kind"] == "deal").unwrap();
    assert_eq!(deal["stage"], "lead");
    assert_eq!(deal["detail"], "Acme Corp", "a deal names its company");
    let company = rows.iter().find(|r| r["kind"] == "company").unwrap();
    assert_eq!(company["stage"], Value::Null, "null, not absent");
}

#[test]
fn the_answer_and_the_pushed_snapshot_share_one_revision() {
    let mut st = state();
    let out = st.command(&json!({ "cmd": "status" }), None, &ctx());
    assert_eq!(out.resp["rev"], out.snapshot["rev"]);
    assert!(out.resp["rev"].as_u64().unwrap() > 0, "0 is reserved for an unstamped snapshot");
}

#[test]
fn revisions_only_ever_go_up() {
    let st = state();
    let first = st.snapshot(now())["rev"].as_u64().unwrap();
    let second = st.snapshot(now())["rev"].as_u64().unwrap();
    assert!(second > first);
}

#[test]
fn the_window_and_the_cli_ask_the_same_question() {
    let mut st = a_full_board();
    let window = st.command(&json!({ "cmd": "state" }), None, &ctx()).resp;
    let agent = st.command(&json!({ "cmd": "status" }), Some("a-1"), &ctx()).resp;
    assert_eq!(window["counts"], agent["counts"], "one state, one answer");
    assert_eq!(window["board"], agent["board"]);
}

#[test]
fn an_unknown_verb_is_refused_and_points_at_the_manual() {
    let mut st = state();
    let out = st.command(&json!({ "cmd": "teleport" }), None, &ctx());
    assert_eq!(out.resp["ok"], false);
    assert!(out.resp["error"].as_str().unwrap().contains("crm -h"));
    assert!(!out.dirty, "a refusal changed nothing, so it owes no write");
}

/// A snapshot goes everywhere. v1 holds no credentials, so this passes trivially today —
/// which is the point of writing it now rather than the day it can fail.
#[test]
fn nothing_secret_is_in_the_snapshot() {
    let snap = a_full_board().snapshot(now()).to_string().to_lowercase();
    for word in ["password", "secret", "token", "apikey", "api_key", "credential"] {
        assert!(!snap.contains(word), "`{word}` reached a snapshot");
    }
}

/// A read never owes a signal, whoever asked. The write verbs' own emit tests are in the
/// M2 section below, alongside the caller-gate they all share.
#[test]
fn a_read_emits_nothing_from_either_surface() {
    let mut st = state();
    assert!(st.command(&json!({ "cmd": "status" }), None, &ctx()).emits.is_empty());
    assert!(st.command(&json!({ "cmd": "status" }), Some("a-1"), &ctx()).emits.is_empty());
}

// MARK: - Round 3: the snapshot the window can actually draw
//
// `docs/work-orders/round-3-snapshot.md`. Every addition is additive: the tests further up
// that pin the pre-round-3 keys are unchanged, and still pass.

/// An agent id in the shape Clatch actually issues — a numeric string, not a ULID.
const AGENT: &str = "1789126979";

fn agent_row() -> AgentRow {
    AgentRow {
        id: AGENT.into(),
        name: "Scout".into(),
        backend: Some("claude".into()),
        model: None,
        avatar: None,
    }
}

fn human() -> Value {
    json!({ "kind": "human" })
}

fn agent() -> Value {
    json!({ "kind": "agent", "id": AGENT })
}

/// Any run of Crockford base32 long enough to be a ULID, anywhere in a string.
fn first_ulid_in(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|word| Ulid::parse(word).is_some())
        .map(str::to_string)
}

// -- §1 cards ----------------------------------------------------------------------------

#[test]
fn every_deal_on_the_board_has_a_card_and_nothing_else_does() {
    let mut st = a_full_board();
    let archived = st.add_deal("Retired deal", None, None, None, &ctx());
    st.archive(&archived, &ctx()).unwrap();
    st.add_company("Acme Corp", &ctx());

    let snap = st.snapshot(now());
    let on_board: std::collections::BTreeSet<String> = snap["board"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["dealIds"].as_array().unwrap().iter())
        .map(|id| id.as_str().unwrap().to_string())
        .collect();
    let carded: std::collections::BTreeSet<String> =
        snap["cards"].as_object().unwrap().keys().cloned().collect();

    assert_eq!(on_board.len(), 6);
    assert_eq!(carded, on_board, "one card per id on the board, and no others");
    assert!(!carded.contains(&archived), "an archived deal is off the board, so off the cards");
}

#[test]
fn a_card_is_the_row_plus_who_last_moved_it() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, Some(Money::new(4_500_000, "USD")), None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Proposal), Some(AGENT), &at(later(1))).unwrap();

    let snap = st.snapshot(now());
    let card = &snap["cards"][&id];
    for key in ["kind", "id", "handle", "label", "detail", "stage", "status", "value", "archived"] {
        assert!(card.get(key).is_some(), "a card lost the row's `{key}`: {card}");
    }
    assert_eq!(card["handle"], "acme-renewal");
    assert_eq!(card["stage"], "proposal");
    assert_eq!(card["by"], agent(), "the agent that moved it, keyed on its id");
    assert_eq!(card["movedAt"], later(1).at);
    assert_eq!(card["value"]["formatted"], "$45,000.00");
}

/// A stage move is not an activity. If `by` were derived from the latest activity, it
/// would name whoever last logged a call — and the ring would tint the wrong agent.
#[test]
fn logging_a_call_does_not_change_who_moved_the_card() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Qualified), None, &at(later(1))).unwrap();

    st.log(ActivityKind::Call, "Rang them", vec![id.clone()], Some(AGENT), &at(later(2)));
    st.rename(&id, "Acme renewal 2027", &at(later(3))).unwrap();

    let card = &st.snapshot(now())["cards"][&id];
    assert_eq!(card["by"], human(), "the person moved it; the agent only called");
    assert_eq!(card["movedAt"], later(1).at, "renaming and logging are not moves");
}

#[test]
fn creating_a_deal_is_its_first_move() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, Some(AGENT), &ctx());
    let db = st.db();
    let deal = db.deal(&id).unwrap();
    assert_eq!(deal.moved_by, Actor::Agent { id: AGENT.into() });
    assert_eq!(deal.moved_at, ctx().at());
}

#[test]
fn a_stage_filter_narrows_the_cards_with_the_board() {
    let mut st = a_full_board();
    st.set_stage_filter(Some(Stage::Proposal));
    let snap = st.snapshot(now());
    assert_eq!(snap["cards"].as_object().unwrap().len(), 1);
}

/// A data file written before round 3 has no `movedBy` or `movedAt`. It must still open —
/// and say the person, which is the only honest guess about a move nobody recorded.
#[test]
fn a_deal_saved_before_moved_by_existed_still_loads() {
    let mut st = state();
    st.add_deal("Acme renewal", None, None, Some(AGENT), &ctx());
    let mut raw = serde_json::to_value(st.db()).unwrap();
    let deal = raw["deals"][0].as_object_mut().unwrap();
    deal.remove("movedBy");
    deal.remove("movedAt");
    deal.remove("handle");

    let db: Db = serde_json::from_value(raw).expect("an older file must still load");
    assert_eq!(db.deals[0].moved_by, Actor::Human);
    assert_eq!(db.deals[0].moved_at, 0);
}

// -- §2 focused --------------------------------------------------------------------------

#[test]
fn focused_is_present_exactly_when_focus_is() {
    let mut st = state();
    let snap = st.snapshot(now());
    assert_eq!(snap["focus"], Value::Null);
    assert!(snap.get("focused").is_none(), "absent, not null, when nothing is open");

    let id = st.add_company("Acme Corp", &ctx()); // adding opens it
    let snap = st.snapshot(now());
    assert_eq!(snap["focus"]["id"], id.as_str());
    assert_eq!(snap["focused"]["row"]["id"], id.as_str());
    assert_eq!(snap["focused"]["row"]["handle"], "acme-corp");
}

#[test]
fn the_timeline_is_newest_first_and_capped_with_a_total() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    let other = st.add_deal("Hooli platform", None, None, None, &ctx());
    for i in 0..60 {
        st.log(ActivityKind::Note, &format!("note {i}"), vec![id.clone()], None, &at(later(i)));
    }
    st.log(ActivityKind::Note, "about the other deal", vec![other], None, &ctx());
    st.show(&id).unwrap();

    let focused = &st.snapshot(now())["focused"];
    let timeline = focused["timeline"].as_array().unwrap();
    assert_eq!(timeline.len(), TIMELINE_CAP, "a push must not carry a record's whole history");
    assert_eq!(focused["timelineTotal"], 60, "…but it says how much there is");
    assert_eq!(timeline[0]["body"], "note 59", "newest first");
    assert_eq!(timeline[49]["body"], "note 10");
    assert!(timeline.iter().all(|a| a["body"] != "about the other deal"), "only this record's lines");
    for key in ["id", "kind", "body", "at", "by"] {
        assert!(timeline[0].get(key).is_some(), "a timeline entry lost `{key}`");
    }
}

/// Two lines in one millisecond must still read in the order they were written. The id
/// is a ULID, so it breaks the tie in creation order.
#[test]
fn two_lines_in_one_millisecond_keep_their_order() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    st.log(ActivityKind::Note, "first", vec![id.clone()], None, &ctx());
    st.log(ActivityKind::Note, "second", vec![id.clone()], None, &ctx());
    st.show(&id).unwrap();

    let timeline = &st.snapshot(now())["focused"]["timeline"];
    assert_eq!(timeline[0]["body"], "second");
    assert_eq!(timeline[1]["body"], "first");
}

#[test]
fn tasks_are_open_first_soonest_first_and_carry_a_handle() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, None, &ctx());
    let later_task = st.add_task("Send contract", Date::new(2026, 9, 20), vec![id.clone()], None, &ctx());
    let sooner = st.add_task("Call back", Date::new(2026, 9, 10), vec![id.clone()], Some(AGENT), &ctx());
    let done = st.add_task("Intro call", Date::new(2026, 9, 1), vec![id.clone()], None, &ctx());
    st.complete_task(&done, &at(later(1))).unwrap();
    st.show(&id).unwrap();

    let tasks = st.snapshot(now())["focused"]["tasks"].as_array().unwrap().clone();
    let order: Vec<&str> = tasks.iter().map(|t| t["what"].as_str().unwrap()).collect();
    assert_eq!(order, ["Call back", "Send contract", "Intro call"], "open first, soonest first");

    assert_eq!(tasks[0]["id"], sooner.as_str());
    assert_eq!(tasks[0]["handle"], "call-back", "`crm done <task-handle>` has something to type");
    assert_eq!(tasks[0]["due"], "2026-09-10", "a civil date, in its one spelling");
    assert_eq!(tasks[0]["doneAt"], Value::Null);
    assert_eq!(tasks[0]["by"], agent());
    assert_eq!(tasks[2]["doneAt"], later(1).at);
    assert_eq!(tasks[1]["id"], later_task.as_str());
}

#[test]
fn task_handles_share_the_one_namespace() {
    let mut st = state();
    st.add_company("Call back", &ctx());
    let t1 = st.add_task("Call back", Date::new(2026, 9, 10), vec![], None, &ctx());
    let t2 = st.add_task("Call back", Date::new(2026, 9, 11), vec![], None, &ctx());
    let db = st.db();
    let handles: Vec<&str> = db.tasks.iter().map(|t| t.handle.as_str()).collect();
    assert_eq!(handles, ["call-back-2", "call-back-3"], "no typed word may mean two things");
    assert_eq!(db.task_by_handle("call-back-2").unwrap().id, t1);
    assert_eq!(db.task_by_handle("CALL-BACK-3").unwrap().id, t2);
    assert!(db.by_handle("call-back-2").is_none(), "a task is not a record `show` opens");
}

// -- §2 fields ---------------------------------------------------------------------------

#[test]
fn a_deal_s_fields_name_related_records_by_label_and_handle() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let ada = st.add_contact("Ada Lovelace", Some(&acme), &ctx());
    let deal = st.add_deal("Acme renewal", Some(&acme), Some(Money::new(4_500_000, "USD")), None, &ctx());
    st.link(&deal, &ada, &ctx()).unwrap();
    st.move_deal(&deal, MoveTarget::Close(Status::Won), None, &ctx()).unwrap();

    let fields: Vec<(String, String)> =
        st.fields(&deal).into_iter().map(|f| (f.label, f.value)).collect();
    assert_eq!(
        fields,
        vec![
            ("Company".into(), "Acme Corp (acme-corp)".into()),
            ("Contacts".into(), "Ada Lovelace (ada-lovelace)".into()),
            ("Value".into(), "$45,000.00".into()),
            ("Stage".into(), "Lead".into()),
            ("Status".into(), "Won".into()),
        ],
        "the order is part of the contract: the window and `crm show` both follow it"
    );
}

#[test]
fn a_company_s_fields_count_its_open_deals_by_currency() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    st.add_contact("Ada Lovelace", Some(&acme), &ctx());
    st.add_deal("One", Some(&acme), Some(Money::new(100_000, "USD")), None, &ctx());
    st.add_deal("Two", Some(&acme), Some(Money::new(50_000, "EUR")), None, &ctx());
    let closed = st.add_deal("Three", Some(&acme), Some(Money::new(999, "USD")), None, &ctx());
    st.move_deal(&closed, MoveTarget::Close(Status::Lost), None, &ctx()).unwrap();

    let fields = st.fields(&acme);
    let labels: Vec<&str> = fields.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(labels, ["Contacts", "Open deals"], "empty fields are omitted, not dashed");
    assert_eq!(fields[1].value, "2 · €500.00, $1,000.00", "never summed across currencies");
}

#[test]
fn a_contact_s_fields_follow_the_same_rules() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let ada = st.add_contact("Ada Lovelace", Some(&acme), &ctx());
    let labels: Vec<String> = st.fields(&ada).into_iter().map(|f| f.label).collect();
    assert_eq!(labels, ["Company"]);
}

#[test]
fn a_long_list_of_related_records_is_cut_with_a_count() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    for i in 0..8 {
        st.add_contact(&format!("Person {i}"), Some(&acme), &ctx());
    }
    let contacts = st.fields(&acme).into_iter().find(|f| f.label == "Contacts").unwrap();
    assert!(contacts.value.ends_with("and 3 more"), "{}", contacts.value);
}

#[test]
fn the_snapshot_s_fields_are_that_function_s_output() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let deal = st.add_deal("Acme renewal", Some(&acme), None, None, &ctx());
    st.show(&deal).unwrap();
    let from_snapshot = st.snapshot(now())["focused"]["fields"].clone();
    assert_eq!(from_snapshot, serde_json::to_value(st.fields(&deal)).unwrap());
}

// -- §3 list.kind ------------------------------------------------------------------------

#[test]
fn the_list_can_be_narrowed_to_one_kind_and_both_surfaces_see_it() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    st.add_contact("Ada Lovelace", Some(&acme), &ctx());
    st.add_contact("Grace Hopper", None, &ctx());
    st.add_deal("Acme renewal", Some(&acme), None, None, &ctx());

    st.set_list_kind(Some(Kind::Contact));
    let snap = st.snapshot(now());
    assert_eq!(snap["list"]["kind"], "contact");
    assert_eq!(snap["list"]["total"], 2, "the footer counts what the rows show");
    assert!(snap["list"]["rows"].as_array().unwrap().iter().all(|r| r["kind"] == "contact"));

    st.set_list_kind(None);
    let snap = st.snapshot(now());
    assert_eq!(snap["list"]["kind"], Value::Null, "null is all three");
    assert_eq!(snap["list"]["total"], 4);
}

// -- §4 money ----------------------------------------------------------------------------

#[test]
fn every_money_value_on_the_wire_carries_its_formatted_string() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let deal = st.add_deal("Acme renewal", Some(&acme), Some(Money::new(4_500_000, "USD")), None, &ctx());
    st.find("", false);
    let snap = st.snapshot(now());

    let row = snap["list"]["rows"].as_array().unwrap().iter().find(|r| r["kind"] == "deal").unwrap();
    let card = &snap["cards"][&deal];
    let total = &column(&snap, "lead")["totals"][0];
    for money in [&row["value"], &card["value"], total] {
        assert_eq!(money["amount"], 4_500_000, "the raw amount stays, for sorting");
        assert_eq!(money["currency"], "USD");
        assert_eq!(money["formatted"], "$45,000.00", "and the one formatted string rides beside it");
    }
}

// -- §5 the envelope ---------------------------------------------------------------------

fn run(st: &mut AppState, req: Value) -> Outcome {
    st.command(&req, None, &ctx())
}

#[test]
fn the_state_envelope_reads_and_owes_no_write() {
    let mut st = a_full_board();
    let out = run(&mut st, json!({ "cmd": "state" }));
    assert_eq!(out.resp["ok"], true);
    assert_eq!(out.resp["answer"], "read");
    assert!(!out.dirty);
    assert_eq!(out.resp["rev"], out.snapshot["rev"], "one moment, one revision");
}

#[test]
fn the_show_envelope_opens_a_record_by_id() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let deal = st.add_deal("Acme renewal", Some(&acme), None, None, &ctx());

    let out = run(&mut st, json!({ "cmd": "show", "kind": "company", "id": acme }));
    assert_eq!(out.resp["ok"], true);
    assert!(out.dirty, "what is open is persisted state");
    assert_eq!(out.snapshot["focus"]["id"], acme.as_str());
    assert_eq!(out.snapshot["focused"]["row"]["kind"], "company");

    // A kind that does not match the id is refused rather than trusted.
    let out = run(&mut st, json!({ "cmd": "show", "kind": "company", "id": deal }));
    assert_eq!(out.resp["ok"], false);
    assert!(!out.dirty);
    assert_eq!(st.snapshot(now())["focus"]["id"], acme.as_str(), "a refusal changes nothing");
}

#[test]
fn the_move_envelope_moves_a_deal_and_records_the_person() {
    let mut st = state();
    let deal = st.add_deal("Acme renewal", None, None, Some(AGENT), &ctx());

    let out = st.command(&json!({ "cmd": "move", "id": deal, "to": "negotiation" }), None, &at(later(1)));
    assert_eq!(out.resp["ok"], true);
    assert!(out.dirty);
    assert_eq!(out.snapshot["cards"][&deal]["stage"], "negotiation");
    assert_eq!(out.snapshot["cards"][&deal]["by"], human(), "the window is the person");
    assert_eq!(out.snapshot["cards"][&deal]["movedAt"], later(1).at);

    let out = run(&mut st, json!({ "cmd": "move", "id": deal, "to": "won" }));
    assert_eq!(out.snapshot["cards"][&deal]["status"], "won");
    assert_eq!(out.snapshot["cards"][&deal]["stage"], "negotiation", "closing keeps the stage");
}

#[test]
fn the_move_envelope_refuses_what_it_cannot_do_and_teaches() {
    let mut st = state();
    let deal = st.add_deal("Acme renewal", None, None, None, &ctx());
    let acme = st.add_company("Acme Corp", &ctx());

    for (req, needle) in [
        (json!({ "cmd": "move", "id": deal, "to": "closed" }), "lead, qualified"),
        (json!({ "cmd": "move", "id": deal }), "somewhere to go"),
        (json!({ "cmd": "move", "id": acme, "to": "won" }), "only a deal"),
        // `move` used to require `id`; it now accepts `handle` too (round-3 M2's
        // id-or-handle split), so a request with neither refuses via the shared
        // resolve_write_target message rather than a move-specific one.
        (json!({ "cmd": "move", "to": "won" }), "give its handle"),
    ] {
        let out = run(&mut st, req.clone());
        assert_eq!(out.resp["ok"], false, "{req}");
        assert!(!out.dirty, "{req}");
        let error = out.resp["error"].as_str().unwrap();
        assert!(error.contains(needle), "{req} → {error}");
    }
    assert_eq!(st.db().deal(&deal).unwrap().stage, Stage::Lead, "nothing moved");
}

#[test]
fn the_select_envelope_answers_a_parked_question_one_based() {
    let mut st = state();
    st.add_company("Acme Corp", &ctx());
    let industries = st.add_company("Acme Industries", &ctx());
    let Resolved::Ambiguous(candidates) = st.resolve("acme", false) else { panic!() };
    st.park("which Acme?", candidates);

    let out = run(&mut st, json!({ "cmd": "select", "n": 2 }));
    assert_eq!(out.resp["ok"], true);
    assert_eq!(out.snapshot["pending"], Value::Null);
    assert_eq!(out.snapshot["focus"]["id"], industries.as_str(), "2 is the second printed choice");

    let out = run(&mut st, json!({ "cmd": "select" }));
    assert_eq!(out.resp["ok"], false);
    assert!(out.resp["error"].as_str().unwrap().contains("crm select 2"));
}

#[test]
fn the_find_envelope_changes_only_what_it_names() {
    let mut st = state();
    for i in 0..30 {
        st.add_company(&format!("Company {i:02}"), &ctx());
    }
    st.add_contact("Ada Lovelace", None, &ctx());

    // query + kind: a new search, from the first page.
    let out = run(&mut st, json!({ "cmd": "find", "query": "company", "kind": "company" }));
    assert_eq!(out.resp["ok"], true);
    assert_eq!(out.snapshot["list"]["total"], 30);
    assert_eq!(out.snapshot["list"]["page"], 0, "0-based on the wire");

    // page alone: the search is kept.
    let out = run(&mut st, json!({ "cmd": "find", "page": 1 }));
    assert_eq!(out.snapshot["list"]["page"], 1);
    assert_eq!(out.snapshot["list"]["query"], "company", "an omitted query keeps its value");
    assert_eq!(out.snapshot["list"]["kind"], "company", "…and so does an omitted kind");
    assert_eq!(out.snapshot["list"]["rows"].as_array().unwrap().len(), 5);

    // nothing at all: nothing changes, not even the page.
    let out = run(&mut st, json!({ "cmd": "find" }));
    assert_eq!(out.snapshot["list"]["page"], 1);
    assert_eq!(out.snapshot["list"]["total"], 30);

    // sort re-pages, because it is state.
    let out = run(&mut st, json!({ "cmd": "find", "sort": "name" }));
    assert_eq!(out.snapshot["list"]["sort"], "name");
    assert_eq!(out.snapshot["list"]["page"], 0);
    assert_eq!(out.snapshot["list"]["rows"][0]["label"], "Company 00");

    // kind: null is not omitted — it is all three.
    let out = run(&mut st, json!({ "cmd": "find", "query": "", "kind": null }));
    assert_eq!(out.snapshot["list"]["kind"], Value::Null);
    assert_eq!(out.snapshot["list"]["total"], 31);
}

#[test]
fn a_bad_find_field_is_refused_before_anything_changes() {
    let mut st = state();
    st.add_company("Acme Corp", &ctx());
    run(&mut st, json!({ "cmd": "find", "query": "acme" }));

    for (req, needle) in [
        (json!({ "cmd": "find", "query": "zzz", "kind": "people" }), "company, contact or deal"),
        (json!({ "cmd": "find", "query": "zzz", "sort": "size" }), "updated, name, value"),
        (json!({ "cmd": "find", "query": "zzz", "page": -1 }), "counted from 0"),
        (json!({ "cmd": "find", "query": 7 }), "text"),
    ] {
        let out = run(&mut st, req.clone());
        assert_eq!(out.resp["ok"], false, "{req}");
        assert!(out.resp["error"].as_str().unwrap().contains(needle), "{req} → {}", out.resp["error"]);
        assert_eq!(out.snapshot["list"]["query"], "acme", "{req} half-applied");
    }
}

#[test]
fn the_cli_show_envelope_resolves_a_handle_at_the_edge() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    st.add_company("Hooli", &ctx());

    let out = st.command(&json!({ "cmd": "open", "handle": "ACME-CORP" }), Some(AGENT), &ctx());
    assert_eq!(out.resp["ok"], true);
    assert_eq!(out.resp["answer"], "changed");
    assert_eq!(out.snapshot["focus"]["id"], acme.as_str());
}

/// Ambiguity is a question, not a failure — `m2-cli.md` says it returns 0.
#[test]
fn an_ambiguous_handle_parks_a_question_and_is_not_an_error() {
    let mut st = state();
    st.add_company("Acme Corp", &ctx());
    st.add_company("Acme Industries", &ctx());
    let before = st.snapshot(now())["focus"].clone();

    let out = run(&mut st, json!({ "cmd": "open", "handle": "acme" }));
    assert_eq!(out.resp["ok"], true, "a question, not a failure");
    assert_eq!(out.resp["answer"], "ambiguous");
    assert_eq!(out.snapshot["pending"]["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(out.snapshot["focus"], before, "nothing was opened on a guess");
}

#[test]
fn a_handle_that_matches_nothing_is_refused_with_a_real_suggestion() {
    let mut st = state();
    let out = run(&mut st, json!({ "cmd": "open", "handle": "acme" }));
    assert_eq!(out.resp["ok"], false);
    assert!(out.resp["error"].as_str().unwrap().contains("no records yet"), "{}", out.resp["error"]);

    st.add_company("Acme", &ctx());
    st.add_company("Hooli", &ctx());
    let out = run(&mut st, json!({ "cmd": "open", "handle": "acmee" }));
    let error = out.resp["error"].as_str().unwrap();
    assert!(error.contains("no record matches “acmee”"), "{error}");
    assert!(error.contains("`crm show acme`"), "a suggestion names a record that exists: {error}");

    let out = run(&mut st, json!({ "cmd": "open", "handle": "zzzzzzzz" }));
    let error = out.resp["error"].as_str().unwrap();
    assert!(!error.contains("crm show"), "nothing close means no suggestion: {error}");

    let out = run(&mut st, json!({ "cmd": "open" }));
    assert!(out.resp["error"].as_str().unwrap().contains("crm show <handle>"));

    // The window's `show` always names a record by id; without one it is refused.
    let out = run(&mut st, json!({ "cmd": "show" }));
    assert_eq!(out.resp["ok"], false);
}

/// Round 2's open flag, now closed: the core's refusals are reachable from both surfaces,
/// and none of them may carry an id.
#[test]
fn no_refusal_the_core_can_give_contains_an_id() {
    let mut st = state();
    let deal = st.add_deal("Acme renewal", None, None, None, &ctx());
    let ghost = Ulid::from_parts(1, [9; 10]).to_string();

    let refusals = [
        run(&mut st, json!({ "cmd": "show", "kind": "deal", "id": ghost })).resp,
        run(&mut st, json!({ "cmd": "move", "id": ghost, "to": "won" })).resp,
        run(&mut st, json!({ "cmd": "show", "kind": "company", "id": deal })).resp,
    ];
    let direct = [
        st.show(&ghost).unwrap_err(),
        st.move_deal(&ghost, MoveTarget::To(Stage::Lead), None, &ctx()).unwrap_err(),
        st.archive(&ghost, &ctx()).unwrap_err(),
        st.rename(&ghost, "x", &ctx()).unwrap_err(),
        st.complete_task(&ghost, &ctx()).unwrap_err(),
        st.link(&deal, &ghost, &ctx()).unwrap_err(),
        st.link(&ghost, &deal, &ctx()).unwrap_err(),
    ];
    for resp in &refusals {
        assert_eq!(resp["ok"], false);
        let text = resp["error"].as_str().unwrap();
        assert!(first_ulid_in(text).is_none(), "an id reached a refusal: {text}");
    }
    for text in &direct {
        assert!(first_ulid_in(text).is_none(), "an id reached a refusal: {text}");
    }
}

// -- §6 the golden snapshot --------------------------------------------------------------
//
// Round 2's blocker survived because the window was only ever tested against snapshots
// the window invented. These two files are the bytes the core really emits, built through
// the core's own API; the frontend loads them verbatim and never edits them.

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/snapshot.json");
const GOLDEN_PENDING: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/snapshot-pending.json");

/// A realistic workspace: companies, contacts, deals in several stages and both closings,
/// an agent-attributed move, activities from both parties, open and done tasks, and a
/// record open.
fn golden_state() -> AppState {
    let mut st = state();
    st.set_agents(vec![agent_row()]);
    let t = |minutes: i64| {
        let n = now();
        at(Now { at: n.at + minutes * 60_000, today: n.today })
    };

    let hollis = st.add_company("Hollis Partners", &t(1));
    let northwind = st.add_company("Northwind Traders", &t(2));
    let acme = st.add_company("Acme Corp", &t(3));

    let maya = st.add_contact("Maya Hollis", Some(&hollis), &t(4));
    let theo = st.add_contact("Theo Park", Some(&hollis), &t(5));
    st.add_contact("Priya Nair", Some(&northwind), &t(6));

    let renewal = st.add_deal("Hollis renewal", Some(&hollis), Some(Money::new(4_500_000, "USD")), None, &t(7));
    st.link(&renewal, &maya, &t(8)).unwrap();
    st.link(&renewal, &theo, &t(8)).unwrap();
    st.move_deal(&renewal, MoveTarget::To(Stage::Negotiation), Some(AGENT), &t(30)).unwrap();

    let expansion = st.add_deal("Northwind expansion", Some(&northwind), Some(Money::new(1_200_000, "EUR")), None, &t(9));
    st.move_deal(&expansion, MoveTarget::To(Stage::Proposal), None, &t(20)).unwrap();

    st.add_deal("Acme pilot", Some(&acme), Some(Money::new(800_000, "USD")), Some(AGENT), &t(10));
    let won = st.add_deal("Acme onboarding", Some(&acme), Some(Money::new(250_000, "USD")), None, &t(11));
    st.move_deal(&won, MoveTarget::Close(Status::Won), None, &t(25)).unwrap();
    let lost = st.add_deal("Northwind trial", Some(&northwind), None, None, &t(12));
    st.move_deal(&lost, MoveTarget::To(Stage::Qualified), None, &t(13)).unwrap();
    st.move_deal(&lost, MoveTarget::Close(Status::Lost), Some(AGENT), &t(26)).unwrap();

    st.log(ActivityKind::Call, "Walked Maya through the renewal terms.", vec![renewal.clone(), maya.clone()], None, &t(14));
    st.log(ActivityKind::Email, "Sent the revised quote.", vec![renewal.clone()], Some(AGENT), &t(22));
    st.log(ActivityKind::Note, "Theo wants a two-year term.", vec![renewal.clone(), theo], Some(AGENT), &t(31));

    st.add_task("Call Maya about the term", Date::new(2026, 9, 10), vec![renewal.clone()], Some(AGENT), &t(32));
    st.add_task("Send the contract", Date::new(2026, 9, 7), vec![renewal.clone()], None, &t(33));
    let done = st.add_task("Book the kickoff", Date::new(2026, 9, 5), vec![renewal.clone()], None, &t(15));
    st.complete_task(&done, &t(21)).unwrap();

    st.find("", false);
    st.show(&renewal).unwrap();
    st
}

/// The same workspace with a question parked.
fn golden_pending_state() -> AppState {
    let mut st = golden_state();
    let at = at(Now { at: now().at + 40 * 60_000, today: now().today });
    st.add_company("Acme Industries", &at);
    let Resolved::Ambiguous(candidates) = st.resolve("acme", false) else {
        panic!("the golden workspace must contain an ambiguity");
    };
    st.park("which “acme”?", candidates);
    st
}

/// The snapshot as a file: `rev` is a process-wide counter and would differ between runs,
/// so it is pinned. Everything else is exactly what the core produced.
fn golden_json(st: &AppState) -> String {
    let mut snap = st.snapshot(now());
    snap["rev"] = json!(1);
    serde_json::to_string_pretty(&snap).unwrap() + "\n"
}

fn check_golden(path: &str, produced: &str) {
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, produced).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    let same = match (
        serde_json::from_str::<Value>(&committed),
        serde_json::from_str::<Value>(produced),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    assert!(
        same,
        "\n{path} no longer matches what the core produces.\n\
         If the snapshot changed on purpose, regenerate it and review the diff:\n\n    \
         cd src-tauri && UPDATE_FIXTURES=1 cargo test golden\n\n\
         A snapshot change is a contract change — M3 renders these bytes.\n"
    );
}

#[test]
fn golden_snapshot_matches_the_committed_fixture() {
    check_golden(GOLDEN, &golden_json(&golden_state()));
}

#[test]
fn golden_pending_snapshot_matches_the_committed_fixture() {
    check_golden(GOLDEN_PENDING, &golden_json(&golden_pending_state()));
}

/// The fixture is only a gate if it exercises what the window draws. If a later change
/// quietly empties it, this says so.
#[test]
fn golden_fixtures_carry_everything_the_window_draws() {
    let snap: Value = serde_json::from_str(&golden_json(&golden_state())).unwrap();
    assert_eq!(snap["cards"].as_object().unwrap().len(), 5, "three open deals, one won, one lost — all six columns are the board");
    assert!(snap["cards"].as_object().unwrap().values().any(|c| c["by"] == agent()), "an agent-moved card");
    assert!(snap["cards"].as_object().unwrap().values().any(|c| c["by"] == human()), "a person-moved card");
    assert_eq!(snap["focused"]["row"]["handle"], "hollis-renewal");
    assert!(snap["focused"]["fields"].as_array().unwrap().len() >= 4);
    assert_eq!(snap["focused"]["timelineTotal"], 3);
    let tasks = snap["focused"]["tasks"].as_array().unwrap();
    assert!(tasks.iter().any(|t| t["doneAt"].is_null()) && tasks.iter().any(|t| !t["doneAt"].is_null()));
    assert!(Ulid::parse(snap["focus"]["id"].as_str().unwrap()).is_some(), "real ULIDs");
    assert_eq!(snap["agents"][0]["id"], AGENT);
    assert_eq!(snap["due"]["overdue"], 1);
    assert!(column(&snap, "negotiation")["totals"][0]["formatted"].is_string());

    let pending: Value = serde_json::from_str(&golden_json(&golden_pending_state())).unwrap();
    // "acme" starts four names here — the company, two Acme deals and Acme Industries — so
    // the window draws a real, longer question rather than the minimal two-way one.
    assert_eq!(pending["pending"]["candidates"].as_array().unwrap().len(), 4);
    assert_eq!(pending["list"]["total"], 4, "the candidates are the shared list");
}

/// The fixture is what the window renders as text. Everything a person reads in it must be
/// id-free — the ids are there, but only under keys that are never displayed.
#[test]
fn golden_fixtures_keep_ids_under_keys_that_are_never_displayed() {
    fn walk(v: &Value, path: &str, leaks: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    // The keys that exist to carry an id. `cards` is keyed *by* id.
                    if matches!(k.as_str(), "id" | "dealIds") || path.ends_with("cards") {
                        if path.ends_with("cards") {
                            walk(child, &format!("{path}.<id>"), leaks);
                        }
                        continue;
                    }
                    walk(child, &format!("{path}.{k}"), leaks);
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, &format!("{path}[]"), leaks)),
            Value::String(s) => {
                if let Some(u) = first_ulid_in(s) {
                    leaks.push(format!("{path}: {u}"));
                }
            }
            _ => {}
        }
    }
    for st in [golden_state(), golden_pending_state()] {
        let mut leaks = Vec::new();
        walk(&st.snapshot(now()), "", &mut leaks);
        assert!(leaks.is_empty(), "an id sits under a displayed key: {leaks:?}");
    }
}
// MARK: - M2: every write verb, the CLI's handle path equals the window's id path
//
// `docs/work-orders/m2-cli.md` requires more than "both paths work" — it requires the
// SAME core call, proven by identical resulting state. Two states built through an
// identical sequence of `ctx()` calls mint identical ids (the entropy and the instant
// never change), so a window-style `{..., id}` request against one and a CLI-style
// `{..., handle}` request against the other can be compared directly — as long as both
// are attributed to the same actor, since attribution is *meant* to differ by caller and
// must not be mistaken for the two envelopes disagreeing.
//
// Folded into the same proof: **only a human write signals.** `caller: None` (the
// window, or a person typing `crm` directly — the same convention `Actor` already uses)
// gets the emit; `caller: Some(agent)` (the CLI, driven by an agent) gets none, for the
// identical action. Checked on a *third*, independently-seeded state, so a signal
// assertion can never be satisfied by re-reading the id-path call's own result.

fn signal_ids(emits: &[Emit]) -> Vec<&str> {
    emits.iter().map(|e| e.id.as_str()).collect()
}

/// One proof, reused for every write verb below: `seed` builds one deterministic dataset
/// (called three times — it must mint the same ids each time, which it does, since
/// `ctx()` never advances); `window_req`/`cli_req` build the two envelopes from that
/// seeded state, so they can reference the ids/handles `seed` just minted.
fn assert_same_core_call_and_human_only_signal(
    seed: impl Fn(&mut AppState),
    window_req: impl Fn(&AppState) -> Value,
    cli_req: impl Fn(&AppState) -> Value,
    expected_signal: &str,
) {
    let mut a = state();
    seed(&mut a);
    let mut b = state();
    seed(&mut b);
    assert_eq!(a.db(), b.db(), "seeding must be deterministic before the write under test");

    // Same actor (the person) on both sides, so attribution cannot be the thing that
    // makes them differ — only the envelope shape is under test here.
    let out_a = a.command(&window_req(&a), None, &ctx());
    assert_eq!(out_a.resp["ok"], true, "the window's id-based request: {:?}", out_a.resp);
    assert_eq!(signal_ids(&out_a.emits), vec![expected_signal], "a human write must signal");

    let out_b = b.command(&cli_req(&b), None, &ctx());
    assert_eq!(out_b.resp["ok"], true, "the CLI's handle-based request: {:?}", out_b.resp);
    assert_eq!(a.db(), b.db(), "an id and a handle for the same record reached different state");

    // A third, independently-seeded state proves the agent path signals nothing — the
    // architecture's own rule, tested at exactly the boundary it is stated at.
    let mut c = state();
    seed(&mut c);
    let out_c = c.command(&cli_req(&c), Some("agent-1"), &ctx());
    assert_eq!(out_c.resp["ok"], true, "{:?}", out_c.resp);
    assert!(out_c.emits.is_empty(), "an agent's own write must never be told about itself");
}

#[test]
fn add_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            st.add_company("Acme Corp", &ctx());
        },
        |_| json!({ "cmd": "add", "kind": "contact", "name": "Ada", "fields": { "company": "acme-corp" } }),
        |_| json!({ "cmd": "add", "kind": "contact", "name": "Ada", "fields": { "company": "acme-corp" } }),
        "record.changed",
    );
}

#[test]
fn set_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            st.add_company("Acme Corp", &ctx());
        },
        |st| json!({ "cmd": "set", "id": id_of(st, "acme-corp"), "field": "domain", "value": "acme.com" }),
        |_| json!({ "cmd": "set", "handle": "acme-corp", "field": "domain", "value": "acme.com" }),
        "record.changed",
    );
}

#[test]
fn log_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            st.add_deal("Acme renewal", None, None, None, &ctx());
        },
        |st| json!({ "cmd": "log", "kind": "call", "id": id_of(st, "acme-renewal"), "body": "rang" }),
        |_| json!({ "cmd": "log", "kind": "call", "handle": "acme-renewal", "body": "rang" }),
        "note.added",
    );
}

#[test]
fn move_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            st.add_deal("Acme renewal", None, None, None, &ctx());
        },
        |st| json!({ "cmd": "move", "id": id_of(st, "acme-renewal"), "to": "won" }),
        |_| json!({ "cmd": "move", "handle": "acme-renewal", "to": "won" }),
        "stage.changed",
    );
}

#[test]
fn task_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            st.add_deal("Acme renewal", None, None, None, &ctx());
        },
        |st| json!({ "cmd": "task", "id": id_of(st, "acme-renewal"), "what": "call back", "due": "2026-09-30" }),
        |_| json!({ "cmd": "task", "handle": "acme-renewal", "what": "call back", "due": "2026-09-30" }),
        "record.changed",
    );
}

#[test]
fn done_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            let deal = st.add_deal("Acme renewal", None, None, None, &ctx());
            st.add_task("Call back", Date::new(2026, 9, 30), vec![deal], None, &ctx());
        },
        |st| json!({ "cmd": "done", "id": task_id_of(st, "call-back") }),
        |_| json!({ "cmd": "done", "handle": "call-back" }),
        "record.changed",
    );
}

#[test]
fn link_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            st.add_deal("Acme renewal", None, None, None, &ctx());
            st.add_contact("Ada Lovelace", None, &ctx());
        },
        |st| json!({ "cmd": "link", "id": id_of(st, "acme-renewal"), "toId": id_of(st, "ada-lovelace") }),
        |_| json!({ "cmd": "link", "handle": "acme-renewal", "toHandle": "ada-lovelace" }),
        "record.changed",
    );
}

/// `link`'s two positionals are unordered — the core decides which one is the deal.
#[test]
fn link_accepts_either_order_and_refuses_two_deals_or_neither() {
    let mut st = state();
    let deal = st.add_deal("Acme renewal", None, None, None, &ctx());
    let ada = st.add_contact("Ada Lovelace", None, &ctx());
    let other_deal = st.add_deal("Hooli deal", None, None, None, &ctx());
    let acme = st.add_company("Acme Corp", &ctx());

    // contact-then-deal, the reverse of what the doc's own example shows
    let out = st.command(&json!({ "cmd": "link", "id": ada.clone(), "toId": deal.clone() }), None, &ctx());
    assert_eq!(out.resp["ok"], true, "{:?}", out.resp);

    let out = st.command(&json!({ "cmd": "link", "id": deal.clone(), "toId": other_deal }), None, &ctx());
    assert_eq!(out.resp["ok"], false);
    assert!(out.resp["error"].as_str().unwrap().contains("one deal, not two"));

    let out = st.command(&json!({ "cmd": "link", "id": ada, "toId": acme }), None, &ctx());
    assert_eq!(out.resp["ok"], false);
    assert!(out.resp["error"].as_str().unwrap().contains("a deal and"));
}

#[test]
fn archive_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            st.add_company("Acme Corp", &ctx());
        },
        |st| json!({ "cmd": "archive", "id": id_of(st, "acme-corp") }),
        |_| json!({ "cmd": "archive", "handle": "acme-corp" }),
        "record.changed",
    );
}

#[test]
fn archive_restore_is_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            let id = st.add_company("Acme Corp", &ctx());
            st.archive(&id, &ctx()).unwrap();
        },
        |st| json!({ "cmd": "archive", "id": id_of(st, "acme-corp"), "restore": true }),
        |_| json!({ "cmd": "archive", "handle": "acme-corp", "restore": true }),
        "record.changed",
    );
}

/// A window `show`/`select` and a CLI `open`/`select` both open a record — the one place
/// `deal.opened` fires.
#[test]
fn show_and_open_are_one_core_call_for_both_surfaces() {
    assert_same_core_call_and_human_only_signal(
        |st| {
            st.add_deal("Acme renewal", None, None, None, &ctx());
        },
        |st| json!({ "cmd": "show", "kind": "deal", "id": id_of(st, "acme-renewal") }),
        |_| json!({ "cmd": "open", "handle": "acme-renewal" }),
        "deal.opened",
    );
}

/// The id a handle currently names — the fixture-side mirror of what
/// `AppState::resolve_write_target` does for real, so a window-envelope fixture can name
/// a record the same way a person clicking a row would: by the id the snapshot handed it.
fn id_of(st: &AppState, handle_word: &str) -> Id {
    match st.resolve(handle_word, false) {
        Resolved::One(_, id) => id,
        other => panic!("`{handle_word}` did not resolve decisively: {other:?}"),
    }
}

fn task_id_of(st: &AppState, handle_word: &str) -> Id {
    st.db().task_by_handle(handle_word).unwrap_or_else(|| panic!("no task `{handle_word}`")).id.clone()
}

// MARK: - M2: `set_field`

#[test]
fn set_field_edits_every_field_each_kind_actually_has() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let ada = st.add_contact("Ada Lovelace", None, &ctx());
    let deal = st.add_deal("Acme renewal", None, None, None, &ctx());

    st.set_field(&acme, "domain", "acme.com", &ctx()).unwrap();
    st.set_field(&acme, "notes", "VIP account", &ctx()).unwrap();
    st.set_field(&acme, "tags", "vip, enterprise", &ctx()).unwrap();
    st.set_field(&acme, "name", "Acme Corporation", &ctx()).unwrap();
    let db = st.db();
    let c = db.company(&acme).unwrap();
    assert_eq!(c.domain.as_deref(), Some("acme.com"));
    assert_eq!(c.notes.as_deref(), Some("VIP account"));
    assert_eq!(c.tags, vec!["vip", "enterprise"]);
    assert_eq!(c.name, "Acme Corporation");
    assert_eq!(handle(&st, &acme), "acme-corp", "a rename through `set` still leaves the handle alone");

    st.set_field(&ada, "email", "ada@acme.com", &ctx()).unwrap();
    st.set_field(&ada, "phone", "555-1", &ctx()).unwrap();
    st.set_field(&ada, "title", "Engineer", &ctx()).unwrap();
    st.set_field(&ada, "company", "acme-corp", &ctx()).unwrap();
    let db = st.db();
    let c = db.contact(&ada).unwrap();
    assert_eq!(c.email.as_deref(), Some("ada@acme.com"));
    assert_eq!(c.phone.as_deref(), Some("555-1"));
    assert_eq!(c.title.as_deref(), Some("Engineer"));
    assert_eq!(c.company_id.as_deref(), Some(acme.as_str()));

    st.set_field(&deal, "value", "45000.5", &ctx()).unwrap();
    st.set_field(&deal, "company", &acme, &ctx()).unwrap(); // an id works too, not only a handle
    let db = st.db();
    let d = db.deal(&deal).unwrap();
    assert_eq!(d.value, Some(Money::new(4_500_050, "USD")), "no currency given yet — USD by default");
    assert_eq!(d.company_id.as_deref(), Some(acme.as_str()));

    // Giving a currency alongside a later edit changes it; omitting it keeps what is there.
    st.set_field(&deal, "value", "50000 EUR", &ctx()).unwrap();
    assert_eq!(st.db().deal(&deal).unwrap().value, Some(Money::new(5_000_000, "EUR")));
    st.set_field(&deal, "value", "1", &ctx()).unwrap();
    assert_eq!(st.db().deal(&deal).unwrap().value.as_ref().unwrap().currency, "EUR", "the currency carries over");
}

#[test]
fn set_field_clears_an_optional_field_with_an_empty_value() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    st.set_field(&acme, "domain", "acme.com", &ctx()).unwrap();
    st.set_field(&acme, "domain", "", &ctx()).unwrap();
    assert_eq!(st.db().company(&acme).unwrap().domain, None);
}

#[test]
fn set_field_refuses_a_field_this_kind_does_not_have() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let err = st.set_field(&acme, "email", "x@x.com", &ctx()).unwrap_err();
    assert!(err.contains("not a field on a company"), "{err}");
    assert!(err.contains("domain"), "the refusal names what IS allowed: {err}");
}

#[test]
fn set_field_refuses_setting_a_company_to_something_that_is_not_one() {
    let mut st = state();
    let ada = st.add_contact("Ada Lovelace", None, &ctx());
    let deal = st.add_deal("Acme renewal", None, None, None, &ctx());
    let err = st.set_field(&ada, "company", &deal, &ctx()).unwrap_err();
    assert!(err.contains("is a deal, not a company"), "{err}");
}

// MARK: - M2: archive / restore refuse a no-op

#[test]
fn archiving_an_already_archived_record_is_a_refusal_not_a_silent_success() {
    let mut st = state();
    let id = st.add_company("Acme Corp", &ctx());
    st.archive(&id, &ctx()).unwrap();
    let err = st.archive(&id, &ctx()).unwrap_err();
    assert!(err.contains("already archived"), "{err}");
}

#[test]
fn restoring_a_record_that_is_not_archived_is_a_refusal_not_a_silent_success() {
    let mut st = state();
    let id = st.add_company("Acme Corp", &ctx());
    let err = st.restore(&id, &ctx()).unwrap_err();
    assert!(err.contains("not archived"), "{err}");
}

// MARK: - M2: export

#[test]
fn export_rows_are_handles_and_decimals_never_ids_or_symbols() {
    let mut st = state();
    let acme = st.add_company("Acme Corp", &ctx());
    let ada = st.add_contact("Ada Lovelace", Some(&acme), &ctx());
    let deal = st.add_deal("Acme renewal", Some(&acme), Some(Money::new(4_500_050, "USD")), None, &ctx());
    st.link(&deal, &ada, &ctx()).unwrap();

    let companies = st.export_rows(Kind::Company, false);
    assert_eq!(companies.len(), 1);
    let by_key = |row: &[(&str, String)], key: &str| row.iter().find(|(k, _)| *k == key).unwrap().1.clone();
    assert_eq!(by_key(&companies[0], "handle"), "acme-corp");
    assert!(first_ulid_in(&by_key(&companies[0], "handle")).is_none());

    let deals = st.export_rows(Kind::Deal, false);
    assert_eq!(by_key(&deals[0], "company"), "acme-corp", "a handle, not the company's id");
    assert_eq!(by_key(&deals[0], "contacts"), "ada-lovelace");
    assert_eq!(by_key(&deals[0], "value"), "45000.50", "a plain decimal, no symbol, no thousands separator");
    assert_eq!(by_key(&deals[0], "currency"), "USD");

    st.archive(&acme, &ctx()).unwrap();
    assert_eq!(st.export_rows(Kind::Company, false).len(), 0, "excluded by default, like `find`");
    assert_eq!(st.export_rows(Kind::Company, true).len(), 1);
}
