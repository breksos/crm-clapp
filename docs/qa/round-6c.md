# QA round 6c — one commit point, rechecked from scratch

**Reviewed:** 4 October 2026 · **By:** QA · **Build:** `main` @ `a53bab5`
**Verdict on the three 6b MAJORs:** **pass** · **Verdict on the category:** **pass on the agent's door, send back on the person's** — two MAJORs, both on the window side
**Previous:** [`round-6b.md`](round-6b.md)

Findings only. Nothing was fixed, merged, tagged or published. Everything ran under isolated `HOME`s, either
`CLATCH_STANDALONE=1` or a private Clatch daemon; none of your real agents, apps or data were touched. Scratch source
edits were made in a scratch worktree and reverted (`git status` clean afterwards). All scratch homes, daemons and
processes are gone.

| | Verdict | The one sentence |
|---|---|---|
| 6b MAJOR 1 — `import` | **pass** | Imported records survive `kill -9` and a graceful close. |
| 6b MAJOR 2 — a refused write | **pass** | The CLI exits 1 with `not saved`, and the in-memory state is rolled back to what the disk holds. |
| 6b MAJOR 3 — window door within 400 ms | **pass** | The same writes survive a kill at 0, 0.1 and 0.25 s. |
| Category, the agent's door | **pass** | Every write verb, and every parked write resumed with `select`, matches the disk after `kill -9`. |
| Category, the person's door | **send back** | A refused write is still invisible on three window controls, and the only recovery for a reminder lost to a muted agent is a window control that does not exist. |
| The sweep test (item 3) | **partly holds** | It catches a verb added the ordinary way; it can be walked around by formatting. The real protection is structural, which is better. |

---

## Rule zero — `main` @ `a53bab5`

| Command | Result |
|---|---|
| `cargo test` | **341 passed, 0 failed** (from 325 at 6b) |
| `cargo clippy --all-targets` | clean on our code (the same upstream `block v0.1.6` warning) |
| `cargo fetch --locked` | exit 0 |
| `npx tsc --noEmit` / `npm test` | clean / **74 passed, 0 failed** |
| `npm run verify` | **green, 8/8** — "the two surfaces agree" |

---

## 1. The three 6b MAJORs, rerun the way I ran them

Same method as 6b: an isolated standalone app, the CLI driven from `crm -h`, the store file read directly, the app
killed with `kill -9` or closed with `crm close`.

| 6b finding | Reproduction | 6b result | Now |
|---|---|---|---|
| `import` acknowledged, not saved | `crm import c.csv --kind contacts`, then kill -9 / graceful close, relaunch | `contacts 0` both ways | **contacts 2** both ways. The "saved by accident" shape (an unrelated write afterwards) no longer matters, since nothing depends on it. |
| A refused write acknowledged with exit 0 | `chmod 500 ~/.crm`, then `crm add company …` and `crm task …` | exit 0, both in memory, neither on disk | **exit 1**, `not saved`; `crm find ""` shows only what the disk holds, so the refusal left no phantom records in memory. |
| Window door lost inside 400 ms | door swapped to `Via::Window` in a scratch build | gone at 0 / 0.1 / 0.25 s | The `Via` distinction no longer exists, so there is nothing to swap. A kill at 0, 0.1 and 0.25 s after the acknowledgement leaves every write on disk. |

I did not drive the real window as a webview in this round either; the window door is now the same function as the
CLI's door, which is why the result above is meaningful for it, and I say so rather than claim a window run.

---

## 2. The category, from scratch

### 2a. Every verb, memory against disk

For each verb: perform it, export the whole dataset from the running app, `kill -9`, relaunch, export again, compare.
All identical.

`add company` · `add contact` (with `--company`) · `add deal` · `set` (value, then `--clear`) · `log` · `task` · `done` ·
`move` (to proposal, won, back to lead) · `link` · `archive` · `archive --restore` · `import`.

### 2b. A parked write resumed with `select`

