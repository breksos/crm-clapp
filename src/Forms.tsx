// Shared pieces of the write controls M7 adds: a refusal, shown next to the control that
// caused it, and never invented — the core's own sentence, verbatim
// (`docs/architecture.md` §10b, `m7-window-editing.md` § The parts that are easy to get
// wrong). Nothing here holds a snapshot or a draft value; that lives in whichever control
// is using it, via `useWrite`.

import { XIcon } from "./icons";

/**
 * A refusal, inline. Never a toast, never a dialog — it sits exactly where the control
 * that produced it sits, because that is where the person is already looking, and closing
 * it does not need to feel like dismissing an alert.
 */
export function ErrorLine({ error, onDismiss }: { error: string; onDismiss: () => void }) {
  return (
    <p className="write-error" role="alert">
      <span>{error}</span>
      <button type="button" className="icon-button write-error-dismiss" aria-label="Dismiss" onClick={onDismiss}>
        <XIcon size={12} />
      </button>
    </p>
  );
}
