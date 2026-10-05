//! **p1: one snapshot becomes two.** The new shape, published beside today's.
//!
//! View state is **per seat** — `focus`, `list`, `board`, `pending`: one person and their
//! agents. Record data is **per org** — companies, contacts, deals, tasks and the logs —
//! delivered as a bootstrap and then as per-row actions in the order they were committed.
//! `docs/work-orders/p1-the-split.md` is the order; `docs/split-contract.md` is the shape.
//!
//! **Two counters, two jobs — never merged.**
//!
//! * `rev` is process-local. It orders what one window *draws*: the client keeps the highest
//!   it has seen and drops anything older (`clappkit::snapshot`). It orders **view** updates.
//! * `watermark` is the data's. Every row action takes the next number, it persists with the
//!   data it numbers ([`Db::watermark`]), and a client holds the highest it has seen and asks
//!   for everything after. It orders **record data**.
//!
//! Everything here is a pure function of the state, like the classic snapshot: no I/O, no
//! clock of its own, nothing it does can change what either surface is looking at. That is
//! the point of [`AppState::search_json`] (gap 4): a query that serves one surface's own
//! convenience is expressible without touching shared state.
//!
//! The two shapes are built from the **same** projections (`row`, `board`, `cards`,
//! `focused_json`, `counts`, …). Where they overlap they cannot disagree, and a test says so
//! over both golden workspaces.

use super::*;
use std::collections::{HashMap, VecDeque};

/// How many row actions the core remembers for a client that is a little behind. Older than
/// this and the client is told to bootstrap again — the watermark persists, the log does not.
pub const LOG_CAP: usize = 5_000;

/// The most a bootstrap's feed head, a feed page or a task list carries unless asked for less.
const FEED_PAGE: usize = 25;
const FEED_MAX: usize = 100;
const TASKS_CAP: usize = 200;
const SEARCH_DEFAULT: usize = 8;
const SEARCH_MAX: usize = 50;

// MARK: - The delta

/// One row action. Today the only action is `upsert` — there is no hard delete in this app
/// (archiving is a field), and an activity or a move is appended and never edited — but the
/// word is on the wire so the day there is a `delete` no client has to change shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    /// The server watermark this action took. Strictly increasing, no gaps.
    pub seq: u64,
    /// `company`, `contact`, `deal`, `task`, `activity` or `move`.
    pub kind: &'static str,
    pub id: Id,
    /// The row as it now is. For `activity` and `move` it is the **feed item**.
    pub record: Value,
}

impl Op {
    pub fn json(&self) -> Value {
        json!({ "seq": self.seq, "op": "upsert", "kind": self.kind, "id": self.id, "record": self.record })
    }
}

/// The recent row actions, so a client holding a watermark can ask for what came after it.
#[derive(Debug, Default)]
pub struct DeltaLog {
    ops: VecDeque<Op>,
}

impl DeltaLog {
    pub fn push(&mut self, ops: Vec<Op>) {
        self.ops.extend(ops);
        while self.ops.len() > LOG_CAP {
            self.ops.pop_front();
        }
    }

    /// Everything after `since`, if the log still reaches back that far; `None` means the
    /// client must bootstrap again — it is too far behind, or ahead of a data set that was
    /// replaced underneath it.
    pub fn since(&self, now: u64, since: u64) -> Option<Vec<Op>> {
        if since == now {
            return Some(Vec::new());
        }
        if since > now {
            return None;
        }
        match self.ops.front() {
            Some(first) if first.seq <= since + 1 => {
                Some(self.ops.iter().filter(|op| op.seq > since).cloned().collect())
            }
            _ => None,
        }
    }
}

// MARK: - Rows as records

fn actor_json(a: &Actor) -> Value {
    json!(a)
}

fn company_json(c: &Company) -> Value {
    json!({
        "id": c.id, "handle": c.handle, "orgId": c.org_id, "ownerId": c.owner_id,
        "name": c.name, "domain": c.domain, "tags": c.tags, "notes": c.notes,
        "archivedAt": c.archived_at, "updatedAt": c.updated_at,
    })
}

fn contact_json(c: &Contact) -> Value {
    json!({
        "id": c.id, "handle": c.handle, "orgId": c.org_id, "ownerId": c.owner_id,
        "name": c.name, "email": c.email, "phone": c.phone, "title": c.title,
        "companyId": c.company_id, "tags": c.tags,
        "archivedAt": c.archived_at, "updatedAt": c.updated_at,
    })
}

