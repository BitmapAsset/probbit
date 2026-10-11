#!/bin/sh
# install.sh end to end with no GitHub release: pack a local binary as release.yml does, serve it on 127.0.0.1, then install
# it piped through sh (as `curl ... | sh` does), into the default directory under a scratch HOME, and from a server whose
# .sha256 is wrong (must fail and install nothing).
#   scripts/bench/test_install_sh.sh <probbit binary> <target triple> [tag]      SH=dash scripts/bench/... to pick the shell
set -eu
BIN=$1 TARGET=$2 TAG=${3:-v$(node -p "require('./npm/package.json').version")}
SH=${SH:-sh}
PY=${PY:-$(command -v python3 || command -v python)}
WORK=$(mktemp -d 2> /dev/null || mktemp -d -t probbitinst)
SRV=""
cleanup() { if [ -n "$SRV" ]; then kill "$SRV" 2> /dev/null || true; fi; rm -rf "$WORK"; }
trap cleanup EXIT
sh scripts/bench/package_like_release.sh "$BIN" "$TARGET" "$TAG" "$WORK/srv/good/$TAG" > /dev/null
cp -R "$WORK/srv/good" "$WORK/srv/bad"
printf '%064d  %s\n' 0 "probbit-$TAG-$TARGET.tar.gz" > "$WORK/srv/bad/$TAG/probbit-$TAG-$TARGET.tar.gz.sha256"
PORT=$("$PY" -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
"$PY" scripts/bench/serve.py "$WORK/srv" "$PORT" > "$WORK/http.log" 2>&1 &
SRV=$!
i=0
until curl -fsS -o /dev/null "http://127.0.0.1:$PORT/" 2> /dev/null; do
    i=$((i + 1)); [ "$i" -lt 50 ] || { echo "http server did not start"; exit 1; }; sleep 0.2
done
BASE="http://127.0.0.1:$PORT"
echo "shell under test: $SH ($(command -v "$SH")); archive: $(ls "$WORK/srv/good/$TAG")"

echo "== 1. piped to $SH, PROBBIT_INSTALL_DIR set, version given without the v"
PROBBIT_DOWNLOAD_BASE="$BASE/good" PROBBIT_VERSION="${TAG#v}" PROBBIT_INSTALL_DIR="$WORK/bin" "$SH" < install.sh
PATH="$WORK/bin:$PATH"
echo "\$ command -v probbit: $(command -v probbit)"
echo "\$ probbit version: $(probbit version)"
probbit demo --tasks 12 | probbit decide > "$WORK/d12.json"
echo "\$ probbit demo --tasks 12 | probbit decide: exit $?, $("$PY" -c 'import json,sys; d=json.load(open(sys.argv[1])); print("verdict", d["verdict"], "| released", len(d["released"]), "of", d["tasks"])' "$WORK/d12.json")"

echo "== 2. default directory, scratch HOME"
HOME="$WORK/home" PROBBIT_DOWNLOAD_BASE="$BASE/good" PROBBIT_VERSION="$TAG" "$SH" install.sh
want="$WORK/home/.local/bin/probbit"
[ -x "$want" ] || { echo "FAIL: expected $want"; exit 1; }
echo "ok: installed to $want ($("$want" version))"

echo "== 3. tampered .sha256 must fail and install nothing"
if PROBBIT_DOWNLOAD_BASE="$BASE/bad" PROBBIT_VERSION="$TAG" PROBBIT_INSTALL_DIR="$WORK/badbin" "$SH" install.sh; then
    echo "FAIL: a wrong checksum was accepted"; exit 1
fi
[ ! -e "$WORK/badbin/probbit" ] || { echo "FAIL: probbit installed despite the checksum mismatch"; exit 1; }
echo "ok: rejected, $WORK/badbin/probbit absent"

echo "== 4. PROBBIT_DOWNLOAD_BASE without PROBBIT_VERSION must fail"
if PROBBIT_DOWNLOAD_BASE="$BASE/good" PROBBIT_INSTALL_DIR="$WORK/bin4" "$SH" install.sh; then echo "FAIL: accepted"; exit 1; fi
echo "ok: rejected"

echo "== 5. failed update preserves an existing install"
before=$("$PY" -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$WORK/bin/probbit")
if PROBBIT_DOWNLOAD_BASE="$BASE/bad" PROBBIT_VERSION="$TAG" PROBBIT_INSTALL_DIR="$WORK/bin" "$SH" install.sh; then echo "FAIL: accepted damaged update"; exit 1; fi
after=$("$PY" -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$WORK/bin/probbit")
[ "$before" = "$after" ] || { echo "FAIL: existing install changed"; exit 1; }

echo "== 6. invalid release path is rejected before download"
if PROBBIT_DOWNLOAD_BASE="$BASE/good" PROBBIT_VERSION='../bad' PROBBIT_INSTALL_DIR="$WORK/bin" "$SH" install.sh; then echo "FAIL: invalid version accepted"; exit 1; fi

echo "== 7. checksummed but unrunnable candidate must preserve the working binary"
mkdir -p "$WORK/unrunnable"
printf '#!/bin/sh\nexit 25\n' > "$WORK/unrunnable/probbit"
chmod +x "$WORK/unrunnable/probbit"
sh scripts/bench/package_like_release.sh "$WORK/unrunnable/probbit" "$TARGET" "$TAG" "$WORK/srv/unrunnable/$TAG" > /dev/null
if PROBBIT_DOWNLOAD_BASE="$BASE/unrunnable" PROBBIT_VERSION="$TAG" PROBBIT_INSTALL_DIR="$WORK/bin" "$SH" install.sh; then echo "FAIL: unrunnable candidate accepted"; exit 1; fi
after=$("$PY" -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$WORK/bin/probbit")
[ "$before" = "$after" ] || { echo "FAIL: unrunnable candidate replaced working binary"; exit 1; }
echo "install.sh: all checks passed ($SH, $TARGET)"
