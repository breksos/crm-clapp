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
# macOS ships `zip` and `shasum`. Git Bash on a Windows runner has neither: it has `7z` and
# `sha256sum` (playbook §9). Both pairs produce the same bytes' worth of meaning — a plain zip
# rooted at clatch.json, and "<hex>  <file>" — so the choice is by what is installed.
if command -v zip >/dev/null 2>&1; then zipper=zip; elif command -v 7z >/dev/null 2>&1; then zipper=7z
else die "neither zip nor 7z is on PATH"; fi
if command -v shasum >/dev/null 2>&1; then summer="shasum -a 256"; elif command -v sha256sum >/dev/null 2>&1; then summer=sha256sum
else die "neither shasum nor sha256sum is on PATH"; fi

app_id=$(node -e '
  const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  if (!m.id) process.exit(3);
  process.stdout.write(m.id);
' "$pkg/clatch.json") || die "$pkg/clatch.json has no id"

clapp="$app_id-$target.clapp"
rm -f "$root/$clapp" "$root/$clapp.sha256"

# -X: no extra file attributes (uid/gid, resource forks) that make the archive depend on
# the machine that built it. -x: never a .DS_Store, which sips/Finder can leave in pkg/.
if [ "$zipper" = zip ]; then
  ( cd "$pkg" && zip -r -X -q "$root/$clapp" . -x '*.DS_Store' )
else
  # -tzip: deflate, the one method the reader accepts. 7z keeps no extra attributes by default.
  # Name the entries explicitly, clatch.json first: 7z otherwise sorts directories first, and the
  # macOS zip has the manifest at the front. The format only needs it at the root, but a reader
  # that peeks at the first entry should find the same thing on every platform.
  ( cd "$pkg" && rest=$(ls -A | grep -v -x clatch.json | grep -v -x .DS_Store | tr '\n' ' ') \
      && 7z a -tzip -mx=6 -bso0 -bsp0 "$root/$clapp" clatch.json $rest )
fi
[ -s "$root/$clapp" ] || die "$clapp is empty"

( cd "$root" && $summer "$clapp" > "$clapp.sha256" )

printf 'packed  %s  (%s bytes)\n' "$clapp" "$(wc -c < "$root/$clapp" | tr -d ' ')"
printf 'summed  %s.sha256\n' "$clapp"
