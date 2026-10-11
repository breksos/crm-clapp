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
import { existsSync, readdirSync, readFileSync } from "node:fs";
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
const GOLDEN_FILES = ["bootstrap.json", "bootstrap-pending.json"];

type View = "home" | "inbox" | "pipeline" | "deals" | "contacts" | "reports" | "team" | "settings";
const ALL_VIEWS: View[] = ["home", "inbox", "pipeline", "deals", "contacts", "reports", "team", "settings"];
// Loose on purpose: these modules arrive through Vite at runtime, and the test checks their
// output, not their types — `tsc` already checks those.
type Snap = any; // eslint-disable-line @typescript-eslint/no-explicit-any

let vite: ViteDevServer;
let Window: (props: object) => unknown;
let split: { fromBootstrap: (b: Snap) => Snap; toSnapshot: (s: Snap) => Snap; applyUpdate: (s: Snap, u: Snap) => { state: Snap } };
let SCENARIOS: Record<string, { label: string; build: () => Snap; then?: { apply: (s: Snap) => Snap } }>;

/** A golden fixture as the window holds it: the core's own `bootstrap`, folded by the reducer the
 *  app ships with. Read from disk verbatim — not a world this suite invented. */
function golden(file = "bootstrap.json"): Snap {
  return split.toSnapshot(split.fromBootstrap(JSON.parse(readFileSync(join(FIXTURES, "split", file), "utf8"))));
}

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
  split = (await vite.ssrLoadModule("/src/split.ts")) as typeof split;
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
  for (const view of ALL_VIEWS) {
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
        existsSync(join(FIXTURES, "split", file)),
        `${join(FIXTURES, "split", file)} is missing. The window is tested against the core's own ` +
          `output; without it, this suite is back to testing a guess.`,
      );
    }
  });

  for (const file of GOLDEN_FILES) {
    test(`${file}: no id anywhere in any view, and every \`crm show\` names a real record`, () => {
      assertClean(file, load(file));
    });
  }

  test("bootstrap.json: the board shows every card's title, company and value", () => {
    const state = load("bootstrap.json");
    const shown = text(render(state, "pipeline"));
    for (const card of Object.values(state.cards) as Snap[]) {
      assert.ok(shown.includes(card.label), `the card "${card.label}" is not on the board`);
      if (card.detail) assert.ok(shown.includes(card.detail), `"${card.label}" lost its company`);
      if (card.value) assert.ok(shown.includes(card.value.formatted), `"${card.label}" lost its value`);
    }
  });

  test("bootstrap.json: column totals are the core's formatted strings", () => {
    const state = load("bootstrap.json");
    const shown = text(render(state, "pipeline"));
    for (const column of state.board.columns) {
      for (const total of column.totals) assert.ok(shown.includes(total.formatted), `${column.label}: ${total.formatted}`);
    }
  });

  test("bootstrap.json: the open record, with the core's fields in the core's order", () => {
    const state = load("bootstrap.json");
    assert.ok(state.focused, "the fixture has a record open — that is what this checks");
    const shown = text(render(state, "pipeline"));

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

  test("bootstrap.json: every timeline line and every task is on screen", () => {
    const state = load("bootstrap.json");
    const shown = text(render(state, "pipeline"));
    for (const a of state.focused.timeline) assert.ok(shown.includes(a.body), `timeline line missing: ${a.body}`);
    for (const t of state.focused.tasks) assert.ok(shown.includes(t.what), `task missing: ${t.what}`);
  });

  test("bootstrap-pending.json: the question and every candidate are on screen", () => {
    const state = load("bootstrap-pending.json");
    const shown = text(render(state, "pipeline"));
    assert.ok(state.pending, "the fixture has a pending question — that is what this checks");
    assert.ok(shown.includes(state.pending.prompt), "the prompt is missing");
    for (const c of state.pending.candidates) {
      assert.ok(shown.includes(c.label), `candidate missing: ${c.label}`);
      assert.ok(shown.includes(c.handle), `candidate ${c.label} does not show its handle`);
    }
  });

  test("a question whose write was lost offers no click and says so", () => {
    const state = load("bootstrap-pending.json");
    const lost = { ...state, pending: { ...state.pending, resuming: false, lost: true } };
    const html = render(lost, "pipeline");
    const shown = text(html);
    assert.ok(shown.includes("write was lost — run it again"), "the window must say the write is gone");
    assert.equal((html.match(/class="candidate"/g) ?? []).length, 0, "no candidate may invite a click that does nothing");
    // The shared list beside it still shows these records (they are real); the *question* must not.
    const banner = /<section class="pending[^"]*"[\s\S]*?<\/section>/.exec(html)?.[0] ?? "";
    assert.ok(banner.includes("pending-lost"), "the lost question has its own banner");
    for (const c of state.pending.candidates) {
      assert.ok(!banner.includes(c.handle), `${c.handle} is offered as if it could be picked`);
    }
    assert.ok(!shown.includes("from the terminal"), "and no `crm select` hint that would be refused");
    // The ordinary question is untouched.
    const ordinary = render(state, "pipeline");
    assert.ok((ordinary.match(/class="candidate"/g) ?? []).length > 0);
  });

  test("a card whose body is missing is a skeleton, not an id", () => {
    // Round 3's blocker: the window used to fall back to printing the id. Take the real
    // snapshot, drop every body, and check nothing id-shaped comes out.
    const state = { ...load("bootstrap.json"), cards: {} };
    const html = render(state, "pipeline");
    assert.deepEqual(findIds(html), []);
    const deals = state.board.columns.reduce((n: number, c: Snap) => n + c.dealIds.length, 0);
    assert.equal((html.match(/card-skeleton/g) ?? []).length, deals, "one skeleton per deal on the board");
  });

  function load(file: string): Snap {
    return golden(file);
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
    const html = render(empty.build(), "pipeline");
    const shown = commands(html).filter((c) => c.startsWith("crm "));
    assert.ok(shown.length > 0, "the empty state should still say what to type");
    for (const c of shown) {
      assert.match(c, /^crm (add|import) /, `"${c}" is shown with no records, and it names one`);
    }
  });

  test("a hostile title renders verbatim", () => {
    const long = Object.values(SCENARIOS).find((s) => s.label === "Long text")!;
    const html = render(long.build(), "pipeline");
    assert.ok(text(html).includes(`O'Hara "Q4" $upsell \`now\``), "the hostile title is not rendered verbatim");
  });

  test("a hostile search is echoed as a command a real shell reads back intact", () => {
    // The one place a person's own text flows into a printed command: an empty result list
    // echoes the query as `crm find …`. That is where quoting has to hold.
    const query = `O'Hara "Q4" $HOME \`id\``;
    const base = Object.values(SCENARIOS).find((s) => s.label === "Pipeline")!.build();
    const state = { ...base, list: { ...base.list, query, rows: [], total: 0 } };
    const shown = commands(render(state, "contacts")).find((c) => c.startsWith("crm find "));
    assert.ok(shown, "the empty list should echo the search as a command");
    const words = execFileSync("/bin/sh", ["-c", `for w in ${shown}; do printf '%s\\0' "$w"; done`], {
      encoding: "utf8",
    }).split("\0").slice(0, -1);
    assert.deepEqual(words, ["crm", "find", query]);
  });
});

