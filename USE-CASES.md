# pbit use cases: what the measurements support

Every "gives" cell below points to a measured table in `BENCHMARKS.md` (§ numbers) or a separate run; every "do not promise"
cell points to where pbit lost or was not tested. All measurements are on one Apple M4 Mac mini with synthetic or generated
problems. **Nothing here was measured on a customer's data.** Tags: *(inferred)* = reasoned, not run; *(unmeasured)* = not run.

## When to reach for pbit, and when not to

Use it when all three hold:
1. the decision has **hard rules** (who may do what, quotas, one-of constraints) that must never be broken;
2. you want **odds for each part** of the decision, not only one answer;
3. you want the tool to **say "I can't vouch for this part"** instead of guessing, on a laptop, offline.

Do not use it when:
- **you only need one best plan.** An ILP solver (HiGHS) proved the optimum of the 300-task router in 52-352 ms and was faster
  than pbit's default path on 5 of 6 queues; pbit's plan was 0.45-2.0 nats short (§2.2, three runs). Use ILP / CP-SAT, or greedy + polish.
- **the instance is tiny.** Then pbit answers exactly anyway (12 tasks: 7.1 ms p50, §2.3), so its sampler adds nothing.
- **you need raw search speed on a known puzzle class.** Backtracking counts sudoku solutions 15-102x faster per puzzle than pbit's
  exact tier, and pbit's sampler refuses every generated sudoku (20/20; in the current build a puzzle that naked singles solve is certified, §4).
- **you need the best cut or lowest energy.** Simulated annealing found better max-cuts at equal time (§3).

## Catalogue by problem shape

