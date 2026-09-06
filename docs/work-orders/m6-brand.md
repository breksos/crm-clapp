# Work order — M6 brand

**Owner:** frontend / design · **Blocks:** M5 packaging · **Branch:** `m6-brand`

Read [`CLAUDE.md`](../../CLAUDE.md), then
[`clappkit/docs/icons.md`](../../clappkit/docs/icons.md) — that document is the standard and
this order does not repeat all of it.

**This runs now, in parallel with the backend's M0 and M1.** It is genuinely independent of
the core, and it is a hard gate at packaging time: an asset that misses its aspect ratio is
refused at `clatch install`, not at build.

## Scope

The mark, the palette, and the three image assets. **Not the window** — that is M3, and it
cannot start until the backend freezes the snapshot shape, or you would be building against a
guess.

## Deliverables

| | |
|---|---|
| `assets/icon.svg` | the editable source — the mark is regenerated, never hand-traced |
| `assets/icon.png` | 1024×1024 RGBA, ≤ 1 MiB |
| `src-tauri/icons/icon.ico` | derived from the same PNG at 16/24/32/48/64/128/256 |
| `assets/banner.png` | 3440×512 (215:32), ≤ 2 MiB |
| `src/styles.css` | design tokens only — colours, type, spacing. No components. |
| `THIRD_PARTY_NOTICES.md` | if the mark comes from a licensed set |
| a short rationale | what the mark is, why, and where the palette came from |

### The icon

Full-bleed tile: the rounded tile fills the whole canvas, corner radius `0.225 × side`
(≈230 px at 1024), only the corners transparent. That is the default for anything with a
coloured ground, and it is Apple's own ratio — the Dock is where the corner is actually seen.
The launcher re-rounds every tile to `0.26 × side` in CSS, so the shelf is uniform by its
doing, not yours.

Icons sit side by side in the library. If one fills its tile and another floats at 70%, the
shelf looks broken — so measure the fill rather than trusting your eye:

```sh
python3 -c "
from PIL import Image
im = Image.open('assets/icon.png').convert('RGBA'); W,H = im.size; b = im.getbbox()
print(f'{100*(b[2]-b[0])//W}% x {100*(b[3]-b[1])//H}%')"   # ~100% for a tile
```

`icon.ico` is **not optional**: `tauri-build` compiles a Windows resource from the first
`.ico` in `bundle.icon` and fails the build without one, even with bundling off. Windows
picks a different size per context, and a scaled-down 256 looks it — hence the seven sizes.
Derive it from the same PNG so the two cannot drift.

### The banner

**The library draws it 128 px tall.** Everything follows from that. A 4 px hairline at 3440
is one pixel at 860 and gone at 128 — so nothing is thinner than **6 px** at full size, and
most strokes are 12–18. Detail you cannot see is worse than empty space: it becomes noise.

**The left 40% is not yours.** The launcher lays a dark scrim there and prints the app's name
over it in white. Keep the motif right of centre; leave the left as ground or quiet texture.

**Draw what the app does, not what it is called.** Somebody scrolling a shelf reads the
picture before the name, and a logo repeated at banner size tells them nothing the icon did
not already say. For this app that means the pipeline itself — cards crossing stages, a deal
advancing — not the letters C, R and M.

**Sample the icon's colours; don't pick new ones.** The two sit inches apart, and a second
palette makes them look like two products.

## Two rules that constrain the mark

**Don't design a logo by hand.** [`icons.md`](../../clappkit/docs/icons.md) §5 is explicit:
use the real mark for a real product, otherwise a permissively licensed one — Lucide (ISC) is
the house choice — credited in `THIRD_PARTY_NOTICES.md`. Never approximate a logo, and never
stand a system symbol in for a brand mark; it is close enough to read as a bug.

**Wear this app's brand, not the template's.** A fork still wearing the template's tokens
reads as unfinished. Replace the tokens in `src/styles.css` outright.

## Open question — answer before you start

**Does Breksos have existing brand assets?** A logo, a defined palette, a typeface, anything
shipped under that name. If so, this work is *applying* an identity and the palette is
sampled from what exists — not invented. If not, say so and propose two directions with a
sentence of reasoning each, and the PM picks.

Do not guess at this one. An invented palette that contradicts a real company's is worse than
a placeholder, and it is the kind of thing nobody notices until it is everywhere.

## Acceptance

- [ ] fill measured and recorded in the rationale (~100% for a tile)
- [ ] the mark reads on light and dark grounds
- [ ] `.ico` re-derived from the final PNG, all seven sizes
- [ ] banner checked **at 128 px tall**, not at full size — nothing thinner than 6 px
- [ ] the left 40% of the banner carries nothing that has to be read
- [ ] the banner's palette is sampled from the icon
- [ ] the mark is real or properly licensed, and credited if required
- [ ] `src/styles.css` carries no template colour

## Do not

- **Build the window.** That is M3, gated on the M1 freeze.
- **Touch anything under `clappkit/`**, or any Rust file.
- **Commit `pkg/` or `*.clapp`.** Both are derived and gitignored.
- **Add `photos` to the manifest yet.** Screenshots need a window to screenshot; they come
  after M3.
