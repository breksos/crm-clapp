# Work order — QA

**Owner:** QA · **Branch:** `qa-round-1` (findings only; you do not fix) · **Reviews:**
`m1-core`, `m6-brand`, `m3-window`

Read [`CLAUDE.md`](../../CLAUDE.md) and [`docs/architecture.md`](../architecture.md). Then
read the work order for each branch you are reviewing — **the acceptance checklist in each
one is your test plan**, and a box that cannot be ticked with evidence is a finding.

---

## Your mandate

**You verify. You do not fix.** A QA agent that repairs what it finds destroys the evidence
and hides a pattern — three sloppy commits from one agent is information the PM needs, and it
disappears if you quietly clean up after each one.

Produce a report. Rank findings by severity. For each: what you ran, what you expected, what
happened, and the file and line.

## Rule zero — never trust that a test exists

**Run it.** A test file that does not compile is worse than no test file, because it looks
like coverage in a diff and a reviewer's eye slides over it.

This is not hypothetical on this project. `cargo build` cannot see `#[cfg(test)]`
([`playbook.md`](../../clappkit/docs/playbook.md) field notes), so test code rots while
everything looks green. Start every branch review with the commands in `CLAUDE.md` and record
the actual output:

```sh
cargo test          # must COMPILE and pass — count the assertions that ran
cargo clippy --all-targets
cargo fetch --locked
npm run verify
```

If a claimed test never executed, the rule it was supposed to pin is **unverified**, and every
acceptance box that depended on it is a finding — not a pass with a note.

## The pass that matters most: be the agent

This is an agent-native app. Its primary user is not a person, and the usual QA pass will miss
everything that matters to it.

**Drive the CLI from `crm -h` alone, with the source closed.** Do not read `cli.rs` first. Do
not look at the work order's verb table. Open the manual the way an agent does, and try to do
real work with only what it tells you:

- Add a company, a contact and a deal. Log a call. Set a next step. Move the deal to
  proposal, then close it won.
- Find something. Open it. Page through a long list.
- Get something wrong on purpose — a stage that does not exist, a name matching two records,
  a required argument omitted.

Then answer, in the report:

1. **Could you complete each task using only the manual?** Anything you had to guess is a
   documentation defect, and on this app that is a product defect.
2. **Does every refusal teach?** An error that says what was wrong *and* what to do instead is
   the bar. "invalid argument" is a finding.
3. **Does an ambiguous name park a `pending` rather than guessing or erroring?** Architecture
   §6. A silent pick is the worst outcome; a bare error is the second worst.
4. **Is any verb in the manual missing, broken, or undocumented?** `clatch validate` checks
   the manifest and nothing checks that the code agrees with it — you are that check.

## The two surfaces must agree

The whole architecture exists to stop these drifting. Test the seam, not each side.

- **Every word the window shows, the CLI accepts.** Stage names especially: if the board draws
  a column labelled "Negotiation", `crm move <deal> negotiation` works and `crm -h` names it.
- **Pagination is shared.** Ask the CLI for 3 results and confirm the window's page did **not**
  become 3 rows. Confirm both surfaces say "N of TOTAL" about the same page, in the same
  words.
- **An agent write reaches the window**, and a human action reaches the agent as a signal —
  and **an agent is never signalled about its own write**.
- **One `rev` per moment.** The command response and the pushed snapshot must carry the same
  revision when they describe the same state.

## The rules with teeth

Check each against the code and against behaviour. Each is in
[`architecture.md`](../architecture.md) or the backend order:

- [ ] `AppState` contains no `std::fs`, no `std::net`, no `#[cfg(target_os)]`
- [ ] `pipeline_id` survives a store round-trip
- [ ] `move` to `won` sets the status and **preserves** the stage; moving a closed deal to a
      stage reopens it
- [ ] an archived record leaves the board, the counts and default `find`, and `show` still
      loads it
- [ ] an activity from an agent records `Actor::Agent` keyed on the **id**; from the person,
      `Actor::Human`
- [ ] currency totals group and are never summed across currencies
- [ ] **an empty pipeline prints `0.00`, never `-0.00`** — sum an empty `f64` list and you get
      `-0.0`; confirm the guard exists and a test pins it
- [ ] nothing secret is in the snapshot (nothing is secret in v1 — confirm the test exists so
      it stays true)
- [ ] a roster rename relabels in place and keeps the id

## The window — M3 and M6

M2 does not exist yet, so the window cannot be driven by real CLI data. **Review it through
`src/preview.ts`**, which renders it in a plain browser against a fake snapshot.

Design compliance is checkable, not a matter of taste. From
[`m3-window.md`](m3-window.md):

- [ ] **both themes**, plus the un-stamped system-default state in both OS appearances — every
      token defined in bare `:root` before any media or `[data-theme]` block redefines it
- [ ] fonts render **with networking off** (they are bundled; confirm no request to
      `fonts.googleapis.com`)
- [ ] table rows 32px; money columns use tabular numerals and line up
- [ ] board cards carry no more than three fields plus the attribution disc
- [ ] an agent-driven move rings the card in that agent's tint, keyed on agent id
- [ ] `prefers-reduced-motion` honoured; keyboard focus visible on every interactive element
- [ ] the "reminders need the app open" honesty line is present near the due indicator
- [ ] **no gradient, no shadow outside drag/popover, no emoji, no centred layout, nothing above
      6px radius, no `Inter`**
- [ ] no "ask the agent" button anywhere
- [ ] `preview.ts` covers all six required states

For M6, work the checklist in [`m6-brand.md`](m6-brand.md) — measure the icon fill rather than
eyeballing it, and check the banner **at 128px tall**, which is how the library draws it.

## The install path, not just the build

`npm run verify` proves the surfaces talk. Before you pass anything, prove it installs
([`playbook.md`](../../clappkit/docs/playbook.md) §7):

```sh
npm run pack
clatch install ./com.breksos.crm-*.clapp
clatch run com.breksos.crm
crm status
```

And the negative half, which is the better half:

```sh
# with the app NOT running
crm status     # must FAIL, with our own "not running" sentence
```

That proves the CLI role got as far as dialling its socket — that the two-surface wiring
survived packaging.

**Check the depot's own manifest**, not the repo's: every component of `connector.cliBin` and
`launch` must be a safe segment, `[A-Za-z0-9._-]`, no whitespace. Our display name has a space
in it and this is the exact bug [`playbook.md`](../../clappkit/docs/playbook.md) §12b was
written about. Confirm the bundle is `crm.app` and not `Breksos CRM.app`.

## Report format

```
SEVERITY  branch  file:line
  what you ran
  expected / got
  why it matters
```

**Blocker** — a rule in `architecture.md` is broken, a test does not run, packaging is
refused, or the two surfaces disagree.
**Major** — the manual does not let an agent do its job; an acceptance box cannot be ticked.
**Minor** — cosmetic, or a defect with an obvious workaround.

End with a plain verdict per branch: **pass**, or **send back**, and the one sentence that
decides it. Do not soften a send-back; a milestone that merges broken costs more than one that
goes back.

## Do not

- **Do not fix anything.** Report it.
- **Do not merge.** The PM merges after reading your report.
- **Do not touch `clappkit/`.**
- **Do not review M2.** It does not exist yet; the CLI verbs beyond `status`, `focus` and
  `close` are declared and deliberately refused.
