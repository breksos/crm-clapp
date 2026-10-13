# QA round 11 — p3 acceptance: two people, one board

**Reviewed:** 13 October 2026 · **By:** QA · **Build:** `origin/s6-review` @ `b832e9d` (see the first note)
**Verdict:** **every §5 acceptance item passes**, live, on two real seats against `npm run dev:server`. **One MAJOR** in the window (the join report is never shown) and eight MINORs. Fix the MAJOR before the product owner rents the server.
**Previous:** [`round-9.md`](round-9.md)

Findings only. Nothing was fixed, merged, tagged or published.

> **The build is not `main`.** `origin/main` (5478dcb) has p3-backend steps 1–5 but **not** step 6 (`scripts/converge.sh`)
> and **not** `p3-frontend`; both are on their own branches. `origin/s6-review` (b832e9d) is `main` + `p3-backend` +
> `p3-frontend` merged, so that is what I reviewed. Anything you call "main after step 6 and p3-frontend" is this commit.

**The product owner's demo data** was copied aside first (`/tmp/q11backup/`) and nothing was installed into the real
Clatch; every seat ran under its own `HOME` (`/tmp/q11a`, `b`, `c`) against a private PostgreSQL (port 55432, the Quayt
runtime's binaries with their own data directory — its port and data were never used). Final SHA-1s: live `crm.json`
`728bde1a…`, `seat.json` `0835055b…`, identical to the copies and to rounds 8 and 9.

| §5 item | Verdict | The one sentence |
|---|---|---|
| Convergence | **pass** | `converge.sh` green, and my own 2 × 50 live writes left both replicas and the server identical. |
| Live | **pass** | 0.12 s to the other seat's core with the stream on; 7–10 s with it cut; **34.7 s** with it silently dead (MINOR). |
| Conflict | **pass** | One wins; the other is refused with the current value and who set it; the window keeps the draft. |
| Handles | **pass** | Six simultaneous creates of one name: twelve distinct handles, each CLI printed its own. |
| Offline | **pass** | Reads work, every write verb exits 1 with a clear message, nothing is queued; reconnect is one `changes`, no bootstrap. |
| Restart | **pass** | Ten seats connected, server restarted: no bootstrap, no `stale`. |
| Tenancy | **pass** | Every endpoint from another org; a foreign id answers exactly like a missing one. |
| The boundary | **pass** | No view state in anything the apps send, nor accepted by the server. |
| Local mode, join choice | **pass** | Never merged without a click; claimed-machine refusal holds; the window never says what was renamed (**MAJOR**). |
| Tests | **pass** | Counts below. |

---

## Rule zero — `b832e9d`

| Command | Result |
|---|---|
| `cargo test` (app) | **493 passed, 0 failed** (from 452) |
| server tests, real PostgreSQL | **55 passed, 0 failed** (from 43) |
| `cargo clippy --all-targets` — app / server | clean on our code (the same upstream `block v0.1.6` warning) / **clean** |
| `cargo fetch --locked` — app / server | exit 0 / exit 0 |
| `npx tsc --noEmit` / `npm test` | clean / **202 passed, 0 failed** (from 168) |
| `npm run verify` | **green** — "the two surfaces agree" |
| `scripts/converge.sh` | **green**: two live runs of the workload, both tests pass |

`converge.sh` prints, per run, what each seat's 50 writes came to — e.g. `a {ok 46, stale 3, rule 0, other 1}  b {ok 47,
stale 2, …}  server watermark 102` — across eight workloads. It drives the app's real core, real HTTP and the real
server; people **and** their agents, every write verb.

## How the live run was built, and what that is worth

Three real app instances (`crm app`, standalone), each with its own data directory and its own CLI socket; one
`crm-server` on PostgreSQL; the real React window for each seat in its own browser tab. The window cannot call the app's
Tauri commands from a browser, so I built a **scratch bridge** (about 35 lines in `main.rs`, applied to a scratch
worktree and reverted — never committed) that lets the window's `account_*` and `sync_*` calls reach the same functions.
Two limits, stated plainly:

- **The window is not the Tauri webview**, and the core's pushes to it are **emulated** by polling every 300 ms and sending
  the same `update` (with the real `changes` ops as its delta). Window latencies below include up to 0.3 s of that.
- **No agents in the live runs**: standalone has no Clatch roster, so every CLI call is the person's. Agents are covered
  only by `converge.sh`.

The server was also reached through a small transparent proxy (cut the stream, cut the network, make the stream silent,
freeze `changes`) so network faults could be made on demand.

---

## 1. Convergence

`converge.sh`: see above. My own live run: both seats made **50 mixed writes at the same time**, twice (100 each in all),
through every write verb (`add` company/contact/deal, `set`, `log`, `move`, `task`, `done`, `link`, `archive`/restore,
`import`). Afterwards, with both seats `online` and at the server's watermark:

```
watermark   A 214   B 214   server 214
A's record data == B's record data (every stored field of every company, contact, deal and task, sorted by id):  identical
server tables:  companies 39  contacts 20  deals 21  tasks 16  activities 20  moves 8     A feed 28 = B feed 28
handle sets: server == A == B (companies and deals)     duplicate handles in the org: 0
```

My workload's first random generator was broken (every move was a move to `lead`, so no stage ever changed); I fixed it and
reran, which is the result above.

## 2. Live

Ana (seat A) moves a deal; Berk's (seat B) core:

```
stream on          CLI returned in 36–96 ms; B's replica showed it 121 ms later
                   B's window: the card in the new column 0.4 s after the CLI call, ringed for ~1.2 s, disc "Ana Torres"
