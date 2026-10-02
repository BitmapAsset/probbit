# probbit IR JSON v1: the wire format of the virtual p-bit processor

`probbit run < program.json` executes a general probbit-ir program. The assignment router (`probbit decide`) is one front-end that
lowers to the same IR (`Problem::lower` in `probbit-decide`); through that lowering the IR's enumeration, gate, and sampler with the
router's accelerators off reproduce the router bit for bit on the 7 acceptance cases (`ir_lowering_bit_identical`). `probbit decide`
itself uses the IR for the frontier DP and the diagnostics gate (on `Problem::lower()`); its enumeration and its sampler (with the
accelerators) are its own.

## Program

JSON Schema (draft 2020-12, hand-written): `docs/probbit-ir.schema.json`; this page is normative where they differ, and
test `ir_schema_matches_the_parser` keeps every object's field list equal to the parser's.

```json
{ "probbit_ir": 1,
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
  Cardinality (the first IR v2 construct; still `probbit_ir: 1`, a v1 program parses and answers unchanged): a `value` cap
  may carry `min` = "at least `min` of these variables (those able to take the value) take it", alone or with `limit` (a
  range; `min` = `limit` = exactly k). It lowers to one more cap: at most |M| - `min` of those M variables take any of their
  other allowed values (each variable holds one value), so enumeration, the frontier / components tiers, the sampler and the
  gate run it unchanged. `min` above |M| is rejected (exit 2, code `value`: no plan satisfies it); `min` with `members` is a
  schema error. Test `run_cardinality_min_and_range_caps_match_brute_force` (>= 2 a, 1..2 b, exactly one c among three,
  6 variables x 3 values: odds = brute force to print rounding; sampler at defaults, seeds 1..5: 5/5 `diagnostics_passed`,
  0 violations, max TV 0.0016).
- `all_different[]`: `{"vars": [ids]}` (>= 2 distinct ids) = no two of these variables take the same value; lowered to
  one cap per value with limit 1 over the listed variables able to take it.
- `implies[]`: `{"if": {"var": x, "value": a}, "then": {"var": y, "in": [values]}}` (x != y) = whenever x takes a, y
  takes one of `in`; lowered to one cap with limit 1 over (x, a) and y's allowed values outside `in` (y holds exactly one value,
  so the two cannot both hold). Hard, like every cap. Test `run_all_different_and_implies_match_brute_force` (5 variables x 4
  values, one all_different + two implications: n_feasible, log Z and odds = brute force; sampler at defaults, seeds 1..5: 5/5
  `diagnostics_passed`, 0 violations, max TV 0.0010).
- `tables[]`: `{"vars": [1 to 3 distinct ids], "forbid": [[one value per var], ...]}` or `"allow"` instead of
  `"forbid"` (exactly one). A forbidden tuple = one cap with limit arity - 1 over its (var, value) pairs (all of them at once
  is the only way to exceed it); `allow` forbids every other tuple of the variables' allowed values (at most 100,000 tuples,
  else exit 2 code `limit`); tuples naming a value a variable cannot take are dropped. Test `run_tables_match_brute_force`
  (4 variables x 3 values: an allow-list on a pair, a forbid-list on a triple, an arity-1 forbid: n_feasible, log Z and odds
  = brute force).
- `precedes[]`: `{"before": x, "after": y, "gap": g}` (x != y; `gap` a non-negative integer, default 1), the
  (job, slot) scheduling pattern: values are read as ordered slots (their order in `values`) and slot(y) >= slot(x) + g
  (`gap` 0 = not earlier, 1 = strictly later). Lowered to one cap (limit 1) per violating (slot of x, slot of y) pair, so
  O(k^2) caps per precedence. Test `run_precedes_matches_brute_force` (4 jobs x 5 slots, three precedences + one
  all_different: n_feasible, log Z and odds = brute force). Long chains (found and fixed during 0.2.0, BENCHMARKS "Known
  failure modes"): 20 jobs x 30 slots in one chain was refused after 15.2 s; the arc-consistent start (below) now starts its
  chains and the call stays near the budget (0.54 s at 200 ms: `refused` by the gate, honestly; `diagnostics_passed` at 3000 ms).
- `linear[]`: `{"terms": [[var id, value name, weight], ...], "limit": L}` = the linear rule
  `sum of weight x [var = value] <= L` with whole-number weights 0..=1,000,000 and `limit` 0..2^53 (knapsack: one per budget,
  terms `[item, "in", weight]`; bin packing: one per bin, terms `[item, bin, size]`; multi-dimensional: several over the same
  (var, value) pairs). Each (var, value) at most once per rule (else `value`); a term with weight 0, or a value its var cannot
  take, is dropped; a rule with no terms left is dropped. Lowered to ONE weighted cap (library: `Cap { members, limit, weights }`,
  `Cap::new` = unit weights): members are never replicated, so the compile pass drops it only when no plan can break it (per
  variable its heaviest term, summed, <= limit), and enumeration, the components tier, the start searches (incl. the
  arc-consistent one: forward checking prunes every open term that no longer fits), every sampler move (site, swap, global flip,
  label swap, three-cycle, cluster: each checks the weighted load change), `violations` and the gate (occupancy R-hat on the
  weighted load; a weighted cap is "full" when no term fits on top) read the weights. Weighted programs are never "partition"
  programs, so the frontier DP and the occupancy-count DP decline them (enumeration, components or the sampler answer). Tests:
  `weighted_caps_match_brute_force` (300 random programs: violations, enumeration, components, compile pass, start search, 1e-9),
  `weighted_caps_sampler_matches_enumeration` (10-item knapsack + 6 items x 3 bins, every move on, max |odds - exact| < 0.02),
  `weighted_cap_rules`, `run_linear_matches_brute_force` (two weighted rules + a unit cap: n_feasible, log Z, odds) + 10
  contract cases (term shape, negative / fractional / > 1,000,000 weight, duplicate term, unknown var, missing limit, unknown
  field, non-array, an infeasible warm start). Only `<=` (an at-least / equality linear rule is not in v1).
  All six constructs (cardinality, all_different, implies, tables, precedes, linear) are rules, not weights; their odds and plans come from
  the same tiers as any cap. Their caps often overlap (one (variable, value) in several caps); then the frontier DP, which
  needs every pair in at most one cap, declines and enumeration, the components tier or the general sampler answer.
  Examples: worker quotas (`value` form), one-hot sudoku rows (`limit` 1 per digit), job-shop slot capacity.
- `start` (optional warm start): `{"var": "value", ...}` naming EVERY variable once, each on an allowed value (and its
  `clamp`, if any), with every rule satisfied (all caps and constructs, checked before the compile pass); anything else exits 2
  (`schema` for a non-object / non-string value / duplicate key, `value` with path `start.<var>` for an unknown var or value or a
  forbidden value, path `start` for a missing variable or a broken rule). Chain 0 starts from it, then its 5 uniform
  over-dispersion sweeps as every chain; the other chains run their own start search, so the starts stay over-dispersed and the
  gate's R-hat keeps its meaning. Exact tiers ignore it. Use: a known feasible plan for programs whose start search is slow.
  Library: `Model::start` (used only if feasible; `with_clamp` drops a contradicting one). Tests `run_warm_start_seeds_chain_0`,
  `warm_start_is_used_only_when_feasible` + 9 contract cases. Not done: chains other than 0 do not fall back to it when their own
  search fails (the answer is then `refused` as without it).

The distribution is `P(x) ∝ exp(log w(x))` over the assignments that satisfy every hard rule:
`log w(x) = Σ_i h_i(x_i) + Σ_pairs J_ij(x_i, x_j)`.

## Input contract (wire format, CLI, library)

**Wire format (both JSON documents: this one and the router's, README "Problem format") is strict.** Every field listed
here is type-checked when present; `null` means absent for optional fields; anything else that does not match is an error,
never ignored or coerced. Encoding: UTF-8; one leading byte-order mark is skipped (since 0.3.0: Windows PowerShell 5.1 adds
one to every pipe).

| Document | Fields (all others rejected) |
|---|---|
| program | `probbit_ir` (number, must be 1), `values` (non-empty array of distinct strings, at most 65,535), `vars` (non-empty array), `pairs` (array), `caps` (array), `all_different` (array), `implies` (array), `tables` (array), `precedes` (array), `linear` (array), `start` (object), `comment` (string, ignored) |
| `vars[]` | `id` (string, unique; default `x<index>`), `h` (object: value name -> number, each key once), `allowed` / `forbid` (arrays of distinct value names; `allowed` non-empty), `clamp` (a value name) |
| `pairs[]` | `i`, `j` (two different var ids), exactly one of `potts` (number) or `table` (k arrays of k numbers) |
| `caps[]` | `limit` (integer 0..2^53) and/or (`value` form only) `min` (integer 0..2^53, at least), exactly one of `members` (array of `[var id, value name]`) or `value` (a value name, with optional `vars`: distinct var ids) |
| `linear[]` | `terms` (array of `[var id, value name, weight]`, weight an integer 0..=1,000,000, each (var, value) once), `limit` (integer 0..2^53) |
| router problem | `workers` (non-empty array, at most 65,535), `tasks` (non-empty array; tasks x workers at most 20,000,000), `affinity` (number >= 0, default 0), `comment` (string, ignored) |
| `workers[]` | `id` (string, unique), exactly one of `cap` / `capacity` (integer 0..2^53) |
| `tasks[]` | `id` (string, unique; default `task<index>`), `scores` (object: worker id -> number, each key once), `allowed` (non-empty array of distinct worker ids), `group` (string or number), `clamp` (a worker id), `text` (string, ignored) |

Numbers: any JSON number that is a finite double; `1e309` (which parses to infinity) is rejected (it aborted the process,
exit 134, before 0.2.0). Arrays and objects may nest at most 256 deep (the parser is recursive). Empty domains are errors: an
empty `values`, an empty `allowed`, or a variable / task left with no allowed value after `allowed`, `forbid` and `scores`.

**CLI (`probbit decide`, `probbit run`, `probbit ir`): bad input = exit 2 and ONE JSON object on stdout**
`{"error":{"code":"schema|value|limit","path":"tasks[3].allowed","message":"..."}}` plus one human line on stderr
(`probbit: bad problem: tasks[3].allowed: must be an array of strings`). `code`: `schema` = not JSON, a wrong type, an unknown,
duplicate or missing field; `value` = well-typed but unusable (an unknown or duplicate id / value name, an empty domain, a
negative or fractional cap, a clamp to a forbidden value, `probbit_ir` other than 1); `limit` = beyond a documented limit (a
number that is not a finite double, nesting deeper than 256, more than 65,535 values or workers, an integer above 2^53, the
size limits in "Limits in v1", stdin above `--max-input-mb`). Unreadable stdin and invalid UTF-8 are `schema` errors too
(0.2.1; they printed a stderr line only). `path` locates
the offending field (`""` = the whole document, as for JSON syntax errors and non-finite numbers, whose message gives the byte
offset). Flag errors keep their convention: exit 2, a message on stderr, nothing on stdout. The same strict reader parses
both front-ends (`probbit-cli/src/json.rs`; CLI test `input_contract_rejects_malformed_documents`, which includes the 2026-09-30 external
review's cases: `"allowed": "A"` used to enable every scored worker and answer `exact`; a cap object where the caps array
belongs was dropped; duplicate value names emitted duplicate keys).

**Library (`probbit_ir::Model::new`, `probbit_decide::Problem`):** typed Rust values, so there is no schema to check; `Model::new`
returns `Err(String)` for structural errors (lengths, out-of-range indices, `i == j` pairs, duplicate cap members, k outside
1..=65535). It does not check numeric magnitudes; the JSON front-ends are the validated entry point.

## Numeric contract

- **Inputs.** Every score / weight (`h`, `potts`, `table`, router `scores` and `affinity`) is a finite number with
  |x| <= 1e9 (`limit` error otherwise; `probbit-cli/src/json.rs` `MAX_WEIGHT`). Why 1e9: weights are natural-log odds, and a
  weight difference beyond ~745 already decides (e^745 is past the range of double-precision probability ratios), so the limit
  removes no distribution you can express; it keeps every log-space quantity (log w sums over variables, pairs and
  same-group pairs; log Z) below ~1e9 x (input size), far inside the double range (1.8e308). Caps and limits are integers
  0..2^53. Above the limit a value is rejected, not clamped: rescale your scores.
- **Arithmetic** stays in log space: Gibbs conditionals and enumeration subtract the running max before `exp`; the frontier DP
  carries log weights and merges by log-sum-exp (before 0.2.0 it used linear weights scaled per layer, which underflowed past ~745
  nats; with weights of +-1000 it returned NaN marginals labelled `exact` and the CLI aborted, exit 134: probbit-ir test
  `frontier_is_exact_in_log_space_for_huge_weight_spreads`, which fails on the old DP; cost +9% / +12% wall on the
  42 / 60-task demos at `--frontier-states 4194304`, identical printed odds and log Z).
- **Outputs.** Every number in a decision is finite. Numbers with |x| >= 1e15 print in exponent form (`1e20`); the printer
  used to multiply by 1e6 before rounding and printed a literal `inf` (invalid JSON) for |x| > ~1.8e302. A gate diagnostic
  that cannot be estimated (`rhat` / `tv_bound` infinite: too few samples, chains stuck at different values) appears only on
  a refusal (exit 3): it prints as `null` and is named in `gate.non_finite` (e.g. `["rhat", "tv_bound"]`). Any other
  non-finite computed number means no answer: ONE `{"error":{"code":"numeric","path","message"}}` object on stdout, exit 3
  (before 0.2.0 it printed `null` with exit 0). Telemetry rates are 0 when no time elapsed (they divided by zero).
- **Fuzz test** (CLI `fuzz_inputs_never_crash_and_outputs_stay_finite_json`): 10,000 seeded documents (programs and router
  problems with weights from -1e9 to 1e9, 5e-324, -0.0, plus 1 draw in 30 out of contract: 1e9+1, 1e15, 1e308, 1e309, -1e400;
  a third malformed by truncation, byte edits, an inserted `1e999`, duplicated slices or 300-deep nesting) through `probbit run` /
  `decide` / `ir` at fixed work: never a crash, stdout always one document that re-parses with the crate's own reader, no
  stray null. Exits on the current build: 1,045 answers, 491 infeasible, 8,015 bad input, 449 refused (~4 s on 4 threads).
  Before the log-space DP it found 2 aborts (router programs with scores of 1e9 on the frontier tier).

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
     ceil(chains / threads)); if any chain finds no start, all samples are dropped and the exact search runs again past its gap budget until the end of `--budget-ms`, counted from the start of the sampling phase, or half a budget after the fallback starts if later, and a hard stop since 0.2.0 (proof -> `infeasible`; a program
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
  2b. else (inference compiler) the components tier: the variables split into connected components of pairs AND caps
     (two variables are linked when a pair or a cap mentions both); log Z is the sum of the components' log Z and each
     variable's odds and MAP value come from its own component. A cap-free component whose pairs form a tree (pairs =
     variables - 1) is solved by sum-product (odds, log Z) and max-product (MAP) in log space; a two-value component with
     one identical symmetric coupling on every pair (complete graph, or no pairs) and caps that count every member able to
     take their value by the occupancy-count DP (O(n^2), n <= 2048); any other component by enumeration when its raw
     space fits what is left of ONE shared budget of `--exact-limit` (summed over the enumerated components, so the tier never
     costs more than one whole-program enumeration), else by the frontier DP. Output `tier: "forest"`
     (every component a tree), `"occupancy"` (every component counted) or `"components"`, plus `components` {count, forest,
     occupancy, enumerate, frontier}; one infeasible component makes the whole program `infeasible` (exit 1). It declines (and the sampler runs)
     when one component cannot be solved, the program is a single non-tree component (the whole-program tiers above already
     tried it), or the forest work (sum over edges of |values(i)| x |values(j)|) exceeds 5e7. When the raw space
     exceeds `--exact-limit` it runs FIRST, before the frontier DP and the whole-program enumeration (100,000
     independent two-value variables took 4,441.9 ms in the frontier DP vs 32.5 ms here, medians N = 5, same log Z), else
     after them; `--frontier-states 0` switches it off too (the earlier path). Measured (Apple M4, medians of N = 5, load 3.6-11): the external review's chain100 / chain1000 sampled
     281.9 / 281.3 ms -> forest 0.055 / 0.390 ms; four independent 8-variable rings with overlapping caps (non-partition,
     2^32 raw space) sampled 385.5 ms -> components 0.057 ms (odds within print rounding of a per-block brute force);
  3. else run 4 constraint-preserving Gibbs chains for `--budget-ms` (default 200) and apply the diagnostics gate
     (`tier: "sample"`, `gate` object).
- `exact`: enumeration, plus the components tier with enumeration per component (no frontier): a decomposable program
  is solved component by component, and a forest by sum-/max-product (chain1000: 0.47 ms). Whole-program enumeration is
  exhaustive, so use `exact` only on small or decomposable programs (an earlier build gave no answer within 60 s on a loose
  100,000-variable path program; with `--exact-ms 1000` it answers `declined` in 1.1 s).
- `sample`: chains + gate, never enumerate.
- `--sweeps N` replaces the wall-clock budget with N sweeps per chain (`--sweeps 0` = the wall-clock default). With `--polish-ms 0` or
  `--polish-sweeps M` the decision is then a pure function of (program, seed, `--chains` and the work/limit flags, without the wall-clock
  `--exact-ms`): byte-identical across runs (only timing
  fields differ; CLI tests
  `run_general_ir_programs`, `polish_sweeps_makes_the_whole_answer_deterministic`). With the default wall-clock polish
  (`--polish-ms 50`) the odds, verdict and released set are still identical, but the polished `plan` can differ run to run
  (997.89 vs 998.42 log w on the same 300-task input at `--sweeps 150`).
- `--polish-sweeps M` (both `probbit decide` and `probbit run`): polish for M sweeps per polish chain instead of `--polish-ms`
  of wall clock (deterministic; 400 sweeps cost ~20 ms on the 300-task demo). Telemetry reports `polish_sweeps`.

## Resource controls

Every control below works on both `probbit run` (this format) and `probbit decide` (the assignment front-end's sampler path).

- `--chains N` (env `PROBBIT_CHAINS`, default 4): independent chains (more chains = tighter between-chain checks, more work).
- `--threads N` (env `PROBBIT_THREADS`, default min(4, available cores)): worker threads; worker w runs chains w, w+N, ...
  Precedence: flag > environment > config file > default; `--chains` must be 1..=100,000 and `--threads` 1..=1,024, from any
  source (else exit 2; 0.2.1: `--chains 576460752303423488` aborted with exit 134, 1,000,000,000 exhausted memory; 100,000 is
  the largest count measured, BENCHMARKS §6). Config file:
  `$PROBBIT_CONFIG`, else `./probbit.json` if it exists, e.g. `{"chains": 4, "threads": 2, "cpu_limit": 50, "priority": "low"}`
  (CLI test `run_config_file_layer`); keys `chains`, `threads`, `cpu_limit`, `mem_limit_mb`, `priority` only (0.2.1: another
  key, or a document that is not an object, exits 2), and `decide` / `run` name the file in `telemetry.config`. At fixed work the thread count does not change the answer: with `--sweeps`
  fixed and a fixed-work polish (`--polish-ms 0` or `--polish-sweeps M`) the decision is identical at 1, 2, 3, 4 and 6 threads
  (each chain's RNG stream is independent of its thread; CLI tests `run_threads_and_chains_are_resource_controls`,
  `polish_sweeps_makes_the_whole_answer_deterministic`). Under a wall-clock budget or polish it does (below). The polish runs its
  4 annealing chains on `--threads` workers (before, it used 4 threads whatever `--threads` said); with a wall-clock
  `--polish-ms` each chain then gets polish-ms / ceil(4 / threads), so `--threads 1` polishes less: on the 300-task demo
  (`--sweeps 400`, N = 5) CPU/wall went from 2.26 to 1.04 and the plan from 707.77 to 705.12 log w (median; `bench/polish_threads.py`).
  With a wall-clock `--budget-ms`, the budget is the deadline of the whole sampling phase: each chain gets
  budget / ceil(chains / threads), so fewer threads = fewer sweeps per chain = a looser (honestly reported) bound (`probbit run`; the
  router sampler of `probbit decide` instead stops the r-th chain of a worker at call start + (r + 1) slices since 0.2.0, so a chain's
  overrun is charged to the worker's next chains: 100,000 chains sampled 354-432 -> 269-318 ms on 200 at loads 3.5-4.2, with no
  chain sweeping, BENCHMARKS §6). In the current build
  that slice includes the chain's feasible-start search (before, a 200-job schedule sampled for 1.75 s on a 1 s budget:
  now 1.00 s); a start slower than the slice leaves the chain with no samples and the answer is `refused`, and the
  start search itself stops at the deadline (a 200-job program that cannot start: `--budget-ms 500` answered `refused` in 0.67 s (measured on an earlier build, before the fallback; not re-measured),
  was 2.4 s; `--op sample`: 504 ms inside the run, 0.58 s wall). The exact tiers,
  the gate and the polish run outside the budget (200-job schedule, `--budget-ms 1000`: 1.29 s wall), except the
  fallback of `--op decide` (the start search stops at half the slice and the fallback stops HARD at the end of the
  sampling budget or half a budget after it starts, whichever is later; before 0.2.0 its deadline counted from the start of the call and was consulted only between plans, so a
  program with many plans ran on toward `--exact-limit`: 20 x 30 precedence chain, ~7 s past a 200 ms budget).
  Under `--op decide` with a wall-clock budget and no `--exact-ms`, a program whose raw space (product of domain
  sizes) exceeds `--exact-limit` gets the exact tiers before the sampler for at most `--budget-ms` (from the start of the
  call); past it they decline as at `--exact-ms`, the sampler then gets its full budget, and the sampled answer carries
  `telemetry.exact_budget_reached: true`. So such a call takes at most about 2 x `--budget-ms` plus gate and polish
  (20 x 30 precedence chain at 200 ms: 0.54 s wall, was 15.2 s). Programs whose raw space is within the limit, fixed work
  (`--sweeps`) and `--op exact` are unchanged; a program whose exact answer needs longer than the budget is now sampled.
  Loose 3-colourings (`bench/csp_gap_probe.py` generator, mean degree 2, one edge clamped, seeds 1..3, M4 load ~5.5): 3,000
  vertices 1,502-1,504 ms total with the old uncapped tiers (emulated by `--exact-ms 1e9`: exact phase 1,189-1,191 ms) ->
  510-515 ms (exact phase 200.1 ms), the same `refused` answer (the enumeration declines either way); 300 vertices unchanged
  (exact phase ~140 ms < the 200 ms budget; total ~440 ms both ways).
  The exact tier's cost can dominate: on loose programs with more than `--exact-limit` plans it counts plans up to the limit
  before declining (`--budget-ms 200`, loose 3-colourings: 0.44 / 0.73 / 1.66 s wall at 300 / 1,000 / 3,000
  variables vs 0.29-0.35 s for `--op sample`). Lower `--exact-limit`, or use `--op sample`, to keep such calls near the budget (3,000 variables at
  `--exact-limit 1000` / `20000`: 0.38 / 0.39 s), or cap them with `--exact-ms N` (below).
- `--deadline-ms N` (`probbit run` only, opt-in): a whole-call wall-clock target counted from before parsing. The
  exact tiers stop at N/4 under `--op decide` (N under `--op exact`; the smaller of this and `--exact-ms`); the sampler then
  gets 0.6 of the time left (at most `--budget-ms` when that flag is also given; otherwise `--budget-ms` is replaced) and the
  polish at most 0.1; the rest is the reserve for the gate, which is not bounded, so the answer reports `deadline: {"ms": N,
  "met": bool}` (met = `phases` total <= N). If N runs out in the exact tiers the answer is `refused` (exit 3). With `--sweeps`
  (fixed work) it is a flag error (exit 2). Measured (20 x 30 precedence chain, M4, load ~6.5, N = 5 each): N = 500 / 1000 /
  2000 -> median total 488.4 / 950.0 / 1,848.8 ms (max 488.9 / 951.3 / 1,852.0), met 15/15; answers `refused` / 2 of 5
  `partial` / 5 of 5 `partial` (the gate costs ~44% of the sampling time on this program). Test `run_deadline_ms_bounds_the_whole_call`.
- `--exact-ms N` (opt-in; absent = no cap, output unchanged): a wall-clock stop for the exact tiers that run before the
  sampler (enumeration / MRV search and the frontier DP, together), counted from the start of the call; the clock is read every 64
  search nodes and per DP step. Past it they decline as at `--exact-limit`: `--op decide` (and `probbit decide --mode auto`) goes on to
  the sampler with the full `--budget-ms`; `--op exact` and `probbit decide --mode exact` answer `declined` (exit 3, the reason names
  `--exact-ms`; `decide --mode exact` exited 2 with an empty stdout before 0.3.0). A sampled answer then carries `telemetry.exact_ms` and `telemetry.exact_ms_reached` (only when the flag is
  given). Not capped by `--exact-ms`: the no-start fallback above (it stops hard at the end of the sampling budget). The router's
  lowering for its components tier (O(group size^2) pairs) runs only under `--mode auto` with `--frontier-states` > 0 and stops at the
  cap (clock read per group member); what still runs past it is one `Model::new` of the lowered pairs (one group of 3,000 tasks:
  `probbit decide --exact-ms 50` 741 ms whole call vs 618 ms under `--mode sample`, N = 5 medians; answers identical at fixed work). Loose 3-colourings at `--budget-ms 200`
  (re-run on the final binary of an earlier build, medians of 5): `--op decide` 430 / 699 / 1,480 ms at 300 / 1,000 / 3,000 variables ->
  341 / 347 / 346 ms at `--exact-ms 50` (`--op sample`: 292 / 296 / 296 ms). A cap shorter than a program's exact time turns its exact answer into a sampled one.
  The frontier DP stops per DP step: `probbit decide --frontier-states 4194304` answers the 42 / 60 / 90-task demos (`--seed 2`) exactly
  in 0.24 / 1.60 / 26.4 s (layers up to 27k / 121k / 855k states); with `--exact-ms 50` the whole call took 51 / 69 / 56 ms
  (a 60-sweep sampler run included; 1 run each).
- `--cpu-limit PCT` (env `PROBBIT_CPU_LIMIT`, 1-100, default 100): per-thread duty cycle (sleep after each ~2 ms of sweeping).
  Same answer at fixed `--sweeps`. Measured (2000-spin ring, 4 threads, fixed work, medians of 5): utilisation 98.4% /
  40.2% / 19.0% at 100 / 50 / 25, so the sweeping stays under the cap; but the same work cost 916 / 1104 / 3751 ms CPU and 233 / 688 /
  4948 ms wall: duty-cycled threads run much less efficiently on the M4 (likely lower clocks / efficiency cores, inferred).
  To cap the CPU share cheaply, lower `--threads` first. Only the sweep loop is duty-cycled; the exact tiers, the feasible-start
  search, the gate (one thread per chain per statistic, whatever `--threads` says) and the polish run at full speed.
- `--priority low|normal` (env `PROBBIT_PRIORITY`, default normal): `low` sets nice 10 via setpriority(2) before any chain
  starts (Unix; `ps` shows NI 10 during the run; on Windows `--priority low` exits 2: not implemented, untested). Reported as `telemetry.nice`.
  Under contention (M4, a 10-thread foreground run vs a 10-thread probbit hog, 5 reps): nice 10 alone did NOT yield on
  macOS (foreground slowed x1.78 by a normal hog, x1.73 by a nice-10 hog). So on macOS `low` also sets PRIO_DARWIN_BG (the
  background band: throttled, may stay on efficiency cores): the foreground is then slowed only x1.02. The price: a `low` run
  on an otherwise idle M4 takes x4.2 the wall time (x3.9 CPU) of a normal one (400-spin ring, fixed work; same answer).
  Use `low` to stay out of the way; use `--threads` to share the machine and stay fast. Linux (nice 10 = ~1/9 of a nice-0
  thread's CFS weight) is unmeasured.
- `--mem-limit-mb N` (env `PROBBIT_MEM_LIMIT_MB`, config `mem_limit_mb`; default 1024 in the current build, `0` = unbounded): caps the buffers that grow with the budget
  (per chain, a trajectory of n x 2 bytes + 8 bytes per kept sweep, needed for the gate's batch means) at N MB in total, never
  below 64 rows per chain. When a chain's buffer is full it keeps every other row and records every 2nd sweep from then on
  (equally spaced thinning); the odds still count every sweep, and the gate's error bar is computed on the thinned chain, which
  is conservative (the full-chain mean has variance <= a thinned-chain mean for any stationary chain; MacEachern & Berliner
  1994, lit.). Measured (earlier build, M4): 300-task `probbit decide --budget-ms 3000` peaks at 580 MB unbounded, 40.6 MB with
  `--mem-limit-mb 16` (same sweeps, the gate passed both); against exact odds (forced sampler, 8 seeds each of the 18-task, 24-task and
  24-task `--hard` demos, unbounded vs 1 MB: 24 runs per setting) a 1 MB cap (~31x thinning; ~2x on `--hard`) released the
  same 528 tasks with 0 outside tolerance. **Where it costs**: on the slowly mixing
  300-task `--hard` demo (1 s budget, only ~13k sweeps, 36.6 MB peak unbounded) a 2 MB cap (~4x thinning) cut releases from
  300 to a median of 239 (5/5 `partial`); 8 MB barely thinned (300, 300, 300, 299, 300). Thinning a chain that mixes slowly
  leaves fewer rows for the same information, so the gate escalates more: set the cap generously; it is meant for long budgets.
  The capped buffer is now reserved once (it used to grow by doubling, so peak RSS reached ~1.6x the cap). 300-task
  demo, one run each: 10 s budget, peak RSS 1,667 MB unbounded, 1,632 MB at `--mem-limit-mb 1024` before the fix, 1,042 MB
  after; 3 s at 256 MB: 275.6 MB; 3 s at 16 MB: 33.5 MB (all passed the gate, 300 released). The cap bounds the sample buffers, so peak
  RSS = cap + the process's base (~20 MB here). Unbounded RSS grows ~167-195 MB per second of budget on this demo (1 / 3 / 10 s:
  190.5 / 585.6 / 1,666.9 MB). Caveat: the reservation is virtual until touched on macOS/Linux; on Windows it is committed (untested).
  Default 1024 MB (300-task demo, 10 s: 1,024 MB peak vs 1,625 MB unbounded, N = 3; 200 ms: 41.7 vs 52.5 MB, N = 5; all
  passed 300/300). On a machine with less free memory than ~1.1 GB, set a smaller cap. Program, JSON and marginals are not capped.
  Telemetry: `mem_limit_mb`, `traj_rows` (rows kept, all chains).
- `--progress [MS]` (default 100): one JSONL line on stderr roughly every MS ms while the decision runs,
  `{"event":"progress","ms","sweeps","site_updates_per_s","process_cpu_ms","peak_rss_mb"}` (sweeps over all chains, counted in
  steps of 64; updates/s since the previous line; `sweeps` stays 0 while an exact tier runs). Stdout is unchanged. Cost at fixed
  work (two runs): router -3.0% / -0.6%, 400-spin ring +1.3% / +4.6%, 3-variable toy -3.1% / -8.3% site updates/s
  (IQRs overlap; tiny programs likely pay a real cost of up to ~8%).
- `probbit stats [--sweeps N]`: the processor's spec sheet: OS/arch/logical CPUs, compiled-in target features, every control's
  effective value and its source (`flag` / `env` / `file` / `default`), and a measured self-test (400-spin ring, fixed work) at 1
  thread and at the effective thread count. Apple M4, 7 runs: 23.05 M site updates/s on 1 thread, 77.65 M on 4 (x3.37).
- `probbit decide` threads (300-task demo, 4 chains x 400 sweeps, polish 0, 5 runs, medians): 74.1 / 58.5 / 50.9 ms wall at
  `--threads` 1 / 2 / 4 (sampler 14.6 / 27.1 / 47.4 M site updates/s; the exact tiers' decline is a fixed cost); answer identical.
- Measured (Apple M4, 400-spin ring, 4 chains x 2,000 sweeps, 5 runs, medians): `--threads 1` 139.0 ms wall /
  146.5 ms CPU; `--threads 2` 74.8 / 154.7; `--threads 4` 42.7 / 165.6 (3.26x faster for +13% CPU); marginals identical.

## Decision

`verdict`: `exact` (proof-grade under the score model) | `diagnostics_passed` (0.1.0: `certified`; the gate's 3σ Monte-Carlo
bound on each variable's TV error is ≤ 0.05, plus split R-hat < 1.05, ≥ 8 batches per chain in both batch passes, batch-size stability and no frozen
caps; tuned on exact oracles; a diagnostic, not a proof: it cannot see a mode no chain visits, BENCHMARKS "Known failure modes") |
`partial` (`released` variables passed their own bound and the stricter per-item global checks; `escalated` did not) | `refused` (exit 3) | `infeasible` (exit 1) |
`declined` (exit 3; `--op exact` stopped at `--exact-ms`, or ran out of its node budget, ~9.2e18 nodes: unreachable in practice, inferred). Every document
has `engine`, `op`, `program` (vars, values, pairs, caps counts), `verdict`, `ms`; `infeasible`, `declined` and a no-start
`refused` add only `reason`. Answers add `plan` (the best plan seen, or the exact MAP), `plan_logw`, `violations` (of `plan`),
`compiled` ({pairs_dropped, caps_dropped}: pairs whose every entry is exactly 0 and caps whose limit reaches their
number of distinct member variables are removed before tier selection; tables_folded: tables that are a sum of one
term per end, t[a][b] = u[a] + w[b] exactly in f64 (constant tables included), are folded into the two variables' unaries;
none of this changes any odds, log Z or plan beyond float summation order; `program` counts the compiled program, so the
input had program.pairs + pairs_dropped + tables_folded pairs and program.caps + caps_dropped caps),
`marginals` (per variable, values sorted by probability), `released`, `escalated`; exact tiers add `tier`, `logz`, `top_plans`
and `n_feasible` or `frontier_states` or `components`. Sampled answers add `tier`, `sampled_best_logw`, `gate` (rhat, tv_bound, tv_tol,
frozen_saturated_caps, frozen_escalated (variables the frozen rule escalates; on programs that are not partitions only the
stuck parts' connected components, forced variables included while anything is stuck), min_batches, samples, sweeps, chains, budget_ms (echoed even under `--sweeps`), seed, and gate/2+: version ("gate/3" since 0.2.0: every release also needs the variable's own
indicator split-R-hat < 1.05, whole answers every variable's; reason `item_rhat`), item_rhat_max (largest finite per-variable
indicator R-hat) and item_rhat_infinite (variables whose R-hat is infinite: chains holding different values throughout),
occupancy_rhat_max / occupancy_rhat_infinite (per capacity constraint, split-R-hat of its load trace across chains,
largest finite value and count of infinite ones; a variable is released only if every constraint it belongs to is below 1.05,
reason `item_rhat`), mode_transitions (accepted collective moves summed over chains: global_flips, label_swaps, and chains_without = chains
that accepted none; all 0 with `--collective off`; Wolff cluster and three-cycle moves are NOT counted (checked); present when the
sampler tracks them per chain), assumptions
(strings: what a sampled release assumes and does not check), mcse_tv_short / mcse_tv_long (the two batch-means MCSEs, max over
variables), min_batches_long (the long pass; gate/2 requires both passes to have ≥ 8 batches per chain), worst (id, value, pooled_mean, chain_means,
chain_rows: the worst item's per-chain estimates), non_finite (only on a refusal whose rhat / tv_bound is infinite)), a per-variable
`release_reason` (`whole_answer_gate` | `item_gate` | `frozen` | `rhat` | `batches` | `batch_stability` | `item_rhat` | `item_bound`) and
`telemetry`: chains, threads (= min(`--threads`, chains)), sweeps, site_updates_per_s, sample_ms, gate_ms, polish_ms, polish_sweeps,
cpu_limit_pct, mem_limit_mb, traj_rows, exact_ms and exact_ms_reached (only with `--exact-ms`; `exact_ms_reached` = the exact tiers ran and the clock at the
sampler's start was past the cap; always false under `--op sample` / `--mode sample`, where no exact tier runs), exact_budget_reached
(only when true: `--op decide` stopped the exact tiers at `--budget-ms`, see Resource controls), and on Unix
process_cpu_ms and peak_rss_mb (getrusage; matches `/usr/bin/time -l` on macOS) and nice (getpriority); all three null on
Windows. `probbit decide` reports the same `telemetry` on its sampler path (`sweeps` = all sweeps incl. burn-in, as `probbit run`); its
document differs: `tasks` / `workers` / `affinity` instead of `op` / `program`, `odds` instead of `marginals`, no `tier` or
`sampled_best_logw` on the sampler path, and gate `frozen_saturated_workers` plus `batch_ratio`. Bad input: exit 2 (bad JSON
or a document that breaks the input contract above: structured error on stdout; an unknown flag, a flag value that does not
parse or is out of range, a value flag given twice (`--progress` too in the current build), `--mode` other than auto / exact /
sample: message on stderr).

Every `probbit run` answer (exact, sampled, refused, infeasible, declined) ends with `phases`: an array of
`{"phase": name, "ms": wall}` in the order parse (reading stdin JSON), compile (validation, lowering, compile pass), exact
(the exact tiers; on a sampled answer the time before the sampler), sample (the chains, including their start search), gate,
polish (measured wall time; `telemetry.polish_ms` is the configured budget), total (= parse + compile + the answer's `ms`).
Exact answers have sample = gate = polish = 0. Example (20 x 30 precedence chain, defaults, M4): parse 0.8, compile 2.7, exact
200.0, sample 200.6, gate 88.9, polish 51.5, total 544.6 ms. Test `run_reports_phase_times`.

## Summary (`--summary`, decide and run)

The same answer without its per-item tables, for agents and dashboards: every field of the full document except `plan`,
`odds` / `marginals`, `released`, `escalated`, `release_reason` and `top_plans`, plus

- `summary`: 1 (the summary format's version), right after `engine`;
- `counts`, right after the verdict (and its `tier` / `reason`): `tasks` (decide) or `vars` (run), and, when the full document
  lists them, `released` and `escalated`;
- `worst_released`, `worst_escalated` (when the full document has `released`): up to 5 items each, `{id, value, p, odds,
  reason, bar}`: `value` is the item's value in the plan and `p` that value's probability, `odds` the item's two most likely
  values, `reason` its `release_reason` (sampled answers), `bar` the error bar the gate compares with `tv_tol` (z x the larger
  of the two batch-means MCSEs; `null` when it cannot be estimated; sampled answers only). Order: largest `bar` first on a
  sampled answer; on an exact answer the smallest gap between the top two odds first.

Verdict and exit code are the full document's. With `--pretty` and a terminal on stderr a boxed summary is drawn there
too (not with `NO_COLOR`, `--plain` or `PROBBIT_THEME=plain`). Test `summary_is_the_answer_without_the_tables`.

## Decision API (`probbit evaluate`)

Decision models (judges: a hosted System One API, a local server, a Workers AI model) answer each question about a piece of
content on its own: text in, a probability per option out, no rules, no joint answer, no refusal. `probbit evaluate` is the layer
after any of them: it reads the judge's request plus the judge's probabilities and the workflow's rules, and returns the most
likely answer set that obeys every rule, in the judge's response shape, with odds per option and the gate's verdict.

**Request** = a System One request plus one optional `probbit` block. The System One fields were checked against the published
schemas on 2026-10-02: TypeSafe's OpenAPI 0.2.0 (`https://api.typesafe.ai/openapi.json`, `POST /v1/systemone`; the official Python
SDK, PyPI `typesafe-sdk` 0.7.2, generates its wire models from it) and the Workers AI input / output schemas of
`@cf/cloudflare/clef` and `@cf/cloudflare/clef-flash` (`https://developers.cloudflare.com/workers-ai/models/clef/`). They agree:

