#!/usr/bin/env python3
"""R19 P1.4 calibration runner (stdlib only). Runs the frozen stress corpus (probbit-cli/tests/stress/) through the release
binary at defaults AND with the sampler forced (`--mode sample` / `--op sample`), seeds 1..S, and scores every answer
against an oracle that does not use probbit's inference: the occupancy count DP via elementary symmetric polynomials (router
files), brute-force enumeration with caps (IR programs with k^n <= 2^20), a log-space transfer matrix (paths).

Per family and mode it reports: whole answers (`exact` or `diagnostics_passed`), FALSE whole answers (any variable off by
more than 0.05 TV), released items, WRONG released items (> 0.05 TV), partial and refused counts (refusal rate), runtime
median (the answer's `ms`). The header records the commit, the gate version, the gate thresholds read from
probbit-ir/src/lib.rs (frozen: see CONTRIBUTING, "Calibration"), the machine, the load before / after and every command.

    cargo build --release -p probbit-cli && python3 bench/calibrate.py [--seeds 20] [--jobs 2] [--out results.jsonl]
"""
import argparse, itertools, json, math, os, platform, re, statistics, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PROBBIT = os.environ.get("PROBBIT_BIN") or os.path.join(ROOT, "target", "release", "probbit")  # PROBBIT_BIN: compare another binary
STRESS = os.path.join(ROOT, "probbit-cli", "tests", "stress")
TOL = 0.05

def lse(xs):
    xs = list(xs); m = max(xs)
    return m if m == -math.inf else m + math.log(sum(math.exp(x - m) for x in xs))

def router_oracle(doc):
    """P(task on each worker) for two workers, one group, uniform affinity, non-binding caps (count DP over #tasks on B)."""
    ws = [w["id"] for w in doc["workers"]]; assert len(ws) == 2
    ts = doc["tasks"]; n = len(ts); lam = doc.get("affinity", 0.0)
    assert all(w["cap"] >= n for w in doc["workers"]) and len({t.get("group") for t in ts}) == 1
    r = [math.exp(t["scores"][ws[1]] - t["scores"][ws[0]]) for t in ts]
    c2 = lambda k: k * (k - 1) / 2
    aff = [math.exp(lam * (c2(k) + c2(n - k))) for k in range(n + 1)]
    def esp(skip):
        e = [1.0] + [0.0] * n
        for j, rj in enumerate(r):
            if j == skip: continue
            for k in range(n, 0, -1): e[k] += rj * e[k - 1]
        return e
    allv = esp(None); z = sum(allv[k] * aff[k] for k in range(n + 1)); out = {}
    for i, t in enumerate(ts):
        e = esp(i); pb = r[i] * sum(e[k - 1] * aff[k] for k in range(1, n + 1)) / z
        out[t["id"]] = {ws[0]: 1 - pb, ws[1]: pb}
    return out

def ir_parts(doc):
    vals = doc["values"]; k = len(vals); vi = {v: q for q, v in enumerate(vals)}
    ids = [v.get("id", f"x{i}") for i, v in enumerate(doc["vars"])]; xi = {x: i for i, x in enumerate(ids)}; n = len(ids)
    h = [[0.0] * k for _ in range(n)]; allowed = [[True] * k for _ in range(n)]
    for i, v in enumerate(doc["vars"]):
        if "allowed" in v: allowed[i] = [vals[q] in v["allowed"] for q in range(k)]
        for f in v.get("forbid", []): allowed[i][vi[f]] = False
        for val, x in v.get("h", {}).items(): h[i][vi[val]] = x
        if "clamp" in v: allowed[i] = [q == vi[v["clamp"]] and allowed[i][q] for q in range(k)]
    pairs = []
    for p in doc.get("pairs", []):
        t = [[p["potts"] if a == b else 0.0 for b in range(k)] for a in range(k)] if "potts" in p else p["table"]
        pairs.append((xi[p["i"]], xi[p["j"]], t))
    caps = []
    for c in doc.get("caps", []):
        if "members" in c: mem = [(xi[a], vi[b]) for a, b in c["members"]]
        else:
            over = [xi[x] for x in c["vars"]] if "vars" in c else range(n)
            mem = [(i, vi[c["value"]]) for i in over if allowed[i][vi[c["value"]]]]
        caps.append((mem, c.get("limit", 1 << 60), c.get("min", 0)))
    return ids, vals, n, k, h, allowed, pairs, caps

