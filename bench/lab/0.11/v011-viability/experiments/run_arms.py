"""Run the arms over seeded episodes and write the tables (simulation; no network, no model calls).

V0.11 v-run: arms A, B, C, D and A-t0 (A with needs.valence.tonic = 0); per episode the registered nulls (100 time-shuffles)
and the V3 margin/worst checks against the world; tables/vbars_<label>.tsv judges V1-V4 as registered (LOOP.md §1, §3).

usage: run_arms.py CONFIG.json LABEL [--seeds A-B] [--strands N] [--procs P]
"""
import copy
import hashlib
import json
import math
import os
import random
import sys
from multiprocessing import Pool

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
LAB = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import persona as P  # noqa: E402
from bridge import Engine, make_event  # noqa: E402
from policies import LEVEL_IDX, policy_affect, policy_deficit_only, policy_random, policy_reactive, view  # noqa: E402
from world import RES, SERVES, World  # noqa: E402

VRUN = os.path.join(LAB, "out")  # strands and generated personas (not committed)
WORK = os.path.join(VRUN, "work")
ARMS = ["A", "B", "C", "D", "A-t0"]
N_STRANDS = {"A": 1000, "B": 60, "C": 60, "D": 60, "A-t0": 60}
NULL_K = 100
BOOT_SEED = 20261021


def persona_path(cfg, tonic=0.8):
    doc = json.dumps(P.build(cfg["persona"], tonic), sort_keys=True)
    h = hashlib.sha256(doc.encode()).hexdigest()[:16]
    os.makedirs(WORK, exist_ok=True)
    path = os.path.join(WORK, "persona-%s.json" % h)
    if not os.path.exists(path):
        tmp = path + ".%d.tmp" % os.getpid()
        P.write(cfg["persona"], tmp, tonic)
        os.replace(tmp, path)
    return path


def rest_stance(ppath, seed):
    """the individual's stance on an empty event: what arm B is fed every step"""
    e = Engine(ppath, seed)
    doc = e.send({"elapsed_hours": 0})
    e.close()
    sv = view(doc)
    sv["deficit"] = {r: 0.0 for r in RES}
    sv["margin"] = {r: 1.0 for r in RES}
    return sv


def expected_idx(odds, order):
    return sum(i * odds[l] for i, l in enumerate(order))


def ranks(a):
    a = np.asarray(a, dtype=float)
    order = np.argsort(a, kind="mergesort")
    r = np.empty(len(a))
    r[order] = np.arange(len(a))
    s = a[order]
    i = 0
    while i < len(s):
        j = i
        while j + 1 < len(s) and s[j + 1] == s[i]:
            j += 1
        if j > i:
            r[order[i:j + 1]] = (i + j) / 2.0
        i = j + 1
    return r


def spearman(x, y):
    if len(x) < 3:
        return 0.0
    rx, ry = ranks(x), ranks(y)
    sx, sy = rx.std(), ry.std()
    if sx == 0 or sy == 0:
        return 0.0
    return float(((rx - rx.mean()) * (ry - ry.mean())).mean() / (sx * sy))


def world_margins(w, cfg):
    """V3: the world's margin per resource on the engine's 6-decimal grid, and its worst (ties: declared order)"""
    m = {r: round(min(1.0, max(0.0, (w.level[r] - cfg["world"]["lo"][r]) / (1.0 - cfg["world"]["lo"][r]))) + 0.0, 6) for r in RES}
    worst = min(RES, key=lambda r: (m[r], RES.index(r)))
    return m, worst


def shuffled(series, dists, seed, sign):
    """null (i): Spearman of NULL_K within-episode permutations of the series against the distance (sign -1: -distance)"""
    if len(series) < 3:
        return [0.0] * NULL_K
    rng = np.random.default_rng(seed)
    x = np.asarray(series, float)
    y = [sign * d for d in dists]
    return [round(spearman(rng.permutation(x), y), 6) for _ in range(NULL_K)]


