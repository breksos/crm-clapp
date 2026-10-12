//! Step 1 of p3 against a real PostgreSQL: the tables, `bootstrap`, `changes`, and the tenancy walls
//! around them. Nothing writes rows through the API yet, so the rows are put in with SQL.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crm_server::{App, Config};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::str::FromStr;
use tower::ServiceExt;

struct T {
    app: App,
    pool: deadpool_postgres::Pool,
    schema: String,
    admin: tokio_postgres::Client,
}

static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

async fn start() -> T {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must point at a PostgreSQL the tests may create schemas in");
    let schema = format!("s_{}_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst), crm_server::now());
    let (admin, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.expect("connect");
    tokio::spawn(conn);
    admin.batch_execute(&format!("CREATE SCHEMA {schema}")).await.unwrap();
    let mut pg = tokio_postgres::Config::from_str(&url).unwrap();
    pg.options(format!("-c search_path={schema}"));
    let mgr = deadpool_postgres::Manager::new(pg, tokio_postgres::NoTls);
    let pool = deadpool_postgres::Pool::builder(mgr).max_size(8).build().unwrap();
    let app = App::new(pool.clone(), Config::from_env()).await.map_err(|e| format!("{e:?}")).unwrap();
    T { app, pool, schema, admin }
}

impl T {
    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path).header("content-type", "application/json");
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let res = self.app.clone().router().oneshot(req.body(Body::from(body.to_string())).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
    async fn get(&self, path: &str, token: &str) -> (StatusCode, Value) {
        self.call("GET", path, Some(token), Value::Null).await
    }
    async fn sign_up(&self, email: &str, org: &str) -> Value {
        let (s, v) = self
            .call("POST", "/v1/signup", None, json!({ "email": email, "password": "correct horse battery", "name": email.split('@').next().unwrap(), "orgName": org, "device": "test" }))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }
    async fn cleanup(self) {
        drop(self.pool);
        let _ = self.admin.batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema)).await;
    }
}


fn token(s: &Value) -> String {
    s["token"].as_str().unwrap().to_string()
}
fn user(s: &Value) -> String {
    s["person"]["id"].as_str().unwrap().to_string()
}
fn org(s: &Value) -> String {
    s["org"]["id"].as_str().unwrap().to_string()
}
fn human(s: &Value) -> Value {
    json!({ "kind": "human", "id": user(s) })
}

fn company(handle: &str, name: &str) -> Value {
    json!({ "handle": handle, "name": name, "tags": [], "updatedAt": 1, "archivedAt": null })
}

fn create(kind: &str, id: &str, record: Value) -> Value {
    json!({ "kind": kind, "op": "create", "id": id, "record": record })
}

fn update(kind: &str, id: &str, base: i64, record: Value) -> Value {
    json!({ "kind": kind, "op": "update", "id": id, "base": base, "record": record })
}

impl T {
    async fn post_invite(&self, inviter: &Value, email: &str) -> Value {
        let (s, inv) = self.call("POST", "/v1/invites", Some(&token(inviter)), json!({})).await;
        assert_eq!(s, StatusCode::OK, "{inv}");
        let (s, v) = self.call("POST", "/v1/invites/accept", None, json!({ "token": inv["token"], "email": email, "password": "another long password", "name": email.split('@').next().unwrap(), "device": "d" })).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }
    async fn write(&self, who: &Value, write_id: &str, ops: Vec<Value>) -> (StatusCode, Value) {
        self.write_as(who, human(who), write_id, ops).await
    }
    async fn write_as(&self, who: &Value, by: Value, write_id: &str, ops: Vec<Value>) -> (StatusCode, Value) {
        self.call("POST", "/v1/sync/write", Some(&token(who)), json!({ "writeId": write_id, "by": by, "ops": ops })).await
    }
    async fn watermark(&self, who: &Value) -> i64 {
        self.get("/v1/sync/changes?since=0", &token(who)).await.1["watermark"].as_i64().unwrap()
    }
    async fn count(&self, table: &str) -> i64 {
        self.pool.get().await.unwrap().query_one(&format!("SELECT count(*) FROM {table}"), &[]).await.unwrap().get(0)
    }
}

