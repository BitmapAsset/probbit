#!/usr/bin/env python3
"""R19.7 (P2.2 family 5): 3-SAT solution counting and sampling. Random planted 3-SAT (n variables, m = ratio x n clauses of 3
distinct variables, each clause satisfied by a hidden assignment so the formula is satisfiable). Program: one {F, T} variable per
SAT variable, no scores (uniform over the solutions), one IR `tables` forbid per clause (the single falsifying tuple).
Oracle: brute force over 2^n (bitmask clause checks): solution count, P(x_i = T). Baseline: WalkSAT (p = 0.5) finds ONE solution.
probbit: `probbit run` at defaults and `probbit run --op sample --seed S`. MaxSAT (soft clauses) is NOT here: v1 has no soft ternary
terms. Stdlib only. Usage: python3 bench/sat.py [--sizes 14,18] [--ratio 4.0] [--instances 2] [--seeds 4]"""
import argparse, json, os, random, subprocess, time
PROBBIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'probbit')
def instance(n, m, seed):
    r = random.Random(seed); hidden = [r.random() < 0.5 for _ in range(n)]; cl = []
    while len(cl) < m:
        vs = r.sample(range(n), 3); sg = [r.random() < 0.5 for _ in vs]  # literal i is x_v == sg
        if any(hidden[v] == s for v, s in zip(vs, sg)): cl.append(list(zip(vs, sg)))
    return cl
def program(n, cl):
    tv = lambda b: "T" if b else "F"
    return {"probbit_ir": 1, "comment": "planted 3-SAT, uniform over solutions: each clause forbids its one falsifying tuple", "values": ["F", "T"],
            "vars": [{"id": f"x{i}"} for i in range(n)], "tables": [{"vars": [f"x{v}" for v, _ in c], "forbid": [[tv(not s) for _, s in c]]} for c in cl]}
def brute(n, cl):
    masks = []
    for c in cl:
        mask = pat = 0
        for v, s in c: mask |= 1 << v; pat |= (0 if s else 1) << v  # falsified iff every literal false: x_v == not s
        masks.append((mask, pat))
    cnt = 0; on = [0] * n
    for x in range(1 << n):
        ok = True
        for mask, pat in masks:
            if x & mask == pat: ok = False; break
        if ok:
            cnt += 1
            for i in range(n):
                if x >> i & 1: on[i] += 1
    return cnt, [o / cnt for o in on]
def walksat(n, cl, seed, max_flips=100000):
    r = random.Random(seed); x = [r.random() < 0.5 for _ in range(n)]
    sat = lambda c: any(x[v] == s for v, s in c)
    for f in range(max_flips):
        un = [c for c in cl if not sat(c)]
        if not un: return f
        c = r.choice(un)
        if r.random() < 0.5: v = r.choice(c)[0]
        else:
            def breaks(v): x[v] = not x[v]; b = sum(not sat(d) for d in cl); x[v] = not x[v]; return b
            v = min((v for v, _ in c), key=breaks)
        x[v] = not x[v]
    return None
def run(prog, *args):
    t0 = time.time(); p = subprocess.run([PROBBIT, 'run', *args], input=json.dumps(prog), capture_output=True, text=True)
    return p.returncode, json.loads(p.stdout), (time.time() - t0) * 1e3
def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--sizes', default='14,18'); ap.add_argument('--ratio', type=float, default=4.0)
    ap.add_argument('--instances', type=int, default=2); ap.add_argument('--seeds', type=int, default=4); a = ap.parse_args()
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, cwd=os.path.dirname(PROBBIT)).stdout.strip()
    print(json.dumps({"commit": commit, "load_before": os.getloadavg(), "sizes": a.sizes, "ratio": a.ratio, "instances": a.instances, "seeds": a.seeds}))
    print("| n | m | inst | solutions | WalkSAT flips | default: verdict, tier, n_feasible, max \\|odds - brute\\|, ms | sampler verdicts | released | max \\|odds - brute\\| released | median ms |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    tot = {"answers": 0, "released": 0, "wrong": 0, "false": 0}
    for n in map(int, a.sizes.split(',')):
        m = int(round(a.ratio * n))
        for inst in range(1, a.instances + 1):
            cl = instance(n, m, 31 * n + inst); prog = program(n, cl); cnt, pt = brute(n, cl); ws = walksat(n, cl, inst)
            od = lambda e, v: abs(e['marginals'][v].get('T', 0.0) - pt[int(v[1:])])
            c, d, ms = run(prog); dm = max(od(d, f"x{i}") for i in range(n)) if 'marginals' in d else float('nan')
            vs, rel, worst, times = [], 0, 0.0, []
            for s in range(1, a.seeds + 1):
                c2, e, ms2 = run(prog, '--op', 'sample', '--seed', str(s)); vs.append(e['verdict']); times.append(ms2); tot["answers"] += 1
                released = e.get('released', []); rel += len(released); tot["released"] += len(released)
                err = max([od(e, v) for v in released] or [0.0]); worst = max(worst, err); tot["wrong"] += sum(od(e, v) > 0.05 for v in released)
                if e['verdict'] == 'diagnostics_passed' and err > 0.05: tot["false"] += 1
            vstr = ', '.join(f"{v} {vs.count(v)}" for v in sorted(set(vs)))
            print(f"| {n} | {m} | {inst} | {cnt} | {ws} | {d.get('verdict')}, {d.get('tier')}, {d.get('n_feasible')}, {dm:.1e}, {ms:.1f} | {vstr} | {rel}/{n * a.seeds} | {worst:.4f} | {sorted(times)[len(times) // 2]:.0f} |")
    print(json.dumps({"totals": tot, "load_after": os.getloadavg()}))
if __name__ == '__main__': main()
