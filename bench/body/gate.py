"""P5 surprise (RFC 0002 Design B): bars s1, s2 and s3 (s4 in replay.py, s5 in p0.py).

    python3 gate.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

The flip stream: phase A praises `mid`, phase B praises `high` (T turns each, T 40, 60, 100); the feedback sign is flipped
where the seeded draw is below the noise (0, 0.10, 0.20). The same draws play gate.yaml and its fixed-rate twin.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "surprise"
FIXTURES = ["gate.yaml", "variants/gate-nosurprise.yaml"]
NOISES = (0.0, 0.10, 0.20)
G_MAX, STEP_CAP, TOTAL_CAP = 3.0, 0.2, 1.0


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def one(persona, row, noise, tmp, tag):
    T = row["T"]
    targets = ["mid"] * T + ["high"] * T
    flips = [u < noise for u in row["u"]]
    strand = os.path.join(tmp, "%s-%d-%d-%02d.strand" % (tag, row["seed"], T, int(noise * 100)))
    life = common.feedback_life(persona, row["seed"], strand, targets, flips, [1] * (2 * T))
    states = life.states()
    learned = [s["learned"]["approach"] for s in states]
    moved_a = sum(abs(learned[t][l] - learned[t - 1][l]) for t in range(1, T) for l in range(3))
    steps_ok = all(abs(learned[t][l] - learned[t - 1][l]) <= G_MAX * STEP_CAP + 1e-6 and abs(learned[t][l]) <= TOTAL_CAP + 1e-9
                   for t in range(1, len(learned)) for l in range(3))
    return {"relearn": common.relearn(life.stances, T, "high"), "moved_a": moved_a, "steps_ok": steps_ok, "life": life}


def metric(bars, tmp):
    b = bars["surprise"]
    rows = common.stream("flip")
    gated_p = common.fixture("gate.yaml")
    fixed_p = common.fixture("variants/gate-nosurprise.yaml")
    r = {"s1_ratio": {}, "s2_ratio": {}, "s1_median_turns": {}}
    steps_ok, steps_n = 0, 0
    lives = []
    for T in (40, 60, 100):
        for noise in NOISES:
            g = [one(gated_p, row, noise, tmp, "g") for row in rows if row["T"] == T]
            f = [one(fixed_p, row, noise, tmp, "f") for row in rows if row["T"] == T]
            key = "T%d_noise%.2f" % (T, noise)
            mg, mf = common.median([x["relearn"] for x in g]), common.median([x["relearn"] for x in f])
            r["s1_median_turns"][key] = [mg, mf]
            r["s1_ratio"][key] = round(mg / mf, 4) if mf else None
            ag = sum(x["moved_a"] for x in g) / len(g)
            af = sum(x["moved_a"] for x in f) / len(f)
            r["s2_ratio"][key] = round(ag / af, 4) if af else None
            steps_ok += sum(1 for x in g if x["steps_ok"])
            steps_n += len(g)
            lives += [(gated_p, row["seed"], x["life"]) for row, x in zip([w for w in rows if w["T"] == T], g)]
    common.LIVES.setdefault("surprise", []).extend(lives)
    r["s3_lives_within_caps"] = "%d/%d" % (steps_ok, steps_n)
    r["pass"] = {
        "s1": all(v is not None and v <= b["s1_ratio_max"] for v in r["s1_ratio"].values()),
        "s2": all(v is not None and v <= b["s2_ratio_max"] for v in r["s2_ratio"].values()),
        "s3": steps_ok == steps_n,
    }
    r["scope"] = "gate.yaml vs gate-nosurprise.yaml, flip stream, seeds 1000-1199, T 40/60/100, noise 0/0.10/0.20"
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
