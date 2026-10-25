#!/bin/bash
# After the loop: freeze best_coupling (best with the baseline world), run the original evaluations, then rerun.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${1:-python3}"
$PY - <<'PYEOF'
import json
b = json.load(open("experiments/config_baseline.json"))
best = json.load(open("experiments/config_best.json"))
bc = dict(best); bc["world"] = b["world"]; bc["reactive"] = b["reactive"]
for name, c in [("config_best.json", best), ("config_best_coupling.json", bc)]:
    with open("experiments/" + name, "w") as fh:
        json.dump(c, fh, indent=1, sort_keys=True); fh.write("\n")
PYEOF
for l in best best_coupling; do $PY experiments/run_arms.py experiments/config_$l.json $l; done
$PY experiments/loop_check.py
$PY experiments/ablation.py
