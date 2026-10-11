# QA round 8 — W1 (watchable) and the p2 security review

**Reviewed:** 7 October 2026 · **By:** QA · **Build:** `main` @ `cf7c721`
**W1:** the 10-second test **passes both runs**, but one MAJOR in the same work order's own rule sends it back.
**p2:** **send back** — three MAJORs in the server/app code that is on `main`, one of which is a default configuration.
**Previous:** [`round-7.md`](round-7.md)

Findings only. Nothing was fixed, merged, tagged or published. Everything ran under isolated `HOME`s, with a private
PostgreSQL cluster, an isolated Clatch daemon and a bound `debug` agent; none of your real agents or apps were touched.

**The product owner's demo data.** Before anything ran I copied `~/.clatch/appdata/com.breksos.crm/crm.json` and
`~/.crm/seat.json` aside (`/tmp/q8backup/`). I installed nothing into the real Clatch, so there was nothing to restore:
at the end the live files and the copies have identical SHA-1s (`728bde1a…` and `0835055b…`). One slip to report: a
helper script I wrote without setting `HOME` tried to connect to `~/.crm/crm.sock`; it was refused (a stale socket) and
sent nothing. The scratch PostgreSQL ran on port 55432 using the Quayt runtime's *binaries* with its own data directory;
its port 54329 and data were never used.

| | Verdict | The one sentence |
|---|---|---|
| W1 — 10-second test, Follow on | **pass** | 23 calls, 23 lines in the window, in order, each in the right place; the ambiguity was answered from the window. |
| W1 — 10-second test, Follow off, panel collapsed | **pass** | The same 23 lines, in order; the page stayed put, the rail followed the record, the dot lit. |
| W1 — never lose unsaved work | **MAJOR** | A half-written note travelled with the panel to the record the agent opened, and was saved onto the wrong record. |
| p2 — enumeration, sessions, invites, hashing, TLS gate | **pass** | Each held under attack. |
| p2 — rate limiting | **MAJOR ×2** | By default 20 bad logins lock everyone out; behind a proxy the address limit is attacker-chosen. |
| p2 — plain-HTTP safeguard | **MAJOR** | The warning comes after the password has gone, and a crafted URL gets no warning at all. |
| The 8 known limits | 6 accept · 1 fix · 1 accept-with-condition | Table in §3. |

---

## Rule zero — `main` @ `cf7c721`

| Command | Result |
|---|---|
| `cargo test` (app) | **445 passed, 0 failed** |
| server tests (`npm run test:server`, via `verify`, real PostgreSQL) | **17 passed, 0 failed** |
| `cargo clippy --all-targets` — app / server | clean on our code (the same upstream `block v0.1.6` warning) / **clean** |
| `cargo fetch --locked` — app / server | exit 0 / exit 0 |
| `npx tsc --noEmit` / `npm test` | clean / **133 passed, 0 failed** |
| `npm run verify` | **green** — "the two surfaces agree" |

`npm run dev:server` itself worked as written (throwaway PostgreSQL, then the server on 127.0.0.1:8787).

---

## 1. W1 — the 10-second test

**How it was run, and what that is worth.** A bound agent in an isolated Clatch ran a 25-step script with `sleep 10`
between calls, using **every verb in `crm -h`** — `status`, `add` (company, contact, deal), `task`, `log`, `set`, `link`,
`move`, `find`, `show` (ambiguous), `board`, `due`, `stages`, `export`, `import`, `done`, `archive` and `--restore`, one
refusal (`move … nowhere`, exit 1), `select`, `focus`, `close`. The "person watching only the window" was the shipped
React window, run in a browser against the real core, with an automatic observer reading what is on screen once a second
and **clicking the first candidate of the ambiguity six seconds after it appeared**. The agent is a scripted roster
agent, not a model; the window is not the Tauri webview; and the core's push to the webview is emulated by polling every
400 ms (steps are 10 s apart, so the lag is invisible). The core, the CLI, the feed and the window code are real.

A first attempt counted only lines that are buttons and missed the ones without a target; I discarded it and reran.

