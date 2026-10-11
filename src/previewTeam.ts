// The people in the preview's team worlds. A different namespace from record ids — these are
// accounts and agents, like the roster, and the window keys on them but never renders one — so
// they are literals on purpose (`commands.test.ts` guards *record* fixtures).

import type { Snapshot } from "./bridge";

export const TEAM_ME = { id: "me", name: "Berk Öge" };
export const TEAM_ANA = { id: "ana", name: "Ana Torres" };

export const TEAM: Pick<Snapshot, "me" | "members" | "teamAgents"> = {
  me: TEAM_ME,
  members: [TEAM_ME, TEAM_ANA, { id: "mert", name: "Mert Kaya" }],
  teamAgents: [{ id: "ag_ana_scout", name: "Scout", ownerId: TEAM_ANA.id }],
};
