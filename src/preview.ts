// The preview harness: the whole window, in a plain browser, with no Rust build and no
// Clatch.
//
//     npm run preview      →  http://localhost:5174
//
// **Why this exists.** It is how the window got designed at all — the board, the table
// and the ring were all built here before the core could answer a single verb. It is also
// the only place some states are reachable on demand: an agent-driven move, a `pending`
// with three candidates, a board with one column overfull and another empty, a 40-character
// deal title, and no deals at all.
//
// **What it does NOT do is re-implement the bridge.** It installs Tauri's own
// `mockIPC` underneath `@tauri-apps/api`, so `useSnapshot`, `useAsset` and `cmd` are the
// real ones, running their real code — including the `rev` ordering that drops a stale
// snapshot, and the listen/unlisten cycle that React's StrictMode exercises twice on
// mount. A harness that mocked the bridge instead would be a second window, and the
// states it proved would be states of the mock.
//
// Everything in this file is preview-only. `main.tsx` never imports it, and `preview.html`
// is not an entry in `vite build`, so none of it reaches the shipped bundle.

import "./styles.css";
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  Actor, Agent, Board, BoardColumn, Card, ColumnKey, Money, Row, Snapshot,
} from "./bridge";
import { COLUMN_KEYS, EMPTY } from "./bridge";

// MARK: - The fake IPC

/** Deliver a snapshot on the `state` event — the same name `clappkit::app::STATE_EVENT`
 *  emits, routed through the mock's own event plugin so the real `listen` receives it. */
function pushState(snapshot: Snapshot): void {
  void emit("state", snapshot);
}

function installFakeCore(): void {
  // `shouldMockEvents` makes the mock answer `plugin:event|listen`, `|emit` and
  // `|unlisten` itself, which is the half a hand-rolled stub gets wrong: StrictMode mounts
  // twice, so the very first thing the window does is tear a listener down again.
  mockIPC((command, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (command) {
      case "run_cmd":
        // A real round trip: the reply is a stamped snapshot, same as the core's.
        return core.command(a.req as Record<string, unknown>);
      case "asset":
        return fakeAvatar(a.path as string);
      default:
        throw new Error(`preview: no fake for "${command}"`);
    }
  }, { shouldMockEvents: true });
}

/** The roster avatars are absolute paths the webview cannot open; the core reads them and
 *  hands back a data: URI. One agent here has a picture and one does not, so both the
 *  image path and the monogram fallback are on screen at once. */
