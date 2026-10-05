"""probbit.py: a zero-dependency Python wrapper for the `probbit` CLI (stdlib only; one subprocess per call).

    import probbit
    answer = probbit.run(program)                      # probbit-ir program (dict or JSON text) -> `probbit run --op decide`
    answer = probbit.exact(program, exact_ms=500)      # --op exact (declines past the cap)
    answer = probbit.sample(program, sweeps=2000, seed=3, polish_ms=0)   # fixed work: a pure function of (program, seed)
    answer = probbit.decide(router_doc, budget_ms=200) # router document -> `probbit decide`
    answer = probbit.run(program, deadline_ms=1000)    # whole-call deadline (the process is also killed past a slack)
    answer = probbit.decide(router_doc, summary=True)  # compact answer: verdict, counts, gate, the worst items (no per-item tables)
    answer = probbit.evaluate(request, judge="http://127.0.0.1:8080", auth_env="JUDGE_KEY")   # a judge's System One answers + rules
    state = probbit.persona_init("examples/persona/tutor.yaml", seed=2)          # an individual of a persona (docs/persona.md)
    turn = probbit.persona_turn(persona, state, {"loss": True})                  # {"stance": ..., "state": ...}; put turn["stance"]["line"] in the prompt
    trace = probbit.persona_replay(persona, script, seed=2)                      # init + every turn of a script -> the stances
    report = probbit.persona_fuzz(persona, never={"when": {"sentiment": "negative"}, "then": {"humour": {"at_most": "light"}}})
    verdicts = probbit.persona_prove(persona, props=[rule1, rule2], seeds="0-9")  # held_by_construction | proved | unknown per rule

Answers: `probbit run` reports per-variable odds under "marginals", `probbit decide` under "odds" (docs/probbit-ir-json.md, README).
Every call returns the decoded JSON answer for exit 0 (exact / diagnostics_passed / partial), exit 1 (`infeasible`, a proof)
and exit 3 (`refused` / `declined`: the gate or a cap said no; that is an answer, not an error). It raises:
  ProbbitInputError   exit 2: the input or a flag was rejected (.code / .path / .message from the structured error object;
                   flag errors carry the CLI's stderr line in .message and code "flag")
  ProbbitNumericError exit 3 with an {"error": {"code": "numeric"}} object: a computed quantity was not finite (no answer)
  ProbbitTimeout      the call passed `timeout_s` (the process was killed)
  ProbbitJudgeError   `evaluate`'s judge failed (HTTP error, unreachable, not JSON, a missing key variable): no answer
  ProbbitError        anything else (missing binary, a crash, unparseable output)
Keyword flags map to CLI flags: budget_ms=200 -> --budget-ms 200; collective=False -> --collective off (collective, cluster,
cycles take on / off); any other boolean is a switch: summary=True -> --summary, pretty=True -> --pretty, False leaves it out.
The binary: `binary=` argument, else $PROBBIT_BIN, else `probbit` on PATH, else ../target/release/probbit next to this file.
"""
import json, os, shutil, subprocess, tempfile, urllib.error, urllib.parse, urllib.request

__all__ = ["run", "exact", "sample", "decide", "demo", "evaluate", "persona_init", "persona_turn", "persona_replay", "persona_fuzz", "persona_prove", "find_binary", "ProbbitError",
           "ProbbitInputError", "ProbbitNumericError", "ProbbitTimeout", "ProbbitJudgeError"]


class ProbbitError(Exception):
    def __init__(self, message, exit_code=None, stdout="", stderr=""):
        super().__init__(message); self.message, self.exit_code, self.stdout, self.stderr = message, exit_code, stdout, stderr


class ProbbitInputError(ProbbitError):
    def __init__(self, code, path, message, exit_code=2, stdout="", stderr=""):
        super().__init__(message, exit_code, stdout, stderr); self.code, self.path = code, path

    def __str__(self):
        return f"{self.code} at {self.path or '<flags>'}: {self.message}"


class ProbbitNumericError(ProbbitError):
    def __init__(self, path, message, exit_code=3, stdout="", stderr=""):
        super().__init__(message, exit_code, stdout, stderr); self.path = path


class ProbbitTimeout(ProbbitError):
    pass


class ProbbitJudgeError(ProbbitError):
    def __init__(self, message, status=None, body=""):
        super().__init__(message); self.status, self.body = status, body


