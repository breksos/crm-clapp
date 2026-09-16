# QA round 2 — `m1-core`, `m3-window`

**Reviewed:** 16 September 2026 · **By:** QA · **Verdict:** 1 pass, 1 send back
**Against:** [`round-2-fixes.md`](../work-orders/round-2-fixes.md) and
[`m2-cli.md`](../work-orders/m2-cli.md) § The argument grammar · **Previous:** [`round-1.md`](round-1.md)

Findings only. Nothing was fixed, nothing was merged, nothing under `clappkit/` was
touched. Both branches were reviewed together, in their own clean worktrees, at the heads
below — both rebased on `main` at `4c62b72`. There is no remote, so the local branches are
the current state.

| Branch | At | Verdict | The one sentence |
|---|---|---|---|
| `m1-core` | `ae72ddf` | **pass** | All five round-2 items are fixed and proven live against a ULID-keyed store: `crm status` prints the handle, every exit code matches the manual, and no output carries an id. |
| `m3-window` | `37e9af5` | **send back** | Every printed command is fixed — but fed the real core's snapshot, the window renders a ULID as every board card's title, and its `crm find` hint breaks on any search with a space in it. |

## The defect moved; it did not go away

Round 1's defect was an id reaching text a person reads. Round 2 fixed it on **both** surfaces
for everything each side could test on its own — and it survives exactly where round 1 said it
would: where the surfaces meet.

**Fed a real `m1-core` snapshot, the `m3-window` window puts three ULIDs on the board** — its
own leak alarm fires — and says **"Nothing open"** while the core has a record open and
`crm status` names it. Neither branch can see this alone. The preview always supplies the
`cards` and `focused` fields the window proposed in `docs/window.md` §1. The core, which was
told not to change the snapshot, never sends them. And no gate builds the two halves
together: each branch's `npm run verify` pairs its own half with `main`'s.

Round 1 missed the second half of this. It reviewed the window only through the preview,
which is the same blind spot.

---

## Rule zero — the commands, and their real output

| Command | `m1-core` @ `ae72ddf` | `m3-window` @ `37e9af5` |
|---|---|---|
| `cargo test` | **112 passed, 0 failed** — one target, compiles | **20 passed, 0 failed** — `main`'s M0 core; no Rust changed |
| `npm test` | — (no frontend change) | **6 passed, 0 failed** — `node --test`, Node 24.20.0 |
| `npx tsc --noEmit` | — | clean |
| `cargo clippy --all-targets` | clean on our code | clean on our code |
| `cargo fetch --locked` (ssh forced off) | exit 0 | exit 0 |
| `npm run verify` | all 7 steps green | all 7 steps green |

The 112 matches the backend commit's claim. The one new dependency, `chrono 0.4.45`, is
`default-features = false, features = ["clock", "std"]`: timezone lookup only, nothing that
reaches the network. The only upstream warning on either branch is `block v0.1.6`
future-incompat.

`npm run verify` does **not** run `npm test` — see the process finding below.

---

## 1. Every round-1 finding, one by one

