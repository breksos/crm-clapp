#!/usr/bin/env bash
#
# `release.yml` builds whatever `clatch.json` says version `v${version}` is, so a tag that
# disagrees with the manifest would silently release the wrong number. Catch that before
# any build starts, not after five minutes of compiling.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

die() { printf 'assert-tag-matches-manifest: %s\n' "$*" >&2; exit 1; }

[ $# -eq 1 ] || die "usage: $0 <tag>"
tag=$1

case "$tag" in
  v*) version=${tag#v} ;;
  *) die "tag '$tag' does not start with 'v'" ;;
esac

manifest_version=$(node -e '
  const m = JSON.parse(require("fs").readFileSync("clatch.json", "utf8"));
  if (!m.version) process.exit(3);
  process.stdout.write(m.version);
') || die "clatch.json has no version"

[ "$version" = "$manifest_version" ] \
  || die "tag '$tag' names version '$version', but clatch.json says '$manifest_version'"

printf 'tag %s matches clatch.json version %s\n' "$tag" "$manifest_version"