| field | type | probbit |
|---|---|---|
| `model` | string | echoed when `probbit.judge` has no `model` (else the judge's, else `"probbit"`) |
| `state` | string, object or array | accepted, never read |
| `images` | array (a Workers AI extension) | accepted, never read |
| `questions` | object: question id -> question (at least one) | one probbit variable per question |
| `questions.<id>.type` | `noul` (yes / no), `choice`, `score` | anything else: exit 2, code `value` |
| `questions.<id>.instructions` | any JSON | accepted, never read |
| `questions.<id>.criteria` | `choice`: object option -> description; `score`: array of level descriptions, lowest first; `noul`: optional object with `true` / `false` | the options: `choice` the keys in order, `score` the levels `"0"` .. `"L-1"`, `noul` `"false"`, `"true"` |

`probbit` (all optional; strict like every probbit document): `judge` = the judge's System One response, of which only `answers`
(`type`, `noul` = P(yes), `probabilities` keyed by option or level), `model` and `usage` are read; `weights` = per question id
a probability (`noul`: P(true)) or probabilities keyed by option, as a judge returns them; `logw` = per question id natural-log
weights keyed by option (a missing option 0); `floor` = the probability floor (default 1e-6, 0 < floor <= 1); `rules` = the
probbit-ir constructs over question ids and option names: `caps`, `implies`, `tables`, `precedes`, `linear`, `all_different` and the
soft `pairs`, exactly as in a program. One weight source per question (a second is exit 2); a question with none has uniform
weights. Every (question, option) a rule names must be one of that question's options, and a `value` cap must count at least one
question that has the value (exit 2, code `value`, at the rule's path): the program's value names are one alphabet shared by all
questions, where another question's option would be accepted and would quietly exclude (an `implies` target) or be dropped (a
table tuple, a cap member, a linear term).

**Compilation** (no new engine code): one variable per question, its options as its allowed values, log-weight = ln max(p,
floor): p = 0 becomes ln 1e-6 = -13.815511, so a rule can still force an answer the judge ruled out. The value alphabet lists the
score levels first in order (`precedes` and `linear` read values as ordered slots), then `false`, `true`, then the choice options
in order of first appearance; value names are shared, so a cap on `"true"` without `vars` counts every yes / no question. The
program runs exactly as `probbit run` runs it, with every `probbit run` flag; `--program` prints it instead (`probbit evaluate --program |
probbit run` gives the same document minus `model`, `answers` and `usage`: test `the_program_round_trips_through_run`, exact and
sampled, exits 0 and 3).

**Response** = `engine`, `model`, `answers` (keyed by question id, in request order), `usage` (the judge's, else zeros), then
every field of the `probbit run` document (`op`, `program`, `compiled`, `verdict`, `tier`, `plan`, `plan_logw`, `violations`,
`marginals`, `released`, `escalated`, the gate, `telemetry`, `phases`). Each answer has the judge's shape filled from the joint
answer: `noul` = P(true) under the rules; `choice` = the joint answer's option, `confidence` = its probability, `probabilities`
per option; `score` = the expected level under the rules, `confidence` = the joint level's probability, `legend`, `probabilities`
per level. Plus `probbit`: `value` (the joint answer's option), `p` (its probability), `judge` (the judge's own answer: its most
likely option; on an exact tie the tied option the plan holds, so a tie is no change; null without weights), `changed` and
`released`. `confidence` has no published vendor formula; here it is probbit's probability of the returned option. The joint answer
is the most likely record, so an option can differ from its question's most likely option under the rules (the 12-question
example: `sla` is `24h` with probability 0.363 while `4h` has 0.604, because the record with `urgent` = false leaves `sla` free;
"Modelling" below). No `answers` when there is no plan (`infeasible`, `declined`, a refusal before sampling).

**Exit codes and errors** as `probbit run`: 0 an answer, 1 `infeasible` (the rules admit no record), 2 bad input (one error object;
paths inside the request, e.g. `probbit.rules.implies[0].then.var`, `questions.a.type`) or a bad flag (stderr), 3 `refused` /
`declined` (every answer present with `released: false`, all in `escalated`).

**Invariants** (probbit-cli/tests/evaluate.rs):
- no rules = the judge (`without_rules_the_answer_is_the_judge_argmax_with_the_same_probabilities`): 150 random requests of 1-12
  questions, every answer = the judge's argmax, `changed` false, `exact`, and the probabilities equal the judge's to print
  precision (largest gap 1.1e-16; probabilities with 4 decimals that sum to 1). A judge's p = 0 prints as 0.000001 (the floor)
  and the other options shrink by at most 1e-6 per such option: (0.7, 0.3, 0) prints as (0.699999, 0.3, 0.000001).
- with rules, answers move only where the judge's record breaks a rule (`with_rules_only_broken_components_move`): 300 random
  requests (3-12 questions, 1-4 rules each: implies, forbidden pairs, all_different), 235 answered: in every rule-connected group
  of questions whose judge record obeys its rules nothing moved (120 groups); in every group where it broke one at least one
  answer moved and the group's answer is the most likely assignment that obeys its rules (191 groups, each checked by brute
  force); nothing moved outside them; `violations` 0; `changed` marks exactly the moved answers.
