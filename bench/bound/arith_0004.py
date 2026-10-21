# Arithmetic check for RFC 0004 `bound` (Python 3.9, standard library only; no engine).
# Recomputes every number of the RFC's worked examples and writes ARITH-CHECK-0004.json. Every check must be "ok".
# Inputs: the reference persona's resting odds (engine outputs of the 0.8.2 build, rest probe 200 h, seeds 0-2) and the seed
# reference's printed epsilons. The toys are plain-Python exponential families (label: simulation): exact enumeration.
import itertools, json, math, os, random

HERE = os.path.dirname(os.path.abspath(__file__))
checks = []


def check(name, got, want, tol):
    ok = abs(got - want) <= tol
    checks.append({"check": name, "got": round(got, 9), "want": round(want, 9), "tol": tol, "status": "ok" if ok else "MISMATCH"})


def truth(name, cond, detail=""):
    checks.append({"check": name, "got": bool(cond), "want": True, "detail": detail, "status": "ok" if cond else "MISMATCH"})


def subsets(n):
    return [tuple(i for i in range(n) if m >> i & 1) for m in range(1, 2 ** n - 1)]


def closed(p, c):
    """max over proper non-empty S of k a_S / (k a_S + 1 - a_S) - a_S, k = e^(2c); returns (epsilon, S)."""
    k = math.exp(2 * c)
    best = (-1.0, None)
    for S in subsets(len(p)):
        a = sum(p[i] for i in S)
        best = max(best, (k * a / (k * a + 1 - a) - a, S))
    return best


# 1. The reference persona (three 3-level learned traits, step_cap 0.2): resting odds, engine outputs.
REST = {0: {"directness": [0.325003, 0.333263, 0.341734], "verbosity": [0.309123, 0.332732, 0.358145],
            "initiative": [0.166128, 0.298288, 0.535584]},
        1: {"directness": [0.415589, 0.327045, 0.257366], "verbosity": [0.356885, 0.332791, 0.310323],
            "initiative": [0.276953, 0.329953, 0.393094]},
        2: {"directness": [0.08575, 0.240383, 0.673867], "verbosity": [0.389414, 0.330347, 0.280239],
            "initiative": [0.226534, 0.320375, 0.453091]}}
SEEDRUN_C1 = {(0, "directness"): 0.45559, (0, "initiative"): 0.460224, (0, "verbosity"): 0.45865,
              (1, "directness"): 0.461793, (1, "initiative"): 0.461969, (1, "verbosity"): 0.458449,
              (2, "directness"): 0.460062, (2, "initiative"): 0.457422, (2, "verbosity"): 0.461825}
for (s, t), want in sorted(SEEDRUN_C1.items()):
    check("closed form, total_cap 1, seed %d %s = seed run's printed epsilon" % (s, t), closed(REST[s][t], 1.0)[0], want, 2e-6)
v0 = REST[0]["verbosity"]
check("closed form, total_cap 0.25, seed 0 verbosity", closed(v0, 0.25)[0], 0.121011, 2e-6)
check("closed form, total_cap 2, seed 0 verbosity", closed(v0, 2.0)[0], 0.651552, 2e-6)
truth("attaining set at total_cap 1 is {terse}", closed(v0, 1.0)[1] == (0,), str(closed(v0, 1.0)[1]))
truth("attaining set at total_cap 0.25 is {full}", closed(v0, 0.25)[1] == (2,), str(closed(v0, 0.25)[1]))
truth("attaining set at total_cap 2 is {terse}", closed(v0, 2.0)[1] == (0,), str(closed(v0, 2.0)[1]))
for c, want in ((0.25, 0.124353), (1.0, 0.462117), (2.0, 0.761594)):
    check("ceiling tanh(total_cap/2) at %g" % c, math.tanh(c / 2), want, 1e-6)
    k, a = math.exp(2 * c), 1 / (1 + math.exp(c))
    check("ceiling attained at a = 1/(1+e^c), total_cap %g" % c, k * a / (k * a + 1 - a) - a, math.tanh(c / 2), 1e-12)
    worst = max(closed(REST[s][t], c)[0] for s in REST for t in REST[s])
    truth("every reference epsilon <= ceiling at total_cap %g" % c, worst <= math.tanh(c / 2) + 1e-12, "%.6f" % worst)
rng = random.Random(4)
for _ in range(3):
    c = rng.uniform(0.05, 3)
    grid = max(math.exp(2 * c) * a / (math.exp(2 * c) * a + 1 - a) - a for a in [i / 20000 for i in range(1, 20000)])
    check("ceiling = max over a of the closed form (grid), total_cap %.4f" % c, grid, math.tanh(c / 2), 1e-7)
