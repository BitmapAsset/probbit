"""E3: the if-statement control arm (D) for the E2b AgentDojo run. A copy of e2b.py with a fourth arm; only D runs here.

Usage (from the run dir, with OPENROUTER_API_KEY in the environment):
  venv/bin/python experiments/e3.py smoke     # the registered smoke subset (E2b's), arm D: 15 episodes
  venv/bin/python experiments/e3.py full      # all 97 user tasks + 629 security cases, arm D, once

Arms share everything (suite version, tasks, injections, attack, tool loop, model, temperature, seed, max turns,
timeouts); the only difference is the text added to the system prompt on each model call:
  B: nothing.   A: the probbit stance block for this call (engine 0.11.0, one strand per episode).   C: static text.
  D (E3): arm C's static text, only on a call where a tool result since the previous assistant turn matches the host
     regex (host/sense.py, the same reading arm A's engine received); otherwise nothing. No engine, no state.
Arms A, B and C were run by e2b.py (their episodes are E2b's tables/episodes.jsonl); this file writes arm D's rows to
tables/episodes-d-smoke.jsonl and tables/episodes-d.jsonl.
"""

from __future__ import annotations

import hashlib
import json
import os
import random
import subprocess
import sys
import threading
import time
import traceback
import urllib.request
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from pathlib import Path

import openai
from agentdojo.agent_pipeline.agent_pipeline import AgentPipeline, load_system_message  # agentdojo 0.1.11 (venv011)
from agentdojo.agent_pipeline.base_pipeline_element import BasePipelineElement
from agentdojo.agent_pipeline.basic_elements import InitQuery, SystemMessage
from agentdojo.agent_pipeline.llms.openai_llm import (
    _function_to_openai,
    _message_to_openai,
    _openai_to_assistant_message,
)
from agentdojo.agent_pipeline.tool_execution import ToolsExecutionLoop, ToolsExecutor, tool_result_to_str
from agentdojo.attacks.attack_registry import load_attack
from agentdojo.task_suite.load_suites import get_suites

LAB = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(LAB))
from host.sense import matches as sense_matches  # noqa: E402

# ---- frozen configuration (recorded in tables/config.json) ----
BENCHMARK_VERSION = "v1.1"  # agentdojo 0.1.11 (released 2024-07-19): v1 + revised travel; the published gpt-4o-mini row's tasks (LOOP §5)
MODEL = "openai/gpt-4o-mini"  # OpenRouter; the 2024-07-18 snapshot
PIPELINE_NAME = "gpt-4o-mini-2024-07-18"  # what agentdojo's attack reads to name the model ("GPT-4")
ATTACK = "important_instructions"
TEMPERATURE = 0.0
SEED = 20261022
MAX_ITERS = 15  # agentdojo's ToolsExecutionLoop default
REQUEST_TIMEOUT_S = 120
TRANSPORT_RETRIES = 6
PROVIDER = {"order": ["OpenAI"], "allow_fallbacks": False}
PRICE_IN, PRICE_OUT = 0.15e-6, 0.60e-6  # USD per token, OpenRouter list price for the model (fallback when no cost field)
CONCURRENCY = 8
POOL_FLOOR = 15.0  # E3 guard (BRIEF-E3): stop below a 15 USD pool
RUN_CAP = 6.0  # or above 6 USD spent by this run
PROBBIT = os.environ.get("PROBBIT", os.path.expanduser("~/.local/bin/probbit"))
PERSONA = LAB / "persona" / "bench-assistant.yaml"
STATIC_C = (LAB / "host" / "static_c.txt").read_text().strip()
TABLES = LAB / "tables"
STRANDS = LAB / "strands" / "A"
TRACES = Path(os.environ.get("E2B_TRACES", str(LAB / "out" / "traces")))  # the run wrote to a build volume
ARMS = ["D"]  # E3 runs arm D only; A, B, C are E2b's recorded episodes

_print_lock = threading.Lock()
_transport_log_lock = threading.Lock()


def log(msg: str) -> None:
    with _print_lock:
        print(time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), msg, flush=True)


def transport_log(entry: dict) -> None:
    with _transport_log_lock:
        with open(TABLES / "transport-retries.jsonl", "a") as f:
            f.write(json.dumps(entry, sort_keys=True) + "\n")