- the refusal (`a_refusal_keeps_the_exit_code_and_releases_nothing`): 40 questions under a tight cap at `--op sample --sweeps 30`:
  `refused`, exit 3, every answer present and unreleased; no plan at all (the rules admit none): `infeasible`, exit 1, no `answers`.
- bad input (`bad_requests_are_one_error_object_located_in_the_request`): 21 malformed requests (5 of them rules naming an option
  their question does not have), each exit 2 with one error object at the right path; a bad flag prints a stderr line only.
- the vendor shape (`answers_have_the_vendor_shape`) and the weight sources (`weights_judge_and_logw_are_the_same_program`: the
  judge's answers, `weights` and `logw` compile to the same program).

Measured (Apple M4, load 2.1, medians of 7, `examples/evaluate/support-12.json`: 12 questions, 10 implications, a forbidden pair
and a linear budget): `exact` (enumeration), 5 of 12 answers moved, 0 violations, 1.24 ms inside the answer, 3.4 ms for the whole
process; without its rules 4.62 / 6.6 ms (the joint space of 248,832 records is enumerated whole); `--op sample --sweeps 2000`:
`diagnostics_passed`, 2.62 / 5.5 ms; at `--sweeps 400` the gate refuses (1,440 samples, bound 0.071 > 0.05). MCP: tool
`probbit_evaluate` (docs/agents.md). Python: `probbit.evaluate(request, judge=...)` with a callable or a System One URL (python/probbit.py).
One example per surface, from the repository root with `probbit` on `PATH`:

```sh
# shell
probbit evaluate --summary --pretty < examples/evaluate/support-12.json
# Python (python/probbit.py, stdlib only)
PYTHONPATH=python python3 -c 'import json, probbit; print(probbit.evaluate(json.load(open("examples/evaluate/support-12.json")))["verdict"])'
# MCP: tool probbit_evaluate, here over a pipe (in an agent: claude mcp add probbit -- probbit mcp)
{ echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"sh","version":"1"}}}'
  python3 -c 'import json,sys; print(json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"probbit_evaluate","arguments":json.load(sys.stdin)}}))' < examples/evaluate/support-12.json; } | probbit mcp
# browser: build the module, then open the page from disk (no server)
sh playground/build.sh && python3 -m webbrowser "file://$PWD/playground/index.html"
```

## Visuals and agents

Everything visual goes to stderr, only when stderr is a terminal, never with `NO_COLOR` (non-empty), `--plain` or
`PROBBIT_THEME=plain` (anything but `neon`), and never changes stdout: `--top` (decide, run) draws a monitor at 10 Hz (tier,
sweeps against the budget, site updates/s, CPU, peak RSS, the gate) and erases it at exit; `probbit demo --live` (default when
stdout and stderr are both terminals) tells the router story with a live field drawn from the chains' states; `probbit` and
`probbit --help` at a terminal print the hero screen on stdout (exit 0; piped, `--help` prints the usage and a bare `probbit`
exits 2). `--top` with `--progress` is a flag error (both write stderr). Test `a_terminal_on_stderr_never_changes_stdout`
runs every command with stderr on a pseudo-terminal and compares stdout with a piped run.

`probbit mcp` serves `probbit decide`, `probbit run`, `probbit stats`, `probbit demo` and `probbit evaluate` as Model Context Protocol tools on
stdio, with this document's schema (plus a `flags` object) as `probbit_run`'s input and the Decision API request (whose `rules` take
this document's rule definitions) as `probbit_evaluate`'s; see docs/agents.md.

## Modelling

- One entity, one variable: decisions of one entity tied together by `implies` (per message: an action, its tool, its
  length) freeze single-site moves, because no single variable can change without breaking an implication; the gate then
  refuses at every budget (correctly) and the refused plan can be far from the best. One variable per entity whose values are
  the allowed combinations, each with its combined score, mixes: an external tester's day plan (120 variables, 203 caps) was
  refused at `--budget-ms 2000` with plan log-weight 65.37; as 20 product variables it passed the gate at `--budget-ms 20`
  with 93.25 (27.9 nats better; re-run on 0.3.0, M4, load ~4).
- Plan or marginals: `plan` is one joint plan that satisfies every rule at once; `marginals` are each variable's
  probabilities over all plans. A variable's plan value need not be its most likely value (the plan must fit every other
  variable). Act on the plan when the variables must be consistent with each other; use the marginals for one variable's
  uncertainty or to rank variables for review.
- Budgets: `--budget-ms` bounds the sampling phase; parsing, the exact tiers, the gate and the polish come on top (each in
  `phases`). `--deadline-ms` targets the whole `probbit run` call.

## Limits in v1 (measured or stated)

- Size limits (0.2.1; each a `limit` error, exit 2, checked before the allocation it guards): at most 20,000,000
  (variable, value) pairs, n x k for `probbit run` and tasks x workers for `probbit decide` / `probbit ir`; at most 65,535 workers (the
  IR's value limit; 65,536 aborted `probbit decide`, exit 134); per `precedes`, at most 100,000 (slot of `before`, slot of
  `after`) pairs of allowed slots to check, as for a `tables` allow-list (a 30 KB program with 3,000 slots made 9 million caps,
  1.45 GB); at most 20,000,000 cap members in total after the constructs are lowered; and `--max-input-mb N` (`decide`, `run`,
  `ir`; default 256, 0 = no limit) on stdin, since parsing costs up to ~9x the input (a 200 MB numeric array peaked at
  1.78 GB). The dense bound keeps the largest program measured below (200 x 65,535 = 13.1 million) and refuses the review's
  0.6 MB amplifier (500 x 65,535 = 32.8 million pairs, 4.1 GB). Within the bounds memory still grows with n x k and with the
  caps (below): a program near both bounds can need several GB.

- Dense domains (a documented v1 limit: sparse domains are not built): memory and per-step cost scale with n x k even when each
  variable allows a few values. k = 65,535 values, 3 allowed per variable, unaries only (`probbit run`, seed 1, M4, load ~10, N = 1): n = 50: `decide` exact (forest) 141 MB peak RSS, 27.5 ms; `--op sample` 1,083 MB,
  860.8 ms (gate 513.3 ms, polish 120.1 ms at a 50 ms polish budget). n = 200: `decide` 512 MB, 125.5 ms; `--op sample`
  3,584 MB, 1,851.9 ms (sample 238.4, gate 999.2, polish 549.7 ms). So at large k the gate and the polish overrun their budgets
  and `--deadline-ms` cannot be met; keep k small (one value per slot actually used) until sparse domains land. Where it goes
  (n = 200, `--op sample`): `--polish-ms 0` 2,435 MB / 1,204 ms vs 3,444 MB / 1,751 ms with the default 50 ms polish (polish
  526 ms): the polish (`anneal_on`) built one scaled copy of the whole model per temperature (5), outside its clock; the other
  ~2.4 GB and the ~0.9 s gate are the sampler's and gate's dense per-(variable, value) arrays. Polish fixed in 0.2.0 (the
  chain carries the temperature, `Chain::beta`; no copies; same plans bit for bit, test `anneal_beta_matches_scaled_clones`):
  n = 200 `--op sample` median of 5 (load 17-22): polish 50.1 ms at its 50 ms budget, peak RSS 2,322 MB with or without it,
  total 1,280 ms (polish off 1,220 ms); n = 50: polish 50.1 ms, 621 MB (was 120.1 ms, 1,083 MB). The gate (0.5-0.9 s) and the
  dense arrays remain: **sparse per-variable domains are a v1 limit** (not done): `Model` stores allowed / h / cap
  memberships for all n x k pairs, the sampler and gate keep per-(variable, value) statistics. Programs with k in the
  thousands should list only the values actually used (or split them into several variables).
- Fixed in the current build (was a known false refusal, found by a kill test): on the router and any partition program, what-if clamps (or
  single-allowed tasks) that fill a worker's capacity on their own made the gate call the worker frozen and refuse (10/10
  clamped 24-task demos). `probbit_ir::FORCED_CAPS_PARTITION` is now on: such a capacity, and the classes of its forced members,
  are not a mixing signal (the rule non-partition programs have used since an earlier build). After the fix: clamped demos 10/10 passed the gate, 240
  released, 0 outside ±0.05 of exact; 20 random-clamp demos 480 released, 0 false; acceptance `forced_caps_do_not_freeze_the_gate`.
  It moved 2 of 7 golden gate digests (inputs with forced legal tickets); the calibration families have no forced members.

