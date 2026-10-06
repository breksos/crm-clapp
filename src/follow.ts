// Following the agent (W1 B3): the window goes where the agent went — unless that would take the
// person out of unsaved work.
//
// **The core does the following; the window only draws it.** With follow on, an agent call that has
// a target moves the shared focus in the core, *without* going through `show`, so no `deal.opened`
// is ever emitted about the agent's own action. The window must not undo that discipline by
// navigating with a `show` of its own — so it never sends a command here. It reads `view.page`
// (where the last agent read pointed) and `view.focus`, and changes only what this window is
// *showing*: `setView`, which is local.
//
// **Never take the person out of unsaved work.** If any control is dirty (`dirty.ts`), the
// navigation is held and a chip says where the agent went; the record panel keeps showing the
// record the draft belongs to, so a half-typed note cannot be submitted against a different
// record. The hold lifts when the person clicks the chip, or the moment they have nothing unsaved.

import { useCallback, useEffect, useRef, useState } from "react";
import type { Focused, Snapshot } from "./bridge.ts";
import { isDirty, onDirtyChange } from "./dirty.ts";
import { idKey } from "./ids.ts";
import type { View } from "./nav.ts";

export type Held = {
  /** The page the agent went to, if it went to one. */
  page: View | null;
  /** What the chip names: the record, or failing that the page. */
  label: string;
};

const PAGE_WORD: Record<View, string> = {
  home: "Home", inbox: "the inbox", pipeline: "the pipeline", deals: "the deals", contacts: "the contacts",
  reports: "Reports", team: "Team", settings: "Settings",
};

/** What to do with the core's page instruction. Pure, so "held while a form is dirty" is a fact
 *  a test can hold rather than something only a browser can show. */
export function decideFollow(a: { seq: number | undefined; seen: number | undefined; follow: boolean; dirty: boolean }): "ignore" | "go" | "hold" {
  if (a.seq === undefined || a.seq === a.seen) return "ignore"; // not a new instruction
  if (!a.follow) return "ignore";
  return a.dirty ? "hold" : "go";
}

/** The focus moved: did the agent take the panel away from a record the person has a draft on? */
export function focusTakenFromDraft(a: { now: string; before: string; dirty: boolean; ownAskedFor: boolean }): boolean {
  return a.now !== "" && a.now !== a.before && a.dirty && !a.ownAskedFor;
}

export const chipText = (h: Held) => `Agent opened ${h.label} — go`;

/** `ownShow`: the record the *person* last asked to open, and when. A focus change to exactly that
 *  record, soon after, is theirs; a change to any other record is not, however close it falls. */
export function useFollowing(
  state: Pick<Snapshot, "page" | "follow" | "focus" | "focused">,
  setView: (v: View) => void,
  ownShow: { current: { at: number; id: string } | null },
) {
  const [held, setHeldState] = useState<Held | null>(null);
  const heldRef = useRef<Held | null>(null);
  const setHeld = useCallback((h: Held | null | ((p: Held | null) => Held | null)) => {
    const next = typeof h === "function" ? h(heldRef.current) : h;
    heldRef.current = next;
    setHeldState(next);
  }, []);
  const [frozen, setFrozen] = useState<Focused | null>(null);
  const seen = useRef<number | undefined>(state.page?.seq);
  const focusKey = useRef<string>(String(state.focus?.handle ?? ""));
  /** The record panel's content as of the render *before* this one — what a draft belongs to. */
  const before = useRef<Focused | null | undefined>(state.focused);

  const release = useCallback(
    (go: boolean) => {
      const h = heldRef.current;
      if (go && h?.page) setView(h.page);
      setHeld(null);
      setFrozen(null);
    },
    [setView, setHeld],
  );

  // A new instruction from the core: go there — or hold, if the person is mid-sentence.
  useEffect(() => {
    const page = state.page;
    if (!page) return;
    const verdict = decideFollow({ seq: page.seq, seen: seen.current, follow: state.follow !== false, dirty: isDirty() });
    seen.current = page.seq;
    if (verdict === "ignore") return;
    if (verdict === "go") return setView(page.name);
    setHeld({ page: page.name, label: state.focus?.handle ? String(state.focus.handle) : PAGE_WORD[page.name] });
  }, [state.page, state.follow, state.focus, setView]);

  // The focus moved under an unsaved draft, and not because the person moved it: keep the panel on
  // the record the draft belongs to, and say where the agent went.
  useEffect(() => {
    const now = String(state.focus?.handle ?? "");
    if (now === focusKey.current) return;
    const was = focusKey.current;
    focusKey.current = now;
    const mine = ownShow.current;
    const theirs = !!mine && Date.now() - mine.at < 5000 && String(idKey(state.focus!.id)) === mine.id;
    if (!focusTakenFromDraft({ now, before: was, dirty: isDirty(), ownAskedFor: theirs })) return;
    setFrozen((f) => f ?? before.current ?? null);
    setHeld((h) => ({ page: h?.page ?? null, label: now }));
  }, [state.focus, ownShow]);

  // Declared after the effect above, so that one reads the previous value before this overwrites it.
  useEffect(() => {
    if (!frozen) before.current = state.focused;
  });

  // The person finished what they were doing: let the agent's navigation through.
  useEffect(() => onDirtyChange(() => (!isDirty() ? release(true) : undefined)), [release]);

  return {
    held,
    /** What the record panel should show: the frozen record while a draft is held. */
    focused: frozen ?? state.focused,
    go: () => release(true),
    dismiss: () => release(false),
  };
}