fn deal_json(d: &Deal) -> Value {
    json!({
        "id": d.id, "handle": d.handle, "orgId": d.org_id, "ownerId": d.owner_id,
        "title": d.title, "companyId": d.company_id, "contactIds": d.contact_ids,
        "value": d.value.as_ref().map(money_json),
        "stage": d.stage.word(), "status": d.status.word(), "pipelineId": d.pipeline_id,
        "openedAt": d.opened_at, "closedAt": d.closed_at,
        "movedBy": actor_json(&d.moved_by), "movedAt": d.moved_at,
        "archivedAt": d.archived_at, "updatedAt": d.updated_at,
    })
}

/// A next step as stored. `sentAt` is when the timer last **sent** it (never "delivered" —
/// the platform does not say), which is what lets a surface pick one step to send again.
fn task_json(t: &Task) -> Value {
    json!({
        "id": t.id, "handle": t.handle, "orgId": t.org_id, "ownerId": t.owner_id,
        "what": t.what, "due": t.due.to_string_iso(), "links": t.links,
        "doneAt": t.done_at, "by": actor_json(&t.by),
        "sentAt": t.due_sent_at, "updatedAt": t.updated_at,
    })
}

// MARK: - Diffing: what a commit did, as row actions

/// Which rows differ between two copies of the dataset, as unnumbered actions, in a stable
/// order (kind, then creation order). View state is **never** compared: moving where a
/// seat is looking produces no row action, by construction.
pub fn diff(before: &Db, after: &Db) -> Vec<(&'static str, Id, Value)> {
    let mut out = Vec::new();

    macro_rules! rows {
        ($kind:expr, $field:ident, $json:expr) => {{
            let old: HashMap<&str, &_> = before.$field.iter().map(|r| (r.id.as_str(), r)).collect();
            for row in &after.$field {
                if old.get(row.id.as_str()).is_none_or(|prev| *prev != row) {
                    out.push(($kind, row.id.clone(), $json(row)));
                }
            }
        }};
    }
    rows!("company", companies, company_json);
    rows!("contact", contacts, contact_json);
    rows!("deal", deals, deal_json);
    rows!("task", tasks, task_json);

    // Append-only logs: an id that was not there before is the whole story.
    let seen: std::collections::HashSet<&str> = before.activities.iter().map(|a| a.id.as_str()).collect();
    for a in after.activities.iter().filter(|a| !seen.contains(a.id.as_str())) {
        out.push(("activity", a.id.clone(), activity_item(after, a)));
    }
    let seen: std::collections::HashSet<&str> = before.moves.iter().map(|m| m.id.as_str()).collect();
    for m in after.moves.iter().filter(|m| !seen.contains(m.id.as_str())) {
        out.push(("move", m.id.clone(), move_item(after, m)));
    }
    out
}

/// `{kind, id, handle, label}` — what a feed item or a task says it is *about*.
fn record_ref(db: &Db, id: &str) -> Option<Value> {
    if let Some(c) = db.company(id) {
        return Some(json!({ "kind": "company", "id": c.id, "handle": c.handle, "label": c.name }));
    }
    if let Some(c) = db.contact(id) {
        return Some(json!({ "kind": "contact", "id": c.id, "handle": c.handle, "label": c.name }));
    }
    db.deal(id).map(|d| json!({ "kind": "deal", "id": d.id, "handle": d.handle, "label": d.title }))
}

/// The record a line is chiefly about: a deal if it names one, else the first thing it names.
fn primary_ref(db: &Db, links: &[Id]) -> Value {
    let refs: Vec<Value> = links.iter().filter_map(|l| record_ref(db, l)).collect();
    refs.iter()
        .find(|r| r["kind"] == "deal")
        .or_else(|| refs.first())
        .cloned()
        .unwrap_or(Value::Null)
}

/// A timeline line as the global feed carries it.
fn activity_item(db: &Db, a: &Activity) -> Value {
    json!({
        "id": a.id, "kind": a.kind.word(), "body": a.body, "at": a.at, "by": actor_json(&a.by),
        "record": primary_ref(db, &a.links),
        "links": a.links.iter().filter_map(|l| record_ref(db, l)).collect::<Vec<_>>(),
        "orgId": a.org_id,
    })
}

/// A stage change as the global feed carries it: the history a card's `by`/`movedAt` forget.
fn move_item(db: &Db, m: &Move) -> Value {
    json!({
        "id": m.id, "kind": "stage", "body": Value::Null, "at": m.at, "by": actor_json(&m.by),
        "record": record_ref(db, &m.deal_id).unwrap_or(Value::Null),
        "links": record_ref(db, &m.deal_id).into_iter().collect::<Vec<_>>(),
        "from": m.from, "to": m.to, "orgId": m.org_id,
    })
}

