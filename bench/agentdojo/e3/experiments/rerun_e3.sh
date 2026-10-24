#!/bin/bash
# E3: recompute the E3 tables from tables/episodes-d.jsonl and E2b's tables/episodes.jsonl twice; the outputs must be
# byte-identical. No API calls, no probbit binary. Usage: experiments/rerun_e3.sh [PYTHON]   (any python with numpy)
# In the repo (bench/agentdojo/e3/) the E2b inputs are found at ../tables/episodes.jsonl and ../experiments/judge.py;
# elsewhere set E2B_EPISODES and E2B_JUDGE.
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${1:-python3}"
OUT=logs/rerun-e3
rm -rf "$OUT"; mkdir -p "$OUT/1" "$OUT/2"
for i in 1 2; do
  "$PY" experiments/judge_e3.py > "$OUT/$i/stdout.txt"
  cp tables/results-e3.tsv tables/bars-e3.tsv "$OUT/$i/"
done
status=0
for f in stdout.txt results-e3.tsv bars-e3.tsv; do
  if cmp -s "$OUT/1/$f" "$OUT/2/$f"; then echo "identical  $f  $(shasum -a 256 "$OUT/1/$f" | cut -d' ' -f1)"; else echo "DIFFERS    $f"; status=1; fi
done
exit $status
