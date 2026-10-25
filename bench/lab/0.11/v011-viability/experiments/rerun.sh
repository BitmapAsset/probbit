#!/bin/bash
# V0.11 v-run replay (V7): regenerate every evaluation table twice from the frozen config and check byte identity with the
# original run; then verify every strand (all A strands + 60 per other arm) with `probbit live verify`. No network, no
# model calls. Binary: $PROBBIT (the run used the 0.11 PR #50 head). Usage: PROBBIT=<probbit> experiments/rerun.sh [PYTHON]
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${1:-python3}"
B="${PROBBIT:?set PROBBIT to a probbit binary}"
S=out/strands
manifest() { (cd tables && shasum -a 256 arms_vrun.tsv tracking_vrun.tsv episodes_vrun.tsv vbars_vrun.tsv nulls_vrun.tsv); }
[ -f tables/original.sha256 ] || manifest > tables/original.sha256
run_once() {
  rm -rf "$S"
  $PY experiments/run_arms.py experiments/config_baseline.json vrun > /dev/null
  manifest
}
run_once > tables/rerun-1.sha256
run_once > tables/rerun-2.sha256
if cmp -s tables/rerun-1.sha256 tables/rerun-2.sha256 && cmp -s tables/original.sha256 tables/rerun-1.sha256; then
  echo "tables: byte-identical (original, rerun 1, rerun 2): $(wc -l < tables/rerun-1.sha256) files"
else
  echo "tables: DIFFER"; diff tables/original.sha256 tables/rerun-1.sha256 || true; diff tables/rerun-1.sha256 tables/rerun-2.sha256 || true; exit 1
fi
ls "$S"/vrun/*/*.strand | xargs -P 8 -n 1 sh -c "\"$B\" live verify \"\$0\" > /dev/null 2>&1 || echo \"verify FAIL \$0\"" > tables/strand_verify_fail.txt
n=$(ls "$S"/vrun/*/*.strand | wc -l | tr -d ' '); bad=$(wc -l < tables/strand_verify_fail.txt | tr -d ' ')
for a in A B C D A-t0; do echo "$a $(ls "$S"/vrun/$a/*.strand | wc -l | tr -d ' ')"; done > tables/strand_counts.txt
echo "strands: $n verified, $bad failed" | tee tables/strand_verify.txt
(cd "$S" && shasum -a 256 vrun/*/*.strand) > out/vrun-MANIFEST.sha256
[ "$bad" -eq 0 ]
