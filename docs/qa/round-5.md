# QA round 5 — the pre-release look

**Reviewed:** 24 September 2026 · **By:** QA · **Verdict:** send back — **do not publish the v0.1.0 draft as staged**
**Against:** [`m4-timer.md`](../work-orders/m4-timer.md), [`m11-colour-ownership.md`](../work-orders/m11-colour-ownership.md), [`m2-cli.md`](../work-orders/m2-cli.md), `docs/release-plan.md` · **At:** `main` @ `a314b57`, and the `v0.1.0` draft (tag `4310bfa`) · **Previous:** [`round-4.md`](round-4.md)

Findings only. Nothing was fixed, merged, tagged or published. Clatch was `0.4.5-stage.23`.

**Everything Clatch-shaped ran under an isolated `HOME` with its own daemon, and a `debug`-backend
agent I created there.** None of your real agents, apps or data were touched, and no signal reached
a real agent. The isolated daemon was stopped and every scratch home removed afterwards.

| | Verdict | The one sentence |
|---|---|---|
| 1 · The agent pass | **send back** | M2 round 3 works — `done` is reachable and `select` names its write — but `crm find` still hides results behind a sticky query it never mentions, and I under-called that in round 4. |
| 2 · The person's pass | **pass** | Every M7 control and M11's desk work end to end; the window is simply blind to the timer, which is item 3's finding, not this item's. |
| 3 · M4's timer | **send back** | Six of the eight things you asked for hold outright against a real Clatch-bound agent; "fire once, never again" holds except when the app is quit within ~0.4 s of the send, and a refused reminder is invisible on both surfaces. |
| 4 · The draft release | **send back** | The assets are sound as artifacts — but they were built from a commit that does not contain M11, and item 3's flaw is in them. |

**The call you asked for: don't publish this draft.** The shortest path back is in the last section.

---

## Rule zero — `main` @ `a314b57`

