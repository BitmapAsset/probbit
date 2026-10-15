"""Recompute every closed form RFC 0002 states (Python 3.9, standard library only): bench/body's arithmetic check.

    python3 arith_0002.py [--json FILE]   # prints the counts; exit 0 when every check matches, 1 on a mismatch

Each check records the value RFC 0002 or its research states (`expected`), the value computed here (`computed`), the
tolerance and `ok`. A mismatch is reported as a finding, never adjusted. Stand-in reruns (seeded) are marked
`kind: stand-in`; they repeat the research's small simulations from their published recipe, not the engine. The ARITH ids
are the bracketed references in README.md.
"""
import json
import math
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
R6 = lambda x: round(x, 6)
checks = []


def add(cid, what, expected, computed, tol, source, kind="closed form", note=""):
    if expected == "informational":
        ok = True
    elif isinstance(expected, (int, float)) and isinstance(computed, (int, float)):
        ok = abs(expected - computed) <= tol
    else:
        ok = expected == computed
    checks.append({"id": cid, "what": what, "expected": expected, "computed": computed, "tolerance": tol,
                   "ok": bool(ok), "source": source, "kind": kind, "note": note})


# ---------------------------------------------------------------- P1 needs: D(x) = (sum w x^n)^(1/m), n = 4, m = 3
N, M = 4.0, 3.0


def drive(xs, ws=None):
    ws = ws or [1.0] * len(xs)
    return sum(w * x ** N for x, w in zip(xs, ws)) ** (1.0 / M)


def reduction(xs, i, k, ws=None):
    after = list(xs)
    after[i] = abs(xs[i] - k)
    return drive(xs, ws) - drive(after, ws)


r_hi, r_lo, r_sat = reduction([0.8], 0, 0.2), reduction([0.2], 0, 0.2), reduction([0.0], 0, 0.2)
add("n1.ratio", "pay ratio of amount 0.2 at deficit 0.8 vs 0.2 (bar >= 2.0)", 2.02, round(r_hi / r_lo, 4), 0.005,
    "RFC 0002 research P1 bar n1")
add("n1.values", "r(0.8, 0.2) and r(0.2, 0.2)", "informational", [R6(r_hi), R6(r_lo)], 0, "computed here", note="no stated value")
add("n2.overshoot", "r at deficit 0, amount 0.2 (bar < 0)", -0.117, round(r_sat, 4), 0.0005, "RFC 0002 research P1 bar n2")

# n9 by construction: for one need, sign(D(x) - D(|x - K|)) = sign(x - K/2) on a grid (0 negative wins at horizon 0)
bad = 0
grid = [i / 100.0 for i in range(0, 101)]
for x in grid:
    for k in grid[1:]:
        r = reduction([x], 0, k)
        s = (x > k / 2.0) - (x < k / 2.0)
        if (r > 1e-12) - (r < -1e-12) != s:
            bad += 1
add("n9.sign_rule", "grid points where sign(r) != sign(x - K/2), one need, x and K on a 0.01 grid", 0, bad, 0,
    "derived: x^(4/3) is increasing, so r > 0 iff |x - K| < x iff x > K/2")

# n3 null: on the frozen probe grid (two needs, K 0.6 and 0.8, weights 1, equal E), states where pursuing by the marginal
# drive dD/dx picks a different goal than pursuing by the expected reduction (the null n3 must reject)
def marginal(xs, i):
    d = drive(xs)
    return 0.0 if d == 0 else (1.0 / M) * d ** (1.0 - M) * N * xs[i] ** (N - 1.0)


grid81 = [(a / 10.0, b / 10.0) for a in range(1, 10) for b in range(1, 10) if a != b]
tie_states = [p for p in grid81 if abs(reduction(list(p), 0, 0.6) - reduction(list(p), 1, 0.8)) < 1e-6]
probe = [p for p in grid81 if p not in tie_states]
disagree = sum(1 for (a, b) in probe
               if (marginal([a, b], 0) > marginal([a, b], 1)) != (reduction([a, b], 0, 0.6) > reduction([a, b], 1, 0.8)))
