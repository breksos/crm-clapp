# QA round 1 — `m1-core`, `m6-brand`, `m3-window`

**Reviewed:** 11 September 2026 · **By:** QA · **Verdict:** 2 send back, 1 pass

Findings only. Nothing was fixed, nothing was merged, nothing under `clappkit/` was
touched. Each branch was verified in its own clean worktree against the acceptance
checklist in its own work order.

| Branch | At | Verdict | The one sentence |
|---|---|---|---|
| `m1-core` | `73d57c3` | **send back** | The core is strong and the tests genuinely run — but the one read verb an agent has prints a ULID where the snapshot hands it a handle, and the test guarding it asserts the pre-revision fixture. |
| `m6-brand` | `823dead` | **pass** | Every measurable claim measured and true; all four assets regenerate byte-identical. The only finding is a missing paragraph in `brand.md`. |
| `m3-window` | `d08a84d` | **send back** | The window hard-codes CLI commands it tells the person to type — including opaque ids — against an argument shape M2 has not agreed to. |

## The two send-backs are one defect seen from both ends

The M1 revision turned ids into ULIDs and added `handle` beside them, **additively**, so
that nothing would break. Neither surface picked it up. `crm status` prints `focus.id`;
the window interpolates `row.id` into commands it asks the person to type. Both have
`handle` available in the snapshot and neither reads it.

Worth fixing as one change, not two — and it is the only thing standing between these
branches and a merge. **If the backend and frontend are briefed separately, each will fix
half, and the other half will keep looking fine in its own harness.** That is how this
survived a revision.

---

## Rule zero — the commands, and their real output

Run on `m1-core` at `73d57c3`. The revision fixed the defect it was raised for: the test
file compiles, and **103 assertions executed** rather than being asserted.

| Command | Result |
|---|---|
| `cargo test` | **103 passed, 0 failed**, 0 ignored — compiles |
| `cargo clippy --all-targets` | clean on our code (one upstream `block v0.1.6` future-incompat) |
| `cargo fetch --locked` | exit 0, with ssh forced off |
| `npm run verify` | all 7 steps green |
| `npm run pack` → `clatch install` → `clatch run` | `crm status` round-trips on a real install |
| `crm status`, app **not** running | exit 1 — “app is not running — start it with `clatch run com.breksos.crm`” |
| installed bundle directory | `bin/crm.app/…`, not `Breksos CRM.app` — playbook §12b avoided |

---

## `m1-core`

### MAJOR — `crm status` prints the raw ULID where a handle exists

`src-tauri/src/cli.rs:126-128`

```
ran    status_lines() against the post-revision snapshot shape
want   looking at: deal acme-renewal
got    looking at: deal 01K4Z8QH3M7XC9VBN2RTFA6EDS
```

`focus_json()` at `state.rs:840-846` puts `handle` right beside `id` for exactly this, and
`cli.rs` never reads it — grep for `handle` in that file returns nothing.
`m1-revision.md` §2 is explicit: *“Do not start displaying `id` anywhere.”* An agent
cannot type a ULID back into any verb.

### MAJOR — the test that should have caught it pins the pre-revision model

`src-tauri/src/cli.rs:388-390`

```rust
status_lines(&json!({ "focus": { "kind": "deal", "id": "acme" } … }))
assert!(out.contains("deal acme"))
```

`"acme"` was the **id** before the revision and is the **handle** after it. The fixture was
never updated, so the test passes and verifies nothing — green, and pinning a model the
code no longer has. Rule zero's failure mode, one layer down.

### MINOR — every argument after the verb is silently discarded

`src-tauri/src/cli.rs:51`

```
crm status --json           → human text, exit 0
crm status extra args here  → exit 0
crm close --please          → "bye", exit 0
```

`run()` reads `args.first()` and drops the rest. Nothing can produce the `exit 2` the
manual documents for a bad argument — only a bad *verb* does. M2 is where arguments
arrive, but the dispatch shape freezes here.

### MINOR — the exit code for a declared-but-unbuilt verb contradicts the manual

`src-tauri/src/cli.rs:37-44`

```
crm add            → exit 2
crm status (down)  → exit 1
```

The manual's table reads `1  the app is not running, or it refused`, and its own M2
paragraph says calling an unbuilt verb *“is refused”*. An agent branching on the code
cannot separate a typo from a not-yet verb. The message text can; the code cannot.

### Noted, not findings

- **`local_today` is UTC, not local** (`main.rs:119-134`) — disclosed in place and already
  flagged to settle before M4, when a day-out bucket becomes a reminder firing on the
  wrong day.
- **`snapshot()` bumps `rev` on every call**, including `status` (`state.rs:803`), so an
  agent's *read* pushes a fresh snapshot to the window. The response and the push share
  one rev, which is what §6 actually requires.

---

## `m6-brand`

No Pillow on this machine, so the assets were decoded directly and measured against real
pixels rather than eyeballed.

| Check | Measured |
|---|---|
| `icon.png` | 1024×1024 RGBA · 21,194 B · opaque bbox **100% × 100%** |
| corner radius | first opaque px x=215 at y=0, reaching x=0 at y=230 → **r ≈ 230 = 0.225 × 1024** |
| tile ground / glyph | **exactly** `#123B33` / `#F2EFE6` |
| `banner.png` | 3440×512 · ratio **6.71875 = 215:32 exactly** · 44,190 B |
| banner left 40% | **zero** non-ground pixels below x=1376 |
| banner palette | `#123B33` · `#F2EFE6` · `#2E8B72` — sampled from the icon |
| `icon.ico` | **7 images** — 16/24/32/48/64/128/256 |
| at 128px tall | discs 16px, thinnest ink 16px against a 6px floor — motif reads |
| `render-brand.py` | all four assets regenerate **byte-identical** (md5 unchanged) |
| the mark | Lucide `square-kanban` verbatim · ISC credited |

