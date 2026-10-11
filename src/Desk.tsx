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
//   · Reminders — `snapshot.reminders`: when the timer last ran, what it last sent, what is
//     waiting, the upgrade backlog, and a refusal. **Sent is not delivered**, and the desk says
//     so, and beside the sent-and-still-open count is the control `crm status` promises: send it
//     again (`UnconfirmedSends`).
//   · Agents — each bound agent and the last deal it moved. Not "what it is doing now": the
//     core has no live status, only what was written.
//   · Recent moves — the latest move per deal, newest first. A deal's *previous* moves are not
//     in the snapshot, so this is not a full history.
//
// It is one shared timeline's neighbour, not a replacement for it (M11): the record's timeline
// still shows people and agents together. Nothing here is an "ask the agent" control.

import { useEffect, useRef, useState } from "react";
import { cardOf, idKey, type Agent, type AgentCall, type Command, type Id, type Snapshot } from "./bridge";
import { tintVar } from "./tints";
import { humanName, teamAgentName } from "./identity";
import { useIdentity } from "./IdentityContext";
import { ErrorLine } from "./Forms";
import { useWrite } from "./useWrite";
import { Disc } from "./Attribution";
import { dueCmd, GRANT_CMD } from "./commands";
import { SendIcon } from "./icons";
import { reminderView } from "./reminders";
import { ago, agoShort } from "./time";

export type AgentMove = {
  agent: Agent;
  /** The deal, so a page can open it. Opaque: never rendered. */
  id: Id;
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
      moves.push({ agent, id, label: card.label, to: column.label, at: card.movedAt });
    }
  }
  return moves.sort((a, b) => b.at - a.at);
}

