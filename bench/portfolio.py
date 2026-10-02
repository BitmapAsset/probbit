#!/usr/bin/env python3
"""R19.7 (P2.2 family 4): subset selection under a budget with pair correlations (portfolio shape). n assets in a sector chain:
x_i in {out, in}, h_in ~ U(-0.2, 1.0) (return minus risk), cost 1..10, budget 35% of the total cost (one IR `linear` rule), and a
Potts term J_i ~ U(-0.6, 0.6) between neighbours i, i+1 (+J when both take the same choice). No exact tier of pbit applies
(a weighted rule plus a chain of pairs): defaults sample. Independent oracle: exact DP over (position, choice, used budget):
log Z, P(in) per asset, the optimum. Baseline: greedy by h_in / cost ignoring the pairs (scored with them). Stdlib only.
Usage: python3 bench/portfolio.py [--sizes 30,60] [--instances 2] [--seeds 4]"""
import argparse, json, math, os, random, subprocess, time
PBIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'pbit')
NEG = float('-inf')
def lse2(a, b): m = max(a, b); return m if m == NEG else m + math.log(math.exp(a - m) + math.exp(b - m))
def instance(n, seed):
    r = random.Random(seed); h = [round(r.uniform(-0.2, 1.0), 4) for _ in range(n)]; w = [r.randint(1, 10) for _ in range(n)]
    J = [round(r.uniform(-0.6, 0.6), 4) for _ in range(n - 1)]; return h, w, J, sum(w) * 7 // 20
def program(h, w, J, L):
    n = len(h); return {"pbit_ir": 1, "comment": "budgeted selection with neighbour correlations", "values": ["out", "in"],
        "vars": [{"id": f"a{i}", "h": {"in": h[i]}} for i in range(n)], "pairs": [{"i": f"a{i}", "j": f"a{i+1}", "potts": J[i]} for i in range(n - 1)],
        "linear": [{"terms": [[f"a{i}", "in", w[i]] for i in range(n)], "limit": L}]}
def logw(x, h, J): return sum(h[i] for i in range(len(x)) if x[i]) + sum(J[i] for i in range(len(x) - 1) if x[i] == x[i + 1])
def dp(h, w, J, L, mx=False):
    n = len(h); comb = max if mx else lse2
    F = [dict() for _ in range(n)]; F[0][(0, 0)] = 0.0
    if w[0] <= L: F[0][(1, w[0])] = h[0]
    for i in range(n - 1):
        for (a, c), v in F[i].items():
            for b in (0, 1):
                c2 = c + w[i + 1] * b
                if c2 > L: continue
                x = v + (h[i + 1] if b else 0.0) + (J[i] if a == b else 0.0); F[i + 1][(b, c2)] = comb(F[i + 1].get((b, c2), NEG), x)
    total = NEG
    for v in F[n - 1].values(): total = comb(total, v)
    if mx: return total
    Bk = [dict() for _ in range(n)]
    for k in F[n - 1]: Bk[n - 1][k] = 0.0
    for i in range(n - 2, -1, -1):
        for (a, c) in F[i]:
            acc = NEG
            for b in (0, 1):
                c2 = c + w[i + 1] * b
                if (b, c2) in Bk[i + 1]: acc = lse2(acc, Bk[i + 1][(b, c2)] + (h[i + 1] if b else 0.0) + (J[i] if a == b else 0.0))
            Bk[i][(a, c)] = acc
    p = [sum(math.exp(F[i][k] + Bk[i][k] - total) for k in F[i] if k[0] == 1 and Bk[i][k] > NEG) for i in range(n)]
    return total, p
def greedy(h, w, J, L):
    x = [0] * len(h); load = 0
    for i in sorted(range(len(h)), key=lambda i: -h[i] / w[i]):
        if h[i] > 0 and load + w[i] <= L: x[i] = 1; load += w[i]
    return logw(x, h, J)
def run(prog, *args):
    t0 = time.time(); p = subprocess.run([PBIT, 'run', *args], input=json.dumps(prog), capture_output=True, text=True)
    return p.returncode, json.loads(p.stdout), (time.time() - t0) * 1e3
def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--sizes', default='30,60'); ap.add_argument('--instances', type=int, default=2); ap.add_argument('--seeds', type=int, default=4); a = ap.parse_args()
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, cwd=os.path.dirname(PBIT)).stdout.strip()
    print(json.dumps({"commit": commit, "load_before": os.getloadavg(), "sizes": a.sizes, "instances": a.instances, "seeds": a.seeds}))
    print("| n | inst | DP log Z | DP optimum | greedy (gap) | default: tier, plan log w (gap), ms | sampler verdicts | released | max \\|odds - DP\\| | median ms |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    tot = {"answers": 0, "released": 0, "wrong": 0, "false": 0}
    for n in map(int, a.sizes.split(',')):
        for inst in range(1, a.instances + 1):
            h, w, J, L = instance(n, 7 * n + inst); prog = program(h, w, J, L); logz, p = dp(h, w, J, L); opt = dp(h, w, J, L, mx=True); g = greedy(h, w, J, L)
            c, d, ms = run(prog); plw = d.get('plan_logw', float('nan'))
            vs, rel, worst, times = [], 0, 0.0, []
            for s in range(1, a.seeds + 1):
                c2, e, ms2 = run(prog, '--op', 'sample', '--seed', str(s)); vs.append(e['verdict']); times.append(ms2); tot["answers"] += 1
                released = e.get('released', []); rel += len(released); tot["released"] += len(released)
                od = lambda v: abs(e['marginals'][v].get('in', 0.0) - p[int(v[1:])])
                err = max([od(v) for v in released] or [0.0]); worst = max(worst, err); tot["wrong"] += sum(od(v) > 0.05 for v in released)
                if e['verdict'] == 'diagnostics_passed' and err > 0.05: tot["false"] += 1
            vstr = ', '.join(f"{v} {vs.count(v)}" for v in sorted(set(vs)))
            print(f"| {n} | {inst} | {logz:.6f} | {opt:.4f} | {g:.4f} ({opt - g:+.4f}) | {d.get('tier', d.get('verdict'))}, {plw:.4f} ({opt - plw:+.4f}), {ms:.1f} | {vstr} | {rel}/{n * a.seeds} | {worst:.4f} | {sorted(times)[len(times) // 2]:.0f} |")
    print(json.dumps({"totals": tot, "load_after": os.getloadavg()}))
if __name__ == '__main__': main()