fn deal(handle: &str, stage: &str, moved_by: Value) -> Value {
    json!({ "handle": handle, "title": "Pilot", "companyId": null, "contactIds": [], "value": null, "stage": stage, "status": "open",
            "pipelineId": "sales", "openedAt": 5, "closedAt": null, "movedBy": moved_by, "movedAt": 5, "archivedAt": null, "updatedAt": 5 })
}

fn assert_no_view_state(v: &Value) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                assert!(!["focus", "list", "board", "pending", "feed", "follow", "page"].contains(&k.as_str()), "view state on the wire: {k}");
                assert_no_view_state(v);
            }
        }
        Value::Array(a) => a.iter().for_each(assert_no_view_state),
        _ => {}
    }
}

#[tokio::test]
async fn a_create_is_accepted_numbered_logged_and_visible_to_bootstrap_and_changes() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let (s, v) = t.write(&a, "w1", vec![create("company", "c1", company("acme", "Acme"))]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["result"], "accepted");
    assert_eq!((v["seq"]["from"].clone(), v["seq"]["to"].clone(), v["watermark"].clone()), (json!(1), json!(1), json!(1)));
    assert_eq!(v["rows"][0], json!({ "kind": "company", "id": "c1", "version": 1, "handle": "acme" }));
    assert_no_view_state(&v);

    let (_, b) = t.get("/v1/sync/bootstrap", &token(&a)).await;
    assert_eq!(b["watermark"], 1);
    let row = &b["chunk"]["rows"][0];
    assert_eq!((row["id"].clone(), row["version"].clone(), row["name"].clone(), row["ownerId"].clone()), (json!("c1"), json!(1), json!("Acme"), json!(user(&a))));
    let (_, c) = t.get("/v1/sync/changes?since=0", &token(&a)).await;
    assert_eq!(c["ops"][0]["seq"], 1);
    assert_eq!(c["ops"][0]["op"], "upsert");
    assert_eq!(c["ops"][0]["by"], human(&a));
    assert_eq!(c["ops"][0]["record"]["version"], 1);
    t.cleanup().await;
}

#[tokio::test]
async fn an_update_needs_the_version_it_read_and_a_stale_one_names_the_row_and_who_wrote_it() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let inv = t.post_invite(&a, "bo@example.com").await;
    let b = inv;
    t.write(&a, "w1", vec![create("company", "c1", company("acme", "Acme"))]).await;

    // No base: refused. The right base: accepted, version moves to its seq.
    let mut rec = company("acme", "Acme Corp");
    rec["updatedAt"] = json!(2);
    let (s, v) = t.write(&a, "w2", vec![json!({ "kind": "company", "op": "update", "id": "c1", "record": rec.clone() })]).await;
    assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("needs-base")));
    let (s, v) = t.write(&a, "w3", vec![update("company", "c1", 1, rec.clone())]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["rows"][0]["version"], 2);

    // Bo read version 1 and writes from it: stale — the row as it is now, and Ada as its writer.
    let mut mine = company("acme", "Acme Ltd");
    mine["updatedAt"] = json!(3);
    let (s, v) = t.write(&b, "w4", vec![update("company", "c1", 1, mine)]).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["result"], "stale");
    assert_eq!(v["current"]["name"], "Acme Corp");
    assert_eq!(v["current"]["version"], 2);
    assert_eq!(v["by"], human(&a));
    assert!(v["at"].as_i64().unwrap() > 0);
    assert_eq!(v["watermark"], 2);
    assert_eq!(t.watermark(&a).await, 2, "a stale write changed nothing");
    t.cleanup().await;
}

