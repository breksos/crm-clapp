//! Record data on the server (p3). See `docs/sync-contract.md`.
//!
//! Step 1: the tables (migration `0002_sync`), the **subscription seam**, `GET /v1/sync/bootstrap` and
//! `GET /v1/sync/changes`. Nothing writes rows yet (step 2).
//!
//! Two rules hold for every function here:
//! * **The org comes from the session.** A request has no org field; every query takes its org from a
//!   [`Scope`], and a [`Scope`] is only made by [`subscription`] from the authenticated session.
//! * **Nothing outside this file says "whole org".** Visibility is `subscription()`'s to decide (p4).

use super::*;
use axum::extract::RawQuery;
use serde_json::{json, Map, Value};
use deadpool_postgres::GenericClient;

/// What a seat may be sent. In p3 it is the whole org; p4 narrows it without touching the protocol.
pub(crate) enum Scope {
    Org(String),
}

impl Scope {
    fn org(&self) -> &str {
        match self {
            Scope::Org(id) => id,
        }
    }
}

/// The only place a session becomes a scope.
pub(crate) fn subscription(who: &Authed) -> Scope {
    Scope::Org(who.org_id.clone())
}

pub(crate) async fn seed_pipeline(db: &impl GenericClient, org_id: &str) -> Result<(), ApiError> {
    db.execute(
        "INSERT INTO pipelines (org_id, id, name, stages) VALUES ($1, 'sales', 'Sales', $2)",
        &[&org_id, &json!(["lead", "qualified", "proposal", "negotiation"])],
    )
    .await?;
    Ok(())
}

pub const PAGE: i64 = 500;
pub const CHANGES_DEFAULT: i64 = 500;
pub const CHANGES_MAX: i64 = 2000;

/// The order bootstrap walks the kinds in. Parents first, so a client applying pages in order never
/// holds a row that points at one it has not seen.
const KINDS: [&str; 6] = ["companies", "contacts", "deals", "tasks", "activities", "moves"];

/// Query parameters, parsed after authentication. A credential-looking one is **ignored** (a session
/// token is never read from a URL); any other unknown one is refused, so an `org=` can never be mistaken
/// for something the server might honour.
fn query(raw: Option<String>, allowed: &[&str]) -> Result<std::collections::HashMap<String, String>, ApiError> {
    let mut out = std::collections::HashMap::new();
    for pair in raw.unwrap_or_default().split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == "token" || k == "access_token" {
            continue;
        }
        if !allowed.contains(&k) {
            return Err(ApiError::bad("unknown parameter"));
        }
        out.insert(k.to_string(), v.to_string());
    }
    Ok(out)
}

fn number(q: &std::collections::HashMap<String, String>, key: &str) -> Result<Option<i64>, ApiError> {
    match q.get(key) {
        None => Ok(None),
        Some(v) => v.parse::<i64>().ok().filter(|n| *n >= 0).map(Some).ok_or_else(|| ApiError::bad("a number is needed")),
    }
}

async fn watermark(db: &impl GenericClient, scope: &Scope) -> Result<(i64, i64), ApiError> {
    let row = db.query_one("SELECT watermark, retained_from FROM orgs WHERE id = $1", &[&scope.org()]).await?;
    Ok((row.get(0), row.get(1)))
}

// MARK: - Rows, as the split shape's `data` carries them

fn put(o: &mut Map<String, Value>, k: &str, v: impl Into<Value>) {
    o.insert(k.to_string(), v.into());
}

