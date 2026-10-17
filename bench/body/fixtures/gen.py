"""Seeded generators for bench/body (Python 3.9, standard library only).

    python3 gen.py            # write streams/*.jsonl and variants/*.yaml
    python3 gen.py --verify   # regenerate in memory and compare with the committed files byte for byte (exit 1 on a diff)

Every output is a pure function of this file and the fixture personas: the seeds are fixed below, each generator draws from
`random.Random(1000 + seed)`, and every drawn number is rounded before it is written. The outputs are committed, and
MANIFEST records their sha256, so a changed generator shows as a changed fixture.

Streams that a scripted host plays turn by turn (fortnight, absent, flip, reversal, lineage) commit the host's seeded draws,
not events: the events depend on the stance the host reads, and the per-part scripts build them from these draws.
"""
import json
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
STREAMS = os.path.join(HERE, "streams")
VARIANTS = os.path.join(HERE, "variants")

# Seeds (README "Streams and seeds")
FORTNIGHT_SEEDS = range(0, 100)
FLIP_SEEDS = range(1000, 1200)
FLIP_T = (40, 60, 100)
REVERSAL_SEEDS = range(0, 200)
PAIRS = [(2 * k, 2 * k + 1) for k in range(20)]
TWO_POINT_SEED = 0
BLOCKS_SEED = 0
LINEAGE_SEED = 0
PATTERNS_SEED = 0


def rng(seed):
    return random.Random(1000 + seed)


def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"))


def r6(x):
    v = round(x, 6)
    return 0.0 if v == 0 else v


def bits(flags):
    return "".join("1" if f else "0" for f in flags)


# ---------------------------------------------------------------- needs2: fortnight, absent and the probe grid
def quiet(hour):
    """Quiet hours 23:00-08:00; hour 0 of a stream is 00:00 of day 1."""
    h = hour % 24
    return h >= 23 or h < 8


