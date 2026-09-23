#!/usr/bin/env bash
#
# format.md § Distribution: the manifest is identical across every depot of one version,
# except the per-platform paths (`launch`, `connector.cliBin`), which must differ. Both of
# our v1 depots are macOS, so those paths should not even differ between them — a mismatch
# here is `package.sh` drifting between the two matrix jobs, not a platform difference.
set -euo pipefail

die() { printf 'compare-depot-manifests: %s\n' "$*" >&2; exit 1; }

[ $# -eq 2 ] || die "usage: $0 <manifest-a.json> <manifest-b.json>"
a=$1
b=$2
[ -f "$a" ] || die "$a does not exist"
[ -f "$b" ] || die "$b does not exist"

node -e '
  const fs = require("fs");
  const [aPath, bPath] = process.argv.slice(1);
  const a = JSON.parse(fs.readFileSync(aPath, "utf8"));
  const b = JSON.parse(fs.readFileSync(bPath, "utf8"));

  const platformFields = ["launch.macos", "connector.cliBin"];
  const get = (obj, path) => path.split(".").reduce((o, k) => (o ?? {})[k], obj);

  // Within EACH depot, launch.macos and connector.cliBin must already agree — package.sh
  // asserts this too, but a manifest read straight off disk deserves its own check.
  for (const [name, m] of [["a", a], ["b", b]]) {
    const launch = get(m, "launch.macos");
    const cliBin = get(m, "connector.cliBin");
    if (launch !== cliBin) {
      console.error(`depot ${name}: launch.macos (${launch}) != connector.cliBin (${cliBin})`);
      process.exit(1);
    }
  }

  // Strip the platform fields, then the rest must be byte-identical.
  const strip = (m) => {
    const c = JSON.parse(JSON.stringify(m));
    c.launch = { ...c.launch };
    delete c.launch.macos;
    if (c.connector) c.connector = { ...c.connector, cliBin: undefined };
    return c;
  };
  if (JSON.stringify(strip(a)) !== JSON.stringify(strip(b))) {
    console.error("manifests disagree outside launch.macos / connector.cliBin:");
    console.error("  a:", JSON.stringify(strip(a)));
    console.error("  b:", JSON.stringify(strip(b)));
    process.exit(1);
  }

  // Both depots are macOS: the platform paths themselves should match too.
  const la = get(a, "launch.macos"), lb = get(b, "launch.macos");
  if (la !== lb) {
    console.error(`launch.macos differs between two macOS depots: ${la} vs ${lb}`);
    process.exit(1);
  }

  console.log("manifests agree: identical apart from (and including) launch.macos/connector.cliBin, which also match between depots");
' "$a" "$b"