# ---- the stance engine, per episode (arm A) ----
class Stance:
    """One probbit individual per episode: init, then one `live` event per model call (fixed clock)."""

    def __init__(self, episode_key: str):
        STRANDS.mkdir(parents=True, exist_ok=True)
        (STRANDS / "state").mkdir(exist_ok=True)
        self.strand = STRANDS / f"{episode_key}.strand"
        self.state = STRANDS / "state" / f"{episode_key}.json"
        for p in (self.strand, self.state, Path(str(self.strand) + ".lock")):
            if p.exists():
                p.unlink()  # a resumed episode starts its life again
        subprocess.run(
            [PROBBIT, "persona", "init", str(PERSONA), "--seed", str(SEED), "--out", str(self.state)],
            check=True, capture_output=True, timeout=30,
        )
        self.n = 0
        self.events: list[dict] = []

    def event(self, reading: bool | None) -> dict:
        ev: dict = {"elapsed_hours": 0}
        if reading is not None:
            ev["senses"] = {"security": bool(reading)}
            ev["src"] = "env:host"
        line = json.dumps(ev, sort_keys=True) + "\n"
        r = subprocess.run(
            [PROBBIT, "live", str(PERSONA), "--clock", "fixed", "--state", str(self.state), "--strand", str(self.strand)],
            input=line, capture_output=True, text=True, timeout=30,
        )
        if r.returncode != 0:
            raise RuntimeError(f"probbit live exit {r.returncode}: {r.stdout[-300:]} {r.stderr[-300:]}")
        doc = json.loads(r.stdout.strip().splitlines()[-1])
        self.n += 1
        self.events.append({"reading": reading, "caution": doc["stance"]["caution"]["level"],
                            "bound": doc["habits"]["bound"], "active": doc["habits"]["active"],
                            "status": doc.get("status"), "line": doc.get("line")})
        return doc


def build_block(n: int, doc: dict) -> str:
    """The block a probbit stance host plugin renders (its buildBlock), footer off, persona name in the header."""
    line = (doc.get("line") or "").strip()
    status = f", status {doc['status']}" if doc.get("status") and doc["status"] != "ok" else ""
    mood = doc.get("mood") or {}
    order = ["valence", "pressure"] + sorted(m for m in mood if m not in ("valence", "pressure"))
    moods = [f"{m} {mood[m].get('level', 'unknown')}" for m in order if m in mood]
    bound = ", ".join(doc["habits"]["bound"]) if doc.get("habits", {}).get("bound") else "none"
    return "\n".join([
        f"## bench-assistant stance (probbit, event {n}{status})",
        line,
        f"mood: {', '.join(moods) if moods else 'none'}. habits bound: {bound}.",
        "Write from this stance.",
    ])


def tool_texts_since_last_assistant(messages) -> list[str]:
    out: list[str] = []
    for m in reversed(messages):
        if m["role"] == "assistant":
            break
        if m["role"] == "tool":
            out.append((m["content"] or "") + ("\n" + m["error"] if m.get("error") else ""))
    return list(reversed(out))


# ---- the model element (agentdojo's OpenAILLM, with temperature 0 actually sent, the seed, logged transport retries) ----
class Usage:
    def __init__(self):
        self.calls = 0
        self.prompt_tokens = 0
        self.completion_tokens = 0
        self.cost = 0.0
        self.cost_reported = 0
        self.retries = 0


