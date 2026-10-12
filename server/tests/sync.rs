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
    async fn sql(&self, q: &str) {
        self.pool.get().await.unwrap().batch_execute(q).await.unwrap();
    }
    async fn cleanup(self) {
        drop(self.pool);
        let _ = self.admin.batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema)).await;
    }
}

fn token(s: &Value) -> String {
    s["token"].as_str().unwrap().to_string()
}
fn org(s: &Value) -> String {
    s["org"]["id"].as_str().unwrap().to_string()
}

fn company(org: &str, id: &str, handle: &str, name: &str, version: i64) -> String {
    format!(
        "INSERT INTO companies (org_id,id,handle,version,owner_id,updated_at,fields) VALUES ('{org}','{id}','{handle}',{version},'u',1,'{{\"name\":\"{name}\",\"tags\":[]}}');
         INSERT INTO handles (org_id,handle,kind,id) VALUES ('{org}','{handle}','company','{id}');"
    )
}

fn op(org: &str, seq: i64, id: &str) -> String {
    format!("INSERT INTO ops (org_id,seq,kind,id,op,record,by,at,write_id) VALUES ('{org}',{seq},'company','{id}','upsert','{{\"id\":\"{id}\"}}','{{\"kind\":\"human\",\"id\":\"u\"}}',1,'w{seq}');")
}

/// No response of this milestone may carry anything that belongs to one seat's view.
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
async fn a_new_org_gets_its_pipeline_and_a_zero_watermark_and_every_sync_table_has_an_org() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let (s, b) = t.get("/v1/sync/bootstrap", &token(&a)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["watermark"], 0);
    assert_eq!(b["pipeline"]["stages"], json!(["lead", "qualified", "proposal", "negotiation"]));
    assert_eq!(b["members"].as_array().unwrap().len(), 1);
    assert_eq!(b["members"][0]["name"], "ada");
    assert_eq!(b["chunk"]["kind"], "companies");
    assert_eq!(b["chunk"]["next"], json!({ "kind": "contacts", "after": null }));
    let rows = t
        .admin
        .query(
            &format!("SELECT table_name FROM information_schema.columns WHERE table_schema='{}' AND column_name='org_id'", t.schema),
            &[],
        )
        .await
        .unwrap();
    let names: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
    for table in ["handles", "companies", "contacts", "deals", "tasks", "activities", "moves", "ops", "writes", "pipelines"] {
        assert!(names.iter().any(|n| n == table), "{table} has no org_id");
    }
    t.cleanup().await;
}

