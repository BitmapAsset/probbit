"""Recompute every closed form RFC 0002 states (Python 3.9, standard library only): bench/body's arithmetic check.

    python3 arith_0002.py [--json FILE]   # prints the counts; exit 0 when every check matches, 1 on a mismatch

Bars v2 import it: the closed forms a repaired bar compares an engine against (need_win, gate_first, pace_run, omission,
lineage_standin) are defined once, here.

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


def checks_by_id(cid):
    return [c for c in checks if c["id"] == cid][0]


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

# ================================================================ bars v2: the closed forms the repaired bars compare against
# (README "Bars v2"). Every function here is also imported by the part scripts or nulls.py, so a bar and its check share
# one formula.
NEEDS1_PER_HOUR = 0.1
N1_HOURS = (0.0, 0.5, 2.0, 5.0, 8.0)          # deficits 0, 0.05, 0.2, 0.5, 0.8 (needs1 grows 0.1 an hour)
N1_AMOUNTS = (0.1, 0.2, 0.3)


def need_win(x, a, r_cap=1.0):
    """RFC 0002 A.3, one need: r = clip(D(x) - D(|x - a|), -r_cap, r_cap), rounded to 6 decimals."""
    return R6(min(max(reduction([x], 0, a), -r_cap), r_cap)) + 0.0


def n1_grid():
    """(hours, amount, deficit, r) per cell of the needs1 grid."""
    return [(h, a, R6(NEEDS1_PER_HOUR * h), need_win(R6(NEEDS1_PER_HOUR * h), a)) for h in N1_HOURS for a in N1_AMOUNTS]


cells = n1_grid()
pos = [c for c in cells if c[3] > 0]
neg = [c for c in cells if c[3] < 0]
zero = [c for c in cells if c[3] == 0]
add("n1.grid", "wins on the positive cells of the needs1 grid (deficit, amount, r)", "informational",
    [[c[2], c[1], c[3]] for c in pos], 0, "computed here (RFC 0002 A.3)")
add("n1.grid_sizes", "cells with r > 0 / r < 0 / r = 0", [9, 5, 1], [len(pos), len(neg), len(zero)], 0, "computed here")
prod_err = max(abs(c[1] * c[2] - c[3]) for c in pos)
add("n1.null_product", "the competent null win = amount x deficit: its ratio at 0.8 / 0.2 (v1 bar >= 2) and its largest error "
    "on the positive cells (v2 tolerance 5e-6)", "> 5e-6 with ratio >= 2", [round((0.2 * 0.8) / (0.2 * 0.2), 4), round(prod_err, 6)], 0,
    "computed here", note="ok when the null passes v1's ratio and fails v2's exact wins")
checks[-1]["ok"] = (0.2 * 0.8) / (0.2 * 0.2) >= 2 and prod_err > 5e-6
add("n2.grid", "signed rewards on the negative cells and the zero cell (deficit, amount, r)", "informational",
    [[c[2], c[1], c[3]] for c in neg + zero], 0, "computed here")
lin_err = max(abs((min(c[1], c[2]) - max(c[1] - c[2], 0.0)) - c[3]) for c in neg)
add("n2.null_linear", "the competent null r = removed deficit - overshoot: r at deficit 0, amount 0.2 (v1 bar < 0), and its "
    "largest error on the negative cells", "< 0 and > 5e-6", [round(min(0.2, 0.0) - max(0.2 - 0.0, 0.0), 6), round(lin_err, 6)], 0,
    "computed here", note="ok when the null passes v1 and fails v2")
checks[-1]["ok"] = lin_err > 5e-6

# n3 v2: success rates set apart by three compact wins from success.init 0.2 (needs2-s02.yaml)
N3_INIT, N3_WARM_WINS = 0.2, 3
s_c, s_r = N3_INIT, N3_INIT
for _ in range(N3_WARM_WINS):
    s_c = R6(s_c + 0.2 * (1 - s_c))
add("n3.success_warmup", "success rates after three compact wins from init 0.2 (compact, report)", [0.5904, 0.2], [s_c, s_r], 1e-9,
    "RFC 0002 A.3 success rule")
flips = [p for p in probe if (s_c * reduction(list(p), 0, 0.6) > s_r * reduction(list(p), 1, 0.8))
         != (reduction(list(p), 0, 0.6) > reduction(list(p), 1, 0.8))]
add("n3.s_flips", "probe states whose order flips when the terms are weighted by (0.5904, 0.2): the s-blind null misorders them",
    "> 0", len(flips), 0, "computed here")
checks[-1]["ok"] = len(flips) > 0
add("n3.s_flip_states", "those states (x_context, x_contact)", "informational", [list(p) for p in flips], 0, "computed here")
tmax_s = max(2.0 * max(s_c * reduction(list(q), 0, 0.6), s_r * reduction(list(q), 1, 0.8)) for q in probe)
add("n3.term_max_s02", "largest term on the s02 probe (below pursue.cap 3)", "< 3", round(tmax_s, 6), 0, "computed here")
checks[-1]["ok"] = tmax_s < 3


# s3 v2: the opening gated step of a new learner (v 0, r 1): v 0.15, Omega 0, r 2, g = 5/3 at the defaults
def gate_first(sign, odds, taken, rate=0.5, step_cap=0.2, a=0.3, v0=0.45, r_ref=4.0, r_max=20.0, g_max=3.0, gated=True):
    """RFC 0002 B with RFC 0001 B.2 at e = 1: the learned weights and (v, r, g) after a learner's opening paid feedback."""
    if gated:
        v = R6(a * (1 - min(max(0.5, 0.01), 0.99)))           # mb 0 (no counts): p_same 0.5
        om = min(max((v - v0) / (1 - v0), 0.0), 1.0)
        r = R6(min((1 - om) * (1.0 + 1) + om, r_max))
        g = R6(min(max((1 + om * r) / (r + 1) * (r_ref + 1), 1.0), g_max))
    else:
        v, r, g = None, None, 1.0
    c = [(1.0 if l == taken else 0.0) - odds[l] for l in range(len(odds))]
    st = [min(max(sign * rate * g * x, -step_cap * g), step_cap * g) for x in c]
    mu = sum(st) / len(st)
    return [R6(x - mu) for x in st], (v, r, g)


