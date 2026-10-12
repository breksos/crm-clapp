//! **p3: the app side of sync.** `docs/sync-contract.md` is the spec.
//!
//! This file is the part with no `Core` in it: the wire types, the way a change to the dataset becomes the
//! server's *row ops*, the way a server row becomes a model row again, and the [`Backend`] seam the network
//! sits behind (so the client logic is tested against a fake that keeps the contract, and the real server is
//! the step-6 harness's business). Nothing here prints, and nothing here holds a session token.

use crate::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// MARK: - Wire types

/// One entry of the org's log, as `changes` returns it.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerOp {
    pub seq: u64,
    /// `upsert` (a row) or `append` (a feed item).
    pub op: String,
    /// `company`, `contact`, `deal`, `task`, `activity` or `move`.
    pub kind: String,
    pub id: String,
    /// The full row as the server holds it, with `version` on a row.
    pub record: Value,
    pub by: Value,
    pub at: i64,
}

impl ServerOp {
    pub fn from_json(v: &Value) -> Option<ServerOp> {
        Some(ServerOp {
            seq: v.get("seq")?.as_u64()?,
            op: v.get("op")?.as_str()?.to_string(),
            kind: v.get("kind")?.as_str()?.to_string(),
            id: v.get("id")?.as_str()?.to_string(),
            record: v.get("record")?.clone(),
            by: v.get("by").cloned().unwrap_or(Value::Null),
            at: v.get("at").and_then(Value::as_i64).unwrap_or(0),
        })
    }
}

/// What `changes?since=N` answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Changes {
    Ops { watermark: u64, to: u64, ops: Vec<ServerOp> },
    /// The log no longer reaches back that far (or the client is ahead of the org): bootstrap again.
    Stale { watermark: u64 },
}

/// Every page of `bootstrap`, assembled. `watermark` is the **first** page's (the replay point).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bootstrap {
    pub watermark: u64,
    pub pipeline: Value,
    pub members: Vec<(String, String)>,
    /// `(id, name, ownerId)`
    pub agents: Vec<(String, String, String)>,
    /// `(kind, row)` for the four upsert kinds, then the feed kinds.
    pub rows: Vec<(String, Value)>,
}

/// `(members: (id, name), agents: (id, name, ownerId))`
pub type Team = (Vec<(String, String)>, Vec<(String, String, String)>);

/// One change to one record, in the server's write format.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RowOp {
    pub kind: &'static str,
    pub op: &'static str,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<u64>,
    pub record: Value,
}

