import { useEffect, useRef, useState } from "react";
import {
  agentTint, asHandle, asId, money,
  type Actor, type Agent, type Board, type Card, type ColumnKey, type Command,
} from "./bridge";
import { Disc } from "./Attribution";
import { addDealCmd } from "./commands";

/**
 * Which cards moved between the last snapshot and this one, and what colour to ring them.
 *
 * **This is what the milestone is for.** A person has this window open beside their
 * terminal while their agent works; when a card changes column because the agent moved it,
 * they have to see that it happened and who did it. Without this the board just silently
 * differs from the one they were looking at a second ago.
 *
 * A deal with no previous column is new, not moved, so it does not ring — otherwise the
 * first snapshot after launch would ring the entire board.
 *
 * The ring is coloured by whoever moved it: an agent's own tint, from the same djb2 over
 * its immutable id that every other app in this family uses, so Nia is the same colour
 * here as she is in the strip above. A move the person made themselves rings in the accent
 * — they know what they just did, but the confirmation costs nothing and keeps one code
 * path.
 */
function useMoveRings(board: Board, cards: Record<string, Card> | undefined): Map<string, string> {
  const previous = useRef(new Map<string, ColumnKey>());
  const [rings, setRings] = useState(new Map<string, string>());

  useEffect(() => {
    const now = new Map<string, ColumnKey>();
    for (const column of board.columns) {
      for (const id of column.dealIds) now.set(id, column.key);
    }

    const moved: [string, string][] = [];
    for (const [id, key] of now) {
      const was = previous.current.get(id);
      if (was !== undefined && was !== key) moved.push([id, tintOf(cards?.[id]?.by)]);
    }
    previous.current = now;
    if (moved.length === 0) return;

    setRings((held) => new Map([...held, ...moved]));
    const timer = window.setTimeout(() => {
      setRings((held) => {
        const next = new Map(held);
        for (const [id] of moved) next.delete(id);
        return next;
      });
    }, 1200);
    return () => window.clearTimeout(timer);
  }, [board, cards]);

  return rings;
}

function tintOf(by: Actor | undefined): string {
  return by && by.kind === "agent" ? agentTint(by.id) : "var(--accent)";
}

export function BoardView({
  board,
  cards,
  agents,
  focusId,
  run,
}: {
  board: Board;
  cards?: Record<string, Card>;
  agents: Agent[];
  focusId: string | null;
  run: (c: Command) => void;
}) {
  const rings = useMoveRings(board, cards);
  const [over, setOver] = useState<ColumnKey | null>(null);

  const empty = board.columns.every((c) => c.count === 0);
  if (empty) {
    return (
      <p className="empty">
        No deals yet. Your agent can add one: <code>{addDealCmd("Northwind renewal", asHandle("northwind"))}</code>
      </p>
    );
  }

  return (
    <div className="board">
      {board.columns.map((column) => (
        <section
          key={column.key}
          className={`column${over === column.key ? " column-over" : ""}`}
          onDragOver={(e) => {
            e.preventDefault();
            setOver(column.key);
          }}
          onDragLeave={() => setOver((k) => (k === column.key ? null : k))}
          onDrop={(e) => {
            e.preventDefault();
            setOver(null);
            // Our own `setData` put an id here a moment ago; the DataTransfer API hands
            // it back as a bare string, so re-brand it rather than widen the envelope.
            const id = asId(e.dataTransfer.getData("text/plain"));
            // A drop back where it started is not a move: sending it would write a
            // snapshot, push it to the agent, and ring a card that did not go anywhere.
            if (id && !column.dealIds.includes(id)) run({ cmd: "move", id, to: column.key });
          }}
        >
          <header className="column-head">
            <h2>{column.label}</h2>
            <span className="column-count num">{column.count}</span>
            {/* Grouped by currency, one line each, and **never summed across them**: we
                hold no rate source, and one wrong number is worse than two right ones. */}
            <div className="column-totals">
              {column.totals.length === 0 ? (
                <span className="column-total num">—</span>
              ) : (
                column.totals.map((t) => (
                  <span className="column-total num" key={t.currency}>
                    {money(t)}
                  </span>
                ))
              )}
            </div>
          </header>

          <div className="column-cards">
            {column.dealIds.length === 0 ? (
              <p className="column-empty">Nothing here.</p>
            ) : (
              column.dealIds.map((id) => (
                <DealCard
                  key={id}
                  id={id}
                  card={cards?.[id]}
                  agents={agents}
                  ring={rings.get(id)}
                  focused={focusId === id}
                  run={run}
                />
              ))
            )}
          </div>
        </section>
      ))}
    </div>
  );
}

/**
 * A deal is a card because a deal *is* a card — one of the few things in this window that
 * earns the shape.
 *
 * **Three fields and no more**: title, company, value. The fourth slot is the attribution
 * disc, which is not a field but the answer to "who moved this".
 */
function DealCard({
  id,
  card,
  agents,
  ring,
  focused,
  run,
}: {
  id: string;
  card?: Card;
  agents: Agent[];
  ring?: string;
  focused: boolean;
  run: (c: Command) => void;
}) {
  // The frozen snapshot carries `dealIds` but no deal bodies — see the note in bridge.ts.
  // Until that is settled the card degrades to its id rather than disappearing, so the
  // board still shows the right number of deals in the right columns.
  if (!card) {
    return (
      <article className="card card-bodyless" title="The core has not sent this deal's fields">
        <span className="card-title mono">{id}</span>
      </article>
    );
  }

  return (
    <article
      className={`card${focused ? " card-focused" : ""}${ring ? " card-ringed" : ""}`}
      style={ring ? ({ "--ring-tint": ring } as React.CSSProperties) : undefined}
      draggable
      onDragStart={(e) => e.dataTransfer.setData("text/plain", id)}
      tabIndex={0}
      role="button"
      onClick={() => run({ cmd: "show", kind: "deal", id })}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          run({ cmd: "show", kind: "deal", id });
        }
      }}
    >
      <div className="card-top">
        <span className="card-title">{card.label}</span>
        <Disc by={card.by} agents={agents} />
      </div>
      <span className="card-company">{card.detail ?? "—"}</span>
      {card.value ? <span className="card-value num">{money(card.value)}</span> : null}
    </article>
  );
}
