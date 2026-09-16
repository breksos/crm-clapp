# Work order — release, round 1: the gates, then a plan for M5

**Owner:** release · **Branch:** `release-r1`, from `main` · **Source:**
[`docs/qa/round-2.md`](../qa/round-2.md) § Process

Read [`CLAUDE.md`](../../CLAUDE.md), [`architecture.md`](../architecture.md) §12, and
[`playbook.md`](../../clappkit/docs/playbook.md) §5–§9 and §12–§12b before touching anything.
Those sections are the afternoons other clapps already lost.

**Your tree:** `scripts/`, `.github/`, and the `"scripts"` block of `package.json`. Nothing
else — `src-tauri/` is the backend's, `src/` is the frontend's, and `clatch.json` is the PM's.

---

## 1. `npm run verify` must run the frontend's tests

QA round 2: the window has guard tests (`npm test`) and a type check, and **a green verify says
nothing about either.** Add both to `scripts/verify.sh`:

```sh
npm test --if-present
npx tsc --noEmit
```

`--if-present` because `main` has no `test` script until M3 merges — the gate must not break
the branch that does not have it yet, and must not stay silent once it does.

Report a failure the way the existing steps do: name the step, show the output, exit non-zero.

## 2. CI, so the gates run somewhere that is not a developer's laptop

Add `.github/workflows/verify.yml`, on every push and pull request, on macOS:

- `cargo fetch --locked` **with no SSH key available** — playbook §8: the honest check is a
  runner that has no key, so a dependency that reached for one fails right there
- `npm ci`, then `npm run verify`
- **`TZ=UTC` explicitly.** The core computes due dates in local time, and the regression it
  guards against is invisible on a developer machine in `+03`. A UTC runner is the one that
  catches it.
- `cache-on-failure: true` on the Rust cache, so a failed run still seeds the next (§9)

**There is no GitHub remote yet**, so this cannot run until the product owner creates
`breksos/crm-clapp`. Write it, check its syntax, and say plainly in your report that it has
never executed. A workflow nobody has run is a claim, not a gate.

## 3. A written plan for M5 — for the PM to approve, not to execute

M5 is packaging and distribution. Before anyone builds it, write `docs/release-plan.md`
answering:

1. **Which depots does v1 ship?** `clatch.json` declares only `launch.macos`, and every OS key is
   a promise that a depot exists (`format.md` § Which platforms you owe). The proposal to judge:
   `macos-arm64` and `macos-x64`, with Windows and Linux deferred until they are built *and*
   tested. Say what an Intel Mac gets if we ship arm64 only.
2. **What does `release.yml` do on a `v*` tag?** Per-arch depot, a `.sha256` beside each, and the
   manifest identical across depots except the per-platform `launch` and `cliBin`.
3. **What runs before an asset is published?** At minimum: `clatch validate` on the depot's own
   manifest; the safe-segment assertion on every `cliBin` and `launch` component (our display
   name has a space — §12b); and the negative smoke test on the packaged binary.
4. **What is the install check?** `clatch install` from the built asset, `clatch run`,
   `crm status`, then uninstall. Playbook §7: verify against a real launcher, not the build.
5. **What is not covered, and what that costs.** Signing and notarisation: a `.clapp` carries
   no signature by design, but the binary inside a macOS `.app` is still subject to
   Gatekeeper. Find out what an unsigned `crm.app` does on a fresh Mac and write it down.

## Acceptance

- [ ] `npm run verify` runs `npm test` (when present) and `tsc --noEmit`, and fails loudly
- [ ] `verify.yml` exists, passes a syntax check, sets `TZ=UTC`, and is reported as never run
- [ ] `docs/release-plan.md` answers all five questions, including what it does not cover
- [ ] `npm run verify` green on `main`
- [ ] nothing outside your tree touched

## Do not

- **Do not add a `launch` key for Windows or Linux.** An OS key is a promise.
- **Do not publish, tag or push.** There is no remote, and releasing is a PM decision.
- **Do not change `clatch.json`.**
