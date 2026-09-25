# M12 — what the pages wanted and the snapshot did not have

**This is `p1`'s input.** [`m12-shell.md`](work-orders/m12-shell.md) asked for the surface first so
the contract could be designed once. Every row below is a field, envelope or filter that a real page
reached for and did not find. **None of them was invented, faked or approximated in the window** —
each page drew what the snapshot holds, said so in one line where a person would otherwise be misled,
and left the rest out.

Shapes in the "Would need" column are *suggestions to make the gap concrete*, not a contract. The
window builds against whatever `p1` freezes.

Priority is by how many pages it blocks and whether it forces the window to misrepresent something:
**A** blocks a page's core purpose · **B** blocks a feature the design shows · **C** would improve
what is already there.

## The short version

| # | Gap | Pri | Pages that wanted it |
|---|---|---|---|
| 1 | A **list of next steps** across records | A | Home, Inbox, Sidebar |
| 2 | A **global activity feed**, and a **move log** (not just each deal's latest) | A | Inbox, Home, Desk |
| 3 | A **pipeline total per currency** | A | Home |
| 4 | A **side-effect-free search** | A | ⌘K |
| 5 | **More `find` filters**: status, stage, actor, date | A | Saved views, Deals |
| 6 | **`openedAt` / `closedAt`** on a deal — **stored, not sent** — and an expected **close date** — not stored | B | Home, Saved views, Reports |
| 7 | **Owner / assignee** on a deal | B | Saved views, Team |
| 8 | **Who the person is** (a name for `Actor::Human`) | B | every attribution disc, Team |
| 9 | **Agent status** (working / idle / last active) | B | Desk, Team |
| 10 | **Read state / what is new** | B | Inbox, sidebar counts |
| 11 | **Row facts** the list lacks: `updatedAt` (**stored, not sent**), deal count, open value, contact's company handle | B | Deals, Contacts & companies |
| 12 | **Creation as an event** (`created` with `by`) | C | Inbox |
| 13 | **Per-kind counts** for a search | C | ⌘K |
| 14 | **History** for Reports (stage transitions, snapshots over time) | — | Reports (stubbed) |
| 15 | **Users, roles, seats** | — | Team, Settings (stubbed) |
| 16 | **Approvals** | — | Home, Desk |

Items 14–16 are platform work; the stubs say so. Items 1–13 are things this single-user app could
serve today with a contract change.

---

## A — the page cannot do its job without it

### 1. The next steps themselves

- **Wanted by:** Home "Needs you today", the Inbox, and every "N overdue" that should be a link.
- **Snapshot has:** `due: { overdue, today, week }` — three numbers. Tasks exist only on
  `focused.tasks`, i.e. for the one open record.
- **The window instead:** Home says "2 next steps overdue" and "Open a deal to see its next steps".
  It cannot say *which*, so the most important sentence on the page is a dead end.
- **Would need:** `tasks: { open: Task[], total }` (capped, newest-due first), each with the record
  it belongs to — `record: { kind, id, handle, label }`. `Task` already has `id`, `handle`, `what`,
  `due`, `doneAt`, `by`.

### 2. A global activity feed, and a move log

- **Wanted by:** Inbox ("every timeline entry, filterable by actor"), Home's recent moves, the desk.
- **Snapshot has:** (a) `focused.timeline` — the 50 newest entries of the **open record only**;
  (b) each deal's **latest** move (`card.by`, `card.movedAt`).
- **The window instead:** the Inbox merges (a) and (b) and says exactly that in one line: *"Each
  deal's last move, and the timeline of the record you have open."* It fills out as records are
  opened, and a deal that moved four times shows once.
- **Would need:** `feed: { items, total, page }` where an item is `{ id, kind: call|email|meeting|
  note|stage|created, body, at, by, record: {kind,id,handle,label}, from?, to? }`, plus a `feed`
  envelope taking `actor?`, `kind?`, `page?`. Stage moves need their **from → to** and history — today
  a move overwrites `moved_by`/`moved_at`. The same log answers Reports' funnel (#14).
- **Design note for `p1`:** if the feed's filter is shared state like `find`'s, decide that on
  purpose. The Inbox's filters are local today, which is the safe default.

### 3. A pipeline total per currency

- **Wanted by:** Home's headline tiles ("Open · USD", "Open · EUR").
- **Snapshot has:** `board.columns[].totals` — already-formatted `Money` per currency, **per column**.
- **The window instead:** the window formats no money and will not sum strings, so Home shows the
  open **deal count** and the Won/Lost totals (which are single columns), not an open-pipeline value.
  A `window.test.ts` check fails if Home ever shows a money string the core did not send.
- **Would need:** `pipeline: { open: Money[], won: Money[], lost: Money[] }` — formatted by the core,
  grouped by currency, never summed across them (the rule already on `BoardColumn.totals`).

### 4. A side-effect-free search

- **Wanted by:** ⌘K, "searches across companies, contacts and deals".
- **Snapshot has:** one search, `find`, whose result *is* the shared list.
- **The window instead:** the palette sends `find` (all kinds) while somebody types and, when the
  search ends, **sends the old query, kind and page back** (`Search.tsx`). It works and it is tested,
  but while the palette is open the list behind it — and the agent's `crm find` state — *is* the
  search. That is honest and a little ugly: a Deals page under the dropdown shows mixed results.
- **Would need:** `search { q, kinds?, limit? }` returning `{ rows, counts }` **without touching
  `list`**. This is the cleanest single addition on this page.

### 5. More `find` filters

- **Wanted by:** Saved views (Direction A shows "All open deals", "My open deals", "Closing by Oct 31",
  "Agent-touched today", "Stalled 60+ days", "Won & lost, Q3"), and the Deals page generally.
- **Snapshot has:** `find` filters by `query`, `kind`, `sort`, and a non-sticky `--archived`.
- **The window instead:** saved views store only those three fields, and the built-ins are the four
  that are honest with them (Biggest deals, Recently touched deals, Companies A–Z, People A–Z). A view
  like "open deals" cannot be expressed, so it is not offered.
- **Would need:** `find` fields `status` (open|won|lost), `stage`, `touchedBy` (an Actor), `since` /
  `before` (a date on `movedAt`). "Stalled 60+ days" is then `status: open, before: <date>` — `movedAt`
  is already on every card.
- **Note:** saved views are stored per seat in the webview (`views.ts`) and **never enter the
  snapshot**, as ordered. If `p1` wants agent-visible named views (`crm view save`), that is a new
  decision, not a fix.

---

## B — the design shows it, the data does not exist

### 6. Deal dates

Two different gaps, and the cheaper one is the larger:

- **Stored but not sent.** The core's `Deal` carries `opened_at` and `closed_at` (`model.rs`), and the
  snapshot's `Row`/`Card` send neither. Home's "Won this quarter" and any age or cycle-time figure
  are one field away. **Would need:** `openedAt`, `closedAt` on deal rows.
- **Not stored.** There is no *expected* close date, so Home's "Closing by Oct 31" panel and a
  "closing this month" saved view are not built. **Would need:** `closesOn: date | null` on `Deal`
  (settable, filterable — #5).

### 7. Owner

`card.by` is who last *moved* the card — not who owns the deal. "My open deals" and Team's per-person
views need `owner: Actor | null` (settable). One person today makes "mine" trivial; agents that own
work makes it real.

### 8. The person has no name

`Actor::Human` is `{ kind: "human" }` — no id, no name. Every attribution says "You" and draws a
neutral "Y" disc; nothing can tint it or name it, and Team cannot list it. **Would need:**
`me: { id, name }` in the snapshot (and `Actor::Human { id }` if a second person is ever possible).
This is the smallest change that unblocks Team.

### 9. Agent status

The roster is `{ id, name, backend, model, avatar }`. Direction A's rail shows *"Drafting security
answers…"* and *"Idle — last run 25m ago"*; the desk can only say "moved X · 2h ago". **Would need:**
`agents[].status: { state: working|idle, note?, at }` reported by the agent — or, at minimum,
`lastActiveAt`.

### 10. What is new

There is no read state: no unread count for the Inbox (Direction A shows one), no "since you last
looked". **Would need:** either a core-side `changes(sinceRev)` or a per-seat `lastSeen` the window
keeps (a local convenience, like saved views) *plus* the feed (#2) to compare against.

### 11. Row facts the lists lack

`Row` is `{ kind, id, handle, label, detail, stage, status, value, archived }` — one `detail` string.
**The core already stores much more** and the snapshot does not send it: `updated_at` on companies and
contacts (the list *sorts* by "recent" and cannot show when), a company's `domain` and `tags`, a
contact's `title`, `email` and `phone`, and every relation (`company_id`, `contact_ids`). The open
record gets all of it through `focused.fields`; the list gets none.

**Would need on `Row`:** `updatedAt`; for a company, `domain`, `dealCount`, `openValue`,
`contactCount`; for a contact, `title` and its company's **handle** (today `detail` is a display name,
so it cannot be a link). Direction A's Companies view has domain, associated deals, last-contact and
fit columns; ours has a name.

---

## C — would improve what is already there

- **12. Creation as an event.** Direction A's log has "Created deal from inbound RFQ". Creation is not
  an activity today, so it never appears in a timeline or the Inbox. `kind: created` with `by`.
- **13. Per-kind counts for a search** (`{ company: 2, contact: 1, deal: 1 }`) so the palette can
  group results and say how many of each.

## Not built, on purpose

- **14. History (Reports).** Needs the move log (#2), `closedAt` (#6) and a way to compare periods.
  One person's pipeline has nothing to compare. The page says so.
- **15. Users, roles, seats (Team, Settings).** Platform work per `p0-platform-spec.md`. Settings
  holds only the theme — kept on the device, like saved views.
- **16. Approvals.** Direction A's home leads with "Approval — Wants to offer 8% discount…". There is
  no approval concept in the core, and inventing one in a window is how a product acquires a workflow
  nobody designed. The desk shows `pending` (the ambiguity question) and nothing else.

## What was *not* a gap

Worth writing down, because it looked like one: **stage counts** (Home's bars) come from
`board.columns[].count`; **"stalled"** is derivable — `card.movedAt` is when a deal entered its
current stage; the **Inbox's actor counts** are computed from what it holds; **Companies/People
totals** are `counts`. None needed a new field.

## What this order taught about the contract

1. **A read that changes shared state is a design smell.** ⌘K had to write the list and put it back
   (#4); saved views had to be kept out of the snapshot entirely. Anything a surface does *for its
   own convenience* should be expressible without touching what the other surface is looking at.
2. **The snapshot is shaped like the board and the open record.** Both are well served; everything
   *across* records (tasks, activity, totals, history) is missing. The Home and Inbox pages are the
   first that need cross-record data, and that is exactly where the gaps cluster (#1–#3).
3. **Attribution is half a feature.** The core records *who* on writes (`by`) but not *who the person
   is* (#8) or *what an agent is doing* (#9), so the most distinctive thing this product has stops at
   a disc and a name.
