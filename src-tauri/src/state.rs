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

/// Where an open next step falls relative to today. Disjoint by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Overdue,
    Today,
    Week,
}

impl Bucket {
    pub fn word(&self) -> &'static str {
        match self {
            Bucket::Overdue => "overdue",
            Bucket::Today => "today",
            Bucket::Week => "week",
        }
    }
}

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
    /// Which record type the list is narrowed to, or `None` for all three.
    ///
    /// **Shared, like page and sort.** The window's People and Companies are this filter;
    /// narrowing only in the window would leave the footer saying "25 of 143" over four
    /// visible rows, and leave the agent looking at a list the person is not.
    #[serde(default)]
    pub kind: Option<Kind>,
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
            kind: None,
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
///
/// **The parked question is the parked ACTION.** "Log a call with Acme" that hit two
/// companies is not asking "which Acme did you mean, so I can open it" — it is asking
/// "which Acme did you mean, so I can finish logging the call." [`resume`] is what makes
/// `select` complete that write instead of merely opening whichever record was chosen; a
/// plain `show`/`open` ambiguity carries none, because opening the record *is* the whole
/// of what it was asked to do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub prompt: String,
    pub candidates: Vec<Candidate>,
    /// Whether answering this one *completes a write* rather than only opening a record
    /// — the plain, always-true half of [`resume`] that rides the snapshot, so the CLI
    /// (and the window) can say so without needing the deferred command itself, which
    /// never leaves this process.
    #[serde(default)]
    pub resuming: bool,
    /// The write that was interrupted, re-run against the id `select` resolves. Never
    /// serialized: it is core-internal, does not survive a restart (a short-lived
    /// question is an acceptable place for that to matter), and the `resuming` flag
    /// above is the only thing anything outside this file ever needs to know about it.
    #[serde(skip)]
    pub resume: Option<PendingResume>,
}

/// What to do once a parked ambiguity resolves to one id: re-dispatch `cmd` with `req`,
/// after writing the resolved id into `req[id_key]`. `req` is the original envelope
/// verbatim (still carrying the handle that was ambiguous, which the second dispatch
/// simply ignores in favour of the id now present) — capturing it whole, rather than
/// picking apart which fields mattered, is what lets one mechanism serve every write
/// verb that resolves a reference, including `link`'s second one if resolving the first
/// leaves it still ambiguous.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingResume {
    pub cmd: String,
    pub req: Value,
    pub id_key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub kind: Kind,
    pub id: Id,
    /// What to type to pick this one. The id is never shown to anybody.
    #[serde(default)]
    pub handle: Handle,
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
    /// The last id minted, so the next one can be made monotonic when the clock has not
    /// moved. Transient: after a restart the first id of the session draws fresh entropy,
    /// which is exactly as unique.
    last_ulid: Option<Ulid>,
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
        AppState { db, agents: Vec::new(), last_ulid: None }
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
// The typed API both surfaces drive. M2's `cli.rs` maps every CLI verb onto these, and
// M3's window maps its own controls onto the same ones, which is what makes it
// impossible for the two to disagree about a rule.
impl AppState {
    // -- minting ----------------------------------------------------------------------

    /// A fresh [`Ulid`] for a new record.
    ///
    /// Monotonic within a millisecond, which matters because one command can create more
    /// than one record and a command gets **one** draw of entropy — without this, the
    /// second record would take the first one's id and overwrite it.
    fn mint_id(&mut self, ctx: &Ctx) -> Id {
        let next = match self.last_ulid {
            Some(prev) => Ulid::next_after(prev, ctx.at(), ctx.entropy),
            None => Ulid::from_parts(ctx.at(), ctx.entropy),
        };
        self.last_ulid = Some(next);
        next.to_string()
    }

    /// A typable handle from a name, uniquified against the ones already taken.
    fn mint_handle(&self, name: &str, fallback: &str) -> Handle {
        unique_handle(name, &|h| self.db.handle_taken(h), fallback)
    }

    // -- creating ---------------------------------------------------------------------

    pub fn add_company(&mut self, name: &str, ctx: &Ctx) -> Id {
        let id = self.mint_id(ctx);
        let handle = self.mint_handle(name, "company");
        self.db.companies.push(Company {
            id: id.clone(),
            handle,
            name: name.trim().to_string(),
            domain: None,
            tags: Vec::new(),
            notes: None,
            archived_at: None,
            updated_at: ctx.at(),
            origin: ctx.origin.clone(),
        });
        self.focus_on(Kind::Company, &id);
        id
    }

    pub fn add_contact(&mut self, name: &str, company_id: Option<&str>, ctx: &Ctx) -> Id {
        let id = self.mint_id(ctx);
        let handle = self.mint_handle(name, "contact");
        self.db.contacts.push(Contact {
            id: id.clone(),
            handle,
            name: name.trim().to_string(),
            email: None,
            phone: None,
            title: None,
            company_id: company_id.map(str::to_string),
            tags: Vec::new(),
            archived_at: None,
            updated_at: ctx.at(),
            origin: ctx.origin.clone(),
        });
        self.focus_on(Kind::Contact, &id);
        id
    }

    /// A new deal starts at [`Stage::Lead`] and [`Status::Open`], in the one pipeline.
    pub fn add_deal(
        &mut self,
        title: &str,
        company_id: Option<&str>,
        value: Option<Money>,
        caller: Option<&str>,
        ctx: &Ctx,
    ) -> Id {
        let id = self.mint_id(ctx);
        let handle = self.mint_handle(title, "deal");
        let pipeline_id = self.db.pipeline().id;
        self.db.deals.push(Deal {
            id: id.clone(),
            handle,
            title: title.trim().to_string(),
            company_id: company_id.map(str::to_string),
            contact_ids: Vec::new(),
            value,
            stage: Stage::Lead,
            status: Status::Open,
            pipeline_id,
            opened_at: ctx.at(),
            closed_at: None,
            // Creating a deal is putting it on the board, so its creator is its first
            // mover — the card has a `by` from the moment it exists.
            moved_by: Actor::from_caller(caller),
            moved_at: ctx.at(),
            archived_at: None,
            updated_at: ctx.at(),
            origin: ctx.origin.clone(),
        });
        self.focus_on(Kind::Deal, &id);
        id
    }

