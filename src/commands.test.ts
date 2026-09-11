// The guard on the surface contract. Run it with the runtime that is already on PATH:
//
//     node --test src/
//
// Node 24 strips types itself, so this needs no runner, no config and no dependency — which
// matters, because a test that is awkward to run is a test that stops being run.
//
// Three things are checked, and each one is a defect QA actually found in round one:
//
//   1. Every command the window prints appears in the frozen grammar. Round one printed
//      seven hard-coded commands using flags nobody had defined.
//   2. No component hard-codes a command string. That is what let those seven drift out of
//      sight of any review of the grammar.
//   3. The preview fixtures are ULIDs. Readable fixture ids are why a window that printed
//      `crm task 01K4Z…` looked perfectly correct in the harness.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { addDealCmd, findCmd, GRANT_CMD, importCmd, logCmd, selectCmd, showCmd, taskCmd } from "./commands.ts";
import { asHandle, looksLikeId } from "./ids.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");

const H = asHandle("acme");

/** Every command this window is capable of printing, rendered. */
const RENDERED = [
  showCmd(H),
  findCmd("acme"),
  addDealCmd("Northwind renewal", H),
  taskCmd(H, "call back", "2026-09-30"),
  logCmd("note", H, "…"),
  importCmd("contacts.csv"),
  selectCmd(2),
];

/**
 * The frozen grammar, read from the work order rather than restated here.
 *
 * Restating it would make this test agree with a copy of the contract instead of the
 * contract — which is the same mistake as the backend fixture that pinned `"id": "acme"`
 * and stayed green through a revision that changed what an id was.
 */
function grammarVerbs(): Map<string, string[]> {
  const md = readFileSync(join(ROOT, "docs/work-orders/m2-cli.md"), "utf8");
  const start = md.indexOf("## The argument grammar");
  assert.notEqual(start, -1, "m2-cli.md no longer has a grammar section");
  const block = md.slice(md.indexOf("```", start) + 3, md.indexOf("```", md.indexOf("```", start) + 3));

  const byVerb = new Map<string, string[]>();
  for (const line of block.split("\n")) {
    const m = line.trim().match(/^crm\s+(\S+)/);
    if (!m) continue;
    const list = byVerb.get(m[1]) ?? [];
    list.push(line.trim());
    byVerb.set(m[1], list);
  }
  assert.ok(byVerb.size > 10, "the grammar block parsed to almost nothing — check the format");
  return byVerb;
}

test("every printed command uses a verb in the frozen grammar", () => {
  const grammar = grammarVerbs();
  for (const command of RENDERED) {
    const verb = command.split(/\s+/)[1];
    assert.ok(
      grammar.has(verb),
      `"${command}" uses the verb "${verb}", which is not in m2-cli.md's grammar. ` +
        `A command the window prints is a promise: either it is in the grammar or it does not exist.`,
    );
  }
});

test("every flag a printed command uses is declared for that verb", () => {
  const grammar = grammarVerbs();
  for (const command of RENDERED) {
    const verb = command.split(/\s+/)[1];
    const declared = (grammar.get(verb) ?? []).join(" ");
    for (const flag of command.match(/--[a-z-]+/g) ?? []) {
      assert.ok(
        declared.includes(flag),
        `"${command}" passes ${flag} to \`crm ${verb}\`, which the grammar does not declare.`,
      );
    }
  }
});

test("no printed command carries anything id-shaped", () => {
  for (const command of RENDERED.concat(GRANT_CMD)) {
    for (const word of command.split(/[\s"']+/)) {
      assert.ok(
        !looksLikeId(word),
        `"${command}" contains ${word}, which is shaped like a ULID. ` +
          `ids are for machines; a command a person types takes a handle.`,
      );
    }
  }
});

test("components never hard-code a command — they all come from commands.ts", () => {
  const offenders: string[] = [];
  for (const file of readdirSync(join(ROOT, "src"))) {
    if (!file.endsWith(".tsx")) continue;
    const source = readFileSync(join(ROOT, "src", file), "utf8");
    for (const line of source.split("\n")) {
      // A command inside JSX, rather than inside a comment explaining one.
      if (/<code>\s*(crm|clatch)\s/.test(line)) offenders.push(`${file}: ${line.trim()}`);
    }
  }
  assert.deepEqual(
    offenders,
    [],
    "a command string is hard-coded into a component; move it to commands.ts so the " +
      "grammar test can see it",
  );
});

test("the preview fixtures mint ULIDs, not readable ids", () => {
  let source = readFileSync(join(ROOT, "src/preview.ts"), "utf8");

  // The agent roster is a different namespace: those ids are Clatch's, not record ULIDs,
  // and the window never renders one — it keys on them and feeds them to `agentTint`. So
  // they are literals on purpose, and this test is about record fixtures.
  const roster = source.indexOf("const AGENTS: Agent[] = [");
  assert.notEqual(roster, -1, "the agent roster moved; this exclusion needs rechecking");
  source = source.slice(0, roster) + source.slice(source.indexOf("];", roster));

  const offenders = [...source.matchAll(/\bid:\s*"([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(
    offenders,
    [],
    "a preview fixture carries a literal string id. Fixtures must mint ULIDs — a readable " +
      "id in the harness is what made a window printing real ids look correct.",
  );
});

test("looksLikeId matches the core's ULID and nothing a person would type", () => {
  assert.ok(looksLikeId("01K4Z8QH3M7XC9VBN2RTFA6EDS"));
  assert.ok(!looksLikeId("acme"), "a handle is not an id");
  assert.ok(!looksLikeId("acme-2"), "a uniquified handle is not an id");
  assert.ok(!looksLikeId("northwind-renewal"), "a slug is not an id");
  // Crockford omits I, L, O and U so a handwritten id cannot be misread.
  assert.ok(!looksLikeId("01K4Z8QH3M7XC9VBN2RTFA6EDI"), "I is not in the alphabet");
  assert.ok(!looksLikeId("01K4Z8QH3M7XC9VBN2RTFA6ED"), "25 characters is not a ULID");
});
