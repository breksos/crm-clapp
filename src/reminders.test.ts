// The window's words about the timer, held to the CLI's.
//
//     npm test
//
// `crm status` (`cli.rs`, `reminder_lines`) and the desk describe one timer to two audiences.
// If they drift, a person and their agent are told different stories — so every sentence the
// window builds must be a sentence the CLI also says, and the facts must come out right.

import { after, before, describe, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer, type ViteDevServer } from "vite";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const NOW = 1_800_000_000_000;
const MIN = 60_000;

type R = {
  awaiting: number;
  backlog: number;
  everyMinutes: number;
  lastSweepAt: number | null;
  lastSignalAt: number | null;
  lastSignalCount: number;
  refusal: null | { at: number; agent: string; agentName: string | null; reason: string; tasks: number };
};
type View = { checked: string; sent: string; status: null | { kind: string; text: string }; backlog: string | null };

let vite: ViteDevServer;
let reminderView: (r: R, hasAgents: boolean, now?: number) => View;

before(async () => {
  vite = await createServer({
    root: ROOT,
    configFile: join(ROOT, "vite.config.ts"),
    logLevel: "error",
    appType: "custom",
    server: { middlewareMode: true, hmr: false },
  });
  ({ reminderView } = (await vite.ssrLoadModule("/src/reminders.ts")) as { reminderView: typeof reminderView });
});

after(async () => {
  await vite?.close();
});

const quiet: R = { awaiting: 0, backlog: 0, everyMinutes: 5, lastSweepAt: null, lastSignalAt: null, lastSignalCount: 0, refusal: null };

describe("the reminder view", () => {
  test("before the first check it says why, and that nothing has been sent", () => {
    const v = reminderView(quiet, true, NOW);
    assert.match(v.checked, /^Not checked yet — the first check runs moments after the app starts$/);
    assert.equal(v.sent, "Nothing sent yet");
    assert.equal(v.status, null);
    assert.equal(v.backlog, null);
  });

  test("after a sweep and a send it gives the distance, the cadence and the count", () => {
    const v = reminderView({ ...quiet, lastSweepAt: NOW - 7 * MIN, lastSignalAt: NOW - 7 * MIN, lastSignalCount: 1 }, true, NOW);
    assert.equal(v.checked, "Checked 7m ago, every 5 min");
    assert.equal(v.sent, "Last sent 7m ago (1 next step)");
    assert.match(reminderView({ ...quiet, lastSignalAt: NOW - MIN * 90, lastSignalCount: 3 }, true, NOW).sent, /\(3 next steps\)/);
  });

  test("waiting says whether anyone is there to be told", () => {
    assert.equal(reminderView({ ...quiet, awaiting: 1 }, true, NOW).status?.text, "1 due next step, sent at the next check.");
    assert.equal(reminderView({ ...quiet, awaiting: 2 }, false, NOW).status?.text, "2 due next steps, and no agent is connected to tell.");
  });

  test("a refusal outranks waiting, names who by name, and says nobody was told", () => {
    const v = reminderView(
      { ...quiet, awaiting: 2, refusal: { at: NOW, agent: "ag_scout_9f", agentName: "Scout", reason: "inbox_full", tasks: 2 } },
      true,
      NOW,
    );
    assert.equal(v.status?.kind, "refused");
    assert.equal(v.status?.text, "Scout would not take the last one — its inbox is full. Nobody was told; 2 next steps will be tried again at the next check.");
    assert.doesNotMatch(v.status!.text, /ag_scout/, "an id is keyed on; a name is shown");
  });

  test("a refusal from an agent no longer on the roster still reads", () => {
    const v = reminderView({ ...quiet, awaiting: 1, refusal: { at: NOW, agent: "ag_gone", agentName: null, reason: "queue_full", tasks: 1 } }, true, NOW);
    assert.match(v.status!.text, /^an agent would not take the last one — its context queue is full\./);
  });

  test("a reason newer than we know is quoted, not guessed at", () => {
    const v = reminderView({ ...quiet, awaiting: 1, refusal: { at: NOW, agent: "a", agentName: "Scout", reason: "paused", tasks: 1 } }, true, NOW);
    assert.match(v.status!.text, /it said paused/);
  });

  test("the backlog is singular and plural, and says nobody was woken", () => {
    assert.equal(reminderView({ ...quiet, backlog: 1 }, true, NOW).backlog, "1 next step was already overdue when reminders began — nobody was woken for it.");
    assert.equal(reminderView({ ...quiet, backlog: 3 }, true, NOW).backlog, "3 next steps were already overdue when reminders began — nobody was woken for them.");
  });
});

describe("the window says what the CLI says", () => {
  const cli = readFileSync(join(ROOT, "src-tauri/src/cli.rs"), "utf8");
  const lower = (s: string) => s.charAt(0).toLowerCase() + s.slice(1);

  test("every sentence the window builds is one `crm status` also prints", () => {
    const refused = reminderView({ ...quiet, awaiting: 1, refusal: { at: 1, agent: "a", agentName: "Scout", reason: "inbox_full", tasks: 1 } }, true, NOW);
    const parts = [
      lower(reminderView(quiet, true, NOW).checked),
      lower(reminderView(quiet, true, NOW).sent),
      "would not take the last one",
      "its inbox is full",
      "Nobody was told",
      "will be tried again at the next check",
      "and no agent is connected to tell",
      "sent at the next check",
      "already overdue when reminders began",
      "nobody was woken for",
    ];
    for (const p of parts) assert.ok(cli.includes(p), `the window says "${p}" and cli.rs does not`);
    assert.ok(refused.status);
  });
});
