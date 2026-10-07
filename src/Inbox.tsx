// Inbox: what was done, by whom — one feed for people and agents together.
//
// **p1 replaced the M12 stand-in.** The snapshot used to carry only each deal's last move and the
// open record's timeline, so this page merged the two and said so. The core now serves the real
// feed (gap 2): every timeline entry and every stage change, newest first, filterable by who did it
// and what kind, with the record each is about. Filters and page are **this page's own state** —
// the `feed` envelope answers without touching the shared list — and the unfiltered first page
// is the one the snapshot already holds, kept live by the row actions, so it never waits.
//
// People and agents share this one list. Filtering by actor narrows it; it never splits it into
// two lanes (`m11-colour-ownership.md`: both parties act on one surface).

import { useState } from "react";
import { idKey, type Actor, type Command, type FeedItem, type Feed, type Snapshot } from "./bridge";
import { byLine, Disc } from "./Attribution";
import { useIdentity } from "./IdentityContext";
import { KindIcon, NextIcon, PrevIcon } from "./icons";
import { useRead } from "./useRead";
import { ago } from "./time";

type Type = FeedItem["kind"] | "all";

const TYPES: [Type, string][] = [
  ["all", "All types"],
  ["call", "Calls"],
  ["email", "Emails"],
  ["meeting", "Meetings"],
  ["note", "Notes"],
  ["stage", "Stage changes"],
];

const PAGE = 25;

/** What a feed line says. A stage change has no body of its own; it is the move. */
export function lineOf(item: FeedItem): string {
  if (item.kind !== "stage") return item.body ?? "";
  const to = item.to ? item.to[0].toUpperCase() + item.to.slice(1) : "";
  return `Moved ${item.record?.label ?? "a deal"} to ${to}`.trim();
}

export function Inbox({ state, run }: { state: Snapshot; run: (c: Command) => void }) {
  const [type, setType] = useState<Type>("all");
  const [actor, setActor] = useState<string>("all"); // "all" | "human" | an agent id
  const [page, setPage] = useState(0);
  const identity = useIdentity();

  const filtered = type !== "all" || actor !== "all";
  const asked = filtered || page > 0;
  const request: Command | null = asked
    ? {
        cmd: "feed",
        pageSize: PAGE,
        page,
        ...(actor !== "all" ? { actor } : {}),
        ...(type !== "all" ? { kind: type } : {}),
      }
    : null;
  const fetched = useRead<Feed>(request, "feed", state.watermark);
  const feed: Feed | null = asked ? fetched.data : state.feed ?? null;

  const actors: [string, string, Actor][] = [
    ["human", identity.signedIn && identity.me ? identity.me.name : "You", { kind: "human" }],
    ...state.agents.map((a): [string, string, Actor] => [a.id, a.name, { kind: "agent", id: a.id }]),
  ];
  const pages = feed ? Math.max(1, Math.ceil(feed.total / feed.pageSize)) : 1;

  function pick<T>(set: (v: T) => void, v: T) {
    set(v);
    setPage(0);
  }

  return (
    <div className="inbox">
      <aside className="inbox-actors" aria-label="Filter by who">
        <h2 className="micro">Who</h2>
        <ul className="nav">
          <li>
            <button type="button" className="nav-item" aria-pressed={actor === "all"} onClick={() => pick(setActor, "all")}>
              <span className="actor-blank" />
              <span className="nav-label">Everyone</span>
            </button>
          </li>
          {actors.map(([key, label, by]) => (
            <li key={key}>
              <button type="button" className="nav-item" aria-pressed={actor === key} onClick={() => pick(setActor, key)}>
                <Disc by={by} agents={state.agents} size={16} />
                <span className="nav-label">{label}</span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <section className="inbox-feed" aria-label="Activity">
        <div className="sorts" role="group" aria-label="Type">
          {TYPES.map(([key, label]) => (
            <button key={key} type="button" className="chip" aria-pressed={type === key} onClick={() => pick(setType, key)}>
              {label}
            </button>
          ))}
        </div>

        {feed === null ? (
          <p className="empty">{fetched.loading ? "Loading…" : "The activity feed is not available from this core."}</p>
        ) : feed.items.length === 0 ? (
          <p className="empty">{filtered ? "Nothing matches these filters." : "Nothing has happened yet."}</p>
        ) : (
          <>
            <ol className="feed">
              {feed.items.map((e) => {
                const open = e.record;
                const row = (
                  <>
                    <time className="feed-when num" dateTime={new Date(e.at).toISOString()} title={new Date(e.at).toLocaleString()}>
                      {ago(e.at)}
                    </time>
                    <span className="feed-type micro">
                      {e.kind === "stage" ? null : <KindIcon kind={e.kind} size={12} />}
                      {e.kind}
                    </span>
                    <span className="feed-who">
                      <Disc by={e.by} agents={state.agents} size={16} />
                      {byLine(e.by, state.agents, identity)}
                    </span>
                    <span className="feed-text">{lineOf(e)}</span>
                    <span className="feed-sub">{e.kind === "stage" ? "" : open?.label ?? ""}</span>
                  </>
                );
                return (
                  <li key={idKey(e.id)}>
                    {open ? (
                      <button type="button" className="feed-row" onClick={() => run({ cmd: "show", kind: open.kind, id: idKey(open.id) })}>
                        {row}
                      </button>
                    ) : (
                      <div className="feed-row">{row}</div>
                    )}
                  </li>
                );
              })}
            </ol>
            <div className="feed-pager">
              <span className="num">
                {feed.total} {feed.total === 1 ? "entry" : "entries"} · page {feed.page + 1} of {pages}
              </span>
              <button type="button" className="icon-button" aria-label="Newer" disabled={page === 0} onClick={() => setPage(page - 1)}>
                <PrevIcon />
              </button>
              <button type="button" className="icon-button" aria-label="Older" disabled={page + 1 >= pages} onClick={() => setPage(page + 1)}>
                <NextIcon />
              </button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
