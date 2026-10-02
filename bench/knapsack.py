#!/usr/bin/env python3
"""R19.7 (P2.2 family 1): knapsack through IR `linear` rules, against an independent exact DP and a greedy baseline.
Stdlib only. Per instance (n items, weights 1..20, unaries h_in ~ U(0, 1.5), budget = 40% of the total weight):
  oracle   = log-space forward/backward DP over the used capacity: log Z, P(item in) for every item, and the optimum (max-product)
  baseline = greedy by h_in / weight (classical heuristic for the best plan)
  probbit     = `probbit run` at defaults (tier, plan log w) and `probbit run --op sample --seed S` (verdict, released items' max |odds - DP|)
Usage: python3 bench/knapsack.py [--sizes 20,40,80] [--seeds 4] [--instances 3] [--write-example examples/knapsack-20.json]"""
import argparse, json, math, os, random, subprocess, time
PROBBIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'probbit')
NEG = float('-inf')
def lse(a, b): m = max(a, b); return m if m == NEG else m + math.log(math.exp(a - m) + math.exp(b - m))
def instance(n, seed):
    r = random.Random(seed); w = [r.randint(1, 20) for _ in range(n)]; h = [round(r.uniform(0, 1.5), 4) for _ in range(n)]
    return w, h, sum(w) * 2 // 5
def program(w, h, L):
    return {"probbit_ir": 1, "comment": "knapsack: item i in (+h_i) or out; total weight of the items in <= limit",
            "values": ["out", "in"], "vars": [{"id": f"i{q}", "h": {"in": h[q]}} for q in range(len(w))],
            "linear": [{"terms": [[f"i{q}", "in", w[q]] for q in range(len(w))], "limit": L}]}
def dp(w, h, L):
    n = len(w); F = [[NEG] * (L + 1) for _ in range(n + 1)]; F[0][0] = 0.0
    for i in range(n):
        for c in range(L + 1):
            if F[i][c] == NEG: continue
            F[i + 1][c] = lse(F[i + 1][c], F[i][c])
            if c + w[i] <= L: F[i + 1][c + w[i]] = lse(F[i + 1][c + w[i]], F[i][c] + h[i])
    B = [[NEG] * (L + 1) for _ in range(n + 1)]; B[n] = [0.0] * (L + 1)
    for i in range(n - 1, -1, -1):
        for c in range(L + 1): B[i][c] = lse(B[i + 1][c], B[i + 1][c + w[i]] + h[i] if c + w[i] <= L else NEG)
    logz = B[0][0]
    p_in = []
    for i in range(n):
        acc = NEG
        for c in range(L + 1 - w[i]):
            if F[i][c] != NEG: acc = lse(acc, F[i][c] + h[i] + B[i + 1][c + w[i]])
        p_in.append(math.exp(acc - logz))
    # optimum: max-product over (item, used capacity)
    M = [[NEG] * (L + 1) for _ in range(n + 1)]; M[n] = [0.0] * (L + 1)
    for i in range(n - 1, -1, -1):
        for c in range(L + 1): M[i][c] = max(M[i + 1][c], M[i + 1][c + w[i]] + h[i] if c + w[i] <= L else NEG)
    return logz, p_in, M[0][0]
def greedy(w, h, L):
    load = val = 0.0
    for q in sorted(range(len(w)), key=lambda q: -h[q] / w[q]):
        if h[q] > 0 and load + w[q] <= L: load += w[q]; val += h[q]
    return val
def run(prog, *args):
    t = time.time(); p = subprocess.run([PROBBIT, 'run', *args], input=json.dumps(prog), capture_output=True, text=True)
    return p.returncode, json.loads(p.stdout), (time.time() - t) * 1e3
def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--sizes', default='20,40,80'); ap.add_argument('--seeds', type=int, default=4)
    ap.add_argument('--instances', type=int, default=3); ap.add_argument('--write-example', default=None); a = ap.parse_args()
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, cwd=os.path.dirname(PROBBIT)).stdout.strip()
    print(json.dumps({"commit": commit, "load_before": os.getloadavg(), "sizes": a.sizes, "seeds": a.seeds, "instances": a.instances}))
    if a.write_example:
        w, h, L = instance(20, 1); json.dump(program(w, h, L), open(a.write_example, 'w'), indent=1); print("wrote", a.write_example)
    print("| n | inst | DP log Z | DP optimum | greedy (gap) | default: tier, plan log w (gap), ms | sampler: verdicts | released | max \\|odds - DP\\| released | median ms |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    tot = {"released": 0, "wrong": 0, "false": 0, "answers": 0}
    for n in map(int, a.sizes.split(',')):
        for inst in range(1, a.instances + 1):
            w, h, L = instance(n, 1000 * n + inst); prog = program(w, h, L); logz, p_in, opt = dp(w, h, L); g = greedy(w, h, L)
            c, d, ms = run(prog); tier = d.get('tier', d.get('verdict')); plw = d.get('plan_logw', float('nan'))
            verdicts, rel, worst, times = [], 0, 0.0, []
            for s in range(1, a.seeds + 1):
                c2, e, ms2 = run(prog, '--op', 'sample', '--seed', str(s)); verdicts.append(e['verdict']); times.append(ms2)
                released = e.get('released', []); rel += len(released); tot["answers"] += 1
                err = max([abs(e['marginals'][v].get('in', 0.0) - p_in[int(v[1:])]) for v in released] or [0.0])
                worst = max(worst, err); tot["released"] += len(released); tot["wrong"] += sum(abs(e['marginals'][v].get('in', 0.0) - p_in[int(v[1:])]) > 0.05 for v in released)
                if e['verdict'] == 'diagnostics_passed' and err > 0.05: tot["false"] += 1
            vs = ', '.join(f"{v} {verdicts.count(v)}" for v in sorted(set(verdicts)))
            print(f"| {n} | {inst} | {logz:.6f} | {opt:.4f} | {g:.4f} ({opt - g:+.4f}) | {tier}, {plw:.4f} ({opt - plw:+.4f}), {ms:.1f} | {vs} | {rel}/{n * a.seeds} | {worst:.4f} | {sorted(times)[len(times) // 2]:.0f} |")
    print(json.dumps({"totals": tot, "load_after": os.getloadavg()}))
if __name__ == '__main__': main()
