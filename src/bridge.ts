// The one seam between the window and the core. Every component talks to the app
// through this file and nothing else — no component imports `invoke`, and none of them
// knows there is a socket underneath.
//
// The transport half is clappkit's and is re-exported unchanged. This app's own types
// and its command envelope are here, because they are the shape of *our* core.

export { cmd, onState, useSnapshot, useAsset, prefetchAssets, agentTint } from "@clappkit";

// MARK: - The vocabulary
//
// **Any enum a surface shows lives in the core.** These are not the window's words: they
// are `Stage`, `Status`, `Kind`, `Sort` and `ActivityKind` in `src-tauri/src/model.rs`,
// mirrored here as string unions so TypeScript can hold us to them. If the board draws a
// "Negotiation" column, `crm move` takes that word and `crm -h` names it — and a core
// test pins all three against one list. Adding a word here without adding it there is
// the bug that rule exists to prevent.

/** The four open stages, in pipeline order. */
export const STAGES = ["lead", "qualified", "proposal", "negotiation"] as const;
export type Stage = (typeof STAGES)[number];

/** Open, or closed one of two ways. A different axis from `Stage`: closing a deal leaves
 *  its stage exactly where it was, so "how many did we win out of Negotiation" stays
 *  answerable. */
export type Status = "open" | "won" | "lost";

/** The six board columns: the four stages, then the two closings. */
export const COLUMN_KEYS = [...STAGES, "won", "lost"] as const;
export type ColumnKey = (typeof COLUMN_KEYS)[number];

export type Kind = "company" | "contact" | "deal";
export type Sort = "updated" | "name" | "value";
export type ActivityKind = "call" | "email" | "meeting" | "note";

// MARK: - Who wrote it
//
// Attribution is the feature. Every activity records the person or a specific agent, and
// the window draws that agent's avatar beside the line it logged.

/** Serialised from Rust's `Actor` with `#[serde(tag = "kind")]`. */
export type Actor = { kind: "human" } | { kind: "agent"; id: string };

/** One agent bound to this app, as Clatch last described it.
 *
 *  `id` is immutable and is what everything is keyed on; `name` is a re-pointable label
 *  we only ever display. A rename arrives as a fresh roster with the same id. */
export type Agent = {
  id: string;
  name: string;
  backend: string | null;
  model: string | null;
  /** An absolute file path, not bytes — resolve it with `useAsset`. */
  avatar: string | null;
};

/**
 * A stable key for an actor. **Never the display name.** The id is immutable for the
 * agent's whole lifetime; the name is unique but re-pointable, so keying on it would
 * silently re-attribute every line an agent ever wrote the moment somebody renamed it —
 * and, in React, would drop and re-create the row instead of relabelling it in place.
 */
export function actorKey(by: Actor): string {
  return by.kind === "human" ? "human" : `agent:${by.id}`;
}

// MARK: - Money
//
// Minor units and an ISO 4217 code, never a float: summing an empty list of floats yields
// `-0.0`, so an empty pipeline prints `$-0.00`. An integer has no negative zero, so that
// failure is absent rather than fixed.

export type Money = { amount: number; currency: string };

/** Currencies whose minor unit is not 1/100. Mirrors `Money::exponent` in
 *  `src-tauri/src/model.rs` — see the note on `formatMoney`. */
const EXPONENTS: Record<string, number> = {
  BIF: 0, CLP: 0, DJF: 0, GNF: 0, ISK: 0, JPY: 0, KMF: 0, KRW: 0, PYG: 0, RWF: 0,
  UGX: 0, UYI: 0, VND: 0, VUV: 0, XAF: 0, XOF: 0, XPF: 0,
  BHD: 3, IQD: 3, JOD: 3, KWD: 3, LYD: 3, OMR: 3, TND: 3,
};

