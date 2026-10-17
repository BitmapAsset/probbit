"""Replay (RFC 0002 Invariants): bars n4, s4, r3 and l4. Every strand the parts wrote this run is verified, re-run as a
third copy and as a split copy (two `live` runs continuing one state and strand); pace's strands count under n4 (README
"P2"), and the dispositions and valence strands are reported.

    python3 replay.py    # runs the parts, then replays; exit 0 every gated bar passes, 1 one fails, 2 setup error

Bars v2 gate the null each replay bar names: a copy of the bar's earliest strand with one input changed, one with a line removed
and one with two lines swapped must each be refused by `live verify` (a verifier that always succeeds fails the bar).
"""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BARS = {"n4": ["needs", "pace"], "s4": ["surprise"], "r3": ["rest"], "l4": ["load"]}
REPORTED = ["dispositions", "valence"]
CHANGE = {"n4": ('"amount":0.6', '"amount":0.5'), "s4": ('"praise":true', '"criticism":true'),
          "r3": ('"praise":true', '"criticism":true'), "l4": ('"criticism":true', '"praise":true')}


def check():
    import gate, load, needs, pace, rest
    for m in (needs, gate, pace, rest, load):
        m.check()


def canon_all(docs):
    return [common.canon(d) for d in docs]


def one(persona, seed, life, tmp, k):
    vcode, _ = common.verify(life.strand)
    code, docs, _ = common.live(persona, life.events, seed=seed)
    third = canon_all(docs) != canon_all(life.stances)
    st = os.path.join(tmp, "split-%d.state" % k)
    strand = os.path.join(tmp, "split-%d.strand" % k)
    common.init(persona, seed, st)
    half = len(life.events) // 2
    _, d1, _ = common.live(persona, life.events[:half], state=st, strand=strand)
    _, d2, _ = common.live(persona, life.events[half:], state=st, strand=strand)
    split = canon_all(d1 + d2) != canon_all(life.stances)
    scode, _ = common.verify(strand)
    return {"verify_fail": int(vcode != 0) + int(scode != 0), "third_differs": int(third), "split_differs": int(split)}


def planted(strand, tmp, old, new, drop_key=None, drop_at=None, swap_at=None):
    """A copy of a strand with one changed value, one line kind removed, the line at `drop_at` removed, or the lines at
    `swap_at` and `swap_at` + 1 swapped: `live verify` must refuse it (True when the change was planted and refused)."""
    lines = open(strand, encoding="utf-8").read().split("\n")
    out, done = [], False
    for i, line in enumerate(lines):
        if not done and drop_at is not None and i == drop_at and i < len(lines) - 1:
            done = True
            continue
        if not done and drop_key and line.startswith('{"%s"' % drop_key):
            done = True
            continue
        if not done and old and old in line:
            line, done = line.replace(old, new, 1), True
        out.append(line)
    if swap_at is not None and swap_at + 1 < len(out) - 1 and out[swap_at] != out[swap_at + 1]:
        out[swap_at], out[swap_at + 1] = out[swap_at + 1], out[swap_at]
        done = True
    copy = os.path.join(tmp, "planted.strand")
    with open(copy, "w", encoding="utf-8") as f:
        f.write("\n".join(out))
    code, _ = common.verify(copy)
    return done and code != 0


def corruptions_refused(bar, strand, tmp):
    """The three planted corruptions of bars v2: changed input, removed line, swapped lines."""
    old, new = CHANGE[bar]
    return {"changed": planted(strand, tmp, old, new), "removed": planted(strand, tmp, None, None, drop_at=3),
            "swapped": planted(strand, tmp, None, None, swap_at=3)}


def metric(bars, tmp):
    r, gates, k = {}, {}, 0
    for bar, parts in sorted(BARS.items()) + [("reported", REPORTED)]:
        tot = {"strands": 0, "verify_fail": 0, "third_differs": 0, "split_differs": 0}
        for part in parts:
            for persona, seed, life in common.LIVES.get(part, []):
                k += 1
                res = one(persona, seed, life, tmp, k)
                tot["strands"] += 1
                for key in res:
                    tot[key] += res[key]
        if tot["strands"] == 0:
            continue   # the parts behind this bar did not run
        r[bar] = tot
        if bar != "reported":
            head = [life for part in parts for _, _, life in common.LIVES.get(part, [])][0]
            tot["planted_refused"] = corruptions_refused(bar, head.strand, tmp)
            gates[bar] = (tot["strands"] > 0 and tot["verify_fail"] == tot["third_differs"] == tot["split_differs"] == 0
                          and all(tot["planted_refused"].values()))
    needs = common.LIVES.get("needs", [])
    if needs:
        r["n4_null_changed_amount_refused"] = planted(needs[0][2].strand, tmp, '"amount":0.6', '"amount":0.5')
    rest = common.LIVES.get("rest", [])
    if rest:
        r["r3_null_rest_line_removed_refused"] = planted(rest[0][2].strand, tmp, None, None, drop_key="rest")
    r["pass"] = gates
    r["scope"] = "every strand written by needs, gate, pace, rest, load (gated) and dispositions, valence (reported) this run"
    return r


def main(argv):
    import dispositions, gate, load, needs, pace, rest, valence
    tmp = common.tmpdir()
    try:
        common.binary()
        check()
        b = common.bars()
        for m in (needs, gate, pace, rest, load, dispositions, valence):
            m.check()
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
