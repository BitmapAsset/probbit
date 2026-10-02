"""pbit.py: a zero-dependency Python wrapper for the `pbit` CLI (stdlib only; one subprocess per call).

    import pbit
    answer = pbit.run(program)                      # pbit-ir program (dict or JSON text) -> `pbit run --op decide`
    answer = pbit.exact(program, exact_ms=500)      # --op exact (declines past the cap)
    answer = pbit.sample(program, sweeps=2000, seed=3, polish_ms=0)   # fixed work: a pure function of (program, seed)
    answer = pbit.decide(router_doc, budget_ms=200) # router document -> `pbit decide`
    answer = pbit.run(program, deadline_ms=1000)    # whole-call deadline (the process is also killed past a slack)
    answer = pbit.decide(router_doc, summary=True)  # compact answer: verdict, counts, gate, the worst items (no per-item tables)

Answers: `pbit run` reports per-variable odds under "marginals", `pbit decide` under "odds" (docs/pbit-ir-json.md, README).
Every call returns the decoded JSON answer for exit 0 (exact / diagnostics_passed / partial), exit 1 (`infeasible`, a proof)
and exit 3 (`refused` / `declined`: the gate or a cap said no; that is an answer, not an error). It raises:
  PbitInputError   exit 2: the input or a flag was rejected (.code / .path / .message from the structured error object;
                   flag errors carry the CLI's stderr line in .message and code "flag")
  PbitNumericError exit 3 with an {"error": {"code": "numeric"}} object: a computed quantity was not finite (no answer)
  PbitTimeout      the call passed `timeout_s` (the process was killed)
  PbitError        anything else (missing binary, a crash, unparseable output)
Keyword flags map to CLI flags: budget_ms=200 -> --budget-ms 200; collective=False -> --collective off (collective, cluster,
cycles take on / off); any other boolean is a switch: summary=True -> --summary, pretty=True -> --pretty, False leaves it out.
The binary: `binary=` argument, else $PBIT_BIN, else `pbit` on PATH, else ../target/release/pbit next to this file.
"""
import json, os, shutil, subprocess

__all__ = ["run", "exact", "sample", "decide", "demo", "find_binary", "PbitError", "PbitInputError", "PbitNumericError", "PbitTimeout"]


class PbitError(Exception):
    def __init__(self, message, exit_code=None, stdout="", stderr=""):
        super().__init__(message); self.message, self.exit_code, self.stdout, self.stderr = message, exit_code, stdout, stderr


class PbitInputError(PbitError):
    def __init__(self, code, path, message, exit_code=2, stdout="", stderr=""):
        super().__init__(message, exit_code, stdout, stderr); self.code, self.path = code, path

    def __str__(self):
        return f"{self.code} at {self.path or '<flags>'}: {self.message}"


class PbitNumericError(PbitError):
    def __init__(self, path, message, exit_code=3, stdout="", stderr=""):
        super().__init__(message, exit_code, stdout, stderr); self.path = path


class PbitTimeout(PbitError):
    pass


def find_binary(binary=None):
    for b in (binary, os.environ.get("PBIT_BIN"), shutil.which("pbit"),
              os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "pbit")):
        if b and os.path.isfile(b) and os.access(b, os.X_OK):
            return b
    raise PbitError("pbit binary not found: pass binary=, set PBIT_BIN, or build with `cargo build --release`")


_ON_OFF = ("collective", "cluster", "cycles")


def _flags(flags):
    out = []
    for k, v in flags.items():
        name = "--" + k.replace("_", "-")
        if v is None:
            continue
        if isinstance(v, bool):
            if k in _ON_OFF:
                out += [name, "on" if v else "off"]
            elif v:
                out.append(name)  # a switch (pretty, summary, progress): it takes no value; False leaves it out
            continue
        out += [name, str(v)]
    return out


def _call(cmd, doc, flags, timeout_s, binary):
    text = doc if isinstance(doc, str) else json.dumps(doc)
    args = [find_binary(binary), *cmd, *_flags(flags)]
    try:
        p = subprocess.run(args, input=text, capture_output=True, encoding="utf-8", timeout=timeout_s)  # pbit reads and writes UTF-8, whatever the locale
    except subprocess.TimeoutExpired as e:
        raise PbitTimeout(f"pbit {' '.join(cmd)} passed timeout_s={timeout_s}") from e
    try:
        out = json.loads(p.stdout) if p.stdout.strip() else None
    except ValueError:
        raise PbitError(f"unparseable output (exit {p.returncode})", p.returncode, p.stdout, p.stderr)
    err = out.get("error") if isinstance(out, dict) else None
    if p.returncode == 2:
        if err:
            raise PbitInputError(err.get("code"), err.get("path"), err.get("message"), 2, p.stdout, p.stderr)
        raise PbitInputError("flag", None, p.stderr.strip().removeprefix("pbit: "), 2, p.stdout, p.stderr)
    if p.returncode == 3 and err:
        raise PbitNumericError(err.get("path"), err.get("message"), 3, p.stdout, p.stderr)
    if p.returncode in (0, 1, 3) and isinstance(out, dict):
        return out
    raise PbitError(f"pbit exited {p.returncode}: {p.stderr.strip()[:300]}", p.returncode, p.stdout, p.stderr)


def _timeout(timeout_s, deadline_ms):
    # a deadline the CLI honours itself; the subprocess timeout is only a backstop (2x the deadline + 5 s)
    return timeout_s if timeout_s is not None or deadline_ms is None else 2 * deadline_ms / 1e3 + 5


def run(program, op="decide", *, deadline_ms=None, timeout_s=None, binary=None, **flags):
    """`pbit run --op decide|exact|sample` on a pbit-ir program (docs/pbit-ir-json.md)."""
    if op not in ("decide", "exact", "sample"):
        raise ValueError("op must be decide, exact or sample")
    return _call(["run", "--op", op], program, dict(flags, deadline_ms=deadline_ms), _timeout(timeout_s, deadline_ms), binary)


def exact(program, **kw):
    return run(program, "exact", **kw)


def sample(program, **kw):
    return run(program, "sample", **kw)


def decide(problem, *, timeout_s=None, binary=None, **flags):
    """`pbit decide` on a router document (workers, tasks, affinity; README)."""
    return _call(["decide"], problem, flags, timeout_s, binary)


def demo(tasks=24, seed=1, hard=False, binary=None):
    """`pbit demo`: a synthetic agent-routing document (dict)."""
    args = [find_binary(binary), "demo", "--tasks", str(tasks), "--seed", str(seed)] + (["--hard"] if hard else [])
    p = subprocess.run(args, capture_output=True, text=True)
    if p.returncode != 0:
        raise PbitError(f"pbit demo exited {p.returncode}: {p.stderr.strip()}", p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout)
