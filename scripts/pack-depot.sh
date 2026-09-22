#!/usr/bin/env bash
#
# Zip `pkg/` into the one file format a `.clapp` is: a zip rooted at `clatch.json`, entries
# stored or deflated only (format.md § The package — the reader has no bzip2/lzma/zstd).
# `clatch pack` is the canonical tool for this, but Clatch has no public binary, so no
# runner can call it (docs/release-plan.md's constraint). This is CI's stand-in, doing
# exactly what that command does to the bytes and nothing else.
#
# Writes <id>-<target>.clapp and its .sha256 at the repo root, matching the asset-naming
# rule in format.md § Distribution (the id, then -<os>-<arch>.clapp).
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

die() { printf 'pack-depot: %s\n' "$*" >&2; exit 1; }

[ $# -eq 1 ] || die "usage: $0 <target, e.g. macos-arm64>"
target=$1
pkg="pkg"

[ -f "$pkg/clatch.json" ] || die "$pkg/clatch.json does not exist — run scripts/package.sh first"
command -v zip >/dev/null 2>&1 || die "zip is not on PATH — macOS ships it; this script is macOS-only by design"
command -v shasum >/dev/null 2>&1 || die "shasum is not on PATH"

app_id=$(node -e '
  const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  if (!m.id) process.exit(3);
  process.stdout.write(m.id);
' "$pkg/clatch.json") || die "$pkg/clatch.json has no id"

clapp="$app_id-$target.clapp"
rm -f "$root/$clapp" "$root/$clapp.sha256"

# -X: no extra file attributes (uid/gid, resource forks) that make the archive depend on
# the machine that built it. -x: never a .DS_Store, which sips/Finder can leave in pkg/.
( cd "$pkg" && zip -r -X -q "$root/$clapp" . -x '*.DS_Store' )
[ -s "$root/$clapp" ] || die "$clapp is empty"

( cd "$root" && shasum -a 256 "$clapp" > "$clapp.sha256" )

printf 'packed  %s  (%s bytes)\n' "$clapp" "$(wc -c < "$root/$clapp" | tr -d ' ')"
printf 'summed  %s.sha256\n' "$clapp"
