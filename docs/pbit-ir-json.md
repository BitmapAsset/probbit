# pbit-ir JSON v1: the wire format of the virtual p-bit processor

`pbit run < program.json` executes a general pbit-ir program. The assignment router (`pbit decide`) is one front-end that
lowers to the same IR (`Problem::lower` in `pbit-decide`); through that lowering the IR's enumeration, gate, and sampler with the
router's accelerators off reproduce the router bit for bit on the 7 acceptance cases (`ir_lowering_bit_identical`). `pbit decide`
itself uses the IR for the frontier DP and the certification gate (on `Problem::lower()`); its enumeration and its sampler (with the
accelerators) are its own.

## Program

```json
{ "pbit_ir": 1,
  "values": ["r", "g", "b"],
  "vars":  [ {"id": "a", "h": {"r": 0.5}, "allowed": ["r", "g", "b"], "forbid": ["b"], "clamp": "r"}, ... ],
  "pairs": [ {"i": "a", "j": "b", "potts": -2.0},
             {"i": "a", "j": "c", "table": [[0.0, 0.3, 0.1], [0.3, 0.0, 0.2], [0.1, 0.2, 0.0]]} ],
  "caps":  [ {"limit": 1, "value": "r"},
             {"limit": 1, "value": "g", "vars": ["a", "b"]},
             {"limit": 2, "members": [["a", "r"], ["c", "g"]]} ] }
```

- `values`: the shared value alphabet (k names). Every variable takes exactly one value.
- `vars[].h`: unary log-weights per value (missing = 0). `allowed` (default: all values) and `forbid` remove values
  (hard). `clamp` forces a value (hard; this is the what-if instruction).
- `pairs[]`: `potts: w` adds `w` to log w(x) when x_i == x_j; `table` (k x k, row = value of `i`, column = value of `j`)
  adds `table[x_i][x_j]`. Negative Potts = soft "differ" (colouring); a table expresses Ising / max-cut couplings.
- `caps[]`: at most `limit` of the listed (variable, value) pairs may hold at once (hard). `value` + optional `vars` is
  shorthand for "at most `limit` of these variables (default: all) take this value"; `members` lists pairs explicitly.
  Examples: worker quotas (`value` form), one-hot sudoku rows (`limit` 1 per digit), job-shop slot capacity.

The distribution is `P(x) ∝ exp(log w(x))` over the assignments that satisfy every hard rule:
`log w(x) = Σ_i h_i(x_i) + Σ_pairs J_ij(x_i, x_j)`.

## Instructions (`--op`)

- `decide` (default): three tiers, cheapest exact answer first.
  1. enumerate exactly when the feasible set is ≤ `--exact-limit` (default 2M; `tier: "enumerate"`, with `n_feasible`,
     `logz`, `top_plans`). The search is bounded: at most max(64 × limit, 1M) DFS nodes, and (programs that are not
     partitions) at most max(64 × limit, 10^8) capacity checks without finding a new plan, so a hard search declines in
     tens of ms (200-job schedule: 96 ms at the default limit, 75 ms at limit 1, was 1.15 s) instead of hunting
     for its first plan; `--op exact` is unbounded.
     Cost (`bench/csp_gap_probe.py`): a search for the first plan is also how infeasibility is proved, so hard
     infeasible CSPs came back `refused` (exit 3) instead of `infeasible` (exit 1). Fallback in the current build: with a wall-clock budget,
     on a program that is not a partition, each chain's feasible-start search stops at HALF of its slice of `--budget-ms` (the budget /
     ceil(chains / threads)); if any chain finds no start, all samples are dropped and the exact search runs again past its gap budget until the end of `--budget-ms` (proof -> `infeasible`; a program
     it enumerates in time -> `exact`; else `refused`, whose reason names `--op exact`). A proof that takes P needs a budget of
     about 2 x (P + 0.1 s): the tier's first attempt (~0.1 s) and the start search's half come first (random 3-colourings,
     150-200 vertices, mean degree 4.6: P = 1.9 s refused at 3.4 s, `infeasible` at 4.2 s; P = 0.19 / 0.25 s refused at 500 / 600 ms,
     `infeasible` at 1000 ms, where an earlier build refused all; P = 4.4 s `infeasible` at 9.5 s, in 7.1 s: a start search that
     gives up early leaves the proof more time).
     The fallback costs a feasible program nothing when its chains start within half the budget (200-job schedules at 5 s:
     173 / 157 released vs 170 / 157 before). `--op sample` and `--sweeps N` (deterministic runs) have no fallback;
  2. else the frontier DP (`tier: "frontier"`, exact `marginals`, `logz`, the exact MAP `plan`, `frontier_states`):
     for programs where every (variable, value) is in at most one `cap`; the connected components of the coupling graph
     are eliminated one by one, carrying only the loads of caps shared with components still to come. It declines (in
     milliseconds) when a component has > 4096 assignments, more than 16 caps are open at once, a DP layer exceeds
     `--frontier-states` (default 4096; 0 = off), or a cap's limit and member count both exceed 255 (its load lives in an 8-bit
     lane; before this rule such a program came back truncated but labelled `exact`, e.g. 600 free spins under an
     at-most-600 cap gave log Z 406.99 instead of 415.89). When the raw space (product of allowed values) exceeds `--exact-limit`
     the frontier runs before enumeration, which would otherwise spend its node budget first (80 / 300-variable team
     rosters: 169 ms / 1.09 s → 1.8 / 57 ms);
  3. else run 4 constraint-preserving Gibbs chains for `--budget-ms` (default 200) and apply the certification gate
     (`tier: "sample"`, `gate` object).