- The frontier-DP exact tier runs on the IR; the assignment front-end calls it through `Problem::lower` (its earlier copy
  was deleted after matching it: same DP layers, marginals ≤ 3.3e-16, log Z ≤ 6.8e-13 apart). The router's mixing
  accelerators (group block moves, two-group moves, window move, focus) stay in the assignment front-end: its hot loops
  are kept specialized (IR sampler 0.95–1.25× the specialized one; IR enumeration +2.3% on the 300-task probe) and are
  bit-identical to the IR's with the accelerators off (acceptance `ir_lowering_bit_identical`). `probbit run` samples with
  site + swap Gibbs plus, with `--collective on` (the default; `Model::collective` in the library, off by default there),
  a global two-value flip and a label swap per sweep; both samplers stay bit-identical with it on
  (`ir_lowering_bit_identical_collective`). `--cluster on` (default since 0.2.0) adds a Wolff cluster move (probability 1/2
  per sweep) and `--cycles on` (default since 0.2.0) max(n/4, 1) three-cycle rotations per sweep, attempted only when the
  program has k > 2 values and at least one cap (two values cannot hold three distinct values; without caps site moves already
  connect every state, and the answer is then byte-identical to `--cycles off` at fixed work). Library defaults
  (`Model::cluster` / `Model::cycles`) stay off.
