# Work order — M12: the shell and the pages that have data

**Owner:** frontend · **Branch:** `m12-shell`, from `main` after round-5 fixes land
**Source:** `design_ideas/A Instrument.dc.html` — the direction the product owner chose

---

## Why now, when I said later

I deferred this because Reports and Team need data one user cannot produce. That is still true,
but it was never true of the *whole* IA, and one thing makes the timing matter:

**`p1` is about to redesign the snapshot.** If the real surface exists first, `p1` designs a
contract that serves it. If it does not, `p1` freezes a contract the IA then breaks — which is
precisely the mistake I made twice: a snapshot that could not draw a card, and an envelope with
no write shapes. Build the surface, find the gaps, then design the contract once.

## Build now — every one of these reads data we already have

| Page | Source |
|---|---|
| **The shell** | sidebar, saved views, ⌘K global search, the agent desk from M11 |
| **Home** | counts, currency totals, what needs attention today, recent agent moves |
| **Inbox** | the activity feed — every timeline entry, filterable by actor |
| **Pipeline** | today's board, unchanged |
| **Deals** | today's table, given its own page |
| **Contacts & companies** | the list, filtered by `list.kind`, plus the record |

Serve them from the snapshot as it stands. **Where a page wants a field that does not exist, do
not invent it and do not fake it — write it down.** That list is this order's most valuable
output, because it becomes `p1`'s input.

## Stub visibly — do not build

**Reports** and **Team** get a real page with a plain sentence saying they need the platform, and
nothing else. Not a spinner, not a mock chart, not lorem. A person who clicks Reports should
learn something true in one line.

**Settings** gets only what exists today: the theme. Users, roles and pipeline editing are
platform work.

## Constraints

Everything from [`m3-window.md`](m3-window.md) and [`m11-colour-ownership.md`](m11-colour-ownership.md)
still holds — density, both themes, no gradients, no shadow past drag and ring, 6px radius
ceiling, and **colour means an agent did this**. The shell is new; the rules are not.

Direction A is the reference for *layout and information architecture*. Its amber agent marking
is **not** adopted — we settled on violet in M11, and `--due` owns amber.

Every page keeps: no id in rendered text, no "ask the agent" button, controls before CLI hints.

## Acceptance

- [ ] the six pages above work against real snapshot data, keyboard-reachable
- [ ] Reports, Team and Settings tell the truth about what they are waiting for
- [ ] ⌘K searches across companies, contacts and deals
- [ ] saved views persist per seat and never enter the snapshot
- [ ] **a written list of every field a page wanted and the snapshot lacked** — for `p1`
- [ ] `npm test`, `tsc`, `npm run contrast`, `npm run verify` all green
- [ ] screenshots of all six pages, both themes

## Do not

- **Do not build Reports or Team.**
- **Do not invent snapshot fields.** List them.
- **Do not adopt Direction A's amber.**
- **Do not touch `src-tauri/`.** If a page needs a field, it goes on the list, not into the core.
