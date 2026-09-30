# pbit benchmarks

Every section names the command that produced it (a `bench/` script or `cargo run --example`, from the repo root; `PBIT=path/to/pbit` overrides the binary);
numbers carried over from an earlier build are marked as such and were not re-run.
Machine unless stated: Apple M4 (4 performance + 6 efficiency cores), 16 GB, macOS, rustc 1.98.1, `cargo build --release`
(fat LTO, **portable**: no `target-cpu` pin). Timing runs were made with no other benchmark running (except §5.4, contention on
purpose), but not always on an idle machine: during some runs fseventsd used ~190% CPU, and other runs logged load averages of 3-17 from other
processes; sections give the load where it was recorded.
Format: median [interquartile range] over N runs, unless a cell says p50/p95 or min-max. Each table ends with what it lets us claim
and what it does not.

**Certified / released** (every section): an item's odds passed the gate, i.e. a 3σ Monte-Carlo error bound of at most 0.05 total
variation from multi-chain batch means, plus split R-hat, batch-size stability and frozen-resource checks, calibrated against exact
oracles. It is a statistical check, not a proof. **Oracles**: router family A (§2.1) is checked against a transfer-matrix DP
(`pbit-decide/src/oracle.rs`), separate code from the sampler and the exact tiers. Every other oracle is pbit's own exact tier
(`pbit_ir::exact` enumeration for max-cut, colouring and scheduling; the frontier DP for router family B), not an independent
solver. Cross-checks: that enumeration equals an independent bitmask DFS on 20 sudokus (counts and per-cell odds, §4) and the
frontier DP equals enumeration to 1e-9 (earlier build); a truncation bug found in `exact()` was fixed, and every oracle total re-ran
unchanged afterwards.

## §1 Kernel and sampler updates/s (portable build, `bench/portable_vs_native.py`, N = 5)
The p-bit literature counts "flips/s" (p-bit updates per second). Ours, from the fastest structured kernel down to general programs:

| what runs | threads | site updates/s |
|---|---|---|
| `pbit-core` multispin, 2D ±J lattice L = 512, 64 replicas bit-sliced per word (fast path) | 4 | 1.48e11 [1.43e11-1.48e11] |
| same | 10 | 1.47e11 [1.44e11-1.48e11] |
| same, one thread | 1 | 3.71e10 [3.67e10-3.72e10] |
| `pbit-core` heat-bath, f32 lookup table, one lattice (fast path) | 1 | 9.13e8 [9.01e8-9.15e8] |
| `pbit-ir` general sampler, 400-spin ring program (`pbit stats`) | 1 | 2.28e7 [2.23e7-2.31e7] |
| same | 4 | 7.68e7 [7.63e7-7.74e7] |
| router sampler, 300-task demo (`pbit decide`, 2,000 sweeps) | 4 | 5.31e7 [5.18e7-5.40e7] |

Claim: the substrate's structured kernels reach ~1.5e11 replica-site updates/s on a Mac mini; a sparse general program (the
400-spin ring) runs at ~2.3e7 updates/s per thread, denser ones slower (router ~1.3e7 per thread, the G1-like max-cut ~3e6, §3).
The first four rows are standalone kernels (`pbit-core/examples/kernels`); `pbit decide` / `pbit run` use only pbit-core's RNG. Not claimed: that a general program gets kernel speed (the general sampler is ~40x slower per
thread than the one-lattice heat-bath kernel and ~1,600x slower than the multispin figure); the multispin rate counts 64
independent replicas per word, so one problem does not get 64x faster; FPGA/ASIC p-bit numbers *(lit.)*, not quoted here, are not
comparable to these without the same problem and metric. The fast multispin path stops scaling past 4 threads on the M4
(1.48e11 at 4 and 10 threads; the older multispin path goes 9.37e10 -> 1.27e11), likely memory-bound (inferred).

## §2 Router: pbit vs greedy+repair vs exact vs ILP (`router_bench.rs`, `bench/router_ilp.py`, `bench/router_latency.py`)
The router assigns tasks to workers under hard rules (allowed workers, quotas) with soft scores and a same-workflow affinity.
Methods: **exact** = the frontier tier `pbit decide` runs first; **greedy** = best-score-first matching with augmenting-path repair
(`feasible_init`); **greedy+polish** = greedy, then the same anneal pbit uses, for the same wall time pbit spends; **pbit** = what
`pbit decide --mode sample` runs (4 chains for 200 ms, the certification gate, 50 ms polish of the best plan). A task is
*released* when the gate certifies its odds; it is *wrong* if its odds are more than 0.05 TV from the exact odds.

### 2.1 Against exact odds and the exact optimum (`cargo run --release -p pbit-decide --example router_bench`, 36 s)
A: 16 exact-oracle queues (chains of blocks, 8 settings incl. saturated and strong affinity, T = 200 x 12 and T = 80 x 4, 2,720
tasks; exact odds and optimum by transfer-matrix DP).

| method | ms p50 / p95 | plan gap to the optimum, nats p50 (max) | tasks whose most likely worker matches the exact odds | tasks released (wrong) |
|---|---|---|---|---|
| exact frontier tier | 3.17 / 5.65 | 0 (0) | 100% (odds error 3.7e-15) | all, exact |
| greedy | 0.025 / 0.042 | 176.5 (1,060) | 51.9% (a plan, no odds) | none |
| greedy+polish, equal time | 259.0 / 267.4 | 0.151 (26.3) | 79.2% (a plan, no odds) | none |
| pbit | 258.9 / 267.3 | 0.011 (3.93) | 96.6% | 1,580 of 2,720 (0 wrong); 7/16 queues certified whole |

A replication on a later build gave the same exact, greedy and pbit rows (pbit released 1,590, 0 wrong);
greedy+polish, being wall-clock bound, came out at 0.081 nats median instead of 0.151.