/**
 * The amount as a person reads it — `45000.00`, `4500`, `45.000` — without the symbol.
 *
 * **This mirrors `Money::format()` in the core, and that is a seam worth naming.** The
 * core comments that formatting lives there "because both surfaces show totals and two
 * formatters is two answers" — but the snapshot ships `{ amount, currency }` raw, so the
 * window has no formatted string to render and has to do the arithmetic itself. The two
 * implementations agree today, digit for digit, including the exponent table above.
 *
 * The durable fix is for the snapshot to carry a formatted string beside the raw amount;
 * that is a shape change, so it is the PM's, not ours. Recorded in `docs/window.md`.
 */
export function formatMoney(m: Money): string {
  const places = EXPONENTS[m.currency] ?? 2;
  if (places === 0) return String(m.amount);
  const unit = 10 ** places;
  const sign = m.amount < 0 ? "-" : "";
  const magnitude = Math.abs(m.amount);
  const major = Math.floor(magnitude / unit);
  const minor = magnitude % unit;
  return `${sign}${major}.${String(minor).padStart(places, "0")}`;
}

/** Amount and code, the way a column header shows it: `45000.00 USD`. The code rather
 *  than a symbol, because the board holds several currencies side by side and `$` in two
 *  columns that are not both dollars is the kind of wrong nobody catches. */
export function money(m: Money): string {
  return `${formatMoney(m)} ${m.currency}`;
}

// MARK: - The snapshot — frozen in docs/work-orders/m0-m1-backend.md
//
// The window builds against this and does not get to change it: M2 is being built against
// the same shape. Mirrored field for field from `AppState::snapshot()`.

export type Focus = { kind: Kind; id: string };

export type Pipeline = { id: string; name: string; stages: Stage[] };

/** One board column. `dealIds` indexes into `cards` — see the note there. */
export type BoardColumn = {
  key: ColumnKey;
  label: string;
  dealIds: string[];
  count: number;
  /** **Grouped by currency and never summed across them.** We hold no rate source, and
   *  inventing one would be worse than showing two numbers. Render two lines. */
  totals: Money[];
};

export type Board = {
  pipelineId: string;
  stageFilter: Stage | null;
  columns: BoardColumn[];
};

/** One row of the shared list, in the shape both this table and `crm find` render. The
 *  deal-only fields are `null` on a company or a contact rather than absent, so the table
 *  has one shape to draw. */
export type Row = {
  kind: Kind;
  id: string;
  label: string;
  detail: string | null;
  stage: Stage | null;
  status: Status | null;
  value: Money | null;
  archived: boolean;
};

/** The shared result list. **`pageSize` is state, not a caller's request** — an agent
 *  passing `-n 3` limits what its own terminal prints and must not repaginate the
 *  person's table to three rows. Sort is state for the same reason. */
export type ListView = {
  query: string;
  sort: Sort;
  page: number;
  pageSize: number;
  /** Every id that matched, not just this page — so both surfaces agree on the total. */
  total: number;
  rows: Row[];
  /**
   * Which record type the list is narrowed to, or null for all three.
   *
   * **Proposed; see the block below.** The rail's People and Companies entries are a
   * filter on this one shared list, and the filter has to live in the core for the same
   * reason sort and page do: narrowing the rows in the window alone would leave the
   * footer saying "25 of 143" over four visible rows, and would leave the agent looking
   * at a list the person is not. That is the drift §6 exists to prevent, so the window
   * writes the filter through `find` and reads it back here rather than holding it
   * locally.
   */
  kind?: Kind | null;
};

export type Candidate = { kind: Kind; id: string; label: string };

/** Ambiguity is a state, not a guess and not an error. */
export type Pending = { prompt: string; candidates: Candidate[] };

export type Due = { overdue: number; today: number; week: number };

export type Counts = {
  companies: number;
  contacts: number;
  deals: number;
  activities: number;
  tasks: number;
};

