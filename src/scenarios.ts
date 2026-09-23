/// <reference types="vite/client" />
//
// The worlds the preview harness shows, and that `src/window.test.ts` renders.
//
// Pure data and pure functions: no DOM, no Tauri, no timers. `preview.ts` wraps these in a
// fake IPC layer and a bar of buttons; the render test imports them directly. One set of
// worlds, so the harness a person looks at and the test that fails the build are looking at
// the same thing.
//
// **Two kinds of world live here, and only one is the truth.**
//
//   golden   `src-tauri/fixtures/*.json`, written by the core's own test from
//            `AppState::snapshot()` and loaded here byte for byte. This is what the window
//            actually receives. Round 2's blocker survived because the window had only ever
//            been tested against snapshots it invented; these are the ones it did not.
//   invented Everything else. Useful for states the core's fixture does not happen to
//            contain — an overfull column, a 60-character company, an agent moving a card
//            *while you watch* — and deliberately built to lie in the direction of the truth:
//            ULID ids, handles derived the way the core derives them, money pre-formatted.
//
// The frontend reads the golden files and never writes them. If one is wrong, the core is.

import type {
  Actor, ActivityKind, Agent, Board, BoardColumn, Card, ColumnKey, Focused, Handle, Id, Kind, Money, Row,
  Snapshot, Stage,
} from "./bridge";
import { asHandle, asId, cardOf, COLUMN_KEYS, EMPTY, idKey, STAGES } from "./bridge";

// MARK: - The golden snapshots

/**
 * Every fixture the core has committed, keyed by file name. Eager, so it is plain data by
 * the time anything reads it; a glob rather than two imports, so a branch that does not
 * have the files yet still builds, and `window.test.ts` is what insists they exist.
 */
const GOLDEN_FILES = import.meta.glob<Snapshot>("../src-tauri/fixtures/*.json", {
  eager: true,
  import: "default",
});

export const GOLDEN: Record<string, Snapshot> = Object.fromEntries(
  Object.entries(GOLDEN_FILES).map(([path, snapshot]) => [path.split("/").pop()!, snapshot]),
);

// MARK: - Ids and handles
//
// The invented worlds mint real ULIDs. They used to be `d_hollis` and `c_acme_hold`, which
// is precisely why round 1 found the window printing ids and the harness showing nothing
// wrong: a readable id looks like a handle. Deterministic, so a scenario renders identically
// on every reload.

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const FIXED_MS = 1_757_000_000_000; // a fixed instant: these fixtures are not a clock
let minted = 0n;

/** 48 bits of timestamp then 80 bits of entropy, as 26 Crockford base32 characters — the
 *  encoding `model.rs`'s `Ulid` prints. */
export function ulid(): Id {
  minted += 0x9e37_79b9_7f4a_7c15n; // a big odd stride, so consecutive ids are not neighbours
  const value = (BigInt(FIXED_MS) << 80n) | (minted & ((1n << 80n) - 1n));
  let out = "";
  for (let i = 25; i >= 0; i--) out += CROCKFORD[Number((value >> BigInt(5 * i)) & 31n)];
  return asId(out);
}

/** Every handle any invented world has handed out. The core checks one namespace across all
 *  record types, and so does this. */
const TAKEN = new Set<string>();

/** The core's `unique_handle`: lower-cased, non-alphanumerics collapsed to single dashes,
 *  uniquified — a second "Brightsea" becomes `brightsea-2`. */
function handleFor(name: string): Handle {
  const stem = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "record";
  let candidate = stem;
  for (let n = 2; TAKEN.has(candidate); n++) candidate = `${stem}-${n}`;
  TAKEN.add(candidate);
  return asHandle(candidate);
}

// MARK: - Money
//
// **A stand-in, not the window's formatter.** The window renders `formatted` and formats
// nothing; the core's `Money::format()` is the only implementation that ships. Invented
// worlds still have to *carry* a `formatted` string, so this produces a plausible one. The
// golden fixtures are where the real strings come from, and they are what the tests trust.