/// What `write` answers.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteAnswer {
    Accepted { seq_from: u64, seq_to: u64, rows: Vec<AcceptedRow> },
    Stale { kind: String, id: String, current: Value, by: Value, at: i64 },
    Refused { code: String, reason: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedRow {
    pub kind: String,
    pub id: String,
    pub version: u64,
    pub handle: Option<String>,
}

/// Why a call to the server did not get an answer. Only ever one thing, so the caller can only say one thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offline(pub String);

/// Everything the app asks of the server. The real one is HTTP; the tests' is a fake that keeps the contract.
pub trait Backend: Send + Sync {
    fn bootstrap(&self) -> Result<Bootstrap, Offline>;
    fn changes(&self, since: u64) -> Result<Changes, Offline>;
    /// The org's people and agents, for naming someone the seat has not met (a new teammate's first change).
    fn team(&self) -> Result<Team, Offline>;
    fn write(&self, write_id: &str, by: &Actor, ops: &[RowOp]) -> Result<WriteAnswer, Offline>;
    /// Hold the push channel open, calling `on_mark(watermark)` for each `mark`, until it closes, `on_mark`
    /// returns `false`, or the connection fails. `Ok(())` means it ended cleanly (a `bye`, or `on_mark` said stop).
    fn stream(&self, on_mark: &mut dyn FnMut(u64) -> bool) -> Result<(), Offline>;
}

// MARK: - Rows ↔ the server's records

/// The four kinds that are rows (updated in place); the other two are feed items (appended).
pub const ROW_KINDS: [&str; 4] = ["company", "contact", "deal", "task"];

fn wire(mut v: Value, task_due: Option<Date>) -> Value {
    if let (Some(d), Some(o)) = (task_due, v.as_object_mut()) {
        o.insert("due".into(), json!(d.to_string_iso()));
        o.remove("dueSentAt");
    }
    v
}

fn to_value<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

/// A task without the seat-local reminder mark: the comparison that decides whether a task changed *for the org*.
fn org_view(t: &Task) -> Task {
    Task { due_sent_at: None, ..t.clone() }
}

/// What a command did to the dataset, as the server wants to hear it: creates and updates of rows (updates carry
/// the version they read), appends of feed items — in the order that lets each link resolve (companies, contacts,
/// deals, tasks, then activities and moves). View state and the seat's reminder marks are **never** compared.
pub fn row_ops(before: &Db, after: &Db) -> Vec<RowOp> {
    use std::collections::HashMap;
    let mut out = Vec::new();
    let base = |id: &str| Some(before.versions.get(id).copied().unwrap_or(0));
    macro_rules! rows {
        ($kind:expr, $field:ident, $same:expr, $wire:expr) => {{
            let old: HashMap<&str, &_> = before.$field.iter().map(|r| (r.id.as_str(), r)).collect();
            for row in &after.$field {
                match old.get(row.id.as_str()) {
                    None => out.push(RowOp { kind: $kind, op: "create", id: row.id.clone(), base: None, record: $wire(row) }),
                    Some(prev) if !$same(prev, row) => {
                        out.push(RowOp { kind: $kind, op: "update", id: row.id.clone(), base: base(&row.id), record: $wire(row) })
                    }
                    Some(_) => {}
                }
            }
        }};
    }
    rows!("company", companies, |a: &&Company, b: &Company| *a == b, |r: &Company| wire(to_value(r), None));
    rows!("contact", contacts, |a: &&Contact, b: &Contact| *a == b, |r: &Contact| wire(to_value(r), None));
    rows!("deal", deals, |a: &&Deal, b: &Deal| *a == b, |r: &Deal| wire(to_value(r), None));
    rows!("task", tasks, |a: &&Task, b: &Task| org_view(a) == org_view(b), |r: &Task| wire(to_value(r), Some(r.due)));
    let seen: std::collections::HashSet<&str> = before.activities.iter().map(|a| a.id.as_str()).collect();
    for a in after.activities.iter().filter(|a| !seen.contains(a.id.as_str())) {
        out.push(RowOp { kind: "activity", op: "append", id: a.id.clone(), base: None, record: to_value(a) });
    }
    let seen: std::collections::HashSet<&str> = before.moves.iter().map(|m| m.id.as_str()).collect();
    for m in after.moves.iter().filter(|m| !seen.contains(m.id.as_str())) {
        out.push(RowOp { kind: "move", op: "append", id: m.id.clone(), base: None, record: to_value(m) });
    }
    out
}

/// Put one server row into the dataset. `Err` names what was wrong with the row, never its contents.
pub fn put_row(db: &mut Db, kind: &str, record: &Value) -> Result<(), String> {
    let mut v = record.clone();
    let o = v.as_object_mut().ok_or("a row is an object")?;
    let version = o.remove("version").and_then(|n| n.as_u64());
    if kind == "task" {
        let due = o.get("due").and_then(Value::as_str).and_then(Date::parse).ok_or("a task has a due date")?;
        o.insert("due".into(), to_value(&due));
    }
    macro_rules! upsert {
        ($ty:ty, $field:ident) => {{
            let mut row: $ty = serde_json::from_value(v).map_err(|e| format!("a {kind} row did not parse: {e}"))?;
            match db.$field.iter_mut().find(|r| r.id == row.id) {
                Some(slot) => {
                    // The reminder mark is this seat's own; the org's row never carries or erases it.
                    keep_local(&mut row, slot);
                    *slot = row;
                }
                None => db.$field.push(row),
            }
        }};
    }
    let id = o.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    match kind {
        "company" => upsert!(Company, companies),
        "contact" => upsert!(Contact, contacts),
        "deal" => upsert!(Deal, deals),
        "task" => upsert!(Task, tasks),
        other => return Err(format!("{other} is not a row kind")),
    }
    if let Some(n) = version {
        db.versions.insert(id, n);
    }
    Ok(())
}

trait KeepLocal {
    fn keep(&mut self, _existing: &Self) {}
}
impl KeepLocal for Company {}
impl KeepLocal for Contact {}
impl KeepLocal for Deal {}
impl KeepLocal for Task {
    fn keep(&mut self, existing: &Task) {
        self.due_sent_at = existing.due_sent_at;
    }
}
fn keep_local<T: KeepLocal>(new: &mut T, existing: &T) {
    new.keep(existing);
}

/// Append one feed item (an activity or a move). Already there → nothing.
pub fn put_feed(db: &mut Db, kind: &str, record: &Value) -> Result<(), String> {
    match kind {
        "activity" => {
            let a: Activity = serde_json::from_value(record.clone()).map_err(|e| format!("an activity did not parse: {e}"))?;
            if !db.activities.iter().any(|x| x.id == a.id) {
                db.activities.push(a);
            }
        }
        "move" => {
            let m: Move = serde_json::from_value(record.clone()).map_err(|e| format!("a move did not parse: {e}"))?;
            if !db.moves.iter().any(|x| x.id == m.id) {
                db.moves.push(m);
            }
        }
        other => return Err(format!("{other} is not a feed kind")),
    }
    Ok(())
}

/// Counts of what a dataset holds, in the shape the join plan reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub companies: usize,
    pub contacts: usize,
    pub deals: usize,
    pub tasks: usize,
    pub activities: usize,
    pub moves: usize,
    pub total: usize,
}

