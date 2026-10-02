#!/usr/bin/env python3
"""R19.7 (P2.2 family 3): tree-structured probabilistic inference. Random trees (parent of node i uniform in 0..i-1), k values,
unaries ~ U(-1, 1), one k x k table per edge ~ U(-1, 1). Independent oracle: log-space two-pass sum-product (marginals, log Z).
pbit: `pbit run` at defaults (the inference compiler's forest tier, exact) and `pbit run --op sample --seed S` (gated sampler).
Stdlib only. Usage: python3 bench/tree_infer.py [--sizes 200,1000] [--k 3] [--instances 2] [--seeds 4]"""
import argparse, json, math, os, random, subprocess, time
PBIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'pbit')
def lse(xs): m = max(xs); return m + math.log(sum(math.exp(x - m) for x in xs))
def instance(n, k, seed):
    r = random.Random(seed); par = [None] + [r.randrange(i) for i in range(1, n)]
    h = [[round(r.uniform(-1, 1), 4) for _ in range(k)] for _ in range(n)]; t = [None] + [[[round(r.uniform(-1, 1), 4) for _ in range(k)] for _ in range(k)] for _ in range(1, n)]
    return par, h, t
def program(par, h, t, k):
    n = len(par); vals = [f"v{a}" for a in range(k)]
    return {"pbit_ir": 1, "comment": "random tree, table couplings parent -> child", "values": vals,
            "vars": [{"id": f"x{i}", "h": {vals[a]: h[i][a] for a in range(k)}} for i in range(n)],
            "pairs": [{"i": f"x{par[i]}", "j": f"x{i}", "table": t[i]} for i in range(1, n)]}
def sum_product(par, h, t, k):
    n = len(par); ch = [[] for _ in range(n)]
    for i in range(1, n): ch[par[i]].append(i)
    up = [None] * n  # up[i][a] = log message from i to its parent, as a function of the parent's value a
    inner = [None] * n
    for i in range(n - 1, -1, -1):  # children have larger indices
        inner[i] = [h[i][b] + sum(up[c][b] for c in ch[i]) for b in range(k)]
        if i > 0: up[i] = [lse([t[i][a][b] + inner[i][b] for b in range(k)]) for a in range(k)]
    logz = lse(inner[0]); down = [None] * n; down[0] = [0.0] * k; marg = [None] * n
    for i in range(n):
        full = [inner[i][b] + down[i][b] for b in range(k)]; z = lse(full); marg[i] = [math.exp(x - z) for x in full]
        for c in ch[i]: down[c] = [lse([t[c][a][b] + full[a] - up[c][a] for a in range(k)]) for b in range(k)]
    return logz, marg
def run(prog, *args):
    t0 = time.time(); p = subprocess.run([PBIT, 'run', *args], input=json.dumps(prog), capture_output=True, text=True)
    return p.returncode, json.loads(p.stdout), (time.time() - t0) * 1e3
def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--sizes', default='200,1000'); ap.add_argument('--k', type=int, default=3)
    ap.add_argument('--instances', type=int, default=2); ap.add_argument('--seeds', type=int, default=4); a = ap.parse_args(); k = a.k
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, cwd=os.path.dirname(PBIT)).stdout.strip()
    print(json.dumps({"commit": commit, "load_before": os.getloadavg(), "sizes": a.sizes, "k": k, "instances": a.instances, "seeds": a.seeds}))
    print("| n | inst | oracle log Z | default: tier, \\|log Z - oracle\\|, max \\|odds - oracle\\|, ms | sampler verdicts | released | max \\|odds - oracle\\| released | median ms |")
    print("|---|---|---|---|---|---|---|---|")
    tot = {"answers": 0, "released": 0, "wrong": 0, "false": 0}
    for n in map(int, a.sizes.split(',')):
        for inst in range(1, a.instances + 1):
            par, h, t = instance(n, k, 10 * n + inst); prog = program(par, h, t, k); logz, marg = sum_product(par, h, t, k)
            c, d, ms = run(prog); od = lambda e, v: max(abs(e['marginals'][v].get(f"v{b}", 0.0) - marg[int(v[1:])][b]) for b in range(k))
            dz = abs(d.get('logz', float('nan')) - logz); dm = max(od(d, f"x{i}") for i in range(n))
            vs, rel, worst, times = [], 0, 0.0, []
            for s in range(1, a.seeds + 1):
                c2, e, ms2 = run(prog, '--op', 'sample', '--seed', str(s)); vs.append(e['verdict']); times.append(ms2); tot["answers"] += 1
                released = e.get('released', []); rel += len(released); tot["released"] += len(released)
                err = max([od(e, v) for v in released] or [0.0]); worst = max(worst, err); tot["wrong"] += sum(od(e, v) > 0.05 for v in released)
                if e['verdict'] == 'diagnostics_passed' and err > 0.05: tot["false"] += 1
            vstr = ', '.join(f"{v} {vs.count(v)}" for v in sorted(set(vs)))
            print(f"| {n} | {inst} | {logz:.6f} | {d.get('tier')}, {dz:.1e}, {dm:.1e}, {ms:.1f} | {vstr} | {rel}/{n * a.seeds} | {worst:.4f} | {sorted(times)[len(times) // 2]:.0f} |")
    print(json.dumps({"totals": tot, "load_after": os.getloadavg()}))
if __name__ == '__main__': main()
