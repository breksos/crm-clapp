//! Breksos CRM's server — **identity only** (p2). Accounts, orgs, invites, seats, sessions and
//! workspace agent ids. It does not hold, move or serve record data: that is p3. Who may see
//! what is p4.
//!
//! The security surface (`docs/work-orders/p2-identity.md` §5), in one place:
//!
//! * **Passwords** are argon2id, at least [`MIN_PASSWORD`] characters, at most [`MAX_PASSWORD`].
//! * **Tokens** (sessions, invites) are 256 random bits shown once and stored as SHA-256 hashes.
//!   A leaked database is not a leaked set of sessions.
//! * **Sessions** expire ([`SESSION_TTL`]) and sign-out revokes server-side.
//! * **Invites** are single use, expiring, hashed.
//! * **Login is rate-limited** per account and per address, in the database, so it holds across
//!   instances.
//! * **Errors** never say which half of a credential was wrong, and an unknown address costs the
//!   same as a known one (a dummy hash is verified).
//! * **Nothing secret is logged.** There is no request logging at all; the only lines this crate
//!   prints are its own start-up and a generic "internal error".
//! * **Every request body refuses unknown fields** (`identity` types), so a client cannot put
//!   anything on the wire the contract does not name — view state included.
//! * **TLS:** the server speaks HTTP behind a TLS-terminating proxy. With `require_tls` set it
//!   refuses any request that did not arrive as `https`.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use deadpool_postgres::{Object, Pool};
use identity::{
    AcceptInvite, CreateInvite, ErrorBody, InviteCreated, Member, Org, Person, RegisterAgent, Role,
    Session, SignIn, SignUp, WhoAmI, WorkspaceAgent, WRONG_CREDENTIALS,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub mod sync;

pub const MIN_PASSWORD: usize = 10;
pub const MAX_PASSWORD: usize = 256;
pub const SESSION_TTL: i64 = 12 * 60 * 60;
pub const INVITE_TTL: i64 = 7 * 24 * 60 * 60;
/// Failed attempts allowed in [`ATTEMPT_WINDOW`] before the door shuts: per account, per address.
pub const MAX_FAILS_PER_ACCOUNT: i64 = 5;
pub const MAX_FAILS_PER_ADDRESS: i64 = 20;
pub const ATTEMPT_WINDOW: i64 = 15 * 60;

#[derive(Clone)]
pub struct Config {
    /// Refuse any request that did not arrive over TLS (`X-Forwarded-Proto: https`).
    pub require_tls: bool,
    /// How many proxies you control sit in front of this server (0 = none: a client is the
    /// connection's own peer address). With N ≥ 1 the client address is the Nth entry from the
    /// **right** of `X-Forwarded-For` — the one your own outermost proxy appended; whatever a
    /// client wrote further left is theirs to invent, and is never believed.
    pub trusted_hops: usize,
}

impl Config {
    pub fn from_env() -> Config {
        let on = |k: &str| matches!(std::env::var(k).as_deref(), Ok("1") | Ok("true"));
        // `CRM_TRUST_PROXY=1` (or `true`) is one trusted hop; a larger number is that many.
        let hops = match std::env::var("CRM_TRUST_PROXY").as_deref() {
            Ok("true") => 1,
            Ok(n) => n.trim().parse().unwrap_or(0),
            Err(_) => 0,
        };
        Config { require_tls: on("CRM_REQUIRE_TLS"), trusted_hops: hops }
    }
}

#[derive(Clone)]
pub struct App {
    pool: Pool,
    config: Config,
    /// Verified when an address is unknown, so "no such account" costs what "wrong password" costs.
    dummy_hash: Arc<String>,
    /// Who is listening to the push channel (see `sync::stream`).
    hub: Arc<sync::Hub>,
    stream: sync::StreamTiming,
}

// MARK: - Plumbing

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the OS must be able to provide randomness");
    b
}

/// An opaque id: a short prefix saying what it names, then 128 random bits.
fn new_id(prefix: &str) -> String {
    let b: [u8; 16] = random_bytes();
    format!("{prefix}_{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

/// A token for a credential: 256 random bits, URL-safe. Shown once; only [`hash_token`] is kept.
fn new_token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

/// SHA-256, hex. Fast on purpose: a token is already 256 random bits, so there is nothing to
/// brute-force; a password is not, and is hashed with argon2id instead.
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::encode_b64(&random_bytes::<16>()).map_err(|_| ApiError::internal())?;
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|_| ApiError::internal())
}

