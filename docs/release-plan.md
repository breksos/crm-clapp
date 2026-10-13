# Release plan — M5, package and distribute

**Status:** proposal for the PM to approve. Nothing here is built yet. · **Owner:** release ·
**Answers:** [`work-orders/release-r1.md`](work-orders/release-r1.md) §3

The contract this plan answers to: [`format.md`](../clappkit/docs/format.md) § The package
and § Distribution, and [`playbook.md`](../clappkit/docs/playbook.md) §5–§9 and §12–§12b.
Where this plan and those documents disagree, they win.

**One constraint runs through the whole plan.** Clatch is a private launcher with no public
binary, so **no GitHub runner can run `clatch validate`, `clatch pack` or `clatch install`.**
Every check that needs the launcher happens on a machine that has Clatch installed, between
"CI built it" and "the release is published." The design below makes that gap a named
step with an owner, so nobody has to remember it.

---

## 1. Which depots v1 ships

**Proposal: `macos-arm64` and `macos-x64`. Windows and Linux are deferred until each is
built *and* tested.**

`clatch.json` declares only `launch.macos`, and every OS key is a promise that a depot
exists (format.md § Which platforms you owe). Two macOS depots keep that promise without
making a new one. `package.sh` already refuses to finish if `launch` names anything but
`macos`, so the manifest cannot drift ahead of the release by accident.

| host | asset it asks for | v1 ships it? |
|---|---|---|
| Apple Silicon Mac | `com.breksos.crm-macos-arm64.clapp` | yes |
| Intel Mac | `com.breksos.crm-macos-x64.clapp` | yes |
| Windows x64 | `com.breksos.crm-windows-x64.clapp` | **yes, since win1** — built on a native Windows runner; its depot declares `launch.windows` and no other OS key. Not yet QA'd on real Windows 11 |
| Windows on ARM, Linux | — | no. No `windows-arm64`, no `-windows-any` (format.md: `-any` is a claim about the bytes), and no depot's `launch` claims them |

**What an Intel Mac gets if we ship arm64 only:** nothing. Asset selection matches on
`-<os>-<arch>` first. Rule 2 needs a `-macos-any` asset and rule 3 needs a markerless one,
and we would ship neither. So `clatch install breksos/crm-clapp` fails with "no match"
and lists the assets the release has. It fails loudly, not with a crash. Rosetta does not
help, because selection happens before anything is executed. It is still a user who cannot
install, so the x64 depot belongs in v1.

**How the x64 depot is built.** A native Intel runner (`macos-15-intel`) runs the same
`package.sh` and the same smoke test on the arch it ships. Cross-compiling on arm64 would
need a `--target` path that `package.sh` does not read today (`target/release/crm` is
hardcoded), and its smoke test would run under Rosetta instead of on real Intel hardware.
GitHub has announced an end date for its Intel macOS images. When that date arrives, this
job moves to a cross-build on arm64 with the smoke test under Rosetta, and the plan says so
at that point.

**An alternative the PM may prefer:** one `-macos-any` depot holding a universal2 binary
(`lipo` of both builds). Rule 2 explicitly allows this, because one binary really does serve
both arches. It means one asset instead of two, and a depot roughly twice the size. I
recommend two per-arch depots: that is the family's convention, each depot is smoke-tested
on its own arch, and a broken x64 build cannot ride inside an arm64 user's download.

**Deferred, with what each would cost:**

- **Windows x64.** *Lifted by [win1](work-orders/win1-windows.md); see the win1 section at the end of this plan.* What it needed, as predicted: `icons/icon.ico` (present), an MSVC host assertion, `script-shell bash`, and a Windows arm in `package.sh`.
- **Linux.** No family precedent, and a WebKitGTK dependency on the user's machine.

## 2. What `release.yml` does on a `v*` tag

```
on: push tags v*   (+ workflow_dispatch with an existing tag, to rebuild without moving one)
permissions: contents: write     # the only job in the repo that gets it
```

A matrix with one job per depot:

| job | runner | asset |
|---|---|---|
| arm64 | `macos-14` | `com.breksos.crm-macos-arm64.clapp` + `.sha256` |
| x64 | `macos-15-intel` | `com.breksos.crm-macos-x64.clapp` + `.sha256` |

Each job runs these steps:

1. **Checkout** with `submodules: recursive` and `persist-credentials: false`. No secret and
   no key, same as `verify.yml`.
2. **Assert the tag is the manifest's version.** `v${clatch.json .version}` must equal the
   tag. A `v0.2.0` tag on a `0.1.0` manifest fails before anything is built.
3. **`cargo fetch --locked`, then `cargo test`**, with `TZ=UTC` as in `verify.yml`.
4. **`npm ci`, `npm test --if-present`, `npx tsc --noEmit`, `npm run build`,
   `bash scripts/package.sh`.** This is the same path as `verify`. There is no second recipe
   for release.
