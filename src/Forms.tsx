// Shared pieces of the write controls M7 adds: a refusal, shown next to the control that
// caused it, and never invented — the core's own sentence, verbatim
// (`docs/architecture.md` §10b, `m7-window-editing.md` § The parts that are easy to get
// wrong). Nothing here holds a snapshot or a draft value; that lives in whichever control
// is using it, via `useWrite`.

import type { Refused } from "./bridge";
import { XIcon } from "./icons";
import { currentFacts } from "./refusal";
import { ago } from "./time";

/**
 * A refusal, inline. Never a toast, never a dialog — it sits exactly where the control
 * that produced it sits, because that is where the person is already looking, and closing
 * it does not need to feel like dismissing an alert.
 *
 * **A stale refusal (p3) is more than a sentence.** Somebody changed the record while the person
 * was looking, so the panel says what it says *now* and who changed it — and says, in words, that
 * what they typed is still where they left it. The draft is never cleared by a refusal: the
 * control keeps it, and the panel only has to tell them so.
 */
export function ErrorLine({ error, refused, onDismiss }: { error: string; refused?: Refused | null; onDismiss: () => void }) {
  if (refused?.reason === "stale") return <StalePanel error={error} refused={refused} onDismiss={onDismiss} />;
  return (
    <p className="write-error" role="alert">
      <span>{error}</span>
      <button type="button" className="icon-button write-error-dismiss" aria-label="Dismiss" onClick={onDismiss}>
        <XIcon size={12} />
      </button>
    </p>
  );
}

function StalePanel({ error, refused, onDismiss }: { error: string; refused: Extract<Refused, { reason: "stale" }>; onDismiss: () => void }) {
  const facts = currentFacts(refused.current);
  return (
    <div className="stale-panel" role="alert">
      <p className="stale-sentence">{error}</p>
      <dl className="stale-now">
        <dt className="micro">{refused.current.label} now</dt>
        <dd>{facts.length > 0 ? facts.join(" · ") : "—"}</dd>
        <dt className="micro">Changed by</dt>
        <dd>
          {refused.by?.name ?? "a teammate"} <time className="num" dateTime={new Date(refused.at).toISOString()}>{ago(refused.at)}</time>
        </dd>
      </dl>
      <p className="stale-kept">Nothing was saved. What you typed is still in the box — look at the new value, then try again or leave it.</p>
      <button type="button" className="icon-button write-error-dismiss" aria-label="Dismiss" onClick={onDismiss}>
        <XIcon size={12} />
      </button>
    </div>
  );
}
