# Work order — WIN1: a Windows build, through CI

**Owners:** release (lead: CI/CD, packaging, the release), backend (only the platform seams in
`src-tauri`, if release finds any), QA
**Branches:** `win1-release`, and `win1-backend` only if needed
**Builds on:** [`release-plan.md`](../release-plan.md) §1, which deferred Windows. **This order
lifts that deferral.**

Read [`format.md`](../../clappkit/docs/format.md) (§ launch, § Which platforms you owe, the asset
grid), [`protocol.md`](../../clappkit/docs/protocol.md) (the Windows named pipe), and
[`playbook.md`](../../clappkit/docs/playbook.md) before writing anything.

---

## 1. What this is

Ship **`com.breksos.crm-windows-x64.clapp`** beside the two macOS builds, built and tested by CI
on a real Windows runner, never cross-compiled. Same features, same `crm -h`, same data rules.
**Windows on ARM is out of scope:** no `windows-arm64` and **no `-windows-any`** (format.md:
`-any` is a claim about the bytes, not a wildcard).

## 2. What already works, so don't rebuild it

- **The app ⇄ CLI connection.** clappkit's `ipc` already uses a named pipe on Windows
  (`\\.\pipe\clapp-crm-<user>`) with no `cfg` in our code.
- **The control pipe to Clatch.** clappkit uses `\\.\pipe\clatch-<instanceId>` (protocol.md).
- **The WebView2 check.** clappkit's `webview::ensure` names the download instead of a bare error.
- **Atomic writes** go through `clappkit::store`.

## 3. What has to be done

**Release**
1. **Windows packaging.** Write `scripts/package.ps1`, or make `package.sh` run under Git Bash on
   the runner; release's choice, but **one** source of truth for the layout. The depot is
   `pkg/clatch.json` with **`launch.windows`** (exactly one OS key per depot, format.md),
   `bin/crm.exe` and `assets/`. `connector.cliBin` is valid for Windows and contains no space.
2. **CI.** Add a `windows-latest` job to `verify.yml`: `cargo fetch --locked`, `cargo test`,
   `npm test`, `npm run build` and the package. Add `windows-x64` to the `release.yml` matrix,
   so a tag builds **three** assets into the draft release with SHA-256s, as today.
   `clatch validate` runs where Clatch exists; on the runner it's skipped **loudly**, as on macOS.
3. **Don't set `windows_subsystem = "windows"`.** clappkit's `role.rs` explains why: it makes
   every CLI call return instantly, empty and with exit 0. Pin this with a CI check that runs the
   built `crm.exe -h` and requires real output and exit 0.
4. **Paths.** The data dir, `seat.json`, `session.json`, the replica and export paths must
   resolve under the user's profile (`%APPDATA%` / `%LOCALAPPDATA%`), never a hard-coded `~/.crm`.
   Find every place that assumes Unix and list it. Fixing them in `src-tauri` is backend's job.
5. **Signing.** Document what an unsigned `.exe` triggers (SmartScreen), and what code signing
   would cost and need. **Don't buy a certificate.** The product owner decides.
6. Update `release-plan.md` §1 and `CHANGELOG.md`.

**Backend** (only what release's list from step 4 needs)
- Platform seams in `src-tauri`: paths, the `#[cfg(unix)]` directory fsync, and anything else.
  **The core stays pure.** A `cfg` belongs at the edge (store, cli, account), never in `AppState`.
- **`session.json` must be readable by its owner only on Windows too.** A file in the user's
  profile inherits owner/SYSTEM/Administrators by default. Verify that, write it down, and pin
  it with a Windows-only test. A token readable by other users is a MAJOR.

**QA**
- On a **real Windows 11 x64** machine or VM, not only the CI runner: install the `.clapp` via
  Clatch for Windows, then run the demo script, a real agent, the 10-second test, export and
  import, and a restart.
- Check that CLI calls really wait and print. This is the subsystem trap.
- Check `session.json`'s permissions.

## 4. Acceptance

- [ ] CI is green on macOS **and** Windows on every push
- [ ] a `v*` tag yields **three** draft assets (`macos-arm64`, `macos-x64`, `windows-x64`), each with a SHA-256
- [ ] on the Windows runner, `crm.exe -h` prints the full manual and exits 0; a bad flag exits 2
- [ ] `clatch validate` passes on the Windows depot (locally, where Clatch exists)
- [ ] QA passes the live Windows run in §3
- [ ] no `cfg` in `AppState`; no `windows_subsystem` attribute anywhere

## 5. Do not

- Don't cross-compile. Use native runners only.
- Don't ship `-windows-any` or `windows-arm64`.
- Don't buy a signing certificate or publish a release.
- Don't change behaviour on macOS. Its CI and its builds must not move.
