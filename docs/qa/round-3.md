# QA round 3 — `m1-core` (merged), `m3-window`, release round 1

**Reviewed:** 22 September 2026 · **By:** QA · **Verdict:** pass, pass, pass
**Against:** [`round-3-snapshot.md`](../work-orders/round-3-snapshot.md),
[`release-r1.md`](../work-orders/release-r1.md) · **Previous:** [`round-2.md`](round-2.md)

Findings only. Nothing was fixed, nothing was merged, nothing under `clappkit/` was
touched. Three things were reviewed: the frontend's round 3 (`m3-window` @ `b3b563b`), the
two branches merged to `main` without a QA pass (backend round 3 `3c12f5a`/`da85ed8` and
release round 1 `4ad4a17`/`172ed2d`), and the two surfaces built together.

| Item | Verdict | The one sentence |
|---|---|---|
| `m1-core` round 3, on `main` | **pass** | Every round-2 backend item is fixed and proven live — a store seeded through the core, a real `crm show` against a raw ULID, and the golden gate demonstrably fails when the core changes underneath it. |
| `m3-window` round 3 | **pass** | The blocker is closed structurally, not by convention: `Id` is no longer a string at the type level, so a component that tries to render one fails to compile, and the leak guard now scans real rendered output — including the two files the core actually emits. |
| release round 1, on `main` | **pass** | `npm run verify` now runs the window's own tests, a real CI workflow exists and has since run four times on a real remote, and the release plan answers everything it was asked. |

## A correction to the brief

**`m3-window` is not based on `main` from before the backend's round 3.** Its merge base
with `main` is `8fc8106`, which is *after* both `da85ed8` (backend round 3) and `172ed2d`
(release round 1) merged — the sequencing `round-3-snapshot.md` asked for was followed.
`main` has moved two commits further since (`2f8a36a`, `19ff7f1`), but both are **PM
documents only** — M7 and M8 work orders, and matching edits to `CLAUDE.md`,
`architecture.md` and `m2-cli.md`. A full merge of `m3-window` into current `main`
confirmed this directly: `diff -rq` between the two trees, excluding `.git`, shows **zero
code differences** — only those same doc files. `m3-window`'s own tree already *is* the
combination this round exists to test.

That does not make the merge step wasted — it is what proved the claim rather than assumed
it — but the review below tests one artifact, not two glued together, because there was
only ever one to test.

---

## Rule zero — every tree, real counts

| Tree | `cargo test` | `cargo clippy --all-targets` | `cargo fetch --locked` (no ssh) | `npm test` | `npx tsc --noEmit` | `npm run verify` |
|---|---|---|---|---|---|---|
| `main` alone (backend r3 + release r1, no window) | **160 passed, 0 failed** | clean¹ | exit 0 | — | — | green, 8/8, correctly notes *"no `test` script on this branch"* |
| `m3-window` alone (`b3b563b`) | **160 passed, 0 failed** | clean¹ | exit 0 | **23 passed, 0 failed** | clean | green, 8/8 |
| `main` + `m3-window` merged | **160 passed, 0 failed** | clean¹ | exit 0 | **23 passed, 0 failed** | clean | green, 8/8 |

¹ the sole warning on every tree is upstream: `block v0.1.6` will be rejected by a future
Rust — not our code.

`cargo test` under `TZ=UTC` and under `TZ=Pacific/Kiritimati` (+14:00) both still return
**160 passed, 0 failed** on every tree — the local-date fix does not depend on the runner's
zone, which is the point of it.

---

## 1. Every round-2 finding, one by one