// MARK: - The watermark and the log

impl AppState {
    /// The data's position: the number the last row action took. (Read by the tests and by
    /// the next milestone's sync client; the messages carry it themselves.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn watermark(&self) -> u64 {
        self.db.watermark
    }

    /// Number what a commit just did. Called **inside the commit, before the write**, so the
    /// watermark goes to the disk with the rows it numbers: the persisted pair can never
    /// disagree. Returns the numbered actions (empty when only view state or a parked
    /// question changed — those are not row actions, and take no number).
    ///
    /// Not yet visible to [`changes`](Self::changes): [`publish`](Self::publish) does that once
    /// the write has succeeded, so a refused write is rolled back without a trace.
    pub fn stamp(&mut self, before: &Db) -> Vec<Op> {
        let raw = diff(before, &self.db);
        let mut ops = Vec::with_capacity(raw.len());
        for (kind, id, record) in raw {
            self.db.watermark += 1;
            ops.push(Op { seq: self.db.watermark, kind, id, record });
        }
        ops
    }

    /// Make stamped actions available to clients asking for what they missed.
    pub fn publish(&mut self, ops: Vec<Op>) {
        self.log.push(ops);
    }

    /// Every row action after `since`, or `None` when the client must bootstrap again.
    pub fn changes(&self, since: u64) -> Option<Vec<Op>> {
        self.log.since(self.db.watermark, since)
    }
}

// MARK: - Projections shared by both shapes' new facts

impl AppState {
    /// A row of the shared list **with the facts it used to lack** (gaps 6 and 11): the
    /// classic row, plus what the core stores and had never sent, plus the counts a list
    /// wants. The classic keys are untouched — this only adds — so the two shapes cannot
    /// disagree about a row.
    fn row_v2(&self, id: &str) -> Option<Value> {
        let mut row = self.row(id)?;
        let kind = row["kind"].as_str().unwrap_or("").to_string();
        match kind.as_str() {
            "company" => {
                let c = self.db.company(id)?;
                let deals: Vec<&Deal> = self.db.active_deals().filter(|d| d.company_id.as_deref() == Some(id)).collect();
                let open: Vec<&Deal> = deals.iter().copied().filter(|d| d.status.is_open()).collect();
                row["updatedAt"] = json!(c.updated_at);
                row["ownerId"] = json!(c.owner_id);
                row["domain"] = json!(c.domain);
                row["tags"] = json!(c.tags);
                row["dealCount"] = json!(deals.len());
                row["openValue"] = json!(total_by_currency(open.iter().copied()).iter().map(money_json).collect::<Vec<_>>());
                row["contactCount"] =
                    json!(self.db.contacts.iter().filter(|p| p.archived_at.is_none() && p.company_id.as_deref() == Some(id)).count());
            }
            "contact" => {
                let c = self.db.contact(id)?;
                row["updatedAt"] = json!(c.updated_at);
                row["ownerId"] = json!(c.owner_id);
                row["title"] = json!(c.title);
                row["email"] = json!(c.email);
                row["phone"] = json!(c.phone);
                row["companyHandle"] = json!(c.company_id.as_deref().and_then(|cid| self.db.company(cid)).map(|co| co.handle.clone()));
            }
            _ => {
                let d = self.db.deal(id)?;
                row["updatedAt"] = json!(d.updated_at);
                row["ownerId"] = json!(d.owner_id);
                row["openedAt"] = json!(d.opened_at);
                row["closedAt"] = json!(d.closed_at);
                row["movedAt"] = json!(d.moved_at);
                row["by"] = actor_json(&d.moved_by);
                row["companyHandle"] = json!(d.company_id.as_deref().and_then(|cid| self.db.company(cid)).map(|co| co.handle.clone()));
            }
        }
        Some(row)
    }

    /// The deal filters in force on the shared list, or `None` when there are none. Shared
    /// with the classic snapshot, which names them only when set.
    pub(super) fn filters_json(&self) -> Option<Value> {
        let l = &self.db.view.list;
        if !l.has_deal_filters() {
            return None;
        }
        Some(l.filters_value())
    }
}

// MARK: - The new shape

