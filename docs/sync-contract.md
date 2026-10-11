# Sync — the contract (p3, backend)

**Status:** approved with changes (§0 defaults, the `status` line, and the five amendments marked **[A1]–[A5]** below). Step 1 of §10 is in progress.
**Order:** [`work-orders/p3-sync.md`](work-orders/p3-sync.md) §3 · **Builds on:**
[`split-contract.md`](split-contract.md) (the shapes), [`identity-contract.md`](identity-contract.md)
(sessions, seats, workspace agent ids) · **Code (to come):** `server/` (migrations, `/v1/sync/*`),
`src-tauri/src/sync.rs` (the client, SQLite replica), `src-tauri/src/main.rs` (`Core::commit`).

> Frontend and release build against this file. Where it is silent, the work order decides; where it
> disagrees with the work order, this is a bug in this file — say so in review.

---

## 0. Decisions I need from the PM

These are places the work order does not decide and the design has to. Each has a default; I build the
default unless told otherwise.

1. **Reminder marks stay on the seat.** A task's `dueSentAt` ("this seat's agent was told") is **not** record
   data and never travels. It lives in the replica, beside the rows. Otherwise Ana's agent being told
   would silence Berk's. *Which tasks wake my agent?* Those I own (`ownerId == me`) — a teammate's task due
   date reaches my screen but does not wake my agent (order §2, "Signals").
2. **Row version = the `seq` of the op that last wrote it.** No separate counter: one number orders the org
   and also serves as every row's version.
3. **A write is a batch, applied atomically.** `move` is a deal update plus a move event; `link` may touch
   two rows; `import` touches hundreds. One request, one transaction, all-or-nothing, so no verb can
   leave half of itself on the server.
4. **The server checks who a write claims to be from.** A human `by` must be the session's person; an agent
   `by` must be a workspace agent id minted for the session's person. `ownerId` on a create is the session's
   person, whatever the client sent. Without this, "attributed to Ana" is only a label a client chose.
5. **A handle is unique per org across every kind** (company, contact, deal *and* task — the one namespace
   `crm show`/`crm done` already rely on), enforced by a database constraint, not by a check.
6. **No pipeline editing in p3.** Each org gets the one seeded pipeline (stages fixed); it is delivered in the
   bootstrap and never written.
7. **Server-sent events, not WebSockets,** for the push channel: one long-lived `GET`, nothing new in the
   stack, and the work order's design (a hint carrying a number) needs only server → client.

---

## 1. Principles (restated, because everything below follows from them)

* The server is the truth for a signed-in org; the app keeps a **replica** and the **watermark** it has applied.
* The replica moves **only along the op log**, in order. Pushes are hints; the log is the truth.
* A write is **confirmed before it is acknowledged.** The CLI's exit 0 and the window's success mean the
  server has it.
* Conflicts are **per row, last-write-wins with a version check.** Never a field merge. Activities and moves
  are append-only and are never checked.
* `org_id` comes **from the session, never from a request.** No sync request or body has an org field; a body
  that carries one is refused (`422`, unknown field — the same `deny_unknown_fields` rule as p2).
* **View state never travels.** No request, response or event carries `focus`, `list`, `board`, `pending`,
  `feed`, `follow` or `page`. The wire types have no such field, and the p2 test is extended over every
  new endpoint.
* **Subscription-scoped from day one.** Every read path (bootstrap, changes, stream, the echo of a write) goes
  through one function, `subscription(&Session) -> Scope`. In p3 it returns `Scope::Org`. p4 narrows it
  without touching anything else. Nothing else in the server mentions "whole org".
* Local mode (no account) is **unchanged**: same file, same code path, no network.

---

## 2. Server tables

New migration `0002_sync.sql`. Every table has `org_id`; every query's `WHERE` starts with `org_id = $session_org`.