/// `fields` first, then the typed columns on top: a column is the truth for what it holds.
fn base(fields: Value, org: &str) -> Map<String, Value> {
    let mut o = match fields {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    put(&mut o, "orgId", org);
    o
}

fn company_row(r: &tokio_postgres::Row, org: &str) -> Value {
    let mut o = base(r.get("fields"), org);
    put(&mut o, "id", r.get::<_, String>("id"));
    put(&mut o, "handle", r.get::<_, String>("handle"));
    put(&mut o, "version", r.get::<_, i64>("version"));
    put(&mut o, "ownerId", r.get::<_, String>("owner_id"));
    put(&mut o, "archivedAt", r.get::<_, Option<i64>>("archived_at"));
    put(&mut o, "updatedAt", r.get::<_, i64>("updated_at"));
    Value::Object(o)
}

fn contact_row(r: &tokio_postgres::Row, org: &str) -> Value {
    let Value::Object(mut o) = company_row(r, org) else { unreachable!() };
    put(&mut o, "companyId", r.get::<_, Option<String>>("company_id"));
    Value::Object(o)
}

fn deal_row(r: &tokio_postgres::Row, org: &str) -> Value {
    let Value::Object(mut o) = contact_row(r, org) else { unreachable!() };
    put(&mut o, "stage", r.get::<_, String>("stage"));
    put(&mut o, "status", r.get::<_, String>("status"));
    put(&mut o, "movedBy", r.get::<_, Value>("moved_by"));
    put(&mut o, "movedAt", r.get::<_, i64>("moved_at"));
    put(&mut o, "openedAt", r.get::<_, i64>("opened_at"));
    put(&mut o, "closedAt", r.get::<_, Option<i64>>("closed_at"));
    put(&mut o, "pipelineId", r.get::<_, String>("pipeline_id"));
    Value::Object(o)
}

fn task_row(r: &tokio_postgres::Row, org: &str) -> Value {
    let mut o = base(r.get("fields"), org);
    put(&mut o, "id", r.get::<_, String>("id"));
    put(&mut o, "handle", r.get::<_, String>("handle"));
    put(&mut o, "version", r.get::<_, i64>("version"));
    put(&mut o, "ownerId", r.get::<_, String>("owner_id"));
    put(&mut o, "updatedAt", r.get::<_, i64>("updated_at"));
    put(&mut o, "due", r.get::<_, String>("due"));
    put(&mut o, "doneAt", r.get::<_, Option<i64>>("done_at"));
    put(&mut o, "by", r.get::<_, Value>("by"));
    Value::Object(o)
}

fn activity_row(r: &tokio_postgres::Row, org: &str) -> Value {
    json!({
        "id": r.get::<_, String>("id"), "orgId": org, "ownerId": r.get::<_, String>("owner_id"),
        "at": r.get::<_, i64>("at"), "by": r.get::<_, Value>("by"), "kind": r.get::<_, String>("kind"),
        "body": r.get::<_, String>("body"), "links": r.get::<_, Value>("links"),
    })
}

fn move_row(r: &tokio_postgres::Row, org: &str) -> Value {
    json!({
        "id": r.get::<_, String>("id"), "orgId": org, "dealId": r.get::<_, String>("deal_id"),
        "at": r.get::<_, i64>("at"), "by": r.get::<_, Value>("by"),
        "from": r.get::<_, String>("from_stage"), "to": r.get::<_, String>("to_stage"),
    })
}

fn select_sql(table: &str) -> &'static str {
    match table {
        "companies" => "SELECT id, handle, version, owner_id, archived_at, updated_at, fields FROM companies",
        "contacts" => "SELECT id, handle, version, owner_id, archived_at, updated_at, company_id, fields FROM contacts",
        "deals" => "SELECT id, handle, version, owner_id, archived_at, updated_at, company_id, stage, status, moved_by, moved_at, opened_at, closed_at, pipeline_id, fields FROM deals",
        "tasks" => "SELECT id, handle, version, owner_id, updated_at, due, done_at, by, fields FROM tasks",
        "activities" => "SELECT id, owner_id, at, by, kind, body, links FROM activities",
        _ => "SELECT id, deal_id, at, by, from_stage, to_stage FROM moves",
    }
}

fn render(table: &str, r: &tokio_postgres::Row, org: &str) -> Value {
    match table {
        "companies" => company_row(r, org),
        "contacts" => contact_row(r, org),
        "deals" => deal_row(r, org),
        "tasks" => task_row(r, org),
        "activities" => activity_row(r, org),
        _ => move_row(r, org),
    }
}

// MARK: - GET /v1/sync/bootstrap

/// One page of the subscription's rows. The first call (no `kind`) also carries the pipeline, the org's
/// people and the org's agents; each page says where the next one starts.
///
/// Pages are not one snapshot: the client keeps the first page's `watermark`, finishes paging, then
/// applies `changes?since=<that>` — applying an upsert is idempotent and only moves a row forward.
pub(crate) async fn bootstrap(State(app): State<App>, headers: HeaderMap, RawQuery(raw): RawQuery) -> Res<Value> {
    let (db, who) = authenticate(&app, &headers).await?;
    let q = query(raw, &["kind", "after"])?;
    let scope = subscription(&who);
    let org = scope.org().to_string();
    let first = !q.contains_key("kind");
    let kind = q.get("kind").map(String::as_str).unwrap_or(KINDS[0]);
    let Some(at) = KINDS.iter().position(|k| *k == kind) else {
        return Err(ApiError::bad("unknown kind"));
    };
    let after = q.get("after").cloned().unwrap_or_default();
    if after.len() > 200 {
        return Err(ApiError::bad("unknown cursor"));
    }

    let (mark, _) = watermark(&db, &scope).await?;
    let sql = select_sql(kind);
    let rows = db.query(&format!("{sql} WHERE org_id = $1 AND id > $2 ORDER BY id LIMIT $3"), &[&org, &after, &(PAGE + 1)]).await?;
    let more = rows.len() as i64 > PAGE;
    let page: Vec<&tokio_postgres::Row> = rows.iter().take(PAGE as usize).collect();
    let rendered: Vec<Value> = page
        .iter()
        .map(|r| render(kind, r, &org))
        .collect();
    let next = if more {
        let last = page.last().map(|r| r.get::<_, String>("id")).unwrap_or_default();
        json!({ "kind": kind, "after": last })
    } else if let Some(k) = KINDS.get(at + 1) {
        json!({ "kind": k, "after": Value::Null })
    } else {
        Value::Null
    };

    let mut out = json!({ "watermark": mark, "chunk": { "kind": kind, "rows": rendered, "next": next } });
    if first {
        let p = db.query_one("SELECT id, name, stages FROM pipelines WHERE org_id = $1", &[&org]).await?;
        out["pipeline"] = json!({ "id": p.get::<_, String>(0), "name": p.get::<_, String>(1), "stages": p.get::<_, Value>(2) });
        let people = db
            .query("SELECT u.id, u.name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.org_id = $1 ORDER BY u.name, u.id", &[&org])
            .await?;
        out["members"] = people.iter().map(|r| json!({ "id": r.get::<_, String>(0), "name": r.get::<_, String>(1) })).collect();
        let agents = db.query("SELECT workspace_id, name, user_id FROM agent_idents WHERE org_id = $1 ORDER BY workspace_id", &[&org]).await?;
        out["agents"] = agents
            .iter()
            .map(|r| json!({ "id": r.get::<_, String>(0), "name": r.get::<_, String>(1), "ownerId": r.get::<_, String>(2) }))
            .collect();
    }
    Ok(Json(out))
}

