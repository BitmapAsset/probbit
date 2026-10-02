#!/usr/bin/env python3
"""R19.7 (P2.2 family 1b): bin packing through IR `linear` rules (one weighted rule per bin), against an independent exact DP
over the vector of bin loads (beyond enumeration's reach), and a largest-first greedy baseline (onto the preferred bin that still fits). Stdlib only.
Instance: n items (sizes 1..6), b = 3 bins of capacity ceil(total / 3) + 2, preferences h(item, bin) ~ U(-0.5, 0.5).
Oracle: forward/backward over load vectors: log Z, P(item in bin) for every (item, bin), optimum (max-product).
Usage: python3 bench/binpacking.py [--sizes 12,24] [--instances 2] [--seeds 4]"""
import argparse, json, math, os, random, subprocess, time
PROBBIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'probbit')
B = 3
def lse2(a, b): m = max(a, b); return m + math.log(math.exp(a - m) + math.exp(b - m))
def instance(n, seed):
    r = random.Random(seed); sz = [r.randint(1, 6) for _ in range(n)]; cap = -(-sum(sz) // B) + 2
    h = [[round(r.uniform(-0.5, 0.5), 4) for _ in range(B)] for _ in range(n)]; return sz, h, cap
def program(sz, h, cap):
    n = len(sz); return {"probbit_ir": 1, "comment": "bin packing: item -> bin, per bin the sizes of its items sum to <= capacity",
        "values": [f"b{v}" for v in range(B)], "vars": [{"id": f"i{q}", "h": {f"b{v}": h[q][v] for v in range(B)}} for q in range(n)],
        "linear": [{"terms": [[f"i{q}", f"b{v}", sz[q]] for q in range(n)], "limit": cap} for v in range(B)]}
def dp(sz, h, cap):
    n = len(sz); F = [{(0,) * B: 0.0}]
    for i in range(n):
        nx = {}
        for st, lw in F[-1].items():
            for v in range(B):
                if st[v] + sz[i] <= cap: t = list(st); t[v] += sz[i]; t = tuple(t); nx[t] = lse2(nx[t], lw + h[i][v]) if t in nx else lw + h[i][v]
        F.append(nx)
    Bk = [None] * (n + 1); Bk[n] = {st: 0.0 for st in F[n]}; M = [None] * (n + 1); M[n] = {st: 0.0 for st in F[n]}
    for i in range(n - 1, -1, -1):
        Bk[i] = {}; M[i] = {}
        for st in F[i]:
            acc, best = None, None
            for v in range(B):
                if st[v] + sz[i] <= cap:
                    t = list(st); t[v] += sz[i]; t = tuple(t)
                    if t in Bk[i + 1]: x = Bk[i + 1][t] + h[i][v]; acc = x if acc is None else lse2(acc, x); y = M[i + 1][t] + h[i][v]; best = y if best is None else max(best, y)
            if acc is not None: Bk[i][st] = acc; M[i][st] = best
    logz = Bk[0][(0,) * B]; p = [[0.0] * B for _ in range(n)]
    for i in range(n):
        for st, lw in F[i].items():
            if st not in Bk[i]: continue
            for v in range(B):
                if st[v] + sz[i] <= cap:
                    t = list(st); t[v] += sz[i]; t = tuple(t)
                    if t in Bk[i + 1]: p[i][v] += math.exp(lw + h[i][v] + Bk[i + 1][t] - logz)
    return logz, p, M[0][(0,) * B]
def ffd(sz, h, cap):
    load = [0] * B; val = 0.0
    for q in sorted(range(len(sz)), key=lambda q: -sz[q]):
        opts = [v for v in range(B) if load[v] + sz[q] <= cap]
        if not opts: return None
        v = max(opts, key=lambda v: h[q][v]); load[v] += sz[q]; val += h[q][v]
    return val
def run(prog, *args):
    t = time.time(); p = subprocess.run([PROBBIT, 'run', *args], input=json.dumps(prog), capture_output=True, text=True)
    return p.returncode, json.loads(p.stdout), (time.time() - t) * 1e3
def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--sizes', default='12,24'); ap.add_argument('--instances', type=int, default=2); ap.add_argument('--seeds', type=int, default=4); a = ap.parse_args()
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, cwd=os.path.dirname(PROBBIT)).stdout.strip()
    print(json.dumps({"commit": commit, "load_before": os.getloadavg(), "sizes": a.sizes, "instances": a.instances, "seeds": a.seeds}))
    print("| n | inst | DP log Z | DP optimum | FFD (gap) | default: tier, plan log w (gap), ms | sampler verdicts | released | max \\|odds - DP\\| | median ms |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    tot = {"answers": 0, "released": 0, "wrong": 0, "false": 0}
    for n in map(int, a.sizes.split(',')):
        for inst in range(1, a.instances + 1):
            sz, h, cap = instance(n, 100 * n + inst); prog = program(sz, h, cap); logz, p, opt = dp(sz, h, cap); f = ffd(sz, h, cap)
            c, d, ms = run(prog); plw = d.get('plan_logw', float('nan'))
            vs, rel, worst, times = [], 0, 0.0, []
            for s in range(1, a.seeds + 1):
                c2, e, ms2 = run(prog, '--op', 'sample', '--seed', str(s)); vs.append(e['verdict']); times.append(ms2); tot["answers"] += 1
                released = e.get('released', []); rel += len(released); tot["released"] += len(released)
                od = lambda v: max(abs(e['marginals'][v].get(f"b{b}", 0.0) - p[int(v[1:])][b]) for b in range(B))
                err = max([od(v) for v in released] or [0.0]); worst = max(worst, err); tot["wrong"] += sum(od(v) > 0.05 for v in released)
                if e['verdict'] == 'diagnostics_passed' and err > 0.05: tot["false"] += 1
            vstr = ', '.join(f"{v} {vs.count(v)}" for v in sorted(set(vs)))
            fs = f"{f:.4f} ({opt - f:+.4f})" if f is not None else "no fit"
            print(f"| {n} | {inst} | {logz:.6f} | {opt:.4f} | {fs} | {d.get('tier', d.get('verdict'))}, {plw:.4f} ({opt - plw:+.4f}), {ms:.1f} | {vstr} | {rel}/{n * a.seeds} | {worst:.4f} | {sorted(times)[len(times) // 2]:.0f} |")
    print(json.dumps({"totals": tot, "load_after": os.getloadavg()}))
if __name__ == '__main__': main()
