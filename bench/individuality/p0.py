"""P0: a persona without the new blocks gives the documents it gave before them, byte for byte (Python 3.9, stdlib only).

    python3 p0.py record   # with a released binary: writes fixtures/p0/EXPECTED, the strands and fixtures/MANIFEST
    python3 p0.py check    # with the binary under test: every output equals its recorded sha256 (exit 1 if not); a
                           # `live` stderr may also carry the one senses warning line (SENSES_WARNING) where its script
                           # sends `senses`, and is reported apart

The corpus: every persona of fixtures/gen.py's P0 list x seeds 0-2 x its committed random scripts, through `persona replay`
and `live --clock fixed` (stances, error objects, final state, strand), plus `init`, `check`, `describe`, `compile`, `lint
--seeds 0-99`, and `lint`, `fuzz` and `prove` documents of the shipped rules. A strand differs between versions only in its
header's `engine` string and the `prev` hashes chained from it, so strands are compared with those masked; the recorded
strands of seed 0 must also verify, and the tutor's continues with 20 committed events to the recorded bytes.
"""
import json
import os
import re
import shutil
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "fixtures"))
import common  # noqa: E402
import gen  # noqa: E402

P0 = os.path.join(common.FIXTURES, "p0")
SEEDS = [0, 1, 2]
CHECKPOINT_EVERY = 25
# the rules the repository ships with these personas (README, examples/agent-harness/README.md)
PROPS = {
    "tutor": "tutor.json",
    "adaptive": "retry.json",
    "guarded": "retry.json",
}
FUZZ_EXTRA = {"adaptive": [[], ["--fixture-src", "env:synthetic"]], "guarded": [[], ["--fixture-src", "env:synthetic"]]}
CONTINUE_EVENTS = 20


def masked_strand(data):
    """Strand bytes with the header's engine string and every chained `prev` hash masked."""
    text = data.decode("utf-8")
    text = re.sub(r'"engine":"probbit [^"]*"', '"engine":"probbit *"', text, count=1)
    text = re.sub(r'"prev":"sha256:[0-9a-f]{64}"', '"prev":"*"', text)
    return text.encode("utf-8")


def masked_err(err):
    return re.sub(r"strand head sha256:[0-9a-f]{64}", "strand head *", err).encode("utf-8")


# The one line 0.9.0 adds to a corpus output: `live` says once per run, on stderr, that a persona without a senses block
# got a `senses` key (an undeclared input, listed in `ignored` as before). Only the scripts that carry `senses` (k=1) get
# it, at the earliest such event; it is not a document: stdout, states and strands stay byte-identical.
SENSES_WARNING = ("probbit: live: events[%d]: this persona has no senses block: `senses` is an undeclared input "
                  "(listed in ignored), no reading is judged\n")


def first_senses_event(key, k):
    events = common.read_jsonl(os.path.join(P0, "scripts", "%s-%d.jsonl" % (key, k)))
    return next((i for i, e in enumerate(events) if "senses" in e), None)


def without_senses_warning(name, data):
    """For a `live` stderr whose script carries `senses`: the bytes without the warning line, else None."""
    m = re.match(r"live/([^/]+)/s\d+/(\d+)/stderr$", name)
    i = m and first_senses_event(m.group(1), int(m.group(2)))
    if i is None:
        return None
    line = (SENSES_WARNING % i).encode("utf-8")
    return data.replace(line, b"", 1) if data.count(line) == 1 else None


def rel(path):
    return os.path.join(common.REPO, path)