5. **The pre-publish gates** (§3 below).
6. **Pack.** `zip -r -X` from inside `pkg/`, so the archive is rooted at `clatch.json` and
   uses deflate only, the one method the reader accepts. `clatch pack` would be the
   canonical tool, but it is not available on a runner. `.DS_Store` is excluded.
7. **Checksum.** `shasum -a 256 <asset> > <asset>.sha256`. The first field is the lowercase
   hex digest, as format.md requires.
8. **Upload to a DRAFT release** for the tag. The job creates the draft if it is missing and
   uploads with `--clobber`, so a re-run replaces its own assets.

One more job runs after both finish:

9. **Cross-depot manifest check.** Download both assets and unzip each `clatch.json`. Assert
   they are byte-identical once `launch.macos` and `connector.cliBin` are removed, and that
   those two fields are identical to each other within each depot. format.md requires the
   manifest to match across every depot of a version, except the per-platform paths.
   Because both depots are macOS, those paths should even match between depots. The check
   asserts that too, so a drift in `package.sh` shows up here.

**The release stays a draft.** `clatch install <owner>/<repo>` resolves *latest*, and a
draft is never latest, so nobody can install assets that have not passed §4. The PM
publishes the draft by hand after §4 passes. That keeps releasing a PM decision, as the work
order requires, and leaves room for the checks a runner cannot make.

## 3. What runs before an asset is published

In CI, on each depot, before upload:

| gate | where | what it catches |
|---|---|---|
| safe-segment assertion on `connector.cliBin`, `launch.macos`, `icon` | `package.sh` (exists) | a path the launcher would refuse at install. Our display name has a space (§12b) |
| every manifest path resolves inside `pkg/`, `cliBin` executable | `package.sh` (exists) | a depot that lies about its files |
| `launch` names `macos` only | `package.sh` (exists) | an OS key with no depot |
| `crm -h` offers only declared verbs | `verify.sh` step 5 | a verb the agent is told about with no grant behind it |
| **negative smoke test**: `crm status` fails, says "not running", and names `com.breksos.crm` | `verify.sh` step 6, run on the **packaged** binary | proof that the CLI role dialled its socket and the two-surface wiring survived packaging |
| `file` on the binary reports the job's arch | new, in `release.yml` | an arm64 binary inside the x64 asset |
| no symlinks in `pkg/` | new, in `release.yml` | playbook §1 |

On a machine with Clatch, before the draft is published (§4):

| gate | what it catches |
|---|---|
| `clatch validate` on the **unzipped downloaded asset** | everything the contract checks. It runs on the file users will receive, not on the local `pkg/` |
| `sha256` of the download matches its `.sha256` | a truncated or replaced upload |

`clatch validate` cannot run in CI (see the constraint at the top). Today `verify.yml` skips
it explicitly, with a warning in the log. The release process makes up for that by running
it on the downloaded asset, which is a stronger check than validating `pkg/` would be.

## 4. The install check

This is `scripts/release-check.sh <tag>`, proposed for M5 and run by release on an Apple
Silicon Mac with Clatch installed. It covers the arm64 depot. The x64 depot needs an Intel
Mac, or at least Rosetta; see §5.

```sh
gh release download <tag> -R breksos/crm-clapp -p '*macos-arm64.clapp*'   # the draft's assets
shasum -a 256 -c <asset>.sha256
unzip -q <asset> -d depot && clatch validate depot
clatch install ./<asset>                  # the real launcher, the real unpack (playbook §7)
clatch run com.breksos.crm                # must reach a registered state
crm status                                # round-trips against the INSTALLED app
crm -h | head -1                          # the manual comes from the installed binary
clatch stop com.breksos.crm
crm status                                # must fail again: "not running"
clatch uninstall com.breksos.crm
```

It leaves the machine as it found it, with one exception: `clatch uninstall` keeps
settings, and `~/.clatch/appdata/com.breksos.crm` survives by design. The script says so
and does not purge it, because a developer's real data may live there. That points to an
open question: running the check under a disposable `HOME` would keep it away from real
data, but Clatch's daemon lives under `HOME`, so that needs testing first.

After the script passes, the PM publishes the draft. After publishing, the same script run
with `clatch install breksos/crm-clapp` (no `./`) proves the GitHub route resolves the
right asset.

## 5. What is not covered, and what that costs

**Signing and notarisation.** A `.clapp` has no signature by design (format.md § The
package). The `.sha256` proves the bytes arrived intact, not who made them. The binary
inside `crm.app` is still a macOS executable, so it is still subject to macOS code-signing
rules. What I found on this machine (macOS 26.2, Apple Silicon, Clatch 0.4.5-stage.17):

