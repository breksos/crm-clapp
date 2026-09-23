# QA round 4 — the agent pass and the person's pass

**Reviewed:** 23 September 2026 · **By:** QA · **Verdict:** send back
**Against:** [`m2-cli.md`](../work-orders/m2-cli.md), [`m7-window-editing.md`](../work-orders/m7-window-editing.md), [`m8-visual.md`](../work-orders/m8-visual.md) · **At:** `main` @ `1ec2742` · **Previous:** [`round-3.md`](round-3.md)

Findings only. Nothing was fixed, nothing was merged. Everything is on `main`; there was one
tree to review, not several. The agent pass — finally runnable — is the point of this round,
so it comes first.

| | Verdict | The one sentence |
|---|---|---|
| The agent pass | **1 Major** | A full day's work completes end to end, refuses well, and prints no id — except that `crm done`'s own refusal names a recovery path, `crm show`, that does not carry what it points to. |
| The person's pass | **pass** | All eight controls work end to end, keyboard-only, with real refusal-and-restore — the `Link…` gap and one contrast token are self-disclosed, not hidden. |
| Signals & identical state | **pass** | One core function, shared by both wire shapes; independently seeded states prove byte-identical results and human-only signalling. |
| M8 tokens & mono discipline | **pass** | Every measurement re-derived from the actual hex values matches the commit's claims exactly, including a self-flagged one. |
| The carried-in finding | **CONFIRMED**, root cause found | `crm select`'s confirmation is one function that never looks at what it resumed. |

---

## Rule zero

| Command | Result |
|---|---|
| `cargo test` | **208 passed, 0 failed, 0 ignored** |
| `cargo clippy --all-targets` | clean on our code (the one upstream `block v0.1.6` warning persists across every round) |
| `cargo fetch --locked` (no ssh key) | exit 0 |
| `npx tsc --noEmit` | clean |
| `npm test` | **25 passed, 0 failed** |
| `npm run verify` | **green, 8/8** — `crm -h` now names all 16 agent verbs plus `focus`/`close` |

---

## The agent pass — `crm -h` alone, source closed

A full day, against a fresh isolated store, driving the packaged binary exactly as an agent
would: read `-h`, then `<verb> -h` for anything about to be typed for the first time.

**Completed without having to guess anything, except one item below:**

```
add company "Acme Corp" --domain acme.com          → added company acme-corp
add contact "Ada Whitlock" --company acme-corp …   → added contact ada-whitlock
add deal "Acme renewal" --company acme-corp …      → added deal acme-renewal
log call acme-renewal "…"                          → logged
task acme-renewal "Send the signed order form" …   → task set
find / find acme                                    → the shared list, handles only
add company "Acme Industries"
log note acme "…"                                   → parked: 3 candidates, exit 0
select 1                                             → resolved against Acme Corp
set acme domain … (same "acme" ambiguity)            → resolved the same way
move acme-renewal proposal → move acme-renewal won   → Stage: Proposal, Status: Won
board                                                → totals correct per column
archive ada-whitlock → find --kind contact (0) → --restore → find (1)
show ada-whitlock (while archived)                   → loads, marked "(archived)"
move acme-renewal not-a-real-stage                   → names the six real stages, exit 1
move nope proposal                                   → "no record matches", exit 1
export deals / export deals --format json --out …    → real files, handles only, no ids
```

Stage/status behaved exactly as promised: closing a deal preserves the stage it closed
from, and the pipeline's own vocabulary is what every refusal quotes back.

**`find`'s sticky state did exactly what it says, which looked like a bug until it
wasn't.** `crm find deal -n 1` treats `deal` as a **query**, not a kind shortcut — the
grammar is `crm find [<query>] [--kind …]`, and `crm find -h` says so. Left as the sticky
query, it made a later `--kind deal` search return nothing until cleared with
`crm find "" --kind deal`. This is the documented "an omitted field keeps its current
value" rule working as designed, confirmed once cleared — not a finding, recorded so the
next reader doesn't chase the same shadow.

### MAJOR — `crm done`'s refusal names a recovery path that does not exist

`src-tauri/src/state.rs:2154`