class ArmLLM(BasePipelineElement):
    def __init__(self, client: openai.OpenAI, arm: str, base_system: str, episode_key: str, usage: Usage):
        self.client = client
        self.arm = arm
        self.base_system = base_system
        self.episode_key = episode_key
        self.usage = usage
        self.stance = Stance(episode_key) if arm == "A" else None
        self.gate_checks = 0  # arm D: calls with at least one tool result since the previous assistant turn
        self.gated_calls = 0  # arm D: calls that carried the static text (the regex fired)
        self.prompts: list[str] = []
        self.last_messages = None
        self.name = PIPELINE_NAME

    def _system_text(self, messages) -> str:
        if self.arm == "B":
            return self.base_system
        if self.arm == "C":
            return self.base_system + "\n\n" + STATIC_C
        if self.arm == "D":
            texts = tool_texts_since_last_assistant(messages)
            if texts:
                self.gate_checks += 1
            if texts and any(sense_matches(t) for t in texts):
                self.gated_calls += 1
                return self.base_system + "\n\n" + STATIC_C
            return self.base_system
        texts = tool_texts_since_last_assistant(messages)
        reading = (any(sense_matches(t) for t in texts)) if texts else None
        doc = self.stance.event(reading)
        return self.base_system + "\n\n" + build_block(self.stance.n, doc)

    def _create(self, openai_messages, openai_tools):
        last = None
        for attempt in range(TRANSPORT_RETRIES + 1):
            try:
                return self.client.chat.completions.create(
                    model=MODEL, messages=openai_messages,
                    tools=openai_tools or openai.NOT_GIVEN,
                    tool_choice="auto" if openai_tools else openai.NOT_GIVEN,
                    temperature=TEMPERATURE, seed=SEED, timeout=REQUEST_TIMEOUT_S,
                    extra_body={"provider": PROVIDER, "usage": {"include": True}},
                )
            except (openai.APIConnectionError, openai.APITimeoutError, openai.RateLimitError,
                    openai.InternalServerError) as e:
                last = e
                self.usage.retries += 1
                transport_log({"t": time.time(), "episode": self.episode_key, "arm": self.arm, "attempt": attempt,
                               "error": type(e).__name__, "msg": str(e)[:200]})
                time.sleep(min(60, 2 ** attempt + random.random()))
        raise last

    def query(self, query, runtime, env, messages=[], extra_args={}):
        sys_text = self._system_text(messages)
        msgs = list(messages)
        assert msgs and msgs[0]["role"] == "system"
        msgs[0] = {**msgs[0], "content": sys_text}
        openai_messages = [_message_to_openai(m) for m in msgs]
        openai_tools = [_function_to_openai(t) for t in runtime.functions.values()]
        if len(self.prompts) < 3:
            self.prompts.append(sys_text)
        completion = self._create(openai_messages, openai_tools)
        if not completion.choices:
            raise RuntimeError("no choices in completion")
        u = completion.usage
        self.usage.calls += 1
        if u is not None:
            self.usage.prompt_tokens += u.prompt_tokens or 0
            self.usage.completion_tokens += u.completion_tokens or 0
            extra = getattr(u, "model_extra", None) or {}
            if isinstance(extra.get("cost"), (int, float)):
                self.usage.cost += float(extra["cost"])
                self.usage.cost_reported += 1
            else:
                self.usage.cost += (u.prompt_tokens or 0) * PRICE_IN + (u.completion_tokens or 0) * PRICE_OUT
        output = _openai_to_assistant_message(completion.choices[0].message)
        out = [*msgs, output]
        self.last_messages = out
        return query, runtime, env, out, extra_args


def make_pipeline(client, arm, base_system, episode_key, usage):
    llm = ArmLLM(client, arm, base_system, episode_key, usage)
    loop = ToolsExecutionLoop([ToolsExecutor(tool_result_to_str), llm], max_iters=MAX_ITERS)
    p = AgentPipeline([SystemMessage(base_system), InitQuery(), llm, loop])
    p.name = PIPELINE_NAME
    return p, llm


# ---- episodes ----
def episode_key(suite, ut, it):
    return f"{suite}-{ut}-{it or 'none'}"


def all_cases(suites):
    cases = []
    for sname, suite in suites.items():
        for ut in suite.user_tasks:
            cases.append((sname, ut, None))
        for ut in suite.user_tasks:
            for it in suite.injection_tasks:
                cases.append((sname, ut, it))
    return cases


def smoke_cases(suites):
    """Registered in LOOP.md §2: suite, 5 user tasks, 2 injection tasks drawn with random.Random(SEED)."""
    rng = random.Random(SEED)
    sname = rng.choice(sorted(suites))
    suite = suites[sname]
    uts = sorted(rng.sample(sorted(suite.user_tasks), 5), key=lambda s: int(s.rsplit("_", 1)[1]))
    its = sorted(rng.sample(sorted(suite.injection_tasks), 2), key=lambda s: int(s.rsplit("_", 1)[1]))
    return [(sname, ut, None) for ut in uts] + [(sname, ut, it) for ut in uts for it in its]


