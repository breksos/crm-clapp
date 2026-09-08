//! The core. **Pure state and logic**: no files, no sockets, no platform code, and no
//! clock — time arrives as a [`Now`] value. That is what makes the rules testable without
//! a window server, and it is why the tests are where the rules actually live.
//!
//! Both surfaces call the same methods on the same [`AppState`], so they cannot drift.
//! Persistence reaches it only through the [`CrmStore`](crate::store::CrmStore) port: the
//! core is handed a [`Db`] at startup and hands one back to be written.

use crate::model::*;
use clappkit::control::Emit;
use clappkit::AgentRow;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// How many rows a page holds until somebody changes it. Shared, like the rest of the
/// view — see [`ListView`].
pub const DEFAULT_PAGE_SIZE: usize = 25;

/// How far ahead "this week" reaches, in days.
pub const WEEK_DAYS: i64 = 7;

/// How much better the best match must be than the runner-up before it is treated as the
/// answer. Below this the two are close enough that choosing between them is the person's
/// call, not ours.
const DECISIVE_MARGIN: i32 = 10;

// MARK: - The shared view state

/// What is currently *being looked at*, by both surfaces.
///
/// This is the part that makes this a clapp and not a CRM with a CLI bolted on. It is as
/// real as the data, rides the same snapshot, is persisted with the rest, and is mutated
/// by whichever surface acts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The record currently open. The agent's `crm show acme` opens that record in the
    /// person's window — not a side effect, the point.
    pub focus: Option<Focus>,
    pub list: ListView,
    pub board: BoardView,
    /// An unresolved question and its candidates. See [`AppState::resolve`].
    pub pending: Option<Pending>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    pub kind: Kind,
    pub id: Id,
}

/// The shared result list: what was searched for, how it is ordered, and which page of it
/// both surfaces are on.
///
/// **`page_size` lives here, never in a caller's request.** An agent passing `-n 3` limits
/// what its own terminal prints; it must not repaginate the person's table to three rows.
/// The bug reads as "why does searching Acme return one result?" and it is invisible from
/// the side that caused it.
///
/// Sort is state for the same reason: a control that only reorders the page you happen to
/// hold is a lie about the data underneath it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListView {
    pub query: String,
    pub sort: Sort,
    pub page: usize,
    pub page_size: usize,
    /// Every id that matched, in sort order — not just the visible page, so paging never
    /// re-runs the search and both surfaces agree on the total.
    #[serde(default)]
    pub results: Vec<Id>,
}

impl Default for ListView {
    fn default() -> ListView {
        ListView {
            query: String::new(),
            sort: Sort::Updated,
            page: 0,
            page_size: DEFAULT_PAGE_SIZE,
            results: Vec::new(),
        }
    }
}

impl ListView {
    /// The ids on the current page. Clamped rather than wrapped: a page that has fallen
    /// off the end of a shrinking result set shows the last page, never an error.
    pub fn page_ids(&self) -> &[Id] {
        if self.page_size == 0 {
            return &[];
        }
        let start = (self.page * self.page_size).min(self.results.len());
        let end = (start + self.page_size).min(self.results.len());
        &self.results[start..end]
    }

    pub fn pages(&self) -> usize {
        if self.page_size == 0 {
            return 0;
        }
        self.results.len().div_ceil(self.page_size)
    }
}

/// One board, one truth. A filter only one surface knows about is drift with better
/// manners.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardView {
    pub pipeline_id: Id,
    /// When set, the board is narrowed to that one stage's column. `None` draws all six.
    pub stage_filter: Option<Stage>,
}

impl Default for BoardView {
    fn default() -> BoardView {
        BoardView { pipeline_id: Pipeline::seed().id, stage_filter: None }
    }
}

/// An unresolved question, parked where both surfaces can see it.
///
/// **Ambiguity is a state, not a guess and not an error.** "Log a call with Acme" when two
/// companies match: picking one silently is confidently wrong, and refusing teaches
/// nothing. So a visible placeholder goes here, the candidates go into the same result list
/// both surfaces already render, and *either* surface answers — `crm select 2`, or a click
/// on the row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub prompt: String,
    pub candidates: Vec<Candidate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub kind: Kind,
    pub id: Id,
    pub label: String,
}

