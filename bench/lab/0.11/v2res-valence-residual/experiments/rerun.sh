#!/bin/bash
# V2-res replay: regenerate every evaluation table twice from the frozen config and check byte identity with the
# original run; then verify every strand (A 1,000 + 60 per other arm) with `probbit live verify`. No network, no
# model calls. Binary: $PROBBIT (the run used the 0.11 PR #50 head). Usage: PROBBIT=<probbit> experiments/rerun.sh [PYTHON]
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${1:-python3}"
B="${PROBBIT:?set PROBBIT to a probbit binary}"
S=out/strands
manifest() { (cd tables && shasum -a 256 arms_v2res.tsv tracking_v2res.tsv episodes_v2res.tsv nulls_v2res.tsv); }
[ -f tables/original.sha256 ] || manifest > tables/original.sha256
run_once() {
  rm -rf "$S/v2res"
  $PY experiments/run_arms.py experiments/config_baseline.json v2res ${PROCS:+--procs "$PROCS"} > /dev/null
  manifest
}
run_once > tables/rerun-1.sha256
run_once > tables/rerun-2.sha256
if cmp -s tables/rerun-1.sha256 tables/rerun-2.sha256 && cmp -s tables/original.sha256 tables/rerun-1.sha256; then
  echo "tables: byte-identical (original, rerun 1, rerun 2): $(wc -l < tables/rerun-1.sha256) files"
else
  echo "tables: DIFFER"; diff tables/original.sha256 tables/rerun-1.sha256 || true; diff tables/rerun-1.sha256 tables/rerun-2.sha256 || true; exit 1
fi
ls "$S"/*/*/*.strand | xargs -P 8 -n 1 sh -c "\"$B\" live verify \"\$0\" > /dev/null 2>&1 || echo \"verify FAIL \$0\"" > tables/strand_verify_fail.txt
n=$(ls "$S"/*/*/*.strand | wc -l | tr -d ' '); bad=$(wc -l < tables/strand_verify_fail.txt | tr -d ' ')
for d in "$S"/*/*/; do echo "$(basename "$(dirname "$d")")/$(basename "$d") $(ls "$d"*.strand | wc -l | tr -d ' ')"; done > tables/strand_counts.txt
echo "strands: $n verified, $bad failed" | tee tables/strand_verify.txt
(cd "$S" && shasum -a 256 */*/*.strand) > out/v2res-MANIFEST.sha256
[ "$bad" -eq 0 ]