// MARK: - The agent desk

describe("the agent desk", () => {

  test("is on every page", () => {
    for (const view of ALL_VIEWS) {
      assert.match(render(golden(), view), /aria-label="Agent desk"/, `no desk on ${view}`);
    }
  });

  test("names the bound agents and only the moves the snapshot records", () => {
    const state = golden();
    const html = render(state, "pipeline");
    const shown = text(html);
    for (const a of state.agents) assert.ok(shown.includes(a.name), `${a.name} is missing from the desk`);
    // A move by the person is not an agent move: no desk line names "You".
    assert.doesNotMatch(html, /desk-move[^>]*>[^]*?<b>You<\/b>/);
  });

  test("says when nothing is waiting, and draws no approval workflow the core does not have", () => {
    const state = { ...golden(), pending: null };
    const html = render(state, "pipeline");
    assert.match(text(html), /Nothing is waiting on you/);
    assert.doesNotMatch(html, /approve|approval|Review|Undo|Keep/i);
  });

  test("shows the pending question when there is one", () => {
    const pending = golden("bootstrap-pending.json");
    assert.match(text(render(pending, "pipeline")), new RegExp(pending.pending.prompt.slice(0, 20)));
  });

  test("an agent move draws the agent's stripe, and a stage stripe is gone", () => {
    const html = render(golden(), "pipeline");
    assert.match(html, /card-agent/);
    assert.doesNotMatch(html, /data-stage/);
  });
});

// MARK: - The timer on the desk

describe("the reminders on the desk", () => {

  test("the core's own snapshot shows the timer, in the core's words", () => {
    const shown = text(render(golden(), "pipeline"));
    assert.match(shown, /Reminders/);
    assert.match(shown, /Not checked yet/);
    assert.match(shown, /Nothing sent yet/);
    assert.match(shown, /1 due next step, sent at the next check/);
  });

  test("a snapshot with no `reminders` says nothing about them, rather than claim a state", () => {
    const state = golden();
    delete state.reminders;
    assert.doesNotMatch(render(state, "pipeline"), /aria-label="Reminders"/);
  });

  test("a refusal is an alert, names the agent, and prints no id", () => {
    const html = render(SCENARIOS.refused.build(), "pipeline");
    assert.match(html, /role="alert"/);
    assert.match(text(html), /Pilot would not take the last one — its inbox is full/);
    assert.deepEqual(findIds(html), []);
  });

  test("the first-run world shows the backlog and where to list it", () => {
    const html = render(SCENARIOS.firstRun.build(), "pipeline");
    assert.match(text(html), /3 next steps were already overdue when reminders began/);
    assert.ok(commands(html).includes("crm due"));
  });

  test("it never claims delivery", () => {
    for (const s of [golden(), SCENARIOS.refused.build(), SCENARIOS.firstRun.build(), SCENARIOS.pipeline.build()]) {
      assert.doesNotMatch(text(render(s, "pipeline")), /\bdelivered\b|\bconfirmed\b|\breceived\b/i);
    }
  });
});

