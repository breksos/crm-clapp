// The "Team server (self-hosted)" setting: whether the window shows any account at all.
//
// **Off by default, and then the window is local mode as it always was** — no sign-in, no sign-up,
// no invite, no Account or Organisation, no sync indicator, no mention of a server anywhere. Most
// people will never have one, and a product that talks about servers to someone who has none is
// asking them a question they cannot answer.
//
// **A session that already exists always shows the account screens**, whatever the setting says —
// otherwise someone signed in could be locked out of signing out.
//
// Window chrome, like the theme: this webview's `localStorage`, allowed to say no, and never in the
// snapshot. An agent cannot turn it on.

import { useCallback, useEffect, useState } from "react";

export const TEAM_SERVER_KEY = "breksos.teamServer";

export type TeamStore = Pick<Storage, "getItem" | "setItem">;

export function loadTeamServer(store: TeamStore | null | undefined): boolean {
  try {
    return store?.getItem(TEAM_SERVER_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveTeamServer(store: TeamStore | null | undefined, on: boolean): boolean {
  try {
    if (!store) return false;
    store.setItem(TEAM_SERVER_KEY, on ? "1" : "0");
    return true;
  } catch {
    return false;
  }
}

/** Show any account screen? Only if the person asked for one, or one already exists. Pure. */
export function showAccount(enabled: boolean, signedIn: boolean): boolean {
  return enabled || signedIn;
}

export function useTeamServer(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(false);
  // Read after mount so the first render matches the server's, which has no storage.
  useEffect(() => {
    try {
      setOn(loadTeamServer(window.localStorage));
    } catch {
      /* off */
    }
  }, []);
  const set = useCallback((next: boolean) => {
    setOn(next);
    try {
      saveTeamServer(window.localStorage, next);
    } catch {
      /* holds for this session */
    }
  }, []);
  return [on, set];
}