    /// Change a record's display name.
    ///
    /// **Neither the id nor the handle moves.** The id is the identity and every reference
    /// stores it; the handle is what somebody has already written down, in a note or in a
    /// shell history. Re-deriving it from the new name would silently break both.
    pub fn rename(&mut self, id: &str, name: &str, ctx: &Ctx) -> Result<(), String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("a name cannot be empty".to_string());
        }
        if let Some(c) = self.db.companies.iter_mut().find(|c| c.id == id) {
            c.name = name;
            c.updated_at = ctx.at();
            c.origin = ctx.origin.clone();
            return Ok(());
        }
        if let Some(c) = self.db.contacts.iter_mut().find(|c| c.id == id) {
            c.name = name;
            c.updated_at = ctx.at();
            c.origin = ctx.origin.clone();
            return Ok(());
        }
        if let Some(d) = self.db.deals.iter_mut().find(|d| d.id == id) {
            d.title = name;
            d.updated_at = ctx.at();
            d.origin = ctx.origin.clone();
            return Ok(());
        }
        Err(gone())
    }

    // -- the pipeline -----------------------------------------------------------------

    /// `crm move <deal> <word>`.
    ///
    /// **Stage and status are different axes.** Closing sets the status and leaves the
    /// stage exactly where it was, which is what makes "how many did we win out of
    /// Negotiation" answerable later. Moving a closed deal to a stage **reopens** it,
    /// because that is the only thing the request can sensibly mean.
    pub fn move_deal(
        &mut self,
        id: &str,
        target: MoveTarget,
        caller: Option<&str>,
        ctx: &Ctx,
    ) -> Result<(), String> {
        let Some(deal) = self.db.deals.iter_mut().find(|d| d.id == id) else {
            return Err(gone());
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
                deal.closed_at = Some(ctx.at());
                // `deal.stage` is deliberately untouched.
            }
        }
        // Who put it here. Set on a move and on creation and on nothing else, so the
        // card's attribution disc names the mover, not whoever last logged a call.
        deal.moved_by = Actor::from_caller(caller);
        deal.moved_at = ctx.at();
        deal.updated_at = ctx.at();
        deal.origin = ctx.origin.clone();
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
        ctx: &Ctx,
    ) -> Id {
        let id = self.mint_id(ctx);
        self.db.activities.push(Activity {
            id: id.clone(),
            kind,
            body: body.trim().to_string(),
            at: ctx.at(),
            links,
            by: Actor::from_caller(caller),
        });
        id
    }

    // -- next steps -------------------------------------------------------------------

    pub fn add_task(
        &mut self,
        what: &str,
        due: Date,
        links: Vec<Id>,
        caller: Option<&str>,
        ctx: &Ctx,
    ) -> Id {
        let id = self.mint_id(ctx);
        let handle = self.mint_handle(what, "task");
        self.db.tasks.push(Task {
            id: id.clone(),
            handle,
            what: what.trim().to_string(),
            due,
            links,
            done_at: None,
            by: Actor::from_caller(caller),
            updated_at: ctx.at(),
            origin: ctx.origin.clone(),
        });
        id
    }

    pub fn complete_task(&mut self, id: &str, ctx: &Ctx) -> Result<(), String> {
        let Some(task) = self.db.tasks.iter_mut().find(|t| t.id == id) else {
            return Err("that task no longer exists — reopen its record".to_string());
        };
        task.done_at = Some(ctx.at());
        task.updated_at = ctx.at();
        task.origin = ctx.origin.clone();
        Ok(())
    }

    /// Every open next step that is overdue, due today, or due within the next week, with
    /// the bucket it falls in — soonest first, then in creation order.
    ///
    /// **The one place the buckets are decided.** [`AppState::due_counts`] (the numbers on
    /// every snapshot) and `crm due`'s listing both come from here, so the count and the
    /// list beside it cannot disagree about which task is "this week".
    pub fn due_tasks(&self, now: Now) -> Vec<(Bucket, &Task)> {
        let mut out: Vec<(Bucket, &Task)> = self
            .db
            .tasks
            .iter()
            .filter(|t| t.done_at.is_none())
            .filter_map(|t| {
                let bucket = match now.today.days_until(t.due) {
                    d if d < 0 => Bucket::Overdue,
                    0 => Bucket::Today,
                    d if d <= WEEK_DAYS => Bucket::Week,
                    _ => return None,
                };
                Some((bucket, t))
            })
            .collect();
        out.sort_by(|a, b| a.1.due.cmp(&b.1.due).then_with(|| a.1.id.cmp(&b.1.id)));
        out
    }

    /// Overdue, due today, and due within the next week. **Disjoint buckets**, so the three
    /// numbers can be read side by side without double-counting the same task.
    pub fn due_counts(&self, now: Now) -> (usize, usize, usize) {
        let tasks = self.due_tasks(now);
        let count = |b: Bucket| tasks.iter().filter(|(bucket, _)| *bucket == b).count();
        (count(Bucket::Overdue), count(Bucket::Today), count(Bucket::Week))
    }

    // -- associating ------------------------------------------------------------------

    /// Associate a contact or a company with a deal. Idempotent: linking twice is not an
    /// error, because an agent retrying a call must not corrupt the graph.
    pub fn link(&mut self, deal_id: &str, other_id: &str, ctx: &Ctx) -> Result<(), String> {
        let is_contact = self.db.contact(other_id).is_some();
        let is_company = self.db.company(other_id).is_some();
        if !is_contact && !is_company {
            return Err("only a contact or a company can be linked to a deal, and that is neither".to_string());
        }
        let Some(deal) = self.db.deals.iter_mut().find(|d| d.id == deal_id) else {
            return Err(gone());
        };
        if is_contact {
            if !deal.contact_ids.iter().any(|c| c == other_id) {
                deal.contact_ids.push(other_id.to_string());
            }
        } else {
            deal.company_id = Some(other_id.to_string());
        }
        deal.updated_at = ctx.at();
        deal.origin = ctx.origin.clone();
        Ok(())
    }

    // -- editing ------------------------------------------------------------------------

    /// `crm set <handle> <field> <value>`.
    ///
    /// Field names are the core's vocabulary, exactly like a stage word: unknown ones are
    /// a **valid request the core declined**, not a bad command line, because the CLI has
    /// no list of its own to check them against — the core is what would know if a field
    /// were ever added or removed. `name` is accepted on all three kinds and aliases to
    /// [`AppState::rename`], so the same word edits a company, a contact or a deal.
    pub fn set_field(&mut self, id: &str, field: &str, raw: &str, ctx: &Ctx) -> Result<(), String> {
        let kind = self.kind_of(id).ok_or_else(gone)?;
        let field = field.trim().to_ascii_lowercase();
        if field == "name" {
            return self.rename(id, raw, ctx);
        }
        match kind {
            Kind::Company => match field.as_str() {
                "domain" => self.db.companies.iter_mut().find(|c| c.id == id).unwrap().domain = non_empty(raw),
                "notes" => self.db.companies.iter_mut().find(|c| c.id == id).unwrap().notes = non_empty(raw),
                "tags" => self.db.companies.iter_mut().find(|c| c.id == id).unwrap().tags = parse_tag_list(raw),
                other => return Err(unknown_field(kind, other, &["name", "domain", "notes", "tags"])),
            },
            Kind::Contact => match field.as_str() {
                "email" => self.db.contacts.iter_mut().find(|c| c.id == id).unwrap().email = non_empty(raw),
                "phone" => self.db.contacts.iter_mut().find(|c| c.id == id).unwrap().phone = non_empty(raw),
                "title" => self.db.contacts.iter_mut().find(|c| c.id == id).unwrap().title = non_empty(raw),
                "tags" => self.db.contacts.iter_mut().find(|c| c.id == id).unwrap().tags = parse_tag_list(raw),
                "company" => {
                    let company_id = non_empty(raw).map(|r| self.resolve_decisive(&r)).transpose()?;
                    if let Some((found, _)) = &company_id {
                        if *found != Kind::Company {
                            return Err(format!("“{raw}” is a {}, not a company", found.word()));
                        }
                    }
                    self.db.contacts.iter_mut().find(|c| c.id == id).unwrap().company_id =
                        company_id.map(|(_, cid)| cid);
                }
                other => {
                    return Err(unknown_field(kind, other, &["name", "email", "phone", "title", "company", "tags"]))
                }
            },
            Kind::Deal => match field.as_str() {
                "value" => {
                    let existing = self.db.deal(id).and_then(|d| d.value.as_ref()).map(|v| v.currency.clone());
                    let value = non_empty(raw).map(|r| parse_money_field(&r, existing.as_deref())).transpose()?;
                    self.db.deals.iter_mut().find(|d| d.id == id).unwrap().value = value;
                }
                "company" => {
                    let company_id = non_empty(raw).map(|r| self.resolve_decisive(&r)).transpose()?;
                    if let Some((found, _)) = &company_id {
                        if *found != Kind::Company {
                            return Err(format!("“{raw}” is a {}, not a company", found.word()));
                        }
                    }
                    self.db.deals.iter_mut().find(|d| d.id == id).unwrap().company_id =
                        company_id.map(|(_, cid)| cid);
                }
                other => return Err(unknown_field(kind, other, &["name", "value", "company"])),
            },
        }
        self.touch(id, ctx);
        Ok(())
    }

    /// Re-stamp `updated_at`/`origin` on whatever `id` names, after [`set_field`] edited a
    /// field directly rather than through a method that already stamps its own write.
    fn touch(&mut self, id: &str, ctx: &Ctx) {
        if let Some(c) = self.db.companies.iter_mut().find(|c| c.id == id) {
            c.updated_at = ctx.at();
            c.origin = ctx.origin.clone();
        } else if let Some(c) = self.db.contacts.iter_mut().find(|c| c.id == id) {
            c.updated_at = ctx.at();
            c.origin = ctx.origin.clone();
        } else if let Some(d) = self.db.deals.iter_mut().find(|d| d.id == id) {
            d.updated_at = ctx.at();
            d.origin = ctx.origin.clone();
        }
    }

    /// A record named by an **id** the caller already has, or a **handle** it typed —
    /// resolved decisively, with no ambiguity parking. Used for a reference that is a
    /// *detail* of a bigger write (`--company acme`), never the write's own subject: an
    /// ambiguous detail is refused outright rather than turned into a second pending
    /// question competing with the main one.
    fn resolve_decisive(&self, s: &str) -> Result<(Kind, Id), String> {
        if let Some(kind) = self.kind_of(s) {
            return Ok((kind, s.to_string()));
        }
        match self.resolve(s, true) {
            Resolved::One(kind, id) => Ok((kind, id)),
            Resolved::Ambiguous(candidates) => {
                let handles: Vec<String> = candidates.iter().map(|c| c.handle.clone()).collect();
                Err(format!("“{s}” matches more than one record — use its exact handle: {}", handles.join(", ")))
            }
            Resolved::None => Err(self.no_match_message(s)),
        }
    }

    /// The refusal for a handle or id that named nothing — shared by every write that
    /// resolves a reference, so the message is the same whichever verb hit it.
    fn no_match_message(&self, typed: &str) -> String {
        match self.closest_handle(typed) {
            Some(near) => format!("no record matches “{typed}” — did you mean `crm show {near}`?"),
            None if self.db.companies.is_empty() && self.db.contacts.is_empty() && self.db.deals.is_empty() => {
                format!("no record matches “{typed}” — there are no records yet")
            }
            None => format!(
                "no record matches “{typed}” — every record's handle is shown beside it in the window"
            ),
        }
    }

    // -- exporting --------------------------------------------------------------------

    /// One kind's active (or, with `include_archived`, every) record as ordered
    /// `(column, value)` pairs — the same columns for every row of one kind, which is
    /// what lets a generic CSV or JSON renderer draw them without knowing the domain.
    ///
    /// Values are exactly what a person could type back in: handles, never ids; a
    /// decimal amount, never a formatted one with a symbol or thousands separators.
    pub fn export_rows(&self, kind: Kind, include_archived: bool) -> Vec<Vec<(&'static str, String)>> {
        let company_ref = |id: &Option<Id>| id.as_deref().and_then(|c| self.db.handle_of(c)).unwrap_or("").to_string();
        match kind {
            Kind::Company => self
                .db
                .companies
                .iter()
                .filter(|c| include_archived || c.archived_at.is_none())
                .map(|c| {
                    vec![
                        ("handle", c.handle.clone()),
                        ("name", c.name.clone()),
                        ("domain", c.domain.clone().unwrap_or_default()),
                        ("tags", c.tags.join(";")),
                        ("notes", c.notes.clone().unwrap_or_default()),
                        ("archived", c.archived_at.is_some().to_string()),
                    ]
                })
                .collect(),
            Kind::Contact => self
                .db
                .contacts
                .iter()
                .filter(|c| include_archived || c.archived_at.is_none())
                .map(|c| {
                    vec![
                        ("handle", c.handle.clone()),
                        ("name", c.name.clone()),
                        ("company", company_ref(&c.company_id)),
                        ("email", c.email.clone().unwrap_or_default()),
                        ("phone", c.phone.clone().unwrap_or_default()),
                        ("title", c.title.clone().unwrap_or_default()),
                        ("tags", c.tags.join(";")),
                        ("archived", c.archived_at.is_some().to_string()),
                    ]
                })
                .collect(),
            Kind::Deal => self
                .db
                .deals
                .iter()
                .filter(|d| include_archived || d.archived_at.is_none())
                .map(|d| {
                    let contacts: Vec<&str> =
                        d.contact_ids.iter().filter_map(|c| self.db.handle_of(c)).collect();
                    vec![
                        ("handle", d.handle.clone()),
                        ("title", d.title.clone()),
                        ("company", company_ref(&d.company_id)),
                        ("contacts", contacts.join(";")),
                        ("value", d.value.as_ref().map(Money::decimal).unwrap_or_default()),
                        ("currency", d.value.as_ref().map(|v| v.currency.clone()).unwrap_or_default()),
                        ("stage", d.stage.word().to_string()),
                        ("status", d.status.word().to_string()),
                        ("archived", d.archived_at.is_some().to_string()),
                    ]
                })
                .collect(),
        }
    }

    // -- retiring ---------------------------------------------------------------------

    /// Retire a record. **Reversible, and never a hard delete** — an agent holding a delete
    /// verb and a bad fuzzy match is an unrecoverable afternoon.
    ///
    /// An archived record leaves the board, the counts and default `find` results. `show`
    /// still loads it, and [`AppState::restore`] brings it back.
    pub fn archive(&mut self, id: &str, ctx: &Ctx) -> Result<(), String> {
        match self.record_archived_state(id) {
            None => Err(gone()),
            Some(true) => Err("that record is already archived".to_string()),
            Some(false) => self.set_archived(id, Some(ctx.at()), ctx),
        }
    }

    pub fn restore(&mut self, id: &str, ctx: &Ctx) -> Result<(), String> {
        match self.record_archived_state(id) {
            None => Err(gone()),
            Some(false) => Err("that record is not archived — nothing to restore".to_string()),
            Some(true) => self.set_archived(id, None, ctx),
        }
    }

    /// Whether `id` names a company, contact or deal that is currently archived — `None`
    /// when it names nothing. Separate from [`AppState::is_archived`], which collapses
    /// "archived" and "no such record" into one `false`; [`archive`](AppState::archive)
    /// and [`restore`](AppState::restore) need to tell those apart.
    fn record_archived_state(&self, id: &str) -> Option<bool> {
        self.db.company(id).map(|c| c.archived_at.is_some())
            .or_else(|| self.db.contact(id).map(|c| c.archived_at.is_some()))
            .or_else(|| self.db.deal(id).map(|d| d.archived_at.is_some()))
    }

    fn set_archived(&mut self, id: &str, at: Option<Timestamp>, ctx: &Ctx) -> Result<(), String> {
        let origin = ctx.origin.clone();
        if let Some(c) = self.db.companies.iter_mut().find(|c| c.id == id) {
            c.archived_at = at;
            c.updated_at = ctx.at();
            c.origin = origin;
            return Ok(());
        }
        if let Some(c) = self.db.contacts.iter_mut().find(|c| c.id == id) {
            c.archived_at = at;
            c.updated_at = ctx.at();
            c.origin = origin;
            return Ok(());
        }
        if let Some(d) = self.db.deals.iter_mut().find(|d| d.id == id) {
            d.archived_at = at;
            d.updated_at = ctx.at();
            d.origin = origin;
            return Ok(());
        }
        Err(gone())
    }

    /// Not called by any verb — `archive`/`restore` use `record_archived_state`
    /// instead, since they need to tell "archived" from "no such record" apart.
    /// Kept for what a caller checking only "is this hidden from the default views"
    /// would want, and exercised by the tests that pin that behaviour.
    #[allow(dead_code)]
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
        let scored = self.search(query, include_archived);
        // Ordered before the assignment, so the immutable borrow `ordered` needs is over
        // before the list is written back.
        let ordered = self.ordered(scored);
        self.db.view.list.query = query.trim().to_string();
        self.db.view.list.results = ordered;
        self.db.view.list.page = 0;
        self.db.view.list.results.len()
    }

    /// Every match for `query` within the list's current kind filter, scored.
    fn search(&self, query: &str, include_archived: bool) -> Vec<(i32, Kind, Id)> {
        let only = self.db.view.list.kind;
        self.searchable(include_archived)
            .into_iter()
            .filter(|(kind, ..)| only.is_none_or(|k| k == *kind))
            .filter_map(|(kind, id, handle, name)| {
                match_score(query, &handle, &name).map(|score| (score, kind, id))
            })
            .collect()
    }

    /// Narrow the shared list to one record type, or widen it back to all three. Re-runs
    /// the search, because a filter that changes the rows without changing the total is
    /// the footer lying.
    /// Not called by any verb: `find`'s own envelope carries `kind` alongside `query`,
    /// so both surfaces narrow the list through one round trip rather than two — see
    /// `cmd_find`. Kept as the one-line operation the rule describes, and exercised
    /// directly by the tests that pin it.
    #[allow(dead_code)]
    pub fn set_list_kind(&mut self, kind: Option<Kind>) {
        self.db.view.list.kind = kind;
        let query = self.db.view.list.query.clone();
        self.find(&query, false);
    }

    /// Open one record: sets [`View::focus`], which is what puts it on the person's screen.
    /// Archived records are still loadable — that is the difference between archiving and
    /// deleting.
    pub fn show(&mut self, id: &str) -> Result<(), String> {
        let kind = self.kind_of(id).ok_or_else(gone)?;
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

        // **An exact handle match is decisive.** This is the whole of "resolution happens
        // at the edge": what somebody types is a handle, and past this line the core
        // speaks only ids. Agent ergonomics are unchanged by ids becoming ULIDs.
        if let Some((kind, id)) = self.db.by_handle(&needle) {
            return Resolved::One(kind, id);
        }

        // An id is decisive too, for a caller that already has one — the window clicking
        // a row, or an agent pasting a value straight back out of a snapshot.
        if let Some(kind) = self.db.kind_of(query.trim()) {
            return Resolved::One(kind, query.trim().to_string());
        }

        let mut scored: Vec<(i32, Kind, Id, Handle, String)> = Vec::new();
        for (kind, id, handle, name) in self.searchable(include_archived) {
            if let Some(score) = match_score(query, &handle, &name) {
                scored.push((score, kind, id, handle, name));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.4.cmp(&b.4)));

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
                        .map(|(_, kind, id, handle, label)| Candidate { kind, id, handle, label })
                        .collect(),
                )
            }
        }
    }

    /// Park a plain question — answering it only opens whichever record was meant, which
    /// is the whole of what `show`/`open` were asked to do. Write verbs that resolve a
    /// reference use [`park_with_resume`](Self::park_with_resume) instead, so answering
    /// completes the write it interrupted rather than merely opening a record.
    pub fn park(&mut self, prompt: &str, candidates: Vec<Candidate>) {
        self.park_with_resume(prompt, candidates, None);
    }

    /// Park a question where both surfaces can see it, put the candidates in the same
    /// result list they already render — so there is no second list to drift — and, when
    /// `resume` is given, remember the write that hit the ambiguity so `select` can finish
    /// it once a candidate is chosen.
    fn park_with_resume(&mut self, prompt: &str, candidates: Vec<Candidate>, resume: Option<PendingResume>) {
        self.db.view.list.results = candidates.iter().map(|c| c.id.clone()).collect();
        self.db.view.list.page = 0;
        self.db.view.pending =
            Some(Pending { prompt: prompt.to_string(), candidates, resuming: resume.is_some(), resume });
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
    /// Not reachable from either surface today: `page_size` is fixed at
    /// `DEFAULT_PAGE_SIZE` by convention (`docs/qa/round-2.md`), and no verb in the
    /// frozen grammar changes it. Kept because the rule — shared, never the caller's —
    /// needs an operation to say so, for the day a surface earns a control for it.
    #[allow(dead_code)]
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
        self.db.kind_of(id)
    }

    /// Every record a search may see, as `(kind, id, handle, display name)`. A query is
    /// matched against the handle and the name — never the id, which nobody types and
    /// nobody reads.
    fn searchable(&self, include_archived: bool) -> Vec<(Kind, Id, Handle, String)> {
        let mut out = Vec::new();
        for c in &self.db.companies {
            if include_archived || c.archived_at.is_none() {
                out.push((Kind::Company, c.id.clone(), c.handle.clone(), c.name.clone()));
            }
        }
        for c in &self.db.contacts {
            if include_archived || c.archived_at.is_none() {
                out.push((Kind::Contact, c.id.clone(), c.handle.clone(), c.name.clone()));
            }
        }
        for d in &self.db.deals {
            if include_archived || d.archived_at.is_none() {
                out.push((Kind::Deal, d.id.clone(), d.handle.clone(), d.title.clone()));
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
fn match_score(query: &str, handle: &str, name: &str) -> Option<i32> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        // An empty query is "everything", not "nothing" — `crm find` with no argument
        // lists the lot.
        return Some(0);
    }
    let name_l = name.to_ascii_lowercase();
    if handle == q || name_l == q {
        return Some(100);
    }
    if name_l.starts_with(&q) {
        return Some(70);
    }
    if name_l.split_whitespace().any(|w| w.starts_with(&q)) {
        return Some(60);
    }
    if name_l.contains(&q) || handle.contains(&q) {
        return Some(40);
    }
    None
}

// MARK: - The snapshot

/// How many of a record's activities ride the snapshot. A snapshot is pushed on every
/// change; an unbounded timeline would make every push as large as the busiest record's
/// entire history. `timelineTotal` says how many there are in all.
pub const TIMELINE_CAP: usize = 50;

/// How many related records a single field names before it says "and N more".
const RELATED_CAP: usize = 5;

/// One labelled line of a record, exactly as both surfaces print it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Field {
    pub label: String,
    pub value: String,
}

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
    ///
    /// **Every change to this shape is additive.** Round 3 added `cards`, `focused`,
    /// `list.kind` and `formatted` on every money value; nothing that was here before was
    /// removed, renamed or retyped. `fixtures/snapshot.json` is this function's output,
    /// committed, and a test fails when the two disagree.
    pub fn snapshot(&self, now: Now) -> Value {
        let pipeline = self.db.pipeline();
        let (overdue, today, week) = self.due_counts(now);
        let view = &self.db.view;
        let columns = self.board_columns();

        let mut snap = json!({
            "ok": true,
            "pipeline": {
                "id": pipeline.id,
                "name": pipeline.name,
                "stages": pipeline.stages.iter().map(|s| s.word()).collect::<Vec<_>>(),
            },
            "board": self.board(&columns),
            "cards": self.cards(&columns),
            "focus": self.focus_json(),
            "list": {
                "query": view.list.query,
                "sort": view.list.sort.word(),
                "page": view.list.page,
                "pageSize": view.list.page_size,
                "total": view.list.results.len(),
                "rows": view.list.page_ids().iter().filter_map(|id| self.row(id)).collect::<Vec<_>>(),
                "kind": view.list.kind.map(|k| k.word()),
            },
            "pending": view.pending,
            "due": { "overdue": overdue, "today": today, "week": week },
            "counts": self.counts(),
            "agents": self.agents,
        });

        // Present **if and only if** `focus` is non-null — absent, not null, otherwise.
        if let Some(focused) = self.focused_json() {
            snap["focused"] = focused;
        }

        clappkit::snapshot::with_rev(snap)
    }

    /// What is open, as the snapshot carries it: the stored `{kind, id}` plus the
    /// `handle`, looked up rather than stored — a handle is derived data, and a second
    /// copy of it in the view would be a second thing to keep true.
    ///
    /// **Additive.** `kind` and `id` are exactly what they were; `handle` is new beside
    /// them.
    fn focus_json(&self) -> Value {
        match &self.db.view.focus {
            None => Value::Null,
            Some(f) => json!({
                "kind": f.kind.word(),
                "id": f.id,
                "handle": self.db.handle_of(&f.id),
            }),
        }
    }

    /// The board's columns and the active deals in each, after the stage filter. Computed
    /// once per snapshot so `board` and `cards` cannot disagree about which deals are
    /// on it.
    fn board_columns(&self) -> Vec<(Column, Vec<&Deal>)> {
        let filter = self.db.view.board.stage_filter;
        Column::ALL
            .into_iter()
            // A stage filter narrows the board to that one column. Both surfaces see the
            // same narrowed board — a filter only one of them knew about would be drift
            // with better manners.
            .filter(|column| match (filter, column) {
                (Some(want), Column::Stage(s)) => *s == want,
                (Some(_), Column::Closed(_)) => false,
                (None, _) => true,
            })
            .map(|column| {
                let deals = self.db.active_deals().filter(|d| column.holds(d)).collect();
                (column, deals)
            })
            .collect()
    }

    /// The board: every column, its deals and its totals.
    ///
    /// Archived deals are absent. Totals are **grouped by currency and never summed across
    /// them** — we hold no rate source, and inventing one would be worse than showing two
    /// numbers.
    fn board(&self, columns: &[(Column, Vec<&Deal>)]) -> Value {
        let view = &self.db.view;
        let columns: Vec<Value> = columns
            .iter()
            .map(|(column, deals)| {
                let deal_ids: Vec<&str> = deals.iter().map(|d| d.id.as_str()).collect();
                let totals: Vec<Value> =
                    total_by_currency(deals.iter().copied()).iter().map(money_json).collect();
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

    /// The bodies behind `board.columns[].dealIds`, keyed by id: **one entry per id on the
    /// board and no others.** The key is an index; nothing reads it as text.
    ///
    /// Each card is the row shape plus who last put the deal where it is — `by` and
    /// `movedAt` are [`Deal::moved_by`] and [`Deal::moved_at`] verbatim. A card whose
    /// `movedAt` changed since the last snapshot is the one the window rings, in `by`'s
    /// tint.
    fn cards(&self, columns: &[(Column, Vec<&Deal>)]) -> Value {
        let mut cards = serde_json::Map::new();
        for deal in columns.iter().flat_map(|(_, deals)| deals.iter()) {
            if let Some(mut card) = self.row(&deal.id) {
                card["by"] = json!(deal.moved_by);
                card["movedAt"] = json!(deal.moved_at);
                cards.insert(deal.id.clone(), card);
            }
        }
        Value::Object(cards)
    }

    /// Everything the record panel draws for what `focus` points at, or `None` when
    /// nothing is open.
    fn focused_json(&self) -> Option<Value> {
        let focus = self.db.view.focus.as_ref()?;
        let row = self.row(&focus.id)?;

        // Newest first. Ties on the instant fall back to the id, which is a ULID and
        // therefore sorts in creation order — so two lines written in one millisecond
        // still read in the order they were written.
        let mut timeline: Vec<&Activity> =
            self.db.activities.iter().filter(|a| a.links.contains(&focus.id)).collect();
        timeline.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
        let timeline_total = timeline.len();
        let timeline: Vec<Value> = timeline
            .into_iter()
            .take(TIMELINE_CAP)
            .map(|a| {
                json!({ "id": a.id, "kind": a.kind.word(), "body": a.body, "at": a.at, "by": a.by })
            })
            .collect();

        // Open first, soonest due first; then done, most recently done first.
        let mut tasks: Vec<&Task> =
            self.db.tasks.iter().filter(|t| t.links.contains(&focus.id)).collect();
        tasks.sort_by(|a, b| match (a.done_at, b.done_at) {
            (None, None) => a.due.cmp(&b.due).then_with(|| a.id.cmp(&b.id)),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => y.cmp(&x).then_with(|| a.id.cmp(&b.id)),
        });
        let tasks: Vec<Value> = tasks
            .into_iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "handle": t.handle,
                    "what": t.what,
                    // A civil date, in its one spelling. `Date` persists as a struct; on
                    // the wire it is the string a person would type.
                    "due": t.due.to_string_iso(),
                    "doneAt": t.done_at,
                    "by": t.by,
                })
            })
            .collect();

        Some(json!({
            "row": row,
            "fields": self.fields(&focus.id),
            "timeline": timeline,
            "timelineTotal": timeline_total,
            "tasks": tasks,
        }))
    }

    /// **The** description of a record: labels and formatted values, in order.
    ///
    /// One function, two surfaces. The window's record panel draws `focused.fields` and
    /// `crm show` prints the same list from the same snapshot, so the two cannot describe
    /// one record two ways.
    ///
    /// Empty fields are omitted rather than shown as a dash. A related record is named by
    /// its label **and its handle** — `Acme Corp (acme-corp)` — because an agent reading
    /// this wants to open that record next, and a name alone is not something it can type.
    pub fn fields(&self, id: &str) -> Vec<Field> {
        let mut out = Vec::new();
        let mut push = |label: &str, value: Option<String>| {
            if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
                out.push(Field { label: label.to_string(), value });
            }
        };

        if let Some(c) = self.db.company(id) {
            push("Domain", c.domain.clone());
            let people: Vec<(&str, &str)> = self
                .db
                .contacts
                .iter()
                .filter(|p| p.archived_at.is_none() && p.company_id.as_deref() == Some(id))
                .map(|p| (p.name.as_str(), p.handle.as_str()))
                .collect();
            push("Contacts", related(&people));
            let open: Vec<&Deal> = self
                .db
                .active_deals()
                .filter(|d| d.status.is_open() && d.company_id.as_deref() == Some(id))
                .collect();
            if !open.is_empty() {
                let totals: Vec<String> =
                    total_by_currency(open.iter().copied()).iter().map(Money::format).collect();
                let value = if totals.is_empty() {
                    open.len().to_string()
                } else {
                    format!("{} · {}", open.len(), totals.join(", "))
                };
                push("Open deals", Some(value));
            }
            push("Tags", Some(c.tags.join(", ")));
            push("Notes", c.notes.clone());
        } else if let Some(c) = self.db.contact(id) {
            push("Company", c.company_id.as_deref().and_then(|cid| self.company_ref(cid)));
            push("Title", c.title.clone());
            push("Email", c.email.clone());
            push("Phone", c.phone.clone());
            push("Tags", Some(c.tags.join(", ")));
        } else if let Some(d) = self.db.deal(id) {
            push("Company", d.company_id.as_deref().and_then(|cid| self.company_ref(cid)));
            let people: Vec<(&str, &str)> = d
                .contact_ids
                .iter()
                .filter_map(|cid| self.db.contact(cid))
                .map(|p| (p.name.as_str(), p.handle.as_str()))
                .collect();
            push("Contacts", related(&people));
            push("Value", d.value.as_ref().map(Money::format));
            push("Stage", Some(d.stage.label().to_string()));
            push("Status", Some(d.status.label().to_string()));
        }
        out
    }

    /// `Acme Corp (acme-corp)` — a name to read and a handle to type.
    fn company_ref(&self, id: &str) -> Option<String> {
        self.db.company(id).map(|c| format!("{} ({})", c.name, c.handle))
    }

    /// One row of the shared list, in the shape both the window's table and `crm find`
    /// render. The deal-only fields are `null` on a company or a contact rather than
    /// absent, so a table has one shape to draw.
    ///
    /// `handle` is what an agent reads a row for: it prints the list, then types
    /// `crm show <handle>`. The `id` beside it stays opaque and is never displayed.
    fn row(&self, id: &str) -> Option<Value> {
        if let Some(c) = self.db.company(id) {
            return Some(json!({
                "kind": Kind::Company.word(), "id": c.id, "handle": c.handle, "label": c.name,
                "detail": c.domain, "stage": Value::Null, "status": Value::Null,
                "value": Value::Null, "archived": c.archived_at.is_some(),
            }));
        }
        if let Some(c) = self.db.contact(id) {
            let detail = c.company_id.as_deref().and_then(|cid| self.db.company(cid)).map(|co| co.name.clone())
                .or_else(|| c.title.clone());
            return Some(json!({
                "kind": Kind::Contact.word(), "id": c.id, "handle": c.handle, "label": c.name,
                "detail": detail, "stage": Value::Null, "status": Value::Null,
                "value": Value::Null, "archived": c.archived_at.is_some(),
            }));
        }
        if let Some(d) = self.db.deal(id) {
            let detail = d.company_id.as_deref().and_then(|cid| self.db.company(cid)).map(|co| co.name.clone());
            return Some(json!({
                "kind": Kind::Deal.word(), "id": d.id, "handle": d.handle, "label": d.title,
                "detail": detail, "stage": d.stage.word(), "status": d.status.word(),
                "value": d.value.as_ref().map(money_json), "archived": d.archived_at.is_some(),
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

    /// The handle nearest to something that matched nothing, if one is close enough to be
    /// what was meant. A suggestion must name a record that exists — that is the only kind
    /// worth printing.
    fn closest_handle(&self, typed: &str) -> Option<String> {
        let typed = typed.trim().to_ascii_lowercase();
        let handles = self
            .db
            .companies
            .iter()
            .map(|c| &c.handle)
            .chain(self.db.contacts.iter().map(|c| &c.handle))
            .chain(self.db.deals.iter().map(|d| &d.handle));
        let (best, distance) = handles
            .map(|h| (h, edit_distance(&typed, h)))
            .min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)))?;
        // Close means a typo, not a different word: at most a quarter of what was typed,
        // and never more than three edits.
        let allowed = (typed.len() / 4).clamp(1, 3);
        (distance <= allowed).then(|| best.clone())
    }
}

/// Every money value on the wire: the raw amount, because sorting and comparison need it,
/// and the one formatted string, so no surface does the arithmetic twice.
fn money_json(m: &Money) -> Value {
    json!({ "amount": m.amount, "currency": m.currency, "formatted": m.format() })
}

/// `Ada Lovelace (ada-lovelace), Grace Hopper (grace-hopper) and 3 more`.
fn related(people: &[(&str, &str)]) -> Option<String> {
    if people.is_empty() {
        return None;
    }
    let shown: Vec<String> =
        people.iter().take(RELATED_CAP).map(|(name, handle)| format!("{name} ({handle})")).collect();
    let rest = people.len().saturating_sub(RELATED_CAP);
    Some(if rest > 0 {
        format!("{} and {rest} more", shown.join(", "))
    } else {
        shown.join(", ")
    })
}

/// Levenshtein distance over bytes. Handles are ASCII by construction, so bytes are
/// characters here.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitute = prev[j] + usize::from(ca != cb);
            row[j + 1] = substitute.min(prev[j + 1] + 1).min(row[j] + 1);
        }
        std::mem::swap(&mut prev, &mut row);
    }
    prev[b.len()]
}