B: `pbit demo --tasks N` queues, N = 12 / 18 / 24 / 30, 4 seeds each (336 tasks; exact odds from the frontier tier with a raised cap).
The exact tier answers in 0.17 / 1.47 / 5.83 ms at N = 12 / 18 / 24 and declines N = 30 at its default 4,096-state cap. Greedy is
2.95 / 6.68 / 8.14 / 9.14 nats (median) below the optimum. Greedy+polish and pbit both reach the optimum on 16/16 queues. pbit
certified 16/16 and released 336/336 tasks, 0 wrong; its odds name the exact most likely worker for 98.6-100% of tasks, while a
single optimal plan agrees on 71-78%. Claim: on these queues with an oracle, pbit's released odds were not once wrong (1,916 released
tasks, 0 wrong) and its plan was at least as close to the optimum as greedy+polish at equal time (0.011 vs 0.151 nats median on A).
Not claimed: coverage everywhere (A: 58% of tasks released; nothing was released on the 4 saturated queues, whose odds were within
0.011-0.087 TV, i.e. two of them were refused needlessly, nor on the 4 strong-affinity T = 80 queues, off by 0.04-0.44, one of them
(0.041; 0.042 in a later re-run) also needlessly: 3 of the 8 zero-release queues were inside tolerance) or that the
plan is optimal (up to 3.93 nats short on A).

### 2.2 The single best plan vs an ILP solver (`python3 bench/router_ilp.py`, needs numpy + scipy >= 1.9, N = 5)
Exact MAP as an integer program (HiGHS through scipy 1.17.1 `milp`; a benchmark dependency, not a pbit one), same queues as
`pbit demo` (generator signatures checked identical). HiGHS proves optimality (status 0) on every queue.

| queue | ILP: optimum, ms median [min-max] | `pbit decide` defaults: plan log w, gap | pbit ms | greedy / greedy+polish gap (2.1 harness) |
|---|---|---|---|---|
| 30 tasks | 69.88, 7.5 [7.0-11.9] | 69.88 certified, 0 | 287.0 | - |
| 300, seed 7 | 708.31, 52.9 [52.6-54.7] | 707.30 certified, 1.01 | 299.6 | 109.79 / 0.98 |
| 300, seed 8 | 690.09, 347.6 [345.4-350.1] | 689.20 certified, 0.89 | 298.6 | 99.87 / 0.37 |
| 300, seed 9 | 677.31, 111.7 [111.2-114.3] | 675.59 certified, 1.72 | 298.6 | 104.84 / 2.10 |
| 300, seed 10 | 694.59, 52.4 [52.4-53.7] | 692.71 certified, 1.88 | 299.2 | 107.58 / 1.48 |
| 300 `--hard` | 1012.21, 212.3 [211.6-214.4] | 1002.50 partial, 9.71 | 344.8 | - |

Claim: none for pbit here: for ONE best plan an off-the-shelf ILP solver is optimal, proves it, and was faster than pbit's
default path on 5 of 6 queues. pbit's plan is 0.9-1.9 nats short at 300 tasks (9.7 on `--hard`; a replication on a later
build gave 0.8-2.0 and 9.3 with ILP times within 4%; a third run on a still later build gave 0.45-1.91
and 9.71 with ILP times within 3%: the gap moves run to run with the wall-clock polish, 0.45-2.0 nats over three runs), and greedy+polish at equal time
was closer to the optimum than pbit's sampler+polish on all 8 seed-runs at 300 tasks (0.37-2.10 vs 0.42-2.75 nats; the `router_bench` run and its
replication). What the ILP does not return: per-task odds, a certificate that those odds are right, a refusal when they
are not, or what-if odds under clamps; that is what the pbit column is for. Not measured: whether those odds are worth more to a
user than LP relaxation values or a plain MCMC run would be.

### 2.3 End-to-end latency at defaults (`bench/router_latency.py`, N = 20; re-run)
Wall = the whole subprocess (start, parse, decide, print); pbit ms = the time pbit reports.

| queue | verdicts | pbit ms p50 / p95 | wall ms p50 / p95 |
|---|---|---|---|
| 12 tasks | exact (enumeration) x20 | 7.1 / 7.5 | 9.0 / 9.6 |
| 24 tasks | exact (frontier) x20 | 25.1 / 25.5 | 27.2 / 27.9 |
| 30 tasks | certified x20 | 287.1 / 288.6 | 290.6 / 291.9 |
| 300 tasks | certified x20 | 299.0 / 301.1 | 303.1 / 305.5 |
| 300 tasks `--hard` | partial x20 | 344.2 / 345.9 | 348.1 / 350.2 |

The frontier cap is a knob (`--frontier-states`; `bench/frontier_cap.py`, N = 5, normal and `--hard` queues): at 16,384 states
instead of the default 4,096, the 30- and 36-task queues are answered exactly in 40.8-104.0 ms instead of 277.8-304.0 ms sampled;
queues it still declines pay +22.5 to +33.2 ms at 42-60 tasks and +3.6 to +5.9 ms at 90-300 tasks. At 65,536 (N = 5 probe) the
48-task queue is exact in 416 ms, slower than sampling it, and 300 tasks pay +114 ms. The default stays 4,096: which cap is better
depends on your queue sizes (unmeasured on real traffic). Load average was ~9.7 during the cap run (another process was busy). Claim: small queues get exact answers in milliseconds, and the sampled path is budget-bound at ~0.3 s with a tight p95.
Not claimed: that the sampled path is fast; it spends its whole budget by design.
## §3 Max-cut: odds vs brute force, and cut value vs simulated annealing (re-runs of `maxcut_oracle.rs` and `maxcut_gset.rs`)
Odds vs brute force (`cargo run --release -p pbit-ir --example maxcut_oracle`, 2.3 s): random 3-regular graphs n = 12 / 16,
coupling 0.3 / 0.7 / 1.5 per cut edge, fields 0 or uniform ±0.2, 4 chains x 500 or 5,000 sweeps, 6 instances per setting =
144 runs, each checked against the exact marginals from enumerating all 2^n states.

| sweeps per chain | runs | certified | false certificates | released spins | wrong released spins |
|---|---|---|---|---|---|
| 500 | 72 | 11 | 0 | 417 | 0 |
| 5,000 | 72 | 61 | 0 | 929 | 0 |

Claim: on these frustrated Ising programs no certificate and no released spin was wrong (tolerance 0.05 TV), and more work buys
coverage (at coupling 1.5 and 500 sweeps the gate released nothing). Not claimed: correctness beyond n = 16 (brute force stops there).

`cargo run --release -p pbit-ir --example maxcut_gset`. The graphs are generated with the shapes of the Gset recipes, NOT the
Gset files, so literature best cuts do not apply. Equal wall time (200 ms), 4 threads per method, 5 seeds, best cut median [IQR].

