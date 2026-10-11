//! The window's account: the session, and the calls that make and end it.
//!
//! **Credentials live here and nowhere else** (`p2-identity.md` §2, playbook §13):
//!
//! * the session token is a [`Secret`] — it has no `Debug`, no `Display` and no `Serialize`, so it
//!   cannot be formatted into a log line or a snapshot by accident;
//! * it is stored in the app's data dir (`session.json`, owner-only, atomic) and read back only to
//!   be put on a request header;
//! * **nothing in this module prints** — a test fails the build if a print macro appears;
//! * every call here is made for **the window** (Tauri commands). None is a CLI verb, none is an
//!   envelope the core's `command` knows, so an agent cannot reach any of it.
//!
//! What goes out is built only from the `identity` crate's wire types, which have no field that
//! could hold view state.

use identity::{
    AcceptInvite, CreateInvite, ErrorBody, InviteCreated, Member, Org, Person, RegisterAgent, Role, Secret, Session,
    SignIn, SignUp, WhoAmI, WorkspaceAgent,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// One HTTP exchange. A trait so the tests can read exactly what would have gone on the wire.
pub trait Transport: Send + Sync {
    /// `Err` is a transport failure (no route, no TLS, timeout); an HTTP error status is `Ok`.
    fn send(&self, method: &str, url: &str, bearer: Option<&Secret>, body: Option<Value>) -> Result<(u16, Value), String>;
}

/// The real transport. Blocking, short timeouts; call it from `spawn_blocking`.
pub struct Http;

impl Transport for Http {
    fn send(&self, method: &str, url: &str, bearer: Option<&Secret>, body: Option<Value>) -> Result<(u16, Value), String> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(15))
            .redirects(0) // a redirect could carry the bearer token somewhere nobody chose
            .build();
        let mut req = agent.request(method, url);
        if let Some(t) = bearer {
            req = req.set("Authorization", &format!("Bearer {}", t.expose()));
        }
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        let (status, resp) = match res {
            Ok(r) => (r.status(), r),
            Err(ureq::Error::Status(code, r)) => (code, r),
            // The transport error can quote the URL; the URL holds no secret, but say no more than needed.
            Err(ureq::Error::Transport(t)) => return Err(format!("could not reach the server ({})", t.kind())),
        };
        let json = resp.into_json::<Value>().unwrap_or(Value::Null);
        Ok((status, json))
    }
}

/// What is kept between launches. The token is the only secret in it.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    server: String,
    token: Secret,
    seat_id: String,
    person: Person,
    org: Org,
    role: Role,
    expires_at: i64,
    #[serde(default)]
    agents: HashMap<String, String>,
}

/// Written by hand, field by field: `Secret` refuses `Serialize`, and this is the one place a
/// token is meant to be written down.
fn stored_json(s: &Stored) -> Value {
    serde_json::json!({
        "server": s.server, "token": s.token.expose(), "seatId": s.seat_id,
        "person": s.person, "org": s.org, "role": s.role, "expiresAt": s.expires_at, "agents": s.agents,
    })
}

impl std::fmt::Debug for Stored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stored").field("server", &self.server).field("person", &self.person).field("token", &"[redacted]").finish()
    }
}

/// What the window may know about the account: **never the token**.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub signed_in: bool,
    pub server: Option<String>,
    pub person: Option<Person>,
    pub org: Option<Org>,
    pub role: Option<Role>,
    pub expires_at: Option<i64>,
    /// The server speaks plain HTTP to something that is not this machine. The window says so,
    /// loudly, and never downgrades silently.
    pub insecure: bool,
}

pub struct Account {
    path: PathBuf,
    transport: Box<dyn Transport>,
    held: Mutex<Option<Stored>>,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("signed_in", &self.held.lock().map(|h| h.is_some()).unwrap_or(false)).finish()
    }
}

/// What the window must know about a server address **before** it sends a password or an invite to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCheck {
    /// The address parses as an `http(s)` URL with a host, so a request can be made to it at all.
    pub valid: bool,
    /// The host the request would really go to — what the window shows ("your password will be sent to evil.example"),
    /// because the part of the address a person reads first is not always the host.
    pub host: Option<String>,
    /// Plain HTTP to something that is not this machine, or an address that cannot be read: fail closed.
    pub insecure: bool,
    /// The address carries `user:pass@` in front of its host: the classic way to make one host look like another.
    pub userinfo: bool,
}