add("n3.null_marginal", "probe states (of %d) where the marginal-drive rule and the reduction rule pick different goals" % len(probe),
    "> 0 (so the reverted rule fails n3)", disagree, 0, "computed here (probe grid of the bench fixture)",
    note="ok when > 0")
checks[-1]["ok"] = disagree > 0
add("n3.ties", "off-diagonal grid states with equal expected reductions, excluded from the probe set", "informational",
    [list(p) for p in tie_states], 0, "computed here", note="(0.3, 0.4): both deficits at K/2, both reductions 0")
add("n3.probe_size", "probe states after excluding ties", 71, len(probe), 0, "computed here")
tmax = max(2.0 * 1.0 * max(reduction(list(q), 0, 0.6), reduction(list(q), 1, 0.8)) for q in probe)
add("n3.term_max", "largest pursue term on the probe grid at want 2, success 1 (must stay below pursue.cap 3)", "< 3",
    round(tmax, 6), 0, "computed here")
checks[-1]["ok"] = tmax < 3

# n11 by construction: with no wins during the absence, s only falls (x 0.8 per unanswered report); the contact term is
# at most want x s x max reduction, and it must fall under the interest gap ln(1.0 / 0.6) between work and report
gap = math.log(1.0 / 0.6)
dmax = max(reduction([a / 100.0, b / 100.0], 1, 0.8) for a in range(0, 101) for b in range(0, 101))
k_need = 0
while 2.0 * 0.8 ** k_need * dmax >= gap:
    k_need += 1
add("n11.extinction", "unanswered reports after which 2 x 0.8^k x max contact reduction < ln(1/0.6) (max reduction %.4f)" % dmax,
    "informational", k_need, 0, "computed here", note="at most this many report pursuits after the absence starts, "
    "so they fit in the absence's opening waking day (15 h)")
checks[-1]["ok"] = k_need <= 15

# ---------------------------------------------------------------- P4 valence: inertia k = 0.61 plays the forgetting factor
k = 0.61
add("v3.five_back", "weight of an event 5 events back at k = 0.61 (bar <= 0.09)", 0.0845, round(k ** 5, 4), 0.00005,
    "RFC 0002 research P4 (the forgetting factor 0.61 +- 0.30)")
add("v3.half_life", "half-life in events of k = 0.61", 1.402, round(math.log(0.5) / math.log(k), 3), 0.0005,
    "RFC 0002 research P4")
add("s.vol_half_life", "half-life in events of the volatility average, a = 0.3", 1.943,
    round(math.log(0.5) / math.log(1 - 0.3), 3), 0.0005, "RFC 0002 research P5")

# om1: an omitted expected win, closed form (test values: E 0.8, rate 0.5, pe_cap 1, gene 0)
E0, rate, pe_cap = 0.8, 0.5, 1.0
d_om = -min(E0, pe_cap)
E1 = R6(E0 + rate * (0 - E0))
add("om1.closed_form", "omission at E 0.8, rate 0.5: d = -min(E, pe_cap), E' = E (1 - rate) (test values)", [-0.8, 0.4],
    [d_om, E1], 0, "RFC 0002 omission rule", note="identity check at test values")

# ---------------------------------------------------------------- P3 optimism: expectile tau b + (1 - tau) a
for ap, an, exp_stated in ((0.2, 0.2, None), (0.2, 0.1, 1.1667), (0.1, 0.2, None)):
    tau = ap / (ap + an)
    closed = tau * 1.5 + (1 - tau) * 0.5
    stationary = (0.5 * ap * 1.5 + 0.5 * an * 0.5) / (0.5 * ap + 0.5 * an)   # E[alpha x] / E[alpha]: the drift is linear
    if exp_stated is not None:
        add("o1.expectile", "expectile closed form, a+ %.1f a- %.1f, wins 0.5 / 1.5 at p 0.5" % (ap, an), exp_stated,
            round(closed, 4), 0.00005, "RFC 0002 research P3 bar o1")
    add("o1.stationary_%.1f_%.1f" % (ap, an), "stationary mean of E equals the expectile (linear drift on (a, b))",
        round(closed, 6), round(stationary, 6), 1e-9, "derived here")