- **The binary is ad-hoc signed and nothing more.** On arm64, the linker signs every binary
  it produces (`flags=adhoc,linker-signed`), and `package.sh` copies that signature intact.
  The kernel requires at least this on Apple Silicon, so the binary runs. The *bundle* is not
  sealed: `codesign` finds no resource seal, and `spctl --assess --type execute` **rejects**
  `crm.app`. The installed sibling `Chess.app` shows exactly the same result.
- **Gatekeeper only assesses quarantined files.** A file gets the `com.apple.quarantine`
  flag when a quarantine-aware app, such as a browser or Mail, downloads it. The Clatch
  binary contains `xattr -dr com.apple.quarantine` and the log line "cleared the quarantine
  flag on", so the launcher clears the flag on what it installs. The sibling apps installed
  on this machine carry no quarantine flag, only `com.apple.provenance`. **So a Clatch-run
  `crm.app` launches without a Gatekeeper prompt.** That is the launcher's behaviour, not
  ours, and it can change under us.
- **What happens outside Clatch.** Suppose someone downloads the `.clapp` with a browser,
  unzips it with Archive Utility (which carries the quarantine flag onto the extracted
  files), and double-clicks `crm.app`. macOS blocks it and says Apple could not verify it.
  Since macOS 15, right-click → Open no longer bypasses that; the user has to go to System
  Settings → Privacy & Security → Open Anyway. The supported path, `clatch install
  ./file.clapp`, goes through the launcher's quarantine clearing instead. That is the path
  the README should tell people to use.
- **What I could not verify:** a truly *fresh* Mac. I checked one development machine that
  had already run clapps. Whether `clatch install` from a browser-downloaded `.clapp` clears
  quarantine *before* the first launch, and not after, needs a clean macOS user account or
  VM. That test belongs in the first `release-check.sh` run.
- **x64 signing.** Intel binaries need no signature to run, so an unsigned x64 binary
  behaves like the arm64 one under Clatch. The same outside-Clatch caveat applies.

**What signing would cost:** an Apple Developer ID ($99/year), a certificate and a
notarisation credential stored as **repository secrets**, and a macOS signing step. That
breaks "the workflow needs no secret" (playbook §8), and the release depends on a
credential that expires. I recommend deferring it, as long as Clatch clears quarantine, and
revisiting it if the app is ever offered outside Clatch.

**Also not covered in v1:**

- **Windows and Linux.** See §1. `launch` does not claim them, so no user is promised them.
- **An automated install check.** §4 is a script a person runs, because a runner cannot
  have Clatch. A green `release.yml` therefore means "built, packaged and smoke-tested," not
  "installs." The draft-then-publish step is what stops the first from being mistaken for
  the second.
- **The x64 install check** needs an Intel Mac, or at minimum a Rosetta run of the x64
  depot on Apple Silicon. On Apple Silicon, `clatch install` would choose the arm64 asset,
  so the x64 depot has to be installed from a file (`clatch install ./…-macos-x64.clapp`),
  and whether Clatch accepts a foreign-arch depot is untested.
- **An agent pass on the installed build.** The QA charter's "drive it from `crm -h`
  alone" is M2's to close. Release only confirms that the installed `crm -h` is the one
  that was built.

## Open questions for the PM

1. **Two per-arch macOS depots, or one universal `-macos-any`?** I recommend two (§1).
2. **Is draft-then-publish-by-hand acceptable** as the release gate, given that a runner
   cannot run Clatch? The alternative is a self-hosted Mac runner with Clatch installed,
   which would bring back a machine and credentials to maintain.
3. **Signing and notarisation:** defer while Clatch clears quarantine (my recommendation),
   or budget for a Developer ID now?
4. **`verify.yml` skips `clatch validate`** on the runner, with a warning. Is an explicit,
   logged skip acceptable there, given that §3–§4 run it on the real asset before publishing?
5. **Who owns the fresh-Mac test** in §5? It needs a clean macOS account or VM that nobody
   on the team has named yet.

## PM decisions — 2026-09-17

The plan is **approved**. The five questions, answered:

1. **Two per-arch depots**, `macos-arm64` and `macos-x64`, each smoke-tested on its own arch.
   The reasoning in §1 holds: it is the family convention, and a broken x64 build must not ride
   inside an arm64 user's download. Revisit only when GitHub retires its Intel images.
2. **Draft-then-publish-by-hand is the gate.** Releasing stays a product-owner act, and we do
   not take on a self-hosted runner and its credentials for a v1. The PM publishes a draft only
   after the §4 install check has passed on a real Clatch.
3. **Signing and notarisation are deferred** — with the dependency written down rather than
   forgotten: our "no Gatekeeper prompt" rests on Clatch clearing the quarantine flag. If Clatch
   ever stops doing that, this decision reopens the same day. Revisit before any distribution
   that does not go through Clatch.
