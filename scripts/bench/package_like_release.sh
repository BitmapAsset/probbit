#!/bin/sh
# Pack a local probbit binary the way .github/workflows/release.yml's "package" step does (same names, layout and .sha256),
# so install.sh, install.ps1 and the npm wrapper can be tested before any release exists. Keep in sync with release.yml.
#   scripts/bench/package_like_release.sh <probbit binary> <target triple> <tag> <out dir>
# Writes <out dir>/probbit-<tag>-<target>.tar.gz (.zip for *-windows-*, with 7z as release.yml does) and the .sha256 beside it.
set -eu
BIN=$1 TARGET=$2 TAG=$3 OUT=$4
NAME="probbit-$TAG-$TARGET"
STAGE=$(mktemp -d 2> /dev/null || mktemp -d -t probbitpkg)
mkdir -p "$STAGE/$NAME" "$OUT"
cp "$BIN" "$STAGE/$NAME/"
cp README.md LICENSE CHANGELOG.md "$STAGE/$NAME/"
cp -R examples python docs "$STAGE/$NAME/"
rm -rf "$STAGE/$NAME/python/__pycache__"
case "$TARGET" in
    *windows*) ASSET="$NAME.zip"; (cd "$STAGE" && 7z a "$ASSET" "$NAME" > /dev/null) ;;
    *) ASSET="$NAME.tar.gz"; (cd "$STAGE" && tar czf "$ASSET" "$NAME") ;;
esac
mv "$STAGE/$ASSET" "$OUT/"
rm -rf "$STAGE"
cd "$OUT"
if command -v sha256sum > /dev/null; then sha256sum "$ASSET" > "$ASSET.sha256"; else shasum -a 256 "$ASSET" > "$ASSET.sha256"; fi
echo "$OUT/$ASSET"