// MARK: - The command envelope

/// What a successful command did, for the caller who asked. The snapshot says what is
/// true now; this says what *this* request came to, which the snapshot alone cannot —
/// a `pending` in it may predate the request.
enum Answer {
    /// Read-only: nothing changed, nothing owes a write.
    Read,
    /// Read-only, but the response carries something beside the snapshot — `export`'s
    /// rows, which are this one request's answer and not shared state anybody persists.
    ReadWith(&'static str, Value),
    /// The shared state changed.
    Changed,
    /// The shared state changed, and the response says what *this* request came to beyond
    /// the snapshot — `select` uses it to name the write it just completed.
    ChangedWith(&'static str, Value),
    /// A lookup was too close to call, and a question is now parked for either surface.
    Ambiguous,
}

impl AppState {
    /// Apply one command envelope from either surface.
    ///
    /// The read/select/find/show/move envelopes are frozen in
    /// `docs/work-orders/round-3-snapshot.md` §5; the seven write shapes (`add`, `set`,
    /// `log`, `task`, `done`, `link`, `archive`) were added to it for M2. All of them carry
    /// **ids, not handles** — no person reads the wire, and the window already holds the
    /// id. The one exception is `open`, the CLI's form of `show`, which carries what the
    /// agent typed: resolution happens here, at the edge, and nowhere later. It has its
    /// own name because clappkit's IPC relay answers `show` itself (as `focus`) before a
    /// request reaches the core; the window's `run_cmd` does not pass through that relay.
    ///
    /// **Every write a CLI verb makes goes through the SAME handler the window's id-based
    /// envelope does** — `crm task <handle> …` and the window dragging a task both end in
    /// [`AppState::add_task`]. What differs is only how the target record is named: the
    /// window already has an `id`; the CLI has a `handle` a person typed, and
    /// [`AppState::resolve_write_target`] turns that into the same id, with the same
    /// ambiguity handling `open` uses. This is what makes "the CLI and the window cannot
    /// drift" true for every verb here, not only the read ones M1 shipped.
    ///
    /// | envelope                                        | same as                        |
    /// |-------------------------------------------------|--------------------------------|
    /// | `{ cmd: "state" }` · `{ cmd: "status" }`        | `crm status`                   |
    /// | `{ cmd: "show", kind, id }`                     | `crm show <handle>`            |
    /// | `{ cmd: "open", handle }`  *(the CLI's)*        | `crm show <handle>`            |
    /// | `{ cmd: "board", stage? }`                      | `crm board [--stage <stage>]`  |
    /// | `{ cmd: "stages" }`                              | `crm stages`                   |
    /// | `{ cmd: "due" }`                                 | `crm due`                      |
    /// | `{ cmd: "export", kind }`                        | `crm export <kind>`            |
    /// | `{ cmd: "move", id\|handle, to }`                | `crm move <handle> <to>`       |
    /// | `{ cmd: "select", n }`                           | `crm select <n>`               |
    /// | `{ cmd: "find", query?, sort?, page?, kind? }`   | `crm find …`                   |
    /// | `{ cmd: "add", kind, name, fields? }`            | `crm add <kind> <name> …`      |
    /// | `{ cmd: "set", id\|handle, field, value }`       | `crm set <handle> <f> <v>`     |
    /// | `{ cmd: "log", kind, id\|handle, body, at? }`    | `crm log <kind> <handle> …`    |
    /// | `{ cmd: "task", id\|handle, what, due }`         | `crm task <handle> <what> …`   |
    /// | `{ cmd: "done", id\|handle }`                    | `crm done <task-handle>`       |
    /// | `{ cmd: "link", id\|handle, to\|toHandle }`      | `crm link <handle> <handle>`   |
    /// | `{ cmd: "archive", id\|handle, restore? }`       | `crm archive <handle>`         |
    /// | `{ cmd: "import", kind, rows }` *(the CLI's)*    | `crm import <path>`            |
    ///
    /// `page` is **0-based** on the wire and in state; `n` is **1-based**, because it is the
    /// number printed beside a candidate.
    ///
    /// The snapshot is taken **after** the command, and the response is that same
    /// snapshot, so the two carry one `rev`.
    ///
    /// **Only a human write signals** (`caller.is_none()`, exactly the test [`Actor`]
    /// attribution already uses). Every handler below computes the emit its action would
    /// produce regardless of who asked — that computation is pure and harmless — and this
    /// one gate at the bottom decides whether it actually leaves the core. An agent's CLI
    /// write is never told about its own write; a person's is.
    ///
    /// The window verbs (`focus`, `close`, `ping`) never arrive here: clappkit's
    /// `window_cmd` answers those itself, because they are the app process rather than its
    /// state.
    pub fn command(&mut self, req: &Value, caller: Option<&str>, ctx: &Ctx) -> Outcome {
        let result = self.dispatch(req, caller, ctx);

        let snapshot = self.snapshot(ctx.now);
        let (resp, dirty, emits) = match result {
            Ok((answer, emits)) => {
                let mut resp = snapshot.clone();
                let (word, dirty, extra) = match answer {
                    Answer::Read => ("read", false, None),
                    Answer::ReadWith(key, value) => ("read", false, Some((key, value))),
                    Answer::Changed => ("changed", true, None),
                    Answer::ChangedWith(key, value) => ("changed", true, Some((key, value))),
                    Answer::Ambiguous => ("ambiguous", true, None),
                };
                // For the caller only. The window ignores it; the CLI reads it to tell
                // "here is the record" from "here is a question".
                resp["answer"] = json!(word);
                if let Some((key, value)) = extra {
                    resp[key] = value;
                }
                // Only human actions signal. An agent's own write is never told about
                // itself — see the doc comment above.
                let emits = if caller.is_none() { emits } else { Vec::new() };
                (resp, dirty, emits)
            }
            Err(error) => (json!({ "ok": false, "error": error }), false, Vec::new()),
        };

        Outcome { resp, snapshot, emits, dirty }
    }

    /// The verb table `command` builds a response from. Split out so [`cmd_select`] can
    /// call back into it — resuming a deferred write is *re-dispatching the write's own
    /// verb*, now that the id it was missing is filled in, and this is the one place that
    /// knows how to turn a verb name into the handler that answers it.
    fn dispatch(&mut self, req: &Value, caller: Option<&str>, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let cmd = req.get("cmd").and_then(Value::as_str).unwrap_or("");
        match cmd {
            // `status` is the agent's; `state` is the window asking for its first paint.
            // One answer, because there is one state.
            "status" | "state" => Ok((Answer::Read, Vec::new())),
            "show" => self.cmd_show(req),
            "open" => self.cmd_open(req),
            "board" => self.cmd_board(req),
            "stages" => Ok((Answer::Read, Vec::new())),
            "due" => Ok((self.cmd_due(ctx), Vec::new())),
            "export" => self.cmd_export(req).map(|a| (a, Vec::new())),
            "move" => self.cmd_move(req, caller, ctx),
            "select" => self.cmd_select(req, caller, ctx),
            "find" => self.cmd_find(req).map(|a| (a, Vec::new())),
            "add" => self.cmd_add(req, caller, ctx),
            "set" => self.cmd_set(req, ctx),
            "log" => self.cmd_log(req, caller, ctx),
            "task" => self.cmd_task(req, caller, ctx),
            "done" => self.cmd_done(req, ctx),
            "link" => self.cmd_link(req, ctx),
            "archive" => self.cmd_archive(req, ctx),
            "import" => self.cmd_import(req, ctx).map(|a| (a, Vec::new())),
            other => Err(unknown(other)),
        }
    }

    /// The window's `show`: it holds the id already.
    fn cmd_show(&mut self, req: &Value) -> Result<(Answer, Vec<Emit>), String> {
        let id = req.get("id").and_then(Value::as_str).ok_or("show needs the record to open")?;
        let actual = self.kind_of(id).ok_or_else(gone)?;
        if let Some(want) = req.get("kind").and_then(Value::as_str) {
            if Kind::parse(want) != Some(actual) {
                return Err(format!("that record is a {}, not a {want}", actual.word()));
            }
        }
        self.show(id)?;
        let emit = self.deal_opened(id);
        Ok((Answer::Changed, emit.into_iter().collect()))
    }

    /// The CLI's `show`: it has what somebody typed.
    fn cmd_open(&mut self, req: &Value) -> Result<(Answer, Vec<Emit>), String> {
        let typed = req.get("handle").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if typed.is_empty() {
            return Err("show needs a record — `crm show <handle>`".to_string());
        }
        // Archived records are still loadable: that is the difference from deleting.
        match self.resolve(&typed, true) {
            Resolved::One(_, id) => {
                self.show(&id)?;
                let emit = self.deal_opened(&id);
                Ok((Answer::Changed, emit.into_iter().collect()))
            }
            Resolved::Ambiguous(candidates) => {
                self.park(&format!("which “{typed}”?"), candidates);
                Ok((Answer::Ambiguous, Vec::new()))
            }
            Resolved::None => Err(self.no_match_message(&typed)),
        }
    }

    /// `crm board [--stage <stage>]`. `stage` omitted reads the board as it currently is
    /// — including a filter the window set — exactly like `find`'s omitted fields; given,
    /// it sets the **shared** filter, because a board only one surface has narrowed is the
    /// drift `docs/architecture.md` §6 exists to prevent.
    fn cmd_board(&mut self, req: &Value) -> Result<(Answer, Vec<Emit>), String> {
        if let Some(word) = req.get("stage").and_then(Value::as_str) {
            let stage = Stage::parse(word).ok_or_else(|| {
                let all: Vec<&str> = Stage::ALL.iter().map(|s| s.word()).collect();
                format!("“{word}” is not a stage — use one of {}", all.join(", "))
            })?;
            self.set_stage_filter(Some(stage));
            return Ok((Answer::Changed, Vec::new()));
        }
        Ok((Answer::Read, Vec::new()))
    }

    /// `crm due`: the open next steps behind the three counts every snapshot carries, each
    /// with its handle — the only thing `crm done` accepts — and the records it is on, so
    /// an agent that reads "1 overdue" can act on it without a second lookup.
    ///
    /// Answers *beside* the snapshot rather than inside it: a list of tasks is this one
    /// request's answer, not shared state, and every push of every snapshot should not
    /// carry it.
    fn cmd_due(&self, ctx: &Ctx) -> Answer {
        let rows: Vec<Value> = self
            .due_tasks(ctx.now)
            .into_iter()
            .map(|(bucket, t)| {
                let on: Vec<&str> = t.links.iter().filter_map(|id| self.db.handle_of(id)).collect();
                json!({
                    "bucket": bucket.word(),
                    "handle": t.handle,
                    "what": t.what,
                    "due": t.due.to_string_iso(),
                    "on": on,
                })
            })
            .collect();
        Answer::ReadWith("dueTasks", json!(rows))
    }

    /// `crm export <kind> [--format …] [--out …]`. Read-only against the shared state —
    /// the format and the file are the CLI's own business, done after this answers, in
    /// the agent's own working directory rather than the app's.
    fn cmd_export(&mut self, req: &Value) -> Result<Answer, String> {
        let word = req.get("kind").and_then(Value::as_str).unwrap_or("");
        let kind = Kind::parse(word)
            .ok_or_else(|| format!("“{word}” is not a kind — use company, contact or deal"))?;
        let include_archived = req.get("archived").and_then(Value::as_bool).unwrap_or(false);
        let rows: Vec<Vec<[String; 2]>> = self
            .export_rows(kind, include_archived)
            .into_iter()
            .map(|row| row.into_iter().map(|(k, v)| [k.to_string(), v]).collect())
            .collect();
        Ok(Answer::ReadWith("export", json!(rows)))
    }

    fn cmd_move(&mut self, req: &Value, caller: Option<&str>, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let to = req.get("to").and_then(Value::as_str).ok_or_else(|| {
            format!("move needs somewhere to go — one of {}", MoveTarget::vocabulary().join(", "))
        })?;
        let target = MoveTarget::parse(to).ok_or_else(|| {
            format!("“{to}” is not a stage — use one of {}", MoveTarget::vocabulary().join(", "))
        })?;
        let Some(id) = self.resolve_write_target("move", req, "id", "handle", "move")? else {
            return Ok((Answer::Ambiguous, Vec::new()));
        };
        match self.kind_of(&id) {
            Some(Kind::Deal) => {}
            Some(other) => return Err(format!("only a deal can be moved, and that is a {}", other.word())),
            None => return Err(gone()),
        }
        let handle = self.db.handle_of(&id).unwrap_or_default().to_string();
        self.move_deal(&id, target, caller, ctx)?;
        let emit = Emit { id: "stage.changed".into(), target: Vec::new(), payload: json!({ "handle": handle, "to": to }) };
        Ok((Answer::Changed, vec![emit]))
    }

    /// `crm select <n>`.
    ///
    /// **Answering a parked question completes the write it interrupted.** `crm log note
    /// acme "…"` hitting two companies is not asking "which Acme did you mean, so I can
    /// open it" — it is asking "which Acme, so I can finish logging the note." The
    /// deferred write [`resolve_write_target`](Self::resolve_write_target) stashed on the
    /// pending is re-dispatched here, with the id `select` just resolved filled in, so the
    /// note is logged (or the move made, or the field set…) rather than silently dropped.
    /// A plain `show`/`open` ambiguity carries no such write — opening the chosen record
    /// *is* the whole of what it asked for — so that case is unchanged.
    fn cmd_select(&mut self, req: &Value, caller: Option<&str>, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let n = req
            .get("n")
            .and_then(Value::as_u64)
            .ok_or("select needs the number printed beside a result — `crm select 2`")?;
        // Captured before `select()` clears `pending` as part of resolving it.
        let resume = self.db.view.pending.as_ref().and_then(|p| p.resume.clone());

        let id = self.select(n as usize)?;

        match resume {
            Some(r) => {
                // What the response will say was completed. The handle is the *resolved*
                // record's — the one typed was, by definition, ambiguous — and no id
                // appears in it: this reaches whoever is reading.
                let handle = self.db.handle_of(&id).unwrap_or_default().to_string();
                let handle_key = if r.id_key == "toId" { "toHandle" } else { "handle" };
                let mut described = r.req.clone();
                if let Some(obj) = described.as_object_mut() {
                    obj.remove("id");
                    obj.remove("toId");
                    obj.insert(handle_key.to_string(), json!(handle));
                }
                let cmd = r.cmd.clone();

                let mut resumed = r.req;
                resumed[r.id_key] = json!(id);
                match self.dispatch(&resumed, caller, ctx)? {
                    // Completed. `answer` is "changed" for this *and* for a plain select,
                    // so it cannot be what tells them apart — this can.
                    (Answer::Changed, emits) => Ok((
                        Answer::ChangedWith("resumed", json!({ "cmd": cmd, "handle": handle, "req": described })),
                        emits,
                    )),
                    // Still ambiguous (a second slot), or something else: say exactly that.
                    other => Ok(other),
                }
            }
            None => {
                let emit = self.deal_opened(&id);
                Ok((Answer::Changed, emit.into_iter().collect()))
            }
        }
    }

    /// Query, sort, page and kind ride one envelope, and **an omitted field keeps its
    /// current value** — which is what lets the window turn a page without restating the
    /// search. `kind: null` is not omitted: it is "all three".
    fn cmd_find(&mut self, req: &Value) -> Result<Answer, String> {
        let kind = match req.get("kind") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(v) => {
                let word = v.as_str().unwrap_or("");
                let parsed = Kind::parse(word).ok_or_else(|| {
                    format!("“{word}” is not a kind — use company, contact or deal")
                })?;
                Some(Some(parsed))
            }
        };
        let query = match req.get("query") {
            None => None,
            Some(v) => Some(v.as_str().ok_or("query must be text")?.to_string()),
        };
        let sort = match req.get("sort") {
            None => None,
            Some(v) => {
                let word = v.as_str().unwrap_or("");
                Some(Sort::parse(word).ok_or_else(|| {
                    let all: Vec<&str> = Sort::ALL.iter().map(|s| s.word()).collect();
                    format!("“{word}” is not a sort — use one of {}", all.join(", "))
                })?)
            }
        };
        let page = match req.get("page") {
            None => None,
            Some(v) => Some(v.as_u64().ok_or("page must be a whole number, counted from 0")? as usize),
        };
        let include_archived = req.get("archived").and_then(Value::as_bool).unwrap_or(false);

        // Validated in full before anything changes, so a bad field leaves the list as it
        // was rather than half-applied.
        let narrowed = kind.is_some() || query.is_some();
        let previous_page = self.db.view.list.page;
        if let Some(kind) = kind {
            self.db.view.list.kind = kind;
        }
        if let Some(query) = query {
            self.db.view.list.query = query;
        }

        // Always re-run: records may have been added since the list was last built, and a
        // page of stale ids is a page that disagrees with the board. `--archived` is not
        // sticky (m2-cli.md): it shapes only this one search.
        let query = self.db.view.list.query.clone();
        self.find(&query, include_archived);
        if !narrowed {
            self.set_page(previous_page);
        }
        if let Some(sort) = sort {
            self.set_sort(sort);
        }
        if let Some(page) = page {
            self.set_page(page);
        }
        Ok(Answer::Changed)
    }

    /// `crm add <kind> <name> [flags]`. The window's envelope shape and the CLI's
    /// flag-built one are identical — `fields.company` accepts an id (the window) or a
    /// handle (the CLI), resolved the same way either way — so this needs no id/handle
    /// split of its own the way a reference to an *existing* record does.
    fn cmd_add(&mut self, req: &Value, caller: Option<&str>, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let kind = req.get("kind").and_then(Value::as_str).and_then(Kind::parse).ok_or_else(|| {
            "add needs a kind — company, contact or deal".to_string()
        })?;
        let name = req
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("add needs a name — `crm add {} <name>`", kind.word()))?;
        let empty = Value::Null;
        let fields = req.get("fields").unwrap_or(&empty);
        let field_str = |k: &str| fields.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
        let company_ref = match field_str("company") {
            Some(c) => {
                let (found, id) = self.resolve_decisive(c)?;
                if found != Kind::Company {
                    return Err(format!("“{c}” is a {}, not a company", found.word()));
                }
                Some(id)
            }
            None => None,
        };

        let id = match kind {
            Kind::Company => {
                let id = self.add_company(name, ctx);
                if let Some(domain) = field_str("domain") {
                    self.db.companies.iter_mut().find(|c| c.id == id).unwrap().domain = Some(domain.to_string());
                }
                if let Some(tags) = fields.get("tags").and_then(Value::as_array) {
                    let tags: Vec<String> = tags.iter().filter_map(|t| t.as_str().map(str::to_string)).collect();
                    self.db.companies.iter_mut().find(|c| c.id == id).unwrap().tags = tags;
                }
                id
            }
            Kind::Contact => {
                let id = self.add_contact(name, company_ref.as_deref(), ctx);
                let c = self.db.contacts.iter_mut().find(|c| c.id == id).unwrap();
                if let Some(v) = field_str("email") {
                    c.email = Some(v.to_string());
                }
                if let Some(v) = field_str("phone") {
                    c.phone = Some(v.to_string());
                }
                if let Some(v) = field_str("title") {
                    c.title = Some(v.to_string());
                }
                id
            }
            Kind::Deal => {
                let value = match field_str("value") {
                    Some(v) => {
                        let currency = field_str("currency").unwrap_or("USD");
                        Some(parse_money_field(&format!("{v} {currency}"), None)?)
                    }
                    None => None,
                };
                let id = self.add_deal(name, company_ref.as_deref(), value, caller, ctx);
                if let Some(word) = field_str("stage") {
                    let stage = Stage::parse(word).ok_or_else(|| {
                        let all: Vec<&str> = Stage::ALL.iter().map(|s| s.word()).collect();
                        format!("“{word}” is not a stage — use one of {}", all.join(", "))
                    })?;
                    self.move_deal(&id, MoveTarget::To(stage), caller, ctx)?;
                }
                id
            }
        };
        let handle = self.db.handle_of(&id).unwrap_or_default().to_string();
        let emit = Emit {
            id: "record.changed".into(),
            target: Vec::new(),
            payload: json!({ "kind": kind.word(), "handle": handle }),
        };
        Ok((Answer::Changed, vec![emit]))
    }