### Run 1 — Follow on, panel open

What the window showed, step by step (time is seconds into the script):

| # | Call | The window |
|---|---|---|
| 1 | `status` | line `status`; stays on Home |
| 2 | `add company 'Orbit Line'` | line; goes to **Contacts & companies**, record open |
| 3 | `add company 'Orbit Labs'` | line; record Orbit Labs |
| 4 | `add contact 'Ines Moreau' --company orbit-line` | line; record Ines Moreau |
| 5 | `add deal 'Orbit monitoring' …` | line; goes to **Pipeline**, deal open |
| 6–10 | `task`, `log call`, `set orbit-line domain`, `link`, `move … proposal` | one line each, in order; page and record follow (`set` goes to the company on Contacts, the rest to the deal on Pipeline) |
| 11 | `find orbit` | line; goes to **Deals** |
| 12 | `show orbit` | line marked **asked**; the question "which “orbit”?" shown; **answered from the window**, which opened Orbit Labs — and the answer added no line (the person's own action) |
| 13–17 | `board`, `due`, `stages`, `export deals`, `import contacts (2 rows)` | one line each; `board` → Pipeline, `due` → Inbox; the rest stay where they are |
| 18–20 | `done`, `archive`, `archive --restore` | one line each, following to the record |
| 21 | `move orbit-monitoring nowhere` | line marked **refused**, with the exact sentence the agent saw: `“nowhere” is not a stage — use one of lead, qualified, p…`, styled as an alert |
| 22–23 | `move … won`, `select 1` | one line each |
| 24–25 | `focus`, `close` | **no line** (see below) |

**23 calls, 23 lines, the same order, no extras, none missing, the refusal as visible as a success.** No id and no
ULID-shaped string in any line.

### Run 2 — Follow off, panel collapsed

Follow switched off in the desk and the panel collapsed with its control, then the same script. **The same 23 lines in
the same order.** The page stayed on Home throughout; the rail followed the shared focus (`Orbit Line`, `Orbit Labs`,
`Ines Moreau`, `Orbit monitoring`, … `Imp Two`, back to `Orbit monitoring`) and showed the **unread dot** from step 6
(the first call to touch the open record) onward; the ambiguity banner appeared and was answered the same way; the
refusal line carried its message. The collapsed state persisted in `localStorage`.

Also checked: an agent **cannot** flip Follow (`following is the person's choice, made in the window`); the person can.
No "ask the agent" control. The `crm -h` intro carries the new line ("Everything you run shows, step by step, in the
person's window.").

### MAJOR — a half-written note follows the agent to a different record, and is saved there

`src/follow.ts` / `src/Record.tsx` (the panel), `src/dirty.ts` — the work order: *"Never take the person out of unsaved
work. If a form … is dirty, hold the navigation and show a chip."*

With Follow on, a record open (Alpha Co) and a half-written note typed in its timeline field, the agent ran
`crm show beta-co`:

```
chip shown:           "Agent opened beta-co — go"                  ← the page navigation was held
record panel:         Beta Co                                       ← but the panel swapped
the typed text:       "half-written note…" still in the field       ← and travelled with it
person presses Add:   the note appears on Beta Co's timeline        (not on Alpha Co)
```

The chip holds the *page*; the *record* underneath the form is not held. The person's draft, meant for one record,
lands on another — the nearest this feature comes to writing a person's words into the wrong customer. (I did not
establish whether a person who clicks another row by hand has the same carry-over; that would make it older than W1.)

### MINOR — `import` is the one agent write that does not bring the person to its result

With Follow on, `add contact` lands on Contacts with the new row visible and the record open (the 2026-10-07 amendment).
`import contacts (2 rows)` left the window on the page it was on (Inbox, from `due`), its line unclickable, the new
people one nav click away. The amendment says watching an agent should never need a click.

### MINOR — `focus` and `close` leave no line

Both are clappkit window verbs answered before the app's state sees them, so the rule "every call … produces one line"
has two exceptions the order does not name. `close` ends the window, so a line there would be moot; `focus` raises it.
Worth one sentence in the order.

### MINOR — the unread dot says something changed, not what

In run 2 the dot lit at step 6 and stayed lit while the rail moved through six other records, including ones the agent
had only just created. I could not tell, from the rail, which record it referred to.

---

## 2. p2 — the security review

Attacked with `npm run dev:server`'s server (debug build) and a release build, on a throwaway database, from scripts
that speak to it directly, plus instances configured for trusted-proxy and TLS-only modes.

### What held

- **Account enumeration (sign-in).** An unknown address and a wrong password give the same status, the same body and
  the same header names, and — measured over repeated attempts — the same time (237 ms vs 240 ms median, debug build).
  An invite accepted for an address that already has an account answers exactly like a bogus token, and does **not**
  burn the invite.
- **Invites.** Reuse → `that invite is not valid`. Thirty concurrent accepts of one link: exactly **one** account
  created. A bound invite is not burned by the wrong address and works for the right one, compared case- and
  space-insensitively. An expired invite (set in the past in the throwaway DB) is refused. A plain member can create
  invites (limit 7, by order); an invitee lands in the inviter's org only.
- **Sessions.** Sign-out revokes **server-side** (the token 401s everywhere; a second session of the same person
  survives); signing out twice is a clean 401; an expired session 401s with `sign in again`; the lifetime is a fixed
  12 h. Only an exact `Bearer <token>` is accepted — lowercase scheme, `Token`, trailing junk and a `?token=` query
  string all 401. One org's token sees only that org's members; the same local agent id in two orgs gives two different
  workspace ids, and `/v1/agents` lists only the caller's own.
- **At rest.** I dumped every table (`pg_dump --data-only`) after creating accounts, sessions and invites: none of the
  session tokens, the invite token or either password appears; passwords are `$argon2id$v=19$m=19456,t=2,p=1$…`.
- **Logs, snapshots, CLI.** Five server instances' logs contain no secret and no request line (the only lines are
  start-up, a TLS-off warning and "internal error", which never fired). With the app signed in (a real session placed in
  its data dir), a search of everything under its `HOME`, its log, every CLI verb's output, an export and the socket's
  `bootstrap`, `status`, `feed`, `tasks`, `changes` and `search` answers found the token **only** in `session.json`
  (mode `0600`); it is not in the process arguments; `account_*` are not envelopes (`is not a verb this build answers`).
  A live run of the app's own HTTP client against the server passed.