export function Desk({ state, apply, run }: { state: Snapshot; apply: (next: Snapshot) => void; run: (c: Command) => void }) {
  const moves = agentMoves(state);
  const lastBy = (agent: Agent) => moves.find((m) => m.agent.id === agent.id);

  return (
    <section className="desk" aria-label="Agent desk">
      <h2 className="desk-title micro">Agent desk</h2>

      <AgentActivity state={state} apply={apply} run={run} />

      <div className="desk-block">
        <h3 className="desk-head micro">Waiting on you</h3>
        {state.pending ? (
          <p className="desk-waiting">{state.pending.prompt}</p>
        ) : (
          <p className="desk-quiet">Nothing is waiting on you.</p>
        )}
      </div>

      <ReminderPanel state={state} apply={apply} />

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

/**
 * The timer, as the person needs it: has it run, did it send, is something stuck.
 *
 * Renders nothing when the core reported no `reminders` — see `Snapshot.reminders`.
 */
function ReminderPanel({ state, apply }: { state: Snapshot; apply: (next: Snapshot) => void }) {
  const r = state.reminders;
  if (!r) return null;
  const view = reminderView(r, state.agents.length > 0);
  const refusal = r.refusal;
  const who = refusal ? state.agents.find((a) => a.id === refusal.agent) : undefined;

  return (
    <div className="desk-block" aria-label="Reminders">
      <h3 className="desk-head micro">Reminders</h3>
      <p className="desk-fact">{view.checked}</p>
      <p className="desk-fact">{view.sent}</p>
      {view.status ? (
        <p className={view.status.kind === "refused" ? "desk-alert desk-refused" : "desk-alert"} role={view.status.kind === "refused" ? "alert" : undefined}>
          {who ? <Disc by={{ kind: "agent", id: who.id }} agents={state.agents} size={16} /> : null}
          <span>{view.status.text}</span>
        </p>
      ) : null}
      {view.backlog ? (
        <p className="desk-fact">
          {view.backlog} <code>{dueCmd()}</code> lists them.
        </p>
      ) : null}
      {view.unconfirmed ? <UnconfirmedSends view={view.unconfirmed} apply={apply} /> : null}
    </div>
  );
}

/**
 * Sent and still open, and the control to send it again.
 *
 * The platform never confirms delivery, so a muted agent or a full inbox looks exactly like
 * success from here — the app marks a reminder sent the moment it emits it, and never sends it
 * twice on its own. The person is the one who knows whether their agent acted, so the person
 * decides: this is `{ cmd: "resend" }`, the person's alone (the core refuses it from an agent).
 * It is the control `crm status` points at ("the person can send it again from the window").
 *
 * Sent through `useWrite` like every other control, so a refusal is the core's own sentence —
 * "no agent is connected to send it to", "nothing to send again" — not ours. It is sent with no
 * `id`, which is the whole last batch still open: the snapshot carries a *count* of those, not
 * a list, so a per-step "send this one" has nothing to hang on (docs/m12-snapshot-gaps.md #1).
 */
function UnconfirmedSends({
  view,
  apply,
}: {
  view: { text: string; note: string; count: number };
  apply: (next: Snapshot) => void;
}) {
  const { send, busy, error, refused, dismiss } = useWrite(apply);
  return (
    <div className="desk-resend">
      <p className="desk-fact">{view.text}</p>
      <p className="desk-fact">{view.note}</p>
      <button type="button" className="composer-open" disabled={busy} onClick={() => void send({ cmd: "resend" })}>
        <SendIcon size={14} />
        Send {view.count === 1 ? "it" : "them"} again
      </button>
      {error ? <ErrorLine error={error} refused={refused} onDismiss={dismiss} /> : null}
    </div>
  );
}

// MARK: - W1: every step an agent takes, as it takes it

const OUTCOME_WORD: Record<AgentCall["outcome"], string> = { ok: "ran", refused: "refused", asked: "asked you" };

/** One call as a line the way a person reads a log: who, what they typed, how it came out. */
export function CallLine({
  call, agents, now, fresh, onOpen,
}: { call: AgentCall; agents: Agent[]; now: number; fresh: boolean; onOpen?: (c: AgentCall) => void }) {
  const agent = call.actor.kind === "agent" ? agents.find((a) => a.id === (call.actor as { id: string }).id) : undefined;
  const identity = useIdentity();
  const name = call.actor.kind === "human" ? humanName(call.actor, identity) : agent?.name ?? teamAgentName(call.actor.id, identity) ?? "An agent";
  const style = call.actor.kind === "agent" ? ({ "--fresh-tint": tintVar(call.actor.id) } as React.CSSProperties) : undefined;
  const body = (
    <>
      <span className="call-head">
        <Disc by={call.actor} agents={agents} size={16} />
        <span className="call-name" style={call.actor.kind === "agent" ? { color: tintVar(call.actor.id) } : undefined}>
          {name}
        </span>
        <time className="call-when num" dateTime={new Date(call.at).toISOString()} title={new Date(call.at).toLocaleString()}>
          {agoShort(call.at, now)}
        </time>
      </span>
      <code className="call-line">{call.line}</code>
      {call.outcome === "refused" ? (
        <span className="call-refused" role="alert">
          {call.message ?? "refused"}
        </span>
      ) : call.outcome === "asked" ? (
        <span className="call-asked">asked you a question</span>
      ) : null}
    </>
  );
  const cls = `call${fresh ? " call-fresh" : ""}`;
  return call.target && onOpen ? (
    <button type="button" className={cls} style={style} data-outcome={call.outcome} aria-label={`${name} ${OUTCOME_WORD[call.outcome]}: ${call.line}. Open ${call.target.handle}`} onClick={() => onOpen(call)}>
      {body}
    </button>
  ) : (
    <div className={cls} style={style} data-outcome={call.outcome}>
      {body}
    </div>
  );
}

/**
 * The agent activity strip: one line per call that reached the app, newest first — reads and
 * refusals included. **A refusal is as visible as a success.** Clicking a line opens what the call
 * was about. The feed is not persisted, and the footer says so.
 *
 * A line that arrives while you are looking is emphasised for about two seconds (and only
 * emphasised — `prefers-reduced-motion` turns the fade into a still highlight).
 */
function AgentActivity({ state, apply, run }: { state: Snapshot; apply: (next: Snapshot) => void; run: (c: Command) => void }) {
  const feed = state.agentFeed ?? [];
  const [now, setNow] = useState(() => Date.now());
  const seen = useRef<Set<number> | null>(null);
  const [fresh, setFresh] = useState<Set<number>>(new Set());

  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    if (seen.current === null) {
      seen.current = new Set(feed.map((c) => c.seq)); // what was already there is not news
      return;
    }
    const arrived = feed.filter((c) => !seen.current!.has(c.seq)).map((c) => c.seq);
    if (arrived.length === 0) return;
    for (const s of arrived) seen.current.add(s);
    setFresh((f) => new Set([...f, ...arrived]));
    const t = window.setTimeout(() => setFresh((f) => new Set([...f].filter((s) => !arrived.includes(s)))), 2000);
    return () => window.clearTimeout(t);
  }, [feed]);

  return (
    <div className="desk-block" aria-label="Agent activity">
      <div className="activity-head">
        <h3 className="desk-head micro">Agent activity</h3>
        <FollowToggle follow={state.follow ?? true} apply={apply} />
      </div>
      {feed.length === 0 ? (
        <p className="desk-quiet">Nothing yet. What your agent runs shows here, step by step.</p>
      ) : (
        <ol className="activity" aria-live="polite">
          {feed.map((c) => (
            <li key={c.seq}>
              <CallLine
                call={c}
                agents={state.agents}
                now={now}
                fresh={fresh.has(c.seq)}
                onOpen={(call) => call.target && run({ cmd: "show", kind: call.target.kind, id: idKey(call.target.id) })}
              />
            </li>
          ))}
        </ol>
      )}
      <p className="desk-fact">Since the app started.</p>
    </div>
  );
}

/** Follow agent: the person's choice about their own window, bound to the core's `follow`. It is
 *  not an agent's to flip — the core refuses it from one — and it has no CLI verb. */
function FollowToggle({ follow, apply }: { follow: boolean; apply: (next: Snapshot) => void }) {
  const { send, busy, error, refused, dismiss } = useWrite(apply);
  return (
    <>
      <label className="follow">
        <input type="checkbox" checked={follow} disabled={busy} onChange={(e) => void send({ cmd: "follow", on: e.target.checked })} />
        <span>Follow agent</span>
      </label>
      {error ? <ErrorLine error={error} refused={refused} onDismiss={dismiss} /> : null}
    </>
  );
}
