# Work order — p1: one snapshot becomes two

**Owners:** backend **and** frontend · **Branches:** `p1-backend`, `p1-frontend`, both from `main`
**Decided in:** [`p0-platform-spec.md`](p0-platform-spec.md) · **Informed by:**
[`m12-snapshot-gaps.md`](../m12-snapshot-gaps.md) · **Blocks:** every platform milestone

> **Both agents read this whole file.** This changes the contract both surfaces are built
> against. Briefed apart, you would build two halves of two different things.

**This is the gate.** Nothing in Phase 2 — identity, sync, permissions — starts until this
lands. It is also the last cheap moment: every surface we add afterwards is one more thing to
re-answer if the shape is wrong.

---

## 1. What this is, in one sentence

**View state stays shared per seat; record data becomes shared per org, delivered as bootstrap
plus scoped deltas against a server watermark.**

Nothing about co-presence is lost. "Both of you are looking at the same thing" always meant a
person and *their* agents — it now scopes to exactly that, and stops meaning "and also the other
forty-nine people", which is the only reading that was ever nonsense.

## 2. What is wrong with what we have

Three things, and the third is the one that forced this order.

1. **The snapshot is the whole state.** Every change pushes everything. Correct for 200 records
   on a laptop; untenable at fifty people and a hundred thousand records.
2. **The store is a JSON file on one disk.**
3. **View state is shared by design** — `focus`, `list`, `board`, `pending`. Across fifty people
   that is broken: Ayşe running `crm show` must not move Berk's window.

And a fourth, which M12 found by building on it: **the snapshot is shaped like the board and the
open record.** Both are well served. Everything *across* records — tasks, activity, totals,
history — is absent, which is exactly where Home and Inbox hit the wall.

## 3. The two halves

| | **View state** | **Record data** |
|---|---|---|
| scope | **per seat** — one person and their agents | **per org** |
| holds | `focus`, `list`, `board`, `pending` | companies, contacts, deals, activities, tasks |
| authority | the client | the server |
| travels | never leaves the seat | bootstrap + scoped deltas |
| ordered by | the local `rev`, unchanged | the **server watermark** |

### What this means concretely

- `crm show acme` opens the record in **my** window. Never in anyone else's.
- A deal I move appears on **everyone's** board, because that is record data.
- My list page, filter and sort are mine. Yours are yours.
- `pending` — an unresolved ambiguity — is mine and my agent's. Nobody else answers my question.
- Saved views are already per seat and already outside the snapshot. They were right; they are
  the model for the rest.

## 4. The sync contract

**Watermark.** Every committed change takes a monotonic, server-assigned sequence number. A
client holds the highest it has seen and asks for everything after it. This is today's `rev`
moved to the server; the client's local `rev` keeps its present job of ordering view updates.
**Two counters, two jobs — do not merge them.**

**Bootstrap, then deltas.** A client opens with a scoped bootstrap covering what its
subscriptions reach, then receives per-row actions in order. **No code path pushes whole state
again.**

**Subscriptions are the permission boundary.** The server sends deltas only for rows this seat
may see. Visibility is enforced by what is sent, never filtered on arrival — a client that never
receives a row cannot leak it.

**Writes are optimistic, version-checked, last-write-wins.** Apply locally, send with the
version you read, and on a stale version re-read and tell the person plainly. No CRDTs, no
field-level merge. This is what Salesforce does and what Linear does; inventing something
cleverer here would be the expensive kind of ambition.

**Activities stay append-only.** They are already the right shape for a log, which is why that
decision has survived every round.

## 5. What the split must also fix, because M12 proved it

`m12-snapshot-gaps.md` is a list of what a real surface reached for and could not find. **The
priority-A items are not "later" — they are the evidence that the current shape is wrong**, and
`p1` is where the shape changes. Serve them now or `p1` freezes a contract M12 has already
broken:

| From the gaps doc | What p1 owes it |
|---|---|
| **1 · tasks across records** | cross-record read, not `focused.tasks` only |
| **2 · a global activity feed and a move log** | the delta log answers this by construction — a feed is a query over it |
| **3 · pipeline totals per currency** | core-formatted, grouped, never summed |
| **4 · a side-effect-free search** | **the design smell this order must end** — see below |
| **5 · richer `find` filters** | status, stage, actor, date |
| **6, 11 · fields already stored and never sent** | `openedAt`, `closedAt`, `updatedAt`, domain, title |

**Gap 4 is the one to read twice.** ⌘K has to write the shared list and put it back, because the
only search we have *is* the shared list. A read that mutates what the other surface is looking
at is a design smell, and the split is the moment it stops: **a query that serves one surface's
own convenience must be expressible without touching shared state.**

## 6. Identity and tenancy — recorded now, enforced later

- **`org_id` on every row, from the first migration.** Retrofitting multi-tenancy is a rewrite in
  which every query gains a filter and the one you miss is a data leak.
- **`owner_id` on every record**, set at creation. Not enforced yet. Gap 7 wants it too.
- **`me: { id, name }` in the snapshot.** Gap 8: `Actor::Human` has no name, so every attribution
  says "You" and nothing can tint or list it. The smallest change that unblocks Team.
- **Agent identity is workspace-scoped.** `CLATCH_AGENT_ID` is local to one Clatch install and
  means nothing on another machine.

**Build no server in this order.** No network, no accounts, no sync engine. `p1` changes the
*shape* so that `p2` and `p3` are features rather than rewrites — exactly how ULIDs and `origin`
made multi-user cheap.

## 7. Storage

The client keeps a local store behind `CrmStore`; `AppState` stays pure. Whether it becomes
SQLite here or at `p3` is the backend's call — say which and why. The server is PostgreSQL and
is not built in this order.

## 8. How to land it without breaking both surfaces at once

The honest risk: this is the first order that **cannot** be additive. Sequence it.

1. **Backend first**, publishing both shapes — today's snapshot *and* the new split — behind a
   flag, with the golden fixtures regenerated for each.
2. **Frontend moves to the new shape** against those fixtures, as in M3 and round 3.
3. **The old shape is deleted** once nothing reads it, in its own commit, so a revert is cheap.

If at any point the two shapes disagree about the same state, stop: that is the drift this
architecture exists to prevent, arriving by our own hand.

## 9. Acceptance

- [ ] view state is per seat and never crosses; record data is per org
- [ ] a server-assigned watermark exists, the client holds and advances it, and the local `rev`
      still orders view updates — two counters, documented
- [ ] bootstrap + delta replaces whole-state push; **no code path pushes everything**
- [ ] a search serves ⌘K **without touching `list`** — gap 4 closed
- [ ] gaps 1, 2, 3, 5, 6 and 11 are served by the new shape
- [ ] `org_id`, `owner_id` and `me` round-trip through the store, pinned by tests
- [ ] golden fixtures regenerated; the window renders them unchanged in meaning
- [ ] `cargo test` and `npm test` counts reported; `npm run verify` green; CI green on both branches
- [ ] the old shape is gone, in its own commit

## 10. Do not

- **Do not build a server, accounts, or network calls.** Shape only.
- **Do not merge the two counters.**
- **Do not let a read mutate shared state.** That is the thing being fixed.
- **Do not invent approvals, roles or permissions UI.** Recorded, not enforced.
- **Do not delete the old shape before the frontend is off it.**