```
orgs            + watermark bigint NOT NULL DEFAULT 0          -- the org's op counter (see §4)
                + retained_from bigint NOT NULL DEFAULT 0      -- oldest seq still in ops (see retention)

handles         (org_id, handle) PRIMARY KEY, kind, id         -- the one namespace; constraint, not check
companies       org_id, id, handle, version, owner_id, archived_at, updated_at, fields jsonb
contacts        org_id, id, handle, version, owner_id, archived_at, updated_at, company_id, fields jsonb
deals           org_id, id, handle, version, owner_id, archived_at, updated_at, company_id,
                stage, status, moved_by jsonb, moved_at, opened_at, closed_at, pipeline_id, fields jsonb
tasks           org_id, id, handle, version, owner_id, updated_at, due date, done_at, by jsonb, fields jsonb
activities      org_id, id, owner_id, at, by jsonb, kind, body, links jsonb          -- append-only
moves           org_id, id, deal_id, at, by jsonb, from_stage, to_stage              -- append-only
ops             org_id, seq, kind, id, op, record jsonb, by jsonb, at, write_id      PRIMARY KEY (org_id, seq)
writes          org_id, write_id, seq_from, seq_to, answer jsonb, at                 PRIMARY KEY (org_id, write_id)
pipelines       org_id, id, name, stages jsonb                                       -- seeded; read-only in p3
```

* **Typed columns** are the ones something filters, joins or constrains on (`handle`, `owner_id`, `archived_at`,
  `company_id`, `stage`, `status`, `due`, `done_at`). **`fields`** holds the rest of the stored fields exactly as
  the split shape's `data` carries them (name/title, domain, tags, notes, email, phone, title, contactIds, value,
  what, links…). A row sent to a client is always rebuilt to the split `data` row shape, so the client's model
  and the server's cannot drift (the types are shared from `crates/identity`, as in p2).
* **`version`** is the `seq` of the last op that wrote the row (decision 2). A new row's version is the seq of its
  create.
* **No hard delete.** Archiving is a field (`archived_at`); there is no `delete` op in p3, but the word is on the
  wire (as in p1) so the day there is one no client changes shape.
