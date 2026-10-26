#!/bin/sh
# Builds "probbit Monitor.app": the Swift shell (arm64 + x86_64, one universal file) around a given probbit binary, ad-hoc
# signed, zipped. CI (ci.yml, release.yml) runs exactly this script.
#
#   build.sh [--binary PATH] [--asset-version STR] [--out DIR]
#
#   --binary PATH         the probbit engine to bundle (default: target/release/probbit of this checkout)
#   --asset-version STR   the version in the zip's name (default: the version in Cargo.toml; the release passes the tag)
#   --out DIR             where the bundle and the zip go (default: apps/macos/ProbbitMonitor/dist)
#
# Needs the Xcode command-line tools (swiftc, lipo, codesign, ditto). No network, no third-party code.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
binary="$root/target/release/probbit"
out="$here/dist"
asset=""
while [ $# -gt 0 ]; do
  case "$1" in
    --binary) binary=$2; shift 2 ;;
    --asset-version) asset=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    -h|--help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "build.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
[ -n "$version" ] || { echo "build.sh: no version in $root/Cargo.toml" >&2; exit 1; }
[ -n "$asset" ] || asset=$version
build=$(printf '%s' "$version" | sed 's/[^0-9.].*//')   # CFBundleVersion: digits and dots only
[ -x "$binary" ] || { echo "build.sh: no probbit binary at $binary (cargo build --release -p probbit-cli, or pass --binary)" >&2; exit 1; }
"$binary" version > /dev/null

mkdir -p "$out"
out=$(cd "$out" && pwd)
app="$out/probbit Monitor.app"
work="$out/.build"
rm -rf "$app" "$work"
mkdir -p "$work" "$app/Contents/MacOS" "$app/Contents/Resources"

# the shell: one slice per architecture, warnings fail the build, then one universal file
for arch in arm64 x86_64; do
  swiftc -O -swift-version 5 -warnings-as-errors -target "$arch-apple-macos13.0" -module-name ProbbitMonitor \
    "$here"/Sources/*.swift -o "$work/ProbbitMonitor-$arch"
done
lipo -create "$work/ProbbitMonitor-arm64" "$work/ProbbitMonitor-x86_64" -output "$app/Contents/MacOS/ProbbitMonitor"

cp "$binary" "$app/Contents/MacOS/probbit"
chmod 755 "$app/Contents/MacOS/probbit" "$app/Contents/MacOS/ProbbitMonitor"
cp "$here/Resources/AppIcon.icns" "$app/Contents/Resources/"
sed -e "s/@VERSION@/$version/" -e "s/@BUILD@/$build/" "$here/Resources/Info.plist" > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
plutil -lint "$app/Contents/Info.plist" > /dev/null

# no extended attributes in the bundle: a zip carries them as ._ files, and an unzip that writes those breaks the seal
xattr -cr "$app"

# ad-hoc signature (no Apple Developer ID: the app is not notarized, see docs/monitor-app.md)
# (the linkers already left an ad-hoc signature, so codesign says "replacing existing signature": kept out unless it fails)
codesign --force --sign - --identifier dev.probbit.cli "$app/Contents/MacOS/probbit" 2> "$work/codesign.log" || { cat "$work/codesign.log" >&2; exit 1; }
codesign --force --deep --sign - "$app" 2> "$work/codesign.log" || { cat "$work/codesign.log" >&2; exit 1; }
codesign --verify --deep --strict "$app"

zip="$out/probbit-Monitor-$asset-macos.zip"
rm -f "$zip" "$zip.sha256"
ditto -c -k --norsrc --noextattr --noacl --keepParent "$app" "$zip"
(cd "$out" && shasum -a 256 "$(basename "$zip")" > "$(basename "$zip").sha256")
rm -rf "$work"

echo "app:      $app ($(du -sk "$app" | cut -f1) KB; shell: $(lipo -archs "$app/Contents/MacOS/ProbbitMonitor"); probbit: $(lipo -archs "$app/Contents/MacOS/probbit"))"
echo "zip:      $zip ($(wc -c < "$zip" | tr -d ' ') bytes)"
echo "sha256:   $(cut -d' ' -f1 "$zip.sha256")"