- **Injection and shape.** Three SQL-injection strings in the email, name, device and token fields: ordinary refusals,
  `users` intact. A 70 KB body → 413. An unknown field (including `focus`, i.e. view state) → 422. Not JSON → 400. No
  endpoint accepts view state, which is the §7 boundary.
- **TLS-only mode.** A plain request, including `/healthz` and `GET`/`HEAD`/`OPTIONS`, → **426**. `X-Forwarded-Proto: http`
  and a mixed `https, http` list → 426; an RFC 7239 `Forwarded: proto=https` is ignored (fails closed).

### MAJOR — by default, twenty bad logins from anyone lock every sign-in and sign-up for fifteen minutes

`server/src/lib.rs` `client_address` / `check_not_limited` — known limit 2, but its consequence is larger than the
document says.

Without `CRM_TRUST_PROXY` every client is the single address `direct`, and the per-address limit is 20 failures per 15
minutes. Run against the default server:

```
22 wrong logins for 22 different nonexistent accounts →  401 ×20, then 429 429
an unrelated person, correct password                 →  429  "too many attempts — wait a few minutes"
a brand-new sign-up from anyone                        →  429
```

One script, twenty requests, takes the whole server's front door down, and repeating it every fifteen minutes keeps it
down. The document calls this "degrades to a global limit"; in effect it is a denial of service on the default
configuration. The connection's own peer address is available to the server and would give each client its own bucket.

### MAJOR — behind a proxy, the address limit is chosen by the attacker