stream cut (503 and every open stream closed)    6.9 s, 9.9 s   — the 10 s poll the contract promises
stream silently dead (open, no bytes, no FIN)    34.7 s
```

### MINOR — a stream that goes silent without closing takes 35 s, not 30

The cut-stream case meets "within 30 s". A stream that stays open but stops delivering — a middlebox that drops
packets — is detected by the missing heartbeat, and took **34.7 s** to catch up (one run). The order says "with the push
channel killed, within 30 s (the heartbeat)". Either a shorter watchdog (about 2 × the poll) or a sentence in the
contract that the bound is 30 s *plus one grace period* would close it.

## 3. Conflict

Both seats edit one deal's value from the same version (Berk's push held back, so he is genuinely stale):

```
A:  crm set acme-deal-b2 value 11111     → updated
B:  crm set acme-deal-b2 value 22222     → crm: not saved — acme-deal-b2 changed while you were looking: its value is now
                                            $11,111.00 and it is lead (set by Ana Torres, just now). Nothing was changed;
                                            look again and retry.                                         (exit 1)
B again                                  → updated    (the refusal had already re-read the row)
```

Nothing silently lost: the first write stood, the second was refused with the current value and the author's name, and the
retry succeeded. **In the window**, with the same setup (Berk's `changes` frozen, Ana sets 7 777, Berk types 9 999 into the
value field and presses Enter): an alert with the same sentence; **the draft `9999` is still in the box**; the field is
still open. A stale **append** (Berk logs a call on a deal Ana has just archived) is accepted, as the contract says.

### MINOR — the stale panel prints "Value undefined" and calls Ana "a teammate"

Under that sentence, the window's "what the record says now" block read: *"now Value undefined · Stage lead — Changed by a
teammate just now. Nothing was saved. What you typed is still in the box…"*. The core's sentence is right (`$7,777.00`, "set
by Ana Torres"); the panel built from `refused.current`/`refused.by` formats the value wrongly and does not resolve the name.
The order says the refusal shows "what the record says now and who changed it".

## 4. Handles

Six rounds, each with both seats creating "Collide *n*" at the same moment:

```
round 1:  A printed collide-1-2   | B printed collide-1        round 4:  A collide-4   | B collide-4-2
round 2:  A printed collide-2-2   | B printed collide-2        round 5:  A collide-5-2 | B collide-5
round 3:  A printed collide-3-2   | B printed collide-3        round 6:  A collide-6-2 | B collide-6
```

On the server: `collide-1 … collide-6-2`, twelve records, each created by the person whose CLI printed that handle; zero
duplicates among every handle in the org; each seat can open the handle the *other* printed.

## 5. Offline

Network pulled from Berk's seat. `crm status`: `sync: offline — reads work, writes are refused until it is back`.

- **Reads:** `find`, `board`, `due`, `show` all work, from the replica.
- **Writes:** `add`, `move`, `set`, `log`, `archive`, `task`, `link`, `import` → **exit 1**, *"not saved — offline: your changes
  cannot reach the team server right now. Reads still work; nothing was queued. Try again when it is back."* (`done` on an
  already-finished step gives its own correct refusal.) The server holds nothing from the attempts.
- **The window:** indicator **Offline — last heard 38 s ago. Reads still work; changes are refused, not queued**; "Add next
  step" and "Log" are disabled; a drag-move is attempted and refused with the same sentence.
- **Reconnect:** while Berk was offline Ana added a company, moved a deal and logged a note. When the network returned, Berk
  caught up in 6.6 s (the retry backoff) by calling **one `changes` and one `stream` — no bootstrap**; the indicator went back
  to Online.

## 6. Restart

Ten seats: the two real apps and eight protocol clients (sign-in, bootstrap, stream, `changes`), all behind the proxy so every
request could be counted. The server process was killed and restarted with the same database:

```
all ten seats, requests through the proxy since the restart:   52 changes, 42 stream reconnects, 3 writes    bootstraps: 0
eight protocol clients:                      16 caught up                                      stale: 0   re-bootstraps: 0
a write made while the server was down        → refused (offline); after the restart both seats show "After Restart"
```

(The numbers are the ten seats' retries during the four seconds of outage and the seconds after.) The log is persisted;
nobody was told to bootstrap again.

## 7. Tenancy

A fresh account (Mallory, a different org) with a valid session and the real ids, versions and person/agent ids of org A:

- **Writes** — an `update` of org A's company, deal, task and contact (with the **right** base version): all refused
  `422 unknown-record`, **byte-identical** to the same request against an id that exists nowhere. An `append` linked to
  org A's deal: `unknown-link`, identical to a missing one. Org A's rows hold none of Mallory's strings; nothing was planted.
- **Forged actors** — `by` as org A's person, org A's agent, a made-up agent, and an activity whose own `by` is org A's person:
  all `forged-actor`.
- **Fields that are not the server's** — `orgId` at the top level: refused, unknown field; `orgId` and `ownerId` (org A's
  person) inside a record: accepted *and dropped* (the stored `ownerId` was Mallory's own); a `focus` field: `unknown-field`.
- **Reads** — bootstrap: 4 rows, all Mallory's, members `["Mallory"]`; `bootstrap?kind=deals&after=<org A's deal id>`: nothing;
  `changes?since=0`: 4 ops, all her own; `?org=`, `?orgId=`, `?org_id=` → 400; an `X-Org-Id` header ignored; the stream's first
  event carried Mallory's watermark (4) while org A's was 243; p2's `/v1/org/members` and `/v1/agents`: her own only.
