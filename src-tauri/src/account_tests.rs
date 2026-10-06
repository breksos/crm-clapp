//! The window's account, as tests: what is kept, what goes out, and what must never come out.

use super::*;
use crate::model::{Date, Db};
use crate::state::AppState;
use serde_json::json;
use std::sync::{Arc, Mutex as StdMutex};

pub(crate) const TOKEN: &str = "tok-SECRET-4f1c9a";
pub(crate) const PASSWORD: &str = "pw-SECRET-correct-horse";
pub(crate) const INVITE: &str = "inv-SECRET-0d77";

/// (method, url, had a bearer token, body)
pub(crate) type Sent = (String, String, bool, Option<Value>);
type Answerer = Box<dyn Fn(&str, &str, Option<&Value>) -> Result<(u16, Value), String> + Send>;

/// A server that answers from a closure and remembers every request it was sent.
#[derive(Clone, Default)]
pub(crate) struct Fake {
    sent: Arc<StdMutex<Vec<Sent>>>,
    answer: Arc<StdMutex<Option<Answerer>>>,
}

impl Fake {
    pub(crate) fn answering(f: impl Fn(&str, &str, Option<&Value>) -> Result<(u16, Value), String> + Send + 'static) -> Fake {
        let fake = Fake::default();
        *fake.answer.lock().unwrap() = Some(Box::new(f));
        fake
    }
    pub(crate) fn requests(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }
}

impl Transport for Fake {
    fn send(&self, method: &str, url: &str, bearer: Option<&Secret>, body: Option<Value>) -> Result<(u16, Value), String> {
        self.sent.lock().unwrap().push((method.into(), url.into(), bearer.is_some(), body.clone()));
        match &*self.answer.lock().unwrap() {
            Some(f) => f(method, url, body.as_ref()),
            None => Err("no server".into()),
        }
    }
}

pub(crate) fn session(user: &str) -> Value {
    json!({
        "token": TOKEN, "seatId": format!("seat-{user}"),
        "person": { "id": format!("user-{user}"), "name": user }, "org": { "id": "org-1", "name": "Analytical" },
        "role": "owner", "expiresAt": 4_102_444_800i64
    })
}

/// A server that signs anyone in and mints workspace agent ids per (user token, local id).
pub(crate) fn friendly() -> Fake {
    Fake::answering(|_, url, body| {
        if url.ends_with("/v1/agents") {
            let local = body.and_then(|b| b["localAgentId"].as_str()).unwrap_or("");
            return Ok((200, json!({ "workspaceId": format!("wa-ada-{local}"), "localAgentId": local, "name": "Scout" })));
        }
        if url.ends_with("/v1/signout") {
            return Ok((204, Value::Null));
        }
        if url.ends_with("/v1/me") {
            return Ok((200, json!({ "person": { "id": "user-ada", "name": "ada" }, "org": { "id": "org-1", "name": "Analytical" }, "role": "owner", "seatId": "seat-ada" })));
        }
        if url.ends_with("/v1/org/members") {
            return Ok((200, json!([{ "id": "user-ada", "name": "ada" }])));
        }
        if url.ends_with("/v1/invites") {
            return Ok((200, json!({ "token": INVITE, "expiresAt": 4_102_444_800i64 })));
        }
        Ok((200, session("ada")))
    })
}