// MARK: - GET /v1/sync/changes

/// Every op after `since`, in order — or `{stale:true}` when the log no longer reaches back that far,
/// or the client is ahead of the org (its replica is not this org's history).
pub(crate) async fn changes(State(app): State<App>, headers: HeaderMap, RawQuery(raw): RawQuery) -> Res<Value> {
    let (db, who) = authenticate(&app, &headers).await?;
    let q = query(raw, &["since", "limit"])?;
    let scope = subscription(&who);
    let Some(since) = number(&q, "since")? else {
        return Err(ApiError::bad("since is needed"));
    };
    let limit = number(&q, "limit")?.unwrap_or(CHANGES_DEFAULT).clamp(1, CHANGES_MAX);

    let (mark, retained_from) = watermark(&db, &scope).await?;
    if since > mark || (retained_from > 0 && since + 1 < retained_from) {
        return Ok(Json(json!({ "stale": true, "watermark": mark })));
    }
    let rows = db
        .query(
            "SELECT seq, op, kind, id, record, by, at FROM ops WHERE org_id = $1 AND seq > $2 ORDER BY seq LIMIT $3",
            &[&scope.org(), &since, &limit],
        )
        .await?;
    let ops: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "seq": r.get::<_, i64>(0), "op": r.get::<_, String>(1), "kind": r.get::<_, String>(2),
                "id": r.get::<_, String>(3), "record": r.get::<_, Value>(4), "by": r.get::<_, Value>(5), "at": r.get::<_, i64>(6),
            })
        })
        .collect();
    let to = rows.last().map(|r| r.get::<_, i64>(0)).unwrap_or(since);
    Ok(Json(json!({ "watermark": mark, "from": since, "to": to, "ops": ops })))
}

// MARK: - POST /v1/sync/write

pub const WRITE_MAX_OPS: usize = 10_000;
pub const WRITE_MAX_BYTES: usize = 8 * 1024 * 1024;
const ROW_MAX_BYTES: usize = 64 * 1024;
const WRITES_KEPT: i64 = 24 * 60 * 60;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct WriteReq {
    write_id: String,
    /// Who made the command: the session's person, or one of that person's workspace agents.
    by: identity::Actor,
    ops: Vec<OpReq>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct OpReq {
    kind: String,
    op: String,
    id: String,
    #[serde(default)]
    base: Option<i64>,
    record: Value,
}

/// Why a batch wrote nothing.
pub(crate) enum Fail {
    Stale(Value),
    Refused { code: &'static str, reason: String },
    Api(ApiError),
}

impl From<tokio_postgres::Error> for Fail {
    fn from(e: tokio_postgres::Error) -> Fail {
        Fail::Api(e.into())
    }
}

fn refuse<T>(code: &'static str, reason: impl Into<String>) -> Result<T, Fail> {
    Err(Fail::Refused { code, reason: reason.into() })
}

fn table_of(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "company" => "companies",
        "contact" => "contacts",
        "deal" => "deals",
        "task" => "tasks",
        "activity" => "activities",
        "move" => "moves",
        _ => return None,
    })
}

/// A record object being taken apart: every field a handler understands is *removed*, so whatever
/// is left over is something the server was not told how to store — and is refused, never kept.
struct Rec(Map<String, Value>);