third = [0.333333, 0.333333, 0.333333]
dg, vrg = gate_first(-1, third, 0)
df, _ = gate_first(-1, third, 0, gated=False)
add("s3.first_state", "(v, r, g) after a new gated learner's opening paid feedback", [0.15, 2.0, 1.666667], list(vrg), 1e-9,
    "RFC 0002 B (the listed order: v, Omega, counts, r, g, step)")
add("s3.first_step", "learned weights after a criticised opening step on `low` at odds 1/3: gated vs fixed (g 1)", "informational",
    [dg, df], 0, "computed here", note="the constant-gain null's step differs from the gate's by %.6f" % abs(dg[0] - df[0]))

# p1c v2 and p6 v2: pace's event form, R <- clip(R 0.5^(h/hl) + k r, 0, cap), rounded
PACE_K = 1 - 0.5 ** (1.0 / 6.0)


def pace_run(events, R=0.0, cap=4.0, hl=6.0, clip=True):
    out = []
    for h, r in events:
        R = R * 0.5 ** (h / hl) + PACE_K * r
        R = R6(min(max(R, 0.0), cap)) if clip else R6(max(R, 0.0))
        out.append(R)
    return out


steady = pace_run([(0 if i == 0 else 1, 0.5) for i in range(600)])
bursty = pace_run([(0 if i == 0 else 1, 4.0 if i % 8 == 7 else 0.0) for i in range(600)])
spaced = pace_run([(0 if i == 0 else 3, 1.5) for i in range(200)])
W0, W1 = 304, 600                                    # 37 whole 8-hour cycles
mean_s, mean_b = sum(steady[W0:W1]) / (W1 - W0), sum(bursty[W0:W1]) / (W1 - W0)
rng_b = max(bursty[W0:W1]) - min(bursty[W0:W1])
add("p1c.equal_average", "mean R-bar over hours 304-599, steady vs bursty (both 0.5 an hour)", "|diff| <= 0.02",
    [round(mean_s, 6), round(mean_b, 6)], 0, "computed here")
checks[-1]["ok"] = abs(mean_s - mean_b) <= 0.02
add("p1c.bursty_range", "R-bar's range over hours 304-599 on the bursty stream (bar <= 1.0); the current-reward pace's is 4",
    "<= 1.0", round(rng_b, 6), 0, "computed here")
checks[-1]["ok"] = rng_b <= 1.0
spaced_closed = PACE_K * 1.5 / (1 - 0.5 ** (3.0 / 6.0))
add("p1c.spaced", "R-bar at the last spaced event (3-hour spacing, wins 1.5) vs the closed form k r / (1 - 0.5^(3/6))",
    round(spaced_closed, 6), spaced[-1], 1e-4, "derived here")