// MARK: - M12: the shell and its pages

describe("the shell", () => {
  const NAV = ["Home", "Inbox", "Pipeline", "Deals", "Contacts & companies", "Reports", "Team", "Settings"];

  test("every page is in the sidebar, in order, as a real button", () => {
    const html = render(golden(), "home");
    const labels = [...html.matchAll(/<button type="button" class="nav-item"[^>]*><svg[^>]*>.*?<\/svg><span class="nav-label">(.*?)<\/span>/g)].map((m) => decode(m[1]));
    assert.deepEqual(labels.slice(0, NAV.length), NAV);
  });

  test("the page you are on is named in the header and marked current", () => {
    const titles: Record<View, string> = {
      home: "Home", inbox: "Inbox", pipeline: "Pipeline", deals: "Deals", contacts: "Contacts & companies",
      reports: "Reports", team: "Team", settings: "Settings",
    };
    for (const view of ALL_VIEWS) {
      const html = render({ ...golden(), list: { ...golden().list, kind: view === "deals" ? "deal" : view === "contacts" ? "company" : null } }, view);
      assert.match(text(html), new RegExp(`Breksos CRM\\s+${titles[view].replace("&", "&")}\\b`), `${view} is not named in the header`);
      assert.equal((html.match(/aria-current="page"/g) ?? []).length, 1, `${view}: exactly one nav entry is current`);
    }
  });

  test("⌘K search is a labelled combobox, and saved views are there, built in", () => {
    const html = render(golden(), "home");
    assert.match(html, /role="combobox"[^>]*aria-label="Search companies, people and deals"/);
    for (const name of ["Biggest deals", "Companies A–Z", "People A–Z"]) assert.match(text(html), new RegExp(name));
  });

  test("saved views never enter the snapshot", () => {
    for (const s of Object.values(SCENARIOS)) {
      const wire = JSON.stringify(s.build());
      assert.doesNotMatch(wire, /savedViews|Biggest deals|breksos\.savedViews/);
    }
  });
});

describe("Home", () => {

  test("shows counts the snapshot holds, and only money the core formatted", () => {
    const state = golden();
    const html = render(state, "home");
    const shown = text(html);
    const known = new Set<string>();
    for (const c of state.board.columns) for (const t of c.totals) known.add(t.formatted);
    for (const c of Object.values(state.cards) as Snap[]) if (c.value) known.add(c.value.formatted);
    // The pipeline totals (gap 3) are the core's own, per currency, already formatted.
    for (const k of ["open", "won", "lost"]) for (const t of state.pipelineTotals?.[k] ?? []) known.add(t.formatted);
    const money = shown.match(/[$€£¥]\s?[\d,]+(?:\.\d+)?/g) ?? [];
    // Home draws no money the window itself made: nothing summed, nothing formatted here.
    const homeOnly = text(html.slice(html.indexOf('class="page home"'), html.indexOf("</main>")));
    for (const m of homeOnly.match(/[$€£¥]\s?[\d,]+(?:\.\d+)?/g) ?? []) {
      assert.ok([...known].some((k) => k.includes(m)), `Home shows "${m}", which is not a string the core sent`);
    }
    assert.ok(money.length >= 0);
    assert.match(homeOnly, new RegExp(`Companies\\s+${state.counts.companies}\\b`));
    assert.match(homeOnly, new RegExp(`People\\s+${state.counts.contacts}\\b`));
  });

  test("says what needs the person from `due`, and points at the record for the rest", () => {
    const html = render({ ...golden(), due: { overdue: 2, today: 1, week: 5 } }, "home");
    assert.match(text(html), /2 next steps overdue/);
    assert.match(text(html), /1 next step due today/);
    assert.match(text(html), /Open a deal to see its next steps/);
  });

  test("a quiet world says so", () => {
    const html = render({ ...golden(), due: { overdue: 0, today: 0, week: 0 }, pending: null, reminders: null }, "home");
    assert.match(text(html), /Nothing needs you today/);
  });
});

/** The split shape's bootstrap, as the components see it. */
const splitSnap = (file = "split/bootstrap.json"): Snap =>
  split.toSnapshot(split.fromBootstrap(JSON.parse(readFileSync(join(FIXTURES, file), "utf8"))));

describe("Inbox", () => {
  test("is one feed for people and agents, filterable by who and what, never two lanes", () => {
    const html = render(splitSnap(), "inbox");
    const shown = text(html);
    for (const chip of ["All types", "Calls", "Emails", "Meetings", "Notes", "Stage changes"]) assert.match(shown, new RegExp(chip));
    for (const who of ["Everyone", "You"]) assert.match(shown, new RegExp(who));
    assert.doesNotMatch(html, /people lane|agent lane/i);
    assert.deepEqual(findIds(html), []);
  });

  test("shows the core's feed, one line per item, newest first, with the first page's total", () => {
    const state = splitSnap();
    const html = render(state, "inbox");
    assert.equal((html.match(/class="feed-row"/g) ?? []).length, state.feed.items.length);
    assert.match(text(html), new RegExp(`${state.feed.total} entries`));
    const whens = [...html.matchAll(/datetime="([^"]+)"/g)].map((m) => Date.parse(m[1]));
    assert.deepEqual(whens, [...whens].sort((a, b) => b - a), "newest first");
  });

  test("a stage change reads as the move it was, not as an empty line", () => {
    const html = text(render(splitSnap(), "inbox"));
    assert.match(html, /Moved [A-Za-z ]+ to (Qualified|Proposal|Negotiation|Lead|Won|Lost)/);
  });

  test("before the first bootstrap there is no feed, and the Inbox says so rather than draw an empty page", () => {
    const state = golden();
    delete state.feed;
    assert.match(text(render(state, "inbox")), /feed is not available from this core/);
  });
});

