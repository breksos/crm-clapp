# Work order — M3 the window

**Owner:** frontend · **Branch:** `m3-window` · **Runs after** M6 brand, alongside M2

Read [`CLAUDE.md`](../../CLAUDE.md), [`docs/architecture.md`](../architecture.md) §6 and §11,
and [`m6-brand.md`](m6-brand.md) — the palette below comes from there and is already decided.

**You can start before M1 lands.** The snapshot shape is frozen in
[`m0-m1-backend.md`](m0-m1-backend.md) and is the contract you build against. Build the whole
window against a **mock snapshot** through the preview harness (below); wiring to the live
core is the last step, not the first.

---

## The thesis

This is a **working instrument**, not a landing page. A salesperson has it open all day
beside their terminal, and their agent is moving things in it while they watch. Two
consequences that drive every decision below:

- **Density is the feature.** Someone wants to see their whole pipeline without scrolling.
  Rows are 32px, not 56px. Research on enterprise tables puts compact at 24–32px and that is
  where a CRM belongs — power users almost always want the densest mode
  ([setproduct](https://www.setproduct.com/blog/data-table-ui-design)).
- **Agent activity must be legible.** When a card moves because the agent moved it, the
  person needs to see that it happened and who did it. This is the one thing our window has
  that no other CRM does; do not bury it.

## Do not build a generic AI app

This is the explicit prohibition list. Every item is a tell, and they arrive by default
unless forbidden:

- **No gradients.** Not in a header, not on a button, not as a "hero".
- **No shadow on everything.** Border and fill do separation. A shadow means *this object is
  genuinely floating* — a drag ghost, a popover — and nothing else.
- **Not everything is a card.** A deal on the board is a card because a deal *is* a card. A
  row in a table is a row. Do not wrap lists, panels or sections in rounded boxes.
- **No radius above 6px.** Cards and inputs are 4–6px. Nothing is a pill except a status
  chip.
- **Nothing is centred.** This is a left-aligned instrument. No centred hero, no centred
  empty state with a big illustration — empty states are one line of plain text.

  > **Corrected 2026-09-22.** This line used to end "and, where useful, the exact CLI verb
  > that would fill them", and that instruction is what left the window read-only: every
  > empty state printed a command instead of offering a control. An empty state leads with
  > **the control that fills it**; the CLI hint may sit beside it, never instead of it. See
  > [`m7-window-editing.md`](m7-window-editing.md) and architecture §10b.
- **No emoji** as icons, bullets, or section markers. Icons are Lucide, 16px, 1.5 stroke,
  matching the mark.
- **No animation** except two: the drag itself, and a 120ms settle on state change. No page
  transitions, no fade-ins, no scroll-triggered reveals. Everything is visible at rest.
- **No `Inter` as an unconsidered default.** Attio uses Inter well, deliberately, inside a
  strong system ([stacksync](https://www.stacksync.com/blog/attio-crm-2025-review-features-pros-cons-pricing)) — but reaching for it *because it is the default* is the tell. We are
  picking a different pair on purpose, below.
- **No "ask the agent" button, anywhere.** Architecture §11. The person reaches their agent
  through Clatch; a button that prompts on their behalf inverts the model. This is not a
  style rule and it is not negotiable.

## Typography

**Bundle the font files. Do not link Google Fonts.** This app is local-first and must render
identically with no network — a `<link>` to `fonts.googleapis.com` silently falls back to
system fonts offline, and Tauri's CSP would need widening to allow it. Ship `.woff2` under
`src/assets/fonts/`, declare `@font-face` with local `src`, and credit the licences in
`THIRD_PARTY_NOTICES.md` beside Lucide.

| Role | Face | Why |
|---|---|---|
| UI | **Geist** | Drawn for interfaces, tight at small sizes, real tabular numerals, OFL. Not the default reach. |
| Numerals, ids, money, code | **Geist Mono** | A true companion — same skeleton, so a money column beside a name column does not look bolted on. |

If Geist is unavailable or fails at 12px, the sanctioned alternative is **Instrument Sans**
+ **IBM Plex Mono**. **Render a real table at 12–13px and look at it before committing** — I
cannot verify rendering from a spec, and a face that reads well at 16px can be mud at 12px.

```
13px / 1.45   body, table cells, card titles
12px / 1.4    metadata, secondary fields, column headers
11px / 1.35   uppercase micro-labels, 0.06em tracking   ← small, bold, all-caps
16px / 1.3    record title
```

Money, counts and dates take `font-variant-numeric: tabular-nums` **always**. A column of
figures that does not line up is a bug.

**Mono is for data, not for chrome — added 2026-09-22.** The mono face belongs on money,
dates, handles and CLI hint lines, and nowhere else. Navigation, field labels, record titles,
card titles and buttons are all the UI face. Mono used as an interface font is the single
loudest "this is a developer tool" signal a business app can send, and it was everywhere in the
first build. See [`m8-visual.md`](m8-visual.md).

## Colour — light and dark, both first-class

Define **semantic tokens once**, then map them per theme. Dark is a *mapping*, not an
inversion: surfaces step **up** in lightness from the ground, never down.

Three theme states, not two — an explicit choice stamps `data-theme` on the root, and the
default "system" setting stamps nothing.

```css
:root { /* complete light palette */ }
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) { /* redefine tokens only */ }
}
:root[data-theme="dark"] { /* redefine again, so the toggle wins both ways */ }
```

Style every component through the tokens. **A colour whose only definition sits inside a
media query or `[data-theme]` block never applies in the un-stamped state** — that is the
classic unreadable-window bug.

> **The dark column was replaced on 2026-09-22.** The values below are current; the reasoning
> and the measurements are in [`m8-visual.md`](m8-visual.md). The first dark set sat at 6.9%
> lightness with a heavily green-biased near-black and a full-saturation mint accent, which is
> the phosphor-terminal look, not a business tool.

| token | light | dark |
|---|---|---|
| `--ground` | `#F4F7F5` | `#181B1A` |
| `--surface` | `#FFFFFF` | `#1F2322` |
| `--surface-2` | `#EDF2EF` | `#272B2A` |
| `--border` | `#DCE5E0` | `#333937` |
| `--border-strong` | `#C3D0CA` | `#454C4A` |
| `--ink` | `#14201C` | `#E8EDEB` |
| `--ink-2` | `#4A5A54` | `#A8B3B0` |
| `--ink-3` | `#74857E` | `#7C8784` |
| `--accent` | `#1C6B57` | `#63B69E` |
| `--accent-weak` | `#E2EFEA` | `#1E2E2A` |
| `--won` | `#1C6B57` | `#63B69E` |
| `--lost` | `#A6503F` | `#D98A7B` |
| `--due` | `#9A6415` | `#D3A76A` |

The neutrals are **biased green**, not pure grey — they belong to this app, not to a UI kit.
`#123B33` is the icon tile ground and does **not** appear in the chrome; the app is paper and
ink with the accent used sparingly.

**`--due` is never `--accent`.** Won sharing a value with the accent is deliberate — a won
deal is the app working. But if overdue is drawn in accent green, the one thing demanding
attention dissolves into the furniture.

**Theme choice is local, not shared state.** It goes in `localStorage`, wrapped in
try/catch, and never enters the snapshot. The agent does not care what theme you are in, and
architecture §6's "shared view state" is about *what is being looked at*, not how it is
painted.

## Layout and density

4px base unit. Everything is a multiple.

```
┌──────────────────────────────────────────────────────────────┐
│ header 44px   Breksos CRM      [agent strip]  [theme] [due 3] │
├────────┬─────────────────────────────────────────────────────┤
│ rail   │  Board · People · Companies                          │
│ 180px  │                                                      │
│        │  ┌── board: columns 280px, gap 12px ──────────────┐ │
│        │  │ LEAD 3 · $45,000  │ QUALIFIED 2 · $80,000 │ …  │ │
│        │  │ ┌─card 4px radius┐│                            │ │
└────────┴──┴──┴───────────────┴┴────────────────────────────┴─┘
```

| | |
|---|---|
| Table row | **32px**, cell padding `0 10px`, 1px bottom border |
| Table header | 28px, `--surface-2`, 11px uppercase micro-label |
| Board column | 280px wide, 12px gap |
| Board card | 10px padding, 6px gap between cards, 4px radius |
| Panel padding | 16px |

**Board cards carry three fields and no more** — title, company, value. Kanban guidance is
consistent that 2–4 fields is where cards stay scannable
([virtosoftware](https://www.virtosoftware.com/pm/kanban-board-example/)). A fourth slot
exists only for the attribution disc, below.

**Column headers carry the stage name, the count, and the currency totals** — straight from
`snapshot.board.columns[].count` and `.totals[]`. Totals are **grouped by currency and never
summed across them**; render two lines rather than one wrong number.

## Component inventory

Build these and nothing else. Each maps to a field the snapshot already carries.

| Component | Reads | Notes |
|---|---|---|
| App shell | — | header, rail, main. Theme toggle lives in the header. |
| Board | `board.columns`, `pipeline.stages` | six columns: four stages, then Won, Lost |
| Deal card | a deal + `by` | three fields + attribution disc |
| Table | `list.rows` | 32px rows, sortable headers |
| Pagination footer | `list.page`, `pageSize`, `total` | **must read "N of TOTAL" identically to the CLI** |
| Record detail | `focus` | deal / contact / company, with its timeline |
| Timeline | activities | each entry shows who wrote it — see below |
| Pending banner | `pending` | the question + its candidates as clickable rows |
| Due indicator | `due` | overdue / today / week, in `--due` |
| Agent strip | `agents` | avatars via clappkit's `useAsset` and `agentTint` |
| Empty states | — | one line of text, plus the CLI verb that would fill it |

### Attribution is the differentiator — render it

Every activity carries `by`: the person, or a specific agent id. Use clappkit's
`useAsset(avatar.path)` for the image and `agentTint(id)` for the monogram fallback — both
already exist in `@clappkit`, do not reimplement them.

**Key every per-agent thing on the `id`, never the display name.** A rename arrives as a
fresh roster snapshot with the same id: update the label in place, never drop and re-create
the row.

When a snapshot arrives showing a deal in a different column than the last one, **ring that
card for 1.2s in the moving agent's tint**. That is the person seeing what their agent just
did, and it is the single most valuable 20 lines in this milestone. Respect
`prefers-reduced-motion`: no ring animation, but still mark the card.

### One piece of honesty the window owes the user

Reminders only fire while the app is running — Clatch ships no scheduler and starts nothing
at boot. **Say so in the UI**, near the due indicator, in plain words. Letting someone
discover this by missing a follow-up is the worst way for them to learn it.

## The preview harness

`src/preview.ts` renders the window in a plain browser against a **fake snapshot**, with no
Rust build and no Clatch. It is how you iterate on the look, and — more importantly — how you
check the states that are hard to reach live:

- an agent-driven move (the tint ring)
- `pending` with three candidates
- a board with one column overfull and another empty
- long company names, a 40-character deal title
- **both themes**, and the un-stamped system default
- zero state: no deals at all

Build this **first**, before the components. It is the difference between designing the
window and waiting on the backend.

## Acceptance

- [ ] both themes complete and legible; every token defined in bare `:root` before any
      media/`[data-theme]` block redefines it
- [ ] the un-stamped system-default state renders correctly in both OS themes
- [ ] fonts bundled locally; the window renders identically with networking off
- [ ] table rows 32px; **the shell never scrolls — the list scrolls inside itself** (23 rows
      fit a 900px window once header, list bar, thead and footer are counted; the "25" in
      the first draft of this order was arithmetic, not measurement)
- [ ] money columns use tabular numerals and line up
- [ ] currency totals grouped, never summed across currencies
- [ ] the pagination footer's wording matches `crm find`'s output exactly
- [ ] an agent-driven move rings the card in that agent's tint, keyed on agent id
- [ ] a rename in the roster relabels in place — the row does not flicker or re-create
- [ ] `prefers-reduced-motion` honoured
- [ ] keyboard focus visible on every interactive element
- [ ] `preview.ts` covers all six states above
- [ ] no gradient, no shadow outside drag/popover, no emoji, no centred layout anywhere
- [ ] `src/styles.css` carries no colour from the M0 placeholder palette

## Look at these first

Two real clapps are installed on this machine — run them before designing anything:

```sh
clatch run com.arfium.maps
clatch run com.arfium.chess
```

There is a house style in this family. Ours wears its own brand, but the *shape* of a clapp
window — what belongs in it and what does not — is best learned from one that exists.

## Do not

- **Touch anything under `clappkit/`**, or any Rust file. If you need a field the snapshot
  does not carry, that is a PM conversation, not a core edit.
- **Change the snapshot shape.** It is frozen and M2 is being built against it too.
- **Add a router, a state library, or a component kit.** React, the `@clappkit` bridge, and
  CSS. `useSnapshot` already handles the `rev` race — do not write a second store around it.
- **Build a second chat surface.** The agent handles conversation.
- **Add a density switcher, a settings page, or a pipeline switcher.** Not in v1.
- **Commit `pkg/`, `dist/` or `*.clapp`.**

---

**Sources consulted for the density and board specifications:**
[setproduct — data table UI design](https://www.setproduct.com/blog/data-table-ui-design) ·
[Attio review, stacksync](https://www.stacksync.com/blog/attio-crm-2025-review-features-pros-cons-pricing) ·
[virtosoftware — kanban board examples](https://www.virtosoftware.com/pm/kanban-board-example/)
