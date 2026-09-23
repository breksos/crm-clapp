# p0 — the platform spec

**Owner:** PM · **Status:** decided; `p1` and everything after it build to this
**Supersedes:** the single-user framing in [`architecture.md`](../architecture.md) and the
"demonstration, not a product" conclusion in [`market-position.md`](../market-position.md)

Breksos CRM is a **hosted, multi-tenant CRM for teams of up to ~50**, with self-hosting
available. We host it; the client is the clapp. A browser client is a separate project and is
not in this track.

This document settles the contract everything downstream depends on. It is not a work order —
`p1`…`p8` are. Where it disagrees with an older document, this one wins.

---

## 1. The problem this exists to solve

Every surface we have assumes **one person, one machine, one dataset held whole**. Three
specific things break at fifty people:

1. **The snapshot is the whole state.** Every change pushes everything. Correct for 200
   records on a laptop; untenable for 50 users and 100k records.
2. **View state is shared.** `focus`, `list`, `board` and `pending` are shared *by design* —
   the clapp model's whole point. Across fifty people it is nonsense: Ayşe running
   `crm show` must not move Berk's window.
3. **The store is a JSON file on one disk.**

## 2. How the industry solves it, and where we sit

Two patterns, and one thing everybody does that we do not.

**Salesforce / HubSpot — request/response.** The client asks for the page it is displaying;
the server returns only that, filtered by permission and paginated in the database.

**Linear — a sync engine, which is what we already resemble.** Bootstrap once, then a
monotonic append-only log of per-row actions carrying a watermark. Clients replay in order,
hydrating lazily. One server, one counter, one order every client replays — **and permissions
fall out of the same mechanism**, because you never receive deltas for what you cannot see.

**Conflicts: last-write-wins with a version check.** Salesforce's own documentation names the
lost-update problem; Lightning Data Service prompts a refresh on a stale record, and row locks
last only a transaction. **Nobody uses CRDTs for CRM records** — Linear included. Two people
editing one field in the same second is rare, and the cheap answer is the right one.

**We are shaped like Linear already**: local state, a monotonic `rev`, push-on-change. The
shape is right. The scope is wrong.

## 3. The decision — one snapshot becomes two

| | **View state** | **Record data** |
|---|---|---|
| scope | **per seat** — one person and their agents | **per org** |
| holds | `focus`, `list`, `board`, `pending` | companies, contacts, deals, activities, tasks |
| lives | the client, authoritative locally | the server, authoritative |
| travels | never leaves the seat | bootstrap + scoped deltas |
| ordered by | the local `rev`, unchanged | the **server watermark** |

**Co-presence is untouched.** "Both of you are looking at the same thing" was always about a
person and *their* agents. It now scopes to exactly that, and the differentiator survives
intact — including attribution, which gets stronger: `by` stops being human-or-agent and
becomes *which teammate's* agent.

### What this means concretely

- `crm show acme` opens the record in **my** window. Never in anyone else's.
- A deal I move appears on **everyone's** board, because that is record data.
- My list page, filter and sort are mine. Yours are yours.
- `pending` — an unresolved ambiguity — is mine and my agent's. Nobody else answers my question.

## 4. The sync contract

**Watermark.** Every committed change gets a monotonic, server-assigned sequence number.
Clients hold the highest they have seen and ask for everything after it. This is today's `rev`
moved to the server; the client's local `rev` keeps its current job of ordering view updates.

**Bootstrap, then deltas.** A client opens with a scoped bootstrap — the records its
subscriptions cover — then receives per-row actions in order. No whole-state push ever again.

**Subscriptions are the permission boundary.** A client subscribes to what it may see. The
server sends deltas only for those rows. **Visibility is enforced by what is sent, not filtered
on arrival** — a client that never receives a row cannot leak it.

**Writes are optimistic, checked, last-write-wins.** The client applies locally and sends the
write with the version it read. A stale version is refused and the client re-reads and tells
the person plainly. No CRDTs, no field merging in v1.

**Activities are append-only** and therefore need none of this — they are already the right
shape for a log, which is why that decision has survived every round.

## 5. Identity and tenancy

**`org_id` on every row from the first migration.** Retrofitting multi-tenancy is a rewrite
where every query gains a filter and the one you miss is a data leak.

**`owner_id` on every record**, set at creation.

**Users belong to an org and hold a role.** Roles, teams and territories are designed in `p4`
but the model is decided here: permission resolves to *a set of rows a user may see*, and that
set is what the subscription carries.

**Agent identity is workspace-scoped.** `CLATCH_AGENT_ID` is local to one person's Clatch
install and means nothing on another machine. The server issues a workspace agent identity
bound to `(user, local agent)`. Attribution breaks the moment it syncs otherwise — and
attribution is the product.

## 6. Storage

**Server: PostgreSQL.** Fifty concurrent users and years of activity.

**Client: the local cache**, whatever `p1` chooses — the `CrmStore` trait is the seam, which is
what it was built for. `AppState` stays pure; only the impl behind the port changes.

## 7. Security, and what wakes up

Everything below was dormant while the app touched no network. It is now live:

- **Credentials enter through the window and nowhere else.** Never a CLI verb, never a
  snapshot, never a log line — [`playbook.md`](../../clappkit/docs/playbook.md) §13.
- **A token is scoped to its seat** and revocable from the admin surface.
- **Self-hosted installs share no data with us**, and the docs must say so plainly.
- **Audit follows from attribution** — every write already records who and which instance.

## 8. What this costs, stated plainly

- **Both surfaces are rebuilt against a new contract.** M2, M3 and M7 were written against the
  one-snapshot shape. `p1` changes it, which is why `p1` gates everything.
- **A server is a second product** — uptime, backups, migrations, an on-call story.
- **Self-host doubles the release surface**: a `.clapp` and a container, versioned together.
- **Distribution does not improve.** Fifty seats still means fifty Clatch installs. That is a
  known, accepted tension, decided by the product owner with the web client parked.

## 9. What is explicitly not in this track

Web client · CRDTs or field-level merge · offline write queues beyond optimistic local ·
custom objects and custom fields · workflow automation · a second pipeline.

## 10. What `p1` must deliver

The gate. Nothing in Phase 2 starts until these are true:

- [ ] the snapshot is split — view state per seat, record data per org — **additively where it
      can be, and with a documented break where it cannot**
- [ ] a server-assigned watermark exists, and the client holds and advances it
- [ ] bootstrap + delta replaces whole-state push; no code path pushes everything
- [ ] `org_id` and `owner_id` are on every record and round-trip through the store
- [ ] both surfaces are green against the new contract: `cargo test`, `npm test`, `npm run verify`
- [ ] the golden fixtures are regenerated and the window renders them unchanged in meaning

---

**Status:** decided 2026-09-23 by the product owner — hosted multi-tenant, teams to ~50,
self-host available, Clatch-only clients for now.
