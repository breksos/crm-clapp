// What the timer says about itself, in words.
//
// The core publishes `snapshot.reminders` (`m4-timer.md`) and `crm status` already turns it
// into sentences (`cli.rs`, `reminder_lines`). This is the window's half of the same thing: the
// same facts, the same order, the same honesty — so that a person and their agent are never
// told two different stories about one timer. Kept pure (no React, no clock of its own) so a
// test can hold the wording to the CLI's.
//
// **Emitting is not delivering.** "Last sent" means a `task.due` left this app. The platform
// does not confirm that an agent received it, and nothing here says otherwise.

import type { Reminders } from "./bridge";
import { ago } from "./time";

export type ReminderView = {
  /** "Checked 3m ago, every 5 min" — or, before the first check, why there is none. */
  checked: string;
  /** "Last sent 12m ago (2 next steps)" — or "Nothing sent yet". */
  sent: string;
  /** The one state worth a person's attention, if there is one. Refusal outranks waiting. */
  status: { kind: "refused" | "waiting"; text: string } | null;
  /** Overdue when reminders began, and nobody was woken for them. */
  backlog: string | null;
};

const steps = (n: number) => `${n} next step${n === 1 ? "" : "s"}`;

/** Clatch's word for why, in the person's. Anything newer than we know is quoted, not guessed. */
function why(reason: string): string {
  switch (reason) {
    case "inbox_full":
      return "its inbox is full";
    case "queue_full":
      return "its context queue is full";
    case "":
      return "it could not accept it";
    default:
      return `it said ${reason}`;
  }
}

/** `hasAgents`: whether any agent is bound right now — "no agent is connected to tell" is
 *  only true if none is. */
export function reminderView(r: Reminders, hasAgents: boolean, now: number = Date.now()): ReminderView {
  const checked =
    r.lastSweepAt == null
      ? "Not checked yet — the first check runs moments after the app starts"
      : `Checked ${ago(r.lastSweepAt, now)}, every ${r.everyMinutes} min`;
  const sent =
    r.lastSignalAt == null
      ? "Nothing sent yet"
      : `Last sent ${ago(r.lastSignalAt, now)} (${steps(r.lastSignalCount)})`;

  let status: ReminderView["status"] = null;
  if (r.refusal) {
    const who = r.refusal.agentName ?? "an agent";
    status = {
      kind: "refused",
      text: `${who} would not take the last one — ${why(r.refusal.reason)}. Nobody was told; ${steps(r.awaiting)} will be tried again at the next check.`,
    };
  } else if (r.awaiting > 0 && !hasAgents) {
    status = { kind: "waiting", text: `${r.awaiting} due next step${r.awaiting === 1 ? "" : "s"}, and no agent is connected to tell.` };
  } else if (r.awaiting > 0) {
    status = { kind: "waiting", text: `${r.awaiting} due next step${r.awaiting === 1 ? "" : "s"}, sent at the next check.` };
  }

  const backlog =
    r.backlog > 0
      ? `${steps(r.backlog)} ${r.backlog === 1 ? "was" : "were"} already overdue when reminders began — nobody was woken for ${r.backlog === 1 ? "it" : "them"}.`
      : null;

  return { checked, sent, status, backlog };
}
