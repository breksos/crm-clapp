//! Step 1 of p3 against a real PostgreSQL: the tables, `bootstrap`, `changes`, and the tenancy walls
//! around them. Nothing writes rows through the API yet, so the rows are put in with SQL.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crm_server::sync::StreamTiming;
use crm_server::{App, Config};
use futures_util::StreamExt;
use std::time::Duration;
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

async fn start_with(timing: StreamTiming) -> T {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must point at a PostgreSQL the tests may create schemas in");
    let schema = format!("s_{}_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst), crm_server::now());
    let (admin, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.expect("connect");
    tokio::spawn(conn);
    admin.batch_execute(&format!("CREATE SCHEMA {schema}")).await.unwrap();
    let mut pg = tokio_postgres::Config::from_str(&url).unwrap();
    pg.options(format!("-c search_path={schema}"));
    let mgr = deadpool_postgres::Manager::new(pg, tokio_postgres::NoTls);
    let pool = deadpool_postgres::Pool::builder(mgr).max_size(8).build().unwrap();
    let app = App::new(pool.clone(), Config::from_env()).await.map_err(|e| format!("{e:?}")).unwrap().with_stream_timing(timing);
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



fn fast() -> StreamTiming {
    StreamTiming { heartbeat: Duration::from_millis(400), poll: Duration::from_millis(100), coalesce: Duration::from_millis(20) }
}
fn slow() -> StreamTiming {
    StreamTiming { heartbeat: Duration::from_secs(30), poll: Duration::from_secs(30), coalesce: Duration::from_millis(20) }
}
async fn start() -> T {
    start_with(fast()).await
}

fn token(s: &Value) -> String {
    s["token"].as_str().unwrap().to_string()
}

#[derive(Debug, PartialEq)]
enum Ev {
    Event(String, Value),
    Closed,
    Quiet,
}

struct Stream {
    body: axum::body::BodyDataStream,
    buf: String,
}

impl Stream {
    async fn next(&mut self, within: Duration) -> Ev {
        loop {
            if let Some(i) = self.buf.find("\n\n") {
                let frame: String = self.buf.drain(..i + 2).collect();
                let name = frame.lines().find_map(|l| l.strip_prefix("event: ")).unwrap_or("").to_string();
                let data = frame.lines().find_map(|l| l.strip_prefix("data: ")).unwrap_or("null");
                return Ev::Event(name, serde_json::from_str(data).unwrap());
            }
            match tokio::time::timeout(within, self.body.next()).await {
                Err(_) => return Ev::Quiet,
                Ok(None) => return Ev::Closed,
                Ok(Some(Err(_))) => return Ev::Closed,
                Ok(Some(Ok(b))) => self.buf.push_str(&String::from_utf8_lossy(&b)),
            }
        }
    }
    async fn mark(&mut self, within: Duration) -> i64 {
        match self.next(within).await {
            Ev::Event(n, v) if n == "mark" => v["watermark"].as_i64().unwrap(),
            other => panic!("wanted a mark, got {other:?}"),
        }
    }
}

impl T {
    async fn open(&self, who: &Value) -> Stream {
        self.open_raw("/v1/sync/stream", Some(&token(who))).await.1.expect("a stream")
    }
    async fn open_raw(&self, path: &str, token: Option<&str>) -> (StatusCode, Option<Stream>) {
        let mut req = Request::builder().uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let res = self.app.clone().router().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        if status != StatusCode::OK {
            return (status, None);
        }
        assert_eq!(res.headers()["content-type"], "text/event-stream");
        assert_eq!(res.headers()["cache-control"], "no-cache");
        assert_eq!(res.headers()["x-accel-buffering"], "no");
        (status, Some(Stream { body: res.into_body().into_data_stream(), buf: String::new() }))
    }
    async fn write(&self, who: &Value, id: &str, n: &str) -> Value {
        let by = json!({ "kind": "human", "id": who["person"]["id"] });
        let rec = json!({ "handle": n, "name": n, "tags": [], "updatedAt": 1, "archivedAt": null });
        let (s, v) = self.call("POST", "/v1/sync/write", Some(&token(who)), json!({ "writeId": id, "by": by, "ops": [{ "kind": "company", "op": "create", "id": id, "record": rec }] })).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }
}

const WAIT: Duration = Duration::from_secs(3);

#[tokio::test]
async fn the_first_event_is_a_mark_with_the_current_watermark_and_a_write_pushes_the_next() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    t.write(&a, "c1", "one").await;
    let mut s = t.open(&a).await;
    assert_eq!(s.mark(WAIT).await, 1, "a reconnect catches up at once");
    t.write(&a, "c2", "two").await;
    assert_eq!(s.mark(WAIT).await, 2);
    // Every event carries a number and nothing else.
    t.write(&a, "c3", "three").await;
    match s.next(WAIT).await {
        Ev::Event(name, v) => {
            assert_eq!(name, "mark");
            assert_eq!(v, json!({ "watermark": 3 }));
        }
        other => panic!("{other:?}"),
    }
    t.cleanup().await;
}

#[tokio::test]
async fn a_burst_of_writes_is_coalesced_into_few_marks_ending_at_the_latest() {
    let t = start_with(StreamTiming { heartbeat: Duration::from_secs(30), poll: Duration::from_secs(30), coalesce: Duration::from_millis(300) }).await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let mut s = t.open(&a).await;
    assert_eq!(s.mark(WAIT).await, 0);
    for n in 0..5 {
        t.write(&a, &format!("c{n}"), &format!("n{n}")).await;
    }
    let mut last = 0;
    let mut count = 0;
    while let Ev::Event(_, v) = s.next(Duration::from_millis(900)).await {
        last = v["watermark"].as_i64().unwrap();
        count += 1;
    }
    assert_eq!(last, 5);
    assert!(count <= 2, "five writes inside the window should not be five marks: {count}");
    t.cleanup().await;
}

#[tokio::test]
async fn an_idle_stream_still_carries_the_watermark_every_heartbeat() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    t.write(&a, "c1", "one").await;
    let mut s = t.open(&a).await;
    assert_eq!(s.mark(WAIT).await, 1);
    // Nothing happens; the heartbeat says so.
    assert_eq!(s.mark(Duration::from_millis(900)).await, 1);
    assert_eq!(s.mark(Duration::from_millis(900)).await, 1);
    t.cleanup().await;
}

#[tokio::test]
async fn signing_out_or_expiring_ends_the_stream_with_bye_within_a_poll() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let mut s = t.open(&a).await;
    s.mark(WAIT).await;
    t.call("POST", "/v1/signout", Some(&token(&a)), Value::Null).await;
    assert_eq!(s.next(WAIT).await, Ev::Event("bye".into(), json!({ "reason": "signed-out" })));
    assert_eq!(s.next(WAIT).await, Ev::Closed);
    // A new request with the revoked token is refused outright.
    assert_eq!(t.open_raw("/v1/sync/stream", Some(&token(&a))).await.0, StatusCode::UNAUTHORIZED);

    let b = t.sign_up("bo@example.com", "Org B").await;
    let mut s = t.open(&b).await;
    s.mark(WAIT).await;
    t.pool.get().await.unwrap().execute("UPDATE sessions SET expires_at = 1", &[]).await.unwrap();
    assert_eq!(s.next(WAIT).await, Ev::Event("bye".into(), json!({ "reason": "signed-out" })));
    assert_eq!(s.next(WAIT).await, Ev::Closed);
    t.cleanup().await;
}

