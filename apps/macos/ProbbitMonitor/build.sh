#!/bin/sh
# Builds "probbit Monitor.app": the Swift shell (arm64 + x86_64, one universal file) around a given probbit binary, signed,
# zipped, and notarized when asked. CI (ci.yml, release.yml) runs exactly this script.
#
#   build.sh [--binary PATH] [--asset-version STR] [--out DIR] [--identity ID [--keychain FILE]]
#            [--notarize-profile NAME | --notarize-key P8 --notarize-key-id ID --notarize-issuer ID]
#
#   --binary PATH         the probbit engine to bundle (default: target/release/probbit of this checkout)
#   --asset-version STR   the version in the zip's name (default: the version in Cargo.toml; the release passes the tag)
#   --out DIR             where the bundle and the zip go (default: apps/macos/ProbbitMonitor/dist)
#   --identity ID         sign with the hardened runtime and a secure timestamp, with this identity: a "Developer ID
#                         Application: ..." name or its SHA-1; "-" signs ad-hoc with the hardened runtime and no timestamp
#                         (what ci.yml does). Without --identity: ad-hoc, no hardened runtime.
#   --keychain FILE       find the identity only in this keychain (release.yml imports the certificate into one of its own)
#   --notarize-profile NAME   notarize, with the credentials saved by `xcrun notarytool store-credentials NAME`
#   --notarize-key P8 --notarize-key-id ID --notarize-issuer ID
#                         notarize, with an App Store Connect API key (the .p8 file, its key id, the issuer id)
#
# Notarizing submits the signed app to Apple's notary service, waits (up to 30 minutes), staples the ticket to the app and
# zips the stapled app. The submission's result and log (JSON) are written beside the zip. If the service does not accept
# the app, the script prints the log and fails, and no zip is left: a failed notarization never leaves an asset behind.
#
# Needs the Xcode command-line tools (swiftc, lipo, codesign, ditto; notarytool and stapler to notarize). No other network
# use, no third-party code.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
binary="$root/target/release/probbit"
out="$here/dist"
asset=""
identity=""
keychain=""
n_profile=""
n_key=""
n_key_id=""
n_issuer=""
while [ $# -gt 0 ]; do
  case "$1" in
    --binary) binary=$2; shift 2 ;;
    --asset-version) asset=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    --identity) identity=$2; shift 2 ;;
    --keychain) keychain=$2; shift 2 ;;
    --notarize-profile) n_profile=$2; shift 2 ;;
    --notarize-key) n_key=$2; shift 2 ;;
    --notarize-key-id) n_key_id=$2; shift 2 ;;
    --notarize-issuer) n_issuer=$2; shift 2 ;;
    -h|--help) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "build.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done

notarize=""
if [ -n "$n_profile" ] || [ -n "$n_key$n_key_id$n_issuer" ]; then notarize=yes; fi
[ -z "$n_profile" ] || [ -z "$n_key$n_key_id$n_issuer" ] || { echo "build.sh: --notarize-profile or --notarize-key, not both" >&2; exit 2; }
[ -n "$n_profile" ] || [ -z "$notarize" ] || { [ -n "$n_key" ] && [ -n "$n_key_id" ] && [ -n "$n_issuer" ]; } ||
  { echo "build.sh: --notarize-key needs --notarize-key-id and --notarize-issuer" >&2; exit 2; }
[ -z "$n_key" ] || [ -f "$n_key" ] || { echo "build.sh: no API key file at $n_key" >&2; exit 2; }
[ -z "$notarize" ] || { [ -n "$identity" ] && [ "$identity" != "-" ]; } ||
  { echo "build.sh: notarizing needs --identity with a Developer ID Application identity (not ad-hoc)" >&2; exit 2; }
[ -z "$keychain" ] || [ -n "$identity" ] || { echo "build.sh: --keychain needs --identity" >&2; exit 2; }

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
zip="$out/probbit-Monitor-$asset-macos.zip"
result="$out/probbit-Monitor-$asset-macos.notarization.json"
log="$out/probbit-Monitor-$asset-macos.notarization-log.json"
# the old outputs go first: whatever fails below, no zip of an earlier build is left under this name
rm -rf "$app" "$work" "$zip" "$zip.sha256" "$result" "$log"
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

