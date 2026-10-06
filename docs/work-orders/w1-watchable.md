# Work order — W1 watchable: every agent step visible in the window

**Owners:** backend (part A), then frontend (part B) · **Branches:** `w1-backend`, `w1-frontend`
**Priority:** ahead of the rest of p2. This is the product's pitch, and today it doesn't hold.

Read [`CLAUDE.md`](../../CLAUDE.md), [`architecture.md`](../architecture.md) §7 and §10b,
and [`m11-colour-ownership.md`](m11-colour-ownership.md).

---

## Why

The product owner's test: *"I tell an agent to do something, with 10-second delays between
calls, and I should be able to inspect every step it takes, from the window."* This is the
reason for building on Clatch. It fails today, and the cause is in the core, not the CSS:

| what the agent runs | what the window shows today |
|---|---|
| `add`, `show`, `select` | the record opens (they move the shared focus) |
| `find` | the shared list changes, but only if the person is already on that page |
| `move` | the card moves, if the board is on screen. The desk lists only the *last* move per deal |
| `log`, `task`, `done`, `set`, `link`, `archive` | **nothing**, unless that record already happens to be open |
| `board`, `stages`, `due`, `status`, `export` | **nothing at all** |
| a refused call (exit 1) | **nothing at all**. The person never sees the agent fail |

The core keeps no record of what the agent *did*, only the end state of what it wrote. The
desk's own comment says so: *"the core has no live status, only what was written."*

## The rule this order makes true

**Every call an agent makes that reaches the app produces one visible line in the window, as
it happens, in order, attributed to that agent. Its effect is on screen or one click away.**
That includes reads and refusals.

---

## Part A — backend: the agent feed

### A1. The feed

A new field in the per-seat **view** state: `feed`, a ring of the most recent **100** calls,
newest first. One entry per call that reached the app:

```
{ seq, at, actor, verb, line, outcome, message?, target? }
```

- `actor`: the same `Actor` the write records in `by`. A person's window actions are **not**
  feed entries: the window doesn't log itself to itself.
- `verb`: the `crm -h` verb word.
- `line`: the command **as the agent typed it, in handles**, e.g.
  `move orbit-line-monitoring won`. No ids, ever. No secrets: there are none on our command
  line today, so keep it that way and test it.
- `outcome`: `ok` · `refused` · `asked`. `asked` means it parked a `pending`.
  `message` is the refusal text the agent saw, verbatim.
- `target`: the record the call was about (id plus kind), so a line can open it. Absent for
  `stages`, `status`, `export`.

**Not persisted.** It is view state that lives only while the app runs, and the window says
so. Restarting empties it. Persisting it is p3's problem along with the delta log.

**Not a signal.** The agent is never told about its own calls, so the "only human actions
signal" rule is untouched. The feed is something the window *reads*.

Command lines that fail to parse (exit 2) never reach the app, so they aren't in the feed. That
is accepted for now.

### A2. Follow

A second per-seat view field: `follow: bool`, **default on**. The window sets it with a
control. It needs no CLI verb, because it is the person's choice about their own window.

When follow is on, every agent call that has a `target` **moves the shared focus to that
target**, exactly as `show` does today. The page-bearing reads also set a new view field,
`page`: `board` → pipeline, `find` → the matching table, `due` → inbox. This is done core-side
on purpose. If the window navigated by sending its own `show`, that would emit `deal.opened`
to the agent about the agent's own action, which is the self-talk loop.

When follow is off, focus moves only as it does today.

**Amended 2026-10-07, after the product owner's live test.** A call with a `target` also sets
`page` to **where that record lives**: a contact or company → the contacts table, a deal →
pipeline. So `crm add contact …` with follow on lands the person on Contacts with the new row
visible and the record open. Watching an agent work should never need a click.

**And the shared list is live, not a stored result.** `view.list` rows and `total` are the
current query and filters run against the data *now*. Any write that adds, renames, archives or
restores a record recomputes them in the same commit. A row the person cannot see until they
switch pages is a W1 failure, whoever wrote it, the agent or the person.

### A3. Contract

The snapshot shape changes (`feed`, `follow`, `page`) and so does `crm -h`. Add one line to the
intro: *"Everything you run shows, step by step, in the person's window."* An agent should know
it is being watched. This is a surface-contract change: the PM reviews `clatch.json`, `crm -h`
and the snapshot together.

### A — tests

- [ ] a sweep over **every verb in `crm -h`**: one call → exactly one feed entry with the right
      verb, line, outcome and target
- [ ] a refused call produces a `refused` entry carrying the exact message; an ambiguity produces `asked`
- [ ] a window write produces **no** feed entry, and an agent call still emits **no** signal
- [ ] follow on: `log` on a record moves focus to it; follow off: it doesn't
- [ ] no id and no ULID-shaped string in any `line`
- [ ] the ring holds at 100; `seq` is monotonic across the wrap

---

## Part B — frontend: show it

Starts when part A merges. Item **B1 has no dependency and can start now.**

### B1. The record panel collapses

The right-hand record panel gets a collapse control in its header and a shortcut (`⌘\`).
Collapsed, it becomes a slim rail that still shows the open record's name and an **unread dot**
when an agent touched that record while collapsed. Clicking the rail expands it.

The collapsed state is window chrome, so it lives in `localStorage` (wrapped, allowed to fail,
like the theme). It is not shared state. Follow never expands a panel the person collapsed:
their choice wins, and the dot tells them something happened.

### B2. The feed, on screen

An **Agent activity** strip in the desk, newest at the top, one line per entry:

- the agent's disc and name in its tint (M11: colour means an agent did this)
- the verb and line in mono
- the time ("now", "12s ago")
- for refusals, the message in the error style. **A refusal must be as visible as a success**
- clicking a line opens its target

A newly arrived line is emphasised for about 2 seconds. Respect `prefers-reduced-motion`. A
footer says *"Since the app started"*, because it isn't persisted.

### B3. Follow, in the window

- A **Follow agent** toggle in the desk, bound to the shared `follow`.
- When on, the window goes where the agent went: it switches to `page` and opens `focus`.
- **Never take the person out of unsaved work.** If a form or inline edit is dirty, hold the
  navigation and show a chip: *"Agent opened orbit-line-monitoring — go"*.
- Whatever the agent changed gets a ~2-second highlight in the agent's tint: the board card
  it moved, the timeline entry it logged, the field it set, the next step it added or completed.

### B — tests

- [ ] the feed renders every outcome; a refusal line is styled as an error and carries the message
- [ ] follow navigation is held while a form is dirty, and the chip appears
- [ ] the collapsed panel survives a reload, and the unread dot sets and clears
- [ ] no "ask the agent" control anywhere (CLAUDE.md)

---

## Acceptance — the 10-second test (QA runs it, live)

An agent runs a script using **every verb in `crm -h`**, including one refusal and one
ambiguity answered from the window, with `sleep 10` between calls. A person watching only the
window, with no terminal, writes down each step. **Their list must match the script, in order,
step for step.** Run it twice: follow on, and follow off with the panel collapsed.

- [ ] `cargo test` and `npm test` green, counts reported; `npm run verify` green
- [ ] `crm -h`, `clatch.json` and the snapshot agree, and the PM has signed off

## Do not

- Do not send a window `show` to follow the agent. That is how the app ends up talking to itself.
- Do not add an "ask the agent" button.
- Do not persist the feed.
