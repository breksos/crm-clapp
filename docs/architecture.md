# Breksos CRM — architecture

A local-first sales pipeline CRM built as a **clapp**: one binary serving a window for the
person and a CLI for their agent, over one shared state.

> **This document is the spec of record for this app.** Where it disagrees with the clapp
> platform contract — [`clappkit/docs/elements.md`](../clappkit/docs/elements.md),
> [`format.md`](../clappkit/docs/format.md),
> [`protocol.md`](../clappkit/docs/protocol.md) — **the contract wins and this document is
> the bug.** Those three define what a clapp *is*; this one defines what *this* clapp does.
> There is a rendered copy of this document for humans; it is a view, not a second source.

| | |
|---|---|
| App id | `com.breksos.crm` |
| Display name | Breksos CRM |
| CLI | `crm` |
| Repository | `breksos/crm-clapp` |
| Type | `clapp` (a clapp:app) |
| Protocol | 2 |

## 1. What we are building

Companies, contacts and deals moving through one fixed pipeline. Every call, email and note
is logged against a record — and every log line records **who** wrote it, the person or a
named agent.

The point is not that an agent can script a CRM. It is that both parties are looking at the
same board. When the agent runs `crm move acme proposal`, the card moves in the window the
person is watching. When the person opens a deal, that fact rides along on their next
message, so "what's the history here?" needs no antecedent.

## 2. The model

Inherited from clappkit. Not ours to redesign — see
[`clappkit/docs/architecture.md`](../clappkit/docs/architecture.md).

```
   person ──clicks──▶  window (React)  ──┐
                                         ├─▶  AppState  ──▶  CrmStore ──▶ disk
   agent ──`crm …`──▶  CLI role       ──┘
```

The window learns of an agent's write as a pushed snapshot, ordered by a monotonic `rev`;
the agent learns of a person's edit as a signal, and reads the detail back through its own
CLI.

Three properties follow, and every decision below defers to them:

- **The two surfaces cannot drift**, because there is one implementation of every rule.
- **`AppState` is pure** — no files, no sockets, no platform code. That is what makes the
  rules testable without a window server, and it is why the tests are where the rules
  actually live.
- **The agent is never told about its own writes.** Only human actions signal, or the app
  spends its life talking to itself.

## 3. Decisions locked

With the reasoning, so a later reader can tell a choice from an accident.

| Decision | Chosen | Because |
|---|---|---|
| CRM shape | Sales pipeline | Richest fit for the split — the agent logs and advances, the person drags. Stages become a vocabulary both surfaces speak. |
| Data reach | Fully local | No API keys, no auth, no sidecar, no quota architecture. The agent *is* the import mechanism: it reads a mailbox in its own context and calls our CLI. |
| Persistence | JSON now, SQLite seam | Ships fast and keeps `AppState` pure; the swap point is one trait rather than a rewrite. |
| Window scope | Full workspace | The person genuinely works here. Following the `maps-clapp` precedent over a literal reading of "setup and status". |
| Pipelines | One, multi-ready shape | Many pipelines make stage names ambiguous for the agent, and give the board a mode an agent can yank out from under the person mid-drag. See §5. |
| Stages | Fixed in v1 | Editable stages would make `crm -h` lie the moment someone renames a column. |
| Currency | Per-deal, no conversion | Board totals group by currency. Converting needs a live rate source, and this app reaches nothing outside the machine. |
| Deletion | Archive, never hard delete | An agent holding a delete verb and a bad fuzzy match is an unrecoverable afternoon. |

## 4. Domain model

Six records. Everything else is a view over them.

| Record | Carries | Note |
|---|---|---|
| `Company` | name, domain, tags, notes | |
| `Contact` | name, email, phone, title, company, tags | |
| `Deal` | title, company, contacts, value + currency, stage, status, opened / closed | The spine. Carries `pipeline_id` from day one — §5. |
| `Activity` | kind, body, when, links, **by** | Immutable log. Never edited, only appended. |
| `Task` | what, due, links, done | The "next step". The only thing that can wake an agent. |
| `Pipeline` | id, name, ordered stages | A record, not a hardcoded enum. Exactly one instance in v1. |

### Attribution is the feature

