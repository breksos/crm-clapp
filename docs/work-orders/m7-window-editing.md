# Work order — M7: the person's hands

**Owner:** frontend · **Branch:** `m7-editing`, from `main` after M2 merges
**Depends on:** M2's write envelopes · **Blocks:** nothing, and it should have blocked v1

Read [`CLAUDE.md`](../../CLAUDE.md), [`architecture.md`](../architecture.md) **§10b**, and
[`m3-window.md`](m3-window.md) — its density, palette and prohibition list all still hold.

---

## Why this exists

**The window has no way to add anything.** A person can look at the board, open a record and
drag a card, and that is all. To create a company, log a call or set a next step they must
open a terminal and type a CLI command — which is not a CRM, it is a viewer with a manual.

**This is my error, not the frontend's.** The M3 order told you empty states should print
"the exact CLI verb that would fill them", and the envelope I froze in round 3 carried no
write shapes at all. You built exactly what was specified. Both documents are corrected.

Two things follow, and the second is the one that matters:

1. A person cannot use this app without a terminal. That alone is disqualifying for a CRM.
2. **`record.changed` and `note.added` fire on human actions only.** With nothing in the
   window that creates or logs, those two signals could never fire — so the agent could never
   learn what the person did. A read-only window does not just inconvenience the person; it
   severs half of the loop this app exists for.

## The rule

**Every verb has a control.** Anything the agent can do, the person can do without leaving the
window:

| verb | the person's control |
|---|---|
| `add company` · `add contact` · `add deal` | a **New** control on each rail view, and on the board per column (the column decides the stage) |
| `set` | **edit in place** on the record panel's fields — click the value, type, commit |
| `log` | a composer on the record panel: kind (call/email/meeting/note) + body |
| `task` | **Add next step** on the record panel: what + a due date |
| `done` | a checkbox on each open next step |
| `link` | **Link…** on the record panel, picking from the existing records |
| `archive` | on the record panel, with **restore** reachable from an archived record |
| `move` | already done — the drag. Keep it, and add a stage control on the record panel for keyboard users. |
| `find`, `show`, `select` | already done |

`crm import` and `crm export` stay CLI-only for now — both take a file path, and a file picker
is a bigger surface than this order. Say so in the UI rather than leaving people guessing.

## The parts that are easy to get wrong

**Keep the CLI hints — beside the controls, never instead of them.** They are genuinely good:
they teach a person what they can ask their agent for. An empty board should offer a **New
deal** control *and* show `crm add deal "…"` as a quieter line beneath. A hint that stands in
for a control is the defect being fixed here.

**Editing is in place, not a modal.** This is a dense instrument (`m3-window.md`), and a modal
over a board is how a dense instrument stops being one. The record panel already has the
fields; make them editable where they sit. `New` may open a small inline form in the column or
panel — not a dialog over the whole window.

**Optimistic, then corrected by the snapshot.** A write sends its envelope and the core answers
with a fresh snapshot, ordered by `rev` — `useSnapshot` already drops a stale one. Do not build
a second store to hold pending edits; render the optimistic value, let the snapshot replace it,
and on a refusal restore the previous value and say what the core said.

**A refusal must be shown, not swallowed.** The core rejects things — a handle that matches
nothing, a stage that does not exist. Surface the core's own sentence inline next to the
control. Never invent your own wording for a rule the core owns.

**Do not validate the domain in the window.** The core decides what a valid stage, date or
currency is. The window may stop an empty required field before sending; everything else is the
core's answer, or you have written a second rulebook that will drift from the first.

**Still no "ask the agent" button.** §11 stands and is a different rule from this one: the
person acts on the record directly, they do not ask the agent to act for them.

## Tests

- [ ] every row of the table above has a control reachable **by keyboard alone**
- [ ] each control sends the M2 envelope and renders the returned snapshot
- [ ] a refused write restores the previous value and shows the core's sentence
- [ ] an optimistic value is replaced by the snapshot, and a stale `rev` never wins
- [ ] a window `add`/`set` emits `record.changed`; a window `log` emits `note.added`
- [ ] the leak guard still passes: **no id in any rendered text, and none in any control**
- [ ] the empty board leads with a control, with the CLI hint secondary
- [ ] both themes; `prefers-reduced-motion` honoured; focus visible on every new control

## Acceptance

- [ ] **A person can run a full day from the window alone**: add a company, add a contact under
      it, add a deal, log a call, set a next step, complete it, edit a field, move the deal,
      close it won, and archive something — with the terminal closed
- [ ] the CLI can do all of it too, and both routes produce identical state
- [ ] `npm test`, `npx tsc --noEmit` and `npm run verify` green

## Do not

- **Do not add a modal dialog over the board.**
- **Do not build a second validation layer.** The core owns the rules.
- **Do not touch `src-tauri/`.** If an envelope is missing or wrong, that is a PM conversation.
- **Do not remove the CLI hints.** Demote them; they teach.
