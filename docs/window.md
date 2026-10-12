# The window — M3, and everything since

What was built, how to look at it, and what each round changed.

```sh
npm run preview     # http://localhost:5174/preview.html
```

That is the development loop: the window in a plain browser, no Rust build and no Clatch —
against the core's own golden snapshots first, and invented worlds for the states those do
not contain.

```sh
npm test            # the command guards, and the window rendered against the core's snapshots
```

---

## M7 — the person's hands

`m7-window-editing.md`, and `architecture.md` §10b: every verb gets a control, so nobody is
ever required to open a terminal to use this app. Built against the *frozen shapes* in
`m2-cli.md` — M2 is not merged, so nothing here has talked to a real core yet — through the
preview harness, the same way M3 did.

**The eight controls, and where they live:**

| verb | control |
|---|---|
| `add company` / `add contact` | a **New** button in the People/Companies list-bar, opening an inline form above the table |
| `add deal` | a **+ New deal** row at the foot of each open-stage column; the column supplies the stage, so the form does not ask for one. Won/Lost get no control — there is no sensible envelope for "create a deal already closed" |
| `set` | click a field's value in the record panel, type, blur or Enter to commit |
| `log` | a composer above the Timeline — kind chips, a body field — always present, not only when empty |
| `task` | a composer above Next steps — what, a native date input — always present |
| `done` | the clock icon beside an open task becomes a button; a done task shows a check and stays visible, struck through |
| `link` | a **Link…** control that searches records already loaded in the window (the board's cards and the shared list's current page) by handle — see the limit below |
| `archive` | a button in the record header, flipping to **Restore** once archived |
| `move` | unchanged — the drag — plus a `<select>` beside the header for keyboard users, sending the identical envelope |

**Four fields stay read-only on purpose**: `Company`/`Contacts` are `link`'s, `Stage`/
`Status` are `move`'s. Making them generically `set`-editable would have meant either the
window inventing a second way to change a relationship (drift from `link`), or every edit
attempt on them bouncing off a refusal that exists only to say "use the other control" —
worse than not offering the click at all. `EditableField` simply excludes those four labels;
everything else in the core's own field list is editable.

