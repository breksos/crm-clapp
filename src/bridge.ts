// The one seam between the window and the core. Every component talks to the app
// through this file and nothing else — no component imports `invoke`, and none of them
// knows there is a socket underneath.
//
// The transport half is clappkit's and is re-exported unchanged. This app's own types
// and its command envelope are here, because they are the shape of *our* core.

export { cmd, onState, useSnapshot, useAsset, prefetchAssets, agentTint } from "@clappkit";

// The id/handle namespaces live in `ids.ts` and are re-exported here, so a component still
// has one seam to import from. They are separate because `ids.ts` must stay importable
// without a bundler — the guard test runs it under bare `node --test`.
export { asHandle, asId, findIds, idKey, looksLikeId, type Handle, type Id } from "./ids";
import { idKey, type Handle, type Id } from "./ids";

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
// Minor units and an ISO 4217 code, never a float — and, since round 3, the string a person
// reads, formatted by the core.
//
// **The window has no money formatter.** It had one until round 3: a mirror of
// `Money::format()` that agreed with it digit for digit, which is exactly the "two
// formatters is two answers" the core's own comment warns about. The snapshot now carries
// `formatted` on every money value, so the mirror is gone and every surface prints what the
// one implementation produced. `amount` stays because sorting and comparison need it — it is
// never displayed.

export type Money = { amount: number; currency: string; formatted: string };

// MARK: - The snapshot — frozen in m0-m1-backend.md, extended additively by round 3
//
// The window builds against this and does not get to change it. Mirrored field for field
// from `AppState::snapshot()`, and tested against the core's own golden output in
// `src-tauri/fixtures/` — not against snapshots this window invented, which is how round 2's
// blocker survived.

/** What is open. `handle` is looked up by the core rather than stored — a handle is
 *  derived data — so it is null for an id that no longer resolves. */
export type Focus = { kind: Kind; id: Id; handle: Handle | null };

export type Pipeline = { id: string; name: string; stages: Stage[] };

/** One board column. `dealIds` indexes into `cards` — see the note there. */
export type BoardColumn = {
  key: ColumnKey;
  label: string;
  /** Ids, not handles: this indexes `cards`, and nothing here is displayed. */
  dealIds: Id[];
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
  id: Id;
  /** What to print so the reader has something they can type. */
  handle: Handle;
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
   * Which record type the list is narrowed to, or null for all of them.
   *
   * Shared for the same reason sort and page are: narrowing the rows in the window alone
   * would leave the footer saying "25 of 143" over four visible rows, and would leave the
   * agent looking at a list the person is not. So the window writes it through `find` and
   * reads it back here; it never filters locally. `crm find --kind` sets the same field.
   */
  kind: Kind | null;
};

export type Candidate = { kind: Kind; id: Id; handle: Handle; label: string };

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

// MARK: - The record bodies — round 3
//
// Until round 3 the snapshot gave the board `dealIds` and gave `focus` a bare reference,
// and from that the window could not draw one card title. These were the window's proposal
// in `docs/window.md`; round 3 froze them, with two corrections the frontend had wrong:
// `by` is who last *moved* the card, not who last logged against it, and `movedAt` is what
// says a move happened.

/** A deal as a board card: the row, who last put it where it is, and when. */
export type Card = Row & {
  /** `Deal.moved_by` verbatim. A stage move is not an activity, so this is not derived
   *  from the timeline — that would tint the ring for whoever last logged a call. */
  by: Actor;
  /** `Deal.moved_at` verbatim. A card whose `movedAt` changed is a card that moved. */
  movedAt: number;
};

/** One line of a record's timeline. */
export type Activity = {
  id: Id;
  kind: ActivityKind;
  body: string;
  at: number;
  by: Actor;
};

/** A next step. The only thing that can wake an agent. */
export type Task = {
  id: Id;
  /** What `crm done <task-handle>` takes. */
  handle: Handle;
  what: string;
  due: string;
  doneAt: number | null;
  by: Actor;
};

/** The record `focus` points at, with everything the detail panel draws. */
export type Focused = {
  row: Row;
  /** From one function in the core, which `crm show` prints too — same labels, same
   *  values, same order. The window renders them; it never composes its own. */
  fields: { label: string; value: string }[];
  /** The 50 newest, newest first. A snapshot is pushed on every change, so an unbounded
   *  timeline would make every push as large as the busiest record's whole history. */
  timeline: Activity[];
  /** How many there are in all, so the panel can say it is showing a slice. */
  timelineTotal: number;
  /** Open first. */
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

  /** One per id in any `board.columns[].dealIds`, keyed by that id. Read it through
   *  `cardOf`, never by indexing with a stringified id at the call site. */
  cards: Record<string, Card>;
  /** Present if and only if `focus` is non-null. */
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
  list: { query: "", sort: "updated", page: 0, pageSize: 25, total: 0, rows: [], kind: null },
  pending: null,
  due: { overdue: 0, today: 0, week: 0 },
  counts: { companies: 0, contacts: 0, deals: 0, activities: 0, tasks: 0 },
  agents: [],
  cards: {},
  focused: null,
};

/** The body behind a board id, or undefined if the core did not send one. */
export function cardOf(snapshot: Pick<Snapshot, "cards">, id: Id): Card | undefined {
  return snapshot.cards[idKey(id)];
}

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
