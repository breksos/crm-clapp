# M11 — the agent hue: proposal (not shipped)

**Status: proposed, awaiting PM approval. Nothing below is in `src/styles.css`.** Approve or adjust
and it ships in the same round, with the desk panel and the removal of the board stripe.

Rule from [`m11-colour-ownership.md`](work-orders/m11-colour-ownership.md): *colour means an agent
did this.* Violet, because green is `--accent`/`--won`, red-brown is `--lost` and amber is `--due`.
Direction C already reserves violet for agents; Direction A's rule with C's hue.

## The proposal

**One hue says *an agent*; five tints say *which*.**

| Token | Light | Dark | Role |
|---|---|---|---|
| `--agent` | `#7e54af` | `#b182e7` | the card's left edge, agent labels, agent text |
| `--agent-weak` | `#f0eaf5` | `#312e3a` | the strip behind "Nova is reading…" |
| `--agent-ink` | `#ffffff` | `#181b1a` | initials on a tint |
| `--agent-1` | `#6a56ba` | `#9c86f7` | tint slot 1 |
| `--agent-2` | `#4b3594` | `#897db2` | tint slot 2 |
| `--agent-3` | `#a554a8` | `#cfb2ff` | tint slot 3 |
| `--agent-4` | `#5c335c` | `#dd84e3` | tint slot 4 |
| `--agent-5` | `#853285` | `#bd62b7` | tint slot 5 |

Swatches, both themes: `~/Documents/m11-shots/agent-light.png`, `agent-dark.png`.

The five tints are a **per-theme** set, not one palette. A single hex cannot serve both: white
initials need it ≤0.183 luminance while a 3:1 shape on the dark column needs ≥0.171, which leaves
a window too narrow to hold five distinguishable violets. So each theme has its own five, chosen
from the same slot — slot *n* in light and slot *n* in dark are the same agent.

## Contrast, from `scripts/contrast.py`

`--agent` is used as text and as a shape, so it must clear **4.5:1 on `--ground`, `--surface` and
`--surface-2`**. Each tint must clear **3:1 as a shape on all three**, and its initials **4.5:1**.
Reproduce: add the tokens above to a copy of `src/styles.css` (light into the bare `:root`, dark
into both dark blocks) and run `python3 scripts/contrast.py <copy>`. Excerpt, unedited:

```
== light
  -- agent: the hue that means 'an agent did this' (text and shape on every ground)
  ok   agent on ground      5.16  (need 4.5)
  ok   agent on surface     5.56  (need 4.5)
  ok   agent on surface-2   4.91  (need 4.5)
  ok   agent on agent-weak   4.71  (need 4.5)
  ok   ink on agent-weak  14.20  (need 4.5)
  ok      agent #7e54af  nearest semantic: --lost  18.3 dE  (need 12)
  ok    agent-1 #6a56ba  nearest semantic: --won  20.8 dE  (need 12)
  ok    agent-2 #4b3594  nearest semantic: --won  21.0 dE  (need 12)
  ok    agent-3 #a554a8  nearest semantic: --lost  15.6 dE  (need 12)
  ok    agent-4 #5c335c  nearest semantic: --won  18.4 dE  (need 12)
  ok    agent-5 #853285  nearest semantic: --lost  16.3 dE  (need 12)
  ok   agent-1 #6a56ba:  5.33 ground  5.75 surface  5.08 surface-2 (need 3.0);  initials  5.75 (need 4.5)
  ok   agent-2 #4b3594:  8.71 ground  9.39 surface  8.30 surface-2 (need 3.0);  initials  9.39 (need 4.5)
  ok   agent-3 #a554a8:  4.39 ground  4.74 surface  4.18 surface-2 (need 3.0);  initials  4.74 (need 4.5)
  ok   agent-4 #5c335c:  9.35 ground 10.08 surface  8.90 surface-2 (need 3.0);  initials 10.08 (need 4.5)
  ok   agent-5 #853285:  6.97 ground  7.52 surface  6.64 surface-2 (need 3.0);  initials  7.52 (need 4.5)
  ok   closest pair of the five tints: 10.0 dE  (need 8)
  -- move-ring: the tint alone, on each ground it can sit on
  ok   ring agent-1 #6a56ba:  5.33 ground  5.75 surface  5.08 surface-2  (need 3.0)
  ok   ring agent-2 #4b3594:  8.71 ground  9.39 surface  8.30 surface-2  (need 3.0)
  ok   ring agent-3 #a554a8:  4.39 ground  4.74 surface  4.18 surface-2  (need 3.0)
  ok   ring agent-4 #5c335c:  9.35 ground 10.08 surface  8.90 surface-2  (need 3.0)
  ok   ring agent-5 #853285:  6.97 ground  7.52 surface  6.64 surface-2  (need 3.0)

== dark
  -- agent: the hue that means 'an agent did this' (text and shape on every ground)
  ok   agent on ground      5.96  (need 4.5)
  ok   agent on surface     5.46  (need 4.5)
  ok   agent on surface-2   4.93  (need 4.5)
  ok   agent on agent-weak   4.56  (need 4.5)
  ok   ink on agent-weak  11.21  (need 4.5)
  ok      agent #b182e7  nearest semantic: --lost  17.7 dE  (need 12)
  ok    agent-1 #9c86f7  nearest semantic: --lost  20.7 dE  (need 12)
  ok    agent-2 #897db2  nearest semantic: --lost  16.3 dE  (need 12)
  ok    agent-3 #cfb2ff  nearest semantic: --lost  18.2 dE  (need 12)
  ok    agent-4 #dd84e3  nearest semantic: --lost  15.6 dE  (need 12)
  ok    agent-5 #bd62b7  nearest semantic: --lost  16.2 dE  (need 12)
  ok   agent-1 #9c86f7:  5.92 ground  5.42 surface  4.89 surface-2 (need 3.0);  initials  5.92 (need 4.5)
  ok   agent-2 #897db2:  4.66 ground  4.27 surface  3.85 surface-2 (need 3.0);  initials  4.66 (need 4.5)
  ok   agent-3 #cfb2ff:  9.47 ground  8.67 surface  7.82 surface-2 (need 3.0);  initials  9.47 (need 4.5)
  ok   agent-4 #dd84e3:  7.01 ground  6.42 surface  5.79 surface-2 (need 3.0);  initials  7.01 (need 4.5)
  ok   agent-5 #bd62b7:  4.62 ground  4.23 surface  3.82 surface-2 (need 3.0);  initials  4.62 (need 4.5)
  ok   closest pair of the five tints: 10.4 dE  (need 8)
  -- move-ring: the tint alone, on each ground it can sit on
  ok   ring agent-1 #9c86f7:  5.92 ground  5.42 surface  4.89 surface-2  (need 3.0)
  ok   ring agent-2 #897db2:  4.66 ground  4.27 surface  3.85 surface-2  (need 3.0)
  ok   ring agent-3 #cfb2ff:  9.47 ground  8.67 surface  7.82 surface-2  (need 3.0)
  ok   ring agent-4 #dd84e3:  7.01 ground  6.42 surface  5.79 surface-2  (need 3.0)
  ok   ring agent-5 #bd62b7:  4.62 ground  4.23 surface  3.82 surface-2  (need 3.0)

0 required pairing(s) failing
0 required pairing(s) failing
```