#[tokio::test]
async fn bootstrap_pages_every_kind_in_order_with_the_split_row_shape() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let o = org(&a);
    for n in 0..1100 {
        t.sql(&company(&o, &format!("c{n:05}"), &format!("co-{n}"), "Acme", 7)).await;
    }
    t.sql(&format!(
        "INSERT INTO contacts (org_id,id,handle,version,owner_id,updated_at,company_id,fields) VALUES ('{o}','k1','ada',8,'u',2,'c00001','{{\"name\":\"Ada\",\"email\":\"a@b.c\"}}');
         INSERT INTO deals (org_id,id,handle,version,owner_id,updated_at,company_id,stage,status,moved_by,moved_at,opened_at,pipeline_id,fields)
              VALUES ('{o}','d1','pilot',9,'u',3,'c00001','lead','open','{{\"kind\":\"human\",\"id\":\"u\"}}',3,3,'sales','{{\"title\":\"Pilot\",\"contactIds\":[\"k1\"]}}');
         INSERT INTO tasks (org_id,id,handle,version,owner_id,updated_at,due,by,fields) VALUES ('{o}','t1','call-back',10,'u',4,'2026-10-15','{{\"kind\":\"human\",\"id\":\"u\"}}','{{\"what\":\"Call back\",\"links\":[\"d1\"]}}');
         INSERT INTO activities (org_id,id,owner_id,at,by,kind,body,links) VALUES ('{o}','a1','u',5,'{{\"kind\":\"human\",\"id\":\"u\"}}','note','hello','[\"d1\"]');
         INSERT INTO moves (org_id,id,deal_id,at,by,from_stage,to_stage) VALUES ('{o}','m1','d1',6,'{{\"kind\":\"human\",\"id\":\"u\"}}','lead','qualified');"
    ))
    .await;

    // Walk the cursor chain exactly as a client would.
    let mut seen: Vec<(String, usize)> = Vec::new();
    let mut path = "/v1/sync/bootstrap".to_string();
    let mut sample = std::collections::HashMap::new();
    loop {
        let (s, v) = t.get(&path, &token(&a)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_no_view_state(&v);
        let kind = v["chunk"]["kind"].as_str().unwrap().to_string();
        let rows = v["chunk"]["rows"].as_array().unwrap();
        assert!(rows.len() <= 500);
        seen.push((kind.clone(), rows.len()));
        if let Some(r) = rows.first() {
            sample.entry(kind.clone()).or_insert_with(|| r.clone());
        }
        match &v["chunk"]["next"] {
            Value::Null => break,
            n => {
                path = match n["after"].as_str() {
                    Some(after) => format!("/v1/sync/bootstrap?kind={}&after={after}", n["kind"].as_str().unwrap()),
                    None => format!("/v1/sync/bootstrap?kind={}", n["kind"].as_str().unwrap()),
                }
            }
        }
    }
    let companies: usize = seen.iter().filter(|(k, _)| k == "companies").map(|(_, n)| n).sum();
    assert_eq!(companies, 1100, "{seen:?}");
    assert_eq!(seen.iter().filter(|(k, _)| k == "companies").count(), 3, "500 + 500 + 100");
    let order: Vec<&str> = seen.iter().map(|(k, _)| k.as_str()).fold(Vec::new(), |mut acc, k| {
        if acc.last() != Some(&k) {
            acc.push(k);
        }
        acc
    });
    assert_eq!(order, ["companies", "contacts", "deals", "tasks", "activities", "moves"]);

    assert_eq!(sample["companies"]["name"], "Acme");
    assert_eq!(sample["companies"]["version"], 7);
    assert_eq!(sample["companies"]["orgId"], o);
    assert_eq!(sample["contacts"]["companyId"], "c00001");
    assert_eq!(sample["deals"]["stage"], "lead");
    assert_eq!(sample["deals"]["contactIds"], json!(["k1"]));
    assert_eq!(sample["deals"]["movedBy"]["id"], "u");
    assert_eq!(sample["tasks"]["due"], "2026-10-15");
    assert_eq!(sample["tasks"]["doneAt"], Value::Null);
    assert_eq!(sample["activities"]["body"], "hello");
    assert_eq!(sample["moves"]["to"], "qualified");
    t.cleanup().await;
}

