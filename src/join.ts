// The join choice's words — pure.

import type { HandleChange, JoinCounts, JoinResult } from "./account.ts";

const KINDS: [keyof JoinCounts, string, string][] = [
  ["companies", "company", "companies"],
  ["contacts", "contact", "contacts"],
  ["deals", "deal", "deals"],
  ["tasks", "next step", "next steps"],
];

/** "3 companies, 1 deal" — only what is there, and "nothing" if nothing is. */
export function countsLine(c: JoinCounts | undefined): string {
  const parts = KINDS.filter(([k]) => (c?.[k] ?? 0) > 0).map(([k, one, many]) => `${c![k]} ${c![k] === 1 ? one : many}`);
  return parts.length > 0 ? parts.join(", ") : "nothing";
}

/** "3 handles changed: acme → acme-2, …" — what the server had to rename to keep handles unique. */
export function handleChangesLine(changes: HandleChange[]): string {
  if (changes.length === 0) return "No handles changed.";
  const shown = changes.slice(0, 8).map((c) => `${c.proposed} → ${c.final}`).join(", ");
  const more = changes.length > 8 ? `, and ${changes.length - 8} more` : "";
  return `${changes.length} handle${changes.length === 1 ? "" : "s"} changed so they stay unique: ${shown}${more}.`;
}

/** A refused join in the person's words: the core's sentence, and — for a refused `bring` — how far
 *  it got, because the organisation is exactly as it was after the last complete part. */
export function refusalLine(r: JoinResult): string {
  const base = r.error ?? "That did not work — nothing was changed.";
  if (r.choice !== "bring" || r.partsTotal === undefined || r.partsTotal === 0) return base;
  const where = r.failedAt ? ` It stopped at ${r.failedAt.kind} ${r.failedAt.handle}.` : "";
  const moved = (r.handleChanges ?? []).length > 0 ? ` ${handleChangesLine(r.handleChanges!)}` : "";
  return `${base} ${r.partsApplied ?? 0} of ${r.partsTotal} parts went in, and the organisation is as it was after the last complete one.${where}${moved}`;
}