Clatch injects `CLATCH_AGENT_ID` into the shell of whichever agent invokes our CLI, so
every `Activity` records exactly who wrote it — the person, or a specific agent by its
immutable id. The window draws that agent's avatar beside the line it logged; clappkit
already caches roster avatars, so this costs almost nothing.

This is what makes the app feel agent-native rather than agent-scripted. A shared log where
you cannot tell who said what is a log two parties stop trusting.

**Keying rule.** An agent's `id` is immutable; its `name` is a re-pointable label. Key every
stored attribution on the id and only ever *display* the name. A rename arrives as a fresh
roster snapshot with the same id — update the label in place, never drop and re-create.

## 5. Pipeline and stages

```
Lead ─▶ Qualified ─▶ Proposal ─▶ Negotiation ─▶ Won
                                             └─▶ Lost
```

Fixed in v1. Because there is one pipeline, stage names are globally unique — so
`crm move acme negotiation` needs no disambiguation, and `crm -h` can name every stage
truthfully. Both stop being true the moment a second pipeline exists.

### The seam, and where it stops

We buy the cheap structural half of multi-pipeline support and refuse the expensive
behavioural half.

| Build now | Deliberately skipped |
|---|---|
| `Pipeline` as a record, not an enum | Stage-name → pipeline disambiguation |
| `Deal.pipeline_id` | A pipeline switcher in the window |
| `board.pipeline_id` in the shared view state | Cross-pipeline reporting |
| Exactly one pipeline, seeded at first run | Per-pipeline anything else |

The data shape is multi-ready; the resolution logic never learns about it. Adding a second
pipeline later is a new record plus a switcher, not a refactor of how the agent names
things.

**Known cost:** `pipeline_id` is a field nothing reads in v1, and an untested seam is
usually a subtly wrong one. The core's tests must assert it round-trips through the store,
so it is structural rather than decorative.

**`crm stages` ships anyway.** Even with a fixed list, the read verb exists from day one so
the agent's habit is right before the data becomes mutable in v2. When stages do become
editable, `crm move` must fail by printing the *current* list rather than a compiled-in one.

## 6. Shared view state

The part that makes this a clapp and not a CRM with a CLI bolted on. Beyond the records,
both surfaces agree on what is currently *being looked at*. This state is as real as the
data, rides the same snapshot, and is mutated by both surfaces.

| Field | Holds | Why it is shared |
|---|---|---|
| `focus` | the record currently open | The agent's `crm show acme` opens that record in the person's window. Not a side effect — the point. |
| `list` | query, sort, page, page size | Page size belongs to the state, never the caller. An agent asking for three results must not silently repaginate the person's table to three rows. |
| `board` | active pipeline, stage filter | One board, one truth. A filter only one surface knows about is drift with better manners. |
| `pending` | unresolved ambiguity + candidates | See below. |

### Ambiguity is a state, not a guess and not an error

"Log a call with Acme" when two companies match: picking one silently is confidently wrong,
and refusing teaches nothing. Instead park a visible placeholder in `pending`, drop the
candidates into the same result list both surfaces already render, and let *either* surface
answer — `crm select 2`, or a click on the row.

A clear winner is not ambiguity. Gate on the **margin** between the top two matches, and let
an exact name match be decisive.

## 7. The CLI surface — which is also the permission model

Every verb declared in `connector.commands` becomes its own grant — `Bash(crm move:*)` — so
a person can hand an agent a subset. The split exists so that *read-only* is a grantable
posture rather than a promise.

| verb | grant | does | effect on the shared surface |
|---|---|---|---|
| `find` | read | search contacts, companies, deals | replaces the shared result list |
| `show` | read | one record in full | sets `focus` — opens it in the window |
| `board` | read | the pipeline: stages, deals, totals | — |
| `stages` | read | the stage vocabulary, in order | — |
| `due` | read | overdue, today, this week | — |
| `status` | read | what both surfaces are looking at, plus counts and agents | — |
| `export` | read | write CSV or JSON; prints the path | — |
| `add` | write | create a contact, company or deal | sets `focus` to the new record |
| `set` | write | edit fields on a record | — |
| `log` | write | record a call, email, meeting or note | appends to the timeline, attributed |
| `move` | write | change a deal's stage, or close it won / lost | the card moves on the person's board |
| `task` | write | set a next step with a due date | — |
| `done` | write | complete a task | — |
| `link` | write | associate contact, company and deal | — |
| `archive` | write | retire a record — reversible, never a hard delete | leaves the board and every default list |
| `select` | write | answer a pending ambiguity, or open result N | clears `pending`, sets `focus` |
| `import` | write | read a CSV or vCard export | — |
| `focus` | window | bring the window forward | handled by clappkit |
| `close` | window | quit the app | handled by clappkit |

