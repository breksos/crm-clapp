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
    Ctx { now, entropy: [0x5A; 10], origin: origin() }
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
        let id = st.add_deal(title, None, Some(Money::new(1_000_000, "USD")), &ctx());
        st.move_deal(&id, MoveTarget::To(stage), &ctx()).unwrap();
    }
    let won = st.add_deal("Globex pilot", None, Some(Money::new(2_000_000, "USD")), &ctx());
    st.move_deal(&won, MoveTarget::Close(Status::Won), &ctx()).unwrap();
    let lost = st.add_deal("Soylent trial", None, Some(Money::new(3_000_000, "USD")), &ctx());
    st.move_deal(&lost, MoveTarget::Close(Status::Lost), &ctx()).unwrap();
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
    let id = st.add_deal("Acme renewal", None, None, &ctx());
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
    st.add_deal("Zeta", None, Some(Money::new(100, "USD")), &ctx());
    st.add_deal("Alpha", None, Some(Money::new(900, "USD")), &ctx());
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
    let id = st.add_deal("Acme renewal", None, None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Negotiation), &ctx()).unwrap();

    st.move_deal(&id, MoveTarget::Close(Status::Won), &at(later(1))).unwrap();

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
    let id = st.add_deal("Acme renewal", None, None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Proposal), &ctx()).unwrap();
    st.move_deal(&id, MoveTarget::Close(Status::Lost), &ctx()).unwrap();

    let db = st.db();
    assert_eq!(db.deal(&id).unwrap().status, Status::Lost);
    assert_eq!(db.deal(&id).unwrap().stage, Stage::Proposal);
}

#[test]
fn moving_a_closed_deal_to_a_stage_reopens_it() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, None, &ctx());
    st.move_deal(&id, MoveTarget::Close(Status::Lost), &ctx()).unwrap();

    st.move_deal(&id, MoveTarget::To(Stage::Proposal), &at(later(1))).unwrap();

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
    let id = st.add_deal("Acme renewal", None, None, &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Negotiation), &ctx()).unwrap();
    assert_eq!(column(&st.snapshot(now()), "negotiation")["count"], 1);

    st.move_deal(&id, MoveTarget::Close(Status::Won), &ctx()).unwrap();
    let snap = st.snapshot(now());
    assert_eq!(column(&snap, "negotiation")["count"], 0);
    assert_eq!(column(&snap, "won")["count"], 1);
}

#[test]
fn moving_a_deal_that_does_not_exist_is_an_error_not_a_silent_no_op() {
    let mut st = state();
    assert!(st.move_deal("nope", MoveTarget::To(Stage::Lead), &ctx()).is_err());
}

// MARK: - 5. Archive, never delete
//
// An agent holding a delete verb and a bad fuzzy match is an unrecoverable afternoon. So
// archiving hides a record everywhere it would clutter, and hides it nowhere it would lose
// it.

#[test]
fn an_archived_record_leaves_the_board_the_counts_and_the_default_search() {
    let mut st = state();
    let id = st.add_deal("Acme renewal", None, Some(Money::new(500, "USD")), &ctx());
    st.move_deal(&id, MoveTarget::To(Stage::Proposal), &ctx()).unwrap();

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
    let id = st.add_deal("Acme renewal", None, None, &ctx());
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
        st.add_deal(&format!("Deal {}", value.currency), None, Some(value), &ctx());
    }

    let totals = column(&st.snapshot(now()), "lead")["totals"].clone();
    assert_eq!(
        totals,
        json!([
            { "currency": "EUR", "amount": 1_000_000 },
            { "currency": "USD", "amount": 5_000_000 },
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
    assert_eq!(Money::new(0, "USD").format(), "0.00");
}

#[test]
fn a_deal_with_no_value_is_counted_but_contributes_no_total() {
    let mut st = state();
    st.add_deal("Unpriced", None, None, &ctx());
    st.add_deal("Priced", None, Some(Money::new(100, "USD")), &ctx());

    let snap = st.snapshot(now());
    let lead = column(&snap, "lead");
    assert_eq!(lead["count"], 2, "a deal without a price is still a deal");
    assert_eq!(lead["totals"], json!([{ "currency": "USD", "amount": 100 }]));
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
    let deal = st.add_deal("Acme Corp.", None, None, &ctx());
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
    let id = st.add_deal("Acme renewal", None, None, &ctx());
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
    let deal = st.add_deal("Acme renewal", Some(&acme), None, &ctx());
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
    let id = st.add_deal("Acme renewal", None, None, &ctx());
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
    let deal = st.add_deal("Acme renewal", None, None, &ctx());

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
    let deal = st.add_deal("Acme renewal", None, None, &ctx());
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
    st.add_deal("Acme renewal", Some(&acme), Some(Money::new(100, "USD")), &ctx());
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

/// M4 fills these in. M1 establishes that producing them is the core's job, not the
/// transport's — and that the core is where "only human actions signal" will be decided.
#[test]
fn m1_emits_no_signals_yet_from_either_surface() {
    let mut st = state();
    assert!(st.command(&json!({ "cmd": "status" }), None, &ctx()).emits.is_empty());
    assert!(st.command(&json!({ "cmd": "status" }), Some("a-1"), &ctx()).emits.is_empty());
}
