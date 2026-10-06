// The split shape, against the core's own output.
//
//     npm test
//
// `src-tauri/fixtures/split/*.json` are what `AppState::bootstrap` and `AppState::update` emit,
// read from disk verbatim — not a split shape this window invented, which is how round 2's blocker
// survived. The same discipline as M3 and round 3.
//
// What this holds the reducer to:
//   - rev orders the view, the watermark orders the data, and the two are never confused;
//   - a delta applies only where it starts, never twice, and a gap is a resync, not a guess.

import { describe, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { applyChanges, applyOp, applyUpdate, fromBootstrap, isBootstrap, isUpdate, toSnapshot, type Bootstrap, type Update } from "./split.ts";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURES = process.env.CRM_FIXTURES ?? join(ROOT, "src-tauri/fixtures");
const read = (file: string) => JSON.parse(readFileSync(join(FIXTURES, file), "utf8"));

const bootstrap = (): Bootstrap => read("split/bootstrap.json");
const pending = (): Bootstrap => read("split/bootstrap-pending.json");
const move = (): Update => read("split/update-move.json");

describe("the fixtures are what the window expects", () => {
  test("a bootstrap is a bootstrap and an update is an update", () => {
    assert.ok(isBootstrap(bootstrap()) && isBootstrap(pending()));
    assert.ok(isUpdate(move()));
    assert.ok(!isUpdate(bootstrap()) && !isBootstrap(move()));
  });
});

describe("what the split carries", () => {
  test("what only the split carries rides beside the snapshot", () => {
    const s = toSnapshot(fromBootstrap(bootstrap()));
    assert.ok(s.me && s.me.name, "who the person is (gap 8)");
    assert.ok(s.pipelineTotals && s.pipelineTotals.open.length > 0, "pipeline totals (gap 3)");
    assert.ok(s.feed && s.feed.items.length > 0, "the feed (gap 2)");
    assert.equal(typeof s.watermark, "number");
  });
});

describe("two counters, two jobs", () => {
  test("a bootstrap holds both, and they are different numbers", () => {
    const state = fromBootstrap(bootstrap());
    assert.equal(state.rev, bootstrap().rev);
    assert.equal(state.watermark, bootstrap().watermark);
  });

  test("an update advances the watermark by its delta and takes rev from the message", () => {
    const u = move();
    const got = applyUpdate(fromBootstrap(bootstrap()), { ...u, rev: 7 });
    assert.equal(got.state.rev, 7);
    assert.equal(got.state.watermark, u.delta.to);
    assert.equal(got.resync, false);
  });

  test("an older view never wins, but the data still applies in order", () => {
    const held = { ...fromBootstrap(bootstrap()), rev: 9 };
    const got = applyUpdate(held, { ...move(), rev: 3 });
    assert.equal(got.state.rev, 9, "rev must not go backwards");
    assert.deepEqual(got.state.view, held.view, "an older view must not replace a newer one");
    assert.equal(got.state.watermark, move().delta.to, "the data is ordered by the watermark, not the rev");
  });
});

describe("row actions", () => {
  test("a deal upsert replaces the client's copy, and a move is appended to the feed", () => {
    const before = fromBootstrap(bootstrap());
    const u = move();
    const dealOp = u.delta.ops.find((o) => o.kind === "deal")!;
    const after = applyUpdate(before, u).state;

    const was = before.data.deals.find((d) => d.id === dealOp.id)!;
    const now = after.data.deals.find((d) => d.id === dealOp.id)!;
    assert.notDeepEqual(now, was, "the fixture's move changed the deal");
    assert.equal(now.stage, "qualified");
    assert.equal(after.data.deals.length, before.data.deals.length, "an upsert is not an insert");

    assert.equal(after.feed.total, before.feed.total + 1);
    assert.equal(after.feed.items[0].id, u.delta.ops.find((o) => o.kind === "move")!.record.id);
    assert.ok(after.feed.items.length <= after.feed.pageSize, "the first page stays a page");
  });

  test("applying the same update twice changes nothing the second time", () => {
    const once = applyUpdate(fromBootstrap(bootstrap()), move());
    const twice = applyUpdate(once.state, move());
    assert.equal(twice.resync, false);
    assert.deepEqual(twice.state.data, once.state.data);
    assert.deepEqual(twice.state.feed, once.state.feed);
    assert.equal(twice.state.watermark, once.state.watermark);
  });

  test("a feed item seen twice is not appended twice", () => {
    const item = move().delta.ops.find((o) => o.kind === "move")!;
    const once = applyOp(fromBootstrap(bootstrap()), item);
    assert.deepEqual(applyOp(once, item).feed, once.feed);
  });

  test("a kind the window predates is ignored rather than guessed at", () => {
    const state = fromBootstrap(bootstrap());
    const got = applyOp(state, { seq: 1, op: "upsert", kind: "pipeline-from-the-future", id: "x", record: { id: "x" } });
    assert.deepEqual(got.data, state.data);
  });

  test("a new record is inserted", () => {
    const state = fromBootstrap(bootstrap());
    const got = applyOp(state, { seq: 1, op: "upsert", kind: "company", id: "NEWCO", record: { id: "NEWCO", name: "Newco" } });
    assert.equal(got.data.companies.length, state.data.companies.length + 1);
  });
});

describe("a gap is a resync, not a guess", () => {
  test("an update that does not start where the data is touches no row", () => {
    const state = fromBootstrap(bootstrap());
    const ahead: Update = { ...move(), delta: { ...move().delta, from: 5, to: 7 } };
    const got = applyUpdate(state, ahead);
    assert.equal(got.resync, true);
    assert.deepEqual(got.state.data, state.data);
    assert.equal(got.state.watermark, state.watermark);
    assert.deepEqual(got.state.view, ahead.view, "the view is still this seat's current one");
  });

  test("changes fills the gap in order, and a stale answer means bootstrap again", () => {
    const state = fromBootstrap(bootstrap());
    const ops = move().delta.ops;
    const filled = applyChanges(state, { watermark: 2, from: 0, to: 2, ops });
    assert.equal(filled.resync, false);
    assert.equal(filled.state.watermark, 2);
    assert.equal(applyChanges(state, { stale: true, watermark: 99 }).resync, true);
    assert.equal(applyChanges(state, { watermark: 9, from: 4, to: 9, ops: [] }).resync, true, "changes that do not start where we are");
  });
});
