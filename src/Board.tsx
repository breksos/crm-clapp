import { useEffect, useRef, useState } from "react";
import {
  agentTint, asId, cardOf, idKey, STAGES,
  type Actor, type Agent, type Board, type Card, type ColumnKey, type Command, type Id, type Snapshot, type Stage,
} from "./bridge";
import { Disc } from "./Attribution";
import { addDealCmd } from "./commands";
import { ErrorLine } from "./Forms";
import { PlusIcon, XIcon } from "./icons";
import { useWrite } from "./useWrite";

/** The drag payload the board reads back. Private, so nothing but this board can drop one,
 *  and it carries the id — which is fine, because nobody reads it. */
const DEAL_MIME = "application/x-breksos-deal";

/**
 * Which cards moved since the last snapshot, and what colour to ring them.
 *
 * **This is what the milestone is for.** A person has this window open beside their
 * terminal while their agent works; when a card moves because the agent moved it, they have
 * to see that it happened and who did it.
 *
 * A move is a change in the card's `movedAt` — the core stamps it on every `move` — rather
 * than a change of column. The two agree today, but `movedAt` is the field the contract
 * defines for exactly this, and it is the one that stays right if a card is ever moved and
 * moved back between two snapshots. A card seen for the first time is new, not moved, so the
 * first snapshot after launch rings nothing.
 *
 * The ring is `by`'s colour: an agent's own tint from clappkit's djb2 over its immutable id,
 * so an agent is the same colour here as in the strip above. A move the person made rings
 * in the accent.
 *
 * **Each ring owns its own timer.** The first version cleared its timer in the effect's
 * cleanup, which runs on *every* new snapshot — so any push inside the 1.2s window (the
 * agent's very next write, typically) cancelled the clear and left the ring on for good.
 * Timers live in a ref now and are only cancelled on unmount.
 */
function useMoveRings(board: Board, cards: Snapshot["cards"]): Map<string, string> {
  const seen = useRef(new Map<string, number>());
  const timers = useRef(new Map<string, number>());
  const [rings, setRings] = useState(new Map<string, string>());

  useEffect(() => {
    const now = new Map<string, number>();
    const moved: [string, string][] = [];
    for (const column of board.columns) {
      for (const id of column.dealIds) {
        const card = cardOf({ cards }, id);
        if (!card) continue;
        const key = idKey(id);
        now.set(key, card.movedAt);
        const was = seen.current.get(key);
        if (was !== undefined && was !== card.movedAt) moved.push([key, tintOf(card.by)]);
      }
    }
    seen.current = now;
    if (moved.length === 0) return;

    setRings((held) => new Map([...held, ...moved]));
    for (const [key] of moved) {
      window.clearTimeout(timers.current.get(key));
      timers.current.set(
        key,
        window.setTimeout(() => {
          timers.current.delete(key);
          setRings((held) => {
            const next = new Map(held);
            next.delete(key);
            return next;
          });
        }, 1200),
      );
    }
  }, [board, cards]);

  useEffect(() => {
    const live = timers.current;
    return () => {
      for (const t of live.values()) window.clearTimeout(t);
    };
  }, []);

  return rings;
}

function tintOf(by: Actor): string {
  return by.kind === "agent" ? agentTint(by.id) : "var(--accent)";
}

export function BoardView({
  board,
  cards,
  agents,
  focusId,
  run,
  apply,
}: {
  board: Board;
  cards: Snapshot["cards"];
  agents: Agent[];
  focusId: Id | null;
  run: (c: Command) => void;
  apply: (next: Snapshot) => void;
}) {
  const rings = useMoveRings(board, cards);
  const [over, setOver] = useState<ColumnKey | null>(null);

  return (
    <div className="board">
      {board.columns.map((column) => (
        <section
          key={column.key}
          className={`column${over === column.key ? " column-over" : ""}`}
          onDragOver={(e) => {
            // Only our own cards are droppable here. A file or a snippet of text dragged in
            // from elsewhere carries no deal, and pretending it might would make the column
            // light up for a drop that does nothing.
            if (!e.dataTransfer.types.includes(DEAL_MIME)) return;
            e.preventDefault();
            setOver(column.key);
          }}
          onDragLeave={() => setOver((k) => (k === column.key ? null : k))}
          onDrop={(e) => {
            setOver(null);
            const raw = e.dataTransfer.getData(DEAL_MIME);
            if (!raw) return;
            e.preventDefault();
            const id = asId(raw);
            // A drop back where it started is not a move: sending it would write a
            // snapshot, push it to the agent, and ring a card that did not go anywhere.
            if (column.dealIds.some((d) => d === id)) return;
            run({ cmd: "move", id: idKey(id), to: column.key });
          }}
        >
          <header className="column-head">
            <h2>{column.label}</h2>
            <span className="column-count num">{column.count}</span>
            {/* Grouped by currency, one line each, and **never summed across them**: we
                hold no rate source, and one wrong number is worse than two right ones.
                The string is the core's; this window formats no money. */}
            <div className="column-totals">
              {column.totals.length === 0 ? (
                <span className="column-total num">—</span>
              ) : (
                column.totals.map((t) => (
                  <span className="column-total num" key={t.currency}>
                    {t.formatted}
                  </span>
                ))
              )}
            </div>
          </header>

          <div className="column-cards">
            {column.dealIds.length === 0 ? <p className="column-empty">Nothing here.</p> : null}
            {column.dealIds.map((id) => {
              const key = idKey(id);
              return (
                <DealCard
                  key={key}
                  id={id}
                  card={cardOf({ cards }, id)}
                  agents={agents}
                  ring={rings.get(key)}
                  focused={focusId === id}
                  run={run}
                />
              );
            })}
            {/* `add deal` takes a stage — the four open ones — so Won and Lost get no New
                control here. There is no sensible envelope for "create a deal already
                closed"; a deal earns Won or Lost by being moved there. */}
            {isStage(column.key) ? (
              <NewDealComposer stage={column.key} showHint={column.dealIds.length === 0} apply={apply} />
            ) : null}
          </div>
        </section>
      ))}
    </div>
  );
}