function fakeAvatar(path: string): string | null {
  if (!path.includes("nia")) return null;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">` +
    `<rect width="64" height="64" fill="#267369"/>` +
    `<circle cx="32" cy="25" r="12" fill="#F2EFE6"/>` +
    `<path d="M8 64c0-14 11-22 24-22s24 8 24 22z" fill="#F2EFE6"/></svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

// MARK: - The fake core
//
// It holds one snapshot and answers the four envelopes the window sends. Those envelopes
// are proposed, not settled — see `docs/window.md`. Keeping them honest here is the point:
// if the window can drive this, M2 knows exactly what it has to accept.

const core = {
  snapshot: EMPTY,
  rev: 0,

  /** Replace the world and push it, the way a write on the core's side would. */
  set(next: Snapshot): Snapshot {
    this.rev += 1;
    this.snapshot = { ...next, ok: true, rev: this.rev };
    pushState(this.snapshot);
    return this.snapshot;
  },

  command(req: Record<string, unknown>): Snapshot {
    const s = this.snapshot;
    switch (req.cmd) {
      case "state":
        return s;

      case "show":
        return this.set({ ...s, focus: { kind: req.kind as never, id: req.id as string } });

      case "move":
        return this.set(moveDeal(s, req.id as string, req.to as ColumnKey, HUMAN));

      case "select": {
        // Answering a pending question clears it and opens what was chosen.
        const n = req.n as number;
        const chosen = s.pending?.candidates[n - 1];
        if (!chosen) return s;
        return this.set({
          ...s,
          pending: null,
          focus: { kind: chosen.kind, id: chosen.id },
        });
      }

      case "find": {
        // Query, sort and page ride one verb: an omitted field keeps its current value,
        // which is what lets the window change the page without restating the search.
        const list = { ...s.list };
        if (typeof req.query === "string") {
          list.query = req.query;
          list.page = 0;
        }
        if (typeof req.sort === "string") list.sort = req.sort as never;
        if (typeof req.page === "number") list.page = req.page;
        if ("kind" in req) list.kind = req.kind as never;
        return this.set({ ...s, list: repage(s, list) });
      }

      default:
        return s;
    }
  },
};

const HUMAN: Actor = { kind: "human" };

// MARK: - The world these scenarios are drawn from

// The ids are chosen, not typed at random: `agentTint` picks from five colours, so two
// arbitrary ids collide roughly a fifth of the time — and the first pair here did, which
// made a harness for "tell the two agents apart" draw them both the same brown. These two
// hash to #45548C and #267369.
const AGENTS: Agent[] = [
  { id: "ag_nia_4b02", name: "Nia", backend: "claude-code", model: "opus", avatar: "/agents/nia.png" },
  { id: "ag_pilot_5a72", name: "Pilot", backend: "codex", model: "gpt", avatar: null },
];

const NIA: Actor = { kind: "agent", id: AGENTS[0].id };
const PILOT: Actor = { kind: "agent", id: AGENTS[1].id };

function usd(major: number): Money {
  return { amount: Math.round(major * 100), currency: "USD" };
}
function eur(major: number): Money {
  return { amount: Math.round(major * 100), currency: "EUR" };
}

type Seed = {
  id: string;
  title: string;
  company: string;
  value: Money | null;
  column: ColumnKey;
  by: Actor;
};

const SEEDS: Seed[] = [
  { id: "d_northwind", title: "Northwind renewal", company: "Northwind Traders", value: usd(45000), column: "lead", by: HUMAN },
  { id: "d_kestrel", title: "Kestrel pilot", company: "Kestrel Labs", value: usd(12500), column: "lead", by: NIA },
  { id: "d_brightsea", title: "Brightsea onboarding", company: "Brightsea", value: eur(9800), column: "lead", by: HUMAN },
  { id: "d_orbit", title: "Orbit platform seats", company: "Orbit Systems", value: usd(88000), column: "qualified", by: PILOT },
  { id: "d_marlow", title: "Marlow expansion", company: "Marlow & Co", value: usd(31000), column: "qualified", by: HUMAN },
  { id: "d_ferrous", title: "Ferrous supply deal", company: "Ferrous Works", value: eur(52000), column: "proposal", by: NIA },
  { id: "d_calder", title: "Calder migration", company: "Calder Group", value: usd(140000), column: "proposal", by: HUMAN },
  { id: "d_hollis", title: "Hollis annual", company: "Hollis Partners", value: usd(67500), column: "negotiation", by: NIA },
  { id: "d_penrose", title: "Penrose rollout", company: "Penrose Industrial", value: usd(210000), column: "won", by: HUMAN },
  { id: "d_vantage", title: "Vantage trial", company: "Vantage Retail", value: usd(4200), column: "lost", by: PILOT },
];

const BULK_COMPANIES = [
  "Halcyon", "Brightsea", "Marlow & Co", "Orbit Systems", "Ferrous Works",
  "Calder Group", "Hollis Partners", "Vantage Retail", "Kestrel Labs",
  "Northwind Traders", "Penrose Industrial", "Alder & Vane", "Quinta Foods",
];

const LABELS: Record<ColumnKey, string> = {
  lead: "Lead",
  qualified: "Qualified",
  proposal: "Proposal",
  negotiation: "Negotiation",
  won: "Won",
  lost: "Lost",
};

function rowOf(seed: Seed): Row {
  const stage = COLUMN_KEYS.indexOf(seed.column) < 4 ? (seed.column as Row["stage"]) : "negotiation";
  const status: Row["status"] = seed.column === "won" ? "won" : seed.column === "lost" ? "lost" : "open";
  return {
    kind: "deal",
    id: seed.id,
    label: seed.title,
    detail: seed.company,
    stage,
    status,
    value: seed.value,
    archived: false,
  };
}

/** Totals **grouped by currency and never summed across them**, in first-seen order. */
function totalsOf(seeds: Seed[]): Money[] {
  const byCurrency = new Map<string, number>();
  for (const s of seeds) {
    if (!s.value) continue;
    byCurrency.set(s.value.currency, (byCurrency.get(s.value.currency) ?? 0) + s.value.amount);
  }
  return [...byCurrency].map(([currency, amount]) => ({ currency, amount }));
}

function boardOf(seeds: Seed[]): Board {
  const columns: BoardColumn[] = COLUMN_KEYS.map((key) => {
    const held = seeds.filter((s) => s.column === key);
    return {
      key,
      label: LABELS[key],
      dealIds: held.map((s) => s.id),
      count: held.length,
      totals: totalsOf(held),
    };
  });
  return { pipelineId: "sales", stageFilter: null, columns };
}

function cardsOf(seeds: Seed[]): Record<string, Card> {
  return Object.fromEntries(seeds.map((s) => [s.id, { ...rowOf(s), by: s.by }]));
}

/** Every record the shared list can show: the deals, plus a company per distinct company
 *  name on the board and a contact at each one.
 *
 *  Derived from the same seeds rather than listed separately, so the rail's counts and the
 *  footer's "N of TOTAL" cannot disagree — which is the one thing this table is supposed to
 *  be trusted about. */
const PEOPLE = [
  "Ada Whitlock", "Tomas Reyes", "Priya Raman", "Ines Halloran", "Jonas Feld",
  "Mireille Vance", "Otto Brenner", "Sana Qureshi", "Ruth Okonkwo", "Felix Adler",
  "Nora Lindqvist", "Dov Perelman", "Cai Zhou", "Ilse Brandt", "Marek Sobol",
];

function blank(kind: Row["kind"], id: string, label: string, detail: string | null): Row {
  return { kind, id, label, detail, stage: null, status: null, value: null, archived: false };
}

function recordsFrom(s: Snapshot): Row[] {
  const deals = Object.values(s.cards ?? {});
  const names = [...new Set(deals.map((d) => d.detail).filter((n): n is string => !!n))];
  const companies = names.map((name, i) => blank("company", `c_${i}`, name, null));
  const contacts = names.map((name, i) => blank("contact", `p_${i}`, PEOPLE[i % PEOPLE.length], name));
  return [...deals, ...contacts, ...companies];
}

/** The rows on the page the shared list is currently on. Both surfaces see this page. */
function repage(s: Snapshot, list: Snapshot["list"]): Snapshot["list"] {
  const all = recordsFrom(s)
    // The kind filter is server-side for the same reason the page is: a list narrowed in
    // the window alone would leave the footer counting rows nobody can see.
    .filter((r) => (list.kind ? r.kind === list.kind : true))
    .filter((r) =>
      list.query ? (r.label + " " + (r.detail ?? "")).toLowerCase().includes(list.query.toLowerCase()) : true,
    );
  const sorted = [...all].sort((a, b) => {
    if (list.sort === "name") return a.label.localeCompare(b.label);
    if (list.sort === "value") return (b.value?.amount ?? 0) - (a.value?.amount ?? 0);
    return 0;
  });
  const start = list.page * list.pageSize;
  return { ...list, total: sorted.length, rows: sorted.slice(start, start + list.pageSize) };
}

function moveDeal(s: Snapshot, id: string, to: ColumnKey, by: Actor): Snapshot {
  const seeds = seedsFrom(s).map((seed) => (seed.id === id ? { ...seed, column: to, by } : seed));
  return { ...s, board: boardOf(seeds), cards: cardsOf(seeds) };
}

/** Recover the seed list from a snapshot, so a move can be expressed as data rather than
 *  as six column splices. */
function seedsFrom(s: Snapshot): Seed[] {
  const out: Seed[] = [];
  for (const column of s.board.columns) {
    for (const id of column.dealIds) {
      const card = s.cards?.[id];
      if (!card) continue;
      out.push({
        id,
        title: card.label,
        company: card.detail ?? "",
        value: card.value,
        column: column.key,
        by: card.by,
      });
    }
  }
  return out;
}

function countsOf(seeds: Seed[]) {
  const names = new Set(seeds.map((s) => s.company).filter(Boolean));
  return {
    companies: names.size,
    contacts: names.size,
    deals: seeds.length,
    activities: 34,
    tasks: 6,
  };
}

function world(seeds: Seed[], over: Partial<Snapshot> = {}): Snapshot {
  const base: Snapshot = {
    ...EMPTY,
    rev: 0,
    board: boardOf(seeds),
    cards: cardsOf(seeds),
    counts: countsOf(seeds),
    due: { overdue: 2, today: 1, week: 5 },
    agents: AGENTS,
    ...over,
  };
  return { ...base, list: repage(base, base.list) };
}

/** A record's detail panel and its timeline. Proposed shape — see `docs/window.md`. */
function focusOn(s: Snapshot, id: string): Snapshot {
  const card = s.cards?.[id];
  if (!card) return s;
  const at = Date.now();
  return {
    ...s,
    focus: { kind: "deal", id },
    focused: {
      row: card,
      fields: [
        { label: "Company", value: card.detail ?? "—" },
        { label: "Stage", value: LABELS[(card.stage ?? "lead") as ColumnKey] },
        { label: "Status", value: card.status ?? "open" },
      ],
      timeline: [
        { id: "a3", kind: "note", body: "Renewal paperwork sent for counter-signature.", at: at - 36e5, by: NIA },
        { id: "a2", kind: "call", body: "Walked through the security questionnaire. They are happy.", at: at - 26 * 36e5, by: HUMAN },
        { id: "a1", kind: "email", body: "Introduced the team and shared last quarter's usage.", at: at - 74 * 36e5, by: PILOT },
      ],
      tasks: [
        { id: "t1", what: "Chase the signed order form", due: "2026-09-05", doneAt: null, by: NIA },
        { id: "t2", what: "Book the kickoff call", due: "2026-09-12", doneAt: null, by: HUMAN },
      ],
    },
  };
}

// MARK: - The six states this harness exists to reach

type Scenario = {
  label: string;
  note: string;
  build: () => Snapshot;
  /** Something that happens a moment after mount — an agent's write arriving. */
  script?: () => void;
};

const SCENARIOS: Record<string, Scenario> = {
  pipeline: {
    label: "Pipeline",
    note: "The everyday board: two agents connected, six columns, two currencies.",
    build: () => focusOn(world(SEEDS), "d_hollis"),
  },

  move: {
    label: "Agent move",
    note: "Nia moves Hollis annual into Won after 1.2s. The card rings in her tint.",
    build: () => world(SEEDS),
    script: () => {
      window.setTimeout(() => {
        core.set(moveDeal(core.snapshot, "d_hollis", "won", NIA));
      }, 1200);
    },
  },

  rename: {
    label: "Roster rename",
    note: "Nia is renamed after 1.2s. Same id, so the chip relabels in place — it must not flicker or reload its avatar.",
    build: () => world(SEEDS),
    script: () => {
      window.setTimeout(() => {
        // A rename arrives as a fresh roster carrying the SAME id. Everything per-agent is
        // keyed on that id, so this must relabel the existing chip rather than unmount it
        // and mount a new one — which would drop the avatar back to a monogram and lose
        // any interaction in flight, all because somebody renamed their agent.
        core.set({
          ...core.snapshot,
          agents: core.snapshot.agents.map((a) =>
            a.id === AGENTS[0].id ? { ...a, name: "Nia (ops)" } : a,
          ),
        });
      }, 1200);
    },
  },

  pending: {
    label: "Pending",
    note: "Two companies match \"Acme\" and one is close. Ambiguity is a state, not a guess.",
    build: () =>
      world(SEEDS, {
        pending: {
          prompt: "Which Acme did you mean? Nia asked to log a call against it.",
          candidates: [
            { kind: "company", id: "c_acme_hold", label: "Acme Holdings — acme.com" },
            { kind: "company", id: "c_acme_ind", label: "Acme Industrial — acme-industrial.de" },
            { kind: "company", id: "c_acme_lab", label: "Acme Laboratories — acmelabs.io" },
          ],
        },
      }),
  },

  lopsided: {
    label: "Lopsided board",
    note: "Twenty-six deals in Qualified, nothing in Proposal or Negotiation — and a list long enough to page.",
    build: () => {
      // Twenty-six, not twelve: it makes Qualified genuinely overfull, and it is the only
      // scenario that pushes the shared list past one page, which is where the footer's
      // "N of TOTAL" and the pager are actually worth looking at.
      const many: Seed[] = Array.from({ length: 26 }, (_, i) => ({
        id: `d_bulk_${i}`,
        title: `Inbound ${i + 1} — trial request`,
        company: `${BULK_COMPANIES[i % BULK_COMPANIES.length]} ${Math.floor(i / BULK_COMPANIES.length) + 1}`,
        value: usd(1000 * (i + 3)),
        column: "qualified" as ColumnKey,
        by: i % 3 === 0 ? NIA : HUMAN,
      }));
      return world([...SEEDS.filter((s) => s.column === "lead" || s.column === "won"), ...many]);
    },
  },

  long: {
    label: "Long text",
    note: "A 40-character deal title and company names that have no intention of fitting.",
    build: () =>
      focusOn(
        world([
          {
            id: "d_long",
            title: "Enterprise platform renewal — phase 2",
            company: "Interkontinentale Maschinenbau und Anlagentechnik GmbH & Co. KG",
            value: usd(1250000),
            column: "negotiation",
            by: NIA,
          },
          {
            id: "d_long2",
            title: "Multi-region observability rollout AB",
            company: "Consolidated Southwestern Freight & Logistics Corporation",
            value: eur(430000),
            column: "proposal",
            by: HUMAN,
          },
          ...SEEDS.slice(0, 3),
        ]),
        "d_long",
      ),
  },

  empty: {
    label: "Zero state",
    note: "No deals at all. Empty states are one line of text and the verb that fills them.",
    build: () =>
      world([], {
        counts: { companies: 0, contacts: 0, deals: 0, activities: 0, tasks: 0 },
        due: { overdue: 0, today: 0, week: 0 },
        agents: [],
      }),
  },
};

// MARK: - The harness chrome
//
// Plain DOM, deliberately: it is not part of the window and must not be mistaken for it in
// a screenshot. It sits in its own fixed bar with its own colours and does not touch the
// app's tokens.

const THEMES = [
  ["system", "System"],
  ["light", "Light"],
  ["dark", "Dark"],
] as const;

function chrome(remount: () => void): void {
  const bar = document.createElement("div");
  bar.id = "preview-bar";

  const note = document.createElement("p");
  note.id = "preview-note";

  function group(label: string, items: readonly (readonly [string, string])[], onPick: (k: string) => void, initial: string) {
    const wrap = document.createElement("div");
    wrap.className = "preview-group";
    const title = document.createElement("span");
    title.className = "preview-label";
    title.textContent = label;
    wrap.append(title);
    for (const [key, text] of items) {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = text;
      b.dataset.key = key;
      b.setAttribute("aria-pressed", String(key === initial));
      b.addEventListener("click", () => {
        for (const sib of wrap.querySelectorAll("button")) {
          sib.setAttribute("aria-pressed", String(sib === b));
        }
        onPick(key);
      });
      wrap.append(b);
    }
    return wrap;
  }

  const scenarioKeys = Object.keys(SCENARIOS);
  bar.append(
    group(
      "State",
      scenarioKeys.map((k) => [k, SCENARIOS[k].label] as const),
      (k) => load(k),
      scenarioKeys[0],
    ),
    group(
      "Theme",
      THEMES,
      (k) => {
        // Exactly what the window's own toggle does: an explicit choice stamps the root,
        // and "system" removes the stamp so `prefers-color-scheme` decides. The
        // un-stamped state is the one that breaks when a token is only defined inside a
        // media query, so it has to be reachable here.
        if (k === "system") document.documentElement.removeAttribute("data-theme");
        else document.documentElement.setAttribute("data-theme", k);
        try {
          localStorage.setItem("breksos.theme", k);
        } catch {
          /* a private window is allowed to refuse, and the app must not care */
        }
      },
      (() => {
        try {
          return localStorage.getItem("breksos.theme") ?? "system";
        } catch {
          return "system";
        }
      })(),
    ),
    note,
  );
  document.body.append(bar);

  function load(key: string): void {
    const scenario = SCENARIOS[key];
    note.textContent = scenario.note;
    core.rev += 1; // never rewind: `useSnapshot` would rightly drop a stale snapshot
    core.snapshot = { ...scenario.build(), ok: true, rev: core.rev };
    // Remount rather than push. Switching scenarios replaces the world wholesale, and a
    // pushed replacement looks to the board exactly like several cards moving at once —
    // so the harness for "an agent moved something" would ring half the board every time
    // you changed states. A fresh mount asks for the snapshot itself and starts with no
    // history, which is what launching the app actually does.
    remount();
    scenario.script?.();
  }

  load(scenarioKeys[0]);
}

// MARK: - Go
//
// The mock is installed before anything imports the bridge, so the real `useSnapshot`
// finds an IPC layer already in place on mount.

installFakeCore();

void Promise.all([import("react"), import("react-dom/client"), import("./App")]).then(
  ([React, ReactDOM, App]) => {
    const root = ReactDOM.createRoot(document.getElementById("root")!);
    let generation = 0;
    chrome(() => {
      generation += 1;
      root.render(
        React.createElement(
          React.StrictMode,
          null,
          React.createElement(App.default, { key: generation }),
        ),
      );
    });
  },
);
