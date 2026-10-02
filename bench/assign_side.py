#!/usr/bin/env python3
"""R19.7 (P2.2 family 7): assignment with side constraints. n tasks onto 3 workers {A, B, C}: scores ~ U(-1, 1); per-worker
effort budget (one `linear` rule per worker, efforts 1..4, limit ceil(total / 3) + 1); conflict pairs that may not share a
worker (`all_different` on the pair); one implication (t0 on A => t1 on B or C, `implies`); a value cap: at most
ceil(n / 3) + 1 tasks on C. Oracle: brute force over 3^n by the rules' direct meaning (scipy / HiGHS not installed here, so no
ILP baseline); baseline: greedy (tasks by best score, best feasible worker). Stdlib only.
Usage: python3 bench/assign_side.py [--sizes 8,12] [--instances 2] [--seeds 4]"""
import argparse, itertools, json, math, os, random, subprocess, time
PROBBIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'probbit')
W = ["A", "B", "C"]
def instance(n, seed):
    r = random.Random(seed); sc = [[round(r.uniform(-1, 1), 4) for _ in W] for _ in range(n)]; ef = [r.randint(1, 4) for _ in range(n)]
    conf = set()
    while len(conf) < n // 3: a, b = sorted(r.sample(range(n), 2)); conf.add((a, b))
    return sc, ef, sorted(conf), -(-sum(ef) // 3) + 1, -(-n // 3) + 1
def program(sc, ef, conf, lim, capc):
    n = len(sc); t = lambda i: f"t{i}"
    return {"probbit_ir": 1, "comment": "assignment with side constraints", "values": W,
        "vars": [{"id": t(i), "h": {w: sc[i][q] for q, w in enumerate(W)}} for i in range(n)],
        "linear": [{"terms": [[t(i), w, ef[i]] for i in range(n)], "limit": lim} for w in W],
        "all_different": [{"vars": [t(a), t(b)]} for a, b in conf],
        "implies": [{"if": {"var": t(0), "value": "A"}, "then": {"var": t(1), "in": ["B", "C"]}}],
        "caps": [{"value": "C", "limit": capc}]}
def ok(x, ef, conf, lim, capc):
    for q in range(3):
        if sum(ef[i] for i in range(len(x)) if x[i] == q) > lim: return False
    if any(x[a] == x[b] for a, b in conf): return False
    if x[0] == 0 and x[1] == 0: return False
    return sum(1 for v in x if v == 2) <= capc
def brute(sc, ef, conf, lim, capc):
    n = len(sc); z = 0.0; m = [[0.0] * 3 for _ in range(n)]; best = (-1e9, None); cnt = 0
    for x in itertools.product(range(3), repeat=n):
        if not ok(x, ef, conf, lim, capc): continue
        cnt += 1; v = sum(sc[i][x[i]] for i in range(n)); e = math.exp(v); z += e
        for i in range(n): m[i][x[i]] += e
        if v > best[0]: best = (v, x)
    return cnt, math.log(z), [[p / z for p in row] for row in m], best
def greedy(sc, ef, conf, lim, capc):
    n = len(sc); x = [None] * n
    for i in sorted(range(n), key=lambda i: -max(sc[i])):
        for q in sorted(range(3), key=lambda q: -sc[i][q]):
            x[i] = q; part = [v if v is not None else -1 for v in x]
            if sum(ef[j] for j in range(n) if part[j] == q) <= lim and not any(part[a] == part[b] != -1 for a, b in conf) and not (part[0] == 0 and part[1] == 0) and sum(1 for v in part if v == 2) <= capc: break
            x[i] = None
        if x[i] is None: return None
    return sum(sc[i][x[i]] for i in range(n))
def run(prog, *args):
    t0 = time.time(); p = subprocess.run([PROBBIT, 'run', *args], input=json.dumps(prog), capture_output=True, text=True)
    return p.returncode, json.loads(p.stdout), (time.time() - t0) * 1e3
def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--sizes', default='8,12'); ap.add_argument('--instances', type=int, default=2); ap.add_argument('--seeds', type=int, default=4); a = ap.parse_args()
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, cwd=os.path.dirname(PROBBIT)).stdout.strip()
    print(json.dumps({"commit": commit, "load_before": os.getloadavg(), "sizes": a.sizes, "instances": a.instances, "seeds": a.seeds}))
    print("| n | inst | feasible | optimum | greedy (gap) | default: verdict, tier, n_feasible, max \\|odds - brute\\|, plan = optimum?, ms | sampler verdicts | released | max \\|odds - brute\\| released | median ms |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    tot = {"answers": 0, "released": 0, "wrong": 0, "false": 0}
    for n in map(int, a.sizes.split(',')):
        for inst in range(1, a.instances + 1):
            sc, ef, conf, lim, capc = instance(n, 17 * n + inst); prog = program(sc, ef, conf, lim, capc)
            cnt, logz, mg, best = brute(sc, ef, conf, lim, capc); g = greedy(sc, ef, conf, lim, capc)
            od = lambda e, v: max(abs(e['marginals'][v].get(w, 0.0) - mg[int(v[1:])][q]) for q, w in enumerate(W))
            c, d, ms = run(prog); dm = max(od(d, f"t{i}") for i in range(n)); same = [W.index(d['plan'][f"t{i}"]) for i in range(n)] == list(best[1])
            vs, rel, worst, times = [], 0, 0.0, []
            for s in range(1, a.seeds + 1):
                c2, e, ms2 = run(prog, '--op', 'sample', '--seed', str(s)); vs.append(e['verdict']); times.append(ms2); tot["answers"] += 1
                released = e.get('released', []); rel += len(released); tot["released"] += len(released)
                err = max([od(e, v) for v in released] or [0.0]); worst = max(worst, err); tot["wrong"] += sum(od(e, v) > 0.05 for v in released)
                if e['verdict'] == 'diagnostics_passed' and err > 0.05: tot["false"] += 1
            vstr = ', '.join(f"{v} {vs.count(v)}" for v in sorted(set(vs))); gs = f"{g:.4f} ({best[0] - g:+.4f})" if g is not None else "stuck"
            print(f"| {n} | {inst} | {cnt} | {best[0]:.4f} | {gs} | {d.get('verdict')}, {d.get('tier')}, {d.get('n_feasible')}, {dm:.1e}, {same}, {ms:.1f} | {vstr} | {rel}/{n * a.seeds} | {worst:.4f} | {sorted(times)[len(times) // 2]:.0f} |")
    print(json.dumps({"totals": tot, "load_after": os.getloadavg()}))
if __name__ == '__main__': main()