#[tokio::test]
async fn changes_returns_ops_in_order_pages_by_limit_and_says_stale_when_it_cannot_reach() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let o = org(&a);
    for n in 1..=10 {
        t.sql(&op(&o, n, &format!("c{n}"))).await;
    }
    t.sql(&format!("UPDATE orgs SET watermark = 10 WHERE id = '{o}'")).await;

    let (s, v) = t.get("/v1/sync/changes?since=0", &token(&a)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["watermark"].clone(), v["from"].clone(), v["to"].clone()), (json!(10), json!(0), json!(10)));
    let seqs: Vec<i64> = v["ops"].as_array().unwrap().iter().map(|o| o["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, (1..=10).collect::<Vec<_>>());
    assert_eq!(v["ops"][0]["op"], "upsert");
    assert_eq!(v["ops"][0]["by"]["id"], "u");

    let (_, v) = t.get("/v1/sync/changes?since=3&limit=4", &token(&a)).await;
    assert_eq!((v["from"].clone(), v["to"].clone()), (json!(3), json!(7)));
    let (_, v) = t.get("/v1/sync/changes?since=10", &token(&a)).await;
    assert_eq!((v["ops"].as_array().unwrap().len(), v["to"].clone()), (0, json!(10)), "caught up: nothing, and `to` stays");

    // Ahead of the org: not this org's history.
    let (_, v) = t.get("/v1/sync/changes?since=11", &token(&a)).await;
    assert_eq!(v, json!({ "stale": true, "watermark": 10 }));
    // The log was trimmed: it starts at 6, so 4 is out of reach but 5 is not.
    t.sql(&format!("DELETE FROM ops WHERE org_id='{o}' AND seq < 6; UPDATE orgs SET retained_from = 6 WHERE id='{o}'")).await;
    let (_, v) = t.get("/v1/sync/changes?since=4", &token(&a)).await;
    assert_eq!(v["stale"], true);
    let (_, v) = t.get("/v1/sync/changes?since=5", &token(&a)).await;
    assert_eq!(v["ops"].as_array().unwrap().len(), 5);

    for bad in ["/v1/sync/changes", "/v1/sync/changes?since=-1", "/v1/sync/changes?since=x"] {
        assert_eq!(t.get(bad, &token(&a)).await.0, StatusCode::BAD_REQUEST, "{bad}");
    }
    t.cleanup().await;
}

/// A seat in org A cannot read org B's rows, ops or members through either endpoint — and cannot name
/// another org to try.
#[tokio::test]
async fn org_b_is_invisible_to_org_a_on_every_sync_endpoint_and_no_request_can_name_an_org() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let b = t.sign_up("bo@example.com", "Org B").await;
    let (oa, ob) = (org(&a), org(&b));
    t.sql(&company(&oa, "ca1", "alpha", "Alpha", 1)).await;
    t.sql(&company(&ob, "cb1", "beta", "Beta", 1)).await;
    t.sql(&op(&oa, 1, "ca1")).await;
    t.sql(&op(&ob, 1, "cb1")).await;
    t.sql(&format!("UPDATE orgs SET watermark = 1 WHERE id IN ('{oa}','{ob}')")).await;

    let (_, va) = t.get("/v1/sync/bootstrap", &token(&a)).await;
    let text = va.to_string();
    assert!(text.contains("alpha") && !text.contains("beta") && !text.contains("bo@") && !text.contains(&ob), "{text}");
    assert_eq!(va["members"].as_array().unwrap().len(), 1);
    let (_, ca) = t.get("/v1/sync/changes?since=0", &token(&a)).await;
    let text = ca.to_string();
    assert!(text.contains("ca1") && !text.contains("cb1"), "{text}");
    // …and B sees only B.
    let (_, cb) = t.get("/v1/sync/changes?since=0", &token(&b)).await;
    assert!(cb.to_string().contains("cb1") && !cb.to_string().contains("ca1"));

    // No parameter names an org, a person or a scope; an unknown one is refused, not honoured.
    for q in [
        format!("/v1/sync/bootstrap?org={ob}"),
        format!("/v1/sync/bootstrap?orgId={ob}"),
        format!("/v1/sync/changes?since=0&org={ob}"),
        format!("/v1/sync/changes?since=0&org_id={ob}"),
        "/v1/sync/bootstrap?scope=all".to_string(),
    ] {
        assert_eq!(t.get(&q, &token(&a)).await.0, StatusCode::BAD_REQUEST, "{q}");
    }
    // A cursor that points at another org's id yields nothing of theirs (ids are looked up inside the org).
    let (_, v) = t.get("/v1/sync/bootstrap?kind=companies&after=cb0", &token(&a)).await;
    assert!(!v.to_string().contains("beta"));
    t.cleanup().await;
}

#[tokio::test]
async fn a_token_in_the_query_string_authenticates_nothing_and_is_never_read() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let tok = token(&a);
    for path in [
        format!("/v1/sync/bootstrap?token={tok}"),
        format!("/v1/sync/changes?since=0&token={tok}"),
        format!("/v1/sync/changes?since=0&access_token={tok}"),
    ] {
        let (s, _) = t.call("GET", &path, None, Value::Null).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{path}");
    }
    // Present as well as the header: ignored, not an error and not a credential.
    let (s, _) = t.call("GET", "/v1/sync/changes?since=0&token=junk", Some(&tok), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    // No header, a wrong header, a revoked one: 401 on both endpoints.
    for p in ["/v1/sync/bootstrap", "/v1/sync/changes?since=0"] {
        assert_eq!(t.call("GET", p, None, Value::Null).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(t.call("GET", p, Some("nope"), Value::Null).await.0, StatusCode::UNAUTHORIZED);
    }
    t.call("POST", "/v1/signout", Some(&tok), Value::Null).await;
    assert_eq!(t.get("/v1/sync/bootstrap", &tok).await.0, StatusCode::UNAUTHORIZED);
    t.cleanup().await;
}

/// The one-namespace handle rule is a database constraint, not a check somebody can forget.
#[tokio::test]
async fn a_handle_is_unique_per_org_across_kinds_by_constraint() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let b = t.sign_up("bo@example.com", "Org B").await;
    let (oa, ob) = (org(&a), org(&b));
    t.sql(&format!("INSERT INTO handles (org_id,handle,kind,id) VALUES ('{oa}','acme','company','c1')")).await;
    let db = t.pool.get().await.unwrap();
    let dup = db.execute(&format!("INSERT INTO handles (org_id,handle,kind,id) VALUES ('{oa}','acme','deal','d1')"), &[]).await;
    assert!(dup.is_err(), "two kinds, one handle, one org");
    db.execute(&format!("INSERT INTO handles (org_id,handle,kind,id) VALUES ('{ob}','acme','company','c9')"), &[]).await.expect("another org may use it");
    drop(db);
    t.cleanup().await;
}