#[tokio::test]
async fn a_batch_is_all_or_nothing_even_for_the_rows_that_were_fine() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    t.write(&a, "w1", vec![create("company", "c1", company("acme", "Acme"))]).await;
    let before = (t.watermark(&a).await, t.count("companies").await, t.count("handles").await);

    // A fresh company, then a stale update: the fresh one must not exist afterwards, nor its handle.
    let (s, _) = t.write(&a, "w2", vec![create("company", "c2", company("beta", "Beta")), update("company", "c1", 99, company("acme", "X"))]).await;
    assert_eq!(s, StatusCode::CONFLICT);
    // …and a refusal part-way through is the same.
    let (s, v) = t.write(&a, "w3", vec![create("company", "c3", company("gamma", "Gamma")), create("deal", "d1", deal("pilot", "no-such-stage", human(&a)))]).await;
    assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("bad-field")));
    assert_eq!((t.watermark(&a).await, t.count("companies").await, t.count("handles").await), before);
    // The handle "beta" was never kept: it is free for the next writer.
    let (s, v) = t.write(&a, "w4", vec![create("company", "c4", company("beta", "Beta"))]).await;
    assert_eq!((s, v["rows"][0]["handle"].clone()), (StatusCode::OK, json!("beta")));
    t.cleanup().await;
}

#[tokio::test]
async fn a_move_is_a_deal_update_and_a_move_event_in_one_batch_numbered_in_order() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    t.write(&a, "w1", vec![create("deal", "d1", deal("pilot", "lead", human(&a)))]).await;
    let mut moved = deal("pilot", "qualified", human(&a));
    moved["movedAt"] = json!(9);
    let ev = json!({ "dealId": "d1", "from": "lead", "to": "qualified", "at": 9, "by": human(&a) });
    let (s, v) = t.write(&a, "w2", vec![update("deal", "d1", 1, moved), json!({ "kind": "move", "op": "append", "id": "m1", "record": ev })]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["seq"]["from"].clone(), v["seq"]["to"].clone()), (json!(2), json!(3)));
    let (_, c) = t.get("/v1/sync/changes?since=1", &token(&a)).await;
    let ops: Vec<(i64, String, String)> = c["ops"].as_array().unwrap().iter().map(|o| (o["seq"].as_i64().unwrap(), o["kind"].as_str().unwrap().to_string(), o["op"].as_str().unwrap().to_string())).collect();
    assert_eq!(ops, [(2, "deal".into(), "upsert".into()), (3, "move".into(), "append".into())]);
    // A move with no matching stage change is the client's business; but a move of a deal that is not there is refused.
    let bad = json!({ "dealId": "nope", "from": "lead", "to": "won", "at": 9, "by": human(&a) });
    let (s, v) = t.write(&a, "w3", vec![json!({ "kind": "move", "op": "append", "id": "m2", "record": bad })]).await;
    assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("unknown-link")));
    t.cleanup().await;
}

#[tokio::test]
async fn handles_are_final_from_the_server_and_two_simultaneous_acmes_get_two_handles() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let b = t.post_invite(&a, "bo@example.com").await;
    let (r1, r2) = tokio::join!(
        t.write(&a, "wa", vec![create("company", "ca", company("acme", "Acme"))]),
        t.write(&b, "wb", vec![create("company", "cb", company("acme", "Acme"))]),
    );
    assert_eq!((r1.0, r2.0), (StatusCode::OK, StatusCode::OK));
    let mut got = vec![r1.1["rows"][0]["handle"].as_str().unwrap().to_string(), r2.1["rows"][0]["handle"].as_str().unwrap().to_string()];
    got.sort();
    assert_eq!(got, ["acme", "acme-2"], "two records, two handles, each told its own");
    // One namespace across kinds: a deal proposing "acme" gets the next free one.
    let (_, v) = t.write(&a, "wc", vec![create("deal", "dx", deal("acme", "lead", human(&a)))]).await;
    assert_eq!(v["rows"][0]["handle"], "acme-3");
    // Another org has its own.
    let o = t.sign_up("zed@example.com", "Org Z").await;
    let (_, v) = t.write(&o, "wz", vec![create("company", "cz", company("acme", "Acme"))]).await;
    assert_eq!(v["rows"][0]["handle"], "acme");
    // A handle that cannot be a handle is refused.
    for bad in ["Acme", "a b", "-x", "", "x".repeat(65).as_str()] {
        let (s, v) = t.write(&a, &format!("wh{bad}"), vec![create("company", &format!("cx{}", bad.len()), company(bad, "X"))]).await;
        assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("bad-handle")), "{bad:?}");
    }
    t.cleanup().await;
}