# the greedy-reachable gap the ceiling leaves at total_cap 2 for near-uniform odds (bo2's ceiling null)
check("closed form at total_cap 2 for a 1/3 level", closed([1 / 3, 1 / 3, 1 / 3], 2.0)[0], 0.63133, 1e-6)
check("its ratio to the ceiling", closed([1 / 3, 1 / 3, 1 / 3], 2.0)[0] / math.tanh(1.0), 0.828958, 1e-6)
check("reference seed 0 verbosity: epsilon / ceiling at total_cap 2", closed(v0, 2.0)[0] / math.tanh(1.0), 0.855511, 2e-6)

# 2. Toy exponential families (simulation): exact enumeration over declared variables.
def dist(prior, pairs, w):
    """prior[v] = log weights; pairs[(u, v)] = 3x3 log table; w[v] = unary learned weights. Returns the joint."""
    V = sorted(prior)
    xs = list(itertools.product(range(3), repeat=len(V)))
    e = []
    for x in xs:
        h = sum(prior[v][x[i]] + w[v][x[i]] for i, v in enumerate(V))
        h += sum(T[x[V.index(u)]][x[V.index(v)]] for (u, v), T in pairs.items())
        e.append(math.exp(h))
    Z = sum(e)
    return V, xs, [q / Z for q in e]


def pS(prior, pairs, w, t, S):
    V, xs, p = dist(prior, pairs, w)
    i = V.index(t)
    return sum(q for x, q in zip(xs, p) if x[i] in S)


def marg(prior, pairs, w, t):
    V, xs, p = dist(prior, pairs, w)
    i = V.index(t)
    return [sum(q for x, q in zip(xs, p) if x[i] == l) for l in range(3)]


def align(a):
    c = [-1, 0, 1]
    return [[a * c[i] * c[j] for j in range(3)] for i in range(3)]


P2 = {"x": [0.2, 0.0, -0.4], "y": [-0.3, 0.1, 0.0]}
PAIR2 = {("x", "y"): [[r + s for r, s in zip(ra, rb)] for ra, rb in zip(align(1.4), [[0, 0, 0], [0, 0, -0.9], [0.5, 0, 0]])]}
C = 1.0
zero = {"x": [0.0] * 3, "y": [0.0] * 3}
coords = [(v, l) for v in ("x", "y") for l in range(3)]
grid = [-2 + 0.05 * i for i in range(81)]
lf_err, mono_bad, lines = 0.0, 0, 0
for trial in range(12):
    bg = {v: [rng.uniform(-C, C) for _ in range(3)] for v in ("x", "y")} if trial else zero
    for (v, l) in coords:
        for S in subsets(3):
            def at(wv):
                w = {u: list(bg[u]) for u in bg}
                w[v][l] = wv
                return w
            V, xs, p = dist(P2, PAIR2, at(0.0))
            ix, iv = V.index("x"), V.index(v)
            mu = [q for q in p]
            A0 = sum(q for x, q in zip(xs, mu) if x[ix] in S and x[iv] != l) * math.exp(0)
            A1 = sum(q for x, q in zip(xs, mu) if x[ix] in S and x[iv] == l)
            B0 = sum(q for x, q in zip(xs, mu) if x[ix] not in S and x[iv] != l)
            B1 = sum(q for x, q in zip(xs, mu) if x[ix] not in S and x[iv] == l)
            vals = []
            for g in grid:
                y = math.exp(g)
                f = (A0 + A1 * y) / (A0 + B0 + (A1 + B1) * y)
                got = pS(P2, PAIR2, at(g), "x", S)
                lf_err = max(lf_err, abs(got - f))
                vals.append(got)
            inc = all(b >= a - 1e-13 for a, b in zip(vals, vals[1:]))
            dec = all(b <= a + 1e-13 for a, b in zip(vals, vals[1:]))
            mono_bad += 0 if (inc or dec) else 1
            lines += 1
check("2-trait toy: p_S(w) = (A0 + A1 e^w)/(A0 + B0 + (A1 + B1) e^w), max error over %d lines x 81 points" % lines, lf_err, 0.0, 1e-12)
check("2-trait toy: lines that are not monotone (own and coupled weights, 12 backgrounds)", mono_bad, 0, 0)