**The argument grammar is frozen in [`work-orders/m2-cli.md`](work-orders/m2-cli.md)** — the
window prints these commands to the person as instructions, so each one is a promise. Every
verb takes **handles** (`acme`), never ids: a ULID is for storage and never appears in a
command line, in help text, or in output.

**The manual is the product.** `crm -h` is the agent's *only* documentation. A verb missing
from it does not exist as far as the agent is concerned, and a verb declared in the manifest
but unimplemented is a granted permission that fails at runtime. `clatch validate` checks
the manifest and nothing checks that the code agrees with it — closing that gap is a named
job, see §12.

## 8. Signals

A signal carries no durable state — it is a notice, and the agent reads the real thing back
through the CLI. Each is declared in `clatch.json`; the declaration is the authority, and
Clatch drops anything whose type disagrees with it.

| signal | type | fires when | effect |
|---|---|---|---|
| `deal.opened` | `buffered` | the person opens a record | rides their next prompt, so "what's the history here?" resolves |
| `stage.changed` | `context` | the person drags a card | queued in order, lossless |
| `record.changed` | `context` | the person adds or edits a record | queued in order |
| `note.added` | `context` | the person logs an activity by hand | queued in order |
| `task.due` | **`run`** | a next step comes due | **wakes an idle agent** |

### `task.due` earns the architecture, and carries the caveat

Clatch ships no scheduler and starts no app at boot. A due task therefore only fires *while
the app is running*, on a loop we own, with persistence and missed-schedule policy that are
ours to write. On launch, sweep for anything that came due while the app was closed and emit
**one consolidated** `run`, not twelve — and the window says plainly that reminders need the
app open, rather than letting someone discover it.

**Fan-out is all-or-nothing.** If any bound agent cannot accept a `run` or `context` signal,
the entire emission is refused and Clatch answers `app.toAgentRefused`. Surface that to the
person — a full-inbox agent otherwise reads as a dead button.

## 9. Persistence

`AppState` stays I/O-free and talks to a `CrmStore` port. v1 implements it over clappkit's
atomic JSON writes into `~/.clatch/appdata/com.breksos.crm` — the one directory an app may
write, so uninstall can erase the whole footprint.

- **Writes are debounced.** A card dragged across a board is one save, not sixty.
- **Every write is atomic** (`store::atomic_write`). A half-written CRM is a lost CRM.
- **The swap is one impl.** When the file starts hurting — call it ten thousand records —
  `SqliteStore` satisfies the same trait and nothing above it changes.

Activities are append-only, which is what makes the JSON approach survivable: the hot path
is appending to a list, not rewriting a graph.

## 10. Inherited constraints

Six rules from clappkit that shape this app whether we like them or not.

| Rule | What it costs us |
|---|---|
| No scheduler, no autostart | Reminders need the app open. Catch-up on launch is our policy to write and our honesty to surface. |
| Credentials only ever in the window | Dormant in v1 — no credentials exist. Wakes up the day we sync anything. |
| Only human actions signal | The agent is never notified of its own writes. |
| Any enum a surface shows lives in the core | If the board shows a "Negotiation" column, `crm move` takes that word and `crm -h` names it. A test pins it. |
| Pagination and sort are shared state | `-n` limits what the terminal prints; the page is shared, and both surfaces say "N of TOTAL" about the same page. |
| Nothing secret in a snapshot | Snapshots go everywhere; anything private must be absent by construction, not by redaction. |

## 11. What we are not building

Stated so nobody adds it back by accident.

- **No "ask the agent about this deal" button.** The window is for the person's own actions;
  the agent arrives through Clatch. A button that prompts on the person's behalf inverts the
  model.
