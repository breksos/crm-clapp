#!/usr/bin/env bash
#
# build → the window's gates → package → clatch validate → CLI ⇄ GUI round-trip.
#
# `clatch validate` reads the manifest and **nothing reads the code**. This script is what
# closes that gap: it drives the packaged binary the way an agent does and proves the two
# surfaces reach one state.
#
# It reads every path out of `pkg/clatch.json` rather than assuming `bin/crm` — the depot
# moves the binary into a `.app` on macOS, and a script that hardcodes a layout is wrong
# on one of the three platforms (playbook §12).
#
# The half that matters most is the NEGATIVE one: with the app not running, `crm status`
# must fail with our own "not running" sentence. That proves the CLI role got as far as
# dialling its socket, which is the only evidence that the two-surface wiring survived
# packaging (playbook §9).
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

pkg="pkg"
app_pid=""

green() { printf '\033[32m  ok\033[0m  %s\n' "$*"; }
step()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
die()   { printf '\033[31mFAIL\033[0m  %s\n' "$*" >&2; exit 1; }

# Run one gate quietly, and on failure name it, show everything it printed, and stop.
# A gate that fails behind `>/dev/null` leaves only an exit code, which tells nobody what
# broke — least of all a CI log read the next morning.
# What the gate printed stays in `gate_out`, so a step can quote its summary line.
gate_out=""
gate() {
  local name=$1
  shift
  if ! gate_out=$("$@" 2>&1); then
    printf '%s\n' "$gate_out" | sed 's/^/      /' >&2
    die "$name"
  fi
}

