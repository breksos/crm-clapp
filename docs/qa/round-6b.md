# QA round 6b — the parked-write fix, and the family it belongs to

**Reviewed:** 3 October 2026 · **By:** QA · **Branch:** `r6-fix` @ `142267f` (one commit on `main` @ `165067a`)
**Verdict on the fix:** **pass** · **Verdict on the category:** **send back** — three more acknowledged-but-not-durable writes
**Previous:** [`round-6.md`](round-6.md)

Findings only. Nothing was fixed, merged, tagged or published. Everything ran under isolated `HOME`s with
`CLATCH_STANDALONE=1` or an isolated Clatch daemon; none of your real agents, apps or data were touched, and all
scratch homes and processes are gone.

| | Verdict | The one sentence |
|---|---|---|
| The fix | **pass** | Every verb that can park, with a graceful close and with `kill -9`, now lands its write on `select` — and a question that cannot be completed exits 1, says so, and never turns into a navigation. |
| The category | **send back** | `import` says "imported 2" and loses them, even on a graceful close; a failed disk write is acknowledged with exit 0; and every write that goes through the window's door is still lost if the app is stopped within 400 ms. |

---

## Rule zero — `r6-fix` @ `142267f`

| Command | Result |
|---|---|
| `cargo test` | **325 passed, 0 failed** (from 319; six regression tests) |
| `cargo clippy --all-targets` | clean on our code (the same upstream `block v0.1.6` warning) |
| `npx tsc --noEmit` / `npm test` | clean / **73 passed, 0 failed** |
| `npm run verify` | **green, 8/8** |

Golden fixtures are unchanged, as the commit says: `git diff origin/main..HEAD` touches `cli.rs`, `main.rs`, `state.rs` only.

---

## 1. The fix — my round-6 reproduction, rerun for every verb that can park

The verbs that can park are the ones that resolve a reference through `resolve_write_target`: `move`, `set`, `log`,
`task`, `link` (two slots), `archive`. (`done` resolves a task handle, `add --company` resolves decisively; neither parks.)
Method: park the question through the CLI, restart the app — **graceful `crm close`** or **`pkill -9`** — then
`crm select`, then read the store file directly, so a mis-worded confirmation cannot hide a missing write.

| Verb | Graceful close | `kill -9` | What landed |
|---|---|---|---|
| `log` | `logged — acme-corp`, exit 0 | same | the note is in `activities` |
| `set` | `updated — acme-corp`, exit 0 | same | `domain` is on the company |
| `task` | `task set — acme-corp`, exit 0 | same | the step is in `tasks` |
| `archive` | `archived — acme-corp`, exit 0 | same | `archivedAt` is set |
| `move` (pick the deal) | `moved acme-renewal to proposal`, exit 0 | same | stage is `proposal` |
| `link` first slot ambiguous (`link acme ada`) | `linked — acme-renewal`, exit 0 | same | the contact is on the deal |
| `link` **both** slots ambiguous, a restart between each answer | `linked — acme-corp`, exit 0 | same | the deal's company is set |

The two-slot case is the one I most expected to break: the second park is created *after* the first answer, so its
resume carries a half-resolved request, and it had to survive a second restart. It did, both ways.

**Refusals that are not restart problems, and are truthful.** Picking a company for `move`, or a company for the first
slot of `link`, exits 1 — `only a deal can be moved, and that is a company` / `link needs a deal and a contact or a
company` — and nothing is written. (My first run picked the wrong candidate for these; the app was right.)

### The "cannot be completed" path, against a genuine old-build file

I did not hand-edit a file. I downloaded the **`v0.1.0` draft binary** — a build that has the `resuming` flag and
nothing else — parked a `log` ambiguity with it, let it save, then opened that same data file with the `r6-fix` build:

```
old build file:  pending keys = [prompt, candidates, resuming]   resuming = true     (no `resume`)
$ crm select 1          crm: this question's write was lost — run it again      exit 1
$ crm select 1          crm: this question's write was lost — run it again      exit 1      ← twice
$ crm select 2          crm: this question's write was lost — run it again      exit 1
$ crm status            looking at: company acme-industries                       ← unchanged: no fall-through to "open"
$ crm log note acme "re-run"   → parks a fresh question
$ crm select 1          logged — acme-corp      exit 0
```

Exactly what was asked: exit 1, says so, retries refuse too, and running the write again works. The note on disk
afterwards is the re-run's, and only that.

**One small gap, not blocking:** the *window* does not know about `pending.lost`. `PendingBanner` (`src/Panels.tsx:16`)
still draws the old prompt with clickable candidates, and its picks go through `run`, which drops refusals
(`bridge.ts` — the M7 finding). So for a question inherited from an old-build file, the banner invites a click that
does nothing and says nothing. It can only happen on a data file written before this fix, so it is Minor.

**Verdict on the fix: pass.**

---

## 2. The category — anything that acknowledges a write that is not durable

I went through every way a state change can be answered and asked, for each: what does the person or agent hear, and
is it on disk when they hear it? Three more members, and one hazard I could not prove.

### MAJOR — `crm import` says "imported 2" and the records are not saved

`src-tauri/src/state.rs:2624` (`cmd_import` returns `Answer::ReadWith`) → `state.rs:2012` (`ReadWith` ⇒ `dirty = false`)

```
A) crm import c.csv --kind contacts      →  imported 2     (status: contacts 2)
   kill -9, relaunch                      →  contacts 0
B) crm import …, then crm add company     →  after kill -9 and relaunch: contacts 2     ← saved only by accident
C) crm import …, then crm close (graceful) →  after relaunch: contacts 0
```

`ReadWith` is documented as *"Read-only … rows which are this one request's answer and not shared state anybody
persists"*, and `import` was filed under it because it returns a result block. But it **creates records**. Nothing marks
the dataset dirty, so nothing is written — not on the CLI's write-through, not on the exit flush. Case B is the telling
one: the next unrelated write carries the whole dataset to disk, so the records appear to survive exactly when a
second, unrelated command happens to run. This is the most confusing form of the bug — it passes any test that does
anything after the import. It is a **bulk** operation, so the loss is the largest this family has produced.

### MAJOR — a write the disk refuses is acknowledged with exit 0

`src-tauri/src/main.rs:115-118` (the CLI door) · the same shape at `main.rs:119` for the window door

```
$ crm add company "Before"        →  added company before        (on disk)
$ chmod 500 ~/.crm                ← what a full or failing disk looks like to the writer
$ crm add company "Doomed One"    →  added company doomed-one    exit 0
$ crm task doomed-one "…" --due … →  task set                    exit 0
$ crm find ""                     →  both, as if saved           (the in-memory state)
   app's own log:  crm: save failed: cannot write …/crm.json: Permission denied (os error 13)
                   crm: could not confirm a write reached the disk
   kill, relaunch                  →  on disk: ['before']
```

Round 5's order was explicit — *an acknowledgement is true when it is given* — and `write_through` correctly returns
`false` when the write fails. But the only thing done with that `false` is a `writeln!` to **the app's** stderr, which is
the app's log (under Clatch, `clatchd.log`), not the CLI's. The agent that asked gets the success reply and exit 0. The
reminder path handles the same condition correctly (`main.rs:170`: withhold the signal, `unsend()`), which is what makes
this one visible: the pattern for doing it right exists, one function away. A disk-full or permissions failure is rare,
but it is exactly the case where the answer matters most, and the agent will build on "added" for the rest of the session.

### MAJOR — the window's door is still debounced, so its writes are lost if the app is stopped within 400 ms

`src-tauri/src/main.rs:119` (`self.saves.save(db)` for `Via::Window`) · `main.rs:34` (`SAVE_QUIET` = 400 ms)

The CLI door was made write-through; the window door was deliberately left debounced "and made durable by the exit
flush". But round 5's own measurement was that `clatch stop` runs **no** exit hook (the process is gone in ~30 ms), and a
crash, a `kill -9` or a power cut run none either. So any edit made in the window — which is shown as done the instant
the snapshot comes back — is lost if any of those lands in the next 400 ms.

