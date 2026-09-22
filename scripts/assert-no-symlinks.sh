#!/usr/bin/env bash
#
# playbook §1: a `.clapp` ships real files only. A symlink into this checkout resolves to
# nothing on the machine the depot is unpacked on, and `zip` would either follow it (and
# ship a copy, defeating the point of asking) or store the link literally (and ship a
# dangling path). Neither is caught by `package.sh`'s own checks, which look at whether the
# manifest's paths resolve — a symlink resolves fine, right up until it is unpacked
# somewhere else.
set -euo pipefail

die() { printf 'assert-no-symlinks: %s\n' "$*" >&2; exit 1; }

[ $# -eq 1 ] || die "usage: $0 <dir>"
dir=$1
[ -d "$dir" ] || die "$dir is not a directory"

found=$(find "$dir" -type l)
[ -z "$found" ] || die "symlinks in $dir, which a .clapp must never carry:
$found"

printf 'no symlinks under %s\n' "$dir"
