// Which of our five agent tints an agent gets.
//
// **Clappkit's hash, our palette.** `agentTint(id)` is djb2 over the immutable agent id, mapped
// into clappkit's five colours — shared by every clapp and not ours to change. Two of those
// colours are hues this app has spent on meaning (`#267369` is `--won`, `#996138` is `--due`),
// so the colour it returns is never drawn. What we keep is the *slot*: the position of that
// colour in clappkit's palette. Same id, same slot, forever — the property that matters — and
// the slot indexes `--agent-1..5` in `styles.css`, which are our own violets.
//
// The palette below is a copy of clappkit's, because the slot is only meaningful against it.
// `tints.test.ts` reads `clappkit/web/index.ts` and fails if the two ever differ.

import { agentTint } from "./bridge";

export const CLAPPKIT_TINTS = ["#45548C", "#267369", "#784D82", "#996138", "#4D6178"] as const;

/** 1..5, matching `--agent-1..5`. */
export type TintSlot = 1 | 2 | 3 | 4 | 5;

export function tintSlot(id: string): TintSlot {
  const at = CLAPPKIT_TINTS.indexOf(agentTint(id) as (typeof CLAPPKIT_TINTS)[number]);
  // -1 can only mean clappkit changed its palette under us; the test guards that, so this is
  // a floor rather than a state.
  return ((at < 0 ? 0 : at) + 1) as TintSlot;
}

/** The CSS colour for an agent — a custom property, so it follows the theme. */
export function tintVar(id: string): string {
  return `var(--agent-${tintSlot(id)})`;
}
