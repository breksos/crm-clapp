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
    Config { require_tls: false, trust_proxy: true }
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
    let t = start(Config { require_tls: true, trust_proxy: true }).await;
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