- One thing that looks like a leak and is not: reusing org A's company id in a **create** from Mallory's seat succeeds — in
  Mallory's org. Ids are per org, A's row is untouched, and a second reuse differed from a missing id only in the handle her
  own first row had taken (`squat` vs `squat-2`).

## 8. The boundary holds

I captured every request body the two apps sent while their seats navigated: `show`, `find` with a sort, a kind filter,
`board`, an ambiguous `show` and its `select`, plus writes. Keys on the wire: `amount, archivedAt, at, base, body, by,
closedAt, companyId, contactIds, currency, dealId, domain, doneAt, due, from, handle, id, kind, links, movedAt, movedBy,
name, notes, op, openedAt, ops, orgId, origin, ownerId, pipelineId, record, stage, status, tags, title, to, updatedAt, value,
what, writeId`. **None** of `focus, list, board, cards, pending, feed, follow, page, query, sort, filters, candidates, resume,
seat, agents, reminders, view`. (`orgId` is sent inside records and dropped by the server.) The server refuses the keys on
the way in (§7).

## 9. Local mode, and the join choice

**Never automatic.** Three seats, three paths, all through the window's real screens:

- **Ana, `bring` (the org is empty).** Signing up put her in "choosing": `crm add` → *"not saved — choose how to join the team
  first … Nothing was changed"* (exit 1), reads worked, and the server held **0 rows** until she clicked. After *Bring*: her
  company and deal were on the server.
