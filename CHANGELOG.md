# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## 0.1.0 (unreleased)

Initial public release.

- `pbit-core`: a Philox4x32-10 counter RNG (reproducible, stream-splittable) and two p-bit
  kernels, a bit-sliced multispin kernel (64 replicas per `u64`) and an SoA `f32` heat-bath.
- `pbit-ir`: the instruction set of the virtual processor. Categorical variables with
  allowed / forbidden / forced values, unary weights, pairwise couplings and at-most-k caps;
  three tiers: exact (enumeration, a frontier dynamic program, minimum-remaining-values search),
  a constraint-preserving Gibbs sampler, and the certification gate.
- `pbit-decide`: the assignment-router front-end (tasks to workers under quotas, allowed sets,
  clamps and group affinity), lowered bit-identically onto `pbit-ir`; anytime mode; annealed
  plan polish; exact-oracle instance families for calibration.
- `pbit` CLI: `demo`, `decide`, `run`, `ir`, `stats` and `version`. JSON in, JSON out. Exit codes
  0 plan, 1 infeasible, 2 bad input, 3 refused. Processor controls from flags, `PBIT_*`
  environment variables or `pbit.json`: threads, chains, CPU limit, memory limit, priority,
  progress, fixed-work sweeps, exact-tier time cap, frontier cap, wall-clock budgets.
- Zero external crates: a fresh checkout builds offline with stable Rust.