#[tokio::test]
async fn a_retried_write_applies_once_and_gets_the_same_answer() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let ops = vec![create("company", "c1", company("acme", "Acme")), create("company", "c2", company("beta", "Beta"))];
    let (s1, v1) = t.write(&a, "same", ops.clone()).await;
    let (s2, v2) = t.write(&a, "same", ops.clone()).await;
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK));
    assert_eq!(v1, v2);
    assert_eq!((t.count("companies").await, t.watermark(&a).await), (2, 2));
    // Racing retries too.
    let ops = vec![create("company", "c3", company("gamma", "Gamma"))];
    let (r1, r2, r3) = tokio::join!(t.write(&a, "race", ops.clone()), t.write(&a, "race", ops.clone()), t.write(&a, "race", ops.clone()));
    assert!([&r1, &r2, &r3].iter().all(|r| r.0 == StatusCode::OK && r.1 == r1.1));
    assert_eq!((t.count("companies").await, t.watermark(&a).await), (3, 3));
    // A write id belongs to its org: another org's identical id is a different write.
    let z = t.sign_up("zed@example.com", "Org Z").await;
    let (s, v) = t.write(&z, "same", vec![create("company", "c1", company("acme", "Zed's"))]).await;
    assert_eq!((s, v["seq"]["to"].clone()), (StatusCode::OK, json!(1)));
    t.cleanup().await;
}

#[tokio::test]
async fn two_writers_from_the_same_version_one_wins_and_the_other_is_told() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let b = t.post_invite(&a, "bo@example.com").await;
    t.write(&a, "w0", vec![create("deal", "d1", deal("pilot", "lead", human(&a)))]).await;
    let mut x = deal("pilot", "lead", human(&a));
    x["value"] = json!({ "amount": 100, "currency": "USD" });
    let mut y = x.clone();
    y["value"] = json!({ "amount": 200, "currency": "USD" });
    let (ra, rb) = tokio::join!(t.write(&a, "wa", vec![update("deal", "d1", 1, x)]), t.write(&b, "wb", vec![update("deal", "d1", 1, y)]));
    let codes = { let mut c = vec![ra.0, rb.0]; c.sort(); c };
    assert_eq!(codes, [StatusCode::OK, StatusCode::CONFLICT]);
    let loser = if ra.0 == StatusCode::CONFLICT { &ra.1 } else { &rb.1 };
    assert!(loser["current"]["value"]["amount"].is_i64(), "the refusal names the winner's value: {loser}");
    assert_eq!(t.watermark(&a).await, 2, "exactly one update landed");
    t.cleanup().await;
}

