"""P1 needs (RFC 0002 Design A): bars n1, n2, n3, n7, n8, n9, n10 and n11 (n4 in replay.py, n5 in habits.py, n6 in p0.py).

    python3 needs.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

The fortnight and absent streams are played by a scripted host (README "Streams and seeds"): it sends the hour's reading,
one report per hour the stance pursues `report` (answered at the next event when the seeded draw says so), the principal's
unprompted messages, and a compaction at the next event after a stance pursues `compact`.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "needs"
FIXTURES = ["needs1.yaml", "needs2.yaml", "variants/needs2-tau2.yaml", "variants/needs2-nohabit.yaml",
            "variants/needs2-window1e6.yaml"]
EXPECT_RATE = 0.5      # docs/persona.md section 2.9 default expectation rate (needs1 keeps drives at their defaults)
SETBACK_WEIGHT = 0.5   # section 2.9 default wanting signal weight for a setback
K = {"context": 0.6, "contact": 0.8}
GROWTH_CONTACT = 0.04


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def pursued(doc):
    p = doc.get("pursue") or {}
    return p.get("goal") if p.get("released") else None


def deficits(doc):
    return {k: v["deficit"] for k, v in (doc.get("needs") or {}).items()}


# ---------------------------------------------------------------- n1, n2, n7 (needs1)
def needs1_bars(tmp):
    p = common.fixture("needs1.yaml")
    res = {}
    e = {}
    for label, hours in (("hi", 8), ("lo", 2)):
        code, docs, err = common.live(p, [{"elapsed_hours": hours, "needs": {"fuel": 0.2}, "src": "env:host"}], seed=0)
        if code or not docs:
            raise common.SetupError("needs1 life failed: " + err[-200:])
        e[label] = docs[-1]["drives"]["expect"]["refuel"]
    res["n1_wins"] = [common.r6(e["hi"] / EXPECT_RATE), common.r6(e["lo"] / EXPECT_RATE)]
    res["n1_ratio"] = round(e["hi"] / e["lo"], 6) if e["lo"] else None
    code, docs, err = common.live(p, [{"elapsed_hours": 0, "needs": {"fuel": 0.2}, "src": "env:host"}], seed=0)
    d = docs[-1]["drives"]
    res["n2_expect"] = d["expect"]["refuel"]
    res["n2_win"] = common.r6(-d["want"]["refuel"] / SETBACK_WEIGHT) if d["expect"]["refuel"] == 0 else None
    # n7: refused sources through the CLI, and any amount through MCP
    st = os.path.join(tmp, "n1.state")
    common.init(p, 0, st)
    bad_srcs = [{"amount": 0.2, "src": "self"}, {"amount": 0.2, "src": "clock"}, {"amount": 0.2},
                {"amount": 0.2, "src": "not a source"}]
    refused = 0
    for b in bad_srcs:
        ev = {"elapsed_hours": 1, "needs": {"fuel": b}}
        out = os.path.join(tmp, "n7.out")
        code, doc, err = common.turn(p, st, ev, out)
        refused += 1 if (code != 0 and not os.path.exists(out)) else 0
    with open(p, encoding="utf-8") as f:
        persona_text = f.read()
    with open(st, encoding="utf-8") as f:
        state_doc = f.read()
    mcp = common.mcp_call("probbit_persona_turn", {"persona": persona_text, "state": json.loads(state_doc),
                                                   "inputs": {"elapsed_hours": 1, "needs": {"fuel": 0.2}, "src": "env:host"}})
    refused += 1 if mcp.get("isError") else 0
    total = len(bad_srcs) + 1
    base = [{"elapsed_hours": 1, "src": "env:host"}, {"elapsed_hours": 1, "needs": {"fuel": 0.2}, "src": "env:host"},
            {"elapsed_hours": 1, "src": "env:host"}]
    mixed = [base[0]] + [{"elapsed_hours": 1, "needs": {"fuel": b}} for b in bad_srcs] + base[1:]
    _, d_base, _ = common.live(p, base, seed=0)
    _, d_mixed, _ = common.live(p, mixed, seed=0)
    kept = [d for d in d_mixed if "error" not in d]
    same = [common.canon(x) for x in kept] == [common.canon(x) for x in d_base]
    # the null: the same amount from env is accepted and changes the state
    out = os.path.join(tmp, "n7-env.out")
    code_env, doc_env, _ = common.turn(p, st, {"elapsed_hours": 1, "needs": {"fuel": 0.2}, "src": "env:host"}, out)
    res["n7_refused"] = "%d/%d" % (refused, total)
    res["n7_run_equals_run_without"] = same
    res["n7_null_env_accepted"] = code_env == 0 and os.path.exists(out)
    return res


# ---------------------------------------------------------------- n3 (needs2, probe grid)
def probe_grid(tmp):
    p = common.fixture("needs2.yaml")
    rows = common.stream("probe")
    warm = os.path.join(tmp, "warm.state")
    common.init(p, 0, warm)
    st = os.path.join(tmp, "warm2.state")
    code, _, err = common.turn(p, warm, rows[0]["warmup"], st)
    if code:
        raise common.SetupError("probe warm-up refused: " + err[-200:])
    agree, n, tmax = 0, 0, 0.0
    for row in rows[1:]:
        code, doc, err = common.turn(p, st, row["event"], os.path.join(tmp, "probe.out"))
        if code or doc is None:
            raise common.SetupError("probe refused: " + err[-200:])
        x = deficits(doc)
        xs = [x["context"], x["contact"]]
        red = [common.reduction(xs, 0, K["context"]), common.reduction(xs, 1, K["contact"])]
        odds = doc["pursue"]["odds"]
        n += 1
        want = (red[0] > red[1]) - (red[0] < red[1])
        got = (odds["compact"] > odds["report"]) - (odds["compact"] < odds["report"])
        agree += 1 if want == got else 0
        tmax = max(tmax, abs(doc["needs"]["context"]["term"]), abs(doc["needs"]["contact"]["term"]))
    return {"n3_agree": "%d/%d" % (agree, n), "n3_share": agree / float(n) if n else 0.0, "n3_term_max": tmax, "n3_states": n}


# ---------------------------------------------------------------- the fortnight host
def play(persona, row, tmp, tag):
    """One life on the fortnight (or absent) draws. Returns per-hour records and the life (for replay and habits)."""
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
            # the win of each satisfaction, recomputed from the stance-reported deficits (independent of the engine)
            grown = {"context": min(prev_x["context"] + (0 if q else row["tokens"][t]), 1.0),
                     "contact": min(prev_x["contact"] + GROWTH_CONTACT * ev["elapsed_hours"], 1.0)}
            wins = {}
            for j, need in enumerate(("context", "contact")):
                if need in needs:
                    xs = [grown["context"], grown["contact"]]
                    wins[need] = common.reduction(xs, j, needs[need]["amount"])
            if goal_prev == "compact" and "context" in needs:
                paid["compact"] += 1
            if answered:
                paid["report"] += 1
            x = deficits(doc)
            recs.append({"hour": t, "quiet": q, "goal": pursued(doc), "x": x, "wins": wins, "pursued_prev": goal_prev})
            prev, prev_x = doc, x
    finally:
        life.close()
    return recs, life, paid


def fortnight_lives(persona, rows, tmp, tag):
    out = []
    for row in rows:
        recs, life, paid = play(persona, row, tmp, tag)
        out.append({"seed": row["seed"], "recs": recs, "life": life, "paid": paid})
    return out


def per_day(recs, pred, days):
    return sum(1 for r in recs if pred(r)) / float(days)


def satisfaction_wins(lives, need):
    """Wins of satisfactions of a pursued goal (the host satisfies only after a pursuit, plus unprompted contact)."""
    goal = {"context": "compact", "contact": "report"}[need]
    return [r["wins"][need] for lv in lives for r in lv["recs"] if need in r["wins"] and r["pursued_prev"] == goal]


def fortnight_bars(tmp):
    rows = common.stream("fortnight")
    res = {}
    base = fortnight_lives(common.fixture("needs2.yaml"), rows, tmp, "tau0")
    tau2 = fortnight_lives(common.fixture("variants/needs2-tau2.yaml"), rows, tmp, "tau2")
    # n8: released report pursuits in waking hours per day, mean over seeds; non-vacuity
    per_seed = [per_day(lv["recs"], lambda r: r["goal"] == "report" and not r["quiet"], 14) for lv in base]
    res["n8_per_waking_day"] = round(sum(per_seed) / len(per_seed), 4)
    res["n8_interval"] = [round(v, 4) for v in common.bootstrap(per_seed)]
    nonvacuous = [lv for lv in base
                  if any(r["goal"] == "compact" for r in lv["recs"]) and any(r["goal"] == "report" for r in lv["recs"])
                  and lv["paid"]["compact"] > 0 and lv["paid"]["report"] > 0]
    res["n8_nonvacuous_seeds"] = "%d/%d" % (len(nonvacuous), len(base))
    # n9: satisfactions of a pursued goal with a negative win at horizon 0
    w0 = satisfaction_wins(base, "context") + satisfaction_wins(base, "contact")
    res["n9_negative_wins"] = sum(1 for w in w0 if w < -1e-9)
    res["n9_satisfactions"] = len(w0)
    # n10: hours at cap (context >= 0.95) per day, tau 2 vs tau 0; overshooting satisfactions at tau 2
    def at_cap(lives):
        v = [per_day(lv["recs"], lambda r: r["x"]["context"] >= 0.95, 14) for lv in lives]
        return sum(v) / len(v)
    res["n10_at_cap_per_day"] = {"tau0": round(at_cap(base), 4), "tau2": round(at_cap(tau2), 4)}
    w2 = satisfaction_wins(tau2, "context")
    res["n10_overshoots_tau2"] = sum(1 for w in w2 if w < -1e-9)
    # n11: report pursuits per day on stream days 4-7 (absent days 2-5), mean over seeds; the null at window 1e6
    absent = fortnight_lives(common.fixture("needs2.yaml"), common.stream("absent"), tmp, "absent")
    null = fortnight_lives(common.fixture("variants/needs2-window1e6.yaml"), common.stream("absent"), tmp, "absent1e6")

    def days(lives):
        out = []
        for d in range(7):
            v = [sum(1 for r in lv["recs"] if r["goal"] == "report" and r["hour"] // 24 == d) for lv in lives]
            out.append(round(sum(v) / float(len(v)), 4))
        return out
    res["n11_report_per_day"] = days(absent)
    res["n11_null_window1e6_per_day"] = days(null)
    return res, base


def metric(bars, tmp):
    b = bars["needs"]
    r = needs1_bars(tmp)
    r.update(probe_grid(tmp))
    fr, base = fortnight_bars(tmp)
    r.update(fr)
    common.LIVES.setdefault("needs", []).extend((common.fixture("needs2.yaml"), lv["seed"], lv["life"]) for lv in base)
    common.STANCES["n5"] = [(lv["life"].events, lv["life"].stances) for lv in base]
    gates = {
        "n1": r["n1_ratio"] is not None and r["n1_ratio"] >= b["n1_ratio_min"],
        "n2": r["n2_win"] is not None and r["n2_win"] < b["n2_win_below"],
        "n3": r["n3_states"] == b["n3_states"] and r["n3_share"] >= b["n3_share_min"],
        "n7": r["n7_refused"] == "5/5" and r["n7_run_equals_run_without"] and r["n7_null_env_accepted"],
        "n8": r["n8_per_waking_day"] <= b["n8_per_waking_day_max"] and r["n8_nonvacuous_seeds"] == "100/100",
        "n9": r["n9_negative_wins"] == 0,
        "n10": (r["n10_at_cap_per_day"]["tau2"] <= b["n10_ratio_max"] * r["n10_at_cap_per_day"]["tau0"]
                and r["n10_overshoots_tau2"] == 0),
        "n11": all(v <= b["n11_per_day_max"] for v in r["n11_report_per_day"][3:7]),
    }
    r["pass"] = gates
    r["scope"] = ("needs1.yaml seed 0 (n1, n2, n7); needs2.yaml probe grid, 71 states, seed 0 (n3); fortnight seeds 0-99, "
                  "336 hourly events, tau 0 and 2 h (n8-n10); absent seeds 0-99, 168 events (n11)")
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
