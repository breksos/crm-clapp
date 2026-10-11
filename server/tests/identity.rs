//! The server against a **real PostgreSQL** (`DATABASE_URL`). Each test gets its own schema, so
//! they run in parallel and leave nothing behind.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crm_server::{hash_token, App, Config};
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

async fn start(config: Config) -> T {
    let url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point at a PostgreSQL the tests may create schemas in (CI provides one)");
    let schema = format!("t_{}_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst), crm_server::now());
    let (admin, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await.expect("connect");
    tokio::spawn(conn);
    admin.batch_execute(&format!("CREATE SCHEMA {schema}")).await.unwrap();
    let mut pg = tokio_postgres::Config::from_str(&url).unwrap();
    pg.options(format!("-c search_path={schema}"));
    let mgr = deadpool_postgres::Manager::new(pg, tokio_postgres::NoTls);
    let pool = deadpool_postgres::Pool::builder(mgr).max_size(8).build().unwrap();
    let app = App::new(pool.clone(), config).await.map_err(|e| format!("{e:?}")).unwrap();
    T { app, pool, schema, admin }
}

fn cfg() -> Config {
    // Tests tell the server who is calling, so the per-address limit can be exercised.
    Config { require_tls: false, trusted_hops: 1 }
}

/// One request as the network would deliver it: from `peer` (the connection's own address), with
/// an optional `X-Forwarded-For` the caller wrote or a proxy appended to.
async fn raw(app: &App, method: &str, path: &str, body: Value, peer: Option<&str>, xff: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::builder().method(method).uri(path).header("content-type", "application/json");
    if let Some(x) = xff {
        req = req.header("x-forwarded-for", x);
    }
    let mut req = req.body(Body::from(body.to_string())).unwrap();
    if let Some(p) = peer {
        let addr = std::net::SocketAddr::new(p.parse().unwrap(), 40000);
        req.extensions_mut().insert(axum::extract::ConnectInfo(addr));
    }
    let res = app.clone().router().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn wrong(email: &str) -> Value {
    json!({ "email": email, "password": "wrong wrong wrong", "device": "x" })
}

impl T {
    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Value, ip: &str) -> (StatusCode, Value, String) {
        let mut req = Request::builder().method(method).uri(path).header("content-type", "application/json").header("x-forwarded-for", ip);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let res = self.app.clone().router().oneshot(req.body(Body::from(body.to_string())).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).to_string();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null), text)
    }
    async fn post(&self, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        let (s, v, _) = self.call("POST", path, token, body, "10.0.0.1").await;
        (s, v)
    }
    async fn get(&self, path: &str, token: &str) -> (StatusCode, Value) {
        let (s, v, _) = self.call("GET", path, Some(token), Value::Null, "10.0.0.1").await;
        (s, v)
    }
    async fn sign_up(&self, email: &str, org: &str) -> Value {
        let (s, v) = self.post("/v1/signup", None, json!({ "email": email, "password": "correct horse battery", "name": email.split('@').next().unwrap(), "orgName": org, "device": "test mac" })).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }
    async fn invite_and_accept(&self, inviter_token: &str, email: &str, name: &str) -> Value {
        let (s, inv) = self.post("/v1/invites", Some(inviter_token), json!({})).await;
        assert_eq!(s, StatusCode::OK, "{inv}");
        let (s, v) = self.post("/v1/invites/accept", None, json!({ "token": inv["token"], "email": email, "password": "another long password", "name": name, "device": "test pc" })).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }
    async fn cleanup(self) {
        drop(self.pool);
        let _ = self.admin.batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema)).await;
    }
}

