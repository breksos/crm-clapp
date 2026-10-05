// ⌘K: one search over companies, contacts and deals.
//
// **It reads and writes nothing shared.** The core's `search` envelope (p1, gap 4) answers a query
// without touching the list, the focus or the pending question, so the palette keeps its own
// results in its own state and the Deals page underneath is exactly where the person left it.
// Until p1 the only search was `find`, whose result *is* the shared list: ⌘K had to write the list
// and put it back on the way out, and the agent's `crm find` state was the palette's for as long
// as it was open. That was a read that mutated what the other surface looks at; it is gone, and
// with it the held query/kind/page and the restore.
//
// The one write is the one the person asked for: choosing a result sends `show`, which moves this
// seat's own focus.

import { useEffect, useId, useRef, useState } from "react";
import { cmd, idKey, type Command, type Kind, type Row } from "./bridge";
import { CompanyIcon, DealsIcon, PeopleIcon, SearchIcon } from "./icons";

const KIND_ICON = { company: CompanyIcon, contact: PeopleIcon, deal: DealsIcon } as const;
const KIND_WORD: Record<Kind, string> = { company: "Company", contact: "Person", deal: "Deal" };
const SHOWN = 8;

type Found = { query: string; total: number; counts: Record<Kind, number>; rows: Row[] };

export function SearchBox({ run }: { run: (c: Command) => void }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const [found, setFound] = useState<Found | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const timer = useRef<number | undefined>(undefined);
  /** Which request the latest answer may belong to — a slow answer to an old query must lose. */
  const asked = useRef(0);
  const listId = useId();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        input.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.clearTimeout(timer.current);
    };
  }, []);

  function end() {
    window.clearTimeout(timer.current);
    asked.current += 1;
    setOpen(false);
    setQ("");
    setActive(0);
    setFound(null);
  }

  function change(value: string) {
    setOpen(true);
    setQ(value);
    setActive(0);
    window.clearTimeout(timer.current);
    const term = value.trim();
    if (!term) {
      asked.current += 1;
      setFound(null);
      return;
    }
    const mine = ++asked.current;
    timer.current = window.setTimeout(() => {
      cmd<{ search?: Found; ok?: boolean }, Command>({ cmd: "search", q: term, limit: SHOWN })
        .then((res) => {
          if (mine === asked.current && res?.search) setFound(res.search);
        })
        .catch(() => {});
    }, 140);
  }

  const term = q.trim();
  const answered = term !== "" && found !== null && found.query === term;
  const rows: Row[] = answered ? found.rows.slice(0, SHOWN) : [];

  function choose(row: Row) {
    end();
    run({ cmd: "show", kind: row.kind, id: idKey(row.id) });
    input.current?.blur();
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      end();
      input.current?.blur();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(i + 1, Math.max(0, rows.length - 1)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter" && rows[active]) {
      e.preventDefault();
      choose(rows[active]);
    }
  }

  return (
    <div className="rail-search">
      <label className="search side-search">
        <SearchIcon />
        <input
          ref={input}
          type="text"
          role="combobox"
          aria-expanded={open && term !== ""}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={rows[active] ? `${listId}-${active}` : undefined}
          aria-label="Search companies, people and deals"
          placeholder="Search everything"
          value={q}
          onFocus={() => setOpen(true)}
          onBlur={end}
          onChange={(e) => change(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <kbd className="kbd" aria-hidden="true">
          ⌘K
        </kbd>
      </label>

      {open ? (
        // Mouse-down would blur the input and end the search before the click lands.
        <div className="search-pop" onMouseDown={(e) => e.preventDefault()}>
          {term === "" ? (
            <p className="search-note">Companies, people and deals — start typing a name.</p>
          ) : !answered ? (
            <p className="search-note">Searching…</p>
          ) : rows.length === 0 ? (
            <p className="search-note">Nothing matches “{term}”.</p>
          ) : (
            <>
              <ul className="search-results" role="listbox" id={listId} aria-label="Results">
                {rows.map((row, i) => {
                  const Glyph = KIND_ICON[row.kind];
                  return (
                    <li
                      key={idKey(row.id)}
                      id={`${listId}-${i}`}
                      role="option"
                      aria-selected={i === active}
                      className="search-row"
                      onMouseEnter={() => setActive(i)}
                      onClick={() => choose(row)}
                    >
                      <Glyph />
                      <span className="search-label">{row.label}</span>
                      <span className="search-detail">{row.detail ?? ""}</span>
                      <span className="search-kind micro">{KIND_WORD[row.kind]}</span>
                    </li>
                  );
                })}
              </ul>
              <p className="search-note search-count num">
                {found!.total > rows.length ? `first ${rows.length} of ${found!.total}` : `${found!.total} match${found!.total === 1 ? "" : "es"}`}
                {" · "}
                {(["deal", "company", "contact"] as const)
                  .filter((k) => found!.counts[k] > 0)
                  .map((k) => `${found!.counts[k]} ${KIND_WORD[k].toLowerCase()}${found!.counts[k] === 1 ? "" : "s"}`)
                  .join(", ")}
              </p>
            </>
          )}
        </div>
      ) : null}
    </div>
  );
}
