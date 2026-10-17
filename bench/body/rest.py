"""P6 rest (RFC 0002 part 2): bars r0, r1 and r2 (r3 in replay.py, r4 in habits.py, r5 in p0.py).

    python3 rest.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

Reversal: 60 turns praising `mid` (0.25 h apart, so the pressure S passes the threshold), an 8 h idle gap (an event without
feedback, so the rest line is not deferred), then 59 turns praising `high`; feedback flipped where the draw is below 0.10;
lock turns carry no feedback. Lineage: RFC 0001's absorbing lineage, played by the host: each generation's learned weights
are added to the next generation's prior (`logw`), 10 generations of 300 rewarded events, an 8 h gap every 60 events.

Bars v2: r0 checks the exact contraction at the rest line, strict distinctions above 2 x prune, and a short-activity control
whose pressure stays below the threshold (no rest line); r1 adds retention of phase A's lesson at the stance right after the
gap; r1 runs on the reversal draws without lock turns (rule 2: the lock habit of r4 also delays re-learning, 6/11 instead of
5/11 for a faithful learner); r2 ends every generation with an idle event (the rest line precedes absorption) and reads
generations 1, 2 and 10.
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
RHO, PRUNE = 0.5, 0.05
SHORT_AT = 30              # r0 v2 control: 30 active events (29 x 0.25 h), pressure below the threshold at the gap
CONTROL_SEEDS = range(20)
LINEAGE_READ = (1, 2, 10)


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


def generation_persona(text, logw, path):
    with open(path, "w", encoding="utf-8") as f:
        f.write(text.replace("    levels: [low, mid, high]\n",
                             "    levels: [low, mid, high]\n    logw: %s\n" % json.dumps([common.r6(x) for x in logw]), 1))
    return path


def lineage(base_persona, tmp, tag):
    """Probe distance from generation 0 after generations 1, 2 and 10. Each generation: 300 rewarded events, an 8 h gap every
    60, then one closing idle event (8 h, no feedback) before its learned weights are absorbed into the next prior."""
    with open(base_persona, encoding="utf-8") as f:
        text = f.read()
    logw = [0.0, 0.0, 0.0]
    gen0 = probe_odds(generation_persona(text, logw, os.path.join(tmp, "%s-gen0.yaml" % tag)))
    out = {}
    for row in common.stream("lineage"):
        p = generation_persona(text, logw, os.path.join(tmp, "%s-gen%d.yaml" % (tag, row["generation"])))
        n = len(row["u"]) + 1
        hours = [8 if (i and i % GAP_AT == 0) else 0.25 for i in range(n - 1)] + [8]
        no_pay = tuple(i for i in range(n) if i and i % GAP_AT == 0) + (n - 1,)
        life = common.feedback_life(p, 0, os.path.join(tmp, "%s-gen%d.strand" % (tag, row["generation"])), ["mid"] * n,
                                    [u < NOISE for u in row["u"]] + [False], hours, no_pay=no_pay)
        delta = life.states()[-1]["learned"]["approach"]
        logw = [common.r6(a + d) for a, d in zip(logw, delta)]
        k = row["generation"] + 1
        if k in LINEAGE_READ:
            probe = probe_odds(generation_persona(text, logw, os.path.join(tmp, "%s-gen%d-probe.yaml" % (tag, k))))
            out[str(k)] = round(common.tv(gen0, probe), 6)
    return out


def contraction_error(pre, post):
    want = [common.r6(RHO * x) for x in pre]
    want = [0.0 if abs(x) < PRUNE else x for x in want]
    return max(abs(a - b) for a, b in zip(post, want))


def lost_distinctions(pre, post):
    """Pairs ordered strictly before the rest line, at least one of which survives the prune (|rho x| >= prune), that are not
    strictly ordered the same way after it (two weights both inside the prune band may meet at 0: that is the prune)."""
    bad = 0
    for i in range(len(pre)):
        for j in range(len(pre)):
            survives = abs(common.r6(RHO * pre[i])) >= PRUNE or abs(common.r6(RHO * pre[j])) >= PRUNE
            if pre[i] > pre[j] and survives and not post[i] > post[j]:
                bad += 1
    return bad


def short_control(persona, row, tmp):
    """r0 v2: 30 events at 0.25 h praising `mid`, then the 8 h gap without feedback: pressure below the threshold."""
    targets = ["mid"] * (SHORT_AT + 1)
    hours = [0.25] * SHORT_AT + [8]
    flips = [u < NOISE for u in row["u"][:SHORT_AT]] + [False]
    life = common.feedback_life(persona, row["seed"], os.path.join(tmp, "short-%d.strand" % row["seed"]), targets, flips, hours,
                                no_pay=(SHORT_AT,))
    sts = life.states()
    return rest_lines(life.strand), max(abs(a - b) for a, b in zip(sts[SHORT_AT - 1]["learned"]["approach"],
                                                                    sts[SHORT_AT]["learned"]["approach"]))


def probe_odds(persona):
    code, docs, err = common.live(persona, [{"elapsed_hours": 0, "src": "env:host"}], seed=0)
    if code or not docs:
        raise common.SetupError("lineage probe failed: " + err[-200:])
    return docs[0]["stance"]["approach"]["odds"]


def metric(bars, tmp):
    b = bars["rest"]
    p, norest = common.fixture("rest.yaml"), common.fixture("variants/rest-norest.yaml")
    inv, top_changed, missing, n, contr, lost = 0, 0, 0, 0, 0.0, 0
    t_rest, t_none, recs, states, kept, kept_n = [], [], [], [], 0, 0
    t_rest_free, t_none_free = [], []
    for row in common.stream("reversal"):
        life = reversal(p, row, tmp, "rest")
        sts = life.states()
        pre, post = sts[GAP_AT - 1]["learned"]["approach"], sts[GAP_AT]["learned"]["approach"]
        inv += inversions(pre, post)
        top_changed += 1 if pre.index(max(pre)) != post.index(max(post)) else 0
        missing += 1 if rest_lines(life.strand) < 1 else 0
        contr = max(contr, contraction_error(pre, post))
        lost += lost_distinctions(pre, post)
        n += 1
        t_rest.append(common.relearn(life.stances, GAP_AT, "high"))
        recs.append((life.events, life.stances))
        states.append(sts)
        common.LIVES.setdefault("rest", []).append((p, row["seed"], life))
        t_none.append(common.relearn(reversal(norest, row, tmp, "norest").stances, GAP_AT, "high"))
        # r1 v2: the same draws without lock turns (rule 2: r4's lock habit also delays re-learning)
        free = dict(row, lock="0" * len(row["lock"]))
        lf = reversal(p, free, tmp, "rest-free")
        t_rest_free.append(common.relearn(lf.stances, GAP_AT, "high"))
        t_none_free.append(common.relearn(reversal(norest, free, tmp, "norest-free").stances, GAP_AT, "high"))
        odds = lf.stances[GAP_AT]["stance"]["approach"]["odds"]
        kept += 1 if odds["mid"] > max(odds["low"], odds["high"]) else 0
        kept_n += 1
    common.STANCES["r4"] = recs
    common.STATES["r4"] = states
    ctl = [short_control(p, row, tmp) for row in common.stream("reversal") if row["seed"] in CONTROL_SEEDS]
    r = {"r0_inversions": inv, "r0_top_level_changed": top_changed, "r0_lives_without_a_rest_line": missing,
         "r0_contraction_max_error": round(contr, 9), "r0_lost_distinctions": lost,
         "r0_control_rest_lines": sum(c[0] for c in ctl), "r0_control_max_change": round(max(c[1] for c in ctl), 9),
         "r1_median_turns": [common.median(t_rest_free), common.median(t_none_free)],
         "r1_median_turns_with_lock_turns": [common.median(t_rest), common.median(t_none)],
         "r1_retained": "%d/%d" % (kept, kept_n), "r1_retained_share": kept / float(kept_n) if kept_n else 0.0}
    r["r1_ratio"] = round(r["r1_median_turns"][0] / r["r1_median_turns"][1], 4) if r["r1_median_turns"][1] else None
    lk = r["r1_median_turns_with_lock_turns"]
    r["r1_ratio_with_lock_turns"] = round(lk[0] / lk[1], 4) if lk[1] else None   # reported (v1's measurement), not gated
    r["r2_probe_distance"] = {"rest": lineage(p, tmp, "lin-rest"), "none": lineage(norest, tmp, "lin-none")}
    r["pass"] = {
        "r0": gate_r0(r),
        "r1": r["r1_ratio"] is not None and r["r1_ratio"] <= b["r1_ratio_max"] and r["r1_retained_share"] >= b["r1_retained_min"],
        "r2": gate_r2(r["r2_probe_distance"], b),
    }
    r["scope"] = ("rest.yaml vs rest-norest.yaml; reversal stream seeds 0-199, 120 events, noise 0.10 (r0, r4's lives), the "
                  "same draws without lock turns (r1), its seeds 0-19 cut to 30 events for the r0 control; lineage stream, 10 "
                  "generations of 300 events + 1 idle event, seed 0 (r2)")
    return r


def gate_r0(r):
    return (r["r0_inversions"] == 0 and r["r0_top_level_changed"] == 0 and r["r0_lives_without_a_rest_line"] == 0
            and r["r0_contraction_max_error"] <= 1e-6 and r["r0_lost_distinctions"] == 0
            and r["r0_control_rest_lines"] == 0 and r["r0_control_max_change"] == 0)


def gate_r2(d, b):
    rest, none = d["rest"], d["none"]
    return (none["1"] >= b["r2_baseline_min"] and all(rest[k] <= b["r2_reduction_max"] * none[k] for k in ("1", "2"))
            and rest["10"] <= none["10"])


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
