"""P4 valence and the omission let-down (RFC 0002 Design E and A.4): bars v1, v2, v3 and om1.

    python3 valence.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

The habits go_task and go_other fix which goal the stance pursues, so a bar knows whose E enters the valence evidence.
Every event is at elapsed 0 unless a bar needs the clock, so the mood half-life never acts between the events compared.

Bars v2 compare ratios of the valence push (a_t = k d a_(t-1) + (1 - k) e_t, section 3 step 3: the mood's scale cancels):
v1 the push ratio d_A / d_B and the mood's odds; v2 the move's proportionality to E and the released odds against the
no-block twin; v3 the residual at lags 1-5; om1 the window before, at and past expiry, and with a win inside it.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "drives.valence"
FIXTURES = ["valence.yaml"]
INERTIA, RATE, PE_CAP, HL_E = 0.61, 0.5, 1.0, 48.0
SRC = "env:host"
RATIO_TOL, OM_TOL = 1e-4, 1e-6


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def push(state):
    """The valence accumulator's up-minus-down spread."""
    a = state["mood"]["valence"]
    return a[-1] - a[0]


def ev(h=0, win=None, go=None):
    e = {"elapsed_hours": h, "src": SRC}
    if win is not None:
        e["goals"] = {"task": {"win": win}}
    if go:
        e["go_" + go] = True
    return e


def states(persona, events, tmp, tag):
    code, docs, sts, err = common.live_states(persona, events, 0, os.path.join(tmp, tag + ".strand"))
    if code or len(sts) != len(events):
        raise common.SetupError("valence life %s failed: %s" % (tag, err[-200:]))
    common.LIVES.setdefault("valence", []).append((persona, 0, common.Record(events, docs, os.path.join(tmp, tag + ".strand"))))
    return docs, sts


def up(doc):
    return doc["mood"]["valence"]["odds"]["up"]


def v2_move(persona, tmp, tag, wins=10):
    """The valence evidence of a win equal to E (d = 0), as the accumulator's move beyond the inertia carry; E; P(up)."""
    life = common.Life(persona, 0, os.path.join(tmp, tag + ".strand"))
    try:
        for _ in range(wins):
            life.event(ev(win=1, go="task"))
        doc = life.event(ev(go="task"))
        e_task = doc["drives"]["expect"]["task"]
        last = life.event(ev(win=e_task, go="task"))
    finally:
        life.close()
    sts = life.states()
    common.LIVES.setdefault("valence", []).append((persona, 0, life))
    return push(sts[-1]) - INERTIA * push(sts[-2]), e_task, up(last)


def om1_case(persona, tmp, tag, case):
    """10 wins pursuing `task`, a stance that pursues it (the window opens), then the case's events; (E, d) per case event
    against arith_0002.omission from the engine's E after the pursuing stance."""
    import arith_0002
    evs = [ev(win=1, go="task") for _ in range(10)] + [ev(go="task")] + [ev(h=h, win=m, go="other") for h, m in case]
    docs, _ = states(persona, evs, tmp, tag)
    e_open = docs[10]["drives"]["expect"]["task"]
    want = arith_0002.omission(e_open, case)
    got = [(d["drives"]["expect"]["task"], d["drives"]["surprise"]["task"]) for d in docs[11:]]
    return {"E_open": e_open, "want": want, "got": got,
            "max_error": round(max(max(abs(a[0] - b[0]), abs(a[1] - b[1])) for a, b in zip(got, want)), 9)}


def gate_v1(r, b):
    return (r["v1_max_diff"] > b["v1_differ_above"] and r["v1_push_ratio"] is not None
            and abs(r["v1_push_ratio"] - r["v1_push_ratio_want"]) <= RATIO_TOL and r["v1_up_odds"][0] < r["v1_up_odds"][1])


def gate_v2(r, b):
    m = r["v2_move_ratio"]
    return (r["v2_move"] > 0 and abs(r["v2_null_move"]) <= 2e-6 and m[0] is not None and m[1] is not None
            and abs(m[0] - m[1]) <= RATIO_TOL and r["v2_up_odds"]["block"] > r["v2_up_odds"]["no_block"])


def metric(bars, tmp):
    b = bars["valence"]
    p, null = common.fixture("valence.yaml"), common.fixture("variants/valence-noblock.yaml")
    r = {}
    # v1: the same final win m 1 after a 24 h gap, with E built by 20 earlier wins vs E 0
    da, sa = states(p, [ev(win=1, go="other") for _ in range(20)] + [ev(h=24, win=1)], tmp, "v1-built")
    db, sb = states(p, [ev(go="other"), ev(h=24, win=1)], tmp, "v1-zero")
    r["v1_final_accumulators"] = [sa[-1]["mood"]["valence"], sb[-1]["mood"]["valence"]]
    r["v1_max_diff"] = max(abs(x - y) for x, y in zip(sa[-1]["mood"]["valence"], sb[-1]["mood"]["valence"]))
    e20 = da[-2]["drives"]["expect"]["task"]
    r["v1_push_ratio"] = round(push(sa[-1]) / push(sb[-1]), 6) if push(sb[-1]) else None
    r["v1_push_ratio_want"] = common.r6(1 - common.r6(e20 * 0.5 ** (24 / HL_E)))
    r["v1_up_odds"] = [up(da[-1]), up(db[-1])]
    # v2: a win equal to E moves valence evidence by w_e x E > 0: proportional to E, and released in the mood's odds
    move, e_task, up10 = v2_move(p, tmp, "v2")
    move3, e3, _ = v2_move(p, tmp, "v2-3", wins=3)
    move0, _, up0 = v2_move(null, tmp, "v2-null")
    r["v2_move"], r["v2_E"], r["v2_null_move"] = round(move, 6), e_task, round(move0, 6)
    r["v2_move_ratio"] = [round(move / move3, 6) if move3 else None, round(e_task / e3, 6) if e3 else None]
    r["v2_up_odds"] = {"block": up10, "no_block": up0}
    # v3: push left 1-5 events after an impulse, two-sided at each lag
    _, sv = states(p, [ev(win=1, go="other")] + [ev(go="other") for _ in range(5)], tmp, "v3")
    r["v3_push_left"] = [round(push(sv[j]) / push(sv[0]), 6) if push(sv[0]) else None for j in range(1, 6)]
    # om1: before then at expiry, past expiry then a repeat, and a win inside the window
    r["om1"] = {name: om1_case(p, tmp, "om1-" + name, case) for name, case in
                (("before_then_at", [(1, None), (1, None)]), ("after_then_repeat", [(3, None), (3, None)]),
                 ("win_between", [(1, 1.0), (3, None)]))}
    r["pass"] = {
        "v1": gate_v1(r, b),
        "v2": gate_v2(r, b),
        "v3": all(v is not None and abs(v - INERTIA ** j) <= RATIO_TOL for j, v in enumerate(r["v3_push_left"], 1))
              and r["v3_push_left"][4] <= b["v3_push_left_max"],
        "om1": all(c["max_error"] <= b["om1_tol"] for c in r["om1"].values()),
    }
    r["scope"] = "valence.yaml seed 0 (valence-noblock.yaml for the v2 null); scripted events at elapsed 0, 1, 3 and 24 h"
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
