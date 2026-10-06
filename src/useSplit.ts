// The hook that feeds the window the split shape: bootstrap once, then updates.
//
// It is the window's only source of state. A message is not the whole state, so this holds a
// `SplitState` and folds each message into it (`split.ts`), and hands the components the `Snapshot`
// view of it. (The whole-state shape it replaced — and `useSnapshot`, which stored whatever the core
// last sent — are gone; `p1-the-split.md` §8, step 3.)
//
// **Two counters, never merged.** `rev` orders what is drawn; `watermark` orders what the data did.
//
// **Every way a message reaches the window**, and what each does:
//   - the pushed `state` event, an `update`        → fold it in
//   - a command's response, carrying `.split`       → the same update, fold it in (idempotent)
//   - `{ cmd: "bootstrap" }`                         → replace everything (mount, or a resync)
//   - `{ ok: false }`                                → dropped; the
//     controls that need the sentence use `write()`, which reads it first.

import { useCallback, useEffect, useRef, useState } from "react";
import { cmd, onState, EMPTY, type Command, type Snapshot } from "./bridge";
import {
  applyChanges, applyUpdate, fromBootstrap, isBootstrap, isUpdate, toSnapshot,
  type Applied, type Changes, type SplitState,
} from "./split";

type Wire = { ok?: boolean; split?: unknown; bootstrap?: unknown; changes?: unknown };

export function useSplit(): { state: Snapshot; run: (req: Command) => void; apply: (next: Snapshot) => void } {
  const held = useRef<SplitState | null>(null);
  const [state, setState] = useState<Snapshot>(EMPTY);
  const booting = useRef(false);
  const catching = useRef(false);

  const publish = useCallback((next: SplitState) => {
    held.current = next;
    setState(toSnapshot(next));
  }, []);

  const bootstrap = useCallback(async () => {
    if (booting.current) return;
    booting.current = true;
    try {
      const res = (await cmd<Wire, Command>({ cmd: "bootstrap" })) as Wire | null;
      if (res?.bootstrap && isBootstrap(res.bootstrap)) publish(fromBootstrap(res.bootstrap));
    } catch {
      // The core is not answering; the window keeps what it has.
    } finally {
      booting.current = false;
    }
  }, [publish]);

  const catchUp = useCallback(async () => {
    const at = held.current;
    if (!at || catching.current) return;
    catching.current = true;
    try {
      const res = (await cmd<Wire, Command>({ cmd: "changes", since: at.watermark })) as Wire | null;
      const current = held.current;
      if (!res?.changes || !current) return;
      const applied: Applied = applyChanges(current, res.changes as Changes);
      if (applied.resync) {
        catching.current = false;
        await bootstrap();
      } else {
        publish(applied.state);
      }
    } catch {
      // As above.
    } finally {
      catching.current = false;
    }
  }, [bootstrap, publish]);

  const adopt = useCallback(
    (message: unknown) => {
      if (!message || typeof message !== "object") return;
      const m = message as Wire;
      if (m.ok === false) return;

      if (m.split && isUpdate(m.split)) return adopt(m.split);

      if (isBootstrap(message)) {
        if (!held.current || message.rev >= held.current.rev) publish(fromBootstrap(message));
        return;
      }
      if (isUpdate(message)) {
        const current = held.current;
        if (!current) return void bootstrap(); // an update before the first bootstrap has nothing to fold into
        const applied = applyUpdate(current, message);
        publish(applied.state);
        if (applied.resync) void catchUp();
        return;
      }
    },
    [bootstrap, catchUp, publish],
  );

  useEffect(() => {
    let dead = false;
    let un: (() => void) | undefined;
    onState<unknown>(adopt)
      .then((f) => {
        if (dead) f();
        else un = f;
      })
      .catch(() => {});
    void bootstrap();
    return () => {
      dead = true;
      un?.();
    };
  }, [adopt, bootstrap]);

  const run = useCallback(
    (req: Command) => {
      cmd<unknown, Command>(req).then(adopt).catch(() => {});
    },
    [adopt],
  );

  // `apply` keeps the old hook's signature so every control that already writes through
  // `useWrite(apply)` is unchanged. What `write()` hands it is the command's response, which
  // `adopt` knows how to read (`.split`), not necessarily a `Snapshot` — the type is the old one.
  return { state, run, apply: adopt as (next: Snapshot) => void };
}