impl AppState {
    /// **Per seat.** Where this person and their agents are looking: the open record, the
    /// shared list and its filters, the board and its filter, and the parked question — plus
    /// the projections that depend on them (the list's rows, the board's cards, the open
    /// record's panel). Nothing here is another seat's, and nothing here is a row action.
    fn view_json(&self) -> Value {
        let view = &self.db.view;
        let columns = self.board_columns();
        let mut out = json!({
            "focus": self.focus_json(),
            "list": {
                "query": view.list.query,
                "sort": view.list.sort.word(),
                "page": view.list.page,
                "pageSize": view.list.page_size,
                "total": view.list.results.len(),
                "kind": view.list.kind.map(|k| k.word()),
                "filters": view.list.filters_value(),
                "rows": view.list.page_ids().iter().filter_map(|id| self.row_v2(id)).collect::<Vec<_>>(),
            },
            "board": self.board(&columns),
            "cards": self.cards(&columns),
            "pending": view.pending.as_ref().map(Pending::snapshot_json),
        });
        if let Some(focused) = self.focused_json() {
            out["focused"] = focused;
        }
        out
    }

    /// **Per seat, but not view:** who is sitting here, which agents are bound to *this*
    /// install, and what *this* app's timer has been doing. (`me` is gap 8.)
    fn seat_json(&self, now: Now) -> Value {
        json!({
            "me": { "id": self.me.id, "name": self.me.name },
            "agents": self.agents,
            "reminders": self.reminders_json(now),
        })
    }

    /// **Per org, derived:** what is true of the data as a whole, formatted by the core.
    /// Money is grouped by currency and never summed across them (gap 3).
    fn derived_json(&self, now: Now) -> Value {
        let (overdue, today, week) = self.due_counts(now);
        let totals = |status: Status| -> Vec<Value> {
            total_by_currency(self.db.active_deals().filter(|d| d.status == status)).iter().map(money_json).collect()
        };
        json!({
            "counts": self.counts(),
            "due": { "overdue": overdue, "today": today, "week": week },
            "pipeline": { "open": totals(Status::Open), "won": totals(Status::Won), "lost": totals(Status::Lost) },
        })
    }

    /// What follows a commit: where this seat is looking, what the data just did, and the
    /// aggregates that moved. **The only thing pushed.** Never the whole state — a client that
    /// needs that asks for a [`bootstrap`](Self::bootstrap), once.
    ///
    /// `ops` are the row actions this commit took (empty for a read or a navigation). The
    /// delta's `from` is the watermark the client must already hold to apply it in order.
    pub fn update(&self, now: Now, ops: &[Op]) -> Value {
        let to = self.db.watermark;
        clappkit::snapshot::with_rev(json!({
            "type": "update",
            "shape": "split",
            "watermark": to,
            "view": self.view_json(),
            "seat": self.seat_json(now),
            "derived": self.derived_json(now),
            "delta": {
                "from": to.saturating_sub(ops.len() as u64),
                "to": to,
                "ops": ops.iter().map(Op::json).collect::<Vec<_>>(),
            },
        }))
    }

    /// The first thing a client asks for: the org's rows as they are now and the watermark
    /// they are at, plus this seat's view. After it, only [`update`](Self::update)s and
    /// [`changes`](Self::changes) — **no code path pushes everything again.**
    ///
    /// `data` holds the **stored** fields of every row and nothing derived (a company's deal
    /// count would be stale the moment one of its deals moved, without the company changing):
    /// derived facts ride the view's rows and `derived`.
    pub fn bootstrap(&self, now: Now) -> Value {
        let pipeline = self.db.pipeline();
        clappkit::snapshot::with_rev(json!({
            "type": "bootstrap",
            "shape": "split",
            "watermark": self.db.watermark,
            "org": { "id": self.db.org.id, "name": self.db.org.name },
            "view": self.view_json(),
            "seat": self.seat_json(now),
            "derived": self.derived_json(now),
            "data": {
                "pipeline": {
                    "id": pipeline.id,
                    "name": pipeline.name,
                    "stages": pipeline.stages.iter().map(|s| s.word()).collect::<Vec<_>>(),
                },
                "companies": self.db.companies.iter().map(company_json).collect::<Vec<_>>(),
                "contacts": self.db.contacts.iter().map(contact_json).collect::<Vec<_>>(),
                "deals": self.db.deals.iter().map(deal_json).collect::<Vec<_>>(),
                "tasks": self.db.tasks.iter().map(task_json).collect::<Vec<_>>(),
            },
            "feed": self.feed_json(None, None, 0, FEED_PAGE),
        }))
    }

