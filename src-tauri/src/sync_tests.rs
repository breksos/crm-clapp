//! A server that keeps `docs/sync-contract.md`, in memory, so the client's logic is tested without a network:
//! versions, stale writes, final handles, idempotency, the field whitelist, who a write may claim to be.
//! (The real server's own tests are in `server/`; the two meeting is the step-6 harness.)

use crate::sync::*;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const ORG: &str = "org_1";

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct FakeServer {
    inner: Mutex<Inner>,
    pub bootstraps: AtomicUsize,
    pub writes: AtomicUsize,
    marks: Mutex<Vec<std::sync::mpsc::Sender<u64>>>,
}

#[derive(Default, Clone)]
struct Inner {
    watermark: u64,
    rows: BTreeMap<(String, String), Value>,
    feed: Vec<(String, Value)>,
    ops: Vec<ServerOp>,
    handles: HashSet<String>,
    answers: HashMap<String, WriteAnswer>,
    users: BTreeMap<String, String>,
    agents: Vec<(String, String, String)>,
    retained_from: u64,
}

impl FakeServer {
    pub fn new() -> Arc<FakeServer> {
        Arc::new(FakeServer { inner: Mutex::new(Inner::default()), bootstraps: AtomicUsize::new(0), writes: AtomicUsize::new(0), marks: Mutex::new(Vec::new()) })
    }

    pub fn add_user(&self, id: &str, name: &str) {
        self.inner.lock().unwrap().users.insert(id.into(), name.into());
    }

    pub fn add_agent(&self, id: &str, name: &str, owner: &str) {
        self.inner.lock().unwrap().agents.push((id.into(), name.into(), owner.into()));
    }

    pub fn watermark(&self) -> u64 {
        self.inner.lock().unwrap().watermark
    }

    pub fn row(&self, kind: &str, id: &str) -> Option<Value> {
        self.inner.lock().unwrap().rows.get(&(kind.to_string(), id.to_string())).cloned()
    }

    pub fn rows_of(&self, kind: &str) -> Vec<Value> {
        self.inner.lock().unwrap().rows.iter().filter(|((k, _), _)| k == kind).map(|(_, v)| v.clone()).collect()
    }

    pub fn inner_feed(&self, kind: &str) -> Vec<Value> {
        self.inner.lock().unwrap().feed.iter().filter(|(k, _)| k == kind).map(|(_, v)| v.clone()).collect()
    }

    pub fn trim_log(&self) {
        let mut s = self.inner.lock().unwrap();
        s.retained_from = s.watermark + 1;
        s.ops.clear();
    }

    pub fn client(self: &Arc<Self>, user: &str) -> Arc<FakeClient> {
        Arc::new(FakeClient { server: self.clone(), user: user.to_string(), offline: AtomicBool::new(false), budget: std::sync::atomic::AtomicI64::new(-1), hold: Arc::new((Mutex::new(false), Condvar::new())) })
    }
}

/// One seat's view of the server.
pub struct FakeClient {
    server: Arc<FakeServer>,
    pub user: String,
    pub offline: AtomicBool,
    /// Writes still allowed before the server starts refusing (-1: no limit).
    budget: std::sync::atomic::AtomicI64,
    hold: Arc<(Mutex<bool>, Condvar)>,
}

impl FakeClient {
    pub fn allow_writes(&self, n: i64) {
        self.budget.store(n, Ordering::SeqCst);
    }

    pub fn set_offline(&self, v: bool) {
        self.offline.store(v, Ordering::SeqCst);
    }
    /// Make `write` wait until released (to prove nothing else waits with it).
    pub fn hold_writes(&self, v: bool) {
        *self.hold.0.lock().unwrap() = v;
        self.hold.1.notify_all();
    }
    fn check(&self) -> Result<(), Offline> {
        if self.offline.load(Ordering::SeqCst) {
            Err(Offline("could not reach the server (test)".into()))
        } else {
            Ok(())
        }
    }
}

const DROPPED: [&str; 5] = ["id", "version", "ownerId", "orgId", "dueSentAt"];

fn allowed(kind: &str) -> &'static [&'static str] {
    match kind {
        "company" => &["handle", "name", "domain", "tags", "notes", "origin", "archivedAt", "updatedAt"],
        "contact" => &["handle", "name", "email", "phone", "title", "tags", "origin", "archivedAt", "updatedAt", "companyId"],
        "deal" => &["handle", "title", "contactIds", "value", "origin", "archivedAt", "updatedAt", "companyId", "stage", "status", "movedBy", "movedAt", "openedAt", "closedAt", "pipelineId"],
        "task" => &["handle", "what", "origin", "updatedAt", "due", "doneAt", "by", "links"],
        "activity" => &["at", "kind", "body", "links", "by"],
        _ => &["dealId", "from", "to", "at", "by"],
    }
}

fn is_feed(kind: &str) -> bool {
    kind == "activity" || kind == "move"
}

impl Backend for FakeClient {
    fn bootstrap(&self) -> Result<Bootstrap, Offline> {
        self.check()?;
        self.server.bootstraps.fetch_add(1, Ordering::SeqCst);
        let s = self.server.inner.lock().unwrap();
        let mut rows: Vec<(String, Value)> = Vec::new();
        for kind in ROW_KINDS {
            for ((k, _), v) in &s.rows {
                if k == kind {
                    rows.push((k.clone(), v.clone()));
                }
            }
        }
        for (k, v) in &s.feed {
            rows.push((k.clone(), v.clone()));
        }
        Ok(Bootstrap {
            watermark: s.watermark,
            pipeline: json!({ "id": "sales", "name": "Sales", "stages": ["lead", "qualified", "proposal", "negotiation"] }),
            members: s.users.iter().map(|(i, n)| (i.clone(), n.clone())).collect(),
            agents: s.agents.clone(),
            rows,
        })
    }

    fn team(&self) -> Result<Team, Offline> {
        self.check()?;
        let s = self.server.inner.lock().unwrap();
        Ok((s.users.iter().map(|(i, n)| (i.clone(), n.clone())).collect(), s.agents.clone()))
    }

    fn changes(&self, since: u64) -> Result<Changes, Offline> {
        self.check()?;
        let s = self.server.inner.lock().unwrap();
        if since > s.watermark || (s.retained_from > 0 && since + 1 < s.retained_from) {
            return Ok(Changes::Stale { watermark: s.watermark });
        }
        let ops: Vec<ServerOp> = s.ops.iter().filter(|o| o.seq > since).cloned().collect();
        let to = ops.last().map(|o| o.seq).unwrap_or(since);
        Ok(Changes::Ops { watermark: s.watermark, to, ops })
    }

