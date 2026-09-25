// Inbox: what was done, by whom — one feed for people and agents together.
//
// **The feed is what the snapshot carries, and the page says so.** There is no global activity
// log in the snapshot: only each deal's last move (`by`, `movedAt`) and the *open* record's
// timeline. So the feed is those two, merged and ordered, and it fills out as records are
// opened. A full feed — every timeline entry, paged and filterable — needs a field the core does
// not send (`docs/m12-snapshot-gaps.md`). It is not faked from the parts.
//
// People and agents share this one list. Filtering by actor narrows it; it never splits it into
// two lanes (`m11-colour-ownership.md`: both parties act on one surface).

import { useState } from "react";
import { cardOf, idKey, type ActivityKind, type Actor, type Command, type Id, type Kind, type Snapshot } from "./bridge";
import { byLine, Disc } from "./Attribution";
import { KindIcon } from "./icons";
import { ago } from "./time";

type EntryType = "stage" | ActivityKind;

type Entry = {
  key: string;
  at: number;
  type: EntryType;
  by: Actor;
  text: string;
  /** What it is about — the deal's company, or the open record's name. */
  sub: string;
  /** Where clicking goes; null when it is already the open record. */
  open: { kind: Kind; id: Id } | null;
};

const TYPES: [EntryType | "all", string][] = [
  ["all", "All types"],
  ["call", "Calls"],
  ["email", "Emails"],
  ["meeting", "Meetings"],
  ["note", "Notes"],
  ["stage", "Stage changes"],
];

const actorKey = (by: Actor) => (by.kind === "human" ? "you" : by.id);

export function entriesOf(state: Snapshot): Entry[] {
  const out: Entry[] = [];
  for (const column of state.board.columns) {
    for (const id of column.dealIds) {
      const card = cardOf(state, id);
      if (!card) continue;
      out.push({
        key: `move:${idKey(id)}`,
        at: card.movedAt,
        type: "stage",
        by: card.by,
        text: `Moved ${card.label} to ${column.label}`,
        sub: card.detail ?? "",
        open: { kind: "deal", id },
      });
    }
  }
  const focused = state.focused;
  if (focused) {
    for (const a of focused.timeline) {
      out.push({ key: `act:${idKey(a.id)}`, at: a.at, type: a.kind, by: a.by, text: a.body, sub: focused.row.label, open: null });
    }
  }
  return out.sort((a, b) => b.at - a.at);
}

export function Inbox({ state, run }: { state: Snapshot; run: (c: Command) => void }) {
  const [type, setType] = useState<EntryType | "all">("all");
  const [actor, setActor] = useState<string>("all");

  const all = entriesOf(state);
  const ofType = all.filter((e) => type === "all" || e.type === type);
  const shown = ofType.filter((e) => {
    if (actor === "all") return true;
    if (actor === "agents") return e.by.kind === "agent";
    return actorKey(e.by) === actor;
  });
  const count = (pick: (e: Entry) => boolean) => ofType.filter(pick).length;

  const actors: [string, string, Actor | null, number][] = [
    ["all", "Everyone", null, ofType.length],
    ["you", "You", { kind: "human" }, count((e) => e.by.kind === "human")],
    ["agents", "Agents", null, count((e) => e.by.kind === "agent")],
    ...state.agents.map((a): [string, string, Actor | null, number] => [
      a.id,
      a.name,
      { kind: "agent", id: a.id },
      count((e) => e.by.kind === "agent" && e.by.id === a.id),
    ]),
  ];

  return (
    <div className="inbox">
      <aside className="inbox-actors" aria-label="Filter by who">
        <h2 className="micro">Who</h2>
        <ul className="nav">
          {actors.map(([key, label, by, n]) => (
            <li key={key}>
              <button type="button" className="nav-item" aria-pressed={actor === key} onClick={() => setActor(key)}>
                {by ? <Disc by={by} agents={state.agents} size={16} /> : <span className="actor-blank" />}
                <span className="nav-label">{label}</span>
                <span className="nav-count num">{n}</span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <section className="inbox-feed" aria-label="Activity">
        <div className="sorts" role="group" aria-label="Type">
          {TYPES.map(([key, label]) => (
            <button key={key} type="button" className="chip" aria-pressed={type === key} onClick={() => setType(key)}>
              {label}
            </button>
          ))}
        </div>
        <p className="page-note">Each deal’s last move, and the timeline of the record you have open.</p>

        {shown.length === 0 ? (
          <p className="empty">Nothing here yet — open a record to bring its history in.</p>
        ) : (
          <ol className="feed">
            {shown.map((e) => {
              const row = (
                <>
                  <time className="feed-when num" dateTime={new Date(e.at).toISOString()} title={new Date(e.at).toLocaleString()}>
                    {ago(e.at)}
                  </time>
                  <span className="feed-type micro">
                    {e.type === "stage" ? null : <KindIcon kind={e.type} size={12} />}
                    {e.type === "stage" ? "stage" : e.type}
                  </span>
                  <span className="feed-who">
                    <Disc by={e.by} agents={state.agents} size={16} />
                    {byLine(e.by, state.agents)}
                  </span>
                  <span className="feed-text">{e.text}</span>
                  <span className="feed-sub">{e.sub}</span>
                </>
              );
              return (
                <li key={e.key}>
                  {e.open ? (
                    <button type="button" className="feed-row" onClick={() => run({ cmd: "show", kind: e.open!.kind, id: idKey(e.open!.id) })}>
                      {row}
                    </button>
                  ) : (
                    <div className="feed-row">{row}</div>
                  )}
                </li>
              );
            })}
          </ol>
        )}
      </section>
    </div>
  );
}
