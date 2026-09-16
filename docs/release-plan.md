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
| Windows, Linux | — | no, and `launch` does not claim them |

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

- **Windows x64.** Needs `icons/icon.ico`, an MSVC host assertion, `script-shell bash`,
  and `.exe` rewrites in `package.sh` (playbook §9). A cold build takes about 15 minutes.
  `src-tauri/` also has no WebView2 runtime check yet, which is the backend's work.
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
