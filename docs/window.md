# The window — M3

What was built, how to look at it, and the four things the PM has to settle before it can
run against the real core.

```sh
npm run preview     # http://localhost:5174/preview.html
```

That is the whole development loop: the window in a plain browser against a fake snapshot,
no Rust build and no Clatch. Everything below was designed there.

---

## Four things that need a decision

These are **contract questions, not code problems.** The window is finished and driveable
today against `src/preview.ts`; each item below is a place where the frozen snapshot or
the manifest cannot express something the M3 order asks the window to draw. None of them
changes a field M2 is already building against — every proposal is additive and optional,
and the window degrades to an empty state rather than breaking when a field is absent.

### 1. The snapshot carries no deal bodies, no record, and no timeline

The frozen shape gives the board `columns[].dealIds` and gives `focus` a `{ kind, id }`.
It carries neither the deals those ids name nor the activities of the focused record — so
as frozen it cannot feed **three of the eleven components** in the work order: the deal
card ("a deal + `by`"), the record detail ("deal / contact / company, with its timeline")
and the timeline itself.

Proposed, deliberately shaped so the core can reuse the serialisers it already has —
`AppState::row()` for both, and serde's own `Activity` and `Task`:

```jsonc
"cards": {                       // one per id appearing in board.columns[].dealIds
  "d_hollis": { …the existing row shape…, "by": { "kind": "agent", "id": "…" } }
},
"focused": {                     // present iff `focus` is non-null
  "row":      { …the existing row shape… },
  "fields":   [ { "label": "Company", "value": "Hollis Partners" } ],
  "timeline": [ { "id", "kind", "body", "at", "by" } ],
  "tasks":    [ { "id", "what", "due", "doneAt", "by" } ]
}
```

`by` on a card is *who last moved this deal* — derivable as the actor of the most recent
activity linked to it. It is what the attribution disc on a board card draws, and without
it the differentiator this milestone exists for is not renderable on the board at all.

Until this lands the board still draws the right columns, counts and totals; the cards
fall back to showing the deal id, and the record panel shows its empty state.

### 2. The list has no kind filter, so People and Companies cannot narrow

The rail has three entries. Board draws deals. **People and Companies are a filter on the
one shared list**, and the filter has to live in the core for exactly the reason page and
sort do: narrowing the rows in the window alone leaves the footer saying "25 of 143" over
four visible rows, and leaves the agent looking at a list the person is not. That is the
drift `docs/architecture.md` §6 exists to prevent.

Proposed: an optional `kind` on `list`, echoing back what the window (or `crm find`) set.

```jsonc
"list": { …, "kind": "contact" }   // or null for all three
```

### 3. The window's command envelope is not settled