| graph | pbit sample (beta 2) | pbit sample 100 ms + anneal 100 ms | pbit anneal only | simulated annealing (geometric beta 0.2 -> 5, untuned), 4 restarts |
|---|---|---|---|---|
| G1-like: Erdos-Renyi n = 800, p = 0.06, 18,918 unit edges | 11450 [11437-11454] | 11453 [11447-11460] | 11463 [11463-11469] | **11470** [11465-11470] |
| G11-like: torus 20 x 40, weights ±1 | 562 [558-562] | 568 [564-568] | 568 [566-568] | **572** [570-572] |

Throughput: pbit 11.4-12.2 M site updates/s (G1-like, 4 chains) and 58-62 M (G11-like); SA 76.3 M and 308 M Metropolis
flip attempts/s (a cheaper operation than a heat-bath update). Claim: pbit's anneal-only mode gets within 0.06% (G1-like) and
0.7% (G11-like) of an untuned SA (fixed geometric schedule) at equal time, and its sampler, the mode that gives odds, is 0.17% and
1.75% behind; and its exact tiers (not this table; earlier builds) give exact odds and log Z on small instances. Not claimed:
that pbit finds better cuts. SA wins both, and a dedicated max-cut heuristic would likely win by more (unmeasured).
## §4 Sudoku vs backtracking, and graph colouring vs exact odds (re-runs of `sudoku_bench.rs` and `colouring_oracle.rs`)
`cargo run --release -p pbit-ir --example sudoku_bench`. "Unique" puzzles have one solution; "open" puzzles have extra givens
removed (4,849 to 305,753 solutions). DFS = bitmask backtracking with minimum-remaining-values.

| task | bitmask DFS (MRV) | pbit | pbit / DFS |
|---|---|---|---|
| solve + prove unique, all 20 puzzles | 0.011 [0.008-0.018] ms | sampler + gate 114.7 [114.3-115.1] ms: refused 20/20 (0 released, 0 false) | — |
| count all solutions + exact per-cell odds, unique (10) | 0.0185 ms median | exact tier (MRV enumeration) 0.787 ms | 44x (32.5-74) |
| same, open (10) | 35.7 ms median | 567.6 ms | 16.7x (14.6-18.0) |

A later re-run on the engine with the exact-tier gap budget (N = 5 runs, alternating with the engine before it, `MS=5`, median [IQR] of per-run medians): exact tier
0.818 [0.815-0.819] ms unique / 606.3 [605.7-607.7] ms open vs DFS 0.017 / 35.5 ms, i.e. 48-53x / 17-18x (per puzzle 36-102x / 15-20x;
the largest ratios are on puzzles the DFS finishes in ~4 µs). That gap budget (§6) costs 1.4% (unique, engine before it 0.807
[0.803-0.814] ms) and 4.2% (open, 582.1 [581.1-587.5] ms) here: IQRs disjoint, answers and odds identical (error 0 on all runs).
A later change then moved that count out of the inner loop (same count, same answers); one run afterwards: 0.956 ms unique / 567.0 ms open.
An A/B comparison (script not included in this repository; 12 generated puzzles (`N=6`), 3 runs alternating with the previous engine): after a later change
moved the exact search's stop checks out of its hot path, the IR exact tier is **17% faster** (per-puzzle median new / old 0.827, range
0.811-0.850; e.g. 380 vs 465 ms, 2,644 vs 3,121 ms on open puzzles), solution counts identical; the same change measured 0.832-0.942 on
4 colouring programs. The cause is inferred (code layout from the `#[cold]` split), not proven. The table above is not re-run:
the DFS still wins by more than an order of magnitude *(inferred from the ratios: ~14x / ~37x)*. Measured later (`N=3`: 6 puzzles,
1 run, final binary): 13.3-15.6x on open puzzles, ~37x on unique ones; counts identical, odds error 0, sampler refused 6/6.

pbit's exact odds equal the DFS odds on all 20 puzzles (error 0). Claim: pbit expresses sudoku as a program, gives exact
per-cell odds on puzzles with many solutions, and its sampler refuses rather than guess. Not claimed: speed. A dedicated
backtracker is 15-102x faster at the same exact job (per puzzle, re-runs above), and the sampler cannot certify these frozen, one-hot-constrained
programs at all (later re-run: 20/20 refused, 0 released). A puzzle that unit propagation (naked singles) solves is the exception in
the current build: every cell is then a proven constant and `--op sample` certifies it (the CLI test's puzzle); none of these 20 is one.

Graph colouring (`cargo run --release -p pbit-ir --example colouring_oracle`): random G(n, p) with mean degree 3, two vertices
pre-coloured by clamps, soft colour preferences. The IR's exact tier (pbit's own enumeration, see Oracles above) gives the
reference odds; the sampler (4 chains, 100 ms) is
scored on every vertex it releases.

| setting | graphs | colourings (median, range) | exact tier ms, median [IQR] | sampler verdicts | released vertices | outside ±0.05 TV |
|---|---|---|---|---|---|---|
| n = 12, k = 4 | 20 | 4,320 (198-22,356) | 0.30 [0.20-0.80] | 19 certified, 1 refused | 228 | 0 |
| n = 12, k = 5 | 20 | 93,321 (28,440-2,048,000) | 5.70 [4.00-8.62] | 20 certified | 240 | 0 |
| n = 14, k = 3 | 14 colourable | 97.5 (4-864) | 0.00 [0.00-0.03] | 4 certified, 1 partial, 9 refused (later re-run; earlier builds: 14 refused) | 57 (earlier builds: 0) | 0 |

Claim: on these colouring programs the gate released 525 vertices and none was outside tolerance (later re-run; earlier builds released
468, all 3-colourings refused). A later build changed the stuck-chain rule for programs that are not partitions: a vertex its clamped
neighbours force (unit propagation: a cap forced vertices fill removes that colour from the others, to a fixpoint) is a constant,
not a stuck chain; and a stuck vertex now escalates only its connected component instead of the whole answer. Over 6 sets
(h scale 1 and 6 x 3 seed bases, this one included; M4, 4 chains, 100 ms): 82 colourable 3-colourings -> 14 certified, 9 partial,
59 refused (all 82 refused before), 219 released vertices, 0 outside tolerance (max 0.016); k = 4 / 5 unchanged on every set.
Not claimed: that the sampler is needed at this size (the exact tier is faster than the 100 ms sampler on every instance), or
coverage on few-solution graphs (k = 3: still 59 of 82 refused). With 10x the sampler time (1 s, default set) the k = 3 verdicts
are identical (4 / 1 / 9): the refusals are structural. Of those 9, 6 had a vertex off by 0.11-0.46 (the stuck rule is right to refuse)
and 3 were within 0.048 (over-refusals). Kill test for the component rule (`--example stuck_twins`):
a rigid 3-coloured part next to a 4-coloured part that mixes, joined by one edge in half the programs; 4 seeds, 120 programs (85 with
a frozen variable): 938 released vertices (430 + 508 from the raw CSVs of two runs, not included here; first reported as 859, a mis-sum found in
a later audit), 0 outside tolerance. A deliberately broken rule whose caps do not link components
released 21 vertices outside tolerance on the first 62 of them, so the test can fail.

