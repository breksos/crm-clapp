# QA round 9 — p2 re-check, and the W1 draft MAJOR

**Reviewed:** 12 October 2026 · **By:** QA · **Build:** `main` @ `f2d5a12` (p2-frontend, p2-sec and w1-draft merged; p3-backend also on it)
**p2:** **signed off.** All three round-8 MAJORs are fixed and the five MINORs the fixes were aimed at are fixed; the rest stay as they were, rated below.
**W1 draft MAJOR:** **fixed.**
**Previous:** [`round-8.md`](round-8.md)

Findings only. Nothing was fixed, merged, tagged or published. Everything ran under isolated `HOME`s, a private
PostgreSQL cluster (port 55432, the Quayt runtime's binaries with its own data directory — its own port and data were
never used) and an isolated Clatch daemon with a `debug` agent.

**The product owner's demo data** was copied aside first (`/tmp/q9backup/`) and I installed nothing into the real Clatch,
so there was nothing to restore. Final SHA-1s: live `crm.json` `728bde1a…`, `seat.json` `0835055b…` — identical to the
copies and to round 8.

| Round-8 finding | Round 8 | Now |
|---|---|---|
| Default config: 20 bad logins lock everyone out | MAJOR | **fixed** |
| Spoofed `X-Forwarded-For` defeats the address limit | MAJOR | **fixed** |
| Plain-HTTP warning after the password, and none for userinfo URLs | MAJOR | **fixed** |
| Concurrent burst races the limit (16–17 guesses against 5) | MINOR | **fixed** (exactly 5; exactly 20) |
| W1: a half-written note saved onto the wrong record | MAJOR | **fixed** |
| Anyone can lock a named account | MINOR | unchanged — accept |
| Unauthenticated burst slows signed-in requests | MINOR | unchanged |
| Successful sign-ups not throttled | MINOR | unchanged |
| No sign-out-everywhere, no password change | MINOR | unchanged |
| No `Cache-Control` on token responses | MINOR | unchanged |
| `dev:server` sets `CRM_SERVER_ADDR`, server reads `BIND` | MINOR | unchanged |
| Parser text in error bodies | MINOR | unchanged |

---

## Rule zero — `main` @ `f2d5a12`

| Command | Result |
|---|---|
| `cargo test` (app) | **452 passed, 0 failed** (from 445) |
| server tests (via `verify`, real PostgreSQL) | **43 passed, 0 failed** (from 17) |
| `cargo clippy --all-targets` — app / server | clean on our code (the same upstream `block v0.1.6` warning) / **clean** |
| `cargo fetch --locked` — app / server | exit 0 / exit 0 |
| `npx tsc --noEmit` / `npm test` | clean / **168 passed, 0 failed** (from 133) |
| `npm run verify` | **green** — "the two surfaces agree" |

---

## 1. The three MAJORs, rerun exactly

### Default config: the lockout — fixed

Same attack, now against the default server (no proxy configured). Two listeners on one database give two different
peer addresses (`127.0.0.1` and `::1`):

```
25 wrong logins for 25 different accounts from 127.0.0.1   →  401 ×20, then 429 ×5   (the attacker is still limited)
a person at the SAME address, correct password             →  429                    (same bucket: expected)
an unrelated person at ANOTHER address, correct password   →  200                    (was 429 in round 8)
a brand-new sign-up from the other address                 →  200                    (was 429)
```

The per-address limit is now the connection's own peer address, never a header. An attacker is limited; nobody else is
affected. The per-account limit still works sequentially (five 401s, then 429) and **being refused no longer extends the
lock**: after eight guesses the account holds exactly five attempt slots — refused attempts give theirs back.

### Spoofed `X-Forwarded-For` — fixed

`CRM_TRUST_PROXY=1` is now one trusted hop; the client is the last entry the proxy appended, and anything written
further left is never believed:

```
A. default config, a different spoofed XFF on each of 40 logins     →  20×401, 20×429   (the header is ignored; the peer limits)
B. one trusted hop, attacker prepends junk, "proxy" appends the real client
                                                                    →  20×401, 20×429   (was 40×401, never limited)
C. one trusted hop, a different real client appended each time      →  40×401, 0×429    (distinct clients, distinct buckets)
E. 25 failures claiming to be 198.51.100.9 first, real address appended; the real person at 198.51.100.9
                                                                    →  200              (was 429: an attacker could lock out any address)
F. one trusted hop, header missing                                  →  20×401, 20×429   (falls back to the peer address)
G. the attacker's entry and the proxy's on two separate header lines
                                                                    →  20×401, 10×429   (all lines are read in order; the last is the proxy's)
```

