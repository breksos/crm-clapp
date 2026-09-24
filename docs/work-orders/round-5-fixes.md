# Work order — round 5 fixes

**Owners:** backend **and** frontend · **Branches:** `r5-backend`, `r5-frontend`, both from
`main` after [`k0`](k0-clappkit-update.md) lands · **Source:** [`docs/qa/round-5.md`](../qa/round-5.md)

> **Both agents read this whole file.** The reminder findings span both surfaces: the core
> publishes `snapshot.reminders` and the window reads none of it. Briefed apart, each fixes
> half — round 2 already taught us that.

---

## 1. BACKEND, and it is the most serious defect this project has produced

**The app acknowledges a write and then loses it.**

```
$ crm add company "Flush Test"
    added company flush-test          ← acknowledged, exit 0
$ clatch stop  →  clatch run
$ crm find flush --kind all
    no results (page 1 of 0 total)    ← gone
```

I reproduced this independently. The store writer is debounced at `SAVE_QUIET` = 400 ms and
**nothing flushes it on exit**. The writer's own comment says a closed channel means "the app is
going away — write immediately", and nothing ever closes it: the process just ends.

This has been latent since M1 and survived five QA rounds, because nobody had quit the app
within 400 ms of a write. QA met it chasing a different symptom — a reminder delivered twice,
which is the same bug in another costume.

**Fix:** flush on exit. Close the channel and wait for the writer, on the `app.shutdown` path and
on `crm close`'s grace window. Then pin it with a test that writes, shuts down immediately, and
reads back.

**A CRM that loses a write it confirmed is not shippable.** Everything else in this order is
smaller than this.

## 2. BACKEND — a refused reminder is lost forever, and we cannot detect refusals

`clappkit::Control` never surfaces `app.toAgentRefused`. I checked: it does not use
`clapp_pipe::Client` at all, and its own notification loop handles the roster and silently drops
everything else — the comment says *"the roster; refusals"* and only the roster branch exists.
There is **no workaround that detects a refusal**: `Control` retains nothing, and a second
control connection is impossible (one endpoint per instance, one-time token).

**So stop the harm instead of chasing the detection.** The damage is not that we cannot know —
it is that the core marks a task told *at emit time* and can never unmark it, so a refused
reminder is gone for good while `crm status` reports "last sent just now".

- Record **`sent_at`**, not `told`. Emitting is not delivering.
- `crm status` says so plainly: *"reminder sent 10:42 — the platform does not confirm delivery"*.
- Provide a way to **send it again**. The person knows whether their agent acted; we do not.
- Keep `note_refusal` and the `take_refusals` seam in place, wired to nothing, with a comment
  naming the clappkit gap — so it becomes live the day the accessor exists.

## 3. BACKEND — smaller, from round 5

- **`crm find` hides results behind a sticky *query* it never mentions.** Round 3 fixed the sticky
  *kind* filter; I under-ordered it and the same defect survives one field over. Same treatment:
  a filtered empty result names every filter in force and how to clear it.
- **`crm set <handle> value ""` silently erases a deal's value.** Refuse it, or require an explicit
  `--clear`. A destructive write must not look like a typo.
- The remaining teaching gaps QA lists in §1.

## 4. FRONTEND — the window is blind to the timer

The core publishes `snapshot.reminders`; `grep` finds no component reading `reminders`,
`lastSweepAt`, `refusal`, `backlog` or `awaiting`. The window still shows M3's generic sentence.

Read it, and show: when the last sweep ran, what is waiting, the upgrade backlog, and — once
backend lands §2 — anything sent but unconfirmed, with the re-send control beside it. The desk
panel is the obvious home.

Also: **the preview's Nia avatar is a flat teal disc.** Real avatars are photos; a flat disc reads
as a token, so the fixture invents a collision the product does not have. Same rule as the ULID
fixtures — the harness lies in the direction of the truth. Photo-like image, or the monogram.

## Acceptance

- [ ] a write, an immediate shutdown and a relaunch round-trip the record — pinned by a test
- [ ] a reminder delivered once stays once across an immediate stop and relaunch
- [ ] `crm status` distinguishes sent from delivered; a reminder can be sent again
- [ ] a filtered empty `find` names every filter in force
- [ ] `set value ""` cannot silently erase
- [ ] the window shows last sweep, waiting, backlog and unconfirmed sends
- [ ] `cargo test` and `npm test` counts reported; `npm run verify` green
- [ ] neither agent edited the other's tree

## Do not

- **Do not edit `clappkit/`.**
- **Do not weaken the debounce to "fix" §1.** The debounce is correct; the missing flush is the bug.
- **Do not invent a delivery confirmation the platform does not give us.**