/// What a lookup came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    /// One record, decisively.
    One(Kind, Id),
    /// Two or more too close to call.
    Ambiguous(Vec<Candidate>),
    /// Nothing matched at all — which is an answer, not an ambiguity.
    None,
}

// MARK: - What one command produced

/// The answer for the caller who asked, the snapshot for everyone who is looking, the
/// signals the app owes its agents, and whether the dataset changed.
///
/// The response and the snapshot are taken from **one** call so they leave the same
/// critical section — an app that re-locks to snapshot after answering has a window in
/// which the two describe different moments.
pub struct Outcome {
    pub resp: Value,
    pub snapshot: Value,
    /// Only ever non-empty for a **human** action. The agent already knows about its own
    /// writes, and telling it is the loop that makes an app talk to itself.
    ///
    /// M4 fills this in; M1 establishes that it is the core's to produce, not the
    /// transport's.
    pub emits: Vec<Emit>,
    /// The dataset changed and owes a write. The core does not save — it says so, and the
    /// debounced writer in `store` decides when.
    pub dirty: bool,
}

// MARK: - The state

/// The whole of the app's state: the dataset, and the roster Clatch last described.
#[derive(Debug, Default)]
pub struct AppState {
    db: Db,
    /// Not our data — the launcher's — but it rides the snapshot because the window draws
    /// it. Keyed on the immutable `id`; `name` is a re-pointable label we only display.
    agents: Vec<AgentRow>,
}

// `new` is the tests' door in; `with_db` is the app's. Both are real, and which one is
// "unused" depends on the build, so neither is worth a warning.
#[allow(dead_code)]
impl AppState {
    pub fn new() -> AppState {
        AppState::default()
    }

    /// Open on a dataset the store handed us.
    pub fn with_db(db: Db) -> AppState {
        AppState { db, agents: Vec::new() }
    }

    /// The dataset, for the store to write. A clone because the writer is debounced and
    /// must not hold the core's lock while it waits out a quiet period.
    pub fn db(&self) -> Db {
        self.db.clone()
    }

    /// Replace the roster with Clatch's latest snapshot of it. A rename arrives as a fresh
    /// roster with the same id, so replacing wholesale is the correct read of the protocol
    /// — there is nothing to merge.
    pub fn set_agents(&mut self, agents: Vec<AgentRow>) {
        self.agents = agents;
    }
}

// MARK: - The operations
//
// The typed API both surfaces drive. M2 maps CLI verbs onto these and M3 maps the window's
// controls onto the same ones, which is what makes it impossible for the two to disagree
// about a rule.
//
// `dead_code` is allowed here because M1 lands the core ahead of the verbs that call it —
// every method below is exercised by the tests at the bottom of this file, and M2 removes
// the need for the attribute by wiring them up.
#[allow(dead_code)]
impl AppState {
    // -- creating ---------------------------------------------------------------------

    pub fn add_company(&mut self, name: &str, now: Now) -> Id {
        let id = unique_id(name, &|c| self.db.id_taken(c), "company");
        self.db.companies.push(Company {
            id: id.clone(),
            name: name.trim().to_string(),
            domain: None,
            tags: Vec::new(),
            notes: None,
            archived_at: None,
            updated_at: now.at,
        });
        self.focus_on(Kind::Company, &id);
        id
    }

    pub fn add_contact(&mut self, name: &str, company_id: Option<&str>, now: Now) -> Id {
        let id = unique_id(name, &|c| self.db.id_taken(c), "contact");
        self.db.contacts.push(Contact {
            id: id.clone(),
            name: name.trim().to_string(),
            email: None,
            phone: None,
            title: None,
            company_id: company_id.map(str::to_string),
            tags: Vec::new(),
            archived_at: None,
            updated_at: now.at,
        });
        self.focus_on(Kind::Contact, &id);
        id
    }

