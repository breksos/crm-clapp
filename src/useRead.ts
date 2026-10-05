// A read that serves this surface and touches nothing.
//
// p1's `tasks`, `feed` and `search` envelopes answer a question without changing the list, the
// focus or the pending question — they are not shared state, so their results live here, in the
// component that asked, and never in the snapshot. The data changing is what makes an answer stale,
// so the caller passes the watermark as `fresh` and a new one re-asks. A slow answer to an old
// question never overwrites a newer one.

import { useEffect, useRef, useState } from "react";
import { cmd, type Command } from "./bridge";

export function useRead<T>(request: Command | null, pick: string, fresh: unknown): { data: T | null; loading: boolean } {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(false);
  const asked = useRef(0);
  // The request is an object literal at the call site; its content is what identifies it.
  const key = request ? JSON.stringify(request) : "";

  useEffect(() => {
    if (!key) {
      setData(null);
      return;
    }
    const mine = ++asked.current;
    setLoading(true);
    cmd<Record<string, unknown>, Command>(JSON.parse(key) as Command)
      .then((res) => {
        if (mine !== asked.current) return;
        const got = res && (res as Record<string, unknown>)[pick];
        if (got !== undefined) setData(got as T);
      })
      .catch(() => {})
      .finally(() => {
        if (mine === asked.current) setLoading(false);
      });
  }, [key, pick, fresh]);

  return { data, loading };
}