def v2_ok(doc, ids, vals):
    """R19.5 IR constructs checked by their DIRECT meaning (not probbit's cap lowering): all_different, implies, tables, precedes."""
    xi = {x: i for i, x in enumerate(ids)}; vi = {v: q for q, v in enumerate(vals)}; tests = []
    for c in doc.get("all_different", []): o = [xi[x] for x in c["vars"]]; tests.append(lambda x, o=o: len({x[i] for i in o}) == len(o))
    for c in doc.get("implies", []):
        a, b, s_ = xi[c["if"]["var"]], vi[c["if"]["value"]], {vi[w] for w in c["then"]["in"]}; y = xi[c["then"]["var"]]
        tests.append(lambda x, a=a, b=b, y=y, s_=s_: x[a] != b or x[y] in s_)
    for c in doc.get("tables", []):
        o = [xi[x] for x in c["vars"]]; ts = {tuple(vi[w] for w in t) for t in c.get("forbid", c.get("allow", []))}; allow = "allow" in c
        tests.append(lambda x, o=o, ts=ts, allow=allow: (tuple(x[i] for i in o) in ts) == allow)
    for c in doc.get("precedes", []):
        a, b, g = xi[c["before"]], xi[c["after"]], c.get("gap", 1); tests.append(lambda x, a=a, b=b, g=g: x[b] >= x[a] + g)
    return lambda x: all(t(x) for t in tests)

def brute_oracle(doc):
    ids, vals, n, k, h, allowed, pairs, caps = ir_parts(doc); assert k ** n <= 1 << 20
    lws, xs = [], []; ok2 = v2_ok(doc, ids, vals)
    for x in itertools.product(range(k), repeat=n):
        if not all(allowed[i][x[i]] for i in range(n)): continue
        if any(not lo <= sum(1 for i, q in mem if x[i] == q) <= lim for mem, lim, lo in caps): continue
        if not ok2(x): continue
        lws.append(sum(h[i][x[i]] for i in range(n)) + sum(t[x[i]][x[j]] for i, j, t in pairs)); xs.append(x)
    z = lse(lws); m = [[0.0] * k for _ in range(n)]
    for lw, x in zip(lws, xs):
        w = math.exp(lw - z)
        for i in range(n): m[i][x[i]] += w
    return {ids[i]: {vals[q]: m[i][q] for q in range(k)} for i in range(n)}

def chain_oracle(doc):
    ids, vals, n, k, h, allowed, pairs, caps = ir_parts(doc)
    assert not caps and all(p[0] == q and p[1] == q + 1 for q, p in enumerate(pairs)) and len(pairs) == n - 1
    phi = [[h[i][a] if allowed[i][a] else -math.inf for a in range(k)] for i in range(n)]
    fw = [phi[0][:]]
    for i in range(1, n): fw.append([phi[i][b] + lse(fw[i - 1][a] + pairs[i - 1][2][a][b] for a in range(k)) for b in range(k)])
    bw = [[0.0] * k for _ in range(n)]
    for i in range(n - 2, -1, -1): bw[i] = [lse(pairs[i][2][a][b] + phi[i + 1][b] + bw[i + 1][b] for b in range(k)) for a in range(k)]
    out = {}
    for i in range(n):
        l = [fw[i][a] + bw[i][a] for a in range(k)]; z = lse(l); out[ids[i]] = {vals[a]: math.exp(l[a] - z) for a in range(k)}
    return out


