// What a stale refusal says the record is now — pure, so it can be tested without a renderer.

import type { Refused } from "./bridge.ts";

/** What the record says now, as plain facts — only the ones the row carries. */
export function currentFacts(row: Extract<Refused, { reason: "stale" }>["current"]): string[] {
  const facts: string[] = [];
  if (row.value) facts.push(`Value ${row.value.formatted}`);
  if (row.stage && row.status === "open") facts.push(`Stage ${row.stage}`);
  if (row.status && row.status !== "open") facts.push(`Status ${row.status}`);
  if (row.detail) facts.push(row.detail);
  if (row.archived) facts.push("Archived");
  return facts;
}