impl Counts {
    #[allow(dead_code)] // step 5: the join plan counts what it may offer
    pub fn of(db: &Db) -> Counts {
        let mut c = Counts {
            companies: db.companies.len(),
            contacts: db.contacts.len(),
            deals: db.deals.len(),
            tasks: db.tasks.len(),
            activities: db.activities.len(),
            moves: db.moves.len(),
            total: 0,
        };
        c.total = c.companies + c.contacts + c.deals + c.tasks + c.activities + c.moves;
        c
    }
}

// MARK: - The real backend: HTTP to the server

use std::sync::Arc;
use std::time::Duration;

/// `Backend` over HTTP. Every call reads the session from the [`Account`](crate::account::Account) when it is made, so a
/// sign-out takes effect on the next call. The token goes into a request header and nowhere else — never a URL
/// (contract §3.5a), never a log line, never an error message.
pub struct HttpBackend {
    account: Arc<crate::account::Account>,
}

const CONNECT: Duration = Duration::from_secs(3);
const REQUEST: Duration = Duration::from_secs(10);
const BOOTSTRAP: Duration = Duration::from_secs(30);
/// The heartbeat is 30 s; a stream silent for longer than this is dead and is closed from our side.
const STREAM_SILENCE: Duration = Duration::from_secs(45);

impl HttpBackend {
    pub fn new(account: Arc<crate::account::Account>) -> HttpBackend {
        HttpBackend { account }
    }

    fn agent(&self, read: Duration) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(CONNECT)
            .timeout_read(read)
            .timeout_write(REQUEST)
            .redirects(0) // a redirect could carry the bearer token somewhere nobody chose
            .build()
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>, read: Duration) -> Result<ureq::Response, Offline> {
        let (server, token) = self.account.credentials().ok_or_else(|| Offline("not signed in".into()))?;
        let url = format!("{}{}", server.trim_end_matches('/'), path);
        let req = self.agent(read).request(method, &url).set("Authorization", &format!("Bearer {}", token.expose()));
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        match res {
            Ok(r) => Ok(r),
            // 4xx/5xx still carry an answer the caller understands (a stale or refused write is a 409/422).
            Err(ureq::Error::Status(_, r)) => Ok(r),
            // The transport error can quote the URL; say only the kind.
            Err(ureq::Error::Transport(t)) => Err(Offline(format!("could not reach the server ({})", t.kind()))),
        }
    }

    fn json(&self, method: &str, path: &str, body: Option<&Value>, read: Duration) -> Result<(u16, Value), Offline> {
        let r = self.request(method, path, body, read)?;
        let status = r.status();
        Ok((status, r.into_json::<Value>().unwrap_or(Value::Null)))
    }
}