burst_c = pace_run([(0, 4.0)] * 12)
burst_u = pace_run([(0, 4.0)] * 12, clip=False)
rec = pace_run([(1, 0.0)] * 24, R=burst_c[-1])
rec_null = [R6(min(x, 4.0)) for x in pace_run([(1, 0.0)] * 24, R=burst_u[-1], clip=False)]
add("p6.burst", "after 12 zero-hour wins of 4: R-bar capped (bar) and the uncapped null's", [4.0, "> 4"], [burst_c[-1], burst_u[-1]],
    0, "computed here", note="the cap starts to bind at event %d" % (burst_c.index(4.0) + 1))
checks[-1]["ok"] = burst_c[-1] == 4.0 and burst_u[-1] > 4.0
add("p6.recovery", "R-bar 1, 2, 3 h after the burst = 4 x 0.5^(h/6); a cap applied only when read gives 4, 4, 4",
    [R6(4.0 * 0.5 ** (h / 6.0)) for h in (1, 2, 3)], rec[:3], 2e-6, "derived here",
    note="read-time clip null: %s" % rec_null[:3])
checks[-1]["ok"] = max(abs(a - b) for a, b in zip(checks[-1]["expected"], rec[:3])) <= 2e-6 and rec_null[:2] == [4.0, 4.0]

# v1, v2, v3 v2: ratios of the valence push (the mood's scale, (1 - k) and the level spread, cancels)
E = 0.0
for _ in range(20):
    E = R6(E + 0.5 * (1 - E))
E_dec = R6(E * 0.5 ** (24 / 48.0))
add("v1.ratio", "final push with E built by 20 wins / with E 0, after 24 h: d_A / d_B = 1 - E 0.5^(24/48)", 0.292894,
    R6(1 - E_dec), 1e-6, "derived here (valence.yaml: rate 0.5, half-life 48 h)")
E3 = E10 = 0.0
for i in range(10):
    E10 = R6(E10 + 0.5 * (1 - E10))
    if i < 3:
        E3 = R6(E3 + 0.5 * (1 - E3))
add("v2.linearity", "E after 10 and after 3 wins of 1 (rate 0.5, 6-decimal steps) and their ratio: the bar's evidence-move ratio",
    "informational", [E10, E3, round(E10 / E3, 6)], 0, "derived here", note="an expected win's evidence is w_e x E, so the moves "
    "keep this ratio; a fixed bonus per win gives 1")
add("v3.lags", "push left 1-5 events after an impulse at inertia 0.61 (two-sided, 1e-4)", [round(0.61 ** j, 6) for j in range(1, 6)],
    [round(k ** j, 6) for j in range(1, 6)], 1e-9, "RFC 0002 E (forgetting factor)")


# om1 v2: the window before, at and after expiry, with an intervening win and a repeat (test values: E 0.8, rate 0.5, window 2)
def omission(E, events, window=2.0, rate=0.5, pe_cap=1.0, hl=48.0):
    """events: [(hours, win or None)] after a stance that pursued the goal; returns [(E, d)] per event (the window opens once)."""
    out, w = [], window
    for h, m in events:
        E = E * 0.5 ** (h / hl)
        d = 0.0
        if m:
            d = min(max(m - E, -pe_cap), pe_cap)
            E = E + rate * (m - E)
            w = None
        elif w is not None:
            w = max(w - h, 0.0)
            if w <= 0:
                d = -min(E, pe_cap)
                E = E + rate * (0 - E)
                w = None
        E = R6(E)
        out.append((E, R6(d) + 0.0))
    return out


om_cases = {"before_then_at": omission(0.8, [(1, None), (1, None)]), "after": omission(0.8, [(3, None), (3, None)]),
            "win_between": omission(0.8, [(1, 1.0), (3, None)])}
add("om1.cases", "(E, d) per event: before then at expiry; past expiry then a repeat; a win inside the window then past it",
    "informational", om_cases, 0, "RFC 0002 A.4 (test values)")
add("om1.fires", "events at which the window fires in each case", [[1], [0], []],
    [[i for i, (_, d) in enumerate(v) if d < 0] for v in om_cases.values()], 0, "derived here")

# r0 v2: the pressure S after the reversal's active phase vs a short control (rise 0.055 / h, threshold 0.5)
S_long = R6(1 - math.exp(-0.055 * 59 * 0.25))
S_short = R6(1 - math.exp(-0.055 * 29 * 0.25))
add("r0.pressure", "S after 59 x 0.25 h (reversal: a rest line at the gap) and 29 x 0.25 h (control: none)", "S_long >= 0.5 > S_short",
    [S_long, S_short], 0, "RFC 0002 part 2 (S <- 1 - (1 - S) e^(-r h), from S 0)")