    /// `crm set <handle> <field> <value>`.
    fn cmd_set(&mut self, req: &Value, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let Some(id) = self.resolve_write_target("set", req, "id", "handle", "set")? else {
            return Ok((Answer::Ambiguous, Vec::new()));
        };
        let field = req.get("field").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if field.is_empty() {
            return Err("set needs a field name — `crm set <handle> <field> <value>`".to_string());
        }
        let value = req.get("value").and_then(Value::as_str).unwrap_or("");
        let kind = self.kind_of(&id).ok_or_else(gone)?;
        self.set_field(&id, &field, value, ctx)?;
        let handle = self.db.handle_of(&id).unwrap_or_default().to_string();
        let emit = Emit {
            id: "record.changed".into(),
            target: Vec::new(),
            payload: json!({ "kind": kind.word(), "handle": handle }),
        };
        Ok((Answer::Changed, vec![emit]))
    }

    /// `crm log <call|email|meeting|note> <handle> <body> [--at <date>]`.
    fn cmd_log(&mut self, req: &Value, caller: Option<&str>, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let kind_word = req.get("kind").and_then(Value::as_str).unwrap_or("");
        let activity_kind = ActivityKind::parse(kind_word).ok_or_else(|| {
            let all: Vec<&str> = ActivityKind::ALL.iter().map(|k| k.word()).collect();
            format!("“{kind_word}” is not a kind of activity — use one of {}", all.join(", "))
        })?;
        let body = req
            .get("body")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("log needs something to say")?
            .to_string();
        let Some(id) = self.resolve_write_target("log", req, "id", "handle", "log")? else {
            return Ok((Answer::Ambiguous, Vec::new()));
        };
        // `--at` backdates the entry onto a different day; the core places it on the
        // calendar itself via `Ctx::offset_secs` rather than trusting a raw timestamp.
        let logged_ctx;
        let ctx = match req.get("at").and_then(Value::as_str) {
            Some(date_str) => {
                let date = Date::parse(date_str)
                    .ok_or_else(|| format!("“{date_str}” is not a date — use YYYY-MM-DD"))?;
                let mut c = ctx.clone();
                c.now.at = date.to_timestamp_ms(ctx.offset_secs);
                logged_ctx = c;
                &logged_ctx
            }
            None => ctx,
        };
        let handle = self.db.handle_of(&id).unwrap_or_default().to_string();
        self.log(activity_kind, &body, vec![id], caller, ctx);
        let emit = Emit {
            id: "note.added".into(),
            target: Vec::new(),
            payload: json!({ "kind": activity_kind.word(), "handle": handle }),
        };
        Ok((Answer::Changed, vec![emit]))
    }