#[tokio::test]
async fn sign_up_gives_an_org_a_person_a_seat_and_a_token_and_sign_out_revokes_it_on_the_server() {
    let t = start(cfg()).await;
    let s = t.sign_up("ada@example.com", "Analytical Engines").await;
    let token = s["token"].as_str().unwrap().to_string();
    assert_eq!(s["person"]["name"], "ada");
    assert_eq!(s["org"]["name"], "Analytical Engines");
    assert_eq!(s["role"], "owner");

    let (st, me) = t.get("/v1/me", &token).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["person"]["id"], s["person"]["id"]);
    assert_eq!(me["seatId"], s["seatId"]);

    let (st, _, _) = t.call("POST", "/v1/signout", Some(&token), Value::Null, "10.0.0.1").await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    // Revoked server-side: the very same token is now worthless, anywhere.
    assert_eq!(t.get("/v1/me", &token).await.0, StatusCode::UNAUTHORIZED);
    t.cleanup().await;
}

#[tokio::test]
async fn an_expired_session_stops_working() {
    let t = start(cfg()).await;
    let token = t.sign_up("ada@example.com", "Org").await["token"].as_str().unwrap().to_string();
    t.admin.execute(&format!("UPDATE {}.sessions SET expires_at = 1", t.schema), &[]).await.unwrap();
    assert_eq!(t.get("/v1/me", &token).await.0, StatusCode::UNAUTHORIZED);
    t.cleanup().await;
}

#[tokio::test]
async fn a_wrong_email_and_a_wrong_password_are_indistinguishable() {
    let t = start(cfg()).await;
    t.sign_up("ada@example.com", "Org").await;
    let (s1, _, b1) = t.call("POST", "/v1/signin", None, json!({ "email": "nobody@example.com", "password": "correct horse battery" }), "10.0.0.2").await;
    let (s2, _, b2) = t.call("POST", "/v1/signin", None, json!({ "email": "ada@example.com", "password": "not the password" }), "10.0.0.3").await;
    assert_eq!(s1, StatusCode::UNAUTHORIZED);
    assert_eq!(s1, s2);
    assert_eq!(b1, b2, "the body must not say which half was wrong");
    assert!(b1.contains("wrong email or password"));
    t.cleanup().await;
}

#[tokio::test]
async fn sign_in_works_and_each_sign_in_is_its_own_seat_and_session() {
    let t = start(cfg()).await;
    let up = t.sign_up("ada@example.com", "Org").await;
    let (s, a) = t.post("/v1/signin", None, json!({ "email": "ADA@example.com ", "password": "correct horse battery", "device": "laptop" })).await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_ne!(a["seatId"], up["seatId"]);
    assert_ne!(a["token"], up["token"]);
    assert_eq!(a["person"]["id"], up["person"]["id"]);
    // Signing out one seat leaves the other signed in.
    t.call("POST", "/v1/signout", Some(a["token"].as_str().unwrap()), Value::Null, "10.0.0.1").await;
    assert_eq!(t.get("/v1/me", up["token"].as_str().unwrap()).await.0, StatusCode::OK);
    t.cleanup().await;
}

