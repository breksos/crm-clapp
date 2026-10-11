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
// **A draft belongs to the record it was started on** (QA round 8). While a record-scoped draft is
// dirty (`dirty.ts`), BOTH the page and the record panel are held: the panel keeps showing the
// record the draft is on, however the focus moved, so what the person has typed can never be saved
// anywhere else. A chip says where the agent went; the person goes when they are ready. The hold
// lifts by itself the moment nothing is unsaved.
//
// The panel's content is *derived* in render from the draft's owner — not set by an effect that
// races the update that moved the focus — so there is no moment at which the other record shows.

import { useCallback, useEffect, useRef, useState } from "react";
import type { Focused, Snapshot } from "./bridge.ts";
import { dirtyOwner, isDirty, onDirtyChange, useDirtyOwner } from "./dirty.ts";
import type { View } from "./nav.ts";

export type Held = {
  /** The page the agent went to, if it went to one and the person was mid-sentence. */
  page: View | null;
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

/**
 * Which record the panel shows. Pure.
 *
 * With an unsaved draft on record `owner`, that is `owner` — taken from what the window has already
 * shown (`seen`) — whatever the focus is now. Otherwise it is the focus. If the owner was never seen
 * (it cannot be: a draft is typed into a record that is on screen) the focus is all there is.
 */
export function shownRecord(owner: string | null, current: Focused | null | undefined, seen: ReadonlyMap<string, Focused>): Focused | null | undefined {
  if (!owner) return current;
  if (current && String(current.row.handle) === owner) return current;
  return seen.get(owner) ?? current;
}

/** Where the agent went, if the panel is being held away from it. */
export function wentTo(owner: string | null, focus: Snapshot["focus"]): string | null {
  if (!owner || !focus?.handle) return null;
  return String(focus.handle) === owner ? null : String(focus.handle);
}

export function chipText(label: string): string {
  return `Agent opened ${label} — go`;
}

export function useFollowing(
  state: Pick<Snapshot, "page" | "follow" | "focus" | "focused">,
  setView: (v: View) => void,
) {
  const owner = useDirtyOwner();
  const [held, setHeldState] = useState<Held | null>(null);
  const heldRef = useRef<Held | null>(null);
  const setHeld = useCallback((h: Held | null) => {
    heldRef.current = h;
    setHeldState(h);
  }, []);
  const seenSeq = useRef<number | undefined>(state.page?.seq);

  // Every record the panel has shown, as it last looked — what a draft's record is restored from.
  const seen = useRef(new Map<string, Focused>());
  if (state.focused) seen.current.set(String(state.focused.row.handle), state.focused);

  // A new instruction from the core: go there — or hold, if the person is mid-sentence.
  useEffect(() => {
    const page = state.page;
    if (!page) return;
    const verdict = decideFollow({ seq: page.seq, seen: seenSeq.current, follow: state.follow !== false, dirty: isDirty() });
    seenSeq.current = page.seq;
    if (verdict === "ignore") return;
    if (verdict === "go") return setView(page.name);
    setHeld({ page: page.name });
  }, [state.page, state.follow, setView, setHeld]);

  // The person finished what they were doing: let the agent's navigation through.
  useEffect(
    () =>
      onDirtyChange(() => {
        if (isDirty()) return;
        const h = heldRef.current;
        if (h?.page) setView(h.page);
        setHeld(null);
      }),
    [setView, setHeld],
  );

  const target = wentTo(owner, state.focus);
  const label = target ?? (held?.page ? PAGE_WORD[held.page] : null);

  return {
    /** What the record panel shows: the draft's record while one is unsaved. */
    focused: shownRecord(owner, state.focused, seen.current),
    /** The chip, if the agent went somewhere the person has not followed. */
    chip: held || target ? { label: label ?? "elsewhere", handle: target } : null,
    /** The page that is waiting, if any. */
    pendingPage: held?.page ?? null,
    /** The record whose draft is holding the panel. */
    owner,
    /** The person chose to go (having confirmed, if a draft is open): take the held page. */
    go: () => {
      const h = heldRef.current;
      if (h?.page) setView(h.page);
      setHeld(null);
    },
    dismiss: () => setHeld(null),
  };
}

export { dirtyOwner };
