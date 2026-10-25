#!/bin/bash
# E3: recompute the E3 tables from tables/episodes-d.jsonl and E2b's tables/episodes.jsonl twice; the two passes must be
# byte-identical, and each table must match the committed one (git HEAD, or the file on disk before the run outside git).
# No API calls, no probbit binary. Usage: experiments/rerun_e3.sh [PYTHON]   (default python3; Python 3.9 to 3.12 with
# numpy 1.26.4). PYTHON=<python> experiments/rerun_e3.sh runs an interpreter outside that range with a warning instead.
# In the repo (bench/agentdojo/e3/) the E2b inputs are found at ../tables/episodes.jsonl and ../experiments/judge.py;
# elsewhere set E2B_EPISODES and E2B_JUDGE.
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${PYTHON:-${1:-python3}}"
# numpy 1.26.4 has wheels for CPython 3.9 to 3.12 only; a source build on 3.14 shifted E2b's bars.tsv cells.
pyv=$("$PY" -c 'import sys; print("%d.%d" % sys.version_info[:2])') || { echo "cannot run $PY" >&2; exit 2; }
case "$pyv" in
  3.9|3.10|3.11|3.12) ;;
  *) if [ -n "${PYTHON:-}" ]; then
       echo "warning: Python $pyv is outside 3.9-3.12 (numpy 1.26.4 wheels); running anyway (PYTHON= override), tables may differ" >&2
     else
       echo "refused: Python $pyv; this rerun needs Python 3.9-3.12 (numpy 1.26.4 wheels): python3.12 -m venv venv011 (PYTHON=<python> overrides)" >&2; exit 2
     fi ;;
esac
TABLES="results-e3.tsv bars-e3.tsv"
OUT=logs/rerun-e3
rm -rf "$OUT"; mkdir -p "$OUT/1" "$OUT/2" "$OUT/committed"
ref="git HEAD"
for f in $TABLES; do
  git show "HEAD:./tables/$f" > "$OUT/committed/$f" 2>/dev/null || { cp "tables/$f" "$OUT/committed/$f"; ref="the tables on disk before the run"; }
done
for i in 1 2; do
  "$PY" experiments/judge_e3.py > "$OUT/$i/stdout.txt"
  cp tables/results-e3.tsv tables/bars-e3.tsv "$OUT/$i/"
done
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
status=0
for f in stdout.txt $TABLES; do
  if cmp -s "$OUT/1/$f" "$OUT/2/$f"; then echo "two passes identical  $f  $(sha "$OUT/1/$f")"; else echo "two passes DIFFER     $f"; status=1; fi
done
echo "committed tables: $ref"
for f in $TABLES; do
  got=$(sha "$OUT/1/$f"); want=$(sha "$OUT/committed/$f")
  if [ "$got" = "$want" ]; then echo "MATCH tables/$f $got"; else echo "DIFFERS tables/$f $got (committed $want)"; status=1; fi
done
exit $status