| Command | Result |
|---|---|
| `cargo test` | **262 passed, 0 failed, 0 ignored** |
| `cargo clippy --all-targets` | clean on our code (the same upstream `block v0.1.6` warning) |
| `cargo fetch --locked` (ssh forced off) | exit 0 |
| `npx tsc --noEmit` | clean |
| `npm test` | **38 passed, 0 failed** |
| `npm run contrast` | **0 required pairings failing**; all five agent tints ≥3:1 as a shape on every ground, initials ≥4.5:1, ΔE from `--lost` 15.6–20.7 against a floor of 12 |
| `npm run verify` | **green, 8/8** |
| CI on the merge | green — [run 35984040241](https://github.com/breksos/crm-clapp/actions/runs/35984040241) |

`src-tauri/`, `clatch.json` and `scripts/` are **byte-identical** between the `v0.1.0` tag and `main`
apart from `scripts/contrast.py`. Everything M11 changed is window code. So the backend I tested
live below is the backend in the draft.

---

## 1. The agent pass — from `crm -h` alone, source closed

A full day against a fresh store, driving the packaged binary as an agent would: add a company, a
contact and a deal; log a call; set two next steps; find, open, page; trip an ambiguity four ways
(log, set, task, archive) and resolve each; move through proposal to won; archive, show while
archived, restore; complete a next step; export. **It completes.** Nothing had to be guessed.

**Round 4's two findings are fixed, and I checked each the way round 4 found it:**

```
$ crm show acme-renewal
    …
    Next steps  (finish one with `crm done <handle>`)
      book-the-kickoff  due 2026-10-04  Book the kickoff
$ crm due
    week 1
      send-the-signed-order-form  2026-09-26  Send the signed order form  (on acme-renewal)
$ crm done send-the-signed-order-form          →  done

$ crm log note acme "…"        →  3 candidates, exit 0     $ crm select 1  →  logged — acme-corp
$ crm set acme domain …        →  3 candidates, exit 0     $ crm select 2  →  updated — acme-industries
$ crm task acme "…" --due …    →  3 candidates, exit 0     $ crm select 1  →  task set — acme-corp
$ crm archive acme             →  3 candidates, exit 0     $ crm select 2  →  archived — acme-industries
```

`due` and `show` print the handle `done` takes, and every resumed write is named with the record it
landed on. `crm done nope` now says `` `crm due` and `crm show <record>` list open ones with their handles `` —
which is true. Of thirty-odd deliberately wrong invocations, bad command lines exit 2 and valid-but-declined ones exit 1
(one exception below), and all but the ones listed below name what was wrong and what to do.

### MAJOR — `crm find` reports "no results" and never says why, and its hint points at the wrong lever

`src-tauri/src/cli.rs:1161, 1175`

```
$ crm find deal                    ← "deal" is a QUERY here, not a kind — the grammar says so
    no results (page 1 of 0 total)
$ crm find --kind deal
    no results (page 1 of 0 total)
      filtered to deal — `crm find --kind all` searches everything
$ crm find --kind all              ← the advice, followed
    no results (page 1 of 0 total)                     ← nothing at all explains this
$ crm status | sed -n 4p
      list: query “deal”   kind all   sort updated   page 1 (0 total)     ← the only place the cause appears
$ crm find ""                      ← the actual fix, which nothing ever suggested
      acme-renewal … / ada-whitlock … / acme-corp …    3 of 3
```

`find` edits the shared list and "an omitted option keeps its current value" (`m2-cli.md`) — that is
the documented design. But **no `find` output ever states the active query.** A prior `crm find deal`
therefore makes every later `find` return nothing, the one hint printed blames the `--kind` filter,
and following that hint changes nothing. An agent will reasonably conclude the workspace is empty. I
reproduced exactly this in round 4 and recorded it as "working as designed, not a finding". The
*design* is documented; the *output* is what teaches, and it teaches the wrong thing. That was an
under-call, and it is the failure you described as "find was broken".

### MINOR — `crm set <handle> value ""` silently erases a deal's value

`src-tauri/src/state.rs:893` (`non_empty(raw)` → `None`)

```
$ crm set acme-renewal value ""      →  updated        (exit 0)
$ crm show acme-renewal              →  the Value line is gone; the board total is gone
```

`crm set -h` says nothing about clearing. The window's inline edit stops an empty commit client-side, so
the two surfaces disagree about whether empty is allowed. Ranked Minor only because the trigger is narrow
and the fix is one line; an agent templating an empty variable would lose a price and be told "updated".

### MINOR — smaller teaching gaps

- **`no record matches “x” — every record's handle is shown beside it in the window`** (`state.rs:956`) is
  the most common refusal and sends an *agent* to a GUI it cannot see. `m2-cli.md` gives the bar in so many
  words: ``want  no record matches "acmee" — try `crm find acme` ``. `crm find` is the true instruction.
- **`crm done <handle>` on a finished next step answers `done` (exit 0), twice.** `archive` on an archived
  record correctly refuses (exit 1). Same class of no-op, opposite behaviour.
- `crm find --kind widget` says "use company, contact or deal"; the grammar also accepts `all`.
- `crm import /missing.csv` exits **2** ("command line was wrong"); the request was valid and declined, which
  `m2-cli.md` assigns **1**. It also leaks `(os error 2)`.

---

## 2. The person's pass — terminal closed, M7 and M11

Driven live in the real component tree (`npm run preview`, only the Tauri transport mocked), because
`window.test.ts` renders every scenario collapsed and clicks nothing.

| Control (architecture §10b) | Result |
|---|---|
| New company / contact / deal | inline forms, no modal; the new record appears immediately |
| `set` (edit in place) | commits on a real blur; a programmatic `.blur()` does **not** — see the footnote |
| `log` composer | a note appears newest-first, attributed **You** |
| `task` composer + `done` | a step is added; the clock icon is a real `<button>` and strikes it through |
| `archive` / **Restore** | flips and reverses |
| `move` | the drag, plus a keyboard `<select>`; Stage survives a close (`Negotiation` / `Won`) |

**M11's desk panel** is on every page (Board, People, Companies), reachable from the rail, and is honest
about its limits: *Waiting on you* (the pending question), *Agents* (each and the last deal it moved),
*Recent moves* (an agent's only — a person's move correctly drops the deal out of the list). No split
timelines, no "ask the agent" control. Agent identity is one violet: a 3 px left edge on cards an agent
moved, and a tinted initial disc from clappkit's own hash. `--agent` is `#B182E7` in dark and sits well
clear of `--due` `#D3A76A`, `--won` `#63B69E` and `--lost` `#D98A7B`. No `box-shadow` exists in
`styles.css`. All eight icon-only buttons are 24×24 with an `aria-label`. Both themes read as an ordinary
business tool; the stage badge appears in the table and record, and not on board cards.

**Footnote — closing something I left open in round 4.** I reported one inline edit that would not commit
and could not reproduce it. It reproduced today the moment I called `input.blur()` from a script: the
preview page is not the focused document, so no real blur fires. A mouse click commits every time. It was
my method, not the window.

### Things worth knowing, not findings

- The preview harness lists three companies twice after "New company" (`scenarios.ts` `recordsFrom`/`repage`
  unions `list.rows` with a derivation). It is the mock, not the product — but a harness that lies in the
  *comfortable* direction is the failure round 2 was about, so it is worth a line.
- Not testable here: the window as a real Tauri webview. Everything above is the same components against a
  mocked transport.
- The flat-teal avatar fixture is known and already assigned; not counted.

---

## 3. M4's timer — against a real Clatch-bound agent

**Method.** The *released* arm64 depot, installed with `clatch install` into an isolated home, run with
`clatch run`, with a `debug`-backend agent granted `app:com.breksos.crm`. Deliveries were read from three
independent sources that agreed every time: `clatchd.log` (`signal task.due … [Run] -> <agent id>`),
`clatch agent log` (the agent's own timeline, including that it *ran a turn* on each), and `crm status`.
Person vs agent authorship used the real mechanism: no `CLATCH_AGENT_ID` for the person, the agent's real
id for the agent. For the upgrade test I built the last **pre-M4** commit (`9144e16`) and let it write the
data file, rather than hand-editing one.

| # | You asked for | Result | Evidence |
|---|---|---|---|
| a | fires once | **pass** | one `[Run] task.due` with `catchUp/count/tasks[handle, what, due, on]`; the agent ran it and finished `[done Ok]`. First delivery ~5 min after creation (the next tick), as `crm status` promised |
| b | never again across a restart | **pass — with a hole**, below | three relaunches, each left 12 s to settle: 0 further deliveries |
| c | a pile-up with nobody bound arrives as ONE signal on connect | **pass** | 30 person-made overdue tasks, agent revoked: held across a real tick (deliveries unchanged, status: *"30 due next steps, and no agent is connected to tell"*). On grant: **one** signal, `count 30`, ten listed, `more 20`, `catchUp false` |
| d | an agent's own already-due task must not wake it | **pass** | made with the agent's real id: no *waiting* line, 0 deliveries across a launch sweep |
| e | a person's must | **pass** | same task made as the person: delivered |
| f | upgrading wakes nothing | **pass** | pre-M4 data with two overdue tasks (one person's, one agent's): first M4 launch, agent bound, **0 deliveries**; both marked told; `crm status`: *"2 next steps were already overdue when reminders began — nobody was woken for them"* |
| g | completed / archived-record tasks never fire | **pass** | a done task and a task on an archived company: 0 deliveries; restoring the company put its task back to *waiting* |
| h | a refused emission is recorded and surfaced | **FAIL** | below |

### MAJOR — "fire once, ever" fails if the app is stopped within ~0.4 s of the send

`src-tauri/src/main.rs:33, 275` · `src-tauri/src/store.rs:137-180` · `src-tauri/src/state.rs:725`

The sweep sets `due_signalled_at`, emits, and hands the store to a debounced writer (`SAVE_QUIET` = 400 ms).
**Nothing flushes that writer on exit.** The writer's own comment says a closed channel "means the app is
going away — write immediately", but nothing closes it or waits for it; the process just ends.

```
# deterministic: stop the app the instant the signal is delivered, then relaunch
deliveries after the first launch:     5
$ clatch stop   (≈0.1 s after delivery)
after relaunch +12 s:                  6      ← the same next step, delivered again      (a correct app stays at 5)
```

I first met it by accident: three quick relaunches delivered `overdue-step-t` three times. With 12 s of
settling between them it never repeated, which is why (b) passes. The same missing flush loses **any**
acknowledged write in the same window, and that is the sharper form of the bug:

```
$ crm add company "Persist Two"      →  added company persist-two     (acknowledged)
$ clatch stop                        (immediately)
$ clatch run … ; crm find            →  Persist Two is not there.     `crm close` immediately: same.
                                        Waits of 0.6, 1.0 and 1.5 s before `crm close`, and 1.0 and 3.0 s before
                                        `clatch stop`, all persisted.
```

Real quits are not scripted, so the window is narrow — a launch sweep lands about 3–4 s after start, and
someone glancing at the app and closing it is the plausible collision. But the guarantee M4 asked for
("fires once and never again across a restart") is not a probability, and a CRM that acknowledges a write it
then loses is a data-loss bug in its own right. The fix is small and conventional: flush the writer on exit.

### MAJOR — a refused reminder is invisible on both surfaces, and would be lost

`src-tauri/src/main.rs:139-146` · `src/Panels.tsx:92-99`

M4 acceptance: *"a refusal is visible in the window and in `crm status`"*, and *"the reminders caveat states the
real behaviour"*. Neither holds in the running app:

- **Clatch does refuse, and the app never hears it.** Filling the agent's context queue (256) produced
  `clatchd: signal record.changed … -> REFUSED: 1790244317 cannot accept (all-or-nothing; nothing was
  delivered)` in the launcher's log, while `crm status` showed no refusal at all. `take_refusals` is a stub
  returning `Vec::new()` — the commit says so plainly: clappkit's `Control` discards the notification. That is a
  platform seam the frontend and backend cannot close alone.
- **So the core cannot undo a refused `task.due`.** It marks a task told *when it emits*
  (`state.rs:725`), and `note_refusal` — which would put it back — is never called. A refused run is a reminder
  lost for good, and `crm status` says *"last sent just now"*.
- **The window never reads `snapshot.reminders`.** `grep` finds no use of `reminders`, `lastSweepAt`, `refusal`,
  `backlog` or `awaiting` in any component. The window still shows M3's generic sentence
  (*"Reminders fire only while this app is open…"*), not when the last check ran, what is waiting, or the
  upgrade backlog. Only `crm status` says any of that.

I could not provoke a refusal of a `run` specifically: the `debug` backend never fills an inbox. What I
demonstrated is that Clatch refuses when it cannot accept, that the app has no receiving seam, and that its
bookkeeping assumes delivery. This is the outcome M4's own order named as the thing to prevent — *"a
full-inbox agent otherwise reads as a dead button"* — so it is ranked accordingly, with the platform limit
stated.

### MINOR

- **A muted agent silently loses reminders and the app says they were sent.** `clatch agent mute` drops the
  signal (`no bind passes the cut matrix (dropped)`); the task is marked told and `crm status` reports
  *"last sent"*. Muting is the person's choice, so dropping is defensible — but the reminder is consumed for good.
- **`catchUp: true` is set on the first emission of *each run*, not on a catch-up.** The very first delivery above
  was for a task made overdue while the app was running. The manual says a burst "that built up while the app was
  closed arrives as one signal"; the flag means something narrower and different.
- **"When an agent connects" means "at the next 5-minute check".** The pile-up arrived 219 s after the grant. The
  status line says *"sent at the next check"*, so it is honest — but it is not immediate.

---

## 4. The draft release, checked as a user would

Both assets downloaded from the draft with `gh`.

| Check | arm64 | x64 |
|---|---|---|
| `shasum -a 256 -c` against the shipped `.sha256` | **OK** | **OK** |
| against GitHub's own upload digest | **match** | **match** |
| `clatch validate` on the depot's *own* manifest | **valid** | **valid** |
| manifests identical across the two depots | **identical** (byte for byte) | |
| depot manifest vs repo manifest at the tag | differs only in `cliBin` added and `launch.macos` rewritten | same |
| `cliBin` / `launch.macos` = `bin/crm.app/Contents/MacOS/crm` | 5 components, all `[A-Za-z0-9._-]` | same |
| `icon` = `assets/icon.png` | safe; byte-identical to the repo's | same |
| bundle directory | `crm.app`, not `Breksos CRM.app`; `CFBundleDisplayName` carries the full name | same |
| binary | Mach-O arm64 | Mach-O x86_64 |
| zip | rooted at `clatch.json`, 5 deflated + 6 stored directory entries, 0 symlinks | same |

**Installed with the real launcher, arm64:** `clatch install` from the asset → `clatch run` → `crm status`
answers (and shows the M4 reminders line) → `crm -h` from the installed binary → `clatch stop` → `crm status`
fails with our own sentence, exit 1 → `clatch uninstall`. **Passed.**

### The two things M5 could not verify

**The x64 install — closed on Apple Silicon, open on Intel.** I installed the x64 depot from the file with the
real launcher, ran it (the binary is `x86_64`-only, so it can only run translated), and drove it: `crm status`,
`crm -h`, a write (`crm add company`), a read-back (`crm find`), `stop`. All worked. M5 had only a negative smoke
test under Rosetta. What I cannot close is a **native Intel** Mac; there is none here.

**A first launch from a quarantined download — partly closed.** With Gatekeeper on (`spctl --status`:
*assessments enabled*), I stamped a real `com.apple.quarantine` flag (`0083;…;Safari;<uuid>`) on the downloaded
`.clapp`:

- `clatch install` of the quarantined file works, and **Clatch's own unpacking does not carry the flag onto the
  installed files** — the installed binary has only `com.apple.provenance`. `clatch run` and `crm status` then work.
- With the flag stamped directly on the installed bundle (as Archive Utility would), `clatch run`, a direct exec,
  and `open` all launched with no block, and `clatch run` does **not** strip it (the flag was still there after).

What I cannot honestly claim: a **genuine browser download on a fresh account**. A hand-written xattr is not a
download event, and Gatekeeper's answer for a real one is exactly the unknown. `codesign --verify --deep --strict`
rejects the bundle (*"code has no resources but signature indicates they must be present"*, ad-hoc,
`Sealed Resources=none`) — identically to the installed sibling `Chess.app`, so nothing new. Someone with a second
Mac user account should still do this once before it is called closed.

### MAJOR — the draft was built from a commit that does not contain M11

`git log v0.1.0..main` is three commits: **M11's implementation** (`83b2f94`, the desk panel, violet family, no
board stripe), its palette proposal, and the merge. The tag sits on `4310bfa`, M11's *order*. M5's own report says
so ("`main`'s head at that commit, so any later merge is not in it"), and the backend and manifest are identical —
but the **window in the draft is the M10 window**: a stage stripe on every board card, no agent desk, stage-coloured
edges. M11's whole point was that colour means an agent did this and *not* stage. Publishing the draft ships the
design the product owner ruled against. The draft also carries item 3's two flaws.

### MINOR

- **The fix for the M4 hazard M5 found is not on `main`.** `RELEASE_CHECK_HOME` (run the install check under an
  isolated home so `clatch run` cannot signal a real agent) and the §7 results live only on the unmerged
  `m5-release`. `main`'s `scripts/release-check.sh` still runs on the real home.
- The draft's body is GitHub's auto-generated "Full Changelog" link and nothing else. Nothing tells a user what
  v0.1.0 is, or that reminders only fire while the app runs.
- `clatch install breksos/crm-clapp` (the GitHub route) cannot be tried until a release is published; unchanged.

---

## What would change this verdict

**Before v0.1.0 is published**, in this order:

1. **Flush the store on exit** (item 3, first Major). It fixes the duplicate wake and the lost acknowledged write in
   one change. Re-run: stop within 100 ms of a delivery, relaunch, expect no second delivery; `crm add` then an
   immediate `clatch stop`, expect the record.
2. **Make `find` say what it is filtering by**, and point the empty result at the right lever (item 1).
3. **Rebuild the release from a commit that contains M11** — move the tag, or cut `v0.1.1` — and merge `m5-release` so
   the safe install check is the one on `main`.

**Yours to decide, not mine to soften:** the refused-reminder gap needs a platform change (`Control` must expose
`app.toAgentRefused`) *and* a window that reads `reminders`. If you ship without it, the release notes should say
plainly that a reminder the launcher refuses is not reported. I would not call the M4 acceptance box ticked.

**Not blocking, worth a follow-up:** the muted-agent consumption, the `catchUp` naming, and the smaller teaching gaps.

---

## What this round did, and did not do

**Did:** rule zero on `main`; the agent pass from `-h` with every refusal read for truth; the M7/M11 window driven
live; the timer against a real Clatch-bound agent in an isolated launcher, timed against real 5-minute ticks; a
genuine pre-M4 data file for the upgrade test; both draft assets downloaded and checked; the arm64 depot installed,
run and uninstalled; the x64 depot installed and driven under Rosetta; the quarantine question tested as far as a
hand-stamped flag honestly goes.

**Did not:** fix, merge, tag or publish anything; touch `clappkit/`; test a native Intel Mac, a real
browser-downloaded first launch, the window as a real Tauri webview, or a `run`-signal refusal.

**Round 4, corrected:** I called find's sticky query "not a finding" — it is one (item 1). And the inline edit that
"would not commit" was my `.blur()` call, not the window (item 2).
