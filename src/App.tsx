// The window, M0: a shell that proves the wiring and says so.
//
// **M3 replaces all of this** with the board, the table, the record detail and the agent
// strip. What must survive the replacement is the shape: the window talks to the core
// through `bridge.ts` and nothing else, and it renders the snapshot rather than any
// state of its own.
//
// There is no "ask the agent" button here, and there never will be. The window is for
// the person's own actions; their agent reaches this app through Clatch.

import { useSnapshot, EMPTY, type Snapshot, type Command, type Agent } from "./bridge";
import { useAsset, agentTint } from "./bridge";

export default function App() {
  const { state } = useSnapshot<Snapshot, Command>(EMPTY);
  const connected = state.rev >= 0;

  return (
    <main className="app">
      <header className="head">
        <h1>Breksos CRM</h1>
        <p className="sub">
          A sales pipeline you and your agent share — whatever either of you opens, logs
          or moves, the other is looking at it too.
        </p>
      </header>

      <section className="panel">
        <h2>Records</h2>
        <dl className="counts">
          <Count label="Companies" n={state.counts.companies} />
          <Count label="Contacts" n={state.counts.contacts} />
          <Count label="Deals" n={state.counts.deals} />
          <Count label="Activities" n={state.counts.activities} />
          <Count label="Tasks" n={state.counts.tasks} />
        </dl>
      </section>

      <section className="panel">
        <h2>Agents</h2>
        {state.agents.length === 0 ? (
          <p className="quiet">
            No agent is bound to this app yet. Grant one with{" "}
            <code>clatch agent grant &lt;name&gt; app:com.breksos.crm</code>.
          </p>
        ) : (
          <ul className="agents">
            {state.agents.map((a) => (
              <AgentChip key={a.id} agent={a} />
            ))}
          </ul>
        )}
      </section>

      <footer className="foot">
        <span className={connected ? "dot live" : "dot"} />
        {connected ? `core connected · rev ${state.rev}` : "waiting for the core…"}
        <span className="milestone">M0 — scaffold. The board lands in M3.</span>
      </footer>
    </main>
  );
}

function Count({ label, n }: { label: string; n: number }) {
  return (
    <div className="count">
      <dt>{label}</dt>
      <dd>{n}</dd>
    </div>
  );
}

/** An agent's avatar is an absolute path on this machine; the webview cannot open one,
 *  so the core reads it and hands back a data: URI. A monogram tinted from the immutable
 *  id stands in until it arrives — and stays, for an agent that has no picture. */
function AgentChip({ agent }: { agent: Agent }) {
  const src = useAsset(agent.avatar);
  return (
    <li className="agent">
      {src ? (
        <img className="avatar" src={src} alt="" />
      ) : (
        <span className="avatar mono" style={{ background: agentTint(agent.id) }}>
          {agent.name.slice(0, 1).toUpperCase()}
        </span>
      )}
      <span className="who">
        <strong>{agent.name}</strong>
        {agent.backend ? <em>{agent.backend}</em> : null}
      </span>
    </li>
  );
}
