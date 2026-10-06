// W1 B1: the record panel's collapsed state and its unread dot.
//
//     npm test

import { test } from "node:test";
import assert from "node:assert/strict";

import { agentTouched, loadCollapsed, PANEL_KEY, saveCollapsed, touchOf, type PanelStore } from "./panel.ts";

function memory(): PanelStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
}

test("the collapsed panel survives a reload", () => {
  const store = memory();
  assert.equal(loadCollapsed(store), false, "expanded by default");
  assert.equal(saveCollapsed(store, true), true);
  assert.equal(loadCollapsed(store), true);
  saveCollapsed(store, false);
  assert.equal(loadCollapsed(store), false);
  assert.equal(store.data.get(PANEL_KEY), "0");
});

test("storage that is absent or blocked leaves the panel expanded and breaks nothing", () => {
  const blocked: PanelStore = {
    getItem: () => {
      throw new Error("SecurityError");
    },
    setItem: () => {
      throw new Error("QuotaExceededError");
    },
  };
  assert.equal(loadCollapsed(null), false);
  assert.equal(loadCollapsed(blocked), false);
  assert.equal(saveCollapsed(blocked, true), false);
  assert.equal(saveCollapsed(undefined, true), false);
});

const focused = (over: Record<string, unknown> = {}) =>
  ({
    row: { handle: "orbit", label: "Orbit", archived: false },
    fields: [["Stage", "Lead"]].map(([label, value]) => ({ label, value })),
    timeline: [{ id: "a1" }],
    tasks: [{ id: "t1", what: "call", due: "2026-10-10", doneAt: null }],
    timelineTotal: 1,
    ...over,
  }) as never;
const card = (movedAt: number, by: unknown) => ({ movedAt, by }) as never;
const HUMAN = { kind: "human" };
const AGENT = { kind: "agent", id: "ag" };

test("the dot sets when an agent logs, completes a step, or sets a field; clears when nothing changed", () => {
  const base = touchOf(focused(), card(1, HUMAN));
  assert.equal(agentTouched(base, touchOf(focused(), card(1, HUMAN))), false, "nothing changed");
  assert.equal(agentTouched(base, touchOf(focused({ timeline: [{ id: "a2" }, { id: "a1" }] }), card(1, HUMAN))), true, "a log");
  assert.equal(agentTouched(base, touchOf(focused({ tasks: [{ id: "t1", what: "call", due: "2026-10-10", doneAt: 5 }] }), card(1, HUMAN))), true, "a done step");
  assert.equal(agentTouched(base, touchOf(focused({ fields: [{ label: "Stage", value: "Won" }] }), card(1, HUMAN))), true, "a field");
});

test("the person's own drag with the panel shut is not an agent touching it; an agent's move is", () => {
  const base = touchOf(focused(), card(1, AGENT));
  assert.equal(agentTouched(base, touchOf(focused(), card(2, HUMAN))), false);
  assert.equal(agentTouched(base, touchOf(focused(), card(2, AGENT))), true);
});

test("a different record is a new baseline, never a change", () => {
  const base = touchOf(focused(), card(1, HUMAN));
  const other = touchOf(focused({ row: { handle: "acme", label: "Acme", archived: false } }), card(9, AGENT));
  assert.equal(agentTouched(base, other), false);
  assert.equal(agentTouched(null, base), false);
  assert.equal(agentTouched(base, null), false);
});
