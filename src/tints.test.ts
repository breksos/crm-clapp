// The agent palette: clappkit's hash, our colours.
//
//     npm test
//
// `agentTint` is shared by every clapp and is not ours to change, so we keep its *slot* and
// draw our own five violets (`--agent-1..5`). That only holds while the palette we copied is
// still the palette it indexes — so this fails, loudly, if clappkit ever changes it.

import { after, before, describe, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer, type ViteDevServer } from "vite";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

let vite: ViteDevServer;
let tints: { tintSlot: (id: string) => number; tintVar: (id: string) => string; CLAPPKIT_TINTS: readonly string[] };
let clappkit: { agentTint: (id: string) => string };

before(async () => {
  vite = await createServer({
    root: ROOT,
    configFile: join(ROOT, "vite.config.ts"),
    logLevel: "error",
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  tints = (await vite.ssrLoadModule("/src/tints.ts")) as typeof tints;
  clappkit = (await vite.ssrLoadModule("/src/bridge.ts")) as typeof clappkit;
});

after(async () => {
  await vite?.close();
});

describe("tint slot", () => {
  test("our copy of clappkit's palette is clappkit's palette, in order", () => {
    const source = readFileSync(join(ROOT, "clappkit/web/index.ts"), "utf8");
    const body = /AGENT_TINTS\s*=\s*\[([^\]]*)\]/.exec(source)?.[1] ?? "";
    const theirs = [...body.matchAll(/"(#[0-9A-Fa-f]{6})"/g)].map((m) => m[1]);
    assert.deepEqual([...tints.CLAPPKIT_TINTS], theirs);
  });

  test("the slot is the position of clappkit's own answer, so the same id keeps the same slot", () => {
    for (let i = 0; i < 300; i++) {
      const id = `ag_test_${i.toString(16)}`;
      const slot = tints.tintSlot(id);
      assert.equal(slot, tints.CLAPPKIT_TINTS.indexOf(clappkit.agentTint(id)) + 1, id);
      assert.equal(tints.tintSlot(id), slot, "stable across calls");
      assert.ok(slot >= 1 && slot <= 5);
    }
  });

  test("pinned: the preview agents keep their slots", () => {
    // If these move, somebody's agent has changed colour. Never edit them to make a test pass.
    assert.equal(tints.tintSlot("ag_nia_4b02"), PINNED.nia);
    assert.equal(tints.tintSlot("ag_pilot_5a72"), PINNED.pilot);
  });

  test("all five slots are reachable", () => {
    const seen = new Set<number>();
    for (let i = 0; i < 300; i++) seen.add(tints.tintSlot(`ag_${i}`));
    assert.deepEqual([...seen].sort(), [1, 2, 3, 4, 5]);
  });

  test("the colour drawn is one of ours, never clappkit's", () => {
    assert.match(tints.tintVar("ag_nia_4b02"), /^var\(--agent-[1-5]\)$/);
  });
});

describe("the stylesheet", () => {
  const css = readFileSync(join(ROOT, "src/styles.css"), "utf8");

  test("all eight agent tokens exist in the light block and both dark blocks", () => {
    const tokens = ["agent", "agent-weak", "agent-ink", "agent-1", "agent-2", "agent-3", "agent-4", "agent-5"];
    for (const t of tokens) {
      const count = [...css.matchAll(new RegExp(`--${t}:\\s*#[0-9a-fA-F]{6};`, "g"))].length;
      assert.equal(count, 3, `--${t} should be defined in :root and in both dark blocks`);
    }
  });

  test("no stage stripe on the board, and the ring's ink edge is retired", () => {
    assert.doesNotMatch(css, /\.card\[data-stage\]/);
    assert.doesNotMatch(css, /--ring-edge/);
  });

  test("initials take --agent-ink, not a literal white", () => {
    assert.match(css, /\.disc-mono\s*\{[^}]*color:\s*var\(--agent-ink\)/s);
  });
});

const PINNED = { nia: 1, pilot: 2 };
