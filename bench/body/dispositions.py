"""P3 dispositions (RFC 0002 Design D): bars o1, o2 and o3 (o4 in habits.py, o5 in p0.py).

    python3 dispositions.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

Bars v2: o1 adds the two-point-gaps stream (E decays between wins) and o2 the whole two-point trajectory, each E checked
against `expect_trajectory` at every event; o3 adds a distance floor, zero distance up to the earliest win (the gene acts only
through outcomes) and released `pursue` goals that differ between twins.
"""
import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "dispositions"
FIXTURES = ["optimism.yaml", "optimism-spread0.yaml"]
RATE = 0.2
HL_E, WIN_MAX = 48.0, 2.0
E_TOL = 1e-5


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def sign(x):
    return (x > 0) - (x < 0)


def two_point_events(n):
    row = common.stream("two-point")[0]
    return [{"elapsed_hours": 0, "goals": {"task": {"win": row["b"] if c == "b" else row["a"]}}, "src": "env:host"}
            for c in row["wins"][:n]], row["a"], row["b"]


def life_e(persona, seed, events, tmp):
    """E of `task` after every event, and the individual's optimism gene (0 when no gene is stored)."""
    st = os.path.join(tmp, "o-%s-%d.state" % (os.path.basename(persona), seed))
    common.init(persona, seed, st)
    code, docs, err = common.live(persona, events, state=st)
    if code or len(docs) != len(events):
        raise common.SetupError("optimism life failed: " + err[-200:])
    with open(st, encoding="utf-8") as f:
        state = json.load(f)
    o = (state.get("dispositions") or {}).get("optimism", 0)
    return [d["drives"]["expect"]["task"] for d in docs], o


def expect_trajectory(o, wins):
    """E after each (hours, win): section 2.9's decay, then RFC 0002 D's asymmetric step, rounded to 6 decimals (from E 0)."""
    E, out = 0.0, []
    for h, m in wins:
        E = E * 0.5 ** (h / HL_E)
        a = min(RATE * math.exp(o), 1.0) if m > E else min(RATE * math.exp(-o), 1.0)
        E = common.r6(min(max(E + a * (m - E), 0.0), WIN_MAX))
        out.append(E)
    return out


def tv_per_turn(da, db):
    """Per turn: the mean TV distance over the traits and `pursue` (the terms `distance` averages)."""
    return [(sum(common.tv(x["stance"][t]["odds"], y["stance"][t]["odds"]) for t in x["stance"])
             + common.tv(x["pursue"]["odds"], y["pursue"]["odds"])) / (len(x["stance"]) + 1) for x, y in zip(da, db)]


def distance(da, db):
    """docs/persona.md section 5.4 over the traits and `pursue`: the mean TV distance of their odds per turn."""
    tot, n = 0.0, 0
    for x, y in zip(da, db):
        for t in x["stance"]:
            tot += common.tv(x["stance"][t]["odds"], y["stance"][t]["odds"])
            n += 1
        tot += common.tv(x["pursue"]["odds"], y["pursue"]["odds"])
        n += 1
    return tot / n if n else 0.0


def gate_o3(dist, before, flipped, b):
    return dist >= b["o3_distance_min"] and before == 0 and flipped >= b["o3_pairs_changed_min"]


