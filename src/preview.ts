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
  Actor, Agent, Board, BoardColumn, Card, ColumnKey, Handle, Id, Money, Row, Snapshot,
} from "./bridge";
import { asHandle, asId, COLUMN_KEYS, EMPTY, looksLikeId } from "./bridge";

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
        return this.set({ ...s, focus: focusTo(s, req.kind as never, req.id as Id) });

      case "move":
        return this.set(moveDeal(s, req.id as Id, req.to as ColumnKey, HUMAN));

      case "select": {
        // Answering a pending question clears it and opens what was chosen.
        const n = req.n as number;
        const chosen = s.pending?.candidates[n - 1];
        if (!chosen) return s;
        return this.set({
          ...s,
          pending: null,
          focus: { kind: chosen.kind, id: chosen.id, handle: chosen.handle },
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

/** `AppState::focus_json` looks the handle up from the id rather than storing it, and
 *  answers null when nothing resolves. The harness does the same, so the window meets the
 *  null case here rather than for the first time against the real core. */
function focusTo(s: Snapshot, kind: Row["kind"], id: Id): Snapshot["focus"] {
  const record = recordsFrom(s).find((r) => r.id === id);
  return { kind, id, handle: record ? record.handle : null };
}

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

// MARK: - Ids and handles
//
// **The fixtures mint real ULIDs.** They used to be `d_hollis` and `c_acme_hold`, which is
// precisely why round one of QA found the window printing ids into commands and the preview
// showing nothing wrong: a readable id looks like a handle, so every screen in the harness
// was quietly correct and every screen against the real core was broken. A harness that
// lies in the comfortable direction is not a harness.
//
// Deterministic, so a scenario renders identically on every reload and a screenshot from
// yesterday still matches.

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const FIXED_MS = 1_757_000_000_000; // a fixed instant: these fixtures are not a clock
let minted = 0n;

/** 48 bits of timestamp then 80 bits of entropy, as 26 Crockford base32 characters —
 *  the encoding `model.rs`'s `Ulid` prints, so these are the shape the real core emits. */
function ulid(): Id {
  minted += 0x9E37_79B9_7F4A_7C15n; // a big odd stride, so consecutive ids are not neighbours
  const value = (BigInt(FIXED_MS) << 80n) | (minted & ((1n << 80n) - 1n));
  let out = "";
  for (let i = 25; i >= 0; i--) out += CROCKFORD[Number((value >> BigInt(5 * i)) & 31n)];
  return asId(out);
}

/** The core's `unique_handle`: lower-cased, non-alphanumerics collapsed to single dashes.
 *  Uniquified against what is already taken, so a second "Brightsea" becomes `brightsea-2`
 *  rather than colliding — which is exactly what the real core does, and what makes a
 *  handle typable without being an identity. */
function handleFor(name: string, taken: Set<string>): Handle {
  const stem = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "record";
  let candidate = stem;
  for (let n = 2; taken.has(candidate); n++) candidate = `${stem}-${n}`;
  taken.add(candidate);
  return asHandle(candidate);
}

function usd(major: number): Money {
  return { amount: Math.round(major * 100), currency: "USD" };
}
function eur(major: number): Money {
  return { amount: Math.round(major * 100), currency: "EUR" };
}

type Seed = {
  id: Id;
  /** What the window prints and a person types. Never the id. */
  handle: Handle;
  title: string;
  company: string;
  value: Money | null;
  column: ColumnKey;
  by: Actor;
};

/** A seed's id is minted and its handle is derived, so no fixture can accidentally carry a
 *  readable id again. `taken` is threaded through so handles uniquify the way real ones do. */
function deal(
  title: string, company: string, value: Money | null, column: ColumnKey, by: Actor,
  taken: Set<string>,
): Seed {
  return { id: ulid(), handle: handleFor(title, taken), title, company, value, column, by };
}

const SEED_HANDLES = new Set<string>();

const SEEDS: Seed[] = [
  deal("Northwind renewal", "Northwind Traders", usd(45000), "lead", HUMAN, SEED_HANDLES),
  deal("Kestrel pilot", "Kestrel Labs", usd(12500), "lead", NIA, SEED_HANDLES),
  deal("Brightsea onboarding", "Brightsea", eur(9800), "lead", HUMAN, SEED_HANDLES),
  deal("Orbit platform seats", "Orbit Systems", usd(88000), "qualified", PILOT, SEED_HANDLES),
  deal("Marlow expansion", "Marlow & Co", usd(31000), "qualified", HUMAN, SEED_HANDLES),
  deal("Ferrous supply deal", "Ferrous Works", eur(52000), "proposal", NIA, SEED_HANDLES),
  deal("Calder migration", "Calder Group", usd(140000), "proposal", HUMAN, SEED_HANDLES),
  deal("Hollis annual", "Hollis Partners", usd(67500), "negotiation", NIA, SEED_HANDLES),
  deal("Penrose rollout", "Penrose Industrial", usd(210000), "won", HUMAN, SEED_HANDLES),
  deal("Vantage trial", "Vantage Retail", usd(4200), "lost", PILOT, SEED_HANDLES),
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
    handle: seed.handle,
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

function blank(kind: Row["kind"], label: string, detail: string | null, taken: Set<string>): Row {
  return {
    kind,
    id: ulid(),
    handle: handleFor(label, taken),
    label,
    detail,
    stage: null,
    status: null,
    value: null,
    archived: false,
  };
}

/** Memoised per snapshot: `repage` runs on every keystroke, and minting a fresh set of ids
 *  each time would make a record's id change under the person mid-search — which is not a
 *  thing the real core can do, so the harness must not do it either. */
const recordCache = new WeakMap<Snapshot, Row[]>();

function recordsFrom(s: Snapshot): Row[] {
  const hit = recordCache.get(s);
  if (hit) return hit;

  const deals = Object.values(s.cards ?? {});
  const names = [...new Set(deals.map((d) => d.detail).filter((n): n is string => !!n))];
  // Company and contact handles share one namespace with the deals', the way the core's
  // `handle_taken` checks every record type — so a company called "Brightsea" beside a deal
  // called "Brightsea onboarding" gets its own handle rather than colliding.
  const taken = new Set<string>([...deals.map((d) => String(d.handle))]);
  const companies = names.map((name) => blank("company", name, null, taken));
  const contacts = names.map((name, i) => blank("contact", PEOPLE[i % PEOPLE.length], name, taken));

  const all = [...deals, ...contacts, ...companies];
  recordCache.set(s, all);
  return all;
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

function moveDeal(s: Snapshot, id: Id, to: ColumnKey, by: Actor): Snapshot {
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
        handle: card.handle,
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
function focusOn(s: Snapshot, id: Id, bare = false): Snapshot {
  const card = s.cards?.[id];
  if (!card) return s;
  const at = Date.now();
  if (bare) {
    // A record nothing has been written against yet. This is the *only* state in which the
    // record panel prints `crm task <handle>` and `crm log note <handle>` — the two
    // commands round one of QA caught carrying a ULID. Without a scenario that reaches it,
    // the harness cannot show the defect even with the right fixtures.
    return {
      ...s,
      focus: { kind: "deal", id, handle: card.handle },
      focused: {
        row: card,
        fields: [
          { label: "Company", value: card.detail ?? "—" },
          { label: "Stage", value: LABELS[(card.stage ?? "lead") as ColumnKey] },
          { label: "Status", value: card.status ?? "open" },
        ],
        timeline: [],
        tasks: [],
      },
    };
  }
  return {
    ...s,
    focus: { kind: "deal", id, handle: card.handle },
    focused: {
      row: card,
      fields: [
        { label: "Company", value: card.detail ?? "—" },
        { label: "Stage", value: LABELS[(card.stage ?? "lead") as ColumnKey] },
        { label: "Status", value: card.status ?? "open" },
      ],
      timeline: [
        { id: ulid(), kind: "note", body: "Renewal paperwork sent for counter-signature.", at: at - 36e5, by: NIA },
        { id: ulid(), kind: "call", body: "Walked through the security questionnaire. They are happy.", at: at - 26 * 36e5, by: HUMAN },
        { id: ulid(), kind: "email", body: "Introduced the team and shared last quarter's usage.", at: at - 74 * 36e5, by: PILOT },
      ],
      tasks: [
        { id: ulid(), what: "Chase the signed order form", due: "2026-09-05", doneAt: null, by: NIA },
        { id: ulid(), what: "Book the kickoff call", due: "2026-09-12", doneAt: null, by: HUMAN },
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

/** A scenario names a deal the way a person would: by its handle. The id is minted and
 *  opaque, so nothing in this file should be holding one as a literal. */
function idOf(seeds: Seed[], handle: string): Id {
  const hit = seeds.find((x) => x.handle === handle);
  if (!hit) throw new Error(`preview: no seed with handle "${handle}"`);
  return hit.id;
}

const HOLLIS = idOf(SEEDS, "hollis-annual");

const SCENARIOS: Record<string, Scenario> = {
  pipeline: {
    label: "Pipeline",
    note: "The everyday board: two agents connected, six columns, two currencies.",
    build: () => focusOn(world(SEEDS), HOLLIS),
  },

  move: {
    label: "Agent move",
    note: "Nia moves Hollis annual into Won after 1.2s. The card rings in her tint.",
    build: () => world(SEEDS),
    script: () => {
      window.setTimeout(() => {
        core.set(moveDeal(core.snapshot, HOLLIS, "won", NIA));
      }, 1200);
    },
  },

  untouched: {
    label: "Untouched record",
    note: "A deal with nothing logged against it — the only state where the record panel prints `crm task` and `crm log`.",
    build: () => focusOn(world(SEEDS), idOf(SEEDS, "kestrel-pilot"), true),
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
            { kind: "company", id: ulid(), handle: asHandle("acme"), label: "Acme Holdings — acme.com" },
            { kind: "company", id: ulid(), handle: asHandle("acme-2"), label: "Acme Industrial — acme-industrial.de" },
            { kind: "company", id: ulid(), handle: asHandle("acme-3"), label: "Acme Laboratories — acmelabs.io" },
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
      const taken = new Set<string>();
      const many: Seed[] = Array.from({ length: 26 }, (_, i) =>
        deal(
          `Inbound ${i + 1} — trial request`,
          `${BULK_COMPANIES[i % BULK_COMPANIES.length]} ${Math.floor(i / BULK_COMPANIES.length) + 1}`,
          usd(1000 * (i + 3)),
          "qualified",
          i % 3 === 0 ? NIA : HUMAN,
          taken,
        ),
      );
      return world([...SEEDS.filter((s) => s.column === "lead" || s.column === "won"), ...many]);
    },
  },

  long: {
    label: "Long text",
    note: "A 40-character deal title and company names that have no intention of fitting.",
    build: () => {
      const taken = new Set<string>();
      const seeds = [
        deal(
          "Enterprise platform renewal — phase 2",
          "Interkontinentale Maschinenbau und Anlagentechnik GmbH & Co. KG",
          usd(1250000), "negotiation", NIA, taken,
        ),
        deal(
          "Multi-region observability rollout AB",
          "Consolidated Southwestern Freight & Logistics Corporation",
          eur(430000), "proposal", HUMAN, taken,
        ),
        ...SEEDS.slice(0, 3),
      ];
      return focusOn(world(seeds), seeds[0].id);
    },
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

// MARK: - The guard that was missing
//
// Round one shipped a window that printed ULIDs into commands, and this harness showed
// nothing wrong because its own fixtures used readable ids. The fixtures are ULIDs now, so
// the defect would be visible — but "visible" depends on somebody looking at the right
// scenario. This looks for us.
//
// Anything rendered into the window that is shaped like an id is a bug by construction:
// `id` is for machines, and nothing machine-facing belongs in text a person reads. The
// observer watches every render, including the ones a pushed snapshot causes.

function watchForLeakedIds(root: HTMLElement, report: (leaked: string[]) => void): void {
  let queued = 0;
  const scan = () => {
    queued = 0;
    const seen = new Set<string>();
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      for (const word of (n.textContent ?? "").split(/[\s"'`()\[\]{}<>,;]+/)) {
        if (looksLikeId(word)) seen.add(word);
      }
    }
    report([...seen]);
  };
  new MutationObserver(() => {
    // Coalesce: one render is many mutations, and the answer only matters once it settles.
    if (queued) window.clearTimeout(queued);
    queued = window.setTimeout(scan, 60);
  }).observe(root, { subtree: true, childList: true, characterData: true });
}

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

  const alarm = document.createElement("p");
  alarm.id = "preview-alarm";
  alarm.hidden = true;

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
    alarm,
  );
  document.body.append(bar);

  watchForLeakedIds(document.getElementById("root")!, (leaked) => {
    alarm.hidden = leaked.length === 0;
    if (leaked.length === 0) return;
    alarm.textContent = `id leaked into the window: ${leaked.join(", ")}`;
    // Loud in the console too: a harness nobody is looking at should still fail audibly.
    console.error("[preview] ids must never be rendered — found:", leaked);
  });

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