impl Rec {
    fn new(v: Value) -> Result<Rec, Fail> {
        match v {
            Value::Object(m) => Ok(Rec(m)),
            _ => refuse("bad-record", "a record is an object"),
        }
    }
    fn drop_keys(&mut self, keys: &[&str]) {
        for k in keys {
            self.0.remove(*k);
        }
    }
    fn str(&mut self, k: &'static str) -> Result<String, Fail> {
        match self.0.remove(k) {
            Some(Value::String(s)) if s.len() <= 20_000 => Ok(s),
            _ => refuse("bad-field", format!("{k} is needed, as text")),
        }
    }
    fn opt_str(&mut self, k: &'static str) -> Result<Option<String>, Fail> {
        match self.0.remove(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if s.len() <= 20_000 => Ok(Some(s)),
            _ => refuse("bad-field", format!("{k} must be text or null")),
        }
    }
    fn int(&mut self, k: &'static str) -> Result<i64, Fail> {
        match self.0.remove(k).and_then(|v| v.as_i64()) {
            Some(n) if n >= 0 => Ok(n),
            _ => refuse("bad-field", format!("{k} is needed, as a number")),
        }
    }
    fn opt_int(&mut self, k: &'static str) -> Result<Option<i64>, Fail> {
        match self.0.remove(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => match v.as_i64() {
                Some(n) if n >= 0 => Ok(Some(n)),
                _ => refuse("bad-field", format!("{k} must be a number or null")),
            },
        }
    }
    fn ids(&mut self, k: &'static str) -> Result<Vec<String>, Fail> {
        match self.0.remove(k) {
            None => Ok(Vec::new()),
            Some(Value::Array(a)) if a.len() <= 5_000 => a
                .into_iter()
                .map(|v| match v {
                    Value::String(s) if s.len() <= 200 => Ok(s),
                    _ => refuse("bad-field", format!("{k} holds ids")),
                })
                .collect(),
            _ => refuse("bad-field", format!("{k} must be a list of ids")),
        }
    }
    fn raw(&mut self, k: &str) -> Option<Value> {
        self.0.remove(k)
    }
    /// What is left becomes `fields`: only the names this kind stores, and nothing else.
    fn rest(self, allowed: &[&str]) -> Result<Value, Fail> {
        for k in self.0.keys() {
            if !allowed.contains(&k.as_str()) {
                return refuse("unknown-field", format!("“{k}” is not a field the server stores"));
            }
        }
        let v = Value::Object(self.0);
        if v.to_string().len() > ROW_MAX_BYTES {
            return refuse("too-big", "that record is too large");
        }
        Ok(v)
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn valid_handle(h: &str) -> bool {
    !h.is_empty() && h.len() <= 64 && !h.starts_with('-') && !h.ends_with('-') && h.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_date(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && d[..4].parse::<u32>().is_ok()
        && d[5..7].parse::<u32>().is_ok_and(|m| (1..=12).contains(&m))
        && d[8..].parse::<u32>().is_ok_and(|x| (1..=31).contains(&x))
}

struct Batch<'a> {
    tx: &'a deadpool_postgres::Transaction<'a>,
    org: String,
    user: String,
    stages: Vec<String>,
    pipeline_id: String,
}

impl Batch<'_> {
    /// May the session's person say this actor did a thing? Themselves, or one of **their** workspace agents.
    async fn may_claim(&self, a: &identity::Actor) -> Result<bool, Fail> {
        Ok(match a {
            identity::Actor::Human { id } => *id == self.user,
            identity::Actor::Agent { id } => self
                .tx
                .query_opt("SELECT 1 FROM agent_idents WHERE org_id = $1 AND user_id = $2 AND workspace_id = $3", &[&self.org, &self.user, id])
                .await?
                .is_some(),
        })
    }

    async fn actor(&self, v: Value, what: &'static str) -> Result<identity::Actor, Fail> {
        let Ok(a) = serde_json::from_value::<identity::Actor>(v) else { return refuse("bad-field", format!("{what} is not an actor")) };
        if !self.may_claim(&a).await? {
            return refuse("forged-actor", format!("{what} names someone this session cannot act as"));
        }
        Ok(a)
    }

    async fn exists(&self, table: &str, id: &str) -> Result<bool, Fail> {
        Ok(self.tx.query_opt(&format!("SELECT 1 FROM {table} WHERE org_id = $1 AND id = $2"), &[&self.org, &id]).await?.is_some())
    }

    /// Every id must be a record **in this org**: a link can never reach into another one.
    async fn all_exist(&self, ids: &[String], tables: &[&str]) -> Result<(), Fail> {
        for id in ids {
            let mut found = false;
            for t in tables {
                found |= self.exists(t, id).await?;
            }
            if !found {
                return refuse("unknown-link", "a link points at a record that does not exist");
            }
        }
        Ok(())
    }

    async fn fetch(&self, table: &str, id: &str) -> Result<Value, Fail> {
        let r = self.tx.query_one(&format!("{} WHERE org_id = $1 AND id = $2", select_sql(table)), &[&self.org, &id]).await?;
        Ok(render(table, &r, &self.org))
    }

    /// The stale answer: the row as it is now, and who wrote it and when.
    async fn stale(&self, table: &str, kind: &str, id: &str, mark: i64) -> Result<Fail, Fail> {
        let current = self.fetch(table, id).await?;
        let version = current["version"].as_i64().unwrap_or(0);
        let w = self.tx.query_opt("SELECT by, at FROM ops WHERE org_id = $1 AND seq = $2", &[&self.org, &version]).await?;
        let (by, at) = w.map(|r| (r.get::<_, Value>(0), r.get::<_, i64>(1))).unwrap_or((Value::Null, 0));
        Ok(Fail::Stale(json!({ "result": "stale", "kind": kind, "id": id, "current": current, "by": by, "at": at, "watermark": mark })))
    }

    /// Claim the final handle: the proposal, or `proposal-2`, `-3`… — decided by the table's primary
    /// key, never by a check that two writers could both pass.
    async fn claim_handle(&self, kind: &str, id: &str, proposed: &str) -> Result<String, Fail> {
        if !valid_handle(proposed) {
            return refuse("bad-handle", "a handle is lower-case letters, digits and hyphens");
        }
        for n in 1..=1000 {
            let candidate = if n == 1 { proposed.to_string() } else { format!("{proposed}-{n}") };
            if candidate.len() > 64 {
                break;
            }
            let done = self
                .tx
                .execute("INSERT INTO handles (org_id, handle, kind, id) VALUES ($1,$2,$3,$4) ON CONFLICT (org_id, handle) DO NOTHING", &[&self.org, &candidate, &kind, &id])
                .await?;
            if done == 1 {
                return Ok(candidate);
            }
        }
        refuse("bad-handle", "no free handle could be made from that")
    }
}

pub(crate) async fn write(State(app): State<App>, headers: HeaderMap, Json(req): Json<WriteReq>) -> Result<Response, ApiError> {
    let (mut db, who) = authenticate(&app, &headers).await?;
    let scope = subscription(&who);
    match apply(&mut db, &who, &scope, req).await {
        Ok(body) => {
            if let Some(mark) = body["watermark"].as_i64() {
                app.hub.publish(&who.org_id, mark);
            }
            Ok((StatusCode::OK, Json(body)).into_response())
        }
        Err(Fail::Stale(body)) => Ok((StatusCode::CONFLICT, Json(body)).into_response()),
        Err(Fail::Refused { code, reason }) => Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "result": "refused", "reason": reason, "code": code }))).into_response()),
        Err(Fail::Api(e)) => Err(e),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

