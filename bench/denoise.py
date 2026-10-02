#!/usr/bin/env python3
"""R19.7 (P2.2 family 2): Ising-MRF image denoising on a W x H grid (W <= 8), the classic p-bit demo. Stdlib only.
Clean image = a filled ellipse; each pixel flipped with probability `flip`; program = one {0, 1} variable per pixel, unary
+-eta (log-odds of the observed pixel), Potts +J between 4-neighbours. Oracle = row transfer matrix (2^W states per row):
exact log Z, per-pixel P(1) and the MAP image (Viterbi). Baseline = ICM (iterated conditional modes from the noisy image).
probbit = `probbit run` at defaults (tier, plan) and `probbit run --op sample --seed S` (verdict, released pixels' max |odds - exact|).
Usage: python3 bench/denoise.py [--w 8] [--h 12] [--images 3] [--seeds 4] [--write-example examples/denoise-8x12.json]"""
import argparse, json, math, os, random, subprocess, time
PROBBIT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'probbit')
def lse(xs): m = max(xs); return m + math.log(sum(math.exp(x - m) for x in xs))
def image(w, h, seed, flip):
    r = random.Random(seed); cx, cy, rx, ry = (w - 1) / 2, (h - 1) / 2, w * r.uniform(0.25, 0.4), h * r.uniform(0.25, 0.4)
    clean = [[int(((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1) for x in range(w)] for y in range(h)]
    noisy = [[p ^ (r.random() < flip) for p in row] for row in clean]
    return clean, noisy
def program(noisy, eta, J):
    h, w = len(noisy), len(noisy[0]); vid = lambda y, x: f"p{y}_{x}"
    pairs = [{"i": vid(y, x), "j": vid(y, x + 1), "potts": J} for y in range(h) for x in range(w - 1)] + \
            [{"i": vid(y, x), "j": vid(y + 1, x), "potts": J} for y in range(h - 1) for x in range(w)]
    return {"probbit_ir": 1, "comment": f"Ising denoising {w}x{h}: unary +-{eta} toward the observed pixel, Potts {J} between 4-neighbours",
            "values": ["0", "1"], "vars": [{"id": vid(y, x), "h": {"1": eta if noisy[y][x] else -eta}} for y in range(h) for x in range(w)], "pairs": pairs}
def exact(noisy, eta, J):
    h, w = len(noisy), len(noisy[0]); S = 1 << w; bit = lambda s, x: (s >> x) & 1
    R = [[sum((eta if noisy[y][x] else -eta) * bit(s, x) for x in range(w)) + J * sum(bit(s, x) == bit(s, x + 1) for x in range(w - 1)) for s in range(S)] for y in range(h)]
    V = [[J * (w - bin(a ^ b).count('1')) for b in range(S)] for a in range(S)]
    al = [R[0][:]]; mx = [R[0][:]]; bp = []
    for y in range(1, h):
        al.append([R[y][s] + lse([al[-1][a] + V[a][s] for a in range(S)]) for s in range(S)])
        row = []; nm = []
        for s in range(S): best = max(range(S), key=lambda a: mx[-1][a] + V[a][s]); row.append(best); nm.append(R[y][s] + mx[-1][best] + V[best][s])
        bp.append(row); mx.append(nm)
    be = [[0.0] * S for _ in range(h)]
    for y in range(h - 2, -1, -1): be[y] = [lse([V[s][b] + R[y + 1][b] + be[y + 1][b] for b in range(S)]) for s in range(S)]
    logz = lse(al[-1]); p1 = [[0.0] * w for _ in range(h)]
    for y in range(h):
        g = [al[y][s] + be[y][s] - logz for s in range(S)]
        for x in range(w): p1[y][x] = sum(math.exp(g[s]) for s in range(S) if bit(s, x))
    s = max(range(S), key=lambda a: mx[-1][a]); rows = [s]
    for y in range(h - 2, -1, -1): s = bp[y][s]; rows.append(s)
    rows.reverse(); mp = [[bit(rows[y], x) for x in range(w)] for y in range(h)]
    return logz, p1, mp
def icm(noisy, eta, J):
    h, w = len(noisy), len(noisy[0]); x = [row[:] for row in noisy]; changed = True
    while changed:
        changed = False
        for y in range(h):
            for c in range(w):
                nb = [x[yy][cc] for yy, cc in ((y - 1, c), (y + 1, c), (y, c - 1), (y, c + 1)) if 0 <= yy < h and 0 <= cc < w]
                e1 = (eta if noisy[y][c] else -eta) + J * sum(nb); e0 = J * (len(nb) - sum(nb)); v = int(e1 > e0)
                if v != x[y][c]: x[y][c] = v; changed = True
    return x
def errs(a, b): return sum(p != q for ra, rb in zip(a, b) for p, q in zip(ra, rb))
def run(prog, *args):
    t = time.time(); p = subprocess.run([PROBBIT, 'run', *args], input=json.dumps(prog), capture_output=True, text=True)
    return p.returncode, json.loads(p.stdout), (time.time() - t) * 1e3
def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--w', type=int, default=8); ap.add_argument('--h', type=int, default=12)
    ap.add_argument('--images', type=int, default=3); ap.add_argument('--seeds', type=int, default=4); ap.add_argument('--flip', type=float, default=0.1)
    ap.add_argument('--eta', type=float, default=1.1); ap.add_argument('--J', type=float, default=0.7); ap.add_argument('--write-example', default=None); a = ap.parse_args()
    commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True, cwd=os.path.dirname(PROBBIT)).stdout.strip()
    print(json.dumps({"commit": commit, "load_before": os.getloadavg(), "w": a.w, "h": a.h, "flip": a.flip, "eta": a.eta, "J": a.J, "seeds": a.seeds}))
    print("| image | noisy err | ICM err | exact MAP err | exact MPM err | default: tier, plan err, plan = MAP?, ms | sampler verdicts | released | max \\|odds - exact\\| | sampler MPM err | median ms |")
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    tot = {"answers": 0, "released": 0, "wrong": 0, "false": 0}
    for im in range(1, a.images + 1):
        clean, noisy = image(a.w, a.h, im, a.flip); prog = program(noisy, a.eta, a.J)
        if a.write_example and im == 1: json.dump(prog, open(a.write_example, 'w'), indent=1)
        logz, p1, mp = exact(noisy, a.eta, a.J); mpm = [[int(p > 0.5) for p in row] for row in p1]; ic = icm(noisy, a.eta, a.J)
        c, d, ms = run(prog); plan = [[int(d['plan'][f"p{y}_{x}"]) for x in range(a.w)] for y in range(a.h)] if 'plan' in d else None
        dflt = f"{d.get('tier', d.get('verdict'))}, {errs(plan, clean) if plan else '-'}, {plan == mp if plan else '-'}, {ms:.1f}"
        vs, rel, worst, times, smpm = [], 0, 0.0, [], []
        for s in range(1, a.seeds + 1):
            c2, e, ms2 = run(prog, '--op', 'sample', '--seed', str(s)); vs.append(e['verdict']); times.append(ms2); tot["answers"] += 1
            released = e.get('released', []); rel += len(released); tot["released"] += len(released)
            def od(v): y, x = map(int, v[1:].split('_')); return abs(e['marginals'][v].get('1', 0.0) - p1[y][x])
            err = max([od(v) for v in released] or [0.0]); worst = max(worst, err); tot["wrong"] += sum(od(v) > 0.05 for v in released)
            if e['verdict'] == 'diagnostics_passed' and err > 0.05: tot["false"] += 1
            smpm.append(errs([[int(e['marginals'][f"p{y}_{x}"].get('1', 0.0) > 0.5) for x in range(a.w)] for y in range(a.h)], clean))
        vstr = ', '.join(f"{v} {vs.count(v)}" for v in sorted(set(vs)))
        print(f"| {im} | {errs(noisy, clean)} | {errs(ic, clean)} | {errs(mp, clean)} | {errs(mpm, clean)} | {dflt} | {vstr} | {rel}/{a.w * a.h * a.seeds} | {worst:.4f} | {min(smpm)}-{max(smpm)} | {sorted(times)[len(times) // 2]:.0f} |")
    print(json.dumps({"totals": tot, "load_after": os.getloadavg()}))
if __name__ == '__main__': main()
