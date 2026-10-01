# QA round 6 — the pre-release gate, second attempt

**Reviewed:** 1 October 2026 · **By:** QA · **Verdict:** send back — **do not re-tag and publish v0.1.0**
**Against:** [`m4-timer.md`](../work-orders/m4-timer.md), [`round-5-fixes.md`](../work-orders/round-5-fixes.md),
[`k0-clappkit-update.md`](../work-orders/k0-clappkit-update.md), [`m12-shell.md`](../work-orders/m12-shell.md) ·
**At:** `main` @ `6eb2b7f` · **Previous:** [`round-5.md`](round-5.md)

Findings only. Nothing was fixed, merged, tagged or published. Clatch was `0.4.5-stage.23`.
`main` has moved one commit further since (`81ce965`, `p1-the-split.md`) — a docs-only work order
for the *next* phase, zero code. Not reviewed here; said so rather than silently ignored.

**Everything Clatch-shaped ran under an isolated `HOME` with its own daemon**, a `debug`-backend
agent for the durability and timer work and a `codex-acp`-backend agent (the only kind that will
hold an avatar) for the K0 check. None of your real agents, apps or data were touched. The isolated
daemon was stopped and every scratch home removed afterwards.

| | Verdict | The one sentence |
|---|---|---|
| 1 · Durability | **send back** | Every write verb survives `kill -9` the instant it is acknowledged — that part fully holds — but a write deferred behind an ambiguity is silently lost on **any** restart, graceful or not, and the CLI tells the agent it still finished. |
| 2 · K0 | **pass** | 14 commits, K1–K5, confirmed; the avatar bridge works end to end against a real agent's real photo; nothing else regressed. |
| 3 · M12 | **pass** | Eight pages, all keyboard-reachable, all honest; ⌘K restores the list correctly in every case I could construct; saved views never touch the wire. |
| Agent pass | **send back** | A full day completes — the round-5 fixes all hold live — but item 1 is reachable from `crm -h` alone, with no warning in the manual. |
| Person's pass | **pass** | All eight destinations, both passes already known from M7/M11, hold under the new shell. |

**The call you asked for: do not re-tag and publish.** Item 1 is silent, confirmed data loss, reachable by any agent that gets a wrong-sounding name back from an ambiguous record.

---

## Rule zero — `main` @ `6eb2b7f`

| Command | Result |
|---|---|
| `cargo test` | **319 passed, 0 failed, 0 ignored** |
| `cargo clippy --all-targets` | clean on our code (the same upstream `block v0.1.6` warning, every round) |
| `cargo fetch --locked` (ssh forced off) | exit 0 |
| `npx tsc --noEmit` | clean |
| `npm test` | **73 passed, 0 failed** |
| `npm run contrast` | **0 required pairings failing** |
| `npm run verify` | **green, 8/8** |

---

## 1. Durability — the fix holds for acknowledged writes, and breaks for deferred ones

**Method.** The real packaged binary, `kill -9` (no grace at all) timed against the process's own
acknowledgement, not a sleep. Write verbs tested: `add` (company/contact/deal), `set`, `log`, `task`,
`move`, `done`, `archive`, `link`, `import` (refusal path), and `select` resolving a parked question.

### What holds

```
for each verb: run it, read "added"/"updated"/"logged"/… (exit 0), kill -9 the app THAT INSTANT, relaunch
  add company / add contact / add deal / set / log / task / move   →  all present after relaunch
```

Every one of these survived immediately after its acknowledgement, every time. This is round 5's fix
working exactly as designed: the CLI's writes go through before the process is told the answer, and a
kill that lands after the answer printed cannot touch a write that already reached the disk.

### MAJOR — a write parked behind an ambiguity is lost on *any* restart, and `select` still says it worked

`src-tauri/src/state.rs:179-181` (the `resume` field, `#[serde(skip)]`) · `src-tauri/src/cli.rs:1121-1135`

```
$ crm log note acme "cross-restart test"
    More than one record matches — pick the one you meant to finish this:
      crm select 1  Acme Corp (company)
      crm select 2  Acme Industries (company)
    Picking one finishes it. `crm show <handle>` only opens a record — it will not.
$ crm close                                    ← graceful; no kill, no signal, nothing destructive
$ clatch run com.breksos.crm
$ crm select 1
    opened company acme-corp                   ← NOT "logged — acme-corp"; exit 0

$ crm show acme-corp
    company acme-corp — Acme Corp               ← no Timeline, no Note. It never happened.
```

I checked the store directly to rule out the CLI merely mis-describing a write that did land: the
note is **not** in `activities`. I reproduced this with `set` too (a domain value never written) and
with a hard `kill -9` in place of the graceful `crm close` — identical result either way, so this is
not a timing race at all, it is a shape the data cannot survive a restart in.

