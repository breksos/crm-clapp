// What an agent just changed on the open record, so it can be lit for ~2 seconds (W1 B3).
//
// Colour means an agent did this, so the glow is the agent's own tint — and only an agent's:
// a timeline entry the person logged themselves is not lit. Attribution comes from the one
// place that has it:
//   · a timeline entry and a next step carry `by`;
//   · a field set, or a next step completed, carries nothing, so it is read from the agent feed —
//     the newest `set` / `done` call on this very record by an agent, seconds ago.
//
// Pure: `diffFresh` compares two snapshots of one record. `useFresh` is the timer around it.

import { useEffect, useRef, useState } from "react";
import type { Actor, AgentCall, Focused } from "./bridge.ts";
import { idKey } from "./ids.ts";

export type Fresh = {
  timeline: Map<string, Actor>;
  tasks: Map<string, Actor>;
  fields: Map<string, Actor>;
};

export const EMPTY_FRESH: Fresh = { timeline: new Map(), tasks: new Map(), fields: new Map() };
export const GLOW_MS = 2000;
/** How recent an agent's call must be to take credit for a change nothing else attributes. */
const CREDIT_MS = 10_000;

function creditFor(feed: AgentCall[], handle: string, verbs: string[], now: number): Actor | null {
  const hit = feed.find(
    (c) => c.actor.kind === "agent" && c.outcome === "ok" && verbs.includes(c.verb) && c.target?.handle === handle && now - c.at <= CREDIT_MS,
  );
  return hit ? hit.actor : null;
}

/**
 * Did somebody other than this person do it? An agent — this seat's or a teammate's — or, since p3,
 * another human. A human with no id, or this person's own, is never "other": your own work does not
 * light up under your hands.
 */
export function byOther(by: Actor, me: string | null): boolean {
  if (by.kind === "agent") return true;
  const id = (by as { id?: string }).id;
  return !!id && !!me && id !== me;
}

export function diffFresh(prev: Focused | null | undefined, next: Focused | null | undefined, feed: AgentCall[], now: number, me: string | null = null): Fresh {
  const out: Fresh = { timeline: new Map(), tasks: new Map(), fields: new Map() };
  if (!prev || !next || prev.row.handle !== next.row.handle) return out;
  const handle = String(next.row.handle);

  const had = new Set(prev.timeline.map((a) => idKey(a.id)));
  for (const a of next.timeline) if (!had.has(idKey(a.id)) && byOther(a.by, me)) out.timeline.set(idKey(a.id), a.by);

  const before = new Map(prev.tasks.map((t) => [idKey(t.id), t]));
  for (const t of next.tasks) {
    const was = before.get(idKey(t.id));
    if (!was) {
      const who = byOther(t.by, me) ? t.by : creditFor(feed, handle, ["task"], now);
      if (who) out.tasks.set(idKey(t.id), who);
    } else if (was.doneAt === null && t.doneAt !== null) {
      const who = creditFor(feed, handle, ["done"], now);
      if (who) out.tasks.set(idKey(t.id), who);
    }
  }

  const fieldsBefore = new Map(prev.fields.map((f) => [f.label, f.value]));
  for (const f of next.fields) {
    if (fieldsBefore.has(f.label) && fieldsBefore.get(f.label) !== f.value) {
      const who = creditFor(feed, handle, ["set", "link", "move"], now);
      if (who) out.fields.set(f.label, who);
    }
  }
  return out;
}

export const isEmpty = (f: Fresh) => f.timeline.size + f.tasks.size + f.fields.size === 0;

export function useFresh(focused: Focused | null | undefined, feed: AgentCall[] | undefined, me: string | null = null): Fresh {
  const prev = useRef<Focused | null | undefined>(focused);
  const [fresh, setFresh] = useState<Fresh>(EMPTY_FRESH);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const found = diffFresh(prev.current, focused, feed ?? [], Date.now(), me);
    prev.current = focused;
    if (isEmpty(found)) return;
    setFresh(found);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setFresh(EMPTY_FRESH), GLOW_MS);
  }, [focused, feed, me]);

  useEffect(() => () => window.clearTimeout(timer.current), []);
  return fresh;
}
