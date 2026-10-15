"""Habits stay rules (RFC 0002 Invariants, proof coverage): bars n5, o4 and r4, checked by an independent Python reading of
each stance against the event that carried the habit's flag.

    python3 habits.py    # runs the parts it reads, then checks; exit 0 every gated bar passes, 1 one fails, 2 setup error

n5: needs2's quiet_hours (a quiet event's stance never pursues `report`); o4: optimism's busy_other (a busy event's stance
pursues `other`); r4: rest's locked (a lock event's stance takes `low`). The habit-free twins are the reported nulls.
"""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

RULES = {
    "n5": ("quiet", lambda d: (d.get("pursue") or {}).get("goal") != "report"),
    "o4": ("busy", lambda d: (d.get("pursue") or {}).get("goal") == "other"),
    "r4": ("lock", lambda d: d["stance"]["approach"]["level"] == "low"),
}


def check():
    import dispositions, needs, rest
    for m in (needs, dispositions, rest):
        m.check()


def count(recs, flag, ok):
    bound = breaks = 0
    for events, stances in recs:
        for ev, doc in zip(events, stances):
            if ev.get(flag):
                bound += 1
                breaks += 0 if ok(doc) else 1
    return bound, breaks


def nulls(tmp, present):
    import needs, rest
    out = {}
    if "n5" in present:
        lives = needs.fortnight_lives(common.fixture("variants/needs2-nohabit.yaml"), common.stream("fortnight"), tmp, "nohabit")
        out["n5"] = count([(lv["life"].events, lv["life"].stances) for lv in lives], *RULES["n5"])[1]
    if "o4" in present:
        recs = []
        for row in common.stream("pairs"):
            for s in row["pair"]:
                code, docs, _ = common.live(common.fixture("variants/optimism-nohabit.yaml"), row["events"], seed=s)
                recs.append((row["events"], docs))
        out["o4"] = count(recs, *RULES["o4"])[1]
    if "r4" in present:
        recs = []
        for row in common.stream("reversal"):
            life = rest.reversal(common.fixture("variants/rest-nohabit.yaml"), row, tmp, "nohabit")
            recs.append((life.events, life.stances))
        out["r4"] = count(recs, *RULES["r4"])[1]
    return out


def metric(bars, tmp):
    b = bars["habits"]
    r, gates = {}, {}
    present = [bar for bar in RULES if bar in common.STANCES]
    null = nulls(tmp, present)
    for bar, (flag, ok) in sorted(RULES.items()):
        if bar not in present:
            continue   # the part behind this bar did not run
        bound, breaks = count(common.STANCES.get(bar, []), flag, ok)
        r[bar] = {"bound": bound, "breaks": breaks, "habit_free_twin_breaks": null[bar]}
        gates[bar] = breaks == 0 and bound >= b.get(bar + "_bound_min", 0)
    r["pass"] = gates
    r["scope"] = "fortnight seeds 0-99 (n5), pairs stream 20 pairs (o4), reversal seeds 0-199 (r4); nulls: the habit-free twins"
    return r


def main(argv):
    import dispositions, needs, rest
    tmp = common.tmpdir()
    try:
        common.binary()
        check()
        b = common.bars()
        for m in (needs, dispositions, rest):
            m.metric(b, tmp)
        res = metric(b, tmp)
    except common.SetupError as e:
        common.fail(str(e))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    print(common.canon(res))
    return 0 if all(res["pass"].values()) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