    /// A new deal starts at [`Stage::Lead`] and [`Status::Open`], in the one pipeline.
    pub fn add_deal(&mut self, title: &str, company_id: Option<&str>, value: Option<Money>, now: Now) -> Id {
        let id = unique_id(title, &|c| self.db.id_taken(c), "deal");
        let pipeline_id = self.db.pipeline().id;
        self.db.deals.push(Deal {
            id: id.clone(),
            title: title.trim().to_string(),
            company_id: company_id.map(str::to_string),
            contact_ids: Vec::new(),
            value,
            stage: Stage::Lead,
            status: Status::Open,
            pipeline_id,
            opened_at: now.at,
            closed_at: None,
            archived_at: None,
            updated_at: now.at,
        });
        self.focus_on(Kind::Deal, &id);
        id
    }

    // -- the pipeline -----------------------------------------------------------------

    /// `crm move <deal> <word>`.
    ///
    /// **Stage and status are different axes.** Closing sets the status and leaves the
    /// stage exactly where it was, which is what makes "how many did we win out of
    /// Negotiation" answerable later. Moving a closed deal to a stage **reopens** it,
    /// because that is the only thing the request can sensibly mean.
    pub fn move_deal(&mut self, id: &str, target: MoveTarget, now: Now) -> Result<(), String> {
        let Some(deal) = self.db.deals.iter_mut().find(|d| d.id == id) else {
            return Err(format!("no deal `{id}`"));
        };
        match target {
            MoveTarget::To(stage) => {
                deal.stage = stage;
                // Reopening: a deal cannot be simultaneously in Negotiation and lost.
                if !deal.status.is_open() {
                    deal.status = Status::Open;
                    deal.closed_at = None;
                }
            }
            MoveTarget::Close(status) => {
                deal.status = status;
                deal.closed_at = Some(now.at);
                // `deal.stage` is deliberately untouched.
            }
        }
        deal.updated_at = now.at;
        Ok(())
    }

    // -- the log ----------------------------------------------------------------------

    /// Append a line to the immutable log, attributed to whoever wrote it.
    ///
    /// `caller` is the agent id Clatch injected into the calling shell, or `None` for the
    /// person at the window. Keyed on the id, never the display name.
    pub fn log(
        &mut self,
        kind: ActivityKind,
        body: &str,
        links: Vec<Id>,
        caller: Option<&str>,
        now: Now,
    ) -> Id {
        let id = unique_activity_id(&self.db, now.at);
        self.db.activities.push(Activity {
            id: id.clone(),
            kind,
            body: body.trim().to_string(),
            at: now.at,
            links,
            by: Actor::from_caller(caller),
        });
        id
    }

    // -- next steps -------------------------------------------------------------------

    pub fn add_task(&mut self, what: &str, due: Date, links: Vec<Id>, caller: Option<&str>, now: Now) -> Id {
        let id = unique_task_id(&self.db, now.at);
        self.db.tasks.push(Task {
            id: id.clone(),
            what: what.trim().to_string(),
            due,
            links,
            done_at: None,
            by: Actor::from_caller(caller),
        });
        id
    }

    pub fn complete_task(&mut self, id: &str, now: Now) -> Result<(), String> {
        let Some(task) = self.db.tasks.iter_mut().find(|t| t.id == id) else {
            return Err(format!("no task `{id}`"));
        };
        task.done_at = Some(now.at);
        Ok(())
    }

    /// Overdue, due today, and due within the next week. **Disjoint buckets**, so the three
    /// numbers can be read side by side without double-counting the same task.
    pub fn due_counts(&self, now: Now) -> (usize, usize, usize) {
        let mut overdue = 0;
        let mut today = 0;
        let mut week = 0;
        for task in self.db.tasks.iter().filter(|t| t.done_at.is_none()) {
            match now.today.days_until(task.due) {
                d if d < 0 => overdue += 1,
                0 => today += 1,
                d if d <= WEEK_DAYS => week += 1,
                _ => {}
            }
        }
        (overdue, today, week)
    }

