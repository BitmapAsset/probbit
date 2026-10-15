"""P4 valence and the omission let-down (RFC 0002 Design E and A.4): bars v1, v2, v3 and om1.

    python3 valence.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

The habits go_task and go_other fix which goal the stance pursues, so a bar knows whose E enters the valence evidence.
Every event is at elapsed 0 unless a bar needs the clock, so the mood half-life never acts between the events compared.
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


def v2_move(persona, tmp, tag):
    """The valence evidence of a win equal to E (d = 0), as the accumulator's move beyond the inertia carry."""
    life = common.Life(persona, 0, os.path.join(tmp, tag + ".strand"))
    try:
        for _ in range(10):
            life.event(ev(win=1, go="task"))
        doc = life.event(ev(go="task"))
        e_task = doc["drives"]["expect"]["task"]
        life.event(ev(win=e_task, go="task"))
    finally:
        life.close()
    sts = life.states()
    common.LIVES.setdefault("valence", []).append((persona, 0, life))
    return push(sts[-1]) - INERTIA * push(sts[-2]), e_task


def metric(bars, tmp):
    b = bars["valence"]
    p, null = common.fixture("valence.yaml"), common.fixture("variants/valence-noblock.yaml")
    r = {}
    # v1: the same final win m 1 after a 24 h gap, with E built by 20 earlier wins vs E 0
    _, sa = states(p, [ev(win=1, go="other") for _ in range(20)] + [ev(h=24, win=1)], tmp, "v1-built")
    _, sb = states(p, [ev(go="other"), ev(h=24, win=1)], tmp, "v1-zero")
    r["v1_final_accumulators"] = [sa[-1]["mood"]["valence"], sb[-1]["mood"]["valence"]]
    r["v1_max_diff"] = max(abs(x - y) for x, y in zip(sa[-1]["mood"]["valence"], sb[-1]["mood"]["valence"]))
    # v2: a win equal to E moves valence evidence by w_e x E > 0 (today's effects.surprise: 0)
    move, e_task = v2_move(p, tmp, "v2")
    move0, _ = v2_move(null, tmp, "v2-null")
    r["v2_move"], r["v2_E"], r["v2_null_move"] = round(move, 6), e_task, round(move0, 6)
    # v3: push left 5 events after an impulse
    _, sv = states(p, [ev(win=1, go="other")] + [ev(go="other") for _ in range(5)], tmp, "v3")
    r["v3_push_left"] = round(push(sv[5]) / push(sv[0]), 6) if push(sv[0]) else None
    # om1: the omission closed form, once per window
    evs = [ev(win=1, go="task") for _ in range(10)] + [ev(go="task"), ev(h=3, go="other"), ev(h=3, go="other")]
    docs, _ = states(p, evs, tmp, "om1")
    e10 = docs[10]["drives"]["expect"]["task"]
    e_dec = common.r6(e10 * 0.5 ** (3 / HL_E))
    want11 = (common.r6(e_dec * (1 - RATE)), -min(e_dec, PE_CAP))
    got11 = (docs[11]["drives"]["expect"]["task"], docs[11]["drives"]["surprise"]["task"])
    want12 = common.r6(got11[0] * 0.5 ** (3 / HL_E))
    got12 = (docs[12]["drives"]["expect"]["task"], docs[12]["drives"]["surprise"]["task"])
    r["om1"] = {"E_before": e10, "fired": {"want": want11, "got": got11}, "next_window": {"want": [want12, 0], "got": got12}}
    ok_om = (abs(got11[0] - want11[0]) <= b["om1_tol"] and abs(got11[1] - want11[1]) <= b["om1_tol"]
             and abs(got12[0] - want12) <= b["om1_tol"] and got12[1] == 0)
    r["pass"] = {
        "v1": r["v1_max_diff"] > b["v1_differ_above"],
        "v2": r["v2_move"] > 0,
        "v3": r["v3_push_left"] is not None and 0 <= r["v3_push_left"] <= b["v3_push_left_max"],
        "om1": ok_om,
    }
    r["scope"] = "valence.yaml seed 0 (valence-noblock.yaml for the v2 null); scripted events at elapsed 0, 3 and 24 h"
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