def run_episode(task):
    arm, seed, cfg, ppath, strand, shadow, rest = task
    w = World(seed, cfg["world"])
    width = {r: cfg["world"]["hi"][r] - cfg["world"]["lo"][r] for r in RES}
    prng = random.Random(7_919 * seed + 3)
    eng = None
    v3_err, v3_mis, v3_n = 0.0, 0, 0
    if arm in ("A", "A0", "A-t0") or shadow:
        if strand and os.path.exists(strand):
            os.remove(strand)
        eng = Engine(ppath, 1, strand)
    sv = None
    deficit = {r: 0.0 for r in RES}
    pres, val, dists = [], [], []
    prio_n = prio_hit = 0
    recon = 0.0
    counts = {}
    while w.viable and w.t < cfg["world"]["steps"]:
        d = w.dist()
        obs = w.obs()
        if arm in ("A", "A-t0"):
            a = policy_affect(sv, obs, cfg["policy"]) if sv else "work_safe"
        elif arm == "A0":
            a = policy_deficit_only(sv, obs, cfg["policy"], width) if sv else "work_safe"
        elif arm == "B":
            a = policy_affect(rest, obs, cfg["policy"])
        elif arm == "C":
            a = policy_reactive(w, obs, cfg["reactive"])
        else:
            a = policy_random(prng)
        counts[a] = counts.get(a, 0) + 1
        mids = [r for r in RES if w.level[r] < (cfg["world"]["lo"][r] + cfg["world"]["hi"][r]) / 2]
        if len(mids) >= 2:
            prio_n += 1
            worst = min(mids, key=lambda r: (d[r], RES.index(r)))
            prio_hit += int(a == SERVES[worst])
        urgency = obs["urgency"]
        rec = w.step(a)
        if eng is not None:
            ev = make_event(rec, urgency, deficit, cfg["bridge"])
            doc = eng.send(ev)
            sv = view(doc)
            deficit = sv["deficit"]
            pres.append(expected_idx(doc["mood"]["pressure"]["odds"], ["low", "mid", "high"]))
            val.append(expected_idx(doc["mood"]["valence"]["odds"], ["down", "even", "up"]))
            dd = w.dist()
            dists.append(min(dd.values()))
            recon = max(recon, max(abs(deficit[r] - (1.0 - w.level[r])) for r in RES))
            wm, ww = world_margins(w, cfg)
            v3_n += 1
            v3_err = max(v3_err, max(abs(sv["margin"][r] - wm[r]) for r in RES))
            v3_mis += int(sv["worst"] != ww or max(abs(sv["margin"][r] - wm[r]) for r in RES) > 1e-6 + 1e-9)
    if eng is not None:
        eng.close()
    sp_p = spearman(pres, [-x for x in dists]) if eng is not None else float("nan")
    sp_v = spearman(val, dists) if eng is not None else float("nan")
    null_p = shuffled(pres, dists, seed * 1000 + 17, -1) if eng is not None else [float("nan")] * NULL_K
    null_v = shuffled(val, dists, seed * 1000 + 29, 1) if eng is not None else [float("nan")] * NULL_K
    return {"arm": arm, "seed": seed, "survived": int(w.viable), "steps": w.t, "value": round(w.value, 6),
            "prio_n": prio_n, "prio_hit": prio_hit, "sp_pressure": round(sp_p, 6), "sp_valence": round(sp_v, 6),
            "recon_err": round(recon, 6), "counts": counts, "null_p": null_p, "null_v": null_v,
            "v3_err": round(v3_err, 9), "v3_mis": v3_mis, "v3_n": v3_n}


def evaluate(cfg, seeds, arms, shadow=False, strands_dir=None, n_strands=None, procs=8, pool=None):
    ppath = persona_path(cfg)
    pt0 = persona_path(cfg, tonic=0.0)
    rest = rest_stance(ppath, 1)
    tasks = []
    for arm in arms:
        for i, s in enumerate(seeds):
            strand = None
            if strands_dir and n_strands and i < n_strands.get(arm, 0):
                os.makedirs(os.path.join(strands_dir, arm), exist_ok=True)
                strand = os.path.join(strands_dir, arm, "ep%04d.strand" % s)
            tasks.append((arm, s, cfg, pt0 if arm == "A-t0" else ppath, strand, shadow, rest))
    own = pool is None
    if own:
        pool = Pool(procs)
    res = pool.map(run_episode, tasks, chunksize=4)
    if own:
        pool.close()
        pool.join()
    out = {arm: [r for r in res if r["arm"] == arm] for arm in arms}
    return out