#[tokio::test]
async fn a_write_cannot_claim_to_be_another_person_or_another_peoples_agent() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let b = t.post_invite(&a, "bo@example.com").await;
    // Bo's agent, registered the proper way.
    let (s, ag) = t.call("POST", "/v1/agents", Some(&token(&b)), json!({ "localAgentId": "scout", "name": "Scout" })).await;
    assert_eq!(s, StatusCode::OK, "{ag}");
    let bo_agent = json!({ "kind": "agent", "id": ag["workspaceId"] });

    // Batch `by`: Ada cannot write as Bo, nor as Bo's agent, nor as an agent nobody minted.
    for forged in [human(&b), bo_agent.clone(), json!({ "kind": "agent", "id": "wa-invented" })] {
        let (s, v) = t.write_as(&a, forged.clone(), "f", vec![create("company", "c1", company("acme", "Acme"))]).await;
        assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("forged-actor")), "{forged}");
    }
    // Bo can write as Bo's own agent.
    let (s, v) = t.write_as(&b, bo_agent.clone(), "ok", vec![create("company", "c1", company("acme", "Acme"))]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, ch) = t.get("/v1/sync/changes?since=0", &token(&a)).await;
    assert_eq!(ch["ops"][0]["by"], bo_agent, "attributed to whose agent it was");

    // Inside a record: an activity, a move, a task and a deal's mover are each checked too.
    let note = |by: Value| json!({ "at": 1, "kind": "note", "body": "hi", "links": ["c1"], "by": by });
    let (s, v) = t.write(&a, "n1", vec![json!({ "kind": "activity", "op": "append", "id": "a1", "record": note(human(&b)) })]).await;
    assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("forged-actor")));
    let (s, _) = t.write(&a, "n2", vec![json!({ "kind": "activity", "op": "append", "id": "a1", "record": note(human(&a)) })]).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = t.write(&a, "n3", vec![create("deal", "d1", deal("pilot", "lead", human(&b)))]).await;
    assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("forged-actor")), "movedBy on create");
    let task = |by: Value| json!({ "handle": "call", "what": "Call", "due": "2026-10-20", "doneAt": null, "links": ["c1"], "by": by, "updatedAt": 1 });
    let (s, v) = t.write(&a, "n4", vec![create("task", "t1", task(human(&b)))]).await;
    assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("forged-actor")), "task by on create");
    let (_, made) = t.write(&a, "n5", vec![create("task", "t1", task(human(&a)))]).await;
    let ver = made["rows"][0]["version"].as_i64().unwrap();
    let (s, v) = t.write(&b, "n6", vec![update("task", "t1", ver, task(human(&b)))]).await;
    assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("forged-actor")), "who made a task cannot be changed");

    // A deal Ada moved stays hers through Bo's edit of its value — a teammate's mover is kept, not forged.
    t.write(&a, "n7", vec![create("deal", "d2", deal("big", "lead", human(&a)))]).await;
    let mut edit = deal("big", "lead", human(&a));
    edit["value"] = json!({ "amount": 5, "currency": "USD" });
    let (_, cur) = t.get("/v1/sync/bootstrap?kind=deals", &token(&b)).await;
    let dver = cur["chunk"]["rows"][0]["version"].as_i64().unwrap();
    let (s, v) = t.write(&b, "n8", vec![update("deal", "d2", dver, edit)]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, cur) = t.get("/v1/sync/bootstrap?kind=deals", &token(&b)).await;
    assert_eq!(cur["chunk"]["rows"][0]["movedBy"], human(&a));
    t.cleanup().await;
}

#[tokio::test]
async fn the_org_and_the_owner_come_from_the_session_whatever_the_record_says() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let b = t.sign_up("bo@example.com", "Org B").await;
    let mut rec = company("acme", "Acme");
    rec["orgId"] = json!(org(&b));
    rec["ownerId"] = json!(user(&b));
    rec["version"] = json!(999);
    let (s, v) = t.write(&a, "w1", vec![create("company", "c1", rec)]).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["rows"][0]["version"], 1);
    let (_, bs) = t.get("/v1/sync/bootstrap", &token(&a)).await;
    let row = &bs["chunk"]["rows"][0];
    assert_eq!((row["orgId"].clone(), row["ownerId"].clone(), row["version"].clone()), (json!(org(&a)), json!(user(&a)), json!(1)));
    assert_eq!(t.get("/v1/sync/changes?since=0", &token(&b)).await.1["ops"].as_array().unwrap().len(), 0, "B saw nothing");
    // The request itself has no org field: one is refused as an unknown field.
    let (s, _) = t.call("POST", "/v1/sync/write", Some(&token(&a)), json!({ "writeId": "x", "by": human(&a), "orgId": org(&b), "ops": [] })).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let op = json!({ "kind": "company", "op": "create", "id": "c9", "org": org(&b), "record": company("zz", "Z") });
    let (s, _) = t.call("POST", "/v1/sync/write", Some(&token(&a)), json!({ "writeId": "y", "by": human(&a), "ops": [op] })).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    t.cleanup().await;
}

