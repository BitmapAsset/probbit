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
