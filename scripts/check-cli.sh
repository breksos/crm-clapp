#!/usr/bin/env bash
#
# The packaged CLI really is a console program that waits and prints (win1 §3.3).
#
#   scripts/check-cli.sh [pkg-dir]
#
# On Windows the trap is the `windows_subsystem = "windows"` attribute: the image is two
# roles, and a GUI-subsystem process gets no console and is not waited on by the `.cmd` shim
# Clatch links onto the agent's PATH — so EVERY CLI call returns instantly, empty, with
# exit 0 (clappkit/src/role.rs). Nothing else fails: the window opens, the tests pass, and
# the agent is told nothing. So this runs the packaged binary the way the shim does and
# insists on what a console program does:
#
#   * no `windows_subsystem` attribute anywhere in our Rust source,
#   * the PE header says CONSOLE (subsystem 3), not GUI (2)      — Windows binaries only,
#   * `-h` prints the real manual and exits 0,
#   * a bad flag prints a refusal on stderr and exits 2,
#   * `status`, with no app running, exits 1 with our own "not running" sentence.
#
# Reads connector.cliBin out of the DEPOT's manifest, never a guessed path (playbook §12).
# OS-neutral on purpose: it runs on the macOS depot too, where only the PE check is skipped.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
pkg=${1:-pkg}

die() { printf '\033[31mFAIL\033[0m  %s\n' "$*" >&2; exit 1; }
ok()  { printf '\033[32m  ok\033[0m  %s\n' "$*"; }

[ -f "$pkg/clatch.json" ] || die "$pkg/clatch.json does not exist — package first"
rel=$(node -e 'const m=JSON.parse(require("fs").readFileSync(process.argv[1]));process.stdout.write(m.connector.cliBin)' "$pkg/clatch.json")
app_id=$(node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync(process.argv[1])).id)' "$pkg/clatch.json")
bin="$pkg/$rel"
[ -f "$bin" ] || die "connector.cliBin '$rel' is not in $pkg"

# --- 1. the attribute, in source ------------------------------------------------------
if grep -rn -E '#!?\[\s*(cfg_attr\([^]]*)?windows_subsystem' src-tauri/src src-tauri/build.rs 2>/dev/null; then
  die "windows_subsystem is set in our source — see the comment in clappkit/src/role.rs"
fi
ok "no windows_subsystem attribute in src-tauri"

# --- 2. the PE header, in the binary --------------------------------------------------
case "$bin" in
  *.exe)
    sub=$(node scripts/pe-subsystem.mjs "$bin")
    case "$sub" in
      3) ok "PE subsystem is CONSOLE (3)" ;;
      2) die "PE subsystem is GUI (2): every CLI call would return instantly, empty, exit 0" ;;
      notpe) die "$rel is not a PE executable" ;;
      *) die "PE subsystem is $sub, expected 3 (CONSOLE)" ;;
    esac ;;
  *) ok "not a PE file — subsystem check does not apply" ;;
esac

# --- 3. -h: the real manual, exit 0 ---------------------------------------------------
err=$(mktemp); trap 'rm -f "$err"' EXIT   # outside pkg/: a stray file there would be packed
set +e
out=$("$bin" -h 2>"$err"); code=$?
set -e
[ "$code" = 0 ] || die "\`-h\` exited $code, expected 0: $(head -c 300 "$err")"
lines=$(printf '%s\n' "$out" | wc -l | tr -d ' ')
[ "$lines" -ge 30 ] || die "\`-h\` printed $lines line(s) — a GUI-subsystem binary prints nothing. Got: '${out:0:200}'"
printf '%s\n' "$out" | head -1 | grep -q 'Breksos CRM' || die "\`-h\` did not start with the manual's title: '$(printf '%s' "$out" | head -1)'"
ok "-h prints the manual ($lines lines) and exits 0"

# --- 4. a bad flag: refusal on stderr, exit 2 ------------------------------------------
set +e
bout=$("$bin" --not-a-flag 2>"$err"); bcode=$?
set -e
[ "$bcode" = 2 ] || die "\`--not-a-flag\` exited $bcode, expected 2"
[ -z "$bout" ] || die "a refusal wrote to stdout: '${bout:0:100}'"
grep -q 'is not a verb' "$err" || die "a refusal said nothing on stderr: '$(head -c 200 "$err")'"
ok "a bad flag is refused on stderr with exit 2"

# --- 5. status with no app: our sentence, exit 1 ---------------------------------------
if "$bin" status >/dev/null 2>&1; then
  die "an instance of the app is already running — stop it, or this test proves nothing"
fi
set +e
"$bin" status >/dev/null 2>"$err"; scode=$?
set -e
[ "$scode" = 1 ] || die "\`status\` with no app running exited $scode, expected 1"
ok "status with no app running exits 1 (the CLI reached its socket/pipe and was refused)"