`client_address` takes the **first** `X-Forwarded-For` entry. A proxy that appends keeps whatever the client sent first,
so the "client address" is whatever the attacker says. With `CRM_TRUST_PROXY=1`:

```
40 wrong logins, a different spoofed X-Forwarded-For each     →  401 ×40, never limited
20 failures claiming to be 198.51.100.9 (then the real person at that address, correct password)  →  429
```

The attacker is never throttled by address, and can lock out any address they name. The documented safe deployment
("only behind a proxy you control") is therefore not safe; the setting needs the **last** entry, or a configured number
of trusted hops. The per-account limit is unaffected, so this does not open one account to unlimited guessing — it
removes the only defence against guessing *across* accounts (credential stuffing).

### MAJOR — the plain-HTTP warning comes after the password, and a crafted URL gets none

`src-tauri/src/account.rs:119` `is_insecure`, and (unmerged) `origin/p2-frontend` `AccountPanel.tsx`.

The order says a self-hoster on plain HTTP "gets a loud warning in the window, never a silent downgrade". Two problems:

1. **When.** `insecure` is computed from the *stored session* (`Status`), so the banner appears only **after** a sign-in
   has succeeded — after the password, and the new session token, have crossed the network in clear. `sign_in`,
   `sign_up` and `accept_invite` send first and never ask.
2. **What it recognises.** The check strips the scheme and cuts the authority at the first `:`, so userinfo is read as
   the host. In a scratch test (reverted):

   ```
   http://127.0.0.1:80@evil.example       is_insecure = false   (the host is evil.example)
   http://localhost:x@evil.example        is_insecure = false
   http://[::1]:80@evil.example           is_insecure = false
   ```

   Those URLs get **no warning at all**. `http://localhost.evil.example` and `http://127.0.0.1.evil.example` are
   correctly flagged. (`http://a.localhost` is treated as local; on a resolver that does not special-case `.localhost`
   that name can resolve anywhere.)

Note also that the window's sign-in/sign-up/invite screens are **not on `main`** — `origin/p2-frontend` (5dc7f74) is
unmerged — so acceptance item 1 ("from the window") cannot be checked on `main`; this finding was read from that branch.

### MINOR — a burst races the limit: about three times as many guesses as the limit says

`check_not_limited` reads the count, then verifies the password, then records the failure; nothing joins the three.
Against one account with a limit of 5, bursts of 20, 100 and 300 wrong passwords evaluated **17, 16 and 16** (the pool's
16 connections are the ceiling); a burst of 200 invite guesses against a limit of 20 evaluated 35. Sequentially the limit is exact
(five 401s, then 429). A burst containing the *correct* password after the limit was reached was still refused. The
excess is bounded and small against 256-bit invites and ten-character passwords, but "5 per 15 minutes" is really "16".

### MINOR — anyone can lock a named person out

Five wrong passwords for an address shut that account's door, and the **correct** password is refused too (`429`) for
fifteen minutes. The classic per-account trade-off; the cost here is that nothing but waiting lifts it.

### MINOR — an unauthenticated burst slows signed-in people

The handlers run argon2 on the async workers. On the release build (about 18 ms per login), 1 000 logins with rotating
addresses took 3.6 s, during which a signed-in `/v1/me` took **2.5 s**; `/healthz` stayed at ~7 ms, so a health check
will not notice. With the default shared-address limit the burst is cut off after 20, which is the previous MAJOR's
other face.

### MINOR — successful sign-ups are not limited

Thirty sequential sign-ups each created a user and an org, all `200`. Only *failures* count, so there is no throttle on
filling the database. (Combined with limit 1 this is also an enumeration oracle with side effects.)

### MINOR — no way to end a stolen session, and no password change

There is no route to list sessions or sign out everywhere (`/v1/sessions`, `/v1/seats`, `/v1/signout-all`: 404), no
password change or reset, and every sign-in mints a seat that is never removed. A stolen token lives for its 12 hours.

### MINOR — smaller