4. **The logged skip of `clatch validate` in CI is acceptable**, because §3–§4 run it on the real
   asset before anything is published. The skip must stay loud: a warning in the job summary,
   never a silent pass.
5. **The fresh-Mac check is owned by QA, on a clean macOS user account the product owner
   creates once** on this machine. A new account is a clean `~/.clatch`, a clean keychain and a
   clean quarantine history, without the cost of a VM. Creating the account is the product
   owner's action; running the check in it is QA's.

---

## Round 2 — 2026-09-23: the branch-push gate, and the release pipeline built

Two things closed this round. Both are in `release-r2`, off `main` at `0740b0d`.

### 6. The branch-push gate — closing what QA round 3 flagged

QA round 3 named the gap precisely: `verify.yml` triggers on every push with no branch
filter, but in practice a specialist's branch was merged into `main` **locally**, and only
`main` was ever pushed. CI checked the merge result after the fact; it never had the chance
to check the branch before it landed, because it never saw the branch. Not a defect in
`verify.yml` — the workflow was always willing to run on a branch push. Nobody asked it to.

**The new flow:**

1. A specialist finishes their branch and pushes it: `git push -u origin <branch>`. This
   was always possible; the fix is making it the step nobody skips, not a new capability.
2. `verify.yml` runs on that push, unchanged — it always would have.
3. **Before merging**, the PM runs `scripts/require-ci-green.sh <branch>` (also
   `npm run release:gate -- <branch>`). It refuses to pass unless:
   - the local branch has no commits `origin/<branch>` lacks — i.e. it really was pushed,
     not just committed, closing the exact gap QA found; and
   - the most recent `verify` run for that branch's current commit is `completed` with
     conclusion `success`, quoting the run URL.
4. Only then does the PM run the local merge (`git merge --no-ff <branch>`, with the same
   hand-written merge message this project already uses) and push `main`.

Tested against real Actions runs on `breksos/crm-clapp`: pointed at `release-r1`'s own
pushed, green commit, it passes and prints that run's real URL. Given a branch with a
commit `origin/<branch>` does not have, it refuses with a named fix (`git push origin
<branch>`) rather than silently checking a stale commit. Given a branch that does not
exist on the remote at all, it says so.

**What this is not:** a server-side rule. Branch protection with a required status check
would enforce the same thing without anyone having to remember to run a script — but
changing repository settings is the product owner's call, not this tree's (`scripts/`,
`.github/`, this file, and `package.json`'s `scripts` block — a branch-protection rule is
none of those). Recorded here as a real option, not implemented: **enable "Require status
checks to pass" for `verify` on `main`, once someone with repo-admin access decides to.**
Until then, `require-ci-green.sh` is the gate, and it is only as good as remembering to run
it.

### 7. The release pipeline — what actually shipped

`.github/workflows/release.yml`, on push of a `v*` tag (or `workflow_dispatch` with an
existing tag, to re-stage without moving one). Three jobs, each depending on the last, so a
failure anywhere upstream means nothing downstream ever runs — not a note in a log, a real
dependency:

1. **`depot`** (matrix: `macos-arm64` on `macos-14`, `macos-x64` on `macos-15-intel`, both
   native — no cross-compiling, matching §1). Asserts the tag matches `clatch.json`'s
   version (`scripts/assert-tag-matches-manifest.sh`), fetches keylessly before the Rust
   cache restores (same ordering as `verify.yml`, same reason), then runs `npm run verify`
   itself — build, `cargo test`, the window's tests, `tsc`, package to `pkg/`, the
   manual-vs-manifest check, and the negative smoke test, all against this job's own
   binary. That one call **is** §3's "declared verbs" and "negative smoke test" rows; there
   is no second implementation of either. Then: the packaged binary really reports the
   job's own arch (`scripts/assert-binary-arch.sh`, reads `connector.cliBin` from the
   depot's manifest, never a guessed path), no symlinks anywhere in `pkg/`
   (`scripts/assert-no-symlinks.sh`, playbook §1), pack to `<id>-<target>.clapp` +
   `.sha256` (`scripts/pack-depot.sh` — zip only, deflate only, `.DS_Store` excluded,
   standing in for `clatch pack`, which no runner can call), uploaded as a build artifact.
   **Nothing is uploaded to a release yet.**
2. **`cross-check`** (needs both `depot` jobs): downloads both packaged manifests, and
   `scripts/compare-depot-manifests.sh` asserts they agree outside `launch.macos` /
   `connector.cliBin`, and — since both v1 depots are macOS — that those two fields also
   agree *between* the depots, so a `package.sh` drift between the two matrix jobs shows up
   here rather than shipping.