fn verify_password(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

fn normalise_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn looks_like_email(e: &str) -> bool {
    e.len() <= 254 && e.split_once('@').is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.'))
}

/// An error a client sees. Its text is the whole of what it learns.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: &'static str,
}

impl ApiError {
    fn new(status: StatusCode, message: &'static str) -> ApiError {
        ApiError { status, message }
    }
    fn bad(message: &'static str) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, message)
    }
    /// Nothing about *why* leaves the server: a database error can carry a value.
    fn internal() -> ApiError {
        eprintln!("crm-server: internal error");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong")
    }
    fn wrong_credentials() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, WRONG_CREDENTIALS)
    }
    fn unauthorised() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "sign in again")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorBody { error: self.message.to_string() })).into_response()
    }
}

impl From<tokio_postgres::Error> for ApiError {
    fn from(_: tokio_postgres::Error) -> ApiError {
        ApiError::internal()
    }
}
impl From<deadpool_postgres::PoolError> for ApiError {
    fn from(_: deadpool_postgres::PoolError) -> ApiError {
        ApiError::internal()
    }
}

type Res<T> = Result<Json<T>, ApiError>;

// MARK: - Migrations

const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_identity", include_str!("../migrations/0001_identity.sql")),
    ("0002_sync", include_str!("../migrations/0002_sync.sql")),
];

/// Apply every migration not yet applied, each in its own transaction, in order.
pub async fn migrate(pool: &Pool) -> Result<(), ApiError> {
    let mut db = pool.get().await?;
    db.batch_execute("CREATE TABLE IF NOT EXISTS _migrations (name text PRIMARY KEY, applied_at bigint NOT NULL)").await?;
    for (name, sql) in MIGRATIONS {
        let done = db.query_opt("SELECT 1 FROM _migrations WHERE name = $1", &[name]).await?.is_some();
        if done {
            continue;
        }
        let tx = db.transaction().await?;
        tx.batch_execute(sql).await?;
        tx.execute("INSERT INTO _migrations (name, applied_at) VALUES ($1, $2)", &[name, &now()]).await?;
        tx.commit().await?;
    }
    Ok(())
}

impl App {
    pub async fn new(pool: Pool, config: Config) -> Result<App, ApiError> {
        migrate(&pool).await?;
        let dummy_hash = Arc::new(hash_password("a password nobody has")?);
        Ok(App { pool, config, dummy_hash, hub: Arc::new(sync::Hub::default()), stream: sync::StreamTiming::default() })
    }

    /// The push channel's clocks. Production uses the defaults; the tests shorten them.
    pub fn with_stream_timing(mut self, t: sync::StreamTiming) -> App {
        self.stream = t;
        self
    }

    pub fn router(self) -> Router {
        Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/v1/signup", post(sign_up))
            .route("/v1/signin", post(sign_in))
            .route("/v1/signout", post(sign_out))
            .route("/v1/me", get(me))
            .route("/v1/org/members", get(members))
            .route("/v1/invites", post(create_invite))
            .route("/v1/invites/accept", post(accept_invite))
            .route("/v1/agents", post(register_agent).get(list_agents))
            .route("/v1/sync/bootstrap", get(sync::bootstrap))
            .route("/v1/sync/changes", get(sync::changes))
            .route("/v1/sync/stream", get(sync::stream))
            .route("/v1/sync/write", post(sync::write).layer(DefaultBodyLimit::max(sync::WRITE_MAX_BYTES)))
            .layer(DefaultBodyLimit::max(64 * 1024))
            .layer(axum::middleware::from_fn_with_state(self.clone(), tls_gate))
            .with_state(self)
    }

    async fn db(&self) -> Result<Object, ApiError> {
        Ok(self.pool.get().await?)
    }
}

/// Refuse a request that did not come over TLS, when the server is told it must.
async fn tls_gate(
    State(app): State<App>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if app.config.require_tls {
        let https = req.headers().get("x-forwarded-proto").and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("https"));
        if !https {
            return ApiError::new(StatusCode::UPGRADE_REQUIRED, "this server only speaks TLS").into_response();
        }
    }
    next.run(req).await
}

