# Work order — p2: who you are, and which org you belong to

**Owners:** backend (server + core), frontend (sign-in and the person's identity), release
(CI with a database) · **Branches:** `p2-backend`, `p2-frontend`, `p2-release`
**Decided in:** [`p0-platform-spec.md`](p0-platform-spec.md) §5 and §7 · **Builds on:**
[`p1-the-split.md`](p1-the-split.md), [`split-contract.md`](../split-contract.md)

> **All three agents read this whole file.** p2 is the first milestone where the app talks to
> a network. It touches a server that does not exist yet, the window's first credentials, and
> CI's first service dependency. Briefed apart, each builds a third of a different thing.

---

## 1. What p2 is, and what it is not

**p2 makes identity real.** A person has an account, belongs to an organisation, and their
agents are known to that organisation by a stable id. `Actor::Human` stops being `{kind:"human"}`
and becomes a named person. The "You" disc becomes a name.

**p2 does not move record data.** Companies, contacts and deals stay where they live today.
Sharing them between people is **p3 (sync)**. Who-sees-what is **p4 (permissions)**. Keep that
line sharp: p2 answers *who*, p3 answers *what is shared*, p4 answers *who may see it*.

That split is deliberate. Identity is the foundation the other two build on, and it is the one
that carries the security surface — so it ships, gets reviewed, and settles before data starts
moving over the same wire.

## 2. Decisions made here

| | Decision | Why |
|---|---|---|
| **Server language** | **Rust**, a new workspace crate `server/` | The core's model types are Rust. Sharing them makes drift between client and server a compile error, not a bug report. |
| **Server storage** | **PostgreSQL**, migrations checked in | Decided in p0. Every table carries `org_id` from the first migration. |
| **Sign-in** | **Email + password**, argon2id, plus **invite links** | Works for self-hosters with no mail server. SSO is what a 50-person org will eventually ask for — recorded as a later order, not built here. |
| **Where credentials live** | **The window only**, stored in the app's data dir | Playbook §13. Never a CLI verb, never a snapshot, never a log line. |
| **What the agent holds** | **Nothing secret** | The agent talks to the local app through the CLI; the *app* holds the session. An agent acts as its person's seat without ever seeing a token. |
| **Local mode** | **Keeps working, unchanged** | No account means one local seat in a local org, exactly as today. Signing in is an upgrade, not a gate. This keeps v0.1.0 viable as a demo and protects every self-hoster's first five minutes. |

## 3. The model

```
Org          { id, name, created_at }
User         { id, email, name, password_hash, created_at }
Membership   { org_id, user_id, role }          role recorded: owner | member — enforced in p4
Invite       { org_id, token_hash, email?, expires_at, used_at? }   single use, expiring
Seat         { org_id, user_id, device }         one per signed-in app install
AgentIdent   { org_id, user_id, local_agent_id, name }   the workspace id for an agent
Session      { seat_id, token_hash, expires_at, revoked_at? }
```

**Tokens are stored hashed** on the server, like passwords. A leaked database must not be a
leaked set of sessions.

**Agent identity is minted by the server.** The first time a seat sees a `CLATCH_AGENT_ID`, the
app registers it and receives a workspace agent id bound to `(user, local agent)`. Attribution
uses the workspace id from then on. A rename updates the label in place, as it always has.

## 4. What changes in the contract

- **`me: { id, name }`** is real — the signed-in user. In local mode, a local person with the
  name the person typed once.
- **`Actor::Human { id }`** carries the person's id. Every activity, move and task records *which*
  human, not just "a human". Existing data migrates to the local person's id.
- **`org: { id, name }`** is the real org.
- **`agents[]`** carry workspace ids.

All of this is in the shape p1 already published; p2 fills it with real values rather than
adding fields.

## 5. Security — the first network surface this app has

Everything p0 §7 said would wake up is now awake:

- **Passwords:** argon2id, a minimum length, no maximum below 64. Login attempts rate-limited per
  account and per address.
- **Sessions:** short-lived, revocable, hashed at rest. Signing out revokes server-side, not just
  locally.
- **Invites:** single use, expiring, hashed at rest. An invite link is a credential; treat it as one.
- **Transport:** the hosted server is TLS only. A self-hoster on plain HTTP gets a loud warning in
  the window, never a silent downgrade.
- **Errors:** "wrong email or password", never which one. Account enumeration is a real attack.
- **Nothing secret in a snapshot, a log line, or a CLI answer.** The existing test that pins this
  extends to tokens and password fields.

**A security review runs before p2 merges** — the first since the project began, because until
now there was nothing to attack. QA's charter covers it, and the review is part of acceptance.

## 6. Who builds what

**Backend**
- `server/`: the model above, migrations, sign-up, sign-in, sign-out, invite create/accept,
  agent registration, seat registration. Session tokens.
- The core: `me`, `Actor::Human { id }`, workspace agent ids; local mode unchanged; migration of
  existing data to the local person's id.
- The app: hold the session in the data dir, attach it to server calls, never expose it.

**Frontend**
- Sign up, sign in, sign out, accept invite — in the window, never anywhere else.
- Settings gains **Account** and **Organisation**: your name, the org's name, its members
  (names only — roles are p4), and invite creation.
- The "You" disc becomes the person's name and a real identity colour, everywhere attribution
  appears.
- Team stops being a stub only as far as listing members — no performance view yet.

**Release**
- CI gains a PostgreSQL service so server tests run on every push.
- `verify` runs the server's tests too.
- A dev script that starts a local server for the app to talk to.

## 7. The test p1 could not run

QA round 7 could not verify that one person's view never reaches another, because there was
only one person. **p2 makes this testable, and it is part of acceptance:**

- Two users in one org, each with a seat. Prove that nothing either seat sends to the server
  carries `focus`, `list`, `board`, `pending` or any other view state.
- Prove one seat's agent identity never appears as another seat's.

Record data still is not shared in p2, so the full "two people on one board" test lands in p3.
This one is about the boundary: view state must never be put on the wire in the first place.

## 8. Acceptance

- [ ] sign up, sign in, sign out and accept an invite, from the window; none from the CLI
- [ ] local mode works exactly as before with no account
- [ ] `me`, `Actor::Human { id }` and workspace agent ids are real, and existing data migrates
- [ ] passwords, sessions and invites hashed at rest; sign-out revokes server-side
- [ ] login rate-limited; errors never reveal which half was wrong
- [ ] no token or password in any snapshot, log line or CLI output — pinned by a test
- [ ] the two-seat boundary test in §7 passes
- [ ] CI runs server tests against a real PostgreSQL; `npm run verify` green
- [ ] a security review is completed and its findings resolved or accepted in writing

## 9. Do not

- **Do not move record data between people.** That is p3.
- **Do not enforce roles.** Record them; p4 enforces.
- **Do not build SSO.** Note it as a later order.
- **Do not let credentials touch the CLI**, a snapshot or a log line.
- **Do not make sign-in mandatory.** Local mode stays.
- **Do not build a web client.** Parked by the product owner.
