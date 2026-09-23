# The window is correct and it looks like nobody made it

**Design memo — m7-editing / m8-visual**

- **From:** Frontend (clapp window)
- **To:** Berk, PM
- **Re:** Self-audit of the M7/M8 window + comparison against Attio, Linear, Pipedrive, folk
- **Date:** 2026-09-23

---

## 01. Why this memo exists

Every M8 token rule and M7 control has been implemented as written, and the tests are green. But looking at it cold, it reads as a component-library default, not a product someone designed. This memo names the specific reasons, checked against how four real CRMs actually render the same problems, and sets a short list of fixes that fit inside the existing constraints — no shadows, no radius over 6px, no gradients. Flat isn't the problem. Flat with nothing else carrying weight is.

## 02. What's on screen right now

Three screenshots from the preview harness (Pipeline and Long text scenarios, light + dark) — board + open record panel, and the record panel under a hostile long company name. (See the published artifact for the images: https://claude.ai/artifact/Sqg4Awbv8UCuxMJk6ru7oK)

## 03. First impression

One hue does every job. The same teal fills the "Pilot" agent avatar, the focus ring on the record card, the active timeline chip, and the accent border — so it registers as a leftover default accent color rather than a decision. Nothing else on the screen is allowed to carry visual weight: no color differentiates a lead from a negotiation, a person from an unassigned row, or a dollar figure from body text. The eye has nowhere specific to land, which is the actual mechanism behind "looks AI-generated" here — it isn't the flatness itself, it's that flatness plus zero secondary signal reads as unstyled.

### Usability

| Finding | Severity | Fix |
|---|---|---|
| Long company/deal names re-flow the whole record panel (a 6-word legal entity name pushes Value/Stage/Status down and the deal title wraps to two lines above it) | 🔴 Critical | Truncate the title/company at 1–2 lines with ellipsis; put the full string in a `title` attribute or an expand affordance. This is a real fixture-backed case, not a hypothetical. |
| The CLI-hint in an empty stage column (`crm add deal …`) renders as a full bordered code block the same size as a deal card | 🟡 Moderate | Shrink to one quiet monospace line with a copy icon, under a plain-language empty state — it should read as a considered empty state, not a leaked dev affordance. |
| Archive control and the Link affordance are icon-only, low-contrast, ~16–20px — easy to miss given M7's "every verb has a control" rule | 🟡 Moderate | Keep the icon, widen the hit area, add a text label on hover/focus at minimum. |
| Money values sit inline as plain text at the same weight as labels | 🟢 Minor | See §05, recommendation 3. |

### Visual hierarchy

The header bar (wordmark, presence avatars, reminder counts, theme toggle) is one row of same-size, same-weight elements with no grouping — nothing announces itself as primary. The board's column headers are genuinely good: small caps label, count, two totals stacked by currency, matches how Attio treats its column meta. The record panel's label/value pairs are consistent but visually flat top-to-bottom; nothing marks "Negotiation, open, $67,500" as the three facts that matter most on the row.

### Consistency

| Element | Observation |
|---|---|
| Tokens | Applied uniformly — light/dark are structurally identical, spacing rhythm (32px rows, 4px unit) holds everywhere. This is a genuine strength, not a gap. |
| Avatars | Person, agent, and "unassigned" all render as the same generic silhouette glyph, distinguished only by fill color — reads as an un-skinned default Avatar component. |
| Iconography | Timeline entries use one clock/checkmark glyph regardless of activity kind (call, email, meeting, note) — the kind is only legible from the text chip next to it, not the icon. |

### Accessibility

Text contrast looked fine in both themes at a glance (light-on-charcoal and ink-on-white both clear AA by eye); this wasn't re-run through the M8 contrast script this round, so treat that as unverified rather than passing. Click targets on the icon-only controls (archive, link) are small for a pointer-driven desktop app and worth measuring against a 24px minimum.

## 04. How four real CRMs handle the same screen

Checked live at attio.com, linear.app, pipedrive.com, and folk.app — marketing screenshots of the real product UI, not renders.

- **Attio — real logos, not avatars.** Company rows use the actual favicon next to the name; a fit/status column renders as a colored pill (green "Excellent", amber "Medium", red "Low") — color carries meaning the text alone doesn't.
- **Linear — one thing gets elevation.** The whole surface is as flat and dark as this window, except a single floating panel (the AI side-chat) gets the one shadow on the page. Elevation is spent once, deliberately, not banned outright or scattered everywhere.
- **Pipedrive — color is the brand, not decoration.** Its signature green shows up in the sidebar, the CTA, and small progress bars on deal cards. Stage totals sit inside pill-shaped badges rather than plain text. Avatars are real headshots.
- **folk — warm neutral, not stark.** Off-white/cream ground instead of pure white or charcoal; money amounts render as soft filled chips ("$30K"); the activity feed uses a distinct colored icon per source (Slack, Gmail, Zapier) instead of one generic glyph reused everywhere.

Common thread across all four: color and imagery are spent *with intent*, on 2–3 specific things (status, source, identity) — never as ambient decoration, and never withheld entirely. Breksos currently withholds it entirely.

## 05. Priority recommendations

1. **Give the pipeline stages distinct semantic hues.** A small fixed palette, one hue per stage (open stages cool, Won a clear green, Lost a clear red), used as a left-edge stripe on cards and the stage badge fill. This alone answers most of "looks generated," because right now zero elements are allowed to carry meaning through color.
2. **Key avatars by identity, not a generic glyph.** Stable initials-on-color per teammate/agent (or a real photo where one exists), the way every product reviewed does it. No product researched uses a silhouette-in-circle as its default — it's a placeholder state everywhere else, and it's the permanent state here.
3. **Turn money into a chip.** A subtle filled background, still radius ≤ 6px, no shadow — matching Attio's ARR column and folk's amount chips — so figures scan as data rather than blending into label text.
4. **Fix the long-name reflow.** Truncate + full value on hover/focus. This is a shipped-fixture bug, not a taste call — see §03.
5. **Quiet the empty-state CLI hint.** One monospace line, not a full bordered block competing with real cards.

## 06. What's already working

- **Token discipline:** light and dark are structurally identical, not two hand-tuned surfaces — that parity is real engineering, not luck.
- **Density:** 32px rows and tight label/value pairing put this closer to Attio's information-per-pixel than to a bubbly consumer dashboard, which is the right comparison class for this product.
- **Restraint where restraint is correct:** no gradients, no emoji, no centered marketing-page layout leaking into a working tool — the M8 prohibitions were the right call, they're just currently the *only* signal on screen.
- **The CLI-hint concept:** surfacing the equivalent agent command in an empty state is genuinely on-brand for a clapp — the idea is right, only the visual weight in §03 needs fixing.

---

Nothing above asks for a shadow past drag/ring, a radius over 6px, or a gradient. Color, iconography, and truncation get us the rest of the way without touching the M8 rules.

*Scope note: this report covers frontend visual/styling only. It does not address the structural breadth of the app (number of tabs/pages/views) relative to larger CRMs — that's a separate architecture-level conversation between Berk and the PM.*

*Breksos CRM · m7-editing / m8-visual · screenshots via preview harness, Pipeline + Long text scenarios*
