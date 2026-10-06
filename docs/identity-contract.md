# Identity — the contract (p2, backend)

**Order:** [`work-orders/p2-identity.md`](work-orders/p2-identity.md) · **Code:** `crates/identity`
(shared types and wire), `server/` (Rust, axum, PostgreSQL), `src-tauri/src/account.rs` (the window's
session), `src-tauri/src/state.rs` (`actor`, `become_person`, workspace agents).

## What changed in the shape

| | Before | Now |
|---|---|---|
| `Actor::Human` | `{kind:"human"}` | `{kind:"human", id}` — the person's id. Old records have no id and are stamped with the opening person's on open (saved). |
| `seat.me` | the local person | the signed-in user `{id, name}`; signed out, the local person (`seat.json`) |
| `org` | the dataset's | the account's `{id, name}` when signed in; the dataset's own (and every row's `org_id`) is untouched — p2 moves no data |
| `seat.agents[].id` | the local `CLATCH_AGENT_ID` | the **workspace** id once the server minted one; the local id before that and always in local mode |
| `Actor::Agent{id}` | local id | the same workspace id, so attribution and the roster agree |

A filter whose actor is `{kind:"human"}` with no id means **me** (`resolve_actor`).
Signing in re-stamps what the local person did to the new id (`become_person`), so the data stays
theirs; signing out stamps it back. Both go through the one commit point.

## The window's account (Tauri commands — window only)

`account_status`, `account_sign_up`, `account_sign_in`, `account_accept_invite`, `account_sign_out`
(→ optional warning if the server could not be told), `account_create_invite` (→ `{token, expiresAt}`,
the token shown **once**), `account_members` (→ `[{id, name}]`). `account_status` →
`{signedIn, server, person, org, role, expiresAt, insecure}` — **never the token**. `insecure` is true
for plain `http://` to anything but this machine: the window must say so loudly.

None is a CLI verb, none is an envelope `Core::command` answers (tests pin both), so an agent cannot
reach any of it. The session is `session.json` in the app's data dir, owner-only, atomic.

## The server (`server/`)

`POST /v1/signup | /v1/signin | /v1/signout | /v1/invites | /v1/invites/accept | /v1/agents`,
`GET /v1/me | /v1/org/members | /v1/agents`, `GET /healthz`. JSON, `Authorization: Bearer`.
Environment: `DATABASE_URL` (required), `BIND`, `CRM_REQUIRE_TLS`, `CRM_TRUST_PROXY`. Migrations are
`server/migrations/*.sql`, applied in order on start. Every table has `org_id` (`orgs`' own `id` is it).
Tests need a PostgreSQL: `DATABASE_URL=postgres://… cargo test` in `server/` (each test gets its own
schema). The app's live test runs when `CRM_TEST_SERVER=http://127.0.0.1:8787` is set.

Data model = the order's §3 (`Org, User, Membership, Invite, Seat, AgentIdent, Session`) plus
`login_attempts` for rate limiting. One home org per user in p2 (`users.org_id`).

## Security properties, and what pins each

| property | pinned by |
|---|---|
| passwords argon2id, 10–256 chars | `the_password_policy_is_enforced`, `…hashed_at_rest` |
| sessions/invites stored as SHA-256 of 256-bit tokens; a dump of every table contains none | `passwords_sessions_and_invites_are_hashed_at_rest` |
| session expires; sign-out revokes **server-side** | `…sign_out_revokes_it_on_the_server`, `an_expired_session_stops_working` |
| invite single-use, expiring, claimed atomically; a wrong address does not burn it | `an_invite_is_single_use…`, `…not_burned_by_the_wrong_one` |
| login rate-limited per account **and** per address, in the database | `login_is_rate_limited_…` |
| wrong email and wrong password are indistinguishable (same status, same body; an unknown address still verifies a dummy hash) | `a_wrong_email_and_a_wrong_password_are_indistinguishable` |
| TLS-only mode refuses plain requests | `tls_only_refuses_plain_requests…` |
| nothing secret in a snapshot, response, CLI output; `Secret` has no `Debug`/`Display`/`Serialize`; `account.rs` has no print macro | `no_token_password_or_invite_reaches_…`, `no_credential_reaches_what_the_cli_prints`, `a_secret_cannot_be_debug_printed…`, `the_account_module_has_no_way_to_print` |
| no CLI verb, no core envelope, nothing in the manual | `the_cli_has_no_way_to_sign_in_out_or_touch_a_credential` |
| no view state ever on the wire (§7): the wire types have no such field, every request refuses unknown fields | `no_endpoint_accepts_view_state_from_a_seat`, `nothing_the_account_sends_carries_view_state…` |
| one seat's agent identity is never another's | server: `a_workspace_agent_id_is_minted_per_user…`; app: `one_seats_agents_never_appear_as_anothers` |

## Known limits — for the security review

1. **Sign-up reveals that an address is taken** (409). Without email verification this cannot be
   avoided; it shares the per-address rate limit. Mail verification is a later order.
2. **Without a trusted proxy header every client shares one address bucket**, so the per-address
   limit degrades to a global one (the per-account limit is unaffected). Set `CRM_TRUST_PROXY=1`
   only behind a proxy you control — otherwise `X-Forwarded-For` is attacker-chosen.
3. **TLS is the proxy's job.** The server never terminates TLS; `CRM_REQUIRE_TLS=1` makes it refuse
   anything not forwarded as `https`, and trusts that header, so it must sit behind a proxy that sets it.
4. **The connection to PostgreSQL is `NoTls`.** Fine on a private network or a socket; not across the internet.
5. **The session token is on disk in clear** (`session.json`, 0600). The OS keychain is the better home; recorded as a follow-up.
6. **Sign-out offline** destroys the local session but cannot revoke on the server; the app says so, and the token expires in 12 h.
7. **Roles are recorded, never read.** Any member can create an invite in p2; p4 decides who may.
8. Not built, by order: SSO, mail verification, password reset, a web client.
