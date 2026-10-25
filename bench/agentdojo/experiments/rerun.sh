#!/bin/bash
# E2b: recompute every table from tables/episodes.jsonl (and the arm-A strands) twice; the two passes must be
# byte-identical, and each table must match the committed one (git HEAD, or the file on disk before the run outside git).
# No API calls. Usage: experiments/rerun.sh [PYTHON]   (default: venv011/bin/python; Python 3.9 to 3.12 with numpy 1.26.4)
# PYTHON=<python> experiments/rerun.sh runs an interpreter outside that range with a warning instead of a refusal.
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${PYTHON:-${1:-venv011/bin/python}}"
# numpy 1.26.4 has wheels for CPython 3.9 to 3.12 only; a source build on 3.14 shifted three bars.tsv cells.
pyv=$("$PY" -c 'import sys; print("%d.%d" % sys.version_info[:2])') || { echo "cannot run $PY" >&2; exit 2; }
case "$pyv" in
  3.9|3.10|3.11|3.12) ;;
  *) if [ -n "${PYTHON:-}" ]; then
       echo "warning: Python $pyv is outside 3.9-3.12 (numpy 1.26.4 wheels); running anyway (PYTHON= override), tables may differ" >&2
     else
       echo "refused: Python $pyv; this rerun needs Python 3.9-3.12 (numpy 1.26.4 wheels): python3.12 -m venv venv011 (PYTHON=<python> overrides)" >&2; exit 2
     fi ;;
esac
TABLES="results.tsv bars.tsv strands-verify.tsv"
OUT=logs/rerun
rm -rf "$OUT"; mkdir -p "$OUT/1" "$OUT/2" "$OUT/committed"
ref="git HEAD"
for f in $TABLES; do
  git show "HEAD:./tables/$f" > "$OUT/committed/$f" 2>/dev/null || { cp "tables/$f" "$OUT/committed/$f"; ref="the tables on disk before the run"; }
done
for i in 1 2; do
  "$PY" experiments/judge.py > "$OUT/$i/stdout.txt"
  cp tables/results.tsv tables/bars.tsv tables/strands-verify.tsv "$OUT/$i/"
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