// MARK: - Retention

#[tokio::test]
async fn retention_trims_old_ops_beyond_the_newest_fifty_thousand_and_says_stale_before_them() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let o = org(&a);
    // 60 000 ops, the first 59 000 ancient and the last 1 000 recent.
    t.sql(&format!(
        "INSERT INTO ops (org_id,seq,kind,id,op,record,by,at,write_id)
         SELECT '{o}', n, 'company', 'c'||n, 'upsert', '{{}}', '{{\"kind\":\"human\",\"id\":\"u\"}}', CASE WHEN n <= 59000 THEN 1 ELSE (extract(epoch from now())*1000)::bigint END, 'w'||n
           FROM generate_series(1, 60000) n;
         UPDATE orgs SET watermark = 60000 WHERE id = '{o}';"
    ))
    .await;
    let deleted = crm_server::sync::retention_pass(&t.pool).await.unwrap();
    // Ancient AND older than the newest 50 000 (seq <= 10 000): only those go. The ancient ones inside the
    // newest 50 000 stay, and nothing recent is touched.
    assert_eq!(deleted, Some(10_000));
    let (_, v) = t.get("/v1/sync/changes?since=9999", &token(&a)).await;
    assert_eq!(v["stale"], true, "9 999 is before what is held");
    let (_, v) = t.get("/v1/sync/changes?since=10000&limit=3", &token(&a)).await;
    assert_eq!(v["ops"][0]["seq"], 10_001);
    let kept: i64 = t.pool.get().await.unwrap().query_one(&format!("SELECT count(*) FROM ops WHERE org_id='{o}'"), &[]).await.unwrap().get(0);
    assert_eq!(kept, 50_000);
    // A second pass has nothing to do.
    assert_eq!(crm_server::sync::retention_pass(&t.pool).await.unwrap(), Some(0));
    t.cleanup().await;
}

#[tokio::test]
async fn retention_keeps_recent_ops_however_many_and_forgets_old_write_ids() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let o = org(&a);
    t.sql(&format!(
        "INSERT INTO ops (org_id,seq,kind,id,op,record,by,at,write_id)
         SELECT '{o}', n, 'company', 'c'||n, 'upsert', '{{}}', '{{}}', (extract(epoch from now())*1000)::bigint, 'w'||n FROM generate_series(1, 60000) n;
         UPDATE orgs SET watermark = 60000 WHERE id = '{o}';
         INSERT INTO writes (org_id, write_id, seq_from, seq_to, answer, at) VALUES ('{o}','old',1,1,'{{}}',1), ('{o}','new',2,2,'{{}}', extract(epoch from now())::bigint);"
    ))
    .await;
    assert_eq!(crm_server::sync::retention_pass(&t.pool).await.unwrap(), Some(0), "nothing recent is deleted, even beyond 50 000");
    let writes: Vec<String> = t.pool.get().await.unwrap().query("SELECT write_id FROM writes ORDER BY write_id", &[]).await.unwrap().iter().map(|r| r.get(0)).collect();
    assert_eq!(writes, ["new"], "an idempotency record is kept for 24 h and no longer");
    t.cleanup().await;
}

#[tokio::test]
async fn only_one_process_does_the_retention_pass() {
    let t = start().await;
    let other = t.pool.get().await.unwrap();
    // Another process holds the housekeeping lock: this pass is skipped, not queued.
    let got: bool = other.query_one("SELECT pg_try_advisory_lock($1)", &[&0x6372_6d5f_7265_7421_i64]).await.unwrap().get(0);
    assert!(got);
    assert_eq!(crm_server::sync::retention_pass(&t.pool).await.unwrap(), None);
    other.query_one("SELECT pg_advisory_unlock($1)", &[&0x6372_6d5f_7265_7421_i64]).await.unwrap();
    drop(other);
    assert_eq!(crm_server::sync::retention_pass(&t.pool).await.unwrap(), Some(0));
    t.cleanup().await;
}