    /// `crm task <handle> <what> --due <date>`.
    fn cmd_task(&mut self, req: &Value, caller: Option<&str>, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let what = req
            .get("what")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("task needs something to do")?
            .to_string();
        let due_str = req.get("due").and_then(Value::as_str).ok_or("task needs `--due <date>`")?;
        let due = Date::parse(due_str).ok_or_else(|| format!("“{due_str}” is not a date — use YYYY-MM-DD"))?;
        let Some(id) = self.resolve_write_target("task", req, "id", "handle", "task")? else {
            return Ok((Answer::Ambiguous, Vec::new()));
        };
        self.add_task(&what, due, vec![id], caller, ctx);
        let emit = Emit {
            id: "record.changed".into(),
            target: Vec::new(),
            payload: json!({ "kind": "task", "what": what }),
        };
        Ok((Answer::Changed, vec![emit]))
    }

    /// `crm done <task-handle>`. Its target is a **task**, in its own handle namespace —
    /// not a company, contact or deal, so it is resolved separately from every other verb
    /// here.
    fn cmd_done(&mut self, req: &Value, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let id = self.resolve_task_target(req, "id", "handle")?;
        let what = self.db.tasks.iter().find(|t| t.id == id).map(|t| t.what.clone()).unwrap_or_default();
        self.complete_task(&id, ctx)?;
        let emit = Emit {
            id: "record.changed".into(),
            target: Vec::new(),
            payload: json!({ "kind": "task", "what": what }),
        };
        Ok((Answer::Changed, vec![emit]))
    }

