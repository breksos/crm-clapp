import { money, type Agent, type Focused } from "./bridge";
import { byLine, Disc } from "./Attribution";
import { ClockIcon } from "./icons";

/** A timestamp the way somebody scanning a log reads one: the distance from now, because
 *  "2 hours ago" is what a person actually wants from an activity feed, with the real date
 *  on hover for when it isn't. */
function ago(at: number): string {
  const seconds = Math.max(0, Math.round((Date.now() - at) / 1000));
  if (seconds < 60) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(at).toISOString().slice(0, 10);
}

/**
 * The record `focus` points at.
 *
 * `focus` is **shared** view state: the agent's `crm show acme` opens that record right
 * here, in the person's window. That is not a side effect of the verb — it is the point of
 * it, and it is why this panel is driven by the snapshot rather than by a click handler.
 */
export function RecordPanel({ focused, agents }: { focused: Focused | null | undefined; agents: Agent[] }) {
  if (!focused) {
    return (
      <aside className="record">
        <p className="empty">
          Nothing open. Your agent can open a record here: <code>crm show acme</code>
        </p>
      </aside>
    );
  }

  const { row, fields, timeline, tasks } = focused;

  return (
    <aside className="record" aria-label="Open record">
      <header className="record-head">
        <span className="micro">{row.kind}</span>
        <h2 className="record-title">{row.label}</h2>
        {row.value ? <p className="record-value num">{money(row.value)}</p> : null}
      </header>

      <dl className="fields">
        {fields.map((f) => (
          <div className="field" key={f.label}>
            <dt className="micro">{f.label}</dt>
            <dd>{f.value}</dd>
          </div>
        ))}
      </dl>

      <section className="record-section">
        <h3 className="micro">Next steps</h3>
        {tasks.length === 0 ? (
          <p className="empty-line">
            None. <code>crm task {row.id} "call back" --due 2026-09-30</code>
          </p>
        ) : (
          <ul className="tasks">
            {tasks.map((t) => (
              <li className="task" key={t.id}>
                <ClockIcon size={14} />
                <span className="task-what">{t.what}</span>
                <span className="task-due num">{t.due}</span>
                <Disc by={t.by} agents={agents} size={14} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="record-section">
        <h3 className="micro">Timeline</h3>
        {timeline.length === 0 ? (
          <p className="empty-line">
            Nothing logged. <code>crm log note {row.id} "…"</code>
          </p>
        ) : (
          <ol className="timeline">
            {/* Every entry says who wrote it. A shared log where you cannot tell who said
                what is a log two parties stop trusting. */}
            {timeline.map((a) => (
              <li className="entry" key={a.id}>
                <Disc by={a.by} agents={agents} size={18} />
                <div className="entry-body">
                  <p className="entry-meta">
                    <span className="entry-who">{byLine(a.by, agents)}</span>
                    <span className="entry-kind micro">{a.kind}</span>
                    <time className="entry-when num" dateTime={new Date(a.at).toISOString()} title={new Date(a.at).toLocaleString()}>
                      {ago(a.at)}
                    </time>
                  </p>
                  <p className="entry-text">{a.body}</p>
                </div>
              </li>
            ))}
          </ol>
        )}
      </section>
    </aside>
  );
}
