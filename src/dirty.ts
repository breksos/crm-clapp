// Unsaved work, so following an agent never takes the person out of it (W1 B3).
//
// Every control that holds a draft — an inline edit mid-keystroke, a composer with text in it, an
// open form — reports whether it is dirty here, and the window asks before it moves. A module-level
// set, not React state: nothing renders from it, and a control that unmounts must not be able to
// leave a stale "dirty" behind (`useDirty` clears its own entry).

import { useEffect, useId } from "react";

const dirty = new Set<string>();
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

export function isDirty(): boolean {
  return dirty.size > 0;
}

/** Called when the dirty set changes: the window uses it to release a held navigation the moment
 *  the person has finished what they were doing. */
export function onDirtyChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** Report this control's draft. Clears itself on unmount. */
export function useDirty(isDraft: boolean): void {
  const key = useId();
  useEffect(() => {
    if (!isDraft) return;
    dirty.add(key);
    notify();
    return () => {
      dirty.delete(key);
      notify();
    };
  }, [isDraft, key]);
}

/** For tests: forget everything. */
export function resetDirty(): void {
  dirty.clear();
  notify();
}