    /// `crm link <handle> <handle>`. The grammar does not say which of the two is the
    /// deal, so this decides: whichever of the two resolved records is a deal takes that
    /// role, and the other is what it links to.
    fn cmd_link(&mut self, req: &Value, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let Some(a) = self.resolve_write_target("link", req, "id", "handle", "link")? else {
            return Ok((Answer::Ambiguous, Vec::new()));
        };
        let Some(b) = self.resolve_write_target("link", req, "toId", "toHandle", "link")? else {
            return Ok((Answer::Ambiguous, Vec::new()));
        };
        let (deal_id, other_id) = match (self.kind_of(&a), self.kind_of(&b)) {
            (Some(Kind::Deal), Some(Kind::Deal)) => return Err("link needs one deal, not two".to_string()),
            (Some(Kind::Deal), Some(_)) => (a, b),
            (Some(_), Some(Kind::Deal)) => (b, a),
            _ => return Err("link needs a deal and a contact or a company".to_string()),
        };
        self.link(&deal_id, &other_id, ctx)?;
        let handle = self.db.handle_of(&deal_id).unwrap_or_default().to_string();
        let emit = Emit {
            id: "record.changed".into(),
            target: Vec::new(),
            payload: json!({ "kind": "deal", "handle": handle }),
        };
        Ok((Answer::Changed, vec![emit]))
    }