### 4.3 Scheduling vs exact odds, and vs greedy + repair (`cargo run --release -p pbit-ir --example schedule_oracle`, ~25 s)
Unit-time jobs are variables, time slots are values. Hard rules: a release/deadline window per job, at most `cap` jobs per slot,
precedence i -> j as pair caps (at most one of (i, a), (j, b) for every a >= b; O(T^2) caps per edge). Soft: weighted completion
time plus noise. Baseline: greedy list scheduling (topological, heaviest ready job first, best slot not later than its latest
start) + repair (move / swap hill climbing); "restarts" repeats it with random ready-job orders for the same wall time.
M4, load average 5-7 from another process; sampler 4 chains / 4 threads, 100 ms per method.

| setting | instances | schedules (median, range) | exact tier ms, median [IQR] | sampler verdicts | released jobs | outside ±0.05 TV | MAP found: greedy+repair / restarts / pbit |
|---|---|---|---|---|---|---|---|
| 10 jobs, 6 slots, cap 2 | 20 | 15,559 (861-100,120) | 1.9 [1.1-3.8] | 20 certified | 200 | 0 (max 0.0058) | 9/14 (6 greedy dead ends) / 20/20 / 20/20 |
| 12 jobs, 6 slots, cap 3 | 19 feasible | 634,026 (48,186-6,148,512) | 30.1 [13.6-79.9] | 19 certified | 228 | 0 (max 0.0050) | 8/19 / 19/19 / 19/19 |

Larger, no oracle (plan log w at equal 100 ms; pbit = anneal from the plain greedy plan; 5 seeds):

| setting | precedence edges (median) | pbit minus restarts, per seed | pbit better | single greedy+repair |
|---|---|---|---|---|
| 200 jobs, 40 slots, cap 10 | 181 | -0.18, +4.63, +2.92, +2.05, +4.06 | 4/5 | 0.36 ms, 8.2 log w below pbit (median) |
| 500 jobs, 50 slots, cap 20 | 469 | +4.02, +17.41, +3.27, +3.18, -7.34 | 4/5 | 2.5 ms, 15.7 below pbit (median) |

