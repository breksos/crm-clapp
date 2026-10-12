// The window: a board, a table, a record, and — everywhere something was written — who
// wrote it.
//
// It renders the snapshot and holds no state of its own except which page is showing and
// which theme is on. **Since p1 the view is this seat's own** — focus, list, board, the question
// parked for the person and their agents — arriving as `view` on every update, so a window and
// the agents bound to it still look at one thing, and nobody else's agent moves it. Record data
// is the org's: a bootstrap, then row actions, folded by `useSplit` (`split.ts`). A control here
// still writes through the core rather than filtering a local copy; `useWrite` hands it the
// core's own sentence when the core says no.
//
// There is no "ask the agent" button here, and there never will be. The window is for the
// person's own actions; their agent reaches this app through Clatch. A control that
// prompted on their behalf would invert the model (`docs/architecture.md` §11).

import { useState } from "react";
import { cardOf, idKey, type Command, type Handle, type Row, type Snapshot } from "./bridge";
import { AgentStrip } from "./Attribution";
import { BoardView } from "./Board";
import { Home } from "./Home";
import { Inbox } from "./Inbox";
import { currentView, LIST_KINDS, TITLES, type View } from "./nav";
import { TableView } from "./Table";
import { RecordPanel } from "./Record";
import { DueIndicator, PendingBanner, ReminderCaveat } from "./Panels";
import { BoardIcon, XIcon, MoonIcon, SunIcon, SystemIcon } from "./icons";
import { Reports, Settings, Team } from "./Stubs";
import { Sidebar } from "./Sidebar";
import { SIGNED_OUT, useAccount, type Account } from "./account";
import { InsecureBanner } from "./AccountPanel";
import { IdentityContext } from "./IdentityContext";
import { useFollowing, chipText } from "./follow";
import { DraftGuard } from "./DraftGuard";
import { useFresh } from "./fresh";
import { usePanel } from "./panel";
import { useSplit } from "./useSplit";
import { useTheme, type Theme } from "./theme";

export type { View } from "./nav";

const THEMES: [Theme, string, (p: { size?: number }) => JSX.Element][] = [
  ["system", "Match the system theme", SystemIcon],
  ["light", "Light theme", SunIcon],
  ["dark", "Dark theme", MoonIcon],
];

/** No account machinery at all — a test, a preview: local mode, which is a real state. */
const LOCAL_ACCOUNT: Account = {
  status: SIGNED_OUT, known: true, busy: false, error: null, dismissError: () => {},
  signUp: async () => false, signIn: async () => false, acceptInvite: async () => false,
  signOut: async () => ({ ok: false, warning: null }),
  confirm: null, confirmSend: async () => false, cancelSend: () => {},
};

export default function App() {
  // p1: the window is fed the split shape — this seat's view, and the org's rows as a bootstrap
  // plus deltas — and `state` is the Snapshot the components read, folded from it.
  const { state, run, apply } = useSplit();
  const [view, setView] = useState<View>("home");
  const [theme, chooseTheme] = useTheme();
  const account = useAccount();
  return (
    <Window state={state} run={run} apply={apply} view={view} setView={setView} theme={theme} chooseTheme={chooseTheme} account={account} />
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
  run: runProp,
  apply,
  view,
  setView,
  theme,
  chooseTheme,
  account,
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
  /** p2: the window's account. Absent (a test, a preview with none), the app is in local mode. */
  account?: Account;
}) {
  const following = useFollowing(state, setView);

  // **A draft belongs to the record it was started on.** Whether an agent moves the focus or the
  // person clicks another row, the text in a note or a next step must never end up on a record it
  // was not written for — so the person is asked, in words, before they leave it (QA round 8).
  // `epoch` remounts the panel when they say yes, so a discarded draft really is gone.
  const [epoch, setEpoch] = useState(0);
  const [asking, setAsking] = useState<{ then: () => void } | null>(null);
  const leaving = (then: () => void) => {
    if (following.owner) setAsking({ then });
    else then();
  };
  const run = (c: Command) => {
    const here = following.focused ? idKey(following.focused.row.id) : null;
    // Opening the record the draft is on is not leaving it.
    if (c.cmd === "show" && following.owner && String(c.id) !== here) return leaving(() => runProp(c));
    runProp(c);
  };
  const fresh = useFresh(following.focused, state.agentFeed);

  function go(next: View): void {
    following.dismiss();
    setView(next);
    // The filter is shared state, so it is written through the core. Narrowing the rows here
    // instead would leave the footer counting a page the person cannot see. Contacts &
    // companies keeps whichever of the two the list already shows.
    if (next === "deals") run({ cmd: "find", kind: "deal", page: 0 });
    if (next === "contacts" && state.list.kind !== "company" && state.list.kind !== "contact") {
      run({ cmd: "find", kind: "company", page: 0 });
    }
  }

  const acct: Account = account ?? LOCAL_ACCOUNT;
  const identity = { signedIn: acct.status.signedIn && !!state.me, me: state.me ?? null };
  const current = currentView(view, state.list.kind);
  const panel = usePanel(following.focused, following.focused ? cardOf(state, following.focused.row.id) : undefined);

  return (
    <IdentityContext.Provider value={identity}>
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

      <InsecureBanner status={acct.status} />

      <div className="body">
        <Sidebar state={state} run={run} apply={apply} current={current} go={go} />

        <main className="main">
          {/* Directly under the due indicator in the header, because that is the thing it
              qualifies. */}
          <ReminderCaveat />

          {/* The agent went somewhere while the person had unsaved work: say where, and let them
              choose when. Never navigates by itself while a draft is open. */}
          {following.chip ? (
            <div className="follow-chip" role="status">
              <button type="button" className="link-button" onClick={() => leaving(following.go)}>
                {chipText(following.chip.label)}
              </button>
              <button type="button" className="icon-button" aria-label="Stay here" onClick={following.dismiss}>
                <XIcon size={12} />
              </button>
            </div>
          ) : null}

          {asking && following.owner ? (
            <DraftGuard
              owner={following.owner}
              onKeep={() => setAsking(null)}
              onDiscard={() => {
                setEpoch((e) => e + 1);
                const next = asking.then;
                setAsking(null);
                next();
              }}
            />
          ) : null}

          {/* Ambiguity is a state: it sits above whatever is showing, because it is the
              one thing here that is waiting on the person. */}
          {state.pending ? <PendingBanner pending={state.pending} apply={apply} /> : null}

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
          {current === "team" ? <Team account={acct} /> : null}
          {current === "settings" ? <Settings theme={theme} chooseTheme={chooseTheme} account={acct} /> : null}
        </main>

        <RecordPanel
          key={`${following.focused ? String(following.focused.row.handle) : ""}:${epoch}`}
          focused={following.focused}
          fresh={fresh}
          agents={state.agents}
          example={exampleHandle(state)}
          known={knownRecords(state)}
          collapsed={panel.collapsed}
          unread={panel.unread}
          onToggle={panel.toggle}
          apply={apply}
        />
      </div>
    </div>
    </IdentityContext.Provider>
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

