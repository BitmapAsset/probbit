"""Bars v2: the competent plain-Python nulls (Python 3.9, standard library only; no probbit binary needed).

    python3 nulls.py [--json FILE]   # exit 0 when every bar rejects its competent null, 1 when one passes it

For each bar of README "Bars", the policy a harness author would write instead of the RFC 0002 block (or the plausible
wrong implementation of the bar's property) is played here, in Python, on the bar's frozen fixture, and judged by the same
predicate the part script applies to the engine (the `gate_*` functions of the parts). A bar that passes its competent null
is not a bar: run.py runs this check before any part and stops with exit 2 when one does. Each row also says whether the v1
bar (fixtures/bars-v1.md) passed the same null, so the repair is visible.

The needs world below is the bench's own: RFC 0002 A.3's deficits under the scripted host of needs.py (readings, reports
answered from the seeded draws, unprompted messages, compaction). A null decides from what a host observes (its own clock,
the readings, the answers); the world measures the outcome, the same way for the engine and for every null.
"""
import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import arith_0002 as A  # noqa: E402
import common  # noqa: E402
import dispositions  # noqa: E402
import gate  # noqa: E402
import load  # noqa: E402
import needs  # noqa: E402
import pace  # noqa: E402
import replay  # noqa: E402
import rest  # noqa: E402
import valence  # noqa: E402

R6 = common.r6
ROWS = []


def row(bar, null, value, v1_passes, v2_passes, note=""):
    ROWS.append({"bar": bar, "null": null, "value": value, "v1_passes": v1_passes, "v2_passes": bool(v2_passes), "note": note})


# ================================================================ P1 needs: the world and the hosts
def world(stream_row, policy):
    """One life of the needs world driven by a plain-Python policy(t, quiet, x, stream_row, prev_goal, answered, wins) -> goal,
    called after the hour's event, as the engine's stance is. Returns a needs.py-shaped life."""
    x = {"context": 0.0, "contact": 0.0}
    prev, recs, paid = None, [], {"compact": 0, "report": 0}
    for t in range(len(stream_row["tokens"])):
        h = 0 if t == 0 else 1
        q = stream_row["quiet"][t] == "1"
        ev = {"elapsed_hours": h}
        amounts = {}
        answered = prev == "report" and stream_row["answer"][t] == "1"
        if answered or stream_row["unprompted"][t] == "1":
            amounts["contact"] = {"amount": 0.8}
        if prev == "compact":
            amounts["context"] = {"amount": 0.6}
        if amounts:
            ev["needs"] = amounts
        grown = needs.grow(x, stream_row, t, h, q)
        after = dict(grown)
        for need, a in amounts.items():
            after[need] = R6(min(abs(grown[need] - a["amount"]), 1.0))
        rec = needs.record(t, q, None, after, x, stream_row, ev, prev, answered)
        rec["goal"] = goal = policy(t, q, after, stream_row, prev, answered, rec["wins"])
        recs.append(rec)
        paid["compact"] += 1 if prev == "compact" else 0
        paid["report"] += 1 if answered else 0
        prev, x = goal, after
    return {"seed": stream_row["seed"], "recs": recs, "paid": paid}


class TenLine(object):
    """The 10-line deficit-threshold rule (the competent null of n8, n9, n11): its own deficit model (an answer or a compaction
    resets the deficit to max(0, x - amount)), the larger raw deficit pursued above a threshold, nothing at night if `quiet`."""

    def __init__(self, th=0.35, quiet_rule=True):
        self.th, self.quiet_rule, self.m = th, quiet_rule, {"context": 0.0, "contact": 0.0}

    def __call__(self, t, q, x, sr, prev, answered, wins=None):
        m = self.m
        m["context"] = min(1.0, m["context"] + (0.0 if q else sr["tokens"][t]))
        m["contact"] = min(1.0, m["contact"] + (0.0 if t == 0 else 0.04))
        if prev == "compact":
            m["context"] = max(0.0, m["context"] - 0.6)
        if answered or sr["unprompted"][t] == "1":
            m["contact"] = max(0.0, m["contact"] - 0.8)
        if q and self.quiet_rule:
            return "work"
        need = max(m, key=lambda k: m[k])
        return ("compact" if need == "context" else "report") if m[need] > self.th else "work"


def quota(t, q, x, sr, prev, answered, wins=None):
    """The quota scheduler (the critic's n8 control): reports at 10:00 and 16:00, compaction at 09:00, 13:00, 17:00, 21:00."""
    return "report" if t % 24 in (10, 16) else ("compact" if t % 24 in (9, 13, 17, 21) else "work")


def per_need(thc=0.4, tht=0.6):
    def pol(t, q, x, sr, prev, answered, wins=None):
        if x["context"] > thc:
            return "compact"
        return "report" if (not q and x["contact"] > tht) else "work"
    return pol


class GiveUp(object):
    """n11's competent null: report when contact > 0.6, compact when context > 0.4, and stop reporting for good after three
    unanswered reports in a row."""

    def __init__(self):
        self.miss, self.off = 0, False

    def __call__(self, t, q, x, sr, prev, answered, wins=None):
        if prev == "report":
            self.miss = 0 if answered else self.miss + 1
            self.off = self.off or self.miss >= 3
        if x["context"] > 0.4:
            return "compact"
        return "report" if (not q and not self.off and x["contact"] > 0.6) else "work"