def find_binary(binary=None):
    for b in (binary, os.environ.get("PROBBIT_BIN"), shutil.which("probbit"),
              os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "probbit")):
        if b and os.path.isfile(b) and os.access(b, os.X_OK):
            return b
    raise ProbbitError("probbit binary not found: pass binary=, set PROBBIT_BIN, or build with `cargo build --release`")


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
        p = subprocess.run(args, input=text, capture_output=True, encoding="utf-8", timeout=timeout_s)  # probbit reads and writes UTF-8, whatever the locale
    except subprocess.TimeoutExpired as e:
        raise ProbbitTimeout(f"probbit {' '.join(cmd)} passed timeout_s={timeout_s}") from e
    try:
        out = json.loads(p.stdout) if p.stdout.strip() else None
    except ValueError:
        raise ProbbitError(f"unparseable output (exit {p.returncode})", p.returncode, p.stdout, p.stderr)
    err = out.get("error") if isinstance(out, dict) else None
    if p.returncode == 2:
        if err:
            raise ProbbitInputError(err.get("code"), err.get("path"), err.get("message"), 2, p.stdout, p.stderr)
        raise ProbbitInputError("flag", None, p.stderr.strip().removeprefix("probbit: "), 2, p.stdout, p.stderr)
    if p.returncode == 3 and err:
        raise ProbbitNumericError(err.get("path"), err.get("message"), 3, p.stdout, p.stderr)
    if p.returncode in (0, 1, 3) and isinstance(out, dict):
        return out
    raise ProbbitError(f"probbit exited {p.returncode}: {p.stderr.strip()[:300]}", p.returncode, p.stdout, p.stderr)


def _timeout(timeout_s, deadline_ms):
    # a deadline the CLI honours itself; the subprocess timeout is only a backstop (2x the deadline + 5 s)
    return timeout_s if timeout_s is not None or deadline_ms is None else 2 * deadline_ms / 1e3 + 5


def run(program, op="decide", *, deadline_ms=None, timeout_s=None, binary=None, **flags):
    """`probbit run --op decide|exact|sample` on a probbit-ir program (docs/probbit-ir-json.md)."""
    if op not in ("decide", "exact", "sample"):
        raise ValueError("op must be decide, exact or sample")
    return _call(["run", "--op", op], program, dict(flags, deadline_ms=deadline_ms), _timeout(timeout_s, deadline_ms), binary)


def exact(program, **kw):
    return run(program, "exact", **kw)


def sample(program, **kw):
    return run(program, "sample", **kw)


def decide(problem, *, timeout_s=None, binary=None, **flags):
    """`probbit decide` on a router document (workers, tasks, affinity; README)."""
    return _call(["decide"], problem, flags, timeout_s, binary)


def demo(tasks=24, seed=1, hard=False, binary=None):
    """`probbit demo`: a synthetic agent-routing document (dict)."""
    args = [find_binary(binary), "demo", "--tasks", str(tasks), "--seed", str(seed)] + (["--hard"] if hard else [])
    p = subprocess.run(args, capture_output=True, text=True)
    if p.returncode != 0:
        raise ProbbitError(f"probbit demo exited {p.returncode}: {p.stderr.strip()}", p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout)


def _judge_url(url):
    # a base URL (no path) gets the System One path of TypeSafe's OpenAPI 0.2.0; a URL with a path is the endpoint itself,
    # e.g. Workers AI: https://api.cloudflare.com/client/v4/accounts/<account id>/ai/run/@cf/cloudflare/clef
    u = urllib.parse.urlsplit(url)
    if u.scheme not in ("http", "https") or not u.netloc:
        raise ValueError(f"judge URL must be http(s)://host[:port][/path]: {url!r}")
    return url.rstrip("/") + "/v1/systemone" if u.path in ("", "/") else url


def _ask_judge(url, ask, auth_env, timeout_s):
    headers = {"Content-Type": "application/json"}
    if auth_env:  # the key is read from the variable the caller names; none is ever stored or defaulted here
        key = os.environ.get(auth_env)
        if not key:
            raise ProbbitJudgeError(f"judge auth: the environment variable {auth_env} is not set")
        headers["Authorization"] = "Bearer " + key
    req = urllib.request.Request(url, data=json.dumps(ask, ensure_ascii=False).encode("utf-8"), headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=timeout_s) as r:
            raw = r.read()
    except urllib.error.HTTPError as e:
        raise ProbbitJudgeError(f"judge {url}: HTTP {e.code}", e.code, e.read().decode("utf-8", "replace")[:500]) from e
    except OSError as e:  # URLError, refused connection, timeout
        raise ProbbitJudgeError(f"judge {url}: {e}") from e
    try:
        return json.loads(raw.decode("utf-8"))
    except ValueError as e:
        raise ProbbitJudgeError(f"judge {url}: the reply is not JSON", None, raw[:500].decode("utf-8", "replace")) from e


def _is_response(x):
    return isinstance(x, dict) and isinstance(x.get("answers"), dict) and all(isinstance(a, dict) and "type" in a for a in x["answers"].values())


