// The one seam between the window and the core. Every component talks to the app
// through this file and nothing else — no component imports `invoke`, and none of them
// knows there is a socket underneath.
//
// The transport half is clappkit's and is re-exported unchanged. This app's own types
// and its command envelope are here, because they are the shape of *our* core.

export { cmd, onState, useSnapshot, useAsset, prefetchAssets, agentTint } from "@clappkit";

/** One agent bound to this app, as Clatch last described it.
 *
 *  `id` is immutable and is what everything is keyed on; `name` is a re-pointable label
 *  we only ever display. A rename arrives as a fresh roster with the same id. */
export type Agent = {
  id: string;
  name: string;
  backend: string | null;
  model: string | null;
  /** An absolute file path, not bytes — resolve it with `useAsset`. */
  avatar: string | null;
};

/** What is currently open, and therefore what the person's next message is about. */
export type Focus = {
  kind: "company" | "contact" | "deal";
  id: string;
};

export type Counts = {
  companies: number;
  contacts: number;
  deals: number;
  activities: number;
  tasks: number;
};

/** The whole of what both surfaces agree on.
 *
 *  M1 freezes this shape and adds `pipeline`, `board`, `list` and `due`; M3 builds the
 *  window against it. `rev` orders the two writers that feed this window — our own
 *  `run_cmd` reply and the `state` event the core pushes — and `useSnapshot` drops
 *  whichever arrives stale. */
export type Snapshot = {
  ok: boolean;
  rev: number;
  focus: Focus | null;
  pending: null;
  counts: Counts;
  agents: Agent[];
};

/** The command envelope both surfaces send. The CLI sends the same shape over the
 *  socket, which is what keeps one implementation of every rule. */
export type Command = { cmd: string; [arg: string]: unknown };

/** The state a window shows before the core has answered. Never rendered for long: the
 *  first snapshot is asked for on mount. */
export const EMPTY: Snapshot = {
  ok: true,
  rev: -1,
  focus: null,
  pending: null,
  counts: { companies: 0, contacts: 0, deals: 0, activities: 0, tasks: 0 },
  agents: [],
};
