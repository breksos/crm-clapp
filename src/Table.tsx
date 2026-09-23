import { useState } from "react";
import { idKey, pageOf, pageWording, type Command, type Kind, type ListView, type Snapshot, type Sort } from "./bridge";
import { addCompanyCmd, addContactCmd, findCmd, importCmd } from "./commands";
import { ErrorLine } from "./Forms";
import { NextIcon, PlusIcon, PrevIcon, SearchIcon, XIcon } from "./icons";
import { useWrite } from "./useWrite";

/** The shared list's kind filter. `null` is "all", which `crm find --kind all` also sets. */
const KINDS: [Kind | null, string][] = [
  [null, "All"],
  ["contact", "People"],
  ["company", "Companies"],
  ["deal", "Deals"],
];

const SORTS: [Sort, string][] = [
  ["updated", "Recent"],
  ["name", "Name"],
  ["value", "Value"],
];

/**
 * The shared result list.
 *
 * **Query, sort, page and page size are all shared state**, so every control here writes
 * through the core rather than filtering a local copy. That is the whole reason the person
 * and their agent can talk about "the third one" and mean the same row. `-n` on the CLI
 * side limits what that terminal prints; it does not repaginate this table.
 *
 * Rows are 32px. Density is the feature — somebody wants their pipeline without scrolling,
 * and a 25-row page has to fit a 900px window.
 */