describe("the pages that wait on the platform", () => {

  test("Reports and Team say one true thing and draw nothing else", () => {
    for (const view of ["reports", "team"] as View[]) {
      const html = render(golden(), view);
      const main = html.slice(html.indexOf("<main"), html.indexOf("</main>"));
      assert.match(text(main), /arrive with the platform|are on the desk/);
      assert.doesNotMatch(main, /<svg|<canvas|<table|lorem|spinner/i, `${view} drew more than a sentence`);
    }
    assert.match(text(render(golden(), "reports")), /arrive with the platform/);
  });

  test("Settings has the theme and only the theme", () => {
    const html = render(golden(), "settings");
    const main = html.slice(html.indexOf("<main"), html.indexOf("</main>"));
    for (const t of ["System", "Light", "Dark"]) assert.match(text(main), new RegExp(t));
    assert.match(text(main), /Roles and pipeline editing arrive with the platform/);
  });
});

describe("the list pages", () => {

  test("Deals is about deals: no kind switcher", () => {
    const html = render({ ...golden(), list: { ...golden().list, kind: "deal" } }, "deals");
    assert.doesNotMatch(html, /aria-label="Show"/);
  });

  test("Contacts & companies offers exactly those two kinds", () => {
    const html = render({ ...golden(), list: { ...golden().list, kind: "company" } }, "contacts");
    const group = html.slice(html.indexOf('aria-label="Show"'));
    assert.match(group, /People/);
    assert.match(group.slice(0, group.indexOf("</div>")), /Companies/);
    assert.doesNotMatch(group.slice(0, group.indexOf("</div>")), />Deals</);
  });

  test("when the agent clears the kind, the page says so and offers the way back", () => {
    const html = render({ ...golden(), list: { ...golden().list, kind: null } }, "deals");
    assert.match(text(html), /The shared list is showing every kind of record/);
    assert.match(text(html), /Show deals/);
  });

  test("when the agent narrows the list to another page's kind, the highlight follows it", () => {
    const html = render({ ...golden(), list: { ...golden().list, kind: "contact" } }, "deals");
    assert.match(text(html), /Breksos CRM\s+Contacts & companies/);
  });

});

// MARK: - QA round 6c: a refusal reaches the person, and a reminder can be sent again