function money(amount: number, currency: string): Money {
  const fmt = new Intl.NumberFormat("en-US", { style: "currency", currency, currencyDisplay: "narrowSymbol" });
  const places = fmt.resolvedOptions().maximumFractionDigits ?? 2;
  return { amount, currency, formatted: fmt.format(amount / 10 ** places) };
}

const usd = (major: number) => money(Math.round(major * 100), "USD");
const eur = (major: number) => money(Math.round(major * 100), "EUR");

// MARK: - The cast

export const HUMAN: Actor = { kind: "human" };

// Agent ids are Clatch's, in a different namespace from record ids, and never rendered — so
// they are literals. They are *chosen*: `agentTint` picks from five colours, so two arbitrary
// ids collide about a fifth of the time, and the first pair here did. These two hash to
// #45548C and #267369.
export const AGENTS: Agent[] = [
  { id: "ag_nia_4b02", name: "Nia", backend: "claude-code", model: "opus", avatar: "/agents/nia.png" },
  { id: "ag_pilot_5a72", name: "Pilot", backend: "codex", model: "gpt", avatar: null },
];

export const NIA: Actor = { kind: "agent", id: AGENTS[0].id };
const PILOT: Actor = { kind: "agent", id: AGENTS[1].id };

// MARK: - Deals

type Seed = {
  id: Id;
  handle: Handle;
  title: string;
  company: string;
  value: Money | null;
  column: ColumnKey;
  by: Actor;
  movedAt: number;
};

let clock = 0;

function deal(title: string, company: string, value: Money | null, column: ColumnKey, by: Actor): Seed {
  clock += 1;
  return { id: ulid(), handle: handleFor(title), title, company, value, column, by, movedAt: FIXED_MS + clock * 60_000 };
}