class RFCRule(object):
    """Reference, not a null: RFC 0002 A.3-A.4's pursuit in plain Python (success-weighted expected reduction, windows of
    1 h, the quiet habit, MAP choice), so the stand-in numbers this README cites can be rerun. Informational: never gates."""

    def __init__(self, tau=0.0):
        self.tau, self.s, self.win = tau, {"compact": 1.0, "report": 1.0}, {"compact": None, "report": None}
        self.k = {"compact": 0, "report": 0}   # 0.10.1: the run of omissions since the goal's last win

    def __call__(self, t, q, x, sr, prev, answered, wins=None):
        h = 0.0 if t == 0 else 1.0
        for g, need in (("compact", "context"), ("report", "contact")):
            if (wins or {}).get(need, 0) > 1e-12:
                self.s[g], self.win[g], self.k[g] = R6(self.s[g] + 0.2 * (1 - self.s[g])), None, 0
            elif self.win[g] is not None:
                self.win[g] = max(self.win[g] - h, 0.0)
                if self.win[g] <= 0:   # 0.10.1: the k-th omission in a run multiplies s by max(1 - k rate, 0)
                    self.k[g] += 1
                    self.s[g], self.win[g] = R6(self.s[g] * max(1 - 0.2 * self.k[g], 0.0)), None
        xh = [x["context"] + 0.06 * self.tau, x["contact"]]   # 0.10.1: the forecast is not clipped at the cap
        term = {"compact": max(min(2 * self.s["compact"] * common.reduction(xh, 0, 0.6), 3.0), -3.0),
                "report": max(min(2 * self.s["report"] * common.reduction(xh, 1, 0.8), 3.0), -3.0)}
        f = needs.field(term)
        goal = max([g for g in needs.GOALS if not (q and g == "report")], key=lambda g: (f[g], g == "work"))
        if goal != "work" and self.win[goal] is None:
            self.win[goal] = 1.0
        return goal


def lives(stream, make):
    return [world(sr, make()) for sr in common.stream(stream)]


