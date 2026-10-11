# Work order — p3: one board for the whole team

**Owners:** backend (server + the app's sync client), frontend, release, QA
**Branches:** `p3-contract` → `p3-backend`, `p3-frontend`, `p3-release`
**Decided in:** [`p0-platform-spec.md`](p0-platform-spec.md) §3–§6 · **Builds on:**
[`p1-the-split.md`](p1-the-split.md), [`split-contract.md`](../split-contract.md),
[`p2-identity.md`](p2-identity.md), [`identity-contract.md`](../identity-contract.md)

> **Every agent reads this whole file.** p3 is the first milestone where one person's write
> reaches another person's screen. The protocol, the window and the deployment have to agree on
> one design.

---

## 1. What p3 is, and what it is not

**p3 makes record data shared.** Two people signed into the same org see the same companies,
contacts, deals, next steps and timeline. A deal Ana moves appears on Berk's board, attributed to
Ana. The server is the source of truth for a signed-in org; each app keeps a local replica.

**p3 does not restrict visibility.** In p3, everyone in an org sees every record in it. *Who may
see what* is p4. But the code path must already be **subscription-scoped**: the server sends a
seat the rows its subscription covers. In p3 that subscription is "the whole org"; p4 narrows it
without touching the protocol.

**View state never travels.** `focus`, `list`, `board`, `pending`, `feed`, `follow` and `page`
stay on the seat. This is already enforced by p1 and p2, and p3 must not weaken it.

**Local mode is unchanged.** With no account, the app works exactly as it does today, on its own
file.

## 2. Decisions made here

| | Decision | Why |
|---|---|---|
| **Truth** | The server, for a signed-in org | One ordering for everyone: the server watermark, as p0 §4 decided. |
| **The replica** | The app keeps the org's rows and the watermark it has applied, persisted locally | Opening the app is instant, and a server restart doesn't force fifty full re-downloads. |
| **Writes** | **Confirmed before acknowledged.** A write goes to the server with the version it read; the app answers the CLI or the window only after the server accepts it | This is the durability rule we already have ("persist before acknowledging"), extended to the network. An agent that got exit 0 knows its write is real. |
| **Conflicts** | **Last-write-wins with a version check**, per row (p0 §4). A stale write is refused, the row is re-read, and the person or agent is told plainly what it says now | No CRDTs and no field merging, as decided in p0. |
| **Activities** | Append-only, no version check | They never conflict. |
| **Handles** | **Unique per org, assigned by the server.** The client proposes; the server returns the final handle (`acme` or `acme-2`), and the CLI prints the final one | Two people creating "Acme" at once must not end up with one handle for two records. |
| **Offline** | **Reads work from the replica; writes are refused** with a clear message (CLI exit 1), and the window shows that it's offline | p0 §9 rules out offline write queues. An honest refusal beats a queue that loses data. |
| **Live updates** | The server pushes "the watermark is now N" over one long-lived connection per seat. The app fetches `changes since` itself. A **heartbeat every 30 s** carries the current watermark, so a missed push is noticed within 30 s | Pushes are hints, the log is the truth, and the heartbeat closes the gap p1 left open. |
| **The log** | **Persisted on the server** (an `ops` table), with a retention window. `changes since N` older than the window answers "bootstrap again" | p1's log was memory-only, so a restart forced every client to bootstrap: a thundering herd at 50 seats. |
| **Joining an org with local data** | **The person chooses** in the window: bring my local records into this org, or start from the org's. Never automatic. The local file is kept as a backup either way | Silently merging someone's private pipeline into a shared one is a trust failure we can't take back. |
| **Signals** | My agent is signalled about **my** actions only, as today. A teammate's changes reach my window but **don't** signal my agent in p3 | Team-level signals need their own design (noise, permission). Recorded as a later order. |

## 3. The protocol — written first, reviewed before code

**Step 1 for backend is `docs/sync-contract.md`, and nothing else. Then stop for PM review.**
Frontend and release build against it, as they built against `split-contract.md` in p1. It must
cover:

- **Server tables:** each record kind with `org_id`, `id`, `handle`, `version` (the watermark of
  its last write) and its fields; `ops (org_id, seq, kind, id, op, record, by, at)`; retention.
- **Endpoints:** bootstrap (scoped by subscription), `changes?since=N`, write (with base
  `version`; the answers are accepted with new version + seq + final handle, stale with the
  current row, or refused with the reason), the push channel and its heartbeat.
- **The app:** how the replica is stored (behind `CrmStore`; `AppState` stays pure), how a write
  waits for the server, what a stale or offline refusal looks like at the CLI and in the snapshot,
  and how a teammate's op becomes an `update` for the window.
- **Every existing write verb** mapped to its server write, including `move`, `link`, `archive`,
  `done`, `set` on a task, and `import` (a batch, applied as one op set or refused as one).
- **What the snapshot gains:** `sync: { state: online|offline|syncing, lastSeen }` on the seat,
  and enough of the org's members (`id → name`) to attribute a teammate's work.

## 4. Who builds what

**Backend**
- The contract (step 1, then review).
- Server: record tables and the `ops` log (migrations); bootstrap, changes, write with the
  version check, server-assigned handles, push + heartbeat. **Every query takes `org_id` from the
  session, never from the request.**
- App: the sync client, the persisted replica and watermark, writes confirmed before
  acknowledgement, the offline refusal, and turning teammates' ops into `update`s.
- "Bring my local records": an upload that goes through the same write path, with handle
  collisions resolved by the server.

**Frontend**
- A sync indicator: online, syncing, offline. Offline means the window is **read-only, and says so**.
- A refused stale write shows what the record says now and who changed it. The person's draft is
  kept, not thrown away.
- A teammate's work is attributed by name and disc, everywhere attribution appears. A
  teammate's change gets the same brief highlight W1 gives an agent's, in the teammate's colour.
- The join-org choice: bring my records, or start from the org's.

**Release**
- The server as a **container image**, plus a `compose` file (server + PostgreSQL) a self-hoster
  can run with one command.
- `docs/self-host.md`: run it, back it up, upgrade it, and what we never see of their data.
- A staging deploy target, written up but **not rented**. The product owner rents the server
  when p3 passes QA.

**QA**
- The live two-person test (§5), on two seats against a real server.

## 5. Acceptance — two people, one board

- [ ] **Convergence:** two seats in one org. Each makes 50 mixed writes (both people and their
      agents, every write verb) at the same time. Afterwards both replicas and the server are
      byte-identical in record data.
- [ ] **Live:** Ana moves a deal and Berk's board shows it within 2 s, attributed to Ana. With
      the push channel killed, within 30 s (the heartbeat).
- [ ] **Conflict:** both edit the same deal's value from the same version. One wins; the other gets
      a refusal naming the current value and who set it. Nothing is silently lost.
- [ ] **Handles:** both create "Acme" at the same moment. Two records, two handles, and each CLI
      printed its own.
- [ ] **Offline:** pull the network. Reads work, writes are refused with exit 1 and a clear
      message, and the window says offline. Reconnect and catch up without a full bootstrap.
- [ ] **Restart:** restart the server with 10 seats connected. None of them re-bootstraps.
- [ ] **Tenancy:** a seat in org A cannot read or write a row of org B through any endpoint, with
      any id it can guess. QA tries every endpoint.
- [ ] **The boundary holds:** no view state on the wire (p2's test, extended to the new endpoints).
- [ ] **Local mode** is unchanged, and the join choice never merges without the person saying so.
- [ ] `cargo test`, the server tests on PostgreSQL, `npm test` and `npm run verify` all green, with counts.

## 6. Do not

- **Do not restrict visibility.** That's p4. But don't hard-code "whole org" outside the
  subscription.
- **Do not queue offline writes.**
- **Do not merge fields.** Last-write-wins on the row, with a version check.
- **Do not put view state on the wire.**
- **Do not signal my agent about a teammate's work.**
- **Do not rent or deploy a server.** Write the target up; the product owner decides.
- **Do not start code before the contract is reviewed.**
