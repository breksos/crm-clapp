// p3: the team, in the window — built against the contract (`sync-contract.md` §5.5, §5.8, §7) and
// the split fixtures, with the seat's new keys added the way the contract describes them. The
// backend's regenerated golden fixtures replace the hand-added keys when they land.
//
//     npm test

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { byOther, diffFresh, isEmpty } from "./fresh.ts";
import { currentFacts } from "./refusal.ts";
import { countsLine } from "./join.ts";
import { fromBootstrap, toSnapshot, type Bootstrap } from "./split.ts";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURES = process.env.CRM_FIXTURES ?? join(ROOT, "src-tauri/fixtures");

test("the seat's new keys reach the snapshot the window reads, and local mode has none", () => {
  const b: Bootstrap = JSON.parse(readFileSync(join(FIXTURES, "split/bootstrap.json"), "utf8"));
  const local = toSnapshot(fromBootstrap(b));
  assert.equal(local.sync, undefined);
  assert.equal(local.members, undefined);
  b.seat.sync = { state: "online", lastSeen: 5 };
  b.seat.members = [{ id: "me", name: "Berk" }, { id: "ana", name: "Ana Torres" }];
  b.seat.teamAgents = [{ id: "ag1", name: "Scout", ownerId: "ana" }];
  const team = toSnapshot(fromBootstrap(b));
  assert.deepEqual(team.sync, { state: "online", lastSeen: 5 });
  assert.equal(team.members!.length, 2);
  assert.equal(team.teamAgents![0].ownerId, "ana");
  assert.deepEqual(team.agents, b.seat.agents, "seat.agents is still this seat's own roster");
});

test("who counts as 'someone else': agents, and other people — never you", () => {
  assert.equal(byOther({ kind: "agent", id: "x" }, "me"), true);
  assert.equal(byOther({ kind: "human", id: "ana" }, "me"), true);
  assert.equal(byOther({ kind: "human", id: "me" }, "me"), false);
  assert.equal(byOther({ kind: "human" }, "me"), false, "an older record with no id is yours");
  assert.equal(byOther({ kind: "human", id: "ana" }, null), false, "signed out there is no 'other'");
});

test("a teammate's note and next step light up under your hands; your own do not", () => {
  const f = (timeline: unknown[], tasks: unknown[] = []) => ({ row: { handle: "orbit" }, fields: [], timeline, tasks }) as never;
  const prev = f([{ id: "a1", by: { kind: "human", id: "me" } }]);
  const theirs = diffFresh(prev, f([{ id: "a2", by: { kind: "human", id: "ana" } }, { id: "a1", by: { kind: "human", id: "me" } }], [{ id: "t9", by: { kind: "human", id: "ana" }, doneAt: null }]), [], 1, "me");
  assert.deepEqual([...theirs.timeline.keys()], ["a2"]);
  assert.deepEqual([...theirs.tasks.keys()], ["t9"]);
  const mine = diffFresh(prev, f([{ id: "a3", by: { kind: "human", id: "me" } }, { id: "a1", by: { kind: "human", id: "me" } }]), [], 1, "me");
  assert.ok(isEmpty(mine));
});

test("what the record says now is only the facts the row carries", () => {
  const row = (over: Record<string, unknown>) => ({ label: "Orbit", value: null, stage: null, status: null, detail: null, archived: false, ...over }) as never;
  assert.deepEqual(currentFacts(row({ value: { formatted: "$60,000.00" }, stage: "proposal", status: "open", detail: "Orbit Labs" })), ["Value $60,000.00", "Stage proposal", "Orbit Labs"]);
  assert.deepEqual(currentFacts(row({ status: "won", stage: "negotiation" })), ["Status won"]);
  assert.deepEqual(currentFacts(row({ archived: true })), ["Archived"]);
  assert.deepEqual(currentFacts(row({})), []);
});

test("the join choice says what is on each side, in words, and 'nothing' for an empty side", () => {
  assert.equal(countsLine({ companies: 3, contacts: 1, deals: 8, tasks: 0 }), "3 companies, 1 contact, 8 deals");
  assert.equal(countsLine({ tasks: 1 }), "1 next step");
  assert.equal(countsLine({}), "nothing");
  assert.equal(countsLine(undefined), "nothing");
});

test("the stylesheet's blocks are balanced — a merge that eats a brace breaks the whole window, and no render test would notice", () => {
  const css = readFileSync(join(ROOT, "src/styles.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  assert.equal((css.match(/\{/g) ?? []).length, (css.match(/\}/g) ?? []).length);
});