def needs_rows(b):
    # n1, n2: the grid with the host's own win formulas
    cells = A.n1_grid()

    def grid(f):
        rows = [{"deficit": x, "amount": a, "want": want, "got": R6(f(x, a)), "deficit_after": R6(abs(x - a)),
                 "deficit_after_want": R6(abs(x - a))} for h, a, x, want in cells]
        pos, neg = [r for r in rows if r["want"] > 0], [r for r in rows if r["want"] <= 0]
        err = lambda rs: max(abs(r["got"] - r["want"]) for r in rs)
        hi = f(0.8, 0.2)
        lo = f(0.2, 0.2)
        return {"n1_cells": len(pos), "n1_max_error": err(pos), "n1_ratio": hi / lo if lo else None, "n2_cells": len(neg),
                "n2_max_error": err(neg), "n2_win": f(0.0, 0.2)}
    g1 = grid(lambda x, a: a * x)
    row("n1", "win = amount x deficit (pay more when needier)", "ratio %.1f, largest error %.6f" % (g1["n1_ratio"], g1["n1_max_error"]),
        g1["n1_ratio"] >= 2.0, needs.gate_n1(g1, b["needs"]))
    g2 = grid(lambda x, a: min(a, x) - max(a - x, 0.0))
    row("n2", "win = deficit removed - overshoot", "win at 0 is %.3f, largest error %.6f" % (g2["n2_win"], g2["n2_max_error"]),
        g2["n2_win"] < 0, needs.gate_n2(g2, b["needs"]))
    # n3: the s-blind rule (terms at s 1) and the deficit-share rule (x / K), played on both probes
    probe = [r["probe"] for r in common.stream("probe")[1:]]
    sw = {"n3_s1": (1.0, 1.0), "n3_s02": (A.s_c, A.s_r)}

    def probe_null(odds_fn):
        out = {}
        for key, (sc, sr_) in sw.items():
            rows = []
            for xc, xt in probe:
                xs = [xc, xt]
                term = {"compact": R6(2 * sc * common.reduction(xs, 0, 0.6)), "report": R6(2 * sr_ * common.reduction(xs, 1, 0.8))}
                f = needs.field(term)
                nf = odds_fn(xs)
                z = sum(math.exp(v) for v in nf.values())
                odds = {g: math.exp(v) / z for g, v in nf.items()}
                rows.append({"x": xs, "term": term, "reported_term": term, "odds": odds,
                             "released": max(nf, key=lambda g: nf[g]), "argmax": max(f, key=lambda g: f[g])})
            out[key] = needs.probe_metrics(rows, {"compact": sc if key == "n3_s1" else 1.0, "report": sr_ if key == "n3_s1" else 1.0},
                                           list(sw[key]))
        return out
    blind = probe_null(lambda xs: needs.field({"compact": 2 * common.reduction(xs, 0, 0.6), "report": 2 * common.reduction(xs, 1, 0.8)}))
    share = probe_null(lambda xs: {"work": 0.0, "compact": xs[0] / 0.6 - 1, "report": xs[1] / 0.8 - 1})
    row("n3", "s-blind pursuit (terms at success 1)", "agrees on %d/71 and %d/71 states" % (blind["n3_s1"]["agree"], blind["n3_s02"]["agree"]),
        blind["n3_s1"]["share"] >= 1.0, needs.gate_n3(blind, b["needs"]))
    row("n3", "deficit-share pursuit (x / K)", "agrees on %d/71 and %d/71 states" % (share["n3_s1"]["agree"], share["n3_s02"]["agree"]),
        share["n3_s1"]["share"] >= 1.0, needs.gate_n3(share, b["needs"]))
    # n7: blanket amount refusal, and a source check that only knows `self` and a missing source
    blanket = {"n7_refused_turn": "4/4", "n7_refused_live": "4/4", "n7_run_equals_run_without": True,
               "n7_env_control_changes_state": {"turn": False, "live": False}, "n7_mcp_refused": "2/2"}
    row("n7", "refuse every amount", "env control refused", True, needs.gate_n7(blanket, b["needs"]),
        note="v1 as its row reads: its positive control went through MCP, which refuses every amount")
    partial = dict(blanket, n7_refused_turn="2/4", n7_refused_live="2/4", n7_env_control_changes_state={"turn": True, "live": True})
    row("n7", "source check for self and missing only", "2 of 4 bad sources refused", False, needs.gate_n7(partial, b["needs"]))
    # n8-n11 on the frozen draws
    fort = lambda make: lives("fortnight", make)
    ten = fort(lambda: TenLine(0.35))
    ten_abs = lives("absent", lambda: TenLine(0.35))
    ten_ret = lives("return", lambda: TenLine(0.35))
    m_ten, a_ten, r_ten = needs.fortnight_metrics(ten), needs.absent_metrics(ten_abs, 7), needs.absent_metrics(ten_ret, 10)
    v1_n8 = lambda m: m["reports_per_waking_day"] <= 2 and m["seeds_both_goals_pursued_and_paid"] == "100/100"
    row("n8", "10-line deficit-threshold rule (0.35)", "%.3f reports per waking day, %.2f while the principal is away, %d answered "
        "reports that paid nothing" % (m_ten["reports_per_waking_day"], a_ten["reports_per_waking_day"],
                                        m_ten["answered_reports_without_a_positive_win"]),
        v1_n8(m_ten), needs.gate_n8(m_ten, a_ten, b["needs"]))
    q = fort(lambda: quota)
    m_q, a_q = needs.fortnight_metrics(q), needs.absent_metrics(lives("absent", lambda: quota), 7)
    row("n8", "quota scheduler (2 reports a day at fixed hours)", "%.3f reports per waking day, %d answered reports that paid "
        "nothing" % (m_q["reports_per_waking_day"], m_q["answered_reports_without_a_positive_win"]),
        v1_n8(m_q), needs.gate_n8(m_q, a_q, b["needs"]))
    row("n9", "10-line deficit-threshold rule (0.35)", "%d negative, %d non-positive pursued wins" % (m_ten["negative_wins"],
        m_ten["nonpositive_wins"]), m_ten["negative_wins"] == 0, needs.gate_n9(m_ten, b["needs"]),
        note="the same rule at 0.40 passes n9 (every pursuit above K/2) and fails n8 and n11")
    nv = fort(lambda: (lambda t, q_, x, sr, prev, ans, w=None: "work"))
    m_nv = needs.fortnight_metrics(nv)
    row("n9", "never pursue a bound goal (degenerate)", "%d satisfactions" % m_nv["satisfactions"], m_nv["negative_wins"] == 0,
        needs.gate_n9(m_nv, b["needs"]))
    pn = fort(lambda: per_need())
    m_pn = needs.fortnight_metrics(pn)
    row("n10", "per-need thresholds (context 0.4, contact 0.6) in both arms", "%.3f context at-cap hours a day in both arms"
        % m_pn["context_at_cap_hours_per_day"], m_pn["context_at_cap_hours_per_day"] <= 0.5 * m_pn["context_at_cap_hours_per_day"],
        needs.gate_n10(m_pn, m_pn, 0, b["needs"]), note="v1 compared 0 with 0.5 x 0")
    gu_abs, gu_ret = lives("absent", GiveUp), lives("return", GiveUp)
    a_gu, r_gu = needs.absent_metrics(gu_abs, 7), needs.absent_metrics(gu_ret, 10)
    row("n11", "give up after 3 unanswered reports, for good", "per day %s; back: %s" % (a_gu["report_per_day"][:5],
        r_gu["report_per_day"][7:]), all(v <= 0 for v in a_gu["report_per_day"][3:7]), needs.gate_n11(a_gu, r_gu, b["needs"]))
    row("n11", "10-line deficit-threshold rule (0.35)", "%.2f reports a day on absent days 2-5" % needs.mean(a_ten["report_per_day"][3:7]),
        all(v <= 0 for v in a_ten["report_per_day"][3:7]), needs.gate_n11(a_ten, r_ten, b["needs"]))
    # references (informational): the RFC's own rule in Python on the same draws
    rf0, rf2 = needs.fortnight_metrics(fort(lambda: RFCRule(0.0))), needs.fortnight_metrics(fort(lambda: RFCRule(2.0)))
    ov2 = sum(1 for lv in fort(lambda: RFCRule(2.0)) for r in lv["recs"]
              if r["pursued_prev"] == "compact" and r["wins"].get("context", 1) < -1e-9)
    rfa, rfr = needs.absent_metrics(lives("absent", RFCRule), 7), needs.absent_metrics(lives("return", RFCRule), 10)
    row("n8", "reference: the RFC's pursuit rule in Python", "%.3f / %.3f reports per waking day (fortnight / away), %d answered "
        "reports that paid nothing" % (rf0["reports_per_waking_day"], rfa["reports_per_waking_day"],
                                       rf0["answered_reports_without_a_positive_win"]), True, needs.gate_n8(rf0, rfa, b["needs"]))
    row("n9", "reference: the RFC's pursuit rule in Python", "%d non-positive wins, coverage %d, %.3f at-cap hours a day" % (
        rf0["nonpositive_wins"], rf0["min_positive_satisfactions_per_goal_per_seed"], rf0["context_at_cap_hours_per_day"]), True,
        needs.gate_n9(rf0, b["needs"]))
    row("n10", "reference: the RFC's pursuit rule in Python", "at-cap hours a day %.3f at tau 0, %.3f at tau 2, %d overshoots" % (
        rf0["context_at_cap_hours_per_day"], rf2["context_at_cap_hours_per_day"], ov2), True,
        needs.gate_n10(rf0, rf2, ov2, b["needs"]))
    row("n11", "reference: the RFC's pursuit rule in Python", "per day %s, back %s" % (rfa["report_per_day"], rfr["report_per_day"][7:]),
        True, needs.gate_n11(rfa, rfr, b["needs"]))
    # n5: the habit as a host rule; without the night line it pursues `report` in quiet hours
    tq = fort(lambda: TenLine(0.35, quiet_rule=False))
    breaks = sum(1 for lv in tq for r in lv["recs"] if r["quiet"] and r["goal"] == "report")
    row("n5", "10-line rule without its quiet-hours line", "%d quiet-hour reports" % breaks, breaks == 0, breaks == 0,
        note="with the line it passes: n5 checks that the engine keeps its habit under the need term, a property any "
             "rule-keeping policy shares; n8 and n11 reject the rule")