    // -- associating ------------------------------------------------------------------

    /// Associate a contact or a company with a deal. Idempotent: linking twice is not an
    /// error, because an agent retrying a call must not corrupt the graph.
    pub fn link(&mut self, deal_id: &str, other_id: &str, now: Now) -> Result<(), String> {
        let is_contact = self.db.contact(other_id).is_some();
        let is_company = self.db.company(other_id).is_some();
        if !is_contact && !is_company {
            return Err(format!("no contact or company `{other_id}`"));
        }
        let Some(deal) = self.db.deals.iter_mut().find(|d| d.id == deal_id) else {
            return Err(format!("no deal `{deal_id}`"));
        };
        if is_contact {
            if !deal.contact_ids.iter().any(|c| c == other_id) {
                deal.contact_ids.push(other_id.to_string());
            }
        } else {
            deal.company_id = Some(other_id.to_string());
        }
        deal.updated_at = now.at;
        Ok(())
    }

    // -- retiring ---------------------------------------------------------------------

    /// Retire a record. **Reversible, and never a hard delete** — an agent holding a delete
    /// verb and a bad fuzzy match is an unrecoverable afternoon.
    ///
    /// An archived record leaves the board, the counts and default `find` results. `show`
    /// still loads it, and [`AppState::restore`] brings it back.
    pub fn archive(&mut self, id: &str, now: Now) -> Result<(), String> {
        self.set_archived(id, Some(now.at), now)
    }

    pub fn restore(&mut self, id: &str, now: Now) -> Result<(), String> {
        self.set_archived(id, None, now)
    }

    fn set_archived(&mut self, id: &str, at: Option<Timestamp>, now: Now) -> Result<(), String> {
        if let Some(c) = self.db.companies.iter_mut().find(|c| c.id == id) {
            c.archived_at = at;
            c.updated_at = now.at;
            return Ok(());
        }
        if let Some(c) = self.db.contacts.iter_mut().find(|c| c.id == id) {
            c.archived_at = at;
            c.updated_at = now.at;
            return Ok(());
        }
        if let Some(d) = self.db.deals.iter_mut().find(|d| d.id == id) {
            d.archived_at = at;
            d.updated_at = now.at;
            return Ok(());
        }
        Err(format!("no record `{id}`"))
    }

    pub fn is_archived(&self, id: &str) -> bool {
        self.db.company(id).map(|c| c.archived_at.is_some())
            .or_else(|| self.db.contact(id).map(|c| c.archived_at.is_some()))
            .or_else(|| self.db.deal(id).map(|d| d.archived_at.is_some()))
            .unwrap_or(false)
    }

    // -- looking ----------------------------------------------------------------------

    /// Run a search and **replace the shared result list**. Archived records are excluded
    /// unless `include_archived`.
    ///
    /// Returns how many matched. Note what it does *not* take: a page size. That belongs to
    /// the shared view, and a caller asking for three results gets three lines printed, not
    /// a three-row page for everybody.
    pub fn find(&mut self, query: &str, include_archived: bool) -> usize {
        let mut scored: Vec<(i32, Kind, Id)> = Vec::new();
        for (kind, id, name) in self.searchable(include_archived) {
            if let Some(score) = match_score(query, &id, &name) {
                scored.push((score, kind, id));
            }
        }
        // Ordered before the assignment, so the immutable borrow `ordered` needs is over
        // before the list is written back.
        let ordered = self.ordered(scored);
        self.db.view.list.query = query.trim().to_string();
        self.db.view.list.results = ordered;
        self.db.view.list.page = 0;
        self.db.view.list.results.len()
    }

    /// Open one record: sets [`View::focus`], which is what puts it on the person's screen.
    /// Archived records are still loadable — that is the difference between archiving and
    /// deleting.
    pub fn show(&mut self, id: &str) -> Result<(), String> {
        let kind = self.kind_of(id).ok_or_else(|| format!("no record `{id}`"))?;
        self.focus_on(kind, id);
        Ok(())
    }