const SEEDS: Seed[] = [
  deal("Northwind renewal", "Northwind Traders", usd(45000), "lead", HUMAN),
  deal("Kestrel pilot", "Kestrel Labs", usd(12500), "lead", NIA),
  deal("Brightsea onboarding", "Brightsea", eur(9800), "lead", HUMAN),
  deal("Orbit platform seats", "Orbit Systems", usd(88000), "qualified", PILOT),
  deal("Marlow expansion", "Marlow & Co", usd(31000), "qualified", HUMAN),
  deal("Ferrous supply deal", "Ferrous Works", eur(52000), "proposal", NIA),
  deal("Calder migration", "Calder Group", usd(140000), "proposal", HUMAN),
  deal("Hollis annual", "Hollis Partners", usd(67500), "negotiation", NIA),
  deal("Penrose rollout", "Penrose Industrial", usd(210000), "won", HUMAN),
  deal("Vantage trial", "Vantage Retail", usd(4200), "lost", PILOT),
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
  const open = COLUMN_KEYS.indexOf(seed.column) < 4;
  return {
    kind: "deal",
    id: seed.id,
    handle: seed.handle,
    label: seed.title,
    detail: seed.company,
    stage: open ? (seed.column as Row["stage"]) : "negotiation",
    status: seed.column === "won" ? "won" : seed.column === "lost" ? "lost" : "open",
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
  return [...byCurrency].map(([currency, amount]) => money(amount, currency));
}

function boardOf(seeds: Seed[]): Board {
  const columns: BoardColumn[] = COLUMN_KEYS.map((key) => {
    const held = seeds.filter((s) => s.column === key);
    return { key, label: LABELS[key], dealIds: held.map((s) => s.id), count: held.length, totals: totalsOf(held) };
  });
  return { pipelineId: "sales", stageFilter: null, columns };
}

/** `cards` objects built here, so `recordsFrom` can tell an invented world from a golden one. */
const INVENTED = new WeakSet<object>();

function cardsOf(seeds: Seed[]): Record<string, Card> {
  const cards = Object.fromEntries(
    seeds.map((s) => [idKey(s.id), { ...rowOf(s), by: s.by, movedAt: s.movedAt }]),
  );
  INVENTED.add(cards);
  return cards;
}

/** Recover the seeds from a snapshot, so a move is expressed as data rather than as six
 *  column splices. Works on golden snapshots too, which is what lets the harness drag a card
 *  in the core's own fixture. */
function seedsFrom(s: Snapshot): Seed[] {
  const out: Seed[] = [];
  for (const column of s.board.columns) {
    for (const id of column.dealIds) {
      const card = cardOf(s, id);
      if (!card) continue;
      out.push({
        id,
        handle: card.handle,
        title: card.label,
        company: card.detail ?? "",
        value: card.value,
        column: column.key,
        by: card.by,
        movedAt: card.movedAt,
      });
    }
  }
  return out;
}

// MARK: - Companies and contacts
//
// Derived from the company names on the board, and **memoised by name for the whole
// session**. The first version cached per snapshot object — and every command spreads a new
// snapshot, so every keystroke in the search box minted a fresh set of ids for every company
// and contact. The real core cannot do that, so the harness must not.

const PEOPLE = [
  "Ada Whitlock", "Tomas Reyes", "Priya Raman", "Ines Halloran", "Jonas Feld",
  "Mireille Vance", "Otto Brenner", "Sana Qureshi", "Ruth Okonkwo", "Felix Adler",
  "Nora Lindqvist", "Dov Perelman", "Cai Zhou", "Ilse Brandt", "Marek Sobol",
];

const COMPANY_ROWS = new Map<string, Row>();
const CONTACT_ROWS = new Map<string, Row>();

function blank(kind: Kind, label: string, detail: string | null): Row {
  return { kind, id: ulid(), handle: handleFor(label), label, detail, stage: null, status: null, value: null, archived: false };
}

function companyNamed(name: string): Row {
  let row = COMPANY_ROWS.get(name);
  if (!row) COMPANY_ROWS.set(name, (row = blank("company", name, null)));
  return row;
}

function contactAt(name: string): Row {
  let row = CONTACT_ROWS.get(name);
  if (!row) {
    const person = PEOPLE[CONTACT_ROWS.size % PEOPLE.length];
    CONTACT_ROWS.set(name, (row = blank("contact", person, name)));
  }
  return row;
}

/**
 * Every record the shared list can show.
 *
 * For an invented world: its deals plus a company and a contact per company name. For a
 * golden one: exactly what the core sent — its cards and its list rows — and nothing
 * made up, because inventing companies inside the core's own fixture would be testing a
 * blend of the truth and a guess.
 */
export function recordsFrom(s: Snapshot): Row[] {
  const deals: Row[] = Object.values(s.cards);
  if (!INVENTED.has(s.cards)) {
    const byId = new Map<string, Row>();
    for (const r of [...deals, ...s.list.rows]) byId.set(idKey(r.id), r);
    return [...byId.values()];
  }
  const names = [...new Set(deals.map((d) => d.detail).filter((n): n is string => !!n))];
  // A pending question's candidates are records too — answering one opens it.
  const asked = (s.pending?.candidates ?? []).map((c) => COMPANY_ROWS.get(c.label)).filter((r): r is Row => !!r);
  // M7: a company or contact `addRecord` creates stands alone — no deal implies it, so it
  // is not in `names` and would otherwise vanish the instant `repage` recomputes the list.
  // `addRecord` prepends it to `s.list.rows` before calling `repage`; unioning that in here
  // is what keeps it alive. (A record that later scrolls off whatever page it was created
  // on can still drop out of this union on a subsequent search — a real limit of a preview
  // mock with no actual store behind it, not worth a bigger fix in a file nothing ships.)
  const all = [...deals, ...names.map(contactAt), ...names.map(companyNamed), ...asked, ...s.list.rows];
  return [...new Map(all.map((r) => [idKey(r.id), r])).values()];
}

/** The rows on the page the shared list is currently on — filtered, sorted and paged the
 *  way the core does it, so the footer's "N of TOTAL" means what it says. */
export function repage(s: Snapshot, list: Snapshot["list"]): Snapshot["list"] {
  const needle = list.query.toLowerCase();
  const all = recordsFrom(s)
    .filter((r) => (list.kind ? r.kind === list.kind : true))
    .filter((r) => (needle ? `${r.label} ${r.handle} ${r.detail ?? ""}`.toLowerCase().includes(needle) : true));
  const sorted = [...all].sort((a, b) => {
    if (list.sort === "name") return a.label.localeCompare(b.label);
    if (list.sort === "value") return (b.value?.amount ?? 0) - (a.value?.amount ?? 0);
    return 0;
  });
  const start = list.page * list.pageSize;
  return { ...list, total: sorted.length, rows: sorted.slice(start, start + list.pageSize) };
}

function countsOf(seeds: Seed[]) {
  const names = new Set(seeds.map((s) => s.company).filter(Boolean));
  return { companies: names.size, contacts: names.size, deals: seeds.length, activities: 34, tasks: 6 };
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

/**
 * A move: the deal changes column, and `movedAt`/`by` are stamped — which is exactly what
 * the window's ring watches for.
 *
 * **M7: also refreshes the open record, if this is the deal that is open.** The board and
 * `cards` were always rebuilt correctly; `focused` was not touched at all, which the "Agent
 * move" scenario never exercised — it does not open the deal it moves. The record panel's
 * own Move control does exactly that, and a stage select that visibly moves the board while
 * the panel underneath still says "Stage: Negotiation" is the bug this fixes.
 */
export function moveDeal(s: Snapshot, id: Id, to: ColumnKey, by: Actor): Snapshot {
  const stamp = Math.max(Date.now(), ...seedsFrom(s).map((x) => x.movedAt + 1));
  const seeds = seedsFrom(s).map((x) => (x.id === id ? { ...x, column: to, by, movedAt: stamp } : x));
  let next: Snapshot = { ...s, board: boardOf(seeds), cards: cardsOf(seeds) };

  if (next.focused && next.focused.row.id === id) {
    const moved = cardOf(next, id);
    if (moved) {
      next = {
        ...next,
        focused: {
          ...next.focused,
          row: moved,
          fields: next.focused.fields.map((f) => {
            if (f.label === "Stage" && moved.stage) return { ...f, value: LABELS[moved.stage] };
            if (f.label === "Status" && moved.status) {
              return { ...f, value: moved.status[0].toUpperCase() + moved.status.slice(1) };
            }
            return f;
          }),
        },
      };
    }
  }

  return { ...next, list: repage(next, s.list) };
}

// MARK: - The open record

type Detail = { timeline: Focused["timeline"]; timelineTotal: number; tasks: Focused["tasks"] };

const BARE: Detail = { timeline: [], timelineTotal: 0, tasks: [] };

function history(): Detail {
  const at = FIXED_MS;
  const task = (what: string, due: string, doneAt: number | null, by: Actor) => ({
    id: ulid(), handle: handleFor(what), what, due, doneAt, by,
  });
  return {
    timeline: [
      { id: ulid(), kind: "note", body: "Renewal paperwork sent for counter-signature.", at: at - 36e5, by: NIA },
      { id: ulid(), kind: "call", body: "Walked through the security questionnaire. They are happy.", at: at - 26 * 36e5, by: HUMAN },
      { id: ulid(), kind: "email", body: "Introduced the team and shared last quarter's usage.", at: at - 74 * 36e5, by: PILOT },
    ],
    // The core caps the timeline at the 50 newest; this record pretends to have more, so the
    // panel's "newest 3 of 64" is on screen somewhere.
    timelineTotal: 64,
    tasks: [
      task("Chase the signed order form", "2026-09-05", null, NIA),
      task("Book the kickoff call", "2026-09-12", null, HUMAN),
      task("Send the security questionnaire", "2026-08-28", FIXED_MS - 9 * 864e5, HUMAN),
    ],
  };
}

/**
 * Open a record: `focus` and `focused` together, because the contract has `focused` present
 * if and only if `focus` is. The first harness set `focus` alone, so clicking a card changed
 * what was "open" while the panel went on describing the previous record.
 *
 * The field list mimics the shape `crm show` prints — labels and values composed by the
 * core, handles in parentheses — rather than anything the window would compose, because
 * the window composes none.
 */
export function openRecord(s: Snapshot, kind: Kind, id: Id, detail: Detail = BARE): Snapshot {
  const row = recordsFrom(s).find((r) => r.id === id);
  if (!row) return { ...s, focus: { kind, id, handle: null }, focused: null };

  const fields: Focused["fields"] = [];
  if (row.kind === "deal") {
    // Only an invented world gets a company handle made up for it; in a golden snapshot the
    // harness says only what the core said.
    const company = row.detail && INVENTED.has(s.cards) ? companyNamed(row.detail) : null;
    fields.push({
      label: "Company",
      value: company ? `${company.label} (${company.handle})` : row.detail ?? "—",
    });
    if (row.value) fields.push({ label: "Value", value: row.value.formatted });
    if (row.stage) fields.push({ label: "Stage", value: LABELS[row.stage] });
    if (row.status) fields.push({ label: "Status", value: row.status[0].toUpperCase() + row.status.slice(1) });
  } else if (row.detail) {
    fields.push({ label: row.kind === "contact" ? "Company" : "Domain", value: row.detail });
  }

  return {
    ...s,
    focus: { kind: row.kind, id: row.id, handle: row.handle },
    focused: { row, fields, ...detail },
  };
}

// MARK: - Writes — the preview's mock of M2's write verbs
//
// M2 is not merged, so nothing here talks to a real core; these are stand-ins good enough
// to drive the harness and prove every M7 control sends the right envelope, renders
// success through the same `apply` a read does, and shows a refusal when one is due.
// `scenarios.ts` is already exempt from "the window formats no money" for the same
// reason — a mock `formatted` string has to come from somewhere — and this is the same
// kind of exemption. The real rules are the core's, unmerged, in `src-tauri/`.

/** What a write resolves to: a fresh snapshot, or a refusal — the two shapes `write()` in
 *  `bridge.ts` expects back from `cmd()`. */
export type WriteResult = { snapshot: Snapshot } | { error: string };

function findRow(s: Snapshot, id: Id): Row | undefined {
  return recordsFrom(s).find((r) => r.id === id);
}

function companyLabelFor(s: Snapshot, handle: string): string {
  const hit = recordsFrom(s).find((r) => r.kind === "company" && r.handle === handle);
  return hit ? hit.label : handle; // best effort: an unresolved handle still shows as typed
}

/** Patch one row wherever the snapshot carries a copy of it: the shared list, a board card
 *  (deals only), and the open record if it is the one open. The real core recomputes all
 *  three from one store; the mock has three places to keep in step by hand. */
function withRow(s: Snapshot, id: Id, patch: Partial<Row>): Snapshot {
  const key = idKey(id);
  const list = { ...s.list, rows: s.list.rows.map((r) => (r.id === id ? { ...r, ...patch } : r)) };
  const cards = s.cards[key] ? { ...s.cards, [key]: { ...s.cards[key], ...patch } } : s.cards;
  const focused =
    s.focused && s.focused.row.id === id ? { ...s.focused, row: { ...s.focused.row, ...patch } } : s.focused;
  return { ...s, list, cards, focused };
}

/** A decimal like "67500" or "67500.50" to minor units. The mock's own simplification: it
 *  assumes two decimal places, where the real `Money::exponent` varies by currency (JPY is
 *  0, BHD is 3) — reproducing that table here would be exactly the second formatter round 3
 *  removed. Good enough to demo the control; never shipped. */
function parseAmount(text: string): number | null {
  const n = Number(text.replace(/[^0-9.-]/g, ""));
  return Number.isFinite(n) ? Math.round(n * 100) : null;
}

/** Fields a generic click-to-edit must refuse, because a dedicated control owns them
 *  instead: `Company`/`Contacts` are `link`'s, `Stage`/`Status` are `move`'s. Keyed
 *  lower-case, matched against the field's own label — see `setField`. */
const NOT_SET_DIRECTLY: Record<string, string> = {
  company: "linked with the Link control — not set directly",
  contacts: "linked with the Link control — not set directly",
  stage: "moved with the Move control, or dragged — not set directly",
  status: "closed with the Move control (Won or Lost) — not set directly",
};

/** `{ cmd: "add", kind, name, fields? }` — a company, a contact, or a deal into a stage. */
export function addRecord(
  s: Snapshot,
  kind: Kind,
  name: string,
  fields: Record<string, string | undefined>,
): WriteResult {
  const trimmed = name.trim();
  if (!trimmed) return { error: `add needs a name — \`crm add ${kind} <name>\`` };

  if (kind === "deal") {
    const stage = fields.stage;
    if (!STAGES.includes(stage as Stage)) {
      return { error: `"${stage}" is not a stage — use one of ${STAGES.join(", ")}` };
    }
    let value: Money | null = null;
    if (fields.value) {
      const amount = parseAmount(fields.value);
      if (amount === null) return { error: `"${fields.value}" is not a number` };
      value = money(amount, (fields.currency || "USD").toUpperCase());
    }
    const company = fields.company ? companyLabelFor(s, fields.company) : "";
    const seeds = [...seedsFrom(s), deal(trimmed, company, value, stage as ColumnKey, HUMAN)];
    const next: Snapshot = {
      ...s,
      board: boardOf(seeds),
      cards: cardsOf(seeds),
      counts: { ...s.counts, deals: s.counts.deals + 1 },
    };
    return { snapshot: { ...next, list: repage(next, next.list) } };
  }

  const detail = kind === "contact" && fields.company ? companyLabelFor(s, fields.company) : (fields.domain ?? null);
  const row = blank(kind, trimmed, detail);
  const countKey = kind === "company" ? "companies" : "contacts";
  const next: Snapshot = {
    ...s,
    list: { ...s.list, rows: [row, ...s.list.rows] },
    counts: { ...s.counts, [countKey]: s.counts[countKey] + 1 },
  };
  return { snapshot: { ...next, list: repage(next, next.list) } };
}

/** `{ cmd: "set", id, field, value }` — edit one field in place. */
export function setField(s: Snapshot, id: Id, field: string, value: string): WriteResult {
  const key = field.trim().toLowerCase();
  const guard = NOT_SET_DIRECTLY[key];
  if (guard) return { error: `"${field}" is ${guard}` };
  const row = findRow(s, id);
  if (!row) return { error: "gone — that record no longer exists" };
  const trimmed = value.trim();
  if (!trimmed) return { error: `set needs a value — \`crm set ${row.handle} ${field} <value>\`` };

  // The record's own name/title lives in `row.label`, not in `focused.fields` — everything
  // else is whatever field entry the label matches.
  let next = key === "name" || key === "title" ? withRow(s, id, { label: trimmed }) : s;
  if (next.focused && next.focused.row.id === id) {
    next = {
      ...next,
      focused: {
        ...next.focused,
        fields: next.focused.fields.map((f) => (f.label.toLowerCase() === key ? { ...f, value: trimmed } : f)),
      },
    };
  }
  return { snapshot: { ...next, list: repage(next, next.list) } };
}

/** `{ cmd: "log", kind, id, body }` — append to the open record's timeline. */
export function logActivity(s: Snapshot, kind: ActivityKind, id: Id, body: string): WriteResult {
  const trimmed = body.trim();
  if (!trimmed) return { error: `log needs a body — \`crm log ${kind} <handle> "…"\`` };
  if (!s.focused || s.focused.row.id !== id) return { error: "gone — open the record again" };
  const entry = { id: ulid(), kind, body: trimmed, at: Date.now(), by: HUMAN };
  const timeline = [entry, ...s.focused.timeline].slice(0, 50);
  return {
    snapshot: {
      ...s,
      focused: { ...s.focused, timeline, timelineTotal: s.focused.timelineTotal + 1 },
      counts: { ...s.counts, activities: s.counts.activities + 1 },
    },
  };
}

/** `{ cmd: "task", id, what, due }` — a next step on the open record. */
export function addTask(s: Snapshot, id: Id, what: string, due: string): WriteResult {
  const trimmedWhat = what.trim();
  if (!trimmedWhat) return { error: `task needs what to do — \`crm task <handle> "…" --due <date>\`` };
  if (!due) return { error: "task needs a due date — `--due YYYY-MM-DD`" };
  if (!s.focused || s.focused.row.id !== id) return { error: "gone — open the record again" };
  const t = { id: ulid(), handle: handleFor(trimmedWhat), what: trimmedWhat, due, doneAt: null, by: HUMAN };
  return {
    snapshot: {
      ...s,
      focused: { ...s.focused, tasks: [...s.focused.tasks, t] },
      counts: { ...s.counts, tasks: s.counts.tasks + 1 },
    },
  };
}

/** `{ cmd: "done", id }` — complete a next step. One-way: there is no "undone" verb. */
export function doneTask(s: Snapshot, taskId: Id): WriteResult {
  if (!s.focused) return { error: "gone — open the record again" };
  const hit = s.focused.tasks.find((t) => t.id === taskId);
  if (!hit) return { error: "gone — that next step no longer exists" };
  if (hit.doneAt !== null) return { error: "already done" };
  const tasks = s.focused.tasks.map((t) => (t.id === taskId ? { ...t, doneAt: Date.now() } : t));
  return {
    snapshot: {
      ...s,
      focused: { ...s.focused, tasks },
      counts: { ...s.counts, tasks: Math.max(0, s.counts.tasks - 1) },
    },
  };
}

/** `{ cmd: "link", id, to }` — the mock demonstrates the one pair the record panel's Link
 *  control is actually for, a deal and a company; any other pair still round-trips (a
 *  confirmation, no visible change), since the real linking rules are the core's. */
export function linkRecords(s: Snapshot, id: Id, to: Id): WriteResult {
  if (id === to) return { error: "a record cannot be linked to itself" };
  const a = findRow(s, id);
  const b = findRow(s, to);
  if (!a || !b) return { error: "gone — one of those records no longer exists" };

  const dealSide = a.kind === "deal" ? id : b.kind === "deal" ? to : null;
  const companySide = a.kind === "company" ? a : b.kind === "company" ? b : null;
  let next = s;
  if (dealSide && companySide) {
    next = withRow(s, dealSide, { detail: companySide.label });
    if (next.focused && next.focused.row.id === dealSide) {
      next = {
        ...next,
        focused: {
          ...next.focused,
          fields: next.focused.fields.map((f) =>
            f.label === "Company" ? { ...f, value: `${companySide.label} (${companySide.handle})` } : f,
          ),
        },
      };
    }
  }
  return { snapshot: { ...next, list: repage(next, next.list) } };
}

/** `{ cmd: "archive", id, restore? }` */
export function archiveRecord(s: Snapshot, id: Id, restore: boolean): WriteResult {
  const row = findRow(s, id);
  if (!row) return { error: "gone — that record no longer exists" };
  if (restore && !row.archived) return { error: "that record is not archived" };
  if (!restore && row.archived) return { error: "already archived" };
  return { snapshot: withRow(s, id, { archived: !restore }) };
}

// MARK: - The scenarios

export type Scenario = {
  label: string;
  note: string;
  build: () => Snapshot;
  /** Something that arrives a moment after mount — an agent's write. The harness applies it
   *  on a timer; the render test applies it immediately and renders the result too. */
  then?: { after: number; apply: (s: Snapshot) => Snapshot };
};

/** Scenarios name deals the way a person would, by handle. */
function idOf(handle: string, seeds: Seed[] = SEEDS): Id {
  const hit = seeds.find((x) => x.handle === handle);
  if (!hit) throw new Error(`scenarios: no seed with handle "${handle}"`);
  return hit.id;
}

const HOLLIS = idOf("hollis-annual");

const INVENTED_SCENARIOS: Record<string, Scenario> = {
  pipeline: {
    label: "Pipeline",
    note: "The everyday board: two agents connected, six columns, two currencies.",
    build: () => openRecord(world(SEEDS), "deal", HOLLIS, history()),
  },

  move: {
    label: "Agent move",
    note: "Nia moves Hollis annual into Won after 1.2s. The card rings in her tint.",
    build: () => world(SEEDS),
    then: { after: 1200, apply: (s) => moveDeal(s, HOLLIS, "won", NIA) },
  },

  untouched: {
    label: "Untouched record",
    note: "A deal with nothing logged against it — the only state where the record panel prints `crm task` and `crm log`.",
    build: () => openRecord(world(SEEDS), "deal", idOf("kestrel-pilot")),
  },

  rename: {
    label: "Roster rename",
    note: "Nia is renamed after 1.2s. Same id, so the chip relabels in place — it must not flicker or reload its avatar.",
    build: () => world(SEEDS),
    then: {
      after: 1200,
      // A rename arrives as a fresh roster carrying the SAME id.
      apply: (s) => ({ ...s, agents: s.agents.map((a) => (a.id === AGENTS[0].id ? { ...a, name: "Nia (ops)" } : a)) }),
    },
  },

  pending: {
    label: "Pending",
    note: "Three companies match \"Acme\". Ambiguity is a state, not a guess.",
    build: () => {
      const acme = [companyNamed("Acme Holdings"), companyNamed("Acme Industrial"), companyNamed("Acme Laboratories")];
      const base = countsOf(SEEDS);
      return world(SEEDS, {
        // The three Acmes are real companies in this world, so the rail counts them — or it
        // would say 10 over a list of 13.
        counts: { ...base, companies: base.companies + acme.length },
        pending: {
          prompt: "Which Acme did you mean? Nia asked to log a call against it.",
          candidates: acme.map((r) => ({ kind: r.kind, id: r.id, handle: r.handle, label: r.label })),
        },
      });
    },
  },

  lopsided: {
    label: "Lopsided board",
    note: "Twenty-six deals in Qualified, nothing in Proposal or Negotiation — and a list long enough to page.",
    build: () => {
      const many = Array.from({ length: 26 }, (_, i) =>
        deal(
          `Inbound ${i + 1} — trial request`,
          `${BULK_COMPANIES[i % BULK_COMPANIES.length]} ${Math.floor(i / BULK_COMPANIES.length) + 1}`,
          usd(1000 * (i + 3)),
          "qualified",
          i % 3 === 0 ? NIA : HUMAN,
        ),
      );
      return world([...SEEDS.filter((s) => s.column === "lead" || s.column === "won"), ...many]);
    },
  },

  long: {
    label: "Long text",
    note: "A 40-character deal title, company names that have no intention of fitting, and a title full of shell metacharacters.",
    build: () => {
      const seeds = [
        deal(
          "Enterprise platform renewal — phase 2",
          "Interkontinentale Maschinenbau und Anlagentechnik GmbH & Co. KG",
          usd(1250000), "negotiation", NIA,
        ),
        deal(
          "Multi-region observability rollout AB",
          "Consolidated Southwestern Freight & Logistics Corporation",
          eur(430000), "proposal", HUMAN,
        ),
        // Every character `shellQuote` exists for. The record panel prints commands naming
        // this deal; they have to stay pasteable.
        deal(`O'Hara "Q4" $upsell \`now\``, "O'Hara & Sons", usd(9000), "lead", HUMAN),
        ...SEEDS.slice(0, 3),
      ];
      return openRecord(world(seeds), "deal", seeds[0].id, history());
    },
  },

  empty: {
    label: "Zero state",
    note: "No records at all. Empty states are one line of text and a verb that only creates.",
    build: () =>
      world([], {
        counts: { companies: 0, contacts: 0, deals: 0, activities: 0, tasks: 0 },
        due: { overdue: 0, today: 0, week: 0 },
        agents: [],
      }),
  },
};

/** The golden fixtures, as scenarios, first — they are the ones that matter. */
const GOLDEN_SCENARIOS: Record<string, Scenario> = Object.fromEntries(
  Object.entries(GOLDEN).map(([file, snapshot]) => [
    `golden:${file}`,
    {
      label: file === "snapshot.json" ? "Core: snapshot" : `Core: ${file.replace(/^snapshot-|\.json$/g, "")}`,
      note: `src-tauri/fixtures/${file}, verbatim — the bytes AppState::snapshot() actually emits.`,
      build: () => snapshot,
    },
  ]),
);

export const SCENARIOS: Record<string, Scenario> = { ...GOLDEN_SCENARIOS, ...INVENTED_SCENARIOS };