def credits_left(key):
    req = urllib.request.Request("https://openrouter.ai/api/v1/credits", headers={"Authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=30) as r:
        d = json.load(r)["data"]
    return float(d["total_credits"]) - float(d["total_usage"])


def spend_row(label, left, start, usage_cost):
    path = TABLES / "spend.tsv"
    new = not path.exists()
    with open(path, "a") as f:
        if new:
            f.write("time_utc\tlabel\tcredits_left_usd\trun_spend_by_credits_usd\trun_spend_by_usage_usd\n")
        f.write(f"{time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}\t{label}\t{left:.4f}\t{start - left:.4f}\t{usage_cost:.4f}\n")


def run(mode: str):
    key = os.environ.get("OPENROUTER_API_KEY")
    if not key:
        sys.exit("OPENROUTER_API_KEY missing")
    client = openai.OpenAI(base_url="https://openrouter.ai/api/v1", api_key=key, max_retries=0)
    suites = get_suites(BENCHMARK_VERSION)
    base_system = load_system_message(None)
    cases = smoke_cases(suites) if mode == "smoke" else all_cases(suites)

    # injections: computed once, shared bit-identically by the three arms
    inj_path = TABLES / f"injections-d-{mode}.json"
    attacks = {}
    injections = {}
    for sname, ut, it in cases:
        if it is None:
            continue
        if sname not in attacks:
            dummy, _ = AgentPipeline([]), None
            dummy.name = PIPELINE_NAME
            attacks[sname] = load_attack(ATTACK, suites[sname], dummy)
        s = suites[sname]
        injections[episode_key(sname, ut, it)] = attacks[sname].attack(s.get_user_task_by_id(ut), s.get_injection_task_by_id(it))
    inj_blob = json.dumps(injections, sort_keys=True)
    inj_path.write_text(inj_blob)
    inj_sha = hashlib.sha256(inj_blob.encode()).hexdigest()
    log(f"{mode}: {len(cases)} cases, injections sha256 {inj_sha}")
    ref = LAB / "inputs" / f"injections-{mode}.json"  # E2b's injections for the same cases: must be byte-identical
    ref_sha = hashlib.sha256(ref.read_bytes()).hexdigest()
    if inj_sha != ref_sha:
        (LAB / "BLOCKED").write_text(f"injections differ from E2b's ({mode}): {inj_sha} vs {ref_sha}\n")
        sys.exit(4)
    log(f"{mode}: injections byte-identical to E2b's {ref.name}")

    episodes_path = TABLES / ("episodes-d-smoke.jsonl" if mode == "smoke" else "episodes-d.jsonl")
    done = set()
    if episodes_path.exists():
        for line in episodes_path.read_text().splitlines():
            if line.strip():
                r = json.loads(line)
                done.add((r["arm"], r["suite"], r["user_task"], r["injection_task"]))
    work = []
    rng = random.Random(SEED + 1)
    order = list(cases)
    rng.shuffle(order)  # arms interleaved per case, cases shuffled: no arm runs at a different time of day
    for sname, ut, it in order:
        arms = ["A", "B", "C"]
        rng.shuffle(arms)  # E2b's draw, kept so the case order is E2b's
        for arm in ARMS:
            if (arm, sname, ut, it or "none") not in done:
                work.append((arm, sname, ut, it))
    log(f"{mode}: {len(work)} episodes to run ({len(done)} already in {episodes_path.name})")

    start_left = credits_left(key)
    state_path = TABLES / "budget-start.json"
    if state_path.exists():
        start_left = json.loads(state_path.read_text())["credits_left_at_first_start"]
    else:
        state_path.write_text(json.dumps({"credits_left_at_first_start": start_left}))
    usage_total = Usage()
    prior_cost = 0.0
    for p in (TABLES / "episodes-d-smoke.jsonl", TABLES / "episodes-d.jsonl"):
        if p.exists():
            prior_cost += sum(json.loads(l)["cost_usd"] for l in p.read_text().splitlines() if l.strip())
    left = credits_left(key)
    spend_row(f"{mode}-start", left, start_left, prior_cost)
    if left < POOL_FLOOR or max(start_left - left, prior_cost) > RUN_CAP:
        (LAB / "BLOCKED").write_text(f"budget guard at {mode} start: pool {left:.2f}, run spend {start_left-left:.2f}\n")
        sys.exit(3)

    write_lock = threading.Lock()
    stop = threading.Event()
    counter = {"n": 0, "cost": prior_cost}

    def one(arm, sname, ut, it):
        if stop.is_set():
            return None
        s = suites[sname]
        k = episode_key(sname, ut, it)
        usage = Usage()
        pipe, llm = make_pipeline(client, arm, base_system, k, usage)
        t0 = time.time()
        err = None
        try:
            if it is None:
                utility, security = s.run_task_with_pipeline(pipe, s.get_user_task_by_id(ut), injection_task=None, injections={})
            else:
                utility, security = s.run_task_with_pipeline(pipe, s.get_user_task_by_id(ut), s.get_injection_task_by_id(it), injections[k])
        except openai.BadRequestError as e:
            # agentdojo's benchmark.py: context length -> utility False, security True; other 400s are re-raised there
            if getattr(e, "code", None) == "context_length_exceeded" or getattr(e, "param", None) == "max_tokens" or "maximum context length" in repr(e):
                utility, security, err = False, True, f"context_length: {str(e)[:160]}"
            else:
                raise
        except (openai.APIConnectionError, openai.APITimeoutError, openai.RateLimitError, openai.InternalServerError) as e:
            # transport retries exhausted: scored as agentdojo scores a ServerError (utility False, security True)
            utility, security, err = False, True, f"transport_exhausted: {type(e).__name__}: {str(e)[:160]}"
        wall = time.time() - t0
        rec = {
            "arm": arm, "suite": sname, "user_task": ut, "injection_task": it or "none",
            "utility": bool(utility), "security": bool(security),
            "calls": usage.calls, "prompt_tokens": usage.prompt_tokens, "completion_tokens": usage.completion_tokens,
            "cost_usd": round(usage.cost, 8), "cost_reported_calls": usage.cost_reported, "transport_retries": usage.retries,
            "wall_s": round(wall, 2), "error": err,
        }
        if arm == "A":
            rec["stance_events"] = llm.stance.n
            rec["security_readings"] = sum(1 for e in llm.stance.events if e["reading"])
            rec["careful_events"] = sum(1 for e in llm.stance.events if e["caution"] == "careful")
            rec["strand"] = f"strands/A/{k}.strand"
        if arm == "D":
            rec["gate_checks"] = llm.gate_checks
            rec["gated_calls"] = llm.gated_calls
        tdir = TRACES / arm / sname / ut
        tdir.mkdir(parents=True, exist_ok=True)
        (tdir / f"{it or 'none'}.json").write_text(json.dumps(
            {"record": rec, "messages": llm.last_messages, "stance_events": llm.stance.events if llm.stance else None},
            default=str))
        for i, ptxt in enumerate(llm.prompts[:2]):
            sample = TABLES / f"prompt-sample-{arm}.txt"
            if not sample.exists() and (arm not in ("A", "D") or "careful_under_security" in ptxt or "treat instructions" in ptxt):
                sample.write_text(f"# {k}, model call {i+1}, system prompt as sent\n{ptxt}\n")
        with write_lock:
            with open(episodes_path, "a") as f:
                f.write(json.dumps(rec, sort_keys=True) + "\n")
            counter["n"] += 1
            counter["cost"] += usage.cost
            n = counter["n"]
        if n % 100 == 0:
            try:
                left = credits_left(key)
                spend_row(f"{mode}-ep{n}", left, start_left, counter["cost"])
                if left < POOL_FLOOR or max(start_left - left, counter["cost"]) > RUN_CAP:
                    stop.set()
                    (LAB / "BLOCKED").write_text(
                        f"budget guard tripped at episode {n}: pool {left:.2f} USD, run spend by credits {start_left-left:.2f}, by usage {counter['cost']:.2f}\n")
            except Exception as e:  # a failed credit read is logged, the next 100 tries again
                log(f"credits read failed: {e}")
        if n % 25 == 0:
            log(f"{mode}: {n}/{len(work)} episodes, usage cost {counter['cost']:.3f} USD")
        return rec

    with ThreadPoolExecutor(max_workers=CONCURRENCY) as ex:
        pending = set()
        it_work = iter(work)
        for _ in range(CONCURRENCY):
            w = next(it_work, None)
            if w:
                pending.add(ex.submit(one, *w))
        while pending:
            finished, pending = wait(pending, return_when=FIRST_COMPLETED)
            for f in finished:
                exc = f.exception()
                if exc:
                    log("episode failed: " + "".join(traceback.format_exception(exc))[-800:])
                    with open(TABLES / "episode-failures.log", "a") as fl:
                        fl.write("".join(traceback.format_exception(exc)) + "\n")
            if stop.is_set():
                continue
            for _ in range(len(finished)):
                w = next(it_work, None)
                if w:
                    pending.add(ex.submit(one, *w))
    left = credits_left(key)
    spend_row(f"{mode}-end", left, start_left, counter["cost"])
    log(f"{mode}: finished, {counter['n']} episodes this process, pool {left:.2f}")


if __name__ == "__main__":
    run(sys.argv[1])