#[tokio::test]
async fn a_seat_has_at_most_two_streams_and_a_third_closes_the_oldest() {
    let t = start_with(slow()).await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let mut s1 = t.open(&a).await;
    let mut s2 = t.open(&a).await;
    s1.mark(WAIT).await;
    s2.mark(WAIT).await;
    let mut s3 = t.open(&a).await;
    assert_eq!(s1.next(WAIT).await, Ev::Event("bye".into(), json!({ "reason": "superseded" })));
    assert_eq!(s1.next(WAIT).await, Ev::Closed);
    s3.mark(WAIT).await;
    // The other two live on: a write reaches both.
    t.write(&a, "c1", "one").await;
    assert_eq!(s2.mark(WAIT).await, 1);
    assert_eq!(s3.mark(WAIT).await, 1);
    // A different seat of the same person has its own allowance.
    let (_, again) = t.call("POST", "/v1/signin", None, json!({ "email": "ada@example.com", "password": "correct horse battery", "device": "phone" })).await;
    let mut o1 = t.open(&again).await;
    let mut o2 = t.open(&again).await;
    o1.mark(WAIT).await;
    o2.mark(WAIT).await;
    assert_eq!(s2.next(Duration::from_millis(300)).await, Ev::Quiet, "another seat's streams never evict mine");
    t.cleanup().await;
}

