# The split — the contract

**Status:** p1 step 3 done. The split is the only shape; the old whole-state snapshot, its flag and its fixtures are deleted (one commit, so a revert is cheap).
**Order:** [`work-orders/p1-the-split.md`](work-orders/p1-the-split.md) · **Input:**
[`m12-snapshot-gaps.md`](m12-snapshot-gaps.md) · **Code:** `src-tauri/src/state/split.rs`
**Fixtures:** `src-tauri/fixtures/split/*.json` (regenerate: `cd src-tauri && UPDATE_FIXTURES=1 cargo test golden`)

> This is what the frontend builds against in step 2. Where it differs from a suggestion in the
> gaps doc, the difference is listed at the end, with the reason.

---

## 1. One shape

The window is **pushed** an `update` after every change and **starts** with `{cmd:"bootstrap"}`
once. Every command *response* carries the same update under `split` (and, for the CLI, the
answer's own extras such as `answer`, `resumed`, `dueTasks`, `stages`). Nothing else describes the
state: there is no top-level `board`, `list`, `counts`, `focused`… and no `rev` on a response. The
CLI reads `split.view`, `split.seat` and `split.derived` and nothing else — a test renders a day of
every verb with the retired keys deleted from each response and requires identical output.

Golden fixtures: `split/bootstrap.json`, `split/bootstrap-pending.json`, `split/update-move.json`
(they stay in `fixtures/split/`; the subdirectory was there so the old shape's glob did not read them
and the window's own glob now names it).

## 2. Two counters, two jobs — never merged

| | **`rev`** | **`watermark`** |
|---|---|---|
| what it orders | what one window **draws** (view updates) | what the **data** did (row actions) |
| scope | this process; the clappkit counter | the dataset; persisted **in the same atomic write** as the rows it numbers |
| advances | on every message built, reads included | once per row action committed; **not** for navigation, a parked question or a refused write |
| the client | keeps the highest it has seen, drops anything older | keeps the highest it has applied, asks for everything after |
| today | unchanged | server-assigned in spirit: the core numbers it **inside the commit, before the write**, so a refused write is rolled back with its number |

A message carries both: `rev` at its top level, `watermark` beside it.

## 3. What is per seat and what is per org

| | Per seat (`view`, `seat`) | Per org (`data`, `delta`, `derived`) |
|---|---|---|
| holds | `focus`, `list` (+ `filters`), `board`, `cards`, `pending`, `focused`; `me`, `agents`, `reminders` | companies, contacts, deals, tasks (stored fields); the feed; counts, due, pipeline totals |
| ordered by | `rev` | `watermark` |
| leaves the seat | never | bootstrap + deltas |

`crm show acme` moves **this** seat's `focus`. A deal moved by anyone is a row action for **every**
client. The person and their agents share one seat, so they still share one view.

`view.list.rows`, `view.cards` and `view.focused` are *projections* — they are recomputed for the
seat on every message, because they depend on both the seat's view and the org's rows.
`data` holds **stored fields only**: a company's `dealCount` would be stale the moment one of its
deals moved without the company changing, so derived facts ride the view's rows (§6).

## 4. Messages

### `bootstrap` — once, `{cmd:"bootstrap"}` → `resp.bootstrap`

```
{ type:"bootstrap", shape:"split", rev, watermark,
  org:{id,name},
  view:{ focus, focused?, list:{query,sort,page,pageSize,total,kind,filters,rows[]}, board, cards, pending },
  seat:{ me:{id,name}, agents[], reminders },
  derived:{ counts, due:{overdue,today,week}, pipeline:{open[],won[],lost[]} },
  data:{ pipeline:{id,name,stages[]}, companies[], contacts[], deals[], tasks[] },
  feed:{ items[], total, page, pageSize } }
```

### `update` — pushed after every commit, and the `split` key of every response

```
{ type:"update", shape:"split", rev, watermark,
  view, seat, derived,
  delta:{ from, to, ops:[ {seq, op:"upsert", kind, id, record} ] } }
```

`from` is the watermark the client must already hold; `to` is the one it holds afterwards. An
update with no row action (a navigation, a read) has `ops: []` and `from == to`. **An update never
carries `data`: no code path pushes whole state.** If `delta.from` is not the watermark the
client holds, it asks for `changes` (below) or bootstraps again.

`ops[].kind` is `company`, `contact`, `deal`, `task` — the **row**, stored fields, replacing the
client's copy — or `activity`, `move` — a **feed item**, appended. Today every op is `upsert`;
there is no hard delete (archiving is a field). The word is on the wire so the day there is a
`delete` no client changes shape.

### `changes` — `{cmd:"changes", since:N}` → `resp.changes`

`{watermark, from, to, ops[]}` — every row action after `N`, in order — or
`{stale:true, watermark}` when the log no longer reaches back that far (it keeps the last 5,000
actions and does **not** survive a restart; the watermark does). A stale client bootstraps again.
A client *ahead* of the data is stale too.

## 5. Reads that serve one surface and touch nothing

All three change **nothing** — not the list, not the focus, not the pending question — pinned by
tests that compare the whole dataset, view included, before and after. They are not shared state.

| envelope | gap | request | `resp.…` |
|---|---|---|---|
| `search` | **4** (and 13) | `{q, kinds?:["company","contact","deal"], limit?}` (default 8, max 50) | `{query, total, counts:{company,contact,deal}, rows[]}` — relevance, then recently updated; archived not offered; **not** the list's sort or kind |
| `feed` | **2** | `{actor?, kind?:"call"\|"email"\|"meeting"\|"note"\|"stage", page?, pageSize?}` (default 25, max 100) | `{items[], total, page, pageSize}` newest first |
| `tasks` | **1** | `{limit?, includeDone?}` | `{open[], total, done?, doneTotal?}` |

`actor` is `{"kind":"human"}`, `{"kind":"agent","id":…}`, `"human"` or a bare agent id.
A feed item: `{id, kind, body, at, by, record:{kind,id,handle,label}, links[], orgId}`, plus
`from`/`to` (stage words, or `won`/`lost`) when `kind` is `stage`. A task: the stored fields +
`record`, `records[]`, and `bucket` (`overdue|today|week|later|done`), **soonest due first**.

## 6. The gaps, served

| gap | where |
|---|---|
| **1** tasks across records | `tasks` envelope; `data.tasks` in the bootstrap; `task` ops. **`sentAt`** on every task (QA 6c) so a surface can pick one to send again |
| **2** feed + move log | `feed` envelope; `moves` are persisted append-only (`Db.moves`: deal, from, to, at, by); `activity`/`move` ops are feed items |
| **3** pipeline totals per currency | `derived.pipeline.{open,won,lost}` — core-formatted, grouped by currency, **never summed across them**; archived deals excluded |
| **4** side-effect-free search | `search` envelope |
| **5** richer `find` filters | `find` accepts `status`, `stage`, `touchedBy`, `since`, `before` (below) |
| **6** `openedAt` / `closedAt` | on deal records and deal rows. (`closesOn` — an *expected* close date — is not stored and is not served: it is a new field, not a missing one) |
| **11** row facts | list rows gain `updatedAt` and `ownerId` on every kind; **company** `domain`, `tags`, `dealCount`, `openValue[]`, `contactCount`; **contact** `title`, `email`, `phone`, `companyHandle`; **deal** `openedAt`, `closedAt`, `movedAt`, `by`, `companyHandle` |
| **8** who the person is | `seat.me:{id,name}` |

**`find` filters.** Sticky and shared like `query` and `kind`: an omitted field keeps its value,
`null` clears it, and `kind: null` ("all") clears every one, because "all" means everything. Setting
any of them narrows the list to **deals**. `touchedBy` matches the deal's latest mover; `since`
and `before` are local dates on `movedAt` (so "stalled 60+ days" is `status: open, before: <date>`).
A bad value leaves the list exactly as it was. The classic snapshot carries `list.filters` **only
when one is set**, so a snapshot without one is exactly what it was — and the CLI names them on
every `find` result, empty or not, and in `crm status`. (The CLI grammar is frozen: there are no
flags for them. They are set by an envelope, i.e. by the window.)

## 7. Identity and tenancy — recorded, not enforced

* **`orgId` on every row** (company, contact, deal, activity, task, move), defaulting to `local`.
* **`ownerId` on every record**, set at creation to the person at the seat — also for what an
  agent creates, because the agent acts for the person; **`by`/`movedBy`** still say who acted.
  A file written before this gets the opening person stamped on every row, once, and is saved.
* **`me`** is the person at the seat. It lives in its **own file** (`seat.json` beside the data),
  never in the dataset — a synced copy must not be able to claim to be the person who made it.
  Named after the OS user; edit the file until there is a Settings control.
* **Nothing reads any of them to decide anything**; a test moves a deal owned by another person in
  another org.
* Agent identity stays workspace-scoped in intent: `CLATCH_AGENT_ID` is local to one install and
  means nothing on another machine. Not built here.

## 8. Storage — the backend's call

**Stay on the JSON file through p1; SQLite arrives with the sync client at p3.** Reasons:

1. The watermark must commit **atomically with the rows it numbers**, and a parked question's
   promise must commit with the writes it completes (QA round 6). One file replaced by one rename
   gives both for free; two stores give neither.
2. Nothing in p1 needs a query engine: every read above is a pass over in-memory vectors.
3. p3's local replica will want a schema shaped by the sync protocol (per-row version, watermark
   table). Choosing a schema now would be choosing it twice.

The ceiling is real and known: the commit point rewrites the whole file, so a commit is O(dataset).
That is a p3 problem, not a p1 one, and nothing here makes it worse.

## 9. What differs from the gaps doc, and why

* **Tasks are soonest-due first**, not "newest-due first": it is the order `crm due` already uses
  and the order a person works them.
* **`search`, `feed`, `tasks` are stateless envelopes**, not new shared state — the gaps doc's own
  design note: the Inbox's filters stay local.
* **Not served:** 7 (an *editable* owner — only recorded), 9 (agent status — needs the agent to
  report it), 10 (read state), 12 (creation as an event), 14–16 (platform work).
* **The log is bounded and not persisted.** The watermark is. A restart means one bootstrap.

## 10. What step 3 did

Deleted: `AppState::snapshot()`, `Outcome.snapshot`, `Sweep.snapshot`, the `CRM_SHAPE` flag and the
`Shape` enum, `list.filters`-only-when-set, `fixtures/snapshot*.json` and every test of them, the
window's whole-state branch in `useSplit`, the `useSnapshot` export, and the preview mock's classic
fields. The window's own `Snapshot` *type* remains: it is the window's model of the split state
(`toSnapshot`), not a wire shape, and some of its fields are still optional because the invented
preview scenarios predate them.