/// Read a server address as the HTTP client will. A real URL parser, never string slicing:
/// `http://127.0.0.1:80@evil.example` is a request to **evil.example**.
pub fn check_server(server: &str) -> ServerCheck {
    let unreadable = ServerCheck { valid: false, host: None, insecure: true, userinfo: false };
    let Ok(u) = url::Url::parse(server.trim()) else { return unreadable };
    let Some(host) = u.host() else { return unreadable };
    let userinfo = !u.username().is_empty() || u.password().is_some();
    let scheme_ok = matches!(u.scheme(), "http" | "https");
    // Only the loopback addresses and the literal name `localhost` are "this machine". `*.localhost`
    // is not trusted: a resolver that does not special-case it can send it anywhere.
    let local = match host {
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    };
    ServerCheck {
        valid: scheme_ok,
        host: Some(host.to_string()),
        insecure: !scheme_ok || (u.scheme() == "http" && !local),
        userinfo,
    }
}

/// Plain HTTP is fine to this machine and to nothing else.
pub fn is_insecure(server: &str) -> bool {
    check_server(server).insecure
}

/// The gate in front of every request that carries a password or an invite: an insecure or unreadable
/// address is refused unless the person has been shown the warning and said yes. **Before** sending,
/// never after.
pub fn require_confirmation(server: &str, confirmed: bool) -> Result<(), String> {
    let c = check_server(server);
    if !c.valid {
        return Err("that does not look like a server address — use https://…".to_string());
    }
    if (c.insecure || c.userinfo) && !confirmed {
        return Err(format!(
            "not sent: {} is not a secure address — check it with account_check_server and ask the person first",
            c.host.unwrap_or_default()
        ));
    }
    Ok(())
}

/// The server's own sentence for an error, or a generic one. Never a body we did not expect.
fn failure(status: u16, body: &Value) -> String {
    serde_json::from_value::<ErrorBody>(body.clone()).map(|e| e.error).unwrap_or_else(|_| format!("the server answered {status}"))
}