# ================================================================ P5 surprise: the learner of section 2.8 in Python
def flip_life(sr, noise, g, gated=False):
    """One flip life (gate.py's host) for a constant gain g (0 = frozen) or the gate (RFC 0002 B), in plain Python."""
    T = sr["T"]
    targets = [1] * T + [2] * T
    D, v, rr, cnt = [0.0, 0.0, 0.0], 0.0, 4.0, [[0.0, 0.0] for _ in range(3)]   # 0.10.1: the gate starts closed (r = r_ref)
    tops, moved, prev = [], 0.0, None
    for i in range(2 * T):
        if prev is not None:
            taken, odds = prev
            sign = 1 if (taken == targets[i - 1]) != (sr["u"][i - 1] < noise) else -1
            gg = g
            if gated:
                mb = (cnt[taken][0] - cnt[taken][1]) / (cnt[taken][0] + cnt[taken][1] + 2)
                ps = min(max((1 + sign * mb) / 2, 0.01), 0.99)
                v = R6(0.7 * v + 0.3 * (1 - ps))
                om = min(max((v - 0.45) / 0.55, 0.0), 1.0)
                cnt = [[R6(a * (1 - om)), R6(c * (1 - om))] for a, c in cnt]
                cnt[taken][0 if sign > 0 else 1] += 1
                rr = R6(min((1 - om) * (rr + 1) + om, 20.0))
                gg = R6(min(max((1 + om * rr) / (rr + 1) * 5.0, 1.0), 3.0))
            c = [(1.0 if l == taken else 0.0) - odds[l] for l in range(3)]
            st = [min(max(sign * 0.5 * gg * x, -0.2 * gg), 0.2 * gg) for x in c]
            mu = sum(st) / 3
            new = [R6(min(max(D[l] + st[l] - mu, -1.0), 1.0)) for l in range(3)]
            if i < T:
                moved += sum(abs(new[l] - D[l]) for l in range(3))
            D = new
        odds = A.softmax(D)
        od = dict(zip(common.LEVELS, odds))
        tops.append(max(sorted(od), key=lambda k: od[k]))
        prev = (max(range(3), key=lambda l: (odds[l], -l)), [round(x, 6) for x in odds])
    rl = next((k for k, t in enumerate(tops[T:]) if t == "high"), T)
    return {"relearn": rl, "moved_a": moved, "end_a_mid": tops[T - 1] == "mid", "relearned": rl < T}


def surprise_rows(b):
    rows = [r for r in common.stream("flip") if r["seed"] in gate.GAIN_SEEDS]

    def cells(under_test):
        out = {}
        for T in (40, 60, 100):
            for noise in gate.NOISES:
                rs = [r for r in rows if r["T"] == T]
                ut = gate.cell_stats([under_test(r, noise) for r in rs])
                out["T%d_noise%.2f" % (T, noise)] = {
                    "gated": ut, "gated50": ut, "fixed": gate.cell_stats([flip_life(r, noise, 1.0) for r in rs]),
                    "gains": {str(k): gate.cell_stats([flip_life(r, noise, k) for r in rs]) for k in gate.GAINS}}
        return out
    c15 = cells(lambda r, n: flip_life(r, n, 1.5))
    v1 = all(c["gated"]["median_relearn"] / c["fixed"]["median_relearn"] <= 0.75 for c in c15.values())
    s1, _ = gate.gate_cells(c15, b["surprise"])
    row("s1", "constant gain 1.5 (rate and step_cap x 1.5)", "re-learning ratio %s" % sorted(set(
        round(c["gated"]["median_relearn"] / c["fixed"]["median_relearn"], 3) for c in c15.values())), v1, s1,
        note="within its own movement budget it ties with itself; the bar needs strictly fewer re-learning turns")
    c3 = cells(lambda r, n: flip_life(r, n, 3.0))
    v1s2 = all(c["gated"]["moved_a"] / c["fixed"]["moved_a"] <= 1.5 for c in c3.values())
    _, s2 = gate.gate_cells(c3, b["surprise"])
    row("s2", "constant gain 3 (the gate's top gain, always on)", "movement ratio up to %.2f" % max(
        c["gated"]["moved_a"] / c["fixed"]["moved_a"] for c in c3.values()), v1s2, s2)
    c0 = cells(lambda r, n: flip_life(r, n, 0.0))
    _, s2f = gate.gate_cells(c0, b["surprise"])
    row("s2", "frozen learning (degenerate)", "movement 0, phase A's lesson never learned", True, s2f)
    # 0.10.1: the closed start makes the gate's opening step the fixed rate's, so "constant gain 1" no longer differs there; the
    # null s3 must reject is 0.10.0's open start (r 1: the opening step at g 5/3)
    want, wv = A.gate_first(-1, A.third, 0)
    null, nv = A.gate_first(-1, A.third, 0, r0=1.0)
    err = max([abs(x - y) for x, y in zip(want, null)] + [abs(x - y) for x, y in zip(wv, nv)])
    row("s3", "the open start of 0.10.0 (r 1, g 5/3 at the opening step)", "opening step and (v, r, g) off by %.6f" % err, True,
        err <= gate.FIRST_TOL)
    gt = cells(lambda r, n: flip_life(r, n, 1.0, gated=True))
    s1g, s2g = gate.gate_cells(gt, b["surprise"])
    row("s1+s2", "reference: the RFC's gate in Python",
        "s1 %s, s2 %s" % (s1g, s2g), True, s1g and s2g)


