#!/usr/bin/env bash
#
# Assemble `pkg/` — the depot `clatch pack` turns into a `.clapp`, and the exact layout
# `clatch install` unpacks onto someone else's machine.
#
#   pkg/clatch.json                            the manifest, with macOS paths rewritten
#   pkg/bin/crm.app/Contents/MacOS/crm         the binary, in a real bundle
#   pkg/bin/crm.app/Contents/Resources/crm.icns
#   pkg/bin/crm.app/Contents/Info.plist
#   pkg/assets/icon.png
#
# `pkg/` is derived and gitignored; the committed sources are the truth (playbook §5).
#
# Two rules this script exists to obey:
#
#   * **The bundle directory comes from `connector.cli`, never the display name.** Ours
#     is "Breksos CRM" — with a space — and `format.md` § connector limits every
#     component of `cliBin` to `[A-Za-z0-9._-]` because the value is interpolated into an
#     exec shim. A name-derived directory would build here, pass the self-check, pack
#     cleanly, and be refused at `clatch install` in front of a user. That is playbook
#     §12b, and this app is the case it warns about. What a person actually reads is
#     `CFBundleName` / `CFBundleDisplayName`, which carry the name in full.
#
#   * **Never a silent fallback.** A packaging step that "just copies" when its tool is
#     missing ships the wrong artifact on exactly the machines nobody watches. Every step
#     below either succeeds or stops the script.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

manifest="clatch.json"
pkg="pkg"
binary="src-tauri/target/release/crm"

# One script, one source of truth for the layout, two platforms (win1). macOS is the default
# and takes exactly the paths it always did; Git Bash on a Windows runner takes the other arm.
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) os=windows; binary="$binary.exe" ;;
  *)                    os=macos ;;
esac

say() { printf '  %s\n' "$*"; }
die() { printf 'package.sh: %s\n' "$*" >&2; exit 1; }

