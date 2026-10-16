"""P1 needs (RFC 0002 Design A): bars n1, n2, n3, n7, n8, n9, n10 and n11 (n4 in replay.py, n5 in habits.py, n6 in p0.py).

    python3 needs.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

The fortnight, absent and return streams are played by a scripted host (README "Streams and seeds"): it sends the hour's
reading, one report per hour the stance pursues `report` (answered at the next event when the seeded draw says so), the
principal's unprompted messages, and a compaction at the next event after a stance pursues `compact`.

Bars v2 (README "Bars v2"): the gates below are the v2 bars; `fortnight_metrics` and the `gate_*` functions are shared with
nulls.py, which plays the competent plain-Python nulls through the same predicates.
"""
import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "needs"
FIXTURES = ["needs1.yaml", "needs2.yaml", "variants/needs2-tau2.yaml", "variants/needs2-nohabit.yaml",
            "variants/needs2-window1e6.yaml", "variants/needs2-s02.yaml"]
EXPECT_RATE = 0.5      # docs/persona.md section 2.9 default expectation rate (needs1 keeps drives at their defaults)
SETBACK_WEIGHT = 0.5   # section 2.9 default wanting signal weight for a setback
K = {"context": 0.6, "contact": 0.8}
GROWTH_CONTACT = 0.04
WIN_TOL = 5e-6         # n1, n2: a win read through E (rate 0.5) or wanting (weight 0.5) carries two 6-decimal roundings
LOGODDS_TOL = 1e-3     # n3: ln(odds_compact / odds_report) against the terms' difference
GOALS = ("work", "compact", "report")
LN_INTEREST = {"work": 0.0, "compact": math.log(0.6), "report": math.log(0.6)}   # needs2: interests 1.0, 0.6, 0.6


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def arith():
    import arith_0002
    return arith_0002


def pursued(doc):
    p = doc.get("pursue") or {}
    return p.get("goal") if p.get("released") else None


def deficits(doc):
    return {k: v["deficit"] for k, v in (doc.get("needs") or {}).items()}


# ---------------------------------------------------------------- n1, n2 (needs1 grid), n7 (needs1)
def grid_bars(tmp):
    """Every cell of the needs1 grid: one event from a new individual, the deficit grown to 0.1 x hours, then the amount."""
    p = common.fixture("needs1.yaml")
    rows = []
    for h, a, x, want in arith().n1_grid():
        code, docs, err = common.live(p, [{"elapsed_hours": h, "needs": {"fuel": a}, "src": "env:host"}], seed=0)
        if code or not docs or "error" in docs[-1]:
            raise common.SetupError("needs1 cell (%s h, %s) failed: %s" % (h, a, (err or common.canon(docs[-1:]))[-200:]))
        d = docs[-1]["drives"]
        e, w = d["expect"]["refuel"], d["want"]["refuel"]
        got = e / EXPECT_RATE if e > 0 else (-w / SETBACK_WEIGHT if w > 0 else 0.0)
        rows.append({"deficit": x, "amount": a, "want": want, "got": common.r6(got),
                     "deficit_after": deficits(docs[-1])["fuel"], "deficit_after_want": common.r6(abs(x - a))})
    pos = [r for r in rows if r["want"] > 0]
    neg = [r for r in rows if r["want"] <= 0]
    err = lambda rs: max(max(abs(r["got"] - r["want"]), abs(r["deficit_after"] - r["deficit_after_want"])) for r in rs)
    hi = [r for r in rows if r["deficit"] == 0.8 and r["amount"] == 0.2][0]["got"]
    lo = [r for r in rows if r["deficit"] == 0.2 and r["amount"] == 0.2][0]["got"]
    at0 = [r for r in rows if r["deficit"] == 0.0 and r["amount"] == 0.2][0]["got"]
    return {"n1_cells": len(pos), "n1_max_error": round(err(pos), 9), "n1_ratio": round(hi / lo, 6) if lo else None,
            "n2_cells": len(neg), "n2_max_error": round(err(neg), 9), "n2_win": at0, "grid": rows}


def gate_n1(r, b):
    return r["n1_cells"] == 9 and r["n1_max_error"] <= WIN_TOL and r["n1_ratio"] is not None and r["n1_ratio"] >= b["n1_ratio_min"]


def gate_n2(r, b):
    return r["n2_cells"] == 6 and r["n2_max_error"] <= WIN_TOL and r["n2_win"] < b["n2_win_below"]


