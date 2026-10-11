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
    let sql = match kind {
        "companies" => "SELECT id, handle, version, owner_id, archived_at, updated_at, fields FROM companies",
        "contacts" => "SELECT id, handle, version, owner_id, archived_at, updated_at, company_id, fields FROM contacts",
        "deals" => "SELECT id, handle, version, owner_id, archived_at, updated_at, company_id, stage, status, moved_by, moved_at, opened_at, closed_at, pipeline_id, fields FROM deals",
        "tasks" => "SELECT id, handle, version, owner_id, updated_at, due, done_at, by, fields FROM tasks",
        "activities" => "SELECT id, owner_id, at, by, kind, body, links FROM activities",
        _ => "SELECT id, deal_id, at, by, from_stage, to_stage FROM moves",
    };
    let rows = db.query(&format!("{sql} WHERE org_id = $1 AND id > $2 ORDER BY id LIMIT $3"), &[&org, &after, &(PAGE + 1)]).await?;
    let more = rows.len() as i64 > PAGE;
    let page: Vec<&tokio_postgres::Row> = rows.iter().take(PAGE as usize).collect();
    let rendered: Vec<Value> = page
        .iter()
        .map(|r| match kind {
            "companies" => company_row(r, &org),
            "contacts" => contact_row(r, &org),
            "deals" => deal_row(r, &org),
            "tasks" => task_row(r, &org),
            "activities" => activity_row(r, &org),
            _ => move_row(r, &org),
        })
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
