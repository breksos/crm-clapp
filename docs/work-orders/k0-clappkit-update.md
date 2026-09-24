# Work order — K0: catch up with clappkit, and make staleness visible

**Owner:** backend · **Branch:** `k0-clappkit` · **Then:** release adds the freshness gate
**Blocks:** nothing, but do it first — it changes the ground everything else stands on

---

## What happened

Our submodule is pinned at `b5a3aab` and upstream is **14 commits ahead**. Nobody noticed for
three weeks, and the reason is structural: CI runs `submodules: recursive`, which checks out
**the pinned commit**. A stale pin and a fresh pin look identical to every gate we have. No
check, no reminder, nobody's job.

Among the 14 are **K1–K5**, which are security-shaped:

- K1 — local-file safety across `store`, `paths` and the avatar bridge
- K2 — env-driven execution and the one-time token no longer leak
- K3 — framing and transport no longer trust a length prefix or a squatted name
- K4 — CI runs the whole workspace; `clapp-pipe` gets its first tests
- K5 — `install_root`'s ascent is bounded, so a distant `clatch.json` cannot masquerade as the root

Also: a Windows `unused_mut` fix, the dock-icon binary gated on the `icon` feature, and doc work.

## The job

1. **Update the submodule to upstream `main`.** Commit the new pin.
2. **Rebuild everything and report real numbers** — `cargo test`, `clippy`, `cargo fetch --locked`
   with no ssh key, `npm test`, `tsc`, `npm run verify`.
3. **Read the 14 commits, not just the titles.** K1–K3 touch `store`, `paths`, the avatar bridge,
   framing and transport — all of which we call. If any changed a signature, a default or a
   behaviour we depend on, say so explicitly rather than only fixing the compile error.
4. **Re-check the two known gaps against the new pin.** Neither was fixed upstream when I looked
   (`Control` still has no refusals; `AGENT_TINTS` unchanged), but confirm it against what you
   actually check out, and say so in the report — the M11 tint test should still pass, and if it
   fails, that is the test doing its job.

## Acceptance

- [ ] submodule pinned to upstream `main`, new SHA in the commit message
- [ ] every gate green, counts reported
- [ ] a short note per K-commit that touches code we call: what changed, and whether it affects us
- [ ] the M11 `tints.test.ts` hash-pin still passes, or its failure is explained
- [ ] `npm run pack` produces a depot and `clatch validate` accepts it

## Do not

- **Do not edit `clappkit/`.** Update the pin; that is all.
- **Do not fix the round-5 findings here.** Separate order, separate branch.
