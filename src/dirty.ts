// Unsaved work, so nothing ever takes the person out of it — and nothing can carry it somewhere
// it does not belong (W1 B3, and QA round 8's MAJOR).
//
// Every control that holds a draft — an inline edit mid-keystroke, a composer with text in it, an
// open form — reports here, and the window asks before it moves. A control that is about *a record*
// also says which: **a draft belongs to the record it was started on, and can be saved nowhere
// else.** That is what lets the record panel be held, not just the page: while a record-scoped draft
// is dirty, the panel keeps showing its record (`follow.ts`), whoever tried to move the focus.
//
// A module-level set, not React state: nothing renders from it, and a control that unmounts must not
// be able to leave a stale entry behind (`useDirty` clears its own).

import { useEffect, useId, useState } from "react";

/** key → the handle of the record the draft belongs to, or null for a draft about nothing in
 *  particular (a New deal form: it does not care which record is open). */
const dirty = new Map<string, string | null>();
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

export function isDirty(): boolean {
  return dirty.size > 0;
}

/** The record an unsaved draft belongs to — the first such draft, if any is about a record. */
export function dirtyOwner(): string | null {
  for (const owner of dirty.values()) if (owner) return owner;
  return null;
}

/** Called when the dirty set changes: the window uses it to release a held navigation the moment
 *  the person has nothing unsaved. */
export function onDirtyChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** Report this control's draft, and — if it is about a record — which one. Clears on unmount. */
export function useDirty(isDraft: boolean, owner: string | null = null): void {
  const key = useId();
  useEffect(() => {
    if (!isDraft) return;
    dirty.set(key, owner);
    notify();
    return () => {
      dirty.delete(key);
      notify();
    };
  }, [isDraft, key, owner]);
}

/** Re-renders when the owner of an unsaved draft changes. */
export function useDirtyOwner(): string | null {
  const [owner, setOwner] = useState<string | null>(dirtyOwner());
  useEffect(() => {
    const sync = () => setOwner(dirtyOwner());
    sync();
    return onDirtyChange(sync);
  }, []);
  return owner;
}

/** For tests: forget everything. */
export function resetDirty(): void {
  dirty.clear();
  notify();
}

/** For tests: report a draft without a component. */
export function markDirtyForTest(key: string, owner: string | null): void {
  dirty.set(key, owner);
  notify();
}