/// The connection's own peer address, put there by `axum::serve` (see `main.rs`). Absent when a
/// request did not come over a socket (the tests call the router directly).
pub struct Peer(Option<std::net::IpAddr>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Peer {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut axum::http::request::Parts, _: &S) -> Result<Peer, Self::Rejection> {
        Ok(Peer(parts.extensions.get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().map(|c| c.0.ip())))
    }
}

/// Who is calling, for the per-address limit. Without a trusted proxy that is the connection's
/// own peer address — never a header, which the caller writes. Behind `trusted_hops` proxies it is
/// that many entries from the right of `X-Forwarded-For`; if the header is shorter than that, the
/// proxy is not doing what it was configured to, and the peer address is used instead.
fn client_address(app: &App, headers: &HeaderMap, peer: &Peer) -> String {
    let direct = peer.0.map(|p| p.to_string()).unwrap_or_else(|| "direct".to_string());
    let hops = app.config.trusted_hops;
    if hops > 0 {
        let entries: Vec<&str> = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .collect();
        if entries.len() >= hops {
            return entries[entries.len() - hops].to_string();
        }
    }
    direct
}

// MARK: - Rate limiting

/// The attempt slots one request holds. Claimed **before** the password is checked, so a burst of
/// concurrent guesses cannot all be counted as "the first": each one's slot is in the table before
/// it is decided. Kept on failure (that *is* the record); released on success.
pub struct Claim(Vec<i64>);

/// This attempt's **rank**: how many live slots for `key` are at or before its own `id`. Ranking by
/// id rather than counting everything makes a burst exact: the first `limit` slots are in, every
/// later one is out, however the requests interleave.
async fn rank(db: &Object, kind: &str, key: &str, id: i64) -> Result<i64, ApiError> {
    let row = db
        .query_one(
            "SELECT count(*) FROM login_attempts WHERE kind = $1 AND key = $2 AND at > $3 AND id <= $4",
            &[&kind, &key, &(now() - ATTEMPT_WINDOW), &id],
        )
        .await?;
    Ok(row.get(0))
}

async fn release(db: &Object, claim: Claim) -> Result<(), ApiError> {
    if !claim.0.is_empty() {
        db.execute("DELETE FROM login_attempts WHERE id = ANY($1)", &[&claim.0]).await?;
    }
    Ok(())
}

/// Claim a slot for this attempt, then ask whether the door is shut. `Err` (429) when this account
/// or this address already holds its limit — and a refused attempt gives its slot back, so being
/// refused never extends the lock.
async fn claim_attempt(db: &Object, email_key: Option<&str>, ip_key: &str) -> Result<Claim, ApiError> {
    let t = now();
    let mut ids = Vec::new();
    let mut email_slot = None;
    if let Some(k) = email_key {
        let id: i64 = db.query_one("INSERT INTO login_attempts (kind, key, at) VALUES ('email', $1, $2) RETURNING id", &[&k, &t]).await?.get(0);
        ids.push(id);
        email_slot = Some((k, id));
    }
    let ip_slot: i64 = db.query_one("INSERT INTO login_attempts (kind, key, at) VALUES ('ip', $1, $2) RETURNING id", &[&ip_key, &t]).await?.get(0);
    ids.push(ip_slot);
    let claim = Claim(ids);
    let limited = rank(db, "ip", ip_key, ip_slot).await? > MAX_FAILS_PER_ADDRESS
        || match email_slot {
            Some((k, id)) => rank(db, "email", k, id).await? > MAX_FAILS_PER_ACCOUNT,
            None => false,
        };
    if limited {
        release(db, claim).await?;
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too many attempts — wait a few minutes"));
    }
    // Old rows are not worth keeping.
    db.execute("DELETE FROM login_attempts WHERE at < $1", &[&(t - 24 * ATTEMPT_WINDOW)]).await?;
    Ok(claim)
}

// MARK: - Sessions

