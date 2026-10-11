// The window's account (p2): sign up, sign in, sign out, accept an invite, make one.
//
// **These are Tauri commands, window-only.** None is a CLI verb and none is an envelope
// `Core::command` answers — tests on the backend pin both — so an agent cannot reach any of it,
// and this is the only place in the product that can. They are called with `invoke` directly, not
// through `cmd`/`run_cmd`: an account is not part of the snapshot, and nothing here travels on the
// state event.
//
// **The window never holds a secret longer than a keystroke.** A password is in an input until
// the form is submitted, then cleared whether it worked or not. The session token never reaches
// the window at all (`Status` is deliberately token-free). The one credential the window does
// show is a freshly made invite's token — once, because the server keeps only its hash — and
// it is dropped from memory the moment the person dismisses it.

import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";

export type Person = { id: string; name: string };
export type Org = { id: string; name: string };

export type Status = {
  signedIn: boolean;
  server: string | null;
  person: Person | null;
  org: Org | null;
  role: "owner" | "member" | null;
  expiresAt: number | null;
  /** Plain HTTP to something that is not this machine. Said loudly, never downgraded silently. */
  insecure: boolean;
};

export const SIGNED_OUT: Status = { signedIn: false, server: null, person: null, org: null, role: null, expiresAt: null, insecure: false };

export type Invite = { token: string; expiresAt: number };

/** The server address a person last used — not a secret, and retyping it is the worst part of
 *  signing in. Wrapped: storage is allowed to say no. */
const LAST_SERVER = "breksos.lastServer";
export function rememberServer(server: string): void {
  try {
    localStorage.setItem(LAST_SERVER, server);
  } catch {
    /* typing it again is fine */
  }
}
export function lastServer(): string {
  try {
    return localStorage.getItem(LAST_SERVER) ?? "";
  } catch {
    return "";
  }
}

/** What a command's refusal looks like to a person: its own sentence, never a stack. */
export function sentence(e: unknown): string {
  if (typeof e === "string" && e.trim()) return e;
  if (e && typeof e === "object" && "message" in e && typeof (e as { message: unknown }).message === "string") return (e as { message: string }).message;
  return "that did not work — try again";
}

/** How many of each a dataset holds, as `sync_join_plan` counts them. Every key optional: the
 *  window draws what it is told and never assumes a record kind exists. */
export type JoinCounts = Partial<Record<"companies" | "contacts" | "deals" | "tasks" | "activities", number>>;
export type JoinPlan = { localCounts: JoinCounts; orgCounts: JoinCounts };
export type JoinChoice = "bring" | "start-fresh";
/** What `sync_join("bring")` may report: handles the server had to change to keep them unique.
 *  Optional and read defensively — the contract says they are reported, not in what shape. */
export type JoinResult = { renamed?: { from: string; to: string }[] } | null;

export const api = {
  /** p3 (`sync-contract.md` §7): signed in but not joined, and local records exist. Window-only. */
  joinPlan: () => invoke<JoinPlan>("sync_join_plan"),
  join: (choice: JoinChoice) => invoke<JoinResult>("sync_join", { choice }),
  status: () => invoke<Status>("account_status"),
  signUp: (a: { server: string; email: string; password: string; name: string; orgName: string }) => invoke<Status>("account_sign_up", a),
  signIn: (a: { server: string; email: string; password: string }) => invoke<Status>("account_sign_in", a),
  acceptInvite: (a: { server: string; invite: string; email: string; password: string; name: string }) => invoke<Status>("account_accept_invite", a),
  /** → a warning if the server could not be told, else null. */
  signOut: () => invoke<string | null>("account_sign_out"),
  createInvite: (email: string | null) => invoke<Invite>("account_create_invite", { email }),
  members: () => invoke<Person[]>("account_members"),
};

export type Account = {
  status: Status;
  /** `false` until the first answer: nothing is said about the account before it is known. */
  known: boolean;
  busy: boolean;
  error: string | null;
  dismissError: () => void;
  signUp: (a: Parameters<typeof api.signUp>[0]) => Promise<boolean>;
  signIn: (a: Parameters<typeof api.signIn>[0]) => Promise<boolean>;
  acceptInvite: (a: Parameters<typeof api.acceptInvite>[0]) => Promise<boolean>;
  /** → `{ ok, warning }`. A warning means the local session is gone but the server could not be told. */
  signOut: () => Promise<{ ok: boolean; warning: string | null }>;
};

export function useAccount(): Account {
  const [status, setStatus] = useState<Status>(SIGNED_OUT);
  const [known, setKnown] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    api
      .status()
      .then((s) => {
        if (alive.current) {
          setStatus(s);
          setKnown(true);
        }
      })
      .catch(() => {
        // No account machinery here (a preview, an older core): local mode, which is a real state.
        if (alive.current) setKnown(true);
      });
    return () => {
      alive.current = false;
    };
  }, []);

  const attempt = useCallback(async <T,>(work: () => Promise<T>): Promise<{ ok: true; value: T } | { ok: false }> => {
    setBusy(true);
    setError(null);
    try {
      const value = await work();
      return { ok: true, value };
    } catch (e) {
      setError(sentence(e));
      return { ok: false };
    } finally {
      setBusy(false);
    }
  }, []);

  const signedInWith = useCallback(
    (work: () => Promise<Status>, server: string) => async () => {
      const r = await attempt(work);
      if (!r.ok) return false;
      rememberServer(server);
      setStatus(r.value);
      return true;
    },
    [attempt],
  );

  return {
    status,
    known,
    busy,
    error,
    dismissError: () => setError(null),
    signUp: (a) => signedInWith(() => api.signUp(a), a.server)(),
    signIn: (a) => signedInWith(() => api.signIn(a), a.server)(),
    acceptInvite: (a) => signedInWith(() => api.acceptInvite(a), a.server)(),
    signOut: async () => {
      const r = await attempt(() => api.signOut());
      if (!r.ok) return { ok: false, warning: null };
      setStatus(SIGNED_OUT);
      return { ok: true, warning: r.value };
    },
  };
}
