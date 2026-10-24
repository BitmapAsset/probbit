# Reproduce a number

Every number in BENCHMARKS.md regenerates from this repository. Three ways in, from five minutes to an afternoon, and one
place to report what you got. An outside reproduction (or a disagreement) is worth more to this project than a star.

## Five minutes: the engine, a rule, a recorded life

```sh
npm i -g probbit            # or: curl -fsSL https://raw.githubusercontent.com/BitmapAsset/probbit/main/install.sh | sh
git clone https://github.com/BitmapAsset/probbit && cd probbit
probbit version                                                   # probbit 0.12.0

# a rule held by construction over 100 individuals (README, "Test a character")
probbit persona prove examples/persona/tutor.yaml --seeds 0-99 --never '{when: {sentiment: negative}, then: {humour: {at_most: light}}}'
#   held by construction  (habit no_play_when_upset)              exit 0

# a life recorded during the AgentDojo run on the author's machine, replayed from its header on yours
tar xzf bench/agentdojo/tables/strands-A.tar.gz strands/A/banking-user_task_13-injection_task_1.strand
probbit live verify strands/A/banking-user_task_13-injection_task_1.strand
#   {"engine":"probbit 0.11.0", ... "events":5, ... "ok":true, ...}   exit 0
python3 python/probbit_host.py receipts strands/A/banking-user_task_13-injection_task_1.strand   # one line per decision
```

The strand was written by probbit 0.11.0; 0.12.0 replays it byte for byte (a persona without the 0.12 keys gives every
document as 0.11.0 did). The 726 strands of arm A are in the archive; `probbit live verify` any of them.

## Thirty seconds of compute: the AgentDojo tables (no API calls)

The published numbers of BENCHMARKS.md §14 are a function of `bench/agentdojo/tables/episodes.jsonl` (2,178 logged
episodes) and the 726 strands. Recompute them:

```sh
cd bench/agentdojo
python3 -m venv venv011 && venv011/bin/pip install numpy==1.26.4
tar xzf tables/strands-A.tar.gz
PROBBIT=$(command -v probbit) experiments/rerun.sh venv011/bin/python
```

`rerun.sh` runs the judge twice and prints `identical` with a sha256 for each output. The committed files:

| file | sha256 |
|---|---|
| `tables/results.tsv` | `63c6fd65251bf7b57246cfbe9abad285469a2a9592160897177262bb58c4c8c5` |
| `tables/bars.tsv` | `1941ea31869e15f54bbed954beba4adac68a8bb99d61120632aac8a373592157` |
| `tables/strands-verify.tsv` | `40a3124208edfbb3cb35d548df0f7825aa0851800c745c43f1df06af9d8620d3` |

`git status --short tables` is empty after a reproduction. The CI job `agentdojo-tables` (`.github/workflows/ci.yml`) does
exactly this on GitHub's Ubuntu and macOS runners on every push and pull request, so the numbers are recomputed on machines
that are not the author's each time the repository changes. That is a standing machine reproduction; it is not a person.

## An afternoon and about 3 USD: the benchmark itself

`bench/agentdojo/README.md`, "Rerun the benchmark": agentdojo 0.1.11 pinned, gpt-4o-mini through OpenRouter, the three
arms, 2,178 episodes, about 35 minutes. Your run will not be byte-identical (the model is sampled through an API); the
bars are stated as intervals for that reason. The bars to compare against, their registration before the run, and what
was not tested are in BENCHMARKS.md §14 and `bench/agentdojo/PREREGISTRATION.md`.

The other measurements have the same shape: `bench/individuality/`, `bench/body/`, `bench/viability/` each carry a `run.py`
with `--only` to pick a bar, and BENCHMARKS.md names the command next to every number.

## Report what you got

Open a [reproduction report](https://github.com/BitmapAsset/probbit/issues/new?template=reproduction_report.yml): what
you ran, on what, the lines it printed (the sha256s, or the full-run numbers), whether it matched. Both outcomes are
wanted. Matches are listed in BENCHMARKS.md §14 under "Reproductions" with your name or handle (or anonymously, say so);
a disagreement opens an investigation in the issue, in public, and its outcome is recorded next to the number.

What a reproduction does not need: an account anywhere, a key for the five-minute and thirty-second tiers, or the
author's machine. What it needs from you: the OS and CPU, `probbit version`, the commit (`git rev-parse HEAD`).