    // -- reads that serve one surface, and touch nothing (gaps 1, 2, 4) -------------------------

    /// **Gap 1.** Next steps across every record, soonest due first, each with what it is on
    /// and where it falls today. `limit` caps what is carried; `total` is how many there are.
    pub fn tasks_json(&self, now: Now, limit: usize, include_done: bool) -> Value {
        let mut open: Vec<&Task> = self.db.tasks.iter().filter(|t| t.done_at.is_none()).collect();
        open.sort_by(|a, b| a.due.cmp(&b.due).then_with(|| a.id.cmp(&b.id)));
        let total = open.len();
        let item = |t: &Task| {
            let mut v = task_json(t);
            let refs: Vec<Value> = t.links.iter().filter_map(|l| record_ref(&self.db, l)).collect();
            v["record"] = refs.iter().find(|r| r["kind"] == "deal").or_else(|| refs.first()).cloned().unwrap_or(Value::Null);
            v["records"] = json!(refs);
            v["bucket"] = json!(if t.done_at.is_some() {
                "done"
            } else {
                match now.today.days_until(t.due) {
                    d if d < 0 => "overdue",
                    0 => "today",
                    d if d <= WEEK_DAYS => "week",
                    _ => "later",
                }
            });
            v
        };
        let mut out = json!({
            "open": open.iter().take(limit).map(|t| item(t)).collect::<Vec<_>>(),
            "total": total,
        });
        if include_done {
            let mut done: Vec<&Task> = self.db.tasks.iter().filter(|t| t.done_at.is_some()).collect();
            done.sort_by(|a, b| b.done_at.cmp(&a.done_at).then_with(|| a.id.cmp(&b.id)));
            out["done"] = json!(done.iter().take(limit).map(|t| item(t)).collect::<Vec<_>>());
            out["doneTotal"] = json!(done.len());
        }
        out
    }

    /// **Gap 2.** The feed: every timeline line and every stage change, newest first,
    /// filterable by who did it and what kind. A query over the two append-only logs — it
    /// writes nothing and is **not** shared state (the Inbox's filters stay local, on purpose).
    pub fn feed_json(&self, actor: Option<&Actor>, kind: Option<&str>, page: usize, page_size: usize) -> Value {
        let page_size = page_size.clamp(1, FEED_MAX);
        let mut items: Vec<(Timestamp, &str, Value)> = Vec::new();
        for a in &self.db.activities {
            if actor.is_none_or(|w| *w == a.by) && kind.is_none_or(|k| k == a.kind.word()) {
                items.push((a.at, a.id.as_str(), activity_item(&self.db, a)));
            }
        }
        for m in &self.db.moves {
            if actor.is_none_or(|w| *w == m.by) && kind.is_none_or(|k| k == "stage") {
                items.push((m.at, m.id.as_str(), move_item(&self.db, m)));
            }
        }
        // Newest first; ids are ULIDs, so two lines in one millisecond keep creation order.
        items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(a.1)));
        let total = items.len();
        let start = (page * page_size).min(total);
        json!({
            "items": items.into_iter().skip(start).take(page_size).map(|(_, _, v)| v).collect::<Vec<_>>(),
            "total": total,
            "page": page,
            "pageSize": page_size,
        })
    }

    /// **Gap 4, and gap 13.** A search that is *only* a search. It reads the records and
    /// returns rows and per-kind counts; it does not touch `list`, `focus`, `pending` or
    /// anything else either surface is looking at. ⌘K used to write the shared list and put
    /// it back, because the only search there was *was* the shared list.
    ///
    /// Relevance first, then recently updated — its own order, not the list's sort, because the
    /// list's sort is somebody else's setting. Archived records are not offered.
    pub fn search_json(&self, query: &str, kinds: &[Kind], limit: usize) -> Value {
        let limit = limit.clamp(1, SEARCH_MAX);
        let mut hits: Vec<(i32, Kind, Id)> = self
            .searchable(false)
            .into_iter()
            .filter(|(kind, ..)| kinds.is_empty() || kinds.contains(kind))
            .filter_map(|(kind, id, handle, name)| match_score(query, &handle, &name).map(|s| (s, kind, id)))
            .collect();
        hits.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| self.updated_at(&b.2).cmp(&self.updated_at(&a.2)))
                .then_with(|| a.2.cmp(&b.2))
        });
        let count = |k: Kind| hits.iter().filter(|h| h.1 == k).count();
        json!({
            "query": query.trim(),
            "total": hits.len(),
            "counts": { "company": count(Kind::Company), "contact": count(Kind::Contact), "deal": count(Kind::Deal) },
            "rows": hits.iter().take(limit).filter_map(|h| self.row_v2(&h.2)).collect::<Vec<_>>(),
        })
    }
}