/// Mint a seat and a session for `user_id`, and return the one and only time the token is visible.
async fn open_session(db: &Object, org_id: &str, user_id: &str, device: &str) -> Result<(String, String, i64), ApiError> {
    let seat_id = new_id("seat");
    let t = now();
    let device = if device.trim().is_empty() { "this device" } else { device.trim() };
    let device: String = device.chars().take(80).collect();
    db.execute("INSERT INTO seats (id, org_id, user_id, device, created_at) VALUES ($1,$2,$3,$4,$5)", &[&seat_id, &org_id, &user_id, &device, &t]).await?;
    let token = new_token();
    let expires = t + SESSION_TTL;
    db.execute(
        "INSERT INTO sessions (id, org_id, seat_id, token_hash, expires_at) VALUES ($1,$2,$3,$4,$5)",
        &[&new_id("sess"), &org_id, &seat_id, &hash_token(&token), &expires],
    )
    .await?;
    Ok((token, seat_id, expires))
}

struct Authed {
    org_id: String,
    user_id: String,
    seat_id: String,
    session_id: String,
    role: Role,
    person: Person,
    org: Org,
}

fn role_of(s: &str) -> Role {
    if s == "owner" { Role::Owner } else { Role::Member }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ").map(str::trim).filter(|t| !t.is_empty())
}

/// Who a bearer token is, if it is a live session: not expired, not revoked.
async fn authenticate(app: &App, headers: &HeaderMap) -> Result<(Object, Authed), ApiError> {
    let token = bearer(headers).ok_or_else(ApiError::unauthorised)?;
    let db = app.db().await?;
    let row = db
        .query_opt(
            "SELECT s.id, s.seat_id, s.org_id, seat.user_id, u.name, o.name, m.role
               FROM sessions s
               JOIN seats seat ON seat.id = s.seat_id
               JOIN users u ON u.id = seat.user_id
               JOIN orgs o ON o.id = s.org_id
               JOIN memberships m ON m.org_id = s.org_id AND m.user_id = u.id
              WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > $2",
            &[&hash_token(token), &now()],
        )
        .await?
        .ok_or_else(ApiError::unauthorised)?;
    let (org_id, user_id): (String, String) = (row.get(2), row.get(3));
    let authed = Authed {
        session_id: row.get(0),
        seat_id: row.get(1),
        person: Person { id: user_id.clone(), name: row.get(4) },
        org: Org { id: org_id.clone(), name: row.get(5) },
        role: role_of(row.get::<_, &str>(6)),
        org_id,
        user_id,
    };
    Ok((db, authed))
}

fn session_body(token: String, seat_id: String, expires_at: i64, person: Person, org: Org, role: Role) -> Json<Session> {
    Json(Session { token, seat_id, person, org, role, expires_at })
}

fn check_password(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < MIN_PASSWORD {
        return Err(ApiError::bad("the password must be at least 10 characters"));
    }
    if password.len() > MAX_PASSWORD {
        return Err(ApiError::bad("the password is too long"));
    }
    Ok(())
}

fn check_name(name: &str, what: &'static str) -> Result<String, ApiError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 80 {
        return Err(ApiError::bad(what));
    }
    Ok(n.to_string())
}

// MARK: - Handlers

async fn sign_up(State(app): State<App>, peer: Peer, headers: HeaderMap, Json(req): Json<SignUp>) -> Res<Session> {
    let ip = hash_token(&client_address(&app, &headers, &peer));
    let email = normalise_email(&req.email);
    if !looks_like_email(&email) {
        return Err(ApiError::bad("that does not look like an email address"));
    }
    check_password(&req.password)?;
    let name = check_name(&req.name, "a name is needed (80 characters at most)")?;
    let org_name = check_name(&req.org_name, "the organisation needs a name (80 characters at most)")?;

    let mut db = app.db().await?;
    // Sign-up is a guessing surface for "is this address taken?", so it shares the address limit.
    let claim = claim_attempt(&db, None, &ip).await?;
    let (org_id, user_id) = (new_id("org"), new_id("user"));
    let phc = hash_password(&req.password)?;
    let tx = db.transaction().await?;
    let t = now();
    tx.execute("INSERT INTO orgs (id, name, created_at) VALUES ($1,$2,$3)", &[&org_id, &org_name, &t]).await?;
    sync::seed_pipeline(&tx, &org_id).await?;
    let inserted = tx
        .execute(
            "INSERT INTO users (id, org_id, email, email_norm, name, password_hash, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (email_norm) DO NOTHING",
            &[&user_id, &org_id, &req.email.trim(), &email, &name, &phc, &t],
        )
        .await?;
    if inserted == 0 {
        drop(tx);
        // One answer, whatever the reason: it does not say *that* the address is taken.
        return Err(ApiError::new(StatusCode::CONFLICT, "that address cannot be used to sign up"));
    }
    tx.execute("INSERT INTO memberships (org_id, user_id, role) VALUES ($1,$2,'owner')", &[&org_id, &user_id]).await?;
    tx.commit().await?;
    release(&db, claim).await?;
    let (token, seat_id, expires) = open_session(&db, &org_id, &user_id, &req.device).await?;
    Ok(session_body(token, seat_id, expires, Person { id: user_id, name }, Org { id: org_id, name: org_name }, Role::Owner))
}

