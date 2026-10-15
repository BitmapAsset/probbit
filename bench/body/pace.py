"""P2 pace (RFC 0002 Design C): bars p1a, p1b, p1c and p6 (pace's replay and P0 run inside replay.py and p0.py).

    python3 pace.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "pace"
FIXTURES = ["pace.yaml"]
HL, CAP, WIN_MAX = 6.0, 4.0, 4.0
KK = 1 - 0.5 ** (1.0 / HL)
PERTURB_AT = (100, 500, 1000, 1500)
FUZZ_SEEDS = range(10)


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def rbar(doc):
    return doc["pace"]["rbar"]


def recompute(events):
    """R-bar after each event from events 1..t only (RFC 0002 Design C), independent of the engine."""
    R, out = 0.0, []
    for ev in events:
        r_t = sum(g.get("win", 0) for g in (ev.get("goals") or {}).values())
        R = common.r6(min(max(R * 0.5 ** (ev.get("elapsed_hours", 0) / HL) + KK * r_t, 0.0), CAP))
        out.append(R)
    return out


def fuzzed(events, seed):
    """The same rewards with offered stakes varied: cues, deadlines, progress and novelty on both goals, no new wins."""
    g = random.Random(1000 + seed)
    out = []
    for ev in events:
        e = dict(ev)
        goals = {k: dict(v) for k, v in (ev.get("goals") or {}).items()}
        for goal in ("task", "other"):
            sig = goals.setdefault(goal, {})
            if g.random() < 0.5:
                sig["cue"] = True
            if g.random() < 0.3:
                sig["deadline_hours"] = round(g.uniform(0, 48), 3)
            if g.random() < 0.3:
                sig["progress"] = round(g.random(), 3)
            if g.random() < 0.3:
                sig["novelty"] = round(g.random(), 3)
            if not sig:
                del goals[goal]
        if goals:
            e["goals"] = goals
        out.append(e)
    return out


def loop(persona, row, tmp):
    """The scripted host of the loop stream; returns (R-bar after the warm-up, the R-bar trajectory)."""
    life = common.Life(persona, 0, os.path.join(tmp, "loop-%s-%s.strand" % (row["kappa"], row["a_cap"])))
    try:
        w0 = common.r6((row["r0"] + row["start_above_r0"]) / (3 * KK))
        doc = None
        for _ in range(3):   # a new individual starts at R-bar 0: three zero-hour wins bring it to r0 + 0.2
            doc = life.event({"elapsed_hours": 0, "goals": {"task": {"win": w0}}, "src": "env:host"})
        start = rbar(doc)
        traj = []
        for _ in range(row["hours"]):
            a = min(max(row["base"] * (1 + row["kappa"] * (rbar(doc) - row["r0"])), 0.0), row["a_cap"])
            pay = common.r6(min(row["p"] * a, WIN_MAX))
            ev = {"elapsed_hours": 1, "src": "env:host"}
            if pay > 0:
                ev["goals"] = {"task": {"win": pay}}
            doc = life.event(ev)
            traj.append(rbar(doc))
    finally:
        life.close()
    return start, traj, life


def metric(bars, tmp):
    b = bars["pace"]
    p = common.fixture("pace.yaml")
    rows = common.stream("blocks")
    events = [row["event"] for row in rows]
    strand = os.path.join(tmp, "blocks.strand")
    code, docs, err = common.live(p, events, seed=0, strand=strand)
    if code or len(docs) != len(events):
        raise common.SetupError("pace blocks life failed: " + err[-200:])
    common.LIVES.setdefault("pace", []).append((p, 0, common.Record(events, docs, strand)))
    got = [rbar(d) for d in docs]
    want = recompute(events)
    r = {"p1a_max_abs_diff": max(abs(x - y) for x, y in zip(got, want))}
    changed = 0
    for t in PERTURB_AT:
        future = [dict(e) for e in events]
        for e in future[t + 1:]:
            if "goals" in e:
                e["goals"] = {"task": {"win": common.r6(min(e["goals"]["task"]["win"] * 2, WIN_MAX))}}
            else:
                e["goals"] = {"task": {"win": 1}}
        _, d2, _ = common.live(p, future, seed=0)
        changed += sum(1 for x, y in zip(docs[:t + 1], d2[:t + 1]) if common.canon(x) != common.canon(y))
    r["p1a_past_stances_changed_by_the_future"] = changed
    fz = 0
    for s in FUZZ_SEEDS:
        _, d3, _ = common.live(p, fuzzed(events, s), seed=0)
        fz += sum(1 for x, y in zip(docs, d3) if rbar(x) != rbar(y)) + abs(len(d3) - len(docs))
    r["p1b_pace_changes"] = fz
    r["p1c_spearman"] = round(common.spearman(got, [row["rate"] for row in rows]), 4)
    p6 = {}
    ok6 = True
    for row in common.stream("loop"):
        start, traj, life = loop(p, row, tmp)
        G = row["p"] * row["kappa"] * row["base"]
        key = "kappa%.1f_acap%d" % (row["kappa"], row["a_cap"])
        if G < 1:
            closed = row["p"] * row["base"] * (1 - row["kappa"] * row["r0"]) / (1 - G)
            transient = closed + (start - closed) * (1 - KK * (1 - G)) ** row["hours"]
            diff = abs(traj[-1] - transient)
            p6[key] = {"G": round(G, 4), "rbar_400": traj[-1], "closed_form": round(transient, 6), "diff": round(diff, 8)}
            ok6 = ok6 and diff <= b["p6_tol"]
        else:
            pin = min(row["p"] * row["a_cap"], CAP)
            p6[key] = {"G": round(G, 4), "rbar_400": traj[-1], "pin": pin, "max": max(traj)}
            ok6 = ok6 and abs(traj[-1] - pin) <= b["p6_tol"] and max(traj) <= CAP
        common.LIVES.setdefault("pace", []).append((p, 0, life))
    r["p6"] = p6
    r["pass"] = {
        "p1a": r["p1a_max_abs_diff"] <= b["p1a_tol"] and changed == 0,
        "p1b": fz == b["p1b_changes"],
        "p1c": r["p1c_spearman"] >= b["p1c_spearman_min"],
        "p6": ok6,
    }
    r["scope"] = ("pace.yaml seed 0; blocks stream, 2,000 hourly events (p1a-p1c), futures perturbed after events 100, 500, "
                  "1,000, 1,500, fuzz seeds 0-9; loop stream, kappa 0.2-1.0, a_cap 12 and 20, 400 h (p6)")
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