```
$ crm task acme-renewal "Send the signed order form" --due 2026-10-01
    task set
$ crm done acme-renewal
    crm: no task matches "acme-renewal" — its record's `crm show` lists it
$ crm done "Send the signed order form"
    crm: no task matches "Send the signed order form" — its record's `crm show` lists it
$ crm show acme-renewal
    deal acme-renewal — Acme renewal
      Company  Acme Corp (acme-corp)
      Value    $45,000.00
      Stage    Lead
      Status   Open
```

`crm show` prints exactly four fields — Company, Value, Stage, Status — and nothing about
open tasks or the timeline. The refusal's own advice does not work: I confirmed this by
running `crm show acme-renewal` immediately after the refusal, verbatim. `crm find`,
`crm board` and `crm due` (counts only, no listing) were checked too; none of the four read
verbs lists a task by name. The only way to reach `done` is to have minted the task's
handle from its own `what` text using the same slug rule every other handle in the app
follows — `send-the-signed-order-form` — which worked once guessed, but nothing in `crm -h`
or `crm task -h` says a task even has a handle, let alone how to find it again.

`round-3-snapshot.md` asked only that `crm show` print `focused.fields`, and it does that
correctly — `Task.handle` is a real field on the wire (round 3 minted it precisely so
`crm done` would have something to resolve), it is simply never surfaced anywhere a person
or an agent can read it back. This is the exact failure mode the charter's "does every
refusal teach" test exists to catch: it teaches something false.

### The pipeline is unbroken — `crm select`'s reply is the one place it lies

Covered in full below, since the task named it directly.

---

## The person's pass — every control, from the window, terminal closed

`npm test` renders every scenario **collapsed**; nothing in the automated suite clicks or
types. The eight controls exist to be driven, so I drove them, live, in the real
component tree through `npm run preview` — the same `App.tsx`/`Board.tsx`/`Record.tsx`
production code the packaged app ships, with only the Tauri transport mocked.

| Control | Result |
|---|---|
| **New company / New contact** | inline form above the People table, no modal; submitted "Priya Nair Jr" — appeared in the list immediately |
| **New deal** | a `+ New deal` row at the foot of each open column; opens inline, cancels cleanly |
| **`set` — edit in place** | clicked the Value field, it became a real `<input>` pre-filled with the current value; committed on blur (a real mouse click to another focusable control, not a synthetic event) and the field returned to a button showing the new value |
| **`log` — composer** | typed into "What happened?", submitted with the "Note" chip active; the new line appeared newest-first, attributed to **You** |
| **`task` — composer** | "Send updated pricing" + a due date; appeared in Next steps immediately |
| **`done`** | the clock icon on an open task is a real `<button title="crm done <handle>">`; clicking it struck the task through |
| **`archive` / restore** | the header button flipped to **Restore**, with `title="crm archive hollis-renewal --restore"`; restoring cleared the archived tag |
| **`move`** | the drag, plus a keyboard-reachable `<select>` beside the record title, reaching every stage |

**A genuine refusal, reproduced independently.** Firing the same task's Done button twice
before the first write settles: the first succeeds, the second returns `{ ok: false,
error: "already done" }` at the wire level, and `ErrorLine` renders it inline next to the
row without disturbing the completed task. This satisfies the acceptance box directly — I
did not have to take the frontend's word for it.

**Keyboard reachability**: a real Tab sweep from the rail reached a board card, then the
record's stage `<select>`, with a visible 2px focus ring at every stop confirmed against a
real Tab press (`:focus-visible`), not a programmatic `.focus()`. Every control above is a
native `<button>`, `<input>`, or `<select>` — none is a `div` with a bare `onClick`.

**No id anywhere in a control.** The `Link…` field's own hint round-trips a handle to its
label ("Link to Northwind expansion (northwind-expansion)"); nothing rendered was
ULID-shaped in any state I drove it through.

### Two things already disclosed in `docs/window.md` — confirmed, not new

- **"The Link control's real limit"**: it searches only what the window already has
  loaded (the board's cards and the shared list's current page) by handle — a text field
  with a live match/no-match hint, not a picker over the full record set. I confirmed this
  directly: typing a handle for an off-screen record returns "No record with that handle
  is loaded here yet." `m7-window-editing.md` did ask for "picking from the existing
  records," and this is a narrower reading of "existing" than that sentence suggests — but
  the frontend's own report names the exact reason (`m2-cli.md` defines no scoped search
  envelope) and flags a full picker as a reasonable follow-up rather than pretending to
  resolve a handle the window cannot see. Reported here as confirmed and already surfaced,
  not as a fresh defect — it is a PM scoping question, not an unreported gap.