| # | Round 2 | Sev | Status | Evidence |
|---|---|---|---|---|
| 1 | fed a real snapshot, the window renders 3 ULIDs and disagrees about what is open | Blocker | **fixed** | See §2 below — live, against a real seeded store and a real running app. |
| 2 | dragging a card puts its ULID in the drag payload | Minor | **fixed** | Live, in the browser, against the real fixture: `dataTransfer.getData("text/plain")` → `acme-pilot`; the private `application/x-breksos-deal` payload carries the ULID (`src/Board.tsx:232-235`). |
| 3 | `crm find` hint breaks on any search with a space | Major | **fixed** | `commands.ts`'s `shellQuote()` (`src/commands.ts:26-34`) is exercised against a **real `/bin/sh`**, not read — `commands.test.ts` and `window.test.ts` both round-trip five hostile strings (space, `"`, `'`, `$HOME`, `` `id` ``, and all five combined) through `execFileSync("/bin/sh", …)` and assert the shell reads back the original value as one word. |
| 4 | two example commands name records that do not exist | Minor | **fixed** | `addDealCmd` no longer takes `--company` at all (round-3 order, adopted verbatim). `RecordPanel`'s empty state now picks a real row's handle when one exists, and falls back to `crm add company "Acme Corp"` only when there are none (`src/Record.tsx:153-169`, `src/App.tsx:167-177` — `exampleHandle()`). Verified live against the real board: the shown handle exists on the board. |
| 5 | `m1-core`: the clock is read twice | Minor | **fixed** | `clock()` now calls `chrono::Local::now()` once and derives both `at` and `today` from that one value (`src-tauri/src/main.rs:111-115`). |
| 6 | `m1-core`: local-date tests cannot fail on a UTC machine | Minor | **fixed** | `local_date(at_ms, offset_secs)` is a pure function taking the offset as an argument rather than reading the zone; tests pin `+14:00`, `-12:00`, and a millisecond either side of the India (+05:30) midnight boundary — none of which can pass by accident on a UTC runner (`src-tauri/src/main.rs:127-133` and its tests). |
| 7 | process: `npm run verify` never ran `npm test`; nothing built the two surfaces together | Minor | **fixed** for the first half; **the second half is now moot** — see the correction above | `scripts/verify.sh` step 2/8 runs `npm test --if-present` and `tsc --noEmit`, and reports "no test script" on a branch without one rather than passing silently — verified on `main` alone. The "two surfaces together" half no longer needs a gate to prove it: `m3-window`'s committed `src-tauri/fixtures/*.json` are read by its own `npm test`, so its own `npm run verify` already builds and tests the real combination on every run. |

Every round-2 item is closed.

---

## 2. No id reaches any reader — tested against ULID fixtures, live

Round 2's method — a scratch probe seeding a real store through the core, and a scratch
preview scenario loading the real snapshot — is no longer needed as a workaround. Round 3
built exactly that seam **as the product**: `src-tauri/fixtures/snapshot.json` and
`snapshot-pending.json` are committed, generated by the core's own test, and consumed
verbatim by both `npm test` and the interactive preview harness (`src/scenarios.ts` reads
them with `import.meta.glob`).

I still re-derived the check independently rather than trust the harness that grades
itself:

| Channel | Fixture | ULIDs found |
|---|---|---|
| Committed golden fixtures, both files, both views (board/people), text **and** attributes | as committed | **0** (`window.test.ts`, re-run) |
| Interactive preview, "Core: snapshot" scenario, in a real browser | `src-tauri/fixtures/snapshot.json`, loaded live | **0** — `document.getElementById('root').innerHTML` scanned for the ULID pattern by hand |
| A card with its body deliberately dropped (`cards: {}`) | golden snapshot, mutated at test time | **0** — renders `card-skeleton`, one per deal, never an id |
| Real binary, real running app, store seeded through `AppState`'s own API with real ULIDs | a throwaway backend test, reverted | **0** in `crm status`, `crm show`, refusals, `-h` |
| `crm show` given the **raw ULID** instead of the handle | same seeded store | resolves and prints the **handle** in its output, never echoes the id back |
| Drag payload | live browser, real fixture | `text/plain` → handle; only the private MIME type carries the id |

```
$ crm show 01M207BM80B9D5MPJTB9D5MPJX
    deal acme-renewal — Acme renewal
      Company  Acme Corp (acme-corp)
      Value    $45,000.00
      Stage    Lead
      Status   Open
```

**The structural fix, which is the reason I trust this to hold**: `Id` is no longer a
branded *string* (round 2's fix) but an opaque type with no string in its definition
(`src/ids.ts:32-49`). A branded string is still assignable to `ReactNode`, so
`<span>{card.id}</span>` compiled under round 2's types — which is exactly how the blocker
happened. It does not compile under round 3's. `DealCard`'s `id` prop is typed `Id`
(`src/Board.tsx:195-220`); the only sanctioned exit is `idKey()`, used for the React key,
the drag payload's private slot, and the `run_cmd` envelope — never for anything rendered.
I confirmed the type actually bites by checking `tsc --noEmit` is clean *as committed* and
reading every one of the handful of `idKey(` call sites; none reach JSX text or an
attribute a person reads.

**I also confirmed the gate that would catch a regression is real, not decorative.** I
added one bogus key to `AppState::snapshot()`'s JSON on the backend and reran the golden
tests without touching the fixture files:

```
thread 'state::tests::golden_snapshot_matches_the_committed_fixture' panicked:
.../fixtures/snapshot.json no longer matches what the core produces.
If the snapshot changed on purpose, regenerate it and review the diff: …
test result: FAILED. 2 passed; 2 failed
```

Both golden tests failed immediately, naming the exact fix. Reverted; `cargo test` is back
to 160/160.

---

## 3. Round-3 acceptance, box by box

| Box | Result |
|---|---|
| fed `fixtures/snapshot.json` verbatim, the window shows real card titles, the open record, its timeline and tasks, and zero ids in rendered text | **passes** — re-verified live in the browser, not only via `window.test.ts` |
| the leak guard covers rendered text and runs over the golden fixtures | **passes** — `findIds()` scans decoded text *and* HTML attributes; `window.test.ts`'s `describe("the core's golden snapshots")` runs it against both committed files |
| the golden test fails when the snapshot changes and the fixture does not | **passes** — demonstrated live, §2 above |
| `crm show` and the record panel list the same fields in the same order | **passes** — both read `focused_json()`'s `fields` array (`src-tauri/src/state.rs:993`); a backend test (`cli.rs:693`, `show_prints_exactly_the_fields_the_window_draws`) reads the **golden fixture itself** and asserts `crm show`'s printed lines match `focused.fields` field-for-field |
| timeline capped at 50 with a total; tasks carry handles | **passes** — `focused_json()` takes the newest 50 and reports `timelineTotal` separately; every task JSON object carries `handle` |
| every money value carries `formatted`; the window has no formatter of its own | **passes** — a dedicated test (`the window formats no money`) greps every non-test, non-scenario source file for `NumberFormat`, `toFixed(`, `formatMoney` or arithmetic on `.amount` and asserts none exist |
| `run_cmd` accepts each envelope shape; each has a test | **passes** — `state.rs`'s `command()` dispatches `state`/`show`/`move`/`select`/`find` (`cmd_find` at `state.rs:1375` handles an omitted field as "keep current value" with a three-way `None`/`Some(null)`/`Some(value)`, matching the spec's semantics precisely, not just its shape) |
| every interpolated command value is shell-quoted; the five hostile characters are tested | **passes** — see §1 item 3; tested against a real shell, not a regex |
| clock read once; the local-date tests fail if UTC is reintroduced, on a UTC machine | **passes** — see §1 items 5–6 |
| `cargo test` count reported; `npm test` and `npm run verify` green | **passes** — 160 and 23, both real, both reported above |
| nothing removed, renamed or retyped in the snapshot | **passes** — every round-1/round-2 key (`ok`, `rev`, `pipeline`, `board`, `focus`, `list`, `pending`, `due`, `counts`, `agents`) is present unchanged in the golden fixture; every addition (`cards`, `focused`, `list.kind`, `formatted`) is new, not a rename |

Every acceptance box is met.

---

## Scope discipline

Checked by diffing each agent's own commit against its own parent, not against `main`:

- Backend round 3 (`8f9a155`→`3c12f5a`) touches only `src-tauri/`: `cli.rs`, `main.rs`,
  `model.rs`, `state.rs`, `state_tests.rs`, `store.rs`, and the two new
  `src-tauri/fixtures/*.json`.
- Frontend round 3 (`89c6ee3`→`b3b563b`) touches only `src/*`, `docs/window.md`. It does
  **not** touch `src-tauri/fixtures/` — the order's one explicit prohibition for this side.
- Release round 1 (`8f9a155`→`4ad4a17`) touches only `.github/workflows/verify.yml`,
  `docs/release-plan.md`, `scripts/verify.sh`. `clatch.json` is untouched, and no `windows`
  or `linux` key was added to `launch`.

---

## Release round 1, reviewed on `main`

`release-r1.md`'s own acceptance said *"`verify.yml` exists, passes a syntax check, and is
reported as never run"* — true when it was written, because there was no GitHub remote
yet. **That has since changed**: `origin` now points at `breksos/crm-clapp`, and the
workflow has run four times, on real GitHub Actions, all green —
including on `release-r1`'s own branch before it merged, and on `main` after both of the
two PM-only commits since. This is not a finding against the release order; it is the
report catching up to a fact that changed after the order was written.

One thing worth naming rather than treating as resolved: **`m1-core` and `m3-window` were
never pushed to the remote.** `git ls-remote --heads origin` shows only `main` and
`release-r1`. `verify.yml` triggers on `push` and `pull_request` with no branch filter, so
it would have run on either feature branch — it simply never saw them, because in this
project's workflow a branch is merged locally by the PM before anything reaches GitHub. CI
therefore checks `main` after a merge, not a branch before one; it did not, and could not,
have caught anything in this round, since I verified the branches by hand before they were
ever pushed. Not a defect in what was ordered — release-r1 was asked to wire the gate, not
to change how branches reach the remote — but worth the PM knowing the gate's actual
reach, since round 2 asked for exactly this kind of coverage and it is still a manual step.

`docs/release-plan.md` answers all five questions the order asked for, with specifics
(`macos-arm64` + `macos-x64`, deferred Windows/Linux and why, the `release.yml` steps, the
pre-publish gate table, `release-check.sh`, and a genuinely investigated section on
Gatekeeper/quarantine behavior on this machine — not a boilerplate "not covered" list). I
did not re-run `release.yml` itself: there is no tag, no `v*` push, and the order does not
ask for one; it asks for the plan to exist and be answerable, which it is.

---

## What this round did, and did not do

**Did:** ran rule zero on `main` alone, on `m3-window` alone, and on the two merged, in
three separate clean worktrees. Merged `m3-window` into a scratch copy of `main` and
confirmed the merge was a no-op for every file that is not a PM document. Seeded a real
store through the real core with ULID ids under an isolated `HOME` (via a short `/tmp`
symlink — the same `SUN_LEN` socket-path limit as round 2), ran the packaged binary
against it, and read every command's real output by hand. Loaded the actual committed
golden fixture into the actual window in a real browser and scanned the live DOM. Fired a
real `dragstart` and read the real `DataTransfer` payloads. Broke the core's snapshot on
purpose to confirm the golden gate reacts, then reverted it. Diffed each agent's commit
against its own parent to check tree discipline, not against `main`.

**Did not:** fix anything, merge anything, or touch `clappkit/`. Every scratch probe,
seeded store, and mutated file was reverted; all three review worktrees are removed;
`main`, `m1-core` and `m3-window` are exactly as their owners left them.

**Out of scope, as given:** the window's missing add/edit controls (M7), the dark theme
(M8), and the sixteen CLI verbs (M2) — none reviewed here.
