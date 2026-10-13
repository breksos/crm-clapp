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

  // Exactly one OS key per depot (format.md): that is the depot'"'"'s platform, and the one
  // field (besides connector.cliBin) that is allowed to differ between depots.
  const OS = ["macos", "windows", "linux"];
  const osOf = (name, m) => {
    const keys = OS.filter((k) => m.launch && k in m.launch);
    if (keys.length !== 1) {
      console.error(`depot ${name} declares ${keys.length} OS keys (${keys.join(",") || "none"}); exactly one is required`);
      process.exit(1);
    }
    return keys[0];
  };
  const osA = osOf("a", a), osB = osOf("b", b);

  // Within EACH depot, launch.<os> and connector.cliBin must already agree — package.sh
  // asserts this too, but a manifest read straight off disk deserves its own check.
  for (const [name, m, os] of [["a", a, osA], ["b", b, osB]]) {
    const launch = m.launch[os];
    const cliBin = m.connector && m.connector.cliBin;
    if (launch !== cliBin) {
      console.error(`depot ${name}: launch.${os} (${launch}) != connector.cliBin (${cliBin})`);
      process.exit(1);
    }
  }

  // Strip the platform fields, then the rest must be byte-identical.
  const strip = (m, os) => {
    const c = JSON.parse(JSON.stringify(m));
    c.launch = { ...c.launch };
    delete c.launch[os];
    if (c.connector) c.connector = { ...c.connector, cliBin: undefined };
    return c;
  };
  if (JSON.stringify(strip(a, osA)) !== JSON.stringify(strip(b, osB))) {
    console.error(`manifests disagree outside launch.${osA === osB ? osA : `${osA}/${osB}`} / connector.cliBin:`);
    console.error("  a:", JSON.stringify(strip(a, osA)));
    console.error("  b:", JSON.stringify(strip(b, osB)));
    process.exit(1);
  }

  if (osA === osB) {
    // Two depots of one OS: the platform paths themselves should match too.
    const la = a.launch[osA], lb = b.launch[osB];
    if (la !== lb) {
      console.error(`launch.${osA} differs between two ${osA} depots: ${la} vs ${lb}`);
      process.exit(1);
    }
    console.log(`manifests agree: identical apart from (and including) launch.${osA}/connector.cliBin, which also match between depots`);
  } else {
    // Different OSes: the paths are SUPPOSED to differ, and nothing else may.
    console.log(`manifests agree: identical apart from launch.${osA}/launch.${osB} and connector.cliBin, which differ by platform as they must`);
  }
' "$a" "$b"