    /// Answer a parked question, or open result N of the shared list. **One-based**: an
    /// agent reading "1) Acme Corp  2) Acme Inc" types the number it can see.
    pub fn select(&mut self, n: usize) -> Result<Id, String> {
        if let Some(pending) = self.db.view.pending.clone() {
            let Some(choice) = n.checked_sub(1).and_then(|i| pending.candidates.get(i)) else {
                return Err(format!(
                    "there are {} candidates — pick 1 to {}",
                    pending.candidates.len(),
                    pending.candidates.len()
                ));
            };
            let id = choice.id.clone();
            self.db.view.pending = None;
            self.focus_on(choice.kind, &id);
            return Ok(id);
        }

        let ids = self.db.view.list.page_ids().to_vec();
        let Some(id) = n.checked_sub(1).and_then(|i| ids.get(i)).cloned() else {
            return Err(format!("this page has {} results — pick 1 to {}", ids.len(), ids.len()));
        };
        self.show(&id)?;
        Ok(id)
    }

    /// Turn a name or an id into one record — or into a parked question.
    ///
    /// **An exact id match is decisive**, always: ids are ours, typable, and unambiguous by
    /// construction, so `crm show acme` never asks a question it already has the answer to.
    /// Otherwise the gate is the **margin** between the top two scores, because a clear
    /// winner is not ambiguity.
    pub fn resolve(&self, query: &str, include_archived: bool) -> Resolved {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Resolved::None;
        }

        // An id is decisive on its own terms, before anything is scored.
        if let Some(kind) = self.kind_of(&needle) {
            return Resolved::One(kind, needle);
        }

        let mut scored: Vec<(i32, Kind, Id, String)> = Vec::new();
        for (kind, id, name) in self.searchable(include_archived) {
            if let Some(score) = match_score(query, &id, &name) {
                scored.push((score, kind, id, name));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.3.cmp(&b.3)));