# vertex maximum = brute-force grid maximum over the whole box [-C, C]^6
vals5 = [-C, -C / 2, 0.0, C / 2, C]
gmax = {S: -1.0 for S in subsets(3)}
for pt in itertools.product(vals5, repeat=6):
    w = {"x": list(pt[:3]), "y": list(pt[3:])}
    m = marg(P2, PAIR2, w, "x")
    for S in gmax:
        gmax[S] = max(gmax[S], sum(m[i] for i in S))
p0x = marg(P2, PAIR2, zero, "x")
worst = 0.0
for S in gmax:
    vmax = max(sum(marg(P2, PAIR2, {"x": [C if i in S else -C for i in range(3)], "y": list(vy)}, "x")[i] for i in S)
               for vy in itertools.product((-C, C), repeat=3))
    worst = max(worst, abs(vmax - gmax[S]))
check("2-trait toy: vertex max (own corner fixed, 8 vertices of y) = grid max over 15,625 points, every S", worst, 0.0, 1e-12)
eps_vertex = max(max(sum(marg(P2, PAIR2, {"x": [C if i in S else -C for i in range(3)], "y": list(vy)}, "x")[i] for i in S)
                     for vy in itertools.product((-C, C), repeat=3)) - sum(p0x[i] for i in S) for S in subsets(3))
eps_own = max(sum(marg(P2, PAIR2, {"x": [C if i in S else -C for i in range(3)], "y": [0.0] * 3}, "x")[i] for i in S)
              - sum(p0x[i] for i in S) for S in subsets(3))
check("2-trait toy: own-weights-only corner = closed form from the resting marginal (coupling present)", eps_own, closed(p0x, C)[0], 1e-12)
truth("2-trait toy: a coupled learned trait raises epsilon above the per-trait closed form (bo6's clipped-counter null)",
      eps_vertex - closed(p0x, C)[0] > 1e-3, "vertex %.6f vs closed form %.6f" % (eps_vertex, closed(p0x, C)[0]))
truth("2-trait toy: vertex epsilon <= coarse ceiling tanh(m C / 2), m = 2", eps_vertex <= math.tanh(2 * C / 2) + 1e-12,
      "%.6f <= %.6f" % (eps_vertex, math.tanh(C)))

# 3. Derivative identity (own trait) and the covariance form (coupled weight), central differences
h, derr, cerr = 1e-6, 0.0, 0.0
for _ in range(40):
    w = {v: [rng.uniform(-C, C) for _ in range(3)] for v in ("x", "y")}
    m = marg(P2, PAIR2, w, "x")
    for S in subsets(3):
        ps = sum(m[i] for i in S)
        for l in range(3):
            wp = {u: list(w[u]) for u in w}; wp["x"][l] += h
            wm = {u: list(w[u]) for u in w}; wm["x"][l] -= h
            fd = (pS(P2, PAIR2, wp, "x", S) - pS(P2, PAIR2, wm, "x", S)) / (2 * h)
            want = m[l] * (1 - ps) if l in S else -ps * m[l]
            derr = max(derr, abs(fd - want))
            V, xs, p = dist(P2, PAIR2, w)
            joint = sum(q for x, q in zip(xs, p) if x[0] in S and x[1] == l)
            py = sum(q for x, q in zip(xs, p) if x[1] == l)
            wp = {u: list(w[u]) for u in w}; wp["y"][l] += h
            wm = {u: list(w[u]) for u in w}; wm["y"][l] -= h
            fd = (pS(P2, PAIR2, wp, "x", S) - pS(P2, PAIR2, wm, "x", S)) / (2 * h)
            cerr = max(cerr, abs(fd - (joint - ps * py)))
check("derivative identity d p_S/d w_l = p_l(1 - p_S) (l in S), -p_S p_l (l not in S), 40 points", derr, 0.0, 1e-8)
check("coupled weight: d p_S/d w = P(S, y=m) - p_S P(y=m), 40 points", cerr, 0.0, 1e-8)

# 4. 3-trait coupled toy (bo6 stand-in): vertex search with the own corner fixed = brute force
P3 = {"x": [0.3, 0.0, -0.2], "y": [0.0, 0.4, -0.1], "z": [-0.5, 0.0, 0.2]}
PAIR3 = {("x", "y"): align(1.5), ("y", "z"): align(-1.2), ("x", "z"): [[0, 0.6, 0], [0, 0, 0], [-0.7, 0, 0.3]]}
zero3 = {v: [0.0] * 3 for v in P3}
worst3, beat = 0.0, 0
bmax = {(t, S): -1.0 for t in P3 for S in subsets(3)}
for pt in itertools.product((-C, 0.0, C), repeat=9):
    V, xs, p = dist(P3, PAIR3, {"x": list(pt[0:3]), "y": list(pt[3:6]), "z": list(pt[6:9])})
    for i, t in enumerate(V):
        m = [sum(q for x, q in zip(xs, p) if x[i] == l) for l in range(3)]
        for S in subsets(3):
            bmax[(t, S)] = max(bmax[(t, S)], sum(m[j] for j in S))