def fortnight(seed, days=14, absent_from_day=None, absent_until_day=None):
    """Per hour: quiet, the `tokens` reading (waking hours, uniform 0-0.12), the answer draw (p 0.5) for a report sent the hour
    before, the unprompted-message draw (p 0.15, waking hours). Days from `absent_from_day` (0-based) up to (not including)
    `absent_until_day` have no answers and no principal messages."""
    g = rng(seed)
    quiet_s, tokens, answer, unprompted = [], [], [], []
    for hour in range(24 * days):
        q = quiet(hour)
        t = r6(g.uniform(0.0, 0.12))
        a = g.random() < 0.5
        u = g.random() < 0.15
        away = (absent_from_day is not None and hour // 24 >= absent_from_day
                and (absent_until_day is None or hour // 24 < absent_until_day))
        quiet_s.append(q)
        tokens.append(0.0 if q else t)
        answer.append(a and not away)
        unprompted.append(u and not q and not away)
    return {"seed": seed, "quiet": bits(quiet_s), "tokens": tokens, "answer": bits(answer), "unprompted": bits(unprompted)}


def gen_fortnight():
    return [fortnight(s) for s in FORTNIGHT_SEEDS]


def gen_absent():
    # the fortnight's opening 2 days (the same draws), then 5 days with no principal messages and no answers
    return [fortnight(s, days=7, absent_from_day=2) for s in FORTNIGHT_SEEDS]


def gen_return():
    # bars v2 (n11): the absent stream's 7 days (the same draws), then 3 days with the principal back
    return [fortnight(s, days=10, absent_from_day=2, absent_until_day=7) for s in FORTNIGHT_SEEDS]


def gen_probe():
    """The warm-up event, then 71 probe events, each played from the saved warm-up state."""
    def reduction(xs, i, k):
        def drive(v):
            return sum(x ** 4.0 for x in v) ** (1.0 / 3.0)
        after = list(xs)
        after[i] = abs(xs[i] - k)
        return drive(xs) - drive(after)
    rows = [{"warmup": {"goals": {"compact": {"win": 1}, "report": {"win": 1}}, "src": "env:host"}}]
    for a in range(1, 10):
        for b in range(1, 10):
            if a == b:
                continue
            xc, xt = a / 10.0, b / 10.0
            if abs(reduction([xc, xt], 0, 0.6) - reduction([xc, xt], 1, 0.8)) < 1e-6:
                continue   # a tie: (0.3, 0.4), both reductions 0
            ev = {"elapsed_hours": r6(xt / 0.04), "senses": {"tokens": xc}, "src": "env:host"}
            rows.append({"probe": [xc, xt], "event": ev})
    return rows


# ---------------------------------------------------------------- gate and rest: flip and reversal draws
def gen_flip():
    rows = []
    for s in FLIP_SEEDS:
        for T in FLIP_T:
            g = rng(s * 1000 + T)
            rows.append({"seed": s, "T": T, "u": [round(g.random(), 4) for _ in range(2 * T)]})
    return rows


def gen_reversal():
    rows = []
    for s in REVERSAL_SEEDS:
        g = rng(s)
        u = [round(g.random(), 4) for _ in range(120)]
        lock = [g.random() < 0.1 for _ in range(120)]
        rows.append({"seed": s, "u": u, "lock": bits(lock)})
    return rows


def gen_lineage():
    g = rng(LINEAGE_SEED)
    return [{"generation": k, "u": [round(g.random(), 4) for _ in range(300)]} for k in range(10)]


# ---------------------------------------------------------------- pace: blocks and the loop
def gen_blocks():
    """2,000 hourly events in 50-hour blocks; each block's reward rate is drawn from 0.3, 1.0, 2.0; each hour pays a win of
    2 x rate with p 0.5 (mean: the rate), on goal `task` from `env`."""
    g = rng(BLOCKS_SEED)
    rows = []
    for b in range(40):
        rate = g.choice((0.3, 1.0, 2.0))
        for _ in range(50):
            ev = {"elapsed_hours": 1, "src": "env:host"}
            if g.random() < 0.5:
                ev["goals"] = {"task": {"win": r6(2 * rate)}}
            rows.append({"rate": rate, "event": ev})
    return rows


def gen_loop():
    """The scripted host's parameters (no draws: the loop is deterministic)."""
    return [{"base": 4, "r0": 1, "p": 0.3, "hours": 400, "start_above_r0": 0.2, "kappa": k, "a_cap": a}
            for a in (12, 20) for k in (0.2, 0.5, 0.7, 0.9, 1.0)]


def gen_bursts():
    """Bars v2 (p1c), no draws: three streams of 600 hours with the same average reward, 0.5 an hour, on goal `task` from
    `env`: steady (a win of 0.5 every hour), bursty (a win of 4 every 8th hour, an event every hour) and spaced (an event
    every 3 hours, each a win of 1.5)."""
    steady = [{"elapsed_hours": 0 if i == 0 else 1, "goals": {"task": {"win": 0.5}}, "src": "env:host"} for i in range(600)]
    bursty = []
    for i in range(600):
        ev = {"elapsed_hours": 0 if i == 0 else 1, "src": "env:host"}
        if i % 8 == 7:
            ev["goals"] = {"task": {"win": 4}}
        bursty.append(ev)
    spaced = [{"elapsed_hours": 0 if i == 0 else 3, "goals": {"task": {"win": 1.5}}, "src": "env:host"} for i in range(200)]
    return [{"name": "steady", "events": steady}, {"name": "bursty", "events": bursty}, {"name": "spaced", "events": spaced}]


def gen_burst():
    """Bars v2 (p6), no draws: an admissible burst that drives an uncapped R-bar above the cap: 12 events at elapsed 0, each
    paying a win of 4 (win_max), then 24 quiet hourly events (the recovery from the cap)."""
    return [{"burst_events": 12, "win": 4, "recovery_hours": 24}]


# ---------------------------------------------------------------- dispositions: two-point and pairs
def gen_two_point():
    g = rng(TWO_POINT_SEED)
    return [{"wins": "".join("b" if g.random() < 0.5 else "a" for _ in range(20000)), "a": 0.5, "b": 1.5}]


def gen_two_point_gaps():
    """Bars v2 (o1): 3,000 wins of 0.5 or 1.5 at p 0.5 with elapsed hours uniform 0.5-3, so E's 48 h half-life acts."""
    g = rng(TWO_POINT_SEED + 1)
    return [{"wins": [[r6(g.uniform(0.5, 3.0)), 1.5 if g.random() < 0.5 else 0.5] for _ in range(3000)]}]


def gen_pairs():
    rows = []
    for k, pair in enumerate(PAIRS):
        g = rng(k)
        evs = []
        for _ in range(300):
            ev = {"elapsed_hours": r6(g.uniform(0.5, 3.0)), "src": "env:host"}
            sig = {}
            x = g.random()
            if x < 0.3:
                sig["win"] = r6(g.uniform(0.2, 2.0))
            elif x < 0.45:
                sig["setback"] = r6(g.uniform(0.1, 1.0))
            if g.random() < 0.3:
                sig["cue"] = True
            if sig:
                ev["goals"] = {"task": sig}
            if g.random() < 0.2:
                ev["busy"] = True
            evs.append(ev)
        rows.append({"pair": list(pair), "events": evs})
    return rows


# ---------------------------------------------------------------- load: three patterns
def gen_patterns():
    crit = [{"elapsed_hours": 6, "criticism": True, "src": "human:principal"} for _ in range(300)]
    fail = [{"elapsed_hours": 1, "senses": {"fail": True}, "src": "env:host"} for _ in range(300)]
    hold = []
    for i in range(300):
        ev = {"elapsed_hours": 1, "src": "env:host"}
        if i < 48:
            ev["strain_cue"] = True
        hold.append(ev)
    return [{"pattern": "criticism", "events": crit}, {"pattern": "failure", "events": fail},
            {"pattern": "held_mood", "events": hold}]


STREAM_GENS = {
    "fortnight": gen_fortnight, "absent": gen_absent, "probe": gen_probe, "flip": gen_flip, "blocks": gen_blocks,
    "loop": gen_loop, "two-point": gen_two_point, "pairs": gen_pairs, "reversal": gen_reversal, "lineage": gen_lineage,
    "patterns": gen_patterns,
    # bars v2
    "return": gen_return, "bursts": gen_bursts, "burst": gen_burst, "two-point-gaps": gen_two_point_gaps,
}
GAINS = (1.25, 1.5, 2.0, 2.5, 3.0)   # bars v2 (s1, s2): the constant-gain learners


# ---------------------------------------------------------------- variant personas, derived from the frozen fixtures
def read(name):
    with open(os.path.join(HERE, name), encoding="utf-8") as f:
        return f.read()


def drop_block(text, key, indent=""):
    """Remove the block `key:` at `indent` and its more-indented lines (exactly one occurrence)."""
    lines = text.split("\n")
    out, i, hit = [], 0, 0
    while i < len(lines):
        line = lines[i]
        if line.startswith(indent + key + ":"):
            hit += 1
            i += 1
            while i < len(lines) and lines[i].startswith(indent + " "):
                i += 1
            continue
        out.append(line)
        i += 1
    assert hit == 1, (key, hit)
    return "\n".join(out)


def drop_lines(text, needle, count):
    lines = text.split("\n")
    out = [l for l in lines if needle not in l]
    assert len(lines) - len(out) == count, (needle, len(lines) - len(out))
    return "\n".join(out)


def sub_once(text, old, new):
    assert text.count(old) == 1, (old, text.count(old))
    return text.replace(old, new)


def strip_comment(text):
    lines = text.split("\n")
    k = 0
    while k < len(lines) and lines[k].startswith("#"):
        k += 1
    return "\n".join(lines[k:])


def variant(base, comment, *ops):
    text = strip_comment(read(base))
    for op in ops:
        text = op(text)
    return "".join("# " + c + "\n" for c in comment) + text


def gen_variants():
    frozen = "Derived by ../gen.py from the frozen fixture; recorded in MANIFEST."
    v = {}
    v["needs1-noblock.yaml"] = variant("needs1.yaml", ["needs1.yaml without the needs block and expect_by_hours (P0 copy).", frozen],
                                       lambda t: drop_block(t, "needs"), lambda t: drop_lines(t, "expect_by_hours", 1))
    v["needs2-noblock.yaml"] = variant("needs2.yaml", ["needs2.yaml without the needs block and expect_by_hours (P0 copy).", frozen],
                                       lambda t: drop_block(t, "needs"), lambda t: drop_lines(t, "expect_by_hours", 2))
    v["needs2-tau2.yaml"] = variant("needs2.yaml", ["needs2.yaml with the context need's horizon at 2 h (bar n10).", frozen],
                                    lambda t: sub_once(t, "      bind: compact\n      horizon_hours: 0",
                                                       "      bind: compact\n      horizon_hours: 2"))
    v["needs2-nohabit.yaml"] = variant("needs2.yaml", ["needs2.yaml without the habit quiet_hours (the null of n5).", frozen],
                                       lambda t: drop_block(t, "habits"))
    v["needs2-window1e6.yaml"] = variant("needs2.yaml", ["needs2.yaml with report's window at 1e6 h, a window that never fires "
                                                         "inside 7 days (the null of n11).", frozen],
                                         lambda t: sub_once(t, "    - id: report\n      interest: 0.6\n      expect_by_hours: 1",
                                                            "    - id: report\n      interest: 0.6\n      expect_by_hours: 1000000"))
    v["gate-nosurprise.yaml"] = variant("gate.yaml", ["gate.yaml without the surprise block: the fixed rate (s1, s2; P0 copy).", frozen],
                                        lambda t: drop_block(t, "surprise"))
    v["pace-nopace.yaml"] = variant("pace.yaml", ["pace.yaml without the pace block (P0 copy).", frozen],
                                    lambda t: drop_block(t, "pace"))
    v["optimism-noblock.yaml"] = variant("optimism.yaml", ["optimism.yaml without the dispositions block (o5, P0 copy).", frozen],
                                         lambda t: drop_block(t, "dispositions"))
    v["optimism-nohabit.yaml"] = variant("optimism.yaml", ["optimism.yaml without the habit busy_other (the null of o4).", frozen],
                                         lambda t: drop_block(t, "habits"))
    v["valence-noblock.yaml"] = variant("valence.yaml", ["valence.yaml with today's effects.surprise on the mood instead of "
                                                         "drives.valence, and no expect_by_hours (nulls of v1, v2, om1; P0 copy).", frozen],
                                        lambda t: sub_once(t, "  valence: {mood: valence, expect: 0.2, surprise: 0.6}",
                                                           "  effects:\n    surprise: {valence: 0.6}"),
                                        lambda t: drop_lines(t, "expect_by_hours", 1))
    v["rest-norest.yaml"] = variant("rest.yaml", ["rest.yaml without the rest block (r1, r2; P0 copy).", frozen],
                                    lambda t: drop_block(t, "rest"))
    v["rest-nohabit.yaml"] = variant("rest.yaml", ["rest.yaml without the habit locked (the null of r4).", frozen],
                                     lambda t: drop_block(t, "habits"))
    v["load-noload.yaml"] = variant("load.yaml", ["load.yaml without the load block (the null of l1; P0 copy).", frozen],
                                    lambda t: drop_block(t, "load"))
    # bars v2
    v["needs2-s02.yaml"] = variant("needs2.yaml", ["needs2.yaml with success.init 0.2, so warm-up wins on one goal set the two "
                                                   "goals' success rates apart (bar n3, v2).", frozen],
                                   lambda t: sub_once(t, "  success: {rate: 0.2, init: 1}", "  success: {rate: 0.2, init: 0.2}"))
    for g in GAINS:
        v["gate-g%s.yaml" % g] = variant("gate.yaml", ["gate.yaml without surprise, rate and step_cap times a constant gain %s: "
                                                       "the gate's step with g fixed (s1, s2 v2 nulls)." % g, frozen],
                                         lambda t: drop_block(t, "surprise"),
                                         lambda t, g=g: sub_once(t, "  rate: 0.5\n", "  rate: %s\n" % r6(0.5 * g)),
                                         lambda t, g=g: sub_once(t, "  step_cap: 0.2\n", "  step_cap: %s\n" % r6(0.2 * g)))
    v["load-effects.yaml"] = variant("load.yaml", ["load.yaml with a load effect on caution (bar l1 v2: the load reaches a decision).",
                                                   frozen],
                                     lambda t: sub_once(t, "  hold: {mood: strain, above: 1, per_hour: 0.01}",
                                                        "  hold: {mood: strain, above: 1, per_hour: 0.01}\n  effects: {caution: 1}"))
    return v


def outputs():
    """Every generated file: relative path -> bytes."""
    out = {}
    for name, fn in STREAM_GENS.items():
        out["streams/%s.jsonl" % name] = "".join(canon(r) + "\n" for r in fn()).encode("utf-8")
    for name, text in gen_variants().items():
        out["variants/" + name] = text.encode("utf-8")
    return out


def main(argv):
    out = outputs()
    if "--verify" in argv:
        bad = []
        for rel, data in sorted(out.items()):
            path = os.path.join(HERE, rel)
            if not os.path.exists(path):
                bad.append(rel + " (missing)")
                continue
            with open(path, "rb") as f:
                if f.read().replace(b"\r\n", b"\n") != data:
                    bad.append(rel)
        print("gen.py --verify: %d files regenerated, %d differ%s" % (len(out), len(bad), (": " + ", ".join(bad)) if bad else ""))
        return 1 if bad else 0
    os.makedirs(STREAMS, exist_ok=True)
    os.makedirs(VARIANTS, exist_ok=True)
    for rel, data in sorted(out.items()):
        with open(os.path.join(HERE, rel), "wb") as f:
            f.write(data)
    print("gen.py: wrote %d files" % len(out))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