- `exact`: enumeration only. It is exhaustive, so use it only on small programs (it does not return quickly on large ones: a loose
  100,000-variable path program gave no answer within 60 s; with `--exact-ms 1000` it answers `declined` in 1.1 s).
- `sample`: chains + gate, never enumerate.
- `--sweeps N` replaces the wall-clock budget with N sweeps per chain (`--sweeps 0` = the wall-clock default). With `--polish-ms 0` or
  `--polish-sweeps M` the decision is then a pure function of (program, seed, `--chains` and the work/limit flags, without the wall-clock
  `--exact-ms`): byte-identical across runs (only timing
  fields differ; CLI tests
  `run_general_ir_programs`, `polish_sweeps_makes_the_whole_answer_deterministic`). With the default wall-clock polish
  (`--polish-ms 50`) the odds, verdict and released set are still identical, but the polished `plan` can differ run to run
  (997.89 vs 998.42 log w on the same 300-task input at `--sweeps 150`).
- `--polish-sweeps M` (both `pbit decide` and `pbit run`): polish for M sweeps per polish chain instead of `--polish-ms`
  of wall clock (deterministic; 400 sweeps cost ~20 ms on the 300-task demo). Telemetry reports `polish_sweeps`.

## Resource controls

Every control below works on both `pbit run` (this format) and `pbit decide` (the assignment front-end's sampler path).

- `--chains N` (env `PBIT_CHAINS`, default 4): independent chains (more chains = tighter between-chain checks, more work).
- `--threads N` (env `PBIT_THREADS`, default min(4, available cores)): worker threads; worker w runs chains w, w+N, ...
  Precedence: flag > environment > config file > default; values must be ≥ 1 (else exit 2). Config file:
  `$PBIT_CONFIG`, else `./pbit.json` if it exists, e.g. `{"chains": 4, "threads": 2, "cpu_limit": 50, "priority": "low"}`
  (CLI test `run_config_file_layer`). At fixed work the thread count does not change the answer: with `--sweeps`
  fixed and a fixed-work polish (`--polish-ms 0` or `--polish-sweeps M`) the decision is identical at 1, 2, 3, 4 and 6 threads
  (each chain's RNG stream is independent of its thread; CLI tests `run_threads_and_chains_are_resource_controls`,
  `polish_sweeps_makes_the_whole_answer_deterministic`). Under a wall-clock budget or polish it does (below). The polish runs its
  4 annealing chains on `--threads` workers (before, it used 4 threads whatever `--threads` said); with a wall-clock
  `--polish-ms` each chain then gets polish-ms / ceil(4 / threads), so `--threads 1` polishes less: on the 300-task demo
  (`--sweeps 400`, N = 5) CPU/wall went from 2.26 to 1.04 and the plan from 707.77 to 705.12 log w (median; `bench/polish_threads.py`).
  With a wall-clock `--budget-ms`, the budget is the deadline of the whole sampling phase: each chain gets
  budget / ceil(chains / threads), so fewer threads = fewer sweeps per chain = a looser (honestly reported) bound. In the current build
  that slice includes the chain's feasible-start search (before, a 200-job schedule sampled for 1.75 s on a 1 s budget:
  now 1.00 s); a start slower than the slice leaves the chain with no samples and the answer is `refused`, and the
  start search itself stops at the deadline (a 200-job program that cannot start: `--budget-ms 500` answered `refused` in 0.67 s (measured on an earlier build, before the fallback; not re-measured),
  was 2.4 s; `--op sample`: 504 ms inside the run, 0.58 s wall). The exact tiers,
  the gate and the polish run outside the budget (200-job schedule, `--budget-ms 1000`: 1.29 s wall), except the
  fallback of `--op decide` (the start search stops at half the slice and the fallback's deadline is the end of the budget,
  counted from the start of the call: the same schedule's refusal at 1000 ms now comes in 1.00-1.20 s, was 1.21-1.32 s).
  The exact tier's cost can dominate: on loose programs with more than `--exact-limit` plans it counts plans up to the limit
  before declining (`--budget-ms 200`, loose 3-colourings: 0.44 / 0.73 / 1.66 s wall at 300 / 1,000 / 3,000
  variables vs 0.29-0.35 s for `--op sample`). Lower `--exact-limit`, or use `--op sample`, to keep such calls near the budget (3,000 variables at
  `--exact-limit 1000` / `20000`: 0.38 / 0.39 s), or cap them with `--exact-ms N` (below).
- `--exact-ms N` (opt-in; absent = no cap, output unchanged): a wall-clock stop for the exact tiers that run before the
  sampler (enumeration / MRV search and the frontier DP, together), counted from the start of the call; the clock is read every 64
  search nodes and per DP step. Past it they decline as at `--exact-limit`: `--op decide` (and `pbit decide --mode auto`) goes on to
  the sampler with the full `--budget-ms`; `--op exact` answers `declined` (exit 3, the reason names `--exact-ms`); `pbit decide
  --mode exact` exits 2. A sampled answer then carries `telemetry.exact_ms` and `telemetry.exact_ms_reached` (only when the flag is
  given). Not capped: the no-start fallback above (its deadline is the end of `--budget-ms`), and the router's lowering before its frontier DP
  (O(group size^2) pairs) once it has started: one group of 3,000 tasks spent ~165 ms before the sampler at `--exact-ms 50` (found in a code
  audit); in the current build a cap that has already passed skips the lowering (55-57 ms, 3 runs; answers identical at fixed work). Loose 3-colourings at `--budget-ms 200`
  (re-run on the final binary of an earlier build, medians of 5): `--op decide` 430 / 699 / 1,480 ms at 300 / 1,000 / 3,000 variables ->
  341 / 347 / 346 ms at `--exact-ms 50` (`--op sample`: 292 / 296 / 296 ms). A cap shorter than a program's exact time turns its exact answer into a sampled one.
  The frontier DP stops per DP step: `pbit decide --frontier-states 4194304` answers the 42 / 60 / 90-task demos (`--seed 2`) exactly
  in 0.24 / 1.60 / 26.4 s (layers up to 27k / 121k / 855k states); with `--exact-ms 50` the whole call took 51 / 69 / 56 ms
  (a 60-sweep sampler run included; 1 run each).
- `--cpu-limit PCT` (env `PBIT_CPU_LIMIT`, 1-100, default 100): per-thread duty cycle (sleep after each ~2 ms of sweeping).
  Same answer at fixed `--sweeps`. Measured (2000-spin ring, 4 threads, fixed work, medians of 5): utilisation 98.4% /
  40.2% / 19.0% at 100 / 50 / 25, so the sweeping stays under the cap; but the same work cost 916 / 1104 / 3751 ms CPU and 233 / 688 /
  4948 ms wall: duty-cycled threads run much less efficiently on the M4 (likely lower clocks / efficiency cores, inferred).
  To cap the CPU share cheaply, lower `--threads` first. Only the sweep loop is duty-cycled; the exact tiers, the feasible-start
  search, the gate (one thread per chain per statistic, whatever `--threads` says) and the polish run at full speed.
- `--priority low|normal` (env `PBIT_PRIORITY`, default normal): `low` sets nice 10 via setpriority(2) before any chain
  starts (Unix; `ps` shows NI 10 during the run; on Windows `--priority low` exits 2: not implemented, untested). Reported as `telemetry.nice`.
  Under contention (M4, a 10-thread foreground run vs a 10-thread pbit hog, 5 reps): nice 10 alone did NOT yield on
  macOS (foreground slowed x1.78 by a normal hog, x1.73 by a nice-10 hog). So on macOS `low` also sets PRIO_DARWIN_BG (the
  background band: throttled, may stay on efficiency cores): the foreground is then slowed only x1.02. The price: a `low` run
  on an otherwise idle M4 takes x4.2 the wall time (x3.9 CPU) of a normal one (400-spin ring, fixed work; same answer).
  Use `low` to stay out of the way; use `--threads` to share the machine and stay fast. Linux (nice 10 = ~1/9 of a nice-0
  thread's CFS weight) is unmeasured.
- `--mem-limit-mb N` (env `PBIT_MEM_LIMIT_MB`, config `mem_limit_mb`; default 1024 in the current build, `0` = unbounded): caps the buffers that grow with the budget
  (per chain, a trajectory of n x 2 bytes + 8 bytes per kept sweep, needed for the gate's batch means) at N MB in total, never
  below 64 rows per chain. When a chain's buffer is full it keeps every other row and records every 2nd sweep from then on
  (equally spaced thinning); the odds still count every sweep, and the gate's error bar is computed on the thinned chain, which
  is conservative (the full-chain mean has variance <= a thinned-chain mean for any stationary chain; MacEachern & Berliner
  1994, lit.). Measured (earlier build, M4): 300-task `pbit decide --budget-ms 3000` peaks at 580 MB unbounded, 40.6 MB with
  `--mem-limit-mb 16` (same sweeps, certified both); against exact odds (forced sampler, 8 seeds each of the 18-task, 24-task and
  24-task `--hard` demos, unbounded vs 1 MB: 24 runs per setting) a 1 MB cap (~31x thinning; ~2x on `--hard`) released the
  same 528 tasks with 0 outside tolerance. **Where it costs**: on the slowly mixing
  300-task `--hard` demo (1 s budget, only ~13k sweeps, 36.6 MB peak unbounded) a 2 MB cap (~4x thinning) cut releases from
  300 to a median of 239 (5/5 `partial`); 8 MB barely thinned (300, 300, 300, 299, 300). Thinning a chain that mixes slowly
  leaves fewer rows for the same information, so the gate escalates more: set the cap generously; it is meant for long budgets.
  The capped buffer is now reserved once (it used to grow by doubling, so peak RSS reached ~1.6x the cap). 300-task
  demo, one run each: 10 s budget, peak RSS 1,667 MB unbounded, 1,632 MB at `--mem-limit-mb 1024` before the fix, 1,042 MB
  after; 3 s at 256 MB: 275.6 MB; 3 s at 16 MB: 33.5 MB (all certified, 300 released). The cap bounds the sample buffers, so peak
  RSS = cap + the process's base (~20 MB here). Unbounded RSS grows ~167-195 MB per second of budget on this demo (1 / 3 / 10 s:
  190.5 / 585.6 / 1,666.9 MB). Caveat: the reservation is virtual until touched on macOS/Linux; on Windows it is committed (untested).
  Default 1024 MB (300-task demo, 10 s: 1,024 MB peak vs 1,625 MB unbounded, N = 3; 200 ms: 41.7 vs 52.5 MB, N = 5; all
  certified 300/300). On a machine with less free memory than ~1.1 GB, set a smaller cap. Program, JSON and marginals are not capped.
  Telemetry: `mem_limit_mb`, `traj_rows` (rows kept, all chains).
- `--progress [MS]` (default 100): one JSONL line on stderr roughly every MS ms while the decision runs,
  `{"event":"progress","ms","sweeps","site_updates_per_s","process_cpu_ms","peak_rss_mb"}` (sweeps over all chains, counted in
  steps of 64; updates/s since the previous line; `sweeps` stays 0 while an exact tier runs). Stdout is unchanged. Cost at fixed
  work (two runs): router -3.0% / -0.6%, 400-spin ring +1.3% / +4.6%, 3-variable toy -3.1% / -8.3% site updates/s
  (IQRs overlap; tiny programs likely pay a real cost of up to ~8%).
- `pbit stats [--sweeps N]`: the processor's spec sheet: OS/arch/logical CPUs, compiled-in target features, every control's
  effective value and its source (`flag` / `env` / `file` / `default`), and a measured self-test (400-spin ring, fixed work) at 1
  thread and at the effective thread count. Apple M4, 7 runs: 23.05 M site updates/s on 1 thread, 77.65 M on 4 (x3.37).
- `pbit decide` threads (300-task demo, 4 chains x 400 sweeps, polish 0, 5 runs, medians): 74.1 / 58.5 / 50.9 ms wall at
  `--threads` 1 / 2 / 4 (sampler 14.6 / 27.1 / 47.4 M site updates/s; the exact tiers' decline is a fixed cost); answer identical.
- Measured (Apple M4, 400-spin ring, 4 chains x 2,000 sweeps, 5 runs, medians): `--threads 1` 139.0 ms wall /
  146.5 ms CPU; `--threads 2` 74.8 / 154.7; `--threads 4` 42.7 / 165.6 (3.26x faster for +13% CPU); marginals identical.

## Decision

`verdict`: `exact` | `certified` (the gate's 3σ Monte-Carlo bound on each variable's TV error is ≤ 0.05, plus split
R-hat < 1.05, ≥ 8 batches per chain, batch-size stability and no frozen caps; calibrated on exact oracles, not a proof) | `partial`
(`released` variables passed their own bound; `escalated` did not) | `refused` (exit 3) | `infeasible` (exit 1) |
`declined` (exit 3; `--op exact` stopped at `--exact-ms`, or ran out of its node budget, ~9.2e18 nodes: unreachable in practice, inferred). Every document
has `engine`, `op`, `program` (vars, values, pairs, caps counts), `verdict`, `ms`; `infeasible`, `declined` and a no-start
`refused` add only `reason`. Answers add `plan` (the best plan seen, or the exact MAP), `plan_logw`, `violations` (of `plan`),
`marginals` (per variable, values sorted by probability), `released`, `escalated`; exact tiers add `tier`, `logz`, `top_plans`
and `n_feasible` or `frontier_states`. Sampled answers add `tier`, `sampled_best_logw`, `gate` (rhat, tv_bound, tv_tol,
frozen_saturated_caps, frozen_escalated (variables the frozen rule escalates; on programs that are not partitions only the
stuck parts' connected components, forced variables included while anything is stuck), min_batches, samples, sweeps, chains, budget_ms (echoed even under `--sweeps`), seed) and
`telemetry`: chains, threads (= min(`--threads`, chains)), sweeps, site_updates_per_s, sample_ms, gate_ms, polish_ms, polish_sweeps,
cpu_limit_pct, mem_limit_mb, traj_rows, exact_ms and exact_ms_reached (only with `--exact-ms`; `exact_ms_reached` = the exact tiers ran and the clock at the
sampler's start was past the cap; always false under `--op sample` / `--mode sample`, where no exact tier runs), and on Unix
process_cpu_ms and peak_rss_mb (getrusage; matches `/usr/bin/time -l` on macOS) and nice (getpriority); all three null on
Windows. `pbit decide` reports the same `telemetry` on its sampler path (`sweeps` = all sweeps incl. burn-in, as `pbit run`); its
document differs: `tasks` / `workers` / `affinity` instead of `op` / `program`, `odds` instead of `marginals`, no `tier` or
`sampled_best_logw` on the sampler path, and gate `frozen_saturated_workers` plus `batch_ratio`. Bad input: exit 2 (bad JSON, an
unknown flag, a value that does not parse or is out of range, a value flag given twice (`--progress` too in the current build), `--mode` other than auto / exact / sample).

## Limits in v1 (measured or stated)

- Fixed in the current build (was a known false refusal, found by a kill test): on the router and any partition program, what-if clamps (or
  single-allowed tasks) that fill a worker's capacity on their own made the gate call the worker frozen and refuse (10/10
  clamped 24-task demos). `pbit_ir::FORCED_CAPS_PARTITION` is now on: such a capacity, and the classes of its forced members,
  are not a mixing signal (the rule non-partition programs have used since an earlier build). After the fix: clamped demos 10/10 certified, 240
  released, 0 outside ±0.05 of exact; 20 random-clamp demos 480 released, 0 false; acceptance `forced_caps_do_not_freeze_the_gate`.
  It moved 2 of 7 golden gate digests (inputs with forced legal tickets); the calibration families have no forced members.

- The frontier-DP exact tier runs on the IR; the assignment front-end calls it through `Problem::lower` (its earlier copy
  was deleted after matching it: same DP layers, marginals ≤ 3.3e-16, log Z ≤ 6.8e-13 apart). The router's mixing
  accelerators (group block moves, two-group moves, window move, focus) stay in the assignment front-end: its hot loops
  are kept specialized (IR sampler 0.95–1.25× the specialized one; IR enumeration +2.3% on the 300-task probe) and are
  bit-identical to the IR's with the accelerators off (acceptance `ir_lowering_bit_identical`). `pbit run` samples with
  site + swap Gibbs only.
- A feasible start is found by augmenting-path matching when every (variable, value) pair is in at most one `cap`
  (quotas; failure proves `infeasible`, exit 1), otherwise by a randomised depth-first search that branches on the
  variable with the fewest feasible values, each attempt capped at 2M nodes or 5e8 capacity checks, retried in ascending value
  order and with 4 fresh tie-breaks; under `--budget-ms` the search stops at the chain's deadline.
  With `--sweeps` there is none (an unstartable 200-job program: each chain gives up after 2.09-2.12 s). If the
  search gives up the answer is `refused` (exit 3), never `infeasible`: running out of budget proves nothing (under `--op decide`
  the fallback above may still prove it). In an earlier build a start search that went 6,000-8,000 variables deep ABORTED the
  process (stack overflow, exit 134, empty stdout): an 8,000-variable 2-colouring path under `pbit run --op sample` and `--op
  decide` at `--budget-ms 1000` (measured on that build); chain threads now get a stack sized to the program and the same
  runs answer `certified` in 1.7-1.8 s. At 30,000 variables the search gives up first: `refused` (exit 3), before and after.
  The exact tiers recurse once per variable too, on the caller's thread: in an earlier build a 100,000-variable clamped program
  aborted `pbit run --op decide` and `--op exact` (exit 134, main-thread stack overflow after 8.4 s); above 2,048 variables they
  now run on their own thread with a program-sized stack (the same runs: `exact`, exit 0, 0.07 s wall). The JSON
  front-ends look ids up in hash indexes (they were linear scans: a 30,000-variable program parsed in 0.56 s, now 0.03 s wall
  for the whole call; the router's 100,000-task input 12.1 s -> 3.8 s, and then -> 0.07 s: the remaining 3.75 s was the group-mates
  table, also a scan of every task per task, now bucketed by group with identical lists).
- Unique or near-unique solutions (e.g. sudoku): the chains cannot move, the gate reports frozen capacities and refuses
  (20/20 generated puzzles refused, 0 variables released, 0 false releases; re-run on a later build: the same). On programs that are
  not partitions `frozen_saturated_caps` also counts free variables no chain ever moved. In the current build a variable that unit propagation
  forces (a cap its forced members fill removes that value from the others, to a fixpoint) is a constant, not frozen, so a puzzle
  naked singles solve is `certified` by `--op sample`; and a frozen variable escalates only its connected component (free
  variables linked by pairs and caps), so independent parts can still be released (`partial`). A whole answer still needs
  nothing frozen. Partition programs (the router) are unchanged. `--op decide` answers exactly by
  enumeration instead. For programs that are not partitions (a (variable, value) pair in several caps, as in sudoku or
  colouring) enumeration branches on the most constrained variable (dynamic MRV): a test puzzle is solved and
  proved unique in 0.24 ms (was 660 ms), and under-determined puzzles get their exact per-cell odds and solution count
  (4,849-305,753 solutions in 0.11-8.1 s, per-puzzle medians, on an earlier build). A dedicated bitmask backtracking solver is still faster at the same full count: 14-18x on the open puzzles, 32-74x on
  the unique ones (same machine; re-measured with the timings above, 5 runs: 15-20x and 36-102x per puzzle, medians 17-18x and 48-53x).
- Plan polish: a sampled answer's `plan` is the chains' best plan annealed for `--polish-ms` (default 50; beta 2 -> 32,
  never worse; `sampled_best_logw` = before polish). Not a hard deadline: each router polish chain (`pbit decide`) builds its first
  temperature's chain before it reads the clock (~20 ms on 100,000 tasks in groups of 4). Earlier builds measured ~2 s over on that shape
  (~7.7 s at `--threads 1`); the cause was an O(t^2) plan score, fixed in the current build (82-109 ms after sampling at `--polish-ms 50`). Heuristic, not an optimum: on max-cut an untuned simulated annealing
  still finds larger cuts at equal time (anneal-only mode 0.06% / 0.7% behind, sampling mode 0.17% / 1.75%; BENCHMARKS §3).
- Measured on an Apple M4: a 60-spin Ising ring (table couplings) certified in 200 ms, max per-spin error 0.0032 vs
  the exact transfer-matrix marginals (bound reported 0.0039).
- Measured on an Apple M4 (seeds 1–3, polish off): on 80 / 300-variable team rosters (3^80 / 3^300 plans, beyond
  enumeration) the sampler certified 6/6 with true max per-variable TV 0.0032–0.0089 vs the frontier's exact marginals,
  under its reported bound in 6 of 6 runs (0.0055–0.0117). Its best-seen plan was far from the exact MAP (log w 26.9 vs 31.5,
  87 vs 119): for the single best plan, use the exact tier or the polish.