* **`ops`** is the ordered log: one row per row-action. `op` is `upsert` (a company/contact/deal/task row, `record`
  is the full stored row, replacing the client's copy) or `append` (an activity or a move, a feed item). `by` is
  the actor, validated per decision 4. `write_id` ties the ops to the request that made them.
* **Retention.** `ops` is kept for **at least 14 days and at least 50 000 ops**, whichever is more; a nightly job
  deletes older rows and sets `orgs.retained_from` to the oldest remaining `seq`. `changes?since=N` with
  `N < retained_from - 1` answers `{stale:true}` (§3.2): the client bootstraps again. The row tables are the
  current truth, so retention never loses data, only the ability to replay history.
* **`writes`** is the idempotency record (§3.3): a retry of the same `writeId` returns the stored answer and
  changes nothing. Kept 24 h.
* `login_attempts` and the p2 tables are untouched.

Existing orgs (created in p2) get a seeded pipeline and `watermark = 0` in the migration.

---

## 3. Endpoints

All under `/v1/sync/`, all `Authorization: Bearer <session token>`, JSON, TLS-only mode and rate limits as
in p2. A missing/expired/revoked token is `401` exactly as today. Every id in a request is looked up with
`org_id = session org`; **an id that belongs to another org answers exactly like one that does not exist**
(`404`/not found in a refusal, the same body), so a guessed id leaks nothing.

### 3.1 `GET /v1/sync/bootstrap` — the subscription's rows

Scoped by `subscription(session)` — the client does not choose the scope. Paged by keyset so a large org is
not one giant response:

```
GET /v1/sync/bootstrap                         → first page
GET /v1/sync/bootstrap?kind=deals&after=<id>   → next page of that kind
→ 200 { watermark, pipeline:{id,name,stages[]}, members:[{id,name}], agents:[{id,name,ownerId}],
        page:{kind, rows[], next?:{kind,after}} }
```

* `watermark` is the org's watermark **when the first page was read**; the client keeps that number from the
  first page.
* Pages are *not* a single snapshot (a held snapshot across HTTP requests is not something PostgreSQL gives for
  free). Correctness comes from the replay rule: after the last page the client calls `changes?since=<first
  page's watermark>` and applies it, and **applying an `upsert` is idempotent and only moves a row forward**
  (it is ignored if the client already holds that row at an equal or higher `version`). A row read after a later
  write is simply already ahead; the replay then confirms it. Pinned by a test that writes continuously while a
  bootstrap is paged.
* Rows are the split `data` row shapes, plus `version`. Activities and moves come as feed items, paged like the rows (newest last
  within a page; the client sorts). **All of them** — the replica keeps every activity and move (§5.2).
* `members` — everyone in the org as `{id, name}` (no emails, no roles; p2's rule). `agents` — the org's
  workspace agents as `{id, name, ownerId}` (the person whose agent it is), so a teammate's agent is shown as
  *whose* agent it is. Both are what the app needs to attribute work; neither is view state.

### 3.2 `GET /v1/sync/changes?since=N&limit=L`

```
→ 200 { watermark, from, to, ops:[ {seq, op, kind, id, record, by, at} ] }      -- every op with seq > N, in order
→ 200 { stale:true, watermark }                                                 -- N older than the log reaches
```

`limit` defaults to 500, max 2 000; `to` is the last `seq` returned, so a client behind by more pages by
re-asking with `since=to`. `since` ahead of the org's watermark is `stale` too (the client's replica is not this
org's history: bootstrap again). The ops are filtered through `subscription(session)`. This is the p1 `changes`
envelope's shape, served from `ops` instead of memory — which is what removes the restart herd.

### 3.3 `POST /v1/sync/write` — the only way record data changes

```
{ "writeId": "<client ULID>",
  "ops": [ { "kind": "deal", "op": "update", "id": "…", "base": 41, "record": { …stored row… } },
           { "kind": "move", "op": "append", "id": "…", "record": { …feed item… } } ] }
```

* **`op`** is `create` (the id must not exist; no `base`), `update` (needs `base` = the `version` the client read),
  or `append` (activities and moves; never checked).
* **`writeId`** makes the request idempotent: the client mints it before sending; if the response is lost, the
  retry with the same `writeId` returns the same answer and applies nothing twice. This is what lets
  "confirmed before acknowledged" survive a dropped connection.
* The batch is **one transaction** that takes a row lock on the `orgs` row (so seqs are gapless and writers to
  one org are serialised), checks every op, then writes all rows, all `ops` rows and the new `watermark`
  together. Any refusal rolls back the whole batch.
* Limits: 10 000 ops and 8 MB per request (an `import` larger than that is split by the client into batches,
  each atomic; the CLI says so — §6, `import`).

**Answers**

| | status | body |
|---|---|---|
| **accepted** | `200` | `{ "result": "accepted", "seq": {from, to}, "watermark": to, "rows": [ {kind, id, version, handle} ] }` — `handle` is the **final** one (§3.4) |
| **stale** | `409` | `{ "result": "stale", "kind", "id", "current": {…row as the server holds it, with version…}, "by": {actor of the op that wrote it}, "at": <ms>, "watermark" }` — the **first** stale op; nothing was written |
| **refused** | `422` | `{ "result": "refused", "reason": "<the sentence the CLI/window shows>", "code": "…" }` — a rule, not a conflict: unknown id, a link to something that is not a company/contact, an `update` of a row that does not exist, a forged `by`, a bad field, over the limits… |
| not signed in | `401` | as p2 |

`409` carries the row **and who wrote it** because the order requires the refusal to say what the record says
now and who changed it. `by` is resolved to a name by the client from `members`/`agents`.

### 3.4 Handles are assigned by the server

The client proposes a handle in a `create`; the server returns the final one in `rows[].handle`:

* The proposal is inserted into `handles(org_id, handle)`. On conflict the server tries `acme-2`, `acme-3`, …
  (the same `unique_handle` rule the core uses, so a lone creator always gets exactly what they proposed).
* An `update` never changes a handle (renames do not rename handles — unchanged from today). A proposed handle
  on an `update` is ignored.
* Two people creating "Acme" at the same instant get two records, `acme` and `acme-2`, and each CLI prints its
  own record's final handle (§5, "adopting the answer").

### 3.5 `GET /v1/sync/stream` — the push channel (server-sent events)

One long-lived `text/event-stream` per seat, authenticated like everything else.

```
event: mark
data: {"watermark": 1287}
```

* A `mark` is sent **whenever the org's watermark advances** (coalesced: at most one per 100 ms) **and every 30
  seconds as a heartbeat** carrying the current watermark even when nothing happened. A client that sees a
  `mark` greater than what it has applied fetches `changes?since=` itself. The event carries a number and
  nothing else — no row, no name — so there is nothing in it to leak, and nothing to get out of order.
* The first event on connect is a `mark` with the current watermark (so a reconnect catches up at once).
* The server closes streams of revoked or expired sessions within one heartbeat (`event: bye`, `data:
  {"reason":"signed-out"}`).
* The stream is rate-limited per seat (a seat has at most **2** open streams; a third closes the oldest).
* `CRM_REQUIRE_TLS` and the proxy rules are unchanged; a proxy must not buffer `text/event-stream`
  (`X-Accel-Buffering: no` is set; documented for release).

### 3.5a Session tokens are never in a URL **[A5]**

The session token travels **only** in the `Authorization: Bearer` header — on every sync endpoint, **the stream
included**. The app's client is not a browser `EventSource` (which cannot set headers), so it has no reason to
put the token in a query string, and it never does. The server enforces it: a `?token=`, `?access_token=` or
similar query parameter is **ignored** (never read as credentials), so a request that carries the token only in
its URL is `401`; and the stream accepts no other credential. No URL the app builds (checked by a test over every
URL it constructs) contains the token, an invite or a password, and the server logs no request line (p2), so
nothing about a URL can leak one into a log or a proxy's access log. Pinned by
`a_token_in_the_query_string_authenticates_nothing_on_any_sync_endpoint` and `no_url_the_client_builds_holds_a_secret`.

### 3.6 Existing p2 endpoints that p3 touches

`GET /v1/org/members` stays (the window's roster). `POST /v1/agents` stays. **No new endpoint accepts an org id,
a person id other than through `by`/`ownerId` (validated), or any view state.**

---

## 4. The watermark and the versions

* One counter per org: `orgs.watermark`. A write that adds *n* ops advances it by *n* and gives them
  `seq = old+1 … old+n`, in the order of the batch. A row's `version` is the `seq` of the op that last wrote it.
* The counter is the p1 `watermark`, moved to the server. The client's `Db.watermark` is **the highest `seq`
  it has applied**; the client's `rev` keeps its p1 job (ordering what one window draws) and is never sent.
* Ordering for two writers is whatever order they take the `orgs` row lock. The loser of a race on the **same
  row** is the one whose `base` is no longer the row's version: it gets `409` and nothing is written.

---

## 5. The app

### 5.1 What stays pure

`AppState` is untouched in spirit: no I/O, no networking. It keeps doing what it does — validating a verb, mutating
`Db`, building the response. Everything network lives in `src-tauri/src/sync.rs` and in `Core`.

### 5.2 The replica: SQLite, behind `CrmStore` **[A2]**

* A signed-in org has its own database in the data dir, **`replica-<orgId>.sqlite`**, opened through the
  `rusqlite` crate with its **bundled** SQLite (no system library; one new dependency, which the release order
  pins in `Cargo.lock`). WAL mode, `synchronous=FULL`. **`crm.json` stays, unchanged, for local mode.**
* **Written per row.** Applying a batch (our own accepted write, or a teammate's ops) is **one SQLite
  transaction**: the changed rows, the new activities/moves, each row's `version`, and the new `watermark` commit
  together — the p1 invariant "the watermark reaches the disk in the same atomic write as the rows it numbers",
  without rewriting the whole dataset. A crash leaves either the batch or none of it.
* Tables mirror the server's, minus `org_id` (one org per file): `companies`, `contacts`, `deals`, `tasks`
  (stored fields + `version`), `activities`, `moves`, `members`, `agents`, `pipeline`, and `meta(key, value)` for
  `orgId`, `watermark`, `joinChoice`. A separate table `seat_marks(task_id, due_sent_at)` holds the seat-local
  reminder marks (decision 1); they are never sent.
* `CrmStore` grows one method beside `load`/`save`: `apply(&self, change: &RowChange) -> Result<()>`, where a
  `RowChange` is the row ops of one batch plus the new watermark. `JsonStore` implements it as "apply to the
  loaded `Db` and `save`" (local mode never calls it); `SqliteStore` implements it as the transaction above.
  `load` builds the in-memory `Db` the way it does today — **`AppState` still runs on an in-memory `Db` and stays
  pure**; only the persistence is per row.
* **The replica holds all of the org's records and all of its activities and moves** **[A3]** — no window, no
  trimming, in p3. (Sizing is stated in §9.)
* Opening the app is instant: load the replica, show it, mark `sync.state = syncing`, catch up
  (`changes?since=`) in the background. A replica whose `orgId` differs from the session's is deleted and
  bootstrapped fresh (a different account signed in). **`crm.json` is never opened for writing while signed in.**
  Signing out returns to it exactly as it was — "the local file is kept as a backup either way" costs nothing.
* `versions` (id → `seq`) is a column on each row table, loaded into memory beside the rows; it is **not** in
  `data` rows on the window's wire and not in snapshots. Local mode has none.

### 5.3 A write waits for the server — without making anyone else wait **[A1]**

**The rule: the `Core` lock is never held across a network call.** Reads and navigation never wait on a
write. The lock is taken twice, briefly, around the call, never during it.

A signed-in seat has one more lock, the **write gate** (an async mutex). It serialises *this seat's* writes with
each other (two commands from one seat do not race their own base versions); it is never taken by a read, by
navigation, by `apply_remote`, or by the sync task. A second write waits for the first; a `find`, `show`, `board`
or a window redraw does not.

1. **Lock → mutate a working copy → unlock.** Under the `Core` lock: take the write gate (waiting for it
   *before* the lock, never while holding it), clone the dataset, run `AppState::command` on the clone exactly as
   today, take the `Outcome`, and compute the **row ops** with `Db::row_ops(&before)` (each changed/new row → an
   `update`/`create` with `base = versions[id]`; each new activity or move → an `append`). Remember the seat's
   `view_epoch` (a counter bumped by every view change). **Release the lock.** `Db::change_from` already says
   *that* something changed; `row_ops` says *what*. A command with no row ops (navigation, a read, a parked
   question) never leaves the process and never takes the write gate.
2. **Send, with no lock held.** `POST /v1/sync/write` on `spawn_blocking`, fresh `writeId`, **connect timeout 3 s,
   total timeout 10 s**. A connect failure, timeout or dropped connection is retried **once** with the same
   `writeId` (so the server cannot apply it twice) before it is reported as offline. While this waits, the window
   keeps drawing, the CLI's reads keep answering, and teammates' ops keep arriving (§5.6).
3. **Lock again, settle.**
   * **accepted** → *adopt the answer*: for each returned row set `versions[id]` and the **final handle**
     (rewriting the handle in the row ops and in the `Outcome` the verb returns — the keys `add`/`task` answer
     with); apply the accepted rows to the live replica **on top of whatever teammates' ops have been applied in
     the meantime** (they cannot be for the same rows — the server would have refused with `409`); persist (§5.2);
     advance the watermark along the log (§5.7); rebuild the `split` update from the *current* state; publish;
     **only now** answer the caller. The view the verb produced (the focus after `add`, a refreshed list, a
     parked question) is applied only if `view_epoch` is unchanged — if the person navigated while the write was
     in flight, their navigation wins and the verb's view effect is dropped (the row change still lands).
   * **stale / refused** → apply the server's `current` row to the replica (so the window shows the truth);
     the working copy is simply dropped (the live dataset was never mutated); answer with the refusal (§5.5).
   * **offline / unreachable** → drop the working copy; answer with the offline refusal (§5.5).
4. Release the write gate.

The signal rule is unchanged: an `Outcome`'s emits are sent only after step 3 accepted, only for human actions,
and **only for the seat's own writes** — a teammate's op never produces an emit.

"Persist before acknowledging" becomes "the server accepted and the replica holds it before acknowledging".
Because the live dataset is only changed after acceptance, no rollback is needed on refusal; the pre-image
clone's old job is done by never touching the original.

### 5.4 Reads

Every read (find, show, board, due, status, export, `search`/`feed`/`tasks` envelopes) is served from the replica.
They work offline and while `syncing`.

### 5.5 Refusals at the CLI and in the snapshot

CLI exit codes follow the existing rule: **exit 1 = a valid request the system declined**; nothing here is exit 2.

| | CLI (stderr), exit 1 | response |
|---|---|---|
| **stale** | `not saved — acme-renewal changed while you were looking: its value is now $60,000 (set by Ana Torres, 2 min ago). Nothing was changed; look again and retry.` | `{ok:false, error, refused:{reason:"stale", kind, id, handle, by:{id,name}, at, current:{…row…}}}` |
| **offline** | `not saved — offline: your changes cannot reach the team server right now. Reads still work; nothing was queued. Try again when it is back.` | `{ok:false, error, refused:{reason:"offline"}}` |
| **refused (a rule)** | the server's `reason`, prefixed `not saved — ` | `{ok:false, error, refused:{reason:"rule", code}}` |

* The sentences name handles, names and relative times — never an id (the standing rule). `by` is resolved from
  `members`/`agents`; an unknown actor reads "a teammate".
* The window receives the same `refused` object, so it can show *what the record says now and who changed it*
  and keep the person's draft. After a stale refusal the replica already holds `current`, so the next `update`
  the window draws is the truth.
* The agent is told the same sentence; the feed (W1) records the call as `refused` carrying it.

### 5.6 A teammate's op becomes an `update`

The sync task (`stream` + `changes`) holds the same `Core` lock as commands. For each batch of ops, in order,
`Core::apply_remote(ops)`:

1. skips any op with `seq ≤ applied watermark` (our own ops arrive again through the log — this is how they are
   de-duplicated), and refuses to skip a gap: a `seq` that is not `applied + 1` means "fetch from `applied`";
2. applies `upsert`s (only if the op's `seq` is above the row's `version`) and `append`s to `Db`, sets
   `versions`, advances `watermark`;
3. persists the replica (atomic);
4. publishes **one `update`** with `delta:{from, to, ops}` — the p1 shape, unchanged — so the window needs no new
   path: a teammate's change is just a row action it did not make. `view`, `seat` and `derived` are recomputed
   as for any update (a changed focused record redraws the panel; the live list of §A2 picks up an added row);
5. emits **nothing**: no signal to the agent, no entry in the agent feed, no `follow` (the focus never moves
   because someone else worked), no `page` hint.

The delta's ops carry `by` as an actor; the window resolves it to a name with `seat.members` / `seat.agents`, so
"attributed to Ana" needs no extra request.

### 5.7 Catching up, and staying caught up

* **Normal:** the stream's `mark` → `changes?since=applied`. Every 30 s heartbeat repeats the watermark.
* **Stream down:** the client reconnects with backoff (1 s → 30 s) **and polls `changes?since=applied` every
  10 s** while it is down, so the 30 s acceptance bound holds with the channel killed.
* **Restart of the server:** the log is persisted, so a reconnecting client asks `changes?since=applied` and gets
  the gap, not `stale`. No bootstrap. (Pinned by the restart test.)
* **`stale` from `changes`:** bootstrap again, replacing the replica **only if the person has nothing unsent** —
  and in p3 nothing is ever unsent (no offline queue), so this is always safe.
* **Reconnect after offline:** the first successful `changes` call flips `sync.state` to `online`.

### 5.8 What the snapshot gains

`seat` gains two keys. Neither is record data, and neither goes anywhere but this window.

```
seat.sync     { state: "online" | "offline" | "syncing", lastSeen: <ms or null> }
seat.members  [ { id, name } ]                         -- the org's people, to attribute a teammate's work
seat.agents   [ …unchanged… ]                          -- still this seat's own roster
seat.teamAgents [ { id, name, ownerId } ]              -- the org's workspace agents, for "Ana's Scout"
```

* `state`: **online** — last request succeeded and the replica is at the stream's last `mark`; **syncing** —
  bootstrapping or applying `changes` (commands are allowed: the server decides); **offline** — the last request
  failed to connect (writes refused, reads work).
* `lastSeen` — when the app last heard from the server; `null` in local mode.
* In **local mode** `sync` is `{state:"local", lastSeen:null}` and `members` is just the local person: the
  window draws no sync indicator.
* `seat.me`, `org` and the p2 keys are unchanged. Golden fixtures are regenerated, and the diff is the review.
* `crm status` (CLI) gains one line while signed in: `sync: online (last heard 3 s ago)` / `offline`. It is a read.
  `crm -h` gains no verb; the intro line is unchanged. (Surface change to `status` only — flagged for review.)

---

## 6. Every write verb, mapped

"Ops" are what step 2 of §5.3 produces. `?` marks rows the verb touches only sometimes.

| verb (and its window envelope) | ops in one atomic batch | notes |
|---|---|---|
| `add company/contact/deal` (`add`) | `create` the row | the proposed handle comes back final; `--company` resolves against the replica to an id before sending |
| `set <handle> <field> <value>` (`set`) | `update` the row (`base` = its version) | a rename (`name`) is an `update` that keeps the handle |
| `set <task-handle> due|what` (`set`, by task id) | `update` the task | the fixes-r9 verb; same version rule as any row |
| `log` (`log`) | `append` an activity | no `base`. Never stale. If the linked records' `updated_at` is bumped (it is not today), those would be `update`s — it stays out so a log never conflicts with an edit |
| `move` (`move`) | `update` the deal (stage/status/movedBy/movedAt/closedAt) + `append` a move | one batch: a move with no move event, or the reverse, cannot exist. A deal moved by someone else since you read it → `409` (two people moving one deal is a real conflict) |
| `task` (`task`) | `create` the task | handle server-assigned, shared namespace |
| `done` (`done`) | `update` the task (`doneAt`) | a task already completed by a teammate → `409`, and the sentence says by whom |
| `link` (`link`) | `update` **exactly the rows the core changes** | today that is the deal alone (`company_id` or `contactIds`; `link` does not touch the contact or company row). The ops are *derived* by `row_ops` from what the core changed, not listed per verb, so if the core ever starts writing the other side the batch follows by itself — and the sweep test (below) fails until this row says so |
| `archive` / `--restore` (`archive`) | `update` the record (`archivedAt`) | |
| `import` (`import`) | `create` × N (+ `append`s) in as few batches as the limits allow | **one batch is applied or refused as one** (a bad row refuses the batch with the row named, nothing imported). If the file exceeds the batch limits, the core says up front: `this import is N rows; the server takes M at a time — it will be sent in K parts, each all-or-nothing` and each part is its own atomic batch; a refusal in part *j* stops there and says which parts were applied |
| `select N` resuming a parked write | whatever the resumed verb produces | the parked request is **seat state** (never sent); only the write it completes is |
| `resend` / reminder marks | **none — local** (decision 1) | the mark is in the replica file, not the log |
| `show`, `find`, `board`, `stages`, `due`, `status`, `export`, `follow`, `select N` (open), `search`, `feed`, `tasks`, `changes`, `state`, `page`/sort/filter | **none — view or read** | never touch the network (so they work offline) |

The sweep test that already pins "every envelope is a write or declared navigation" gains a third column: **the
ops it produces** — a verb that writes rows but produces none, or the reverse, fails.

---

## 7. Joining an org that already has data

*The choice is the person's, in the window, and never automatic.* The core never decides.

* On sign-in/sign-up/invite the app compares: local file present with records? org has records?
  * **Neither has records** → just join. No question.
  * **Org has records, local has none** → join; the replica is the org's. No question.
  * **Local has records** → the window asks (a Tauri command pair, window-only, like the rest of `account_*`):
    `sync_join_plan` → `{localCounts, orgCounts}` and `sync_join(choice)` with `choice = "bring" | "start-fresh"`.
    Until the person answers, the seat is **signed in but not joined**: `sync.state = "choosing"`, record
    commands are refused with "choose how to join first", and nothing leaves the machine.
* **p2's once-only claim re-stamps `crm.json` once [A4].** The first account to sign in on this install claims what
  the local person did: `crm.json`'s rows are re-stamped to that account **in `crm.json` itself, one time**, and the
  persisted `claimed` flag stops it ever repeating (sign-out and other accounts never re-stamp). So by the time the
  join question is asked, the local rows already belong to the first account. Consequently the plan **offers `bring`
  only for rows owned/attributed to the signing-in person (or still to the local person)**: a second account on the
  same install is told "this file's records belong to someone else" and is offered `start-fresh` only. Nothing
  re-stamps at upload time.
* **`bring`** — upload the local records through the **same write path** (`POST /v1/sync/write`) as `create`
  batches in dependency order (companies, contacts, deals, then tasks, activities, moves), `ownerId`/`by` as they
  already are after the claim, handle collisions
  resolved by the server (§3.4) with the **final handles reported** to the person afterwards ("3 handles changed:
  acme → acme-2 …"). All-or-nothing per batch; a refusal stops with a report and the org is left as it was up to
  the last complete batch. It is idempotent per `writeId`, so a retry cannot duplicate.
* **`start-fresh`** — the replica is the org's; nothing is uploaded.
* **Either way `crm.json` is left exactly as it is** — a backup, and the dataset the app returns to if the person
  signs out. Neither choice deletes, moves or edits it.
* Asked once per (local file, org) pair; the answer is remembered in the replica (`meta.joinChoice`) so a re-sign-in does not ask
  again.

---

## 8. Tenancy and the other boundary — what pins each

| property | how it is enforced | pinned by (tests to be written with the code) |
|---|---|---|
| org comes from the session | no request has an org field; bodies refuse unknown fields | `no_sync_endpoint_accepts_an_org_id` |
| a seat in org A cannot read or write a row of org B | every `WHERE` has `org_id = $session`; B's id answers like a missing one | `org_b_ids_are_not_found_in_org_a_on_every_endpoint` (every endpoint × every kind) |
| a seat cannot forge who did a thing | `by`/`ownerId` validated against the session and its workspace agents | `a_write_cannot_claim_to_be_another_person_or_agent` |
| no view state on the wire | wire types have no such fields | p2's `no_endpoint_accepts_view_state_from_a_seat`, extended over `bootstrap`, `changes`, `write`, `stream` |
| a stream/changes response contains only rows `subscription()` allows | single `subscription()` seam | `changes_and_stream_go_through_subscription` (p4 will tighten the same test) |
| a lost response cannot double-apply a write | `writes` idempotency | `a_retried_write_applies_once` |
| two creators of one handle get two handles | `handles` primary key | `simultaneous_creates_get_distinct_handles` |
| a stale write writes nothing | one transaction + version check | `a_stale_batch_changes_nothing_not_even_its_fresh_rows` |
| the log survives a server restart | persisted `ops` | `restart_does_not_force_a_bootstrap` |
| a bootstrap taken while writes land converges | idempotent, forward-only apply + replay | `a_paged_bootstrap_during_writes_converges` |
| nothing secret in a snapshot, event or log line | unchanged from p2; the stream carries a number | the p2 secrecy tests, extended |

**Convergence (acceptance 1)** is pinned by a harness test that runs two replicas and a server in-process: 50
mixed writes each, concurrently, every write verb, then asserts the three record sets are byte-identical.

---

## 9. What this does not do (so nobody assumes it)

* **No visibility rules** — everyone in the org sees every record (p4). The seam is `subscription()`.
* **No offline write queue** — refused, per the order.
* **No field-level merge, no CRDTs.**
* **No signal for a teammate's work.**
* **No shared reminders** (decision 1).
* **No pipeline editing, no roles, no teams.**
* **No server rental or deployment** — release writes the target up.
* **A window can be looking at a stale screen.** A teammate's change reaches this app within about 2 s when the
  stream is up and within 10 s of polling when it is down (30 s at worst, the heartbeat), and the window redraws
  after that. In that gap — and for as long as the app is offline — a person can act on a row that is no longer
  what they see. p3 does not prevent it and does not try to lock rows; it **detects** it: the write carries the
  version that was read, a stale one is refused with what the record says now and who changed it, and nothing is
  written. The guarantee is "never silently overwrites", not "never shows old data". Reads while offline are
  from the replica and say so (`sync.state`, `lastSeen`).
* **Replica size.** The replica holds every record and every activity of the org (no window, **[A3]**). At the
  order's scale (fifty seats, years of activity) that is the working set of a CRM — hundreds of thousands of rows
  at most, tens of megabytes — and bootstrap pages by keyset. If an org outgrows that, trimming old activities
  becomes a decision of its own, not an accident of p3.
* **Known limit, stated now:** a write is serialised per org on the `orgs` row lock. That is fine at the order's
  scale (fifty seats); it is the first thing to revisit if an org's write rate grows by orders of magnitude.

---

## 10. Order of work after approval

1. `0002_sync.sql` + tables + `subscription()` + `GET bootstrap|changes` + their tenancy tests.
2. `POST write`: version check, atomic batch, handles, idempotency, forgery checks + their tests.
3. `GET stream` (mark + heartbeat).
4. App: replica store, `row_ops`, the `Core::commit` four steps, refusals (CLI + snapshot), `apply_remote`, catch-up
   and polling fallback, `seat.sync`/`members`.
5. Join choice (`bring` / `start-fresh`).
6. The two-replica convergence harness; fixtures regenerated.

Each step is its own branch off `p3-backend` and its own review, as in p1.