def evaluate(request, judge=None, *, auth_env=None, judge_timeout_s=30, timeout_s=None, binary=None, deadline_ms=None, **flags):
    """`probbit evaluate` (docs/probbit-ir-json.md "Decision API"): a System One request (model, state, questions keyed by id; types
    noul | choice | score) plus an optional "probbit" block (judge answers or weights, rules over question ids) -> the most likely
    answer set that obeys every rule, in the judge's response shape, with odds and the gate's verdict (a dict; exit 1 and 3 are
    answers, as for `run`). Every `probbit run` flag applies (sweeps=2000, summary=True, ...).

    judge (optional; without it the request must carry probbit.judge / probbit.weights / probbit.logw itself):
      - a callable: called with the request minus its "probbit" block (state, instructions, criteria untouched); it returns a
        System One response (a dict whose "answers" are answer objects) or per-question probabilities ({id: P(true) for noul,
        {option: p} for choice / score}, as probbit.weights);
      - a URL of a System-One-compatible server: the same request is POSTed there with urllib (no other dependency) and the
        reply goes in as probbit.judge. A base URL (no path) gets TypeSafe's /v1/systemone path; a URL with a path is used as is
        (Workers AI: https://api.cloudflare.com/client/v4/accounts/<id>/ai/run/@cf/cloudflare/clef, whose REST envelope
        {"result": ..., "success": ...} is unwrapped). auth_env names the environment variable holding the key, sent as
        `Authorization: Bearer <key>`; none is sent without it. Status: documented from the vendors' published schemas, tested
        against a local mock (python/mock_judge.py), not exercised against a vendor.
    Judge failures raise ProbbitJudgeError (no answer: probbit is not asked)."""
    req = json.loads(request) if isinstance(request, str) else dict(request)
    if judge is not None:
        ask = {k: v for k, v in req.items() if k != "probbit"}
        if callable(judge):
            reply = judge(ask)
        elif isinstance(judge, str):
            url = _judge_url(judge)
            reply = _ask_judge(url, ask, auth_env, judge_timeout_s)
            if isinstance(reply, dict) and "answers" not in reply and isinstance(reply.get("result"), dict):
                if reply.get("success") is False:
                    raise ProbbitJudgeError(f"judge {url}: success false: {json.dumps(reply.get('errors'))[:300]}", None, json.dumps(reply)[:500])
                reply = reply["result"]  # the Workers AI REST envelope
        else:
            raise TypeError("judge must be a callable or a URL string")
        block = dict(req.get("probbit") or {})
        if _is_response(reply):
            block["judge"] = reply
        elif isinstance(reply, dict):
            block["weights"] = reply
        else:
            raise ProbbitJudgeError(f"the judge returned {type(reply).__name__}, not a System One response or per-question probabilities")
        req["probbit"] = block
    return _call(["evaluate"], req, dict(flags, deadline_ms=deadline_ms), _timeout(timeout_s, deadline_ms), binary)


# ---------------------------------------------------------------- persona: the individuality layer (docs/persona.md)
def _persona_path(persona, d):
    """A persona file path (YAML subset or JSON) as is; a dict (the JSON form of a persona file) written to a file in d."""
    if isinstance(persona, dict):
        path = os.path.join(d, "persona.json")
        with open(path, "w", encoding="utf-8") as f:
            json.dump(persona, f, ensure_ascii=False)
        return path
    if isinstance(persona, (str, os.PathLike)):
        return os.fspath(persona)
    raise TypeError("persona must be a file path or a dict (the JSON form of a persona file)")


def _json_file(d, name, doc):
    path = os.path.join(d, name)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(doc, f, ensure_ascii=False)
    return path


def _persona_call(args, timeout_s, binary, lines=False, answers=(0,)):
    try:
        p = subprocess.run([find_binary(binary), "persona", *args], capture_output=True, encoding="utf-8", timeout=timeout_s)
    except subprocess.TimeoutExpired as e:
        raise ProbbitTimeout(f"probbit persona {args[0]} passed timeout_s={timeout_s}") from e
    try:
        out = [json.loads(x) for x in p.stdout.splitlines()] if lines and p.returncode == 0 else (json.loads(p.stdout) if p.stdout.strip() else None)
    except ValueError:
        raise ProbbitError(f"unparseable output (exit {p.returncode})", p.returncode, p.stdout, p.stderr)
    if p.returncode == 2:
        err = out.get("error") if isinstance(out, dict) else None
        if err:
            raise ProbbitInputError(err.get("code"), err.get("path"), err.get("message"), 2, p.stdout, p.stderr)
        raise ProbbitInputError("flag", None, p.stderr.strip().removeprefix("probbit: "), 2, p.stdout, p.stderr)
    if p.returncode in answers and out is not None:
        return out
    raise ProbbitError(f"probbit persona {args[0]} exited {p.returncode}: {p.stderr.strip()[:300]}", p.returncode, p.stdout, p.stderr)