async fn apply(db: &mut Object, who: &Authed, scope: &Scope, req: WriteReq) -> Result<Value, Fail> {
    if req.write_id.is_empty() || req.write_id.len() > 100 {
        return refuse("bad-write-id", "a write needs an id");
    }
    if req.ops.is_empty() {
        return refuse("empty", "a write needs at least one change");
    }
    if req.ops.len() > WRITE_MAX_OPS {
        return refuse("too-big", format!("a write holds at most {WRITE_MAX_OPS} changes"));
    }
    let org = scope.org().to_string();
    let tx = db.transaction().await?;
    // The org's row is the lock: writers to one org take turns, so seqs are gapless and in order.
    let mut mark: i64 = tx.query_one("SELECT watermark FROM orgs WHERE id = $1 FOR UPDATE", &[&org]).await?.get(0);
    if let Some(prev) = tx.query_opt("SELECT answer FROM writes WHERE org_id = $1 AND write_id = $2", &[&org, &req.write_id]).await? {
        return Ok(prev.get(0)); // a retry: the same answer, and nothing applied twice
    }
    let p = tx.query_one("SELECT id, stages FROM pipelines WHERE org_id = $1", &[&org]).await?;
    let stages = p.get::<_, Value>(1).as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
    let b = Batch { tx: &tx, org: org.clone(), user: who.user_id.clone(), stages, pipeline_id: p.get(0) };

    if !b.may_claim(&req.by).await? {
        return refuse("forged-actor", "this session cannot act as that person or agent");
    }
    let by_json = serde_json::to_value(&req.by).unwrap_or(Value::Null);
    let (seq_from, at) = (mark + 1, now_ms());
    let start = mark;
    let mut rows = Vec::new();

    for o in req.ops {
        let Some(table) = table_of(&o.kind) else { return refuse("bad-kind", "unknown kind of record") };
        let feed = matches!(o.kind.as_str(), "activity" | "move");
        let ok_op = if feed { o.op == "append" } else { o.op == "create" || o.op == "update" };
        if !ok_op {
            return refuse("bad-op", format!("a {} cannot be {}d", o.kind, o.op));
        }
        if !valid_id(&o.id) {
            return refuse("bad-id", "that id is not valid");
        }
        mark += 1;
        let seq = mark;
        let mut rec = Rec::new(o.record)?;
        rec.drop_keys(&["id", "version", "ownerId", "orgId", "dueSentAt"]);
        let proposed = rec.opt_str("handle")?;
        let creating = o.op != "update";

        // The row already there (update), or proof that the id is free (create / append).
        let existing = if creating {
            if b.exists(table, &o.id).await? || tx.query_opt("SELECT 1 FROM handles WHERE org_id = $1 AND id = $2", &[&org, &o.id]).await?.is_some() {
                return refuse("id-taken", "that id is already used");
            }
            None
        } else {
            let sql = match table {
                "deals" => "SELECT version, moved_by FROM deals WHERE org_id = $1 AND id = $2",
                "tasks" => "SELECT version, by FROM tasks WHERE org_id = $1 AND id = $2",
                _ => "SELECT version, null::jsonb FROM {t} WHERE org_id = $1 AND id = $2",
            };
            let sql = sql.replace("{t}", table);
            let Some(r) = tx.query_opt(&sql, &[&org, &o.id]).await? else {
                return refuse("unknown-record", "that record no longer exists");
            };
            let version: i64 = r.get(0);
            match o.base {
                None => return refuse("needs-base", "an update says which version it read"),
                Some(base) if base != version => return Err(b.stale(table, &o.kind, &o.id, start).await?),
                Some(_) => {}
            }
            Some(r.get::<_, Option<Value>>(1))
        };

        let handle = match table {
            "companies" | "contacts" | "deals" | "tasks" => {
                if creating {
                    let want = proposed.ok_or_else(|| Fail::Refused { code: "bad-handle", reason: "a new record proposes a handle".into() })?;
                    Some(b.claim_handle(&o.kind, &o.id, &want).await?)
                } else {
                    None
                }
            }
            _ => None,
        };

        match table {
            "companies" => {
                let (archived, updated) = (rec.opt_int("archivedAt")?, rec.int("updatedAt")?);
                let fields = rec.rest(&["name", "domain", "tags", "notes", "origin"])?;
                if creating {
                    tx.execute(
                        "INSERT INTO companies (org_id,id,handle,version,owner_id,archived_at,updated_at,fields) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                        &[&org, &o.id, &handle, &seq, &who.user_id, &archived, &updated, &fields],
                    )
                    .await?;
                } else {
                    tx.execute("UPDATE companies SET version=$3, archived_at=$4, updated_at=$5, fields=$6 WHERE org_id=$1 AND id=$2", &[&org, &o.id, &seq, &archived, &updated, &fields]).await?;
                }
            }
            "contacts" => {
                let (archived, updated, company) = (rec.opt_int("archivedAt")?, rec.int("updatedAt")?, rec.opt_str("companyId")?);
                if let Some(c) = &company {
                    b.all_exist(std::slice::from_ref(c), &["companies"]).await?;
                }
                let fields = rec.rest(&["name", "email", "phone", "title", "tags", "origin"])?;
                if creating {
                    tx.execute(
                        "INSERT INTO contacts (org_id,id,handle,version,owner_id,archived_at,updated_at,company_id,fields) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                        &[&org, &o.id, &handle, &seq, &who.user_id, &archived, &updated, &company, &fields],
                    )
                    .await?;
                } else {
                    tx.execute("UPDATE contacts SET version=$3, archived_at=$4, updated_at=$5, company_id=$6, fields=$7 WHERE org_id=$1 AND id=$2", &[&org, &o.id, &seq, &archived, &updated, &company, &fields]).await?;
                }
            }
            "deals" => {
                let (archived, updated, company) = (rec.opt_int("archivedAt")?, rec.int("updatedAt")?, rec.opt_str("companyId")?);
                let (stage, status, pipeline) = (rec.str("stage")?, rec.str("status")?, rec.str("pipelineId")?);
                let (moved_at, opened_at, closed_at) = (rec.int("movedAt")?, rec.int("openedAt")?, rec.opt_int("closedAt")?);
                if !b.stages.contains(&stage) {
                    return refuse("bad-field", "that is not a stage of this pipeline");
                }
                if !["open", "won", "lost"].contains(&status.as_str()) {
                    return refuse("bad-field", "a deal is open, won or lost");
                }
                if pipeline != b.pipeline_id {
                    return refuse("bad-field", "that is not this org's pipeline");
                }
                let moved_by_v = rec.raw("movedBy").unwrap_or(Value::Null);
                // Whoever last moved it stays on it through other people's edits; a *change* has to be the session's.
                let moved_by = match &existing {
                    Some(Some(prev)) if *prev == moved_by_v => moved_by_v,
                    _ => serde_json::to_value(b.actor(moved_by_v, "movedBy").await?).unwrap_or(Value::Null),
                };
                if let Some(c) = &company {
                    b.all_exist(std::slice::from_ref(c), &["companies"]).await?;
                }
                let contacts = rec.0.get("contactIds").cloned();
                let ids = Rec(Map::from_iter([("contactIds".to_string(), contacts.unwrap_or(json!([])))])).ids("contactIds")?;
                b.all_exist(&ids, &["contacts"]).await?;
                if let Some(v) = rec.0.get("value") {
                    let ok = v.is_null() || (v["amount"].is_i64() && v["currency"].as_str().is_some_and(|c| c.len() == 3));
                    if !ok {
                        return refuse("bad-field", "a value is an amount and a currency");
                    }
                }
                let fields = rec.rest(&["title", "contactIds", "value", "origin"])?;
                if creating {
                    tx.execute(
                        "INSERT INTO deals (org_id,id,handle,version,owner_id,archived_at,updated_at,company_id,stage,status,moved_by,moved_at,opened_at,closed_at,pipeline_id,fields)
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
                        &[&org, &o.id, &handle, &seq, &who.user_id, &archived, &updated, &company, &stage, &status, &moved_by, &moved_at, &opened_at, &closed_at, &pipeline, &fields],
                    )
                    .await?;
                } else {
                    tx.execute(
                        "UPDATE deals SET version=$3, archived_at=$4, updated_at=$5, company_id=$6, stage=$7, status=$8, moved_by=$9, moved_at=$10, opened_at=$11, closed_at=$12, pipeline_id=$13, fields=$14 WHERE org_id=$1 AND id=$2",
                        &[&org, &o.id, &seq, &archived, &updated, &company, &stage, &status, &moved_by, &moved_at, &opened_at, &closed_at, &pipeline, &fields],
                    )
                    .await?;
                }
            }
            "tasks" => {
                let (updated, due, done_at) = (rec.int("updatedAt")?, rec.str("due")?, rec.opt_int("doneAt")?);
                if !valid_date(&due) {
                    return refuse("bad-field", "a due date is YYYY-MM-DD");
                }
                let by_v = rec.raw("by").unwrap_or(Value::Null);
                let by = match &existing {
                    Some(Some(prev)) => {
                        if *prev != by_v {
                            return refuse("forged-actor", "who made a next step cannot be changed");
                        }
                        by_v
                    }
                    _ => serde_json::to_value(b.actor(by_v, "by").await?).unwrap_or(Value::Null),
                };
                let links = rec.ids("links")?;
                b.all_exist(&links, &["companies", "contacts", "deals"]).await?;
                let mut fields = rec.rest(&["what", "origin"])?;
                fields["links"] = json!(links);
                if creating {
                    tx.execute(
                        "INSERT INTO tasks (org_id,id,handle,version,owner_id,updated_at,due,done_at,by,fields) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                        &[&org, &o.id, &handle, &seq, &who.user_id, &updated, &due, &done_at, &by, &fields],
                    )
                    .await?;
                } else {
                    tx.execute("UPDATE tasks SET version=$3, updated_at=$4, due=$5, done_at=$6, fields=$7 WHERE org_id=$1 AND id=$2", &[&org, &o.id, &seq, &updated, &due, &done_at, &fields]).await?;
                }
            }
            "activities" => {
                let (at_ms, kind, body) = (rec.int("at")?, rec.str("kind")?, rec.str("body")?);
                if !["call", "email", "meeting", "note"].contains(&kind.as_str()) || body.trim().is_empty() {
                    return refuse("bad-field", "an activity is a call, email, meeting or note, with something to say");
                }
                let by = serde_json::to_value(b.actor(rec.raw("by").unwrap_or(Value::Null), "by").await?).unwrap_or(Value::Null);
                let links = rec.ids("links")?;
                b.all_exist(&links, &["companies", "contacts", "deals"]).await?;
                rec.rest(&[])?;
                tx.execute(
                    "INSERT INTO activities (org_id,id,owner_id,at,by,kind,body,links) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                    &[&org, &o.id, &who.user_id, &at_ms, &by, &kind, &body, &json!(links)],
                )
                .await?;
            }
            _ => {
                let (deal, from, to, at_ms) = (rec.str("dealId")?, rec.str("from")?, rec.str("to")?, rec.int("at")?);
                if from.len() > 32 || to.len() > 32 {
                    return refuse("bad-field", "a stage word is short");
                }
                b.all_exist(std::slice::from_ref(&deal), &["deals"]).await?;
                let by = serde_json::to_value(b.actor(rec.raw("by").unwrap_or(Value::Null), "by").await?).unwrap_or(Value::Null);
                rec.rest(&[])?;
                tx.execute(
                    "INSERT INTO moves (org_id,id,deal_id,at,by,from_stage,to_stage) VALUES ($1,$2,$3,$4,$5,$6,$7)",
                    &[&org, &o.id, &deal, &at_ms, &by, &from, &to],
                )
                .await?;
            }
        }

        let record = b.fetch(table, &o.id).await?;
        let op_name = if feed { "append" } else { "upsert" };
        tx.execute(
            "INSERT INTO ops (org_id, seq, kind, id, op, record, by, at, write_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[&org, &seq, &o.kind, &o.id, &op_name, &record, &by_json, &at, &req.write_id],
        )
        .await?;
        let mut row = json!({ "kind": o.kind, "id": o.id, "version": seq });
        if let Some(h) = record.get("handle") {
            row["handle"] = h.clone();
        }
        rows.push(row);
    }

    tx.execute("UPDATE orgs SET watermark = $2 WHERE id = $1", &[&org, &mark]).await?;
    let answer = json!({ "result": "accepted", "seq": { "from": seq_from, "to": mark }, "watermark": mark, "rows": rows });
    tx.execute(
        "INSERT INTO writes (org_id, write_id, seq_from, seq_to, answer, at) VALUES ($1,$2,$3,$4,$5,$6)",
        &[&org, &req.write_id, &seq_from, &mark, &answer, &(now())],
    )
    .await?;
    tx.execute("DELETE FROM writes WHERE org_id = $1 AND at < $2", &[&org, &(now() - WRITES_KEPT)]).await?;
    tx.commit().await?;
    Ok(answer)
}

