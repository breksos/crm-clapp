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
