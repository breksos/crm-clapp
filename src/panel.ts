// The record panel's collapsed state, and the dot that says an agent touched what is inside it.
//
// **Window chrome, not shared state** — like the theme, it lives in this webview's storage and
// never in the snapshot: whether the person has the panel open is nobody else's business, and an
// agent must not be able to open or close it. Storage is allowed to say no (a private window,
// cleared data); every read and write is wrapped, and "expanded" is a real state, not a fallback.
//
// **The person's choice wins.** Nothing the agent does expands a collapsed panel. The dot is how
// they find out something happened.

import { useCallback, useEffect, useRef, useState } from "react";
import type { Actor, Card, Focused } from "./bridge";

export const PANEL_KEY = "breksos.recordPanel.collapsed";

export type PanelStore = Pick<Storage, "getItem" | "setItem">;

export function loadCollapsed(store: PanelStore | null | undefined): boolean {
  try {
    return store?.getItem(PANEL_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveCollapsed(store: PanelStore | null | undefined, collapsed: boolean): boolean {
  try {
    if (!store) return false;
    store.setItem(PANEL_KEY, collapsed ? "1" : "0");
    return true;
  } catch {
    return false;
  }
}

/** What the panel shows, boiled down to "has any of it changed". Id-free for display purposes:
 *  it is only ever compared with itself. */
export type Touch = {
  /** Which record. A different record is a new baseline, not a change. */
  record: string;
  /** Everything but the stage: fields, timeline, next steps. */
  content: string;
  /** Where the deal sits, and who put it there. */
  move: string;
  mover: Actor | null;
};

export function touchOf(focused: Focused | null | undefined, card: Card | undefined): Touch | null {
  if (!focused) return null;
  const { row, fields, timeline, tasks } = focused;
  const content = JSON.stringify([
    row.label,
    row.archived,
    fields.map((f) => [f.label, f.value]),
    timeline.map((a) => a.id),
    tasks.map((t) => [t.id, t.what, t.due, t.doneAt]),
  ]);
  return {
    record: String(row.handle),
    content,
    move: card ? String(card.movedAt) : "",
    mover: card ? card.by : null,
  };
}

/**
 * Has an agent touched this record since `base`?
 *
 * Anything in the content changed while the panel was shut can only have come from outside this
 * window — the panel is where the person edits, and it was closed. A stage change is the one
 * thing the person can do with the panel closed (drag a card), so it counts only when the one who
 * moved it is an agent.
 */
export function agentTouched(base: Touch | null, now: Touch | null): boolean {
  if (!base || !now || base.record !== now.record) return false;
  if (base.content !== now.content) return true;
  return base.move !== now.move && now.mover?.kind === "agent";
}

export function usePanel(focused: Focused | null | undefined, card: Card | undefined) {
  const [collapsed, setCollapsed] = useState(false);
  const [unread, setUnread] = useState(false);
  const base = useRef<Touch | null>(null);
  const now = touchOf(focused, card);
  const key = now ? `${now.record}|${now.content}|${now.move}` : "";

  // Read after mount, so the first render matches the server's (which has no storage).
  useEffect(() => {
    try {
      setCollapsed(loadCollapsed(window.localStorage));
    } catch {
      /* expanded */
    }
  }, []);

  useEffect(() => {
    if (!collapsed || !base.current || base.current.record !== now?.record) {
      // Open, or a different record: this is the new baseline, and nothing is unread.
      base.current = now;
      if (!collapsed) setUnread(false);
      return;
    }
    if (agentTouched(base.current, now)) setUnread(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, collapsed]);

  const set = useCallback((next: boolean) => {
    setCollapsed(next);
    if (!next) setUnread(false);
    try {
      saveCollapsed(window.localStorage, next);
    } catch {
      /* the choice holds for this session */
    }
  }, []);

  const toggle = useCallback(() => set(!collapsed), [set, collapsed]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "\\") {
        e.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggle]);

  return { collapsed, unread, toggle };
}
