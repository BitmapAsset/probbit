"""Metric a: twins distinguishability (Python 3.9, standard library only).

Individuals 0-39 each live room A and room B (60 events), then a month away and 8 calm events; 20 probes are then answered
by `persona turn` from the saved post-life state, so probes never see each other. The host action is the caution level.
"""
import os
import shutil
import tempfile
from concurrent.futures import ThreadPoolExecutor

import common
from fixtures import lives

PERSONA = os.path.join(common.FIXTURES, "sensed.yaml")
SEEDS = list(range(40))
ROOMS = ["A", "B"]
CHECKPOINT_EVERY = 10
WORKERS = 4


def events(room):
    return lives.load("room-" + room.lower()) + lives.load("tail")


def one_life(tmp, persona, seed, room):
    """One life: (stances, strand path, final state path, probe documents)."""
    st = os.path.join(tmp, "s%d-%s.state.json" % (seed, room))
    strand = os.path.join(tmp, "s%d-%s.strand" % (seed, room))
    common.init(persona, seed, st)
    code, docs, err = common.live(persona, events(room), state=st, strand=strand, checkpoint_every=CHECKPOINT_EVERY)
    if code != 0:
        raise common.SetupError("live s%d %s: %s" % (seed, room, err.strip()[-300:]))
    probes = []
    out = os.path.join(tmp, "s%d-%s.probe.json" % (seed, room))
    for p in lives.load("probes"):
        code, doc, err = common.turn(persona, st, p, out)
        if code != 0:
            raise common.SetupError("probe s%d %s: %s" % (seed, room, err.strip()[-300:]))
        probes.append(doc)
    return {"seed": seed, "room": room, "stances": docs, "strand": strand, "state": st, "probes": probes}


def run_lives(tmp, persona=PERSONA, seeds=SEEDS):
    with ThreadPoolExecutor(WORKERS) as ex:
        futs = {(s, r): ex.submit(one_life, tmp, persona, s, r) for s in seeds for r in ROOMS}
        return {k: f.result() for k, f in futs.items()}


def action(doc):
    return doc["stance"]["caution"]["level"]


def traits(doc):
    return sorted(doc["stance"])


def compare(pa, pb):
    """(share of probes whose host action differs, share of (probe, trait) levels that differ, mean TV over probes and traits)"""
    d = sum(action(a) != action(b) for a, b in zip(pa, pb)) / len(pa)
    cells = [(a["stance"][t], b["stance"][t]) for a, b in zip(pa, pb) for t in traits(a)]
    lvl = sum(x["level"] != y["level"] for x, y in cells) / len(cells)
    tv = sum(common.tv(x["odds"], y["odds"]) for x, y in cells) / len(cells)
    return d, lvl, tv


def metric(lv, bars):
    d_life, l_life, tv_life = [], [], []
    for s in SEEDS:
        d, lvl, tv = compare(lv[(s, "A")]["probes"], lv[(s, "B")]["probes"])
        d_life.append(d)
        l_life.append(lvl)
        tv_life.append(tv)
    seed_pairs = {}
    for room in ROOMS:
        ds, tvs = [], []
        for i in range(0, len(SEEDS), 2):
            d, _, tv = compare(lv[(SEEDS[i], room)]["probes"], lv[(SEEDS[i + 1], room)]["probes"])
            ds.append(d)
            tvs.append(tv)
        seed_pairs[room] = (sum(ds) / len(ds), sum(tvs) / len(tvs))
    mean = lambda xs: sum(xs) / len(xs)  # noqa: E731
    lo, hi = common.bootstrap(d_life)
    tlo, thi = common.bootstrap(tv_life)
    r = {
        "scope": "fixtures/sensed.yaml, individuals 0-39, rooms A and B (60 events + 720 h + 8 calm), 20 probes each",
        "d_life": common.r6(mean(d_life)), "d_life_ci95": [common.r6(lo), common.r6(hi)],
        "l_life": common.r6(mean(l_life)), "tv_life": common.r6(mean(tv_life)), "tv_life_ci95": [common.r6(tlo), common.r6(thi)],
        "d_seed_A": common.r6(seed_pairs["A"][0]), "tv_seed_A": common.r6(seed_pairs["A"][1]),
        "d_seed_B": common.r6(seed_pairs["B"][0]), "tv_seed_B": common.r6(seed_pairs["B"][1]),
        "tv_seed_B_over_A": common.r6(seed_pairs["B"][1] / seed_pairs["A"][1]) if seed_pairs["A"][1] else None,
    }
    a1 = r["d_life"] >= bars["a1"]["d_life_mean_min"] and r["d_life_ci95"][0] >= bars["a1"]["d_life_ci_low_min"]
    a2 = r["tv_life"] >= min(r["tv_seed_A"], r["tv_seed_B"])
    r["pass"] = {"a1": a1, "a2": a2}
    return r


if __name__ == "__main__":
    import json
    tmp = tempfile.mkdtemp(prefix="pbit-twins-")
    try:
        print(json.dumps(metric(run_lives(tmp), json.load(open(os.path.join(common.FIXTURES, "bars.json")))), indent=1))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