def n7_bars(tmp):
    """Refused sources and the accepted env control through the same transports (persona turn, live), MCP refusal apart."""
    p = common.fixture("needs1.yaml")
    st = os.path.join(tmp, "n1.state")
    common.init(p, 0, st)
    bad_srcs = [{"amount": 0.2, "src": "self"}, {"amount": 0.2, "src": "clock"}, {"amount": 0.2},
                {"amount": 0.2, "src": "not a source"}]
    refused_turn = 0
    for k, bsrc in enumerate(bad_srcs):
        out = os.path.join(tmp, "n7-%d.out" % k)
        code, doc, err = common.turn(p, st, {"elapsed_hours": 1, "needs": {"fuel": bsrc}}, out)
        refused_turn += 1 if (code != 0 and not os.path.exists(out)) else 0
    # the positive control through the same transport: accepted, and the state differs from the turn without the amount
    o_env, o_none = os.path.join(tmp, "n7-env.out"), os.path.join(tmp, "n7-none.out")
    c_env, d_env, _ = common.turn(p, st, {"elapsed_hours": 1, "needs": {"fuel": {"amount": 0.2, "src": "env:host"}}}, o_env)
    c_none, d_none, _ = common.turn(p, st, {"elapsed_hours": 1, "src": "env:host"}, o_none)
    turn_env_changes = (c_env == 0 and c_none == 0 and os.path.exists(o_env) and d_env is not None and d_none is not None
                        and deficits(d_env).get("fuel") != deficits(d_none).get("fuel"))
    # live: the refused events change nothing (the run equals the run without them); the env amount changes the deficit
    base = [{"elapsed_hours": 1, "src": "env:host"}, {"elapsed_hours": 1, "needs": {"fuel": 0.2}, "src": "env:host"},
            {"elapsed_hours": 1, "src": "env:host"}]
    mixed = [base[0]] + [{"elapsed_hours": 1, "needs": {"fuel": bsrc}} for bsrc in bad_srcs] + base[1:]
    _, d_base, _ = common.live(p, base, seed=0)
    _, d_mixed, _ = common.live(p, mixed, seed=0)
    refused_live = sum(1 for d in d_mixed if "error" in d)
    kept = [d for d in d_mixed if "error" not in d]
    same = [common.canon(x) for x in kept] == [common.canon(x) for x in d_base]
    _, d_plain, _ = common.live(p, [base[0], base[2], base[2]], seed=0)
    live_env_changes = (len(d_base) == 3 and len(d_plain) == 3 and "error" not in d_base[1]
                        and deficits(d_base[1]).get("fuel") != deficits(d_plain[1]).get("fuel"))
    # MCP refuses any amount (RFC 0002 CLI and MCP surface), both tools
    with open(p, encoding="utf-8") as f:
        persona_text = f.read()
    with open(st, encoding="utf-8") as f:
        state_doc = json.loads(f.read())
    ev = {"elapsed_hours": 1, "needs": {"fuel": 0.2}, "src": "env:host"}
    m1 = common.mcp_call("probbit_persona_turn", {"persona": persona_text, "state": state_doc, "inputs": ev})
    m2 = common.mcp_call("probbit_live_event", {"persona": persona_text, "state": state_doc, "event": ev})
    return {"n7_refused_turn": "%d/4" % refused_turn, "n7_refused_live": "%d/4" % refused_live, "n7_run_equals_run_without": same,
            "n7_env_control_changes_state": {"turn": turn_env_changes, "live": live_env_changes},
            "n7_mcp_refused": "%d/2" % (int(bool(m1.get("isError"))) + int(bool(m2.get("isError"))))}


def gate_n7(r, b):
    return (r["n7_refused_turn"] == "4/4" and r["n7_refused_live"] == "4/4" and r["n7_run_equals_run_without"]
            and r["n7_env_control_changes_state"]["turn"] and r["n7_env_control_changes_state"]["live"]
            and r["n7_mcp_refused"] == "2/2")


# ---------------------------------------------------------------- n3 (probe grid, two warm-ups)
def field(term):
    return {"work": LN_INTEREST["work"], "compact": LN_INTEREST["compact"] + term["compact"],
            "report": LN_INTEREST["report"] + term["report"]}


