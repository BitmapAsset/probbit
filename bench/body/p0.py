"""P0 (RFC 0002 Invariants): bars n6, s5, r5 and l5 (the RFC 0001 P0 corpus re-run with this build: every document of a
persona without the new blocks byte-identical to the recorded one), and o5 (optimism_spread 0 gives the bytes of the
persona without the dispositions block, digests aside: spread 0 stores no gene).

    python3 p0.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

With PROBBIT_REF_BIN set to a build without the RFC 0002 blocks, every no-block copy in fixtures/variants is also run on
both binaries over its fixture's events and compared byte for byte (reported).
"""
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "dispositions"
FIXTURES = ["optimism-spread0.yaml"]
NOBLOCK = ["needs1-noblock.yaml", "needs2-noblock.yaml", "gate-nosurprise.yaml", "pace-nopace.yaml",
           "optimism-noblock.yaml", "valence-noblock.yaml", "rest-norest.yaml", "load-noload.yaml"]


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def strip(v):
    """A document without the persona digest, its own digest and every sha256 reference (they differ by construction)."""
    if isinstance(v, dict):
        return {k: strip(x) for k, x in v.items() if k not in ("persona", "digest")
                and not (isinstance(x, str) and x.startswith("sha256:"))}
    if isinstance(v, list):
        return [strip(x) for x in v]
    return v


def corpus():
    """bench/individuality/p0.py check with this build: exit 0 when every recorded document is byte-identical."""
    env = dict(os.environ, PROBBIT_BIN=common.binary())
    p = subprocess.run([sys.executable, os.path.join(common.REPO, "bench", "individuality", "p0.py"), "check"],
                       capture_output=True, text=True, env=env, cwd=common.REPO)
    tail = (p.stdout + p.stderr).strip().splitlines()
    return p.returncode, tail[-1][:300] if tail else ""


def o5(tmp):
    rows = common.stream("pairs")[:2]
    same, n = 0, 0
    sample = None
    for row in rows:
        for s in row["pair"]:
            out = []
            for f in ("optimism-spread0.yaml", "variants/optimism-noblock.yaml"):
                strand = os.path.join(tmp, "o5-%d-%s.strand" % (s, os.path.basename(f)))
                code, docs, sts, err = common.live_states(common.fixture(f), row["events"], s, strand)
                out.append([common.canon(strip(d)) for d in docs] + [common.canon(strip(x)) for x in sts])
            n += 1
            same += 1 if out[0] == out[1] else 0
            if sample is None:
                sample = out
    # the null: a planted one-byte change in one document is detected by the same comparison
    planted = list(sample[1])
    planted[0] = planted[0][:-2] + ("0" if planted[0][-2] != "0" else "1") + planted[0][-1]
    return same, n, sample[0] != planted


def reference(tmp):
    ref = os.environ.get("PROBBIT_REF_BIN")
    if not ref:
        return "not compared (set PROBBIT_REF_BIN to a build without the RFC 0002 blocks)"
    events = [{"elapsed_hours": 1, "src": "env:host"}] * 20
    diff = 0
    for f in NOBLOCK:
        outs = []
        for b in (common.binary(), ref):
            ev = os.path.join(tmp, "ref-events.jsonl")
            common.write_jsonl(ev, events)
            p = subprocess.run([b, "live", common.fixture("variants/" + f), "--clock", "fixed", "--seed", "0", "--events", ev],
                               capture_output=True, cwd=common.REPO)
            outs.append(p.stdout)
        diff += 0 if outs[0] == outs[1] else 1
    return "%d of %d no-block copies differ" % (diff, len(NOBLOCK))


def metric(bars, tmp):
    code, line = corpus()
    same, n, detects = o5(tmp)
    r = {"corpus_exit": code, "corpus_summary": line, "o5_identical": "%d/%d" % (same, n), "o5_planted_change_detected": detects,
         "noblock_vs_reference": reference(tmp)}
    ok = code == 0
    r["pass"] = {"n6": ok, "s5": ok, "r5": ok, "l5": ok, "o5": same == n and detects}
    r["scope"] = ("bench/individuality P0 corpus (8 personas, recorded with 0.8.2) with this build; o5: optimism-spread0 vs "
                  "optimism-noblock on pairs 0-1, stances and states without digests")
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