| problem shape | how it maps to the pbit instruction set (`pbit-ir`) | what pbit gives (measured) | do not promise | who might use it *(inferred)* |
|---|---|---|---|---|
| **Assign items to resources under rules** (tasks to agents/models/people, tickets to agents, jobs to machines) with quotas and a same-group preference | one categorical variable per item over its allowed resources; unary log-weights = your scores; at-most-`cap` per resource; a Potts pair per same-group pair (`pbit decide` lowers to this) | **0 rule violations by construction** (300-task demo: per-task argmax breaks 144 rules incl. 60 PII tasks sent to cloud models; pbit 0); **per-item odds you can check**: 1,916 released tasks on queues with an exact answer, 0 wrong (§2.1); **refusal/escalation** by budget (§2.3, demo 2 below); **exact odds in 0.17-25 ms** up to ~24 tasks (§2.1, §2.3); what-if clamps (including clamps that fill a worker: 10/10 certified, 0 false in the current build) | the single best plan (ILP wins, §2.2); full coverage on saturated or strong-affinity queues (58% of tasks released on the oracle queues, §2.1); speed (the sampled path spends its ~0.3 s budget by design, §2.3) | AI platform teams routing LLM tasks; support operations; internal-tools developers |
| **Binary pairwise models** (Ising, max-cut, binary Markov random fields) | one two-valued variable per spin; `table` pairs = couplings; unary = fields | **per-spin odds with a certificate checked against brute force**: 144 runs, 72 certified, 0 false certificates, 1,346 released spins, 0 wrong (§3, and a re-run); exact odds and log Z by the exact tiers on small models; at equal time, cuts within 0.06-0.7% of simulated annealing in anneal-only mode and 0.17-1.75% short when sampling (the mode that gives odds, §3) | better cuts than simulated annealing (it won by 4-7 edges, §3); log Z from the sampler (not implemented); correctness beyond 16 spins (the brute-force oracle stops there) | researchers and teachers of p-bit / Ising computing; people prototyping before renting annealer time |
| **Constraint satisfaction with odds** (graph colouring, one-hot puzzles, slot assignment) | a categorical variable per node over colours/digits; negative Potts or at-most-1 caps for "differ"; clamps for givens | **exact per-variable odds and solution counts** from the exact tier (sudoku odds equal a backtracking count on 20 puzzles; colouring 0.00-5.70 ms median, §4); the sampler released 525 vertices, 0 outside tolerance (§4; 468 in an earlier build) | speed against backtracking (15-102x slower per puzzle, §4); sampler coverage on frozen or few-solution instances (sudoku: refused 20/20; 3-colouring: 9 of 14 refused on the default set in the current build, 59 of 82 over 6 sets, §4) | CSP and puzzle tooling, teaching, frequency/register-assignment prototypes *(unmeasured on real instances)* |
| **Scheduling with capacity per time slot** (unit-time jobs, release/deadline windows, precedence) | a variable per job over slots; allowed = window; one cap per slot; precedence i -> j as pair caps "not (i at a and j at b)" for a >= b (O(T^2) caps per edge) | **exact odds on small instances** and a gate that released 428 jobs on 39 oracle instances, 0 outside tolerance; at 200-500 jobs, anneal from a greedy plan beat restarted greedy + repair at equal 100 ms on 4/5 seeds each (§4.3) | optimal schedules at scale (an ILP solver, HiGHS, proves the 200-job optimum in 0.68-0.95 s; pbit's plan is 3.3-4.3 nats short, §4.3); checked odds at scale (at 200 jobs the sampler needs 0.7 s just to start, refuses at 1 s, releases 153-171/200 at 5 s on 3 instances in an earlier build (re-run on a later build, N = 3: 156-173 on two, the third refused) (earlier build, start inside the budget; 2 runs each) with no oracle to check them); multi-period durations (unit jobs only) | planners prototyping small shift/slot problems *(inferred)* |

## Industry and user map

| who | problem | shape | status |
|---|---|---|---|
| AI agent platforms, agent-harness builders | route tasks to models or humans under PII / data-residency rules, quotas and workflow affinity | assignment | the flagship demo is this shape, on synthetic data; **not measured on real traffic** |
| Customer support, IT service desks | ticket to agent under skills and queue caps | assignment | same shape *(inferred)*; unmeasured on real tickets |
| Cloud, HPC and CI operators | job to machine or pool under quotas and co-location preference | assignment | same shape *(inferred)*; unmeasured; the router has no time dimension (pbit-ir handles unit-time slots, §4.3) |
| Workforce planning (one period) | person to shift under caps and rules | assignment | same shape *(inferred)*; multi-period rostering: only unit-time slot scheduling was measured (synthetic, §4.3) |
| Research labs, universities | sample Ising/Potts models with a checkable error bound; teach probabilistic computing | binary pairwise, CSP | **measured** (§1, §3, §4); the most honest fit today |
| Teams evaluating Ising / annealing hardware | prototype locally, then hand the model over: `pbit ir` prints the one-hot QUBO form (energy = -log weight) | binary pairwise | the lowering exists; **no hardware was run**; rules hold by construction only on pbit's own sampler: a p-bit sampler on the QUBO form leaked infeasible samples at low penalty weights (§7, one instance, this CPU) |
| Telecom, compiler teams | frequency or register assignment as colouring, with odds | CSP | measured only on random graphs up to 14 vertices; exact tier practical only at small sizes |
| Individual developers | any of the above on a laptop, offline | all | zero external crates; a fresh export builds offline: `cargo build --release` 7.82 s; with all targets 26-28 s |

## Integration (what exists today)
- **CLI + JSON** on stdin/stdout, exit codes 0 plan / 1 infeasible / 2 bad input / 3 refused. Every `bench/` script drives it.
  Process overhead is ~2-4 ms (§2.3: 9.0 ms wall vs 7.1 ms inside pbit at 12 tasks; 303.1 vs 299.0 at 300).
- **Python**: through a subprocess (README snippet; every `bench/*.py` is a working example). There are no Python bindings.
- **Rust**: the `pbit-decide` crate (router front-end) and `pbit-ir` (general programs, JSON v1 in `docs/pbit-ir-json.md`).
- **MCP server**: not built (README roadmap).
- **Running beside other work**: `--threads`, `--cpu-limit`, `--priority low`, `--mem-limit-mb`, `--progress`, `pbit stats` (§5).
  Reproducible runs: fixed `--sweeps` plus `--polish-sweeps`. The defaults are wall-clock (sampling and polish), so the odds,
  the verdict, the released set and the plan can all vary run to run and machine to machine.

## Three flagship 60-second demos (each re-run on the final build on the M4)

**1. The router that won't break policy**: `cargo run --release -p pbit-decide --example agent_router` (3.1 s in an earlier run, 4.7 s in a later one).
You see: a naive per-task router on 300 agent tasks breaks 144 rules (60 PII tasks to cloud models, 3 production-DB
migrations to cheap models, a worker 78 over quota); pbit breaks 0, certifies the whole 300-task answer at 200 ms, prints
per-task odds, and re-plans a what-if ("this customer demands a human"). On a 12-task queue with an exact answer every
released task is within 0.0074 of the exact odds (0.0073 and 0.0074 in two earlier runs, 0.0074 in a later one; the budget is wall-clock). Shows, on
these synthetic queues: rules by construction, checkable odds, what-if. Does not show:
anything about real traffic, cost or latency savings; the plan is not claimed optimal (§2.2).

**2. It says when it can't vouch**: `pbit demo --tasks 300 --hard | pbit decide --sweeps S --polish-sweeps 400` with
S = 100 / 700 / 3300 gives `refused` (exit 3, 0 released) / `partial` (86 released, 214 escalated) / `certified` (300 released).
Fixed work, so the whole output except timing is identical on every run and at `--threads` 1 and 4 (checked);
0.14 / 0.32 / 1.12 s on the M4 with 4 threads. The wall-clock form `--budget-ms 50 / 200 / 1000` gave refused
5/5, partial 5/5 (91-96 released) and certified 5/5 (on an earlier binary; re-run on a later binary: the same, 91-95 released at 200 ms, wall
0.21 / 0.36 / 1.17 s; re-run on the final binary, 3 runs at load ~6: refused / 90-92 released / certified), but shifts to longer budgets on a slower machine. Note: the refusal threshold is a cliff, not a ramp, and
more work can turn `partial` into `refused`: 168 sweeps gave `partial` (R-hat 1.00197) and 250 gave `refused` (R-hat 1.00208 > 1.002).
Shows: pbit escalates instead of guessing when its error bound fails. Does not show: that escalated tasks were hard
(some are refused needlessly: §2.1 saturated queues).

**3. A trust meter checked against brute force**: `cargo run --release -p pbit-ir --example maxcut_oracle` (0.8 s once built).
You see: 144 sampling runs on frustrated 12- and 16-spin max-cut / Ising models, each checked against the exact odds from
enumerating every state: 72 certified, **0 false certificates**, 1,346 released spins, **0 wrong**; with too little work
(500 sweeps at strong coupling) the gate releases nothing rather than something wrong (re-run: identical). Shows: the
certificate meant what it says on models small enough to check; 0 false of 72 bounds the false-certificate rate below ~4% (95%,
rule of three), not at zero. Does not show: anything beyond 16 spins, or better cuts than simulated annealing (§3).