fn url(server: &str, path: &str) -> String {
    format!("{}{}", server.trim_end_matches('/'), path)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn device_label() -> String {
    std::env::var("HOSTNAME").or_else(|_| std::env::var("HOST")).unwrap_or_else(|_| "this device".to_string())
}

impl Account {
    /// Open the account from its file. A file that cannot be read back is moved aside and the app
    /// is simply signed out: a damaged session must never stop the app opening on the person's data.
    pub fn open(path: PathBuf, transport: Box<dyn Transport>) -> Account {
        let held = std::fs::read(&path).ok().and_then(|bytes| match serde_json::from_slice::<Stored>(&bytes) {
            Ok(s) if s.expires_at > now_secs() => Some(s),
            Ok(_) => None, // expired: signed out, locally and for good
            Err(_) => {
                let _ = clappkit::store::quarantine(&path);
                None
            }
        });
        Account { path, transport, held: Mutex::new(held) }
    }

    fn save(&self, s: Option<&Stored>) -> Result<(), String> {
        match s {
            Some(s) => {
                let bytes = serde_json::to_vec_pretty(&stored_json(s)).map_err(|_| "cannot save the session".to_string())?;
                clappkit::store::atomic_write(&self.path, &bytes).map_err(|_| "cannot save the session".to_string())
            }
            None => match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err("cannot remove the session file".to_string()),
            },
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Stored>> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> Status {
        match &*self.lock() {
            Some(s) => Status {
                signed_in: true,
                server: Some(s.server.clone()),
                person: Some(s.person.clone()),
                org: Some(s.org.clone()),
                role: Some(s.role),
                expires_at: Some(s.expires_at),
                insecure: is_insecure(&s.server),
            },
            None => Status { signed_in: false, server: None, person: None, org: None, role: None, expires_at: None, insecure: false },
        }
    }

    /// The person and org this seat acts as, if signed in.
    pub fn identity(&self) -> Option<(Person, Org)> {
        self.lock().as_ref().map(|s| (s.person.clone(), s.org.clone()))
    }

    /// local agent id → workspace agent id, for what has been registered.
    pub fn agent_ids(&self) -> HashMap<String, String> {
        self.lock().as_ref().map(|s| s.agents.clone()).unwrap_or_default()
    }

    fn adopt(&self, server: &str, session: Session, extra_agents: HashMap<String, String>) -> Result<Status, String> {
        let stored = Stored {
            server: server.trim().to_string(),
            token: Secret::new(session.token),
            seat_id: session.seat_id,
            person: session.person,
            org: session.org,
            role: session.role,
            expires_at: session.expires_at,
            agents: extra_agents,
        };
        self.save(Some(&stored))?;
        *self.lock() = Some(stored);
        Ok(self.status())
    }

    fn auth<T: serde::de::DeserializeOwned>(&self, server: &str, path: &str, body: Value, what: &str) -> Result<T, String> {
        let (status, json) = self.transport.send("POST", &url(server, path), None, Some(body))?;
        if status / 100 != 2 {
            return Err(failure(status, &json));
        }
        serde_json::from_value(json).map_err(|_| format!("the server's answer to {what} was not understood"))
    }

    pub fn sign_up(&self, server: &str, email: &str, password: &Secret, name: &str, org_name: &str) -> Result<Status, String> {
        let body = SignUp {
            email: email.into(),
            password: password.expose().into(),
            name: name.into(),
            org_name: org_name.into(),
            device: device_label(),
        };
        let session: Session = self.auth(server, "/v1/signup", serde_json::to_value(&body).unwrap(), "sign-up")?;
        self.adopt(server, session, HashMap::new())
    }

    pub fn sign_in(&self, server: &str, email: &str, password: &Secret) -> Result<Status, String> {
        let body = SignIn { email: email.into(), password: password.expose().into(), device: device_label() };
        let session: Session = self.auth(server, "/v1/signin", serde_json::to_value(&body).unwrap(), "sign-in")?;
        self.adopt(server, session, HashMap::new())
    }

    pub fn accept_invite(&self, server: &str, invite: &Secret, email: &str, password: &Secret, name: &str) -> Result<Status, String> {
        let body = AcceptInvite {
            token: invite.expose().into(),
            email: email.into(),
            password: password.expose().into(),
            name: name.into(),
            device: device_label(),
        };
        let session: Session = self.auth(server, "/v1/invites/accept", serde_json::to_value(&body).unwrap(), "the invite")?;
        self.adopt(server, session, HashMap::new())
    }

    /// Sign out **on the server first** (revoking the token everywhere), then forget it here. If the
    /// server cannot be reached the local copy is still destroyed — the person asked to be out — and
    /// the answer says the server was not told, so the token may still be live there until it expires.
    pub fn sign_out(&self) -> Result<Option<&'static str>, String> {
        let held = self.lock().clone();
        let mut warning = None;
        if let Some(s) = &held {
            match self.transport.send("POST", &url(&s.server, "/v1/signout"), Some(&s.token), None) {
                Ok((status, _)) if status / 100 == 2 || status == 401 => {} // 401: already dead, which is the goal
                _ => warning = Some("signed out here, but the server could not be reached to revoke the session"),
            }
        }
        *self.lock() = None;
        self.save(None)?;
        Ok(warning)
    }

    fn call<T: serde::de::DeserializeOwned>(&self, method: &str, path: &str, body: Option<Value>) -> Result<T, String> {
        let held = self.lock().clone().ok_or_else(|| "sign in first".to_string())?;
        let (status, json) = self.transport.send(method, &url(&held.server, path), Some(&held.token), body)?;
        if status == 401 {
            // The server no longer honours this session: so neither do we.
            *self.lock() = None;
            let _ = self.save(None);
            return Err("your session ended — sign in again".to_string());
        }
        if status / 100 != 2 {
            return Err(failure(status, &json));
        }
        serde_json::from_value(json).map_err(|_| "the server's answer was not understood".to_string())
    }

    /// Ask the server who it thinks this session is — the window's "am I still signed in?".
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn whoami(&self) -> Result<WhoAmI, String> {
        self.call("GET", "/v1/me", None)
    }

    /// Names and ids only; roles are p4's.
    pub fn members(&self) -> Result<Vec<Member>, String> {
        self.call("GET", "/v1/org/members", None)
    }

    /// The invite's token is a credential: it goes to the window once, to be given to the invitee.
    pub fn create_invite(&self, email: Option<&str>) -> Result<InviteCreated, String> {
        let body = CreateInvite { email: email.map(str::to_string) };
        self.call("POST", "/v1/invites", Some(serde_json::to_value(&body).unwrap()))
    }

    /// The workspace id for a local agent, minting one on the server the first time it is seen.
    /// `None` when signed out, or when the server cannot be asked right now (the agent is then
    /// attributed by its local id, and asked about again on its next command).
    pub fn ensure_agent(&self, local_id: &str, name: &str) -> Option<String> {
        if let Some(known) = self.lock().as_ref()?.agents.get(local_id) {
            return Some(known.clone());
        }
        let body = RegisterAgent { local_agent_id: local_id.into(), name: name.into() };
        let reg: WorkspaceAgent = self.call("POST", "/v1/agents", Some(serde_json::to_value(&body).unwrap())).ok()?;
        let mut held = self.lock();
        let s = held.as_mut()?;
        s.agents.insert(local_id.to_string(), reg.workspace_id.clone());
        let snapshot = s.clone();
        drop(held);
        let _ = self.save(Some(&snapshot));
        Some(reg.workspace_id)
    }
}

#[cfg(test)]
#[path = "account_tests.rs"]
pub(crate) mod tests;