- **Berk, `bring` with a collision.** His local "Acme Local" met Ana's `acme-local`; it landed as `acme-local-2`; his
  `crm.json` was byte-identical before and after.
- **Cem, `start-fresh`** with local records and an org of 60 companies: nothing uploaded (the server's counts did not move,
  his "C Local One" is not on it); his replica is the org's; **signing out** revoked the session server-side (401), returned
  him to local mode with his own two records, and a new local write stayed local.
- **A claimed machine.** Signing in as another account (Mallory) on Cem's machine: the window showed *"The records on this
  machine belong to another account, so they cannot be brought into this organisation"* and offered **only** *Start fresh*; a
  forced `sync_join("bring")` answered `ok:false — this file's records belong to another account`; writes stayed refused.
- **The indicator** is absent in local mode, reads *Choose how to join* while choosing, *Online*, *Syncing…* (while changes
  were frozen) and *Offline — last heard N s ago*.

### MAJOR — the renamed handles are never reported to the person

`src/Sync.tsx:93-107` and `src/App.tsx:214`. The contract (§7) and the window's own text say so: *"If a handle is already
taken the server picks another, **and you are told which**"* / *"N handles changed: acme → acme-2"*. In Berk's `bring`,
`acme-local` really became `acme-local-2` — and the window said nothing. `JoinChoice` sets its report after
`sync_join` resolves, but it is mounted only while `sync.state === "choosing"` (`App.tsx:214`), and the same call flips that
state, so the component is unmounted and the report with it. The agent that had been told `acme-local` is now wrong about
which record that handle names, and nobody was told. The preview's fake core renders the report because it flips state
differently, which is why the window tests pass.

### MINOR — "left exactly as it is" is not quite true

The join screen says, for either choice, *"your file on this machine is left exactly as it is"* and the contract says
`crm.json` is never edited. It is edited once, **at sign-in**: the p2 claim re-stamps every row to the first account (Cem's
`crm.json` changed hash after joining; the records and counts were intact, and `ownerId` became his). The design is
documented, but the screen's sentence is the one a person reads.

### MINOR — a second account in the same org on the same machine is not asked

On Ana's machine, signing out and signing in as Berk (same org) joined at once, with no question: the org's replica
already existed on that machine. That is the stated rule ("a re-sign-in … skips the question"), but the replica also stores
the seat's **view and reminders**; I did not check whether Berk inherited Ana's.

### MINOR — an invite's expiry reads "21.01.1970"

Settings → Invite someone shows *"It works once, and expires 21.01.1970 19:54:41."* The server's `expiresAt` is in seconds
and the window formats it as milliseconds.

### MINOR — teammates are indistinguishable by colour

A teammate's change is ringed (2 px, grey) and attributed by disc ("Ana Torres"), but the discs for Ana and Berk are the same
colour (`rgb(39, 43, 42)`) and the ring is the same grey. The order asked for the highlight "in the teammate's colour"; people
are neutral by M11 and only agents have tints, so this may be by design — but then two teammates differ only by initials.
Berk's **own** move through the CLI was also ringed.

---

## What would change my verdict

1. **Report the handle changes** — keep the report in state that outlives `JoinChoice` (or show it from `App` after
   `sync_join` resolves) and add a window test that flips `sync.state` the way the real core does.
2. Then, in order: the stale panel's value and name; the invite date; the 35 s watchdog; the join-screen sentence; a colour for
   a teammate (or a line in M11 that there is none).

## What this round did, and did not do

**Did:** rule zero on `b832e9d` (app, server, window, `verify`, `converge.sh`); two live seats and a third against a real
server with the real windows; every §5 item, with network faults made on demand; both join choices, the claimed-machine
refusal, sign-out and local mode; a tenancy attack on every sync endpoint plus the p2 endpoints; a capture of everything
the apps put on the wire; the owner's-data backup and checksum.

**Did not:** run the window in the Tauri webview (browser, with a scratch bridge and emulated pushes); run agents in the
live seats (converge.sh covers them); run ten **real** app instances (two real apps plus eight protocol clients); review
p4; test TLS. **One incident to report:** partway through, the disk filled (my four scratch worktrees' build directories on
top of an already 95 %-full volume) and the shell stopped working; I deleted only my own scratch `target/` directories and
continued with smaller builds. Nothing of yours was touched.
