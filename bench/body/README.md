# Benchmark: needs, surprise, pace, dispositions, valence, rest and load (`bench/body/`)

## Two rules before anything runs

1. **Freeze before measuring.** Fixtures, generators, seeds and every bar's pass value are committed with their sha256 in
   `fixtures/MANIFEST` in one commit before any measurement on the engine. A bar is never edited after a number is
   seen. A bar that fails is a finding: the design is fixed or the part is reverted, and the failed result stays in the
   history.
2. **No opposing mechanisms.** No fixture makes two mechanisms push against each other on the same probes unless the bar
   is about that opposition. Each fixture below turns on only the blocks its bars measure; every gene spread not under test
   is 0; readings and feedback never share a probe set with opposite signs.

Each bar row names its pass value, how it is measured, the null it must reject (a bar the null passes is not a bar), and
the reason the pass value is reachable by construction or by a stand-in number. Closed forms are recomputed in
`arith_0002.py` (ARITH ids in brackets).

This benchmark defines "done" for RFC 0002 ([docs/rfcs/0002-neuromodulators-and-body.md](../../docs/rfcs/0002-neuromodulators-and-body.md))
with numbers a test can see. It runs on Python 3.9 (standard library only) and
the probbit binary; `run.py` exits 1 when any gated bar fails and prints its wall time. Every number it prints carries its
scope: the persona, the seeds and the events.

```
PROBBIT_BIN=target/release/probbit python3 bench/body/run.py              # every part; writes result.json next to run.py
PROBBIT_BIN=target/release/probbit python3 bench/body/run.py --only needs,gate
python3 bench/body/arith_0002.py                                          # the closed forms: 40 checks, 0 mismatches
python3 bench/body/manifest_check.py                                      # every frozen file against fixtures/MANIFEST
python3 bench/body/fixtures/gen.py --verify                               # every stream and variant regenerates byte for byte
```

**Frozen and unmeasured.** The fixtures, generators, seeds, streams, scripts and every bar's pass value were committed with
their sha256 in `fixtures/MANIFEST` before any engine had the RFC 0002 blocks. No bar has been measured. On an engine
without a part's block, `persona lint` refuses the fixture as an unknown field; that part reports
`setup error: this engine does not support <block>`, and `run.py` reports each such part and exits 2.

## Layout

```
bench/body/
  run.py            every part -> result.json; exit 0 every gated bar passes, 1 one fails, 2 setup error
  common.py         binary lookup, canonical JSON, live/turn/verify wrappers (and an interactive life for a scripted host),
                    the engine check, TV distance, Spearman, bootstrap (2,000 resamples, random.Random(20261014))
  arith_0002.py     the closed forms (Python 3.9): 40 checks; the ARITH ids in brackets below
  manifest_check.py fixtures/MANIFEST: check, or write it (and fixtures/bars.md) at a freeze
  needs.py  gate.py  pace.py  dispositions.py  valence.py  rest.py  load.py  replay.py  habits.py  p0.py
  fixtures/
    needs1.yaml     one need (n 4, m 3, weight 1, cap 1, amount 0.2, satisfy_from [env]), one bound goal with
                    `expect_by_hours: 1` (N-R6), drives defaults: n1, n2, n7
    needs2.yaml     two needs (context: per_reading of number sense `tokens`, amount 0.6, forecast_per_hour 0.06;
                    contact: per_hour 0.04, amount 0.8, satisfy_from [human]), goals work (interest 1.0), compact and
                    report (interest 0.6 each; `expect_by_hours: 1`), want 2, pursue.cap 3, success {rate 0.2, init 1},
                    drives `pursue.glow: 0` and no wanting signals (rule 2: afterglow must not oppose the need term), habit
                    `quiet_hours` (`when: {quiet: true}`, `then: {pursue: {not: [report]}}`): n3, n5, n8, n9, n10, n11
    gate.yaml       one 3-level trait, uniform prior, learning rate 0.5, step_cap 0.2, total_cap 1, credit block, surprise
                    at its defaults (volatility_rate 0.3, v0 0.45, r_ref 4, r_max 20, g_max 3): s1-s5
    pace.yaml       drives with one paid goal (win_max 4), reward_from [env], pace (half_life_hours 6, cap 4, r0 1, levels
                    [0.5, 1.5], effects initiative and verbosity): p1a-p6
    optimism.yaml   drives with one paid goal, expectation rate 0.2, half_life_hours 48, dispositions optimism_spread 0.3;
    optimism-spread0.yaml   the copy at 0: o1-o5
    valence.yaml    drives with drives.valence (mood valence, expect 0.2, surprise 0.6), mood inertia 0.61, mood half-life
                    1 h, expect_by_hours 2: v1-v3, om1
    rest.yaml       the gate's trait without surprise, rest (downscale 0.5, prune 0.05, threshold 0.5, gap_hours 4,
                    rise 0.055, fall 0.238): r0-r5
    load.yaml       load (half_life_hours 72, kappa 0.5, theta 0.5, cap 4, from criticism / sense_fail / expired, hold on a
                    declared mood): l1-l5
    variants/       the twins and nulls, derived from the fixtures above by gen.py: each fixture without its block (the
                    P0 copies and fixed-rate twins), the habit-free twins, needs2 at horizon 2 h and at window 1e6 h
    gen.py          seeded generators; their outputs (streams/*.jsonl, variants/*.yaml) are committed and re-checked
                    against MANIFEST (`python3 fixtures/gen.py --verify` regenerates them byte for byte)
    streams/        fortnight, absent, probe, flip, blocks, loop, two-point, pairs, reversal, lineage, patterns
    bars.json       the pass values the scripts compare against
    bars.md         the bar tables of this README, extracted by manifest_check.py (its sha256 freezes the bars)
    MANIFEST        sha256 and size of every file above
```