def summary(eps):
    surv = np.array([e["survived"] for e in eps], dtype=float)
    pn = sum(e["prio_n"] for e in eps)
    ph = sum(e["prio_hit"] for e in eps)
    return {"n": len(eps), "survival": float(surv.mean()), "steps": float(np.mean([e["steps"] for e in eps])),
            "value": float(np.mean([e["value"] for e in eps])), "prio_acc": (ph / pn) if pn else float("nan"),
            "prio_steps": pn,
            "sp_pressure_med": float(np.median([e["sp_pressure"] for e in eps])),
            "sp_valence_med": float(np.median([e["sp_valence"] for e in eps])),
            "recon_max": float(max(e["recon_err"] for e in eps))}


def boot_diff(a_eps, b_eps, stat, n=10000, seed=0):
    rng = np.random.default_rng(seed)
    na, nb = len(a_eps), len(b_eps)
    if stat == "survival":
        xa = np.array([e["survived"] for e in a_eps], float)
        xb = np.array([e["survived"] for e in b_eps], float)
        ia = rng.integers(0, na, (n, na))
        ib = rng.integers(0, nb, (n, nb))
        d = xa[ia].mean(1) - xb[ib].mean(1)
        point = xa.mean() - xb.mean()
    else:  # pooled prioritisation accuracy
        ha = np.array([e["prio_hit"] for e in a_eps], float)
        pa = np.array([e["prio_n"] for e in a_eps], float)
        hb = np.array([e["prio_hit"] for e in b_eps], float)
        pb = np.array([e["prio_n"] for e in b_eps], float)
        ia = rng.integers(0, na, (n, na))
        ib = rng.integers(0, nb, (n, nb))
        with np.errstate(invalid="ignore", divide="ignore"):
            d = ha[ia].sum(1) / pa[ia].sum(1) - hb[ib].sum(1) / pb[ib].sum(1)
        d = d[~np.isnan(d)]
        point = ha.sum() / pa.sum() - hb.sum() / pb.sum() if pa.sum() and pb.sum() else float("nan")
    lo, hi = np.percentile(d, [2.5, 97.5])
    return float(point), float(lo), float(hi)


def f(x, k=4):
    if isinstance(x, float) and math.isnan(x):
        return "nan"
    return ("%%.%df" % k) % x


def write_tables(res, label, outdir):
    os.makedirs(outdir, exist_ok=True)
    S = {arm: summary(eps) for arm, eps in res.items()}
    with open(os.path.join(outdir, "arms_%s.tsv" % label), "w") as fh:
        fh.write("arm\tn\tsurvival\tmean_steps\tmean_value\tprio_acc\tprio_steps\tmedian_sp_pressure\tmedian_sp_valence\tmax_recon_err\n")
        for arm in res:
            s = S[arm]
            fh.write("%s\t%d\t%s\t%s\t%s\t%s\t%d\t%s\t%s\t%s\n" % (arm, s["n"], f(s["survival"]), f(s["steps"], 2),
                     f(s["value"], 3), f(s["prio_acc"]), s["prio_steps"], f(s["sp_pressure_med"]),
                     f(s["sp_valence_med"]), f(s["recon_max"], 6)))
    with open(os.path.join(outdir, "tracking_%s.tsv" % label), "w") as fh:
        fh.write("arm\tmeasure\tp10\tp25\tp50\tp75\tp90\tshare_ge_0.3\n")
        for arm, eps in res.items():
            for m in ["sp_pressure", "sp_valence"]:
                xs = np.array([e[m] for e in eps], float)
                if np.isnan(xs).all():
                    continue
                q = np.percentile(xs, [10, 25, 50, 75, 90])
                fh.write("%s\t%s\t%s\t%f\n" % (arm, m, "\t".join(f(v) for v in q), float((xs >= 0.3).mean())))
    with open(os.path.join(outdir, "episodes_%s.tsv" % label), "w") as fh:
        fh.write("arm\tseed\tsurvived\tsteps\tvalue\tprio_n\tprio_hit\tsp_pressure\tsp_valence\trecon_err\tactions\n")
        for arm, eps in res.items():
            for e in eps:
                acts = ",".join("%s=%d" % (k, e["counts"][k]) for k in sorted(e["counts"]))
                fh.write("%s\t%d\t%d\t%d\t%s\t%d\t%d\t%s\t%s\t%s\t%s\n" % (arm, e["seed"], e["survived"], e["steps"],
                         f(e["value"], 6), e["prio_n"], e["prio_hit"], f(e["sp_pressure"], 6), f(e["sp_valence"], 6),
                         f(e["recon_err"], 6), acts))
    bars = vbars(res, S, label, outdir)
    return S, bars