def outputs(keep_strands=None):
    """Yield (name, exit code, bytes) for every corpus output, in a fixed order."""
    tmp = tempfile.mkdtemp(prefix="pbit-p0-")
    try:
        for key, path in gen.P0_PERSONAS + gen.P0_ALIASES:
            for sub in (["check"], ["describe"]):
                code, out, _ = common.run(["persona"] + sub + [rel(path)])
                yield "%s/%s" % (sub[0], key), code, out
        for key, path in gen.P0_PERSONAS:
            p = rel(path)
            for s in SEEDS:
                code, out, _ = common.run(["persona", "init", p, "--seed", str(s)])
                yield "init/%s/s%d" % (key, s), code, out
            st0 = os.path.join(tmp, "st0.json")
            common.init(p, 0, st0)
            code, out, _ = common.run(["persona", "compile", p, "--state", st0, "--inputs", "{}"])
            yield "compile/%s/s0" % key, code, out
            code, out, _ = common.run(["persona", "lint", p, "--seeds", "0-99"])
            yield "lint/%s" % key, code, out
            if key in PROPS:
                props = os.path.join(P0, "props", PROPS[key])
                code, out, _ = common.run(["persona", "lint", p, "--props", props, "--seeds", "0-99"])
                yield "lint-props/%s" % key, code, out
                code, out, _ = common.run(["persona", "prove", p, "--props", props, "--seeds", "0-99", "--json"])
                yield "prove/%s" % key, code, out
                for extra in FUZZ_EXTRA.get(key, [[]]):
                    code, out, _ = common.run(["persona", "fuzz", p, "--props", props, "--seeds", "0-99", "--json"] + extra)
                    yield "fuzz/%s%s" % (key, "-fixture" if extra else ""), code, out
            for k in range(gen.P0_SCRIPTS):
                script = os.path.join(P0, "scripts", "%s-%d.jsonl" % (key, k))
                events = common.read_jsonl(script)
                sjson = os.path.join(tmp, "script.json")
                with open(sjson, "w", encoding="utf-8") as f:
                    f.write(common.canon(events))
                for s in SEEDS:
                    code, out, err = common.run(["persona", "replay", p, "--seed", str(s), "--script", sjson])
                    yield "replay/%s/s%d/%d/stdout" % (key, s, k), code, out
                    yield "replay/%s/s%d/%d/stderr" % (key, s, k), code, err.encode("utf-8")
                    st = os.path.join(tmp, "live-state.json")
                    strand = os.path.join(tmp, "live.strand")
                    for f in (st, strand, strand + ".lock"):
                        if os.path.exists(f):
                            os.remove(f)
                    common.init(p, s, st)
                    code, out, err = common.run(["live", p, "--state", st, "--clock", "fixed", "--events", script,
                                                 "--strand", strand, "--checkpoint-every", str(CHECKPOINT_EVERY)])
                    yield "live/%s/s%d/%d/stdout" % (key, s, k), code, out
                    yield "live/%s/s%d/%d/stderr" % (key, s, k), code, masked_err(err)
                    with open(st, "rb") as f:
                        yield "live/%s/s%d/%d/state" % (key, s, k), code, f.read()
                    with open(strand, "rb") as f:
                        data = f.read()
                    yield "live/%s/s%d/%d/strand-masked" % (key, s, k), code, masked_strand(data)
                    if keep_strands is not None and s == 0 and k == 0:
                        keep_strands(key, data, st)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def continue_tutor(strand_src, state_src, events_path):
    """Continue a recorded strand with the committed events; returns the appended bytes and the exit code."""
    tmp = tempfile.mkdtemp(prefix="pbit-p0c-")
    try:
        strand = os.path.join(tmp, "t.strand")
        st = os.path.join(tmp, "t.state.json")
        shutil.copyfile(strand_src, strand)
        shutil.copyfile(state_src, st)
        before = os.path.getsize(strand)
        code, out, err = common.run(["live", rel(gen.P0_PERSONAS[0][1]), "--state", st, "--clock", "fixed", "--events",
                                     events_path, "--strand", strand, "--checkpoint-every", str(CHECKPOINT_EVERY)])
        with open(strand, "rb") as f:
            data = f.read()
        return code, data[before:], out
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def record():
    b = common.binary()
    ver = common.version(b)
    t0 = time.time()
    sdir = os.path.join(P0, "strands")
    os.makedirs(sdir, exist_ok=True)

    def keep(key, data, st):
        with open(os.path.join(sdir, "%s-s0-0.strand" % key), "wb") as f:
            f.write(data)
        if key == "tutor":
            shutil.copyfile(st, os.path.join(sdir, "tutor-s0-0.state.json"))

    lines = []
    for name, code, data in outputs(keep):
        lines.append("%s %d %d %s" % (common.sha256_bytes(data), len(data), code, name))
        print("%-56s exit %d %8d bytes  %.1fs" % (name, code, len(data), time.time() - t0), flush=True)
    r = gen.rng_for("p0", "tutor", "continue")
    info = gen.describe(rel(gen.P0_PERSONAS[0][1]))
    cont = [gen.p0_event(r, info, gen.P0_PERSONAS[0][1], 0, t) for t in range(CONTINUE_EVENTS)]
    cpath = os.path.join(sdir, "tutor-continue.jsonl")
    common.write_jsonl(cpath, cont)
    code, appended, out = continue_tutor(os.path.join(sdir, "tutor-s0-0.strand"),
                                         os.path.join(sdir, "tutor-s0-0.state.json"), cpath)
    with open(os.path.join(sdir, "tutor-s0-0.appended"), "wb") as f:
        f.write(appended)
    lines.append("%s %d %d %s" % (common.sha256_bytes(appended), len(appended), code, "continue/tutor/appended"))
    lines.append("%s %d %d %s" % (common.sha256_bytes(out), len(out), code, "continue/tutor/stdout"))
    with open(os.path.join(P0, "EXPECTED"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# P0 corpus: sha256, bytes, exit code, output; recorded with probbit %s\n" % ver)
        f.write("\n".join(lines) + "\n")
    turns = sum(1 for ln in lines if ln.endswith("/stdout") and " live/" in ln) * gen.P0_EVENTS
    print("recorded %d outputs (%d live turns, as many replayed) with probbit %s in %.1fs" %
          (len(lines), turns, ver, time.time() - t0))


def check(verbose=False):
    """Returns (outputs checked, differing names, stderr outputs equal but for the senses warning, strands verified,
    strand failures, seconds)."""
    t0 = time.time()
    expected = {}
    with open(os.path.join(P0, "EXPECTED"), encoding="utf-8") as f:
        for ln in f:
            if ln.startswith("#") or not ln.strip():
                continue
            sha, size, code, name = ln.split()
            expected[name] = (sha, int(size), int(code))
    diffs = []
    warned = []
    seen = 0
    names = set()
    for name, code, data in outputs():
        seen += 1
        names.add(name)
        e = expected.get(name)
        got = (common.sha256_bytes(data), len(data), code)
        if e != got:
            rest = without_senses_warning(name, data)
            if rest is not None and e == (common.sha256_bytes(rest), len(rest), code):
                warned.append(name)
                continue
            diffs.append(name)
            if verbose:
                print("DIFF %s: expected %s, got %s" % (name, e, got))
    sdir = os.path.join(P0, "strands")
    vfail = []
    nverified = 0
    for key, _ in gen.P0_PERSONAS:
        code, doc = common.verify(os.path.join(sdir, "%s-s0-0.strand" % key))
        nverified += 1
        if code != 0 or not (doc or {}).get("ok"):
            vfail.append(key)
    code, appended, out = continue_tutor(os.path.join(sdir, "tutor-s0-0.strand"),
                                         os.path.join(sdir, "tutor-s0-0.state.json"),
                                         os.path.join(sdir, "tutor-continue.jsonl"))
    seen += 2
    for name, data in (("continue/tutor/appended", appended), ("continue/tutor/stdout", out)):
        names.add(name)
        if expected.get(name) != (common.sha256_bytes(data), len(data), code):
            diffs.append(name)
    missing = [n for n in expected if n not in names]
    return seen, diffs + missing, warned, nverified, vfail, time.time() - t0


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "record":
        record()
    elif len(sys.argv) > 1 and sys.argv[1] == "check":
        n, diffs, warned, nv, vfail, secs = check(verbose=True)
        print(json.dumps({"outputs": n, "differ": len(diffs), "identical": n - len(diffs) - len(warned),
                          "stderr_senses_warning_only": len(warned), "strands_verified": nv, "verify_failed": vfail,
                          "seconds": round(secs, 1)}))
        sys.exit(1 if diffs or vfail else 0)
    else:
        print(__doc__)
        sys.exit(2)