# ---------------- holdout families (R19 P1.4): seeded random programs no threshold was tuned on, brute-force oracle ----------------
import random
def holdout_programs(seed, per_family):
    """(family, doc) pairs: rare modes in disconnected ferro clusters, heterogeneous groups under a binding cap, constrained
    3-value cycles, a 7-value alphabet, varying group sizes; (R19.5) three clusters with mixed-sign bridges, rare modes behind a
    cap, 3-value clusters, a 150-variable 4-value chain (transfer matrix), an 8-value alphabet with caps. Every program but the
    chain has k^n <= 2^20 (brute force)."""
    R = random.Random(seed); out = []
    def doc(vals, n, h, pairs, caps, comment):
        vs = [{"id": f"x{i}", "h": {vals[q]: round(h[i][q], 4) for q in range(len(vals))}} for i in range(n)]
        return {"probbit_ir": 1, "comment": comment, "values": vals, "vars": vs, "pairs": pairs, "caps": caps}
    for t in range(per_family):
        # 1. two 6-spin ferro clusters (Potts 1.5-3), a small field makes one cluster's mode rare; one weak bridge
        n = 12; h = [[0.0, R.uniform(0.05, 0.4) * (1 if i < 6 else -1)] for i in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{j}", "potts": round(R.uniform(1.5, 3.0), 3)} for c in (0, 6) for i in range(c, c + 6) for j in range(i + 1, c + 6)]
        pairs.append({"i": "x0", "j": "x6", "potts": round(R.uniform(0.0, 0.3), 3)})
        out.append(("rare-modes", doc(["a", "b"], n, h, pairs, [], "holdout rare modes")))
        # 2. heterogeneous groups (3, 4, 5) with uniform affinity per group, a binding cap on value b over everyone
        sizes = [3, 4, 5]; n = sum(sizes); h = [[0.0, R.uniform(-1, 1)] for _ in range(n)]; pairs = []; s0 = 0
        for g in sizes:
            w = round(R.uniform(0.3, 1.5), 3); pairs += [{"i": f"x{i}", "j": f"x{j}", "potts": w} for i in range(s0, s0 + g) for j in range(i + 1, s0 + g)]; s0 += g
        out.append(("hetero-groups", doc(["a", "b"], n, h, pairs, [{"value": "b", "limit": R.randint(3, 7)}], "holdout heterogeneous groups")))
        # 3. a 3-value ring of 11 with random tables and a cap on value c
        n = 11; h = [[R.uniform(-0.5, 0.5) for _ in range(3)] for _ in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{(i + 1) % n}", "table": [[round(R.uniform(-1, 1), 3) for _ in range(3)] for _ in range(3)]} for i in range(n)]
        out.append(("constrained-cycle", doc(["a", "b", "c"], n, h, pairs, [{"value": "c", "limit": R.randint(2, 4)}], "holdout constrained cycle")))
        # 4. a 7-value alphabet: 7 variables, colouring-like negative Potts on a random graph, random unaries
        n = 7; h = [[R.uniform(-1, 1) for _ in range(7)] for _ in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{j}", "potts": round(R.uniform(-2, -0.5), 3)} for i in range(n) for j in range(i + 1, n) if R.random() < 0.5]
        out.append(("alphabet-7", doc([f"v{q}" for q in range(7)], n, h, pairs, [], "holdout large alphabet")))
        # 5. varying group sizes 2..6 (two-value), affinity per group, a global cap on b
        sizes = [2, 3, 4, 5, 6]; n = sum(sizes); h = [[0.0, R.uniform(-0.6, 0.6)] for _ in range(n)]; pairs = []; s0 = 0
        for g in sizes:
            w = round(R.uniform(0.5, 2.5), 3); pairs += [{"i": f"x{i}", "j": f"x{j}", "potts": w} for i in range(s0, s0 + g) for j in range(i + 1, s0 + g)]; s0 += g
        out.append(("group-sizes", doc(["a", "b"], n, h, pairs, [{"value": "b", "limit": R.randint(6, 14)}], "holdout varying group sizes")))
    # R19.5 harder families, from their OWN stream (the five above reproduce their R19.4 programs for every seed)
    R2 = random.Random(seed * 1000003 + 5)
    for t in range(per_family):
        # 6. three 4-spin ferro clusters, each cluster pair joined by one +w and one -w bridge (distinct endpoints), small fields
        n = 12; sgn = [R2.choice((-1, 1)) for _ in range(3)]; h = [[0.0, R2.uniform(0.05, 0.3) * sgn[i // 4]] for i in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{j}", "potts": round(R2.uniform(1.5, 3.0), 3)} for c in (0, 4, 8) for i in range(c, c + 4) for j in range(i + 1, c + 4)]
        for a, b in ((0, 4), (4, 8), (0, 8)):
            e = R2.sample([(u, v) for u in range(4) for v in range(4)], 2)
            pairs.append({"i": f"x{a + e[0][0]}", "j": f"x{b + e[0][1]}", "potts": round(R2.uniform(0.1, 0.6), 3)})
            pairs.append({"i": f"x{a + e[1][0]}", "j": f"x{b + e[1][1]}", "potts": -round(R2.uniform(0.1, 0.6), 3)})
        out.append(("three-clusters-mixed", doc(["a", "b"], n, h, pairs, [], "holdout three clusters, mixed-sign bridges")))
        # 7. rare modes behind a cap: two 5-variable 3-value ferro Potts clusters both lean to c; the cap lets only ONE be c
        n = 10; h = [[R2.uniform(-0.1, 0.1), R2.uniform(-0.1, 0.1), R2.uniform(0.1, 0.35)] for _ in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{j}", "potts": round(R2.uniform(1.2, 2.5), 3)} for c in (0, 5) for i in range(c, c + 5) for j in range(i + 1, c + 5)]
        out.append(("rare-behind-cap", doc(["a", "b", "c"], n, h, pairs, [{"value": "c", "limit": 5}], "holdout rare modes behind a cap")))
        # 8. k = 3 cluster structure: 3-value ferro clusters of 3, 3, 4, weak mixed-sign bridges, small random unaries
        n = 10; cl = [(0, 3), (3, 6), (6, 10)]; h = [[R2.uniform(-0.3, 0.3) for _ in range(3)] for _ in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{j}", "potts": round(R2.uniform(1.5, 3.0), 3)} for a, b in cl for i in range(a, b) for j in range(i + 1, b)]
        for (a0, a1), (b0, b1) in ((cl[0], cl[1]), (cl[1], cl[2])):
            pairs.append({"i": f"x{R2.randrange(a0, a1)}", "j": f"x{R2.randrange(b0, b1)}", "potts": round(R2.uniform(-0.5, 0.5), 3)})
        out.append(("k3-clusters", doc(["a", "b", "c"], n, h, pairs, [], "holdout 3-value clusters")))
        # 9. larger n with an exact oracle: a 4-value path of 150, random tables (|entry| <= 1.5) -> transfer matrix
        n = 150; h = [[R2.uniform(-0.5, 0.5) for _ in range(4)] for _ in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{i + 1}", "table": [[round(R2.uniform(-1.5, 1.5), 3) for _ in range(4)] for _ in range(4)]} for i in range(n - 1)]
        out.append(("chain150-k4", doc([f"v{q}" for q in range(4)], n, h, pairs, [], "holdout long 4-value chain")))
        # 10. an 8-value alphabet with caps: 6 variables, negative Potts on a random graph, two values capped at 1
        n = 6; h = [[R2.uniform(-1, 1) for _ in range(8)] for _ in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{j}", "potts": round(R2.uniform(-1.5, -0.3), 3)} for i in range(n) for j in range(i + 1, n) if R2.random() < 0.5]
        out.append(("alphabet-8-caps", doc([f"v{q}" for q in range(8)], n, h, pairs, [{"value": "v0", "limit": 1}, {"value": "v1", "limit": 1}], "holdout 8-value alphabet with caps")))
    # R19.5 (--cycles decision): saturated quotas, own stream. 12 variables, 3 values, each variable allows 2 of them, every
    # value capped at 4 (caps sum to n: every feasible plan uses each value exactly 4 times, so no single-site move is ever
    # feasible), random unaries + weak tables; regenerated until feasible
    R3 = random.Random(seed * 1000003 + 11)
    for t in range(per_family):
        n, vals = 12, ["a", "b", "c"]
        while True:
            al = [sorted(R3.sample(range(3), 2)) for _ in range(n)]
            if any(sorted(x.count(q) for q in range(3)) == [4, 4, 4] for x in itertools.product(*al)): break
        h = [[R3.uniform(-0.5, 0.5) for _ in range(3)] for _ in range(n)]
        vs = [{"id": f"x{i}", "h": {vals[q]: round(h[i][q], 4) for q in al[i]}, "allowed": [vals[q] for q in al[i]]} for i in range(n)]
        pairs = [{"i": f"x{i}", "j": f"x{j}", "table": [[round(R3.uniform(-0.4, 0.4), 3) for _ in range(3)] for _ in range(3)]}
                 for i in range(n) for j in range(i + 1, n) if R3.random() < 0.15]
        out.append(("saturated-k3", {"probbit_ir": 1, "comment": "holdout saturated quotas", "values": vals, "vars": vs, "pairs": pairs,
                                     "caps": [{"value": v, "limit": 4} for v in vals]}))
    # R19.5 IR v2 constructs (own stream): 7 variables x 4 values, random unaries + a few Potts pairs, plus an all_different over
    # 3 variables, an implication, a forbid table on 3 variables, a precedence (gap 0..2) and an at-least cap; regenerated
    # until feasible (the oracle checks every construct by its direct meaning, not by probbit's cap lowering)
    R4 = random.Random(seed * 1000003 + 17)
    for t in range(per_family):
        vals = ["s0", "s1", "s2", "s3"]; n = 7
        while True:
            h = [[R4.uniform(-0.6, 0.6) for _ in range(4)] for _ in range(n)]; ids = [f"x{i}" for i in range(n)]
            pairs = [{"i": f"x{i}", "j": f"x{j}", "potts": round(R4.uniform(-0.8, 0.8), 3)} for i in range(n) for j in range(i + 1, n) if R4.random() < 0.2]
            ad = R4.sample(ids, 3); a, b = R4.sample(ids, 2); tv = R4.sample(ids, 3); pa, pb = R4.sample(ids, 2)
            d = {"probbit_ir": 1, "comment": "holdout IR v2 constructs", "values": vals,
                 "vars": [{"id": ids[i], "h": {vals[q]: round(h[i][q], 4) for q in range(4)}} for i in range(n)], "pairs": pairs,
                 "caps": [{"value": R4.choice(vals), "min": R4.randint(1, 2)}],
                 "all_different": [{"vars": ad}],
                 "implies": [{"if": {"var": a, "value": R4.choice(vals)}, "then": {"var": b, "in": R4.sample(vals, 2)}}],
                 "tables": [{"vars": tv, "forbid": [[R4.choice(vals) for _ in range(3)] for _ in range(6)]}],
                 "precedes": [{"before": pa, "after": pb, "gap": R4.randint(0, 2)}]}
            try: brute_oracle(d); break
            except (ValueError, ZeroDivisionError): continue
        out.append(("v2-constructs", d))
    return out

# family -> (file, front-end, oracle). Frozen corpus: never edit the inputs; never tune a gate threshold on these results.
FAMILIES = [
    ("asym-h0.03", "asymmetric-router32-w0.2-h0.03.json", "decide", router_oracle),
    ("asym-h0.06", "asymmetric-router32-w0.2-h0.06.json", "decide", router_oracle),
    ("heterogeneous", "heterogeneous-router32.json", "decide", router_oracle),
    ("ferro12", "ferro12.json", "run", brute_oracle),
    ("ferro12-rc-w0.5", "ferro12-w0.5-redundantcaps.json", "run", brute_oracle),
    ("ferro12-rc-w1.0", "ferro12-w1.0-redundantcaps.json", "run", brute_oracle),
    ("chain100", "chain100.json", "run", chain_oracle),
]

def thresholds():
    src = open(os.path.join(ROOT, "probbit-ir", "src", "lib.rs")).read()
    keys = ["GATE_VERSION", "GATE_BS_POW", "PARTIAL_RHAT", "BATCH_RATIO_MAX", "ITEM_RHAT_MAX", "GATE"]
    return {k: re.search(rf"pub const {k}: [^=]+= ([^;]+);", src).group(1).strip() for k in keys}

def one(args, inp, truth):
    p = subprocess.run([PROBBIT] + args, input=inp, capture_output=True, text=True)
    try: d = json.loads(p.stdout)
    except ValueError: return {"exit": p.returncode, "verdict": "unparsable", "ms": None}
    v = d.get("verdict"); est = d.get("odds") or d.get("marginals") or {}
    rel = d.get("released", []) if v in ("exact", "diagnostics_passed", "partial") else []
    tv = lambda i: 0.5 * sum(abs(est.get(i, {}).get(val, 0.0) - p_) for val, p_ in truth[i].items())
    wrong = sum(1 for i in rel if tv(i) > TOL)
    return {"exit": p.returncode, "verdict": v, "tier": d.get("tier"), "released": len(rel), "wrong": wrong,
            "false_whole": v in ("exact", "diagnostics_passed") and wrong > 0, "ms": d.get("ms"),
            "gate_version": (d.get("gate") or {}).get("version")}

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--seeds", type=int, default=20); ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--out", default=None); ap.add_argument("--holdout", action="store_true", help="seeded holdout families instead of the frozen corpus")
    ap.add_argument("--holdout-seed", type=int, default=20261001); ap.add_argument("--per-family", type=int, default=5)
    ap.add_argument("--extra", default="", help="flags appended to the sampler-mode command (diagnostics, e.g. '--cluster on')")
    ap.add_argument("--families", default="", help="comma list: run only these families (default all)"); a = ap.parse_args()
    commit = subprocess.run(["git", "-C", ROOT, "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    head = {"commit": commit, "thresholds": thresholds(), "machine": f"{platform.machine()} {platform.system()} {platform.release()}",
            "load_before": os.getloadavg(), "extra": a.extra, "seeds": [1, a.seeds], "jobs": a.jobs, "tolerance_tv": TOL}
    rows, recs = [], []
    if a.holdout:
        head["holdout_seed"] = a.holdout_seed; head["per_family"] = a.per_family
        progs = holdout_programs(a.holdout_seed, a.per_family); fams = {}
        for fam, d in progs: fams.setdefault(fam, []).append(json.dumps(d))
        jobs = [(fam, ins) for fam, ins in fams.items()]
    else:
        jobs = [(name, [open(os.path.join(STRESS, f)).read()], fe, oracle, f) for name, f, fe, oracle in FAMILIES]
    if a.holdout: jobs = [(fam, ins, "run", chain_oracle if fam.startswith("chain") else brute_oracle, f"holdout:{fam}") for fam, ins in jobs]
    if a.families: jobs = [j for j in jobs if j[0] in a.families.split(",")]
    for name, inps, fe, oracle, f in jobs:
      for pi, inp in enumerate(inps):
        truth = oracle(json.loads(inp)); label = name if len(inps) == 1 else f"{name}#{pi}"
        for mode, extra in (("defaults", []), ("sampler", ["--mode", "sample"] if fe == "decide" else ["--op", "sample"])):
            base = [fe] + extra + (a.extra.split() if a.extra and mode == "sampler" else [])
            with ThreadPoolExecutor(a.jobs) as ex:
                res = list(ex.map(lambda s: one(base + ["--seed", str(s)], inp, truth), range(1, a.seeds + 1)))
            src = f"<holdout program {label} from --holdout-seed {a.holdout_seed} (in the --out file)>" if a.holdout else f"probbit-cli/tests/stress/{f}"
            for s, r in zip(range(1, a.seeds + 1), res): recs.append({"family": label, "mode": mode, "seed": s, "command": f"probbit {' '.join(base)} --seed {s} < {src}", **r})
            whole = sum(1 for r in res if r["verdict"] in ("exact", "diagnostics_passed"))
            ms = [r["ms"] for r in res if r["ms"] is not None]
            rows.append((label, mode, " ".join(base), whole, sum(r["false_whole"] for r in res), sum(r["released"] for r in res), sum(r["wrong"] for r in res),
                         sum(1 for r in res if r["verdict"] == "partial"), sum(1 for r in res if r["verdict"] == "refused"), len(res),
                         statistics.median(ms) if ms else float("nan"), sorted({r["tier"] or "-" for r in res}), sorted({r["gate_version"] or "-" for r in res})))
    head["load_after"] = os.getloadavg()
    print(json.dumps(head))
    print("| family | mode | command | whole | FALSE whole | released | WRONG | partial | refused (rate) | median ms | tiers | gate |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for (nm, md, cmd, wh, fw, rl, wr, pa, rf, nr, me, ti, gv) in rows:
        print(f"| {nm} | {md} | `probbit {cmd}` | {wh} | {fw} | {rl} | {wr} | {pa} | {rf} ({rf / nr:.0%}) | {me:.3f} | {','.join(ti)} | {','.join(gv)} |")
    if a.out:
        with open(a.out, "w") as fh:
            fh.write(json.dumps(head) + "\n")
            for r in recs: fh.write(json.dumps(r) + "\n")
            if a.holdout:
                for fam, ins, *_ in jobs:
                    for pi, inp in enumerate(ins): fh.write(json.dumps({"program": f"{fam}#{pi}", "doc": json.loads(inp)}) + "\n")
    bad = sum(r[4] for r in rows) + sum(r[6] for r in rows)
    sys.exit(1 if bad else 0)

if __name__ == "__main__":
    main()