3. **`stage`** (needs `cross-check`): creates the tag's release as a **draft** if it does
   not exist, or refuses outright if it exists and is **not** a draft — a re-run must never
   `--clobber` assets onto something already published — then uploads both `.clapp` files
   and their `.sha256`s, and posts a `::notice::` spelling out that the release is a draft
   and naming the next step.

`scripts/release-check.sh <tag> [macos-arm64|macos-x64]` is §4, built as proposed:
downloads the draft's assets with `gh`, verifies the checksum, `clatch validate`s the
unzipped depot, `clatch install`s from the file, `clatch run`s it, round-trips `crm
status` and `crm -h` against the **installed** binary, `clatch stop`s it, proves the
negative smoke test again post-stop, and `clatch uninstall`s — unless the app was already
installed before the check started, in which case it says so and leaves it alone rather
than uninstalling something that was already there. Defaults to this machine's own arch
when none is given.

**What is verified, and what is not.** Every script above (`assert-tag-matches-manifest`,
`assert-binary-arch`, `assert-no-symlinks`, `pack-depot`, `compare-depot-manifests`) was
run against a real `pkg/` built on this machine, exercising both its pass and its fail
path — including a real `clatch validate` on the packed depot, which passed. `release.yml`
itself has not run end to end: that needs a real `v*` tag, and the order for this round is
explicit that cutting one is not this round's decision to make. `release-check.sh` needs a
real draft release to download from, for the same reason. Both are syntax-checked
(`ruby -ryaml` parses `release.yml` cleanly) and built from pieces that were tested
individually — that is a narrower claim than "this has run," and it is the honest one.

---

## M5 — 2026-09-24: v0.1.0, the first tagged build