- **No chat surface in the window.** The agent handles conversation. Our window is a board,
  a table and a record.
- **No external sync, no OAuth, no sidecar.** The agent is the import mechanism.
- **No background polling of anything.** The due-task loop is the single timer, and it
  reports on a threshold rather than a clock.
- **No second pipeline.** Structurally possible, behaviourally absent.

## 12. Milestones and team

| | Milestone | Owner |
|---|---|---|
| **M0** | Scaffold: `clatch.json`, `package.json`, `tauri.conf.json`, `src-tauri/`, `src/`, `bridge.ts`, package + verify scripts. clappkit is the SDK, not a template — none of this comes free. | Backend + Release |
| **M1** | The core: records, pipeline, `CrmStore` trait, shared view state, tests. Ends with `AppState` and the snapshot shape **frozen**. | Backend, reviewed by PM |
| **M2** | The CLI: every verb, and the help text that is the agent's only manual. Parallel with M3. | Backend |
| **M3** | The window: board, table, record detail, agent strip. Talks to the core through `bridge.ts` and nothing else. Parallel with M2. | Frontend |
| **M4** | Signals and the due loop: five signals, the launch sweep, the refusal surfaced. | Backend + Frontend |
| **M5** | Package and install: per-OS-arch depots, CI on a tag, then `clatch install` → `clatch run` → `crm status`. | Release |
| **M6** | Brand: mark, icon and banner to the format's hard limits, and the window wearing its own colours. | Frontend |

M2 and M3 are genuinely parallel — both build on M1's `AppState`, which is the point of the
design.

### The roster

| Specialist | Owns |
|---|---|
| PM / product owner | This document, the surface contract, and the M1 freeze. Writes no code. |
| Backend | `state.rs`, `cli.rs`, the store, the tests. |
| Frontend | `src/`, the window's design, the brand assets. |
| QA | Verification before anything is pushed — charter below. |
| Release | Packaging, the three platforms, CI, the depot that installs on a machine nobody tested on. |

**Two ownerships that need a name, not a headcount**, both the PM's:

1. **The surface contract.** `clatch.json`'s `commands` + `signals`, the `crm -h` text, and
   the snapshot shape are one artifact spanning backend and frontend.
2. **The integration freeze.** M2 and M3 run in parallel off M1. If they drift you get two
   surfaces that disagree — the exact failure this architecture exists to prevent.

### The QA charter, specifically

Generic QA runs the tests and calls it done. On an agent-native app the important pass is
different:

- **Drive the CLI as an agent would** — from `crm -h` alone, with no access to the source.
  If the help text is not a complete manual, the app is broken for its primary user while
  every test passes.
- **Prove the two surfaces agree** — every vocabulary the window shows is a word the CLI
  accepts, and the same page is described identically on both.
- **Verify against a real install**, not just a build: `clatch install`, `clatch run`,
  `crm status`.
- **The negative smoke test** — with the app not running, `crm status` must **fail** with our
  own "not running" sentence. That is what proves the two-surface wiring survived packaging.

## 13. Open questions

| Question | Blocks | State |
|---|---|---|
| **GitHub remote.** The repository is local-only, and `gh` is installed but signed in to nothing. | M5, and any workflow where agents push branches | Needs `gh auth login` — a browser sign-in only the product owner can complete — then `breksos/crm-clapp`. |

### Cleared

- **Display name** — Breksos CRM. Revisitable before M6; the manifest reads it from one
  field.
- **Toolchain** — Rust 1.98.1, Node 24.20.0 LTS, npm 11.19.0, gh 2.100.0 and
  `clatch 0.4.5-stage.14` all resolve, pinned onto `PATH` in `.claude/settings.json` so
  every agent inherits them. Verified by building the submodule: **74 tests, 0 failures**,
  and `cargo fetch --locked` resolving with no SSH key present — the check
  [`playbook.md`](../clappkit/docs/playbook.md) §8 asks CI to make.
- **Currency and deletion** — decided; see §3.

---

**Status** — v1, approved by the product owner. Sections 3 through 11 are decided. The
repository exists with clappkit as a submodule and the toolchain verified; M0 is unblocked
and unstarted.