| # | Round 1 | Sev | Status | Evidence |
|---|---|---|---|---|
| m1·1 | `crm status` prints a ULID | Major | **fixed** | Live, against a store seeded through the core with a ULID-keyed deal: `looking at: deal acme-renewal`. `cli.rs:171-179` reads `focus.handle`, with no fallback to `id`. Agent ids were dropped from the output too. |
| m1·2 | the guarding test pinned `"id": "acme"` | Major | **fixed** | Fixtures mint real ULIDs (`cli.rs` `an_id`). `no_id_ever_reaches_stdout` scans for any ULID-shaped word, whichever field leaks. Sweep: `state_tests.rs:382` corrected, and hand-spelled pseudo-ULIDs in `model.rs` and `store.rs` (containing `L`, which no minter can produce) replaced. |
| m1·3 | arguments silently discarded | Minor | **fixed** | Live: `crm status --json`, `crm close --please` and `crm focus now` each exit **2**, naming the verb, the rejected argument and `crm -h`. Decided by a pure `plan()`, with tests. |
| m1·4 | unbuilt verb exits 2; manual says 1 | Minor | **fixed** | Live: `crm add company Acme` → **1**; `crm teleport` → **2**; app down → **1**. |
| m1·5 | `local_today` is UTC | Noted → decision | **fixed**, with two notes | Local via `chrono`. Under `TZ=Pacific/Kiritimati`, where the local date is a day ahead of UTC, the clock tests still pass, so it follows the machine's zone. See the two minor findings below. |
| m1·6 | `rev` bumps on a read | Noted | unchanged | Not in the round-2 order. Still true; still not a defect. |
| m6·1 | `brand.md` publishes a palette the chrome does not use | Minor | **fixed** (on `m3-window`) | New § "The chrome uses darker variants, on purpose". All ten contrast ratios it cites recomputed and correct. |
| m6·2 | `lucide-static 1.42.0` unverifiable | Note | unchanged | Not in the order. The glyph path data is right; the version string is still uncheckable offline. |
| m3·1 | ULIDs inside printed commands | Major | **partial** | Printed commands: fixed — `Handle` and `Id` are distinct branded types, and every builder in `commands.ts` takes `Handle` only. **Rendered text: not fixed** — see the blocker below. |
| m3·2 | seven hard-coded commands, undefined flags | Major | **partial** | All moved to `commands.ts`, guarded by a test that parses the real grammar out of `m2-cli.md`. Six of the seven `crm` commands match it exactly. `findCmd` does not — see below. |
| m3·3 | 25 rows do not fit 900px | Minor | **fixed** (by PM) | Box corrected on `main`, `m3-window.md:225`; `pageSize` stays 25, as ordered. |
| m3·4 | reminders caveat far from the due indicator | Minor | **fixed** | Measured at 1280px with a record open: the caveat sits on the strip directly under the header, its text ends at x=948, and the due counts start at x=946, 14px above. One line. |
| m3·5 | footer wording vs `crm find` | Deferred | still deferred | Window prints `10 of 10`; `crm find` is M2. |

Round-2 acceptance, as written:

| Box | Result |
|---|---|
| no id reaches stdout, a rendered string, or a printed command on either surface | **fails** — rendered string, `m3-window` |
| a test on each side fails if one ever does again | **half** — backend covers stdout; frontend covers printed commands, not rendered text |
| `preview.ts` fixtures use ULIDs | **passes** — 26-char Crockford, minted (`preview.ts:170-178`) |
| every command the window prints appears in the grammar | **fails** — `findCmd` quoting |
| stale `id`-shaped fixtures swept | **passes** |
| `cargo test` compiles and the count is reported; `npm run verify` green | **passes** — 112 and 20; both green |
| neither agent edited the other's tree | **passes** — backend touched `src-tauri/` only; frontend `src/`, `docs/brand.md`, `docs/window.md`, `preview.html`, `package.json` |

---

## 2. No id reaches a reader — tested against ULID fixtures

Every row below uses real ULIDs. Readable fixtures are how round 1's defect hid.

| Surface · channel | Fixture | ULIDs found |
|---|---|---|
| CLI · `status`, refusals, `-h`, stdout and stderr | live binary, store seeded through `AppState` with ULID ids | **0** |
| CLI · unit guard | `Ulid::from_parts`, scan any field | **0** |
| Window · printed commands | branded `Handle`; guard test | **0** |
| Window · rendered text, preview scenarios | minted ULIDs | **0** — alarm quiet |
| Window · rendered text, **real `m1-core` snapshot**, board | core snapshot, verbatim | **3** — alarm fires |
| Window · rendered text, real `m1-core` snapshot, pending | core snapshot, verbatim | **0** |
| Window · drag payload (`text/plain`) | any card | **1 per drag** |