// MARK: - GET /v1/sync/stream

/// The push channel's clocks.
#[derive(Clone, Copy, Debug)]
pub struct StreamTiming {
    /// A `mark` is sent at least this often, carrying the current watermark even when nothing happened.
    pub heartbeat: std::time::Duration,
    /// How often an open stream looks at the database itself: the session still valid, and the
    /// watermark (a write made through another server process never reaches this process's hub).
    pub poll: std::time::Duration,
    /// A burst of writes becomes one `mark`.
    pub coalesce: std::time::Duration,
}

impl Default for StreamTiming {
    fn default() -> StreamTiming {
        StreamTiming {
            heartbeat: std::time::Duration::from_secs(30),
            poll: std::time::Duration::from_secs(5),
            coalesce: std::time::Duration::from_millis(100),
        }
    }
}

/// A seat may hold at most this many open streams; a newer one closes the oldest.
pub const STREAMS_PER_SEAT: usize = 2;

type Kill = std::sync::Arc<tokio::sync::Notify>;

/// Who is listening. In-process: a write made through this process wakes its org's streams at once; the
/// stream's own poll covers anything that did not.
#[derive(Default)]
pub struct Hub {
    orgs: std::sync::Mutex<std::collections::HashMap<String, tokio::sync::watch::Sender<i64>>>,
    seats: std::sync::Mutex<std::collections::HashMap<String, Vec<(u64, Kill)>>>,
    next: std::sync::atomic::AtomicU64,
}

