"""Metric a re-measured with RFC Design B, and its reported control (Python 3.9, standard library only).

    python3 twins_addressed.py [--out FILE]

The frozen fixture, streams, probes, seeds and bars of metric a (twins.py), three ways:
- `frozen`: as run.py runs metric a (rewards unaddressed: feedback pays the previous stance).
- `addressed` (Design B): fixtures/sensed.yaml with the credit block the benchmark names (max_open 8, half_life_events 32),
  added at run time, and each room's feedback addressed to the decision it was about, as the shared fixtures describe: room
  A's praise names the decision awaited 2 events earlier, room B's criticism the one awaited 3 events earlier (those
  decisions carry `await: true`). Nothing else in the streams changes.
- `plain` (reported, not gated): fixtures/sensed-plain.yaml, the same lives with each reading sent as plain 0.8 number
  inputs: the push of its z against the fixed prior (the 0.5 dead zone and the cap of 6 of a sense with zero spreads, a
  normal that never moves), tool_fail and tempo also raw for the habits; rewards unaddressed.
Each gets metric a's statistics and bars, and the mean learned caution deltas per room. Exit 0 when the addressed run passes
a1 and a2, 1 when it does not, 2 on a setup error.
"""
import json
import math
import os
import shutil
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "fixtures"))
sys.path.insert(0, HERE)
import common  # noqa: E402
import twins  # noqa: E402
from fixtures import lives  # noqa: E402

LAG = {"A": ("praise", 2), "B": ("criticism", 3)}
CREDIT_BLOCK = "  credit:\n    max_open: 8\n    half_life_events: 32\n"
PRIOR = {"tool_fail": (0.05, 0.05, False), "tool_latency": (0.405, 0.1, True), "tempo": (1.0, 1.0, False), "silence": (0.69, 0.5, True)}
ZCAP, DEAD_ZONE, VAR_FLOOR = 6.0, 0.5, 0.01


def credit_persona(tmp):
    """fixtures/sensed.yaml with the credit block under its learning block"""
    with open(twins.PERSONA, encoding="utf-8") as f:
        src = f.read()
    anchor = "  total_cap: 0.4\nreward_from"
    if src.count(anchor) != 1:
        raise common.SetupError("sensed.yaml: the learning block's end was not found")
    path = os.path.join(tmp, "sensed-credit.yaml")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(src.replace(anchor, "  total_cap: 0.4\n" + CREDIT_BLOCK + "reward_from"))
    return path


def addressed(room):
    flag, lag = LAG[room]
    evs = [dict(e) for e in lives.load("room-" + room.lower())]
    for i, e in enumerate(evs):
        if e.get(flag):
            evs[i - lag]["await"] = True
            e["for"] = i - lag
    return evs + lives.load("tail")


def plain(ev):
    """An event with its readings as plain inputs: each sense's push against the fixed prior; tool_fail and tempo also raw"""
    e = {k: v for k, v in ev.items() if k != "senses"}
    for sid, r in sorted((ev.get("senses") or {}).items()):
        v = r["v"] if isinstance(r, dict) else r
        x = (1.0 if v else 0.0) if isinstance(v, bool) else float(v)
        mean, var, log = PRIOR[sid]
        x = math.log1p(x) if log else x
        z = max(-ZCAP, min(ZCAP, (x - mean) / math.sqrt(var + VAR_FLOOR)))
        zd = math.copysign(max(abs(z) - DEAD_ZONE, 0.0), z)
        if zd > 0:
            e[sid + "_up"] = common.r6(zd)
        elif zd < 0:
            e[sid + "_down"] = common.r6(-zd)
        if sid == "tool_fail":
            e["tool_fail"] = bool(v)
        if sid == "tempo":
            e["tempo"] = v
    return e


class Probes:
    """The frozen probes, mapped"""

    def __init__(self, f):
        self.f = f

    def load(self, name):
        return [self.f(e) for e in lives.load(name)]


def learned(lv, room):
    """Mean learned caution deltas [bold, measured, careful] over the individuals' final states in a room"""
    rows = []
    for (s, r), life in lv.items():
        if r == room:
            with open(life["state"], encoding="utf-8") as f:
                rows.append(json.load(f)["learned"]["caution"])
    return [common.r6(sum(x[i] for x in rows) / len(rows)) for i in range(3)]


def measure(tmp, name, persona, events, probes):
    twins.events, twins.lives = events, probes
    sub = os.path.join(tmp, name)
    os.makedirs(sub)
    lv = twins.run_lives(sub, persona=persona)
    with open(os.path.join(common.FIXTURES, "bars.json"), encoding="utf-8") as f:
        r = twins.metric(lv, json.load(f))
    r["learned_caution_mean"] = {room: learned(lv, room) for room in twins.ROOMS}
    def failed(p):
        x = (p.get("senses") or {}).get("tool_fail")
        return (x.get("v") if isinstance(x, dict) else x) is True
    fail_probe = [i for i, p in enumerate(lives.load("probes")) if failed(p)]
    split = {"with_failure": 0, "without": 0}
    for s in twins.SEEDS:
        for i, (a, b) in enumerate(zip(lv[(s, "A")]["probes"], lv[(s, "B")]["probes"])):
            if twins.action(a) != twins.action(b):
                split["with_failure" if i in fail_probe else "without"] += 1
    r["differing_actions"] = {k: "%d/%d" % (v, len(twins.SEEDS) * (len(fail_probe) if k == "with_failure" else 20 - len(fail_probe))) for k, v in split.items()}
    return r


def main(argv):
    out = argv[argv.index("--out") + 1] if "--out" in argv else None
    events0, lives0 = twins.events, twins.lives
    tmp = tempfile.mkdtemp(prefix="pbit-twins-b-")
    try:
        common.binary()
        res = {"frozen": measure(tmp, "frozen", twins.PERSONA, events0, lives0),
               "addressed": measure(tmp, "addressed", credit_persona(tmp), addressed, lives0),
               "plain": measure(tmp, "plain", os.path.join(common.FIXTURES, "sensed-plain.yaml"),
                                lambda room: [plain(e) for e in events0(room)], Probes(plain))}
    except common.SetupError as e:
        common.fail(str(e))
    finally:
        twins.events, twins.lives = events0, lives0
        shutil.rmtree(tmp, ignore_errors=True)
    res["plain"]["note"] = "reported, not gated"
    if out:
        with open(out, "w", encoding="utf-8", newline="\n") as f:
            f.write(json.dumps(res, indent=1, sort_keys=True) + "\n")
    for k in ("frozen", "addressed", "plain"):
        r = res[k]
        print("%-9s a1 d_life %.6f [%.6f, %.6f]  a2 tv_life %.6f vs min(tv_seed_A %.6f, tv_seed_B %.6f)  pass %s  learned %s  differ %s" % (
            k, r["d_life"], r["d_life_ci95"][0], r["d_life_ci95"][1], r["tv_life"], r["tv_seed_A"], r["tv_seed_B"],
            json.dumps(r["pass"], sort_keys=True), json.dumps(r["learned_caution_mean"], sort_keys=True), json.dumps(r["differing_actions"], sort_keys=True)))
    return 0 if all(res["addressed"]["pass"].values()) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