- **`ink-3` on `--surface` measures 4.279:1**, just under the 4.5:1 AA line for normal
  text — recomputed independently from the committed hex values (`#7C8784` on `#1F2322`)
  and it matches the commit message's own "4.28:1 (*)" to three decimal places. Also
  self-flagged there as a frozen value from `m3-window.md`, not the frontend's to retune
  under "no colour outside the token table." Confirmed real, confirmed already reported —
  it is the one token in the whole palette that does not clear its own bar.

### One thing not independently locatable

`m8-visual.md`'s acceptance asks for "screenshots of both themes in the report — board and
an open record." The M8 commit message says these are "attached to the PR/report," but
there is no GitHub PR for this repository (`gh pr list` returns none — every round has
merged locally) and no image file was committed, so I could not independently locate the
claimed screenshots. I viewed both themes myself, live, against the real `snapshot.json`
fixture, in an interactive browser session — see below.

---

## Signals, and identical state — verified structurally, not just tested

`main.rs:60-84`'s `Core::command` is the **only** function either surface reaches:
`caller` is the CLI's agent id or `None` for the window, and the gate —
`if caller.is_none() { self.control.emit_all(out.emits) }` — is backed by a second,
independent gate inside `state.rs`'s own `command()` (`state.rs:1571`:
`let emits = if caller.is_none() { emits } else { Vec::new() };`). An agent's write cannot
reach a signal through either path.

**Both wire shapes converge to one line of code before either name matters.**
`resolve_write_target(verb, req, "id", "handle", …)` (`state.rs:1685, 1879, 1912, 1950,
1981, 1984, 2006`) is what `move`/`set`/`log`/`task`/`link`/`archive` each call first — the
window's envelope carries `id`, the CLI's carries `handle`, and this one function is where
they stop being two things. Nothing downstream of it can tell which surface asked.

`state_tests.rs` proves this rather than asserting it: `assert_same_core_call_and_human_
only_signal` (`state_tests.rs:1694`) seeds two independent states identically, drives one
with the window's id-shaped envelope and the other with the CLI's handle-shaped one,
asserts `a.db() == b.db()` — literal database equality, not "should behave the same" — and
asserts the window path signals exactly once while a third, independently-seeded state
driven by the CLI **with a real agent caller** signals not at all. Nine such tests exist,
one per write verb including both directions of archive. I ran them directly:

```
running 10 tests
..........
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 198 filtered out
```

I did not need to add my own — this is a stronger proof than a manual comparison could be.

---

## M8 — tokens and the mono discipline, re-measured

Every number below computed independently from `styles.css`'s actual hex values (WCAG
relative-luminance formula), not read from the commit message.

| Token | Claimed | Measured |
|---|---|---|
| `--ground` lightness | ~10.0% | **10.00%** |
| `--surface` / `--surface-2` / `--border` | 12.9 / 16.1 / 21.2% | **12.94 / 16.08 / 21.18%** |
| `ink` on ground | 14.66:1 | **14.66:1** |
| `ink-2` on ground | 8.05:1 | **8.05:1** |
| `ink-3` on ground | 4.67:1 | **4.672:1** |
| `accent` / `due` / `lost` on ground | 7.20 / 7.85 / 6.50:1 | **7.20 / 7.85 / 6.50:1** |
| `ink-3` on **surface** | (self-flagged, below AA) | **4.279:1** — confirmed |

Every one matches. The light theme's own tokens are untouched — `git diff` on the bare
`:root` block between this commit and its parent shows zero lines changed.

**Mono discipline**: exactly two CSS rules apply `var(--font-mono)` in the whole
stylesheet — `.num`/`.mono` and `code`. Grepping every `className="..."` containing `mono`
or `num` across every component (`nav-count`, `column-count`/`-total`, `card-value`,
`due-count`, `candidate-n`/`-handle`, `col-value`, `record-value`, `section-count`,
`entry-when`, `task-due`, `pageWording`) — every site is a count, a currency amount, a
date/relative-time, or a handle. `disc-mono` is a false positive by name only: it sets no
`font-family` at all (it is the initials-avatar fallback, "mono" as in "monogram"). No nav
item, field label, button, or title carries it.

