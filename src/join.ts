// The join choice's words — pure.

import type { JoinCounts } from "./account.ts";

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