**The write path is one hook, used nine times.** `useWrite.ts` wraps `bridge.ts`'s new
`write()`, which is `useSnapshot`'s `run` turned inside out: `run` calls `apply` itself and
*silently drops* anything with `ok: false` (clappkit's own guard) — exactly wrong for a
write a person just typed, where a refusal has to reach the control that asked. `write()`
calls `apply` only on success and hands the caller the refusal otherwise. No second store:
`busy`/`error` are each control's own local state, the same as its draft text.

**No id reaches a control either.** `Id` is opaque (round 3), so the write envelopes are
built by calling `idKey()` at the call site, greppable the same way the read envelopes
already were. The leak guard (`window.test.ts`) covers every new component's collapsed
render; it cannot exercise an open composer or an editing field — `renderToStaticMarkup`
has no click — so that half was checked by hand, in the harness, control by control.

### The Link control's real limit

"Picking from the existing records" is scoped to what the window has already loaded — the
board's cards and the shared list's current page — because `m2-cli.md` defines no second,
scoped search envelope, and inventing one was a bigger surface than this order. Typing a
handle nothing here has loaded says so plainly rather than pretending to resolve it: this
window cannot turn a handle into an id without the core, and guessing would be exactly the
domain logic the order asks it not to own. A full record picker is a reasonable follow-up,
raised here rather than built silently.

### Two bugs the interactive pass found, not the render test

`window.test.ts` renders every scenario in its **collapsed** state — no click ever reaches
an open composer or a field mid-edit, so two real defects only showed up driving the actual
preview harness by hand:

- **A standalone company or contact vanished the instant it was created.** `addRecord`
  correctly prepended the new row to `list.rows`, but `repage()` recomputes the whole list
  from `recordsFrom()`, and for an invented world that function derives companies/contacts
  **only from deal company names** — a record with no deal was invisible to it. Fixed by
  unioning `list.rows` into that computation. (A record that later scrolls off whatever page
  it was created on can still drop out on a subsequent search — a real limit of a mock with
  no actual store behind it, noted in the code rather than chased further.)
- **The record panel kept showing the old Stage and Status after a move.** `moveDeal`
  rebuilt `board` and `cards` correctly but never touched `focused` — invisible until this
  milestone, because the one existing scenario that calls it ("Agent move") does not have
  the moved deal open at the same time. The record panel's own Move control does exactly
  that. Fixed to refresh `focused.row` and its Stage/Status fields when the moved deal is
  the one open; Stage correctly stays put on a close, same as the core's own rule.

Both were found by driving the live harness through real interaction — clicking, typing,
checking the resulting DOM — not by the automated suite, which is why that pass matters as
much as the tests do for a milestone that is mostly new interaction rather than new display.

### Verified, this round

- every one of the eight controls, driven end to end in the browser: New deal (with a real
  `--company` handle resolving to its label), New company, New contact, an edit-in-place
  commit, Move (Stage/Status both update, Stage survives a close), Archive → Restore, Done,
  a Log entry (newest-first, right kind), a Task addition
- a genuine refusal, achieved by firing a task's Done handler twice before the first write's
  `busy` state could disable the button: the first call succeeds, the second returns
  `{ ok: false, error: "already done" }` **at the wire level** — no `rev`, not wrapped in a
  snapshot — and `ErrorLine` renders it beside the row without disturbing the completed task
- the refusal dismisses on its own click, independent of the write that produced it
- `npx tsc --noEmit`, `npm test` (25/25), `npm run build:web` and `npm run verify` all green
  — the last one end to end: `cargo test` (160/160) and the packaged CLI↔GUI round-trip,
  which only exercises the read verbs M1 already answers, since M2 is not merged

## Round 3 — the snapshot the window can draw

[`round-3-snapshot.md`](work-orders/round-3-snapshot.md) settled all four questions this
document used to leave open. The window now consumes exactly that contract, and nothing it
used to propose is still a proposal.

| was open | settled as |
|---|---|
| no deal bodies, no record, no timeline | `cards` (with `by` = who last *moved* it, and `movedAt`) and `focused` (fields from the core, 50 newest activities plus `timelineTotal`, tasks with handles) |
| no kind filter on the list | `list.kind`, written through `find`, read back — People, Companies and a Show filter in the list bar all drive it, and the rail highlights what the shared filter says rather than what was last clicked |
| the `run_cmd` envelope | adopted as proposed: ids on the wire, `page` 0-based, `n` 1-based |
| the window formatting money | `formatted` on every money value; **`formatMoney` is deleted** and a test fails if a formatter comes back |

What the frontend half changed besides consuming it:

- **An id cannot be rendered.** `Id` is no longer a branded string — a branded string is
  still a `ReactNode`, so `{card.id}` compiled, and a fallback that rendered one was round 2's
  blocker. `Id` is now opaque; rendering it, printing it or passing it to a command builder
  is a type error. `idKey()` is the only way to get the string, for keys, lookups and the
  private drag payload, and every call site is greppable. A card whose body is missing is a
  neutral skeleton.
- **The window is tested against the core's bytes.** [`src/window.test.ts`](../src/window.test.ts)
  renders the real `Window` — `App` split into a hook-owning shell and a pure body — to a
  string against `src-tauri/fixtures/*.json` read verbatim, and against every invented
  scenario before and after its scripted write. It fails on any id in the output, text *or*
  attributes; on a `crm show` naming a record the snapshot does not have; and if the card
  titles, the open record, its fields in the core's order, its timeline or its tasks are not
  on screen.
- **Every interpolated value is shell-quoted** by one `shellQuote()` — bare if it is only
  `[A-Za-z0-9._-]`, otherwise POSIX single quotes. Tested by handing the output to a real
  `/bin/sh` with a space, `"`, `'`, `$` and a backtick, and through the one path where a
  person's own text reaches a printed command: the empty list echoing a search.
- **Example commands name records that exist.** The empty board says
  `crm add deal 'Northwind renewal'` with no `--company`; "nothing open" names a real handle
  from the person's records, or `crm add company 'Acme Corp'` if there are none; the `crm task`
  example's date is a week from today in local time rather than a literal that goes stale.
- **The drag carries two payloads.** `application/x-breksos-deal` holds the id and is the only
  thing the board accepts; `text/plain` holds the handle, so a card dropped on the agent's
  terminal pastes something it can type back.
- **The ring watches `movedAt`**, the field the contract defines for it, and each ring owns
  its timer. The first version cancelled its clear-timer on *every* snapshot, so a push inside
  the 1.2s window — the agent's very next write, typically — left the ring on for good.
- **Two harness bugs fixed while moving it into `scenarios.ts`.** Company and contact ids
  were re-minted on every keystroke, because the cache was keyed on a snapshot object that
  every command replaces; and opening a record changed `focus` without rebuilding `focused`,
  so the panel went on describing the previous one.

Every new guard was checked by reintroducing its defect: an id rendered through `idKey`, an id
in a `title` attribute, a `crm show` naming a missing record, double quotes instead of single,
and the fixtures absent — each fails the test meant for it. `{id}` in JSX fails `tsc`.

## Round 2 — handles, not ids

QA found the window printing ULIDs into commands it told the person to type
([`round-2-fixes.md`](work-orders/round-2-fixes.md) § Frontend). The fix is not only the
handle swap:

- **`id` and `handle` are now different TypeScript types** ([`src/ids.ts`](../src/ids.ts)).
  They were both `string`, which is why nothing objected. `commands.ts` takes a `Handle`
  and only a `Handle`, so an id cannot reach a printed command without a deliberate cast.
- **Every printed command lives in [`src/commands.ts`](../src/commands.ts)**, and
  [`src/commands.test.ts`](../src/commands.test.ts) reads the frozen grammar out of
  `m2-cli.md` and fails if one drifts out of it, if a component hard-codes one again, or if
  a preview fixture carries a readable id. Run it with `npm test` — Node strips the types
  itself, so there is no runner and no dependency.
- **The preview fixtures mint real ULIDs.** They were `d_hollis` and `c_acme_hold`, which
  is exactly why the harness showed nothing wrong: a readable id looks like a handle, so
  every screen was quietly correct here and broken against the core. The harness now also
  watches its own renders and raises an alarm bar if anything ULID-shaped reaches the
  screen.

## Decisions taken here

**Attribution is drawn everywhere something was written**, never on hover: on every board
card, beside every timeline entry, on every task. An agent's disc is coloured and pictured;
the person's is plain. The asymmetry is deliberate — scanning a board, the agent's writes
should be the ones that catch the eye, because you already know what you did yourself.

**Everything per-agent is keyed on the immutable id**, never the display name, so a rename
relabels a chip in place instead of unmounting and remounting it. `agentTint` is
clappkit's, so an agent is the same colour here as in every other app in the family.

**The ring.** When a snapshot shows a card whose `movedAt` changed, that card is outlined for
1.2s in the tint of `by`, the actor who moved it. A card seen for the first time is *new*,
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

**The reminder caveat sits directly under the due counts, at rest, in plain words** — not a
tooltip, not a settings page, and not at the foot of the rail where round 1 had it,
diagonally opposite the number it explains.

## The preview harness

The worlds live in [`src/scenarios.ts`](../src/scenarios.ts), which is pure, so the harness a
person looks at and the render test that fails the build see exactly the same set. The
core's golden snapshots come first in the harness, loaded verbatim through a Vite glob — a
branch without them still builds, and the test is what insists they exist. The frontend
reads those files and never writes them.

`src/preview.ts` installs **Tauri's own `mockIPC`** underneath `@tauri-apps/api`, so
`useSnapshot`, `useAsset` and `cmd` are the real ones running their real code — including
the `rev` ordering that drops a stale snapshot, and the listen/unlisten cycle React's
StrictMode exercises twice on mount. A harness that mocked the *bridge* would be a second
window, and the states it proved would be states of the mock.

The core's two snapshots, then eight invented states — the six the M3 order names, plus two
that are acceptance items no other scenario reaches:

| | shows |
|---|---|
| Pipeline | the everyday board: two agents, six columns, two currencies |
| Agent move | Nia moves a deal to Won after 1.2s; the card rings in her tint |
| Roster rename | Nia is renamed after 1.2s, same id — the chip must relabel, not re-create |
| Untouched record | a deal with nothing logged — the only state where the record panel prints `crm task` and `crm log`, which are the two commands round one caught carrying a ULID |
| Pending | three candidates for "Acme", answerable by click or `crm select 2` |
| Lopsided board | 26 deals in Qualified, nothing in Proposal or Negotiation, and a list long enough to page |
| Long text | a 40-character deal title, a 62-character company name, and a title made of every character `shellQuote` exists for |
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

**Record ids are minted ULIDs and handles are derived** the way the core derives them —
lower-cased, non-alphanumerics collapsed, uniquified against what is already taken, so a
second "Brightsea" becomes `brightsea-2`. Both are deterministic, so a scenario renders
identically on every reload. The roster's agent ids stay literal strings: those are
Clatch's, in a different namespace, and the window never renders one.

**The harness watches its own renders.** A `MutationObserver` on the window's root scans
every settled render for anything ULID-shaped and raises an alarm bar if it finds one —
because ULID fixtures only reveal the defect if somebody happens to open the scenario that
shows it. Checked by injecting a ULID into a rendered cell and watching the bar appear, then
removing it and watching it clear.

`preview.html` is not an entry in `vite build` and `main.tsx` never imports `preview.ts`,
so none of this reaches the shipped bundle.

## Density, measured

At a 900px window the shell **never scrolls**: the header and rail stay put, and the board
and the list scroll inside their own panes. The pagination footer is pinned, because the
count both surfaces quote at each other is exactly what you need when the page is long.

A full 25-row page is 828px of table (25 × 32 + a 28px head). The chrome above and below it
is 145px — 44 header, 26 reminder line, 43 list bar, 32 footer — leaving a 679px pane, so
**20 rows are visible** at 900px and the rest is one scroll away *inside the list*.

The reminder line is the one deliberate cost: it sits above the board and the table, so
every line it takes is a row of somebody's pipeline they cannot see. It is one line for
exactly that reason — at two it cost four rows, which is too much to spend on a sentence
that never changes.

The original acceptance box said 25 rows fit in 900px. It does not, at 32px rows with this
chrome, and the PM has since corrected the box rather than the code: the intent was that
the instrument stays put while the data moves, and that holds. **`pageSize` stays at 25** —
it is shared state that the CLI pages against too, and trimming it to fit a window would be
letting the frontend's layout set the contract.

## Verified

- both themes complete; **no token defined only inside a media query or `[data-theme]`
  block** — checked by parsing `styles.css`, not by eye
- the un-stamped system state renders correctly under an emulated OS light *and* dark
- all five font faces load from local files; `document.fonts.check` confirms Geist at 13px
  and Geist Mono at 12px are actually in use, not a fallback
- table rows measure 32px; money is `tabular-nums` in Geist Mono and right-aligned
- currency totals render one line per currency and are never summed across them
- the footer reads `25 of 30` / `page 1 of 2`, from `pageWording` in `bridge.ts`
- against `src-tauri/fixtures/snapshot.json` verbatim, the window shows every card's title,
  company and value, the core's column totals, the open record with its fields in the core's
  order, its timeline and its tasks — and **zero** ids in text or attributes, in the board
  and the list
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
- **no id reaches a rendered string or a printed command**: `commands.ts` takes a `Handle`
  and only a `Handle`, so `tsc` refuses an id; `npm test` fails if a component hard-codes a
  command, if one uses a verb or flag outside `m2-cli.md`'s grammar, or if a preview
  fixture carries a readable id. Each of those three guards was checked by reintroducing
  the defect and watching that test — and only that test — go red.

## Not verified here

The two installed reference clapps (`com.arfium.maps`, `com.arfium.chess`) **could not be
run**: both depots are refused by the installed Clatch with

> `launch` names exactly one OS - this depot's own

so the house-style comparison the work order asks for was done against
`clappkit/docs/elements.md` and the M0 scaffold instead. Worth someone re-running once
those depots are repacked.

## M9 — giving the window something to say

Source: [`design-review-2026-09.md`](design-review-2026-09.md). Everything below is inside the M8
rules: no gradient, no shadow past drag and ring, no radius over 6px, no emoji.

| | |
|---|---|
| Long names | Record title and read-only values clamp to two lines. Company and Contacts become a disclosure (the text itself, no extra row) only when they are actually cut. The full string is the hover text on the title, the fields and the board cards. |
| Money | A filled chip (`.money`, 4px radius, `--surface-2` with a hairline) on cards, column totals, the record header and the table. Tabular numerals unchanged; right edges still line up. |
| Identity | Agents: initials on clappkit's `agentTint(id)`, or the roster's own avatar. The person has no id, so cannot be tinted: an initial on a neutral disc, deliberately quieter than any agent. The move-ring for a human move is the same neutral. |
| Activity kinds | One Lucide glyph each: `phone`, `mail`, `calendar`, `sticky-note`, in the timeline and on the log composer's kind chips. |
| Icon-only controls | 24×24 hit area (was already 24; the glyph is 14). Each names itself on hover and on keyboard focus from its `aria-label`; `title` keeps the CLI spelling. |
| Header | Identity, then presence, then reminder counts, then controls, with a hairline between groups. The mark is the one place accent appears in the chrome. |
| Empty-state CLI hint | A quiet monospace line, no box. |

**Contrast.** `npm run contrast` (`scripts/contrast.py`) reads the tokens out of `styles.css` and
measures every text and graphic pairing in both themes. It found `--ink-3` under 4.5:1 in both
themes and `--due` under it on `--surface-2` in light; both were retuned (see `brand.md`).
Agent initials (white on each `agentTint`) measure 5.10–7.24:1. **Known, not fixed:** the five
agent tints as a *shape* on the dark surface measure 2.19–3.12:1; the initial, not the colour,
carries identity there. The ring an agent move draws in that tint has the same weakness — **fixed in M10**, below.

## M10 — the move-ring on dark

The five `agentTint` values (clappkit's, shared, unchanged) measure **1.98–2.81:1** against the
dark column the ring actually sits on (`--surface-2`), 2.19–3.12 on `--surface`, 2.40–3.40 on
`--ground` — under the 3:1 floor for a graphic, and worse than M9 disclosed, which measured only
`--surface`. Fixed at our end without touching the tint: the tint stays as the 2px outline (it is
*who*), and for the length of the ring the card's own border turns to `--ring-edge`, which is
`--ink` on dark (12.11:1 on `--surface-2`, 13.42 on `--surface`, 14.66 on `--ground`; 4.31–6.12
beside the five tints). Light is unchanged: every tint already clears 4.5:1 there, and ink beside
a dark tint would be the *worse* pairing (2.31–3.29), so `--ring-edge` is just the card border.
`npm run contrast` measures all of it and reads the tints from `clappkit/web/index.ts`.

### The stage ramp

Eight tokens (`--stage-{lead,qualified,proposal,negotiation}` and `-weak`), values and measurements
in [`m10-ramp-proposal.md`](m10-ramp-proposal.md). Rendered as a 3px stripe on an **open** deal's
card (a `::before`, so it cannot collide with the focus border or the ring's edge) and as the fill
behind a stage badge: the column count, the table's Stage cell, and the record's Stage field. Won
and Lost cards carry no stripe — they are statuses, not stages. The badge fills barely differ on
purpose; the stripe carries the ramp. If a late-stage card with an agent disc ever looks muddy
(`#45548C` is 5.2 ΔE from the light Negotiation stripe), the disc moves, not the ramp.

## M11 — colour belongs to agents

Source: [`work-orders/m11-colour-ownership.md`](work-orders/m11-colour-ownership.md), values in
[`m11-agent-palette-proposal.md`](m11-agent-palette-proposal.md).

- **No stage stripe on the board.** The column is the stage. Stage colour now lives only in the
  `.stage-badge` on the table's Stage cell and the record's Stage field (the M10 `-weak` fills).
  The board's column-count chips are plain again.
- **The card's left edge is `--agent`** (a 3px pseudo-element, `.card-agent`) when an agent last
  moved the card — the same violet for every agent. The disc and the ring say which: both draw
  `var(--agent-<slot>)`, so one agent is one colour everywhere it appears.
- **The move-ring is the tint alone.** M10's ink edge is retired: the new tints clear 3:1 by
  themselves on every ground (`npm run contrast`, "ring agent-N").
- **The person's disc** is an initial on a neutral disc (no id, so no tint).
- **The agent desk** is a section of the rail (`Desk.tsx`), so it is on every page. Shared
  timelines are untouched — nothing is split into a people lane and an agent lane.

### What the desk does not have, because the snapshot does not

| Wanted | In the snapshot? | What the desk shows |
|---|---|---|
| What each agent is doing **now** | No — no live status | The last deal each agent moved, and when |
| Approvals awaiting the person | **No.** There is no approval queue in the core | Nothing invented. "Waiting on you" shows `pending` only — the ambiguity question an agent asked |
| A live log of agent activity | Only the latest move per deal (`by`, `movedAt`) | "Recent moves": newest first, one per deal, not a full history |
| Undo / Keep on a move | No | Nothing |
| Agent work on a record | Only the *open* record's timeline | It stays in the shared timeline |

Each of these needs a core change (a status the agent reports, an approvals table, a move log) and
belongs in a backend order, not in a mock-up here.

**Known:** the preview fixtures' roster avatar for Nia is teal artwork. It is data, not our colour,
but beside a Won card it is exactly the collision M11 exists to prevent — real avatars are whatever
the person supplies. The ring and the desk agree with the tint only for agents without a picture.

## Round 5 — the window reads the timer

Source: [`work-orders/round-5-fixes.md`](work-orders/round-5-fixes.md) §4. The core has published
`snapshot.reminders` since M4; until now no component read it, and the window only had M3's
standing sentence. The desk (`Desk.tsx`) now has a **Reminders** block, in the same words as
`crm status` (`reminders.ts` builds them; `reminders.test.ts` fails if a sentence the window says is
not one `cli.rs` also prints):

| Field | Shown as |
|---|---|
| `lastSweepAt`, `everyMinutes` | "Checked 3m ago, every 5 min" — or, before the first check, why there is none |
| `lastSignalAt`, `lastSignalCount` | "Last sent 12m ago (2 next steps)" — or "Nothing sent yet" |
| `awaiting` | "1 due next step, sent at the next check" — or "…and no agent is connected to tell" |
| `refusal` | an alert in `--lost`: who (by name, never the id), why, that nobody was told, that it will be retried |
| `backlog` | "3 next steps were already overdue when reminders began — nobody was woken for them. `crm due` lists them." |

`reminders` is optional on `Snapshot`: if the core sent none, the desk says nothing rather than claim a
state nobody reported. **Sent is not delivered** — the desk never says delivered, confirmed or
received (a test pins that). `--lost`, not `--due`, marks a refusal: `--due` means overdue.

**The seam for backend §2.** `UnconfirmedSends` in `Desk.tsx` renders nothing and takes the
`Reminders` object, so the field names §2 chooses arrive as a type change in `bridge.ts` and the
compiler points at it. It is where the list of sent-but-unconfirmed next steps and its re-send control
go, sent through `useWrite` like every M7 control. Nothing there is guessed.

**The preview's Nia avatar** was a flat teal disc with a silhouette — a token, and beside a Won card it
invented a green-agent collision the product does not have. It is now a soft, grainy, warm portrait
stand-in drawn on a canvas (`preview.ts`); nothing in it is teal. Not a photograph: there is none to
license. Two new scenarios exercise the states: "Reminder refused" and "Reminders: first run".

## M12 — the shell and the pages that have data

Source: [`work-orders/m12-shell.md`](work-orders/m12-shell.md), layout from `design_ideas/A Instrument.dc.html`.
**What the snapshot lacked is in [`m12-snapshot-gaps.md`](m12-snapshot-gaps.md) — that list is `p1`'s input.**

Direction A's layout and information architecture; **not** its amber. Colour means an agent did this,
and that is violet; the rail itself is neutral ink.

| Page | Built from |
|---|---|
| **Home** | `counts`, the core's own formatted totals for Won and Lost, per-stage counts, `due`, `pending`, `reminders`, `agentMoves`. It draws no money the core did not send (a test pins that). |
| **Inbox** | each deal's last move (`card.by`, `movedAt`) merged with the open record's timeline; filterable by actor and type. One list — never a people lane and an agent lane. It says what it is made of. |
| **Pipeline** | the board, unchanged |
| **Deals** | the shared list, fixed to `kind: deal`; no kind switcher |
| **Contacts & companies** | the shared list, offering just those two kinds |
| **Reports · Team** | one true sentence each: what they wait for. Not a chart, spinner or lorem. |
| **Settings** | the theme, kept on the device — only what exists today |

**Shell.** Search (⌘K / Ctrl-K from anywhere), the eight pages, saved views, the agent desk. The
current page is named in the header. The highlight follows the *shared* filter, not the last click: if
the agent runs `crm find --kind deal`, Deals lights up.

**⌘K** searches all three kinds by sending the ordinary `find` and **putting the list back** (query,
kind, page) when the search ends — Escape, choosing a result, clicking away. Typing itself begins the
search, so the list can never be moved without a way back. Gap #4: a side-effect-free `search`.

**Saved views** are a query, a kind and a sort, kept in this webview's storage (`views.ts`), like the
theme. They never enter the snapshot or an envelope; applying one sends the ordinary `find`. Four are
built in; a person saves the rest from the list pages.

**The list page follows the agent.** If the agent leaves the shared list on a kind the page is not
about, the page says so and offers the way back — it never quietly shows something else.

## QA round 6c — a refusal reaches the person, and a reminder can be sent again

Source: [`qa/round-6c.md`](qa/round-6c.md) §4, both MAJORs on the window's side.

**The core's sentence, on every control that changes data.** `run` is clappkit's fire-and-forget and its
`apply` drops `{ ok: false }`, so three gestures — the drag, the "Move to" select, and a candidate click in
the parked-question banner — showed nothing when the core refused them. All three now go through `useWrite`:
the drag's error sits above the board, the select's under it, the banner's inside it, each in the core's own
words ("not saved — the disk refused it; nothing changed"), and the control stays where it was. `run` is now
for view state only (`find`, `show`); `window.test.ts` fails if a component sends any other envelope through it.

**`UnconfirmedSends`** is the control `crm status` points at. When the core reports reminders sent and still
open (`reminders.unconfirmed`), the desk says how many, that **the platform does not confirm delivery**, and that
whether the agent acted is not something the app can see — then offers **Send them again** (`{ cmd: "resend" }`,
the person's alone; the core refuses it from an agent). A refusal is the core's sentence — "no agent is connected
to send it to…" — never ours. The preview's fake core answers `resend`, and `window.__refuseWrites = true` in the
console makes any write refuse with the real wording, which is how all four paths were checked in a browser.

**Known limit.** The snapshot carries a *count* of the unconfirmed, not a list, so the control resends the whole last
batch; a per-step "send this one" (`{ cmd: "resend", id }` exists) has nothing to attach to. That is gap #1 in
[`m12-snapshot-gaps.md`](m12-snapshot-gaps.md), now with a second reason.

## p1 — the window on the split shape

Source: [`work-orders/p1-the-split.md`](work-orders/p1-the-split.md), [`split-contract.md`](split-contract.md), built
against `src-tauri/fixtures/split/*.json` — the core's own output, read verbatim, as in M3 and round 3.

**One seam.** `useSplit` replaces `useSnapshot` as the window's source (`useSnapshot` is still exported; nothing
that reads the old shape is deleted — that is the order's step 3, in its own commit). It holds a `SplitState` and
folds each message into it (`split.ts`, pure), and `toSnapshot` hands the components the `Snapshot` they always read.
A component moves onto the new shape by reading a new field, never by learning a second way to be fed. `split.test.ts`
holds the reducer to the classic golden snapshot as the oracle: every field the old shape carried comes out of the new
one unchanged, and the list's rows only *gained* facts.

**Two counters, never merged.** `rev` orders what is drawn: an older view never wins. `watermark` orders what the data
did: an update applies only where it starts (`delta.from`), never twice, and a gap is a resync (`changes`, or a
bootstrap if the log is stale) — not a guess. Feed items (`activity`, `move`) are appended and de-duplicated; rows
replace; a kind the window predates is ignored.

**Every way a message arrives:** the pushed `update`; a command's response, which carries the same update under
`split`; `bootstrap`; and — so the window still works with the flag off — a classic whole-state snapshot, which is
answered with a bootstrap. That last path goes with the classic shape.

**View state is the seat's.** The core computes the seat's `view` (a person and their agents share one seat, so they
still share it); the window does not hold a second copy or derive it from the replica. The record panel, board and list
read `view`. The replica (`data`, `feed`) is the org's.

**⌘K reads nothing shared.** It sends `search` and keeps the answer in its own state, so the Deals page underneath is
exactly where the person left it. The held query/kind/page and the restore on the way out — the design smell this order
exists to end — are gone, and a test fails if `Search.tsx` ever sends `find` again.

**Home and Inbox get their cross-record data**, each from a stateless envelope (`useRead`: the answer lives in the
component that asked, is re-asked when the watermark moves, and a slow answer never overwrites a newer one):

| Page | Gets | From |
|---|---|---|
| Home | the open pipeline per currency, core-formatted, never summed | `derived.pipeline` (gap 3) |
| Home | next steps across every record, soonest first, each openable | `tasks` (gap 1) |
| Inbox | every timeline entry and stage change, filter by who and what, paged | `feed` (gap 2); the unfiltered first page is the snapshot's own and stays live |

The Inbox's filters and page are the page's own state. Home falls back to the three `due` counts if the core's list has
nothing to show, so it is never emptier than the header above it.

**The preview harness speaks the split shape** (`previewSplit.ts`): the fake core projects its world into `bootstrap`,
`update` (with a real `deal` + `move` row action when a card moves) and the three reads, so the window is exercised
through the same hook and reducer it ships with. It is a mock and says so: `tasks` covers only the open record,
`feed` only each deal's last move and that record's timeline.

**Not yet on screen, deliberately** — the shape carries them, no page asks yet: the `find` filters (gap 5) have no
control; the richer row facts (gap 11 — `updatedAt`, a company's deal count and open value, a contact's company handle)
are in `list.rows` and no table column shows them; `me` is in the snapshot and attributions still say "You"; a task's
`sentAt` could let Send-again pick one reminder. Each is a window order, not a contract gap.

## W1 B1 — the record panel collapses

The panel has a collapse control in its header and `⌘\` (Ctrl-`\`). Collapsed, it is a 32px rail that still names the open
record (running down the rail) and carries a violet dot when an agent touched that record while the person was not looking;
clicking the rail, or `⌘\`, expands it and clears the dot. Collapsed state is window chrome — `localStorage`, wrapped and
allowed to fail like the theme (`panel.ts`) — never in the snapshot, and nothing an agent does expands the panel: the
person's choice wins and the dot is how they hear about it.

**What counts as "touched".** The panel is where the person edits, so anything in the open record's content that changes
while it is shut (fields, timeline, next steps) came from outside the window. A stage change counts only when an agent made
it — dragging a card is the one thing the person can do with the panel shut. A different record is a new baseline, not a
change. Limit: the snapshot does not say who set a *field*, so a field change is read as an agent's; with the panel shut
nobody else can have made it.

B2 (the feed) and B3 (follow) wait on `w1-backend`.

## W1 B2 + B3 — every agent step, on screen

Source: [`work-orders/w1-watchable.md`](work-orders/w1-watchable.md) and backend `01194f2`. Read from the snapshot as
`view.feed`, `view.follow`, `view.page` (folded into `Snapshot.agentFeed`, `.follow`, `.page`).

**Agent activity** (desk, `Desk.tsx`). One line per call that reached the app, newest first, reads and refusals included:
the agent's disc and name in its tint, the command **as it typed it, in handles** (mono), the time ("now", "12s ago"), and —
for a refusal — the sentence the agent saw, verbatim, in the error style with `role="alert"`. A line with a `target` is a
button that opens it (a person's click, so a window `show` is right there); a line about nothing (`board`, `due`) is not.
An arrival is lit for about two seconds; under `prefers-reduced-motion` the fade becomes a still wash for the same two
seconds. The footer says *Since the app started* — the feed is not persisted. The strip is the pitch, so it is first in the desk.

**Follow agent** (a labelled checkbox beside it) is `{ cmd: "follow", on }`, the person's own control; the core refuses it
from an agent and it has no CLI verb. **The core does the following** — it moves the focus without going through `show`, so
the agent is never told about its own action — and the window sends nothing to follow: it reads `view.page` and changes only
what it is *showing*. `follow.ts` has a test that fails if it ever calls `show`.

**Never out of unsaved work.** Every control that holds a draft (inline field and title edits, the note and next-step
composers, the link form, New deal/company/contact) reports to `dirty.ts`. A new page instruction while any is dirty is
**held**, a chip says *Agent opened orbit-line-monitoring — go*, and the record panel stays on the record the draft belongs
to — otherwise a half-typed note would be submitted against whatever the agent opened. The hold lifts when the person clicks
the chip, or the moment nothing is unsaved. A focus change to exactly the record the person themselves just asked for is
theirs and is never held. A collapsed panel is never expanded by follow (B1): the rail's name follows, and the dot sets.

**What lights up** (`fresh.ts`), in the agent's own tint, for ~2 seconds: the timeline entry it logged, the next step it added
or completed, the field it set — and the board card it moved (the existing ring). A person's own write is not lit. A timeline
entry and a next step carry `by`; a field set and a step completed carry nothing, so they are credited to the newest agent
`set`/`done` call on that record in the last 10 seconds. Limit: the core does not say who completed a step or set a field, so
attribution there is the feed's, not the record's.

**Preview.** `Agent session` plays the 10-second test at demo speed (`window.__session(ms)` sets the gap): board, show, log,
task, set, a refusal, a move, due, find. The refusal's wording is a stand-in; the real one is the core's. It is a mock — the
real thing needs QA's live run.

## p2 — the account, in the window

Source: [`work-orders/p2-identity.md`](work-orders/p2-identity.md) §6 and [`identity-contract.md`](identity-contract.md).

**Where.** Sign up, sign in, sign out and accept an invite are in **Settings → Account**, and nowhere else: there is no CLI verb
and no envelope an agent could send (the backend pins both). They are Tauri commands called with `invoke` directly
(`account.ts`), not through `run_cmd` — an account is not part of the snapshot. Signed out, the app is exactly what it was: a
line says so, and nothing else on the page is needed to use it.

**Settings → Organisation** (signed in only): the org's name, its members — names only, roles are p4's — and **Create invite**
(optionally tied to an address). The invite's token is shown **once**, in a panel that says so, with Copy and *Done — hide it*;
it lives in component state only and is gone the moment it is dismissed. **Team** lists the same members, and nothing more.

**Secrets.** A password is in an input until the form is submitted and is cleared whether it worked or not. The session token
never reaches the window (`Status` is token-free by design). A refusal is the core's/server's own sentence, shown inline.

**"You" becomes the person** (`identity.ts`). Signed in, every attribution — the disc, the timeline, the Inbox, the agent strip
— says the person's real name, with their initials on the disc. **Signed out it is still "You" and the neutral "Y"**: the core
names the local person after the OS user, and that is not an account, so it is not said. A human with no id (an older build's
record) is the seat's own person; another person's id is "A teammate", because the window is told only who *you* are. **There is
no hue for a person**: M11 settled that colour means an agent did this, so the disc stays neutral and what makes it yours is the
name and the initials. (If you want a real identity colour for people, it needs a palette outside violet, green, amber, red-brown
and the stage blues — a design decision, not a styling one.)

**Insecure.** When the status says `insecure` — plain HTTP to something that is not this machine — a solid full-width bar sits
above every page: *Not secure. You are signed in to … over plain HTTP. Your password and everything you send can be read by
anyone on the network… Use an https:// address, or sign out.* It is `role="alert"` and stays until the person signs out or
fixes the address. Signed out there is nothing to warn about.

**What was and was not tested.** The window is tested against the command contract (`identity-contract.md`) and driven in the
preview harness, whose fake account answers the seven commands in their real shapes (`window.__insecure`, `window.__offline`).
**It was not run against `npm run dev:server`:** this machine has no PostgreSQL (and no Docker), which that script needs.

## W1 — a draft belongs to its record (QA round 8)

QA found a half-written note following the agent to another record and being saved there: the chip held the *page*, but the
record panel underneath swapped, and the typed text travelled with it. **Fixed at the root, not at the symptom.**

- **A draft is told which record it is on** (`dirty.ts`): the note and next-step composers, the link form, and inline field
  and title edits register the record's handle. While one is dirty, the panel *is that record* — derived in render from the
  draft's owner (`shownRecord`), not set by an effect that races the update that moved the focus, so there is no instant at
  which the other record shows. What is typed can only be saved where it was started: the composers are rendered from the
  held record, so the request carries its id (checked in a browser: with the agent on another record, Add sent `log` to the
  original record's id). A New deal/company form is about no record and pins nothing.
- **The panel remounts per record** (`key`), so no draft state can outlive the record it is about.
- **The chip** still says where the agent went; the person goes when ready. If a draft is open, going asks first:
  *You have unsaved text on `kestrel-pilot`. Leaving discards it — it can only be saved there.* **Keep editing / Discard it and
  go.** Nothing is dropped silently.
- **Clicking another row by hand had the same carry-over — it is older than W1.** The panel was not keyed, so a composer's text
  stayed in place while the record changed under it. Every person-initiated `show` (card, row, search, feed, Home) now goes
  through the same guard: asked, then discarded by choice, or kept. Opening the record the draft is already on is not leaving it.
- The hold lifts by itself, and the page the agent chose is taken, the moment nothing is unsaved.

## p2-sec — look before anything leaves the window

Before **any** password or invite is sent (sign in, create account, join with an invite), the window calls `account_check_server`
(`account.ts` → `sendChecked`). Always, every time, before the request.
- **Secure address** → sent straight away, with no confirmation flag.
- **Plain HTTP to something that is not this machine, or an address with `user:pass@` in front of its host** → **nothing is sent.**
  A warning names the host the request would *really* go to (`evil.example`, not the part of the address a person reads first),
  says in words what is wrong, and offers **Cancel** (first, focused) and **Send it anyway**. Only that second button passes
  `insecureConfirmed: true`, and only for an address that needs it; a yes never widens what is sent to a safe address.
- **An address that cannot be read** → nothing is sent, even on a yes.
- **The core's own refusal** ("check it with account_check_server and ask the person first") is a note to the window, never to the
  person: if it ever got this far, the person is told only "Nothing was sent: that server address needs your confirmation first."
  A test pins that the string cannot reach the page.
- The password is held in memory only while the warning is up, and dropped on Cancel.

## p3 — the team, in the window (built against the contract; fixtures pending)

Source: `sync-contract.md` §5.5, §5.8, §7 (on `p3-contract`). **Not merged until the backend's snapshot lands** — the seat's
new keys (`sync`, `members`, `teamAgents`) are added by hand to the p1 fixtures in tests; the backend's regenerated golden
fixtures replace them.

- **Sync indicator** (header): *Online* (quiet), *Syncing…*, *Offline — last heard 2m ago* in the error colour with a note that
  changes are refused, not queued, and *Choose how to join*. `local`, or no `sync` at all, draws nothing.
- **Stale refusal** (`Forms.tsx`, `refusal.ts`): the server's sentence, what the record says now, who changed it and when, and
  that nothing was saved and **what you typed is still in the box**. The refusal's update is folded in (`useSplit`), so the view
  already shows the truth. Controls never clear a draft on refusal: composers clear on success only; inline editors close on
  success only, and after a refusal leaving the box does not quietly retry — Enter does.
- **Teammates**: a teammate's work carries their name and initials from `seat.members`; their agent reads "Ana's Scout" from
  `seat.teamAgents`; an unknown person is "A teammate", never an id. A teammate's note or next step on the open record glows for
  ~2 seconds in neutral ink (a person has no hue, M11). A teammate's field edit is not attributable from the snapshot, so it does
  not glow.
- **Join choice** (`Sync.tsx`): while `sync.state` is `choosing`, a panel shows what is on this machine and in the organisation and
  offers *Bring my records* or *Start fresh*; either way the local file is left as it is. After *bring*, any handles the server
  changed are listed. **Assumed, not specified:** the shape of `sync_join_plan`'s counts and of `sync_join`'s rename report.
