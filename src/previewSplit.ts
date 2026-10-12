// The split shape, for the preview harness — and only for it.
//
// The fake core (`preview.ts`) keeps a `Snapshot` (the window's own model) as its world, because that is what the
// invented scenarios are built from. This projects that world into the messages the real core
// now sends — `bootstrap`, `update`, and the three stateless reads — so the window is exercised
// through the same hook and reducer it ships with. The golden split fixtures prove the *shape*
// (`split.test.ts`); this makes the *window* drivable in a browser.
//
// **It is a mock, and it is limited in the ways a mock is:** the data replica is built from what
// the world shows (cards, the list's rows, the open record), so `tasks` covers only the open
// record's next steps and `feed` only each deal's last move plus that record's timeline. The real
// core serves all of them. Row actions are emitted for deal moves only — the one write whose
// effect the window shows from the replica's feed.

import type { Actor, Card, Id, FeedItem, Kind, RecordRef, Row, Snapshot, TaskItem } from "./bridge";
import { asId, idKey } from "./bridge";
import { recordsFrom } from "./scenarios";
import type { Bootstrap, Op, Update } from "./split";

const ME = { id: "me", name: "Berk" };
const ORG = { id: "local", name: "Local" };

function ref(r: Row): RecordRef {
  return { kind: r.kind, id: r.id, handle: r.handle, label: r.label };
}

function stageWord(card: Card): string {
  return card.status === "open" ? (card.stage ?? "lead") : (card.status ?? "lead");
}

/** Each deal's last move, then the open record's timeline — newest first. */
export function feedItems(s: Snapshot): FeedItem[] {
  const items: FeedItem[] = [];
  for (const card of Object.values(s.cards)) {
    items.push({
      id: asId(`MOVE${idKey(card.id)}`.slice(0, 26)),
      kind: "stage",
      body: null,
      at: card.movedAt,
      by: card.by,
      record: ref(card),
      links: [ref(card)],
      to: stageWord(card),
    });
  }
  const f = s.focused;
  if (f) {
    for (const a of f.timeline) {
      items.push({ id: a.id, kind: a.kind, body: a.body, at: a.at, by: a.by, record: ref(f.row), links: [ref(f.row)] });
    }
  }
  return items.sort((a, b) => b.at - a.at);
}

const sameActor = (a: Actor, b: Actor) => a.kind === b.kind && (a.kind === "human" || (b.kind === "agent" && a.id === b.id));

export function feedOf(s: Snapshot, req: { actor?: unknown; kind?: unknown; page?: number; pageSize?: number }) {
  const want: Actor | null =
    req.actor === undefined || req.actor === null
      ? null
      : req.actor === "human"
        ? { kind: "human" }
        : { kind: "agent", id: String(req.actor) };
  const all = feedItems(s).filter((i) => (!want || sameActor(i.by, want)) && (!req.kind || i.kind === req.kind));
  const pageSize = Math.max(1, Math.min(req.pageSize ?? 25, 100));
  const page = req.page ?? 0;
  return { items: all.slice(page * pageSize, (page + 1) * pageSize), total: all.length, page, pageSize };
}

export function searchOf(s: Snapshot, q: string, limit = 8) {
  const needle = q.trim().toLowerCase();
  const hits = needle
    ? recordsFrom(s).filter((r) => !r.archived && `${r.label} ${r.detail ?? ""} ${r.handle}`.toLowerCase().includes(needle))
    : [];
  const counts: Record<Kind, number> = { company: 0, contact: 0, deal: 0 };
  for (const r of hits) counts[r.kind] += 1;
  return { query: q.trim(), total: hits.length, counts, rows: hits.slice(0, limit) };
}

function bucket(due: string, today: string): TaskItem["bucket"] {
  if (due < today) return "overdue";
  if (due === today) return "today";
  const week = new Date(`${today}T00:00:00`);
  week.setDate(week.getDate() + 7);
  return due <= week.toISOString().slice(0, 10) ? "week" : "later";
}