    fn write(&self, write_id: &str, by: &Actor, ops: &[RowOp]) -> Result<WriteAnswer, Offline> {
        self.check()?;
        {
            let mut held = self.hold.0.lock().unwrap();
            while *held {
                held = self.hold.1.wait(held).unwrap();
            }
        }
        self.check()?;
        self.server.writes.fetch_add(1, Ordering::SeqCst);
        let mut s = self.server.inner.lock().unwrap();
        if let Some(a) = s.answers.get(write_id) {
            return Ok(a.clone());
        }
        if self.budget.load(Ordering::SeqCst) == 0 {
            return Ok(WriteAnswer::Refused { code: "test".into(), reason: "refused for the test".into() });
        }
        let refuse = |code: &str, reason: &str| Ok(WriteAnswer::Refused { code: code.into(), reason: reason.into() });
        let allowed_by = match by {
            Actor::Human { id } => *id == self.user,
            Actor::Agent { id } => s.agents.iter().any(|(a, _, owner)| a == id && *owner == self.user),
        };
        if !allowed_by {
            return refuse("forged-actor", "this session cannot act as that person or agent");
        }
        let mut scratch = s.clone();
        let by_json = serde_json::to_value(by).unwrap();
        let (seq_from, mut rows) = (scratch.watermark + 1, Vec::new());
        for op in ops {
            let Value::Object(rec) = &op.record else { return refuse("bad-record", "a record is an object") };
            for k in rec.keys() {
                if !allowed(op.kind).contains(&k.as_str()) && !DROPPED.contains(&k.as_str()) {
                    return refuse("unknown-field", &format!("“{k}” is not a field the server stores"));
                }
            }
            scratch.watermark += 1;
            let seq = scratch.watermark;
            let key = (op.kind.to_string(), op.id.clone());
            let mut row = rec.clone();
            for d in DROPPED {
                row.remove(d);
            }
            row.insert("id".into(), json!(op.id));
            row.insert("orgId".into(), json!(ORG));
            let mut accepted = AcceptedRow { kind: op.kind.into(), id: op.id.clone(), version: seq, handle: None };
            if is_feed(op.kind) {
                row.insert("ownerId".into(), json!(self.user));
                scratch.feed.push((op.kind.to_string(), Value::Object(row.clone())));
                scratch.ops.push(ServerOp { seq, op: "append".into(), kind: op.kind.into(), id: op.id.clone(), record: Value::Object(row), by: by_json.clone(), at: 1 });
            } else {
                match op.op {
                    "create" => {
                        if scratch.rows.contains_key(&key) {
                            return refuse("id-taken", "that id is already used");
                        }
                        let proposed = rec.get("handle").and_then(Value::as_str).unwrap_or("").to_string();
                        let mut n = 1;
                        let final_handle = loop {
                            let c = if n == 1 { proposed.clone() } else { format!("{proposed}-{n}") };
                            if scratch.handles.insert(c.clone()) {
                                break c;
                            }
                            n += 1;
                        };
                        row.insert("handle".into(), json!(final_handle));
                        row.insert("ownerId".into(), json!(self.user));
                        accepted.handle = Some(final_handle);
                    }
                    _ => {
                        let Some(cur) = scratch.rows.get(&key) else { return refuse("unknown-record", "that record no longer exists") };
                        let version = cur["version"].as_u64().unwrap_or(0);
                        if op.base != Some(version) {
                            let writer = s.ops.iter().find(|o| o.seq == version);
                            return Ok(WriteAnswer::Stale {
                                kind: op.kind.into(),
                                id: op.id.clone(),
                                current: cur.clone(),
                                by: writer.map(|o| o.by.clone()).unwrap_or(Value::Null),
                                at: writer.map(|o| o.at).unwrap_or(0),
                            });
                        }
                        row.insert("handle".into(), cur["handle"].clone());
                        row.insert("ownerId".into(), cur["ownerId"].clone());
                    }
                }
                row.insert("version".into(), json!(seq));
                let row = Value::Object(row);
                scratch.rows.insert(key, row.clone());
                scratch.ops.push(ServerOp { seq, op: "upsert".into(), kind: op.kind.into(), id: op.id.clone(), record: row, by: by_json.clone(), at: now_ms() });
            }
            rows.push(accepted);
        }
        let _ = self.budget.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |b| if b > 0 { Some(b - 1) } else { None });
        let answer = WriteAnswer::Accepted { seq_from, seq_to: scratch.watermark, rows };
        scratch.answers.insert(write_id.to_string(), answer.clone());
        *s = scratch;
        let mark = s.watermark;
        drop(s);
        self.server.marks.lock().unwrap().retain(|tx| tx.send(mark).is_ok());
        Ok(answer)
    }

    fn stream(&self, on_mark: &mut dyn FnMut(u64) -> bool) -> Result<(), Offline> {
        self.check()?;
        let (tx, rx) = std::sync::mpsc::channel();
        self.server.marks.lock().unwrap().push(tx);
        if !on_mark(self.server.watermark()) {
            return Ok(());
        }
        loop {
            if self.offline.load(Ordering::SeqCst) {
                return Err(Offline("the push channel dropped (test)".into()));
            }
            match rx.recv_timeout(std::time::Duration::from_millis(30)) {
                Ok(w) => {
                    if !on_mark(w) {
                        return Ok(());
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Ok(()),
            }
        }
    }
}

// MARK: - The wire, without a Core

fn ctx(n: u8) -> Ctx {
    let now = Now { at: 1_700_000_000_000 + n as i64 * 1000, today: Date::new(2026, 10, 12) };
    Ctx { now, entropy: [n; 10], origin: InstanceId::from_bytes([1; 16]), offset_secs: 0 }
}

fn state_for(user: &str) -> crate::state::AppState {
    let mut st = crate::state::AppState::new();
    st.set_account(Some((Person { id: user.into(), name: user.into() }, Org { id: ORG.into(), name: "Acme".into() })), &Person::default());
    st
}

#[test]
fn what_every_write_verb_sends_is_what_the_server_accepts_and_comes_back_the_same_row() {
    let server = FakeServer::new();
    server.add_user("ada", "Ada");
    let ada = server.client("ada");
    let mut st = state_for("ada");
    let before = st.db();
    let steps = [
        json!({ "cmd": "add", "kind": "company", "name": "Acme Corp" }),
        json!({ "cmd": "add", "kind": "contact", "name": "Ada Whitlock", "fields": { "company": "acme-corp" } }),
        json!({ "cmd": "add", "kind": "deal", "name": "Acme renewal", "fields": { "company": "acme-corp", "value": "45000" } }),
        json!({ "cmd": "task", "handle": "acme-renewal", "what": "Call back", "due": "2026-10-30" }),
        json!({ "cmd": "log", "kind": "call", "handle": "acme-renewal", "body": "went well" }),
        json!({ "cmd": "move", "handle": "acme-renewal", "to": "qualified" }),
        json!({ "cmd": "set", "handle": "acme-corp", "field": "domain", "value": "acme.com" }),
        json!({ "cmd": "link", "handle": "acme-renewal", "toHandle": "ada-whitlock" }),
        json!({ "cmd": "done", "handle": "call-back" }),
        json!({ "cmd": "archive", "handle": "ada-whitlock" }),
    ];
    for (i, s) in steps.iter().enumerate() {
        let out = st.command(s, None, &ctx(i as u8 + 1));
        assert_eq!(out.resp["ok"], true, "{s}: {}", out.resp);
    }
    let after = st.db();
    let ops = row_ops(&before, &after);
    assert!(ops.iter().any(|o| o.kind == "task") && ops.iter().any(|o| o.kind == "move") && ops.iter().any(|o| o.kind == "activity"));
    let ans = ada.write("w1", &Actor::Human { id: "ada".into() }, &ops).unwrap();
    assert!(matches!(ans, WriteAnswer::Accepted { .. }), "{ans:?}");

    // A second seat loads the org from the server and ends up with the same records.
    let mut other = state_for("bo");
    let b = server.client("ada").bootstrap().unwrap();
    other.load_bootstrap(&b).unwrap();
    let (a, o) = (st.db(), other.db());
    let strip = |mut v: Value| {
        for k in ["ownerId", "orgId", "dueSentAt"] {
            v.as_object_mut().unwrap().remove(k);
        }
        v
    };
    for (x, y) in a.companies.iter().zip(&o.companies) {
        assert_eq!(strip(serde_json::to_value(x).unwrap()), strip(serde_json::to_value(y).unwrap()));
    }
    for (x, y) in a.contacts.iter().zip(&o.contacts) {
        assert_eq!(strip(serde_json::to_value(x).unwrap()), strip(serde_json::to_value(y).unwrap()));
    }
    for (x, y) in a.deals.iter().zip(&o.deals) {
        assert_eq!(strip(serde_json::to_value(x).unwrap()), strip(serde_json::to_value(y).unwrap()));
    }
    for (x, y) in a.tasks.iter().zip(&o.tasks) {
        assert_eq!(strip(serde_json::to_value(x).unwrap()), strip(serde_json::to_value(y).unwrap()));
    }
    assert_eq!((a.activities.len(), a.moves.len()), (o.activities.len(), o.moves.len()));
    assert_eq!((o.companies.len(), o.contacts.len(), o.deals.len(), o.tasks.len()), (1, 1, 1, 1));
    assert_eq!(o.watermark, server.watermark());
    assert!(o.deals[0].value.is_some() && o.contacts[0].archived_at.is_some() && o.tasks[0].done_at.is_some());
    assert_eq!(o.versions.len(), 4, "every row remembers the version it was read at");
}

#[test]
fn reminder_marks_are_the_seats_own_and_never_a_row_op() {
    let mut st = state_for("ada");
    st.command(&json!({ "cmd": "add", "kind": "company", "name": "Acme" }), None, &ctx(1));
    st.command(&json!({ "cmd": "task", "handle": "acme", "what": "Call", "due": "2026-10-01" }), None, &ctx(2));
    let before = st.db();
    let mut after = before.clone();
    after.tasks[0].due_sent_at = Some(5);
    assert!(row_ops(&before, &after).is_empty(), "a sweep that marks a task writes nothing to the org");
    after.tasks[0].what = "Call again".into();
    assert_eq!(row_ops(&before, &after).len(), 1);
}

#[test]
fn a_gap_in_the_log_applies_nothing_past_it_and_our_own_ops_are_skipped_and_a_teammates_carry_their_by() {
    let server = FakeServer::new();
    server.add_user("ada", "Ada");
    server.add_user("bo", "Bo");
    let (ada, bo) = (server.client("ada"), server.client("bo"));
    let mut st = state_for("ada");
    st.load_bootstrap(&ada.bootstrap().unwrap()).unwrap();
    let rec = |h: &str| json!({ "handle": h, "name": h, "tags": [], "updatedAt": 1, "archivedAt": null });
    let create = |id: &str, h: &str| RowOp { kind: "company", op: "create", id: id.into(), base: None, record: rec(h) };
    bo.write("a", &Actor::Human { id: "bo".into() }, &[create("c1", "one")]).unwrap();
    bo.write("b", &Actor::Human { id: "bo".into() }, &[create("c2", "two")]).unwrap();
    let Changes::Ops { ops, .. } = ada.changes(0).unwrap() else { panic!() };
    // A hole: seq 2 without 1.
    assert_eq!(st.apply_server_ops(&ops[1..]), Err(crate::state::replicate::Gap { have: 0 }));
    assert_eq!(st.watermark(), 0);
    let delta = st.apply_server_ops(&ops).unwrap();
    assert_eq!(st.watermark(), 2);
    assert_eq!(delta.len(), 2);
    assert_eq!(delta[0].by, Actor::Human { id: "bo".into() }, "attributed to the teammate");
    assert_eq!(delta[0].record["name"], "one");
    assert_eq!(st.name_of_actor(&json!({ "kind": "human", "id": "bo" })), "Bo");
    // Replaying what we hold is a no-op, not a duplicate.
    assert!(st.apply_server_ops(&ops).unwrap().is_empty());
    assert_eq!(st.db_ref().companies.len(), 2);
    let u = st.update(Now { at: 1, today: Date::new(2026, 10, 12) }, &delta);
    assert_eq!(u["delta"]["ops"][0]["by"], json!({ "kind": "human", "id": "bo" }));
    assert_eq!(u["delta"]["from"], 0);
    assert_eq!(u["seat"]["sync"]["state"], "local");
}

#[test]
fn local_delta_ops_carry_who_did_them() {
    for caller in [None, Some("agent-7")] {
        let mut st = crate::state::AppState::new();
        let before = st.db();
        st.command(&json!({ "cmd": "add", "kind": "company", "name": "Acme" }), caller, &ctx(1));
        let ops = st.stamp(&before);
        let want = match caller {
            Some(id) => Actor::Agent { id: id.into() },
            None => Actor::Human { id: st.me().id.clone() },
        };
        assert_eq!(ops[0].by, want);
        assert_eq!(ops[0].json()["by"], serde_json::to_value(&want).unwrap());
    }
}

#[test]
fn only_my_tasks_wake_my_agent_while_signed_in() {
    let mut st = state_for("ada");
    st.set_agents(vec![clappkit::AgentRow { id: "a1".into(), name: "Scout".into(), ..Default::default() }]);
    st.command(&json!({ "cmd": "add", "kind": "company", "name": "Acme" }), None, &ctx(1));
    st.command(&json!({ "cmd": "task", "handle": "acme", "what": "Mine", "due": "2026-10-01" }), None, &ctx(2));
    let mut db = st.db();
    let mut theirs = db.tasks[0].clone();
    theirs.id = "t-theirs".into();
    theirs.handle = "theirs".into();
    theirs.owner_id = "bo".into();
    db.tasks.push(theirs);
    st.replace_db(db);
    let sweep = st.sweep(&ctx(3));
    let sent: Vec<String> = st.db_ref().tasks.iter().filter(|t| t.due_sent_at.is_some()).map(|t| t.handle.clone()).collect();
    assert_eq!(sent, ["mine"], "a teammate's overdue task is on my screen but does not wake my agent");
    assert_eq!(sweep.emits.len(), 1);
}

// MARK: - The Core, signed in

mod core {
    use super::*;
    use crate::replica::ReplicaStore;
    use crate::tests::{a_core, scratch_dir};
    use crate::{Core, Mutex as AsyncMutex};
    use std::time::Duration;

    struct Seat {
        core: Arc<Core>,
        rt: std::mem::ManuallyDrop<tokio::runtime::Runtime>,
        client: Arc<FakeClient>,
        dir: std::path::PathBuf,
        pushed: Arc<Mutex<Vec<Value>>>,
    }

    fn org() -> Org {
        Org { id: ORG.into(), name: "Acme".into() }
    }

    /// A seat signed in as `user`, joined to `server`, with its worker running.
    fn seat(server: &Arc<FakeServer>, user: &str) -> Seat {
        server.add_user(user, user);
        let dir = scratch_dir(&format!("sync-{user}"));
        std::fs::create_dir_all(&dir).unwrap();
        let (core, rt) = a_core(&dir.join("crm.json"));
        let client = server.client(user);
        let pushed: Arc<Mutex<Vec<Value>>> = Default::default();
        let sink = pushed.clone();
        *core.pusher.lock().unwrap() = Some(Arc::new(move |v| sink.lock().unwrap().push(v)));
        let seat = Seat { core, rt: std::mem::ManuallyDrop::new(rt), client, dir, pushed };
        seat.join(user);
        seat
    }

    impl Seat {
        fn join(&self, user: &str) {
            let replica = Arc::new(ReplicaStore::open(&self.dir.join("replica.sqlite")).unwrap());
            self.rt.block_on(async {
                {
                    let mut st = self.core.state.lock().await;
                    st.set_account(Some((Person { id: user.into(), name: user.into() }, org())), &Person::default());
                }
                let c = self.core.clone();
                c.start_sync(self.client.clone(), replica, org(), Duration::from_millis(50)).await.unwrap();
            });
            self.settle();
        }

        /// Wait for the link to be online.
        fn settle(&self) {
            self.until("online", |s| s.core_state() == "online");
        }

        fn core_state(&self) -> String {
            self.rt.block_on(async { self.core.state.lock().await.sync_info().state.word().to_string() })
        }

        fn until(&self, what: &str, f: impl Fn(&Seat) -> bool) {
            for _ in 0..300 {
                if f(self) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("timed out waiting for {what}");
        }

        fn run(&self, req: Value) -> Value {
            self.rt.block_on(self.core.command(req, None)).resp
        }

        fn run_as_agent(&self, req: Value, agent: &str) -> Value {
            self.rt.block_on(self.core.command(req, Some(agent.to_string()))).resp
        }

        fn stop_worker(&self) {
            let rt = self.core.sync_rt().unwrap();
            let handle = rt.worker.lock().unwrap().take();
            if let Some(h) = handle {
                h.abort();
            }
        }

        fn companies(&self) -> Vec<String> {
            self.rt.block_on(async { self.core.state.lock().await.db().companies.iter().map(|c| c.handle.clone()).collect() })
        }
    }

    impl Drop for Seat {
        fn drop(&mut self) {
            // The stream's blocking thread ends when the link drops; a runtime waits for it, so cut the link first
            // and never wait on the runtime.
            self.client.set_offline(true);
            self.client.hold_writes(false);
            // SAFETY: `rt` is taken exactly once, here, and never used again.
            unsafe { std::mem::ManuallyDrop::take(&mut self.rt) }.shutdown_background();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn add(name: &str) -> Value {
        json!({ "cmd": "add", "kind": "company", "name": name })
    }

    #[test]
    fn a_write_is_the_servers_before_it_is_acknowledged_and_says_what_it_was_called() {
        let server = FakeServer::new();
        let ada = seat(&server, "ada");
        let resp = ada.run(add("Acme Corp"));
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(resp["handles"], json!([{ "kind": "company", "proposed": "acme-corp", "final": "acme-corp" }]));
        // The server has it, owned by the person; and so does the replica, at the server's version.
        let row = server.row("company", &ada.rt.block_on(async { ada.core.state.lock().await.db().companies[0].id.clone() })).unwrap();
        assert_eq!((row["handle"].clone(), row["ownerId"].clone(), row["version"].clone()), (json!("acme-corp"), json!("ada"), json!(1)));
        ada.rt.block_on(async {
            let st = ada.core.state.lock().await;
            assert_eq!(st.watermark(), 1);
            assert_eq!(st.db_ref().versions.len(), 1);
        });
        // The replica on disk agrees (a restart would show it).
        let reopened = ReplicaStore::open(&ada.dir.join("replica.sqlite")).unwrap();
        use crate::store::CrmStore;
        let db = reopened.load().unwrap();
        assert_eq!((db.companies.len(), db.watermark), (1, 1));
        // The update the window got carries the op, attributed.
        let last = ada.pushed.lock().unwrap().iter().rev().find(|u| u["delta"]["ops"].as_array().is_some_and(|o| !o.is_empty())).cloned().unwrap();
        assert_eq!(last["delta"]["ops"][0]["by"], json!({ "kind": "human", "id": "ada" }));
    }

    #[test]
    fn two_people_creating_acme_get_two_handles_and_each_is_told_its_own() {
        let server = FakeServer::new();
        let (ada, bo) = (seat(&server, "ada"), seat(&server, "bo"));
        bo.stop_worker();
        ada.stop_worker();
        let a = ada.run(add("Acme"));
        let b = bo.run(add("Acme"));
        assert_eq!(a["handles"][0]["final"], "acme");
        assert_eq!(b["handles"][0], json!({ "kind": "company", "proposed": "acme", "final": "acme-2" }));
        assert_eq!(bo.companies(), ["acme", "acme-2"].iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter().filter(|h| bo.companies().contains(h)).collect::<Vec<_>>());
        assert!(bo.companies().contains(&"acme-2".to_string()), "{:?}", bo.companies());
        assert_eq!(server.rows_of("company").len(), 2);
    }

    #[test]
    fn a_stale_write_is_refused_naming_the_current_value_and_who_set_it_and_the_replica_shows_it() {
        let server = FakeServer::new();
        let (ada, bo) = (seat(&server, "ada"), seat(&server, "bo"));
        ada.run(json!({ "cmd": "add", "kind": "deal", "name": "Pilot", "fields": { "value": "1000" } }));
        bo.until("bo has the deal", |s| s.rt.block_on(async { s.core.state.lock().await.db().deals.len() }) == 1);
        // Ada is looking at version 2; Bo changes the value; Ada, who has not heard yet, changes it too.
        ada.stop_worker();
        let r = bo.run(json!({ "cmd": "set", "handle": "pilot", "field": "value", "value": "60000" }));
        assert_eq!(r["ok"], true, "{r}");
        let resp = ada.run(json!({ "cmd": "set", "handle": "pilot", "field": "value", "value": "2000" }));
        assert_eq!(resp["ok"], false);
        let err = resp["error"].as_str().unwrap();
        assert!(err.starts_with("not saved — pilot changed while you were looking"), "{err}");
        assert!(err.contains("$60,000") && err.contains("set by bo"), "{err}");
        assert_eq!(resp["refused"]["reason"], "stale");
        assert_eq!(resp["refused"]["current"]["value"]["amount"], 6_000_000);
        // Nothing was lost and nothing silently overwritten: the org still says 60,000, and so does Ada's screen.
        assert_eq!(server.rows_of("deal")[0]["value"]["amount"], 6_000_000);
        ada.rt.block_on(async {
            let st = ada.core.state.lock().await;
            assert_eq!(st.db_ref().deals[0].value.as_ref().unwrap().amount, 6_000_000);
        });
        // The agent's feed would say refused; here, the window's own update says the same sentence.
        assert_eq!(resp["split"]["view"]["feed"].as_array().map(|a| a.len()).unwrap_or(0), 0, "a window write is no agent feed line");
    }

    #[test]
    fn offline_refuses_writes_clearly_keeps_reads_and_catches_up_without_a_bootstrap() {
        let server = FakeServer::new();
        let (ada, bo) = (seat(&server, "ada"), seat(&server, "bo"));
        ada.run(add("Acme"));
        bo.until("bo caught up", |s| s.companies().len() == 1);
        let boots = server.bootstraps.load(Ordering::SeqCst);

        bo.client.set_offline(true);
        let resp = bo.run(add("Beta"));
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["refused"]["reason"], "offline");
        assert!(resp["error"].as_str().unwrap().starts_with("not saved — offline:"), "{resp}");
        assert_eq!(bo.companies(), ["acme"], "nothing was queued and nothing half-applied");
        let read = bo.run(json!({ "cmd": "find", "query": "acme" }));
        assert_eq!(read["ok"], true, "reads still work offline");
        bo.until("offline shown", |s| s.core_state() == "offline");
        assert_eq!(read["split"]["seat"]["sync"]["state"], "offline", "and the window is told so");

        // Meanwhile Ada works; Bo comes back and catches up from his watermark.
        ada.run(add("Gamma"));
        bo.client.set_offline(false);
        bo.until("bo back online with Gamma", |s| s.core_state() == "online" && s.companies().len() == 2);
        assert_eq!(server.bootstraps.load(Ordering::SeqCst), boots, "reconnecting is a catch-up, not a bootstrap");
    }

    #[test]
    fn reads_and_navigation_never_wait_on_a_write() {
        let server = FakeServer::new();
        let ada = seat(&server, "ada");
        ada.run(add("Acme"));
        ada.client.hold_writes(true);
        let (core, rt) = (ada.core.clone(), &ada.rt);
        let pending = rt.spawn(async move { core.command(add("Beta"), None).await.resp });
        std::thread::sleep(Duration::from_millis(100));
        // The write is stuck on the network. A read and a navigation go straight through.
        for req in [json!({ "cmd": "find", "query": "acme" }), json!({ "cmd": "show", "id": ada.rt.block_on(async { ada.core.state.lock().await.db().companies[0].id.clone() }) }), json!({ "cmd": "board" })] {
            let r = ada.rt.block_on(async { tokio::time::timeout(Duration::from_secs(2), ada.core.command(req.clone(), None)).await });
            assert!(r.is_ok(), "{req} waited on the write");
            assert_eq!(r.unwrap().resp["ok"], true);
        }
        ada.client.hold_writes(false);
        let resp = rt.block_on(pending).unwrap();
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(ada.companies().len(), 2);
    }

    #[test]
    fn a_writes_view_effects_are_dropped_if_the_person_navigated_meanwhile() {
        let server = FakeServer::new();
        let ada = seat(&server, "ada");
        ada.run(add("Acme"));
        let acme_id = ada.rt.block_on(async { ada.core.state.lock().await.db().companies[0].id.clone() });
        let focus = |s: &Seat| s.rt.block_on(async { s.core.state.lock().await.update(Now { at: 1, today: Date::new(2026, 10, 12) }, &[])["view"]["focus"]["handle"].clone() });
        // Without anyone moving, the new record is what the person is looking at.
        ada.run(add("Beta"));
        assert_eq!(focus(&ada), "beta");

        ada.client.hold_writes(true);
        let core = ada.core.clone();
        let pending = ada.rt.spawn(async move { core.command(add("Gamma"), None).await.resp });
        std::thread::sleep(Duration::from_millis(100));
        ada.run(json!({ "cmd": "show", "id": acme_id }));
        ada.client.hold_writes(false);
        assert_eq!(ada.rt.block_on(pending).unwrap()["ok"], true);
        assert!(ada.companies().contains(&"gamma".to_string()), "the row change always lands");
        assert_eq!(focus(&ada), "acme", "…but the focus the write produced is dropped: the person looked elsewhere");
    }

    #[test]
    fn a_teammates_change_reaches_the_window_attributed_and_signals_nobody() {
        let server = FakeServer::new();
        let (ada, bo) = (seat(&server, "ada"), seat(&server, "bo"));
        let n0 = ada.pushed.lock().unwrap().len();
        bo.run(add("Acme"));
        ada.until("ada sees acme", |s| s.companies().len() == 1);
        let updates: Vec<Value> = ada.pushed.lock().unwrap()[n0..].to_vec();
        let op = updates.iter().flat_map(|u| u["delta"]["ops"].as_array().cloned().unwrap_or_default()).find(|o| o["kind"] == "company").unwrap();
        assert_eq!(op["by"], json!({ "kind": "human", "id": "bo" }), "attributed to Bo");
        assert_eq!(op["record"]["name"], "Acme");
        let u = updates.iter().rev().find(|u| u["delta"]["ops"].as_array().is_some_and(|o| !o.is_empty())).unwrap();
        let latest = updates.last().unwrap();
        assert_eq!(u["view"]["focus"], Value::Null, "someone else's work never moves my focus");
        assert_eq!(u["view"]["feed"].as_array().unwrap().len(), 0, "…or lands in my agent's feed");
        assert_eq!(u["view"]["page"], Value::Null);
        ada.until("ada has met bo", |s| s.pushed.lock().unwrap().last().is_some_and(|u| u["seat"]["members"].as_array().is_some_and(|m| m.iter().any(|m| m["name"] == "bo"))));
        let _ = latest;
    }

    #[test]
    fn an_agents_write_is_attributed_to_its_agent_id_and_an_agent_the_server_never_minted_is_refused() {
        let server = FakeServer::new();
        let ada = seat(&server, "ada");
        // An id the server never minted for Ada: refused, nothing written, and the reason says why.
        let resp = ada.run_as_agent(add("Acme"), "wa-forged");
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["refused"]["code"], "forged-actor");
        assert!(resp["error"].as_str().unwrap().starts_with("not saved — "), "{resp}");
        assert_eq!(server.rows_of("company").len(), 0);
        // Ada's own agent: accepted, and the log says whose agent did it.
        server.add_agent("wa-scout", "Scout", "ada");
        let resp = ada.run_as_agent(add("Acme"), "wa-scout");
        assert_eq!(resp["ok"], true, "{resp}");
        let pushed = ada.pushed.lock().unwrap().iter().rev().find_map(|u| u["delta"]["ops"].get(0).cloned()).unwrap();
        assert_eq!(pushed["by"], json!({ "kind": "agent", "id": "wa-scout" }));
    }

    #[test]
    fn leaving_returns_to_the_local_file_exactly_as_it_was_and_a_restart_does_not_bootstrap() {
        let server = FakeServer::new();
        let dir = scratch_dir("sync-local");
        std::fs::create_dir_all(&dir).unwrap();
        let ada = seat(&server, "ada");
        // Local data, written before joining, then flushed to crm.json by the commit.
        let local = ada.dir.join("crm.json");
        ada.rt.block_on(async {
            let mut st = ada.core.state.lock().await;
            let mut db = st.db();
            db.companies.push(serde_json::from_value(json!({ "id": "local-co", "handle": "mine", "name": "Mine", "tags": [], "updatedAt": 1 })).unwrap());
            st.replace_db(db);
        });
        ada.run(add("Acme"));
        let _ = dir;
        // The replica holds the org's rows; crm.json was never touched while joined.
        assert!(!local.exists() || std::fs::read(&local).map(|b| !String::from_utf8_lossy(&b).contains("acme")).unwrap_or(true));
        ada.rt.block_on(ada.core.stop_sync()).unwrap();
        assert_eq!(ada.rt.block_on(async { ada.core.state.lock().await.sync_info().state.word() }), "local");
        assert!(ada.rt.block_on(async { ada.core.state.lock().await.db().companies.is_empty() }), "the local file never held the org's rows");

        // A restart: the replica is there, so joining again is a catch-up.
        let boots = server.bootstraps.load(Ordering::SeqCst);
        ada.join("ada");
        assert_eq!(ada.companies(), ["acme"]);
        assert_eq!(server.bootstraps.load(Ordering::SeqCst), boots, "the replica was loaded, not downloaded again");
        let _ = AsyncMutex::new(());
    }

    #[test]
    fn a_log_that_no_longer_reaches_back_means_one_bootstrap_and_then_normal_service() {
        let server = FakeServer::new();
        let (ada, bo) = (seat(&server, "ada"), seat(&server, "bo"));
        ada.stop_worker();
        bo.run(add("Acme"));
        bo.run(add("Beta"));
        server.trim_log();
        let boots = server.bootstraps.load(Ordering::SeqCst);
        let rt = ada.core.sync_rt().unwrap();
        ada.rt.block_on(ada.core.sync_pass(&rt)).unwrap();
        assert_eq!(server.bootstraps.load(Ordering::SeqCst), boots + 1);
        assert_eq!(ada.companies().len(), 2);
    }
}

// MARK: - The replica, on disk

mod replica {
    use super::*;
    use crate::replica::{path_for, ReplicaStore};
    use crate::store::CrmStore;

    fn scratch() -> std::path::PathBuf {
        let d = crate::tests::scratch_dir("replica");
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn op(seq: u64, kind: &str, id: &str, record: Value) -> ServerOp {
        ServerOp { seq, op: if kind == "activity" || kind == "move" { "append" } else { "upsert" }.into(), kind: kind.into(), id: id.into(), record, by: Value::Null, at: 1 }
    }

    fn company(id: &str, v: u64, name: &str) -> Value {
        json!({ "id": id, "handle": id, "name": name, "tags": [], "updatedAt": 1, "archivedAt": null, "orgId": ORG, "ownerId": "ada", "version": v })
    }

    #[test]
    fn a_batch_lands_with_its_watermark_in_one_transaction_and_a_reopen_sees_exactly_that() {
        let dir = scratch();
        let path = path_for(&dir, ORG);
        {
            let r = ReplicaStore::open(&path).unwrap();
            r.apply(&[op(1, "company", "c1", company("c1", 1, "One")), op(2, "company", "c2", company("c2", 2, "Two"))], 2).unwrap();
            // An older version of a row never replaces a newer one.
            r.apply(&[op(3, "company", "c1", company("c1", 1, "Stale"))], 3).unwrap();
        }
        let r = ReplicaStore::open(&path).unwrap();
        let db = r.load().unwrap();
        assert_eq!((db.companies.len(), db.watermark), (2, 3));
        assert_eq!(db.companies[0].name, "One");
        assert_eq!(db.versions["c2"], 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_seats_view_and_reminder_marks_persist_without_touching_a_row() {
        let dir = scratch();
        let r = ReplicaStore::open(&path_for(&dir, ORG)).unwrap();
        let task = json!({ "id": "t1", "handle": "call", "what": "Call", "due": "2026-10-01", "links": [], "doneAt": null, "by": { "kind": "human", "id": "ada" },
                           "updatedAt": 1, "orgId": ORG, "ownerId": "ada", "version": 1 });
        r.apply(&[op(1, "task", "t1", task)], 1).unwrap();
        let mut db = r.load().unwrap();
        db.tasks[0].due_sent_at = Some(42);
        db.view.list.query = "acme".into();
        r.save(&db).unwrap();
        let again = r.load().unwrap();
        assert_eq!(again.tasks[0].due_sent_at, Some(42), "the mark is this seat's, in its own table");
        assert_eq!(again.view.list.query, "acme");
        assert_eq!(again.watermark, 1, "saving the seat's things does not move the org's watermark");
        // A teammate's edit of the same task keeps my mark.
        let mut st = crate::state::AppState::with_db(again);
        let edited = json!({ "id": "t1", "handle": "call", "what": "Call again", "due": "2026-10-02", "links": [], "doneAt": null, "by": { "kind": "human", "id": "ada" },
                             "updatedAt": 2, "orgId": ORG, "ownerId": "ada", "version": 2 });
        st.apply_server_ops(&[op(2, "task", "t1", edited)]).unwrap();
        assert_eq!(st.db_ref().tasks[0].what, "Call again");
        assert_eq!(st.db_ref().tasks[0].due_sent_at, Some(42));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_database_is_set_aside_and_the_replica_starts_over() {
        let dir = scratch();
        let path = path_for(&dir, ORG);
        std::fs::write(&path, b"this is not a database, it is a very long string of nothing that sqlite would accept").unwrap();
        let r = ReplicaStore::open(&path).unwrap();
        assert!(!r.is_loaded(), "empty, to be downloaded again");
        let kept = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(kept, "the damaged file was moved aside, not deleted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_name_cannot_be_steered_by_an_org_id() {
        let p = path_for(std::path::Path::new("/data"), "../../etc/passwd");
        assert_eq!(p, std::path::Path::new("/data/replica-etcpasswd.sqlite"));
    }
}

// MARK: - The join commands' result shapes (fixtures)

mod join_shapes {
    use super::*;

    fn counts(c: usize, k: usize, d: usize, t: usize, a: usize, m: usize) -> Counts {
        Counts { companies: c, contacts: k, deals: d, tasks: t, activities: a, moves: m, total: c + k + d + t + a + m }
    }

    fn golden(name: &str, produced: Value) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sync").join(name);
        if std::env::var("UPDATE_FIXTURES").is_ok() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&produced).unwrap() + "\n").unwrap();
        }
        let committed: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{name} is missing — regenerate with UPDATE_FIXTURES=1 cargo test golden"))).unwrap();
        assert_eq!(committed, produced, "{name} no longer matches — if the shape changed on purpose: cd src-tauri && UPDATE_FIXTURES=1 cargo test golden");
    }

    #[test]
    fn golden_join_plan_needed() {
        golden("join-plan-needed.json", join_plan(counts(12, 30, 8, 5, 40, 6), counts(100, 250, 60, 80, 900, 70), None));
    }

    #[test]
    fn golden_join_plan_claimed() {
        golden("join-plan-claimed.json", join_plan(counts(12, 30, 8, 5, 40, 6), counts(100, 250, 60, 80, 900, 70), Some(CLAIMED_BY_ANOTHER)));
    }

    #[test]
    fn golden_join_plan_not_needed() {
        golden("join-plan-not-needed.json", join_plan(Counts::default(), counts(100, 250, 60, 80, 900, 70), None));
    }

    #[test]
    fn golden_join_bring() {
        let changes = [HandleChange { kind: "company".into(), proposed: "acme".into(), final_handle: "acme-2".into() }];
        golden("join-bring.json", join_done("bring", counts(12, 30, 8, 5, 40, 6), 1, &changes));
    }

    #[test]
    fn golden_join_bring_refused() {
        golden(
            "join-bring-refused.json",
            join_refused("not saved — a link points at a record that does not exist", counts(12, 30, 0, 0, 0, 0), 1, 3, Some(("deal", "acme-renewal")), &[]),
        );
    }

    #[test]
    fn golden_join_start_fresh() {
        golden("join-start-fresh.json", join_done("start-fresh", Counts::default(), 0, &[]));
    }

    #[test]
    fn the_plan_never_offers_bring_when_there_is_nothing_to_bring_or_it_is_not_the_persons() {
        assert_eq!(join_plan(counts(1, 0, 0, 0, 0, 0), Counts::default(), None)["choices"], json!(["bring", "start-fresh"]));
        assert_eq!(join_plan(counts(1, 0, 0, 0, 0, 0), Counts::default(), Some(CLAIMED_BY_ANOTHER))["choices"], json!(["start-fresh"]));
        assert_eq!(join_plan(Counts::default(), Counts::default(), None)["state"], "not-needed");
    }
}

// MARK: - Joining an org that already has data (step 5)

mod joining {
    use super::*;
    use crate::replica::ReplicaStore;
    use crate::store::{CrmStore, JsonStore};
    use crate::tests::{a_core, scratch_dir};
    use crate::Core;
    use std::time::Duration;

    struct Seat {
        core: Arc<Core>,
        rt: std::mem::ManuallyDrop<tokio::runtime::Runtime>,
        client: Arc<FakeClient>,
        dir: std::path::PathBuf,
    }

    impl Drop for Seat {
        fn drop(&mut self) {
            self.client.set_offline(true);
            // SAFETY: taken once, here.
            unsafe { std::mem::ManuallyDrop::take(&mut self.rt) }.shutdown_background();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A seat whose local file holds `build`'s records (written as the local person), about to sign in as `user`.
    fn seat(server: &Arc<FakeServer>, user: &str, build: impl Fn(&mut crate::state::AppState)) -> Seat {
        server.add_user(user, user);
        let dir = scratch_dir(&format!("join-{user}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut local = crate::state::AppState::new();
        build(&mut local);
        JsonStore::at(dir.join("crm.json")).save(&local.db()).unwrap();
        let (core, rt) = a_core(&dir.join("crm.json"));
        rt.block_on(async { core.state.lock().await.replace_db(local.db()) });
        Seat { core, rt: std::mem::ManuallyDrop::new(rt), client: server.client(user), dir }
    }

    fn sign_in(s: &Seat, user: &str) -> Result<(), String> {
        let replica = Arc::new(ReplicaStore::open(&s.dir.join("replica.sqlite")).unwrap());
        s.rt.block_on(async {
            s.core.state.lock().await.set_account(Some((Person { id: user.into(), name: user.into() }, Org { id: ORG.into(), name: "Acme".into() })), &Person::default());
            let c = s.core.clone();
            c.begin_join(s.client.clone(), replica, Org { id: ORG.into(), name: "Acme".into() }, Duration::from_millis(50)).await
        })
    }

    fn state_word(s: &Seat) -> String {
        s.rt.block_on(async { s.core.state.lock().await.sync_info().state.word().to_string() })
    }

    fn wait(s: &Seat, what: &str, f: impl Fn(&Seat) -> bool) {
        for _ in 0..300 {
            if f(s) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    fn mine(st: &mut crate::state::AppState) {
        st.command(&json!({ "cmd": "add", "kind": "company", "name": "Acme" }), None, &ctx(1));
        st.command(&json!({ "cmd": "add", "kind": "deal", "name": "Pilot", "fields": { "company": "acme" } }), None, &ctx(2));
        st.command(&json!({ "cmd": "task", "handle": "pilot", "what": "Call", "due": "2026-10-30" }), None, &ctx(3));
        st.command(&json!({ "cmd": "log", "kind": "note", "handle": "pilot", "body": "hi" }), Some("local-agent"), &ctx(4));
        st.command(&json!({ "cmd": "move", "handle": "pilot", "to": "qualified" }), Some("local-agent"), &ctx(5));
    }

    /// `crm.json` still holds exactly the local records (p2's once-only claim may have re-stamped their owner, and
    /// nothing else): never the org's rows, never an `acme-2`, never a version.
    fn assert_local_file_is_only_local(s: &Seat) {
        let db = JsonStore::at(s.dir.join("crm.json")).load().unwrap();
        assert_eq!(db.companies.iter().map(|c| c.handle.as_str()).collect::<Vec<_>>(), ["acme"]);
        assert_eq!((db.deals.len(), db.tasks.len(), db.activities.len(), db.moves.len()), (1, 1, 1, 1));
        assert!(db.versions.is_empty());
    }

    fn plan(s: &Seat) -> Value {
        s.rt.block_on(s.core.join_plan())
    }

    fn join(s: &Seat, choice: &str, part: usize) -> Value {
        let core = s.core.clone();
        s.rt.block_on(async move { core.join(choice, part).await })
    }

    fn teammates_acme(server: &Arc<FakeServer>) {
        server.add_user("bo", "bo");
        let bo = server.client("bo");
        let rec = json!({ "handle": "acme", "name": "Acme", "tags": [], "updatedAt": 1, "archivedAt": null });
        bo.write("bo-1", &Actor::Human { id: "bo".into() }, &[RowOp { kind: "company", op: "create", id: "bo-co".into(), base: None, record: rec }]).unwrap();
    }

    #[test]
    fn an_empty_local_file_just_joins_and_nothing_is_asked() {
        let server = FakeServer::new();
        teammates_acme(&server);
        let s = seat(&server, "ada", |_| {});
        sign_in(&s, "ada").unwrap();
        wait(&s, "online", |s| state_word(s) == "online");
        assert_eq!(s.rt.block_on(async { s.core.state.lock().await.db().companies.len() }), 1, "the org's rows");
        assert_eq!(plan(&s)["state"], "not-needed");
    }

    #[test]
    fn with_local_records_the_seat_waits_sends_nothing_refuses_writes_and_still_reads() {
        let server = FakeServer::new();
        teammates_acme(&server);
        let s = seat(&server, "ada", mine);
        let before = server.watermark();
        sign_in(&s, "ada").unwrap();
        assert_eq!(state_word(&s), "choosing");
        let p = plan(&s);
        assert_eq!((p["state"].clone(), p["choices"].clone(), p["reason"].clone()), (json!("needed"), json!(["bring", "start-fresh"]), Value::Null));
        assert_eq!((p["local"]["companies"].clone(), p["local"]["deals"].clone(), p["local"]["tasks"].clone(), p["local"]["total"].clone()), (json!(1), json!(1), json!(1), json!(5)));
        assert_eq!(p["org"]["companies"], 1);
        // Reads work. A write is refused, says why, and changes nothing.
        let read = s.rt.block_on(s.core.command(json!({ "cmd": "find", "query": "pilot" }), None)).resp;
        assert_eq!(read["ok"], true);
        let w = s.rt.block_on(s.core.command(json!({ "cmd": "add", "kind": "company", "name": "Zed" }), None)).resp;
        assert_eq!(w["ok"], false);
        assert_eq!(w["refused"]["reason"], "choosing");
        assert!(w["error"].as_str().unwrap().contains("choose how to join"), "{w}");
        assert_eq!((server.watermark(), server.writes.load(Ordering::SeqCst)), (before, 1), "only the teammate's own earlier write; nothing of ours left the machine");
        assert_eq!(s.rt.block_on(async { s.core.state.lock().await.db().companies.len() }), 1);
    }

    #[test]
    fn start_fresh_makes_the_replica_the_orgs_leaves_the_file_alone_and_is_not_asked_again() {
        let server = FakeServer::new();
        teammates_acme(&server);
        let s = seat(&server, "ada", mine);
        sign_in(&s, "ada").unwrap();
        let r = join(&s, "start-fresh", BRING);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!((r["choice"].clone(), r["state"].clone(), r["parts"].clone(), r["uploaded"]["total"].clone()), (json!("start-fresh"), json!("joined"), json!(0), json!(0)));
        wait(&s, "online", |s| state_word(s) == "online");
        assert_eq!(s.rt.block_on(async { s.core.state.lock().await.db().companies.iter().map(|c| c.handle.clone()).collect::<Vec<_>>() }), ["acme"], "the org's, not the local file's");
        assert_eq!(server.rows_of("deal").len(), 0, "nothing was uploaded");
        assert_local_file_is_only_local(&s);
        // Leaving and coming back: joined once, not asked again.
        s.rt.block_on(s.core.stop_sync()).unwrap();
        sign_in(&s, "ada").unwrap();
        wait(&s, "online again", |s| state_word(s) == "online");
        assert_eq!(plan(&s)["state"], "not-needed");
    }

    #[test]
    fn bring_uploads_in_dependency_order_reports_changed_handles_and_attributes_unknown_agents_to_the_person() {
        let server = FakeServer::new();
        teammates_acme(&server); // Bo already has "acme": mine will come back as acme-2.
        let s = seat(&server, "ada", mine);
        sign_in(&s, "ada").unwrap();
        let r = join(&s, "bring", 2); // small parts, to exercise more than one
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["choice"], "bring");
        assert_eq!(r["state"], "joined");
        assert!(r["parts"].as_u64().unwrap() >= 3, "{r}");
        assert_eq!((r["uploaded"]["companies"].clone(), r["uploaded"]["deals"].clone(), r["uploaded"]["tasks"].clone(), r["uploaded"]["activities"].clone(), r["uploaded"]["moves"].clone()), (json!(1), json!(1), json!(1), json!(1), json!(1)));
        assert_eq!(r["handleChanges"], json!([{ "kind": "company", "proposed": "acme", "final": "acme-2" }]));
        wait(&s, "online", |s| state_word(s) == "online");
        // Everything is in the org, owned by the person; the agent (never minted on the server) is attributed to them.
        assert_eq!(server.rows_of("company").len(), 2);
        let deal = &server.rows_of("deal")[0];
        assert_eq!((deal["ownerId"].clone(), deal["movedBy"].clone()), (json!("ada"), json!({ "kind": "human", "id": "ada" })));
        let activity = server.inner_feed("activity");
        assert_eq!(activity[0]["by"], json!({ "kind": "human", "id": "ada" }));
        // The local file is untouched, and the replica now holds the org including the upload.
        assert_local_file_is_only_local(&s);
        wait(&s, "replica has both companies", |s| s.rt.block_on(async { s.core.state.lock().await.db().companies.len() }) == 2);
    }

    #[test]
    fn a_minted_agents_work_keeps_its_attribution_when_brought() {
        let server = FakeServer::new();
        server.add_agent("wa-scout", "Scout", "ada");
        let s = seat(&server, "ada", |st| {
            st.command(&json!({ "cmd": "add", "kind": "company", "name": "Acme" }), None, &ctx(1));
            st.command(&json!({ "cmd": "log", "kind": "note", "handle": "acme", "body": "from the agent" }), Some("wa-scout"), &ctx(2));
        });
        sign_in(&s, "ada").unwrap();
        let r = join(&s, "bring", BRING);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(server.inner_feed("activity")[0]["by"], json!({ "kind": "agent", "id": "wa-scout" }));
    }

    #[test]
    fn a_refused_part_stops_with_a_report_and_a_retry_continues_without_duplicating() {
        let server = FakeServer::new();
        let s = seat(&server, "ada", mine);
        sign_in(&s, "ada").unwrap();
        s.client.allow_writes(2); // the third part is refused
        let r = join(&s, "bring", 2);
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!((r["state"].clone(), r["partsApplied"].clone()), (json!("choosing"), json!(2)));
        assert!(r["partsTotal"].as_u64().unwrap() > 2);
        assert!(r["error"].as_str().unwrap().starts_with("not saved — "));
        assert_eq!(r["uploaded"]["total"], 4, "two parts of two");
        assert_eq!(state_word(&s), "choosing", "still not joined");
        let on_server = server.watermark();
        // Fixed; try again: the parts that landed are not applied twice.
        s.client.allow_writes(-1);
        let r = join(&s, "bring", 2);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(server.watermark(), 5, "five records, once each (was {on_server} after the first try)");
        assert_eq!(server.rows_of("company").len(), 1);
    }

    #[test]
    fn offline_during_bring_is_a_clear_refusal_and_the_seat_stays_choosing() {
        let server = FakeServer::new();
        let s = seat(&server, "ada", mine);
        sign_in(&s, "ada").unwrap();
        s.client.set_offline(true);
        let r = join(&s, "bring", BRING);
        assert_eq!((r["ok"].clone(), r["state"].clone()), (json!(false), json!("choosing")));
        assert!(r["error"].as_str().unwrap().contains("offline"), "{r}");
        assert_eq!(server.watermark(), 0);
    }

    #[test]
    fn a_second_account_on_a_claimed_machine_may_only_start_fresh_and_is_told_why() {
        let server = FakeServer::new();
        teammates_acme(&server);
        // Ada signed in first and claimed the file; Bo now signs in on the same install.
        let s = seat(&server, "bo", mine);
        s.rt.block_on(async {
            let mut st = s.core.state.lock().await;
            st.set_account(Some((Person { id: "ada".into(), name: "ada".into() }, Org { id: ORG.into(), name: "Acme".into() })), &Person::default());
            st.set_account(None, &Person::default());
        });
        sign_in(&s, "bo").unwrap();
        let p = plan(&s);
        assert_eq!((p["state"].clone(), p["choices"].clone(), p["reason"].clone()), (json!("needed"), json!(["start-fresh"]), json!("claimed-by-another-account")));
        assert_eq!(p["local"]["total"], 5, "what the file holds is shown");
        let r = join(&s, "bring", BRING);
        assert_eq!(r["ok"], false);
        assert!(r["error"].as_str().unwrap().contains("belong to another account"), "{r}");
        assert_eq!(state_word(&s), "choosing");
        assert_eq!(server.watermark(), 1, "nothing of Ada's went to Bo's org");
        let r = join(&s, "start-fresh", BRING);
        assert_eq!(r["ok"], true, "{r}");
    }

    #[test]
    fn signing_out_while_choosing_returns_to_the_local_file_and_forgets_the_question() {
        let server = FakeServer::new();
        let s = seat(&server, "ada", mine);
        sign_in(&s, "ada").unwrap();
        assert_eq!(state_word(&s), "choosing");
        s.rt.block_on(s.core.stop_sync()).unwrap();
        assert_eq!(state_word(&s), "local");
        assert_eq!(plan(&s)["state"], "not-needed");
        assert_eq!(join(&s, "bring", BRING)["ok"], false, "nothing to join any more");
    }

    const BRING: usize = crate::join::BRING_PART;
}