        match scored.len() {
            0 => Resolved::None,
            1 => Resolved::One(scored[0].1, scored[0].2.clone()),
            _ if scored[0].0 - scored[1].0 >= DECISIVE_MARGIN => {
                Resolved::One(scored[0].1, scored[0].2.clone())
            }
            _ => {
                // Everything tied with the leader is a candidate; a distant third is not
                // a question anybody is really being asked.
                let top = scored[0].0;
                Resolved::Ambiguous(
                    scored
                        .into_iter()
                        .filter(|(s, ..)| top - *s < DECISIVE_MARGIN)
                        .map(|(_, kind, id, label)| Candidate { kind, id, label })
                        .collect(),
                )
            }
        }
    }

    /// Park a question where both surfaces can see it, and put the candidates in the same
    /// result list they already render — so there is no second list to drift.
    pub fn park(&mut self, prompt: &str, candidates: Vec<Candidate>) {
        self.db.view.list.results = candidates.iter().map(|c| c.id.clone()).collect();
        self.db.view.list.page = 0;
        self.db.view.pending = Some(Pending { prompt: prompt.to_string(), candidates });
    }

    // -- the shared page --------------------------------------------------------------

    /// Change the page **both** surfaces are on. Clamped to the last page rather than
    /// refused: a page that fell off the end of a shrinking result set is not an error.
    pub fn set_page(&mut self, page: usize) {
        let last = self.db.view.list.pages().saturating_sub(1);
        self.db.view.list.page = page.min(last);
    }

    /// Change the page size **both** surfaces see. This is deliberately not reachable from
    /// a caller's `-n`: see [`ListView`].
    pub fn set_page_size(&mut self, size: usize) {
        self.db.view.list.page_size = size.max(1);
        let page = self.db.view.list.page;
        self.set_page(page);
    }

    /// Re-order the shared list. Sort re-pages, because it is state.
    pub fn set_sort(&mut self, sort: Sort) {
        self.db.view.list.sort = sort;
        let ids = std::mem::take(&mut self.db.view.list.results);
        let scored: Vec<(i32, Kind, Id)> = ids
            .into_iter()
            .filter_map(|id| self.kind_of(&id).map(|k| (0, k, id)))
            .collect();
        let ordered = self.ordered(scored);
        self.db.view.list.results = ordered;
        self.db.view.list.page = 0;
    }

    pub fn set_stage_filter(&mut self, stage: Option<Stage>) {
        self.db.view.board.stage_filter = stage;
    }

    fn focus_on(&mut self, kind: Kind, id: &str) {
        self.db.view.focus = Some(Focus { kind, id: id.to_string() });
    }

    // -- internals --------------------------------------------------------------------

    fn kind_of(&self, id: &str) -> Option<Kind> {
        if self.db.company(id).is_some() {
            Some(Kind::Company)
        } else if self.db.contact(id).is_some() {
            Some(Kind::Contact)
        } else if self.db.deal(id).is_some() {
            Some(Kind::Deal)
        } else {
            None
        }
    }

    /// Every record a search may see, as `(kind, id, display name)`.
    fn searchable(&self, include_archived: bool) -> Vec<(Kind, Id, String)> {
        let mut out = Vec::new();
        for c in &self.db.companies {
            if include_archived || c.archived_at.is_none() {
                out.push((Kind::Company, c.id.clone(), c.name.clone()));
            }
        }
        for c in &self.db.contacts {
            if include_archived || c.archived_at.is_none() {
                out.push((Kind::Contact, c.id.clone(), c.name.clone()));
            }
        }
        for d in &self.db.deals {
            if include_archived || d.archived_at.is_none() {
                out.push((Kind::Deal, d.id.clone(), d.title.clone()));
            }
        }
        out
    }

    /// Put matches in the shared list's order. Relevance breaks ties in every mode, so two
    /// records updated in the same millisecond still come back in a stable order.
    fn ordered(&self, mut scored: Vec<(i32, Kind, Id)>) -> Vec<Id> {
        match self.db.view.list.sort {
            Sort::Updated => scored.sort_by(|a, b| {
                self.updated_at(&b.2).cmp(&self.updated_at(&a.2)).then_with(|| b.0.cmp(&a.0))
            }),
            Sort::Name => scored.sort_by(|a, b| {
                self.label_of(&a.2).to_lowercase().cmp(&self.label_of(&b.2).to_lowercase())
            }),
            Sort::Value => scored.sort_by(|a, b| {
                // Deals have a value and nothing else does, so everything else sorts
                // after — grouped rather than interleaved with an invented zero.
                self.value_of(&b.2).cmp(&self.value_of(&a.2)).then_with(|| b.0.cmp(&a.0))
            }),
        }
        scored.into_iter().map(|(_, _, id)| id).collect()
    }

    fn updated_at(&self, id: &str) -> Timestamp {
        self.db.company(id).map(|c| c.updated_at)
            .or_else(|| self.db.contact(id).map(|c| c.updated_at))
            .or_else(|| self.db.deal(id).map(|d| d.updated_at))
            .unwrap_or(0)
    }

    fn label_of(&self, id: &str) -> String {
        self.db.company(id).map(|c| c.name.clone())
            .or_else(|| self.db.contact(id).map(|c| c.name.clone()))
            .or_else(|| self.db.deal(id).map(|d| d.title.clone()))
            .unwrap_or_default()
    }

    /// A deal's value in minor units, for ordering only — never summed across currencies.
    fn value_of(&self, id: &str) -> i64 {
        self.db.deal(id).and_then(|d| d.value.as_ref()).map(|m| m.amount).unwrap_or(i64::MIN)
    }
}

