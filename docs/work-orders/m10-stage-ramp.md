# Work order — M10: the stage ramp, and the tints that are too quiet

**Owner:** frontend · **Branch:** `m10-ramp`, from `main` · **Runs alongside** M4

Read [`m9-visual-system.md`](m9-visual-system.md) §1 — the ruling there stands unchanged — and
your own [`design-review-2026-09.md`](../design-review-2026-09.md).

---

## Why there is a second order

M9 landed everything on its list except the item the memo called first and largest: *"give the
pipeline stages distinct semantic hues… this alone answers most of 'looks generated'."* It was
neither shipped nor proposed, and M9's own instruction was to propose it and wait.

So the window now has a second signal — chips, discs, glyphs — but still nothing that says
*where a deal is* without reading the text. That is the gap this closes.

## 1. The ramp — propose first, and stop

**Do not ship values before I approve them.** That gate exists because the palette is exactly
where M8 went wrong, and approving four hex values costs me minutes where another round of
rework costs a milestone.

Constraints, unchanged from M9 §1:

- **The four open stages only** — lead, qualified, proposal, negotiation. One cool,
  low-saturation family stepping in value, so it reads as *position in a sequence*.
- **Won and Lost are statuses, not stages.** They keep `--won` and `--lost` untouched.
- **Nothing lands near `--due`.** Overdue must stay the one thing that catches the eye.
- Rendered as a **left-edge stripe on the card** plus the stage badge fill.

Propose eight values (four stages × two themes) with `npm run contrast` output for each, on
`--surface` and `--surface-2`, not only on `--ground`. That last part is the mistake the script
caught in my own tokens; do not repeat it.

## 2. The agent tints are below the graphic minimum on dark

You disclosed it yourself: the five `agentTint` values measure **2.19–3.12:1** as a shape on the
dark surface, against a 3:1 floor. The initial carries identity there, and the move-ring has the
same weakness — which matters more, because the ring is how a person sees what an agent just
did, and it is the product's one unique moment.

`agentTint` lives in clappkit and is shared by every clapp, so **do not change it**. Fix it at
our end: a ring treatment that reads at 3:1 on our dark ground regardless of tint — a heavier
stroke, a paired outline, or the tint over a fixed backing. Your call, measured.

## Acceptance

- [ ] ramp proposed with contrast on `--ground`, `--surface` and `--surface-2`, both themes —
      **PM approval recorded before the values ship**
- [ ] no stage hue within reach of `--due`; `--won` and `--lost` unchanged
- [ ] stripe and badge render on board cards, both themes
- [ ] the move-ring measures ≥3:1 on dark for all five tints, with the numbers in the report
- [ ] `npm run contrast` reports zero required pairings failing
- [ ] `npm test`, `npx tsc --noEmit`, `npm run verify` green
- [ ] screenshots of the board in both themes

## Do not

- **Do not edit `clappkit/`** — `agentTint` is shared.
- **Do not touch `src-tauri/`.** M4 is in that tree this round.
- **Do not ship the ramp before approval.**
- **Do not relax an M8 prohibition.** If one genuinely blocks the result, name it and stop.