# (the linkers already left an ad-hoc signature, so codesign says "replacing existing signature": kept out unless it fails)
if [ -z "$identity" ]; then
  # ad-hoc signature (no Apple Developer ID: the app is not notarized, see docs/monitor-app.md)
  codesign --force --sign - --identifier dev.probbit.cli "$app/Contents/MacOS/probbit" 2> "$work/codesign.log" || { cat "$work/codesign.log" >&2; exit 1; }
  codesign --force --deep --sign - "$app" 2> "$work/codesign.log" || { cat "$work/codesign.log" >&2; exit 1; }
  signature="ad-hoc"
else
  # the hardened runtime, the bundled engine first (nested code is signed before what contains it), then the app; no
  # entitlements: the web view's content runs in WebKit's own processes, and the engine is a plain child process
  set -- --force --options runtime --sign "$identity"
  if [ "$identity" = "-" ]; then set -- "$@" --timestamp=none; else set -- "$@" --timestamp; fi
  [ -z "$keychain" ] || set -- "$@" --keychain "$keychain"
  codesign "$@" --identifier dev.probbit.cli "$app/Contents/MacOS/probbit" 2> "$work/codesign.log" || { cat "$work/codesign.log" >&2; exit 1; }
  codesign "$@" "$app" 2> "$work/codesign.log" || { cat "$work/codesign.log" >&2; exit 1; }
  for f in "$app/Contents/MacOS/probbit" "$app"; do
    codesign -dv --verbose=2 "$f" 2>&1 | grep -Eq '^CodeDirectory .*flags=0x[0-9a-f]+\(([a-z-]+,)*runtime' ||
      { echo "build.sh: $f is not signed with the hardened runtime" >&2; exit 1; }
  done
  if [ "$identity" = "-" ]; then signature="ad-hoc, hardened runtime"; else signature="Developer ID, hardened runtime, timestamp"; fi
fi
codesign --verify --deep --strict "$app"

if [ -n "$notarize" ]; then
  # credentials as arguments of notarytool; the .p8 stays a file, nothing here prints a credential
  notary() {
    if [ -n "$n_profile" ]; then xcrun notarytool "$@" --keychain-profile "$n_profile"
    else xcrun notarytool "$@" --key "$n_key" --key-id "$n_key_id" --issuer "$n_issuer"; fi
  }
  ditto -c -k --norsrc --noextattr --noacl --keepParent "$app" "$work/notarize.zip"
  echo "notarize: submitting the app to Apple's notary service, waiting up to 30 minutes"
  rc=0
  notary submit "$work/notarize.zip" --wait --timeout 30m --output-format json > "$result" || rc=$?
  id=$(plutil -extract id raw -o - "$result" 2> /dev/null || true)
  status=$(plutil -extract status raw -o - "$result" 2> /dev/null || true)
  [ -z "$id" ] || notary log "$id" "$log" > /dev/null 2>&1 || echo "notarize: could not fetch the log of submission $id" >&2
  if [ "$status" != Accepted ]; then
    echo "build.sh: notarization failed: status ${status:-unknown} (notarytool rc $rc, submission ${id:-none}); no zip written" >&2
    cat "$result" >&2
    [ ! -f "$log" ] || cat "$log" >&2
    rm -rf "$work"
    exit 1
  fi
  echo "notarize: submission $id: Accepted"
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  codesign --verify --deep --strict "$app"
  signature="$signature, notarized, ticket stapled"
fi

ditto -c -k --norsrc --noextattr --noacl --keepParent "$app" "$zip"
(cd "$out" && shasum -a 256 "$(basename "$zip")" > "$(basename "$zip").sha256")
rm -rf "$work"

echo "app:      $app ($(du -sk "$app" | cut -f1) KB; shell: $(lipo -archs "$app/Contents/MacOS/ProbbitMonitor"); probbit: $(lipo -archs "$app/Contents/MacOS/probbit"))"
echo "signed:   $signature"
echo "zip:      $zip ($(wc -c < "$zip" | tr -d ' ') bytes)"
echo "sha256:   $(cut -d' ' -f1 "$zip.sha256")"