def metric(bars, tmp):
    b = bars["dispositions"]
    opt, zero = common.fixture("optimism.yaml"), common.fixture("optimism-spread0.yaml")
    r = {}
    # o1: E settles at the expectile, individuals 0-4, wins 10,001-20,000
    events, a, bb = two_point_events(20000)
    errs, per = [], {}
    for seed in range(5):
        e, o = life_e(opt, seed, events, tmp)
        ebar = sum(e[10000:20000]) / 10000.0
        ap, am = min(RATE * math.exp(o), 1.0), min(RATE * math.exp(-o), 1.0)
        tau = ap / (ap + am)
        per[seed] = {"o": o, "E_bar": round(ebar, 6), "expectile": round(tau * bb + (1 - tau) * a, 6)}
        if abs(o) >= b["o1_min_abs_o"]:
            errs.append(sign(o) * (ebar - tau * bb - (1 - tau) * a))
    r["o1_individuals"] = per
    r["o1_pooled_error"] = round(sum(errs) / len(errs), 6) if errs else None
    # o1 v2: varied gaps, every individual's E against the closed form at every event
    gw = common.stream("two-point-gaps")[0]["wins"]
    gev = [{"elapsed_hours": h, "goals": {"task": {"win": m}}, "src": "env:host"} for h, m in gw]
    gap_err = 0.0
    for seed in range(5):
        e, o = life_e(opt, seed, gev, tmp)
        gap_err = max(gap_err, max(abs(x - y) for x, y in zip(e, expect_trajectory(o, gw))))
    r["o1_gaps_max_error"] = round(gap_err, 9)
    # o2: the gene's sign is readable, individuals 0-99, wins 2,001-3,000
    ev3 = events[:3000]
    w3 = [(0, e["goals"]["task"]["win"]) for e in ev3]
    match, traj_err = 0, 0.0
    for seed in range(100):
        ez, o = life_e(opt, seed, ev3, tmp)
        e0, _ = life_e(zero, seed, ev3, tmp)
        diff = sum(ez[2000:3000]) / 1000.0 - sum(e0[2000:3000]) / 1000.0
        match += 1 if sign(round(diff, 9)) == sign(o) else 0
        traj_err = max(traj_err, max(abs(x - y) for x, y in zip(ez, expect_trajectory(o, w3))),
                       max(abs(x - y) for x, y in zip(e0, expect_trajectory(0.0, w3))))
    r["o2_sign_readable"] = "%d/100" % match
    r["o2_trajectory_max_error"] = round(traj_err, 9)
    # o3: twin distance over 20 seed pairs at spread 0.3, and at spread 0 (the null, exactly 0)
    d3, d0, recs, before, flipped = [], [], [], 0.0, 0
    for row in common.stream("pairs"):
        sa, sb = row["pair"]
        lives = {}
        for tag, p in (("opt", opt), ("zero", zero)):
            for s in (sa, sb):
                strand = os.path.join(tmp, "pairs-%s-%d.strand" % (tag, s))
                code, docs, err = common.live(p, row["events"], seed=s, strand=strand)
                if code or len(docs) != len(row["events"]):
                    raise common.SetupError("pairs life failed: " + err[-200:])
                lives[(tag, s)] = docs
                if tag == "opt":
                    recs.append((row["events"], docs))
                    common.LIVES.setdefault("dispositions", []).append((p, s, common.Record(row["events"], docs, strand)))
        d3.append(distance(lives[("opt", sa)], lives[("opt", sb)]))
        d0.append(distance(lives[("zero", sa)], lives[("zero", sb)]))
        earliest = [i for i, e in enumerate(row["events"]) if ((e.get("goals") or {}).get("task") or {}).get("win")][0]
        per = tv_per_turn(lives[("opt", sa)], lives[("opt", sb)])
        before = max([before] + per[:earliest + 1])
        flipped += 1 if any((x.get("pursue") or {}).get("goal") != (y.get("pursue") or {}).get("goal")
                            for x, y in zip(lives[("opt", sa)], lives[("opt", sb)])) else 0
    common.STANCES["o4"] = recs
    r["o3_distance_spread0.3"] = round(sum(d3) / len(d3), 6)
    r["o3_distance_spread0"] = round(sum(d0) / len(d0), 9)
    r["o3_max_distance_up_to_first_win"] = round(before, 12)
    r["o3_pairs_with_a_changed_pursue"] = "%d/%d" % (flipped, len(d3))
    r["pass"] = {
        "o1": (r["o1_pooled_error"] is not None and abs(r["o1_pooled_error"]) <= b["o1_error_max"]
               and r["o1_gaps_max_error"] <= E_TOL),
        "o2": match == 100 and r["o2_trajectory_max_error"] <= E_TOL,
        "o3": gate_o3(r["o3_distance_spread0.3"], before, flipped, b),
    }
    r["scope"] = ("optimism.yaml (spread 0.3) and optimism-spread0.yaml; two-point stream, 20,000 wins at elapsed 0, "
                  "individuals 0-4 (o1) and 0-99 (o2); two-point-gaps stream, 3,000 wins, individuals 0-4 (o1); pairs stream, "
                  "20 pairs (0, 1) ... (38, 39), 300 events (o3)")
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
