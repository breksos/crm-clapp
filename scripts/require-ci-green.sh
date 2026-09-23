#!/usr/bin/env bash
#
# The gate QA round 3 flagged: a branch was merged into `main` locally and only pushed
# afterward, so `verify.yml` — which triggers on every push, no branch filter — checked
# `main` after the merge instead of the branch before it. Nothing was wrong with the
# workflow; nothing ever asked the question early enough for the workflow to answer it.
#
# This is that question, asked before a merge instead of after one:
#
#   scripts/require-ci-green.sh <branch>
#
# It refuses to pass unless:
#   1. the branch has no commits `origin/<branch>` does not also have (i.e. it really was
#      pushed, not just committed locally), and
#   2. the most recent `verify` run for that branch's current commit finished successfully.
#
# This is deliberately a script a person runs, not a server-side rule: branch protection
# on `required status checks` would enforce the same thing automatically, but that is a
# repository setting, and changing repository settings is the product owner's call, not
# something this tree makes unilaterally. Until that decision is made, this is the gate —
# run it, read what it says, then merge.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

workflow="verify.yml"

die() { printf '\033[31mFAIL\033[0m  %s\n' "$*" >&2; exit 1; }
ok()  { printf '\033[32m  ok\033[0m  %s\n' "$*"; }
note(){ printf '\033[33mnote\033[0m  %s\n' "$*"; }

[ $# -eq 1 ] || die "usage: $0 <branch>"
branch=$1

command -v git >/dev/null 2>&1 || die "git is not on PATH"
command -v gh  >/dev/null 2>&1 || die "gh is not on PATH — install it, this gate reads Actions runs through it"
gh auth status >/dev/null 2>&1 || die "gh is not authenticated — run \`gh auth login\`"

# --- 1. the branch really is on the remote, not just committed locally ---------------
git rev-parse --verify --quiet "refs/heads/$branch" >/dev/null \
  || die "no local branch named '$branch'"
local_tip=$(git rev-parse "refs/heads/$branch")

git fetch origin "$branch" --quiet 2>/dev/null \
  || die "origin has no branch '$branch' — push it first: git push -u origin $branch"
remote_tip=$(git rev-parse "refs/remotes/origin/$branch" 2>/dev/null) \
  || die "origin has no branch '$branch' — push it first: git push -u origin $branch"

if [ "$local_tip" != "$remote_tip" ]; then
  die "local '$branch' ($local_tip) and origin/$branch ($remote_tip) disagree — push before checking CI, not after: git push origin $branch"
fi
ok "'$branch' is pushed — origin/$branch is $local_tip"

# --- 2. the workflow ran, on this exact commit, and passed ---------------------------
runs_json=$(gh run list --workflow "$workflow" --branch "$branch" --limit 20 \
  --json databaseId,headSha,status,conclusion,url,createdAt 2>&1) \
  || die "\`gh run list\` failed: $runs_json"

match=$(printf '%s' "$runs_json" | node -e '
  const sha = process.argv[1];
  const runs = JSON.parse(require("fs").readFileSync(0, "utf8"));
  const hit = runs.filter((r) => r.headSha === sha)
                   .sort((a, b) => new Date(b.createdAt) - new Date(a.createdAt))[0];
  process.stdout.write(hit ? JSON.stringify(hit) : "");
' "$local_tip")

[ -n "$match" ] || die "no '$workflow' run found for $local_tip on '$branch' yet — push triggers it; wait for it to appear (gh run list --branch $branch)"

status=$(printf '%s' "$match" | node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync(0,"utf8")).status)')
conclusion=$(printf '%s' "$match" | node -e 'const r=JSON.parse(require("fs").readFileSync(0,"utf8"));process.stdout.write(r.conclusion||"")')
url=$(printf '%s' "$match" | node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync(0,"utf8")).url)')

case "$status" in
  completed) ;;
  *) die "the run for $local_tip is '$status', not finished yet — $url" ;;
esac

case "$conclusion" in
  success) ;;
  *) die "the run for $local_tip concluded '$conclusion', not success — $url" ;;
esac

ok "verify passed for $local_tip — $url"
printf '\n\033[32mgreen: safe to merge '\''%s'\''.\033[0m\n' "$branch"