fn gone(status: u16) -> Offline {
    Offline(match status {
        401 => "signed out on the server — sign in again".to_string(),
        s => format!("the server answered {s}"),
    })
}

impl Backend for HttpBackend {
    fn bootstrap(&self) -> Result<Bootstrap, Offline> {
        let mut out = Bootstrap::default();
        let mut path = "/v1/sync/bootstrap".to_string();
        let mut first = true;
        loop {
            let (status, v) = self.json("GET", &path, None, BOOTSTRAP)?;
            if status != 200 {
                return Err(gone(status));
            }
            if first {
                out.watermark = v["watermark"].as_u64().unwrap_or(0);
                out.pipeline = v["pipeline"].clone();
                out.members = v["members"].as_array().map(|a| a.iter().filter_map(|m| Some((m["id"].as_str()?.to_string(), m["name"].as_str()?.to_string()))).collect()).unwrap_or_default();
                out.agents = v["agents"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|m| Some((m["id"].as_str()?.to_string(), m["name"].as_str()?.to_string(), m["ownerId"].as_str()?.to_string()))).collect())
                    .unwrap_or_default();
                first = false;
            }
            let chunk = &v["chunk"];
            let kind = match chunk["kind"].as_str().unwrap_or("") {
                "companies" => "company",
                "contacts" => "contact",
                "deals" => "deal",
                "tasks" => "task",
                "activities" => "activity",
                _ => "move",
            };
            for row in chunk["rows"].as_array().cloned().unwrap_or_default() {
                out.rows.push((kind.to_string(), row));
            }
            match &chunk["next"] {
                Value::Null => break,
                n => {
                    let k = n["kind"].as_str().unwrap_or("");
                    path = match n["after"].as_str() {
                        Some(after) => format!("/v1/sync/bootstrap?kind={k}&after={after}"),
                        None => format!("/v1/sync/bootstrap?kind={k}"),
                    };
                }
            }
        }
        Ok(out)
    }

    fn team(&self) -> Result<Team, Offline> {
        // The first page of a bootstrap carries the people; its rows are not wanted here.
        let (status, v) = self.json("GET", "/v1/sync/bootstrap", None, REQUEST)?;
        if status != 200 {
            return Err(gone(status));
        }
        let members = v["members"].as_array().map(|a| a.iter().filter_map(|m| Some((m["id"].as_str()?.to_string(), m["name"].as_str()?.to_string()))).collect()).unwrap_or_default();
        let agents = v["agents"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| Some((m["id"].as_str()?.to_string(), m["name"].as_str()?.to_string(), m["ownerId"].as_str()?.to_string()))).collect())
            .unwrap_or_default();
        Ok((members, agents))
    }

    fn changes(&self, since: u64) -> Result<Changes, Offline> {
        let (status, v) = self.json("GET", &format!("/v1/sync/changes?since={since}&limit=2000"), None, REQUEST)?;
        if status != 200 {
            return Err(gone(status));
        }
        let watermark = v["watermark"].as_u64().unwrap_or(0);
        if v["stale"] == true {
            return Ok(Changes::Stale { watermark });
        }
        let ops = v["ops"].as_array().map(|a| a.iter().filter_map(ServerOp::from_json).collect()).unwrap_or_default();
        Ok(Changes::Ops { watermark, to: v["to"].as_u64().unwrap_or(since), ops })
    }

    fn write(&self, write_id: &str, by: &Actor, ops: &[RowOp]) -> Result<WriteAnswer, Offline> {
        let body = json!({ "writeId": write_id, "by": by, "ops": ops });
        let (status, v) = self.json("POST", "/v1/sync/write", Some(&body), REQUEST)?;
        match status {
            200 => Ok(WriteAnswer::Accepted {
                seq_from: v["seq"]["from"].as_u64().unwrap_or(0),
                seq_to: v["seq"]["to"].as_u64().unwrap_or(0),
                rows: v["rows"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|r| AcceptedRow {
                                kind: r["kind"].as_str().unwrap_or("").to_string(),
                                id: r["id"].as_str().unwrap_or("").to_string(),
                                version: r["version"].as_u64().unwrap_or(0),
                                handle: r["handle"].as_str().map(String::from),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }),
            409 => Ok(WriteAnswer::Stale {
                kind: v["kind"].as_str().unwrap_or("").to_string(),
                id: v["id"].as_str().unwrap_or("").to_string(),
                current: v["current"].clone(),
                by: v["by"].clone(),
                at: v["at"].as_i64().unwrap_or(0),
            }),
            422 => Ok(WriteAnswer::Refused {
                code: v["code"].as_str().unwrap_or("").to_string(),
                reason: v["reason"].as_str().unwrap_or("the server refused that").to_string(),
            }),
            s => Err(gone(s)),
        }
    }

    fn stream(&self, on_mark: &mut dyn FnMut(u64) -> bool) -> Result<(), Offline> {
        use std::io::BufRead;
        let r = self.request("GET", "/v1/sync/stream", None, STREAM_SILENCE)?;
        if r.status() != 200 {
            return Err(gone(r.status()));
        }
        let mut lines = std::io::BufReader::new(r.into_reader()).lines();
        let mut event = String::new();
        for line in lines.by_ref() {
            let line = line.map_err(|_| Offline("the push channel dropped".into()))?;
            if let Some(name) = line.strip_prefix("event: ") {
                event = name.to_string();
            } else if let Some(data) = line.strip_prefix("data: ") {
                match event.as_str() {
                    "mark" => {
                        if let Some(w) = serde_json::from_str::<Value>(data).ok().and_then(|v| v["watermark"].as_u64()) {
                            if !on_mark(w) {
                                return Ok(());
                            }
                        }
                    }
                    "bye" => return Ok(()),
                    _ => {}
                }
            }
        }
        Err(Offline("the push channel closed".into()))
    }
}

// MARK: - The join commands' answers (`sync_join_plan`, `sync_join`) — contract §7.1

/// A final handle that is not the one proposed, as `sync_join` reports it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HandleChange {
    pub kind: String,
    pub proposed: String,
    #[serde(rename = "final")]
    pub final_handle: String,
}