The values in the fixtures are the stand-in values the research measured with (needs2, gate, pace, rest) or test values
for a closed form (needs1, valence, load); none is tuned on the engine.

## Streams and seeds

| stream | generator (seeded) | used by |
|---|---|---|
| **fortnight** | 14 days of hourly events; `quiet` true 23:00-08:00; context `tokens` reading uniform 0-0.12 per waking hour; contact grows 0.04/h; the host sends one report per hour the stance pursues `report`, answered (contact amount 0.8 from `human:principal`) at the next event with p 0.5; the principal also writes unprompted with p 0.15 per waking hour; a pursued `compact` satisfies context 0.6 from `env:compactor` at the next event | n5, n8, n10 |
| **absent** | the fortnight's opening 2 days, then 5 days with no principal messages and no answers | n11 |
| **probe grid** | warm-up: one event with `goals.compact.win: 1` and `goals.report.win: 1` from `env`, so both bound goals start at one E; then 71 probe states (x_context, x_contact) on the 0.1 grid, off the diagonal, minus (0.3, 0.4) where both reductions are 0 [n3.probe_size], each reached by one event from the saved warm-up state (a `tokens` reading for context, `elapsed_hours` x/0.04 for contact) | n3 |
| **flip** | phase A praises level 1, phase B praises level 2; phase lengths 40, 60, 100; feedback sign flipped with p 0, 0.10, 0.20 | s1-s3 |
| **blocks** | 2,000 hourly events in 50-hour blocks of reward rate 0.3, 1.0, 2.0 | p1a-p1c |
| **loop** | 400 hourly events; a scripted host acts a = base × (1 + κ × (R̄ − r0)) times an hour (base 4, r0 1, actions clipped to [0, a_cap]) and pays p × a as one win; p 0.30, κ 0.2 / 0.5 / 0.7 / 0.9 / 1.0, R̄ starts at r0 + 0.2; a_cap 12, and a_cap 20 for the engine-cap variant | p6 |
| **two-point** | 20,000 wins of 0.5 or 1.5 at p 0.5, `elapsed_hours` 0 | o1, o2 |
| **pairs** | 300 events with wins, cues and setbacks, one stream for every seed pair | o3, o4 |
| **reversal** | 60 turns praising level 1, an idle gap of 8 h with S above threshold, 60 turns praising level 2; noise 0.10 | r0, r1 |
| **lineage** | RFC 0001's absorbing lineage: 10 generations of 300 rewarded events, with and without rest lines | r2 |
| **patterns** | three 300-event streams: repeated criticism every 6 h; a failure reading every event (no habituation past the drift cap); a mood held above θ_m for 48 h | l1-l3 |

