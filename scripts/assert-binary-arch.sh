#!/usr/bin/env bash
#
# The one bug a per-arch release matrix can hide: the x64 job accidentally ships an arm64
# binary (a stale cache, a misconfigured cross-target) and nobody notices until an Intel
# Mac fails to launch it. `file` reads what the linker actually wrote, so it is the check
# that does not trust the job that built the thing to have built it correctly.
#
# Reads connector.cliBin out of the DEPOT's own manifest, never a guessed path — the same
# rule package.sh follows (playbook §12).
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

die() { printf 'assert-binary-arch: %s\n' "$*" >&2; exit 1; }

[ $# -eq 2 ] || die "usage: $0 <pkg-dir> <expected-arch: arm64|x86_64>"
pkg=$1
expected=$2

case "$expected" in
  arm64|x86_64) ;;
  *) die "expected-arch must be 'arm64' or 'x86_64', got '$expected'" ;;
esac

[ -f "$pkg/clatch.json" ] || die "$pkg/clatch.json does not exist — run scripts/package.sh first"

cli_bin=$(node -e '
  const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  const p = m.connector && m.connector.cliBin;
  if (!p) process.exit(3);
  process.stdout.write(p);
' "$pkg/clatch.json") || die "$pkg/clatch.json has no connector.cliBin"

bin="$pkg/$cli_bin"
[ -f "$bin" ] || die "connector.cliBin '$cli_bin' does not exist in $pkg"

report=$(file -b "$bin")
printf '%s: %s\n' "$cli_bin" "$report"

# A universal binary lists every slice `file` found; a thin one names its own. Either way,
# the word for the expected arch must appear — `x86_64` is not a substring of `arm64` or
# vice versa, so a plain grep is exact enough here.
# `file` spells the same architecture differently per format: Mach-O says x86_64, a Windows
# PE says x86-64. Accept either spelling of the one architecture asked for.
pattern=$expected
[ "$expected" != x86_64 ] || pattern='x86_64|x86-64'
printf '%s' "$report" | grep -Eqw "$pattern" \
  || die "$cli_bin does not report '$expected': $report"

# A Windows executable must be a CONSOLE program: a GUI one makes every CLI call return
# instantly, empty, exit 0 (clappkit/src/role.rs). `file` says which, in parentheses.
case "$report" in
  *PE32*) printf '%s' "$report" | grep -q '(console)' \
            || die "$cli_bin is a Windows executable but not a console program: $report" ;;
esac

printf '%s carries %s, as this job promised\n' "$cli_bin" "$expected"
