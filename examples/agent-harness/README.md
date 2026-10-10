# A recorded incident becomes an agent regression

This small host runs a deliberately failing **local** probe. No model, credentials, network,
package install, or real external action is involved. Python 3.9+ and the built `probbit`
binary are the only requirements.

From the repository root:

```sh
cargo build --release -p probbit-cli
python3 examples/agent-harness/run.py --binary ./target/release/probbit
PROBBIT_BIN=./target/release/probbit python3 -m unittest discover -s examples/agent-harness -v
```

Use `--out /path/to/new-directory` to retain the strands, host action receipts, exact replay
copies, input events and proof document. Without it the demonstration uses temporary files.
On Windows, pass the path to `target/release/probbit.exe`; set `PROBBIT_BIN` using your shell's
environment syntax when running tests.

## What runs

1. The fresh `adaptive.json` policy dispatches two failing probes, then selects `ask`.
2. Twenty **explicitly synthetic** feedback events change its bounded learned weights. The same
   failures now allow a third attempt. The independent host limit stops further dispatch.
3. `guarded.json` adds one hard habit: after two consecutive errors, select `ask`. The same
   feedback and incident now dispatch only two calls; the host never executes a third.
4. Every incident replays into a separate strand with identical bytes and no tool calls. The
   original incident is also replayed under the repaired policy as a regression. `persona prove`
   checks the declared stance rule for seeds 0–19 and reports `held_by_construction`.

The expected tool-call counts are **2 → 3 → 2**. These are a deterministic fixture, not a
production reliability or behavior-quality benchmark. Recorded durations vary between runs;
replay uses the actual captured elapsed-time inputs.

## The host enforces the decision

`run.py:gate` dispatches only the allowlisted `local_probe` when the stance is `ok`, the
`action` is released and equals `retry`, the habit violation count is zero, no inputs were
ignored, and the host attempt budget remains. Partial, refused, fallback, malformed and
unknown actions escalate without calling the tool. Engine errors abort before dispatch.
This is a real branch around a callable—not an instruction pasted into model prose.

The host measures tool outcomes and elapsed time. `host-actions.json` correlates each proposal,
dispatch/stop, tool outcome and duration with the stance turn, state digest and strand head.
That action receipt is separate from the engine strand; the strand verifies decisions, not
external actions. There are no live approvals, distributed locks, crash-atomic dispatch,
exactly-once execution, or production authentication in this deliberately bounded example.
A plain circuit breaker can enforce this single static retry rule. The additional workflow
shown here is history-dependent reproduction, bounded learning, deterministic replay and a
scoped rule check.

## Replaying generated counterexamples

When `reward_from` requires a source, fuzz searches an abstract accepted-source history.
It must not silently label that history as a production observation. Explicitly authorize
a synthetic fixture source when exporting a runnable counterexample:

```sh
./target/release/probbit persona fuzz examples/agent-harness/adaptive.json --seeds 0-19 --never '{when: {error_streak: 2}, then: {action: [ask]}}' --fixture-src env:synthetic --json
```

The command exits 1 when it finds a counterexample, 0 when none was found, and 2 on an input
error. `human:synthetic` is also accepted, but only where the persona permits human reward
sources. No source restriction is bypassed. The exported script and final stance are produced
by strict replay, and `fixture_provenance` records the synthetic assumption. Without the flag,
a source-required counterexample still reports the abstract script but its `replay` and
`explain` commands are `null`. Personas without `reward_from` keep their existing output.

Do not copy synthetic labels onto production logs. In production the host must authenticate,
classify and record original observations. A source label or hash chain alone is not proof
of origin, and a stance proof says nothing about model obedience, subjective experience, or
whether the author's chosen rule covers every operational hazard.
