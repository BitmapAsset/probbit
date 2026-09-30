# Contributing

Thanks for looking at pbit. This page covers how to build, test and measure it, and the few
rules that keep its claims honest.

## Build and test

```
cargo build --release --workspace          # no network needed: there are no external crates
cargo test --release --workspace           # 65 tests: 3 core, 26 CLI, 14 acceptance, 22 pbit-ir
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
  output must not depend on the thread count or the run. Anything that changes the RNG stream
  or the sampler's move order is a reproducibility break and needs a `CHANGELOG.md` entry.
- **Negative results stay runnable.** Examples that show an accelerator not helping, or a
  classical baseline winning, are kept so the comparison can be repeated.
- **"Certified" keeps its definition.** It is a 3-sigma Monte-Carlo error bound calibrated on
  exact oracles, not a proof. Do not describe it as one.

## Benchmarks

The scripts in `bench/` drive the release binary through a subprocess. `router_ilp.py` and
`schedule_ilp.py` need `numpy` and `scipy >= 1.9` for the HiGHS baseline; the others need only
Python 3. Build first with `cargo build --release -p pbit-cli`.

## Reporting a problem

Please include the problem JSON (or the `pbit demo` seed and flags that produce it), the exact
command, the output of `pbit stats --pretty`, and the operating system. If the sampler returned
`refused` or `partial` where you expected `certified`, include the `gate` object from the
decision: it says which check failed.

## License

By contributing you agree that your contributions are licensed under the Apache License 2.0,
the same license as the project.
