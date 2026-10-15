"""Shared helpers for bench/individuality (Python 3.9, standard library only).

The binary is PROBBIT_BIN, else `probbit` on PATH. Every document is compared as canonical JSON (docs/persona.md section
5.2): keys sorted, no whitespace, whole numbers as integers, other numbers as Python's repr of a float.
"""
import hashlib
import json
import math
import os
import random
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
FIXTURES = os.path.join(HERE, "fixtures")


class SetupError(Exception):
    """A missing binary or fixture: run.py exits 2."""


def binary():
    b = os.environ.get("PROBBIT_BIN") or shutil.which("probbit")
    if not b or not os.path.exists(b):
        raise SetupError("no probbit binary: set PROBBIT_BIN or put probbit on PATH")
    return b


def version(b=None):
    out = subprocess.run([b or binary(), "--version"], capture_output=True, text=True).stdout.strip()
    return out.split()[-1] if out else "?"


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def sha256_file(path):
    with open(path, "rb") as f:
        return sha256_bytes(f.read())


def _num(x):
    if isinstance(x, bool):
        return "true" if x else "false"
    if isinstance(x, int):
        return str(x)
    if x != x or x in (float("inf"), float("-inf")):
        raise ValueError("not a finite number")
    if x == int(x) and abs(x) <= 2 ** 53:
        return str(int(x))
    return repr(float(x))


def _str(s):
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\b":
            out.append("\\b")
        elif ch == "\f":
            out.append("\\f")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif o < 0x20:
            out.append("\\u%04x" % o)
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def canon(v):
    """Canonical JSON text of a value (docs/persona.md section 5.2)."""
    if v is None:
        return "null"
    if isinstance(v, (bool, int, float)):
        return _num(v)
    if isinstance(v, str):
        return _str(v)
    if isinstance(v, (list, tuple)):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{" + ",".join(_str(k) + ":" + canon(v[k]) for k in sorted(v)) + "}"
    raise TypeError(type(v))


def run(args, stdin=None, cwd=None):
    """Run the binary; returns (exit code, stdout bytes, stderr text)."""
    p = subprocess.run([binary()] + list(args), input=stdin, capture_output=True, cwd=cwd or REPO)
    return p.returncode, p.stdout, p.stderr.decode("utf-8", "replace")


def jsonl(data):
    return [json.loads(line) for line in data.decode("utf-8").splitlines() if line.strip()]


def write_jsonl(path, events):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        for e in events:
            f.write(canon(e) + "\n")


def read_jsonl(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def live(persona, events, seed=None, state=None, strand=None, checkpoint_every=None, extra=()):
    """`probbit live --clock fixed` over a list of events. Returns (exit, stances (list of documents), stderr)."""
    tmp = tempfile.mkdtemp(prefix="pbit-bench-")
    try:
        ev = os.path.join(tmp, "events.jsonl")
        write_jsonl(ev, events)
        args = ["live", persona, "--clock", "fixed", "--events", ev]
        if state is not None:
            args += ["--state", state]
        else:
            args += ["--seed", str(seed)]
        if strand is not None:
            args += ["--strand", strand]
        if checkpoint_every is not None:
            args += ["--checkpoint-every", str(checkpoint_every)]
        code, out, err = run(args + list(extra))
        return code, jsonl(out), err
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def init(persona, seed, out):
    code, _, err = run(["persona", "init", persona, "--seed", str(seed), "--out", out])
    if code:
        raise SetupError("init failed: " + err)


def turn(persona, state, inputs, out):
    """One `persona turn` from a saved state, written to `out` (the saved state is not touched)."""
    code, data, err = run(["persona", "turn", persona, "--state", state, "--inputs", canon(inputs), "--out", out])
    return code, (json.loads(data) if data.strip() else None), err


def verify(strand):
    code, out, _ = run(["live", "verify", strand])
    return code, (json.loads(out) if out.strip() else None)


def tv(p, q):
    """Total-variation distance between two odds mappings over the same levels."""
    return 0.5 * sum(abs(p.get(k, 0.0) - q.get(k, 0.0)) for k in set(p) | set(q))


def bootstrap(values, stat=None, n=2000, seed=20261014, level=0.95):
    """Percentile bootstrap interval of a statistic (default the mean) over `values`."""
    stat = stat or (lambda xs: sum(xs) / len(xs))
    rng = random.Random(seed)
    k = len(values)
    reps = sorted(stat([values[rng.randrange(k)] for _ in range(k)]) for _ in range(n))
    lo = reps[int(math.floor((1 - level) / 2 * n))]
    hi = reps[min(n - 1, int(math.ceil((1 + level) / 2 * n)) - 1)]
    return lo, hi


def r6(x):
    v = round(x, 6)
    return 0.0 if v == 0 else v


def fail(msg):
    print("setup error: " + msg, file=sys.stderr)
    sys.exit(2)