for t in ("x", "y", "z"):
    others = [v for v in ("x", "y", "z") if v != t]
    for S in subsets(3):
        vmax = -1.0
        for vv in itertools.product((-C, C), repeat=6):
            w = {t: [C if i in S else -C for i in range(3)], others[0]: list(vv[:3]), others[1]: list(vv[3:])}
            m = marg(P3, PAIR3, w, t)
            vmax = max(vmax, sum(m[i] for i in S))
        for _ in range(150):
            w = {v: [rng.uniform(-C, C) for _ in range(3)] for v in P3}
            m = marg(P3, PAIR3, w, t)
            beat += sum(m[i] for i in S) > vmax + 1e-12
        worst3 = max(worst3, abs(vmax - bmax[(t, S)]))
check("3-trait coupled toy: vertex max (64 vertices) = grid max over 19,683 points, every trait and S", worst3, 0.0, 1e-12)
check("3-trait coupled toy: random interior points above the vertex max (18 x 150)", beat, 0, 0)

# 5. Speed floor: a feedback event moves a weight at most 2 s g (L-1)/L after recentring; epsilon of the shrunk box
for L in range(2, 8):
    s, best = 0.2, 0.0
    for _ in range(4000):
        st = [rng.choice((-s, s, rng.uniform(-s, s))) for _ in range(L)]
        mu = sum(st) / L
        best = max(best, max(abs(x - mu) for x in st))
    check("recentred step bound 2 s (L-1)/L, L = %d (attained by +s on one level, -s on the rest)" % L, best, 2 * s * (L - 1) / L, 1e-12)
m1 = 2 * 0.2 * 1 * 2 / 3
check("reference persona per-event move m = 2 x 0.2 x 1 x 2/3", m1, 0.266667, 1e-6)
e_full = closed(v0, 1.0)[0]
floor = next(n for n in range(1, 50) if closed(v0, min(1.0, n * m1))[0] >= e_full / 2)
check("epsilon after 1 feedback event (box 0.266667), seed 0 verbosity", closed(v0, m1)[0], 0.129335, 2e-6)
check("epsilon after 2 feedback events (box 0.533333), seed 0 verbosity", closed(v0, 2 * m1)[0], 0.260368, 2e-6)
check("speed floor: fewest feedback events whose box reaches epsilon/2 = 0.229325", floor, 2, 0)

# 6. Decayed residual of a mood (reference valence: inertia 0.6, half-life 6 h, effect range per level 3.6 nats, gains 1)
k_in, W = 0.6, 1.8 - (-1.8)
for hours, want in ((200.0, 4.9895e-11), (42.0, 0.004219), (180.0, 5.0291e-10)):
    d = 0.5 ** (hours / 6.0)
    r = math.tanh(k_in * d * W / 4)
    check("residual tanh(k d W / 4) at rest %g h, relative to %g" % (hours, want), r / want, 1.0, 1e-3)
check("no half-life (d = 1): coarse cap tanh(0.6 x 3.6 / 4) on a coupled trait (measured trace 0.121 below it)",
      math.tanh(k_in * W / 4), 0.492988, 1e-6)

# 7. Cost: (2^L - 2) level sets x 2^V vertices; the 12-level cap
check("level sets of a 3-level trait", 2 ** 3 - 2, 6, 0)
check("probes per seed, 3-level trait alone", (2 ** 3 - 2) * 1, 6, 0)
check("probes per seed, 3-level trait with 9 other learned levels (cap)", (2 ** 3 - 2) * 2 ** 9, 3072, 0)
check("coarse ceiling, 5 coupled learned traits at total_cap 1: tanh(5/2)", math.tanh(5 / 2), 0.986614, 1e-6)

ok = sum(c["status"] == "ok" for c in checks)
out = {"total": len(checks), "ok": ok, "mismatches": len(checks) - ok, "checks": checks}
json.dump(out, open(os.path.join(HERE, "ARITH-CHECK-0004.json"), "w"), indent=1)
print(json.dumps({"total": len(checks), "ok": ok, "mismatches": len(checks) - ok}))
for c in checks:
    if c["status"] != "ok":
        print("MISMATCH", c["check"], c["got"], c["want"])