/// How well a query matches one record, or `None` for no match at all.
///
/// The scale is what the [`DECISIVE_MARGIN`] is measured against: an exact hit is far
/// enough above a prefix, and a prefix far enough above a substring, that those are never
/// ambiguous — while two records matching the same way are exactly as close as they look.
fn match_score(query: &str, id: &str, name: &str) -> Option<i32> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        // An empty query is "everything", not "nothing" — `crm find` with no argument
        // lists the lot.
        return Some(0);
    }
    let name_l = name.to_ascii_lowercase();
    if id == q || name_l == q {
        return Some(100);
    }
    if name_l.starts_with(&q) {
        return Some(70);
    }
    if name_l.split_whitespace().any(|w| w.starts_with(&q)) {
        return Some(60);
    }
    if name_l.contains(&q) || id.contains(&q) {
        return Some(40);
    }
    None
}

/// Activities and tasks are not named, so their ids are not slugs. Time-ordered and
/// uniquified, so the log reads in the order it was written.
fn unique_activity_id(db: &Db, at: Timestamp) -> Id {
    sequential_id("a", at, &|c| db.activities.iter().any(|a| a.id == c))
}

fn unique_task_id(db: &Db, at: Timestamp) -> Id {
    sequential_id("t", at, &|c| db.tasks.iter().any(|t| t.id == c))
}

