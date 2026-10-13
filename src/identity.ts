// Who "you" are, in words (p2).
//
// Until p2 the person had no name and no id, so every attribution said "You" and drew a neutral
// disc. Signed in, the core knows who is at the seat — `seat.me`, and `Actor::Human { id }` on what
// they did — and the window says so: their name, and their initials on the disc.
//
// **Local mode stays exactly as it was.** Signed out there is no account and no stranger to be
// told apart from, so "You" is still the right word and the disc is still the neutral one. The
// name is a signed-in thing, not an OS-user-name thing.
//
// **No hue for a person.** M11 settled that colour means an agent did this; the disc stays ink on
// a neutral ground, and what makes it *this* person is the name and the initials.

import type { Actor } from "./bridge.ts";

export type Me = { id: string; name: string };

export type Identity = {
  /** Whether an account is signed in — the only time a person has a name worth saying. */
  signedIn: boolean;
  me: Me | null;
  /** p3: the org's people, so a teammate's work carries their name. Names only. */
  members?: Me[];
  /** p3: the org's workspace agents, so a teammate's agent is "Ana's Scout", not "an agent". */
  teamAgents?: { id: string; name: string; ownerId: string }[];
};

export const LOCAL: Identity = { signedIn: false, me: null };

const memberOf = (id: Identity, who: string | undefined) => (who ? id.members?.find((m) => m.id === who) : undefined);

/** Whose act is this? A human with no id is the seat's own person (every record written before
 *  p2 is stamped on open, but a record from an older build may still lack one). */
export function isMe(by: Actor, id: Identity): boolean {
  if (by.kind !== "human") return false;
  const their = (by as { id?: string }).id;
  return !their || !id.me || their === id.me.id;
}

/** The words a timeline line is prefixed with. */
export function humanName(by: Actor, id: Identity): string {
  if (by.kind !== "human") return "";
  if (!id.signedIn || !id.me) return "You";
  if (isMe(by, id)) return id.me.name;
  return memberOf(id, (by as { id?: string }).id)?.name ?? "A teammate";
}

/** A name for an agent that is not on this seat's roster: a teammate's, "Ana's Scout". */
export function teamAgentName(agentId: string, id: Identity): string | null {
  const a = id.teamAgents?.find((t) => t.id === agentId);
  if (!a) return null;
  const owner = memberOf(id, a.ownerId);
  const first = owner?.name.trim().split(/\s+/)[0];
  return first ? `${first}'s ${a.name}` : a.name;
}

/** Up to two letters from a name: "Ayşe Demir" → "AD", "berk" → "B". */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  const first = [...words[0]][0] ?? "?";
  const last = words.length > 1 ? [...words[words.length - 1]][0] ?? "" : "";
  return (first + last).toLocaleUpperCase();
}

/** What the disc says inside it. Signed out it is the same "Y" it always was. */
export function humanInitial(by: Actor, id: Identity): string {
  if (!id.signedIn || !id.me) return "Y";
  if (isMe(by, id)) return initials(id.me.name);
  const other = memberOf(id, (by as { id?: string }).id);
  return other ? initials(other.name) : "?";
}
