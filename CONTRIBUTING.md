# Contributing

Thanks for looking at pbit. This page covers how to build, test and measure it, and the few
rules that keep its claims honest.

## Build and test

```
cargo build --release --workspace          # no network needed: there are no external crates
cargo test --release --workspace           # 127 tests: 3 core, 48 CLI, 11 stress (+1 ignored), 18 acceptance, 47 pbit-ir
cargo run --release -p pbit-cli -- demo --tasks 12 | cargo run --release -p pbit-cli -- decide --pretty
```

`acc1` in `pbit-decide/tests/acceptance.rs` has a wall-clock bound and can fail on a heavily
loaded machine; re-run it alone before reading anything into a failure.
`decide_fallback_enumerates_a_program_no_chain_can_start` in `pbit-cli/tests/cli.rs` is wall-clock-bound too
(a 4 s budget sized for a fast desktop) and skips itself when the `CI` environment variable is set; run it locally.

The default build is portable (no CPU pin). `RUSTFLAGS="-C target-cpu=native"` gives the last
bit of speed on your own machine; that binary may not run elsewhere.

## Ground rules

- **No external crates on the shipped path.** `cargo build` must keep working offline.
- **Every number in the docs points at something runnable**: a test, an example under
  `examples/`, or a script under `bench/`. A change that moves a number updates the table it
  lives in (`BENCHMARKS.md`, `USE-CASES.md`, `docs/pbit-ir-json.md`) with the machine, the load
  and `N` stated.
- **Fixed-work modes stay bit-identical.** With `--sweeps` and `--polish-sweeps` the whole
  output must not depend on the thread count or the run, except the fields that report the run itself: `ms`, the timings and
  rates in `telemetry` (`*_ms`, `site_updates_per_s`, `process_cpu_ms`, `peak_rss_mb`) and the echoed `threads` / `nice`. Remove
  those before comparing two runs or two binaries (a background job in zsh runs at nice 5, which alone changes the bytes). Anything that changes the RNG stream
  or the sampler's move order is a reproducibility break and needs a `CHANGELOG.md` entry.
- **Negative results stay runnable.** Examples that show an accelerator not helping, or a
  classical baseline winning, are kept so the comparison can be repeated.
- **`diagnostics_passed` keeps its definition** (it replaced "certified" in 0.2.0). It means every released item passed
  the versioned heuristic gate (a 3-sigma Monte-Carlo error bound plus mode diagnostics, calibrated on exact oracles); it
  is not a proof. Only the `exact` and `infeasible` verdicts are proof-grade (under the score model).
- **Stress corpus and calibration.** The inputs in `pbit-cli/tests/stress/` are frozen: never edit or
  regenerate them. Gate thresholds (`GATE`, `ITEM_RHAT_MAX`, `PARTIAL_RHAT`, `BATCH_RATIO_MAX`, `GATE_BS_POW` in
  `pbit-ir/src/lib.rs`, recorded by `bench/calibrate.py` in its header) are FROZEN before a holdout run and never retuned
  on its results: a holdout failure is reported in `BENCHMARKS.md` ("Known failure modes") and fixed by a new mechanism
  (a move, a diagnostic, an exact tier) validated on a NEW holdout, with `GATE_VERSION` bumped. `bench/calibrate.py
  --holdout` generates the holdout families from a seed; pick a new `--holdout-seed` for each new holdout.

## Benchmarks

The scripts in `bench/` drive the release binary through a subprocess. `router_ilp.py` and
`schedule_ilp.py` need `numpy` and `scipy >= 1.9` for the HiGHS baseline; the others need only
Python 3. Build first with `cargo build --release -p pbit-cli`.

## Reporting a problem

Please include the problem JSON (or the `pbit demo` seed and flags that produce it), the exact
command, the output of `pbit stats --pretty`, and the operating system. If the sampler returned
`refused` or `partial` where you expected `diagnostics_passed`, include the `gate` object from the
decision: it says which check failed.

## License

By contributing you agree that your contributions are licensed under the Apache License 2.0,
the same license as the project.
