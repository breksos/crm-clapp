import { useEffect, useRef, useState } from "react";
import {
  COLUMN_KEYS, idKey,
  type ActivityKind, type Agent, type ColumnKey, type Command, type Focused, type Handle, type Id, type Row,
  type Snapshot,
} from "./bridge";
import {
  addCompanyCmd, archiveCmd, doneCmd, logCmd, restoreCmd, setCmd, showCmd, taskCmd,
} from "./commands";
import { byLine, Disc } from "./Attribution";
import { ErrorLine } from "./Forms";
import { ArchiveIcon, CheckIcon, ClockIcon, LinkIcon, PlusIcon, RestoreIcon, XIcon } from "./icons";
import { useWrite } from "./useWrite";

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

const cap = (word: string) => word[0].toUpperCase() + word.slice(1);

/**
 * The record `focus` points at.
 *
 * `focus` is **shared** view state: the agent's `crm show <handle>` opens that record right
 * here, in the person's window. That is not a side effect of the verb — it is the point of
 * it, and it is why this panel is driven by the snapshot rather than by a click handler.
 *
 * **M7 turns this from a viewer into the person's hands** (`architecture.md` §10b): every
 * field here is editable in place, the two sections that used to show a hint *instead of* a
 * control now show a composer *beside* one, and Move/Link/Archive live in the header. All of
 * it goes through `apply` — see `useWrite.ts` — never through `run`'s fire-and-forget path,
 * because a refusal here has to reach the control that asked, not vanish.
 */