export function TableView({
  list,
  run,
  apply,
}: {
  list: ListView;
  run: (c: Command) => void;
  apply: (next: Snapshot) => void;
}) {
  // The New control lives on each *rail* view (`m7-window-editing.md`'s table), not on the
  // "Deals" or "All" chip within this shared table — deals are the board's, and a New
  // control that only sometimes appears in this bar would be its own small surprise.
  const [composerOpen, setComposerOpen] = useState(false);

  return (
    <div className="list">
      <div className="list-bar">
        <label className="search">
          <SearchIcon />
          <input
            type="search"
            value={list.query}
            placeholder="Search contacts, companies and deals"
            aria-label="Search contacts, companies and deals"
            onChange={(e) => run({ cmd: "find", query: e.target.value })}
          />
        </label>

        {/* The kind filter is shared state, like sort: the core narrows the list and the
            footer counts what the person actually sees. When the agent runs
            `crm find --kind deal`, this is where the person sees that it did. */}
        <div className="sorts" role="group" aria-label="Show">
          <span className="micro">Show</span>
          {KINDS.map(([kind, label]) => (
            <button
              key={label}
              type="button"
              className="chip"
              aria-pressed={list.kind === kind}
              onClick={() => {
                setComposerOpen(false);
                run({ cmd: "find", kind, page: 0 });
              }}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="sorts" role="group" aria-label="Sort">
          <span className="micro">Sort</span>
          {SORTS.map(([key, label]) => (
            <button
              key={key}
              type="button"
              className="chip"
              aria-pressed={list.sort === key}
              onClick={() => run({ cmd: "find", sort: key })}
            >
              {label}
            </button>
          ))}
        </div>

        {list.kind === "company" || list.kind === "contact" ? (
          <button type="button" className="composer-open list-bar-new" onClick={() => setComposerOpen((v) => !v)}>
            <PlusIcon size={14} />
            New {list.kind === "company" ? "company" : "contact"}
          </button>
        ) : null}
      </div>

      {composerOpen && list.kind === "company" ? (
        <NewCompanyForm apply={apply} onDone={() => setComposerOpen(false)} />
      ) : null}
      {composerOpen && list.kind === "contact" ? (
        <NewContactForm apply={apply} onDone={() => setComposerOpen(false)} />
      ) : null}

      {/* The rows scroll; the bar above and the footer below do not. The footer carries the
          count both surfaces quote at each other, so it has to stay on screen — a page
          long enough to push it out of the window is exactly when somebody needs it. */}
      <div className="list-scroll">
        {list.rows.length === 0 ? (
          <p className="empty">
            {list.query ? (
              <>
                Nothing matches “{list.query}”. Your agent searches the same list:{" "}
                <code>{findCmd(list.query)}</code>
              </>
            ) : (
              <>
                No records yet. Your agent can import some: <code>{importCmd("contacts.csv")}</code>
              </>
            )}
          </p>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th scope="col" className="col-kind">
                  Kind
                </th>
                <th scope="col" aria-sort={list.sort === "name" ? "ascending" : "none"}>
                  <button type="button" className="th-sort" onClick={() => run({ cmd: "find", sort: "name" })}>
                    Name
                  </button>
                </th>
                <th scope="col">Detail</th>
                <th scope="col" className="col-stage">
                  Stage
                </th>
                <th scope="col" className="col-status">
                  Status
                </th>
                <th scope="col" className="col-value" aria-sort={list.sort === "value" ? "descending" : "none"}>
                  <button type="button" className="th-sort" onClick={() => run({ cmd: "find", sort: "value" })}>
                    Value
                  </button>
                </th>
              </tr>
            </thead>
            <tbody>
              {list.rows.map((row) => (
                <tr
                  key={idKey(row.id)}
                  tabIndex={0}
                  onClick={() => run({ cmd: "show", kind: row.kind, id: idKey(row.id) })}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") run({ cmd: "show", kind: row.kind, id: idKey(row.id) });
                  }}
                >
                  <td className="col-kind micro">{row.kind}</td>
                  <td className="cell-name">
                    {row.label}
                    {row.archived ? <span className="tag">archived</span> : null}
                  </td>
                  <td className="cell-detail">{row.detail ?? ""}</td>
                  <td className="col-stage">
                    {row.stage && row.status === "open" ? (
                      <span className={`stage-badge stage-${row.stage}`}>{row.stage}</span>
                    ) : (
                      row.stage ?? ""
                    )}
                  </td>
                  <td className="col-status">
                    {row.status ? <span className={`state state-${row.status}`}>{row.status}</span> : null}
                  </td>
                  {/* Tabular numerals, right-aligned: a column of figures that does not line
                      up is a bug, and two currencies in one column make it a worse one. */}
                  <td className="col-value num">{row.value ? <span className="money">{row.value.formatted}</span> : ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <Footer list={list} run={run} />
    </div>
  );
}

/** The Companies view's New control — `crm add company <name> [--domain D]`. */
function NewCompanyForm({ apply, onDone }: { apply: (next: Snapshot) => void; onDone: () => void }) {
  const [name, setName] = useState("");
  const [domain, setDomain] = useState("");
  const { send, busy, error, dismiss } = useWrite(apply);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    const ok = await send({ cmd: "add", kind: "company", name, fields: { domain: domain.trim() || undefined } });
    if (ok) onDone();
  }

  return (
    <form className="composer composer-bar" onSubmit={submit}>
      <input className="composer-input" placeholder="Company name" aria-label="Company name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      <input className="composer-input" placeholder="Domain (optional)" aria-label="Domain" value={domain} onChange={(e) => setDomain(e.target.value)} />
      <button type="submit" className="composer-submit" disabled={busy || !name.trim()}>
        Add company
      </button>
      <button type="button" className="icon-button" aria-label="Cancel" onClick={onDone}>
        <XIcon size={14} />
      </button>
      <p className="empty-line composer-hint">
        Your agent: <code>{addCompanyCmd("Acme Corp")}</code>
      </p>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </form>
  );
}

/** The People view's New control — `crm add contact <name> [--company <handle>] […]`. Only
 *  the company handle is offered here; email/phone/title exist as flags for the agent and
 *  as `set` targets once the contact is open — one more field would not earn its place in
 *  a row this dense. */
function NewContactForm({ apply, onDone }: { apply: (next: Snapshot) => void; onDone: () => void }) {
  const [name, setName] = useState("");
  const [company, setCompany] = useState("");
  const { send, busy, error, dismiss } = useWrite(apply);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    const ok = await send({ cmd: "add", kind: "contact", name, fields: { company: company.trim() || undefined } });
    if (ok) onDone();
  }

  return (
    <form className="composer composer-bar" onSubmit={submit}>
      <input className="composer-input" placeholder="Contact name" aria-label="Contact name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      <input
        className="composer-input"
        placeholder="Company handle (optional)"
        aria-label="Company handle"
        value={company}
        onChange={(e) => setCompany(e.target.value)}
      />
      <button type="submit" className="composer-submit" disabled={busy || !name.trim()}>
        Add contact
      </button>
      <button type="button" className="icon-button" aria-label="Cancel" onClick={onDone}>
        <XIcon size={14} />
      </button>
      <p className="empty-line composer-hint">
        Your agent: <code>{addContactCmd("Ada Whitlock")}</code>
      </p>
      {error ? <ErrorLine error={error} onDismiss={dismiss} /> : null}
    </form>
  );
}

/**
 * "25 of 143" — and `crm find` prints the same string about the same page.
 *
 * That is a shared-surface promise, not a caption: page size is state both surfaces read,
 * so if the window and the terminal word it differently, two people describing the same
 * page to each other are quoting different numbers. The wording lives in `bridge.ts` so
 * there is exactly one of it on this side.
 */
function Footer({ list, run }: { list: ListView; run: (c: Command) => void }) {
  const pages = list.pageSize > 0 ? Math.ceil(list.total / list.pageSize) : 0;
  const of = pageOf(list);

  return (
    <footer className="list-foot">
      <span className="num">{pageWording(list)}</span>
      {of ? <span className="foot-page">{of}</span> : null}
      <span className="foot-spacer" />
      <button
        type="button"
        className="icon-button"
        aria-label="Previous page"
        disabled={list.page <= 0}
        onClick={() => run({ cmd: "find", page: list.page - 1 })}
      >
        <PrevIcon />
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label="Next page"
        disabled={list.page >= pages - 1}
        onClick={() => run({ cmd: "find", page: list.page + 1 })}
      >
        <NextIcon />
      </button>
    </footer>
  );
}
