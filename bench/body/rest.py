"""P6 rest (RFC 0002 part 2): bars r0, r1 and r2 (r3 in replay.py, r4 in habits.py, r5 in p0.py).

    python3 rest.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

Reversal: 60 turns praising `mid` (0.25 h apart, so the pressure S passes the threshold), an 8 h idle gap (an event without
feedback, so the rest line is not deferred), then 59 turns praising `high`; feedback flipped where the draw is below 0.10;
lock turns carry no feedback. Lineage: RFC 0001's absorbing lineage, played by the host: each generation's learned weights
are added to the next generation's prior (`logw`), 10 generations of 300 rewarded events, an 8 h gap every 60 events.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "rest"
FIXTURES = ["rest.yaml", "variants/rest-norest.yaml"]
NOISE = 0.10
GAP_AT = 60


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def reversal(persona, row, tmp, tag):
    targets = ["mid"] * GAP_AT + ["high"] * GAP_AT
    hours = [0.25] * (2 * GAP_AT)
    hours[GAP_AT] = 8
    lock = [c == "1" for c in row["lock"]]
    flips = [u < NOISE for u in row["u"]]
    strand = os.path.join(tmp, "%s-%d.strand" % (tag, row["seed"]))
    return common.feedback_life(persona, row["seed"], strand, targets, flips, hours, lock=lock, no_pay=(GAP_AT,))


def rest_lines(strand):
    return sum(1 for line in common.read_jsonl(strand) if "rest" in line)


def inversions(pre, post):
    bad = 0
    for i in range(len(pre)):
        for j in range(len(pre)):
            if pre[i] > pre[j] and post[i] < post[j]:
                bad += 1
    return bad


def lineage(base_persona, tmp, tag):
    with open(base_persona, encoding="utf-8") as f:
        text = f.read()
    logw = [0.0, 0.0, 0.0]
    probe = None
    gen0 = None
    for row in common.stream("lineage"):
        p = os.path.join(tmp, "%s-gen%d.yaml" % (tag, row["generation"]))
        with open(p, "w", encoding="utf-8") as f:
            f.write(text.replace("    levels: [low, mid, high]\n",
                                 "    levels: [low, mid, high]\n    logw: %s\n" % json.dumps([common.r6(x) for x in logw]), 1))
        if gen0 is None:
            gen0 = probe_odds(p)
        n = len(row["u"])
        hours = [8 if (i and i % GAP_AT == 0) else 0.25 for i in range(n)]
        no_pay = tuple(i for i in range(n) if i and i % GAP_AT == 0)
        life = common.feedback_life(p, 0, os.path.join(tmp, "%s-gen%d.strand" % (tag, row["generation"])), ["mid"] * n,
                                    [u < NOISE for u in row["u"]], hours, no_pay=no_pay)
        delta = life.states()[-1]["learned"]["approach"]
        logw = [common.r6(a + d) for a, d in zip(logw, delta)]
    p = os.path.join(tmp, "%s-gen10.yaml" % tag)
    with open(p, "w", encoding="utf-8") as f:
        f.write(text.replace("    levels: [low, mid, high]\n",
                             "    levels: [low, mid, high]\n    logw: %s\n" % json.dumps(logw), 1))
    probe = probe_odds(p)
    return common.tv(gen0, probe)


def probe_odds(persona):
    code, docs, err = common.live(persona, [{"elapsed_hours": 0, "src": "env:host"}], seed=0)
    if code or not docs:
        raise common.SetupError("lineage probe failed: " + err[-200:])
    return docs[0]["stance"]["approach"]["odds"]


def metric(bars, tmp):
    b = bars["rest"]
    p, norest = common.fixture("rest.yaml"), common.fixture("variants/rest-norest.yaml")
    inv, top_changed, missing, n = 0, 0, 0, 0
    t_rest, t_none, recs = [], [], []
    for row in common.stream("reversal"):
        life = reversal(p, row, tmp, "rest")
        sts = life.states()
        pre, post = sts[GAP_AT - 1]["learned"]["approach"], sts[GAP_AT]["learned"]["approach"]
        inv += inversions(pre, post)
        top_changed += 1 if pre.index(max(pre)) != post.index(max(post)) else 0
        missing += 1 if rest_lines(life.strand) < 1 else 0
        n += 1
        t_rest.append(common.relearn(life.stances, GAP_AT, "high"))
        recs.append((life.events, life.stances))
        common.LIVES.setdefault("rest", []).append((p, row["seed"], life))
        t_none.append(common.relearn(reversal(norest, row, tmp, "norest").stances, GAP_AT, "high"))
    common.STANCES["r4"] = recs
    r = {"r0_inversions": inv, "r0_top_level_changed": top_changed, "r0_lives_without_a_rest_line": missing,
         "r1_median_turns": [common.median(t_rest), common.median(t_none)]}
    r["r1_ratio"] = round(r["r1_median_turns"][0] / r["r1_median_turns"][1], 4) if r["r1_median_turns"][1] else None
    r["r2_probe_distance"] = {"rest": round(lineage(p, tmp, "lin-rest"), 6), "none": round(lineage(norest, tmp, "lin-none"), 6)}
    r["pass"] = {
        "r0": inv == 0 and top_changed == 0 and missing == 0,
        "r1": r["r1_ratio"] is not None and r["r1_ratio"] <= b["r1_ratio_max"],
        "r2": r["r2_probe_distance"]["rest"] <= r["r2_probe_distance"]["none"],
    }
    r["scope"] = ("rest.yaml vs rest-norest.yaml; reversal stream seeds 0-199, 120 events, noise 0.10 (r0, r1); "
                  "lineage stream, 10 generations of 300 events, seed 0 (r2)")
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
