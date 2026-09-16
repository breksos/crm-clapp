// The window: a board, a table, a record, and — everywhere something was written — who
// wrote it.
//
// It renders the snapshot and holds no state of its own except which view is showing and
// which theme is on. Query, sort, page and focus are all *shared* state: they ride the
// snapshot, both surfaces mutate them, and a control here writes through the core rather
// than filtering a local copy. `useSnapshot` already orders the two writers that feed this
// window; there is no second store around it.
//
// There is no "ask the agent" button here, and there never will be. The window is for the
// person's own actions; their agent reaches this app through Clatch. A control that
// prompted on their behalf would invert the model (`docs/architecture.md` §11).

import { useState } from "react";
import { cardOf, useSnapshot, EMPTY, type Command, type Handle, type Kind, type Snapshot } from "./bridge";
import { AgentStrip } from "./Attribution";
import { BoardView } from "./Board";
import { TableView } from "./Table";
import { RecordPanel } from "./Record";
import { DueIndicator, PendingBanner, ReminderCaveat } from "./Panels";
import { BoardIcon, CompanyIcon, MoonIcon, PeopleIcon, SunIcon, SystemIcon } from "./icons";
import { useTheme, type Theme } from "./theme";

/** Which surface the main pane shows. Local — it is how *this* window is laid out — while
 *  the list's kind filter, which People and Companies set, is shared state. */
export type View = "board" | "people" | "companies";

const NAV: [View, string, (p: { size?: number }) => JSX.Element][] = [
  ["board", "Board", BoardIcon],
  ["people", "People", PeopleIcon],
  ["companies", "Companies", CompanyIcon],
];

const THEMES: [Theme, string, (p: { size?: number }) => JSX.Element][] = [
  ["system", "Match the system theme", SystemIcon],
  ["light", "Light theme", SunIcon],
  ["dark", "Dark theme", MoonIcon],
];

/** The record kind each rail entry narrows the shared list to. */
const KIND_OF: Record<View, Kind | null> = { board: null, people: "contact", companies: "company" };

export default function App() {
  const { state, run } = useSnapshot<Snapshot, Command>(EMPTY);
  const [view, setView] = useState<View>("board");
  const [theme, chooseTheme] = useTheme();
  return <Window state={state} run={run} view={view} setView={setView} theme={theme} chooseTheme={chooseTheme} />;
}

/**
 * The whole window as a pure function of the snapshot.
 *
 * Split from `App` so it can be rendered without Tauri, without hooks that subscribe to
 * anything, and without a DOM — which is how `src/window.test.ts` renders the core's own
 * golden snapshots and checks that no id reaches the page.
 */
export function Window({
  state,
  run,
  view,
  setView,
  theme,
  chooseTheme,
}: {
  state: Snapshot;
  run: (c: Command) => void;
  view: View;
  setView: (v: View) => void;
  theme: Theme;
  chooseTheme: (t: Theme) => void;
}) {
  function go(next: View): void {
    setView(next);
    // The filter is shared state, so it is written through the core. Narrowing the rows
    // here instead would leave the footer counting a page the person cannot see.
    if (next !== "board") run({ cmd: "find", kind: KIND_OF[next], page: 0 });
  }

  // The rail highlights whatever the *shared* filter says, not what was last clicked: if
  // the agent runs `crm find --kind deal`, People is no longer what the list is showing.
  const current: View =
    view === "board" ? "board" : state.list.kind === "company" ? "companies" : state.list.kind === "contact" ? "people" : view;

  return (
    <div className="shell">
      <header className="head">
        <BoardIcon />
        <h1>Breksos CRM</h1>

        <div className="head-agents">
          <AgentStrip agents={state.agents} />
        </div>

        <DueIndicator due={state.due} />

        <div className="themes" role="group" aria-label="Theme">
          {THEMES.map(([key, label, Glyph]) => (
            <button
              key={key}
              type="button"
              className="icon-button"
              aria-label={label}
              aria-pressed={theme === key}
              onClick={() => chooseTheme(key)}
            >
              <Glyph />
            </button>
          ))}
        </div>
      </header>

      <div className="body">
        <nav className="rail" aria-label="Views">
          <ul className="nav">
            {NAV.map(([key, label, Glyph]) => (
              <li key={key}>
                <button
                  type="button"
                  className="nav-item"
                  aria-current={current === key ? "page" : undefined}
                  onClick={() => go(key)}
                >
                  <Glyph />
                  <span>{label}</span>
                  <span className="nav-count num">{countFor(state, key)}</span>
                </button>
              </li>
            ))}
          </ul>

        </nav>

        <main className="main">
          {/* Directly under the due indicator in the header, because that is the thing it
              qualifies. It used to live at the foot of the rail, which is diagonally
              opposite: somebody reading "2 overdue" in the top right was never going to
              find the sentence explaining it in the bottom left. */}
          <ReminderCaveat />

          {/* Ambiguity is a state: it sits above whatever is showing, because it is the
              one thing here that is waiting on the person. */}
          {state.pending ? <PendingBanner pending={state.pending} run={run} /> : null}

          {view === "board" ? (
            <BoardView
              board={state.board}
              cards={state.cards}
              agents={state.agents}
              focusId={state.focus?.id ?? null}
              run={run}
            />
          ) : (
            <TableView list={state.list} run={run} />
          )}
        </main>

        <RecordPanel focused={state.focused} agents={state.agents} example={exampleHandle(state)} />
      </div>
    </div>
  );
}

/**
 * A handle that exists, for the "nothing open" line to name — the first row on the shared
 * list, else the first card on the board — or null when there are no records at all.
 */
function exampleHandle(state: Snapshot): Handle | null {
  const row = state.list.rows[0];
  if (row) return row.handle;
  for (const column of state.board.columns) {
    for (const id of column.dealIds) {
      const card = cardOf(state, id);
      if (card) return card.handle;
    }
  }
  return null;
}

function countFor(state: Snapshot, view: View): number {
  if (view === "people") return state.counts.contacts;
  if (view === "companies") return state.counts.companies;
  return state.counts.deals;
}
