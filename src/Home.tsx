// Home: how the pipeline stands and what needs the person today.
//
// **p1 gave it what M12 found missing**, each from the core and none computed here:
//   · the open pipeline's value per currency — `derived.pipeline`, core-formatted, grouped by
//     currency, never summed across them (gap 3). This window still formats no money;
//   · the next steps themselves, across every record, soonest due first — the `tasks` envelope
//     (gap 1). `due` was only ever three counts, and a task list rode `focused` alone.
// Still absent, and still not faked: "closing soon" — a deal has no expected close date.
//
// The bars are neutral ink: colour means an agent did this, and a stage count is not one.

import { idKey, STAGES, type Command, type Money, type Snapshot, type TaskItem } from "./bridge";
import { useRead } from "./useRead";
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
  const totals = state.pipelineTotals;
  // Next steps across records. Re-asked whenever the data moves (the watermark), never stored.
  const steps = useRead<{ open: TaskItem[]; total: number }>({ cmd: "tasks", limit: 8 }, "tasks", state.watermark);
  // What is due now or soon. If the core's list has none to show, the three counts still say
  // what `due` says rather than leave the panel emptier than the header above it.
  const shown = (steps.data?.open ?? []).filter((t) => t.bucket !== "later");
  const view = reminders ? reminderView(reminders, state.agents.length > 0) : null;
  const anything = due.overdue + due.today + due.week > 0 || pending || view?.status || view?.backlog;

  return (
    <div className="page home">
      <div className="tiles">
        <Tile label="Open deals" value={open}>
          <Totals list={totals?.open ?? []} fallback={`in ${stages.length} stages`} />
        </Tile>
        <Tile label="Won" value={won?.count ?? 0}>
          <Totals list={totals?.won ?? won?.totals ?? []} />
        </Tile>
        <Tile label="Lost" value={lost?.count ?? 0}>
          <Totals list={totals?.lost ?? lost?.totals ?? []} />
        </Tile>
        <Tile label="Companies" value={state.counts.companies} />
        <Tile label="People" value={state.counts.contacts} />
      </div>

      <div className="page-grid">
        <section className="panel" aria-label="Needs you today">
          <h2 className="panel-title">Needs you today</h2>
          {anything ? (
            <ul className="panel-list">
              {shown.length > 0
                ? steps.data!.open
                    .filter((t) => t.bucket !== "later")
                    .map((t) => (
                      <li key={idKey(t.id)}>
                        <button
                          type="button"
                          className="need need-button"
                          disabled={!t.record}
                          onClick={() => t.record && run({ cmd: "show", kind: t.record.kind, id: idKey(t.record.id) })}
                        >
                          <span className={`need-tag micro${t.bucket === "overdue" ? " need-due" : ""}`}>{BUCKET[t.bucket]}</span>
                          <span className="need-text">
                            {t.what}
                            {t.record ? <span className="need-on"> · {t.record.label}</span> : null}
                          </span>
                          <span className="need-when num">{t.due}</span>
                        </button>
                      </li>
                    ))
                : (
                    <>
                      {due.overdue > 0 ? <Line tag="Overdue" tone="due">{due.overdue} next step{due.overdue === 1 ? "" : "s"} overdue</Line> : null}
                      {due.today > 0 ? <Line tag="Today">{due.today} next step{due.today === 1 ? "" : "s"} due today</Line> : null}
                      {due.week > 0 ? <Line tag="This week">{due.week} more due this week</Line> : null}
                    </>
                  )}
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
          {steps.data && shown.length > 0 && steps.data.total > shown.length ? (
            <p className="panel-foot">
              {steps.data.total} open in all — the soonest {shown.length} are here.
            </p>
          ) : shown.length > 0 ? null : (
            <p className="panel-foot">Open a deal to see its next steps.</p>
          )}
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

const BUCKET: Record<TaskItem["bucket"], string> = { overdue: "Overdue", today: "Today", week: "This week", later: "Later", done: "Done" };

/** The core's own formatted strings, one line per currency — summed by nobody. */
function Totals({ list, fallback }: { list: Money[]; fallback?: string }) {
  if (list.length === 0) return fallback ? <span className="tile-sub">{fallback}</span> : null;
  return (
    <>
      {list.map((m) => (
        <span className="tile-sub num" key={m.currency}>
          {m.formatted}
        </span>
      ))}
    </>
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