export function tasksOf(s: Snapshot, limit = 8) {
  const d = new Date();
  const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  const f = s.focused;
  const open: TaskItem[] = (f?.tasks ?? [])
    .filter((t) => t.doneAt === null)
    .sort((a, b) => a.due.localeCompare(b.due))
    .map((t) => ({
      id: t.id, handle: t.handle, what: t.what, due: t.due, doneAt: null, sentAt: null, by: t.by,
      bucket: bucket(t.due, today), record: f ? ref(f.row) : null,
    }));
  return { open: open.slice(0, limit), total: open.length };
}

function pipelineOf(s: Snapshot) {
  const col = (k: string) => s.board.columns.find((c) => c.key === k)?.totals ?? [];
  const byCurrency = new Map<string, { amount: number; currency: string; formatted: string }>();
  for (const c of s.board.columns.filter((c) => c.key !== "won" && c.key !== "lost")) {
    for (const t of c.totals) {
      const at = byCurrency.get(t.currency);
      byCurrency.set(t.currency, at ? { ...at, amount: at.amount + t.amount } : { ...t });
    }
  }
  // The preview has no core to format money, so it re-formats the sum the way its own totals are.
  const open = [...byCurrency.values()].map((t) => {
    const sample = s.board.columns.flatMap((c) => c.totals).find((x) => x.currency === t.currency);
    const sym = sample ? sample.formatted.replace(/[\d.,\s]/g, "") : "";
    return { ...t, formatted: `${sym}${(t.amount / 100).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}` };
  });
  return { open, won: col("won"), lost: col("lost") };
}

function projections(s: Snapshot) {
  return {
    view: {
      focus: s.focus, list: s.list, board: s.board, cards: s.cards, pending: s.pending, focused: s.focused ?? undefined,
      feed: s.agentFeed ?? [], follow: s.follow ?? true, page: s.page ?? null,
    },
    seat: { me: s.me ?? ME, agents: s.agents, reminders: s.reminders ?? null },
    derived: { counts: s.counts, due: s.due, pipeline: pipelineOf(s) },
  };
}

export function bootstrapOf(s: Snapshot, watermark: number): Bootstrap {
  const rows = recordsFrom(s);
  const stored = (r: Row) => ({ id: idKey(r.id), handle: r.handle, name: r.label, orgId: ORG.id, ownerId: ME.id });
  return {
    type: "bootstrap",
    shape: "split",
    rev: s.rev,
    watermark,
    org: ORG,
    ...projections(s),
    data: {
      pipeline: s.pipeline as unknown as Bootstrap["data"]["pipeline"],
      companies: rows.filter((r) => r.kind === "company").map(stored),
      contacts: rows.filter((r) => r.kind === "contact").map(stored),
      deals: Object.values(s.cards).map((c) => ({ ...stored(c), title: c.label, stage: c.stage, status: c.status, value: c.value })),
      tasks: [],
    },
    feed: feedOf(s, {}),
  } as unknown as Bootstrap;
}

/** A move's own id — distinct from the id the bootstrap gave that deal's *last* move, or the
 *  reducer would rightly take the new line for one it already has. */
function moveId(key: string, movedAt: number): Id {
  return asId(`D${movedAt}${key}`.slice(0, 26));
}

/** The row actions between two worlds: a deal that moved, as the core would say it. */
export function opsBetween(prev: Snapshot, next: Snapshot, from: number): Op[] {
  const ops: Op[] = [];
  for (const [key, card] of Object.entries(next.cards)) {
    const was = prev.cards[key];
    if (!was || was.movedAt === card.movedAt) continue;
    ops.push({
      seq: from + ops.length + 1, op: "upsert", kind: "deal", id: key,
      record: { id: key, handle: card.handle, title: card.label, stage: card.stage, status: card.status, movedAt: card.movedAt, movedBy: card.by },
    });
    ops.push({
      seq: from + ops.length + 1, op: "upsert", kind: "move", id: idKey(moveId(key, card.movedAt)),
      record: {
        id: moveId(key, card.movedAt), kind: "stage", body: null, at: card.movedAt, by: card.by,
        from: stageWord(was), to: stageWord(card), record: ref(card), links: [ref(card)],
      },
    });
  }
  return ops;
}

export function updateOf(s: Snapshot, from: number, ops: Op[]): Update {
  return {
    type: "update",
    shape: "split",
    rev: s.rev,
    watermark: from + ops.length,
    ...projections(s),
    delta: { from, to: from + ops.length, ops },
  } as unknown as Update;
}

