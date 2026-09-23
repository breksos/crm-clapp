// The one hook every M7 control sends its write through.
//
// `m7-window-editing.md` sets three rules for a write from the window:
//
//   - **optimistic, then corrected by the snapshot** — render the value the person just
//     typed, let the real snapshot replace it;
//   - **a refusal must be shown, not swallowed** — the core's own sentence, next to the
//     control that asked;
//   - **no second store** — an edit's in-flight value is the input's own local state,
//     nothing more.
//
// This hook is the only thing standing between a control and `bridge.ts`'s `write()`. It
// holds no snapshot and no draft value; `busy` and `error` are the one control's own
// transient UI state, and every caller gets its own instance — a text field mid-edit and
// a composer beside it never share, or share by accident, one `busy` flag.

import { useCallback, useState } from "react";
import { write, type Command, type Snapshot, type WriteOutcome } from "./bridge";

export function useWrite(apply: (next: Snapshot) => void) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const send = useCallback(
    async (command: Command): Promise<boolean> => {
      setBusy(true);
      setError(null);
      const outcome: WriteOutcome = await write(command, apply);
      setBusy(false);
      if (!outcome.ok) {
        setError(outcome.error);
        return false;
      }
      return true;
    },
    [apply],
  );

  const dismiss = useCallback(() => setError(null), []);

  return { send, busy, error, dismiss };
}