# --- read the identity out of the manifest, never out of this script ----------------
# Node is the app's own pinned toolchain, and it is the one JSON reader present on every
# platform this family builds on — `jq` is absent from Git Bash (playbook §9).
read_json() { node -e '
  const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  const at = process.argv[2].split(".").reduce((o, k) => (o ?? {})[k], m);
  if (at === undefined || at === null || at === "") { process.exit(3); }
  process.stdout.write(String(at));
' "$manifest" "$1"; }

cli=$(read_json connector.cli)        || die "clatch.json has no connector.cli"
app_id=$(read_json id)                || die "clatch.json has no id"
display=$(read_json name)             || die "clatch.json has no name"
version=$(read_json version)          || die "clatch.json has no version"
icon=$(read_json icon)                || die "clatch.json has no icon"

# --- the one rule a launcher enforces and a packaging script usually does not ---------
# `[A-Za-z0-9._-]`, no `..`, no `*`, no whitespace — per component. Asserted here so the
# refusal arrives in this terminal rather than at someone's install.
assert_safe_path() {
  local what=$1 path=$2 seg
  [ -n "$path" ] || die "$what is empty"
  case $path in
    /*) die "$what must be relative to the depot root, not '$path'" ;;
  esac
  # Split on `/` with globbing OFF — a `*` in a path is one of the very things this
  # function rejects, and expanding it first would check the wrong string.
  local IFS=/
  set -f
  for seg in $path; do
    [ -n "$seg" ] || die "$what has an empty segment: '$path'"
    [ "$seg" != ".." ] || die "$what escapes the depot: '$path'"
    case $seg in
      *[!A-Za-z0-9._-]*) die "$what component '$seg' is not a safe segment (format.md § connector): '$path'" ;;
    esac
  done
  set +f
}

if [ "$os" = macos ]; then
  bundle="$cli.app"
  assert_safe_path "the bundle directory" "$bundle"
  exe_rel="bin/$bundle/Contents/MacOS/$cli"
else
  # A bare console executable: no bundle, so no directory name to get wrong (§12b).
  exe_rel="bin/$cli.exe"
fi
assert_safe_path "connector.cliBin" "$exe_rel"

# --- the binary must already exist ---------------------------------------------------
# Built by `npm run build`, which is `tauri build` and therefore carries the
# `custom-protocol` feature. A bare `cargo build` produces a binary that loads the dev
# URL and shows a white window, so this script will not build one for you.
[ -f "$binary" ] || die "no release binary at $binary — run \`npm run build\` first"

echo "packaging $display $version ($app_id)"

rm -rf "$pkg"
if [ "$os" = macos ]; then
  mkdir -p "$pkg/bin/$bundle/Contents/MacOS" "$pkg/bin/$bundle/Contents/Resources" "$pkg/assets"
else
  mkdir -p "$pkg/bin" "$pkg/assets"
fi

# --- the binary ----------------------------------------------------------------------
# Copied, never linked: a symlink into this source tree resolves to nothing on the
# machine the depot is unpacked on (playbook §1).
cp "$binary" "$pkg/$exe_rel"
chmod +x "$pkg/$exe_rel"
say "bin        $exe_rel  ($(wc -c < "$pkg/$exe_rel" | tr -d ' ') bytes)"

# --- the icon, twice: the depot's own, and the bundle's ------------------------------
[ -f "$icon" ] || die "the manifest declares icon '$icon' and it is not on disk — validate and install both fail on that"
mkdir -p "$pkg/$(dirname "$icon")"
cp "$icon" "$pkg/$icon"
say "icon       $icon"

# macOS only: the .icns and Info.plist that make this a real bundle. A Windows .exe takes
# its icon from the resource tauri-build compiles in (icons/icon.ico), not from the depot.
if [ "$os" = macos ]; then
# A bare executable has no icon identity on macOS, so the Dock falls back to a generic
# terminal tile. A real `.app` with an `.icns` is what actually fixes it (icons.md).
command -v iconutil >/dev/null 2>&1 || die "iconutil is missing — it ships with macOS, and without it this depot's Dock icon would be wrong on every machine but this one"
command -v sips >/dev/null 2>&1 || die "sips is missing — same story as iconutil"

iconset=$(mktemp -d)/"$cli.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$icon" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) "$icon" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$pkg/bin/$bundle/Contents/Resources/$cli.icns"
rm -rf "$(dirname "$iconset")"
say "icns       bin/$bundle/Contents/Resources/$cli.icns"

# --- Info.plist ----------------------------------------------------------------------
# The display name in full is here, which is the only place a person reads it. The
# directory above is a path component inside a depot nobody browses.
cat > "$pkg/bin/$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>$cli</string>
  <key>CFBundleIdentifier</key><string>$app_id</string>
  <key>CFBundleName</key><string>$display</string>
  <key>CFBundleDisplayName</key><string>$display</string>
  <key>CFBundleIconFile</key><string>$cli</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
say "plist      CFBundleDisplayName = $display"
fi

# --- the manifest, with the macOS paths pointed inside the bundle --------------------
# Everything else is copied through untouched: the depot's manifest is identical across
# a version's depots except the per-platform paths.
node -e '
  const fs = require("fs");
  const [src, dst, exe, os] = process.argv.slice(1);
  const m = JSON.parse(fs.readFileSync(src, "utf8"));
  if (os === "macos") {
    m.launch.macos = exe;
  } else {
    // Exactly one OS key per depot (format.md): the source manifest names macOS, a Windows
    // depot must not, or it promises a platform it does not carry.
    const { macos, linux, ...rest } = m.launch;
    m.launch = { windows: exe, ...rest };
  }
  m.connector.cliBin = exe;
  fs.writeFileSync(dst, JSON.stringify(m, null, 2) + "\n");
' "$manifest" "$pkg/clatch.json" "$exe_rel" "$os"
say "manifest   launch.$os = connector.cliBin = $exe_rel"

# --- the self-check ------------------------------------------------------------------
# "The files are present" is not "the depot is installable" (playbook §12b). So: assert
# the rewritten paths are still safe segments, and that every path the manifest points at
# actually resolves inside the depot.
packed_cli_bin=$(cd "$pkg" && node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync("clatch.json","utf8")).connector.cliBin)')
packed_launch=$(cd "$pkg" && node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync("clatch.json","utf8")).launch["'"$os"'"])')
packed_icon=$(cd "$pkg" && node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync("clatch.json","utf8")).icon)')

assert_safe_path "the packed connector.cliBin" "$packed_cli_bin"
assert_safe_path "the packed launch.$os" "$packed_launch"
assert_safe_path "the packed icon" "$packed_icon"

for p in "$packed_cli_bin" "$packed_launch" "$packed_icon"; do
  [ -e "$pkg/$p" ] || die "the depot manifest points at '$p' and it is not in pkg/"
done
[ -x "$pkg/$packed_cli_bin" ] || die "$packed_cli_bin is not executable"

# Only this build's own OS is declared, and an OS key is the claim "runs on that OS" — every key must
# have a depot in the release (format.md § Distribution). Adding `windows` or `linux` to
# the manifest without shipping their depots is a promise the launcher only discovers it
# cannot keep at install time, in front of a user.
declared_os=$(cd "$pkg" && node -e '
  const m = JSON.parse(require("fs").readFileSync("clatch.json", "utf8"));
  process.stdout.write(Object.keys(m.launch).filter((k) => k !== "args").join(" "));
')
[ "$declared_os" = "$os" ] || die "launch declares '$declared_os' but this build is a $os depot: exactly one OS key per depot, and it is this one"

echo "pkg/ is ready — \`clatch validate pkg\`, then \`clatch pack pkg\`"