/// Why a seat may not bring its local records.
#[allow(dead_code)]
pub const CLAIMED_BY_ANOTHER: &str = "claimed-by-another-account";

/// `sync_join_plan()`: nothing has been sent anywhere; `local` counts only what may be brought.
#[allow(dead_code)]
pub fn join_plan(local: Counts, org: Counts, reason: Option<&str>) -> Value {
    let needed = local.total > 0;
    let can_bring = needed && reason.is_none();
    json!({
        "state": if needed { "needed" } else { "not-needed" },
        "choices": if can_bring { json!(["bring", "start-fresh"]) } else { json!(["start-fresh"]) },
        "reason": reason,
        "local": local,
        "org": org,
    })
}

/// `sync_join(choice)` when it worked.
#[allow(dead_code)]
pub fn join_done(choice: &str, uploaded: Counts, parts: usize, changes: &[HandleChange]) -> Value {
    json!({ "ok": true, "choice": choice, "state": "joined", "uploaded": uploaded, "parts": parts, "handleChanges": changes })
}

/// `sync_join("bring")` when a part was refused: the org is as it was after the last complete part.
#[allow(dead_code)]
pub fn join_refused(error: &str, uploaded: Counts, applied: usize, total: usize, failed_at: Option<(&str, &str)>, changes: &[HandleChange]) -> Value {
    json!({
        "ok": false, "choice": "bring", "state": "choosing", "error": error,
        "uploaded": uploaded, "partsApplied": applied, "partsTotal": total,
        "failedAt": failed_at.map(|(kind, handle)| json!({ "kind": kind, "handle": handle })),
        "handleChanges": changes,
    })
}