def null_975(eps, key):
    """null (i): per shuffle index the median over episodes; the 97.5th percentile of those NULL_K medians"""
    m = np.array([e[key] for e in eps], float)  # episodes x NULL_K
    return float(np.percentile(np.median(m, axis=0), 97.5))


def vbars(res, S, label, outdir):
    """V1-V4 as registered (LOOP.md §1); V5-V7 are judged outside this script (cargo tests, the bound suite, rerun.sh)"""
    A, B = S["A"], S["B"]
    n1p, n1v = null_975(res["A"], "null_p"), null_975(res["A"], "null_v")
    t0 = S["A-t0"]["sp_valence_med"]
    sd = boot_diff(res["A"], res["B"], "survival", n=10000, seed=BOOT_SEED)
    ev = sum(e["v3_n"] for e in res["A"]); mis = sum(e["v3_mis"] for e in res["A"]); err = max(e["v3_err"] for e in res["A"])
    bars = {
        "V1": (A["sp_pressure_med"] >= 0.30 and A["sp_pressure_med"] > n1p,
               "median Spearman(pressure, -distance) A %s (bar >= 0.30) vs null(i) 97.5th pct %s; I3 reference 0.832 (habits)" % (f(A["sp_pressure_med"]), f(n1p))),
        "V2": (A["sp_valence_med"] >= 0.30 and A["sp_valence_med"] > n1v and t0 <= 0.10,
               "median Spearman(valence, distance) A %s (bar >= +0.30) vs null(i) 97.5th pct %s; A-t0 median %s (bar <= +0.10); reference -0.149" % (f(A["sp_valence_med"]), f(n1v), f(t0))),
        "V3": (mis == 0 and A["prio_acc"] >= 0.9799,
               "A events %d, margin/worst mismatches %d (max |strand - world margin| %s); prio accuracy A %s (bar >= 0.9799; reference 0.9999); policy A reads no band width" % (ev, mis, f(err, 9), f(A["prio_acc"]))),
        "V4": (sd[1] > 0 and A["survival"] >= 0.515,
               "survival A %s vs B %s, diff %s 95%% CI [%s, %s] (10,000 resamples, seed %d); A survival bar >= 0.515 (reference 0.559)" % (f(A["survival"]), f(B["survival"]), f(sd[0]), f(sd[1]), f(sd[2]), BOOT_SEED)),
    }
    with open(os.path.join(outdir, "vbars_%s.tsv" % label), "w") as fh:
        fh.write("bar\tresult\tevidence\n")
        for k, (ok, e) in bars.items():
            fh.write("%s\t%s\t%s\n" % (k, "PASS" if ok else "FAIL", e))
    with open(os.path.join(outdir, "nulls_%s.tsv" % label), "w") as fh:
        fh.write("arm\tmeasure\tobserved_median\tnull_median_of_medians\tnull_975\n")
        for arm, eps in res.items():
            for key, m in [("null_p", "sp_pressure"), ("null_v", "sp_valence")]:
                mm = np.median(np.array([e[key] for e in eps], float), axis=0)
                fh.write("%s\t%s\t%s\t%s\t%s\n" % (arm, m, f(S[arm][m + "_med"]), f(float(np.median(mm))), f(float(np.percentile(mm, 97.5)))))
    return bars


def main():
    cfg = json.load(open(sys.argv[1]))
    label = sys.argv[2]
    seeds = list(range(1000, 2000))
    n_strands, procs = dict(N_STRANDS), 8
    args = sys.argv[3:]
    for i, a in enumerate(args):
        if a == "--seeds":
            lo, hi = args[i + 1].split("-")
            seeds = list(range(int(lo), int(hi) + 1))
        if a == "--strands":
            n_strands = {k: min(v, int(args[i + 1])) for k, v in N_STRANDS.items()}
        if a == "--procs":
            procs = int(args[i + 1])
    res = evaluate(cfg, seeds, ARMS, shadow=True, strands_dir=os.path.join(VRUN, "strands", label),
                   n_strands=n_strands, procs=procs)
    S, bars = write_tables(res, label, os.path.join(LAB, "tables"))
    for arm in ARMS:
        print(arm, json.dumps({k: round(v, 4) if isinstance(v, float) else v for k, v in S[arm].items()}))
    for k, (ok, ev) in bars.items():
        print(k, "PASS" if ok else "FAIL", ev)


if __name__ == "__main__":
    main()