/// A seat in org A cannot touch a row of org B with any id it can guess — and cannot tell it is there.
#[tokio::test]
async fn org_a_cannot_update_link_or_clash_with_org_bs_rows_and_learns_nothing() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let b = t.sign_up("bo@example.com", "Org B").await;
    t.write(&b, "wb", vec![create("company", "cb", company("beta", "Beta")), create("deal", "db", deal("bd", "lead", human(&b)))]).await;
    let nowhere = t.write(&a, "wn", vec![update("company", "ghost", 1, company("g", "G"))]).await;
    let theirs = t.write(&a, "wt", vec![update("company", "cb", 1, company("beta", "Hacked"))]).await;
    assert_eq!(theirs.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(theirs.1["code"], "unknown-record");
    assert_eq!((theirs.1["reason"].clone(), theirs.1["code"].clone()), (nowhere.1["reason"].clone(), nowhere.1["code"].clone()), "indistinguishable from a record that is not there");
    // A link to their company is a link to nothing.
    let mut contact = json!({ "handle": "k", "name": "K", "companyId": "cb", "tags": [], "updatedAt": 1 });
    let linked = t.write(&a, "wl", vec![create("contact", "k1", contact.clone())]).await;
    contact["companyId"] = json!("ghost");
    let ghost = t.write(&a, "wg", vec![create("contact", "k2", contact)]).await;
    assert_eq!((linked.0, linked.1["code"].clone(), linked.1["reason"].clone()), (ghost.0, ghost.1["code"].clone(), ghost.1["reason"].clone()));
    // A move of their deal, an activity on it: the same.
    let ev = json!({ "dealId": "db", "from": "lead", "to": "won", "at": 1, "by": human(&a) });
    assert_eq!(t.write(&a, "wm", vec![json!({ "kind": "move", "op": "append", "id": "m1", "record": ev })]).await.1["code"], "unknown-link");
    // Their row is untouched and the id may be reused in my org (ids are per org).
    let (_, bs) = t.get("/v1/sync/bootstrap", &token(&b)).await;
    assert_eq!(bs["chunk"]["rows"][0]["name"], "Beta");
    let (s, _) = t.write(&a, "wr", vec![create("company", "cb", company("beta", "Mine"))]).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(t.get("/v1/sync/bootstrap", &token(&b)).await.1["chunk"]["rows"][0]["name"], "Beta");
    t.cleanup().await;
}

#[tokio::test]
async fn a_record_stores_only_the_fields_it_names_and_never_view_state() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    for (field, value) in [("focus", json!({ "kind": "deal" })), ("pending", json!(1)), ("feed", json!([])), ("follow", json!(true)), ("page", json!("board")), ("surprise", json!(1))] {
        let mut rec = company("acme", "Acme");
        rec[field] = value;
        let (s, v) = t.write(&a, "w", vec![create("company", "c1", rec)]).await;
        assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("unknown-field")), "{field}");
    }
    // The seat-local reminder mark is dropped silently, not stored and not an error.
    let task = json!({ "handle": "t", "what": "W", "due": "2026-10-20", "doneAt": null, "links": [], "by": human(&a), "updatedAt": 1, "dueSentAt": 123 });
    let (s, _) = t.write(&a, "wt", vec![create("task", "t1", task)]).await;
    assert_eq!(s, StatusCode::OK);
    let (_, bs) = t.get("/v1/sync/bootstrap?kind=tasks", &token(&a)).await;
    assert!(bs["chunk"]["rows"][0].get("dueSentAt").is_none());
    t.cleanup().await;
}

