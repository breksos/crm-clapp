# Work order — M0 scaffold + M1 core

**Owner:** backend · **Blocks:** M2, M3, M4, M5 · **Branch:** `m0-scaffold`, then `m1-core`

Read [`CLAUDE.md`](../../CLAUDE.md) and [`docs/architecture.md`](../architecture.md) first.
This order fills in the parts that document leaves to implementation, and **freezes the two
things other agents build against**: the manifest surface and the snapshot shape. Both are
the PM's to change — if something here is wrong or impossible, say so and stop; do not
improvise a different shape.

---

## M0 — scaffold

**Definition of done:** `npm run verify` passes, and `crm status` round-trips against the
running window.

### Files to create

```
clatch.json                    identity + the agent-facing surface (given verbatim below)
package.json                   scripts: dev, build, verify, pack
src-tauri/Cargo.toml           clappkit = { path = "../clappkit", features = ["tauri"] }
src-tauri/tauri.conf.json
src-tauri/build.rs
src-tauri/src/main.rs          role dispatch — clappkit::role::main_dispatch
src-tauri/src/state.rs         AppState (M1 fills it; M0 needs only `status`)
src-tauri/src/cli.rs           verb dispatch + the -h manual (M0: `status` only)
src-tauri/icons/icon.ico       placeholder until M6 — the build FAILS without it
src/                           React + TS shell, bridge.ts re-exporting from @clappkit
scripts/package.sh
scripts/verify.sh
.gitattributes                 `* text=auto eol=lf` + binary rules for icons and fonts
```

### `clatch.json` — verbatim, this is the frozen surface

```jsonc
{
  "manifestVersion": 1,
  "type": "clapp",
  "id": "com.breksos.crm",
  "name": "Breksos CRM",
  "description": "A sales pipeline you and your agent share — whatever either of you opens, logs or moves, the other is looking at it too.",
  "version": "0.1.0",
  "protocol": 2,
  "publisher": "breksos",
  "icon": "assets/icon.png",
  "tags": ["sales", "productivity"],
  "launch": { "macos": "bin/crm", "args": ["app"] },
  "connector": {
    "cli": "crm",
    "commands": [
      { "name": "find",    "about": "search contacts, companies and deals — results land in the shared list" },
      { "name": "show",    "about": "one record in full, and open it in the window" },
      { "name": "board",   "about": "the pipeline: every stage, its deals and its totals" },
      { "name": "stages",  "about": "the stage vocabulary, in order" },
      { "name": "due",     "about": "next steps that are overdue, due today, or due this week" },
      { "name": "status",  "about": "what both surfaces are looking at, plus counts and connected agents" },
      { "name": "export",  "about": "write records as CSV or JSON; prints the file path" },
      { "name": "add",     "about": "create a company, contact or deal" },
      { "name": "set",     "about": "edit fields on a record" },
      { "name": "log",     "about": "record a call, email, meeting or note against a record" },
      { "name": "move",    "about": "change a deal's stage, or close it won or lost" },
      { "name": "task",    "about": "set a next step with a due date" },
      { "name": "done",    "about": "complete a next step" },
      { "name": "link",    "about": "associate a contact, company and deal" },
      { "name": "archive", "about": "retire a record, or restore one — never a delete" },
      { "name": "select",  "about": "answer a pending question, or open result N" },
      { "name": "import",  "about": "read records from a CSV or vCard file" },
      { "name": "focus",   "about": "focus the app window" },
      { "name": "close",   "about": "quit the app" }
    ],
    "signals": [
      { "id": "deal.opened",    "type": "buffered" },
      { "id": "stage.changed",  "type": "context"  },
      { "id": "record.changed", "type": "context"  },
      { "id": "note.added",     "type": "context"  },
      { "id": "task.due",       "type": "run"      }
    ]
  }
}
```

Only `launch.macos` is declared. **An OS key is the claim "runs on that OS", and every key
must have a depot in the release** — do not add `windows` or `linux` until M5 actually
builds and ships them.

### ⚠ The display name has a space in it

`scripts/lib.sh` in the clapp family built the macOS `.app` bundle directory out of the
manifest's **display name**, and every app in the family happened to have a one-word name.
Ours does not. "Breksos CRM" would produce `bin/Breksos CRM.app/…`, and
[`format.md`](../../clappkit/docs/format.md) limits every component of `cliBin` to
`[A-Za-z0-9._-]` — no whitespace. The depot would build, the self-check would pass, `clatch
pack` would succeed, and the refusal would arrive at **`clatch install`, in front of a
user.** This is [`playbook.md`](../../clappkit/docs/playbook.md) §12b, and we are the case
it warns about.

So, in `scripts/package.sh`:

