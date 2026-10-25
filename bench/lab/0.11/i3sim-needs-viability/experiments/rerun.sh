#!/bin/bash
# Reproduce every evaluation table twice from the frozen configs and check byte identity; verify the strands.
# The 30-minute loop trajectory (tables/loop_trajectory.tsv) is not re-run here; its endpoints are re-measured in
# tables/loop_check.tsv. No network, no model calls.
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${1:-python3}"
B="${PROBBIT:?set PROBBIT to a probbit binary}"
LABELS="baseline best best_coupling"
manifest() { (cd tables && shasum -a 256 $(for l in $LABELS; do echo arms_$l.tsv tracking_$l.tsv episodes_$l.tsv bars_$l.tsv; done) loop_check.tsv ablation_baseline.tsv); }
[ -f tables/original.sha256 ] || manifest > tables/original.sha256
run_once() {
  rm -rf strands
  for l in $LABELS; do $PY experiments/run_arms.py experiments/config_$l.json $l ${PROCS:+--procs "$PROCS"} > /dev/null; done
  $PY experiments/loop_check.py
  $PY experiments/ablation.py > /dev/null
  manifest
}
run_once > tables/rerun-1.sha256
run_once > tables/rerun-2.sha256
if cmp -s tables/rerun-1.sha256 tables/rerun-2.sha256 && cmp -s tables/original.sha256 tables/rerun-1.sha256; then
  echo "tables: byte-identical (original, rerun 1, rerun 2): $(wc -l < tables/rerun-1.sha256) files"
else
  echo "tables: DIFFER"; diff tables/original.sha256 tables/rerun-1.sha256 || true; diff tables/rerun-1.sha256 tables/rerun-2.sha256 || true; exit 1
fi
n=0; bad=0
for f in strands/*/*/*.strand; do
  n=$((n+1))
  if ! "$B" live verify "$f" > /dev/null 2>&1; then bad=$((bad+1)); echo "verify FAIL $f"; fi
done
echo "strands: $n verified, $bad failed" | tee tables/strand_verify.txt
[ "$bad" -eq 0 ]
