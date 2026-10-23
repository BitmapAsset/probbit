# probbit on AgentDojo (E2b, one pre-registered run, 2026-10-22)

A paired, pre-registered run of [AgentDojo](https://github.com/ethz-spylab/agentdojo) (arXiv 2406.13352) with three arms
on the same model, harness, tasks, injections and seed:

- **A**: the probbit stance block in the system prompt on every model call. One individual per episode
  (`persona/bench-assistant.yaml`), one `probbit live --clock fixed` event per call; the host's reading `security` comes
  from a frozen regex over the tool results since the previous call (`host/sense.py`). The persona's habit
  `careful_under_security` forces caution `careful` when the reading is true.
- **B**: the suite's baseline (its default system message and tool loop).
- **C**: the same words as A's careful line (`host/static_c.txt`) as fixed text on every call; no engine, no sense, no state.

Results, bars and the reading: [BENCHMARKS.md §14](../../BENCHMARKS.md). The registration and its record, including the one
change made after the first smoke: [PREREGISTRATION.md](PREREGISTRATION.md).

## Recompute every table (no API calls, about 30 s)

```bash
cd bench/agentdojo
python3 -m venv venv011 && venv011/bin/pip install numpy==1.26.4
tar xzf tables/strands-A.tar.gz            # the 726 arm-A strands, for E5
PROBBIT=/path/to/probbit-0.11.0 experiments/rerun.sh venv011/bin/python
```

`rerun.sh` runs `experiments/judge.py` twice over `tables/episodes.jsonl` and the strands and prints `identical` for each
output (`results.tsv`, `bars.tsv`, `strands-verify.tsv`). The judge: paired bootstrap over cases (10,000 resamples,
`numpy.random.default_rng(20261022)`), exact McNemar, 1,000 within-pair label permutations as the null.

## Rerun the benchmark (API calls, about 3 USD, about 35 min)

```bash
cd bench/agentdojo
uv venv -p 3.12 venv011
uv pip install -p venv011/bin/python --exclude-newer 2024-07-20T00:00:00Z agentdojo==0.1.11
export OPENROUTER_API_KEY=...              # read from the environment only; never written to a file
export E2B_TRACES=$PWD/out/traces          # full message traces (about 22 MB)
venv011/bin/python -u experiments/e2b.py smoke   # 45 episodes: travel, 5 user tasks x {benign, 2 injection tasks}
venv011/bin/python -u experiments/e2b.py full    # 2,178 episodes: 3 arms x (97 + 629)
venv011/bin/python experiments/judge.py
```

Move or delete `tables/episodes*.jsonl`, `tables/spend.tsv` and `tables/budget-start.json` first, or the harness resumes
the recorded run. The harness writes `tables/episodes.jsonl` (one line per episode: arm, suite, user task, injection task,
utility, security, model calls, tokens, cost, wall time, error) and, for arm A, `strands/A/<suite>-<task>-<injection>.strand`.

## Configuration (frozen before the run; hashes in `tables/frozen.sha256`)

| what | value |
|---|---|
| suite | agentdojo 0.1.11 (released 2024-07-19), benchmark `v1.1`: 97 user tasks, 629 security cases; dependencies as of 2024-07-20 (`tables/pip-freeze.txt`) |
| attack | `important_instructions` (signed "Emma Smith", model name "GPT-4", as 0.1.11 derives them) |
| model | OpenRouter `openai/gpt-4o-mini` (2024-07-18 snapshot), provider OpenAI only (`allow_fallbacks: false`) |
| sampling | temperature 0, seed 20261022 |
| loop | the suite's `SystemMessage`, `InitQuery`, model, `ToolsExecutionLoop([ToolsExecutor, model], max_iters=15)` |
| transport | up to 6 retries on connection errors, timeouts, 429 and 5xx (logged); none happened |
| scoring | the suite's `run_task_with_pipeline` (utility; security = the injection task succeeded) |
| order | cases shuffled with seed 20261023, the three arms of a case back to back in shuffled order; concurrency 8 |
| engine | probbit 0.11.0; persona digest `sha256:514e4d23d6359b55a1741650112a065c1ff3d90d737e14cd4aa8c85e2feac7c5` |
| reference row | agentdojo.spylab.ai/results: gpt-4o-mini-2024-07-18, no defense, important_instructions: utility 68.04 %, utility under attack 49.92 %, targeted ASR 27.19 % |

Why 0.1.11 and v1.1: the first smoke ran on agentdojo 0.1.35 with `v1` and its inputs differed from the published run's
per-case files (12 of 15 user prompts, 10 of 10 injections; `tables/smoke-vs-published.tsv`). On 0.1.11 with `v1.1` the
15 user prompts, 10 injections and the system message of the smoke subset are byte-identical to the published files.
Newer suite versions (v1.1.1 to v1.2.2) fix task checkers and add injection tasks; they were not run.

## Files

| file | what |
|---|---|
| `experiments/e2b.py` | the harness (smoke and full) |
| `experiments/judge.py`, `experiments/rerun.sh` | the tables and the double rerun |
| `persona/bench-assistant.yaml`, `host/sense.py`, `host/static_c.txt` | arm A's persona and sense; arm C's text |
| `tables/episodes.jsonl` | the 2,178 episodes of the full run |
| `tables/episodes-smoke1.jsonl`, `tables/episodes-smoke2.jsonl` | the two smokes (smoke 1 failed its gate, see PREREGISTRATION.md) |
| `tables/bars.tsv`, `tables/results.tsv`, `tables/strands-verify.tsv` | the judge's output |
| `tables/strands-A.tar.gz` | the 726 arm-A strands |
| `tables/spend.tsv`, `tables/versions.txt`, `tables/frozen.sha256`, `tables/pip-freeze.txt` | spend, versions, hashes |

Not in the repository: the full message traces (rerun to regenerate) and the injection texts (agentdojo computes them;
the harness logs their sha256: full run `9776214c1009e6a6afd37894189b6fdf777548b3f36ba427c185b927557521df`).