### MINOR — `brand.md` publishes a palette the window does not use

`docs/brand.md:62-64`

```
brand.md      accent #2E8B72   due #C08A2E
styles.css    accent #1c6b57   due #9a6415
```

The darker pair is **correct** — it is what `m3-window.md` specifies, and the brand values
fail AA on paper: `#2E8B72` is 4.16:1 on white and `#C08A2E` is 3.04:1, against 6.38:1 and
4.99:1 for what shipped. The defect is only that `brand.md`, the record of the palette,
never says the chrome uses darker variants or why. One paragraph fixes it.

### Unverifiable — the `lucide-static 1.42.0` citation

`scripts/render-brand.py:62`

Lucide is not a dependency and is not on disk. The path data *does* match Lucide's
published `square-kanban` — `M8 7v7` · `M12 7v4` · `M16 7v9` — so the glyph is right; only
the version string is uncheckable without the package.

---

## `m3-window`

Driven live through `src/preview.ts` at 1200×800 and 1200×976, both themes plus the
un-stamped default in both OS appearances. Theme states were confirmed from **computed
tokens**, not screenshots — the first screenshot of the light switch was a stale paint and
the DOM was correct underneath it.

Verified:

- every token in bare `:root` before any media or `[data-theme]` block
- the un-stamped default correct in both OS appearances
- zero external requests; five `.woff2` served from disk
- table rows exactly 32px, header 28px, tabular numerals
- six columns; totals one line per currency, never summed across them
- cards carry three fields plus the attribution disc
- an agent move rings the card 2px in `#45548C` = `agentTint(by.id)`, 1.2s `ring-pulse`
- a roster rename relabels in place: same `<li>`, same `<img>`, same avatar src
- `prefers-reduced-motion` honoured; the ring still marks the card
- focus ring live on a real Tab, nothing left uncompensated
- no gradient, zero `box-shadow`, no emoji, max radius 6px, no `Inter`, nothing centred
- no “ask the agent” control anywhere
- `preview.ts` covers seven states, including all six required
- nothing derived committed on any branch

### MAJOR — the window prints opaque ULIDs inside commands it tells the person to type

`src/Record.tsx:61` and `:81`, `src/bridge.ts:142-151`

```
crm task {row.id} "call back" --due 2026-09-30
crm log note {row.id} "…"
```

`Row` in `bridge.ts` has **no `handle` field** — the window was built against the
pre-revision snapshot. M1 added `handle` beside `id` additively for exactly this case. The
preview fixture uses readable ids (`d_hollis`, `c_acme_hold`), so the defect is invisible
in the harness and unusable against the real core.

### MAJOR — seven CLI invocations are hard-coded as instructions, using flags M2 has not defined

`Board.tsx:78` · `Record.tsx:61,81` · `Table.tsx:61,65` · `Panels.tsx:32`

```
crm add deal "Northwind renewal" --company northwind
crm task <id> "call back" --due 2026-09-30
crm log note <id> "…"
```

`crm -h` documents none of these flags. `docs/window.md` §3 honestly records the envelope
as *“proposed, not settled”* — but the **window** states them to the person as fact. Every
one is a promise M2 must keep or the window is lying. This is the M2/M3 integration freeze
named in architecture §12, and it needs the PM before both land.

### MINOR — “a 25-row page fits a 900px window” is not met

`m3-window.md`, acceptance

```
shell height    900px   (measured, not extrapolated)
header 44 · list bar 43 · thead 28 · footer 32
rows that fit   23      of a 25-row page
shell scrolls   false
```

The intent holds — the shell never scrolls, the list scrolls inside itself. The number does
not. Either the acceptance box or the default `pageSize: 25` should move.

### MINOR — the “reminders need the app open” line is not near the due indicator

`src/App.tsx` — `.rail-foot`

The due indicator sits top-right in the header; the REMINDERS caveat sits bottom-left in
the rail, diagonally opposite the thing it explains. The work order asks for it *“near the
due indicator”*. It is present and well worded — just not where somebody reading “2
overdue” is looking.

### Deferred — the pagination footer's wording

`src/Table.tsx:131`

The window prints `10 of 10`, verified. `crm find` does not exist. Not tickable on one
side's evidence; it carries to the M2 round.

---

## The pass that matters most could not be run

The charter asks QA to add a company, log a call, set a next step, move a deal, page a
list and trip an ambiguity — from `crm -h` alone, with the source closed. **All sixteen of
those verbs are M2 and deliberately refused.**

What *could* be driven as an agent:

1. **The manual is complete and truthful for this build.** It fits 80 columns, generates
   the stage vocabulary from the core's own list, and names what is missing and which
   milestone lands it.
2. **Refusals teach.** A typo, a declared-but-unbuilt verb and a dead app are three
   distinguishable messages, each pointing at `crm -h` — though the exit codes do not
   distinguish the first two.
3. **Ambiguity is unreachable.** `pending` is implemented in the core, pinned by tests, and
   renders correctly in the window with three candidates and `crm select 2` — but no agent
   can reach it.

Items 1 through 3 of the charter's agent pass carry over to the M2 round in full.

---

## What this round did, and did not do

**Did:** ran every command in `CLAUDE.md` on each branch in its own clean worktree, packed
and installed the depot, drove the CLI from the manual with the source closed, and drove
the window through its preview harness.

**Did not:** fix anything, merge anything, or touch `clappkit/`. The test install was
uninstalled afterwards and `main` is untouched.