rnd, E, tail = random.Random(1), 1.0, []
for t in range(20000):
    x = 1.5 if rnd.random() < 0.5 else 0.5
    E += (0.2 if x > E else 0.1) * (x - E)
    if t >= 10000:
        tail.append(E)
add("o1.stand_in", "seeded rerun of the research's recipe: mean E over steps 10,001-20,000", 1.1613, round(sum(tail) / len(tail), 4),
    0.00005, "RFC 0002 research P3 bar o1 (stand-in value)", kind="stand-in")
add("o1.margin", "|stand-in mean - expectile| (bar: within 0.01)", "< 0.01", round(abs(sum(tail) / len(tail) - 7.0 / 6.0), 4), 0,
    "computed here")
checks[-1]["ok"] = abs(sum(tail) / len(tail) - 7.0 / 6.0) < 0.01

# o2 by coupling: same stream, E_z >= E_0 at every step when z > 0 (and <= when z < 0); 100 individuals
def o2(spread, n_ind=100, steps=3000):
    zr = random.Random(7)
    zs = [zr.gauss(0.0, 1.0) for _ in range(n_ind)]
    sr = random.Random(11)
    stream = [1.5 if sr.random() < 0.5 else 0.5 for _ in range(steps)]
    agree = 0
    for z in zs:
        e_z = e_0 = 1.0
        ok = True
        for x in stream:
            ap, an = min(0.2 * math.exp(spread * z), 1.0), min(0.2 * math.exp(-spread * z), 1.0)
            e_z += (ap if x > e_z else an) * (x - e_z)
            e_0 += 0.2 * (x - e_0)
            if (z > 0 and e_z < e_0 - 1e-12) or (z < 0 and e_z > e_0 + 1e-12):
                ok = False
        agree += ok and ((e_z > e_0) == (z > 0))
    return agree


add("o2.coupling_s0.1", "individuals (of 100) whose E stays on the side of its z = 0 twin set by sign(z), every step, s 0.1",
    100, o2(0.1), 0, "RFC 0002 research P3 bar o2 (100/100)", kind="stand-in", note="seeds 7 / 11 here, not the research's")
add("o2.coupling_s0.3", "the same at s 0.3", 100, o2(0.3), 0, "RFC 0002 research P3 bar o2 (100/100)", kind="stand-in",
    note="seeds 7 / 11 here, not the research's")

# ---------------------------------------------------------------- P2 pace: R* = p base (1 - kappa R0) / (1 - G), G = p kappa base
BASE, R0, CAP, A_CAP, HL = 4.0, 1.0, 4.0, 12.0, 6.0
kk = 1 - 0.5 ** (1.0 / HL)
for p, kappa, stated in ((0.30, 0.2, 1.2632), (0.30, 0.5, 1.5000), (0.30, 0.7, 2.2500), (0.30, 0.9, 3.6000), (0.30, 1.0, 3.6000)):
    G = p * kappa * BASE
    R = R0 + 0.2
    for _ in range(400):   # the RFC's event form at h = 1: R <- R 0.5^(h/hl) + (1 - 0.5^(1/hl)) r, r = p a
        a = min(max(BASE * (1 + kappa * (R - R0)), 0.0), A_CAP)
        R = R6(min(max(R * 0.5 ** (1.0 / HL) + kk * (p * a), 0.0), CAP))
    if G < 1:
        closed = p * BASE * (1 - kappa * R0) / (1 - G)
        transient = closed + (R0 + 0.2 - closed) * (1 - kk * (1 - G)) ** 400
        add("p6.fixed_point_G%.2f" % G, "closed-form level R* at G = %.2f" % G, stated, round(closed, 4), 0.00005,
            "RFC 0002 research P2 bar p6")
        add("p6.trajectory_G%.2f" % G, "event-form R after 400 h vs the exact transient closed form", round(transient, 6), R,
            1e-4, "derived here (6-decimal rounding per event)")
    else:
        add("p6.pinned_G%.2f" % G, "R after 400 h at G = %.2f (pinned)" % G, stated, round(R, 4), 0.00005,
            "RFC 0002 research P2 bar p6", note="the pin is p x the host's action cap (0.3 x 12 = 3.6), not R_cap 4")