Park each parkable verb on an ambiguous reference, `select 1`, then compare as above:
`log` → `logged — acme-corp`, `set` → `updated — acme-corp`, `task` → `task set — acme-corp`,
`archive` → `archived — acme-corp`, `archive --restore` → `restored — acme-corp`. Each landed, each survived the kill.

### 2c. "Is anything different?" — can the comparison miss a real change?

`Db::change_from` (`model.rs:1058`) decides Data / View / None by comparing the pre-image with the post-image. I read it
against `Db`'s field list rather than trusting it:

- **Every field is covered today.** `companies`, `contacts`, `deals`, `activities`, `tasks`, `pipelines`, `reminders`
  and `view.pending` compare as Data; the rest of `view` compares as View. All the types derive `PartialEq`; none has a
  hand-written one that could ignore a field. `Task.due_sent_at` is on the task, so a reminder mark is a Data change.
  I found no way, with today's `Db`, to change something persisted and be told `None`.
- **MINOR — it is a hand-written list.** The eight comparisons are `&&`-ed by name. A ninth `Db` field added later is
  compared by nobody, silently, and the verbs that change only that field would be acknowledged unsaved: a false
  "unchanged" built in by omission. Destructuring `let Db { companies, … } = self;` makes the compiler refuse the new
  field. Nothing pins the list today.
- **Cost, not correctness:** the pre-image is a full clone of the dataset per command. That is the O(n) that shows up
  in the scale numbers below.

### 2d. The other doors

- **Reminder marks.** The write-ahead mark goes through `commit`; on a refused save the sweep calls `unsend()` and sends
  nothing, so a reminder is never announced that the disk does not know was sent. Unchanged from 6b, and consistent.
- **Export** (isolated, standalone). Writes to a staged file, syncs, renames. A missing directory, `/dev/full`, a
  read-only directory and a directory given as the file each exit 1 with the OS reason; an existing file at the target
  is left intact when the write fails (it still read `KEEP`); no temp file is left behind. Nothing in this door
  acknowledges a write that is not there. One note, not a finding: exporting to a path that is a symlink replaces the
  link with a regular file rather than writing through it — correct for rename-based atomic writes, but unexpected to
  anyone who points an export at a link.
- **View state** (`show`, `find`, focus, board filter) stays on the debounced writer by design and never refuses. A kill
  in the next 400 ms resets "looking at". Not a write acknowledgement; noted again only because it is the other half of
  `change_from`.

### MAJOR — a question inherited from an old-build file is forgotten after any unrelated write

`src-tauri/src/state.rs:163-213` (`Pending.promised`, `skip_serializing`, `lost()`)

The 6b fix handles a question that arrives in a file from an old build: `select` exits 1, "this question's write was
lost — run it again". I rechecked that with the `v0.1.0` draft binary and it still holds. But the "lost" knowledge is
held only in `promised`, which is read-only and never written back. Sequence, against a genuine old-build file:

```
open an old-build file with a parked question    →  select 1: "…write was lost — run it again"   exit 1   ✓
crm add company Anything                          →  (any ordinary Data write)
restart the app
crm select 1                                      →  "opened …"   exit 0                          ✗
```

The first unrelated write rewrites the file with the question but without the flag, so after the next launch the
question is an ordinary one with no `resume` and no memory that its write was promised. `select` then falls through to
its navigation meaning and says it opened the record — the same false acknowledgement 6b closed, reached by one more
step. It is only reachable from a data file written by the pre-fix draft build, which has not been released, so the
audience is whoever ran that draft. I rank it MAJOR because the fix was pronounced complete and the family's whole
history is "fixed, until one more step"; the cheap cure is to persist the lost state (or drop a resume-less question
when migrating, so it can never be mistaken for a live one).

---

## 3. Does the sweep test hold?

The claim: a test over every write verb fails the build when a new verb is added without protection. In a scratch
worktree of `a53bab5`, I inserted a new `dispatch` arm that clears `companies` and answers `Answer::Read` — the shape of
the original `import` bug — and ran `cargo test`. Reverted after each run.

