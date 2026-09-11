import type { Command, Due, Pending } from "./bridge";
import { selectCmd } from "./commands";
import { AlertIcon, ClockIcon } from "./icons";

/**
 * An unresolved question, parked where both surfaces can see it.
 *
 * **Ambiguity is a state, not a guess and not an error** (`docs/architecture.md` §6).
 * Two companies matching "Acme": picking one silently is confidently wrong, and refusing
 * teaches nothing. So the question sits here with its candidates as clickable rows, and
 * *either* surface answers — a click here, or `crm select 2` in the terminal. The numbers
 * are shown because they are what the agent will type.
 */
export function PendingBanner({ pending, run }: { pending: Pending; run: (c: Command) => void }) {
  return (
    <section className="pending" aria-label="A question is waiting">
      <p className="pending-prompt">
        <AlertIcon />
        {pending.prompt}
      </p>
      <ol className="candidates">
        {pending.candidates.map((c, i) => (
          <li key={c.id}>
            <button type="button" className="candidate" onClick={() => run({ cmd: "select", n: i + 1 })}>
              <span className="candidate-n num">{i + 1}</span>
              <span className="candidate-label">{c.label}</span>
              <span className="candidate-kind micro">{c.kind}</span>
            </button>
          </li>
        ))}
      </ol>
      <p className="pending-cli">
        Or, from the terminal: <code>{selectCmd(2)}</code>
      </p>
    </section>
  );
}

/**
 * Overdue, today, this week — drawn in `--due`, which is **never** `--accent`.
 *
 * Won shares a value with the accent on purpose: a won deal is the app working. Overdue is
 * the opposite, and if it were painted the same green the one thing demanding attention
 * would dissolve into the furniture.
 */
export function DueIndicator({ due }: { due: Due }) {
  const total = due.overdue + due.today + due.week;
  return (
    <div className="due" aria-label="Next steps coming due">
      <ClockIcon />
      {total === 0 ? (
        <span className="due-quiet">Nothing due</span>
      ) : (
        <>
          {due.overdue > 0 ? (
            <span className="due-count due-overdue num" title="Overdue">
              {due.overdue} overdue
            </span>
          ) : null}
          {due.today > 0 ? (
            <span className="due-count num" title="Due today">
              {due.today} today
            </span>
          ) : null}
          {due.week > 0 ? (
            <span className="due-count due-week num" title="Due this week">
              {due.week} this week
            </span>
          ) : null}
        </>
      )}
    </div>
  );
}

/**
 * The one piece of honesty this window owes the person.
 *
 * Clatch ships no scheduler and starts nothing at boot, so a reminder can only fire while
 * this app is open. Somebody who learns that by missing a follow-up has learned it the
 * worst possible way, so it is said here in plain words, directly under the due counts in
 * the header — not in a settings page nobody opens, and not at the foot of the rail, which
 * is where it used to be and is diagonally opposite the number it explains.
 *
 * One line, deliberately. It sits above the board and the table, so a second line is a row
 * of pipeline somebody cannot see.
 */
export function ReminderCaveat() {
  return (
    <p className="caveat">
      Reminders fire only while this app is open — anything that came due while it was
      closed is reported when you next open it.
    </p>
  );
}