checks[-1]["ok"] = S_long >= 0.5 > S_short


# r2 v2: the lineage stand-in (the r1 learner; each generation ends with an idle event, so the rest line precedes absorption)
def lineage_standin(rho, gens, noise=0.10, prune_only=False):
    rows_l = [json.loads(l) for l in open(os.path.join(HERE, "fixtures", "streams", "lineage.jsonl"), encoding="utf-8")]
    logw = [0.0, 0.0, 0.0]
    for row in rows_l[:gens]:
        D, prev = [0.0, 0.0, 0.0], None
        n = len(row["u"])
        for i in range(n):
            if i and i % 60 == 0:                     # the 8 h gap: a rest line, no feedback, its decision unpaid
                if rho < 1 or prune_only:
                    D = [R6(rho * x) for x in D]
                    D = [0.0 if abs(x) < 0.05 else x for x in D]
                prev = None
                continue
            elif prev is not None:
                taken, od = prev
                sign = 1 if taken == 1 else -1
                if row["u"][i - 1] < noise:
                    sign = -sign
                c = [(1.0 if l == taken else 0.0) - od[l] for l in range(3)]
                st = [min(max(sign * 0.5 * x, -0.2), 0.2) for x in c]
                mu = sum(st) / 3
                D = [R6(min(max(D[l] + st[l] - mu, -1.0), 1.0)) for l in range(3)]
            od = softmax([a + b for a, b in zip(logw, D)])
            prev = (max(range(3), key=lambda l: (od[l], -l)), od)
        if rho < 1 or prune_only:                     # the closing idle event: a rest line before absorption
            D = [R6(rho * x) for x in D]
            D = [0.0 if abs(x) < 0.05 else x for x in D]
        logw = [R6(a + d) for a, d in zip(logw, D)]
    p = softmax(logw)
    return R6(0.5 * sum(abs(x - 1.0 / 3) for x in p))


lin = {g: (lineage_standin(1.0, g), lineage_standin(0.5, g)) for g in (1, 2, 10)}
add("r2.stand_in", "lineage probe distance without / with rest after generations 1, 2, 10 (seeded stand-in, not the engine)",
    "informational", lin, 0, "the r1 recipe's learner on the frozen lineage draws", kind="stand-in",
    note="ratios %s; generation 10 saturates" % [round(v[1] / v[0], 4) if v[0] else None for v in lin.values()])

# l1, l2, l3 v2: load's closed forms (load.yaml: hl 72 h, kappa 0.5, theta 0.5, cap 4, criticism weight 1)
crit = []
L = 0.0
for _ in range(300):
    L = R6(min(max(L * 0.5 ** (6.0 / HL_L) + KAP * max(1.0 - THETA, 0.0), 0.0), L_CAP))
    crit.append(L)
add("l1.criticism", "L after hits 1-5 and the hit at which the cap starts to bind (a hit every 6 h)", "informational",
    [crit[:5], crit.index(L_CAP) + 1], 0, "RFC 0002 load rule")
add("l2.induction", "L after 4 criticisms at elapsed 0 (positive, checked independently)", 1.0, R6(4 * KAP * (1.0 - THETA)), 1e-9,
    "RFC 0002 load rule")
L3_GAPS = (1.0, 5.0, 12.0, 24.0)
tail, tot = [], 0.0
for g in L3_GAPS:
    tot += g
    tail.append(R6(L_CAP * 0.5 ** (tot / HL_L)))
add("l3.recovery", "L after the pinned hits, then quiet gaps of 1, 5, 12, 24 h", "informational", tail, 0, "RFC 0002 load rule")

if __name__ == "__main__":   # the scripts import this module for the v2 closed forms; run alone, it reports
    ok = all(c["ok"] for c in checks)
    doc = {"doc": "ARITH-CHECK-0002", "python": "3.9-compatible, standard library only", "script": "bench/body/arith_0002.py",
           "checks": checks, "mismatches": [c["id"] for c in checks if not c["ok"]], "all_ok": ok}
    if "--json" in sys.argv[1:]:
        with open(sys.argv[sys.argv.index("--json") + 1], "w", encoding="utf-8") as f:
            json.dump(doc, f, indent=1, sort_keys=True)
            f.write("\n")
    print("checks %d, mismatches %d: %s" % (len(checks), len(doc["mismatches"]), ", ".join(doc["mismatches"]) or "none"))
    sys.exit(0 if ok else 1)