# `cargo test` prints one "test result:" line per test binary; sum them.
test_counts() { printf '%s\n' "$gate_out" | awk '
  /^test result:/ { for (i = 1; i <= NF; i++) {
      if ($(i) ~ /^passed/)  p += $(i - 1)
      if ($(i) ~ /^failed/)  f += $(i - 1)
      if ($(i) ~ /^ignored/) g += $(i - 1) } }
  END { printf "%d passed, %d failed, %d ignored", p, f, g }'; }

cleanup() {
  if [ -n "$app_pid" ] && kill -0 "$app_pid" 2>/dev/null; then
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

json() { node -e '
  const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  const at = process.argv[2].split(".").reduce((o, k) => (o ?? {})[k], m);
  process.stdout.write(at === undefined || at === null ? "" : String(at));
' "$1" "$2"; }

# ---------------------------------------------------------------------------------
step "1/8  build"
# `npm run build` and never a bare `cargo build`: without Tauri's `custom-protocol`
# feature the binary loads the dev URL and the window comes up white.
gate "npm run build" npm run build
green "npm run build"

# `cargo build` cannot see #[cfg(test)], so test code rots silently while everything
# looks green.
gate "cargo test" sh -c 'cd src-tauri && cargo test --quiet'
green "cargo test — $(test_counts)"

# A dependency that reached for an ssh key would fail authentication right here — which
# is the check a runner with no key can honestly make (playbook §8).
gate "cargo fetch --locked" sh -c 'cd src-tauri && cargo fetch --locked'
green "cargo fetch --locked"

# ---------------------------------------------------------------------------------
step "2/8  the window's gates"
# The window has its own guard tests and a type check, and until this step a green verify
# said nothing about either (QA round 2). `--if-present` because a branch without a `test`
# script must not break here — and a branch with one can no longer pass without running it.
gate "npm test" npm test --if-present
if node -e 'process.exit(require("./package.json").scripts?.test ? 0 : 1)'; then
  # `node --test` ends with "ℹ tests N", "ℹ pass N", "ℹ fail N" (or "# tests N" as TAP).
  green "npm test — $(printf '%s\n' "$gate_out" | awk '$1 ~ /^(ℹ|#)$/ && $2 ~ /^(tests|pass|fail)$/ { printf "%s%s %s", sep, $3, $2; sep=", " }')"
else
  printf '\033[33mnote\033[0m  no `test` script on this branch, so there were no window tests to run\n'
fi
gate "tsc --noEmit" npx --no-install tsc --noEmit
green "tsc --noEmit"

# ---------------------------------------------------------------------------------
step "3/8  package"
gate "scripts/package.sh" bash scripts/package.sh
green "pkg/ assembled"

cli=$(json "$pkg/clatch.json" connector.cli)
app_id=$(json "$pkg/clatch.json" id)
cli_bin=$(json "$pkg/clatch.json" connector.cliBin)
launch=$(json "$pkg/clatch.json" launch.macos)

[ -n "$cli_bin" ] || die "the depot manifest carries no connector.cliBin"
[ "$cli_bin" = "$launch" ] || die "launch.macos ($launch) and connector.cliBin ($cli_bin) disagree"
bin="$root/$pkg/$cli_bin"
[ -x "$bin" ] || die "$cli_bin is not executable in the depot"
green "cliBin read from the depot manifest: $cli_bin"

# ---------------------------------------------------------------------------------
step "4/8  clatch validate"
# Clatch is a private launcher, so a CI runner has no `clatch` to call. Rather than skip
# quietly wherever it happens to be missing, the skip is opt-in and says so in the log:
# the workflow sets VERIFY_WITHOUT_CLATCH=1, and on a developer machine a missing clatch
# is still a failure. package.sh has already asserted the safe-segment rule on every
# path the manifest points at — the one rule a launcher enforces that bit us before.
if command -v clatch >/dev/null 2>&1; then
  gate "clatch validate" clatch validate "$pkg"
  green "the depot passes the contract"
elif [ "${VERIFY_WITHOUT_CLATCH:-}" = "1" ]; then
  msg="clatch validate NOT RUN — no clatch on this machine (VERIFY_WITHOUT_CLATCH=1). Validate locally before any release."
  printf '\033[33mskip\033[0m  %s\n' "$msg"
  [ -z "${GITHUB_ACTIONS:-}" ] || printf '::warning title=clatch validate skipped::%s\n' "$msg"
else
  die "clatch is not on PATH — install Clatch, or set VERIFY_WITHOUT_CLATCH=1 to skip this one step on purpose"
fi

# ---------------------------------------------------------------------------------
step "5/8  the manual is the manifest"
# The rule nothing else enforces (playbook §4): a verb the manual offers that the manifest
# does not declare is a command with no grant behind it, and it fails in front of an agent
# that was told it would work. The other direction — declared but not yet built — is where
# M0 deliberately stands, and the manual says so; M2 closes it.
help=$("$bin" -h) || die "\`$cli -h\` did not run"
declared=$(node -e '
  const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  process.stdout.write(m.connector.commands.map((c) => c.name).join("\n"));
' "$pkg/clatch.json")

# The verbs the manual actually offers: the indented lines of its `verbs:` block.
offered=$(printf '%s\n' "$help" | awk '/^verbs:$/{on=1;next} on&&/^[^ ]/{on=0} on&&NF{print $1}')
[ -n "$offered" ] || die "\`$cli -h\` lists no verbs at all"

while read -r verb; do
  [ -n "$verb" ] || continue
  printf '%s\n' "$declared" | grep -qx "$verb" \
    || die "\`$cli -h\` offers '$verb', which clatch.json does not declare — a granted permission that does not exist"
done <<< "$offered"
green "every verb the manual offers is declared ($(printf '%s\n' "$offered" | tr '\n' ' '))"

missing=""
while read -r verb; do
  [ -n "$verb" ] || continue
  printf '%s\n' "$offered" | grep -qx "$verb" || missing="$missing $verb"
done <<< "$declared"

if [ -n "$missing" ]; then
  # M0 ships three verbs on purpose. This is a KNOWN GAP, and it is only tolerable while
  # the manual names it: an agent must never be refused by a verb it was told it had.
  printf '%s\n' "$help" | grep -q "M2" \
    || die "declared verbs are unbuilt ($missing ) and the manual does not say so"
  for verb in $missing; do
    printf '%s\n' "$help" | grep -q "$verb" \
      || die "'$verb' is declared, unbuilt, and never mentioned in the manual"
  done
  printf '\033[33mnote\033[0m  declared and not in this build, each named by the manual as M2:%s\n' "$missing"
fi

# ---------------------------------------------------------------------------------
step "6/8  the negative smoke test"
# With the app NOT running, `crm status` must fail with our own sentence. An exit code of
# 0 here would mean the CLI answered without ever reaching the app.
#
# A socket file outlives the process that bound it, so `~/.crm/crm.sock` is usually still
# on disk from the last run. That is exactly the condition this test wants: the CLI must
# dial it, be refused, and say so. What would make the test meaningless is a LIVE instance
# answering — so rule that out first rather than pass by accident.
if "$bin" status >/dev/null 2>&1; then
  die "an instance of $cli is already running — stop it, or this test proves nothing"
fi
if out=$("$bin" status 2>&1); then
  die "\`$cli status\` succeeded with no app running: $out"
fi
printf '%s' "$out" | grep -q "not running" || die "the failure did not say the app is not running: $out"
printf '%s' "$out" | grep -q "$app_id" || die "the failure did not name the app to start: $out"
green "$(printf '%s' "$out" | head -1)"

# ---------------------------------------------------------------------------------
step "7/8  the round-trip"
# Run from inside the depot, which is where Clatch runs an installed app from — and on
# macOS the only place the binary can find its own `clatch.json`, since the bundle puts it
# four directories above the executable rather than one.
#
# CLATCH_STANDALONE is the dev hatch: no launcher, no control pipe, signals dropped. Every
# other path is the real one.
( cd "$pkg" && CLATCH_STANDALONE=1 "./$cli_bin" app >/dev/null 2>&1 ) &
app_pid=$!

# Wait for an ANSWER, not for a socket file. The file appears the moment the app binds —
# and, more to the point, a file from the last run is on disk before this one starts, so
# waiting on it declares readiness while the CLI is still dialling a dead socket. The only
# thing worth waiting for is the app replying.
status=""
for _ in $(seq 1 150); do
  if status=$("$bin" status 2>/dev/null); then break; fi
  status=""
  kill -0 "$app_pid" 2>/dev/null || die "the app exited before it answered — run it by hand to see why"
  sleep 0.1
done
[ -n "$status" ] || die "the app never answered \`$cli status\` in 15s — the window never came up"
green "the window is up, answering on $(node -e 'process.stdout.write(require("os").homedir())')/.$cli/$cli.sock"

printf '%s' "$status" | grep -q "running" || die "status did not answer: $status"
printf '%s\n' "$status" | sed 's/^/      /'
green "\`$cli status\` round-tripped"

# `focus` is a window verb: clappkit answers it in the app process, before the state sees
# anything. It proves the same socket carries more than one verb.
"$bin" focus >/dev/null || die "\`$cli focus\` failed"
green "\`$cli focus\` round-tripped"

# ---------------------------------------------------------------------------------
step "8/8  the app closes when asked"
# `close` answers first and exits after the grace period, because the CLI is still holding
# the socket — a process that dies before its response frame is written leaves the agent
# with "the app closed the connection" instead of "bye".
bye=$("$bin" close) || die "\`$cli close\` failed"
printf '%s' "$bye" | grep -q "bye" || die "close did not say goodbye: $bye"

for _ in $(seq 1 50); do
  kill -0 "$app_pid" 2>/dev/null || break
  sleep 0.1
done
kill -0 "$app_pid" 2>/dev/null && die "the app is still running after \`$cli close\`"
app_pid=""
green "\`$cli close\` shut it down"

printf '\n\033[32mverify: the two surfaces agree.\033[0m  `npm run pack` builds the .clapp.\n'