// MARK: - The envelopes

/// How many results the caller asked for, defaulted and bounded.
fn limit_of(req: &Value, default: usize, max: usize) -> usize {
    req.get("limit").and_then(Value::as_u64).map(|n| n as usize).unwrap_or(default).clamp(1, max)
}

/// An actor as a request carries it: `{"kind":"human"}`, `{"kind":"agent","id":…}`, or the
/// short forms `"human"` and a bare agent id.
pub fn parse_actor(v: &Value) -> Result<Actor, String> {
    if let Some(word) = v.as_str() {
        return Ok(if word.eq_ignore_ascii_case("human") || word.eq_ignore_ascii_case("you") {
            Actor::Human
        } else {
            Actor::Agent { id: word.to_string() }
        });
    }
    serde_json::from_value::<Actor>(v.clone()).map_err(|_| {
        "an actor is `{\"kind\":\"human\"}`, `{\"kind\":\"agent\",\"id\":…}`, `\"human\"` or an agent's id".to_string()
    })
}

impl AppState {
    pub(super) fn cmd_bootstrap(&self, ctx: &Ctx) -> Answer {
        Answer::ReadWith("bootstrap", self.bootstrap(ctx.now))
    }

    /// `{cmd: "changes", since: N}`: everything the data did after watermark `N`, in order —
    /// or `{stale: true}` when the log no longer reaches that far and the client should
    /// bootstrap again. Never a partial answer.
    pub(super) fn cmd_changes(&self, req: &Value) -> Result<Answer, String> {
        let since = req.get("since").and_then(Value::as_u64).ok_or("changes needs the watermark you hold — `since`")?;
        let out = match self.changes(since) {
            Some(ops) => json!({
                "watermark": self.db.watermark,
                "from": since,
                "to": self.db.watermark,
                "ops": ops.iter().map(Op::json).collect::<Vec<_>>(),
            }),
            None => json!({ "stale": true, "watermark": self.db.watermark }),
        };
        Ok(Answer::ReadWith("changes", out))
    }

    pub(super) fn cmd_tasks(&self, req: &Value, ctx: &Ctx) -> Answer {
        let include_done = req.get("includeDone").and_then(Value::as_bool).unwrap_or(false);
        Answer::ReadWith("tasks", self.tasks_json(ctx.now, limit_of(req, TASKS_CAP, 1_000), include_done))
    }

    pub(super) fn cmd_feed(&self, req: &Value) -> Result<Answer, String> {
        let actor = match req.get("actor") {
            None | Some(Value::Null) => None,
            Some(v) => Some(parse_actor(v)?),
        };
        let kind = match req.get("kind") {
            None | Some(Value::Null) => None,
            Some(v) => {
                let word = v.as_str().unwrap_or("");
                if !["call", "email", "meeting", "note", "stage"].contains(&word) {
                    return Err(format!("“{word}” is not a feed kind — use call, email, meeting, note or stage"));
                }
                Some(word)
            }
        };
        let page = req.get("page").and_then(Value::as_u64).unwrap_or(0) as usize;
        let size = req.get("pageSize").and_then(Value::as_u64).map(|n| n as usize).unwrap_or(FEED_PAGE);
        Ok(Answer::ReadWith("feed", self.feed_json(actor.as_ref(), kind, page, size)))
    }

    /// `{cmd: "search", q, kinds?, limit?}`. Touches nothing — see [`search_json`](Self::search_json).
    pub(super) fn cmd_search(&self, req: &Value) -> Result<Answer, String> {
        let q = match req.get("q") {
            None => "",
            Some(v) => v.as_str().ok_or("q must be text")?,
        };
        let mut kinds = Vec::new();
        if let Some(list) = req.get("kinds") {
            for v in list.as_array().ok_or("kinds must be a list")? {
                let word = v.as_str().unwrap_or("");
                kinds.push(
                    Kind::parse(word).ok_or_else(|| format!("“{word}” is not a kind — use company, contact or deal"))?,
                );
            }
        }
        Ok(Answer::ReadWith("search", self.search_json(q, &kinds, limit_of(req, SEARCH_DEFAULT, SEARCH_MAX))))
    }
}

#[cfg(test)]
#[path = "split_tests.rs"]
mod tests;