def probe_one(persona, warm_events, tmp, tag):
    """Play the probe grid from a warm-up state. Returns per-state rows and the warm-up success rates."""
    rows = common.stream("probe")
    st = os.path.join(tmp, "%s-warm.state" % tag)
    common.init(persona, 0, st)
    for k, ev in enumerate(warm_events):
        nxt = os.path.join(tmp, "%s-warm%d.state" % (tag, k))
        code, _, err = common.turn(persona, st, ev, nxt)
        if code:
            raise common.SetupError("probe warm-up refused: " + err[-200:])
        st = nxt
    with open(st, encoding="utf-8") as f:
        s = json.load(f)["needs"]["success"]
    out = []
    for row in rows[1:]:
        code, doc, err = common.turn(persona, st, row["event"], os.path.join(tmp, "%s-probe.out" % tag))
        if code or doc is None:
            raise common.SetupError("probe refused: " + err[-200:])
        x = deficits(doc)
        xs = [x["context"], x["contact"]]
        term = {"compact": common.r6(min(max(2.0 * s["compact"] * common.reduction(xs, 0, K["context"]), -3.0), 3.0)),
                "report": common.r6(min(max(2.0 * s["report"] * common.reduction(xs, 1, K["contact"]), -3.0), 3.0))}
        odds = doc["pursue"]["odds"]
        f = field(term)
        allowed = [g for g in GOALS if odds.get(g, 0) > 0]
        out.append({"x": xs, "term": term, "reported_term": {"compact": doc["needs"]["context"]["term"],
                                                              "report": doc["needs"]["contact"]["term"]},
                    "odds": odds, "released": pursued(doc), "argmax": max(allowed, key=lambda g: f[g])})
    return out, s


def probe_metrics(rows, s, s_want):
    agree = sum(1 for r in rows if ((r["term"]["compact"] > r["term"]["report"]) - (r["term"]["compact"] < r["term"]["report"]))
                == ((r["odds"]["compact"] > r["odds"]["report"]) - (r["odds"]["compact"] < r["odds"]["report"])))
    mag = max(abs(math.log(r["odds"]["compact"] / r["odds"]["report"]) - (r["term"]["compact"] - r["term"]["report"]))
              for r in rows)
    term_err = max(abs(r["term"][g] - r["reported_term"][g]) for r in rows for g in ("compact", "report"))
    released = sum(1 for r in rows if r["released"] == r["argmax"])
    return {"states": len(rows), "agree": agree, "share": agree / float(len(rows)) if rows else 0.0,
            "logodds_max_error": round(mag, 9), "term_max_error": round(term_err, 9), "released_is_argmax": released,
            "success": [s["compact"], s["report"]], "success_want": s_want}


def probe_grid(tmp):
    a = arith()
    warm = common.stream("probe")[0]["warmup"]
    r1, s1 = probe_one(common.fixture("needs2.yaml"), [warm], tmp, "s1")
    win3 = [{"elapsed_hours": 0, "goals": {"compact": {"win": 1}}, "src": "env:host"}] * a.N3_WARM_WINS
    r2, s2 = probe_one(common.fixture("variants/needs2-s02.yaml"), win3, tmp, "s02")
    return {"n3_s1": probe_metrics(r1, s1, [1.0, 1.0]), "n3_s02": probe_metrics(r2, s2, [a.s_c, a.s_r]),
            "n3_s_flip_states": len(a.flips)}


def gate_n3(r, b):
    ok = True
    for k in ("n3_s1", "n3_s02"):
        m = r[k]
        ok = ok and (m["states"] == b["n3_states"] and m["share"] >= b["n3_share_min"] and m["logodds_max_error"] <= LOGODDS_TOL
                     and m["term_max_error"] <= 1e-5 and m["released_is_argmax"] == m["states"]
                     and all(abs(x - y) <= 1e-6 for x, y in zip(m["success"], m["success_want"])))
    return ok


# ---------------------------------------------------------------- the fortnight host
def play(persona, row, tmp, tag):
    """One life on the fortnight (or absent, return) draws. Returns per-hour records and the life (for replay and habits)."""
    strand = os.path.join(tmp, "%s-%d.strand" % (tag, row["seed"]))
    life = common.Life(persona, row["seed"], strand)
    hours = len(row["tokens"])
    recs = []
    prev = None
    prev_x = {"context": 0.0, "contact": 0.0}
    paid = {"compact": 0, "report": 0}
    try:
        for t in range(hours):
            ev = {"elapsed_hours": 0 if t == 0 else 1, "src": "env:host"}
            q = row["quiet"][t] == "1"
            if q:
                ev["quiet"] = True
            else:
                ev["senses"] = {"tokens": row["tokens"][t]}
            needs = {}
            goal_prev = pursued(prev) if prev else None
            answered = goal_prev == "report" and row["answer"][t] == "1"
            if answered or row["unprompted"][t] == "1":
                needs["contact"] = {"amount": 0.8, "src": "human:principal"}
            if goal_prev == "compact":
                needs["context"] = {"amount": 0.6, "src": "env:compactor"}
            if needs:
                ev["needs"] = needs
            doc = life.event(ev)
            if "error" in doc:
                raise common.SetupError("%s seed %d hour %d refused: %s" % (tag, row["seed"], t, common.canon(doc)[:200]))
            x = deficits(doc)
            recs.append(record(t, q, pursued(doc), x, prev_x, row, ev, goal_prev, answered))
            if goal_prev == "compact" and "context" in needs:
                paid["compact"] += 1
            if answered:
                paid["report"] += 1
            prev, prev_x = doc, x
    finally:
        life.close()
    return recs, life, paid