// MARK: - What the frozen snapshot does not carry
//
// **These three types are a PROPOSAL, not the contract.** The frozen shape gives the
// board `dealIds` and gives `focus` a `{ kind, id }` — but no deal bodies and no
// activities, so as frozen it cannot feed the deal card, the record detail or the
// timeline, which are three of M3's eleven components.
//
// That is a PM conversation, not a core edit, and it is raised in `docs/window.md`. The
// window is built against the shape below and **degrades rather than breaks** when the
// fields are absent: the board draws its columns, counts and totals from what the
// snapshot really carries, and only the card bodies and the detail panel fall back to an
// empty state. Nothing here changes a field M2 is already building against; every
// addition is new and optional.

/** A deal as a board card: three fields and who last moved it. Deliberately the `Row`
 *  shape plus `by`, so the core can reuse the serialiser it already has. */
export type Card = Row & { by: Actor };

/** One line of a record's timeline. */
export type Activity = {
  id: string;
  kind: ActivityKind;
  body: string;
  at: number;
  by: Actor;
};

/** A next step. The only thing that can wake an agent. */
export type Task = {
  id: string;
  what: string;
  due: string;
  doneAt: number | null;
  by: Actor;
};

/** The record `focus` points at, with everything the detail panel draws. */
export type Focused = {
  row: Row;
  fields: { label: string; value: string }[];
  timeline: Activity[];
  tasks: Task[];
};

// MARK: - The whole of it

/**
 * Everything both surfaces agree on.
 *
 * `rev` orders the two writers that feed this window — our own `run_cmd` reply and the
 * `state` event the core pushes — and `useSnapshot` drops whichever arrives stale. Do not
 * write a second store around it.
 */
export type Snapshot = {
  ok: boolean;
  rev: number;
  pipeline: Pipeline;
  board: Board;
  focus: Focus | null;
  list: ListView;
  pending: Pending | null;
  due: Due;
  counts: Counts;
  agents: Agent[];

  /** Proposed; see the block above. Absent until the PM settles the shape. */
  cards?: Record<string, Card>;
  /** Proposed; see the block above. */
  focused?: Focused | null;
};

/** The command envelope both surfaces send. The CLI sends the same shape over the
 *  socket, which is what keeps one implementation of every rule. */
export type Command = { cmd: string; [arg: string]: unknown };

/** The state a window shows before the core has answered. Never rendered for long: the
 *  first snapshot is asked for on mount. */
export const EMPTY: Snapshot = {
  ok: true,
  rev: -1,
  pipeline: { id: "sales", name: "Sales", stages: [...STAGES] },
  board: { pipelineId: "sales", stageFilter: null, columns: [] },
  focus: null,
  list: { query: "", sort: "updated", page: 0, pageSize: 25, total: 0, rows: [] },
  pending: null,
  due: { overdue: 0, today: 0, week: 0 },
  counts: { companies: 0, contacts: 0, deals: 0, activities: 0, tasks: 0 },
  agents: [],
};

// MARK: - Shared wording
//
// The pagination footer has to read the same as `crm find`'s. One string, defined once,
// so the two cannot drift by an em dash.

/**
 * `"25 of 143"` — how many rows are on the page both surfaces are looking at, of how many
 * matched in total.
 *
 * **`crm find` must print this exact string.** It is a shared-surface promise from
 * `docs/architecture.md` §6, not a label: page size is shared state, so if the two
 * surfaces word it differently the person and their agent are describing the same page to
 * each other in different numbers. The CLI is Rust and cannot import this, so the
 * agreement is by review — flagged for M2 in `docs/window.md`.
 */
export function pageWording(list: ListView): string {
  return `${list.rows.length} of ${list.total}`;
}

/** `"page 2 of 6"`, or null when there is only one. Separate from `pageWording` because
 *  the shared promise above is about rows, and a page counter that pretended to be part
 *  of it would be a second string to keep in step. */
export function pageOf(list: ListView): string | null {
  const pages = list.pageSize > 0 ? Math.ceil(list.total / list.pageSize) : 0;
  if (pages <= 1) return null;
  return `page ${list.page + 1} of ${pages}`;
}
