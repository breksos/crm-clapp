# Work order — M4: the due-task timer

**Owner:** backend · **Branch:** `m4-timer`, from `main` after round 3 lands
**Blocks:** M5

Read [`CLAUDE.md`](../../CLAUDE.md), [`architecture.md`](../architecture.md) §8, and
[`clappkit/docs/architecture.md`](../../clappkit/docs/architecture.md) § Always-on apps.

---

## Why this is the milestone that matters

Everything shipped so far is a CRM with two surfaces. **`task.due` is the only signal that
wakes an agent** — the one place the app reaches out on its own instead of answering. Without
it, the agent is a tool the person drives; with it, the app participates.

## The platform constraint, and it is not negotiable

**Clatch ships no scheduler and starts no app at boot.** A timer exists only while the app is
running. Missed-schedule policy is ours to write, and the honesty about it is ours to surface.

## What to build

**The loop.** While the app runs, check for tasks that have come due. Check on a threshold, not
a tight clock — a due date changes at most once a day, so a sweep every few minutes is ample and
a per-second timer is a bug. `clock()` is already read once per command; reuse that shape.

**The launch sweep.** On startup, find everything that came due while the app was closed and
emit **one consolidated `run`**, not one per task. Twelve `run` signals on launch is how an
agent's inbox becomes useless.

**Emit once per task, ever.** A task that has fired must not fire again on the next sweep or
the next launch. Persist what has been signalled; `Task` already round-trips through the store.

**Fan-out is all-or-nothing.** If any bound agent cannot accept a `run`, Clatch refuses the
whole emission and answers `app.toAgentRefused`. **Surface that to the person** — a
full-inbox agent otherwise reads as a dead button. Show it in the window near the due indicator,
and in `crm status`.

**Only the timer signals here.** A task coming due is not a human action, and this is the one
sanctioned exception in `architecture.md` §8 — the clock-clapp pattern. A CLI write still emits
nothing.

## The honesty the app owes

The window already says reminders need the app open. Now that the timer is real, that line must
be **true and specific**: say when the last sweep ran, and say plainly that anything due while
the app was closed is reported at next launch rather than when it happened. A person who misses
a follow-up and then discovers this is a person who stops trusting the app.

## Tests

- [ ] a task due in the past fires once and never again, across a restart
- [ ] tasks that came due while closed produce **one** consolidated signal on launch
- [ ] a completed task never fires
- [ ] an archived record's tasks never fire
- [ ] the sweep is threshold-based — a test asserts no signal storm across a simulated day
- [ ] a refused emission is recorded and surfaced, not swallowed
- [ ] local dates decide "due", not UTC — the existing `local_date(at, offset)` tests extend here

## Acceptance

- [ ] `task.due` fires against a real Clatch-bound agent, verified live, not only in tests
- [ ] the launch sweep consolidates
- [ ] a refusal is visible in the window and in `crm status`
- [ ] the reminders caveat states the real behaviour
- [ ] `cargo test` count reported; `npm run verify` green
- [ ] push the branch so CI runs before review

## Do not

- **Do not add a scheduler, a daemon, or anything that runs when the app does not.** The
  platform has none and neither do we.
- **Do not poll a continuous value.** Threshold, not clock.
- **Do not emit a signal for an agent's own write.**
- **Do not touch `src/`.**