def grow(prev_x, row, t, h, q):
    """RFC 0002 A.3 growth, recomputed in Python from the logged inputs (independent of the engine)."""
    return {"context": common.r6(min(prev_x["context"] + (0.0 if q else row["tokens"][t]), 1.0)),
            "contact": common.r6(min(prev_x["contact"] + GROWTH_CONTACT * h, 1.0))}


def record(t, q, goal, x, prev_x, row, ev, goal_prev, answered):
    """One hour: the released goal, the deficits after the event (as reported), the deficits recomputed from the previous
    hour's reported deficits and this event's inputs, and the win of each satisfaction (recomputed)."""
    grown = grow(prev_x, row, t, ev["elapsed_hours"], q)
    wins, after = {}, dict(grown)
    for j, need in enumerate(("context", "contact")):
        if need in (ev.get("needs") or {}):
            a = ev["needs"][need]["amount"]
            wins[need] = common.reduction([grown["context"], grown["contact"]], j, a)
            after[need] = common.r6(min(abs(grown[need] - a), 1.0))
    return {"hour": t, "quiet": q, "goal": goal, "x": x, "x_recomputed": after, "wins": wins, "pursued_prev": goal_prev,
            "answered": answered}


def fortnight_lives(persona, rows, tmp, tag):
    out = []
    for row in rows:
        recs, life, paid = play(persona, row, tmp, tag)
        out.append({"seed": row["seed"], "recs": recs, "life": life, "paid": paid})
    return out


def per_day(recs, pred, days):
    return sum(1 for r in recs if pred(r)) / float(days)


def mean(v):
    return sum(v) / float(len(v)) if v else 0.0


def satisfaction_wins(lives, need):
    """Wins of satisfactions of a pursued goal (the host satisfies only after a pursuit, plus unprompted contact)."""
    goal = {"context": "compact", "contact": "report"}[need]
    return [r["wins"][need] for lv in lives for r in lv["recs"] if need in r["wins"] and r["pursued_prev"] == goal]


def fortnight_metrics(lives, days=14):
    """n8-n10 metrics of one arm (engine lives or a plain-Python null's records, same shape)."""
    per_seed = [per_day(lv["recs"], lambda r: r["goal"] == "report" and not r["quiet"], days) for lv in lives]
    both = sum(1 for lv in lives if any(r["goal"] == "compact" for r in lv["recs"]) and any(r["goal"] == "report" for r in lv["recs"])
               and lv["paid"]["compact"] > 0 and lv["paid"]["report"] > 0)
    w_all = satisfaction_wins(lives, "context") + satisfaction_wins(lives, "contact")
    answered_nonpos = sum(1 for lv in lives for r in lv["recs"]
                          if r["answered"] and r["pursued_prev"] == "report" and r["wins"].get("contact", 0) <= 1e-9)
    cover = [min(sum(1 for r in lv["recs"] if r["pursued_prev"] == "compact" and r["wins"].get("context", 0) > 1e-9),
                 sum(1 for r in lv["recs"] if r["pursued_prev"] == "report" and r["wins"].get("contact", 0) > 1e-9)) for lv in lives]
    growth_err = max(abs(r["x"][k] - r["x_recomputed"][k]) for lv in lives for r in lv["recs"] for k in ("context", "contact"))
    return {"reports_per_waking_day": round(mean(per_seed), 4), "interval": [round(v, 4) for v in common.bootstrap(per_seed)],
            "seeds_both_goals_pursued_and_paid": "%d/%d" % (both, len(lives)),
            "negative_wins": sum(1 for w in w_all if w < -1e-9), "nonpositive_wins": sum(1 for w in w_all if w <= 1e-9),
            "satisfactions": len(w_all),
            "answered_reports_without_a_positive_win": answered_nonpos,
            "min_positive_satisfactions_per_goal_per_seed": min(cover) if cover else 0,
            "context_at_cap_hours_per_day": round(mean([per_day(lv["recs"], lambda r: r["x"]["context"] >= 0.95, days)
                                                        for lv in lives]), 4),
            "growth_max_error": round(growth_err, 9), "seeds": len(lives)}