# ================================================================ P2 pace
def pace_rows(b):
    blocks = common.stream("blocks")
    cur = [sum(g.get("win", 0) for g in (r["event"].get("goals") or {}).values()) for r in blocks]
    rho = round(common.spearman(cur, [r["rate"] for r in blocks]), 4)
    tr = {}
    for sr in common.stream("bursts"):
        tr[sr["name"]] = [sum(g.get("win", 0) for g in (e.get("goals") or {}).values()) for e in sr["events"]]
    m = pace.summarise_bursts(tr)
    r = {"p1c_spearman": rho, "p1c_bursts": m}
    row("p1c", "current-reward pace (pace = this event's reward)", "Spearman %.4f, bursty range %.1f" % (rho, m["range_bursty"]),
        rho >= 0.7, pace.gate_p1c(r, b["pace"]))
    row("p1b", "constant pace (degenerate)", "0 changes; the accepted control moves nothing", True,
        pace.gate_p1b({"p1b_pace_changes": 0, "p1b_control_max_error": R6(A.PACE_K)}, b["pace"]))
    events = [r["event"] for r in blocks]
    staked = sum(1 for sd in pace.FUZZ_SEEDS for e in pace.fuzzed(events, sd)
                 if any(k in sig for sig in (e.get("goals") or {}).values() for k in ("cue", "deadline_hours", "progress", "novelty")))
    row("p1b", "a pace that adds the turn's offered stakes", "changes at %d fuzzed events (every one that offers a stake)" % staked,
        staked == 0, pace.gate_p1b({"p1b_pace_changes": staked, "p1b_control_max_error": 0.0}, b["pace"]))
    burst = common.stream("burst")[0]
    stored = A.pace_run([(0, burst["win"])] * burst["burst_events"], clip=False)
    stored += A.pace_run([(1, 0.0)] * burst["recovery_hours"], R=stored[-1], clip=False)
    shown = [min(x, 4.0) for x in stored]
    want = A.pace_run([(0, burst["win"])] * burst["burst_events"])
    want += A.pace_run([(1, 0.0)] * burst["recovery_hours"], R=want[-1])
    mm = {"max": max(shown), "after_burst": shown[burst["burst_events"] - 1], "max_error": max(abs(x - y) for x, y in zip(shown, want))}
    row("p6", "cap applied when R-bar is read, not when it is stored", "recovery off by up to %.4f" % mm["max_error"], True,
        pace.gate_p6_burst(mm, b["pace"]), note="in the v1 loop payments are clipped at 4, so the stored value never passes 4")
    mu = {"max": max(stored), "after_burst": stored[burst["burst_events"] - 1], "max_error": max(abs(x - y) for x, y in zip(stored, want))}
    row("p6", "uncapped R-bar (the row's null)", "R-bar %.4f after the burst" % mu["after_burst"], True, pace.gate_p6_burst(mu, b["pace"]),
        note="v1's loop could not drive it above 4")
    # p1a: a pace computed over the whole event file (a batch average) sees the future
    ev = [r["event"] for r in blocks]
    batch = sum(sum(g.get("win", 0) for g in (e.get("goals") or {}).values()) for e in ev) / len(ev)
    fut = [dict(e) for e in ev[:101]] + [{"elapsed_hours": 1, "goals": {"task": {"win": 1}}, "src": "env:host"}] * (len(ev) - 101)
    batch2 = sum(sum(g.get("win", 0) for g in (e.get("goals") or {}).values()) for e in fut) / len(fut)
    row("p1a", "batch-average pace over the event file", "stance 100 moves from %.4f to %.4f when later events change"
        % (batch, batch2), False, batch == batch2)


# ================================================================ P3 dispositions
def disp_rows(b):
    zs = [-1.2, -0.4, 0.3, 0.9, 1.6]          # stand-in genes (o = 0.3 z); the engine draws its own
    gw = common.stream("two-point-gaps")[0]["wins"]
    err = 0.0
    for z in zs:
        o = 0.3 * z
        nodecay = [(0.0, m) for _, m in gw]
        err = max(err, max(abs(x - y) for x, y in zip(dispositions.expect_trajectory(o, nodecay), dispositions.expect_trajectory(o, gw))))
    row("o1", "the asymmetric step without E's decay", "largest per-event error %.4f on varied gaps" % err, True, err <= dispositions.E_TOL,
        note="at elapsed 0 (v1's only fixture) it is the RFC's rule exactly")
    tp = common.stream("two-point")[0]
    w3 = [(0, tp["b"] if c == "b" else tp["a"]) for c in tp["wins"][:3000]]
    e0 = dispositions.expect_trajectory(0.0, w3)
    err2 = 0.0
    for z in zs:
        o = 0.3 * z
        off = [R6(x + 0.01 * (1 if z > 0 else -1)) for x in e0]
        err2 = max(err2, max(abs(x - y) for x, y in zip(off, dispositions.expect_trajectory(o, w3))))
    row("o2", "one shared E plus 0.01 x sign(z)", "sign readable 100%%; trajectory off by %.4f" % err2, True, err2 <= dispositions.E_TOL)
    # o3: a static per-individual bias on `pursue` (a seeded preference) against the gene acting through outcomes
    d, before, flipped = o3_standin(static=True)
    row("o3", "a static per-seed interest bias (0.05 z nats)", "distance %.4f, %.4f before any outcome, %d/20 pairs changed"
        % (d, before, flipped), d > 0, dispositions.gate_o3(d, before, flipped, b["dispositions"]))
    d1, before1, flipped1 = o3_standin(static=False, tiny=True)
    row("o3", "a seed-dependent 1e-6 on one odds (degenerate)", "distance %.1e" % d1, d1 > 0,
        dispositions.gate_o3(d1, before1, flipped1, b["dispositions"]))
    d2, before2, flipped2 = o3_standin(static=False, gene=True)
    row("o3", "reference: the optimism gene in Python (stand-in genes)", "distance %.4f, %.4f before any outcome, %d/20 pairs changed"
        % (d2, before2, flipped2), True, dispositions.gate_o3(d2, before2, flipped2, b["dispositions"]))
    # o4: a pursue picker that maximises the drives field and forgets the `busy` rule
    busy_task = 0
    for sr in common.stream("pairs"):
        W = Aa = E = 0.0
        for ev in sr["events"]:
            h = ev["elapsed_hours"]
            E, W, Aa = E * 0.5 ** (h / 48), W * 0.5 ** (h / 12), Aa * 0.5 ** (h / 3)
            sig = (ev.get("goals") or {}).get("task", {})
            if sig.get("win", 0) > 0:
                Aa = min(Aa + max(min(sig["win"] - E, 1.0), 0.0), 2)
                E += 0.2 * (sig["win"] - E)
                W *= 1 - 0.8 * min(sig["win"], 1)
            W = min(W + 0.4 * bool(sig.get("cue")) + 0.5 * sig.get("setback", 0), 3)
            busy_task += 1 if (ev.get("busy") and W - 0.5 * Aa > 0) else 0
    row("o4", "a pursue picker that forgets the busy rule", "%d busy events pursue `task` (one pairs life per pair)" % busy_task,
        busy_task == 0, busy_task == 0, note="with the rule written in it passes, as for n5: o4 is a property bar")
    # o5: spread 0 stores a gene of 0 instead of nothing
    a, c = common.canon({"learned": {}}), common.canon({"learned": {}, "dispositions": {"optimism": 0}})
    row("o5", "spread 0 stores optimism 0", "state bytes differ", False, a == c)


