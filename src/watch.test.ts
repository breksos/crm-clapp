// W1: watching an agent — the feed's wording, follow's decisions, and what lights up.
//
//     npm test

import { describe, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { chipText, decideFollow, shownRecord, wentTo } from "./follow.ts";
import { diffFresh, isEmpty } from "./fresh.ts";
import { dirtyOwner, isDirty, markDirtyForTest, onDirtyChange, resetDirty } from "./dirty.ts";
import { agoShort } from "./time.ts";
import { fromBootstrap, toSnapshot, applyUpdate, type Bootstrap, type Update } from "./split.ts";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURES = process.env.CRM_FIXTURES ?? join(ROOT, "src-tauri/fixtures");
const read = (f: string) => JSON.parse(readFileSync(join(FIXTURES, f), "utf8"));

describe("follow decisions", () => {
  test("a new page instruction goes there when nothing is unsaved", () => {
    assert.equal(decideFollow({ seq: 2, seen: 1, follow: true, dirty: false }), "go");
  });

  test("navigation is held while a form is dirty — and only then", () => {
    assert.equal(decideFollow({ seq: 2, seen: 1, follow: true, dirty: true }), "hold");
  });

  test("follow off, an old instruction, or no instruction goes nowhere", () => {
    assert.equal(decideFollow({ seq: 2, seen: 1, follow: false, dirty: false }), "ignore");
    assert.equal(decideFollow({ seq: 2, seen: 2, follow: true, dirty: false }), "ignore");
    assert.equal(decideFollow({ seq: undefined, seen: undefined, follow: true, dirty: false }), "ignore");
  });

  test("the same page twice is a new instruction, because seq changed", () => {
    assert.equal(decideFollow({ seq: 5, seen: 4, follow: true, dirty: false }), "go");
  });

  const rec = (handle: string) => ({ row: { handle } }) as never;

  test("a draft holds the PANEL, not just the page: it shows the draft's record whatever the focus is", () => {
    const seen = new Map([["alpha-co", rec("alpha-co")], ["beta-co", rec("beta-co")]]);
    assert.equal(String(shownRecord("alpha-co", rec("beta-co"), seen)!.row.handle), "alpha-co");
    assert.equal(String(shownRecord("alpha-co", rec("alpha-co"), seen)!.row.handle), "alpha-co");
  });

  test("with nothing unsaved the panel is simply the focus", () => {
    const seen = new Map([["alpha-co", rec("alpha-co")]]);
    assert.equal(String(shownRecord(null, rec("beta-co"), seen)!.row.handle), "beta-co");
    assert.equal(shownRecord(null, null, seen), null);
  });

  test("a draft on a record the window never showed cannot invent one", () => {
    assert.equal(String(shownRecord("ghost", rec("beta-co"), new Map())!.row.handle), "beta-co");
  });

  test("the chip names where the agent went — only when the panel is held away from it", () => {
    assert.equal(wentTo("alpha-co", { kind: "company", id: "x", handle: "beta-co" } as never), "beta-co");
    assert.equal(wentTo("alpha-co", { kind: "company", id: "x", handle: "alpha-co" } as never), null);
    assert.equal(wentTo(null, { kind: "company", id: "x", handle: "beta-co" } as never), null);
    assert.equal(wentTo("alpha-co", null), null);
  });

  test("the chip says where the agent went", () => {
    assert.equal(chipText("orbit-line-monitoring"), "Agent opened orbit-line-monitoring — go");
  });

  test("following never sends a command: the core moves the focus, and a window `show` would be the app talking to itself", () => {
    const src = readFileSync(join(ROOT, "src/follow.ts"), "utf8").replace(/\/\/.*$/gm, "");
    assert.doesNotMatch(src, /\bcmd\s*\(|\brun\s*\(|cmd:\s*"show"|\bsend\s*\(/);
  });
});

describe("unsaved work", () => {
  test("is tracked as a set, and says when it changes", () => {
    resetDirty();
    assert.equal(isDirty(), false);
    let heard = 0;
    const off = onDirtyChange(() => heard++);
    resetDirty();
    assert.ok(heard >= 1);
    off();
  });
});

describe("a draft belongs to its record", () => {
  test("is told which record it is on, and reports the owner of the first record-scoped draft", () => {
    resetDirty();
    assert.equal(dirtyOwner(), null);
    markDirtyForTest("new-deal-form", null);
    assert.equal(isDirty(), true);
    assert.equal(dirtyOwner(), null, "a New deal form is not about any one record, so it does not pin the panel");
    markDirtyForTest("note", "alpha-co");
    assert.equal(dirtyOwner(), "alpha-co");
    resetDirty();
    assert.equal(dirtyOwner(), null);
  });

  test("every record-scoped control says which record it is on", () => {
    const src = readFileSync(join(ROOT, "src/Record.tsx"), "utf8");
    const calls = [...src.matchAll(/useDirty\(([^;]*)\);/g)].map((m) => m[1]);
    assert.ok(calls.length >= 5);
    for (const c of calls) assert.match(c, /,\s*(String\(|owner)/, `useDirty(${c}) has no owner: its draft could travel`);
  });

  test("the panel is remounted per record, so state cannot outlive the record it is about", () => {
    const src = readFileSync(join(ROOT, "src/App.tsx"), "utf8");
    assert.match(src, /<RecordPanel\s+key=\{/);
  });

  test("the person's own click on another record is asked about, not carried or silently dropped", () => {
    const src = readFileSync(join(ROOT, "src/App.tsx"), "utf8");
    assert.match(src, /c\.cmd === "show" && following\.owner/);
    assert.match(src, /<DraftGuard/);
  });
});

describe("what an agent just changed", () => {
  const AGENT = { kind: "agent", id: "ag1" } as const;
  const HUMAN = { kind: "human" } as const;
  const focused = (over: Record<string, unknown> = {}) =>
    ({
      row: { handle: "orbit", label: "Orbit" },
      fields: [{ label: "Value", value: "$1" }],
      timeline: [{ id: "a1", by: HUMAN }],
      tasks: [{ id: "t1", by: HUMAN, doneAt: null }],
      ...over,
    }) as never;
  const call = (verb: string, at = 1000, actor = AGENT) => ({ seq: 1, at, actor, verb, line: "", outcome: "ok", target: { kind: "deal", id: "x", handle: "orbit" } }) as never;

  test("a timeline entry an agent logged is lit in that agent's name; the person's own is not", () => {
    const prev = focused();
    const agent = diffFresh(prev, focused({ timeline: [{ id: "a2", by: AGENT }, { id: "a1", by: HUMAN }] }), [], 2000);
    assert.deepEqual([...agent.timeline.keys()], ["a2"]);
    assert.deepEqual(agent.timeline.get("a2"), AGENT);
    const mine = diffFresh(prev, focused({ timeline: [{ id: "a3", by: HUMAN }, { id: "a1", by: HUMAN }] }), [], 2000);
    assert.ok(isEmpty(mine));
  });

  test("a next step an agent added, and one it completed, are lit", () => {
    const prev = focused();
    const added = diffFresh(prev, focused({ tasks: [{ id: "t1", by: HUMAN, doneAt: null }, { id: "t2", by: AGENT, doneAt: null }] }), [], 2000);
    assert.deepEqual([...added.tasks.keys()], ["t2"]);
    const done = diffFresh(prev, focused({ tasks: [{ id: "t1", by: HUMAN, doneAt: 5 }] }), [call("done")], 2000);
    assert.deepEqual([...done.tasks.keys()], ["t1"], "credited to the agent's done call");
    assert.ok(isEmpty(diffFresh(prev, focused({ tasks: [{ id: "t1", by: HUMAN, doneAt: 5 }] }), [], 2000)), "no call, no credit: the person did it");
  });

  test("a field an agent set is lit, from the call that set it — not from one that is old", () => {
    const prev = focused();
    const next = focused({ fields: [{ label: "Value", value: "$2" }] });
    assert.deepEqual([...diffFresh(prev, next, [call("set", 1500)], 2000).fields.keys()], ["Value"]);
    assert.ok(isEmpty(diffFresh(prev, next, [call("set", 1)], 60_000)), "a call a minute ago does not take credit");
    assert.ok(isEmpty(diffFresh(prev, next, [call("set", 1500, HUMAN as never)], 2000)), "nor does the person's own");
  });

  test("a different record is not a change", () => {
    assert.ok(isEmpty(diffFresh(focused(), focused({ row: { handle: "acme" }, timeline: [{ id: "z", by: AGENT }] }), [], 2000)));
    assert.ok(isEmpty(diffFresh(null, focused(), [], 2000)));
  });
});

describe("the clock", () => {
  test("reads in seconds for a step-by-step log", () => {
    const now = 100_000;
    assert.equal(agoShort(now - 500, now), "now");
    assert.equal(agoShort(now - 12_000, now), "12s ago");
    assert.equal(agoShort(now - 3 * 60_000, now), "3m ago");
    assert.equal(agoShort(now - 3 * 3_600_000, now), "3h ago");
  });
});

describe("the core's view.feed, view.follow and view.page", () => {
  const boot = (): Bootstrap => read("split/bootstrap.json");
  const move = (): Update => read("split/update-move.json");

  test("arrive on the snapshot the window reads", () => {
    const s = toSnapshot(fromBootstrap(boot()));
    assert.deepEqual(s.agentFeed, boot().view.feed);
    assert.equal(s.follow, boot().view.follow);
    assert.equal(s.page ?? null, boot().view.page ?? null);
  });

  test("an update brings the call it made, newest first, and never loses its own line", () => {
    const after = toSnapshot(applyUpdate(fromBootstrap(boot()), move()).state);
    assert.equal(after.agentFeed!.length, move().view.feed!.length);
    assert.equal(after.agentFeed![0].verb, "move");
    assert.equal(after.agentFeed![0].outcome, "ok");
    assert.match(after.agentFeed![0].line, /^move [a-z0-9-]+ [a-z]+$/, "a command in handles");
  });

  test("no line in the fixtures carries an id", () => {
    for (const call of move().view.feed ?? []) {
      assert.doesNotMatch(call.line, /\b[0-9A-HJKMNP-TV-Z]{26}\b/);
    }
  });

  test("follow defaults on when a core predates it", () => {
    const b = boot();
    delete (b.view as { follow?: boolean }).follow;
    assert.equal(toSnapshot(fromBootstrap(b)).follow, true);
  });
});