- Build the bundle directory from **`connector.cli`** (`crm.app`), never the display name.
- The full name goes in `CFBundleName` / `CFBundleDisplayName` in `Info.plist`, which is
  what a person actually reads.
- Assert every component of the rewritten `cliBin` and `launch` is a safe segment before
  the script exits successfully.

Also from the playbook, and all cheaper to do now than to debug later: `verify.sh` must read
`connector.cliBin` **out of `pkg/clatch.json`** rather than assuming `bin/crm`; `zip -r`
needs a `7z a -tzip` fallback; and `icons/icon.ico` must exist or `tauri-build` fails even
with bundling off.

### M0 acceptance

- [ ] `cargo test` green, `cargo fetch --locked` resolves with no ssh key
- [ ] `npm run build` produces a window that opens (never bare `cargo build` — without
      Tauri's `custom-protocol` feature you get a white window)
- [ ] `crm status` returns a snapshot from the running app
- [ ] with the app **not** running, `crm status` **fails** with our own "not running"
      sentence — this is what proves the two-surface wiring exists at all
- [ ] `crm -h` lists exactly `status`, `focus`, `close` and says the rest is coming

---

## M1 — the core

`AppState` is **pure state and logic**: no files, no sockets, no platform code. Persistence
reaches it only through the `CrmStore` port. This is what makes the rules testable without a
window server, and it is why the tests are where the rules actually live.

### Records

Ids are **slugs derived from the name, uniquified** — `acme`, `acme-2`. Stable across a
rename, and typable, so `crm show acme` is an exact match and the ambiguity machinery only
runs when it genuinely has to.

```rust
struct Company { id, name, domain: Option<String>, tags: Vec<String>,
                 notes: Option<String>, archived_at: Option<Timestamp>, updated_at }

struct Contact { id, name, email: Option<String>, phone: Option<String>,
                 title: Option<String>, company_id: Option<Id>, tags: Vec<String>,
                 archived_at: Option<Timestamp>, updated_at }

struct Deal    { id, title, company_id: Option<Id>, contact_ids: Vec<Id>,
                 value: Option<Money>, stage: Stage, status: Status,
                 pipeline_id: Id, opened_at, closed_at: Option<Timestamp>,
                 archived_at: Option<Timestamp>, updated_at }

struct Activity { id, kind: ActivityKind, body: String, at: Timestamp,
                  links: Vec<Id>, by: Actor }          // append-only, never edited

struct Task    { id, what: String, due: Date, links: Vec<Id>,
                 done_at: Option<Timestamp>, by: Actor }

struct Pipeline { id, name: String, stages: Vec<Stage> }

struct Money   { amount: i64 /* minor units */, currency: String /* ISO 4217 */ }

enum Stage  { Lead, Qualified, Proposal, Negotiation }
enum Status { Open, Won, Lost }
enum ActivityKind { Call, Email, Meeting, Note }
enum Actor  { Human, Agent { id: String } }
```

**Stage and status are different axes, and this is the part most likely to be got wrong.**
`stage` is one of the four *open* stages. Won and Lost are **statuses**, not stages — so
`crm move acme won` sets `status = Won` and **leaves `stage` where it was**, which is what
makes conversion reporting possible later. The board draws six columns over four stages plus
two closing ones. `crm move acme proposal` on a closed deal reopens it (`status = Open`).

`Money.amount` is **minor units as an integer**. Never `f64`: summing an empty list of
`f64` yields `-0.0`, so an empty pipeline prints `$-0.00`. Nobody finds that bug twice.

`Actor` comes from the caller id `clappkit::app::spawn_ipc` hands the handler — present for
an agent, absent for the person. Key it on the agent **id**, never the display name: the id
is immutable, the name is a re-pointable label.

### The store port

```rust
pub trait CrmStore: Send + Sync {
    fn load(&self) -> anyhow::Result<Db>;
    fn save(&self, db: &Db) -> anyhow::Result<()>;
}
```

`Db` is the whole serialisable dataset. v1 ships `JsonStore`, writing through
`clappkit::store::atomic_write` into `clappkit::paths::data_dir("crm")`, **debounced** — a
card dragged across a board is one save, not sixty.

Be clear-eyed about what this seam buys: **file-format independence, not query pushdown.** A
future `SqliteStore` implements the same two methods and nothing above it changes, but it
would still be loading the whole set into memory. If we ever need real query pushdown that is
a larger change, and pretending otherwise now would be the expensive kind of optimism.

A test must assert `pipeline_id` survives a `save` → `load` round-trip. It is a field nothing
reads in v1, and an untested seam is usually a subtly wrong one.

### Shared view state

Part of `AppState`, persisted with the rest, and mutated by both surfaces:

```rust
struct View {
    focus:  Option<Focus>,                 // { kind: company|contact|deal, id }
    list:   ListView,                      // query, sort, page, page_size, last result ids
    board:  BoardView,                     // pipeline_id, stage_filter
    pending: Option<Pending>,              // an unresolved question + its candidates
}
```

**`page_size` lives here, never in a caller's request.** An agent passing `-n 3` limits what
its own terminal prints; it must not repaginate the person's table to three rows. Both
surfaces say "N of TOTAL" about the same page. Sort is state for the same reason: a control
that only reorders the page you happen to hold is a lie about the data underneath it.

**`pending` is how ambiguity is handled** — never a silent guess, never a bare error. Two
companies matching "Acme" parks a visible placeholder, puts the candidates in the same list
both surfaces already render, and either surface answers: `crm select 2`, or a click. Gate on
the **margin** between the top two scores; an exact id match is decisive.

### The snapshot — frozen, M3 builds against this

> **Extended additively in [`round-3-snapshot.md`](round-3-snapshot.md)** — `cards`,
> `focused`, `list.kind`, and a formatted string on every money value. The shape below was
> incomplete: it gave the board ids with no bodies, so the window could not draw a card.

Stamped once, in `AppState::snapshot()`, via `clappkit::snapshot::with_rev`. The response to
a command and the pushed `state` event must carry the same `rev` when they describe the same
moment — take both from one call so they leave the same critical section.

```jsonc
{
  "ok": true,
  "rev": 42,
  "pipeline": { "id": "sales", "name": "Sales",
                "stages": ["lead", "qualified", "proposal", "negotiation"] },
  "board": {
    "pipelineId": "sales",
    "stageFilter": null,
    "columns": [                                  // 4 stages, then won, then lost
      { "key": "lead", "label": "Lead", "dealIds": ["acme"], "count": 1,
        "totals": [ { "currency": "USD", "amount": 4500000 } ] }
    ]
  },
  "focus": { "kind": "deal", "id": "acme" },      // or null
  "list": { "query": "", "sort": "updated", "page": 0, "pageSize": 25,
            "total": 0, "rows": [] },
  "pending": null,                                 // or { prompt, candidates: [...] }
  "due": { "overdue": 2, "today": 1, "week": 5 },
  "counts": { "companies": 0, "contacts": 0, "deals": 0, "activities": 0, "tasks": 0 },
  "agents": [ { "id": "…", "name": "…", "backend": "…", "avatar": null } ]
}
```

`counts` are of **active** records; archived ones are excluded here, from the board, and from
default `find` results, but `show` still loads them and `archive --restore` brings them back.

Totals are **grouped by currency and never summed across them** — we hold no rate source and
inventing one would be worse than showing two numbers.

**Nothing secret ever enters this structure.** No credentials exist in v1, so this costs
nothing today; it is written down because a snapshot goes everywhere and absence has to be by
construction, not by redaction.

### Tests — these are the rules, not coverage theatre

- [ ] the stage vocabulary the snapshot carries is exactly what `crm -h` names, and exactly
      what `crm move` accepts — pin all three against one list
- [ ] `pipeline_id` round-trips through `CrmStore`
- [ ] a caller's `-n` does not change `view.list.page_size`
- [ ] `move` to `won` sets the status and **preserves** the stage; `move` to a stage on a
      closed deal reopens it
- [ ] an archived record leaves the board, the counts and default `find`, and is still
      loadable by `show`
- [ ] an activity logged with a caller id records `Actor::Agent`, without one records
      `Actor::Human`
- [ ] totals group by currency; an empty pipeline prints `0.00`, never `-0.00`
- [ ] ambiguity with a close margin parks a `pending`; an exact id match does not

### M1 acceptance

- [ ] `AppState` has no `std::fs`, no `std::net`, no `#[cfg(target_os)]`
- [ ] every test above passes
- [ ] `AppState` and the snapshot shape reviewed and **frozen by the PM** before M2 and M3
      start in parallel

---

## Do not

- **Touch anything under `clappkit/`.** It is a shared upstream submodule.
- **Change `clatch.json`'s `commands` or `signals`, or the snapshot shape**, without the PM.
  Those three are one artifact spanning backend, frontend and the agent's manual; a change to
  any of them is a change to all three.
- **Implement M2's verbs.** M0 ships `status` + the window verbs; the rest is the next order.
- **Add a dependency that reaches the network**, a scheduler, or a sidecar. This app is local
  and stays local.
- **Commit `pkg/` or `*.clapp`.** Both are derived and gitignored.
- **Emit a signal from an agent's own write.** Only human actions signal.
