import { idKey, type Agent, type Focused, type Handle } from "./bridge";
import { addCompanyCmd, logCmd, showCmd, taskCmd } from "./commands";
import { byLine, Disc } from "./Attribution";
import { CheckIcon, ClockIcon } from "./icons";

/**
 * A due date for the example `crm task` line: a week from today, in the person's own
 * timezone, as `YYYY-MM-DD`.
 *
 * Computed rather than written down, because a literal date is an instruction that quietly
 * becomes "set a next step in the past" the day after it was typed. Local, not UTC — the
 * grammar reads dates in the person's timezone, so an example built from UTC's calendar
 * could name yesterday.
 */
function aWeekFromToday(): string {
  const d = new Date();
  d.setDate(d.getDate() + 7);
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${mm}-${dd}`;
}

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
 * `focus` is **shared** view state: the agent's `crm show <handle>` opens that record right
 * here, in the person's window. That is not a side effect of the verb — it is the point of
 * it, and it is why this panel is driven by the snapshot rather than by a click handler.
 */
export function RecordPanel({
  focused,
  agents,
  example,
}: {
  focused: Focused | null | undefined;
  agents: Agent[];
  /** A handle that really exists, for the "nothing open" line — or null if there are no
   *  records at all. See `NothingOpen`. */
  example: Handle | null;
}) {
  if (!focused) return <NothingOpen example={example} />;

  const { row, fields, timeline, timelineTotal, tasks } = focused;

  return (
    <aside className="record" aria-label="Open record">
      <header className="record-head">
        <span className="micro">{row.kind}</span>
        <h2 className="record-title">{row.label}</h2>
        {row.value ? <p className="record-value num">{row.value.formatted}</p> : null}
      </header>

      {/* The core's field list, verbatim: `crm show` prints the same labels and values in the
          same order, from the same function. Composing a second description here would be
          two surfaces describing one record two ways. */}
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
            None. <code>{taskCmd(row.handle, "call back", aWeekFromToday())}</code>
          </p>
        ) : (
          <ul className="tasks">
            {/* Open first, as the core orders them. A done task stays visible, struck through:
                "this was handled" is part of the record, not something to hide. */}
            {tasks.map((t) => (
              <li className={`task${t.doneAt === null ? "" : " task-done"}`} key={idKey(t.id)}>
                {t.doneAt === null ? <ClockIcon size={14} /> : <CheckIcon size={14} />}
                <span className="task-what">{t.what}</span>
                <span className="task-due num">{t.due}</span>
                <Disc by={t.by} agents={agents} size={14} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="record-section">
        <h3 className="micro">
          Timeline
          {/* The core sends the 50 newest. Say so, rather than let a long history look like
              it starts wherever the slice happens to end. */}
          {timelineTotal > timeline.length ? (
            <span className="section-count num">
              {" "}
              · newest {timeline.length} of {timelineTotal}
            </span>
          ) : null}
        </h3>
        {timeline.length === 0 ? (
          <p className="empty-line">
            Nothing logged. <code>{logCmd("note", row.handle, "…")}</code>
          </p>
        ) : (
          <ol className="timeline">
            {/* Every entry says who wrote it. A shared log where you cannot tell who said
                what is a log two parties stop trusting. */}
            {timeline.map((a) => (
              <li className="entry" key={idKey(a.id)}>
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

/**
 * The panel when nothing is open.
 *
 * **A printed command that names a record names one that exists** (`m2-cli.md`). Round 2
 * printed `crm show acme` whatever the data held, which is an instruction that fails when
 * followed by anyone without an Acme. So: a real handle from the person's own records if
 * there is one, and if there are no records at all, the instruction that creates one —
 * an empty state may only create.
 */
function NothingOpen({ example }: { example: Handle | null }) {
  return (
    <aside className="record">
      <p className="empty">
        {example ? (
          <>
            Nothing open. Your agent can open a record here: <code>{showCmd(example)}</code>
          </>
        ) : (
          <>
            No records yet. Your agent can start with one: <code>{addCompanyCmd("Acme Corp")}</code>
          </>
        )}
      </p>
    </aside>
  );
}