def persona_init(persona, seed=None, *, timeout_s=None, binary=None):
    """`probbit persona init`: a new individual of a persona (a file path or the document as a dict) -> its state (a dict to store
    and pass to persona_turn). seed: the individual (default: the persona's identity.seed)."""
    with tempfile.TemporaryDirectory() as d:
        args = ["init", _persona_path(persona, d)] + (["--seed", str(seed)] if seed is not None else [])
        return _persona_call(args, timeout_s, binary)


def persona_turn(persona, state, inputs=None, *, timeout_s=None, binary=None, **flags):
    """`probbit persona turn`: the persona, the individual's state and this turn's inputs -> {"stance": ..., "state": ...}, the same
    shape as the MCP tool probbit_persona_turn. Put stance["line"] into the model's prompt and keep the returned state for the next
    turn. Flags: timing=True (a non-canonical timing object), no_inertia=True. A bad persona, state or input raises
    ProbbitInputError (code "persona", .path, .message); a refused or fallback stance is an answer."""
    with tempfile.TemporaryDirectory() as d:
        nxt = os.path.join(d, "next.json")
        args = ["turn", _persona_path(persona, d), "--state", _json_file(d, "state.json", state), "--inputs", _json_file(d, "inputs.json", inputs or {}),
                "--out", nxt, *_flags(flags)]
        stance = _persona_call(args, timeout_s, binary)
        with open(nxt, encoding="utf-8") as f:
            return {"stance": stance, "state": json.load(f)}


def persona_replay(persona, script, seed=None, *, timeout_s=None, binary=None, **flags):
    """`probbit persona replay`: init at seed, then every turn of the script (a list of input dicts, or {"turns": [...]}) ->
    the list of stances, byte for byte what persona_turn would give turn by turn. Flags: no_inertia=True."""
    with tempfile.TemporaryDirectory() as d:
        args = ["replay", _persona_path(persona, d), "--script", _json_file(d, "script.json", script), *_flags(flags)]
        if seed is not None:
            args += ["--seed", str(seed)]
        return _persona_call(args, timeout_s, binary, lines=True)


def _rules(d, never, props, seeds, flags):
    """The rule arguments of persona fuzz / prove: one rule (a dict, or one line of YAML) or several (a list, or a props file)"""
    if (never is None) == (props is None):
        raise TypeError("give exactly one of never= (a rule in habit syntax) or props= (a list of rules, or a props file path)")
    if never is not None:
        args = ["--never", never if isinstance(never, str) else json.dumps(never, ensure_ascii=False)]
    elif isinstance(props, (str, os.PathLike)):
        args = ["--props", os.fspath(props)]
    else:
        args = ["--props", _json_file(d, "props.json", {"props": list(props)})]
    if not isinstance(seeds, str):
        seeds = ",".join(str(int(s)) for s in seeds)
    lists = {k: ",".join(repr(float(x)) for x in v) if isinstance(v, (list, tuple)) else v for k, v in flags.items()}
    return args + ["--seeds", seeds, "--json", *_flags(lists)]


def persona_fuzz(persona, never=None, props=None, seeds="0-99", *, timeout_s=None, binary=None, **flags):
    """`probbit persona fuzz --json` (docs/persona.md §5.6): search event scripts for each individual's shortest counterexample to
    a character property -> the probbit_persona_fuzz document (a dict; "found" says whether any rule broke, each property its
    shortest counterexample with the replay command). never: one rule in habit syntax ({"when": {...}, "then": {...}}, or one line
    of YAML); props: a list of rules (each with an optional "id") or a props file. seeds: "0-99", "1,4,9" or a list. Flags:
    fuzz_seed, scripts, depth, beam, grid and hours (lists or comma strings), threads. A counterexample is an answer, not an error;
    it tests the stance, not the words a model writes."""
    with tempfile.TemporaryDirectory() as d:
        return _persona_call(["fuzz", _persona_path(persona, d), *_rules(d, never, props, seeds, flags)], timeout_s, binary, answers=(0, 1))


def persona_prove(persona, never=None, props=None, seeds="0-99", *, timeout_s=None, binary=None, **flags):
    """`probbit persona prove --json` (docs/persona.md §5.6): per rule one verdict over the individuals -> the probbit_persona_prove
    document: "held_by_construction" (with the habits), "proved" (for every event sequence) or "unknown" (with the cells the bound
    could not decide; fuzz them). Arguments as persona_fuzz; flag: threads. An unknown rule is an answer, not an error."""
    with tempfile.TemporaryDirectory() as d:
        return _persona_call(["prove", _persona_path(persona, d), *_rules(d, never, props, seeds, flags)], timeout_s, binary, answers=(0, 1))