async fn sign_in(State(app): State<App>, peer: Peer, headers: HeaderMap, Json(req): Json<SignIn>) -> Res<Session> {
    let ip = hash_token(&client_address(&app, &headers, &peer));
    let email = normalise_email(&req.email);
    let email_key = hash_token(&email);
    let db = app.db().await?;
    let claim = claim_attempt(&db, Some(&email_key), &ip).await?;

    let row = db
        .query_opt(
            "SELECT u.id, u.name, u.password_hash, u.org_id, o.name, m.role
               FROM users u JOIN orgs o ON o.id = u.org_id JOIN memberships m ON m.org_id = u.org_id AND m.user_id = u.id
              WHERE u.email_norm = $1",
            &[&email],
        )
        .await?;
    // The same work whether or not the account exists.
    let phc: String = row.as_ref().map(|r| r.get(2)).unwrap_or_else(|| (*app.dummy_hash).clone());
    let ok = verify_password(&req.password, &phc);
    let Some(row) = row.filter(|_| ok) else {
        return Err(ApiError::wrong_credentials());
    };
    db.execute("DELETE FROM login_attempts WHERE kind = 'email' AND key = $1", &[&email_key]).await?;
    release(&db, claim).await?;
    let (user_id, name, org_id, org_name): (String, String, String, String) = (row.get(0), row.get(1), row.get(3), row.get(4));
    let role = role_of(row.get::<_, &str>(5));
    let (token, seat_id, expires) = open_session(&db, &org_id, &user_id, &req.device).await?;
    Ok(session_body(token, seat_id, expires, Person { id: user_id, name }, Org { id: org_id, name: org_name }, role))
}