**Root cause.** `Pending.resume: Option<PendingResume>` — *which write `select` would complete* — is
explicitly `#[serde(skip)]`, with the reasoning written right beside it: *"does not survive a restart
(a short-lived question is an acceptable place for that to matter)."* But `Pending.resuming: bool` —
*whether answering this one completes a write* — is **not** skipped, and it is what drives the CLI's
own sentence (`cli.rs:1121`, `resuming = resp.pointer("/pending/resuming")…`). So across a restart the
prompt keeps confidently promising "picking one finishes it," `cmd_select` finds `resume` already gone
and falls back to a bare `show` (`state.rs`, the `None =>` arm of `cmd_select`'s match), and the answer
still comes back `Answer::Changed`/exit 0 — the one signal an agent has for "did my write go through"
says yes. Nothing anywhere — not the refusal vocabulary, not `crm -h`'s reminders/next-steps sections,
not the window — says a parked question can expire this way.

**Why it is not an edge case.** Architecture §6 calls `pending` a first-class piece of shared state, not
a transient one, and it is reachable from every write verb that resolves a reference (`move`, `set`,
`log`, `task`, `link`, `archive`, `done`) — `m2-cli.md` calls ambiguity *"the most interesting thing in
the app."* Any of the ordinary reasons a long-lived app restarts — an update, a crash, a person
quitting for the night — can land between an agent parking a question and answering it, and every one
of them currently erases the write without a trace.

**What I did not find:** the `resuming` flag itself does not become stale *reporting* — it is accurate
right up until the moment `resume` would be needed, which is the only moment it matters. I also did
not find this affecting a plain `show`/`open` ambiguity (`resume: None` from the start) — that one
never promised more than opening a record, and does not regress.

---

## 2. K0 — clappkit at `2cde169`, re-verified rather than taken on the commit's word

```
clappkit b5a3aab..2cde169: 14 commits (confirmed by count), K1 through K5 all present
```

- **The avatar bridge, end to end, against a real agent.** Created a `codex-acp` agent (the `debug`
  backend refuses `clatch agent avatar` — *"this agent cannot hold an avatar"*, worth knowing if you
  ever script tests against it), set a real photo, granted and ran the app. A temporary, uncommitted
  probe line confirmed the live roster carried `avatar: Some("/…/.clatch/engine/agents/<id>/avatar.png")`
  — the exact file Clatch stores, matching what the commit claims. Read `clappkit::asset::data_uri_allowed`
  directly: it canonicalizes both sides and refuses anything not in the published list, so the K1 claim
  ("cannot be turned into base64-any-file") holds by inspection, not only by the commit's say-so.
- **`CLATCH_INSTANCE_TOKEN` is read and never scrubbed** (`clappkit/src/control.rs:157`) — confirmed by
  reading the line; K2's scrub lives only in the reference `Client`, which we do not use. Carried
  forward from the commit's own disclosure, not new.
- **`standalone` is still a default feature** (`clappkit/Cargo.toml:24`, and our own `Cargo.toml` does not
  pass `default-features = false`) — so the dev hatch is compiled into the shipped binary, exactly as K0
  said. A release-pipeline decision, not a regression.
- `cargo test` 319, `clippy` clean, `cargo fetch --locked` clean, `npm run pack` + `clatch validate` all
  pass against this pin (see Rule zero) — nothing else broke.

**Verdict: pass.** Both of K0's own disclosed gaps are real and already named; neither is new, and the
one thing this update was risky for — the avatar bridge — works against a real file and a real agent.

---

## 3. M12 — the shell, against `m12-shell.md`

Driven live in the real component tree (`npm run preview`), all eight destinations: Home, Inbox,
Pipeline, Deals, Contacts & companies, Reports, Team, Settings.

| Check | Result |
|---|---|
| Six data pages keyboard-reachable, built from real snapshot data | **yes** — Home's tiles/bars/recent-moves, Inbox's actor filter and merged feed, Pipeline/Deals/Contacts unchanged in substance |
| Reports, Team tell the truth in one line and nothing else | **yes** — `<div class="page stub">` holding exactly one `<p class="stub-line">`, no spinner, no mock data, confirmed in the rendered DOM |
| Settings holds only the theme | **yes** |
| ⌘K searches companies, contacts, deals | **yes** |
| ⌘K restores the list it disturbed | **yes — see below** |
| Saved views per-seat, never in the snapshot | **yes** — `SavedView`/`BUILTIN_VIEWS` (`src/views.ts`) have no reference anywhere in `bridge.ts` or the snapshot type; stored under `breksos.savedViews.v1` |
| No id in rendered text or a printed command, any page | **yes** — scanned all eight pages' live DOM for anything ULID-shaped: zero, on every page |

### ⌘K, constructed every way I could

The read-with-a-side-effect `m12-snapshot-gaps.md` §4 names: typing moves the shared list to the
search's own `find`; ending the search is supposed to put back whatever the list held before.

- **Plain case**: search, choose a result → the Deals table returns to its original 5 rows *and* the
  record panel opens on the chosen deal, both correctly, confirmed from the live DOM.
- **A kind filter already active** (Contacts & companies, "People"): search, Escape → the filter chip
  stays pressed and the list returns to exactly its pre-search rows (verified identical before/after,
  not just "some rows").
- **Choosing a result does two `run()` calls back to back** (`end()`'s restore, then the `show`) — I
  specifically checked these do not race each other and silently clobber one or the other, because `find`
  and `show` touch different parts of the snapshot (`list` and `focus`) and both landed correctly.
- **An empty-result baseline**: in one case the pre-search list was itself empty (the "People" chip on a
  fixture whose mock contact rows did not populate — see the Minor below); after Escape it was empty
  again, which is still "restored to what it was," so this was not counted as a ⌘K failure.

**MINOR, probably harness-only.** Clicking "People" on Contacts & companies against the `Core: snapshot`
preview scenario showed zero rows, although the committed fixture's `list.rows` carries three contacts.
`crm find --kind contact` against the real backend filters correctly (confirmed live, see item 1's
environment) — the gap is specifically in `scenarios.ts`'s mock `recordsFrom`/`repage`, which is preview
machinery, not the window's real filtering logic or the real core. The same caution round 5 raised about
the harness applies: worth a look so a client-only quirk is not mistaken for a working feature, but not
counted as a page defect since the real backend path is unaffected.

---

## The agent pass — `crm -h` alone, source closed

A full day against a fresh store: add a company, a contact and a deal; log a call; two next steps; find,
open, page; an ambiguity via `log`, `set`, `task` and `archive`, each resolved; move through proposal to
won; archive, show while archived, restore; complete a next step, including refusing a second `done`;
export. **It completes, and every round-5 fix is live:**

```
$ crm set acme-corp domain ""
    crm: an empty value would erase domain — to do that on purpose, `crm set acme-corp domain --clear`
$ crm done pile-1          (already done)
    crm: "pile-1" is already done — `crm due` lists the open ones
$ crm find xyz --kind contact
    no results (page 1 of 0 total)
      filtered by query "xyz" — `crm find ""` clears the query
      filtered to contact — `crm find --kind all` searches everything
$ crm show nope-xyz
    crm: no record matches "nope-xyz" — try `crm find nope-xyz`
```

`crm -h`'s new reminders section states "a signal is sent, not confirmed" in the same words `crm status`
uses. The manual is otherwise unchanged from round 5 and still correct.

**Send back, for the reason above:** item 1 is reachable by an agent following the manual exactly as
written — park a question, and if the app happens to restart before answering it, `crm -h` gives no
hint that the answer it is about to get back is not telling the truth.

## The person's pass — terminal closed, all eight destinations

Already covered page by page in item 3. The M7 controls (new/edit/log/task/done/archive/move) and M11's
desk were not re-litigated line by line this round — nothing in K0, M4's follow-up or M12 touched them,
and round 5 already verified them live — but every one was exercised in passing while touring the eight
pages and none regressed: the desk panel rendered on every destination with its *Waiting on you* /
*Agents* / *Recent moves* blocks, including the new *Reminders* block reading `last sent` / `still open`
in the same words as `crm status`.

---

## What would change this verdict

1. **Either make `resume` survive a restart, or make `resuming` stop claiming it will finish the write
   once it cannot.** The second is the smaller change: if `resume` is `None` on a persisted `Pending`,
   `resuming` should read `false`, and the CLI's sentence and exit behaviour follow from that honestly —
   "this question only opens a record now; whatever it was going to log, set or move did not happen and
   cannot be recovered. Ask again." That is a worse experience than finishing the write, but it is true,
   which is the bar every other refusal in this app already clears.
2. Re-run the exact repro above (park via `log`, restart — graceful and `kill -9` both — `select`, check
   the store) as the regression test for whichever fix lands.

Nothing else in this round blocks a release.

---

## What this round did, and did not do

**Did:** rule zero on `main` at `6eb2b7f`; every write verb acknowledgement-then-`kill -9`'d against the
real packaged binary; the cross-restart pending-write loss reproduced four independent ways (graceful
close and hard kill, for both a `log` and a `set`) and confirmed against the raw store file each time;
K0's 14-commit count and K1/K2/K5 claims read in the actual clappkit source rather than trusted; the
avatar bridge proven against a real `codex-acp` agent's real photo file, via a temporary uncommitted probe
line, reverted immediately after; the M4 timer's once-ever and one-consolidated-signal guarantees
re-confirmed live against a real bound agent on this pin; all eight M12 pages driven live in the real
component tree; ⌘K's restore constructed under a plain case, an active filter, and an empty baseline;
saved views confirmed absent from every type the wire touches; all eight pages scanned for id leaks live.

**Did not:** fix, merge, tag or publish anything; touch `clappkit/`; drive the window as a real Tauri
webview (same standing limitation as every round); provoke a genuine platform-level `task.due` refusal
(the `debug` backend never fills an inbox; this rests on code review, as in round 5); re-litigate M7/M11
control-by-control, since nothing this round touched them and round 5 already did.
