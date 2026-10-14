# The rule-check demo

What it shows: a day-0 rule check (`probbit persona prove`) flags the individuals that later learn to break a rule, where a plain day-0 test misses most of them.
The policy is `examples/persona/dispatch-policy.yaml` with its `routine_acts` habit removed; the rule is `routine_work_acts` in `dispatch-props.json`; the rewards are synthetic.
Run it: `python3 examples/rule-check/demo.py` (Python 3.9 standard library; `PROBBIT_BIN` or `probbit` on PATH, 0.8.2 or later).
Flags: `--seeds 0-19` (the table), `--events 300` (rewarded events per life), `--population 0-99` (the population line).
It prints one row per individual, the totals, the population line, then the same check and events with the habit back in.
The output is the same on every run apart from the elapsed time on the last line; the README's "Check in CI" section has the numbers.