/// Revokes **on the server**: the token stops working everywhere, not only on this install.
async fn sign_out(State(app): State<App>, headers: HeaderMap) -> Result<StatusCode, ApiError> {
    let (db, who) = authenticate(&app, &headers).await?;
    db.execute("UPDATE sessions SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL", &[&who.session_id, &now()]).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(State(app): State<App>, headers: HeaderMap) -> Res<WhoAmI> {
    let (_, who) = authenticate(&app, &headers).await?;
    Ok(Json(WhoAmI { person: who.person, org: who.org, role: who.role, seat_id: who.seat_id }))
}

/// Names only — no email, no role (roles are p4's).
async fn members(State(app): State<App>, headers: HeaderMap) -> Res<Vec<Member>> {
    let (db, who) = authenticate(&app, &headers).await?;
    let rows = db
        .query("SELECT u.id, u.name FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.org_id = $1 ORDER BY u.name, u.id", &[&who.org_id])
        .await?;
    Ok(Json(rows.iter().map(|r| Member { id: r.get(0), name: r.get(1) }).collect()))
}

async fn create_invite(State(app): State<App>, headers: HeaderMap, Json(req): Json<CreateInvite>) -> Res<InviteCreated> {
    let (db, who) = authenticate(&app, &headers).await?;
    let email = match req.email.as_deref().map(normalise_email) {
        Some(e) if !looks_like_email(&e) => return Err(ApiError::bad("that does not look like an email address")),
        other => other,
    };
    let token = new_token();
    let expires = now() + INVITE_TTL;
    db.execute(
        "INSERT INTO invites (id, org_id, token_hash, email, created_by, expires_at) VALUES ($1,$2,$3,$4,$5,$6)",
        &[&new_id("inv"), &who.org_id, &hash_token(&token), &email, &who.user_id, &expires],
    )
    .await?;
    Ok(Json(InviteCreated { token, expires_at: expires }))
}

async fn accept_invite(State(app): State<App>, peer: Peer, headers: HeaderMap, Json(req): Json<AcceptInvite>) -> Res<Session> {
    let ip = hash_token(&client_address(&app, &headers, &peer));
    let email = normalise_email(&req.email);
    if !looks_like_email(&email) {
        return Err(ApiError::bad("that does not look like an email address"));
    }
    check_password(&req.password)?;
    let name = check_name(&req.name, "a name is needed (80 characters at most)")?;
    let mut db = app.db().await?;
    // An invite token is a credential: guessing it is rate-limited like guessing a password.
    let claim = claim_attempt(&db, None, &ip).await?;

    let phc = hash_password(&req.password)?;
    let tx = db.transaction().await?;
    let t = now();
    // Claimed atomically: two people racing the same link cannot both get in.
    let claimed = tx
        .query_opt(
            "UPDATE invites SET used_at = $2 WHERE token_hash = $1 AND used_at IS NULL AND expires_at > $2
             RETURNING org_id, email",
            &[&hash_token(&req.token), &t],
        )
        .await?;
    let Some(inv) = claimed else {
        drop(tx);
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "that invite is not valid"));
    };
    let org_id: String = inv.get(0);
    let bound: Option<String> = inv.get(1);
    if bound.is_some_and(|b| b != email) {
        // The claim above is rolled back with the transaction: a wrong address does not burn the link.
        drop(tx);
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "that invite is not valid"));
    }
    let user_id = new_id("user");
    let inserted = tx
        .execute(
            "INSERT INTO users (id, org_id, email, email_norm, name, password_hash, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (email_norm) DO NOTHING",
            &[&user_id, &org_id, &req.email.trim(), &email, &name, &phc, &t],
        )
        .await?;
    if inserted == 0 {
        drop(tx);
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "that invite is not valid"));
    }
    tx.execute("INSERT INTO memberships (org_id, user_id, role) VALUES ($1,$2,'member')", &[&org_id, &user_id]).await?;
    let org_name: String = tx.query_one("SELECT name FROM orgs WHERE id = $1", &[&org_id]).await?.get(0);
    tx.commit().await?;
    release(&db, claim).await?;
    let (token, seat_id, expires) = open_session(&db, &org_id, &user_id, &req.device).await?;
    Ok(session_body(token, seat_id, expires, Person { id: user_id, name }, Org { id: org_id, name: org_name }, Role::Member))
}

/// The server mints the workspace id for `(this user, this local agent id)`. Registering the same
/// local id again returns the same workspace id and updates the label (a rename).
async fn register_agent(State(app): State<App>, headers: HeaderMap, Json(req): Json<RegisterAgent>) -> Res<WorkspaceAgent> {
    let (db, who) = authenticate(&app, &headers).await?;
    let local = req.local_agent_id.trim();
    if local.is_empty() || local.len() > 200 {
        return Err(ApiError::bad("an agent needs its local id"));
    }
    let name = check_name(&req.name, "an agent needs a name (80 characters at most)")?;
    let row = db
        .query_one(
            "INSERT INTO agent_idents (workspace_id, org_id, user_id, local_agent_id, name) VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (org_id, user_id, local_agent_id) DO UPDATE SET name = EXCLUDED.name
             RETURNING workspace_id",
            &[&new_id("agent"), &who.org_id, &who.user_id, &local, &name],
        )
        .await?;
    Ok(Json(WorkspaceAgent { workspace_id: row.get(0), local_agent_id: local.to_string(), name }))
}

/// Only the caller's own agents: another seat's identities are not listable.
async fn list_agents(State(app): State<App>, headers: HeaderMap) -> Res<Vec<WorkspaceAgent>> {
    let (db, who) = authenticate(&app, &headers).await?;
    let rows = db
        .query("SELECT workspace_id, local_agent_id, name FROM agent_idents WHERE org_id = $1 AND user_id = $2 ORDER BY workspace_id", &[&who.org_id, &who.user_id])
        .await?;
    Ok(Json(rows.iter().map(|r| WorkspaceAgent { workspace_id: r.get(0), local_agent_id: r.get(1), name: r.get(2) }).collect()))
}