pub(crate) fn scratch(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("crm-acct-{}-{}-{tag}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("session.json")
}

fn signed_in(path: &std::path::Path, fake: &Fake) -> Account {
    let a = Account::open(path.to_path_buf(), Box::new(fake.clone()));
    a.sign_in("https://crm.example", "ada@example.com", &Secret::new(PASSWORD)).unwrap();
    a
}

// MARK: - What is kept

#[test]
fn signing_in_keeps_the_session_in_an_owner_only_file_and_reads_it_back() {
    let (path, fake) = (scratch("keep"), friendly());
    let a = signed_in(&path, &fake);
    assert!(a.status().signed_in);
    assert_eq!(a.identity().unwrap().0.name, "ada");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "the session is the owner's alone");
    }
    // A relaunch: still in, with no network needed to know it.
    let again = Account::open(path.clone(), Box::new(Fake::default()));
    assert!(again.status().signed_in);
    assert_eq!(again.status().person.unwrap().id, "user-ada");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_expired_or_damaged_session_file_means_signed_out_and_never_a_crash() {
    let path = scratch("bad");
    let mut s = session("ada");
    s["server"] = json!("https://crm.example");
    s["expiresAt"] = json!(1);
    std::fs::write(&path, s.to_string()).unwrap();
    assert!(!Account::open(path.clone(), Box::new(Fake::default())).status().signed_in, "expired");

    std::fs::write(&path, "{ not json").unwrap();
    assert!(!Account::open(path.clone(), Box::new(Fake::default())).status().signed_in);
    assert!(std::fs::read_dir(path.parent().unwrap()).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().contains("corrupt")), "kept aside, not overwritten");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn sign_out_tells_the_server_to_revoke_then_forgets_and_a_dead_server_does_not_keep_you_in() {
    let (path, fake) = (scratch("out"), friendly());
    let a = signed_in(&path, &fake);
    assert_eq!(a.sign_out().unwrap(), None);
    let last = fake.requests().pop().unwrap();
    assert!(last.1.ends_with("/v1/signout") && last.2, "the revoke went out carrying the token");
    assert!(!a.status().signed_in && !path.exists(), "and the local copy is gone");

    // Offline: still signed out here, and honest that the server was not told.
    let (path, fake) = (scratch("out2"), friendly());
    let a = signed_in(&path, &fake);
    let offline = Account { transport: Box::new(Fake::default()), ..a };
    let warning = offline.sign_out().unwrap();
    assert!(warning.unwrap().contains("could not be reached"));
    assert!(!offline.status().signed_in && !path.exists());
}

#[test]
fn a_session_the_server_refuses_ends_here_too() {
    let (path, fake) = (scratch("401"), friendly());
    let a = signed_in(&path, &fake);
    let dead = Account { transport: Box::new(Fake::answering(|_, _, _| Ok((401, json!({ "error": "sign in again" }))))), ..a };
    assert!(dead.members().unwrap_err().contains("sign in again"));
    assert!(!dead.status().signed_in && !path.exists());
}

#[test]
fn a_server_errors_own_words_reach_the_window_and_nothing_else_does() {
    let a = Account::open(scratch("err"), Box::new(Fake::answering(|_, _, _| Ok((401, json!({ "error": "wrong email or password" }))))));
    assert_eq!(a.sign_in("https://x", "a@b.c", &Secret::new("whatever-long")).unwrap_err(), "wrong email or password");
    let odd = Account::open(scratch("err2"), Box::new(Fake::answering(|_, _, _| Ok((500, json!({ "trace": "SELECT * FROM users WHERE password = 'pw-SECRET'" }))))));
    let e = odd.sign_in("https://x", "a@b.c", &Secret::new("whatever-long")).unwrap_err();
    assert_eq!(e, "the server answered 500", "an unexpected body is never relayed: {e}");
}

#[test]
fn plain_http_to_anything_but_this_machine_is_flagged_insecure() {
    for (url, insecure) in [
        ("https://crm.example.com", false),
        ("http://localhost:8787", false),
        ("http://127.0.0.1:8787", false),
        ("http://[::1]:8787", false),
        ("http://crm.example.com", true),
        ("http://192.168.1.20:8787", true),
        ("HTTP://Crm.Example.com", true),
    ] {
        assert_eq!(is_insecure(url), insecure, "{url}");
    }
    let (path, fake) = (scratch("insecure"), friendly());
    let a = Account::open(path.clone(), Box::new(fake));
    let st = a.sign_in("http://crm.example.com", "ada@example.com", &Secret::new(PASSWORD)).unwrap();
    assert!(st.insecure, "the window is told, so it can say so loudly");
}

// MARK: - Workspace agent ids