#[tokio::test]
async fn a_stream_that_goes_away_gives_its_place_back() {
    let t = start_with(slow()).await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let mut keep = t.open(&a).await;
    keep.mark(WAIT).await;
    let mut gone = t.open(&a).await;
    gone.mark(WAIT).await;
    drop(gone); // the client hung up
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut fresh = t.open(&a).await;
    fresh.mark(WAIT).await;
    assert_eq!(keep.next(Duration::from_millis(300)).await, Ev::Quiet, "the survivor was not evicted: two places, two streams");
    t.cleanup().await;
}

#[tokio::test]
async fn only_this_orgs_marks_reach_this_orgs_streams() {
    let t = start_with(slow()).await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let b = t.sign_up("bo@example.com", "Org B").await;
    let mut sa = t.open(&a).await;
    let mut sb = t.open(&b).await;
    assert_eq!((sa.mark(WAIT).await, sb.mark(WAIT).await), (0, 0));
    t.write(&b, "c1", "one").await;
    assert_eq!(sb.mark(WAIT).await, 1);
    assert_eq!(sa.next(Duration::from_millis(500)).await, Ev::Quiet, "org A heard nothing of org B's write");
    t.cleanup().await;
}

#[tokio::test]
async fn the_token_is_read_from_the_header_only() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let tok = token(&a);
    for path in [format!("/v1/sync/stream?token={tok}"), format!("/v1/sync/stream?access_token={tok}")] {
        assert_eq!(t.open_raw(&path, None).await.0, StatusCode::UNAUTHORIZED, "{path}");
    }
    assert_eq!(t.open_raw("/v1/sync/stream", None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(t.open_raw("/v1/sync/stream", Some("nope")).await.0, StatusCode::UNAUTHORIZED);
    // A stray token parameter beside the real header is ignored — and an unknown parameter is refused.
    let (s, st) = t.open_raw("/v1/sync/stream?token=junk", Some(&tok)).await;
    assert_eq!(s, StatusCode::OK);
    drop(st);
    assert_eq!(t.open_raw("/v1/sync/stream?org=x", Some(&tok)).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(t.open_raw("/v1/sync/stream?since=0", Some(&tok)).await.0, StatusCode::BAD_REQUEST);
    t.cleanup().await;
}

#[tokio::test]
async fn nothing_but_a_watermark_ever_travels_on_the_stream() {
    let t = start().await;
    let a = t.sign_up("ada@example.com", "Org A").await;
    let mut s = t.open(&a).await;
    t.write(&a, "c1", "secretive").await;
    let mut seen = String::new();
    for _ in 0..4 {
        if let Ev::Event(n, v) = s.next(Duration::from_millis(600)).await {
            assert_eq!(n, "mark");
            assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["watermark"]);
            seen.push_str(&v.to_string());
        }
    }
    assert!(!seen.contains("secretive") && !seen.contains("ada") && !seen.contains(&tok_free(&a)), "{seen}");
    t.cleanup().await;
}
fn tok_free(a: &Value) -> String {
    token(a)
}