#[tokio::test]
async fn the_rules_are_refused_with_a_reason_and_write_nothing() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    t.write(&a, "w0", vec![create("company", "c1", company("acme", "Acme"))]).await;
    let w = t.watermark(&a).await;
    let cases: Vec<(Value, &str)> = vec![
        (create("company", "c1", company("other", "Dup id")), "id-taken"),
        (create("company", "bad id!", company("x", "X")), "bad-id"),
        (json!({ "kind": "gizmo", "op": "create", "id": "g", "record": {} }), "bad-kind"),
        (json!({ "kind": "company", "op": "append", "id": "g", "record": company("g", "G") }), "bad-op"),
        (json!({ "kind": "activity", "op": "create", "id": "g", "record": {} }), "bad-op"),
        (update("company", "nope", 1, company("n", "N")), "unknown-record"),
        (create("company", "c2", json!("not an object")), "bad-record"),
        (create("company", "c2", json!({ "handle": "h", "name": "N" })), "bad-field"),
        (create("task", "t1", json!({ "handle": "t", "what": "W", "due": "tomorrow", "links": [], "by": human(&a), "updatedAt": 1 })), "bad-field"),
        (create("deal", "d1", { let mut d = deal("d", "lead", human(&a)); d["pipelineId"] = json!("other"); d }), "bad-field"),
        (create("deal", "d1", { let mut d = deal("d", "lead", human(&a)); d["status"] = json!("dead"); d }), "bad-field"),
        (create("deal", "d1", { let mut d = deal("d", "lead", human(&a)); d["value"] = json!({ "amount": "x" }); d }), "bad-field"),
        (create("deal", "d1", { let mut d = deal("d", "lead", human(&a)); d["contactIds"] = json!(["ghost"]); d }), "unknown-link"),
        (json!({ "kind": "activity", "op": "append", "id": "a1", "record": { "at": 1, "kind": "gossip", "body": "x", "links": [], "by": human(&a) } }), "bad-field"),
    ];
    for (op, code) in cases {
        let (s, v) = t.write(&a, "case", vec![op.clone()]).await;
        assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!(code)), "{op}");
        assert!(v["reason"].as_str().is_some_and(|r| !r.is_empty()));
    }
    for (ops, code) in [(vec![], "empty")] {
        let (s, v) = t.write(&a, "e", ops).await;
        assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!(code)));
    }
    assert_eq!(t.watermark(&a).await, w);
    // Not signed in.
    assert_eq!(t.call("POST", "/v1/sync/write", None, json!({ "writeId": "x", "by": human(&a), "ops": [] })).await.0, StatusCode::UNAUTHORIZED);
    t.cleanup().await;
}

#[tokio::test]
async fn a_large_import_batch_is_taken_in_one_atomic_write_past_the_default_body_limit() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    // ~400 KB of JSON: over the 64 KB every other route allows, well under this route's 8 MB.
    let ops: Vec<Value> = (0..2000).map(|n| create("company", &format!("c{n}"), company(&format!("co-{n}"), &format!("Company number {n} with a longish name")))).collect();
    let (s, v) = t.write(&a, "import", ops).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["seq"]["from"].clone(), v["seq"]["to"].clone()), (json!(1), json!(2000)));
    assert_eq!(t.count("companies").await, 2000);
    // Over the op limit: refused whole.
    let many: Vec<Value> = (0..10_001).map(|n| create("company", &format!("z{n}"), company(&format!("z-{n}"), "Z"))).collect();
    let (s, v) = t.write(&a, "too-many", many).await;
    assert_eq!((s, v["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("too-big")));
    assert_eq!(t.count("companies").await, 2000);
    // The other routes keep their small limit.
    let big = json!({ "email": "x@example.com", "password": "p".repeat(100_000), "device": "d" });
    assert_eq!(t.call("POST", "/v1/signin", None, big).await.0, StatusCode::PAYLOAD_TOO_LARGE);
    t.cleanup().await;
}