function isStage(key: ColumnKey): key is (typeof STAGES)[number] {
  return (STAGES as readonly string[]).includes(key);
}

/**
 * The board's per-column New control (`m7-window-editing.md`'s table: "a New control on
 * each rail view, and on the board per column — the column decides the stage"). Collapsed
 * to a slim "+ New deal" row at rest; the empty case additionally carries the same quiet
 * CLI line the empty board used to show *alone* — a control now, with the hint kept
 * beside it, exactly the demotion `m7-window-editing.md` asks for.
 */
function NewDealComposer({
  stage,
  showHint,
  apply,
}: {
  stage: Stage;
  showHint: boolean;
  apply: (next: Snapshot) => void;
}) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [company, setCompany] = useState("");
  const [value, setValue] = useState("");
  const [currency, setCurrency] = useState("");
  const { send, busy, error, dismiss } = useWrite(apply);

  if (!open) {
    return (
      <div className="composer-closed">
        <button type="button" className="composer-open" onClick={() => setOpen(true)}>
          <PlusIcon size={14} />
          New deal
        </button>
        {showHint ? (
          <p className="empty-line">
            Your agent can add one: <code>{addDealCmd("Northwind renewal", stage)}</code>
          </p>
        ) : null}
      </div>
    );
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!title.trim()) return; // the window may stop an empty required field; nothing else
    const ok = await send({
      cmd: "add",
      kind: "deal",
      name: title,
      fields: { company: company.trim() || undefined, value: value.trim() || undefined, currency: currency.trim() || undefined, stage },
    });
    if (ok) {
      setOpen(false);
      setTitle("");
      setCompany("");
      setValue("");
      setCurrency("");
    }
  }

  return (
    <form className="composer" onSubmit={submit}>
      <div className="composer-row">
        <input
          className="composer-input"
          placeholder="Deal title"
          aria-label="Deal title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          autoFocus
        />
        <button type="button" className="icon-button" aria-label="Cancel" onClick={() => setOpen(false)}>
          <XIcon size={14} />
        </button>
      </div>
      <div className="composer-row">
        <input
          className="composer-input"
          placeholder="Company handle (optional)"
          aria-label="Company handle"
          value={company}
          onChange={(e) => setCompany(e.target.value)}
        />
      </div>
      <div className="composer-row">
        <input
          className="composer-input composer-input-value"
          placeholder="Value (optional)"
          aria-label="Value"
          inputMode="decimal"
          value={value}
          onChange={(e) => setValue(e.target.value)}
        />
        <input
          className="composer-input composer-input-currency"
          placeholder="USD"
          aria-label="Currency"
          maxLength={3}
          value={currency}
          onChange={(e) => setCurrency(e.target.value.toUpperCase())}
        />
      </div>
      <button type="submit" className="composer-submit" disabled={busy || !title.trim()}>
        Add to {stage[0].toUpperCase() + stage.slice(1)}
      </button>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </form>
  );
}

/**
 * A deal is a card because a deal *is* a card — one of the few things in this window that
 * earns the shape.
 *
 * **Three fields and no more**: title, company, value. The fourth slot is the attribution
 * disc, which is not a field but the answer to "who moved this".
 *
 * `id` is the opaque `Id`, not a string, so `{id}` anywhere in this component is a compile
 * error. That is deliberate: round 2 fell back to rendering the id when a body was missing,
 * and degrading to an id is the same bug as printing one.
 */
function DealCard({
  id,
  card,
  agents,
  ring,
  focused,
  run,
}: {
  id: Id;
  card: Card | undefined;
  agents: Agent[];
  ring?: string;
  focused: boolean;
  run: (c: Command) => void;
}) {
  // The core sends a body for every id on the board, so this is a contract breach rather
  // than a state — but if it happens the column still shows the right *number* of deals,
  // as neutral placeholders, and nothing a person could mistake for a name.
  if (!card) {
    return (
      <article className="card card-skeleton" aria-label="A deal the core has not described">
        <span className="skeleton-line" />
        <span className="skeleton-line skeleton-short" />
      </article>
    );
  }

  const open = () => run({ cmd: "show", kind: "deal", id: idKey(id) });

  return (
    <article
      className={`card${focused ? " card-focused" : ""}${ring ? " card-ringed" : ""}`}
      style={ring ? ({ "--ring-tint": ring } as React.CSSProperties) : undefined}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = "move";
        // Two payloads. The private one is what this board reads back. `text/plain` is what
        // lands anywhere else — the agent's terminal, most likely — so it is the handle:
        // something the person can type back into a verb, never the id.
        e.dataTransfer.setData(DEAL_MIME, idKey(id));
        e.dataTransfer.setData("text/plain", card.handle);
      }}
      tabIndex={0}
      role="button"
      onClick={open}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          open();
        }
      }}
    >
      <div className="card-top">
        <span className="card-title" title={card.label}>{card.label}</span>
        <Disc by={card.by} agents={agents} />
      </div>
      <span className="card-company" title={card.detail ?? undefined}>{card.detail ?? "—"}</span>
      {card.value ? <span className="card-value num">{card.value.formatted}</span> : null}
    </article>
  );
}