**How the real snapshot was obtained.** A throwaway test in a scratch copy of `m1-core` built
a state through the core's own API — companies, three deals across stages, an agent
activity, a task, `find("")`, `show(deal)`, and a second state parking an ambiguity — then
wrote `AppState::snapshot()` to JSON. A throwaway scenario in a scratch copy of
`m3-window`'s `preview.ts` loaded that JSON unmodified. Both scratch edits were reverted, and
neither branch was touched.

### BLOCKER — fed the real core, the window renders ULIDs and disagrees about what is open

`src/Board.tsx:175-179` · `src/App.tsx:126` · cross-branch with `src-tauri/src/state.rs:803-829`

```
window, real m1-core snapshot
  board card titles   01M207BM80B9D5MPJTB9D5MPJX
                      01M207BM80B9D5MPJTB9D5MPJZ
                      01M207BM80B9D5MPJTB9D5MPJY
  preview alarm       id leaked into the window: 01M207BM80B9D5MPJTB9D5MPJX, …
  record panel        Nothing open. Your agent can open a record here: crm show acme

core, same state
  focus               { "kind": "deal", "handle": "hollis-annual", "id": "01M2…JY" }
  crm status          looking at: deal hollis-annual
```

`DealCard` renders `<span className="card-title mono">{id}</span>` whenever `cards` is absent.
`cards` and `focused` are the fields `docs/window.md` §1 proposed. The PM has not settled
them, so the core does not send them — and against the only core that exists, that fallback
is always the path taken. `DealCard`'s `id` prop is a plain `string`, not `Id`, so the brand
does not reach it. The guard test only inspects command strings. The preview's own
leak alarm would catch this, but every scenario supplies `cards`.

The record panel half is a **two-surface disagreement**, which the QA order ranks as a
blocker: the agent opens a record, `crm status` names it, and the person's window says
nothing is open. That is the product's central promise failing.

**Needs a PM decision before either agent can close it:** settle `cards` and `focused` (or an
alternative), which then lands partly on the backend. Independently of that, the window's
fallback must not print the id — the frozen snapshot gives it no handle to print instead.

### MINOR — dragging a card puts its ULID in the drag payload

`src/Board.tsx:188`

```tsx
onDragStart={(e) => e.dataTransfer.setData("text/plain", id)}
```

`text/plain` is what other applications accept on drop. Dragging a card onto the terminal,
where this app's agent lives, pastes `01M2…` — a string nobody can type back into a verb. A
private MIME type (or the handle as the `text/plain` flavour) keeps the internal move working.

---

## 3. Every command the window prints, against the grammar

Read from `src/commands.ts`, the single source since round 2, and checked character by
character against `m2-cli.md` § The argument grammar.

| Window prints | Where | Grammar | Match |
|---|---|---|---|
| `crm show acme` | `Record.tsx:38` | `crm show <handle>` | **yes** |
| `crm find <query>` | `Table.tsx:62` | `crm find <query> …` | **no** — query unquoted |
| `crm add deal "Northwind renewal" --company northwind` | `Board.tsx:82` | `crm add deal <title> [--company <handle>] …` | **yes** |
| `crm task <handle> "call back" --due 2026-09-30` | `Record.tsx:67` | `crm task <handle> <what> --due <date>` | **yes** — `YYYY-MM-DD` |
| `crm log note <handle> "…"` | `Record.tsx:87` | `crm log <call\|email\|meeting\|note> <handle> <body> …` | **yes** |
| `crm import contacts.csv` | `Table.tsx:66` | `crm import <path> …` | **yes** |
| `crm select 2` | `Panels.tsx:33` | `crm select <n>` | **yes** |
| `clatch agent grant <name> app:com.breksos.crm` | `Attribution.tsx:85` | not ours | **yes** — matches `clatch --help`: `clatch agent grant <name> app:<id>`; a documented exception in `commands.ts` |

### MAJOR — `crm find` hint breaks on any search containing a space

`src/commands.ts:28-30` · rendered at `src/Table.tsx:62`

