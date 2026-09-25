// Saved views: a seat's own convenience, never the snapshot's.
//
//     npm test

import { test } from "node:test";
import assert from "node:assert/strict";

import { addView, BUILTIN_VIEWS, isShowing, loadViews, removeView, saveViews, STORAGE_KEY, type ViewStore } from "./views.ts";

function memory(): ViewStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
}

const draft = { name: "Big ones", query: "acme", kind: "deal" as const, sort: "value" as const };

test("a saved view survives a reload", () => {
  const store = memory();
  const views = addView([], draft);
  assert.equal(saveViews(store, views), true);
  assert.deepEqual(loadViews(store), views);
  assert.equal(views[0].name, "Big ones");
});

test("storage that is missing, blocked or full is not an error the person sees", () => {
  assert.deepEqual(loadViews(null), []);
  assert.deepEqual(loadViews(undefined), []);
  const blocked: ViewStore = {
    getItem: () => {
      throw new Error("SecurityError");
    },
    setItem: () => {
      throw new Error("QuotaExceededError");
    },
  };
  assert.deepEqual(loadViews(blocked), []);
  assert.equal(saveViews(blocked, addView([], draft)), false);
  assert.equal(saveViews(null, []), false);
});

test("junk in storage — another version of the app, hand-edited — is ignored, not trusted", () => {
  const store = memory();
  store.setItem(STORAGE_KEY, "{not json");
  assert.deepEqual(loadViews(store), []);
  store.setItem(STORAGE_KEY, JSON.stringify({ not: "an array" }));
  assert.deepEqual(loadViews(store), []);
  const good = addView([], draft)[0];
  store.setItem(
    STORAGE_KEY,
    JSON.stringify([good, { id: "x", name: "", query: "", kind: "deal", sort: "name" }, { id: "y", name: "Bad kind", query: "", kind: "note", sort: "name" }, 7, null]),
  );
  assert.deepEqual(loadViews(store), [good]);
});

test("saving the same name twice is an edit, and an empty name adds nothing", () => {
  const once = addView([], draft);
  const twice = addView(once, { ...draft, query: "other" });
  assert.equal(twice.length, 1);
  assert.equal(twice[0].query, "other");
  assert.equal(addView(twice, { ...draft, name: "   " }).length, 1);
  assert.equal(addView(twice, { ...draft, name: "big ones" }).length, 1, "names compare without case");
});

test("a view can be removed, and ids stay distinct", () => {
  let views = addView([], draft);
  views = addView(views, { ...draft, name: "Another" });
  assert.equal(new Set(views.map((v) => v.id)).size, 2);
  assert.deepEqual(removeView(views, views[0].id).map((v) => v.name), ["Another"]);
});

test("the built-in views only use what the snapshot can filter on", () => {
  assert.ok(BUILTIN_VIEWS.length >= 3);
  assert.equal(new Set(BUILTIN_VIEWS.map((v) => v.id)).size, BUILTIN_VIEWS.length);
  for (const v of BUILTIN_VIEWS) {
    assert.deepEqual(Object.keys(v).sort(), ["id", "kind", "name", "query", "sort"]);
    assert.match(v.kind, /^(deal|company|contact)$/);
    assert.match(v.sort, /^(updated|name|value)$/);
  }
});

test("a view is 'showing' only when query, kind and sort all match the shared list", () => {
  const v = BUILTIN_VIEWS[0];
  assert.equal(isShowing(v, { query: v.query, kind: v.kind, sort: v.sort }), true);
  assert.equal(isShowing(v, { query: "x", kind: v.kind, sort: v.sort }), false);
  assert.equal(isShowing(v, { query: v.query, kind: null, sort: v.sort }), false);
});