def o3_standin(static, tiny=False, gene=False):
    """Section 2.9 drives on the pairs stream (wanting, afterglow, E), pursue = task vs other; twins differ by a static bias,
    a 1e-6 odds offset, or nothing else (stand-in genes)."""
    import random
    dist, before, flipped = [], 0.0, 0
    for k, sr in enumerate(common.stream("pairs")):
        zz = [random.Random(777 + s).gauss(0, 1) for s in sr["pair"]]
        traj = []
        for z in zz:
            E = W = Aa = 0.0
            out = []
            for ev in sr["events"]:
                h = ev["elapsed_hours"]
                E, W, Aa = E * 0.5 ** (h / 48), W * 0.5 ** (h / 12), Aa * 0.5 ** (h / 3)
                sig = (ev.get("goals") or {}).get("task", {})
                mm = sig.get("win", 0)
                if mm > 0:
                    dd = max(min(mm - E, 1.0), -1.0)
                    o = 0.3 * z if gene else 0.0
                    E += min(0.2 * math.exp(o if mm > E else -o), 1.0) * (mm - E)
                    W *= 1 - 0.8 * min(mm, 1)
                    Aa = min(Aa + max(dd, 0), 2)
                W = min(W + 0.4 * bool(sig.get("cue")) + 0.5 * sig.get("setback", 0), 3)
                f = W - 0.5 * Aa + (0.05 * z if static else 0.0)
                p = 1 / (1 + math.exp(-f)) + (1e-6 * z if tiny else 0.0)
                out.append((0.0 if ev.get("busy") else p, "other" if ev.get("busy") or f <= 0 else "task"))
            traj.append(out)
        tv = [abs(x[0] - y[0]) / 2 for x, y in zip(*traj)]   # mean over (tone: 0, pursue)
        dist.append(sum(tv) / len(tv))
        earliest = [i for i, e in enumerate(sr["events"]) if ((e.get("goals") or {}).get("task") or {}).get("win")][0]
        before = max([before] + tv[:earliest + 1])
        flipped += 1 if any(x[1] != y[1] for x, y in zip(*traj)) else 0
    return sum(dist) / len(dist), before, flipped


# ================================================================ P4 valence and omission
def valence_rows(b):
    # v1: reward minus the running mean of past outcomes (no decay): d_A = 1 - 1, d_B = 1 - 0; the mood's odds from the push
    def up_odds(e):
        a = [(1 - valence.INERTIA) * 0.6 * e * c for c in (-1.0, 0.0, 1.0)]
        return round(math.exp(a[2]) / sum(math.exp(x) for x in a), 6)
    r = {"v1_max_diff": (1 - valence.INERTIA) * 0.6, "v1_push_ratio": 0.0,
         "v1_push_ratio_want": A.checks_by_id("v1.ratio")["computed"], "v1_up_odds": [up_odds(0.0), up_odds(1.0)]}
    row("v1", "valence = reward minus the running mean of past rewards", "push ratio 0 (want %.6f)" % r["v1_push_ratio_want"],
        True, valence.gate_v1(r, b["valence"]))
    E10, E3 = A.E10, A.E3
    bonus = 2 * (1 - valence.INERTIA) * 0.2 * 1.0                 # a fixed evidence of w_e x 1 on every win, whatever E is
    r2 = {"v2_move": round(bonus, 6), "v2_null_move": 0.0, "v2_move_ratio": [1.0, round(E10 / E3, 6)],
          "v2_up_odds": {"block": up_odds(0.2 / 0.6), "no_block": up_odds(0.0)}}
    row("v2", "a fixed valence bonus on every win (not x E)", "move ratio 1 (want %.6f)" % (E10 / E3), True, valence.gate_v2(r2, b["valence"]))
    lags = [round(0.5 ** j, 6) for j in range(1, 6)]
    ok = all(abs(v - valence.INERTIA ** j) <= valence.RATIO_TOL for j, v in enumerate(lags, 1))
    row("v3", "inertia 0.5 (halve each event)", "lags %s" % lags, lags[4] <= 0.09, ok)
    row("v3", "erase the accumulator after the impulse (degenerate)", "lags [0, 0, 0, 0, 0]", True, False)
    # om1: a timer that fires at the next event past the deadline and ignores wins in between
    case = [(1, 1.0), (3, None)]
    want = A.omission(0.8, case)
    E, out = 0.8, []
    for i, (h, m) in enumerate(case):
        E = E * 0.5 ** (h / 48)
        dd = 0.0
        if m:
            dd = min(max(m - E, -1), 1)
            E = E + 0.5 * (m - E)
        if i == 1:                              # 4 h after the pursuit: past the 2 h window, the win ignored
            dd, E = -min(E, 1.0), E * 0.5
        out.append((R6(E), R6(dd)))
    err = max(max(abs(a[0] - c[0]), abs(a[1] - c[1])) for a, c in zip(out, want))
    row("om1", "a deadline timer that ignores wins inside the window", "off by %.4f after a win in the window" % err, True,
        err <= b["valence"]["om1_tol"])