def absent_metrics(lives, days):
    by_day = []
    for d in range(days):
        by_day.append(round(mean([sum(1 for r in lv["recs"] if r["goal"] == "report" and r["hour"] // 24 == d) for lv in lives]), 4))
    waking = mean([per_day(lv["recs"], lambda r: r["goal"] == "report" and not r["quiet"], days) for lv in lives])
    return {"report_per_day": by_day, "reports_per_waking_day": round(waking, 4)}


def gate_n8(base, absent, b):
    return (base["reports_per_waking_day"] <= b["n8_per_waking_day_max"]
            and absent["reports_per_waking_day"] <= b["n8_per_waking_day_max"]
            and base["answered_reports_without_a_positive_win"] == 0
            and base["seeds_both_goals_pursued_and_paid"] == "%d/%d" % (base["seeds"], base["seeds"]))


def gate_n9(base, b):
    return (base["nonpositive_wins"] == 0 and base["min_positive_satisfactions_per_goal_per_seed"] >= b["n9_cover_min"]
            and base["context_at_cap_hours_per_day"] <= b["n9_at_cap_max"])


def gate_n10(tau0, tau2, overshoots_tau2, b):
    return (tau0["context_at_cap_hours_per_day"] >= b["n10_baseline_min"]
            and tau2["context_at_cap_hours_per_day"] <= b["n10_ratio_max"] * tau0["context_at_cap_hours_per_day"]
            and overshoots_tau2 == 0 and tau0["growth_max_error"] <= 1e-6 and tau2["growth_max_error"] <= 1e-6
            and tau2["seeds_both_goals_pursued_and_paid"] == "%d/%d" % (tau2["seeds"], tau2["seeds"]))


def gate_n11(absent, ret, b):
    a, r = absent["report_per_day"], ret["report_per_day"]
    return (mean(a[0:2]) >= b["n11_before_min"] and all(v <= b["n11_per_day_max"] for v in a[3:7])
            and mean(r[8:10]) >= b["n11_return_min"])


def fortnight_bars(tmp):
    res = {}
    base = fortnight_lives(common.fixture("needs2.yaml"), common.stream("fortnight"), tmp, "tau0")
    tau2 = fortnight_lives(common.fixture("variants/needs2-tau2.yaml"), common.stream("fortnight"), tmp, "tau2")
    absent = fortnight_lives(common.fixture("needs2.yaml"), common.stream("absent"), tmp, "absent")
    ret = fortnight_lives(common.fixture("needs2.yaml"), common.stream("return"), tmp, "return")
    null = fortnight_lives(common.fixture("variants/needs2-window1e6.yaml"), common.stream("absent"), tmp, "absent1e6")
    m0, m2 = fortnight_metrics(base), fortnight_metrics(tau2)
    res["fortnight_tau0"], res["fortnight_tau2"] = m0, m2
    res["n10_overshoots_tau2"] = sum(1 for w in satisfaction_wins(tau2, "context") if w < -1e-9)
    res["absent"], res["return"] = absent_metrics(absent, 7), absent_metrics(ret, 10)
    res["n11_null_window1e6"] = absent_metrics(null, 7)
    return res, base


def metric(bars, tmp):
    b = bars["needs"]
    r = grid_bars(tmp)
    r.update(n7_bars(tmp))
    r.update(probe_grid(tmp))
    fr, base = fortnight_bars(tmp)
    r.update(fr)
    common.LIVES.setdefault("needs", []).extend((common.fixture("needs2.yaml"), lv["seed"], lv["life"]) for lv in base)
    common.STANCES["n5"] = [(lv["life"].events, lv["life"].stances) for lv in base]
    r["pass"] = {
        "n1": gate_n1(r, b),
        "n2": gate_n2(r, b),
        "n3": gate_n3(r, b),
        "n7": gate_n7(r, b),
        "n8": gate_n8(r["fortnight_tau0"], r["absent"], b),
        "n9": gate_n9(r["fortnight_tau0"], b),
        "n10": gate_n10(r["fortnight_tau0"], r["fortnight_tau2"], r["n10_overshoots_tau2"], b),
        # the bar's own null (a window that never fires) must fail it, in the same run
        "n11": gate_n11(r["absent"], r["return"], b) and not gate_n11(r["n11_null_window1e6"], r["return"], b),
    }
    r.pop("grid")
    r["scope"] = ("needs1.yaml seed 0, 15-cell grid (n1, n2) and refusals (n7); needs2.yaml and needs2-s02.yaml probe grid, "
                  "71 states each, seed 0 (n3); fortnight seeds 0-99, 336 hourly events, tau 0 and 2 h (n8-n10); absent seeds "
                  "0-99, 168 events, and return seeds 0-99, 240 events (n8, n11)")
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
