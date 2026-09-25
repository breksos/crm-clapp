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
import { cardOf, idKey, useSnapshot, EMPTY, type Command, type Handle, type Row, type Snapshot } from "./bridge";
import { AgentStrip } from "./Attribution";
import { BoardView } from "./Board";
import { Home } from "./Home";
import { Inbox } from "./Inbox";
import { currentView, LIST_KINDS, TITLES, type View } from "./nav";
import { TableView } from "./Table";
import { RecordPanel } from "./Record";
import { DueIndicator, PendingBanner, ReminderCaveat } from "./Panels";
import { BoardIcon, MoonIcon, SunIcon, SystemIcon } from "./icons";
import { Reports, Settings, Team } from "./Stubs";
import { Sidebar } from "./Sidebar";
import { useTheme, type Theme } from "./theme";

export type { View } from "./nav";

const THEMES: [Theme, string, (p: { size?: number }) => JSX.Element][] = [
  ["system", "Match the system theme", SystemIcon],
  ["light", "Light theme", SunIcon],
  ["dark", "Dark theme", MoonIcon],
];

export default function App() {
  const { state, run, apply } = useSnapshot<Snapshot, Command>(EMPTY);
  const [view, setView] = useState<View>("home");
  const [theme, chooseTheme] = useTheme();
  return (
    <Window state={state} run={run} apply={apply} view={view} setView={setView} theme={theme} chooseTheme={chooseTheme} />
  );
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
  apply,
  view,
  setView,
  theme,
  chooseTheme,
}: {
  state: Snapshot;
  run: (c: Command) => void;
  /** The write half every M7 control sends through — see `useWrite.ts`. `useSnapshot`
   *  already exposes it; `Window` just passes it on, the same as `run`. */
  apply: (next: Snapshot) => void;
  view: View;
  setView: (v: View) => void;
  theme: Theme;
  chooseTheme: (t: Theme) => void;
}) {
  function go(next: View): void {
    setView(next);
    // The filter is shared state, so it is written through the core. Narrowing the rows here
    // instead would leave the footer counting a page the person cannot see. Contacts &
    // companies keeps whichever of the two the list already shows.
    if (next === "deals") run({ cmd: "find", kind: "deal", page: 0 });
    if (next === "contacts" && state.list.kind !== "company" && state.list.kind !== "contact") {
      run({ cmd: "find", kind: "company", page: 0 });
    }
  }

  const current = currentView(view, state.list.kind);

  return (
    <div className="shell">
      <header className="head">
        <div className="head-id">
          <BoardIcon />
          <h1>Breksos CRM</h1>
          <span className="head-page" aria-live="polite">
            {TITLES[current]}
          </span>
        </div>

        {/* Grouped by what each thing is: who is here, what is coming due, how it looks.
            The name comes first and is the only heavy thing in the row. */}
        <div className="head-group head-agents">
          <AgentStrip agents={state.agents} />
        </div>

        <div className="head-group">
          <DueIndicator due={state.due} />
        </div>

        <div className="head-group themes" role="group" aria-label="Theme">
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
        <Sidebar state={state} run={run} current={current} go={go} />

        <main className="main">
          {/* Directly under the due indicator in the header, because that is the thing it
              qualifies. */}
          <ReminderCaveat />

          {/* Ambiguity is a state: it sits above whatever is showing, because it is the
              one thing here that is waiting on the person. */}
          {state.pending ? <PendingBanner pending={state.pending} run={run} /> : null}

          {current === "home" ? <Home state={state} run={run} go={go} /> : null}
          {current === "inbox" ? <Inbox state={state} run={run} /> : null}
          {current === "pipeline" ? (
            <BoardView
              board={state.board}
              cards={state.cards}
              agents={state.agents}
              focusId={state.focus?.id ?? null}
              run={run}
              apply={apply}
            />
          ) : null}
          {current === "deals" || current === "contacts" ? (
            <TableView list={state.list} kinds={LIST_KINDS[current]} run={run} apply={apply} />
          ) : null}
          {current === "reports" ? <Reports /> : null}
          {current === "team" ? <Team /> : null}
          {current === "settings" ? <Settings theme={theme} chooseTheme={chooseTheme} /> : null}
        </main>

        <RecordPanel
          focused={state.focused}
          agents={state.agents}
          example={exampleHandle(state)}
          known={knownRecords(state)}
          run={run}
          apply={apply}
        />
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

/** Every record currently loaded anywhere in the window — the board's cards, plus the
 *  shared list's current page — for the record panel's Link control to search. Not every
 *  record that exists: there is no envelope for a second, scoped search, so the picker is
 *  scoped to what the window already has in hand. Deduped, since a deal appears in both. */
function knownRecords(state: Snapshot): Row[] {
  const byId = new Map<string, Row>();
  for (const c of Object.values(state.cards)) byId.set(idKey(c.id), c);
  for (const r of state.list.rows) byId.set(idKey(r.id), r);
  return [...byId.values()];
}