`v0.1.0` was tagged on `main` at `4310bfa` and `release.yml` ran for the first time
([run 35942325614](https://github.com/breksos/crm-clapp/actions/runs/35942325614)): both
`depot` jobs, `cross-check` and `stage` succeeded. The release is a **draft**; nothing has
been published. It is `main`'s head at that commit, so any later merge is not in it.

| asset | bytes | sha256 |
|---|---|---|
| `com.breksos.crm-macos-arm64.clapp` | 3,376,182 | `aa62d691d229c0a34f008e9e3bdfd9c7bd13c6a866ff0bc03db1c6ca41153e1a` |
| `com.breksos.crm-macos-x64.clapp` | 3,484,539 | `1ae904668694a83c189c844ea962b4e4431fa035a86c5d81400d45beca9e77ac` |

Each `.sha256` verifies its asset, and both match the digest GitHub computed on upload.

**§3, run on the downloaded assets rather than on a local `pkg/`:**

- `clatch validate` on each depot's own manifest: valid, both.
- Safe-segment assertion on `connector.cliBin`, `launch.macos` and `icon`, using
  `package.sh`'s own function: safe, both. Our display name has a space; the bundle
  directory is `crm.app`, and `Info.plist` carries `Breksos CRM` in full (§12b). The same
  function rejects `bin/Breksos CRM.app/…`, so the check can fail.
- Negative smoke test on the packaged binary: `crm status` exits 1 and says
  `crm: app is not running — start it with `clatch run com.breksos.crm``. The arm64 binary ran
  natively; the x64 binary ran here under Rosetta. Natively, it ran on the Intel runner
  (`file` reports `x86_64`), where `npm run verify` passed its full round-trip.
- The zip is rooted at `clatch.json`, holds only stored and deflated entries, and both depots'
  manifests agree apart from what the format allows.

**§4, on the arm64 depot, with the real launcher:** `clatch install` from the built asset,
`clatch run`, `crm status` round-tripped, `crm -h` answered from the installed binary,
`clatch stop`, `crm status` failed with "not running", `clatch uninstall`. Passed.

**How §4 was run, and why.** It ran under an isolated `HOME`, not the developer's own. The
developer's home holds real CRM data with an overdue task that has never been signalled, and
live agents; `clatch run` there would have sent a real `task.due` signal from the launch sweep.
`scripts/release-check.sh` now takes `RELEASE_CHECK_HOME=<short dir>` for this. It downloads
with the real environment first, because `gh` finds its token through the login keychain, which
is located via `$HOME`. An isolated home is also a cleaner machine than a developer's.

**Not verified:**

- **The x64 install check.** No Intel Mac was available. What exists is the native run of
  `verify` on the Intel runner and a Rosetta smoke test here.
- **A first launch from a browser-downloaded file.** Neither depot was quarantined on this
  machine, so Gatekeeper's behaviour is still the fresh-account check owned by QA (decision 5).
  The arm64 binary is linker-signed (ad hoc) only, with no team identifier; the x64 binary is
  not signed at all, which is normal for Intel.
- **`clatch install breksos/crm-clapp`.** The GitHub route resolves only published releases, so
  it cannot be tried until the PM publishes.

---

## p2 — the server in CI and in `verify` (`p2-release`)

[`p2-identity.md`](work-orders/p2-identity.md) §6 gives release three jobs.

- **CI has a PostgreSQL.** `verify.yml` gains a `server` job on Linux with a `postgres:16`
  service container, because GitHub's `services:` containers do not run on macOS runners. The
  job first proves the database answers (`psql … select version()`), then runs
  `scripts/server-test.sh`. The macOS job is unchanged apart from `VERIFY_WITHOUT_SERVER=1`,
  which turns its server step into a loud warning rather than a silent pass.
- **`npm run verify` runs the server's tests** as step 3 (`npm run test:server` runs them alone).
  The database comes from `scripts/pg-dev.sh`, in order: `DATABASE_URL` if set, a throwaway
  Docker `postgres:16`, a throwaway local cluster (`initdb` + `pg_ctl`), else a failure that says
  what to install. Throwaway databases listen on 127.0.0.1 only and are removed on exit.
- **`npm run dev:server`** starts that database and `cargo run`s the server against it, on
  `CRM_SERVER_ADDR` (default `127.0.0.1:8787`).

**Until `server/` exists** the server step skips with a warning. From the moment
`server/Cargo.toml` exists it cannot skip: tests must run, and a missing database is a failure.

**Assumptions the backend should confirm or change** — they are conventions I chose because the
crate does not exist yet: `server/` is a standalone Cargo package with its own `Cargo.lock`
(not a workspace member of `src-tauri`); it reads `DATABASE_URL` and `CRM_SERVER_ADDR`; it runs
its own migrations on start and its tests create their own schema or database inside the one
`DATABASE_URL` gives them.

---

## `release-gates` — the clappkit freshness gate (k0's "then")

`npm run verify` step 1 (`npm run check:clappkit` alone) fails when the clappkit commit this
repository **records** is behind what its upstream has published. CI checks out the pinned
commit, so before this a stale pin and a fresh one were indistinguishable to every gate we have;
k0 found it 14 commits behind, four of them security fixes, after three weeks.

- **The reference** is upstream's newest release tag (`v1.2.3` or `1.2.3`), and while upstream has
  published none — true today — the tip of its default branch. Without that fallback the gate
  would pass vacuously and would not have caught the incident it exists for.
- **Behind** fails and lists the commits missing, with the command to update. **Ahead of the
  release** passes and says so. **Diverged**, or a pin upstream has never had, fails.
- **No network:** a laptop warns and passes; CI (`GITHUB_ACTIONS` or `CI` set) fails.
- It reads `clappkit/` never; the comparison happens in a throwaway repository.

**Consequences worth knowing.** While there are no tags, an upstream push turns CI red on every
branch with no code change on ours, and `release.yml` (which runs `verify`) cannot cut a release
until the pin is bumped. That is the point, and also the cost. When upstream starts tagging, the
reference moves to the tag: commits after the newest tag stop being flagged, so untagged security
fixes would go unnoticed again — ask upstream to tag them.

---

## p3 — the server as an image, and `docs/self-host.md` (`p3-release`)

[`p3-sync.md`](work-orders/p3-sync.md) §4 asks release for a container image, a compose file and
a self-hosting guide. They are in [`deploy/`](../deploy) and [`self-host.md`](self-host.md).

- **Image:** `deploy/server.Dockerfile`, built from the repository root because the server
  depends on `crates/identity`. Debian slim, unprivileged user, no secrets at build time. It is
  built from source by whoever runs it; **nothing is published to a registry** — that is a later
  decision, like the staging server.
- **Compose:** server + `postgres:16`; the server on `127.0.0.1:8787` only, PostgreSQL not
  published, no default password (the file refuses to start without one), read-only server
  filesystem, no capabilities.
- **CI:** an `image` job in `verify.yml` builds it and runs the stack: health, an unauthenticated
  call refused, migrations applied, not root, database port closed. It deploys nothing.
- **The stream:** `GET /v1/sync/stream` must not be buffered by a proxy. The server sets
  `X-Accel-Buffering: no` (contract §3.5); `self-host.md` also gives Caddy and nginx settings.
  The stream does not exist on `main` yet, so CI cannot check the header and says so.
- **The staging deploy target** (p3 §4) is **not written** — see the report; it is a decision about
  where, not a file.


---

## win1 — a Windows x64 build, through CI (`win1-release`)

[`win1-windows.md`](work-orders/win1-windows.md) lifts §1's Windows deferral. Release's part is
here; the backend's part is the list in "What assumes Unix", below.

### What release built

- **One packaging script.** `scripts/package.sh` now picks its arm from `uname`: macOS exactly
  as before (a test packaged the macOS depot before and after the change and the five files'
  hashes, `clatch.json` included, were identical), and under Git Bash on a Windows runner a bare
  `bin/crm.exe` with `launch.windows` and **no other OS key**. `clatch.json` itself is untouched:
  the Windows depot's manifest is derived from it, the way the macOS one is.
- **CI.** `verify.yml` gains a `windows` job on `windows-latest`: MSVC host assertion, the keyless
  `cargo fetch --locked`, `cargo test`, `npm test`, `tsc`, `npm run build`, the package, an
  architecture/console check, and `scripts/check-cli.sh`. It does **not** take the macOS
  round-trip — there is no window to open on a runner — and `clatch validate` is skipped with a
  warning, as on macOS. The macOS jobs are not changed.
- **Release.** `release.yml` gains `windows-x64` in its matrix, so a tag builds **three** depots,
  each with a `.sha256`, into the one draft. The cross-check compares every depot with the first:
  macOS against macOS must match exactly, macOS against Windows may differ in the OS key and
  `connector.cliBin` and nowhere else. `pack-depot.sh` falls back to `7z` and `sha256sum`, which
  are what Git Bash has instead of `zip` and `shasum`.
- **The subsystem trap, pinned.** `scripts/check-cli.sh` fails unless: no `windows_subsystem`
  attribute exists in our source; the PE header says CONSOLE (read from the file, not trusted);
  `-h` prints the manual (30+ lines) and exits 0; a bad flag is refused on stderr with exit 2;
  and `status` with no app exits 1. It was run against a silent exit-0 binary and a lenient one
  to make sure it fails them. `assert-binary-arch.sh` also reads `(console)` out of `file`.

### What was and was not verified

- **Verified here (macOS):** the macOS depot is byte-identical; the Windows arm of `package.sh`
  (driven with a fake `uname` and a stand-in `.exe`) produces `bin/crm.exe`, `launch.windows`
  only, and a source manifest that is unchanged; the PE parser on synthetic console/GUI/Mach-O/
  truncated headers; `check-cli.sh`'s pass and fail paths; the comparer on mac/mac, mac/windows,
  drift, and a two-OS-key depot.
- **Not verified until the first Windows run:** that anything compiles or passes on Windows;
  the `7z` and `sha256sum` branches; that `file` exists in Git Bash; that `npm run build`
  (tauri, MSVC) works on the runner. **`clatch validate` on a Windows depot cannot run on a Mac**
  — a macOS Clatch answers "no launch command for this OS (macos)" — so that acceptance item
  needs a Windows machine with Clatch.

### What assumes Unix (for the backend; release touches no `src-tauri`)

Found by reading `src-tauri/`, `crates/identity/` and the clappkit paths we depend on. `AppState`
(`state.rs`, `state/split.rs`) has **no `cfg` other than `cfg(test)`**, and no `windows_subsystem`
exists anywhere: both acceptance items already hold.

| # | Where | What assumes Unix | Why it matters on Windows | Severity |
|---|---|---|---|---|
| 1 | `main.rs:668` → `account.rs:214` → `clappkit::store::atomic_write` | `session.json` is born `0600` only under `#[cfg(unix)]` (`clappkit/src/store.rs:98`); clappkit says plainly it sets **no owner-only ACL** on Windows (`paths.rs:15`) | The token is protected only by whatever `%LOCALAPPDATA%\crm` inherits from the user profile (normally owner, SYSTEM, Administrators). Must be **verified and pinned by a Windows-only test** — a token readable by other users is a MAJOR | **MAJOR until verified** |
| 2 | `account_tests.rs:97` | The only test of `session.json`'s mode is `#[cfg(unix)]` (`0o600`) | Windows has no equivalent test, so item 1 is unpinned | needed for #1 |
| 3 | `main.rs:652` (`seat.json`), `replica.rs:32` (`replica-<org>.sqlite` + sidecars) | Same directory, same inheritance, no mode set | Same assumption as #1 for the seat identity and the org's records | verify with #1 |
| 4 | `cli.rs:1553` | `#[cfg(unix)]` directory `fsync` after the export rename | A no-op on Windows (a directory cannot be opened as a file). clappkit argues NTFS journals the rename, so this is probably fine, but it is a durability claim — **backend to decide and write it down** | low |
| 5 | `cli.rs:~1545` `std::fs::rename(&tmp, &target)` in `write_file_atomically` | A bare `rename` over an existing target | On Windows this fails with a sharing violation if anything has the target open — Excel with last week's export open is the common case. clappkit's own `rename_over` retries three times (20/50/120 ms); ours does not, and the person gets a raw OS error | medium |
| 6 | `cli.rs:~1526` `std::fs::canonicalize(path)` | Returns a path as the OS spells it | On Windows that is the verbatim form `\\?\C:\…`, which `export` then **prints to the agent** as the file path | low–medium |
| 7 | `cli.rs:~1560` `render_export_csv` | Rows end in `\n` | Excel reads LF, and nothing here is wrong, but RFC 4180 says CRLF; decide | low |
| 8 | `cli_tests.rs:1477–1547` | Four `#[cfg(unix)]` tests/blocks (a read-only directory refused, `/dev/null` refused, a symlink followed, permissions preserved) | They do not run on Windows, so the export writer's refusals, symlink behaviour and permission preservation have **no Windows coverage** | needs Windows twins or an accepted gap |
| 9 | `cli.rs:917–923` | `chrono::Local` for the local offset | Reads the OS zone; the `TZ` environment variable is a Unix convention. The core is handed the clock, so this is only the edge — but a Windows runner is always UTC and cannot reproduce the "+03" class of due-date bug | note |
| 10 | `clappkit/src/ipc.rs:38–42` (upstream, not `src-tauri`) | The Unix socket sits in a `0700` directory; the Windows named pipe has a **machine-wide default DACL** and the `-<user>` suffix "does not make the channel private" | Another local user on the same Windows machine (a terminal server, a shared PC) could connect to the pipe and send CLI verbs to someone else's running app. Not ours to fix; **report upstream** | medium, shared machines |

Checked and **clean**: no hard-coded `~/.crm` or `$HOME` anywhere in `src-tauri` (the two mentions
are doc comments); the data directory comes from `CLATCH_DATA_DIR`, else clappkit's
`%LOCALAPPDATA%\crm`; no `Command::new`, no signals, no `libc`; no string splitting on `/`;
`rusqlite` is `bundled` so there is no system SQLite to find; `icons/icon.ico` exists.

Release-side Unix assumptions (all mine, handled above): `package.sh`'s `iconutil`/`sips`/`.app`,
`pack-depot.sh`'s `zip`/`shasum`, `assert-binary-arch.sh`'s Mach-O spelling, the comparer's
`launch.macos`, and `verify.sh`, which stays macOS-only (its round-trip needs a window).

### Signing

**What an unsigned `crm.exe` triggers.** *From general knowledge of Windows, not from testing on
a clean machine — QA's Windows 11 run is where this becomes fact.*

- **SmartScreen** shows "Windows protected your PC / Unknown publisher" for an unsigned
  executable that carries the *Mark of the Web* (a browser download's `Zone.Identifier`). "Run
  anyway" gets past it. Reputation builds with downloads and time, for signed and unsigned alike.
- **Smart App Control** (Windows 11, on by default on a fresh install, in "enforce" mode) goes
  further: it blocks an **unsigned** binary that has no reputation, with no "run anyway", and turning
  it off is a one-way switch. This is the bigger risk for an unsigned app on a new PC.
- **Whether either fires depends on Clatch.** The `.clapp` itself carries no signature by design.
  If Clatch extracts it with its own code, the files it writes have no Mark of the Web and
  SmartScreen may never see them — the same reasoning as the macOS quarantine flag (§5), and the
  same dependency: if Clatch ever starts tagging what it installs, this reopens. Smart App Control
  judges the binary on launch regardless of how it arrived. **Find out on a clean Windows 11 VM
  with Smart App Control on.** That is the test that decides whether signing is optional.

**What signing would need** (indicative, from memory, not quotes — check each vendor today):

| route | needs | rough cost | notes |
|---|---|---|---|
| **Organisation (OV) certificate** | A verified legal entity; the private key must live on a hardware token or a cloud HSM (CAs have required this since 2023) | ~$200–500 / year | Signs now; SmartScreen reputation still builds with use |
| **EV certificate** | Same, with stricter vetting | ~$300–700 / year | No longer buys instant SmartScreen trust the way it once did; its remaining value is stricter vetting |
| **Microsoft's cloud signing service** (Trusted/Artifact Signing) | An Azure subscription and identity validation; eligibility has been limited by country and years of business history | ~$10 / month | No key to hold; CI authenticates by OIDC rather than a stored secret |
| **A free signing programme for open source** | An open-source project that meets the programme's terms | $0 | Check whether a proprietary-licence-free repo qualifies (ours declares Apache-2.0 in Cargo.toml) |

**What it costs the pipeline either way:** a signing step in `release.yml` (`signtool sign` with an
RFC 3161 timestamp, or the cloud service's action) on the Windows leg only; **a credential in CI**
— the one thing `verify.yml` and `release.yml` have deliberately never had (playbook §8) — unless
OIDC to a cloud service is used; and an expiry date somebody owns. Sign `crm.exe` *before* it is
zipped; the `.clapp` stays unsigned. The macOS builds are unaffected.

**Recommendation:** ship v1 unsigned **only if** the clean-VM test shows Clatch's install path
neither triggers SmartScreen nor trips Smart App Control; otherwise the cheapest honest route
is the cloud signing service. **Nothing has been bought or applied for. That is the product
owner's decision**, and the answer to "does Clatch tag what it installs" belongs to the Clatch
team.
