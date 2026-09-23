# Work order — M9: give the window something to say

**Owner:** frontend · **Branch:** `m9-visual`, from `main`
**Source:** [`docs/design-review-2026-09.md`](../design-review-2026-09.md) — your own memo

Read that memo first. This order accepts it, and adds the three rulings it correctly left to
the PM.

---

## Why this exists

The memo's diagnosis is right and it names my error: *"it isn't the flatness itself, it's that
flatness plus zero secondary signal reads as unstyled."*

M8 banned the tells — no gradients, no shadows, ≤6px radius — and specified **nothing positive
to replace them**. Forbidding what makes software look generated does not make it look
designed. The prohibitions stay; they are simply not allowed to be the only thing on screen.

Every recommendation below fits inside the M8 rules. No shadow past drag and ring, no radius
over 6px, no gradient.

## The three rulings the memo left open

### 1. Stage hues must not collide with the semantic three

The memo asks for a hue per stage. Granted — with a constraint it did not have, because
`--won`, `--lost` and `--due` already carry meaning and must keep it.

- **The four open stages get a cool, low-saturation ramp** — one family, stepping in value as a
  deal advances. They read as *position in a sequence*, not as status.
- **Won and Lost are not stages and do not join the ramp.** They keep `--won` and `--lost`
  exactly as they are.
- **`--due` stays reserved.** Nothing in the stage ramp may land near it, or overdue stops
  being the one thing that catches the eye.

Render the stage as a **left-edge stripe on the card** plus the stage badge fill. Propose the
four values with their contrast measured on both grounds; I will approve them before they ship.

### 2. Identity avatars use clappkit's existing tint

Do **not** invent a second tinting scheme. `agentTint(id)` is already in `@clappkit`, already
keyed on the immutable agent id, and already what the move-ring uses. Initials on that tint;
a real avatar where the roster provides one. One identity colour per actor, everywhere they
appear — ring, disc, timeline.

The memo is right that a silhouette-in-circle is a placeholder state in every product it
reviewed and the permanent state here.

### 3. Money chips — approved as described

Subtle filled background, radius ≤6px, no shadow, tabular numerals unchanged.

## The rest of the memo, accepted as written

| | |
|---|---|
| 🔴 **Long-name reflow** | A six-word company name pushes Value/Stage/Status down and wraps the title. Truncate to 1–2 lines with the full string reachable on hover/focus. **This is a fixture-backed bug, not a taste call — fix it first.** |
| 🟡 **The CLI hint block** | One quiet monospace line under a plain-language empty state, not a bordered block the size of a deal card. The concept is right; only its weight is wrong. |
| 🟡 **Icon-only controls** | Archive and Link are ~16–20px and low contrast. Widen the hit area to **at least 24px** and give them a text label on hover and focus. |
| 🟢 **Activity icons** | One glyph for call, email, meeting and note. Give each kind its own Lucide glyph — the kind should be legible without reading the chip. |
| 🟢 **Header grouping** | One row of same-weight elements with nothing primary. Group it: identity, then presence, then the reminder counts, then controls. |

## Not in this order

The memo scopes itself to visual/styling and says so. **Structural breadth — more tabs, pages
and views — is not yours to add here.** That conversation is settled in
[`p0-platform-spec.md`](p0-platform-spec.md) and comes after the platform split.

## Acceptance

- [ ] the long-name case renders without reflowing the panel, at both themes
- [ ] stage ramp proposed with measured contrast, approved by the PM, then shipped
- [ ] no stage hue sits near `--due`; `--won` and `--lost` unchanged
- [ ] avatars are initials on `agentTint`, keyed on id, consistent across ring, disc and timeline
- [ ] money renders as a chip; figures still line up
- [ ] icon-only controls ≥24px with a label on hover/focus
- [ ] **contrast re-run through the M8 script** — the memo flagged this as unverified, so verify it
- [ ] no gradient, no shadow outside drag/ring, no radius over 6px, no emoji
- [ ] `npm test`, `npx tsc --noEmit`, `npm run verify` green; leak guard still passes
- [ ] screenshots of both themes in the report

## Do not

- **Do not touch `src-tauri/`.**
- **Do not add tabs, pages or views.**
- **Do not relax an M8 prohibition to get a result.** If one genuinely blocks a fix, say which
  and why, and stop.
