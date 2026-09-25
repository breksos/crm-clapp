// Saved views: a name for a search somebody keeps making.
//
// **A seat's own convenience, never the snapshot's.** A saved view is which query, kind and sort
// this person likes to look at — nothing the core needs to know and nothing the agent should be
// steered by. It lives in this webview's storage, like the theme, and never enters a snapshot or
// an envelope; applying one just sends the ordinary `find`.
//
// Only what the snapshot can actually filter on is here: a query, a kind and a sort. A view like
// "closing this month" or "my deals" needs a close date and an owner the snapshot does not carry;
// those are on `docs/m12-snapshot-gaps.md`, not faked as a query string.

export type ViewKind = "deal" | "company" | "contact";
export type ViewSort = "updated" | "name" | "value";

export type SavedView = {
  id: string;
  name: string;
  query: string;
  kind: ViewKind;
  sort: ViewSort;
};

/** Ready-made views over the same three fields. Not stored; not deletable. */
export const BUILTIN_VIEWS: SavedView[] = [
  { id: "builtin:biggest-deals", name: "Biggest deals", query: "", kind: "deal", sort: "value" },
  { id: "builtin:recent-deals", name: "Recently touched deals", query: "", kind: "deal", sort: "updated" },
  { id: "builtin:companies", name: "Companies A–Z", query: "", kind: "company", sort: "name" },
  { id: "builtin:people", name: "People A–Z", query: "", kind: "contact", sort: "name" },
];

export const STORAGE_KEY = "breksos.savedViews.v1";

/** The two methods of `Storage` this needs — so a test can hand in a Map-backed one. */
export type ViewStore = Pick<Storage, "getItem" | "setItem">;

function isView(v: unknown): v is SavedView {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.id === "string" &&
    typeof o.name === "string" &&
    o.name.trim() !== "" &&
    typeof o.query === "string" &&
    (o.kind === "deal" || o.kind === "company" || o.kind === "contact") &&
    (o.sort === "updated" || o.sort === "name" || o.sort === "value")
  );
}

/** Storage can be absent, blocked or hold junk (a private window, cleared data, another
 *  version of this app). None of that is an error the person needs to see: no views. */
export function loadViews(store: ViewStore | null | undefined): SavedView[] {
  try {
    const raw = store?.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isView) : [];
  } catch {
    return [];
  }
}

export function saveViews(store: ViewStore | null | undefined, views: SavedView[]): boolean {
  try {
    if (!store) return false;
    store.setItem(STORAGE_KEY, JSON.stringify(views));
    return true;
  } catch {
    return false;
  }
}

/** Add a view, or replace the one of the same name — saving "Big deals" twice is an edit, not
 *  two entries. An empty name adds nothing. */
export function addView(views: SavedView[], draft: Omit<SavedView, "id">): SavedView[] {
  const name = draft.name.trim();
  if (!name) return views;
  const kept = views.filter((v) => v.name.toLowerCase() !== name.toLowerCase());
  const id = `view:${Date.now().toString(36)}:${kept.length}`;
  return [...kept, { ...draft, name, id }];
}

export function removeView(views: SavedView[], id: string): SavedView[] {
  return views.filter((v) => v.id !== id);
}

/** Whether the shared list is showing exactly this view — what lights it in the sidebar. */
export function isShowing(view: SavedView, list: { query: string; kind: string | null; sort: string }): boolean {
  return view.query === list.query && view.kind === list.kind && view.sort === list.sort;
}