#[test]
fn an_agent_is_registered_once_and_its_workspace_id_is_kept_across_a_relaunch() {
    let (path, fake) = (scratch("agents"), friendly());
    let a = signed_in(&path, &fake);
    assert_eq!(a.ensure_agent("1789126979", "Scout").as_deref(), Some("wa-ada-1789126979"));
    assert_eq!(a.ensure_agent("1789126979", "Scout").as_deref(), Some("wa-ada-1789126979"));
    let registrations = fake.requests().iter().filter(|r| r.1.ends_with("/v1/agents")).count();
    assert_eq!(registrations, 1, "asked once, then remembered");
    let again = Account::open(path.clone(), Box::new(Fake::default()));
    assert_eq!(again.agent_ids().get("1789126979").map(String::as_str), Some("wa-ada-1789126979"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn signed_out_or_unreachable_an_agent_simply_has_no_workspace_id_yet() {
    let out = Account::open(scratch("ag-out"), Box::new(friendly()));
    assert_eq!(out.ensure_agent("1", "Scout"), None);
    let (path, fake) = (scratch("ag-down"), friendly());
    let a = signed_in(&path, &fake);
    let down = Account { transport: Box::new(Fake::default()), ..a };
    assert_eq!(down.ensure_agent("1", "Scout"), None);
    assert!(down.agent_ids().is_empty(), "nothing is invented, and it will be asked about again");
}

// MARK: - What goes out: no view state, ever (p2-identity.md §7)

/// A core whose seat is looking at plenty: a focus, a filtered list, a parked question.
fn a_seat_with_a_lot_on_screen() -> AppState {
    let mut st = AppState::new();
    let ctx = crate::state::tests::ctx();
    st.add_company("Acme Corp", &ctx);
    st.add_company("Acme Industries", &ctx);
    let deal = st.add_deal("Acme renewal", None, None, None, &ctx);
    st.show(&deal).unwrap();
    st.command(&json!({ "cmd": "find", "query": "acme", "status": "open", "kind": "deal" }), None, &ctx);
    st.command(&json!({ "cmd": "log", "kind": "note", "handle": "acme", "body": "parked" }), None, &ctx);
    assert!(st.update(crate::state::tests::now(), &[])["view"]["pending"].is_object(), "the fixture must really have a parked question");
    st
}

#[test]
fn nothing_the_account_sends_carries_view_state_or_anything_the_contract_does_not_name() {
    let _on_screen = a_seat_with_a_lot_on_screen();
    let (path, fake) = (scratch("wire"), friendly());
    let a = Account::open(path.clone(), Box::new(fake.clone()));
    a.sign_up("https://crm.example", "ada@example.com", &Secret::new(PASSWORD), "Ada", "Analytical").unwrap();
    a.sign_in("https://crm.example", "ada@example.com", &Secret::new(PASSWORD)).unwrap();
    a.accept_invite("https://crm.example", &Secret::new(INVITE), "ada@example.com", &Secret::new(PASSWORD), "Ada").unwrap();
    a.ensure_agent("1789126979", "Scout");
    a.create_invite(Some("grace@example.com")).unwrap();
    a.members().unwrap();
    a.whoami().unwrap();
    a.sign_out().unwrap();

    let allowed: &[&str] = &[
        "email", "password", "name", "orgName", "device", "token", "localAgentId",
    ];
    let forbidden = ["focus", "focused", "list", "board", "cards", "pending", "view", "query", "stage", "status", "rows", "candidates"];
    let mut bodies = 0;
    for (_, url, _, body) in fake.requests() {
        let Some(Value::Object(o)) = body else { continue };
        bodies += 1;
        for key in o.keys() {
            assert!(allowed.contains(&key.as_str()), "{url} sent `{key}`, which the contract does not name");
            assert!(!forbidden.contains(&key.as_str()), "{url} sent view state `{key}`");
        }
    }
    assert!(bodies >= 5, "the test must really have inspected the requests ({bodies})");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// MARK: - What must never come out

/// Every text a surface can show — the pushed update, the bootstrap, every response, the CLI's own
/// words for every verb — with a signed-in account holding secrets. None may contain one.
#[test]
fn no_token_password_or_invite_reaches_a_snapshot_a_response_or_the_clis_output() {
    let (path, fake) = (scratch("leak"), friendly());
    let account = Account::open(path.clone(), Box::new(fake));
    account.accept_invite("https://crm.example", &Secret::new(INVITE), "ada@example.com", &Secret::new(PASSWORD), "Ada").unwrap();
    account.ensure_agent("agent-1", "Scout");

    let (person, org) = account.identity().unwrap();
    let mut st = AppState::new();
    st.set_account(Some((person, org)), &Person::default());
    st.set_workspace_agents(account.agent_ids());

    let mut seen = String::new();
    let secrets = [TOKEN, PASSWORD, INVITE, "SECRET"];
    let mut take = |label: &str, v: &Value| {
        let text = v.to_string();
        for s in secrets {
            assert!(!text.contains(s), "`{s}` reached {label}: {text}");
        }
        seen.push_str(&text);
    };
    let ctx = crate::state::tests::ctx();
    take("a bootstrap", &st.bootstrap(crate::state::tests::now()));
    take("an update", &st.update(crate::state::tests::now(), &[]));
    for (n, req) in [
        json!({ "cmd": "add", "kind": "company", "name": "Acme" }),
        json!({ "cmd": "add", "kind": "deal", "name": "Pilot" }),
        json!({ "cmd": "status" }),
        json!({ "cmd": "find", "query": "a" }),
        json!({ "cmd": "board" }),
        json!({ "cmd": "stages" }),
        json!({ "cmd": "due" }),
        json!({ "cmd": "feed" }),
        json!({ "cmd": "tasks" }),
        json!({ "cmd": "search", "q": "a" }),
        json!({ "cmd": "bootstrap" }),
        json!({ "cmd": "state" }),
    ]
    .into_iter()
    .enumerate()
    {
        let out = st.command(&req, Some("agent-1"), &ctx);
        take(&format!("the response to {n}"), &out.resp);
    }
    assert!(seen.len() > 1000, "the test must have looked at something");
    assert!(seen.contains("wa-ada-agent-1"), "…and what it looked at really did carry the workspace id");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_secret_cannot_be_debug_printed_or_serialised_through_the_account() {
    let (path, fake) = (scratch("dbg"), friendly());
    let a = signed_in(&path, &fake);
    for text in [format!("{a:?}"), format!("{:?}", a.status()), serde_json::to_string(&a.status()).unwrap()] {
        assert!(!text.contains("SECRET"), "{text}");
    }
    let stored = a.lock().clone().unwrap();
    assert!(!format!("{stored:?}").contains("SECRET"));
    assert!(serde_json::to_string(&stored.token).is_err(), "a Secret refuses to serialise");
    // The password is never kept at all — not in memory structures, not in the file.
    assert!(!std::fs::read_to_string(&path).unwrap().contains(PASSWORD));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// `nothing in this module prints`: a print or log macro here is a leak waiting for a value.
#[test]
fn the_account_module_has_no_way_to_print() {
    let src = include_str!("account.rs");
    for (n, line) in src.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        for banned in ["println!", "eprintln!", "print!(", "eprint!(", "dbg!(", "tracing::", "log::", "write!(std::io::stderr"] {
            assert!(!t.contains(banned), "account.rs:{} uses `{banned}`", n + 1);
        }
    }
}

// MARK: - Two seats

/// Two people's installs, each with its own account and its own core. The same local agent number
/// is two different workspace agents, and neither seat's roster, attribution or data names the other's.
#[test]
fn one_seats_agents_never_appear_as_anothers() {
    let server = |user: &'static str| {
        Fake::answering(move |_, url, body| {
            if url.ends_with("/v1/agents") {
                let local = body.and_then(|b| b["localAgentId"].as_str()).unwrap_or("");
                return Ok((200, json!({ "workspaceId": format!("wa-{user}-{local}"), "localAgentId": local, "name": "Scout" })));
            }
            Ok((200, session(user)))
        })
    };
    let (pa, pb) = (scratch("seat-a"), scratch("seat-b"));
    let (a, b) = (Account::open(pa.clone(), Box::new(server("ada"))), Account::open(pb.clone(), Box::new(server("grace"))));
    a.sign_in("https://x", "ada@example.com", &Secret::new(PASSWORD)).unwrap();
    b.sign_in("https://x", "grace@example.com", &Secret::new(PASSWORD)).unwrap();
    // Both installs have a Clatch agent numbered 1789126979.
    let (wa, wb) = (a.ensure_agent("1789126979", "Scout").unwrap(), b.ensure_agent("1789126979", "Scout").unwrap());
    assert_ne!(wa, wb);

    let mut sa = AppState::new();
    let mut sb = AppState::new();
    sa.set_account(a.identity(), &Person::default());
    sb.set_account(b.identity(), &Person::default());
    sa.set_workspace_agents(a.agent_ids());
    sb.set_workspace_agents(b.agent_ids());
    let ctx = crate::state::tests::ctx();
    sa.command(&json!({ "cmd": "add", "kind": "deal", "name": "Pilot" }), Some("1789126979"), &ctx);
    sb.command(&json!({ "cmd": "add", "kind": "deal", "name": "Pilot" }), Some("1789126979"), &ctx);

    let (da, db_) = (sa.db(), sb.db());
    assert_eq!(da.deals[0].moved_by, identity::Actor::Agent { id: wa.clone() });
    assert_eq!(db_.deals[0].moved_by, identity::Actor::Agent { id: wb.clone() });
    let (ta, tb) = (serde_json::to_string(&sa.bootstrap(crate::state::tests::now())).unwrap(), serde_json::to_string(&sb.bootstrap(crate::state::tests::now())).unwrap());
    assert!(!ta.contains(&wb) && !tb.contains(&wa), "an agent identity crossed seats");
    assert_eq!(da.deals[0].owner_id, "user-ada");
    assert_eq!(db_.deals[0].owner_id, "user-grace");
    let _ = (Db::default(), Date::new(2026, 1, 1));
    let _ = std::fs::remove_dir_all(pa.parent().unwrap());
    let _ = std::fs::remove_dir_all(pb.parent().unwrap());
}

// MARK: - The real transport, against a real server

/// Set `CRM_TEST_SERVER=http://127.0.0.1:8787` (a `crm-server` on a scratch database) to run the
/// window's real HTTP client through the whole life of an account. Without it this says so and
/// passes: a unit run must not need a server. CI's server job sets it.
#[test]
fn against_a_real_server_an_account_is_made_used_and_ended() {
    let Ok(server) = std::env::var("CRM_TEST_SERVER") else {
        eprintln!("skipped: CRM_TEST_SERVER is not set");
        return;
    };
    let path = scratch("live");
    let a = Account::open(path.clone(), Box::new(Http));
    let email = format!("live-{}-{}@example.com", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let pw = Secret::new("a long enough password");
    let st = a.sign_up(&server, &email, &pw, "Live", "Live org").unwrap();
    assert!(st.signed_in && !st.insecure);
    assert_eq!(a.whoami().unwrap().person.name, "Live");
    assert_eq!(a.members().unwrap().len(), 1);
    let ws = a.ensure_agent("1789126979", "Scout").unwrap();
    assert_eq!(a.ensure_agent("1789126979", "Scout").unwrap(), ws);
    let invite = a.create_invite(None).unwrap();
    assert!(!invite.token.is_empty());
    assert_eq!(a.sign_out().unwrap(), None);
    // Revoked on the server: signing back in is a new session, and the old token is dead.
    assert!(a.members().is_err());
    let again = a.sign_in(&server, &email, &pw).unwrap();
    assert!(again.signed_in);
    assert!(a.sign_in(&server, &email, &Secret::new("not the password")).unwrap_err().contains("wrong email or password"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
