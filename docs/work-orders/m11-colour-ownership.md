# Work order — M11: colour belongs to agents

**Owner:** frontend · **Branch:** `m11-colour`, from `main`
**Decided by:** the product owner, 2026-09-24, choosing direction **A · Instrument** with
**C · Two Desks**' agent panel · **Source:** `design_ideas/`

---

## The rule

**Colour means an agent did this. Everything else earns its meaning another way.**

That is the whole order. The rest is consequence.

## Why, and what I got wrong

The design review said nothing on screen carried meaning through colour. I answered by
colouring **stages** — and stage already has the strongest encoding available on a board:
**column position**. The M10 ramp was telling people something the layout had already told
them, using the budget that should have gone to the one thing with no other encoding: **who
acted**.

M10 was good work against a wrong brief. The ramp is not deleted; it is demoted to where
position cannot speak.

## 1. Where stage colour lives now

| Surface | Stage | Why |
|---|---|---|
| **Board** | **no stripe** — the column is the stage | Position already says it. The card's left edge is now agent signal. |
| **Table / list** | low-chroma badge, the M10 ramp | No column to say it. |
| **Record detail** | low-chroma badge | Same. |

Keep the M10 values for the badge — they are measured and approved. Drop their chroma if they
compete with the agent hue anywhere they co-appear; re-measure if you do.

## 2. The agent hue is violet, and amber is not available

Direction A uses amber for agents. **`--due` is already amber** (`#9A6415` light, `#D3A76A`
dark). One hue cannot mean both "an agent touched this" and "this is overdue" — that is the
collision ruled against in M9 and M10, and it would be the third time.

**Violet is free**: green is `--accent` and `--won`, red-brown is `--lost`, amber is `--due`,
blue was the retired ramp. Direction C reserves violet for agents already, so taking A's rule
with C's hue is one coherent decision rather than a compromise.

Propose `--agent` and `--agent-weak` for both themes, measured, **and stop for approval** —
same gate as M10, same reason.

## 3. `agentTint` — one hue says *an agent*, the tint says *which*

`clappkit/web/index.ts` ships five tints: `#45548C` blue, `#267369` green, `#784D82` violet,
`#996138` amber, `#4D6178` slate. **Two of those now collide with meanings we have assigned** —
`#267369` against `--won`/`--accent`, `#996138` against `--due`. A green agent disc beside a
won deal is the bug this order exists to prevent.

`agentTint` is clappkit's and shared by every clapp, so **do not edit it**. Solve it at our end,
as M10 solved the ring:

- **Keep clappkit's hash, not its palette.** Same djb2 over the agent id — so the same agent
  keeps the same slot forever, which is the property that matters — mapped to **our own five
  violet-family tints**.
- Five distinguishable violets, each ≥3:1 as a shape on both grounds, and the initials on them
  ≥4.5:1.
- The move-ring keeps M10's solution and takes its colour from the new family.

Document the divergence in `brand.md`: we use clappkit's hash and our own palette, deliberately,
because our semantic colours claim hues its palette also uses.

## 4. C's agent desk — the panel only

A persistent side panel: what each agent is doing now, anything awaiting approval, and the live
log of agent moves. Reachable from every page.

**Do not adopt C's split timelines.** Separating agent work into its own lane argues against the
product: both parties act on **one** surface, and dividing the record into a people lane and an
agent lane is the opposite of co-presence. The panel answers oversight; the shared timeline stays
shared.

## Not in this order

**Direction A's full information architecture** — nine pages, the sidebar, Reports, Team,
Settings. Most of it needs data that does not exist yet: Reports and Team are meaningless with
one user. That lands after the platform split, as its own order.

## Acceptance

- [ ] no stage stripe on board cards; the left edge carries agent signal
- [ ] stage badge remains in table and record detail, measured
- [ ] `--agent` proposed with contrast on `--ground`, `--surface` and `--surface-2`, both
      themes — **approved by the PM before it ships**
- [ ] nothing in the agent family lands near `--due`, `--won` or `--lost`; report the ΔE
- [ ] five violet-family tints from clappkit's own hash; same id → same tint, pinned by a test
- [ ] move-ring ≥3:1 on dark for every new tint
- [ ] agent desk panel present on every page, no split timelines
- [ ] `npm run contrast` zero required failures; `npm test`, `tsc`, `npm run verify` green
- [ ] screenshots, both themes, board and record

## Do not

- **Do not edit `clappkit/`.**
- **Do not use amber for agents.**
- **Do not split the timeline into lanes.**
- **Do not build the nine-page IA.** That waits for the platform.
- **Do not ship the agent hue before approval.**