- A feasible start is found by augmenting-path matching when every (variable, value) pair is in at most one `cap`
  (quotas; failure proves `infeasible`, exit 1), otherwise by a randomised depth-first search that branches on the
  variable with the fewest feasible values, each attempt capped at 2M nodes or 5e8 capacity checks, retried in ascending value
  order and with 4 fresh tie-breaks; under `--budget-ms` the search stops at the chain's deadline. If the program has
  two-variable forbid caps (limit 1; `precedes`, binary `tables`, hand-written pair caps) and root arc consistency over them
  removes at least one value, an arc-consistent search runs first (MAC: AC-3 over those caps after every assignment, plus
  forward checking on full caps; same random value order and tie-break, same budgets). Programs where nothing is pruned skip
  it, so their starts are unchanged (all golden tests identical). Test `mac_start_places_precedence_chains_and_is_sound`.
  Cost where it applies (`examples/mac_start_cost.rs`, median of 7 seeds, M4 load ~9, MAC on vs off via `START_MAC`): loose
  3-colouring with a clamped edge, 300 / 3,000 vertices 0.63 / 12.6 ms vs 1.37 / 41.4 ms; mean degree 4, 300 vertices 2.9 vs
  30.8 ms (max 1,101 vs 1,633 ms); 20 x 30 precedence chain 0.156 vs 5.25 ms (max 0.18 vs 447 ms). MAC was faster in all four;
  untested: programs with many binary caps where propagation prunes little (each node copies the n-variable domain bitsets).
  With `--sweeps` there is none (an unstartable 200-job program: each chain gives up after 2.09-2.12 s). If the
  search gives up the answer is `refused` (exit 3), never `infeasible`: running out of budget proves nothing (under `--op decide`
  the fallback above may still prove it). In an earlier build a start search that went 6,000-8,000 variables deep ABORTED the
  process (stack overflow, exit 134, empty stdout): an 8,000-variable 2-colouring path under `probbit run --op sample` and `--op
  decide` at `--budget-ms 1000` (measured on that build); chain threads now get a stack sized to the program and the same
  runs answer `certified` (now `diagnostics_passed`) in 1.7-1.8 s. At 30,000 variables the search gives up first: `refused` (exit 3), before and after.
  The exact tiers recurse once per variable too, on the caller's thread: in an earlier build a 100,000-variable clamped program
  aborted `probbit run --op decide` and `--op exact` (exit 134, main-thread stack overflow after 8.4 s); above 2,048 variables they
  now run on their own thread with a program-sized stack (the same runs: `exact`, exit 0, 0.07 s wall). The JSON
  front-ends look ids up in hash indexes (they were linear scans: a 30,000-variable program parsed in 0.56 s, now 0.03 s wall
  for the whole call; the router's 100,000-task input 12.1 s -> 3.8 s, and then -> 0.07 s: the remaining 3.75 s was the group-mates
  table, also a scan of every task per task, now bucketed by group with identical lists).
