// Every command the window prints, in one place.
//
// **These are promises, not captions.** The window shows them to the person as
// instructions, so each one has to be a line that actually works when typed. The grammar
// is frozen in `docs/work-orders/m2-cli.md` § The argument grammar, and M2 is being built
// against it — `src/commands.test.ts` reads that section and fails if anything here drifts
// out of it, or if a command string is hard-coded into a component again.
//
// Two rules this module exists to enforce:
//
//   1. **Every record reference is a `Handle`, never an `Id`.** The builders take `Handle`
//      and only `Handle`, so a ULID cannot reach a printed command without a deliberate
//      cast. Round one of QA shipped `crm task 01K4Z8QH3M7XC9VBN2RTFA6EDS "call back"`
//      because the two were the same type and nothing objected.
//
//   2. **A command that is not in the grammar does not exist.** If the window wants to
//      tell somebody to type something, either it is in `m2-cli.md` or it is a PM
//      conversation. Inventing a flag here is inventing a feature.

import type { Handle } from "./ids";

/** `crm show <handle>` — open a record in this window. */
export function showCmd(handle: Handle): string {
  return `crm show ${handle}`;
}

/** `crm find <query>` — the search both surfaces share. */
export function findCmd(query: string): string {
  return `crm find ${query}`;
}

/** `crm add deal <title> [--company <handle>]` — the empty board's one instruction. */
export function addDealCmd(title: string, company: Handle): string {
  return `crm add deal "${title}" --company ${company}`;
}

/** `crm task <handle> <what> --due <date>` — set a next step. */
export function taskCmd(handle: Handle, what: string, due: string): string {
  return `crm task ${handle} "${what}" --due ${due}`;
}

/** `crm log <call|email|meeting|note> <handle> <body>` — append to the timeline. */
export function logCmd(kind: "call" | "email" | "meeting" | "note", handle: Handle, body: string): string {
  return `crm log ${kind} ${handle} "${body}"`;
}

/** `crm import <path>` — the agent is the import mechanism; this app reaches nothing. */
export function importCmd(path: string): string {
  return `crm import ${path}`;
}

/** `crm select <n>` — answer a pending question from the terminal instead of by click.
 *  `n` is the 1-based position shown beside the candidate, which is what the person reads
 *  off the screen and types. */
export function selectCmd(n: number): string {
  return `crm select ${n}`;
}

/**
 * `clatch agent grant …` — not one of ours.
 *
 * It is Clatch's, and it is the only command here that is not in `crm -h`: binding an
 * agent to this app happens before this app has anything to say about it. Kept in this
 * module anyway so that "every command the window prints lives in one file" stays true,
 * and marked so the grammar test knows to skip it.
 */
export const GRANT_CMD = "clatch agent grant <name> app:com.breksos.crm";