One property to put in the run book, not a defect: with a trusted hop configured, a client that reaches the server
**directly** (not through the proxy) writes the only entry, and is believed. The setting is only safe when the server
cannot be reached except through the proxy — the same condition as TLS-only mode (limit 3).

### The plain-HTTP warning — fixed, in the right order

*The gate* (`account.rs`, `check_server` and `require_confirmation`, called first by `account_sign_in`, `account_sign_up`
and `account_accept_invite`): a real URL parser, not string slicing, and the same crate (`url`) that the HTTP client
(`ureq` 2.12) uses to decide where the request goes. I ran 40 addresses through it in a scratch test (reverted):

| Address | Host it reads | Result without the person's yes |
|---|---|---|
| `http://127.0.0.1:80@evil.example` | evil.example | **refused**, userinfo flagged (round 8: no warning) |
| `http://localhost:x@evil.example` · `http://[::1]:80@evil.example` · `http://localhost@evil.example` | evil.example | **refused** |
| `https://127.0.0.1:80@evil.example` · `https://user:pw@crm.example.com` | evil.example · crm.example.com | **refused** (userinfo on its own is enough) |
| `http://crm.example.com` | crm.example.com | **refused** (plain HTTP to a stranger) |
| `http://a.localhost` · `http://localhost.` · `http://0.0.0.0` · `http://[::ffff:127.0.0.1]` | as written | **refused** (fails closed) |
| `http://127.0.0.1#@evil.example` · `…\@evil.example` · `…?@evil.example` | 127.0.0.1 | allowed — correctly: that *is* the host the client would use |
| `http://2130706433` · `http://127.1` · `http://0x7f.1` · `http://%6c%6f%63%61%6c%68%6f%73%74` | 127.0.0.1 · localhost | allowed — correctly, loopback after normalisation |
| `http://127.0.0.1:8787` · `http://localhost` · `https://crm.example.com` | as written | allowed |
| `ftp://x` · `file:///…` · `javascript:…` · `//evil.example` · `127.0.0.1:8787` · empty · `http://:80` | — | **refused**, "not a server address" |

The gate lives in the Rust command, so it holds even if a window were to skip its own check; a refusal says nothing
was sent. With the person's confirmation every one of the valid ones goes through.

*The window* (`AccountPanel.tsx`, `ServerWarning`), driven in the real window code with the Tauri calls logged:

```
server http://127.0.0.1:80@evil.example, password typed, Sign in:
   calls made:   account_check_server only — no sign-in
   shown:        "Your password would be sent to evil.example. The address has a name and password written in front of
                  the server. The request really goes to evil.example — not necessarily the server the address looks like
                  it names. It is plain HTTP, not HTTPS. … Nothing has been sent."   [Cancel] [Send it anyway]
   Cancel:       the warning closes and the password field is emptied; no account call carries the password
server http://crm.example.com   →  the same warning, naming crm.example.com
server https://crm.example.com  →  no warning; account_check_server, then account_sign_in
```

The host is shown as the client will really use it, Cancel is the default (autofocus), and the password is held only until
the person answers. (The preview's own fake core mirrors the Rust guard, so this verifies the **window's order of
operations**; the Rust guard itself is verified above.)

### The concurrent burst — fixed

Attempt slots are claimed before the password is checked and ranked by id, so an interleaving cannot let two requests
both be "the first":

```
one account, limit 5      burst of 20 / 100 / 300 wrong passwords  →  evaluated 5 / 5 / 5       (round 8: 17 / 16 / 16)
invite guessing, limit 20 burst of 200                              →  evaluated 20             (round 8: 35)
correct password inside a 60-wrong burst after the limit            →  429
```

A burst of 100 with the correct password first (fresh window) logged the person in and evaluated 20 wrong guesses: a
successful sign-in clears that account's slots (to forgive the person's own earlier typos), so a racing success reopens
the window. It needs the account's real password to trigger, so it gives an attacker nothing; I note it so nobody is
surprised by the 20.

---

## 2. W1 — a draft belongs to its record

The round-8 sequence, exactly: Follow on, Alpha Co open, a half-written note typed, the agent runs `crm show beta-co`.

```
chip shown:        "Agent opened beta-co — go"
record panel:      Alpha Co                      ← held (round 8: swapped to Beta Co)
the typed text:    "half-written note…"          ← still in Alpha's field
person presses Add:  the note is on alpha-co     (the core's feed says so)   — and the panel then goes to Beta Co,
                                                  the hold having lifted as soon as nothing was unsaved
```