Seeds: individuals 0-99 (fortnight, absent: the stand-in's count, 1,400 days); 1000-1199 (flip: the held-out seeds
the stand-in passed on); 0-4 (two-point, o1); 0-99 (two-point, o2); pairs (0, 1) … (38, 39) (o3); 0-199 (reversal);
generators `random.Random(1000 + seed)`.

## Bars

### P1 `needs` (mechanism)

| bar | pass value | how measured | null it must reject | reachability check |
|---|---|---|---|---|
| n1 deprivation scaling | win at deficit 0.8 / win at 0.2, amount 0.2, ≥ 2.0 | needs1: two events from states grown to 0.8 and 0.2; the wins in the state | block absent (the amount is the win): ratio 1.0 | closed form 2.0229 [n1.ratio]; 6-decimal rounding moves it < 1e-5 |
| n2 overshoot pays < 0 | win at deficit 0, amount 0.2, < 0 | needs1: amount at init | block absent: +0.2 | closed form −0.117 [n2.overshoot] |
| n3 pursuit follows the larger expected reduction | 100 % of the 71 probe states | needs2 probe grid: order of the two bound goals' pursue odds vs the order of s_g × (D(x̂) − D(x̂ with x̂_j ← \|x̂_j − K\|)) | random pursuit: 50 %; the marginal-drive rule ∂D/∂x picks the other goal in 2 of 71 states [n3.null_marginal] | the term enters the field monotonically; both goals have one interest and s = 1 (the warm-up wins keep it there); ties are excluded; the largest term is 1.621, below `pursue.cap` 3 [n3.term_max] |
| n4 replay | 0 differing bytes; every verify exit 0 | every needs strand vs a third copy and a split copy | one changed amount in a copy: verify exit 1 | the arithmetic is a function of logged inputs (RFC Invariants) |
| n5 0 habit breaks | 0 of 1,400 simulated days; ≥ 50 bound stances | fortnight: quiet-hour stances pursuing `report`, by an independent Python check | the habit-free twin pursues `report` in quiet hours at least once | habits are rules of every program; the stand-in held 0 of 1,400 |
| n6 P0 | every document byte-identical | the RFC 0001 P0 corpus re-run with this build | a planted 1-byte change is detected | the blocks are optional and their ids reserved only with them |
| n7 refusal of `self` / `clock` amounts | 100 % refused; run = run without them, byte for byte | needs1: the same amounts with `src` self, clock, missing, malformed, through MCP | the same amounts from `env`: accepted and change the state | N-E2 |
| n8 contact pursuits under a ceiling | ≤ 2 per waking day (mean over seeds 0-99), and every seed pursues each bound goal and is paid at least once (non-vacuity) | fortnight: released stances pursuing `report` in waking hours / 14 | random pursuit over 3 goals: 5 per waking day; marginal-drive rule: 9.5 (stand-in); an inert need (0 pursuits) fails the non-vacuity check | the research stand-in's 1.13 used a fixed answer probability 0.5; a second stand-in of this fixture with the success rate (actions landing at the next event) gave 1.49 |
| n9 0 negative wins at horizon 0 | 0 satisfactions of a pursued goal with win < 0 | fortnight at τ 0 | marginal-drive rule: negative wins down to −0.611 (stand-in) | at τ 0 the term is positive only when x > K/2, and growth before the satisfaction only raises x, so r > 0 [n9.sign_rule] |
| n10 a short horizon pays | at-cap hours per day (context ≥ 0.95 cap) at τ 2 h ≤ 0.5 × τ 0, with 0 overshooting satisfactions at τ 2 h | fortnight at τ 0 and τ 2 | τ 0 (0 % cut); τ 8 (overshoots 0.85 a day, stand-in) | **least sure:** the research stand-in (satisfaction in the same hour, fixed answer probability) gave 0.39 → 0.04 a day (−90 %), 0 overshoot; the second stand-in of this fixture (actions landing at the next event, success rate) gave 0.854 → 0.991, a rise, with 0 overshoot |
| n11 the principal away | contact pursuits per day 0 on absent days 2-5 (stream days 4-7: within 2 days of the absence) | absent stream, mean over seeds 0-99 per day | `expect_by_hours` 1e6 (a window that never fires inside 7 days): s stays, pursuits continue; the fixed-probability rule: 15 a day (stand-in) | by construction: with no wins during the absence s only falls (× 0.8 per unanswered report), and after 6 unanswered reports 2 × 0.8^6 × 0.883 < ln(1.0/0.6) [n11.extinction], so at most 6 report pursuits follow the absence's start, inside its opening waking day; research stand-in 3, 0, 0, 0, 0; second stand-in 2.52, then 0 |