M1 stopped before inventing one — correctly, since "the envelope's argument shape is part
of the surface contract and is the PM's to settle". The window needs four, and they are
written down in one place (`src/preview.ts`'s fake core) so M2 knows exactly what it has
to accept. **Every one maps onto a verb that already exists in `connector.commands`**, so
none of them asks for a change to the manifest:

| envelope | manifest verb | sent when |
|---|---|---|
| `{ cmd: "state" }` | — | on mount, by `useSnapshot` |
| `{ cmd: "show", kind, id }` | `show` | a card or a row is clicked |
| `{ cmd: "move", id, to }` | `move` | a card is dragged to another column |
| `{ cmd: "select", n }` | `select` | a `pending` candidate is clicked |
| `{ cmd: "find", query?, sort?, page?, kind? }` | `find` | search, sort, paging, rail filter |

`find` carries four optional fields rather than four verbs on purpose: an omitted field
keeps its current value, so the window can turn the page without restating the search, and
the CLI expresses the same thing as flags (`crm find acme --sort value --page 2`) without
`connector.commands` growing an entry.

### 4. The window has to format money, and the core says it shouldn't have to

`Money::format()` lives in the core because "both surfaces show totals and two formatters
is two answers" — but the snapshot ships `{ amount, currency }` raw, so the window has no
formatted string to render and does the arithmetic itself. `formatMoney` in
[`src/bridge.ts`](../src/bridge.ts) mirrors it digit for digit, exponent table included.

Two implementations of one rule is exactly what that comment was written to prevent. The
durable fix is for the snapshot to carry the formatted string beside the raw amount; the
raw amount has to stay, because sorting and comparison need it.

---

## Decisions taken here

**Attribution is drawn everywhere something was written**, never on hover: on every board
card, beside every timeline entry, on every task. An agent's disc is coloured and pictured;
the person's is plain. The asymmetry is deliberate — scanning a board, the agent's writes
should be the ones that catch the eye, because you already know what you did yourself.

**Everything per-agent is keyed on the immutable id**, never the display name, so a rename
relabels a chip in place instead of unmounting and remounting it. `agentTint` is
clappkit's, so an agent is the same colour here as in every other app in the family.

**The ring.** When a snapshot shows a deal in a different column than the last one, that
card is outlined for 1.2s in the mover's own tint. A deal with no previous column is *new*,
not moved, and does not ring — otherwise the first snapshot after launch would ring the
whole board. Under `prefers-reduced-motion` the outline still appears and still clears; it
just does not pulse, because the mark is the signal and the animation is only the polish.

**The theme is local, never shared.** `localStorage`, every access wrapped, and "system" is
a real third state that stamps nothing on the root rather than a fallback. Every token is
defined in bare `:root` before any media query or `[data-theme]` block redefines it — a
colour defined only inside a media query does not apply in the un-stamped state, which is
the classic unreadable-window bug. Both un-stamped states were checked under an emulated
OS light and dark.

**There is no "ask the agent" button, and no chat surface.** Architecture §11.

**The reminder caveat is in the rail, at rest, in plain words** — not a tooltip, not a
settings page. Somebody who learns that reminders need the app open by missing a follow-up
has learned it the worst possible way.

## The preview harness

`src/preview.ts` installs **Tauri's own `mockIPC`** underneath `@tauri-apps/api`, so
`useSnapshot`, `useAsset` and `cmd` are the real ones running their real code — including
the `rev` ordering that drops a stale snapshot, and the listen/unlisten cycle React's
StrictMode exercises twice on mount. A harness that mocked the *bridge* would be a second
window, and the states it proved would be states of the mock.

Seven states, one button each — the six the work order names, plus the roster rename,
which is an acceptance item that is otherwise impossible to reach on demand:

| | shows |
|---|---|
| Pipeline | the everyday board: two agents, six columns, two currencies |
| Agent move | Nia moves a deal to Won after 1.2s; the card rings in her tint |
| Roster rename | Nia is renamed after 1.2s, same id — the chip must relabel, not re-create |
| Pending | three candidates for "Acme", answerable by click or `crm select 2` |
| Lopsided board | 26 deals in Qualified, nothing in Proposal or Negotiation, and a list long enough to page |
| Long text | a 40-character deal title and a 62-character company name |
| Zero state | no deals at all |

plus Light / Dark / **System**, where System removes the stamp so the un-stamped state is
reachable on demand.

Switching scenarios **remounts** the window rather than pushing the new world. Replacing
the world wholesale looks to the board exactly like several cards moving at once, so a
pushed switch rang half the board every time you changed states — a fresh mount starts
with no history, which is what launching the app actually does.

The fake agent ids are chosen rather than typed at random: `agentTint` picks from five
colours, so two arbitrary ids collide about a fifth of the time, and the first pair here
did — which made a harness for "tell the two agents apart" draw them both the same brown.

`preview.html` is not an entry in `vite build` and `main.tsx` never imports `preview.ts`,
so none of this reaches the shipped bundle.

## Density, measured

At a 900px window the shell **never scrolls**: the header and rail stay put, and the board
and the list scroll inside their own panes. The pagination footer is pinned, because the
count both surfaces quote at each other is exactly what you need when the page is long.

A full 25-row page is 828px of table (25 × 32 + a 28px head). The chrome above and below it
is 146px — 44 header, 42 list bar, 32 footer — so 705px of it is visible at 900px and the
last two or three rows are one scroll away *inside the list*. Rows cannot be shorter: 32px
is the floor the work order sets and the bottom of the range compact enterprise tables use.
Recorded rather than rounded up, because "fits a 900px window" and "the shell does not
scroll" are the same requirement only if you say which one you meant.

## Verified

- both themes complete; **no token defined only inside a media query or `[data-theme]`
  block** — checked by parsing `styles.css`, not by eye
- the un-stamped system state renders correctly under an emulated OS light *and* dark
- all five font faces load from local files; `document.fonts.check` confirms Geist at 13px
  and Geist Mono at 12px are actually in use, not a fallback
- table rows measure 32px; money is `tabular-nums` in Geist Mono and right-aligned
- currency totals render one line per currency and are never summed across them
- the footer reads `25 of 30` / `page 1 of 2`, from `pageWording` in `bridge.ts`
- an agent move rings the card in `agentTint(<that agent's id>)` for 1.2s, then clears;
  loading a scenario rings nothing
- a roster rename relabels in place: after the name changes, the chip element, its label
  span and its avatar `<img>` are the **same DOM nodes** as before (checked by tagging
  them and confirming the tags survived), so nothing flickers and the avatar is not
  refetched
- keyboard focus shows a 2px `--focus` ring on every interactive element, confirmed with
  real Tab presses (a programmatic `.focus()` does not trigger `:focus-visible`, so an
  earlier check that used one was measuring nothing)
- no gradient, no `box-shadow`, no literal radius above 6px, no `text-align: center`,
  no emoji anywhere in `src/`

## Not verified here

The two installed reference clapps (`com.arfium.maps`, `com.arfium.chess`) **could not be
run**: both depots are refused by the installed Clatch with

> `launch` names exactly one OS - this depot's own

so the house-style comparison the work order asks for was done against
`clappkit/docs/elements.md` and the M0 scaffold instead. Worth someone re-running once
those depots are repacked.
