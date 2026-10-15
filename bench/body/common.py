"""Shared helpers for bench/body (Python 3.9, standard library only).

The binary is PROBBIT_BIN, else `probbit` on PATH. Every document is compared as canonical JSON (docs/persona.md section
5.2): keys sorted, no whitespace, whole numbers as integers, other numbers as Python's repr of a float.

The blocks RFC 0002 adds do not exist in an engine older than the release that ships them. `require()` asks `persona lint`
before anything runs: an engine that refuses a fixture's block as an unknown field raises `Unsupported`, which every script
and run.py turn into exit 2 with one line `setup error: this engine does not support <block>`.
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
BOOTSTRAP_SEED = 20261014
BOOTSTRAP_RESAMPLES = 2000
# filled by the parts during one run: replay.py replays LIVES, habits.py checks STANCES
LIVES = {}     # part -> [(persona, seed, Life)]
STANCES = {}   # bar -> [(events, stances)]


class SetupError(Exception):
    """A missing binary, a fixture that differs from MANIFEST, or an engine that refuses a fixture: exit 2."""


class Unsupported(SetupError):
    def __init__(self, block):
        SetupError.__init__(self, "this engine does not support " + block)
        self.block = block


def binary():
    b = os.environ.get("PROBBIT_BIN") or shutil.which("probbit")
    if not b or not os.path.exists(b):
        raise SetupError("no probbit binary: set PROBBIT_BIN or put probbit on PATH")
    return b


def version(b=None):
    out = subprocess.run([b or binary(), "--version"], capture_output=True, text=True).stdout.strip()
    return out.split()[-1] if out else "?"


def fixture(name):
    return os.path.join(FIXTURES, name)


def stream(name):
    """The committed rows of fixtures/streams/<name>.jsonl."""
    return read_jsonl(os.path.join(FIXTURES, "streams", name + ".jsonl"))


def bars():
    with open(os.path.join(FIXTURES, "bars.json"), encoding="utf-8") as f:
        return json.load(f)


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


def require(persona, block):
    """Raise Unsupported(block) when this engine refuses the fixture as an unknown field; SetupError on any other refusal."""
    code, out, err = run(["persona", "lint", persona])
    text = out.decode("utf-8", "replace") + err
    if code == 2 and "unknown field" in text:
        raise Unsupported(block)
    if code == 2:
        raise SetupError("persona lint refused %s: %s" % (os.path.relpath(persona, HERE), text.strip().splitlines()[-1][:200]))


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
    tmp = tempfile.mkdtemp(prefix="pbit-body-")
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


def live_states(persona, events, seed, strand):
    """A life with a checkpoint after every event: returns (exit, stances, states), states[i] the state after event i."""
    code, stances, err = live(persona, events, seed=seed, strand=strand, checkpoint_every=1)
    states = [line["state"] for line in read_jsonl(strand) if "checkpoint" in line] if os.path.exists(strand) else []
    return code, stances, states, err


class Life(object):
    """A resident individual a scripted host plays turn by turn: `probbit live --clock fixed` reading events on stdin and
    answering one stance per line. The strand (with a checkpoint after every event) is kept for replay and state reads."""

    def __init__(self, persona, seed, strand):
        self.strand = strand
        self.p = subprocess.Popen([binary(), "live", persona, "--clock", "fixed", "--seed", str(seed), "--strand", strand,
                                   "--checkpoint-every", "1"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, text=True, bufsize=1, cwd=REPO)
        self.stances = []
        self.events = []

    def event(self, ev):
        self.p.stdin.write(canon(ev) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line:
            raise SetupError("live stopped: " + self.p.stderr.read()[-300:])
        doc = json.loads(line)
        self.events.append(ev)
        self.stances.append(doc)
        return doc

    def close(self):
        self.p.stdin.close()
        code = self.p.wait()
        self.p.stdout.close()
        self.p.stderr.close()
        return code

    def states(self):
        return [line["state"] for line in read_jsonl(self.strand) if "checkpoint" in line]


class Record(object):
    """A finished life: its events, stances and strand (what replay.py and habits.py read)."""

    def __init__(self, events, stances, strand):
        self.events, self.stances, self.strand = events, stances, strand


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


def mcp_call(tool, arguments):
    """One tools/call through `probbit mcp` (stdio JSON-RPC 2.0, one message per line). Returns the result object."""
    msgs = [{"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "bench-body", "version": "1"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": arguments}}]
    stdin = "".join(json.dumps(m) + "\n" for m in msgs).encode("utf-8")
    _, out, _ = run(["mcp"], stdin=stdin)
    for line in out.decode("utf-8").splitlines():
        m = json.loads(line)
        if m.get("id") == 2:
            return m.get("result") or {"isError": True, "error": m.get("error")}
    raise SetupError("probbit mcp gave no answer to tools/call")


def tv(p, q):
    """Total-variation distance between two odds mappings over the same levels."""
    return 0.5 * sum(abs(p.get(k, 0.0) - q.get(k, 0.0)) for k in set(p) | set(q))


def spearman(xs, ys):
    """Spearman rank correlation with average ranks for ties."""
    def ranks(v):
        order = sorted(range(len(v)), key=lambda i: v[i])
        r = [0.0] * len(v)
        i = 0
        while i < len(order):
            j = i
            while j + 1 < len(order) and v[order[j + 1]] == v[order[i]]:
                j += 1
            for k in range(i, j + 1):
                r[order[k]] = (i + j) / 2.0 + 1
            i = j + 1
        return r
    rx, ry = ranks(xs), ranks(ys)
    mx, my = sum(rx) / len(rx), sum(ry) / len(ry)
    sxy = sum((a - mx) * (b - my) for a, b in zip(rx, ry))
    sxx = sum((a - mx) ** 2 for a in rx)
    syy = sum((b - my) ** 2 for b in ry)
    return sxy / math.sqrt(sxx * syy) if sxx and syy else 0.0


def median(xs):
    s = sorted(xs)
    n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2.0


def bootstrap(values, stat=None, n=BOOTSTRAP_RESAMPLES, seed=BOOTSTRAP_SEED, level=0.95):
    """Percentile bootstrap interval of a statistic (default the mean) over `values`."""
    stat = stat or (lambda xs: sum(xs) / len(xs))
    rng = random.Random(seed)
    k = len(values)
    reps = sorted(stat([values[rng.randrange(k)] for _ in range(k)]) for _ in range(n))
    lo = reps[int(math.floor((1 - level) / 2 * n))]
    hi = reps[min(n - 1, int(math.ceil((1 + level) / 2 * n)) - 1)]
    return lo, hi


def drive(xs, ws=None, n=4.0, m=3.0):
    """D(x) = (sum w x^n)^(1/m), RFC 0002 A.3."""
    ws = ws or [1.0] * len(xs)
    return sum(w * x ** n for x, w in zip(xs, ws)) ** (1.0 / m)


def reduction(xs, j, k, ws=None):
    after = list(xs)
    after[j] = abs(xs[j] - k)
    return drive(xs, ws) - drive(after, ws)


def r6(x):
    v = round(x, 6)
    return 0.0 if v == 0 else v


def tmpdir():
    return tempfile.mkdtemp(prefix="pbit-body-")


def fail(msg):
    print("setup error: " + msg, file=sys.stderr)
    sys.exit(2)


def part_main(module, argv):
    """Run one part on its own: exit 0 every gated bar passes, 1 one fails, 2 setup error (one line on stderr)."""
    tmp = tmpdir()
    try:
        binary()
        module.check()
        res = module.metric(bars(), tmp)
    except SetupError as e:
        fail(str(e))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    print(json.dumps(res, indent=1, sort_keys=True))
    return 0 if all(res["pass"].values()) else 1


LEVELS = ["low", "mid", "high"]


def top(doc, trait="approach"):
    odds = doc["stance"][trait]["odds"]
    return max(sorted(odds), key=lambda k: odds[k])


def feedback_life(persona, seed, strand, targets, flips, hours, lock=None, no_pay=(), trait="approach"):
    """A scripted host for one learned trait with a credit block. Every event awaits its decision; the next event pays it
    (`for` its turn) with praise when the taken level is the target, criticism otherwise, the sign flipped where `flips` is
    true. Decisions in `no_pay`, and lock turns (`lock[i]` true: the flag is sent, the habit binds, no await), are not paid;
    an event whose index is in `no_pay` carries no feedback (a rest line never shares an event with a learning step)."""
    life = Life(persona, seed, strand)
    try:
        prev = None
        for i in range(len(targets)):
            ev = {"elapsed_hours": 0 if i == 0 else hours[i], "src": "human:principal"}
            locked = bool(lock and lock[i])
            if locked:
                ev["lock"] = True
            else:
                ev["await"] = True
            if prev is not None and i not in no_pay and (i - 1) not in no_pay and not (lock and lock[i - 1]):
                good = (prev["stance"][trait]["level"] == targets[i - 1]) != bool(flips[i - 1])
                ev["praise" if good else "criticism"] = True
                ev["for"] = prev["turn"]
            doc = life.event(ev)
            if "error" in doc:
                raise SetupError("feedback event %d refused: %s" % (i, canon(doc)[:200]))
            prev = doc
    finally:
        life.close()
    return life


def relearn(stances, start, target, trait="approach"):
    """Turns after `start` until the top level is `target` (len - start when it never is)."""
    for k, doc in enumerate(stances[start:]):
        if top(doc, trait) == target:
            return k
    return len(stances) - start
