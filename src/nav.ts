// Where the person is in the window.
//
// Local state — it is how *this* window is laid out. What the list is narrowed to (`kind`,
// `query`, `sort`) is shared state and rides the snapshot; a page that shows a list writes
// through `find` rather than filtering a local copy.

export type View = "home" | "inbox" | "pipeline" | "deals" | "contacts" | "reports" | "team" | "settings";

/** The pages that are the shared list, and the kinds each one may show. */
export const LIST_KINDS = {
  deals: ["deal"],
  contacts: ["company", "contact"],
} as const;

export const TITLES: Record<View, string> = {
  home: "Home",
  inbox: "Inbox",
  pipeline: "Pipeline",
  deals: "Deals",
  contacts: "Contacts & companies",
  reports: "Reports",
  team: "Team",
  settings: "Settings",
};

/** The highlighted entry follows the *shared* filter, not the last click: if the agent runs
 *  `crm find --kind deal`, Contacts & companies is no longer what the list is showing. */
export function currentView(view: View, kind: "company" | "contact" | "deal" | null): View {
  if (view === "deals" || view === "contacts") {
    if (kind === "deal") return "deals";
    if (kind === "company" || kind === "contact") return "contacts";
  }
  return view;
}
