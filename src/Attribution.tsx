// Attribution: who wrote the line, who moved the card.
//
// This is the one thing this window has that no other CRM does, so it is drawn everywhere
// something was written — on every board card, beside every timeline entry, on every task —
// and it is never a hover-only detail.
//
// **Everything here is keyed on the agent's id.** The id is immutable for the agent's whole
// lifetime; the name is a re-pointable label. A rename arrives as a fresh roster with the
// same id, so keying on the id relabels the chip in place. Keying on the name would unmount
// and remount it — the row would flicker, the avatar would reload, and any in-flight
// interaction would be lost, all because somebody renamed their agent.

import { agentTint, useAsset, type Actor, type Agent } from "./bridge";
import { GRANT_CMD } from "./commands";

/** Name lookup for display only. Never used as a key. */
export function agentName(agents: Agent[], id: string): string {
  return agents.find((a) => a.id === id)?.name ?? "an agent";
}

function agentOf(agents: Agent[], id: string): Agent | undefined {
  return agents.find((a) => a.id === id);
}

/**
 * The disc: a coloured, pictured avatar for an agent, and a plain one for the person.
 *
 * The asymmetry is the point. Scanning a board, an agent's writes should be the ones that
 * catch the eye — the person already knows what they did themselves.
 */
export function Disc({
  by,
  agents,
  size = 16,
}: {
  by: Actor;
  agents: Agent[];
  size?: number;
}) {
  const agent = by.kind === "agent" ? agentOf(agents, by.id) : undefined;
  // Hooks cannot be conditional, so the human branch asks for the empty path and the
  // bridge answers null without a round trip.
  const src = useAsset(agent?.avatar);
  const style = { width: size, height: size, fontSize: Math.round(size * 0.56) };

  if (by.kind === "human") {
    return (
      <span className="disc disc-human" style={style} title="You" aria-label="You" role="img">
        Y
      </span>
    );
  }

  const name = agentName(agents, by.id);
  if (src) {
    return <img className="disc" style={style} src={src} alt={name} title={name} />;
  }
  // The monogram behind a missing avatar, tinted from the immutable id by clappkit's own
  // djb2 — the same colour this agent gets in every other app in the family.
  return (
    <span
      className="disc disc-mono"
      style={{ ...style, background: agentTint(by.id) }}
      title={name}
      aria-label={name}
      role="img"
    >
      {name.slice(0, 1).toUpperCase()}
    </span>
  );
}

/** "Nia" / "You" — the words a timeline line is prefixed with. */
export function byLine(by: Actor, agents: Agent[]): string {
  return by.kind === "human" ? "You" : agentName(agents, by.id);
}

/**
 * The header strip: every agent bound to this app right now.
 *
 * There is deliberately nothing to click here. The person reaches their agent through
 * Clatch; a control in this strip that prompted on their behalf would invert the model
 * (`docs/architecture.md` §11).
 */
export function AgentStrip({ agents }: { agents: Agent[] }) {
  if (agents.length === 0) {
    return (
      <p className="strip-empty">
        No agent bound — <code>{GRANT_CMD}</code>
      </p>
    );
  }
  return (
    <ul className="strip" aria-label="Agents bound to this app">
      {agents.map((a) => (
        <li className="strip-agent" key={a.id}>
          <Disc by={{ kind: "agent", id: a.id }} agents={agents} size={20} />
          <span className="strip-name">{a.name}</span>
        </li>
      ))}
    </ul>
  );
}