### P5 `surprise` (mechanism)

| bar | pass value | how measured | null it must reject | reachability check |
|---|---|---|---|---|
| s1 flip re-learning | median turns to re-learn phase B, gated / fixed ≤ 0.75, every phase length, noise ≤ 0.20 | gate.yaml vs a copy without `surprise`, flip stream | the fixed rate: 1.0 | held-out stand-in 0.55-0.71 (noise 0-0.20, T 40/60/100); the fixture is the stand-in's learner, so its arithmetic is the stand-in's |
| s2 stable-phase movement | total \|Δ\| moved in phase A, gated / fixed ≤ 1.5 | as s1 | a gate with v0 0.25: 2.08 at noise 0.20 (stand-in) | held-out stand-in 1.01-1.10 |
| s3 caps | 100 % of steps within g_max × step_cap and Δ within ±total_cap | every gated step against the state | a planted unclipped step is detected | g ≤ 3 and total_cap untouched by construction [s.gain_bounds] |
| s4 replay | 0 differing bytes | gate strands | as n4 | as n4 |
| s5 P0 | byte-identical without the block | as n6 | as n6 | as n6 |

Noise above 0.20 is outside s1's scope: the stand-in's speed was 0.84× at 0.40.

### P2 `pace` (mechanism)

| bar | pass value | how measured | null it must reject | reachability check |
|---|---|---|---|---|
| p1a pace reads the past | stance t's `pace.rbar` equals R̄ recomputed from events 1..t, and no later event (including one paying decision t) changes stance t | blocks stream, strand recompute and a perturbed-future copy | a look-ahead pace (R̄ after event t + 1): differs at every rewarded event | the pace step runs before compile; the strand is append-only |
| p1b offered stakes do nothing | 0 pace changes over fuzzed offered stakes (goal cues, deadlines, declared amounts, non-reward inputs) | `fuzz` over pace.yaml with those inputs varied | a pace that adds the turn's offered stakes: changes | pace reads R̄ only (S57: the reward on offer did not move speed) |
| p1c pace tracks the reward rate | Spearman(pace, block reward rate) ≥ 0.7 | blocks stream | a random pace: about 0 | stand-in 0.776 at half-life 6 h, 50-h blocks |
| p6 loop gain | at G < 1 R̄ after 400 h within 1e-4 of the exact transient closed form; at G ≥ 1 pinned at the binding cap, never above `cap` | loop stream, five κ values, two a_cap | an uncapped R̄ at G ≥ 1 with a_cap 20: above 4 | closed forms 1.2632, 1.5000, 2.2500 [p6.fixed_point]; event form equals the stand-in's average on hourly events [p6.trajectory, within 1.1e-5]; pin 3.6000 = p × a_cap 12 [p6.pinned]; with a_cap 20, p × a_cap = 6 > cap, so the pin is cap 4 by the clip |

P2's cap, decay closed form, replay and P0 run inside p6, n4 and n6 (every fixture's strands replay; every fixture has a
copy without the block).

### P3 dispositions (mechanism)

