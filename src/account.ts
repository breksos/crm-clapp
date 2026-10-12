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
  const raw =
    typeof e === "string" && e.trim()
      ? e
      : e && typeof e === "object" && "message" in e && typeof (e as { message: unknown }).message === "string"
        ? (e as { message: string }).message
        : null;
  if (raw === null) return "that did not work — try again";
  // The core's guard speaks to *us* ("check it with account_check_server and ask the person first").
  // That is an instruction to the window, never a sentence for the person: if it ever gets this far,
  // the check was skipped, and the person is told only that nothing was sent.
  if (/account_check_server/.test(raw)) return "Nothing was sent: that server address needs your confirmation first. Try again.";
  return raw;
}

/** The address a person is about to send a password to, and why it needs a yes first. Pure. */
export function serverWarning(check: ServerCheck): { host: string; reasons: string[] } {
  const host = check.host ?? "this address";
  const reasons: string[] = [];
  if (check.userinfo) {
    reasons.push(
      `The address has a name and password written in front of the server. The request really goes to ${host} — not necessarily the server the address looks like it names.`,
    );
  }
  if (check.insecure) {
    reasons.push(
      `It is plain HTTP, not HTTPS. Your password and invite would travel unencrypted to ${host}, and anyone on the network between here and there could read them.`,
    );
  }
  return { host, reasons };
}

export type Req =
  | { kind: "in"; args: Parameters<typeof api.signIn>[0] }
  | { kind: "up"; args: Parameters<typeof api.signUp>[0] }
  | { kind: "invite"; args: Parameters<typeof api.acceptInvite>[0] };

export type Sent =
  | { sent: true; status: Status }
  /** Nothing was sent: the address cannot be read. */
  | { sent: false; reason: "invalid"; message: string }
  /** Nothing was sent: the person has to say yes first. */
  | { sent: false; reason: "confirm"; host: string; reasons: string[] };

type ApiLike = Pick<typeof api, "checkServer" | "signIn" | "signUp" | "acceptInvite">;

/**
 * **Look before anything leaves the window.** The address is checked first, every time; a password
 * or an invite is sent only to an address that is secure, or one the person has said yes to by name.
 * `insecureConfirmed` is passed *only* when `confirmed` is true — the window never decides it.
 * Pure over the api, so a test can hold the order and the flag.
 */
export async function sendChecked(a: ApiLike, req: Req, confirmed: boolean): Promise<Sent> {
  const check = await a.checkServer(req.args.server);
  if (!check.valid) return { sent: false, reason: "invalid", message: "That does not look like a server address — use one that starts with https://." };
  const risky = check.insecure || check.userinfo;
  if (risky && !confirmed) return { sent: false, reason: "confirm", ...serverWarning(check) };
  const flag = risky ? { insecureConfirmed: true } : {};
  const status =
    req.kind === "in" ? await a.signIn({ ...req.args, ...flag }) : req.kind === "up" ? await a.signUp({ ...req.args, ...flag }) : await a.acceptInvite({ ...req.args, ...flag });
  return { sent: true, status };
}

/** What `account_check_server` says about an address, before anything is sent to it. */
export type ServerCheck = {
  /** Parses as an http(s) URL with a host, so a request can be made at all. */
  valid: boolean;
  /** The host the request would really go to — which is not always the part of the address a person reads. */
  host: string | null;
  /** Plain HTTP to something that is not this machine (or unreadable: fail closed). */
  insecure: boolean;
  /** `user:pass@` in front of the host — the classic way to make one host look like another. */
  userinfo: boolean;
};

export const api = {
  checkServer: (server: string) => invoke<ServerCheck>("account_check_server", { server }),
  status: () => invoke<Status>("account_status"),
  signUp: (a: { server: string; email: string; password: string; name: string; orgName: string; insecureConfirmed?: boolean }) => invoke<Status>("account_sign_up", a),
  signIn: (a: { server: string; email: string; password: string; insecureConfirmed?: boolean }) => invoke<Status>("account_sign_in", a),
  acceptInvite: (a: { server: string; invite: string; email: string; password: string; name: string; insecureConfirmed?: boolean }) => invoke<Status>("account_accept_invite", a),
  /** → a warning if the server could not be told, else null. */
  signOut: () => invoke<string | null>("account_sign_out"),
  createInvite: (email: string | null) => invoke<Invite>("account_create_invite", { email }),
  members: () => invoke<Person[]>("account_members"),
};

export type Confirm = { req: Req; host: string; reasons: string[] };

export type Account = {
  status: Status;
  /** Set when an address needs the person's yes before anything is sent to it. */
  confirm: Confirm | null;
  confirmSend: () => Promise<boolean>;
  cancelSend: () => void;
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
  /** The address is risky and nothing has been sent: waiting for the person's yes or no. */
  const [ask, setAsk] = useState<Confirm | null>(null);
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

  const submit = useCallback(
    async (req: Req, confirmed: boolean): Promise<boolean> => {
      setAsk(null);
      const r = await attempt(() => sendChecked(api, req, confirmed));
      if (!r.ok) return false;
      const out = r.value;
      if (out.sent) {
        rememberServer(req.args.server);
        setStatus(out.status);
        return true;
      }
      if (out.reason === "invalid") setError(out.message);
      else setAsk({ req, host: out.host, reasons: out.reasons });
      return false;
    },
    [attempt],
  );

  return {
    status,
    known,
    busy,
    error,
    dismissError: () => setError(null),
    signUp: (a) => submit({ kind: "up", args: a }, false),
    signIn: (a) => submit({ kind: "in", args: a }, false),
    acceptInvite: (a) => submit({ kind: "invite", args: a }, false),
    confirm: ask,
    /** The person said yes to this address: send it, with the confirmation the core wants. */
    confirmSend: () => (ask ? submit(ask.req, true) : Promise.resolve(false)),
    cancelSend: () => setAsk(null),
    signOut: async () => {
      const r = await attempt(() => api.signOut());
      if (!r.ok) return { ok: false, warning: null };
      setStatus(SIGNED_OUT);
      return { ok: true, warning: r.value };
    },
  };
}
