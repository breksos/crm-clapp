#!/usr/bin/env bash
#
# Is the clappkit we pin behind what its upstream has published? (docs/work-orders/k0.)
#
# Why a gate: CI's `submodules: recursive` checks out the PINNED commit, so a stale pin and
# a fresh pin look identical to every other check. clappkit once sat 14 commits behind, four
# of them security fixes, for three weeks because nothing ever compared the pin to upstream.
#
# What "current" means: the newest release tag (v1.2.3 or 1.2.3) upstream has published; and
# while upstream has published none — true today — the tip of its default branch. Without
# the fallback this gate would pass vacuously, and would not have caught the incident above.
#
#   pin == reference                  fresh
#   pin is an ancestor of reference   BEHIND — fail, with the commits we are missing
#   reference is an ancestor of pin   ahead of the release — fine, said out loud
#   neither                           diverged, or the pin is not on upstream at all — fail
#
# No network: a local run warns and passes; CI (GITHUB_ACTIONS or CI set) fails, because a
# gate that goes quiet exactly where it is meant to be strict is not a gate.
# `clappkit/` itself is only read, never modified — the comparison uses a throwaway repo.
#
# CLAPPKIT_UPSTREAM overrides the URL from .gitmodules (used to test this script).
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

die()  { printf '\033[31mFAIL\033[0m  %s\n' "$*" >&2; exit 1; }
ok()   { printf '\033[32m  ok\033[0m  %s\n' "$*"; }
warn() { printf '\033[33mwarn\033[0m  %s\n' "$*"; [ -z "${GITHUB_ACTIONS:-}" ] || printf '::warning title=clappkit freshness not checked::%s\n' "$*"; }

path=clappkit
# The pin the repository RECORDS — not whatever happens to be checked out in the submodule.
pin=$(git ls-tree HEAD "$path" | awk '$2 == "commit" { print $3 }')
[ -n "$pin" ] || die "$path is not a submodule in HEAD"
url=${CLAPPKIT_UPSTREAM:-$(git config -f .gitmodules --get "submodule.$path.url")}
[ -n "$url" ] || die "no url for $path in .gitmodules"

# --- the reference: newest release tag, else the default branch ------------------------------
refs=$(git ls-remote --tags "$url" 2>&1) || net=1
if [ "${net:-}" = 1 ]; then
  reason="could not reach $url — $(printf '%s' "$refs" | head -1)"
  if [ -n "${GITHUB_ACTIONS:-}" ] || [ -n "${CI:-}" ]; then
    die "$reason (CI must be able to check freshness)"
  fi
  warn "$reason; clappkit freshness NOT checked (pinned ${pin:0:7})"
  exit 0
fi

# Peeled lines (`^{}`) carry the commit an annotated tag points at; use them when present.
tag=$(printf '%s\n' "$refs" | awk '{ sub("refs/tags/", "", $2); sub(/\^\{\}$/, "", $2); print $2 }' \
        | grep -E '^v?[0-9]+\.[0-9]+\.[0-9]+$' | sort -uV | tail -1 || true)
if [ -n "$tag" ]; then
  fetch_ref="refs/tags/$tag"; label="release $tag"
else
  fetch_ref="HEAD"; label="upstream's default branch (no release tags published)"
fi

tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
git init --bare --quiet "$tmp"
# Commits and trees only: history is all this needs, and blobs are the expensive part.
git -C "$tmp" fetch --quiet --filter=blob:none "$url" "$fetch_ref" 2>&1 \
  || die "could not fetch $fetch_ref from $url"
ref=$(git -C "$tmp" rev-parse FETCH_HEAD^{commit})

if [ "$pin" = "$ref" ]; then
  ok "clappkit is current — pinned ${pin:0:7}, $label"; exit 0
fi

# Is the pin reachable from the reference? If not, ask upstream for it directly.
if ! git -C "$tmp" cat-file -e "$pin^{commit}" 2>/dev/null; then
  git -C "$tmp" fetch --quiet --filter=blob:none "$url" "$pin" >/dev/null 2>&1 || true
fi
git -C "$tmp" cat-file -e "$pin^{commit}" 2>/dev/null \
  || die "pinned clappkit ${pin:0:7} is not on $url — a local-only commit, or a force-pushed one"

if git -C "$tmp" merge-base --is-ancestor "$pin" "$ref"; then
  n=$(git -C "$tmp" rev-list --count "$pin..$ref")
  {
    printf '\033[31mFAIL\033[0m  clappkit is %s commit(s) behind %s\n' "$n" "$label"
    printf '      pinned   %s\n      current  %s\n\n' "$pin" "$ref"
    git -C "$tmp" log --format='      %h %s' -n 15 "$pin..$ref"
    [ "$n" -le 15 ] || printf '      … and %s more\n' "$((n - 15))"
    printf '\n      read them (K-numbered ones are security fixes), then:\n'
    printf '        git submodule update --remote %s && git add %s && git commit\n' "$path" "$path"
  } >&2
  exit 1
fi

if git -C "$tmp" merge-base --is-ancestor "$ref" "$pin"; then
  ok "clappkit ${pin:0:7} is ahead of $label (${ref:0:7}) — not behind"; exit 0
fi
die "clappkit ${pin:0:7} and $label (${ref:0:7}) have diverged — neither contains the other"