- Unique or near-unique solutions (e.g. sudoku): the chains cannot move, the gate reports frozen capacities and refuses
  (20/20 generated puzzles refused, 0 variables released, 0 false releases; re-run on a later build: the same). On programs that are
  not partitions `frozen_saturated_caps` also counts free variables no chain ever moved. In the current build a variable that unit propagation
  forces (a cap its forced members fill removes that value from the others, to a fixpoint) is a constant, not frozen, so a puzzle
  naked singles solve passes the gate (`diagnostics_passed`) under `--op sample`; and a frozen variable escalates only its connected component (free
  variables linked by pairs and caps), so independent parts can still be released (`partial`). A whole answer still needs
  nothing frozen. Partition programs (the router) are unchanged. `--op decide` answers exactly by
  enumeration instead. For programs that are not partitions (a (variable, value) pair in several caps, as in sudoku or
  colouring) enumeration branches on the most constrained variable (dynamic MRV): a test puzzle is solved and
  proved unique in 0.24 ms (was 660 ms), and under-determined puzzles get their exact per-cell odds and solution count
  (4,849-305,753 solutions in 0.11-8.1 s, per-puzzle medians, on an earlier build). A dedicated bitmask backtracking solver is still faster at the same full count: 14-18x on the open puzzles, 32-74x on
  the unique ones (same machine; re-measured with the timings above, 5 runs: 15-20x and 36-102x per puzzle, medians 17-18x and 48-53x).