impl Hub {
    fn watch(&self, org: &str) -> tokio::sync::watch::Receiver<i64> {
        let mut orgs = self.orgs.lock().unwrap_or_else(|e| e.into_inner());
        orgs.entry(org.to_string()).or_insert_with(|| tokio::sync::watch::channel(0).0).subscribe()
    }

    /// Tell the org's streams the watermark is now `mark` (never backwards).
    pub(crate) fn publish(&self, org: &str, mark: i64) {
        let mut orgs = self.orgs.lock().unwrap_or_else(|e| e.into_inner());
        orgs.entry(org.to_string())
            .or_insert_with(|| tokio::sync::watch::channel(0).0)
            .send_if_modified(|v| if mark > *v { *v = mark; true } else { false });
    }

    /// Register a stream for `seat`, closing the seat's oldest if it already has the limit.
    fn register(&self, seat: &str) -> (u64, Kill) {
        let id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let kill = std::sync::Arc::new(tokio::sync::Notify::new());
        let mut seats = self.seats.lock().unwrap_or_else(|e| e.into_inner());
        let list = seats.entry(seat.to_string()).or_default();
        while list.len() >= STREAMS_PER_SEAT {
            let (_, oldest) = list.remove(0);
            oldest.notify_one();
        }
        list.push((id, kill.clone()));
        (id, kill)
    }