add("p6.stand_in_G0.84", "the research's 400-h stand-in value at G 0.84", 2.2491, None, 0.00005, "RFC 0002 research P2", kind="stand-in")
checks[-1]["computed"] = [c for c in checks if c["id"] == "p6.trajectory_G0.84"][0]["computed"]
checks[-1]["computed"] = round(checks[-1]["computed"], 4)
checks[-1]["ok"] = abs(checks[-1]["computed"] - 2.2491) <= 0.00005

# ---------------------------------------------------------------- P5 gain bounds: g = clip((1 + W r)/(r + 1) (r_ref + 1), 1, 3)
def gain(omega, rhat, r_ref=4.0, g_max=3.0):
    return min(max((1 + omega * rhat) / (rhat + 1) * (r_ref + 1), 1.0), g_max)


add("s.gain_bounds", "g at (omega 1, any r), (omega 0, r 1), (omega 0, r 4), (omega 0, r 20)", [3.0, 2.5, 1.0, 1.0],
    [gain(1, 7), gain(0, 1), gain(0, 4), gain(0, 20)], 0, "stand-in settings r_ref 4 (factor 5), g_max 3, r cap 20")

# ---------------------------------------------------------------- P6 rest: process S example rates (human values)
rise, fall = 0.055, 0.238
add("r.fall_30min", "multiplier per 30 min of idle at 0.238/h", 0.888, round(math.exp(-fall * 0.5), 3), 0.0005,
    "RFC 0002 research Q3 (x0.888 per 30 min)")
S = 0.2
for _ in range(16):
    S = 1 - (1 - S) * math.exp(-rise)
S_closed = 1 - (1 - 0.2) * math.exp(-rise * 16)
add("r.rise_closed_form", "S after 16 active hours from 0.2: hourly steps vs 1 - (1 - S0) e^(-r' t)", round(S_closed, 6), round(S, 6),
    1e-9, "derived here")
add("r.fall_closed_form", "S after 8 idle hours: S e^(-r t)", "informational", round(S_closed * math.exp(-fall * 8), 6), 0,
    "computed here", note="the research states the rates only")

# r0 by construction: Delta <- round(rho Delta, 6), prune |Delta| < eps never inverts a strict order (seeded vectors)
inv = top_changed = 0
vr = random.Random(5)
for _ in range(20000):
    D = [round(vr.uniform(-1, 1), 6) for _ in range(3)]
    for rho in (0.5, 0.75):
        Dn = [round(rho * x, 6) for x in D]
        Dn = [0.0 if abs(x) < 0.05 else x for x in Dn]
        for i in range(3):
            for j in range(3):
                if D[i] > D[j] and Dn[i] < Dn[j]:
                    inv += 1
        if max(D) >= 0.1 and D.index(max(D)) != Dn.index(max(Dn)):
            top_changed += 1
add("r0.inversions", "strict order inversions after a rest line, 20,000 seeded vectors x rho 0.5 / 0.75, prune 0.05", 0, inv, 0,
    "derived: x -> rho x is increasing for rho > 0")
add("r0.top_level", "vectors whose top level (max >= 0.1) changed after the rest line", 0, top_changed, 0, "derived here")


