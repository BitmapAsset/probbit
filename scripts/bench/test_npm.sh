#!/bin/sh
# The npm wrapper end to end with no GitHub release and no registry: pack a local binary as release.yml does, serve it on
# 127.0.0.1, `npm pack` npm/, `npm install -g` the tarball (into a scratch prefix, so the machine's global packages are not
# touched): postinstall fetches + verifies the binary; run it (exit codes 0/1/2/3 must pass through); `npm uninstall -g`.
# Then an --ignore-scripts install (fetched on first run), PBIT_BINARY, and a tampered .sha256 (the install must fail).
#   scripts/bench/test_npm.sh <pbit binary> <target triple> [tag]
set -eu
BIN=$1 TARGET=$2 TAG=${3:-v0.2.0}
PY=${PY:-$(command -v python3 || command -v python)}
WORK=$(mktemp -d 2> /dev/null || mktemp -d -t pbitnpm)
SRV=""
cleanup() { if [ -n "$SRV" ]; then kill "$SRV" 2> /dev/null || true; fi; rm -rf "$WORK"; }
trap cleanup EXIT
fail() { echo "FAIL: $*"; exit 1; }
BIN_ABS=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")
if command -v cygpath > /dev/null; then BIN_ABS=$(cygpath -w "$BIN_ABS"); fi # Windows: node needs a native path
sh scripts/bench/package_like_release.sh "$BIN" "$TARGET" "$TAG" "$WORK/srv/good/$TAG" > /dev/null
cp -R "$WORK/srv/good" "$WORK/srv/bad"
case "$TARGET" in *windows*) EXT=zip ;; *) EXT=tar.gz ;; esac
printf '%064d  %s\n' 0 "pbit-$TAG-$TARGET.$EXT" > "$WORK/srv/bad/$TAG/pbit-$TAG-$TARGET.$EXT.sha256"
PORT=$("$PY" -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
"$PY" -m http.server "$PORT" --bind 127.0.0.1 --directory "$WORK/srv" > "$WORK/http.log" 2>&1 &
SRV=$!
i=0
until curl -fsS -o /dev/null "http://127.0.0.1:$PORT/" 2> /dev/null; do
    i=$((i + 1)); [ "$i" -lt 50 ] || fail "http server did not start"; sleep 0.2
done
BASE="http://127.0.0.1:$PORT"
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) gbin() { echo "$1"; } ;; *) gbin() { echo "$1/bin"; } ;; esac
NPM_FLAGS="--no-audit --no-fund --foreground-scripts"
echo "node $(node --version), npm $(npm --version), target $TARGET"

(cd npm && npm pack --pack-destination "$WORK" > /dev/null)
TGZ=$(ls "$WORK"/pbit-*.tgz)
echo "== npm pack -> $(basename "$TGZ"):"
tar tzf "$TGZ" | sed 's/^/   /'

echo "== 1. npm install -g $(basename "$TGZ") (PBIT_DOWNLOAD_BASE=$BASE/good)"
P1="$WORK/prefix1"
PBIT_DOWNLOAD_BASE="$BASE/good" npm install -g --prefix "$P1" $NPM_FLAGS "$TGZ"
OLDPATH=$PATH
PATH="$(gbin "$P1"):$OLDPATH"
echo "\$ which pbit: $(command -v pbit)"
echo "\$ pbit version: $(pbit version)"
pbit demo --tasks 12 | pbit decide > "$WORK/d12.json"
echo "\$ pbit demo --tasks 12 | pbit decide: exit $?, $("$PY" -c 'import json,sys; d=json.load(open(sys.argv[1])); print("verdict", d["verdict"], "| released", len(d["released"]), "of", d["tasks"])' "$WORK/d12.json")"
set +e
printf '%s' '{"workers":[{"id":"a","cap":1}],"tasks":[{"id":"t1","allowed":["a"],"scores":{"a":1}},{"id":"t2","allowed":["a"],"scores":{"a":1}}]}' | pbit decide > /dev/null; r1=$?
printf '%s' '{"workers": 1}' | pbit decide > /dev/null 2>&1; r2=$?
pbit run --op exact --exact-ms 50 < examples/denoise-8x12.json > /dev/null; r3=$?
set -e
echo "exit codes through the wrapper: infeasible $r1 (want 1), bad input $r2 (want 2), declined $r3 (want 3)"
[ "$r1" = 1 ] && [ "$r2" = 2 ] && [ "$r3" = 3 ] || fail "exit codes"
npm uninstall -g --prefix "$P1" $NPM_FLAGS pbit
PATH=$OLDPATH
[ ! -e "$(gbin "$P1")/pbit" ] || fail "npm uninstall left $(gbin "$P1")/pbit"
echo "ok: uninstalled, $(gbin "$P1")/pbit absent"

echo "== 2. npm install -g --ignore-scripts: the binary is fetched on first run"
P2="$WORK/prefix2"
npm install -g --prefix "$P2" $NPM_FLAGS --ignore-scripts "$TGZ"
PBIT_DOWNLOAD_BASE="$BASE/good" "$(gbin "$P2")/pbit" version
"$(gbin "$P2")/pbit" demo --tasks 12 | "$(gbin "$P2")/pbit" decide > /dev/null
echo "ok: first run fetched it; second run used it"

echo "== 3. PBIT_BINARY=<local build> (no download)"
P3="$WORK/prefix3"
PBIT_BINARY="$BIN_ABS" PBIT_DOWNLOAD_BASE="http://127.0.0.1:9/nowhere" npm install -g --prefix "$P3" $NPM_FLAGS "$TGZ"
"$(gbin "$P3")/pbit" version

echo "== 4. tampered .sha256: npm install must fail"
if PBIT_DOWNLOAD_BASE="$BASE/bad" npm install -g --prefix "$WORK/prefix4" $NPM_FLAGS "$TGZ"; then fail "a wrong checksum was accepted"; fi
echo "ok: rejected"
echo "npm wrapper: all checks passed ($TARGET)"