| Scratch change | Result |
|---|---|
| New arm `"zap" => …` on one line, **not** added to `ENVELOPES` | **fails** — `the_list_of_envelopes_is_exactly_what_dispatch_accepts` |
| Same arm, added to `ENVELOPES` but to neither `the_writes()` nor `NAVIGATION` | **fails** — `every_envelope_is_either_swept_as_a_write_or_declared_navigation`, naming `zap` |
| Same, declared in `NAVIGATION` | **fails** — `navigation_never_changes_the_data`, since it does change the data |
| Same arm but the head is split over two lines: `"zap"` ⏎ `=> { … }`, nothing else touched | **passes — 341 / 341** |

The first three are what was promised and they work. The fourth is the weakness: `dispatch_arms()`
(`main.rs:1255`) finds arms by reading `state.rs` as text and accepting only a line that starts with `"` and contains
`=>`. A verb whose arm is formatted differently — a guard, a head wrapped by the formatter, a pattern that does not open
with a quote — is not seen, so it is neither in `ENVELOPES` nor swept, and nothing fails. (The first scratch run also
produced two extra, unexplained failures at `main.rs:943`; I did not chase them and note them only because I
could not explain them.)

What softens this: the protection itself is structural, not the sweep. Every `dispatch` answer goes through
`Core::commit`, which decides persistence by comparing the data, not by what shape the answer has, so an unlisted verb
that changes data is **still saved** — I read it that way and did not run the unlisted verb through a kill, so that
part is by code, not by test. The sweep is the second line of defence, and it fails open on formatting. The thing it
cannot see at all is a mutation made *outside* `dispatch` — a new Tauri command touching the state directly — which no
test of `dispatch` could.

---

## 4. The two items the backend left open — both confirmed

### MAJOR — when a write is refused, the window drops the error text on three controls

`src/bridge.ts:320-345` (`write`) vs `run` · `src/Board.tsx:133` · `src/Record.tsx:390` · `src/Panels.tsx:43`

The window has two ways to send a command. `write` (through `useWrite`) hands a refusal back to the control that asked,
and the control shows it. `run` is clappkit's fire-and-forget, whose `apply` returns early on `ok: false`. Three
controls that **change data** still go through `run`: the drag-and-drop move, the "Move to" select on a deal, and a
candidate click in the parked-question banner (which resumes a write).

Run in the preview harness with `window.__TAURI_INTERNALS__.invoke` wrapped to answer `move`/`select`/`log` with the
exact shape the core now sends, `{ ok: false, error: "not saved — … nothing changed" }`:

```
banner candidate click   →  select sent;  no text anywhere; the banner stays as it was
drag a card to Proposal  →  move sent;    no text anywhere
Move to ▾ Won            →  move sent;    no text anywhere; the select snaps back to "lead"
log form, same refusal   →  log sent;     role=alert "not saved — the disk refused it; nothing changed"   ← shown
```

So the person sees an error from a form and nothing at all from the three gestures they use most. In 6b I asked for
exactly this ("for the window door, surface it as an `ErrorLine`"); the backend's half is done and the reply carries the
sentence, but the window never reads it on these paths. The harm is smaller than a lost write — nothing is lost, the
state is rolled back — but it is a refusal the person is never told about, and a drag that silently does nothing is the
kind of defect that gets reported as "the board is flaky".

### MAJOR — reminders lost to a muted agent have no recovery that exists

`src/Desk.tsx:184` (`UnconfirmedSends` returns `null`) · `src-tauri/src/cli.rs:1006` · `src/state.rs` (`cmd_resend`)

Isolated Clatch, a real `debug` agent bound to the app, **muted**, an overdue task, then restart so the launch sweep
fires:

```
$ crm status
  reminder sent 14:53 — the platform does not confirm delivery (1 next step, 1 still open)
    Whether the agent acted on it is not something the app can see; the person can send it again from the window.
daemon log:  clatchd: signal task.due from com.breksos.crm [Run] -> no bind passes the cut matrix (dropped)
```