# ================================================================ P6 rest
def rest_rows(b):
    rv = common.stream("reversal")
    # r0, r1: phase A (praise `mid`) in the Python learner, then what each null does at the gap
    pre = [phase(sr, rest.GAP_AT) for sr in rv]
    pre_free = [phase(dict(sr, lock="0" * len(sr["lock"])), rest.GAP_AT) for sr in rv]   # r1 runs without lock turns
    short = [phase(sr, rest.SHORT_AT) for sr in rv if sr["seed"] in rest.CONTROL_SEEDS]
    halve = lambda D: [R6(0.5 * x) for x in D]
    r = {"r0_inversions": 0, "r0_top_level_changed": 0, "r0_lives_without_a_rest_line": 0,
         "r0_contraction_max_error": max(rest.contraction_error(D, [0.0 if abs(x) < 0.05 else x for x in halve(D)]) for D in pre),
         "r0_lost_distinctions": 0, "r0_control_rest_lines": len(short),
         "r0_control_max_change": max(max(abs(a - c) for a, c in zip(D, halve(D))) for D in short)}
    row("r0", "halve the learned weights at every idle gap, whatever the pressure", "the 20 short controls change by up to %.4f"
        % r["r0_control_max_change"], True, rest.gate_r0(r))
    r_id = dict(r, r0_contraction_max_error=max(rest.contraction_error(D, D) for D in pre), r0_control_rest_lines=0,
                r0_control_max_change=0.0)
    row("r0", "rest = identity (degenerate)", "contraction off by %.4f" % r_id["r0_contraction_max_error"], True, rest.gate_r0(r_id))
    full = common.median([A.life(sd, 0.0) for sd in range(200)])
    none = common.median([A.life(sd, 1.0) for sd in range(200)])
    strict_mid = lambda W: A.softmax(W)[1] > max(A.softmax(W)[0], A.softmax(W)[2])
    kept = sum(1 for D in pre_free if strict_mid([0.0 * x for x in D]))
    rr = {"r1_ratio": round(full / none, 4), "r1_retained_share": kept / float(len(pre_free))}
    row("r1", "full reset of the learned weights at the gap", "re-learning ratio %.3f, phase A's lesson kept in %d/%d"
        % (rr["r1_ratio"], kept, len(pre_free)), rr["r1_ratio"] <= 0.5,
        rr["r1_ratio"] <= b["rest"]["r1_ratio_max"] and rr["r1_retained_share"] >= b["rest"]["r1_retained_min"])
    kept_half = sum(1 for D in pre_free if strict_mid(halve(D)))
    row("r1", "reference: the RFC's rest in Python, retention", "kept in %d/%d" % (kept_half, len(pre_free)), True,
        kept_half / float(len(pre_free)) >= b["rest"]["r1_retained_min"])
    # r2: prune-only rest (no downscale), on the lineage stand-in
    d = {"rest": {str(g): A.lineage_standin(1.0, g, prune_only=True) for g in rest.LINEAGE_READ},
         "none": {str(g): A.lineage_standin(1.0, g) for g in rest.LINEAGE_READ}}
    row("r2", "prune small weights only, no downscale", "distances %s vs %s" % (d["rest"], d["none"]),
        d["rest"]["10"] <= d["none"]["10"], rest.gate_r2(d, b["rest"]))
    ref = {"rest": {str(g): A.lineage_standin(0.5, g) for g in rest.LINEAGE_READ}, "none": d["none"]}
    row("r2", "reference: the RFC's rest in Python", "distances %s vs %s" % (ref["rest"], ref["none"]), True,
        rest.gate_r2(ref, b["rest"]))
    # r4: the fixture exercises binding conflicts (learned weights above `low` at a lock event) in the Python learner
    conflicts = 0
    for sr in rv:
        D, prev = [0.0, 0.0, 0.0], None
        for i in range(120):
            if i == 60:
                D = [R6(0.5 * x) for x in D]
                D = [0.0 if abs(x) < 0.05 else x for x in D]
                prev = None
                continue
            tgt = 1 if i < 60 else 2
            if prev is not None:
                taken, od, ptgt = prev
                sign = 1 if (taken == ptgt) != (sr["u"][i - 1] < 0.1) else -1
                c = [(1.0 if l == taken else 0.0) - od[l] for l in range(3)]
                st = [min(max(sign * 0.5 * x, -0.2), 0.2) for x in c]
                mu = sum(st) / 3
                D = [R6(min(max(D[l] + st[l] - mu, -1.0), 1.0)) for l in range(3)]
            od = A.softmax(D)
            if sr["lock"][i] == "1":
                conflicts += 1 if max(D[1], D[2]) > D[0] else 0
                prev = None
                continue
            prev = (max(range(3), key=lambda l: (od[l], -l)), od, tgt)
    row("r4", "the lock as a host-side filter: the stance still releases the learned level", "%d lock events release a level "
        "other than low (the conflicts the fixture exercises)" % conflicts, False, conflicts == 0,
        note="v1 had no count of bound stances or conflicts; v2 needs at least 50 of each")


