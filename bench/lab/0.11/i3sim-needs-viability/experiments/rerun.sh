#!/bin/bash
# Reproduce every evaluation table twice from the frozen configs and check byte identity; verify the strands.
# The 30-minute loop trajectory (tables/loop_trajectory.tsv) is not re-run here; its endpoints are re-measured in
# tables/loop_check.tsv. No network, no model calls.
# Python 3.9 to 3.12 (numpy 2.0.2 wheels); PYTHON=<python> runs another interpreter with a warning instead of a refusal.
# Prints MATCH or DIFFERS per table against tables/original.sha256 (the run's committed manifest).
set -euo pipefail
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
n=0; bad=0
for f in strands/*/*/*.strand; do
  n=$((n+1))
  if ! "$B" live verify "$f" > /dev/null 2>&1; then bad=$((bad+1)); echo "verify FAIL $f"; fi
done
echo "strands: $n verified, $bad failed" | tee tables/strand_verify.txt
[ "$bad" -eq 0 ]
