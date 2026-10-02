#!/usr/bin/env python3
"""One JSON for a `cargo test` run: pass/fail, wall seconds, summed counts of every "test result:" line, machine and toolchain.

    python3 scripts/bench/test_summary.py <cargo test log> <exit code> <seconds> <label> > test-<label>.json
"""
import json, os, re, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from matrix_bench import machine, runner, toolchain  # noqa: E402

log, rc, secs, label = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
text = open(log, encoding="utf-8", errors="replace").read()
res = re.findall(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", text)
tot = {k: sum(int(r[i]) for r in res) for i, k in [(1, "passed"), (2, "failed"), (3, "ignored"), (4, "measured"), (5, "filtered_out")]}
out = {"schema": "pbit-test-matrix/1", "label": label, "date_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
       "cmd": "cargo test --release --locked --workspace --no-fail-fast", "exit": rc, "passed": rc == 0 and tot["failed"] == 0 and len(res) > 0,
       "seconds": secs, "test_binaries": len(res), "totals": tot, "failures": re.findall(r"^---- (\S+) stdout ----", text, re.M),
       "runner": runner(), "machine": machine(), "toolchain": toolchain()}
print(json.dumps(out, indent=1))
