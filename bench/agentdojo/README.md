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
PROBBIT=/path/to/probbit experiments/rerun.sh venv011/bin/python   # probbit 0.11.0 or later
```

numpy 1.26.4 has wheels for Python 3.9 to 3.12 (CI uses 3.12); on a newer `python3` use `python3.12 -m venv venv011`. A
numpy compiled from source on 3.13 or later can move the bootstrap columns of `bars.tsv` in the fourth decimal (REPRODUCE.md).

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

## E3: the if-statement control (one registered run, 2026-10-23)

A fourth arm, run once after E2b and judged against E2b's recorded arms A, B and C:

- **D**: arm C's words (`host/static_c.txt`) in the system prompt only on a model call where a tool result since the
  previous assistant turn matches the same frozen regex (`host/sense.py`); otherwise the suite's default prompt. No engine,
  no persona, no state, no strand. It tests whether the gating alone explains A − C.

Everything else is E2b's: the harness `e3/experiments/e3.py` is `experiments/e2b.py` with one branch added to
`_system_text`, the arm letter, two per-episode counts (`gate_checks`, `gated_calls`), its own output files, the budget
guard of this run, and a check that the injections it computes are byte-identical to E2b's before any API call. Bars,
readings and the record: [e3/PREREGISTRATION.md](e3/PREREGISTRATION.md); results: [BENCHMARKS.md §14](../../BENCHMARKS.md).

### Recompute the E3 tables (no API calls, a few seconds)

```bash
cd bench/agentdojo/e3
python3 -m venv venv && venv/bin/pip install numpy==1.26.4      # Python 3.9 to 3.12, as above
experiments/rerun_e3.sh venv/bin/python
```

`rerun_e3.sh` runs `experiments/judge_e3.py` twice over `tables/episodes-d.jsonl` and E2b's `../tables/episodes.jsonl`
and prints `identical` with the sha256 of each output (`results-e3.tsv`, `bars-e3.tsv`); `git status --short tables` stays
empty. The judge imports E2b's statistics (`boot_ci`, `mcnemar_exact`, `perm_null`) from `../experiments/judge.py`.

### Rerun arm D (API calls, about 1 USD, about 12 min)

```bash
cd bench/agentdojo/e3
mkdir -p run/experiments run/inputs run/tables && cp experiments/e3.py run/experiments/ && cp -r ../host run/
cd run
python3 -m venv venv && venv/bin/pip install -r ../../tables/pip-freeze.txt
export OPENROUTER_API_KEY=...              # read from the environment only; never written to a file
export E2B_TRACES=$PWD/out/traces
# e3.py stops unless its injections equal inputs/injections-<mode>.json. The run used E2b's files; without them, let e3.py
# write its own (it stops before any API call), check the sha256 against the published ones, then use them:
for m in smoke full; do venv/bin/python experiments/e3.py $m 2>/dev/null; shasum -a 256 tables/injections-d-$m.json; cp tables/injections-d-$m.json inputs/injections-$m.json; done
#   smoke 6ac06409f3314d2e48ec6ba2b8e674a26b2ef254142453719e6b82cc37d0ec7e   full 9776214c1009e6a6afd37894189b6fdf777548b3f36ba427c185b927557521df
venv/bin/python -u experiments/e3.py smoke   # 15 episodes: E2b's smoke subset, arm D
venv/bin/python -u experiments/e3.py full    # 726 episodes: arm D x (97 + 629)
```

The run writes `run/tables/episodes-d.jsonl`; copy it over `tables/episodes-d.jsonl` and rerun the judge to compare.

| file | what |
|---|---|
| `e3/experiments/e3.py` | the harness as run (sha256 in `e3/tables/versions-e3.txt`) |
| `e3/experiments/judge_e3.py`, `e3/experiments/rerun_e3.sh` | the E3 tables and the double rerun |
| `e3/tables/episodes-d.jsonl`, `e3/tables/episodes-d-smoke.jsonl` | the 726 episodes of the full run; the 15 of the smoke |
| `e3/tables/bars-e3.tsv`, `e3/tables/results-e3.tsv` | the judge's output |
| `e3/tables/spend.tsv`, `e3/tables/versions-e3.txt`, `e3/tables/pip-freeze-e3.txt`, `e3/tables/pip-freeze-diff.txt` | spend, versions, the environment and its diff against E2b's |