- Token-bearing responses carry no `Cache-Control: no-store`.
- `npm run dev:server` exports `CRM_SERVER_ADDR`, which the server never reads (it reads `BIND`); setting the former
  has no effect and the script prints an address the server is not on. The script's default PostgreSQL port, 54329, is
  the one the Quayt runtime uses on this machine; I avoided it with `PG_DEV_PORT`.
- A malformed body is answered with the parser's own text (`unknown field …, expected one of …`), which names the
  fields.

---

## 3. The eight known limits — accept or fix

| # | Limit | Rating | Why |
|---|---|---|---|
| 1 | Sign-up reveals that an address is taken (409) | **accept** | Confirmed, shares the failure limit, and cannot be closed without mail verification. Add a throttle on *successful* sign-ups (above). |
| 2 | Without a trusted proxy every client shares one address bucket | **fix** | Not "degrades": twenty requests take the front door down for everyone for fifteen minutes (MAJOR above). Key on the connection's peer address when no proxy is trusted; take the last forwarded entry otherwise (second MAJOR). |
| 3 | TLS is the proxy's job; the server trusts `X-Forwarded-Proto` | **accept** | Verified the gate fails closed (plain, mixed lists, `Forwarded:` all 426). It is only as good as the rule that the server is reachable *only* through the proxy; say so in the run book. |
| 4 | PostgreSQL connection is `NoTls` | **accept for p2** | Local and socket use is fine. Make it a tripwire — refuse a non-loopback host without TLS — before p3 puts a database across a network. |
| 5 | Session token on disk in clear | **accept** | `session.json` is `0600`, atomic, and the only place the token appears (not in arguments, logs, snapshots or CLI output). Move it to the OS keychain before a release that is not a demo. |
| 6 | Sign-out offline cannot revoke | **accept, with a condition** | The 12 h bound and the honest message are enough *if* a stolen session can be ended some other way; today it cannot (no sign-out-everywhere, no password change). Pair with that. |
| 7 | Roles recorded, never read | **accept** | Verified: any member can invite, an invitee joins as a member; record data is not shared in p2, so nothing is exposed. p4's job. |
| 8 | Not built: SSO, mail verification, password reset, web client | **accept** | By order. Note that without a reset, a forgotten password is permanent. |

## 4. The two-seat boundary (§7)

Verified at the wire: every request type refuses unknown fields (`focus` → 422), no endpoint takes view state, a
member's `/v1/me` and member list never include another org, and the same local agent id gives different workspace ids
per user and org, with `/v1/agents` scoped to the caller. The app's own tests for the same boundary pass. I did not run
two live app seats against one server.

## What would change my verdict

1. **W1:** keep the record panel (and any form in it) on the record the person was on while a draft is dirty — the chip
   should hold the *focus change*, not only the page. Add a test: dirty form, agent `show` elsewhere, submit, assert the
   note's record.
2. **p2:** fix `client_address` both ways (peer address by default; last forwarded entry or a hop count when trusting a
   proxy); make the plain-HTTP check parse the URL's host properly and run it **before** a credential is sent; then rerun
   the two bursts. Close the check-then-record race by recording the attempt first, or by locking per key.
3. Then, in order: throttle successful sign-ups; add sign-out-everywhere and a password change; make `import` land the
   person on its result; make the dev script and the server agree on `BIND`.

## What this round did, and did not do

**Did:** rule zero (app, server, window, `verify`); both 10-second runs with a bound agent and a window observer; the
dirty-form check; a read of the feed's rules against the live feed for every verb; the p2 attacks above against a
running server, including bursts, spoofed headers, a TLS-only instance and a release build; a plaintext-secret hunt
across the database, five server logs, the signed-in app's files, CLI output and socket answers; the owner's-data
backup and checksum.

**Did not:** fix, merge, tag or publish anything; drive the window in the Tauri webview; run an LLM agent; install into
the real Clatch; run two live app seats; test the p2 frontend's screens (not on `main`); check whether a person who
switches records by hand carries a draft the same way; time the default-profile argon2 settings against a hardware
budget; test TLS itself (the server never terminates it).