#[tokio::test]
async fn login_is_rate_limited_per_account_even_for_the_right_password_and_per_address() {
    let t = start(cfg()).await;
    t.sign_up("ada@example.com", "Org").await;
    for _ in 0..5 {
        let (s, _, _) = t.call("POST", "/v1/signin", None, json!({ "email": "ada@example.com", "password": "wrong wrong wrong" }), "10.9.9.9").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    // The account is now shut — from any address, and for the right password too.
    let (s, _, b) = t.call("POST", "/v1/signin", None, json!({ "email": "ada@example.com", "password": "correct horse battery" }), "10.1.1.1").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{b}");

    // One address guessing at many accounts hits the address limit.
    let ip = "10.7.7.7";
    for n in 0..20 {
        t.call("POST", "/v1/signin", None, json!({ "email": format!("user{n}@example.com"), "password": "wrong wrong wrong" }), ip).await;
    }
    let (s, _, _) = t.call("POST", "/v1/signin", None, json!({ "email": "fresh@example.com", "password": "wrong wrong wrong" }), ip).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    t.cleanup().await;
}

#[tokio::test]
async fn a_successful_sign_in_forgives_the_accounts_earlier_failures() {
    let t = start(cfg()).await;
    t.sign_up("ada@example.com", "Org").await;
    for _ in 0..3 {
        t.call("POST", "/v1/signin", None, json!({ "email": "ada@example.com", "password": "wrong wrong wrong" }), "10.2.2.2").await;
    }
    let (s, _) = t.post("/v1/signin", None, json!({ "email": "ada@example.com", "password": "correct horse battery" })).await;
    assert_eq!(s, StatusCode::OK);
    for _ in 0..4 {
        t.call("POST", "/v1/signin", None, json!({ "email": "ada@example.com", "password": "wrong wrong wrong" }), "10.3.3.3").await;
    }
    let (s, _) = t.post("/v1/signin", None, json!({ "email": "ada@example.com", "password": "correct horse battery" })).await;
    assert_eq!(s, StatusCode::OK, "the three before the success no longer count");
    t.cleanup().await;
}

#[tokio::test]
async fn the_password_policy_is_enforced() {
    let t = start(cfg()).await;
    let short = t.post("/v1/signup", None, json!({ "email": "a@example.com", "password": "short", "name": "A", "orgName": "O" })).await;
    assert_eq!(short.0, StatusCode::BAD_REQUEST);
    let long = "x".repeat(100); // "no maximum below 64"
    let ok = t.post("/v1/signup", None, json!({ "email": "b@example.com", "password": long, "name": "B", "orgName": "O" })).await;
    assert_eq!(ok.0, StatusCode::OK);
    let huge = "x".repeat(300);
    let too = t.post("/v1/signup", None, json!({ "email": "c@example.com", "password": huge, "name": "C", "orgName": "O" })).await;
    assert_eq!(too.0, StatusCode::BAD_REQUEST);
    let (s, _) = t.post("/v1/signin", None, json!({ "email": "b@example.com", "password": long })).await;
    assert_eq!(s, StatusCode::OK, "a 100-character password signs in");
    let bad_mail = t.post("/v1/signup", None, json!({ "email": "not-an-email", "password": "correct horse battery", "name": "D", "orgName": "O" })).await;
    assert_eq!(bad_mail.0, StatusCode::BAD_REQUEST);
    t.cleanup().await;
}

#[tokio::test]
async fn signing_up_twice_with_one_address_is_refused_and_leaves_no_orphan_org() {
    let t = start(cfg()).await;
    t.sign_up("ada@example.com", "First").await;
    let (s, _) = t.post("/v1/signup", None, json!({ "email": "Ada@Example.com", "password": "correct horse battery", "name": "Other", "orgName": "Second" })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let orgs: i64 = t.admin.query_one(&format!("SELECT count(*) FROM {}.orgs", t.schema), &[]).await.unwrap().get(0);
    assert_eq!(orgs, 1, "the failed sign-up's org was rolled back");
    t.cleanup().await;
}

/// A leaked database must not be a leaked set of sessions, invites or passwords.
#[tokio::test]
async fn passwords_sessions_and_invites_are_hashed_at_rest() {
    let t = start(cfg()).await;
    let up = t.sign_up("ada@example.com", "Org").await;
    let token = up["token"].as_str().unwrap().to_string();
    let (_, inv) = t.post("/v1/invites", Some(&token), json!({})).await;
    let invite_token = inv["token"].as_str().unwrap().to_string();

    // Every text column of every table, read back as the database holds it.
    let tables = ["orgs", "users", "memberships", "invites", "seats", "sessions", "agent_idents", "login_attempts"];
    let mut everything = String::new();
    for table in tables {
        let rows = t.admin.query(&format!("SELECT t::text FROM {}.{table} t", t.schema), &[]).await.unwrap();
        for r in rows {
            everything.push_str(&r.get::<_, String>(0));
            everything.push('\n');
        }
    }
    for secret in [token.as_str(), invite_token.as_str(), "correct horse battery"] {
        assert!(!everything.contains(secret), "a secret is stored in the clear");
    }
    assert!(everything.contains(&hash_token(&token)), "the session is stored as its hash");
    assert!(everything.contains(&hash_token(&invite_token)), "the invite is stored as its hash");
    let phc: String = t.admin.query_one(&format!("SELECT password_hash FROM {}.users", t.schema), &[]).await.unwrap().get(0);
    assert!(phc.starts_with("$argon2id$"), "argon2id: {phc}");
    t.cleanup().await;
}

#[tokio::test]
async fn an_invite_is_single_use_expiring_and_lands_in_the_inviters_org() {
    let t = start(cfg()).await;
    let owner = t.sign_up("ada@example.com", "Analytical").await;
    let owner_token = owner["token"].as_str().unwrap().to_string();
    let (_, inv) = t.post("/v1/invites", Some(&owner_token), json!({})).await;

    let body = |email: &str| json!({ "token": inv["token"], "email": email, "password": "another long password", "name": "Grace", "device": "pc" });
    let (s, joined) = t.post("/v1/invites/accept", None, body("grace@example.com")).await;
    assert_eq!(s, StatusCode::OK, "{joined}");
    assert_eq!(joined["org"]["id"], owner["org"]["id"]);
    assert_eq!(joined["role"], "member");

    let (s, again) = t.post("/v1/invites/accept", None, body("hopper@example.com")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "single use: {again}");

    // Expiry.
    let (_, inv2) = t.post("/v1/invites", Some(&owner_token), json!({})).await;
    t.admin.execute(&format!("UPDATE {}.invites SET expires_at = 1 WHERE used_at IS NULL", t.schema), &[]).await.unwrap();
    let (s, _) = t.post("/v1/invites/accept", None, json!({ "token": inv2["token"], "email": "late@example.com", "password": "another long password", "name": "Late" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A guess is not a token.
    let (s, _) = t.post("/v1/invites/accept", None, json!({ "token": "guess", "email": "x@example.com", "password": "another long password", "name": "X" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    t.cleanup().await;
}

#[tokio::test]
async fn an_invite_bound_to_an_address_is_not_burned_by_the_wrong_one() {
    let t = start(cfg()).await;
    let owner = t.sign_up("ada@example.com", "Org").await;
    let (_, inv) = t.post("/v1/invites", Some(owner["token"].as_str().unwrap()), json!({ "email": "grace@example.com" })).await;
    let go = |email: &str| json!({ "token": inv["token"], "email": email, "password": "another long password", "name": "G" });
    assert_eq!(t.post("/v1/invites/accept", None, go("mallory@example.com")).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(t.post("/v1/invites/accept", None, go("grace@example.com")).await.0, StatusCode::OK, "the right address still works");
    t.cleanup().await;
}

#[tokio::test]
async fn members_are_listed_by_name_and_id_only_and_only_within_the_org() {
    let t = start(cfg()).await;
    let ada = t.sign_up("ada@example.com", "A").await;
    let grace = t.invite_and_accept(ada["token"].as_str().unwrap(), "grace@example.com", "Grace").await;
    let other = t.sign_up("eve@example.com", "Other org").await;

    let (_, list, raw) = t.call("GET", "/v1/org/members", Some(ada["token"].as_str().unwrap()), Value::Null, "10.0.0.1").await;
    let mut names: Vec<&str> = list.as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["Grace", "ada"]);
    assert!(!raw.contains("@") && !raw.contains("role") && !raw.contains("email"), "names only: {raw}");
    let (_, theirs) = t.get("/v1/org/members", other["token"].as_str().unwrap()).await;
    assert_eq!(theirs.as_array().unwrap().len(), 1, "another org's members are not visible");
    assert_eq!(theirs[0]["name"], "eve");
    let _ = grace;
    t.cleanup().await;
}

#[tokio::test]
async fn a_workspace_agent_id_is_minted_per_user_and_stable_and_never_shared_between_seats() {
    let t = start(cfg()).await;
    let ada = t.sign_up("ada@example.com", "Org").await;
    let grace = t.invite_and_accept(ada["token"].as_str().unwrap(), "grace@example.com", "Grace").await;
    let (a_tok, g_tok) = (ada["token"].as_str().unwrap(), grace["token"].as_str().unwrap());

    // Both people's installs have an agent whose *local* id is the same number.
    let (s, a1) = t.post("/v1/agents", Some(a_tok), json!({ "localAgentId": "1789126979", "name": "Scout" })).await;
    assert_eq!(s, StatusCode::OK, "{a1}");
    let (_, g1) = t.post("/v1/agents", Some(g_tok), json!({ "localAgentId": "1789126979", "name": "Scout" })).await;
    assert_ne!(a1["workspaceId"], g1["workspaceId"], "one seat's agent must never be another seat's");

    // Stable for the same person, and a rename is a label change, not a new identity.
    let (_, a2) = t.post("/v1/agents", Some(a_tok), json!({ "localAgentId": "1789126979", "name": "Ranger" })).await;
    assert_eq!(a2["workspaceId"], a1["workspaceId"]);
    assert_eq!(a2["name"], "Ranger");

    // A seat sees only its own agents.
    let (_, mine) = t.get("/v1/agents", a_tok).await;
    assert_eq!(mine.as_array().unwrap().len(), 1);
    assert_eq!(mine[0]["workspaceId"], a1["workspaceId"]);
    let (_, theirs) = t.get("/v1/agents", g_tok).await;
    assert_eq!(theirs[0]["workspaceId"], g1["workspaceId"]);
    assert!(!theirs.to_string().contains(a1["workspaceId"].as_str().unwrap()));
    t.cleanup().await;
}

/// §7: nothing a seat sends can carry view state — the wire types do not have the words, and the
/// server refuses a body that smuggles them.
#[tokio::test]
async fn no_endpoint_accepts_view_state_from_a_seat() {
    let t = start(cfg()).await;
    let ada = t.sign_up("ada@example.com", "Org").await;
    let tok = ada["token"].as_str().unwrap();
    let view = json!({ "focus": { "kind": "deal", "id": "x" }, "list": { "query": "acme" }, "board": {}, "pending": {}, "view": {} });
    for (path, mut body) in [
        ("/v1/signup", json!({ "email": "z@example.com", "password": "correct horse battery", "name": "Z", "orgName": "Z" })),
        ("/v1/signin", json!({ "email": "ada@example.com", "password": "correct horse battery" })),
        ("/v1/invites", json!({})),
        ("/v1/invites/accept", json!({ "token": "t", "email": "z@example.com", "password": "correct horse battery", "name": "Z" })),
        ("/v1/agents", json!({ "localAgentId": "1", "name": "Scout" })),
    ] {
        for (k, v) in view.as_object().unwrap() {
            body[k] = v.clone();
            let (s, _) = t.post(path, Some(tok), body.clone()).await;
            assert!(s.is_client_error(), "{path} accepted `{k}`: {s}");
            body.as_object_mut().unwrap().remove(k);
        }
    }
    t.cleanup().await;
}

#[tokio::test]
async fn every_table_carries_an_org_id() {
    let t = start(cfg()).await;
    let rows = t
        .admin
        .query(
            "SELECT table_name FROM information_schema.tables t
              WHERE table_schema = $1 AND table_name <> '_migrations'
                AND NOT EXISTS (SELECT 1 FROM information_schema.columns c
                                 WHERE c.table_schema = t.table_schema AND c.table_name = t.table_name
                                   AND c.column_name IN ('org_id'))",
            &[&t.schema],
        )
        .await
        .unwrap();
    let missing: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
    // `orgs` is the one table whose own `id` *is* the org id.
    assert_eq!(missing, ["orgs"], "a table without org_id: {missing:?}");
    t.cleanup().await;
}

#[tokio::test]
async fn tls_only_refuses_plain_requests_and_accepts_forwarded_https() {
    let t = start(Config { require_tls: true, trusted_hops: 1 }).await;
    let plain = Request::builder().uri("/healthz").body(Body::empty()).unwrap();
    let res = t.app.clone().router().oneshot(plain).await.unwrap();
    assert_eq!(res.status(), StatusCode::UPGRADE_REQUIRED);
    let tls = Request::builder().uri("/healthz").header("x-forwarded-proto", "https").body(Body::empty()).unwrap();
    assert_eq!(t.app.clone().router().oneshot(tls).await.unwrap().status(), StatusCode::OK);
    t.cleanup().await;
}

#[tokio::test]
async fn no_response_ever_contains_a_password_hash_or_another_tokens_hash() {
    let t = start(cfg()).await;
    let ada = t.sign_up("ada@example.com", "Org").await;
    let tok = ada["token"].as_str().unwrap();
    for path in ["/v1/me", "/v1/org/members", "/v1/agents"] {
        let (_, _, raw) = t.call("GET", path, Some(tok), Value::Null, "10.0.0.1").await;
        assert!(!raw.contains("argon2") && !raw.contains("password") && !raw.contains("tokenHash") && !raw.contains("token_hash"), "{path}: {raw}");
    }
    t.cleanup().await;
}

// MARK: - Round 8 §2: the rate limit's idea of "who"

fn no_proxy() -> Config {
    Config { require_tls: false, trusted_hops: 0 }
}

/// QA, MAJOR 1: twenty bad logins from anyone locked every sign-in and sign-up for fifteen minutes.
#[tokio::test]
async fn by_default_each_client_has_its_own_bucket_so_one_script_cannot_shut_the_front_door() {
    let t = start(no_proxy()).await;
    t.sign_up("ada@example.com", "Org").await;
    let mut codes = Vec::new();
    for n in 0..22 {
        codes.push(raw(&t.app, "POST", "/v1/signin", wrong(&format!("nobody{n}@example.com")), Some("203.0.113.9"), None).await.0.as_u16());
    }
    assert_eq!(codes.iter().filter(|c| **c == 401).count(), 20, "{codes:?}");
    assert_eq!(&codes[20..], [429, 429]);
    // An unrelated person, correct password, from their own address: in.
    let (s, b) = raw(&t.app, "POST", "/v1/signin", json!({ "email": "ada@example.com", "password": "correct horse battery" }), Some("198.51.100.4"), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    // A brand-new sign-up from anyone else: in.
    let (s, b) = raw(&t.app, "POST", "/v1/signup", json!({ "email": "new@example.com", "password": "correct horse battery", "name": "New", "orgName": "O", "device": "d" }), Some("198.51.100.5"), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    t.cleanup().await;
}

/// With no trusted proxy a header is only something the caller wrote: it never picks the bucket.
#[tokio::test]
async fn with_no_trusted_proxy_x_forwarded_for_is_ignored() {
    let t = start(no_proxy()).await;
    let mut last = StatusCode::OK;
    for n in 0..25 {
        last = raw(&t.app, "POST", "/v1/signin", wrong(&format!("u{n}@example.com")), Some("203.0.113.9"), Some(&format!("10.0.{n}.1"))).await.0;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS, "spoofing a new address each time must not escape the limit");
    t.cleanup().await;
}

/// QA, MAJOR 2: behind a proxy the attacker chose the bucket, by writing the first entry.
#[tokio::test]
async fn behind_a_proxy_the_last_entry_counts_not_the_first_the_client_wrote() {
    let t = start(Config { require_tls: false, trusted_hops: 1 }).await;
    t.sign_up("victim@example.com", "Org").await;
    // 40 wrong logins, a different spoofed leftmost entry each; the proxy appended the real one.
    let mut codes = Vec::new();
    for n in 0..40 {
        let xff = format!("192.0.2.{n}, 203.0.113.50");
        codes.push(raw(&t.app, "POST", "/v1/signin", wrong(&format!("u{n}@example.com")), Some("10.0.0.2"), Some(&xff)).await.0.as_u16());
    }
    assert_eq!(codes.iter().filter(|c| **c == 401).count(), 20, "the attacker is throttled by address: {codes:?}");
    // Twenty-plus failures *claiming* to be the victim's address do not lock the victim out.
    for n in 0..25 {
        raw(&t.app, "POST", "/v1/signin", wrong(&format!("v{n}@example.com")), Some("10.0.0.2"), Some("198.51.100.9, 203.0.113.51")).await;
    }
    let (s, b) = raw(&t.app, "POST", "/v1/signin", json!({ "email": "victim@example.com", "password": "correct horse battery" }), Some("10.0.0.2"), Some("198.51.100.9")).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    t.cleanup().await;
}

#[tokio::test]
async fn a_configured_number_of_hops_counts_that_far_from_the_right() {
    let t = start(Config { require_tls: false, trusted_hops: 2 }).await;
    let mut last = StatusCode::OK;
    for n in 0..22 {
        // client, edge proxy, inner proxy: the client is two from the right.
        last = raw(&t.app, "POST", "/v1/signin", wrong(&format!("u{n}@example.com")), Some("10.0.0.2"), Some(&format!("forged-{n}, 203.0.113.7, 10.0.0.9"))).await.0;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
    // A different client behind the same proxies is not.
    let (s, _) = raw(&t.app, "POST", "/v1/signin", wrong("x@example.com"), Some("10.0.0.2"), Some("forged, 203.0.113.8, 10.0.0.9")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // A header shorter than the configured hops is not believed: the peer is used.
    let (s, _) = raw(&t.app, "POST", "/v1/signin", wrong("y@example.com"), Some("10.0.0.3"), Some("203.0.113.7")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    t.cleanup().await;
}

/// QA, MINOR: a burst raced the limit — 16 evaluations for a limit of 5.
#[tokio::test]
async fn a_burst_of_wrong_passwords_is_held_to_the_limit_exactly() {
    for burst in [20usize, 100] {
        let t = start(no_proxy()).await;
        t.sign_up("ada@example.com", "Org").await;
        let mut set = tokio::task::JoinSet::new();
        for _ in 0..burst {
            let app = t.app.clone();
            set.spawn(async move { raw(&app, "POST", "/v1/signin", wrong("ada@example.com"), Some("203.0.113.9"), None).await.0 });
        }
        let mut evaluated = 0;
        while let Some(r) = set.join_next().await {
            if r.unwrap() == StatusCode::UNAUTHORIZED {
                evaluated += 1;
            }
        }
        assert_eq!(evaluated, 5, "burst of {burst}: only the limit's worth of passwords may be evaluated");
        // …and the correct password after the limit is still refused.
        let (s, _) = raw(&t.app, "POST", "/v1/signin", json!({ "email": "ada@example.com", "password": "correct horse battery" }), Some("198.51.100.4"), None).await;
        assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
        t.cleanup().await;
    }
}

#[tokio::test]
async fn a_burst_of_invite_guesses_is_held_to_the_address_limit_exactly() {
    let t = start(no_proxy()).await;
    let mut set = tokio::task::JoinSet::new();
    for n in 0..200 {
        let app = t.app.clone();
        set.spawn(async move {
            let body = json!({ "token": format!("guess-{n}"), "email": "a@example.com", "password": "another long password", "name": "A", "device": "d" });
            raw(&app, "POST", "/v1/invites/accept", body, Some("203.0.113.9"), None).await.0
        });
    }
    let mut evaluated = 0;
    while let Some(r) = set.join_next().await {
        if r.unwrap() == StatusCode::BAD_REQUEST {
            evaluated += 1;
        }
    }
    assert_eq!(evaluated, 20, "QA saw 35 evaluated against a limit of 20");
    t.cleanup().await;
}

/// Being refused gives the slot back: the lock does not grow by being knocked on.
#[tokio::test]
async fn a_refused_attempt_does_not_extend_the_lock() {
    let t = start(no_proxy()).await;
    for _ in 0..30 {
        raw(&t.app, "POST", "/v1/signin", wrong("ada@example.com"), Some("203.0.113.9"), None).await;
    }
    let n: i64 = t.pool.get().await.unwrap().query_one("SELECT count(*) FROM login_attempts WHERE kind = 'email'", &[]).await.unwrap().get(0);
    assert_eq!(n, 5);
    t.cleanup().await;
}
