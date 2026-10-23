#!/bin/bash
# E2b: recompute every table from tables/episodes.jsonl (and the arm-A strands) twice; the outputs must be byte-identical.
# No API calls. Usage: experiments/rerun.sh [PYTHON]   (default: venv011/bin/python, any python with numpy works)
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${1:-venv011/bin/python}"
OUT=logs/rerun
rm -rf "$OUT"; mkdir -p "$OUT/1" "$OUT/2"
for i in 1 2; do
  "$PY" experiments/judge.py > "$OUT/$i/stdout.txt"
  cp tables/results.tsv tables/bars.tsv tables/strands-verify.tsv "$OUT/$i/"
done
status=0
for f in stdout.txt results.tsv bars.tsv strands-verify.tsv; do
  if cmp -s "$OUT/1/$f" "$OUT/2/$f"; then echo "identical  $f  $(shasum -a 256 "$OUT/1/$f" | cut -d' ' -f1)"; else echo "DIFFERS    $f"; status=1; fi
done
exit $status