## Distance from the semantic colours

ΔE is OKLab ×100 (~2 just noticeable; 10+ plainly different). The script requires **≥12** from
`--due`, `--won`, `--lost` and `--accent`; the nearest, in each theme:

| | Light | Dark |
|---|---|---|
| `--agent` | `#7e54af` — `--lost` 18.3 | `#b182e7` — `--lost` 17.7 |
| tint 1 | `#6a56ba` — `--won` 20.8 | `#9c86f7` — `--lost` 20.7 |
| tint 2 | `#4b3594` — `--won` 21.0 | `#897db2` — `--lost` 16.3 |
| tint 3 | `#a554a8` — `--lost` 15.6 | `#cfb2ff` — `--lost` 18.2 |
| tint 4 | `#5c335c` — `--won` 18.4 | `#dd84e3` — `--lost` 15.6 |
| tint 5 | `#853285` — `--lost` 16.3 | `#bd62b7` — `--lost` 16.2 |

Nothing is near `--due` (amber is on the far side of the wheel). `--lost` is the nearest semantic
throughout, because the orchid end of the family drifts toward red; that is the pair to watch.

## Decisions inside the proposal you should check

1. **Distinguishable, not identical to the eye.** Five violets is a narrow palette. The closest
   pair of tints is **10.0 ΔE (light) and 10.4 (dark)**, so two agents can look related. They also
   carry an initial. The family spans indigo → orchid to get that spread; if you'd rather it stay
   closer to pure violet, the price is a smaller gap between slots.
2. **The ring's ink edge is retired.** M10 paired the tint with an ink border because clappkit's
   tints measured 1.98–2.81:1 on dark. The new tints clear 3:1 alone on every ground (lowest:
   `#bd62b7` at 3.82 on the dark column, `#a554a8` at 4.18 on the light one), and an ink edge beside
   a light violet is *worse* (1.55–3.17) than the tint alone. So the ring is the tint, one stroke.
   Say so if you would rather keep the edge as a belt-and-braces.
3. **Left edge = `--agent`, disc = tint.** The stripe says "an agent touched this" the same for
   every agent; the disc says which. Board stripe for stage is removed (position says it).
4. **Keeping clappkit's hash, not its palette.** The slot is `AGENT_TINTS.indexOf(agentTint(id))`
   over clappkit's five hexes, so the same id keeps the same slot; a test pins the mapping and
   fails if clappkit's palette ever changes under us. `clappkit/` is untouched.
5. **The stage badge stays** on the table and record, at its M10 values: fills are `-weak`
   near-greys, so nothing there competes with the violet. `--agent` is 9.5 ΔE from the nearest ramp
   stripe (light) but the stripes no longer render on the board.
6. **Amber is untouched.** `--due` is unchanged and reserved.
