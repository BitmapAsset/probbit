"""Metric d: replay (Python 3.9, standard library only).

Every life of metric a is lived twice more from the same persona, seed and events: once in one run (a third copy) and once
split at a seeded event into two runs (`--state` plus the strand continued). Both must equal the original strand byte for
byte, and every strand must pass `live verify`. The P0 tutor strand recorded with the released 0.8.2 binary must verify,
and 20 committed events must append the recorded bytes.
"""
import os
from concurrent.futures import ThreadPoolExecutor

import common
import gen
import p0
import twins


def copies(tmp, life):
    """(differing bytes against the third copy, against the split copy, verify exit codes of the three strands)."""
    seed, room = life["seed"], life["room"]
    evs = twins.events(room)
    with open(life["strand"], "rb") as f:
        orig = f.read()
    tag = os.path.join(tmp, "d-s%d-%s" % (seed, room))
    st3, s3 = tag + "-3.state.json", tag + "-3.strand"
    common.init(twins.PERSONA, seed, st3)
    common.live(twins.PERSONA, evs, state=st3, strand=s3, checkpoint_every=twins.CHECKPOINT_EVERY)
    k = gen.rng_for("replay", str(seed), room).randrange(1, len(evs))
    stx, sx = tag + "-x.state.json", tag + "-x.strand"
    common.init(twins.PERSONA, seed, stx)
    common.live(twins.PERSONA, evs[:k], state=stx, strand=sx, checkpoint_every=twins.CHECKPOINT_EVERY)
    common.live(twins.PERSONA, evs[k:], state=stx, strand=sx, checkpoint_every=twins.CHECKPOINT_EVERY)
    out = []
    for path in (s3, sx):
        with open(path, "rb") as f:
            data = f.read()
        out.append(sum(1 for a, b in zip(orig, data) if a != b) + abs(len(orig) - len(data)))
    codes = [common.verify(p)[0] for p in (life["strand"], s3, sx)]
    return out[0], out[1], codes


def metric(lv, tmp, bars):
    with ThreadPoolExecutor(twins.WORKERS) as ex:
        res = list(ex.map(lambda life: copies(tmp, life), [lv[k] for k in sorted(lv)]))
    third = sum(r[0] for r in res)
    split = sum(r[1] for r in res)
    codes = [c for r in res for c in r[2]]
    sdir = os.path.join(p0.P0, "strands")
    vcode, vdoc = common.verify(os.path.join(sdir, "tutor-s0-0.strand"))
    code, appended, _ = p0.continue_tutor(os.path.join(sdir, "tutor-s0-0.strand"), os.path.join(sdir, "tutor-s0-0.state.json"),
                                          os.path.join(sdir, "tutor-continue.jsonl"))
    with open(os.path.join(sdir, "tutor-s0-0.appended"), "rb") as f:
        equal = f.read() == appended and code == 0
    r = {"scope": "metric a's 80 lives (third copy + split copy each); the P0 tutor strand (released 0.8.2)",
         "strands": len(codes), "differing_bytes_third_copy": third, "differing_bytes_split_copy": split,
         "verify_failures": sum(1 for c in codes if c != 0), "p0_strand_verifies": vcode == 0 and bool((vdoc or {}).get("ok")),
         "p0_appended_equal": equal}
    r["pass"] = {"d": third + split <= bars["d"]["differing_bytes_max"] and r["verify_failures"] <= bars["d"]["verify_failures_max"]
                 and r["p0_strand_verifies"] and equal == bars["d"]["p0_strand_appended_equal"]}
    return r