Claim: scheduling is expressed in the existing IR (no new constraint types; at 100-200 jobs it needed engine fixes: the anneal
warm start and the feasible-start retry; the exact tier's gap budget, the start retries and the start inside the
budget); on 39 oracle instances the gate released 428 jobs and none was
outside tolerance; at 200-500 jobs the anneal from a greedy start beat a restarted greedy + repair at equal time on 4 of 5 seeds.
Not claimed: optimality at scale, or that restarts are the strongest heuristic. A later run added the ILP baseline
(`/opt/homebrew/bin/python3 bench/schedule_ilp.py`, scipy HiGHS; three 200-job x 40-slot programs, cap 10, 77-90 K precedence
pair caps = schedule_oracle `PROBE_START=200 PSEED=0/1/2`, N = 3 pbit runs each, Apple M4): HiGHS PROVES the optimum in
0.68-0.95 s (about 6,000 binaries x 78-90 K rows); `pbit run --op decide` is 3.27-4.26 nats short of it at `--budget-ms 2000`
(refused, 0 released) and 3.27-3.95 short at 5000 (partial 156-173 released on 2 programs, refused on the third; wall 6.2 s;
0 violations in all 18 runs). For the best schedule, use a MIP solver: pbit adds the per-job odds and the refusal, not the plan. Part B anneals from the greedy plan because, when it ran, the sampler could not start: its random
feasible start (a 2M-node MRV search at O(n k caps) per node) gave up after 105.3 s on the feasible 200-job program
(`PROBE_START=200`). Since then that search has a work budget and retries in ascending value order (MRV, then input order): start
found in 0.68-0.73 s (5 runs, 3 instances); sampler at a 1 s budget: refused (R-hat 1.0043), at 5 s: partial on all 3
instances, 186 / 186 / 181 of 200 jobs released (R-hat 1.0006 / 1.0003 / 1.0007), 0 violations. With the start inside the budget
(one run each, load 10-17 from other processes): 1 s refused on all 3 (1.06-1.19 s total incl. gate, was ~1.97 s); 5 s
partial 170 / 170 / 153 of 200 (R-hat 1.0005 / 1.0006 / 1.0016), 0 violations; re-run at load 6.5-7: 171 / 170 / 154, so the
drop from 186 / 186 / 181 follows the ~0.7 s of start now inside the budget, not the load (inferred). No oracle exists at 200 jobs, so
those odds are unchecked. The retry never fired on the sudoku
(§4) or colouring runs (0 of all their starts), so their starts are unchanged.
At 100 jobs (`PROBE_START=100`, 3 instances) one instance could not start 4 chains: 3 of 8 chain streams gave up
without retrying (their random attempt ran out of nodes, not work, and only the work budget triggered the retry) and 2 still
failed both retries. Now any budget exit retries, then up to 4 more MRV attempts with fresh random tie-breaks (only after every
earlier attempt failed, so earlier starts are unchanged): 8 of 8 streams start (slowest 1.36 s). Sampler, 4 chains: at 1 s
refused / partial 76 / refused (R-hat 1.0024 / 1.0010 / 1.0004); at 5 s certified 100 / certified 100 / refused (the third has 6
frozen capacities; later re-run at 5 s with per-component escalation: certified / certified / refused, 0 of 100 released, frozen 6). The 1 s verdicts were measured with the start
outside the budget (earlier build) and were not re-run; the 5 s verdicts were re-run on a later build (start inside the budget) with the same outcome. Unchecked: no oracle at 100 jobs either.
Cost of the extra attempts on a program that cannot start (`PCAP=4 PROBE_START=200 PROBE_STREAMS=2`: 200 jobs, 160
slot places): a chain reports no start after 2.09-2.12 s, vs 0.69-0.75 s with the reranks off (the start search did not
watch the budget then, so a `--budget-ms` shorter than that was overrun). It now stops at the budget's deadline: `pbit run
--budget-ms 500` on that program answers `refused` ("no feasible start found within the search budget") in 0.666 s (measured before the exact-search fallback was added, not re-measured; median of 5;
was 2.42 s, median of 3; 0.845 s with the clock read every 4,096 nodes, ~170 ms apart here, now every 64). `--op sample`
(no exact tiers): 504 ms inside the run, 0.575 s wall.

## §5 Processor controls

### 5.1 `--threads` on the router (`bench/decide_threads.py`, N = 5)
300-task demo, `pbit decide --sweeps 400 --chains 4 --polish-ms 0` (fixed work):

| `--threads` | wall ms | process CPU ms | sampler site updates/s | answer |
|---|---|---|---|---|
| 1 | 74.1 [72.7-74.3] | 78.3 | 14.62 M | reference |
| 2 | 58.5 [57.8-59.1] | 79.9 | 27.08 M | identical |
| 4 | 50.9 [50.1-51.7] | 83.6 | 47.40 M | identical |

Claim: at fixed work (fixed `--sweeps`, and `--polish-ms 0` or the newer `--polish-sweeps`) the answer is bit-identical at 1, 2 and
4 threads (tested; chains are seeded per chain, not per thread), and the sampler scales 3.2x on 4 threads. With the default wall-clock polish only the polished plan can vary (in the current build). Not
claimed: wall time scales the same way; the serial part (exact tiers' decline, parse, gate: ~40 ms here, inferred as wall
minus sampler time) dominates at this size.

### 5.2 `--progress` cost (`bench/progress_overhead.py`, N = 7 alternating)

| program (fixed work, 4 chains / 4 threads) | off, M updates/s | on, M updates/s | change |
|---|---|---|---|
| router, 300-task demo, 2,000 sweeps | 54.00 [52.68-54.32] | 52.39 [51.89-53.34] | -3.0% |
| IR, 400-spin ring, 4,000 sweeps | 78.16 [77.08-78.96] | 79.21 [78.83-79.51] | +1.3% |
| IR, 3-variable toy, 400,000 sweeps | 82.57 [72.44-85.93] | 80.03 [75.31-83.13] | -3.1% |

A second full run (verification) gave -0.6% / +4.6% / -8.3% (toy IQRs 72.28-86.91 off vs 68.94-75.36 on, still overlapping).
Claim: on the two realistic programs the monitor costs within about ±5% and did not change the answer in these runs. Not claimed: zero cost.
On tiny programs it plausibly costs up to ~8% (inferred cause: 4 threads adding to one shared counter every 64 very short sweeps).
The off-path cost against a build without the counter was not A/B measured.

### 5.3 `--mem-limit-mb` (`bench/mem_limit.py`, `bench/mem_limit_hard.py`)

| case | unbounded | capped |
|---|---|---|
| 300 tasks, `--budget-ms 3000`, peak RSS (N = 3, median, min-max) | 579.9 MB (460-622) | 40.6 MB (40.6-41.8) at 16 MB |
| 18/24-task demos vs exact odds, forced sampler, 24 runs per column | 528 released, 0 outside ±0.05 TV | 528 released, 0 outside, at 1 MB (~31x thinning) |
| 300-task `--hard`, 1 s, released tasks (N = 5 seeds) | 300 (5/5 certified) | 300/300/300/299/300 at 8 MB; 239 median at 2 MB (5/5 partial) |

A later measurement (one run each, 300 tasks): unbounded peak RSS grows ~170-195 MB per second of budget (190.5 / 585.6 / 1,666.9 MB at 1 / 3 /
10 s). The 299 at 8 MB in the table is within run-to-run noise (an unbounded replicate also gave 299). At 10 s, `--mem-limit-mb 1024` peaked at 1,632 MB in an earlier build, because the capped buffer grew by doubling. Now it is reserved
once and peaks at 1,042 MB (3 s at 256 MB: 275.6 MB; 3 s at 16 MB: 33.5 MB; all certified).

Claim: the cap bounds the buffers that grow with the budget (peak RSS = cap + ~20 MB base in the current build), and in these runs it never
produced an answer outside tolerance. Not claimed: the cap is free. Thinning a slowly mixing chain costs releases (third row).

**Default in the current build: 1024 MB** (`--mem-limit-mb 0` = unbounded, the old default). Measured (M4, 300 tasks, load average 3.9-6.9 from
another process): at the default 200 ms budget peak RSS 41.7 MB (41.5-41.9, N = 5) vs 52.5 MB (45.9-52.6) unbounded, 300/300 certified
each (no thinning: ~34.5k rows vs a cap of 1.77M); at `--budget-ms 10000` 1,024.2 MB (1,013.9-1,031.5, N = 3) vs 1,625.0 MB
(1,606.8-1,627.6), 300/300 certified each (one capped run thinned: 1.31M rows kept vs 1.71-1.75M). Claim: a default run can no longer
grow its sample memory past ~1 GB + base. Not claimed: 1 GB suits every machine (it is not derived from installed RAM), or the
Windows cost: the capped buffer is reserved once, which Windows commits up front (unmeasured; a smaller cap or 0 avoids it).

### 5.4 `--priority low` under contention (`bench/priority_contention.py`, `bench/priority_low_alone.py`, N = 5)
Foreground: 400-spin ring, 10 chains x 20,000 sweeps on 10 threads. Background: the same program with a 4 s budget on 10 threads.

| background priority | foreground wall ms | slowdown vs alone |
|---|---|---|
| none (alone) | 702.7 [678.2-715.9] | 1.00 |
| normal | 1251.9 [1242.2-1307.6] | 1.78 |
| low = nice 10 only (earlier-build behaviour) | 1275.0 [1227.7-1289.8] | 1.73 (separate run; alone 737.4) |
| low = nice 10 + macOS background band (current build) | 716.9 [687.3-735.0] | 1.02 |

The cost: a `low` run on an idle M4 takes 481.5 [453.4-489.8] ms vs 114.6 [99.3-116.1] ms at normal priority (x4.2 wall,
x3.9 CPU; same answer). Claim: on macOS, `--priority low` kept pbit out of the way of the other job tested (a pbit foreground run). Not claimed: anything
about Linux or Windows (Linux unmeasured; Windows exits 2).

### 5.5 Portable vs `-C target-cpu=native` (`bench/portable_vs_native.py`, N = 5 alternating)
Native / portable ratio on 16 metrics (pbit-core kernels, `pbit stats`, router sampler): 0.975 to 1.012. For example the
older (not the §1 fast-path) heat-bath f32 LUT kernel ran at 5.36e8 updates/s/thread in both builds, and the older
64-lane multispin kernel at 2.41e10/thread
(64 independent replicas per word, not one problem 64x faster). Kernel checksums and the router's answer are identical
across the two builds. Claim: on Apple silicon the portable default loses nothing measurable. Not claimed: anything about
x86-64, where `native` can enable AVX2/AVX-512 (unmeasured).

### 5.6 `pbit stats` self-test (N = 7)
400-spin ring, 4 chains x 2,000 sweeps: 23.05 [22.70-23.07] M site updates/s on 1 thread, 77.65 [76.11-79.37] M on 4
(x3.37 [3.30-3.54]). This is the number `pbit stats` prints on your machine: it is a self-test, not a comparison.

### 5.7 `--cpu-limit` on the router (`bench/decide_cpu_limit.py`, N = 3)
300-task demo, `pbit decide --sweeps 1000 --polish-ms 0`, 4 threads: sampler phase 23.9 / 57.0 / 132.0 ms and process CPU
137.2 / 139.8 / 152.1 ms at `--cpu-limit` 100 / 50 / 25; same answer. Subtracting the ~40 ms serial part
(inferred in 5.1 as wall minus sampler time), the sampler used ~44% and ~21% of 4 cores at 50 and 25. Claim: it stays under the cap
here, for +11% CPU per unit of work at 25. Not claimed: that this holds generally. On `pbit run` with a 2,000-spin ring, 25% cost 4.1x
the CPU (earlier build).

### 5.8 The plan polish honours `--threads` (`bench/polish_threads.py`, N = 5 alternating, old vs new binary)
300-task demo, `pbit decide --sweeps 400 --polish-ms 50`. In an earlier build the polish ran 4 threads whatever `--threads` said.

| `--threads` | binary | wall ms | process CPU ms | CPU / wall | plan log w, median (min-max) |
|---|---|---|---|---|---|
| 1 | before | 122.8 | 277.7 | 2.26 | 707.77 (707.32-708.01) |
| 1 | after | 122.4 | 126.9 | 1.04 | 705.12 (704.63-706.83) |
| 4 | before | 100.1 | 282.9 | 2.83 | 707.32 (707.27-707.86) |
| 4 | after | 99.5 | 282.4 | 2.84 | 707.27 (706.96-707.93) |

Claim: `--threads 1` now means one thread through the whole answer, and 4 threads are unchanged. Not claimed: that it is free:
at the same wall time one thread polishes a quarter as long per chain, so the plan was 2.65 log w lower (the ILP optimum of
this queue is 708.31, §2.2). The load average was ~4.5 during this run (another process was busy; alternating runs).

## §6 Where it loses
- CPU cap: `--cpu-limit 25` kept the CPU share under the cap but spent 4.1x the CPU for the same work on a 2,000-spin ring
  (earlier build); on the router it cost +11% (5.7), so the price depends on the program. To share the machine cheaply, lower `--threads` first.
- Priority: `--priority low` costs 4.2x wall on an idle Mac (5.4).
- One thread: `--threads 1` gives a 2.65 log w worse plan at the same wall time on the 300-task demo, since the polish now obeys it (5.8).
- Memory cap: at 2 MB on the hard 300-task demo, 61 fewer tasks were released (5.3).
- Tiny inputs (`bench/tiny_exact.py`, N = 7): on the 12-task demo, exact enumeration of all 180,540 feasible plans takes
  7.30 [7.29-7.32] ms. The forced sampler takes 273.6 [271.2-276.8] ms (x37) and is only approximately right (certified 7/7,
  max TV 0.0019 vs exact). This is why `pbit decide` runs the exact tiers first.
- Sudoku (§4): DFS backtracking does the same exact job 15-102x faster per puzzle (medians 17x open, ~50x unique), and the sampler refused all 20 generated puzzles (in the current build a puzzle that naked singles solve is certified: every cell is forced).
- Max-cut (§3): simulated annealing finds better cuts at equal time on both Gset-like shapes (by 7 and 4 cut edges against pbit's
  anneal-only mode, by 20 and 10 against its sampler).
- Router, single best plan (§2.2): an ILP solver (HiGHS) proves the optimum on 300 tasks in 52-352 ms (three runs) and was faster than
  pbit's default path on 5 of the 6 queues tested; pbit's plan is 0.45-2.0 nats short over three runs (9.3-9.7 on `--hard`). If you only need one plan, use an ILP/CP-SAT solver
  (or greedy+polish, which beat pbit's plan at equal time on all 8 runs at 300 tasks, §2.2).
- Scheduling, single best plan (§4.3): HiGHS proves the optimum of three 200-job programs in 0.68-0.95 s; pbit's plan is
  3.3-4.3 nats short at 2 s and 5 s budgets, and at 5 s it releases 156-173 of 200 jobs on two programs and none on the third.
- Router, coverage (§2.1): over 16 oracle queues 58% of tasks were released. Nothing was released on saturated queues (every
  worker full; 2 of 4 refused although their odds were within 0.018) or on strong-affinity ones (3 of 4 rightly, odds off by 0.16-0.44;
  one needlessly, 0.041). Mid-size thin queues (30-36 tasks) are sampled at ~0.3 s although a larger
  frontier cap answers them exactly in 41-104 ms (§2.3).
- Scheduling (4.3): on a 200-job precedence schedule the sampler's start takes 0.7 s (it gave up after 105 s in an earlier build) and
  at 1 s of budget the gate refuses; 5 s gives 153-171/200 released on 3 instances since the ~0.7 s start was moved inside the
  budget (181-186 before; unchecked, no oracle). The gate runs after the budget: a 5 s budget returned after 5.91-6.01 s and a
  1 s budget after 1.06-1.19 s on this program (one run per instance). Through the CLI
  (`pbit run --op decide --budget-ms 1000`, the same program as a 5.3 MB pbit-ir JSON; one run each): the binary from
  before the start search got its work budget and retries gave no output within a 45 s alarm; after that change 3.39 s wall, `partial`, 2 of 200 released, 0 violations,
  R-hat 1.00187 (sample ~1.75 s including the starts, gate 210 ms). The other ~1.2 s was the exact tier declining: `--op sample` (no exact tiers)
  took 2.13 s vs 3.33 s for `--op decide`; parse + build is ~0.09 s (`--op sample --sweeps 1`: 0.85 s wall, 0.76 s of it the
  start); `exact()` declined after 1,154 ms at limit 1 and 1,155 ms at limit 2M. A profile showed that the MRV
  enumeration spent 1.66 G capacity checks (19,564 dead ends) finding its FIRST plan, then produced 2M plans in 32 ms. It now
  declines after max(64 x limit, 1e8) checks without a new plan (every answering sudoku / colouring / scheduling oracle run needs
  <= 5.1 M between plans; their exact answers are unchanged): `exact()` declines in 75 / 96 ms at limit 1 / 2M (medians, N = 5),
  and `pbit run --op decide` takes 2.21 s vs 2.12 s for `--op sample` (N = 5 each). Then a later change put the chains' start search
  inside the budget (the docs already promised a deadline for the sampling phase): 1.29 s wall (median of 5, load ~10 from other
  processes; sampling 1,004 ms, gate 38-45 ms), but at this budget the verdict is now `refused` (it was `partial`, ~2 of 200
  released): less sweeping. Precedence costs O(T^2) pair caps per edge (82,678 caps at 200 jobs).
  A single greedy + repair is 0.4-2.5 ms; and it hit dead ends on 6 of 20 tight small instances.
- Also lost or refused (measured, collected in a review pass): colouring: the exact tier is faster than the sampler on every
  instance and 3-colourings (k = 3, n = 14) are mostly refused (59 of 82 over 6 sets after the stuck-chain rule change, all before; §4); max-cut at coupling 1.5 with 500 sweeps released nothing (§3);
  one 100-job schedule with 6 frozen capacities is refused even at 5 s (4.3); an unstartable 200-job program took 0.666 s (measured before the exact-search fallback was added) to
  answer `refused` at a 500 ms budget (4.3); `--progress` costs up to ~8% on tiny programs (5.2); the exact-tier gap budget
  cost 1-4% on sudoku counting (§4) and 18-21% on 3-colouring infeasibility proofs (5 instances, median of 3
  alternating runs); a later change precomputes the per-variable count (same count, same answers): now 2-5% slower than the engine before the gap budget on those proofs
  and 12-16% faster than the first gap-budget engine.
- Proofs of infeasibility (`bench/csp_gap_probe.py`, 3 runs per binary): the exact-tier gap budget made `pbit run
  --op decide` answer `refused` (exit 3) in 0.33-0.35 s on 5 of 5 infeasible random 3-colourings (150-200 vertices, mean degree
  4.6, one edge clamped) that the binary from before the gap budget proved `infeasible` (exit 1) in 0.13-4.3 s (medians 0.126 / 0.190 / 0.247 / 1.835 /
  4.277 s). `--op exact` still proves them (0.15-5.1 s). Safe (no false answer) but a lost proof; the refusal reason now says so.
  Partly fixed in the current build: under `decide` with a wall-clock budget each chain's start search stops at half of its slice of the budget
  (`--budget-ms` / ceil(chains / threads)) and, when any chain finds no start, the samples are dropped and the exact search runs
  again to the budget's end. Same probe (NS=150,200, one run each): 1 of the 3 proofs that take
  0.13-0.27 s comes back `infeasible` at `--budget-ms 500` (0.485 s), 3 of 3 at 1000 (0.73-0.88 s); the 1.9 / 4.4 s proofs are
  refused on budget (1.007 s; in an earlier build a refusal overran the budget by ~110 ms: 612-618 ms at 500, 6 runs each). A first version
  that gave the exact search the FIRST half of the budget recovered more proofs (2 of 3 at 500) but cost feasible programs
  sampler time (200-job schedules at 5 s: 174 -> 97 released, and 157 -> refused); the fallback costs them nothing (173 / 157 vs
  170 / 157, N = 1 each). `--op sample` and `--sweeps N` keep the work-only bound (refused).
- The exact tier is outside `--budget-ms` (`pbit run --budget-ms 200`, loose random 3-colourings from
  `bench/csp_gap_probe.py`'s generator at mean degree 2, seed 7, 1-2 runs each): on a program with more plans than `--exact-limit`
  (2M) the tier enumerates until it has counted `--exact-limit` + 1 plans, then declines, and every plan resets its gap budget. `--op decide` took
  0.44 / 0.71-0.75 / 1.62-1.70 s at 300 / 1,000 / 3,000 variables vs 0.29-0.35 s for `--op sample` on the same programs (all
  `refused`): up to 8x the budget, growing with the size. Bounding it by the clock would also turn exact sudoku answers (0.11-8.1 s
  at the default budget, §4) into sampler refusals; the default is not changed (decision for the maintainer). Opt-in in the current build:
  `--exact-ms N` stops the exact tiers that run before the sampler N ms into the call (re-run after a parsing fix, same programs, 5 runs each,
  interleaved, CLI wall, medians [IQR]): `--op decide` 430 [430-430] / 699 [699-699] / 1,480 [1480-1480] ms -> 341 /
  347 / 346 ms at `--exact-ms 50` and 291 / 295 / 296 ms at `--exact-ms 0`, vs 292 / 296 / 296 ms for `--op sample` (verdicts unchanged:
  refused; in a first run, before the parsing fix: 438 / 711 / 1,530 -> 342 / 352 / 384, sample 292 / 302 / 334).
  On the 300-task router demo (`pbit decide`: 305 ms vs 264 ms with `--mode sample`) the exact tiers add ~41 ms *(inferred from
  the difference)*, so `--exact-ms 50` changes nothing there. One router group of 3,000 tasks
  (`pbit decide` at the default 200 ms budget, 1 run each, machine load 5-8) took 7.55 s wall at 1,159 MB peak RSS (5,000 tasks: 12.77 s,
  1,895 MB; both `refused`), ~37x the budget: ~7 s is the enumeration counting toward `--exact-limit` (`--exact-limit 0` 0.59 s,
  `--exact-ms 0` 0.44 s, `--mode sample` 0.46 s). Without the
  flag the output is unchanged (A/B vs the binary from before the flag, timing fields removed: 13 of 14 cases identical; the 14th, a wall-clock
  sampler run, differs by timing only and is identical at fixed `--sweeps`). What this allows: bounding the
  exact tiers' extra wall time on large loose programs. What it forbids: expecting exact answers from a cap shorter than the
  program's exact time (a capped exact tier declines; `decide` then samples). Kill test: the cap swept
  across the exact time of 5 programs (two 3-colouring infeasibility proofs of 186 / 238 ms, a 7,159,296-plan colouring answered
  in 2.19 s, the router demo at 13 / 11 tasks under `--mode exact`) and the 11-task demo under `--mode auto`: 165 capped runs, 0 wrong. Every exact-mode run
  (155) gave the uncapped answer (identical, timing removed) or declined; no partial sum and no unfinished proof came out labelled exact
  or infeasible, including caps that stop the second enumeration pass (1.1-2.2 s on the 7.16M-plan colouring). Under `--mode auto`,
  the 10 capped runs answered from the sampler with `exact_ms_reached: true` (later check: 10 such runs, seeds 1-10, against the
  exact odds: 110 released tasks, max TV 0.0023, same most likely worker for every task).
  The tradeoff in one program (1 run each, `pbit run` default op): the Wikipedia sudoku as an IR program is `exact` in
  18-38 ms with or without `--exact-ms 100`; with rows 7-9 cleared (502,260 solutions) the default answers `exact` (full count +
  per-cell odds) in 4.53 s, and `--exact-ms 100` answers `refused` (sampler, 0 released) in 0.41 s.
- The plan polish overran `--polish-ms` on huge grouped router inputs (found and measured in an earlier build, `--budget-ms 500`, 100,000 tasks in
  groups of 4: 2.71 s wall at the default `--polish-ms 50` vs 0.68 s at `--polish-ms 0`; 8.38 s at `--threads 1`; `--polish-sweeps 1`
  4.66 s). FIXED in the current build. The cause was not the per-temperature chain builds (an earlier reading of the code; profiled: ~20 ms
  each) but the plan score `Problem::logw`, which tested every pair of tasks for a shared group (O(t^2), ~2 s per call at 100,000
  tasks), run once per polish chain before its clock and once per sweep. It is now linear above 96 tasks (the pairwise loop stays up to 96, where it is faster; it is still 2-4% faster per polish sweep at
  100-110 tasks, so the threshold is slightly low) and bit-identical (test), and the polish
  stops building chains once its time share has passed. Old vs fixed binary, same input shape, `pbit decide --mode sample`, default `--budget-ms 200`,
  3 runs each (machine load 5-8): time after sampling 2,039-2,041 -> 82-86 ms (4 threads), 7,868-7,871 -> 102-109 ms (`--threads 1`)
  (re-run on the final binary, 5 interleaved runs, load ~3, median [IQR]: 2,038 [2,037-2,038] -> 86 [85-86] ms; 7,745 [7,736-7,777] ->
  105 [103-105] ms);
  `--polish-sweeps 1` 4.19 -> 0.34 s; fixed-work outputs identical on 6 inputs; on the 300-task demo the polish does 1.5-1.6x more
  sweeps per second, so at equal wall time its plan is better on 5 of 8 seeds (`--hard`: 7 of 8). What this allows: `--polish-ms`
  as a near-deadline on this shape (32-59 ms over at 50). What it forbids: calling it a hard deadline (each polish chain builds at
  least one chain before its first clock read).
- Very many chains: the gate runs after `--budget-ms` and its cost grows about quadratically with `--chains` (cause, read from the code,
  not profiled: `chain_disagreement` compares every pair of chains' marginals, and the batch statistics spawn one thread
  per chain) (30-task demo,
  `--mode sample`, 1 run each): 64 / 1,000 / 10,000 chains took gate 10 / 103 / 7,488 ms, wall 0.26 / 0.36 / 7.76 s, peak RSS
  31 / 107 / 829 MB (10,000 chains: `refused`, ~33 sweeps each); 100,000 chains gave no answer within 20 s. What this forbids:
  treating `--budget-ms` as a deadline at thousands of chains. The defaults (4 chains) are not affected.
- Not measured: CP-SAT (OR-Tools is not installed here), ILP with per-task odds (it has none), Linux/x86-64 numbers for any table,
  an ILP baseline for scheduling above 200 jobs (4.3 has 200-job programs only).

## §7 The QUBO hand-off vs the native sampler (re-run of `cargo run --release -p pbit-decide --example hardware_lowering`, N = 1)
One run on 3 router instances (seeds 7 / 11 / 13), each lowered to a 34-bit one-hot QUBO with slack bits (165 couplings; the
energy of every feasible plan equals -log w exactly, acceptance test `ir_round_trip_and_lowering_exact`). Every sampler gets 20 ms
of wall clock on this CPU; TV is against the exact odds, over the feasible samples only.

| sampler | infeasible samples (seeds 7 / 11 / 13) | TV on feasible samples (7 / 11 / 13) |
|---|---|---|
| native constraint-preserving Gibbs | 0 by construction (36,800 / 98,240 / 100,352 samples) | 0.0049 / 0.0018 / 0.0019 |
| p-bit Gibbs on the QUBO, penalty 0.5 | 100.0% / 100.0% / 100.0% | 0.2249 / 0.1094 / 0.0452 (1-10 feasible samples) |
| penalty 1 | 99.5% / 99.4% / 98.9% | 0.0166 / 0.0160 / 0.0121 |
| penalty 2 | 87.8% / 90.3% / 82.4% | 0.0076 / 0.0036 / 0.0019 |
| penalty 4 | 24.5% / 37.0% / 19.9% | 0.0111 / 0.0128 / 0.0072 |
| penalty 8 | 0.7% / 0.8% / 0.4% | 0.0698 / 0.1664 / 0.0457 |
| penalty 16 | 0.0% / 0.0% / 0.0% | 0.1612 / 0.2325 / 0.0912 |

Claim: on these instances the native sampler keeps every rule by construction at TV 0.002-0.005. The QUBO form matches that
accuracy on its feasible samples only at penalty 2-4 (seed 13 at penalty 2: 0.0019, equal), where 20-90% of its samples break a
rule and must be thrown away; at the penalties that keep over 99% of samples feasible (8-16) its TV is 0.05-0.23. Not claimed: anything about
p-bit, FPGA or annealing hardware (none was run; their penalty tuning and embedding are unmeasured), or instances beyond 34 bits.
