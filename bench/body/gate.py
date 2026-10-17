"""P5 surprise (RFC 0002 Design B): bars s1, s2 and s3 (s4 in replay.py, s5 in p0.py).

    python3 gate.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

The flip stream: phase A praises `mid`, phase B praises `high` (T turns each, T 40, 60, 100); the feedback sign is flipped
where the seeded draw is below the noise (0, 0.10, 0.20). The same draws play gate.yaml, its fixed-rate twin and (bars v2)
the constant-gain twins variants/gate-g<g>.yaml (rate and step_cap times g: the gate's step with g held fixed), the latter on
seeds 1000-1049.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "surprise"
GAINS = (1.25, 1.5, 2.0, 2.5, 3.0)
FIXTURES = ["gate.yaml", "variants/gate-nosurprise.yaml"] + ["variants/gate-g%s.yaml" % g for g in GAINS]
NOISES = (0.0, 0.10, 0.20)
G_MAX, STEP_CAP, TOTAL_CAP = 3.0, 0.2, 1.0
GAIN_SEEDS = range(1000, 1050)
FIRST_TOL = 1e-5


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def first_step(life, row, noise, gated):
    """The learner's opening paid feedback (event 1 pays decision 0) against arith_0002.gate_first, and (v, r, g) when gated."""
    import arith_0002
    doc0, states = life.stances[0], life.states()
    odds = [doc0["stance"]["approach"]["odds"][l] for l in common.LEVELS]
    taken = common.LEVELS.index(doc0["stance"]["approach"]["level"])
    sign = 1 if (common.LEVELS[taken] == "mid") != (row["u"][0] < noise) else -1
    want, vrg = arith_0002.gate_first(sign, odds, taken, gated=gated)
    got = states[1]["learned"]["approach"]
    err = max(abs(x - y) for x, y in zip(got, want))
    if gated:
        s = states[1]["surprise"]["approach"]
        err = max([err] + [abs(s[k] - w) for k, w in zip(("v", "r", "g"), vrg)])
    return err


def one(persona, row, noise, tmp, tag, gated=False):
    T = row["T"]
    targets = ["mid"] * T + ["high"] * T
    flips = [u < noise for u in row["u"]]
    strand = os.path.join(tmp, "%s-%d-%d-%02d.strand" % (tag, row["seed"], T, int(noise * 100)))
    life = common.feedback_life(persona, row["seed"], strand, targets, flips, [1] * (2 * T))
    states = life.states()
    learned = [s["learned"]["approach"] for s in states]
    moved_a = sum(abs(learned[t][l] - learned[t - 1][l]) for t in range(1, T) for l in range(3))
    # the cap is on the step before recentring (docs/persona.md section 2.8): the recentred changes d = step - mean(step) come
    # from a step within +-cap exactly when max(d) - min(d) <= 2 x cap (v1 compared each recentred change with the cap, which
    # a correct step exceeds by up to a third)
    g_cap = G_MAX if gated else 1.0
    steps_ok = all(max(d) - min(d) <= 2 * g_cap * STEP_CAP + 1e-6 and all(abs(x) <= TOTAL_CAP + 1e-9 for x in learned[t])
                   for t in range(1, len(learned)) for d in [[learned[t][l] - learned[t - 1][l] for l in range(3)]])
    rl = common.relearn(life.stances, T, "high")
    return {"relearn": rl, "moved_a": moved_a, "steps_ok": steps_ok, "life": life, "end_a_mid": common.top(life.stances[T - 1]) == "mid",
            "relearned": rl < T, "first_err": first_step(life, row, noise, gated)}


def cell_stats(xs):
    return {"median_relearn": common.median([x["relearn"] for x in xs]), "moved_a": round(sum(x["moved_a"] for x in xs) / len(xs), 6),
            "end_a_mid": sum(1 for x in xs if x["end_a_mid"]) / float(len(xs)),
            "relearned": sum(1 for x in xs if x["relearned"]) / float(len(xs))}


def gate_cells(cells, b):
    """s1, s2 over the cells {key: {"gated": stats, "fixed": stats, "gains": {g: stats on GAIN_SEEDS}, "gated50": stats}}."""
    s1 = s2 = True
    for c in cells.values():
        g, f = c["gated"], c["fixed"]
        s1 = s1 and f["median_relearn"] > 0 and g["median_relearn"] / f["median_relearn"] <= b["s1_ratio_max"]
        # same movement budget: every constant gain that moves no more than the gate in phase A re-learns strictly slower
        # (a constant-gain learner put in the gate's place fails here: within its own budget it ties with itself)
        for st in c["gains"].values():
            if st["moved_a"] <= c["gated50"]["moved_a"]:
                s1 = s1 and st["median_relearn"] > c["gated50"]["median_relearn"]
        s2 = (s2 and f["moved_a"] > 0 and g["moved_a"] / f["moved_a"] <= b["s2_ratio_max"]
              and g["end_a_mid"] >= b["s2_adapt_min"] and g["relearned"] >= b["s2_adapt_min"])
    return s1, s2


def metric(bars, tmp):
    b = bars["surprise"]
    rows = common.stream("flip")
    gated_p = common.fixture("gate.yaml")
    fixed_p = common.fixture("variants/gate-nosurprise.yaml")
    r = {"cells": {}}
    steps_ok = steps_n = 0
    first_err = 0.0
    lives = []
    for T in (40, 60, 100):
        for noise in NOISES:
            rs = [row for row in rows if row["T"] == T]
            g = [one(gated_p, row, noise, tmp, "g", gated=True) for row in rs]
            f = [one(fixed_p, row, noise, tmp, "f") for row in rs]
            sub = [row for row in rs if row["seed"] in GAIN_SEEDS]
            gains = {}
            for k in GAINS:
                xs = [one(common.fixture("variants/gate-g%s.yaml" % k), row, noise, tmp, "c%s" % k) for row in sub]
                gains[str(k)] = cell_stats(xs)
            key = "T%d_noise%.2f" % (T, noise)
            r["cells"][key] = {"gated": cell_stats(g), "fixed": cell_stats(f), "gains": gains,
                               "gated50": cell_stats([x for row, x in zip(rs, g) if row["seed"] in GAIN_SEEDS])}
            steps_ok += sum(1 for x in g if x["steps_ok"])
            steps_n += len(g)
            first_err = max([first_err] + [x["first_err"] for x in g + f])
            lives += [(gated_p, row["seed"], x["life"]) for row, x in zip(rs, g)]
    common.LIVES.setdefault("surprise", []).extend(lives)
    r["s1_ratio"] = {k: round(c["gated"]["median_relearn"] / c["fixed"]["median_relearn"], 4) if c["fixed"]["median_relearn"] else None
                     for k, c in r["cells"].items()}
    r["s2_ratio"] = {k: round(c["gated"]["moved_a"] / c["fixed"]["moved_a"], 4) if c["fixed"]["moved_a"] else None
                     for k, c in r["cells"].items()}
    r["s3_lives_within_caps"] = "%d/%d" % (steps_ok, steps_n)
    r["s3_first_step_max_error"] = round(first_err, 9)
    s1, s2 = gate_cells(r["cells"], b)
    r["pass"] = {"s1": s1, "s2": s2, "s3": steps_ok == steps_n and first_err <= FIRST_TOL}
    r["scope"] = ("gate.yaml vs gate-nosurprise.yaml, flip stream, seeds 1000-1199, T 40/60/100, noise 0/0.10/0.20; constant "
                  "gains 1.25-3 on seeds 1000-1049")
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
