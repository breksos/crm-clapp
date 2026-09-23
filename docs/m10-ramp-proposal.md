# M10 — the stage ramp: proposal (not shipped)

**Status: proposed, awaiting PM approval. Nothing below is in `src/styles.css`.** Approve or
adjust and the tokens ship in the same round, with the stripe and badge on the board cards.

Rule from [`m9-visual-system.md`](work-orders/m9-visual-system.md) §1: the four open stages only,
one cool low-saturation family that steps in value as a deal advances. Won and Lost keep
`--won` and `--lost`. Nothing lands near `--due`. Rendered as a left-edge stripe on the card
plus the stage badge fill.

## The eight values

Hue about 230° in both themes. Light steps *darker* as a deal advances; dark steps *brighter*.

**Light** (`:root`)

| Stage | `--stage-*` (stripe) | `--stage-*-weak` (badge fill) |
|---|---|---|
| lead | `#7b85b5` | `#eaebf3` |
| qualified | `#687199` | `#e7e8ef` |
| proposal | `#585f81` | `#e4e5eb` |
| negotiation | `#494f6c` | `#e2e3e7` |

**Dark** (`:root[data-theme="dark"]` and the `prefers-color-scheme` block)

| Stage | `--stage-*` (stripe) | `--stage-*-weak` (badge fill) |
|---|---|---|
| lead | `#6f789f` | `#31363d` |
| qualified | `#828eba` | `#353b43` |
| proposal | `#95a2d5` | `#393f49` |
| negotiation | `#a7b6ef` | `#3d434f` |

Badge text is `--ink`, on the `-weak` fill.

## Contrast, from `scripts/contrast.py`

Stripe needs ≥3:1 (graphic) on every ground it can touch: `--ground`, `--surface` **and**
`--surface-2` — the card sits on the column (`--surface-2`), so that is the pairing that
matters most. Badge text needs ≥4.5:1.

Reproduce: add the eight `--stage-*` pairs above to a copy of `src/styles.css` (light into the
bare `:root` block, dark into both dark blocks) and run
`python3 scripts/contrast.py <copy>`. Raw output, unedited:

```
== light
  ok         stage-lead stripe:  3.32 ground   3.58 surface   3.16 surface-2  (need 3.0)
  ok    stage-lead-weak badge:  14.11 ink on the fill  (need 4.5)
  ok   stage-negotiation stripe:  7.44 ground   8.02 surface   7.08 surface-2  (need 3.0)
  ok   stage-negotiation-weak badge:  13.07 ink on the fill  (need 4.5)
  ok     stage-proposal stripe:  5.78 ground   6.23 surface   5.51 surface-2  (need 3.0)
  ok   stage-proposal-weak badge:  13.33 ink on the fill  (need 4.5)
  ok    stage-qualified stripe:  4.41 ground   4.76 surface   4.21 surface-2  (need 3.0)
  ok   stage-qualified-weak badge:  13.72 ink on the fill  (need 4.5)
== dark
  ok         stage-lead stripe:  4.02 ground   3.68 surface   3.32 surface-2  (need 3.0)
  ok    stage-lead-weak badge:  10.28 ink on the fill  (need 4.5)
  ok   stage-negotiation stripe:  8.74 ground   8.00 surface   7.22 surface-2  (need 3.0)
  ok   stage-negotiation-weak badge:   8.39 ink on the fill  (need 4.5)
  ok     stage-proposal stripe:  6.94 ground   6.36 surface   5.74 surface-2  (need 3.0)
  ok   stage-proposal-weak badge:   8.96 ink on the fill  (need 4.5)
  ok    stage-qualified stripe:  5.39 ground   4.94 surface   4.45 surface-2  (need 3.0)
  ok   stage-qualified-weak badge:   9.55 ink on the fill  (need 4.5)
0 required pairing(s) failing
```

## Distance from `--due` (and the other two semantics)

ΔE is OKLab distance ×100 (about 2 is a just-noticeable difference; 10+ is plainly a different
colour). Hue gap is HSV hue, shortest way round.

**Light** — `--due` `#976215`, `--won` `#1c6b57`, `--lost` `#a6503f`

| Stage | Stripe | ΔE to `--due` | hue gap to due | ΔE to `--won` | ΔE to `--lost` |
|---|---|---|---|---|---|
| lead | `#7b85b5` | 19.8 | 166° | 19.4 | 18.9 |
| qualified | `#687199` | 17.0 | 167° | 14.0 | 15.9 |
| proposal | `#585f81` | 16.8 | 166° | 11.0 | 15.7 |
| negotiation | `#494f6c` | 18.8 | 166° | 11.2 | 17.7 |

**Dark** — `--due` `#d3a76a`, `--won` `#63b69e`, `--lost` `#d98a7b`

| Stage | Stripe | ΔE to `--due` | hue gap to due | ΔE to `--won` | ΔE to `--lost` |
|---|---|---|---|---|---|
| lead | `#6f789f` | 23.2 | 166° | 18.0 | 19.1 |
| qualified | `#828eba` | 18.9 | 168° | 13.6 | 15.6 |
| proposal | `#95a2d5` | 17.1 | 167° | 12.7 | 15.2 |
| negotiation | `#a7b6ef` | 17.7 | 167° | 14.9 | 17.4 |

Nothing is near `--due`: the closest stripe is 16.8 ΔE away in light and 17.1 in dark, on the opposite side of the wheel. The closest to
`--won` is 11.0 / 12.7 — a blue against a teal, 64° apart, distinct but the pair worth watching.

## Things you should know before approving

- **Adjacent steps are close** (ΔE 7.1 / 6.4 / 5.9 in light). Intended: position in a sequence, not four statuses. The
  stripe carries the ramp.
- **The badge fills barely differ from each other** — they are tints near grey. The badge says
  "this is a stage label"; the stripe says which one.
- **Lead is the floor.** Light Lead on `--surface-2` is 3.16:1 and dark Lead on `--surface-2` is
  3.32:1. Lighter would fail the graphic minimum.
- **clappkit agent tint `#45548C`** is 5.2 ΔE from the light Negotiation stripe. A disc against
  a stripe, so I would keep both; it is a coincidence worth knowing.