def softmax(h):
    m = max(h)
    e = [math.exp(x - m) for x in h]
    s = sum(e)
    return [x / s for x in e]


def life(seed, rho, T=60, noise=0.10):   # the research's r1 recipe: one 3-level trait, rate 0.5, step_cap 0.2, total_cap 1
    rnd = random.Random(seed)
    D = [0.0, 0.0, 0.0]

    def turn(good):
        odds = softmax(D)
        taken = max(range(3), key=lambda i: (odds[i], -i))
        sign = 1 if taken == good else -1
        if rnd.random() < noise:
            sign = -sign
        credit = [(1.0 if i == taken else 0.0) - odds[i] for i in range(3)]
        step = [min(max(sign * 0.5 * c, -0.2), 0.2) for c in credit]
        mu = sum(step) / 3
        for i in range(3):
            D[i] = round(min(max(D[i] + step[i] - mu, -1.0), 1.0), 6)
    for _ in range(T):
        turn(1)
    for i in range(3):
        D[i] = round(rho * D[i], 6)
    for n in range(1, T + 1):
        turn(2)
        if max(range(3), key=lambda i: (softmax(D)[i], -i)) == 2:
            return n
    return T + 1


def median(v):
    v = sorted(v)
    n = len(v)
    return v[n // 2] if n % 2 else (v[n // 2 - 1] + v[n // 2]) / 2.0


meds = [median([life(s, rho) for s in range(200)]) for rho in (1.0, 0.75, 0.5)]
add("r1.relearn", "median turns to re-learn a reversal, rho 1 / 0.75 / 0.5 (seeds 0-199)", [11, 8, 5], meds, 0,
    "RFC 0002 research P6 bar r1 (11 -> 5 at 0.5; 8 at 0.75)", kind="stand-in")
add("r1.ratio", "re-learning ratio at rho 0.5 (bar <= 0.5)", 0.4545, round(meds[2] / meds[0], 4), 0.00005, "derived from r1")

# ---------------------------------------------------------------- P7 load: recovery and repeated-hit closed forms (test values)
HL_L, KAP, THETA, L_CAP = 72.0, 0.5, 0.5, 4.0
L, gaps = 2.0, [3.0, 10.0, 0.5, 24.0, 7.25, 48.0]
for g in gaps:
    L = R6(min(max(L * 0.5 ** (g / HL_L), 0.0), L_CAP))
add("l2.recovery", "L after quiet gaps (sum 92.75 h) vs L0 0.5^(sum h / hl): test values hl 72 h, L0 2",
    round(2.0 * 0.5 ** (sum(gaps) / HL_L), 6), L, 1e-5, "RFC 0002 load rule", note="identity check at test values")
L = 0.0
for _ in range(400):
    L = R6(min(max(L * 0.5 ** (6.0 / HL_L) + KAP * max(1.0 - THETA, 0.0), 0.0), L_CAP))
steady = KAP * (1.0 - THETA) / (1 - 0.5 ** (6.0 / HL_L))
add("l1.repeated_hits", "repeated hits every 6 h (a 1, theta 0.5, kappa 0.5): L just after a hit -> min(cap, steady state)",
    round(min(steady, L_CAP), 6), L, 1e-5, "derived here", note="steady state %.6f exceeds L_cap, so the cap binds" % steady)

ok = all(c["ok"] for c in checks)
doc = {"doc": "ARITH-CHECK-0002", "python": "3.9-compatible, standard library only", "script": "bench/body/arith_0002.py",
       "checks": checks, "mismatches": [c["id"] for c in checks if not c["ok"]], "all_ok": ok}
if "--json" in sys.argv[1:]:
    with open(sys.argv[sys.argv.index("--json") + 1], "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=1, sort_keys=True)
        f.write("\n")
print("checks %d, mismatches %d: %s" % (len(checks), len(doc["mismatches"]), ", ".join(doc["mismatches"]) or "none"))
sys.exit(0 if ok else 1)
