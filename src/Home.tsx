// Home: how the pipeline stands and what needs the person today.
//
// **Built from the snapshot as it stands, and no further.** Where a tile wanted a field the
// snapshot lacks, it is not drawn and not faked — it is on `docs/m12-snapshot-gaps.md`:
//
//   · an open-pipeline *total* per currency — the core sends per-column totals already
//     formatted, and this window formats no money, so it does not sum them;
//   · the next steps themselves — `due` is three counts, and a task list only rides `focused`;
//   · "closing soon" — a deal has no close date.
//
// The tiles are counts and the core's own formatted strings. Bars are neutral ink: colour means
// an agent did this, and a stage count is not one.

import { idKey, STAGES, type Command, type Snapshot } from "./bridge";
import { Disc } from "./Attribution";
import { dueCmd } from "./commands";
import { agentMoves } from "./Desk";
import { reminderView } from "./reminders";
import { ago } from "./time";
import type { View } from "./nav";

export function Home({ state, run, go }: { state: Snapshot; run: (c: Command) => void; go: (v: View) => void }) {
  const stages = state.board.columns.filter((c) => (STAGES as readonly string[]).includes(c.key));
  const won = state.board.columns.find((c) => c.key === "won");
  const lost = state.board.columns.find((c) => c.key === "lost");
  const open = stages.reduce((n, c) => n + c.count, 0);
  const most = Math.max(1, ...stages.map((c) => c.count));
  const moves = agentMoves(state).slice(0, 8);
  const { due, pending, reminders } = state;
  const view = reminders ? reminderView(reminders, state.agents.length > 0) : null;
  const anything = due.overdue + due.today + due.week > 0 || pending || view?.status || view?.backlog;

  return (
    <div className="page home">
      <div className="tiles">
        <Tile label="Open deals" value={open}>
          <span className="tile-sub">in {stages.length} stages</span>
        </Tile>
        <Tile label="Won" value={won?.count ?? 0}>
          {(won?.totals ?? []).map((t) => (
            <span className="tile-sub num" key={t.currency}>
              {t.formatted}
            </span>
          ))}
        </Tile>
        <Tile label="Lost" value={lost?.count ?? 0}>
          {(lost?.totals ?? []).map((t) => (
            <span className="tile-sub num" key={t.currency}>
              {t.formatted}
            </span>
          ))}
        </Tile>
        <Tile label="Companies" value={state.counts.companies} />
        <Tile label="People" value={state.counts.contacts} />
      </div>

      <div className="page-grid">
        <section className="panel" aria-label="Needs you today">
          <h2 className="panel-title">Needs you today</h2>
          {anything ? (
            <ul className="panel-list">
              {due.overdue > 0 ? <Line tag="Overdue" tone="due">{due.overdue} next step{due.overdue === 1 ? "" : "s"} overdue</Line> : null}
              {due.today > 0 ? <Line tag="Today">{due.today} next step{due.today === 1 ? "" : "s"} due today</Line> : null}
              {due.week > 0 ? <Line tag="This week">{due.week} more due this week</Line> : null}
              {pending ? <Line tag="Question">{pending.prompt}</Line> : null}
              {view?.status ? <Line tag="Reminders" tone={view.status.kind === "refused" ? "lost" : undefined}>{view.status.text}</Line> : null}
              {view?.backlog ? (
                <Line tag="Backlog">
                  {view.backlog} <code>{dueCmd()}</code> lists them.
                </Line>
              ) : null}
            </ul>
          ) : (
            <p className="panel-empty">Nothing needs you today.</p>
          )}
          <p className="panel-foot">Open a deal to see its next steps.</p>
        </section>

        <section className="panel" aria-label="Deals by stage">
          <h2 className="panel-title">Deals by stage</h2>
          <ul className="bars">
            {stages.map((c) => (
              <li className="bar-row" key={c.key}>
                <span className="bar-label">{c.label}</span>
                <span className="bar-track" aria-hidden="true">
                  <span className="bar-fill" style={{ width: `${(c.count / most) * 100}%` }} />
                </span>
                <span className="bar-n num">{c.count}</span>
              </li>
            ))}
          </ul>
          <p className="panel-foot">
            <button type="button" className="link-button" onClick={() => go("pipeline")}>
              Open the pipeline
            </button>
          </p>
        </section>

        <section className="panel panel-wide" aria-label="Recent agent moves">
          <h2 className="panel-title">Recent agent moves</h2>
          {moves.length === 0 ? (
            <p className="panel-empty">No agent has moved a deal yet.</p>
          ) : (
            <ol className="panel-list">
              {moves.map((m, i) => (
                <li key={`${idKey(m.id)}-${i}`}>
                  <button
                    type="button"
                    className="move-row"
                    onClick={() => run({ cmd: "show", kind: "deal", id: idKey(m.id) })}
                  >
                    <Disc by={{ kind: "agent", id: m.agent.id }} agents={state.agents} size={18} />
                    <span className="move-text">
                      <b>{m.agent.name}</b> moved {m.label} to {m.to}
                    </span>
                    <time className="move-when num" dateTime={new Date(m.at).toISOString()}>
                      {ago(m.at)}
                    </time>
                  </button>
                </li>
              ))}
            </ol>
          )}
        </section>
      </div>
    </div>
  );
}

function Tile({ label, value, children }: { label: string; value: number; children?: React.ReactNode }) {
  return (
    <div className="tile">
      <span className="micro tile-label">{label}</span>
      <span className="tile-value num">{value}</span>
      {children}
    </div>
  );
}

function Line({ tag, tone, children }: { tag: string; tone?: "due" | "lost"; children: React.ReactNode }) {
  return (
    <li className="need">
      <span className={`need-tag micro${tone ? ` need-${tone}` : ""}`}>{tag}</span>
      <span className="need-text">{children}</span>
    </li>
  );
}