```
typed into the search    acme "big" corp
window prints            crm find acme "big" corp
a shell passes           find · acme · big · corp        ← three positionals
grammar                  crm find <query>                 ← one
```

Every other free-text argument in `commands.ts` is quoted; `query` is not. The grammar takes
one `<query>`, and `m2-cli.md` § Exit codes makes an unexpected argument an **exit 2**. So
the hint fails verbatim for any multi-word search — "Acme Corp" included — and a query
containing `"`, `$` or a backtick is not a safe string to hand someone to paste into a shell.

The guard test missed it because it renders `findCmd("acme")`, a single word. It checks the
verb and the flags, not the shape of the positionals.

### MINOR — two example commands name records that do not exist where they are shown

`src/Board.tsx:82` · `src/Record.tsx:38` · a PM contract question, not a frontend defect

`crm add deal … --company northwind` is shown on the empty board — where, in a real zero
state, no `northwind` company exists. `crm show acme` is shown when nothing is open, and the
real core's handle for "Acme Corp" is `acme-corp`, so `acme` is a fuzzy match rather than an
exact one. `round-2-fixes.md` explicitly approves the first as "correct and may be stated";
`m2-cli.md` § Acceptance requires the window's printed commands to "all work verbatim". Both
documents are the PM's. One of them needs to say that example values are examples.

---

## `m1-core` — new findings

### MINOR — the clock is now read twice, against its own doc comment

`src-tauri/src/main.rs:111-133`

```rust
fn clock() -> Now {
    let at = std::time::SystemTime::now() …;   // read 1
    Now { at, today: local_today() }            // read 2: chrono::Local::now()
}
```

The comment directly above `clock()` says the time is read **once**, because an instant
and a date taken separately can straddle midnight. Round 1's version derived the date from
`at`; the local-time fix reintroduced the second read. The window is milliseconds wide
and real: at 23:59:59.999 a command can carry today's instant and tomorrow's date. Deriving
the local date from `at` keeps one read.

### MINOR — the local-time tests cannot fail on a UTC machine

`src-tauri/src/main.rs` tests `today_is_the_local_day_not_the_utc_one`,
`the_clock_and_the_calendar_agree_about_today`

Both read the real system clock and zone. On a runner set to UTC — the usual CI default —
local and UTC are the same day, so reverting to UTC would still pass. They are meaningful
here (`+03`) and under the zones I forced, but not as a regression guard. Pinning a zone per
test, or injecting the offset, would make the guarantee hold everywhere. `m2-cli.md` § Tests
already asks for "a date at 23:00 local does not land on the wrong day".

---

## Process — owned by the PM or release, not either agent

### MINOR — no gate builds the two surfaces together, and `verify` never runs `npm test`

`scripts/verify.sh` (on `main`)

`m1-core`'s verify builds its core with `main`'s placeholder window. `m3-window`'s verify
builds its window with `main`'s M0 core. The combination this report's blocker lives in is
built by neither. Separately, the six frontend guard tests are not part of `npm run verify`,
so a green verify says nothing about them. `scripts/` is outside both agents' trees.

---

## What this round did, and did not do

**Did:** ran rule zero on both branches in their own clean worktrees. Drove the real
`m1-core` binary against a store seeded through the core, under an isolated `HOME`, using a
short `/tmp` symlink into the scratch directory because the Unix socket path limit
(`SUN_LEN`) rejected the long one. Ran the clock tests under four timezones. Dumped real core
snapshots through a throwaway test and fed them to the real window through a throwaway
preview scenario. Checked every printed command against the grammar by hand. Recomputed every
contrast ratio in the new `brand.md` paragraph.

**Did not:** fix anything, merge anything, or touch `clappkit/`. Every scratch probe was
reverted, the symlink and seeded store were removed, and neither live checkout
(`CRM-clapp` on `m1-core`, `CRM-clapp-frontend` on `m3-window`) was touched.

**Could not:** run the charter's agent pass. All sixteen verbs are still M2, so it carries
to the M2 round unchanged.