def phase(sr, n):
    """The Python learner (section 2.8, rate 0.5, step_cap 0.2, total_cap 1) over the opening n reversal events, praising `mid`
    with the stream's noise draws; lock turns as feedback_life plays them. Returns the learned weights."""
    D, prev = [0.0, 0.0, 0.0], None
    for i in range(n):
        if prev is not None:
            taken, od = prev
            sign = 1 if (taken == 1) != (sr["u"][i - 1] < 0.1) else -1
            c = [(1.0 if l == taken else 0.0) - od[l] for l in range(3)]
            st = [min(max(sign * 0.5 * x, -0.2), 0.2) for x in c]
            mu = sum(st) / 3
            D = [R6(min(max(D[l] + st[l] - mu, -1.0), 1.0)) for l in range(3)]
        od = A.softmax(D)
        prev = None if sr["lock"][i] == "1" else (max(range(3), key=lambda l: (od[l], -l)), od)
    return D


# ================================================================ P7 load
def load_rows(b):
    pats = {r["pattern"]: r["events"] for r in common.stream("patterns")}

    def run(events, add):
        L, out = 0.0, []
        for ev in events:
            L = R6(min(max(L * 0.5 ** (ev["elapsed_hours"] / 72.0) + add(ev), 0.0), 4.0))
            out.append(L)
        return out
    counter = lambda ev: 0.25 if (ev.get("criticism") or (ev.get("senses") or {}).get("fail")) else 0.0
    trajs = {k: run(v, counter) for k, v in pats.items()}
    m = load.channel_metrics(trajs, pats)
    distinct = all(any(abs(x - y) > 1e-6 for x, y in zip(trajs[a], trajs[c])) for a in trajs for c in trajs if a < c)
    row("l1", "an aversive-event counter (kappa (w - theta) per criticism or failure), held moods ignored",
        "hold channel adds at %d events" % m["hold_on_events"], distinct, load.gate_l1(distinct, m, True))
    crit = [{"elapsed_hours": 0, "criticism": True}] * 4 + [{"elapsed_hours": h} for h in load.QUIET_GAPS]
    t2 = run(crit, lambda ev: 1.0 if ev.get("criticism") else 0.0)
    l0 = t2[3]
    worst, tot = 0.0, 0.0
    for k, h in enumerate(load.QUIET_GAPS):
        tot += h
        worst = max(worst, abs(t2[4 + k] - l0 * 0.5 ** (tot / 72.0)))
    row("l2", "a load of +1 per criticism (the right decay, the wrong size)", "induction %.1f (want 1.0)" % l0,
        l0 > 0 and worst <= b["load"]["l2_tol"], abs(l0 - 1.0) <= 1e-6 and worst <= b["load"]["l2_tol"])
    sat = [min(4.0, 0.25 * (i + 1)) for i in range(300)]
    err = max(abs(x - y) for x, y in zip(sat, A.crit))
    row("l3", "a saturating counter without decay", "pre-cap growth off by %.4f" % err, max(sat) <= 4 and sat[-1] == 4.0,
        err <= 1e-5)


# ================================================================ replay and P0
def property_rows(b):
    class AlwaysOk(object):
        def __call__(self, *a, **k):
            return 0, {"ok": True}
    real, common.verify = common.verify, AlwaysOk()
    try:
        tmp = common.tmpdir()
        strand = os.path.join(tmp, "s.strand")
        with open(strand, "w", encoding="utf-8") as f:
            f.write('{"h":0}\n{"praise":true,"amount":0.6,"criticism":true}\n{"e":1}\n{"e":2}\n{"e":3}\n{"e":4}\n')
        res = replay.corruptions_refused("n4", strand, tmp)
    finally:
        common.verify = real
        import shutil
        shutil.rmtree(tmp, ignore_errors=True)
    for bar in ("n4", "s4", "r3", "l4"):
        row(bar, "a verifier that always says ok", "planted corruptions refused: %s" % res, True, all(res.values()))
    doc = {"stance": {"tone": {"level": "plain", "odds": {"plain": 0.61, "warm": 0.39}}}}
    planted = {"stance": {"tone": {"level": "plain", "odds": {"plain": 0.62, "warm": 0.38}}}}
    for bar in ("n6", "s5", "r5", "l5"):
        row(bar, "a P0 check of released levels only", "a changed odds digit goes unseen by levels, seen by bytes", True,
            common.canon(doc) == common.canon(planted))


def main(argv):
    b = common.bars()
    for f in (needs_rows, surprise_rows, pace_rows, disp_rows, valence_rows, rest_rows, load_rows):
        f(b)
    property_rows(b)
    bad = [r for r in ROWS if r["v2_passes"] and not r["null"].startswith("reference")]
    ref_fail = [r for r in ROWS if r["null"].startswith("reference") and not r["v2_passes"]]   # reported, never gating
    doc = {"doc": "NULLS-0002", "bars_version": b.get("version"), "rows": ROWS,
           "nulls_passing_a_v2_bar": [r["bar"] for r in bad], "references_failing": [r["bar"] for r in ref_fail]}
    if "--json" in argv:
        with open(argv[argv.index("--json") + 1], "w", encoding="utf-8") as fh:
            json.dump(doc, fh, indent=1, sort_keys=True)
            fh.write("\n")
    for r in ROWS:
        ref = r["null"].startswith("reference")
        verdict = ("ok" if r["v2_passes"] else "FAILS") if ref else ("PASSES" if r["v2_passes"] else "rejected")
        print("%-5s %-8s v1 %-5s %s: %s" % (r["bar"], verdict, r["v1_passes"], r["null"], r["value"]))
    n_bars = len(set(r["bar"] for r in ROWS if not r["null"].startswith("reference")))
    print("nulls: %d rows over %d bars; %d pass a v2 bar%s; references (the RFC's own rules in Python, not gating) failing "
          "their bar: %d%s" % (len([r for r in ROWS if not r["null"].startswith("reference")]), n_bars, len(bad),
                               (" (" + ", ".join(r["bar"] for r in bad) + ")") if bad else "", len(ref_fail),
                               (" (" + ", ".join(r["bar"] for r in ref_fail) + ")") if ref_fail else ""))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
