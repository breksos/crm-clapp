#!/usr/bin/env bash
#
# docs/release-plan.md §4 — the install check. Run by a person, on a Mac with Clatch
# installed, after `release.yml` has staged a draft release. Never run by CI: no runner
# has Clatch (the constraint at the top of the plan), which is exactly why this exists as
# a script someone runs rather than a step nobody can automate.
#
# clatch validate → clatch install → clatch run → crm status → clatch stop →
# crm status (must fail again) → clatch uninstall. Playbook §7: verify against a real
# launcher, not the build — this drives the REAL Clatch, the same one a user has.
#
# Usage:
#   scripts/release-check.sh <tag> [macos-arm64|macos-x64]
#   RELEASE_CHECK_HOME=/private/tmp/crmchk scripts/release-check.sh <tag>   # isolated, see step 1
#
# Downloads the named tag's draft release assets with `gh`, so it needs `gh auth login`
# and read access to the release (drafts are only visible to people with repo access,
# which is the point — nobody else can run this against an unpublished release).
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

die() { printf '\033[31mFAIL\033[0m  %s\n' "$*" >&2; exit 1; }
ok()  { printf '\033[32m  ok\033[0m  %s\n' "$*"; }
step(){ printf '\n\033[1m%s\033[0m\n' "$*"; }

[ $# -ge 1 ] || die "usage: $0 <tag> [macos-arm64|macos-x64]"
tag=$1
target=${2:-}

command -v gh >/dev/null 2>&1 || die "gh is not on PATH"
command -v clatch >/dev/null 2>&1 || die "clatch is not on PATH — this check needs a real install of Clatch"

# Pick the target from this machine's own arch when the caller did not name one, so the
# common case — checking the release on the machine that will actually receive it — needs
# no argument. A foreign-arch check (x64 depot on Apple Silicon) still has to be asked for
# explicitly, because `clatch install` from a file does not itself assert the arch matches.
if [ -z "$target" ]; then
  case "$(uname -m)" in
    arm64)  target=macos-arm64 ;;
    x86_64) target=macos-x64 ;;
    *) die "unrecognised uname -m '$(uname -m)' — pass the target explicitly" ;;
  esac
  printf 'no target given — using this machine'\''s arch: %s\n' "$target"
fi

app_id=$(node -e '
  const m = JSON.parse(require("fs").readFileSync("clatch.json", "utf8"));
  process.stdout.write(m.id);
')
cli=$(node -e '
  const m = JSON.parse(require("fs").readFileSync("clatch.json", "utf8"));
  process.stdout.write(m.connector.cli);
')

work=$(mktemp -d)
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

# ---------------------------------------------------------------------------------
step "1/7  download the draft's assets"
asset="$app_id-$target.clapp"
gh release download "$tag" -R breksos/crm-clapp -D "$work" \
  -p "$asset" -p "$asset.sha256" \
  || die "could not download $asset (+.sha256) for tag $tag — is the release still a draft, and do you have access to it?"
ok "downloaded $asset"

# ---------------------------------------------------------------------------------
# RELEASE_CHECK_HOME=<short dir>: run everything below under a fresh, empty home.
#
# `clatch run` starts the real app on the real data directory, and the app's launch sweep
# signals any overdue task to the agents on the machine (M4). A check that wakes someone's
# agent about their own data is not a check anyone should run on a machine that has both.
# A fresh home has no data and no agents, so the launcher gets exercised and nothing else
# does. It is also a closer stand-in for a clean machine than a developer's own.
#
# It comes AFTER the download on purpose: `gh` finds its token through the login keychain,
# which is located via $HOME, so it cannot authenticate from a fake home — and this script
# will not carry a token around to make it. Keep the path short: the daemon's socket path
# lives under it and Unix socket paths top out near 104 bytes.
if [ -n "${RELEASE_CHECK_HOME:-}" ]; then
  [ "${#RELEASE_CHECK_HOME}" -le 60 ] || die "RELEASE_CHECK_HOME is too long for a Unix socket path (${#RELEASE_CHECK_HOME} > 60)"
  mkdir -p "$RELEASE_CHECK_HOME"
  export HOME="$RELEASE_CHECK_HOME"
  export PATH="$HOME/.clatch/bin:$PATH"   # the shim clatch installs, ahead of any real one
  printf '\033[33mnote\033[0m  isolated: clatch and %s run under HOME=%s, not your real home\n' "$cli" "$HOME"
fi

# ---------------------------------------------------------------------------------
step "2/7  the checksum matches"
( cd "$work" && shasum -a 256 -c "$asset.sha256" ) || die "checksum mismatch on $asset"
ok "$asset.sha256 verifies"

# ---------------------------------------------------------------------------------
step "3/7  clatch validate on the unzipped download"
depot="$work/depot"
mkdir -p "$depot"
( cd "$depot" && unzip -q "$work/$asset" ) || die "could not unzip $asset"
clatch validate "$depot" || die "clatch validate rejects the downloaded depot — see above"
ok "the downloaded depot passes the contract"

# ---------------------------------------------------------------------------------
step "4/7  clatch install, from the file"
already_installed=0
if clatch ls 2>/dev/null | awk '{print $1}' | grep -qx "$app_id"; then
  already_installed=1
  printf '\033[33mnote\033[0m  %s is already installed on this machine — this check will leave it installed either way, but it is not a clean-machine install\n' "$app_id"
fi
clatch install "$work/$asset" || die "clatch install failed on $asset"
ok "clatch install"

# ---------------------------------------------------------------------------------
step "5/7  clatch run, then crm status round-trips"
clatch run "$app_id" || die "clatch run failed"
status=""
for _ in $(seq 1 150); do
  if status=$("$cli" status 2>/dev/null); then break; fi
  status=""
  sleep 0.1
done
[ -n "$status" ] || die "$cli status never answered after clatch run"
printf '%s' "$status" | grep -qi running || die "status did not say running: $status"
printf '%s\n' "$status" | sed 's/^/      /'
ok "clatch run -> $cli status round-tripped"

help1=$("$cli" -h | head -1) || die "$cli -h failed on the installed binary"
printf '      %s\n' "$help1"
ok "$cli -h answers from the installed binary"

# ---------------------------------------------------------------------------------
step "6/7  clatch stop, then the negative smoke test again"
clatch stop "$app_id" || die "clatch stop failed"
sleep 1
if out=$("$cli" status 2>&1); then
  die "$cli status succeeded after clatch stop: $out"
fi
printf '%s' "$out" | grep -q "not running" || die "the failure after stop did not say 'not running': $out"
ok "$(printf '%s' "$out" | head -1)"

# ---------------------------------------------------------------------------------
step "7/7  cleanup"
if [ "$already_installed" -eq 0 ]; then
  clatch uninstall "$app_id" || die "clatch uninstall failed"
  ok "uninstalled $app_id — this machine is as it was before this check"
else
  printf '\033[33mnote\033[0m  leaving %s installed — it was here before this check started\n' "$app_id"
fi

printf '\n\033[32mrelease-check: %s (%s) installs, runs, and answers through the real launcher.\033[0m\n' "$tag" "$target"
printf 'Publish the draft only after this has passed for every depot the release ships.\n'