describe("refusals and resend", () => {
  test("no component sends a data-changing envelope through the fire-and-forget `run`", () => {
    // `run` drops `{ ok:false }` (clappkit's apply returns early), so a refusal sent through it
    // reaches nobody. 6c found three controls doing exactly that; this keeps the fourth out.
    // Only view state — `find`, `show` — may use it.
    const dir = join(ROOT, "src");
    const offenders: string[] = [];
    for (const file of readdirSync(dir).filter((f) => f.endsWith(".tsx"))) {
      const src = readFileSync(join(dir, file), "utf8");
      for (const m of src.matchAll(/\brun\(\s*\{\s*cmd:\s*"([a-z]+)"/g)) {
        if (m[1] !== "find" && m[1] !== "show") offenders.push(`${file}: run({ cmd: "${m[1]}" })`);
      }
    }
    assert.deepEqual(offenders, []);
  });

  test("with reminders sent and still open, the desk offers to send them again, beside the count", () => {
    const html = render(SCENARIOS.pipeline.build(), "home");
    assert.match(text(html), /2 next steps still open — the platform does not confirm delivery/);
    assert.match(text(html), /Send them again/);
  });

  test("a single open reminder says 'it'; none open offers nothing", () => {
    const one = SCENARIOS.pipeline.build();
    one.reminders = { ...one.reminders, unconfirmed: 1 };
    assert.match(text(render(one, "home")), /Send it again/);
    const none = SCENARIOS.pipeline.build();
    none.reminders = { ...none.reminders, unconfirmed: 0 };
    assert.doesNotMatch(text(render(none, "home")), /Send (it|them) again/);
    const absent = SCENARIOS.pipeline.build();
    delete absent.reminders.unconfirmed;
    assert.doesNotMatch(text(render(absent, "home")), /Send (it|them) again/);
  });

  test("it never claims the agent saw it", () => {
    for (const s of Object.values(SCENARIOS)) {
      assert.doesNotMatch(text(render(s.build(), "home")), /\bdelivered\b|\bconfirmed\b|\breceived\b|\backnowledged\b/i);
    }
  });
});

// MARK: - p1: the window on the split shape

describe("the split shape, rendered", () => {
  const FILES = ["split/bootstrap.json", "split/bootstrap-pending.json"];

  for (const file of FILES) {
    test(`${file}: no id in any page, and every \`crm show\` names a real record`, () => {
      assertClean(file, splitSnap(file));
    });
  }

  test("after the core's own move update, the board shows the deal where the update says", () => {
    const boot = JSON.parse(readFileSync(join(FIXTURES, "split/bootstrap.json"), "utf8"));
    const update = JSON.parse(readFileSync(join(FIXTURES, "split/update-move.json"), "utf8"));
    const after = split.toSnapshot(split.applyUpdate(split.fromBootstrap(boot), update).state);
    assert.deepEqual(findIds(render(after, "pipeline")), []);
    assert.equal(after.watermark, update.delta.to);
    assertClean("after update-move", after);
  });
});

describe("Home on the split shape", () => {
  test("shows the open pipeline per currency exactly as the core formatted it, and sums nothing", () => {
    const state = splitSnap();
    const home = text(render(state, "home"));
    for (const m of state.pipelineTotals.open) assert.ok(home.includes(m.formatted), `${m.formatted} is missing from Home`);
    assert.ok(state.pipelineTotals.open.length >= 2, "the fixture is multi-currency, which is the point");
    const shown = home.match(/[$€£¥]\s?[\d,]+(?:\.\d+)?/g) ?? [];
    const sent = new Set<string>();
    for (const t of [...state.pipelineTotals.open, ...state.pipelineTotals.won, ...state.pipelineTotals.lost]) sent.add(t.formatted);
    for (const c of state.board.columns) for (const t of c.totals) sent.add(t.formatted);
    for (const card of Object.values(state.cards) as Snap[]) if (card.value) sent.add(card.value.formatted);
    for (const m of shown) assert.ok([...sent].some((k) => k.includes(m)), `Home shows ${m}, which the core never sent`);
  });

  test("without a cross-record task list yet, it falls back to the three counts it has", () => {
    // Server-side rendering runs no effects, so the `tasks` answer has not arrived: Home must
    // still say something true from `due`.
    assert.match(text(render({ ...splitSnap(), due: { overdue: 2, today: 1, week: 5 } }, "home")), /2 next steps overdue/);
  });
});

describe("⌘K (p1, gap 4)", () => {
  test("is driven by `search` and never writes the shared list", () => {
    const src = readFileSync(join(ROOT, "src/Search.tsx"), "utf8");
    assert.match(src, /cmd:\s*"search"/);
    assert.doesNotMatch(src, /cmd:\s*"find"/, "the palette must not write the list");
    assert.doesNotMatch(src, /list\.(query|kind|page)/, "and must have nothing to restore");
  });

  test("the Inbox and Home read their cross-record data from stateless envelopes", () => {
    assert.match(readFileSync(join(ROOT, "src/Inbox.tsx"), "utf8"), /cmd:\s*"feed"/);
    assert.match(readFileSync(join(ROOT, "src/Home.tsx"), "utf8"), /cmd:\s*"tasks"/);
  });
});

// MARK: - W1 B1: the record panel collapses

describe("the record panel's collapse control", () => {
  const golden = (): Snap => splitSnap();

  test("an open record has a collapse control with its shortcut, and starts expanded", () => {
    const html = render(golden(), "pipeline");
    assert.match(html, /aria-label="Collapse record panel \(⌘\\\)"[^>]*aria-expanded="true"/);
    assert.match(html, /class="record"/);
    assert.doesNotMatch(html, /record-rail/);
  });

  test("with nothing open the panel can still be collapsed", () => {
    assert.match(render({ ...golden(), focus: null, focused: null }, "pipeline"), /Collapse record panel/);
  });

  test("the collapsed rail names the record, shows the dot only when told, and offers to expand", async () => {
    const { RecordPanel } = (await vite.ssrLoadModule("/src/Record.tsx")) as { RecordPanel: (p: object) => unknown };
    const s = golden();
    const rail = (unread: boolean) =>
      renderToStaticMarkup(
        createElement(RecordPanel as never, {
          focused: s.focused, agents: s.agents, example: null, known: [], apply: () => {}, collapsed: true, unread, onToggle: () => {},
        }),
      );
    const quiet = rail(false);
    assert.match(quiet, /record-rail/);
    assert.ok(text(quiet).includes(s.focused.row.label));
    assert.doesNotMatch(quiet, /rail-dot/);
    assert.match(quiet, /aria-expanded="false"/);
    const loud = rail(true);
    assert.match(loud, /rail-dot/);
    assert.match(loud, /an agent changed it/);
    assert.deepEqual(findIds(loud), []);
  });

  test("nothing in the window offers to ask the agent", () => {
    for (const view of ALL_VIEWS) assert.doesNotMatch(text(render(golden(), view)), /ask (the |your )?agent/i);
  });
});

// MARK: - W1 B2/B3: the agent's steps, on screen

describe("the agent activity strip", () => {
  const withFeed = (feed: unknown[], over: Record<string, unknown> = {}): Snap => ({ ...splitSnap(), agentFeed: feed, ...over });
  const call = (seq: number, over: Record<string, unknown> = {}) => ({
    seq, at: Date.now() - seq * 1000, actor: { kind: "agent", id: splitSnap().agents[0].id }, verb: "move", line: "move orbit won", outcome: "ok",
    target: { kind: "deal", id: "01M207XY60B9D5MPJTB9D5MPJT", handle: "orbit" }, ...over,
  });

  test("renders every outcome, one line each, newest at the top", () => {
    const html = render(withFeed([call(3, { outcome: "asked", verb: "log", line: "log call acme x", target: undefined }), call(2, { outcome: "refused", message: "crm: no such stage", line: "move orbit wonn" }), call(1)]), "home");
    const lines = [...html.matchAll(/data-outcome="(ok|refused|asked)"/g)].map((m) => m[1]);
    assert.deepEqual(lines, ["asked", "refused", "ok"]);
    assert.match(text(html), /log call acme x/);
    assert.deepEqual(findIds(html), []);
  });

  test("a refusal is styled as an error and carries the core's sentence verbatim", () => {
    const html = render(withFeed([call(1, { outcome: "refused", message: "crm: no such stage — stages: lead, won" })]), "home");
    assert.match(html, /class="call-refused" role="alert"[^>]*>crm: no such stage — stages: lead, won</);
  });

  test("a line with a target opens it; a line about nothing is not a button", () => {
    const html = render(withFeed([call(2, { target: undefined, verb: "board", line: "board" }), call(1)]), "home");
    assert.equal((html.match(/<button[^>]*class="call"/g) ?? []).length, 1);
    assert.match(html, /Open orbit/);
  });

  test("says it is not persisted, and what it is when nothing has happened", () => {
    assert.match(text(render(withFeed([call(1)]), "home")), /Since the app started/);
    assert.match(text(render(withFeed([]), "home")), /Nothing yet\. What your agent runs shows here/);
  });

  test("the follow toggle reflects the core's `follow`, and is a labelled control", () => {
    assert.match(render(withFeed([], { follow: true }), "home"), /type="checkbox" checked=""/);
    assert.doesNotMatch(render(withFeed([], { follow: false }), "home"), /type="checkbox" checked=""/);
    assert.match(text(render(withFeed([]), "home")), /Follow agent/);
  });

  test("there is no 'ask the agent' control anywhere, with a full feed", () => {
    for (const view of ALL_VIEWS) assert.doesNotMatch(text(render(withFeed([call(1)]), view)), /ask (the |your )?agent/i);
  });
});

// MARK: - p2: the account

describe("the account, in the window", () => {
  const signedIn = (over: Record<string, unknown> = {}) => ({
    status: { signedIn: true, server: "https://crm.example.com", person: { id: "usr_1", name: "Ayşe Demir" }, org: { id: "org_1", name: "Demir & Co" }, role: "owner", expiresAt: 1, insecure: false, ...over },
    known: true, busy: false, error: null, dismissError: () => {},
    signUp: async () => false, signIn: async () => false, acceptInvite: async () => false, signOut: async () => ({ ok: true, warning: null }),
  });
  const signedOut = () => ({ ...signedIn(), status: { signedIn: false, server: null, person: null, org: null, role: null, expiresAt: null, insecure: false } });
  const draw = (account: unknown, view: View, state: Snap = splitSnap()) =>
    renderToStaticMarkup(
      createElement(Window as never, { state, view, run: () => {}, apply: () => {}, setView: () => {}, theme: "system", chooseTheme: () => {}, account }),
    );

  test("signed out, Settings offers sign in, sign up and an invite — and says local mode needs none of it", () => {
    const html = draw(signedOut(), "settings");
    for (const w of ["Sign in", "Create account", "Join with an invite"]) assert.match(text(html), new RegExp(w));
    assert.match(text(html), /works exactly as it always has, on this machine, with no account/);
    assert.match(html, /type="password"[^>]*autoComplete="current-password"|autoComplete="current-password"[^>]*type="password"/i);
    assert.doesNotMatch(html, /Organisation/);
  });

  test("with no account machinery at all, the window is local mode and nothing about an account is invented", () => {
    const html = render(splitSnap(), "settings");
    assert.match(text(html), /Account/);
    assert.doesNotMatch(html, /insecure-banner|Organisation/);
  });

  test("signed in, Settings shows your name, the org's name and a way to sign out — and no token, no password", () => {
    const html = draw(signedIn(), "settings");
    const shown = text(html);
    assert.match(shown, /Ayşe Demir/);
    assert.match(shown, /Demir & Co/);
    assert.match(shown, /Sign out/);
    assert.match(shown, /Create invite/);
    assert.doesNotMatch(html, /type="password"/, "nothing asks for a password once signed in");
    assert.doesNotMatch(shown, /token|Bearer/i);
  });

  test("an invite's token is not on the page until one is made", () => {
    assert.doesNotMatch(draw(signedIn(), "settings"), /invite-token/);
  });

  test("plain HTTP is loud, on every page, and https is quiet", () => {
    for (const view of ALL_VIEWS) {
      assert.match(draw(signedIn({ insecure: true }), view), /class="insecure-banner" role="alert"/, `${view}: no warning`);
      assert.doesNotMatch(draw(signedIn({ insecure: false }), view), /insecure-banner/, `${view}: warned about a secure server`);
    }
    assert.match(text(draw(signedIn({ insecure: true }), "home")), /Not secure\.\s+You are signed in to https:\/\/crm\.example\.com over plain HTTP/);
  });

  test("signed out there is nothing to be insecure about", () => {
    assert.doesNotMatch(draw({ ...signedOut(), status: { ...signedOut().status, insecure: true } }, "home"), /insecure-banner/);
  });

  test("the person's real name is on every attribution once signed in — and 'You' while signed out", () => {
    const state = splitSnap();
    state.me = { id: "usr_1", name: "Ayşe Demir" };
    const withHuman = (a: Snap) => ({ ...a, agentFeed: [{ seq: 1, at: Date.now(), actor: { kind: "human", id: "usr_1" }, verb: "x", line: "x", outcome: "ok" }] });
    for (const view of ["home", "inbox", "pipeline"] as View[]) {
      assert.match(text(draw(signedIn(), view, withHuman(state))), /Ayşe Demir/, `${view}: the name is missing`);
    }
    const local = text(draw(signedOut(), "inbox", withHuman(state)));
    assert.doesNotMatch(local, /Ayşe Demir/, "a signed-out window must not use an account's name");
    assert.match(local, /\bYou\b/);
  });

  test("Team lists nothing it was not told, and signed out explains itself", () => {
    assert.match(text(draw(signedOut(), "team")), /sign in under Settings/);
  });

  test("account, sign-in and invites are not reachable from the command line grammar the window prints", () => {
    for (const view of ALL_VIEWS) {
      const cmds = commands(draw(signedIn(), view)).join("\n");
      assert.doesNotMatch(cmds, /sign|invite|password|account/i);
    }
  });
});

// MARK: - p3: the team

describe("the team, in the window", () => {
  const team = (over: Record<string, unknown> = {}): Snap => ({
    ...splitSnap(),
    me: { id: "me", name: "Berk Öge" },
    sync: { state: "online", lastSeen: Date.now() },
    members: [{ id: "me", name: "Berk Öge" }, { id: "ana", name: "Ana Torres" }],
    teamAgents: [{ id: "ag_ana_scout", name: "Scout", ownerId: "ana" }],
    ...over,
  });
  const account = {
    status: { signedIn: true, server: "https://crm.example.com", person: { id: "me", name: "Berk Öge" }, org: { id: "o", name: "Demir & Co" }, role: "owner", expiresAt: 1, insecure: false },
    known: true, busy: false, error: null, dismissError: () => {},
    signUp: async () => false, signIn: async () => false, acceptInvite: async () => false, signOut: async () => ({ ok: true, warning: null }),
  };
  const draw = (state: Snap, view: View = "home") =>
    renderToStaticMarkup(createElement(Window as never, { state, view, run: () => {}, apply: () => {}, setView: () => {}, theme: "system", chooseTheme: () => {}, account }));

  test("local mode draws no sync indicator and no join choice", () => {
    const html = render(splitSnap(), "home");
    assert.doesNotMatch(html, /class="sync /);
    assert.doesNotMatch(html, /Choose how to join/);
    assert.doesNotMatch(draw(team({ sync: { state: "local", lastSeen: null } })), /class="sync /);
  });

  test("online is quiet; syncing says so; offline says how long and that nothing is queued", () => {
    assert.match(draw(team()), /class="sync sync-online"[^>]*>.*Online/s);
    assert.match(draw(team({ sync: { state: "syncing", lastSeen: Date.now() } })), /Syncing…/);
    const off = draw(team({ sync: { state: "offline", lastSeen: Date.now() - 90_000 } }));
    assert.match(off, /class="sync sync-offline"/);
    assert.match(text(off), /Offline\s+last heard 2m ago/);
    assert.match(off, /changes are refused, not queued/);
  });

  test("while choosing, the person is asked how to join — with both sides' counts and the backup promise", () => {
    const html = draw(team({ sync: { state: "choosing", lastSeen: null } }));
    assert.match(html, /role="alertdialog" aria-label="Choose how to join"/);
    assert.match(text(html), /Bring my records into the organisation/);
    assert.match(text(html), /Start fresh with the organisation's records/);
    assert.match(text(html), /left exactly as it is — a backup/);
    assert.match(text(html), /nothing leaves this machine/);
    assert.doesNotMatch(draw(team()), /role="alertdialog" aria-label="Choose how to join"/);
  });

  test("a teammate's work carries their name — and their agent is 'Ana's Scout'", () => {
    const state = team();
    state.agentFeed = [
      { seq: 2, at: Date.now(), actor: { kind: "agent", id: "ag_ana_scout" }, verb: "log", line: "log note orbit x", outcome: "ok" },
      { seq: 1, at: Date.now(), actor: { kind: "human", id: "ana" }, verb: "x", line: "x", outcome: "ok" },
    ];
    const shown = text(draw(state, "home"));
    assert.match(shown, /Ana's Scout/);
    assert.match(shown, /Ana Torres/);
    assert.doesNotMatch(shown, /A teammate/, "a teammate the window was told about is never anonymous");
  });

  test("an unknown teammate is 'A teammate', never an id", () => {
    const state = team({ members: [{ id: "me", name: "Berk Öge" }] });
    state.agentFeed = [{ seq: 1, at: Date.now(), actor: { kind: "human", id: "01M207XY60B9D5MPJTB9D5MPJT" }, verb: "x", line: "x", outcome: "ok" }];
    const html = draw(state, "home");
    assert.match(text(html), /A teammate/);
    assert.deepEqual(findIds(html), []);
  });
});

describe("a stale refusal keeps the draft and says what changed", () => {
  const stale = {
    reason: "stale", kind: "deal", id: "01M207XY60B9D5MPJTB9D5MPJT", handle: "acme-renewal", by: { id: "ana", name: "Ana Torres" }, at: Date.now() - 120_000,
    current: { kind: "deal", label: "Acme renewal", value: { formatted: "$60,000.00" }, stage: "proposal", status: "open", detail: "Acme Corp", archived: false },
  };
  const sentence = "not saved — acme-renewal changed while you were looking: its value is now $60,000.00 (set by Ana Torres, 2 min ago). Nothing was changed; look again and retry.";

  test("the panel shows the sentence, what the record says now, who changed it, and that the draft is kept", async () => {
    const { ErrorLine } = (await vite.ssrLoadModule("/src/Forms.tsx")) as { ErrorLine: (p: object) => unknown };
    const html = renderToStaticMarkup(createElement(ErrorLine as never, { error: sentence, refused: stale, onDismiss: () => {} }));
    const shown = text(html);
    assert.match(html, /class="stale-panel" role="alert"/);
    assert.ok(shown.includes(sentence));
    assert.match(shown, /Acme renewal now\s+Value \$60,000\.00 · Stage proposal · Acme Corp/);
    assert.match(shown, /Changed by\s+Ana Torres/);
    assert.match(shown, /Nothing was saved\. What you typed is still in the box/);
    assert.deepEqual(findIds(html), []);
  });

  test("an offline or rule refusal is the plain error line, and an unknown author is 'a teammate'", async () => {
    const { ErrorLine } = (await vite.ssrLoadModule("/src/Forms.tsx")) as { ErrorLine: (p: object) => unknown };
    const plain = renderToStaticMarkup(createElement(ErrorLine as never, { error: "not saved — offline: …", refused: { reason: "offline" }, onDismiss: () => {} }));
    assert.match(plain, /class="write-error" role="alert"/);
    assert.doesNotMatch(plain, /stale-panel/);
    const anon = renderToStaticMarkup(createElement(ErrorLine as never, { error: "x", refused: { ...stale, by: null }, onDismiss: () => {} }));
    assert.match(text(anon), /Changed by\s+a teammate/);
  });

  test("no control throws away what was typed when a write is refused", () => {
    const rec = readFileSync(join(ROOT, "src/Record.tsx"), "utf8");
    // The inline editors only close on success; the composers only clear on success.
    assert.equal((rec.match(/if \(ok\) setEditing\(false\)/g) ?? []).length, 2);
    assert.match(rec, /onBlur=\{\(\) => \(failed \? undefined : void commit\(\)\)\}/);
  });
});

// MARK: - p2-sec: the warning before anything is sent

describe("the server warning", () => {
  const out = {
    status: { signedIn: false, server: null, person: null, org: null, role: null, expiresAt: null, insecure: false },
    known: true, busy: false, error: null, dismissError: () => {},
    signUp: async () => false, signIn: async () => false, acceptInvite: async () => false, signOut: async () => ({ ok: true, warning: null }),
    confirm: null, confirmSend: async () => false, cancelSend: () => {},
  };
  const draw = (account: unknown) =>
    renderToStaticMarkup(createElement(Window as never, { state: splitSnap(), view: "settings", run: () => {}, apply: () => {}, setView: () => {}, theme: "system", chooseTheme: () => {}, account }));

  test("nothing is shown until an address needs a yes", () => {
    assert.doesNotMatch(draw(out), /server-warning/);
  });

  test("an insecure address is named, in words, with Cancel first and nothing yet sent", () => {
    const html = draw({ ...out, confirm: { req: { kind: "in", args: {} }, host: "crm.example.com", reasons: ["It is plain HTTP, not HTTPS. Your password and invite would travel unencrypted to crm.example.com."] } });
    const shown = text(html);
    assert.match(html, /role="alertdialog" aria-label="Check this server before sending"/);
    assert.match(shown, /Your password would be sent to\s+crm\.example\.com\s*\./);
    assert.match(shown, /Nothing has been sent\. Only continue if crm\.example\.com is exactly the server you mean/);
    assert.ok(shown.indexOf("Cancel") < shown.indexOf("Send it anyway"), "Cancel comes first");
    assert.doesNotMatch(shown, /account_check_server/);
  });

  test("an invite is named as well as the password", () => {
    const html = draw({ ...out, confirm: { req: { kind: "invite", args: {} }, host: "evil.example", reasons: ["x"] } });
    assert.match(text(html), /Your password and invite would be sent to\s+evil\.example/);
  });
});