The loss is real and the app is honest that it cannot see it — that part is as designed and documented in `crm -h`. The
problem is the sentence after the dash. The window has no such control: `UnconfirmedSends` is a stub that renders
nothing, with a comment saying it waits on "backend round-5 §2", which the core has long since implemented
(`{cmd: "resend"}`, `ENVELOPES`). I then **unmuted** the agent and relaunched: no second signal was sent, the daemon
log still shows the one dropped signal, and `crm status` is unchanged. A reminder fires once, ever, so nothing re-sends
it. The agent's own `crm due` lists the task, so the work is not hidden — but the *nudge* is gone, the person is told
they can fix it, and they cannot.

This breaks a rule in `CLAUDE.md` in both directions: "every verb has a control" (the `resend` envelope has none) and a
CLI hint that points at a control that is not there ("a CLI hint instead of a control is a defect"). It is a gap the
platform makes unavoidable to *detect* and the app makes impossible to *recover from*.

### MINOR — scale: the single commit point has a ceiling, and the way it fails is silent-ish

I ran a deliberately oversized dataset to find where the one commit point strains. Not a realistic CRM, but the
numbers say where the edge is.

- **Write latency approaches the 800 ms `FLUSH_WAIT` ceiling at roughly 62 MB (~190 000 records).** Each command clones
  the dataset and rewrites the file with the state lock held, so it grows with the dataset. Past the ceiling,
  `write_through` gives up and the write is reported `not saved` and rolled back.
- **A write can be refused that actually completes** (`src-tauri/src/store.rs:276-285`, code read, not reproduced).
  `write_through` returns `false` on timeout while the writer may still finish the write. The caller then rolls memory
  back to the pre-image; if the late write lands, disk holds the change and memory does not — the opposite divergence to
  the one 6b closed. Needs a very slow disk to occur.
- **`crm import` of a large file** hits the 1 MiB control-pipe frame limit, and the refusal does not tell the caller to
  split the file. At ~65 MB the app became unresponsive while a long import held the state lock.

---

## What would change my verdict

1. **Window:** send the three `run`-based writes (drag, "Move to", banner candidates) through `useWrite`/`write`, or give
   `run` a refusal path, so `not saved` reaches a visible `ErrorLine`. Add a window test per control with an
   `{ok:false}` reply.
2. **Reminders:** build `UnconfirmedSends` — the sent-and-open list and a "send again" control calling `resend` — or
   remove the sentence that promises it from `crm status`. The first is the one `CLAUDE.md` asks for.
3. **Migration:** persist "lost", or discard a resume-less parked question when loading, so it cannot become a
   navigation after one write.
4. **Hardening, not blocking:** destructure `Db` in `change_from`; make `dispatch_arms` stop depending on line shape
   (match on the enum or table that `dispatch` itself uses); say "split the file" when a frame is refused.

## For the release decision

The thing I was asked to judge — an acknowledged write that is not durable — is **closed on the agent's door**: all three
6b MAJORs, every write verb, every parked-write resume and the reminder marks behave. If v0.1.0 is gated on that alone,
it passes. I would not call the category clean while a person's drag can be refused with no message and a promised
recovery for lost reminders has no button; those two are small, front-end-sized changes. The migration item only
affects whoever ran the unreleased draft build. The PM's call is whether items 1 and 2 above block the tag or ship as
known issues in the notes.

## What this round did, and did not do

**Did:** rule zero on `a53bab5`; the three 6b reproductions; every write verb and every parked-write resume against the
disk after `kill -9`; a read of `change_from` against `Db`; scratch experiments on the sweep test (all reverted);
export edge cases; a muted-agent run in a private Clatch; the window's refusal handling in the preview harness; a
scale probe.

**Did not:** fix, merge, tag or publish anything; drive the real window as a webview (the window findings are from the
preview harness with the core's real refusal shape injected); run an unlisted verb through a kill to prove the commit
point saves it (code read only); reproduce the timeout false-negative in `write_through`; test a real `clatch stop` at
0 s on the single door (the `kill -9` results are the stricter case); investigate the two extra failures at
`main.rs:943` seen once under a scratch change.