**Sentence case**: `.micro` (`styles.css:1013`) no longer carries `text-transform`. The
only two remaining `uppercase` rules that touch a label are `.column-head h2` and the
table's own `thead` cell — both genuine column headers, exactly the carve-out the order
named. The third remaining `uppercase` is `.composer-input-currency`, a 3-letter ISO
currency code field — a different and unrelated convention, not a label.

**Viewed live**, not from a static export: `npm run preview`, the "Core: snapshot" scenario
(the real `snapshot.json` fixture, an open record on "Hollis renewal"), switched between
the Light and Dark theme buttons in the same session. No image file was produced — the
tooling available for this review renders and drives the page interactively but does not
export the pixels to a file, and I did not want to spend more of this review chasing a
screenshot exporter than the finding itself was worth. Both themes read as an ordinary
business tool at the size and density the order asked for: soft charcoal grounds in dark,
a muted teal accent, sentence-case field labels, mono confined to the figures. Nothing
about either theme contradicted a number in the table above.

---

## The carried-in finding — confirmed, and the root cause

**`crm select` always answers "opened `<kind>` `<handle>`," whichever write it resumed.**

Reproduced twice, independently, against real ambiguous input:

```
$ crm log note acme "Ambiguity test"
    More than one record matches — pick the one you meant to finish this:
      crm select 1  Acme Corp (company)
      crm select 2  Acme Industries (company)
      crm select 3  Acme renewal (deal)
$ crm select 1
    opened company acme-corp
```

Checking the store directly (not through the CLI, to rule out the CLI hiding its own
success): the note **was** logged against Acme Corp. Repeated with `crm set acme domain
acme-updated.com` → `select 1` → the domain **was** updated. Both writes succeeded every
time; the confirmation never once said so.

**Root cause, in `cli.rs:1077-1084`:**

```rust
fn confirm_select(resp: &Value) -> String {
    let Some(row) = resp.pointer("/focused/row") else {
        return "selected\n".to_string();
    };
    let kind = row.get("kind")...;
    let handle = row.get("handle")...;
    format!("opened {kind} {handle}\n")
}
```

`render()` dispatches purely on the verb the agent *typed* (`"select"`), never on what the
core actually did. The response already carries the answer: `state.rs`'s `command()`
stamps `resp["answer"]` with `"read"`, `"changed"`, or `"ambiguous"` depending on what the
resumed verb produced (`state.rs:1550-1565`) — every other write verb's own confirmation
(`"updated"`, `"logged"`, `"task set"`, `"linked"`) reads directly off this same
distinction. `confirm_select` is the one confirmation function in the file that ignores
it, so a `select` that resumes a `log`, `set`, `task`, `move`, `link`, `archive`, or
`done` all say the same thing a plain `show` would say. An agent cannot distinguish "your
write went through" from "you just looked at a record" from output alone — confirmed
identical to what `crm select` prints when the pending question came from a plain `show`.

**Severity: Major.** Every write this app has can be deferred through `pending`
(architecture §6, and `m2-cli.md`'s own words: "this is the most interesting thing in the
app"), so this is not an edge case — it is the confirmation for an entire class of writes,
silently indistinguishable from a no-op read.

---

## What this round did, and did not do

**Did:** ran rule zero on `main` at `1ec2742` in a clean worktree. Drove the packaged
binary as an agent, from `-h` alone, through a full day's work under an isolated `HOME`
(the same short `/tmp` symlink workaround as rounds 2 and 3, for the Unix socket path
limit), verifying every claim by cross-checking the store directly where the CLI's own
output could not confirm it. Drove all eight of the person's controls live in the real
component tree via `npm run preview`, including a real mouse-driven blur to commit an
inline edit and a real double-click to trigger a genuine refusal. Re-derived every M8
contrast and lightness number independently from the committed hex values. Read the
signal-parity proof in the test suite and ran it directly rather than trusting the count.

**Did not:** fix anything, merge anything, or touch `clappkit/`. One transient,
non-reproducible failure to commit an inline edit was investigated at length (a fresh
reload, a controlled repeat of the exact same sequence, and a rapid-scenario-switch stress
test all committed correctly) and is not reported as a finding, since three deliberate
attempts to reproduce it failed.

**Out of scope, as given:** the window's missing bulk/import UI (unchanged since M7 was
scoped to exclude it), the remainder of M4/M5 signal wiring beyond what M2 built, and
anything under `clappkit/`.