    /// `crm archive <handle> [--restore]`.
    fn cmd_archive(&mut self, req: &Value, ctx: &Ctx) -> Result<(Answer, Vec<Emit>), String> {
        let restore = req.get("restore").and_then(Value::as_bool).unwrap_or(false);
        let Some(id) = self.resolve_write_target("archive", req, "id", "handle", "archive")? else {
            return Ok((Answer::Ambiguous, Vec::new()));
        };
        let kind = self.kind_of(&id).ok_or_else(gone)?;
        if restore {
            self.restore(&id, ctx)?;
        } else {
            self.archive(&id, ctx)?;
        }
        let handle = self.db.handle_of(&id).unwrap_or_default().to_string();
        let emit = Emit {
            id: "record.changed".into(),
            target: Vec::new(),
            payload: json!({ "kind": kind.word(), "handle": handle, "archived": !restore }),
        };
        Ok((Answer::Changed, vec![emit]))
    }

    /// `crm import <path> [--kind …]`. CLI-only: the file lives in the agent's working
    /// directory, so reading it is the CLI's job — this only ever sees rows already
    /// parsed, and creates them through the SAME per-kind method `add` uses, so an
    /// imported record is validated exactly like one typed in by hand.
    fn cmd_import(&mut self, req: &Value, ctx: &Ctx) -> Result<Answer, String> {
        let kind = req.get("kind").and_then(Value::as_str).and_then(Kind::parse).ok_or_else(|| {
            "import needs a kind — company, contact or deal".to_string()
        })?;
        let rows = req.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut created = 0usize;
        let mut skipped: Vec<Value> = Vec::new();
        for row in &rows {
            let get = |k: &str| row.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
            let Some(name) = get(if kind == Kind::Deal { "title" } else { "name" }) else {
                skipped.push(json!({ "row": row, "reason": "no name" }));
                continue;
            };
            let company = get("company").map(|c| self.resolve_decisive(c));
            let company_id = match company {
                Some(Ok((Kind::Company, id))) => Some(id),
                Some(Ok((other, _))) => {
                    skipped.push(json!({ "row": row, "reason": format!("company \"{}\" is a {}", get("company").unwrap_or(""), other.word()) }));
                    continue;
                }
                Some(Err(e)) => {
                    skipped.push(json!({ "row": row, "reason": e }));
                    continue;
                }
                None => None,
            };
            match kind {
                Kind::Company => {
                    let id = self.add_company(name, ctx);
                    if let Some(domain) = get("domain") {
                        self.db.companies.iter_mut().find(|c| c.id == id).unwrap().domain = Some(domain.to_string());
                    }
                }
                Kind::Contact => {
                    let id = self.add_contact(name, company_id.as_deref(), ctx);
                    let c = self.db.contacts.iter_mut().find(|c| c.id == id).unwrap();
                    if let Some(v) = get("email") {
                        c.email = Some(v.to_string());
                    }
                    if let Some(v) = get("phone") {
                        c.phone = Some(v.to_string());
                    }
                    if let Some(v) = get("title") {
                        c.title = Some(v.to_string());
                    }
                }
                Kind::Deal => {
                    let value = match (get("value"), get("currency")) {
                        (Some(v), currency) => match parse_money_field(&format!("{v} {}", currency.unwrap_or("USD")), None) {
                            Ok(m) => Some(m),
                            Err(e) => {
                                skipped.push(json!({ "row": row, "reason": e }));
                                continue;
                            }
                        },
                        (None, _) => None,
                    };
                    self.add_deal(name, company_id.as_deref(), value, None, ctx);
                }
            }
            created += 1;
        }
        Ok(Answer::ReadWith("import", json!({ "created": created, "skipped": skipped })))
    }

