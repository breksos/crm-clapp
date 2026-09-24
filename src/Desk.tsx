// The agent desk: oversight, on every page.
//
// **Built from what the snapshot actually carries, and nothing else.** The snapshot has the
// roster of bound agents, `Pending` (a question an agent asked that the person has not yet
// answered), and — on every board card — `by` and `movedAt`: who last moved that deal, and
// when. So that is what this panel shows, labelled for what it is:
//
//   · Waiting on you — `pending`, the one thing an agent can genuinely be waiting on. There is
//     no approval queue in the core, so there is no approval UI here; drawing one would be
//     inventing a workflow the product does not have.
//   · Agents — each bound agent and the last deal it moved. Not "what it is doing now": the
//     core has no live status, only what was written.
//   · Recent moves — the latest move per deal, newest first. A deal's *previous* moves are not
//     in the snapshot, so this is not a full history.
//
// It is one shared timeline's neighbour, not a replacement for it (M11): the record's timeline
// still shows people and agents together. Nothing here is an "ask the agent" control.

import { cardOf, type Agent, type Snapshot } from "./bridge";
import { Disc } from "./Attribution";
import { GRANT_CMD } from "./commands";
import { ago } from "./time";

export type AgentMove = {
  agent: Agent;
  /** The deal's title — a label, never an id. */
  label: string;
  /** Where it sits now: the column's own label ("Negotiation", "Won"). */
  to: string;
  at: number;
};

/** Every deal an agent last moved, newest first. Deals moved by the person are not agent moves. */
export function agentMoves(state: Pick<Snapshot, "board" | "cards" | "agents">): AgentMove[] {
  const moves: AgentMove[] = [];
  for (const column of state.board.columns) {
    for (const id of column.dealIds) {
      const card = cardOf(state as Snapshot, id);
      if (!card || card.by.kind !== "agent") continue;
      const by = card.by;
      const agent = state.agents.find((a) => a.id === by.id);
      // A move by an agent no longer bound has no name to show; the ring and stripe still
      // mark the card, but a desk line naming "an agent" would say nothing.
      if (!agent) continue;
      moves.push({ agent, label: card.label, to: column.label, at: card.movedAt });
    }
  }
  return moves.sort((a, b) => b.at - a.at);
}

export function Desk({ state }: { state: Snapshot }) {
  const moves = agentMoves(state);
  const lastBy = (agent: Agent) => moves.find((m) => m.agent.id === agent.id);

  return (
    <section className="desk" aria-label="Agent desk">
      <h2 className="desk-title micro">Agent desk</h2>

      <div className="desk-block">
        <h3 className="desk-head micro">Waiting on you</h3>
        {state.pending ? (
          <p className="desk-waiting">{state.pending.prompt}</p>
        ) : (
          <p className="desk-quiet">Nothing is waiting on you.</p>
        )}
      </div>

      <div className="desk-block">
        <h3 className="desk-head micro">Agents</h3>
        {state.agents.length === 0 ? (
          <p className="desk-quiet">
            No agent bound. <code>{GRANT_CMD}</code>
          </p>
        ) : (
          <ul className="desk-list">
            {state.agents.map((a) => {
              const last = lastBy(a);
              return (
                <li className="desk-agent" key={a.id}>
                  <Disc by={{ kind: "agent", id: a.id }} agents={state.agents} size={20} />
                  <div className="desk-body">
                    <span className="desk-name">{a.name}</span>
                    {last ? (
                      // The label may be long and is allowed to give way; the time never is.
                      <span className="desk-sub desk-sub-split">
                        <span className="desk-cut">moved {last.label}</span>
                        <time className="desk-when num" dateTime={new Date(last.at).toISOString()}>
                          {ago(last.at)}
                        </time>
                      </span>
                    ) : (
                      <span className="desk-sub">no moves yet</span>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <div className="desk-block">
        <h3 className="desk-head micro">Recent moves</h3>
        {moves.length === 0 ? (
          <p className="desk-quiet">No agent has moved a deal yet.</p>
        ) : (
          <ol className="desk-list">
            {moves.slice(0, 6).map((m, i) => (
              <li className="desk-move" key={`${m.agent.id}-${m.at}-${i}`}>
                <Disc by={{ kind: "agent", id: m.agent.id }} agents={state.agents} size={16} />
                <div className="desk-body">
                  <span className="desk-line">
                    <b>{m.agent.name}</b> moved {m.label} to {m.to}
                  </span>
                  <time className="desk-sub num" dateTime={new Date(m.at).toISOString()}>
                    {ago(m.at)}
                  </time>
                </div>
              </li>
            ))}
          </ol>
        )}
      </div>
    </section>
  );
}