| bar | pass value | how measured | null it must reject | reachability check |
|---|---|---|---|---|
| o1 E settles at the expectile | \|mean over individuals 0-4 of sign(o) × (Ē − τ_o b − (1 − τ_o) a)\| ≤ 0.01, Ē = mean E over wins 10,001-20,000 | optimism.yaml, two-point stream, fixed clock (`elapsed_hours` 0, so E's 48 h half-life never acts) | a gene-blind E (one rate): misses by tanh(\|o\|)/2 per individual, sign-aligned so it cannot cancel; individuals with \|o\| < 0.02 are excluded | the stationary mean is the expectile exactly (the drift is linear in E on (a, b)) [o1.stationary]; stand-in 1.1613 vs 1.1667 [o1.stand_in]; per-individual window noise is about 0.005, so only the pooled error is gated |
| o2 the gene's sign is readable | sign(E_z − E_twin0) = sign(z) for 100 % of individuals 0-99 | optimism.yaml vs its spread-0 copy on one two-point stream, E averaged over wins 2,001-3,000 | spread 0: sign 0; a shuffled gene: about 50 % | coupling: E_z ≥ E_0 at every step when z > 0 [o2.coupling, 100/100 at s 0.1 and 0.3]; stand-in 100/100 |
| o3 twin distance rises | mean §5.4 distance over 20 seed pairs > 0 at spread 0.3 | pairs stream, every other spread 0 | spread 0: distance exactly 0 | different z give different E (o2), E sets d, d sets afterglow, afterglow enters `pursue` (glow ≠ 0 in the fixture) |
| o4 0 habit breaks | 0; ≥ 50 bound stances | pairs stream | the habit-free twin binds | as n5 |
| o5 P0 | spread 0 gives today's bytes | p0.py over the copies | a planted change | spread 0 stores no gene |

### P4 valence and the omission let-down (simulation; omission = mechanism)

| bar | pass value | how measured | null it must reject | reachability check |
|---|---|---|---|---|
| v1 expectations shape valence | two lives with the same final win m 1 after a 24 h gap, one with E built by 20 earlier wins, one with E 0: final valence accumulators differ | valence.yaml | outcome-sized valence (evidence = m): identical | the 24 h gap rounds the mood accumulators to 0 (half-life 1 h) while E keeps 0.5^(24/48); the final d differs by E |
| v2 an expected win moves valence | a win with d 0 moves valence evidence by w_e × E > 0 | valence.yaml: a win equal to E | today's `effects.surprise`: 0 | the evidence term w_e × E is in the formula |
| v3 forgetting | push left 5 events after an impulse ≤ 0.09 of the impulse | valence.yaml at inertia 0.61 | inertia 0.9: 0.59 | closed form 0.61^5 = 0.0845 [v3.five_back] |
| om1 omission closed form | E' = E(1 − rate) and d = −min(E, pe_cap) to 1e-6, once per window | valence.yaml: a pursuit, no win, `elapsed_hours` past the window | today's format (no window): E unchanged | the rule's arithmetic [om1.closed_form] |

### P6 `rest` (part 2; simulation of S, mechanism of the downscale)

| bar | pass value | how measured | null it must reject | reachability check |
|---|---|---|---|---|
| r0 order kept | 0 strict order inversions among a trait's learned weights at every rest line; the top level unchanged | reversal stream, every rest line | Δ reset to 0 at a rest line: every strict order lost | x → ρx is increasing [r0.inversions, 0 in 40,000]; stand-in 200/200 |
| r1 plasticity returns | median re-learning turns with rest ≤ 0.5 × without, at ρ 0.5 | reversal stream, seeds 0-199 | ρ 1: ratio 1 | stand-in 11 → 5 (ratio 0.4545) [r1.relearn]; ρ 0.75 gives 8 and is outside the bar |
| r2 lineage drift | probe distance after 10 generations with rest ≤ without | lineage stream | RFC 0001's absorbing lineage without rest: 0.460 | inferred, not by construction: rest shrinks the Δ a generation absorbs |
| r3 replay | 0 differing bytes, rest lines included | rest strands | a strand with a rest line removed: verify exit 1 | rest lines are logged |
| r4 0 habit breaks | 0 | reversal stream | as n5 | as n5 |
| r5 P0 | byte-identical without the block | as n6 | as n6 | as n6 |

### P7 `load` (part 2; simulation)

| bar | pass value | how measured | null it must reject | reachability check |
|---|---|---|---|---|
| l1 three patterns | three pairwise distinct L trajectories (differ by > 1e-6 at ≥ 1 event) | patterns streams | block absent: all three 0 | each pattern adds through a different term (hits, non-habituating evidence, the hold term) |
| l2 recovery | L after quiet gaps = L0 × 0.5^(Σh/hl) within 1e-5 | load.yaml, a load then quiet events | a clock-free load: unchanged | identity [l2.recovery] |
| l3 cap | L ≤ L_cap at every event; repeated hits pin at the cap | patterns stream 1 | an uncapped L: steady state 4.454 > 4 | steady state κ(a − θ)/(1 − 0.5^(6/72)) above the cap, so the clip binds [l1.repeated_hits] |
| l4 replay | 0 differing bytes | load strands | as n4 | as n4 |
| l5 P0 | byte-identical without the block | as n6 | as n6 | as n6 |

The fourth pattern (an inadequate response with compensatory over-activity) needs two interacting channels and has no
bar.

## What this benchmark cannot see

- Whether a reading, an amount or a reward is true: a forged env label passes every bar.
- Whether a host acts on the stance or the pace level, or a model writes in it.
- Natural lives: every life here comes from a seeded generator, and sequential turns of one life are correlated.
- Other personas, longer lives, other hardware (bytes match wherever the floating-point maths agrees, persona doc §5.3).
- Whether any of this is like anything from the inside. No bar here can see it, and none claims it.

## How the scripts express the bars before the engine exists

The scripts were written against the RFC text before any engine had the blocks; where the spec above leaves a choice, the
choice is fixed here and frozen with the rest:

- **Scripted hosts.** fortnight, absent, flip, reversal and lineage depend on the stance the host reads (a report is sent
  when the stance pursues `report`), so their files hold the host's seeded draws; the scripts build the events from them,
  turn by turn, over one `live` process per life.
- **Two goals.** A drives block needs 2-7 goals, so every "one goal" fixture declares a second goal no event pays.
- **needs1** grows its deficit at `per_hour` 0.1, so 8 h and 2 h reach 0.8 and 0.2 (n1). The stance carries no win, so n1
  reads the win through E (rate 0.5 from E 0) and n2 through wanting (a setback of weight 0.5). n9 runs on the fortnight at
  τ 0 (needs2), as its row says; the win of each satisfaction is recomputed in Python from the stance-reported deficits.
- **n7** sends the refused sources through `persona turn`, and an `env` amount through MCP (which refuses any amount).
- **s1** counts turns into phase B until the top level is the praised one, censored at T; s2 compares the mean movement
  per (T, noise).
- **p1b** varies offered stakes (cues, deadlines, progress, novelty on both goals) over seeds 0-9 of a perturbed copy of
  the blocks stream; `fuzz` checks rules, not pace values. p1c correlates per event. **p6**: a new individual starts at
  R̄ 0, so three zero-hour wins bring it to r0 + 0.2 and the closed form starts from the engine's R̄; with a_cap 20 the
  host clips each payment at win_max 4, a win above it being an error (docs/persona.md section 2.9).
- **o3**'s distance (section 5.4) includes the `pursue` odds, where the gene acts through afterglow.
- **v2** reads the valence evidence as the accumulator's move beyond the inertia carry at elapsed 0; **v3** keeps the
  stance on the unpaid goal (habit `go_other`), so no E term follows the impulse.
- **r0** finds the rest line as a strand line with the top-level key `rest` (the RFC names the line, not its shape); the
  gap event carries no feedback, so the last phase-A decision is unpaid. **r2**: RFC 0001's lineage block does not exist
  yet, so the host absorbs each generation's learned weights into the next prior (`logw`); the distance is between the
  resting odds of generations 0 and 10.
- **`drive: {"n": 4, "m": 3}`** is quoted in needs1 and needs2: the persona reader refuses a bare `n` key (YAML 1.1
  reads it as false), so the RFC's schema example needs the same quoting, or the build renames the key.
- **load.yaml**'s weights (1 each) and hold (above 1, 0.01 an hour, on the two-level mood `strain`) are test values.
- Field names (`needs.<id>.deficit`, `pace.rbar`, `load.l`, `drives.expect`, `drives.surprise`, `dispositions.optimism`)
  follow the RFC. When the engine names a field differently, the script is fixed and the change is logged below; a pass
  value never moves.

## Changes after the freeze

None.