I could not drive the real window, so I measured the window's door the only honest way available: a scratch build,
**source reverted afterwards**, with the CLI's socket handler switched from `Via::Cli` to `Via::Window` and nothing else
changed. Every `crm add` below was acknowledged.

```
kill -9 after 0 s      →  gone          real `clatch stop`, 0 s      →  gone
kill -9 after 0.1 s    →  gone          real `clatch stop`, 0.15 s   →  gone
kill -9 after 0.25 s   →  gone          real `clatch stop`, 0.3 s    →  gone
kill -9 after 0.6 s    →  saved
```

So this is a stand-in for the window path, not a window run, and I say so plainly. What it establishes is that the
production writer, on that door, behaves this way under the real launcher's stop. The tests round 5 added pin the
*graceful* exit (`RunEvent::Exit`), which does work, and cannot see this. A window write is discrete (an add, a log, a
done), not a drag, so the debounce's reason for existing does not apply to most of them; the drag is the one genuinely
bursty gesture.

### MINOR — a hazard I could not prove: concurrent writes can regress the file

`main.rs:98-102` takes the dataset snapshot inside the state lock, but the slot it is written through is assigned after
the lock is released (`store.rs`, `write_through` → `latest = Some(db)`). Two commands that interleave in that gap can
leave the older snapshot as the last one written. **I could not make it happen**: five trials of 40 parallel `crm add`
and three of 150, each acknowledged in full, each with every record on disk after `kill -9`. I am recording it because
the code permits it and the family so far has been "latent until someone is fast enough", not because I observed it.

### MINOR

- **`crm export` is not atomic or synced** (`cli.rs:1399`, `std::fs::write`). Its failures *are* reported correctly — I
  tried a missing directory, `/dev/full` and `/System`, each exit 1 with the OS reason. But a kill mid-export can leave a
  truncated file next to a "wrote N rows" the caller already read.
- **`show`/`find` change persisted view state (`focus`, the list) and are never dirty**, so "looking at" resets on
  restart. Not an acknowledged write, so not counted — noted because it is the same mechanism as the import bug.
- **`crm <verb> | head -0` panics** (`failed printing to stdout: Broken pipe`). Only the degenerate case; ordinary
  `| head -1` is fine. Mentioned because agents pipe.

---

## What would change the category verdict

1. **Mark `import` as a change** (`Answer::ChangedWith("import", …)`), and add the regression test that imports then
   kills — the shape of every test that has caught this family.
2. **Return the write-through result to the caller.** If `write_through` is `false`, answer with a refusal (exit 1, "not
   saved — the disk refused it"), as the reminder path already does. For the window door, surface it as an `ErrorLine`.
3. **Decide the window door on purpose.** Either write through for discrete edits and keep the debounce only for drags,
   or state that the window's acknowledgement is best-effort and say so where the person can see it. Today it is the
   second, silently.
4. **Pin the family with one test that cannot be forgotten:** for *every* envelope the dispatcher accepts, run it, kill
   without flushing, reload, and assert the effect — failing the build when a new verb is added without one.

## What this round did, and did not do

**Did:** rule zero on `r6-fix`; the parked-write reproduction for every parking verb, graceful and `kill -9`, with the
store read directly; a genuine old-build parked question from the `v0.1.0` binary; and a category sweep — durability of
every write verb, `import`, `export`, a failing disk, 190 parallel acknowledged writes, and the window's door under
`kill -9` and a real `clatch stop`.

**Did not:** fix, merge, tag or publish anything; drive the window as a real webview (the window-door result is a
door-swap stand-in, labelled as such); reproduce the concurrent-write regression.

**A correction to my own run.** I first ran the import probe on the scratch binary I had built for the window-door test,
so its early results were confounded by the debounce. I rebuilt the unpatched `r6-fix` binary and reran all three import
cases; the table above is from that clean run.
