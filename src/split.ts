// The split shape (p1): view state per seat, record data per org.
//
// The core now publishes two things where it published one. **View state** — `focus`, `list`,
// `board`, `cards`, `pending`, `focused` — is this seat's alone, recomputed on every message and
// ordered by `rev`. **Record data** is the org's, delivered as a bootstrap and then row actions,
// ordered by the server **watermark**. Two counters, two jobs; this file never merges them.
//
//   rev        orders what the window DRAWS.   A message older than the view held is dropped.
//   watermark  orders what the DATA did.       An update applies only if it starts where we are.
//
// Everything here is pure: no React, no Tauri, no clock. `useSplit.ts` is the hook around it, and
// `toSnapshot` is the one place the new shape is folded into the `Snapshot` the components read —
// so a component is moved onto the new shape by reading a new field, never by learning a second
// way to be fed. Written against `src-tauri/fixtures/split/*.json`, the core's own output.

import type { AgentCall, Feed, FeedItem, Money, PageHint, Snapshot } from "./bridge.ts";

// MARK: - The wire

/** Stored fields only, exactly as the core sends them (camelCase). Derived facts ride the view. */
export type Row = { id: string; [field: string]: unknown };

export type Data = {
  pipeline: { id: string; name: string; stages: string[] };
  companies: Row[];
  contacts: Row[];
  deals: Row[];
  tasks: Row[];
};

export type View = Pick<Snapshot, "focus" | "list" | "board" | "cards" | "pending"> & {
  focused?: Snapshot["focused"];
  /** W1: the agent's calls, newest first; whether this seat follows; where the last read pointed. */
  feed?: AgentCall[];
  follow?: boolean;
  page?: PageHint | null;
};

export type Seat = {
  me: { id: string; name: string };
  agents: Snapshot["agents"];
  reminders: Snapshot["reminders"];
  /** p3 (`sync-contract.md` §5.8). Absent from a core that predates it: local mode. */
  sync?: Snapshot["sync"];
  members?: Snapshot["members"];
  teamAgents?: Snapshot["teamAgents"];
};

export type Derived = {
  counts: Snapshot["counts"];
  due: Snapshot["due"];
  pipeline: { open: Money[]; won: Money[]; lost: Money[] };
};

/** One row action. `seq` is the watermark this action took. */
export type Op = { seq: number; op: "upsert"; kind: string; id: string; record: Record<string, unknown> };

export type Bootstrap = {
  type: "bootstrap";
  shape: "split";
  rev: number;
  watermark: number;
  org: { id: string; name: string };
  view: View;
  seat: Seat;
  derived: Derived;
  data: Data;
  feed: Feed;
};

export type Update = {
  type: "update";
  shape: "split";
  rev: number;
  watermark: number;
  view: View;
  seat: Seat;
  derived: Derived;
  delta: { from: number; to: number; ops: Op[] };
};

export type Changes = { watermark: number; from: number; to: number; ops: Op[] } | { stale: true; watermark: number };

// MARK: - The state the window holds

export type SplitState = {
  rev: number;
  watermark: number;
  org: Bootstrap["org"];
  view: View;
  seat: Seat;
  derived: Derived;
  /** The org's rows as the window has them: a bootstrap, then deltas. */
  data: Data;
  /** The first page of the feed, kept live by `activity`/`move` row actions. */
  feed: Feed;
};

export function fromBootstrap(b: Bootstrap): SplitState {
  return { rev: b.rev, watermark: b.watermark, org: b.org, view: b.view, seat: b.seat, derived: b.derived, data: b.data, feed: b.feed };
}

export function isBootstrap(m: unknown): m is Bootstrap {
  return typeof m === "object" && m !== null && (m as { type?: unknown }).type === "bootstrap";
}

export function isUpdate(m: unknown): m is Update {
  return typeof m === "object" && m !== null && (m as { type?: unknown }).type === "update";
}

// MARK: - Applying row actions

const TABLE = { company: "companies", contact: "contacts", deal: "deals", task: "tasks" } as const;

function upsert(rows: Row[], record: Row): Row[] {
  const at = rows.findIndex((r) => r.id === record.id);
  if (at < 0) return [...rows, record];
  const next = rows.slice();
  next[at] = record;
  return next;
}

/** Fold one row action into the data and the feed. `activity` and `move` are feed items —
 *  appended, never replacing; everything else is a row, replacing the client's copy. */
export function applyOp(state: SplitState, op: Op): SplitState {
  if (op.kind === "activity" || op.kind === "move") {
    const item = op.record as unknown as FeedItem;
    if (state.feed.items.some((i) => i.id === item.id)) return state;
    // Newest first, and the page stays a page: the first `pageSize` of an ever-growing log.
    const items = [item, ...state.feed.items].sort((a, b) => b.at - a.at).slice(0, state.feed.pageSize);
    return { ...state, feed: { ...state.feed, items, total: state.feed.total + 1 } };
  }
  const table = TABLE[op.kind as keyof typeof TABLE];
  if (!table) return state; // a kind this window predates: ignoring it is safe, it holds no copy
  return { ...state, data: { ...state.data, [table]: upsert(state.data[table], op.record as Row) } };
}

export type Applied =
  | { state: SplitState; resync: false }
  /** The update did not start where the data is: ask for `changes`, or bootstrap again. */
  | { state: SplitState; resync: true };

/**
 * Fold an update in.
 *
 *  - **View, seat, derived** replace what is held unless the message is older (`rev`).
 *  - **Row actions** apply only when `delta.from` is the watermark held. Already applied
 *    (`to <= watermark`): nothing to do. A gap (`from > watermark`): do not guess — flag it.
 */
export function applyUpdate(state: SplitState, u: Update): Applied {
  const fresher = u.rev >= state.rev;
  let next: SplitState = fresher
    ? { ...state, rev: u.rev, view: u.view, seat: u.seat, derived: u.derived }
    : state;

  if (u.delta.to <= state.watermark) return { state: next, resync: false };
  if (u.delta.from !== state.watermark) return { state: next, resync: true };

  for (const op of u.delta.ops) next = applyOp(next, op);
  return { state: { ...next, watermark: u.delta.to }, resync: false };
}

/** What `changes` returned: the actions the window missed, in order. `stale` means the log no
 *  longer reaches back that far — bootstrap again; it is never a partial answer. */
export function applyChanges(state: SplitState, c: Changes): Applied {
  if ("stale" in c) return { state, resync: true };
  if (c.from !== state.watermark) return { state, resync: c.to > state.watermark };
  let next = state;
  for (const op of c.ops) next = applyOp(next, op);
  return { state: { ...next, watermark: c.to }, resync: false };
}

// MARK: - Into the Snapshot the components read

/** The window's one picture of the world. The view is the seat's; the rest rides beside it. */
export function toSnapshot(s: SplitState): Snapshot {
  return {
    ok: true,
    rev: s.rev,
    pipeline: s.data.pipeline as Snapshot["pipeline"],
    board: s.view.board,
    focus: s.view.focus,
    list: s.view.list,
    pending: s.view.pending,
    due: s.derived.due,
    counts: s.derived.counts,
    agents: s.seat.agents,
    sync: s.seat.sync,
    members: s.seat.members,
    teamAgents: s.seat.teamAgents,
    reminders: s.seat.reminders,
    cards: s.view.cards,
    focused: s.view.focused ?? null,
    me: s.seat.me,
    org: s.org,
    watermark: s.watermark,
    pipelineTotals: s.derived.pipeline,
    feed: s.feed,
    agentFeed: s.view.feed ?? [],
    follow: s.view.follow ?? true,
    page: s.view.page ?? null,
  };
}
