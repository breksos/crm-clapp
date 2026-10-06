# QA round 7 — p1, the split

**Reviewed:** 6 October 2026 · **By:** QA · **Build:** `main` @ `fbd2142` (p1 steps 1–3, old shape deleted)
**Verdict:** **pass** — no MAJOR. Four MINORs and one thing I could not test.
**Previous:** [`round-6c.md`](round-6c.md)

Findings only. Nothing was fixed, merged, tagged or published. Everything ran under isolated `HOME`s
(`CLATCH_STANDALONE=1`, or a private Clatch daemon with a `debug` agent); none of your real agents, apps or data were
touched, and every scratch home, daemon and process is gone. The two scratch files I added to drive the real window
(`qa7.html`, `src/qa7.ts`) were deleted; the worktree is clean.

| Question | Verdict | The one sentence |
|---|---|---|
| The two counters never cross | **pass** | Reads move `rev` only, writes move both, refusals move neither, and a failing disk rolls the watermark back with the rows. |
| A delta never leaves the window stale | **pass** | More than 35 steps across both doors, the real window against the real core, every page compared with a fresh bootstrap: no difference. |
| Search, feed and tasks change nothing | **pass** | 60 calls, malformed ones included, with a parked question and a filtered list: state, view, file bytes and watermark identical. |
| Agent pass and person's pass | **pass** | Nothing regressed; refusals teach; the window and the CLI agree. |
| No id anywhere | **pass** | 0 ULIDs in the CLI's output, an export, or the window's text and attributes across 8 pages and three record panels. |
| Durability, 6b/6c | **pass** | Every verb, every parked-write `select`, kill at 0 s — rows and watermark match the disk. The three 6c fixes held. |

---

## Rule zero — `main` @ `fbd2142`

| Command | Result |
|---|---|
| `cargo test` | **406 passed, 0 failed** (from 343) |
| `cargo clippy --all-targets` | clean on our code (the same upstream `block v0.1.6` warning) |
| `cargo fetch --locked` | exit 0, ssh forced off |
| `npx tsc --noEmit` / `npm test` | clean / **101 passed, 0 failed** (from 74) |
| `npm run verify` | **green, 8/8** — "the two surfaces agree" |

---

## 1. The two counters

`rev` orders what a window draws; `watermark` orders what the data did. I recorded both on every kind of message
through the socket of a running app:

```
read: status / search / feed / tasks           rev +1 each           wm 29 → 29     delta 29→29, no ops
navigation: find, open (parks a question)      rev +1                wm unchanged
write: add company                             rev +1                wm 29 → 30     delta 29→30, 1 op
refused: bad stage, missing record             no update at all      wm unchanged
write: move (deal + stage-change feed item)    rev +2                wm 31 → 33     delta 31→33, 2 ops
```

**A failing disk** (`chmod 500` on the data directory): two writes were refused with `not saved`, each answering an
update with `from == to == 33` and no ops. `changes` afterwards held no trace of either ("Doomed" appears nowhere), the
next write took 34, and `changes since 0` returned seqs **1..34, contiguous, no gaps and no duplicates**. The number is
taken inside the commit and rolled back with the rows.

**A restart** (`kill -9`, relaunch): `rev` restarted (21 → 2); `watermark` survived (4 → 4); `changes since 0..3` answers
`stale`, `since 4` answers an empty list, and the next write takes 4 → 5. That is the contract's "the watermark persists,
the log does not", exactly.

**On disk**, after each of 20 writes and a `kill -9`: the watermark in the file, the one the live app reported and the
one the relaunched app reported were the same number every time — the pair really does reach the disk in one write.

## 2. A delta never leaves the window stale

I did this two ways, because the second one found what the first could not.

**a. The reducer, against the core.** A script drove 23 steps through the socket (`add` ×5, `move` by handle and by id,
`set` ×2, `log`, `task`, `link`, `done`, `archive` company and deal, restore, `move` to won and back, `import`, `show`,
`find`), folding each response into the window's *own* `applyUpdate` / `applyChanges` from `src/split.ts`, then
comparing the held rows, feed head, view and derived figures with a fresh bootstrap. **All equal at every step.** A
deliberately dropped update was recovered by the next one through `changes`, as the contract says it should be.