    /// Resolve a write's **target record**: an `id` the caller already has (the window),
    /// or a `handle` it typed (the CLI) — resolved here, at the edge, with the same
    /// ambiguity handling [`AppState::cmd_open`] uses.
    ///
    /// `Ok(Some(id))` is a decisive match. `Ok(None)` means a question was just parked —
    /// the caller must return `Answer::Ambiguous` and do nothing else. `Err` is a refusal:
    /// neither field was given, or nothing matched.
    ///
    /// `cmd` is this verb's own name on the wire (`"move"`, `"log"`, …) — when the lookup
    /// is ambiguous, it is what lets the parked question remember which write to resume,
    /// via [`park_with_resume`](Self::park_with_resume): `req` verbatim, plus `key_id` so
    /// the resolved id lands back in the same field a decisive call would have read it
    /// from.
    fn resolve_write_target(
        &mut self,
        cmd: &str,
        req: &Value,
        key_id: &str,
        key_handle: &str,
        what: &str,
    ) -> Result<Option<Id>, String> {
        if let Some(id) = req.get(key_id).and_then(Value::as_str) {
            return self.kind_of(id).map(|_| Some(id.to_string())).ok_or_else(gone);
        }
        let Some(typed) = req.get(key_handle).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
        else {
            return Err(format!("{what} needs a record — give its handle"));
        };
        let typed = typed.to_string();
        match self.resolve(&typed, true) {
            Resolved::One(_, id) => Ok(Some(id)),
            Resolved::Ambiguous(candidates) => {
                let resume =
                    PendingResume { cmd: cmd.to_string(), req: req.clone(), id_key: key_id.to_string() };
                self.park_with_resume(&format!("which “{typed}”?"), candidates, Some(resume));
                Ok(None)
            }
            Resolved::None => Err(self.no_match_message(&typed)),
        }
    }

    /// [`resolve_write_target`](Self::resolve_write_target) for a **task**: tasks share
    /// the one handle namespace but are not records `show`/`resolve` open, and a task
    /// handle is already unique — there is no scored, ambiguous match to park.
    fn resolve_task_target(&self, req: &Value, key_id: &str, key_handle: &str) -> Result<Id, String> {
        if let Some(id) = req.get(key_id).and_then(Value::as_str) {
            return self
                .db
                .tasks
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.id.clone())
                .ok_or_else(|| "that task no longer exists — reopen its record".to_string());
        }
        let typed = req.get(key_handle).and_then(Value::as_str).unwrap_or("").trim();
        if typed.is_empty() {
            return Err("done needs a task — give its handle".to_string());
        }
        if let Some(t) = self.db.task_by_handle(typed) {
            return Ok(t.id.clone());
        }
        // The commonest slip: the handle of the *record* the task is on. Say so, and say
        // where that record's tasks are actually listed — `crm show` does list them.
        if let Some((kind, _)) = self.db.by_handle(typed) {
            return Err(format!(
                "“{typed}” is a {}, not a next step — `crm show {typed}` lists its open next steps with their handles",
                kind.word()
            ));
        }
        Err(format!(
            "no next step matches “{typed}” — `crm due` and `crm show <record>` list open ones with their handles"
        ))
    }

    /// The `deal.opened` signal for a record a human just brought into focus — `select`
    /// and, indirectly, `show`/`open`'s callers build it the same way. `None` when the
    /// focused record's handle cannot be found, which should not happen but must not
    /// panic a whole command over a signal.
    fn deal_opened(&self, id: &str) -> Option<Emit> {
        let kind = self.kind_of(id)?;
        let handle = self.db.handle_of(id)?.to_string();
        Some(Emit {
            id: "deal.opened".into(),
            target: Vec::new(),
            payload: json!({ "kind": kind.word(), "handle": handle }),
        })
    }
}

/// A record that was named by id and is not there. Deliberately says nothing about the
/// id: this reaches whoever is reading, and an id is not something they can use.
fn gone() -> String {
    "that record no longer exists — reopen it from the list".to_string()
}

/// Trimmed, or `None` — so `crm set acme domain ""` clears an optional field rather than
/// setting it to an empty string nobody meant to store.
fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// A comma-separated list, trimmed and with empty entries dropped — `set`'s single string
/// argument standing in for the array `add`'s `fields.tags` carries, since a CLI verb has
/// one value per field and no second syntax for a list.
fn parse_tag_list(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The refusal for a field name `set` does not have on this kind of record. Exit **1**,
/// not 2: the field vocabulary is the core's, exactly like a stage word, and the CLI has
/// no list of its own to check it against first.
fn unknown_field(kind: Kind, field: &str, allowed: &[&str]) -> String {
    format!("“{field}” is not a field on a {} — use one of {}", kind.word(), allowed.join(", "))
}

/// `"45000"` or `"45000 EUR"` into a [`Money`]. A currency in the input wins; otherwise
/// the deal's existing currency carries over, and only a deal with no value yet falls
/// back to USD — so setting just the amount never silently converts what is already there.
fn parse_money_field(raw: &str, existing_currency: Option<&str>) -> Result<Money, String> {
    let mut parts = raw.split_whitespace();
    let amount_str = parts.next().unwrap_or("");
    let currency = match parts.next() {
        Some(c) => {
            if c.len() != 3 || !c.chars().all(|ch| ch.is_ascii_alphabetic()) {
                return Err(format!("“{c}” is not a 3-letter currency code"));
            }
            c.to_ascii_uppercase()
        }
        None => existing_currency.map(str::to_string).unwrap_or_else(|| "USD".to_string()),
    };
    if parts.next().is_some() {
        return Err(format!("“{raw}” is not `<amount>` or `<amount> <currency>`"));
    }
    let amount = Money::parse_decimal(amount_str, &currency)
        .ok_or_else(|| format!("“{amount_str}” is not a decimal amount — try 45000 or 45000.50"))?;
    Ok(Money::new(amount, &currency))
}

/// The refusal for a verb this build does not have. It names the manual rather than listing
/// the verbs, because `crm -h` is the manual and a second list would be a second thing to
/// keep true.
fn unknown(cmd: &str) -> String {
    let what = if cmd.is_empty() { "a command with no verb".to_string() } else { format!("`{cmd}`") };
    format!("{what} is not a verb this build answers — see `crm -h`")
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
