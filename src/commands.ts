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

/** The four open stages. Mirrored here rather than imported from `bridge.ts`, which
 *  re-exports `@clappkit` through a Vite alias and would break this module's bare
 *  `node --test` run — the same reason `logCmd`'s activity kinds are inlined below rather
 *  than imported. `bridge.ts` stays the source of truth; a core test pins the words. */
type Stage = "lead" | "qualified" | "proposal" | "negotiation";

/**
 * A value made safe to paste into a POSIX shell.
 *
 * Every value a builder interpolates goes through this — handles included, even though a
 * handle is always bare today, because "always" is a property of the core's slug function
 * and not of this file. A value made only of `[A-Za-z0-9._-]` stays bare, because that is
 * what a person would type; anything else is single-quoted, with each `'` written as `'\''`.
 *
 * **Single quotes, not double.** Round 2 printed `crm task acme "call back"`, which reads
 * fine and is wrong the first time a title contains `$` or a backtick: inside double
 * quotes a shell still expands both, so a deal called `Pay $HOME` would be sent as
 * `Pay /Users/…`, and one containing `` `rm -rf ~` `` would run it. Inside single quotes a
 * shell expands nothing.
 */
export function shellQuote(value: string): string {
  if (/^[A-Za-z0-9._-]+$/.test(value)) return value;
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

const q = shellQuote;

/** `crm show <handle>` — open a record in this window. */
export function showCmd(handle: Handle): string {
  return `crm show ${q(handle)}`;
}

/** `crm find <query>` — the search both surfaces share. */
export function findCmd(query: string): string {
  return `crm find ${q(query)}`;
}

/**
 * `crm add deal <title> [--stage <stage>]` — the empty board's one instruction, and (with
 * a stage) what the board per-column composer shows.
 *
 * **No `--company`.** An empty state may only create; it may not name a record the person
 * does not have (`m2-cli.md` § Acceptance). Round 2 printed `--company northwind`, which on
 * an empty board is a handle that resolves to nothing — an instruction that fails when
 * followed. The stage is different: the column the composer opened in is real by
 * construction, so naming it is not the same mistake.
 */
export function addDealCmd(title: string, stage?: Stage): string {
  return `crm add deal ${q(title)}${stage ? ` --stage ${stage}` : ""}`;
}

/** `crm add company <name>` — what to suggest when there are no records at all, so there is
 *  nothing real to `show`, and what the Companies view's own New control echoes. */
export function addCompanyCmd(name: string): string {
  return `crm add company ${q(name)}`;
}

/** `crm add contact <name>` — the People view's New control. Bare, like the other two `add`
 *  hints: the base verb is what teaches, and `--company`/`--email`/`--phone`/`--title` are
 *  there in `crm add contact -h` once somebody wants them. */
export function addContactCmd(name: string): string {
  return `crm add contact ${q(name)}`;
}

/** `crm task <handle> <what> --due <date>` — set a next step. */
export function taskCmd(handle: Handle, what: string, due: string): string {
  return `crm task ${q(handle)} ${q(what)} --due ${q(due)}`;
}

/** `crm done <task-handle>` — complete a next step. Shown as a hover title beside the
 *  checkbox, not a full `<code>` line: one per task row would be a line of chrome for
 *  every next step on the busiest record. */
export function doneCmd(taskHandle: Handle): string {
  return `crm done ${q(taskHandle)}`;
}

/** `crm set <handle> <field> <value>` — edit a field in place. Same demotion as `done`:
 *  a live hover title on the field being edited, not a permanent line under every field. */
export function setCmd(handle: Handle, field: string, value: string): string {
  return `crm set ${q(handle)} ${q(field)} ${q(value)}`;
}

/** `crm link <handle> <handle>` — the terminal's version of the record panel's Link
 *  control. `to` is null before anything is picked, and renders as the literal
 *  placeholder `<other-handle>` — the same convention `GRANT_CMD` uses for `<name>`. */
export function linkCmd(handle: Handle, to: Handle | null): string {
  return `crm link ${q(handle)} ${to ? q(to) : "<other-handle>"}`;
}

/** `crm due` — the list the reminder backlog line points at. */
export function dueCmd(): string {
  return "crm due";
}

/** `crm archive <handle>` — archive a record. Reversible, never a delete. */
export function archiveCmd(handle: Handle): string {
  return `crm archive ${q(handle)}`;
}

/** `crm archive <handle> --restore` — bring an archived record back. */
export function restoreCmd(handle: Handle): string {
  return `crm archive ${q(handle)} --restore`;
}

/** `crm log <call|email|meeting|note> <handle> <body>` — append to the timeline. */
export function logCmd(kind: "call" | "email" | "meeting" | "note", handle: Handle, body: string): string {
  return `crm log ${kind} ${q(handle)} ${q(body)}`;
}

/** `crm import <path>` — the agent is the import mechanism; this app reaches nothing. */
export function importCmd(path: string): string {
  return `crm import ${q(path)}`;
}

/** `crm select <n>` — answer a pending question from the terminal instead of by click.
 *  `n` is the **1-based** position shown beside the candidate: what the person reads off
 *  the screen and types. The wire carries the same number. */
export function selectCmd(n: number): string {
  return `crm select ${n}`;
}

/**
 * `clatch agent grant …` — not one of ours.
 *
 * It is Clatch's, and it is the only command here that is not in `crm -h`: binding an
 * agent to this app happens before this app has anything to say about it. Kept in this
 * module anyway so that "every command the window prints lives in one file" stays true,
 * and marked so the grammar test knows to skip it. `<name>` is a placeholder the person
 * fills in, which is why it is written out rather than built.
 */
export const GRANT_CMD = "clatch agent grant <name> app:com.breksos.crm";
