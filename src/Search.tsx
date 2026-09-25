// ⌘K: one search over companies, contacts and deals.
//
// **It searches by writing the shared list, and puts it back.** The core has one search — `find`
// — and its result is the shared list both surfaces look at. There is no side-effect-free search
// envelope, so this sends the ordinary `find` (all kinds) while somebody types, and when the
// search ends — Escape, choosing a result, clicking away — it sends the query, kind and page the
// list held before, so an open Deals page comes back the way it was. Between, the list behind
// the dropdown *is* the search, which is honest: the agent's `crm find` state is that too.
//
// The missing envelope is on `docs/m12-snapshot-gaps.md` — it is the cleanest thing p1 could add.

import { useEffect, useId, useRef, useState } from "react";
import { idKey, type Command, type Kind, type ListView, type Row } from "./bridge";
import { CompanyIcon, DealsIcon, PeopleIcon, SearchIcon } from "./icons";

const KIND_ICON = { company: CompanyIcon, contact: PeopleIcon, deal: DealsIcon } as const;
const KIND_WORD: Record<Kind, string> = { company: "Company", contact: "Person", deal: "Deal" };
const SHOWN = 8;

type Held = { query: string; kind: Kind | null; page: number };

export function SearchBox({ list, run }: { list: ListView; run: (c: Command) => void }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const held = useRef<Held | null>(null);
  const searched = useRef(false);
  const timer = useRef<number | undefined>(undefined);
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

  function begin() {
    if (!held.current) held.current = { query: list.query, kind: list.kind, page: list.page };
    setOpen(true);
  }

  /** End the search, and put the list back if the search moved it. */
  function end() {
    window.clearTimeout(timer.current);
    const before = held.current;
    if (before && searched.current) run({ cmd: "find", query: before.query, kind: before.kind, page: before.page });
    held.current = null;
    searched.current = false;
    setOpen(false);
    setQ("");
    setActive(0);
  }

  function change(value: string) {
    // Typing is what moves the list, so it is also what must be able to put it back — even if
    // the focus event that normally starts the search never arrived.
    begin();
    setQ(value);
    setActive(0);
    window.clearTimeout(timer.current);
    if (!value.trim()) return;
    timer.current = window.setTimeout(() => {
      searched.current = true;
      run({ cmd: "find", query: value.trim(), kind: null, page: 0 });
    }, 140);
  }

  const term = q.trim();
  // The list is the shared one, so it only *is* this search once the core has answered it.
  const answered = term !== "" && list.query === term && list.kind === null;
  const rows: Row[] = answered ? list.rows.slice(0, SHOWN) : [];

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
          onFocus={begin}
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
                {list.total > rows.length ? `first ${rows.length} of ${list.total}` : `${list.total} match${list.total === 1 ? "" : "es"}`}
              </p>
            </>
          )}
        </div>
      ) : null}
    </div>
  );
}
