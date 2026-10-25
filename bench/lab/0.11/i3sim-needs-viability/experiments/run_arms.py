"""Run the four arms over seeded episodes and write the tables (simulation; no network, no model calls).

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

WORK = os.path.join(LAB, "work")  # generated personas (not committed)
ARMS = ["A", "B", "C", "D"]


def persona_path(cfg):
    doc = json.dumps(P.build(cfg["persona"]), sort_keys=True)
    h = hashlib.sha256(doc.encode()).hexdigest()[:16]
    os.makedirs(WORK, exist_ok=True)
    path = os.path.join(WORK, "persona-%s.json" % h)
    if not os.path.exists(path):
        tmp = path + ".%d.tmp" % os.getpid()
        P.write(cfg["persona"], tmp)
        os.replace(tmp, path)
    return path


def rest_stance(ppath, seed):
    """the individual's stance on an empty event: what arm B is fed every step"""
    e = Engine(ppath, seed)
    doc = e.send({"elapsed_hours": 0})
    e.close()
    sv = view(doc)
    sv["deficit"] = {r: 0.0 for r in RES}
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


def run_episode(task):
    arm, seed, cfg, ppath, strand, shadow, rest = task
    w = World(seed, cfg["world"])
    width = {r: cfg["world"]["hi"][r] - cfg["world"]["lo"][r] for r in RES}
    prng = random.Random(7_919 * seed + 3)
    eng = None
    if arm in ("A", "A0") or shadow:
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
        if arm == "A":
            a = policy_affect(sv, obs, cfg["policy"], width) if sv else "work_safe"
        elif arm == "A0":
            a = policy_deficit_only(sv, obs, cfg["policy"], width) if sv else "work_safe"
        elif arm == "B":
            a = policy_affect(rest, obs, cfg["policy"], width)
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
    if eng is not None:
        eng.close()
    sp_p = spearman(pres, [-x for x in dists]) if eng is not None else float("nan")
    sp_v = spearman(val, dists) if eng is not None else float("nan")
    return {"arm": arm, "seed": seed, "survived": int(w.viable), "steps": w.t, "value": round(w.value, 6),
            "prio_n": prio_n, "prio_hit": prio_hit, "sp_pressure": round(sp_p, 6), "sp_valence": round(sp_v, 6),
            "recon_err": round(recon, 6), "counts": counts}


def evaluate(cfg, seeds, arms, shadow=False, strands_dir=None, n_strands=0, procs=8, pool=None):
    ppath = persona_path(cfg)
    rest = rest_stance(ppath, 1)
    tasks = []
    for arm in arms:
        for i, s in enumerate(seeds):
            strand = None
            if strands_dir and i < n_strands and (arm == "A" or shadow):
                os.makedirs(os.path.join(strands_dir, arm), exist_ok=True)
                strand = os.path.join(strands_dir, arm, "ep%04d.strand" % s)
            tasks.append((arm, s, cfg, ppath, strand, shadow, rest))
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
    bars = {}
    if all(a in res for a in "ABC"):
        A, B, C = S["A"], S["B"], S["C"]
        sd = boot_diff(res["A"], res["B"], "survival")
        pd = boot_diff(res["A"], res["B"], "prio")
        bars["n1"] = (A["sp_pressure_med"] >= 0.3, "median Spearman(pressure, -dist) in A = %s (bar >= 0.3); valence median Spearman(valence, dist) = %s" % (f(A["sp_pressure_med"]), f(A["sp_valence_med"])))
        n2 = sd[1] > 0 and A["value"] >= 0.8 * C["value"]
        bars["n2"] = (n2, "survival A %s vs B %s, diff %s 95%% CI [%s, %s]; value A %s vs 0.8 x C %s (C %s)" % (
            f(A["survival"]), f(B["survival"]), f(sd[0]), f(sd[1]), f(sd[2]), f(A["value"], 3),
            f(0.8 * C["value"], 3), f(C["value"], 3)))
        n3 = A["prio_acc"] >= 0.7 and pd[1] > 0
        bars["n3"] = (n3, "prio accuracy A %s (bar >= 0.7) vs B %s, diff %s 95%% CI [%s, %s]" % (
            f(A["prio_acc"]), f(B["prio_acc"]), f(pd[0]), f(pd[1]), f(pd[2])))
        with open(os.path.join(outdir, "bars_%s.tsv" % label), "w") as fh:
            fh.write("bar\tpass\tevidence\n")
            for k, (ok, ev) in bars.items():
                fh.write("%s\t%s\t%s\n" % (k, "PASS" if ok else "FAIL", ev))
    return S, bars


def main():
    cfg = json.load(open(sys.argv[1]))
    label = sys.argv[2]
    seeds = list(range(1000, 2000))
    n_strands, procs = 20, 8
    args = sys.argv[3:]
    for i, a in enumerate(args):
        if a == "--seeds":
            lo, hi = args[i + 1].split("-")
            seeds = list(range(int(lo), int(hi) + 1))
        if a == "--strands":
            n_strands = int(args[i + 1])
        if a == "--procs":
            procs = int(args[i + 1])
    res = evaluate(cfg, seeds, ARMS, shadow=True, strands_dir=os.path.join(LAB, "strands", label),
                   n_strands=n_strands, procs=procs)
    S, bars = write_tables(res, label, os.path.join(LAB, "tables"))
    for arm in ARMS:
        print(arm, json.dumps({k: round(v, 4) if isinstance(v, float) else v for k, v in S[arm].items()}))
    for k, (ok, ev) in bars.items():
        print(k, "PASS" if ok else "FAIL", ev)


if __name__ == "__main__":
    main()