fn sequential_id(prefix: &str, at: Timestamp, taken: &dyn Fn(&str) -> bool) -> Id {
    let base = format!("{prefix}{at}");
    if !taken(&base) {
        return base;
    }
    for n in 2..=u32::MAX {
        let candidate = format!("{base}-{n}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!("u32::MAX records in one millisecond is not a state this app can reach")
}

// MARK: - The snapshot

impl AppState {
    /// One stamped view of everything both surfaces agree on.
    ///
    /// Stamped **here and nowhere else** (`clappkit::snapshot::with_rev`), so a response
    /// and the pushed `state` event carry the same `rev` when they describe the same
    /// moment.
    ///
    /// **Nothing secret ever enters this structure.** No credentials exist in v1, so that
    /// costs nothing today; it is written down because a snapshot goes everywhere and
    /// absence has to be by construction, not by redaction.
    pub fn snapshot(&self, now: Now) -> Value {
        let pipeline = self.db.pipeline();
        let (overdue, today, week) = self.due_counts(now);
        let view = &self.db.view;

        clappkit::snapshot::with_rev(json!({
            "ok": true,
            "pipeline": {
                "id": pipeline.id,
                "name": pipeline.name,
                "stages": pipeline.stages.iter().map(|s| s.word()).collect::<Vec<_>>(),
            },
            "board": self.board(),
            "focus": view.focus,
            "list": {
                "query": view.list.query,
                "sort": view.list.sort.word(),
                "page": view.list.page,
                "pageSize": view.list.page_size,
                "total": view.list.results.len(),
                "rows": view.list.page_ids().iter().filter_map(|id| self.row(id)).collect::<Vec<_>>(),
            },
            "pending": view.pending,
            "due": { "overdue": overdue, "today": today, "week": week },
            "counts": self.counts(),
            "agents": self.agents,
        }))
    }

    /// The board: every column, its deals and its totals.
    ///
    /// Archived deals are absent. Totals are **grouped by currency and never summed across
    /// them** — we hold no rate source, and inventing one would be worse than showing two
    /// numbers.
    fn board(&self) -> Value {
        let view = &self.db.view;
        let columns: Vec<Value> = Column::ALL
            .into_iter()
            // A stage filter narrows the board to that one column. Both surfaces see the
            // same narrowed board — a filter only one of them knew about would be drift
            // with better manners.
            .filter(|column| match (view.board.stage_filter, column) {
                (Some(want), Column::Stage(s)) => *s == want,
                (Some(_), Column::Closed(_)) => false,
                (None, _) => true,
            })
            .map(|column| {
                let deals: Vec<&Deal> = self.db.active_deals().filter(|d| column.holds(d)).collect();
                let deal_ids: Vec<&str> = deals.iter().map(|d| d.id.as_str()).collect();
                let totals = total_by_currency(deals.iter().copied());
                json!({
                    "key": column.key(),
                    "label": column.label(),
                    "dealIds": deal_ids,
                    "count": deals.len(),
                    "totals": totals,
                })
            })
            .collect();

        json!({
            "pipelineId": view.board.pipeline_id,
            "stageFilter": view.board.stage_filter.map(|s| s.word()),
            "columns": columns,
        })
    }

    /// One row of the shared list, in the shape both the window's table and `crm find`
    /// render. The deal-only fields are `null` on a company or a contact rather than
    /// absent, so a table has one shape to draw.
    fn row(&self, id: &str) -> Option<Value> {
        if let Some(c) = self.db.company(id) {
            return Some(json!({
                "kind": Kind::Company.word(), "id": c.id, "label": c.name,
                "detail": c.domain, "stage": Value::Null, "status": Value::Null,
                "value": Value::Null, "archived": c.archived_at.is_some(),
            }));
        }
        if let Some(c) = self.db.contact(id) {
            let detail = c.company_id.as_deref().and_then(|cid| self.db.company(cid)).map(|co| co.name.clone())
                .or_else(|| c.title.clone());
            return Some(json!({
                "kind": Kind::Contact.word(), "id": c.id, "label": c.name,
                "detail": detail, "stage": Value::Null, "status": Value::Null,
                "value": Value::Null, "archived": c.archived_at.is_some(),
            }));
        }
        if let Some(d) = self.db.deal(id) {
            let detail = d.company_id.as_deref().and_then(|cid| self.db.company(cid)).map(|co| co.name.clone());
            return Some(json!({
                "kind": Kind::Deal.word(), "id": d.id, "label": d.title,
                "detail": detail, "stage": d.stage.word(), "status": d.status.word(),
                "value": d.value, "archived": d.archived_at.is_some(),
            }));
        }
        None
    }

    /// Counts of **active** records: archived ones are excluded here, from the board and
    /// from default `find` results. A task is active until it is done; an activity is a log
    /// line and there is nothing to deactivate.
    fn counts(&self) -> Value {
        json!({
            "companies": self.db.companies.iter().filter(|c| c.archived_at.is_none()).count(),
            "contacts": self.db.contacts.iter().filter(|c| c.archived_at.is_none()).count(),
            "deals": self.db.active_deals().count(),
            "activities": self.db.activities.len(),
            "tasks": self.db.tasks.iter().filter(|t| t.done_at.is_none()).count(),
        })
    }
}

// MARK: - The command envelope

impl AppState {
    /// Apply one command envelope from either surface.
    ///
    /// M1 answers the two read verbs that need no arguments. **M2 maps the rest of
    /// `connector.commands` onto the typed operations above** — the envelope's argument
    /// shape is part of the surface contract and is the PM's to settle, so M1 deliberately
    /// stops here rather than inventing one that M3 would then have to match.
    ///
    /// The window verbs (`focus`, `close`, `ping`) never arrive here: clappkit's
    /// `window_cmd` answers those itself, because they are the app process rather than its
    /// state.
    pub fn command(&mut self, req: &Value, caller: Option<&str>, now: Now) -> Outcome {
        let _ = caller; // M2: becomes the `Actor` on whatever the verb writes.
        let cmd = req.get("cmd").and_then(Value::as_str).unwrap_or("");
        let snapshot = self.snapshot(now);
        match cmd {
            // `status` is the agent's; `state` is the window asking for its first paint.
            // One answer, because there is one state.
            "status" | "state" => Outcome {
                resp: snapshot.clone(),
                snapshot,
                emits: Vec::new(),
                dirty: false,
            },
            other => Outcome {
                resp: unknown(other),
                snapshot,
                emits: Vec::new(),
                dirty: false,
            },
        }
    }
}

/// The refusal for a verb this build does not have. It names the manual rather than listing
/// the verbs, because `crm -h` is the manual and a second list would be a second thing to
/// keep true.
fn unknown(cmd: &str) -> Value {
    let what = if cmd.is_empty() { "a command with no verb".to_string() } else { format!("`{cmd}`") };
    json!({
        "ok": false,
        "error": format!("{what} is not a verb this build answers — see `crm -h`"),
    })
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