    fn unregister(&self, seat: &str, id: u64) {
        let mut seats = self.seats.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(list) = seats.get_mut(seat) {
            list.retain(|(i, _)| *i != id);
            if list.is_empty() {
                seats.remove(seat);
            }
        }
    }
}

/// Removes the stream from its seat when the client goes away, however it goes.
struct Registration {
    hub: Arc<Hub>,
    seat: String,
    id: u64,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.hub.unregister(&self.seat, self.id);
    }
}

struct StreamState {
    app: App,
    org: String,
    session: String,
    rx: tokio::sync::watch::Receiver<i64>,
    kill: Arc<tokio::sync::Notify>,
    last: i64,
    first: bool,
    done: bool,
    heartbeat: tokio::time::Interval,
    poll: tokio::time::Interval,
    _reg: Registration,
}

fn mark_frame(mark: i64) -> Result<axum::body::Bytes, std::convert::Infallible> {
    Ok(axum::body::Bytes::from(format!("event: mark\ndata: {{\"watermark\":{mark}}}\n\n")))
}

fn bye_frame(reason: &str) -> Result<axum::body::Bytes, std::convert::Infallible> {
    Ok(axum::body::Bytes::from(format!("event: bye\ndata: {{\"reason\":\"{reason}\"}}\n\n")))
}

impl StreamState {
    async fn session_ok(&self) -> bool {
        let Ok(db) = self.app.db().await else { return true }; // a busy pool is not a sign-out
        db.query_opt("SELECT 1 FROM sessions WHERE id = $1 AND revoked_at IS NULL AND expires_at > $2", &[&self.session, &now()])
            .await
            .map(|r| r.is_some())
            .unwrap_or(true)
    }

    async fn watermark(&self) -> Option<i64> {
        let db = self.app.db().await.ok()?;
        db.query_one("SELECT watermark FROM orgs WHERE id = $1", &[&self.org]).await.ok().map(|r| r.get(0))
    }

    /// The next frame, or `None` when the stream is over.
    async fn next(&mut self) -> Option<Result<axum::body::Bytes, std::convert::Infallible>> {
        if self.done {
            return None;
        }
        if self.first {
            self.first = false;
            self.last = self.watermark().await?;
            return Some(mark_frame(self.last));
        }
        loop {
            tokio::select! {
                _ = self.kill.notified() => {
                    self.done = true;
                    return Some(bye_frame("superseded"));
                }
                changed = self.rx.changed() => {
                    if changed.is_err() {
                        self.done = true;
                        return None;
                    }
                    // A burst becomes one mark.
                    tokio::time::sleep(self.app.stream.coalesce).await;
                    let mark = (*self.rx.borrow_and_update()).max(self.last);
                    if mark > self.last {
                        self.last = mark;
                        return Some(mark_frame(mark));
                    }
                }
                _ = self.poll.tick() => {
                    if !self.session_ok().await {
                        self.done = true;
                        return Some(bye_frame("signed-out"));
                    }
                    if let Some(mark) = self.watermark().await {
                        if mark > self.last {
                            self.last = mark;
                            return Some(mark_frame(mark));
                        }
                    }
                }
                _ = self.heartbeat.tick() => {
                    if !self.session_ok().await {
                        self.done = true;
                        return Some(bye_frame("signed-out"));
                    }
                    let mark = self.watermark().await.unwrap_or(self.last).max(self.last);
                    self.last = mark;
                    return Some(mark_frame(mark));
                }
            }
        }
    }
}

/// One long-lived `text/event-stream` per seat. Each event is `mark` carrying a watermark and nothing
/// else — no row, no name — so there is nothing in it to leak or to arrive out of order. The session
/// token is read from the `Authorization` header only; a URL never carries it (§3.5a).
pub(crate) async fn stream(State(app): State<App>, headers: HeaderMap, RawQuery(raw): RawQuery) -> Result<Response, ApiError> {
    let (db, who) = authenticate(&app, &headers).await?;
    query(raw, &[])?;
    drop(db);
    let scope = subscription(&who);
    let (id, kill) = app.hub.register(&who.seat_id);
    let reg = Registration { hub: app.hub.clone(), seat: who.seat_id.clone(), id };
    let (hb, poll) = (app.stream.heartbeat, app.stream.poll);
    let start = tokio::time::Instant::now();
    let state = StreamState {
        org: scope.org().to_string(),
        session: who.session_id.clone(),
        rx: app.hub.watch(scope.org()),
        app,
        kill,
        last: 0,
        first: true,
        done: false,
        heartbeat: tokio::time::interval_at(start + hb, hb),
        poll: tokio::time::interval_at(start + poll, poll),
        _reg: reg,
    };
    let body = axum::body::Body::from_stream(futures_util::stream::unfold(state, |mut s| async move {
        let frame = s.next().await?;
        Some((frame, s))
    }));
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(body)
        .map_err(|_| ApiError::internal())
}
