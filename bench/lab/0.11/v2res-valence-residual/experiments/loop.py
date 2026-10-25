"""Karpathy-style autoresearch loop: one mutation per cycle, kept only on held-out improvement (simulation).

Primary metric: arm A survival on held-out seeds 100-199. Constraint: A mean value >= 0.8 x C mean value (held-out).
Each cycle also measures seeds 0-99 (logged). Mutation order comes from a seeded RNG, so a run of N cycles replays.

usage: loop.py MINUTES [--max-cycles N]
"""
import copy
import hashlib
import json
import os
import random
import sys
import time
from multiprocessing import Pool

HERE = os.path.dirname(os.path.abspath(__file__))
LAB = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import run_arms as R  # noqa: E402

TUNE = list(range(0, 100))
HELD = list(range(100, 200))
NEEDS = ["budget", "integrity", "recovery"]


def clip(x, lo, hi):
    return round(min(hi, max(lo, x)), 6)


def mutate(cfg, rng):
    c = copy.deepcopy(cfg)
    kinds = ["habit_mid", "habit_high", "pressure_inertia", "gain", "need_want", "gate_level", "use_pursue",
             "risky_when_bold", "send_time_pressure", "dt_hours", "band_lo", "grant_p", "grant_amt"]
    k = kinds[rng.randrange(len(kinds))]
    p, b, pol, w = c["persona"], c["bridge"], c["policy"], c["world"]
    s = rng.choice([-1, 1])
    if k == "habit_mid":
        n = rng.choice(NEEDS)
        p["habit_mid"][n] = clip(p["habit_mid"][n] + 0.05 * s, 0.05, p["habit_high"][n] - 0.05)
        return c, "mapping", "persona.habit_mid.%s -> %s" % (n, p["habit_mid"][n])
    if k == "habit_high":
        n = rng.choice(NEEDS)
        p["habit_high"][n] = clip(p["habit_high"][n] + 0.05 * s, p["habit_mid"][n] + 0.05, 0.95)
        return c, "mapping", "persona.habit_high.%s -> %s" % (n, p["habit_high"][n])
    if k == "pressure_inertia":
        p["pressure_inertia"] = clip(p["pressure_inertia"] + 0.1 * s, 0.0, 0.9)
        return c, "mapping", "persona.pressure_inertia -> %s" % p["pressure_inertia"]
    if k == "gain":
        g = rng.choice(["tp_gain", "fail_gain", "spend_gain", "strain_gain"])
        p[g] = clip(p[g] + 0.2 * s, -1.0, 2.0)
        return c, "mapping", "persona.%s -> %s" % (g, p[g])
    if k == "need_want":
        p["need_want"] = clip(p["need_want"] + 0.5 * s, 0.5, 4.0)
        return c, "mapping", "persona.need_want -> %s" % p["need_want"]
    if k == "gate_level":
        pol["gate_level"] = 3 - pol["gate_level"]
        return c, "policy", "policy.gate_level -> %s" % pol["gate_level"]
    if k == "use_pursue":
        pol["use_pursue"] = not pol["use_pursue"]
        return c, "policy", "policy.use_pursue -> %s" % pol["use_pursue"]
    if k == "risky_when_bold":
        pol["risky_when_bold"] = not pol["risky_when_bold"]
        return c, "policy", "policy.risky_when_bold -> %s" % pol["risky_when_bold"]
    if k == "send_time_pressure":
        b["send_time_pressure"] = not b["send_time_pressure"]
        return c, "mapping", "bridge.send_time_pressure -> %s" % b["send_time_pressure"]
    if k == "dt_hours":
        b["dt_hours"] = clip(b["dt_hours"] * (2.0 if s > 0 else 0.5), 0.0625, 1.0)
        return c, "mapping", "bridge.dt_hours -> %s" % b["dt_hours"]
    if k == "band_lo":
        r = rng.choice(NEEDS)
        w["lo"][r] = clip(w["lo"][r] + 0.03 * s, 0.05, 0.4)
        return c, "world", "world.lo.%s -> %s" % (r, w["lo"][r])
    if k == "grant_p":
        w["grant_p_request"] = clip(w["grant_p_request"] + 0.05 * s, 0.05, 0.6)
        return c, "world", "world.grant_p_request -> %s" % w["grant_p_request"]
    lo, hi = w["grant_request_amt"]
    w["grant_request_amt"] = [clip(lo + 0.05 * s, 0.05, 0.5), clip(hi + 0.05 * s, 0.1, 0.6)]
    return c, "world", "world.grant_request_amt -> %s" % w["grant_request_amt"]


