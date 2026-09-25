// The shell's left column: search, where to go, the views worth keeping, and the agent desk.
//
// Direction A's layout, with one deliberate difference: A marks agents in amber. Amber is
// `--due` here, and colour means an agent did this — that is violet, and only the discs and the
// desk carry it. The rail itself is neutral ink.

import { useEffect, useState } from "react";
import type { Command, Snapshot } from "./bridge";
import { Desk } from "./Desk";
import { BoardIcon, BookmarkIcon, DealsIcon, HomeIcon, InboxIcon, PeopleIcon, PlusIcon, ReportsIcon, SettingsIcon, TeamIcon, XIcon } from "./icons";
import { TITLES, type View } from "./nav";
import { SearchBox } from "./Search";
import { addView, BUILTIN_VIEWS, isShowing, loadViews, removeView, saveViews, type SavedView, type ViewKind } from "./views";

type Glyph = (p: { size?: number }) => JSX.Element;

const PRIMARY: [View, Glyph][] = [
  ["home", HomeIcon],
  ["inbox", InboxIcon],
  ["pipeline", BoardIcon],
  ["deals", DealsIcon],
  ["contacts", PeopleIcon],
];
const SECONDARY: [View, Glyph][] = [
  ["reports", ReportsIcon],
  ["team", TeamIcon],
  ["settings", SettingsIcon],
];

function countFor(state: Snapshot, view: View): number | null {
  if (view === "deals") return state.counts.deals;
  if (view === "contacts") return state.counts.companies + state.counts.contacts;
  return null;
}

/** The webview's own storage — absent under SSR, blocked in a private window. */
function store(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

export function Sidebar({
  state,
  run,
  current,
  go,
}: {
  state: Snapshot;
  run: (c: Command) => void;
  current: View;
  go: (v: View) => void;
}) {
  const [mine, setMine] = useState<SavedView[]>([]);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");

  // Read after mount: the first render must match the server's, which has no storage.
  useEffect(() => setMine(loadViews(store())), []);

  function persist(next: SavedView[]) {
    setMine(next);
    saveViews(store(), next);
  }

  const { list } = state;
  const onAList = current === "deals" || current === "contacts";
  const savable = onAList && (list.kind === "deal" || list.kind === "company" || list.kind === "contact");

  function apply(v: SavedView) {
    go(v.kind === "deal" ? "deals" : "contacts");
    run({ cmd: "find", query: v.query, kind: v.kind, sort: v.sort, page: 0 });
  }

  function save() {
    if (!savable || list.kind === null) return;
    persist(addView(mine, { name, query: list.query, kind: list.kind as ViewKind, sort: list.sort }));
    setNaming(false);
    setName("");
  }

  const item = ([view, Glyph]: [View, Glyph]) => {
    const n = countFor(state, view);
    return (
      <li key={view}>
        <button type="button" className="nav-item" aria-current={current === view ? "page" : undefined} onClick={() => go(view)}>
          <Glyph />
          <span className="nav-label">{TITLES[view]}</span>
          {n === null ? null : <span className="nav-count num">{n}</span>}
        </button>
      </li>
    );
  };

  return (
    <nav className="rail" aria-label="Views">
      <SearchBox list={list} run={run} />

      <div className="rail-scroll">
        <ul className="nav">{PRIMARY.map(item)}</ul>
        <ul className="nav nav-secondary">{SECONDARY.map(item)}</ul>

        <section className="saved" aria-label="Saved views">
          <div className="saved-head">
            <h2 className="micro">Saved views</h2>
            <button
              type="button"
              className="icon-button"
              aria-label="Save this view"
              disabled={!savable}
              onClick={() => setNaming((v) => !v)}
            >
              <PlusIcon size={14} />
            </button>
          </div>

          {naming && savable ? (
            <form
              className="saved-form"
              onSubmit={(e) => {
                e.preventDefault();
                save();
              }}
            >
              <input
                className="composer-input"
                aria-label="Name this view"
                placeholder="Name this view"
                value={name}
                autoFocus
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => e.key === "Escape" && setNaming(false)}
              />
            </form>
          ) : null}

          <ul className="saved-list">
            {[...BUILTIN_VIEWS, ...mine].map((v) => (
              <li key={v.id} className="saved-item">
                <button
                  type="button"
                  className="nav-item saved-button"
                  aria-current={onAList && isShowing(v, list) ? "true" : undefined}
                  onClick={() => apply(v)}
                >
                  <BookmarkIcon size={14} />
                  <span className="nav-label">{v.name}</span>
                </button>
                {v.id.startsWith("builtin:") ? null : (
                  <button type="button" className="icon-button" aria-label={`Remove ${v.name}`} onClick={() => persist(removeView(mine, v.id))}>
                    <XIcon size={12} />
                  </button>
                )}
              </li>
            ))}
          </ul>
        </section>

        <Desk state={state} />
      </div>
    </nav>
  );
}