**b. The real window, against the real core.** I fed the unmodified React app a proxy to a live core, so every page is
drawn by the shipped code from real messages (not the preview harness's mock). After each write I read the whole page
text (header, sidebar, agent desk, page, record panel) of eight pages — Home, Inbox, Pipeline, Deals, Contacts &
companies, Reports, Team, Settings — and compared it with a second, freshly bootstrapped copy of the same window.

13 steps, with a deal record **open** in the panel throughout, written by the agent (attributed to it): move the open
deal, edit its value, log a call, set and finish a next step on it, edit its company, move another deal to lost,
**archive the open deal and restore it, archive its company and restore it**, import two contacts. Then 2 writes made
in the window itself (log form, drag-move). **No page differed after any step.** My first attempt at this was
invalid — it compared only `<main>`, which excludes the record panel and sidebar, and two scripts raced — so I threw it
out and redid it on `document.body` with one writer at a time. A control run, with the push withheld, did show Home,
Inbox and Pipeline stale, so the method can see staleness.

### MINOR — the window's row replica is kept and never read

`src/split.ts` `toSnapshot` (and `applyOp`): the window holds `data.companies / contacts / deals / tasks` and applies
every row action to it, but `toSnapshot` hands the components nothing from `data` except the pipeline. Every page is
drawn from the seat's per-message `view`, the `derived` figures, the feed head, and two `useRead` queries keyed on the
watermark. So "a delta never leaves the window stale" is true — and holds mostly because those are recomputed per
message, not because the row deltas are applied. Step 2a shows the replica *is* correct today; but nothing in the window
would notice if it were not, and the preview harness's replica (`previewSplit.ts`) is by its own comment "built from what
the world shows" with row actions "for deal moves only", so it cannot catch this either. The first surface to read
`data` is where an error would surface for the first time.

### MINOR — a push that never arrives is not noticed

With a push withheld (my control run), the window stayed stale — Home, Inbox, Pipeline — until some later message
arrived and its `delta.from` exposed the gap, at which point `changes` repaired it. There is no heartbeat. Within one
process, Tauri events do not get lost, so this is a note for p3 rather than a defect today.

## 3. Search, feed and tasks change nothing

With the seat in a busy state (a filtered list, a focused record, a **parked question**), I called `search`, `feed`,
`tasks`, `changes` and `bootstrap` **60 times** with ordinary and hostile input: empty query, 9 999 limit, a
non-string query, an unknown kind, `limit: 0`, `pageSize: 100000`, page 99, an unknown actor, emoji, `since` 999 999.
Before and after:

```
bootstrap (view + data + watermark)   identical
data file bytes                       identical        file rewritten (mtime moved): no       watermark 29 → 29
```

Bad input is refused in a teaching sentence (`“bogus” is not a kind — use company, contact or deal`, `q must be text`,
`changes needs the watermark you hold — `since``) and nothing else. In the window, typing into ⌘K sent exactly one
command, `search`; no `find`, so the shared list was never touched. Gap 4 is closed.

## 4. The agent pass, from `crm -h` alone

Under a real (isolated) Clatch, with a `debug` agent bound and granted:

- A day of verbs: `add` company, contact, deal; `log`; `task`; `move`; `find`; `show` on an ambiguous word (it asked,
  `select 1` answered); `due`; `board`; `set`; `link`; `done`; `archive`; `move … won`; `find -n 3`; `stages`;
  `export`; `status`. Every output readable and every one in the manual's words.
- Bad input teaches and uses the manual's codes: unknown verb, missing argument, unknown option, a bad date → **exit 2**
  with the usage line; an unknown stage, a missing record, `select 9`, `done nope` → **exit 1** with what to try
  (`“nowhere” is not a stage — use one of lead, qualified, …`, `this page has 4 results — pick 1 to 4`).
- **No id anywhere:** 0 ULID-shaped strings in the whole transcript or in the exported CSV.

**The two surfaces agree.** The window drew the agent's work as it happened — "Alpha moved Acme renewal to Proposal" in
Recent moves, the record the agent opened in the panel, the domain it set — and a record the person clicked in the
window became `looking at:` in `crm status`. The window's `focused` view and the CLI's status are one seat's one view.

### MINOR — `crm show <ambiguous word>` leaves the list's kind and its rows disagreeing

```
$ crm status            list: query (none set)   kind deal   sort updated   page 1 (2 total)
$ crm show a            More than one record matches …
$ crm status            list: query (none set)   kind deal   sort updated   page 1 (4 total)
```

The kind filter still says `deal` while the list now holds all four candidates (companies, a contact and deals). Not a
defect of the split — the seat's list is per seat by design, and an ambiguity lands its candidates there — but it is the
one place a lookup still rewrites the shared list, and the header it leaves behind is wrong.

## 5. The person's pass

Beyond the page comparisons above: a click on a row opens the record and the agent sees it; the drag-move, the stage
`select` and the log form all send one command each. The three 6c refusal cases — a refused drag, a refused "Move to",
a refused log form — each show the core's sentence (`not saved — the write did not reach the disk …`) under the split;
none is dropped. (Run with the response replaced by the core's refusal shape.) The "send it again" control is still in
the tree (`Desk.tsx` `UnconfirmedSends`, sending `resend`).

## 6. Durability, 6b/6c — still holds

On the split build: every write verb (`add` ×3, `set`, `set --clear`, `log`, `task`, `done`, `move` ×3, `link`,
`archive`, `archive --restore`, `import`), and every parkable write resumed with `select` (`log`, `set`, `task`,
`archive`, `archive --restore`), each followed by `kill -9` and a relaunch: **rows identical, and the watermark in the
file, in the live app and in the relaunched app the same number, every time.** Three writes each followed immediately
by `kill -9` were all there after relaunch. The refused-disk case is in §1. The three 6c fixes held: a question promising
a write the file cannot keep is dropped on open (`2ac4135`), refusals reach the person (§5), and send-again exists.

The watermark also commits atomically with the rows by construction: one file, one rename. That is the storage decision
(`split-contract.md` §8) and it is sound for p1; its ceiling — every commit rewrites the whole file, plus now a clone
of the dataset for the diff — is the one I measured in 6c and is a p3 problem, as the contract says.

---

## What I could not test, and what is by construction

- **"View state is per seat and never crosses."** There is one seat. Nothing in this build can put two seats on one
  org, so I could confirm only that the person's window and their agent share one view, not that another seat's does not.
  By construction, `view` is built from the local state and never travels; I have no run that would catch a mistake.
- **A reminder through the split.** Agent-made tasks that are already due are born "told" and silent (as before), so I
  could not get the sweep to mark one inside the window; the unit tests cover the mark as a row action and `resend`.
  I did not run a live sweep → `sentAt` → resend round trip.
- **The window as a real webview.** As in earlier rounds, the window was run in a browser against the real core, not in
  the Tauri webview. That means the window's own `run_cmd` was replaced by a proxy to the socket, where `show` is
  claimed by clappkit as the window verb; I had to translate it to `open` in the proxy. Not a defect today (the app's
  own `run_cmd` bypasses that relay), but the day a window talks to a core over the same relay as the CLI, `show` will
  answer "window shown" and navigate nowhere.
- One isolated app exited once, unexplained, partway through (the daemon logged `control pipe closed`) at about the time
  the host app was quit; I relaunched it and did not reproduce it.

## What would change my verdict

Nothing here blocks the gate. In the order I would take them:

1. Make the replica earn its place or remove it: read `data` somewhere real (Team, a tasks page), or drop the row ops
   until p2/p3 need them, so that a wrong op is a visible bug rather than a silent one.
2. Reset the list's `kind` when `show` fills the list with candidates of several kinds, or keep the candidates out of
   the list.
3. Before p2: give the window a way to notice a missed push (a periodic `changes since` is enough), and rename the
   window's `show` envelope, since it is already a clappkit verb on every other path.

## What this round did, and did not do

**Did:** rule zero on `fbd2142`; a 23-step reducer-vs-fresh-bootstrap comparison; a 15-step comparison of every page of
the real window against the real core, including a control that proves it can see staleness; the counters across reads,
navigation, writes, refusals, a failing disk and a restart; 60 hostile read calls with before/after fingerprints; an
agent pass under a real isolated Clatch; an id scan of the window's text and attributes and of the CLI; the durability
sweep with watermark checks.

**Did not:** fix, merge, tag or publish anything; test two seats; run a live reminder sweep through the window; run the
window in the Tauri webview; test a real `clatch stop` at 0 s on the split (the `kill -9` results are the stricter case).
