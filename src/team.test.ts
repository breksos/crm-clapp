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

test("the seat's new keys reach the snapshot the window reads — as the contract and the golden fixtures say them", () => {
  const b: Bootstrap = JSON.parse(readFileSync(join(FIXTURES, "split/bootstrap.json"), "utf8"));
  // §5.8: in local mode `sync` is {state:"local", lastSeen:null} and `members` is just the local person.
  const local = toSnapshot(fromBootstrap(b));
  assert.deepEqual(local.sync, { state: "local", lastSeen: null });
  assert.deepEqual(local.members, [{ id: "me", name: "You" }]);
  assert.deepEqual(local.teamAgents, []);
  b.seat.sync = { state: "online", lastSeen: 5 };
  b.seat.members = [{ id: "me", name: "Berk" }, { id: "ana", name: "Ana Torres" }];
  b.seat.teamAgents = [{ id: "ag1", name: "Scout", ownerId: "ana" }];
  const team = toSnapshot(fromBootstrap(b));
  assert.deepEqual(team.sync, { state: "online", lastSeen: 5 });
  assert.equal(team.members!.length, 2);
  assert.equal(team.teamAgents![0].ownerId, "ana");
  assert.deepEqual(team.agents, b.seat.agents, "seat.agents is still this seat's own roster");
});

test("a core that predates the seat's team keys — the p1 shape — is local mode too", () => {
  const b: Bootstrap = JSON.parse(readFileSync(join(FIXTURES, "split/bootstrap.json"), "utf8"));
  delete b.seat.sync;
  delete b.seat.members;
  delete b.seat.teamAgents;
  const s = toSnapshot(fromBootstrap(b));
  assert.equal(s.sync, undefined);
  assert.equal(s.members, undefined);
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

// MARK: - p3: every op says who did it

import { opBy, applyUpdate as applyUpd, type Update } from "./split.ts";

const focusedOf = (fields: [string, string][], tasks: unknown[] = []) =>
  ({ row: { handle: "orbit", id: "DEAL1" }, fields: fields.map(([label, value]) => ({ label, value })), timeline: [], tasks }) as never;
const ANA = { kind: "human", id: "ana" } as const;
const ME = { kind: "human", id: "me" } as const;

test("a field edit is credited to the op that made it — a teammate lights, you do not, and a feed call cannot take your credit", () => {
  const prev = focusedOf([["Value", "$1"]]);
  const next = focusedOf([["Value", "$2"]]);
  const agentCall = [{ seq: 1, at: 1000, actor: { kind: "agent", id: "ag" }, verb: "set", line: "", outcome: "ok", target: { kind: "deal", id: "x", handle: "orbit" } }] as never;
  const theirs = diffFresh(prev, next, [], 2000, "me", [{ kind: "deal", id: "DEAL1", by: ANA, record: {} }]);
  assert.deepEqual(theirs.fields.get("Value"), ANA);
  const mine = diffFresh(prev, next, agentCall, 2000, "me", [{ kind: "deal", id: "DEAL1", by: ME, record: {} }]);
  assert.ok(isEmpty(mine), "an op that says it was you settles it; an agent's earlier call does not get the credit");
});

test("a completed step is credited from its op, even when the step carries no author of its own", () => {
  const t = (doneAt: number | null) => [{ id: "T1", by: ME, doneAt }];
  const done = diffFresh(focusedOf([], t(null)), focusedOf([], t(5)), [], 2000, "me", [{ kind: "task", id: "T1", by: ANA, record: {} }]);
  assert.deepEqual(done.tasks.get("T1"), ANA);
  assert.ok(isEmpty(diffFresh(focusedOf([], t(null)), focusedOf([], t(5)), [], 2000, "me", [])), "no op and no call: nobody is credited");
});

test("with no op to speak for a change, an agent's call in the feed still can (a core that predates `by`)", () => {
  const call = [{ seq: 1, at: 1500, actor: { kind: "agent", id: "ag" }, verb: "set", line: "", outcome: "ok", target: { kind: "deal", id: "x", handle: "orbit" } }] as never;
  const r = diffFresh(focusedOf([["Value", "$1"]]), focusedOf([["Value", "$2"]]), call, 2000, "me", []);
  assert.equal(r.fields.get("Value")?.kind, "agent");
});

test("an op's author is read from the op, and — for an older core — from the record, never guessed", () => {
  assert.deepEqual(opBy({ seq: 1, op: "upsert", kind: "deal", id: "x", record: {}, by: ANA }), ANA);
  assert.deepEqual(opBy({ seq: 1, op: "upsert", kind: "deal", id: "x", record: { movedBy: ANA } }), ANA);
  assert.equal(opBy({ seq: 1, op: "upsert", kind: "company", id: "x", record: {} }), null);
});

test("an applied update exposes the ops it carried, with their authors; one with none clears them", () => {
  const boot: Bootstrap = JSON.parse(readFileSync(join(FIXTURES, "split/bootstrap.json"), "utf8"));
  const upd: Update = JSON.parse(readFileSync(join(FIXTURES, "split/update-move.json"), "utf8"));
  upd.delta.ops[0].by = ANA;
  const applied = applyUpd(fromBootstrap(boot), upd).state;
  assert.equal(applied.recent.length, upd.delta.ops.length);
  assert.deepEqual(applied.recent[0].by, ANA);
  assert.deepEqual(toSnapshot(applied).recentOps![0].by, ANA);
  const again = applyUpd(applied, upd).state;
  assert.deepEqual(again.recent, [], "an update with nothing new to say lights nothing");
});

// MARK: - the join's real shapes (sync.rs: join_plan, join_done, join_refused)

import { handleChangesLine, refusalLine } from "./join.ts";

test("handle changes read as 'proposed → final', capped, and singular when there is one", () => {
  assert.equal(handleChangesLine([]), "No handles changed.");
  assert.equal(handleChangesLine([{ kind: "company", proposed: "acme", final: "acme-2" }]), "1 handle changed so they stay unique: acme → acme-2.");
  const many = Array.from({ length: 10 }, (_, i) => ({ kind: "deal", proposed: `d${i}`, final: `d${i}-2` }));
  assert.match(handleChangesLine(many), /^10 handles changed so they stay unique: d0 → d0-2, .*, and 2 more\.$/);
});

test("a refused bring says how far it got, and that the organisation is as it was after the last complete part", () => {
  const line = refusalLine({
    ok: false, choice: "bring", state: "choosing", error: "the server refused part 3", partsApplied: 2, partsTotal: 5,
    failedAt: { kind: "deal", handle: "acme-renewal" }, handleChanges: [{ kind: "company", proposed: "acme", final: "acme-2" }],
  });
  assert.match(line, /the server refused part 3/);
  assert.match(line, /2 of 5 parts went in, and the organisation is as it was after the last complete one/);
  assert.match(line, /It stopped at deal acme-renewal\./);
  assert.match(line, /acme → acme-2/);
});

test("a refused start-fresh, or one with no parts, is just the core's sentence", () => {
  assert.equal(refusalLine({ ok: false, choice: "start-fresh", state: "choosing", error: "cannot save the replica" }), "cannot save the replica");
  assert.equal(refusalLine({ ok: false, choice: "bring", state: "choosing" }), "That did not work — nothing was changed.");
});

test("no id ever reaches a join sentence", () => {
  const line = refusalLine({ ok: false, choice: "bring", state: "choosing", error: "x", partsApplied: 0, partsTotal: 2, failedAt: { kind: "deal", handle: "acme-renewal" } });
  assert.doesNotMatch(line, /\b[0-9A-HJKMNP-TV-Z]{26}\b/);
});