- Plan polish: a sampled answer's `plan` is the chains' best plan annealed for `--polish-ms` (default 50; beta 2 -> 32,
  never worse; `sampled_best_logw` = before polish). Not a hard deadline: each router polish chain (`probbit decide`) builds its first
  temperature's chain before it reads the clock (~20 ms on 100,000 tasks in groups of 4). Earlier builds measured ~2 s over on that shape
  (~7.7 s at `--threads 1`); the cause was an O(t^2) plan score, fixed in the current build (82-109 ms after sampling at `--polish-ms 50`). Heuristic, not an optimum: on max-cut an untuned simulated annealing
  still finds larger cuts at equal time (anneal-only mode 0.06% / 0.7% behind, sampling mode 0.17% / 1.75%; BENCHMARKS §3).
- Measured on an Apple M4: a 60-spin Ising ring (table couplings) passed the gate in 200 ms, max per-spin error 0.0032 vs
  the exact transfer-matrix marginals (bound reported 0.0039).
- Measured on an Apple M4 (seeds 1–3, polish off): on 80 / 300-variable team rosters (3^80 / 3^300 plans, beyond
  enumeration) the gate passed 6/6 with true max per-variable TV 0.0032–0.0089 vs the frontier's exact marginals,
  under its reported bound in 6 of 6 runs (0.0055–0.0117). Its best-seen plan was far from the exact MAP (log w 26.9 vs 31.5,
  87 vs 119): for the single best plan, use the exact tier or the polish.
