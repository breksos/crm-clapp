//! Identity: the types the core and the server **share**.
//!
//! Sharing them is the point (`docs/work-orders/p2-identity.md` §2): a client and a server that
//! disagree about who an [`Actor`] is, or about what a sign-in request carries, disagree at
//! compile time, not in a bug report.
//!
//! **What never lives here:** view state. Nothing in this crate — and therefore nothing either
//! side can put on the wire — can express `focus`, `list`, `board` or `pending`. Every request
//! type also refuses unknown fields on the server, so a client that tried would be turned away
//! (`p2-identity.md` §7).

use serde::{Deserialize, Serialize};

pub type OrgId = String;
pub type PersonId = String;

/// The org every row belongs to until an account says otherwise.
pub const LOCAL_ORG: &str = "local";

/// The person at a seat nobody has named. Local mode mints a real id; this is only the
/// default a test or an unconfigured core falls back to.
pub const LOCAL_PERSON: &str = "me";

pub fn local_org() -> OrgId {
    LOCAL_ORG.to_string()
}

// MARK: - Who did a thing

/// Who did a thing: a named person, or a named agent.
///
/// **Both are keyed on an immutable id, never a display name** — a rename must not silently
/// re-attribute everything someone ever wrote. The window looks the name up to draw it.
///
/// `Human { id }` is p2: it used to be a bare `{kind:"human"}`. A record written before that has
/// an **empty** id, which the core's migration stamps with the local person's. An empty id in a
/// *filter* means "me" (the seat's own person) — see the core's `resolve_actor`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Actor {
    Human {
        #[serde(default)]
        id: PersonId,
    },
    /// `id` is the **workspace** agent id once the seat is signed in (minted by the server,
    /// bound to `(user, local agent)`); before that it is the local `CLATCH_AGENT_ID`.
    Agent { id: String },
}

impl Default for Actor {
    /// A record written before any field carried an `Actor` is the person's, whoever that is:
    /// the id is filled in on open.
    fn default() -> Actor {
        Actor::Human { id: PersonId::new() }
    }
}

impl Actor {
    pub fn is_human(&self) -> bool {
        matches!(self, Actor::Human { .. })
    }
}

// MARK: - The model (docs/work-orders/p2-identity.md §3)

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Org {
    pub id: OrgId,
    pub name: String,
}

impl Default for Org {
    fn default() -> Org {
        Org { id: local_org(), name: "My workspace".to_string() }
    }
}

/// Who is sitting at a seat. Not part of the dataset: it belongs to the install.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: PersonId,
    pub name: String,
}

impl Default for Person {
    fn default() -> Person {
        Person { id: LOCAL_PERSON.to_string(), name: "You".to_string() }
    }
}

/// Recorded in p2, enforced in p4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Owner,
    Member,
}

// MARK: - The wire
//
// Every request refuses unknown fields. That is a security property and a boundary property:
// a client cannot smuggle anything the contract does not name — least of all view state.

/// A secret that cannot be printed. `Debug` and `Serialize` are the two ways a value leaks into
/// a log line or a snapshot, so neither prints it: the only way to read one is [`Secret::expose`],
/// which a reader of the code can search for.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }
    /// The one way to read it. Call this only where the secret is *used* (a request header, a hash).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Serialising a secret is **refused**: a `Secret` inside something that is serialised (a snapshot,
/// a log record) fails the serialisation instead of leaking. The few places that must send one over
/// the wire write it out explicitly with [`Secret::expose`].
impl Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("a Secret is never serialised; expose() it where it is used"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignUp {
    pub email: String,
    pub password: String,
    pub name: String,
    pub org_name: String,
    /// What this install calls itself ("Berk's MacBook"). Only a label.
    #[serde(default)]
    pub device: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignIn {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub device: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptInvite {
    /// The invite link's token — a credential, single use.
    pub token: String,
    pub email: String,
    pub password: String,
    pub name: String,
    #[serde(default)]
    pub device: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateInvite {
    /// Optional: bind the invite to one address.
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterAgent {
    pub local_agent_id: String,
    pub name: String,
}

/// What the server answers with once someone is in. The token is shown **once**.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub token: String,
    pub seat_id: String,
    pub person: Person,
    pub org: Org,
    pub role: Role,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhoAmI {
    pub person: Person,
    pub org: Org,
    pub role: Role,
    pub seat_id: String,
}

/// A member as another member sees them: a name and an id. **No email, no role** (roles are p4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: PersonId,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteCreated {
    /// The raw invite token, shown once; the server keeps only its hash.
    pub token: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAgent {
    pub workspace_id: String,
    pub local_agent_id: String,
    pub name: String,
}

/// An error body. One shape, and never a hint at *which half* of a credential was wrong.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

/// The only words an auth failure may use.
pub const WRONG_CREDENTIALS: &str = "wrong email or password";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_prints_and_never_serialises() {
        let s = Secret::new("tok-SECRET");
        assert!(!format!("{s:?}").contains("SECRET") && !format!("{s}").contains("SECRET"));
        assert!(serde_json::to_string(&s).is_err());
        assert!(serde_json::to_string(&Some(&s)).is_err());
        assert_eq!(s.expose(), "tok-SECRET");
    }

    #[test]
    fn an_old_human_actor_reads_with_an_empty_id_and_a_new_one_carries_it() {
        let old: Actor = serde_json::from_str(r#"{"kind":"human"}"#).unwrap();
        assert_eq!(old, Actor::Human { id: String::new() });
        let new = serde_json::to_value(Actor::Human { id: "p1".into() }).unwrap();
        assert_eq!(new, serde_json::json!({ "kind": "human", "id": "p1" }));
    }

    #[test]
    fn requests_refuse_what_the_contract_does_not_name() {
        let r = serde_json::from_str::<SignIn>(r#"{"email":"a","password":"b","focus":{"id":"x"}}"#);
        assert!(r.is_err(), "view state must not be expressible on the wire");
    }
}
