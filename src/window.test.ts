// The window, rendered against the bytes the core actually emits.
//
//     npm test
//
// Round 2's blocker survived because the window had only ever been tested against snapshots
// it invented. This renders the real components — `Window`, the pure half of `App` — against
// the core's golden snapshots in `src-tauri/fixtures/`, read from disk verbatim, and against
// every invented preview scenario besides. For each, in each view, it checks:
//
//   - **no id appears anywhere in the output** — text *and* attributes, because a `title` or
//     an `aria-label` is read to a person too;
//   - every `crm show <handle>` it prints names a record that exists in that snapshot;
//   - for the golden snapshots, that the things the round is for are actually on screen:
//     card titles, the open record, its fields in the core's order, its timeline and tasks.
//
// Components are TSX and resolve `@clappkit` through a Vite alias, so they are loaded through
// Vite's own SSR loader — the project's existing dependency, with the project's own config —
// rather than a second toolchain. React renders to a string; no DOM is involved.

import { after, before, describe, test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

import { findIds } from "./ids.ts";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

/** Where the core's golden snapshots live. Overridable only so they can be read from a
 *  checkout of the backend's branch before it merges; the default is the committed path. */
const FIXTURES = process.env.CRM_FIXTURES ?? join(ROOT, "src-tauri/fixtures");
const GOLDEN_FILES = ["snapshot.json", "snapshot-pending.json"];

type View = "board" | "people" | "companies";
// Loose on purpose: these modules arrive through Vite at runtime, and the test checks their
// output, not their types — `tsc` already checks those.
type Snap = any; // eslint-disable-line @typescript-eslint/no-explicit-any

let vite: ViteDevServer;
let Window: (props: object) => unknown;
let SCENARIOS: Record<string, { label: string; build: () => Snap; then?: { apply: (s: Snap) => Snap } }>;

before(async () => {
  vite = await createServer({
    root: ROOT,
    configFile: join(ROOT, "vite.config.ts"),
    logLevel: "error",
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  ({ Window } = await vite.ssrLoadModule("/src/App.tsx"));
  ({ SCENARIOS } = await vite.ssrLoadModule("/src/scenarios.ts"));
});

after(async () => {
  await vite?.close();
});

// MARK: - Rendering

function render(state: Snap, view: View): string {
  return renderToStaticMarkup(
    createElement(Window as never, {
      state,
      view,
      run: () => {},
      apply: () => {},
      setView: () => {},
      theme: "system",
      chooseTheme: () => {},
    }),
  );
}

/** Text as a person reads it: tags gone, entities decoded. */
function text(html: string): string {
  return decode(html.replace(/<[^>]+>/g, " "));
}

function decode(s: string): string {
  return s
    .replace(/&quot;/g, '"')
    .replace(/&#x27;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

/** Every command the output shows the person, as they would copy it. */
function commands(html: string): string[] {
  return [...html.matchAll(/<code>(.*?)<\/code>/g)].map((m) => decode(m[1]));
}

/** Every handle that names a real record in this snapshot. */
function handlesIn(s: Snap): Set<string> {
  const out = new Set<string>();
  for (const c of Object.values(s.cards ?? {}) as Snap[]) out.add(c.handle);
  for (const r of s.list?.rows ?? []) out.add(r.handle);
  for (const c of s.pending?.candidates ?? []) out.add(c.handle);
  if (s.focused) out.add(s.focused.row.handle);
  if (s.focus?.handle) out.add(s.focus.handle);
  return out;
}

/** The checks every rendered snapshot owes, whatever produced it. */
function assertClean(name: string, state: Snap): void {
  for (const view of ["board", "people"] as View[]) {
    const html = render(state, view);
    assert.deepEqual(findIds(html), [], `${name} / ${view}: an id reached the rendered output`);

    const known = handlesIn(state);
    for (const command of commands(html)) {
      const shown = command.match(/^crm show (\S+)$/);
      if (shown) {
        assert.ok(
          known.has(shown[1]),
          `${name} / ${view}: "${command}" names a record this snapshot does not have. ` +
            `A printed command that names a record names one that exists.`,
        );
      }
    }
  }
}

// MARK: - The core's own snapshots

describe("the core's golden snapshots", () => {
  test("both fixtures are present", () => {
    for (const file of GOLDEN_FILES) {
      assert.ok(
        existsSync(join(FIXTURES, file)),
        `${join(FIXTURES, file)} is missing. The window is tested against the core's own ` +
          `output; without it, this suite is back to testing a guess.`,
      );
    }
  });

  for (const file of GOLDEN_FILES) {
    test(`${file}: no id anywhere in any view, and every \`crm show\` names a real record`, () => {
      assertClean(file, load(file));
    });
  }

  test("snapshot.json: the board shows every card's title, company and value", () => {
    const state = load("snapshot.json");
    const shown = text(render(state, "board"));
    for (const card of Object.values(state.cards) as Snap[]) {
      assert.ok(shown.includes(card.label), `the card "${card.label}" is not on the board`);
      if (card.detail) assert.ok(shown.includes(card.detail), `"${card.label}" lost its company`);
      if (card.value) assert.ok(shown.includes(card.value.formatted), `"${card.label}" lost its value`);
    }
  });

  test("snapshot.json: column totals are the core's formatted strings", () => {
    const state = load("snapshot.json");
    const shown = text(render(state, "board"));
    for (const column of state.board.columns) {
      for (const total of column.totals) assert.ok(shown.includes(total.formatted), `${column.label}: ${total.formatted}`);
    }
  });

  test("snapshot.json: the open record, with the core's fields in the core's order", () => {
    const state = load("snapshot.json");
    assert.ok(state.focused, "the fixture has a record open — that is what this checks");
    const shown = text(render(state, "board"));

    assert.ok(shown.includes(state.focused.row.label), "the open record's title is missing");

    // Same labels and values, same order, as `crm show` prints from the same function.
    let cursor = 0;
    for (const field of state.focused.fields) {
      const at = shown.indexOf(field.label, cursor);
      assert.ok(at >= 0, `field "${field.label}" is missing, or out of the core's order`);
      const value = shown.indexOf(field.value, at);
      assert.ok(value >= 0, `field "${field.label}" lost its value "${field.value}"`);
      cursor = value + field.value.length;
    }
  });

  test("snapshot.json: every timeline line and every task is on screen", () => {
    const state = load("snapshot.json");
    const shown = text(render(state, "board"));
    for (const a of state.focused.timeline) assert.ok(shown.includes(a.body), `timeline line missing: ${a.body}`);
    for (const t of state.focused.tasks) assert.ok(shown.includes(t.what), `task missing: ${t.what}`);
  });

  test("snapshot-pending.json: the question and every candidate are on screen", () => {
    const state = load("snapshot-pending.json");
    const shown = text(render(state, "board"));
    assert.ok(state.pending, "the fixture has a pending question — that is what this checks");
    assert.ok(shown.includes(state.pending.prompt), "the prompt is missing");
    for (const c of state.pending.candidates) {
      assert.ok(shown.includes(c.label), `candidate missing: ${c.label}`);
      assert.ok(shown.includes(c.handle), `candidate ${c.label} does not show its handle`);
    }
  });

  test("a card whose body is missing is a skeleton, not an id", () => {
    // Round 3's blocker: the window used to fall back to printing the id. Take the real
    // snapshot, drop every body, and check nothing id-shaped comes out.
    const state = { ...load("snapshot.json"), cards: {} };
    const html = render(state, "board");
    assert.deepEqual(findIds(html), []);
    const deals = state.board.columns.reduce((n: number, c: Snap) => n + c.dealIds.length, 0);
    assert.equal((html.match(/card-skeleton/g) ?? []).length, deals, "one skeleton per deal on the board");
  });

  function load(file: string): Snap {
    return JSON.parse(readFileSync(join(FIXTURES, file), "utf8"));
  }
});

// MARK: - The invented scenarios

describe("every preview scenario", () => {
  test("renders clean, before and after its scripted write", () => {
    const names = Object.keys(SCENARIOS);
    assert.ok(names.length >= 8, "the scenario list shrank");
    for (const name of names) {
      const scenario = SCENARIOS[name];
      const state = scenario.build();
      assertClean(scenario.label, state);
      if (scenario.then) assertClean(`${scenario.label} (after)`, scenario.then.apply(state));
    }
  });

  test("an empty world only suggests creating things", () => {
    const empty = Object.values(SCENARIOS).find((s) => s.label === "Zero state")!;
    const html = render(empty.build(), "board");
    const shown = commands(html).filter((c) => c.startsWith("crm "));
    assert.ok(shown.length > 0, "the empty state should still say what to type");
    for (const c of shown) {
      assert.match(c, /^crm (add|import) /, `"${c}" is shown with no records, and it names one`);
    }
  });

  test("a hostile title renders verbatim", () => {
    const long = Object.values(SCENARIOS).find((s) => s.label === "Long text")!;
    const html = render(long.build(), "board");
    assert.ok(text(html).includes(`O'Hara "Q4" $upsell \`now\``), "the hostile title is not rendered verbatim");
  });

  test("a hostile search is echoed as a command a real shell reads back intact", () => {
    // The one place a person's own text flows into a printed command: an empty result list
    // echoes the query as `crm find …`. That is where quoting has to hold.
    const query = `O'Hara "Q4" $HOME \`id\``;
    const base = Object.values(SCENARIOS).find((s) => s.label === "Pipeline")!.build();
    const state = { ...base, list: { ...base.list, query, rows: [], total: 0 } };
    const shown = commands(render(state, "people")).find((c) => c.startsWith("crm find "));
    assert.ok(shown, "the empty list should echo the search as a command");
    const words = execFileSync("/bin/sh", ["-c", `for w in ${shown}; do printf '%s\\0' "$w"; done`], {
      encoding: "utf8",
    }).split("\0").slice(0, -1);
    assert.deepEqual(words, ["crm", "find", query]);
  });
});
