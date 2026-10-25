#!/bin/bash
# V2-res replay: regenerate every evaluation table twice from the frozen config and check byte identity with the
# original run; then verify every strand (A 1,000 + 60 per other arm) with `probbit live verify`. No network, no
# model calls. Binary: $PROBBIT (the run used the 0.11 PR #50 head). Usage: PROBBIT=<probbit> experiments/rerun.sh [PYTHON]
# Python 3.9 to 3.12 (numpy 2.0.2 wheels); PYTHON=<python> runs another interpreter with a warning instead of a refusal.
# Prints MATCH or DIFFERS per table against tables/original.sha256 (the run's committed manifest).
set -euo pipefail
export LC_ALL=C  # byte-order globs, so tables/strand_counts.txt lists the arms in the order of the original run
cd "$(dirname "$0")/.."
PY="${PYTHON:-${1:-python3}}"
# numpy 2.0.2 has wheels for CPython 3.9 to 3.12 only (the run used 3.9.6); outside that range pip builds it from source.
pyv=$("$PY" -c 'import sys; print("%d.%d" % sys.version_info[:2])') || { echo "cannot run $PY" >&2; exit 2; }
case "$pyv" in
  3.9|3.10|3.11|3.12) ;;
  *) if [ -n "${PYTHON:-}" ]; then
       echo "warning: Python $pyv is outside 3.9-3.12 (numpy 2.0.2 wheels); running anyway (PYTHON= override), tables may differ" >&2
     else
       echo "refused: Python $pyv; this rerun needs Python 3.9-3.12 (numpy 2.0.2 wheels): python3.12 -m venv venv (PYTHON=<python> overrides)" >&2; exit 2
     fi ;;
esac
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
# One line per table against the committed manifest (tables/original.sha256), then the two passes against each other.
status=0
while read -r want f; do
  got=$(awk -v f="$f" '$2 == f { print $1 }' tables/rerun-1.sha256)
  if [ "$got" = "$want" ]; then echo "MATCH tables/$f $got"; else echo "DIFFERS tables/$f ${got:-missing} (committed $want)"; status=1; fi
done < tables/original.sha256
if cmp -s tables/rerun-1.sha256 tables/rerun-2.sha256; then echo "two passes identical: $(wc -l < tables/rerun-1.sha256 | tr -d ' ') files"
else echo "two passes DIFFER"; diff tables/rerun-1.sha256 tables/rerun-2.sha256 || true; status=1; fi
if [ "$status" -eq 0 ]; then
  echo "tables: byte-identical (original, rerun 1, rerun 2): $(wc -l < tables/rerun-1.sha256) files"
else
  echo "tables: DIFFER"; exit 1
fi
ls "$S"/*/*/*.strand | xargs -P 8 -n 1 sh -c "\"$B\" live verify \"\$0\" > /dev/null 2>&1 || echo \"verify FAIL \$0\"" > tables/strand_verify_fail.txt
n=$(ls "$S"/*/*/*.strand | wc -l | tr -d ' '); bad=$(wc -l < tables/strand_verify_fail.txt | tr -d ' ')
for d in "$S"/*/*/; do echo "$(basename "$(dirname "$d")")/$(basename "$d") $(ls "$d"*.strand | wc -l | tr -d ' ')"; done > tables/strand_counts.txt
echo "strands: $n verified, $bad failed" | tee tables/strand_verify.txt
(cd "$S" && shasum -a 256 */*/*.strand) > out/v2res-MANIFEST.sha256
[ "$bad" -eq 0 ]
