# Work order — M8: make it look like a business tool

**Owner:** frontend · **Branch:** `m8-visual`, from `main` · **Run this before M7**, so the new
controls are built in the corrected system rather than restyled afterwards.

Read [`m3-window.md`](m3-window.md) — its density, layout and prohibition list all stand. The
palette table and the mono rule in it are already updated; this order explains why and adds the
rest.

---

## The problem, stated plainly

Inspected on a real install, the app reads as **a cyberpunk terminal, not a CRM**. Three things
did it, and all three are mine:

1. **A near-black ground with a heavy green hue bias.** `#0E1512` measures **6.9% lightness**.
2. **A full-saturation mint accent** on that ground.
3. **Mono type used as the interface font** — nav, labels, titles, buttons — plus uppercase
   letter-spaced micro-labels nearly everywhere.

Green-on-black in a monospace face is *the* phosphor-terminal signature. Layer on the uppercase
micro-labels and it also lands on the list of tells that make software look machine-generated —
which is the specific thing `m3-window.md` was written to avoid. The order forbade gradients and
`Inter`, then specified a palette and a type system that produced the same impression by another
route.

## What the research says, with numbers

| Guidance | Ours before | Ours now |
|---|---|---|
| Dark base surfaces belong at **8–14% lightness**; below that the contrast is more than the eye handles over a long session ([colorarchive](https://colorarchive.org/guides/dark-mode-palette-guide/)) | **6.9%** | **10.0%** |
| A hue shift of **2–4°** from neutral is already perceptible — that is all a tinted grey needs | far past it: a saturated green-black | a whisper |
| **Desaturate accents 20–30%** for dark grounds, or they vibrate against them | full-saturation mint `#57C4A4` | `#63B69E` |
| Show elevation with **lighter surfaces**, not shadows — a rising overlay per level | one step | ground → surface → surface-2 → border, rising |
| Body text is an **off-white**, never pure `#FFFFFF`; Notion's dark canvas is `#191919` with `#F0EFED` text ([notionanswers](https://notionanswers.com/199/native-notion-dark-mode-and-light-mode-hex-code-colors)) | `#E3EBE7` — fine | `#E8EDEB` |

The replacement dark palette is in [`m3-window.md`](m3-window.md), already measured:

```
ground   #181B1A  L=10.0%      ink    #E8EDEB  14.66:1  AAA
surface  #1F2322  L=12.9%      ink-2  #A8B3B0   8.05:1  AAA
surface-2 #272B2A L=16.1%      ink-3  #7C8784   4.67:1  AA
border   #333937  L=21.2%      accent #63B69E   7.20:1  AAA
                               due    #D3A76A   7.85:1  AAA
                               lost   #D98A7B   6.50:1  AA
```

**The light palette is unchanged.** It was already close to right, and it is the one most people
will work in. Dark was the half that went wrong.

## Typography — the other half, and probably the bigger one

A palette swap alone will not fix this. **Mono as an interface font is the loudest
"developer tool" signal a business app can send**, and it was on nav, labels, titles and buttons.

| Mono is for | The UI face is for |
|---|---|
| money and counts | navigation and the rail |
| dates | field labels |
| handles inside a CLI hint | record and card titles |
| the CLI hint lines themselves | buttons and controls |
| | column headers |

**Uppercase, letter-spaced micro-labels are a seasoning, not a system.** Keep them for kanban
column headers, where they read as a spreadsheet header should. **Field labels in the record
panel become sentence case** — "Company", "Contacts", "Value", not `COMPANY`, `CONTACTS`,
`VALUE`. A form that shouts every label reads as a machine readout.

## Do not undo what is right

The instrument quality is good and it is not what the complaint is about:

- **Keep 32px rows and the density.** Nobody asked for a roomier app; they asked for one that
  does not look like a terminal.
- **Keep the prohibitions** — no gradients, no shadow-on-everything, nothing centred, no emoji,
  6px radius ceiling.
- **Keep the ring, the attribution discs and the semantic three.** They work, and `--due` still
  must never be `--accent`.
- **Keep both themes and the three theme states.**

## Acceptance

- [ ] dark ground measures **8–14% lightness**; every token's contrast recorded in the report
- [ ] elevation rises ground → surface → surface-2 → border, and no shadow substitutes for it
- [ ] mono appears **only** on money, dates, handles-in-hints and CLI hint lines — grep the
      components and list every remaining use
- [ ] record-panel field labels are sentence case; uppercase survives only on column headers
- [ ] the light theme is visually unchanged
- [ ] `npm test`, `npx tsc --noEmit`, `npm run verify` green; the leak guard still passes
- [ ] **screenshots of both themes in the report** — board and an open record, so the next
      judgement is made on the picture, not on the hex values

## Do not

- **Do not touch `src-tauri/`.** This is paint and type.
- **Do not change the light palette.**
- **Do not loosen the density** to make it feel friendlier. That is a different app.
- **Do not add a colour outside the token table.** If the system needs one, say which and why.