export function RecordPanel({
  focused,
  agents,
  example,
  known,
  run,
  apply,
}: {
  focused: Focused | null | undefined;
  agents: Agent[];
  /** A handle that really exists, for the "nothing open" line — or null if there are no
   *  records at all. See `NothingOpen`. */
  example: Handle | null;
  /** Every record currently loaded anywhere in the window — the board's cards plus the
   *  shared list's current page — which is what the Link control can search. See its own
   *  note on why that is a real limit, not a bug. */
  known: Row[];
  run: (c: Command) => void;
  apply: (next: Snapshot) => void;
}) {
  const [linkOpen, setLinkOpen] = useState(false);

  if (!focused) return <NothingOpen example={example} />;

  const { row, fields, timeline, timelineTotal, tasks } = focused;

  return (
    <aside className="record" aria-label="Open record">
      <header className="record-head">
        <div className="record-head-top">
          <span className="micro">{row.kind}</span>
          <ArchiveControl row={row} apply={apply} />
        </div>
        <EditableTitle row={row} apply={apply} />
        {row.value ? <p className="record-value num">{row.value.formatted}</p> : null}
        {row.kind === "deal" ? <MoveSelect row={row} run={run} /> : null}
      </header>

      {/* The core's field list, verbatim: `crm show` prints the same labels and values in
          the same order, from the same function. Composing a second description here would
          be two surfaces describing one record two ways. Four labels stay read-only because
          a dedicated control owns them: Company/Contacts are Link's, Stage/Status are
          Move's — see `EditableField`. */}
      <dl className="fields">
        {fields.map((f) => {
          const key = f.label.toLowerCase();
          const editable = key !== "company" && key !== "contacts" && key !== "stage" && key !== "status";
          return (
            <div className="field" key={f.label}>
              <dt className="micro">{f.label}</dt>
              <EditableField id={row.id} handle={row.handle} label={f.label} value={f.value} editable={editable} apply={apply} />
            </div>
          );
        })}
      </dl>

      <section className="record-section">
        <h3 className="micro">Link</h3>
        {linkOpen ? (
          <LinkForm row={row} known={known} apply={apply} onDone={() => setLinkOpen(false)} />
        ) : (
          <button type="button" className="composer-open" onClick={() => setLinkOpen(true)}>
            <LinkIcon size={14} />
            Link…
          </button>
        )}
      </section>

      <section className="record-section">
        <h3 className="micro">Next steps</h3>
        <TaskComposer id={row.id} apply={apply} />
        {tasks.length === 0 ? (
          <p className="empty-line">
            Your agent: <code>{taskCmd(row.handle, "call back", aWeekFromToday())}</code>
          </p>
        ) : (
          <ul className="tasks">
            {/* Open first, as the core orders them. A done task stays visible, struck
                through: "this was handled" is part of the record, not something to hide. */}
            {tasks.map((t) => (
              <TaskRow key={idKey(t.id)} t={t} agents={agents} apply={apply} />
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
        <LogComposer id={row.id} apply={apply} />
        {timeline.length === 0 ? (
          <p className="empty-line">
            Your agent: <code>{logCmd("note", row.handle, "…")}</code>
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

// MARK: - The record's own name — `set`

/**
 * The title itself, click-to-edit like every other field, but it is not a `.fields` entry
 * — it is `row.label`, shown separately — so it gets its own small component rather than a
 * slot in `EditableField`'s map. Not a `<button>` wrapping an `<h2>`: a heading is not
 * phrasing content, so that nests invalidly. Made keyboard-reachable the same way
 * `Board.tsx`'s `DealCard` already is — `role="button"` and `tabIndex` on the element
 * itself, Enter/Space to activate.
 */
function EditableTitle({ row, apply }: { row: Row; apply: (next: Snapshot) => void }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(row.label);
  const { send, busy, error, dismiss } = useWrite(apply);
  // The core's own struct field: `Deal.title`, but `Company.name` / `Contact.name`.
  const field = row.kind === "deal" ? "title" : "name";

  async function commit() {
    const trimmed = draft.trim();
    if (trimmed && trimmed !== row.label) await send({ cmd: "set", id: idKey(row.id), field, value: trimmed });
    setEditing(false);
  }

  if (editing) {
    return (
      <input
        className="record-title record-title-input"
        value={draft}
        autoFocus
        disabled={busy}
        aria-label="Record title"
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && setEditing(false)}
        onBlur={commit}
      />
    );
  }

  return (
    <>
      <h2
        className="record-title"
        tabIndex={0}
        role="button"
        title={`${row.label}\n${setCmd(row.handle, field, row.label)}`}
        onClick={() => {
          setDraft(row.label);
          dismiss();
          setEditing(true);
        }}
        onKeyDown={(e) => {
          if (e.key !== "Enter" && e.key !== " ") return;
          e.preventDefault();
          setDraft(row.label);
          dismiss();
          setEditing(true);
        }}
      >
        {row.label}
      </h2>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </>
  );
}

// MARK: - A read-only value that may be long

/**
 * Company and Contacts are read-only here (Link owns them), and they are the fields a real
 * legal-entity name lands in. Clamped to two lines so one long name cannot push Value, Stage
 * and Status down the panel; when — and only when — the text really is cut, the value becomes
 * a disclosure button, so the full string is reachable by keyboard as well as by hover.
 */
function ClampedValue({ value }: { value: string }) {
  const ref = useRef<HTMLElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [cut, setCut] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (el && !expanded) setCut(el.scrollHeight > el.clientHeight + 1);
  }, [value, expanded, cut]);

  // The disclosure is the text itself, not a "Show all" line beneath it: a control that adds
  // a row would move Value/Stage/Status anyway, which is the reflow this exists to stop.
  if (cut || expanded) {
    return (
      <dd>
        <button
          type="button"
          ref={ref as React.RefObject<HTMLButtonElement>}
          className={expanded ? "clamp clamp-btn clamp-open" : "clamp clamp-btn"}
          aria-expanded={expanded}
          title={value}
          onClick={() => setExpanded(!expanded)}
        >
          {value}
        </button>
      </dd>
    );
  }
  return (
    <dd>
      <span ref={ref as React.RefObject<HTMLSpanElement>} className="clamp" title={value}>
        {value}
      </span>
    </dd>
  );
}

// MARK: - A generic field — `set`

/**
 * Click the value, type, commit — `m7-window-editing.md`'s own words for this control.
 * Committing sends `{ cmd: "set", id, field: <label, lower-cased>, value }`; a refusal
 * exits edit mode back to the value the core still holds (the `value` prop, unchanged
 * until a fresh snapshot says otherwise) and shows the core's sentence beside it. The
 * `title` teaches the CLI's own spelling of the same edit, without a permanent line under
 * every field in a panel this dense.
 */
function EditableField({
  id,
  handle,
  label,
  value,
  editable,
  apply,
}: {
  id: Id;
  handle: Handle;
  label: string;
  value: string;
  editable: boolean;
  apply: (next: Snapshot) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const { send, busy, error, dismiss } = useWrite(apply);

  if (!editable) return <ClampedValue value={value} />;

  async function commit() {
    const trimmed = draft.trim();
    if (trimmed && trimmed !== value) {
      await send({ cmd: "set", id: idKey(id), field: label.toLowerCase(), value: trimmed });
    }
    setEditing(false);
  }

  if (editing) {
    return (
      <dd>
        <input
          className="field-edit-input"
          value={draft}
          autoFocus
          disabled={busy}
          aria-label={label}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setEditing(false)}
          onBlur={commit}
        />
      </dd>
    );
  }

  return (
    <dd>
      <button
        type="button"
        className="field-value"
        title={`${value}\n${setCmd(handle, label, value)}`}
        onClick={() => {
          setDraft(value);
          dismiss();
          setEditing(true);
        }}
      >
        {value}
      </button>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </dd>
  );
}

// MARK: - Move — the keyboard route to the same envelope the drag already sends

/**
 * A `<select>` beside Stage/Status, for a keyboard user who cannot drag
 * (`m7-window-editing.md`: "already done — the drag. Keep it, and add a stage control on
 * the record panel for keyboard users"). Sent through `run`, not `apply` — the same
 * fire-and-forget path the drag already uses, because it is the same verb; there is no
 * second implementation to keep in step with here, only a second way to reach the first.
 */
function MoveSelect({ row, run }: { row: Row; run: (c: Command) => void }) {
  const current: ColumnKey = row.status === "won" || row.status === "lost" ? row.status : (row.stage ?? "lead");
  return (
    <label className="move-select">
      <span className="micro">Move to</span>
      <select value={current} onChange={(e) => run({ cmd: "move", id: idKey(row.id), to: e.target.value })}>
        {COLUMN_KEYS.map((key) => (
          <option key={key} value={key}>
            {cap(key)}
          </option>
        ))}
      </select>
    </label>
  );
}

// MARK: - Archive / restore

function ArchiveControl({ row, apply }: { row: Row; apply: (next: Snapshot) => void }) {
  const { send, busy, error, dismiss } = useWrite(apply);
  const restoring = row.archived;

  return (
    <div className="record-archive">
      {row.archived ? <span className="micro record-archived-tag">Archived</span> : null}
      <button
        type="button"
        className="icon-button"
        aria-label={restoring ? "Restore" : "Archive"}
        title={restoring ? restoreCmd(row.handle) : archiveCmd(row.handle)}
        disabled={busy}
        onClick={() => send({ cmd: "archive", id: idKey(row.id), restore: restoring })}
      >
        {restoring ? <RestoreIcon size={14} /> : <ArchiveIcon size={14} />}
      </button>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </div>
  );
}

// MARK: - Link

/**
 * "Picking from the existing records" (`m7-window-editing.md`), scoped to records the
 * window has already loaded — the board's cards and the shared list's current page — since
 * there is no envelope in `m2-cli.md` for a second, scoped search. Typing a handle nothing
 * here has loaded says so rather than pretending to resolve it: this window cannot turn a
 * handle into an id without the core, and guessing would be exactly the kind of domain
 * logic `m7-window-editing.md` asks the window not to own.
 */
function LinkForm({
  row,
  known,
  apply,
  onDone,
}: {
  row: Row;
  known: Row[];
  apply: (next: Snapshot) => void;
  onDone: () => void;
}) {
  const [handle, setHandle] = useState("");
  const { send, busy, error, dismiss } = useWrite(apply);
  const typed = handle.trim();
  const target = typed ? known.find((r) => r.handle === typed && r.id !== row.id) : undefined;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!target) return;
    const ok = await send({ cmd: "link", id: idKey(row.id), to: idKey(target.id) });
    if (ok) onDone();
  }

  return (
    <form className="composer composer-inline" onSubmit={submit}>
      <div className="composer-row">
        <input
          className="composer-input"
          placeholder="Handle to link"
          aria-label="Handle to link"
          value={handle}
          onChange={(e) => setHandle(e.target.value)}
          autoFocus
        />
        <button type="submit" className="composer-submit" disabled={busy || !target}>
          Link
        </button>
        <button type="button" className="icon-button" aria-label="Cancel" onClick={onDone}>
          <XIcon size={14} />
        </button>
      </div>
      <p className="empty-line composer-hint">
        {target
          ? `Link to ${target.label} (${target.handle}).`
          : typed
            ? "No record with that handle is loaded here yet — open People, Companies or the board to bring it into view."
            : "Only records already shown in this window can be linked here."}
      </p>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </form>
  );
}

// MARK: - Next steps

function TaskComposer({ id, apply }: { id: Id; apply: (next: Snapshot) => void }) {
  const [what, setWhat] = useState("");
  const [due, setDue] = useState("");
  const { send, busy, error, dismiss } = useWrite(apply);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!what.trim() || !due) return; // stop an empty required field; the core owns everything else
    const ok = await send({ cmd: "task", id: idKey(id), what, due });
    if (ok) {
      setWhat("");
      setDue("");
    }
  }

  return (
    <form className="composer composer-inline" onSubmit={submit}>
      <input className="composer-input" placeholder="Next step" aria-label="Next step" value={what} onChange={(e) => setWhat(e.target.value)} />
      <input
        className="composer-input composer-input-date"
        type="date"
        aria-label="Due date"
        value={due}
        onChange={(e) => setDue(e.target.value)}
      />
      <button type="submit" className="icon-button" aria-label="Add next step" disabled={busy || !what.trim() || !due}>
        <PlusIcon size={14} />
      </button>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </form>
  );
}

function TaskRow({ t, agents, apply }: { t: Focused["tasks"][number]; agents: Agent[]; apply: (next: Snapshot) => void }) {
  const { send, busy, error, dismiss } = useWrite(apply);
  const open = t.doneAt === null;

  return (
    <li className={`task-item${open ? "" : " task-done"}`}>
      <div className="task">
        {open ? (
          <button
            type="button"
            className="icon-button task-done-button"
            aria-label="Mark done"
            title={doneCmd(t.handle)}
            disabled={busy}
            onClick={() => send({ cmd: "done", id: idKey(t.id) })}
          >
            <ClockIcon size={14} />
          </button>
        ) : (
          <CheckIcon size={14} />
        )}
        <span className="task-what">{t.what}</span>
        <span className="task-due num">{t.due}</span>
        <Disc by={t.by} agents={agents} size={14} />
      </div>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </li>
  );
}

// MARK: - Timeline

const ACTIVITY_KINDS: [ActivityKind, string][] = [
  ["call", "Call"],
  ["email", "Email"],
  ["meeting", "Meeting"],
  ["note", "Note"],
];

function LogComposer({ id, apply }: { id: Id; apply: (next: Snapshot) => void }) {
  const [kind, setKind] = useState<ActivityKind>("note");
  const [body, setBody] = useState("");
  const { send, busy, error, dismiss } = useWrite(apply);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!body.trim()) return;
    const ok = await send({ cmd: "log", kind, id: idKey(id), body });
    if (ok) setBody("");
  }

  return (
    <form className="composer composer-inline" onSubmit={submit}>
      <div className="composer-row">
        <div className="sorts" role="group" aria-label="Kind">
          {ACTIVITY_KINDS.map(([k, label]) => (
            <button key={k} type="button" className="chip" aria-pressed={kind === k} onClick={() => setKind(k)}>
              {label}
            </button>
          ))}
        </div>
      </div>
      <div className="composer-row">
        <input className="composer-input" placeholder="What happened?" aria-label="Body" value={body} onChange={(e) => setBody(e.target.value)} />
        <button type="submit" className="icon-button" aria-label="Log" disabled={busy || !body.trim()}>
          <PlusIcon size={14} />
        </button>
      </div>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </form>
  );
}

// MARK: - Nothing open

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