And the paths around it:

```
draft on Beta, agent opens Alpha, person clicks the chip →  "You have unsaved text on beta-co. Leaving discards it —
                                                             it can only be saved there."   [Keep editing] [Discard it and go]
   Keep editing   → panel and draft unchanged, chip still offered
   Discard and go → draft cleared, panel on Alpha
person types a draft, then clicks another row by hand       →  the same question; the panel stays on the draft's record
```

The older carry-over (by hand) that I could not test in round 8 existed and is closed too.

---

## 3. What held, regression-checked on the rewritten limiter

The limiter was rewritten, so I reran round 8's passes. All held:

- **Enumeration.** Unknown address vs wrong password: one distinct status/body/header shape; 217 ms vs 215 ms median. An
  invite accepted for an address that already has an account answers like a bogus token and does not burn the invite.
- **Invites.** Reuse refused; thirty racing accepts of one link → exactly **one** account (the rest 400 or 429); a bound
  invite survives the wrong address and works for the right one; expired refused.
- **Sessions.** Sign-out revokes server-side (the other session of the same person stays live); an expired session 401s.
- **TLS-only.** Plain → 426 (including `/healthz`); `X-Forwarded-Proto: http` and `https, http` → 426; `Forwarded:
  proto=https` ignored.
- **At rest and in logs.** A full `pg_dump` contains none of a session token, an invite token or a password (argon2id,
  `m=19456,t=2,p=1`); five server instances' logs hold no secret and no internal errors.
- **Injection and shape.** SQL strings refused normally with `users` intact; 70 KB → 413; an unknown field (`focus`) → 422.

---

## 4. The round-8 MINORs that were not in the fix — still open

Rerun, nothing changed:

- **Successful sign-ups are not throttled** — 40 sequential sign-ups from one address, all `200`, 40 users and orgs (the
  new limiter releases a successful sign-up's slot).
- **An unauthenticated burst slows signed-in requests** — on the release build, with distinct trusted-hop clients,
  1 000 wrong logins took a signed-in `/v1/me` to **2.4 s** (291 ms at 100, 678 ms at 400). Argon2 still runs on the
  async workers.
- **No sign-out-everywhere, no password change or reset** — `/v1/sessions`, `/v1/seats`, `/v1/signout-all`,
  `/v1/password`, `/v1/me/password`, `/v1/password/reset` all 404.
- **No `Cache-Control: no-store`** on a token-bearing response.
- **`npm run dev:server` still exports `CRM_SERVER_ADDR`**, which the server does not read (`BIND`).
- **Parser text in error bodies** (`unknown field …, expected one of …`).
- **Anyone can lock a named account** for fifteen minutes (the correct password is refused too). Accept; it is the
  per-account limit's price.

None of these blocks a sign-off; the first two are the ones I would fix before a hosted deployment.

## 5. A note on p3, which arrived on the same branch

`main` now carries `/v1/sync/bootstrap`, `/changes` and `/write` (p3-backend). They are outside this round's scope and I
did not review them, but two cheap observations: unauthenticated `GET`s answer 401, as they should; and `POST
/v1/sync/write` parses its body **before** it authenticates (a bare `{}` with no or a bad token gets
`missing field writeId` and a 422, not a 401), which tells an anonymous caller the request schema and has the server read
up to the write endpoint's body cap from anyone. Worth moving authentication ahead of the body in the p3 review.

## Sign-off

**p2 is signed off.** The three MAJORs are closed, each by the property that would have caught it (a per-peer bucket, a
right-hand forwarded entry, a gate in the Rust command that runs before the request and understands URLs), and the new
limiter is exact under concurrency. The window now asks before a password can leave, and says where it would go.
The remaining items are MINORs with owners and none touches what p2 promised. **W1 is cleared** too: the 10-second
test passed in round 8, and its one MAJOR no longer reproduces by agent or by hand.

## What this round did, and did not do

**Did:** rule zero (app, server, window, `verify`); the three MAJORs and the concurrency MINOR rerun with the round-8
scripts (including the two-address default test, five proxy configurations and a split-header case); the 40-address
server check as a scratch test; the window's sign-in flow driven with the Tauri calls logged; the W1 draft sequence, the
chip, keep/discard and the by-hand click; the round-8 "held" checks on the new limiter; the owner's-data backup and
checksum.

**Did not:** fix, merge, tag or publish anything; run the p2 screens in the Tauri webview (the window ran in a browser;
the real core was behind the W1 test, the preview's fake core behind the sign-in screens); test two live app seats; review
p3's endpoints beyond the two observations above; test TLS itself.