def measure(cfg, seeds, pool, c_cache):
    resA = R.evaluate(cfg, seeds, ["A"], pool=pool)["A"]
    wkey = hashlib.sha256((json.dumps(cfg["world"], sort_keys=True) + json.dumps(cfg["reactive"], sort_keys=True)
                           + str(seeds[0])).encode()).hexdigest()
    if wkey not in c_cache:
        c_cache[wkey] = R.summary(R.evaluate(cfg, seeds, ["C"], pool=pool)["C"])
    sA = R.summary(resA)
    sC = c_cache[wkey]
    return {"surv": sA["survival"], "value": sA["value"], "c_value": sC["value"], "c_surv": sC["survival"],
            "track": sA["sp_pressure_med"], "prio": sA["prio_acc"]}


def ok(m):
    return m["value"] >= 0.8 * m["c_value"]


def main():
    minutes = float(sys.argv[1])
    max_cycles = None
    if "--max-cycles" in sys.argv:
        max_cycles = int(sys.argv[sys.argv.index("--max-cycles") + 1])
    out_tsv = os.path.join(LAB, "tables", "loop_trajectory.tsv")
    os.makedirs(os.path.dirname(out_tsv), exist_ok=True)
    rng = random.Random(20261020)
    base = json.load(open(os.path.join(HERE, "config_baseline.json")))
    pool = Pool(8)
    cache = {}
    t0 = time.time()
    cur = base
    cur_t = measure(cur, TUNE, pool, cache)
    cur_h = measure(cur, HELD, pool, cache)
    base_h = dict(cur_h)
    rows = []
    hdr = ("cycle\tkind\tmutation\ttune_surv_before\ttune_surv_after\theld_surv_before\theld_surv_after\t"
           "held_value_after\theld_0.8C\theld_track_after\tkept\n")
    with open(out_tsv, "w") as fh:
        fh.write(hdr)
        fh.write("0\tbaseline\t-\t%.2f\t%.2f\t%.2f\t%.2f\t%.3f\t%.3f\t%.4f\tbase\n" % (
            cur_t["surv"], cur_t["surv"], cur_h["surv"], cur_h["surv"], cur_h["value"], 0.8 * cur_h["c_value"],
            cur_h["track"]))
    md = open(os.path.join(LAB, "LOOP.md"), "a")
    md.write("\nLoop started %s UTC, budget %.0f min. Baseline: tune A survival %.2f; held-out A survival %.2f, "
             "value %.3f (0.8 x C = %.3f), tracking median %.4f.\n\n" % (
                 time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime()), minutes, cur_t["surv"], cur_h["surv"],
                 cur_h["value"], 0.8 * cur_h["c_value"], cur_h["track"]))
    md.write("| cycle | kind | mutation | tune surv before → after | held surv before → after | held value / 0.8·C | kept |\n")
    md.write("|---|---|---|---|---|---|---|\n")
    md.flush()
    cycle = 0
    kept_list = []
    while time.time() - t0 < minutes * 60 and (max_cycles is None or cycle < max_cycles):
        cycle += 1
        cand, kind, desc = mutate(cur, rng)
        mt = measure(cand, TUNE, pool, cache)
        mh = measure(cand, HELD, pool, cache)
        keep = mh["surv"] > cur_h["surv"] and ok(mh)
        line = "%d\t%s\t%s\t%.2f\t%.2f\t%.2f\t%.2f\t%.3f\t%.3f\t%.4f\t%s\n" % (
            cycle, kind, desc, cur_t["surv"], mt["surv"], cur_h["surv"], mh["surv"], mh["value"],
            0.8 * mh["c_value"], mh["track"], "yes" if keep else "no")
        with open(out_tsv, "a") as fh:
            fh.write(line)
        md.write("| %d | %s | `%s` | %.2f → %.2f | %.2f → %.2f | %.2f / %.2f | %s |\n" % (
            cycle, kind, desc, cur_t["surv"], mt["surv"], cur_h["surv"], mh["surv"], mh["value"],
            0.8 * mh["c_value"], "**yes**" if keep else "no"))
        md.flush()
        if keep:
            cur, cur_t, cur_h = cand, mt, mh
            kept_list.append((cycle, kind, desc))
            json.dump(cur, open(os.path.join(HERE, "config_best.json"), "w"), indent=1, sort_keys=True)
    if not kept_list:
        json.dump(cur, open(os.path.join(HERE, "config_best.json"), "w"), indent=1, sort_keys=True)
    md.write("\nLoop stopped %s UTC after %d cycles (%.1f min). Kept: %d (%s). Held-out A survival baseline %.2f → best "
             "%.2f.\n" % (time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime()), cycle, (time.time() - t0) / 60,
                          len(kept_list), "; ".join("c%d %s %s" % k for k in kept_list) or "none", base_h["surv"],
                          cur_h["surv"]))
    md.close()
    pool.close()
    pool.join()
    print("cycles", cycle, "kept", kept_list, "held", base_h["surv"], "->", cur_h["surv"])


if __name__ == "__main__":
    main()
