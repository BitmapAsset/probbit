"""P7 load (RFC 0002 part 2): bars l1, l2 and l3 (l4 in replay.py, l5 in p0.py).

    python3 load.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "load"
FIXTURES = ["load.yaml"]
HL, CAP = 72.0, 4.0
QUIET_GAPS = [1, 5, 12, 24, 30]


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def trajectory(persona, events, tmp, tag):
    strand = os.path.join(tmp, tag + ".strand")
    code, docs, sts, err = common.live_states(persona, events, 0, strand)
    if code or len(sts) != len(events):
        raise common.SetupError("load life %s failed: %s" % (tag, err[-200:]))
    common.LIVES.setdefault("load", []).append((persona, 0, common.Record(events, docs, strand)))
    return [s["load"]["l"] for s in sts]


def metric(bars, tmp):
    b = bars["load"]
    p = common.fixture("load.yaml")
    trajs = {row["pattern"]: trajectory(p, row["events"], tmp, row["pattern"]) for row in common.stream("patterns")}
    names = sorted(trajs)
    distinct = all(any(abs(x - y) > b["l1_differ_above"] for x, y in zip(trajs[a], trajs[c]))
                   for i, a in enumerate(names) for c in names[i + 1:])
    r = {"l1_final": {k: v[-1] for k, v in trajs.items()}, "l1_pairwise_distinct": distinct}
    crit = {"elapsed_hours": 0, "criticism": True, "src": "human:principal"}
    events = [crit] * 4 + [{"elapsed_hours": h, "src": "env:host"} for h in QUIET_GAPS]
    traj = trajectory(p, events, tmp, "recovery")
    l0, worst, total = traj[3], 0.0, 0.0
    for k, h in enumerate(QUIET_GAPS):
        total += h
        worst = max(worst, abs(traj[4 + k] - l0 * 0.5 ** (total / HL)))
    r["l2_L0"], r["l2_max_abs_diff"] = l0, worst
    hits = trajs["criticism"]
    r["l3_max"], r["l3_final"] = max(hits), hits[-1]
    r["pass"] = {
        "l1": distinct,
        "l2": l0 > 0 and worst <= b["l2_tol"],
        "l3": max(hits) <= CAP and abs(hits[-1] - CAP) <= 1e-6,
    }
    r["scope"] = "load.yaml seed 0; patterns streams (300 events each); 4 criticisms then quiet gaps of 1, 5, 12, 24, 30 h"
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
