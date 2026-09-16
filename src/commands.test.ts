// The guard on the surface contract. Run it with the runtime that is already on PATH:
//
//     node --test src/
//
// Node 24 strips types itself, so this needs no runner, no config and no dependency — which
// matters, because a test that is awkward to run is a test that stops being run.
//
// Each check is a defect QA actually found:
//
//   1. Every command the window prints appears in the frozen grammar. Round 1 printed seven
//      hard-coded commands using flags nobody had defined.
//   2. No component hard-codes a command string. That is what let those seven drift out of
//      sight of any review of the grammar.
//   3. The invented preview worlds mint ULIDs. Readable fixture ids are why a window that
//      printed `crm task 01K4Z…` looked perfectly correct in the harness.
//   4. Every interpolated value is shell-quoted, and the quoting is checked by a real shell
//      rather than by reading it. Round 2 double-quoted titles, which still expands `$` and
//      backticks.
//   5. The window has no money formatter. Round 2 mirrored `Money::format()`; round 3 made
//      the core the only implementation.
//
// What the window *renders* is checked by `window.test.ts`, against the core's own snapshots.

import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import {
  addCompanyCmd, addDealCmd, findCmd, GRANT_CMD, importCmd, logCmd, selectCmd, shellQuote, showCmd, taskCmd,
} from "./commands.ts";
import { asHandle, looksLikeId } from "./ids.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");

const H = asHandle("acme");

/** Every command this window is capable of printing, rendered. */
const RENDERED = [
  showCmd(H),
  findCmd("acme"),
  addDealCmd("Northwind renewal"),
  addCompanyCmd("Acme Corp"),
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
  let last: string[] | null = null;
  for (const line of block.split("\n")) {
    const trimmed = line.trim();
    const m = trimmed.match(/^crm\s+(\S+)/);
    if (m) {
      last = byVerb.get(m[1]) ?? [];
      last.push(trimmed);
      byVerb.set(m[1], last);
    } else if (last && trimmed.startsWith("[")) {
      // A wrapped usage line — `crm find` spills its options onto the next line. Dropping it
      // would make `--page` look undeclared.
      last[last.length - 1] += ` ${trimmed}`;
    } else {
      last = null;
    }
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
    for (const flag of shellWords(command).filter((w) => w.startsWith("--"))) {
      assert.ok(
        declared.includes(flag),
        `"${command}" passes ${flag} to \`crm ${verb}\`, which the grammar does not declare.`,
      );
    }
  }
});

test("no printed command carries anything id-shaped", () => {
  const commands = RENDERED.map((c) => [c, shellWords(c)] as const);
  // GRANT_CMD is a template with a `<name>` placeholder the person fills in, which a real
  // shell would read as a redirect — so it is split on whitespace rather than executed.
  commands.push([GRANT_CMD, GRANT_CMD.split(/\s+/)]);
  for (const [command, words] of commands) {
    for (const word of words) {
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

test("the invented preview worlds mint ULIDs, not readable ids", () => {
  let source = readFileSync(join(ROOT, "src/scenarios.ts"), "utf8");

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

// MARK: - Shell quoting

/** How a real POSIX shell splits `command` into words — the ground truth for what an agent
 *  pasting it would actually send. */
function shellWords(command: string): string[] {
  const out = execFileSync("/bin/sh", ["-c", `for w in ${command}; do printf '%s\\0' "$w"; done`], {
    encoding: "utf8",
  });
  return out.split("\0").slice(0, -1);
}

const HOSTILE = [
  "two words",
  'say "hi"',
  "O'Hara",
  "costs $HOME",
  "run `id` now",
  // …and all five at once, because they interact.
  `O'Hara "Q4" $upsell \`now\` & more`,
];

test("shellQuote leaves typable values bare", () => {
  for (const bare of ["acme", "acme-2", "northwind.renewal", "2026-09-30", "contacts.csv", "A_b-9"]) {
    assert.equal(shellQuote(bare), bare);
  }
});

test("shellQuote survives a real shell: space, quote, apostrophe, dollar, backtick", () => {
  for (const value of HOSTILE) {
    const quoted = shellQuote(value);
    // Single quotes, never double: inside double quotes a shell still expands `$` and
    // backticks, which is the round-2 bug.
    assert.ok(quoted.startsWith("'"), `${value} should be single-quoted, got ${quoted}`);
    const echoed = execFileSync("/bin/sh", ["-c", `printf '%s' ${quoted}`], { encoding: "utf8" });
    assert.equal(echoed, value, `the shell received something other than ${JSON.stringify(value)}`);
  }
});

test("every builder quotes what it interpolates — the shell sees each value as one word", () => {
  const nasty = HOSTILE[HOSTILE.length - 1];
  const cases: [string, string[]][] = [
    [addDealCmd(nasty), ["crm", "add", "deal", nasty]],
    [addCompanyCmd(nasty), ["crm", "add", "company", nasty]],
    [findCmd(nasty), ["crm", "find", nasty]],
    [taskCmd(H, nasty, "2026-09-30"), ["crm", "task", "acme", nasty, "--due", "2026-09-30"]],
    [logCmd("note", H, nasty), ["crm", "log", "note", "acme", nasty]],
    [importCmd("my contacts.csv"), ["crm", "import", "my contacts.csv"]],
  ];
  for (const [command, words] of cases) {
    assert.deepEqual(shellWords(command), words, command);
  }
});

// MARK: - No second money formatter

test("the window formats no money — it renders the core's `formatted`", () => {
  const offenders: string[] = [];
  for (const file of readdirSync(join(ROOT, "src"))) {
    // The invented preview worlds need *a* formatted string to carry; that stand-in lives in
    // scenarios.ts and never reaches the shipped bundle. Tests are not the window either.
    if (!/\.(ts|tsx)$/.test(file) || file === "scenarios.ts" || file.includes(".test.")) continue;
    const source = readFileSync(join(ROOT, "src", file), "utf8");
    // `toLocaleString` is deliberately not on this list: a timeline entry's date tooltip uses
    // it, and dates are not money. Arithmetic on `amount` is what a formatter needs.
    if (/NumberFormat|toFixed\(|formatMoney|\.amount\b/.test(source)) offenders.push(file);
  }
  assert.deepEqual(offenders, [], "a money formatter, or arithmetic on `amount`, is back in the window");
});
