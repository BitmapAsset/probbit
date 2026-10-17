# Benchmark: needs, surprise, pace, dispositions, valence, rest and load (`bench/body/`)

## Two rules before anything runs

1. **Freeze before measuring.** Fixtures, generators, seeds and every bar's pass value are committed with their sha256 in
   `fixtures/MANIFEST` in one commit before any measurement on the engine. A bar is never edited after a number is
   seen. A bar that fails is a finding: the design is fixed or the part is reverted, and the failed result stays in the
   history.
2. **No opposing mechanisms.** No fixture makes two mechanisms push against each other on the same probes unless the bar
   is about that opposition. Each fixture below turns on only the blocks its bars measure; every gene spread not under test
   is 0; readings and feedback never share a probe set with opposite signs.

Each bar row names its pass value, how it is measured, the null it must reject (a bar the null passes is not a bar), the
competent plain-Python null it must also reject (the policy a harness author would write instead of the block, played in
`nulls.py` through the bar's own predicate), and the reason the pass value is reachable by construction or by a stand-in
number. Closed forms are recomputed in `arith_0002.py` (ARITH ids in brackets).

This benchmark defines "done" for RFC 0002 ([docs/rfcs/0002-neuromodulators-and-body.md](../../docs/rfcs/0002-neuromodulators-and-body.md))
with numbers a test can see. It runs on Python 3.9 (standard library only) and
the probbit binary; `run.py` exits 1 when any gated bar fails and prints its wall time. Every number it prints carries its
scope: the persona, the seeds and the events.

```
PROBBIT_BIN=target/release/probbit python3 bench/body/run.py              # every part; writes result.json next to run.py
PROBBIT_BIN=target/release/probbit python3 bench/body/run.py --only needs,gate
python3 bench/body/arith_0002.py                                          # the closed forms: 66 checks, 0 mismatches
python3 bench/body/nulls.py                                               # every bar rejects its competent null (no engine)
python3 bench/body/manifest_check.py                                      # every frozen file against fixtures/MANIFEST
python3 bench/body/fixtures/gen.py --verify                               # every stream and variant regenerates byte for byte
```

**Frozen and unmeasured.** The fixtures, generators, seeds, streams, scripts and every bar's pass value were committed with
their sha256 in `fixtures/MANIFEST` before any engine had the RFC 0002 blocks. The bars are now at **version 2**: an
independent review found 28 of the 40 bars weak or vacuous (a degenerate policy or the stated null passed them), and they
were repaired before any engine number existed for any of them (section "Bars v2"). The v1 bar tables and pass values stay,
verbatim, in `fixtures/bars-v1.md` and `fixtures/bars-v1.json`. No bar has been measured. On an engine without a part's
block, `persona lint` refuses the fixture as an unknown field; that part reports
`setup error: this engine does not support <block>`, and `run.py` reports each such part and exits 2.

## Layout

```
bench/body/
  run.py            every part -> result.json; exit 0 every gated bar passes, 1 one fails, 2 setup error (it runs nulls.py
                    before the parts: a bar that passes its competent null is a setup error)
  common.py         binary lookup, canonical JSON, live/turn/verify wrappers (and an interactive life for a scripted host),
                    the engine check, TV distance, Spearman, bootstrap (2,000 resamples, random.Random(20261014))
  arith_0002.py     the closed forms (Python 3.9): 66 checks; the ARITH ids in brackets below; the parts import its v2 forms
  nulls.py          the competent plain-Python nulls, one or more per bar, judged by the parts' own gate functions
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
                    P0 copies and fixed-rate twins), the habit-free twins, needs2 at horizon 2 h and at window 1e6 h; bars
                    v2: needs2-s02 (success.init 0.2: n3), gate-g1.25 ... gate-g3.0 (constant gains: s1, s2), load-effects
                    (a load effect on caution: l1)
    gen.py          seeded generators; their outputs (streams/*.jsonl, variants/*.yaml) are committed and re-checked
                    against MANIFEST (`python3 fixtures/gen.py --verify` regenerates them byte for byte)
    streams/        fortnight, absent, probe, flip, blocks, loop, two-point, pairs, reversal, lineage, patterns; bars v2:
                    return, bursts, burst, two-point-gaps
    bars.json       the pass values the scripts compare against (`"version": 2`)
    bars.md         the bar tables of this README, extracted by manifest_check.py (its sha256 freezes the bars)
    bars-v1.json    the v1 pass values, verbatim (historical)
    bars-v1.md      the v1 bar tables, verbatim (historical)
    MANIFEST        sha256 and size of every file above
```

The values in the fixtures are the stand-in values the research measured with (needs2, gate, pace, rest) or test values
for a closed form (needs1, valence, load); none is tuned on the engine.

## Streams and seeds

| stream | generator (seeded) | used by |
|---|---|---|
| **fortnight** | 14 days of hourly events; `quiet` true 23:00-08:00; context `tokens` reading uniform 0-0.12 per waking hour; contact grows 0.04/h; the host sends one report per hour the stance pursues `report`, answered (contact amount 0.8 from `human:principal`) at the next event with p 0.5; the principal also writes unprompted with p 0.15 per waking hour; a pursued `compact` satisfies context 0.6 from `env:compactor` at the next event | n5, n8, n9, n10 |
| **absent** | the fortnight's opening 2 days, then 5 days with no principal messages and no answers | n8, n11 |
| **return** (v2) | the absent stream's 7 days (the same draws), then 3 days with the principal back | n11 |
| **probe grid** | warm-up: one event with `goals.compact.win: 1` and `goals.report.win: 1` from `env`, so both bound goals start at one E; then 71 probe states (x_context, x_contact) on the 0.1 grid, off the diagonal, minus (0.3, 0.4) where both reductions are 0 [n3.probe_size], each reached by one event from the saved warm-up state (a `tokens` reading for context, `elapsed_hours` x/0.04 for contact); v2 plays the same 71 events from a second warm-up (needs2-s02: three compact wins from success 0.2) | n3 |
| **flip** | phase A praises level 1, phase B praises level 2; phase lengths 40, 60, 100; feedback sign flipped with p 0, 0.10, 0.20 | s1-s3 |
| **blocks** | 2,000 hourly events in 50-hour blocks of reward rate 0.3, 1.0, 2.0 | p1a-p1c |
| **bursts** (v2) | no draws: 600 hours at 0.5 an hour on `task` three ways: steady (a win of 0.5 every hour), bursty (a win of 4 every 8th hour), spaced (an event every 3 hours with a win of 1.5) | p1c |
| **loop** | 400 hourly events; a scripted host acts a = base × (1 + κ × (R̄ − r0)) times an hour (base 4, r0 1, actions clipped to [0, a_cap]) and pays p × a as one win; p 0.30, κ 0.2 / 0.5 / 0.7 / 0.9 / 1.0, R̄ starts at r0 + 0.2; a_cap 12, and a_cap 20 for the engine-cap variant | p6 |
| **burst** (v2) | no draws: a new individual takes 12 events at elapsed 0, each a win of 4 (win_max), then 24 quiet hourly events | p6 |
| **two-point** | 20,000 wins of 0.5 or 1.5 at p 0.5, `elapsed_hours` 0 | o1, o2 |
| **two-point-gaps** (v2) | 3,000 wins of 0.5 or 1.5 at p 0.5, `elapsed_hours` uniform 0.5-3 | o1 |
| **pairs** | 300 events with wins, cues and setbacks, one stream for every seed pair | o3, o4 |
| **reversal** | 60 turns praising level 1, an idle gap of 8 h with S above threshold, 60 turns praising level 2; noise 0.10 | r0, r1, r4 |
| **lineage** | RFC 0001's absorbing lineage: 10 generations of 300 rewarded events, with and without rest lines (v2: each generation closes with one idle event) | r2 |
| **patterns** | three 300-event streams: repeated criticism every 6 h; a failure reading every event (no habituation past the drift cap); a mood held above θ_m for 48 h | l1-l3 |

Seeds: individuals 0-99 (fortnight, absent, return: the stand-in's count, 1,400 days); 1000-1199 (flip: the held-out seeds
the stand-in passed on; the constant-gain twins on 1000-1049); 0-4 (two-point, two-point-gaps, o1); 0-99 (two-point, o2);
pairs (0, 1) … (38, 39) (o3); 0-199 (reversal; 0-19 for r0's short control); generators `random.Random(1000 + seed)`.

## Bars

`version: 2`, repaired before any engine measurement (section "Bars v2"); the v1 tables are `fixtures/bars-v1.md`. A part's
gate is its pass value **and** every engine-run null the row names failing in the same run; `nulls.py` checks the
competent null column without an engine, and `run.py` stops (exit 2) when a bar passes one.

### P1 `needs` (mechanism)

| bar | pass value | how measured | null it must reject | competent null (nulls.py) | reachability check |
|---|---|---|---|---|---|
| n1 deprivation scaling | the win of every positive cell of the needs1 grid (deficits 0.2, 0.5, 0.8 × amounts 0.1, 0.2, 0.3) equals r = D(x) − D(\|x − a\|) within 5e-6, and the deficit after the event equals \|x − a\|; and win at 0.8 / win at 0.2 (amount 0.2) ≥ 2.0 | needs1: one event per cell from a new individual (the deficit grown 0.1 an hour, then the amount); the win read through E (rate 0.5 from E 0) | block absent (the amount is the win): ratio 1.0 | win = amount × deficit: ratio 4.0 (passes v1), off by 0.130 [n1.null_product] | closed forms [n1.grid]; ratio 2.0229 [n1.ratio]; 6-decimal rounding of E moves a win < 2e-6 |
| n2 overshoot pays < 0 | on the grid's five overshooting cells (deficit 0 and 0.05 below a/2) the signed reward equals r within 5e-6, the cell at a/2 (deficit 0.05, amount 0.1) pays nothing, and r(0, 0.2) < 0 | needs1 grid; a negative r read through wanting (a setback of weight 0.5) | block absent: +0.2 | win = deficit removed − overshoot: −0.2 at deficit 0 (passes v1), off by 0.099 [n2.null_linear] | closed forms [n2.grid]; −0.117 [n2.overshoot] |
| n3 pursuit follows the larger expected reduction | on both probes: 100 % of the 71 states order the bound goals' pursue odds as s_g × (D(x̂) − D(x̂ with x̂_j ← \|x̂_j − K\|)); ln(odds_compact / odds_report) equals term_compact − term_report within 1e-3; the stance's need terms equal the recomputed ones within 1e-5; the released goal is the field's argmax in every state; the warm-up success rates equal the closed form | needs2 probe (warm-up wins on both goals: s 1, 1) and needs2-s02 probe (three compact wins from success 0.2: s 0.5904, 0.2) | random pursuit: 50 %; the marginal-drive rule ∂D/∂x picks the other goal in 2 of 71 states [n3.null_marginal] | s-blind pursuit (terms at success 1): 71/71 on v1's probe (passes v1), 63/71 on the s02 probe [n3.s_flips: 8 states]; deficit share x/K: 62/71 | the term enters the field monotonically; ties are excluded; the largest term is 1.621 (v1's probe) and 0.993 (the s02 probe), below `pursue.cap` 3 [n3.term_max, n3.term_max_s02]; E does not enter the pursue field (section 2.9), so wins on one goal move only its s there |
| n4 replay | 0 differing bytes; every verify exit 0 | every needs strand vs a third copy and a split copy | one changed amount in a copy: verify exit 1 | a verifier that always says ok: refused, the gate needs a changed input, a removed line and two swapped lines of the bar's earliest strand refused | the arithmetic is a function of logged inputs (RFC Invariants) |
| n5 0 habit breaks | 0 of 1,400 simulated days; ≥ 50 bound stances | fortnight: quiet-hour stances pursuing `report`, by an independent Python check | the habit-free twin pursues `report` in quiet hours at least once | the 10-line deficit-threshold rule without its quiet-hours line: 2,098 quiet-hour reports; with that line it passes (a property any rule-keeping policy shares; n8 and n11 reject it) | habits are rules of every program; the stand-in held 0 of 1,400 |
| n6 P0 | every document byte-identical | the RFC 0001 P0 corpus re-run with this build | a planted 1-byte change is detected | a P0 check of released levels only: misses a changed odds digit the byte comparison sees | the blocks are optional and their ids reserved only with them |
| n7 refusal of `self` / `clock` amounts | through `persona turn` and through `live`: 4 of 4 bad sources (self, clock, missing, malformed) refused and the run equals the run without them; the same amount from `env` through the same two transports accepted and the deficit changed; MCP refuses an `env` amount on both tools (2 of 2) | needs1 | the `env` amounts: accepted, and they change the state | refuse every amount: the `env` control fails (v1's control went through MCP, which refuses every amount); a source check for `self` and missing only: 2 of 4 | N-E2; the RFC's MCP row (both tools refuse amounts) |
| n8 contact pursuits under a ceiling, serviced | ≤ 2 report pursuits per waking day (mean over seeds 0-99) on the fortnight **and** on the absent stream; 0 answered reports whose win is ≤ 0 (every answered report reduces the drive); every seed pursues each bound goal and is paid at least once | fortnight and absent: released stances pursuing `report` in waking hours / days; wins recomputed from the stance-reported deficits | random pursuit over 3 goals: 5 per waking day; marginal-drive rule: 9.5 (stand-in); an inert need fails the coverage check | the 10-line deficit-threshold rule (0.35): 1.976 on the fortnight (passes v1), 10.39 while the principal is away, 45 answered reports that paid nothing; the quota scheduler (reports at 10:00 and 16:00): 2.000, 467 answered reports that paid nothing | research stand-in 1.13; second stand-in 1.49; the RFC's rule in Python on the frozen draws (nulls.py reference): 1.447 on the fortnight, 0.710 while away, 0 answered reports paying ≤ 0 |
| n9 0 non-positive wins at horizon 0 | 0 satisfactions of a pursued goal with win ≤ 0; every seed has ≥ 1 positive-win satisfaction of each bound goal; context at cap (≥ 0.95) ≤ 2 hours per day | fortnight at τ 0; wins recomputed | marginal-drive rule: negative wins down to −0.611 (stand-in); never pursuing: 0 satisfactions | the 10-line rule (0.35): 0 negative (passes v1) but 51 that pay nothing; the same rule at 0.40 passes n9 and fails n8 and n11 | at τ 0 the term is positive only when x > K/2, and growth before the satisfaction only raises x, so r > 0 [n9.sign_rule]; the RFC's rule in Python: 0 non-positive, at least 3 positive satisfactions of each goal per seed, 0.798 at-cap hours a day |
| n10 a short horizon pays | the τ 0 baseline has context at cap (≥ 0.95 cap) ≥ 0.25 hours per day; at τ 2 ≤ 0.5 × τ 0, with 0 overshooting satisfactions; every deficit the stance reports equals the Python recomputation from the logged readings and amounts within 1e-6 (both arms); every τ 2 seed compacts and is paid at least once | fortnight at τ 0 and τ 2 | τ 0 (0 % cut); τ 8 (overshoots 0.85 a day, stand-in) | per-need thresholds (context 0.4, contact 0.6) in both arms: 0 at-cap hours, so 0 ≤ 0.5 × 0 passed v1; the baseline floor rejects it | **least sure:** the research stand-in (satisfaction in the same hour) gave 0.39 → 0.04 a day (−90 %); the second stand-in 0.854 → 0.991, a rise; the RFC's rule in Python on the frozen draws (nulls.py reference) 0.798 → 0.569 (−29 %): the baseline clause holds in every stand-in, the halving in one of three; the pass value is kept |
| n11 the principal away | contact pursuits per day (mean over seeds 0-99): ≥ 0.5 on stream days 1-2 (before the absence), 0 on absent days 2-5 (stream days 4-7), ≥ 0.5 on return days 2-3 (stream days 9-10 of the return stream); and the window-1e6 twin fails the same gate in the same run | absent and return streams | `expect_by_hours` 1e6 (a window that never fires inside 7 days): pursuits continue; the fixed-probability rule: 15 a day (stand-in) | give up after 3 unanswered reports, for good: 0 a day once the principal is back; the 10-line rule: 13.69 a day while away | with no wins during the absence s only falls (× 0.8 per unanswered report); after 6, 2 × 0.8^6 × 0.883 < ln(1.0/0.6) [n11.extinction]: this bounds the count, not the day; research stand-in 3, 0, 0, 0, 0; **least sure:** the RFC's rule in Python on the frozen draws (nulls.py reference) gives 0.95, 1.72, 2.28, **0.02**, 0, 0, 0, then 0.12, 1.01, 1.30: 2 of 100 seeds pursue once on stream day 4, so it fails the away clause; the pass value is kept |

### P5 `surprise` (mechanism)

| bar | pass value | how measured | null it must reject | competent null (nulls.py) | reachability check |
|---|---|---|---|---|---|
| s1 flip re-learning | median turns to re-learn phase B, gated / fixed ≤ 0.75, every phase length, noise ≤ 0.20; and in every (T, noise) cell, every constant-gain twin (rate and step_cap × g, g 1.25 / 1.5 / 2 / 2.5 / 3) whose phase-A movement is no more than the gate's needs strictly more turns to re-learn (seeds 1000-1049) | gate.yaml vs a copy without `surprise` and the constant-gain twins, flip stream | the fixed rate: 1.0 | constant gain 1.5: ratios 0.60-0.75 (passes v1); within its own movement budget it ties with itself | held-out stand-in 0.55-0.71 (noise 0-0.20, T 40/60/100); the RFC's gate in Python (nulls.py reference, seeds 1000-1049) passes both clauses: strictly fewer re-learning turns than every same-budget constant gain in 9 of 9 cells |
| s2 stable-phase movement | total \|Δ\| moved in phase A, gated / fixed ≤ 1.5; and in every cell ≥ 90 % of gated lives end phase A with `mid` on top and re-learn phase B before T | as s1 | a gate with v0 0.25: 2.08 at noise 0.20 (stand-in); frozen learning: 0 movement, no lesson | constant gain 3 (the gate's top gain, always on): up to 2.56× | held-out stand-in 1.01-1.10; the RFC's gate in Python passes s2 (nulls.py reference, seeds 1000-1049) |
| s3 caps and the opening step | 100 % of steps within g_max × step_cap and Δ within ±total_cap; and every life's opening paid step equals the closed form within 1e-5: the gate's learned weights and (v, r, g) = (0.15, 2, 1.666667), the fixed twin's weights | every gated step against the state; event 1 of every life (it pays decision 0) | a planted unclipped step is detected; a learner that never steps | constant gain 1 on a new learner: its opening step is off by 0.163 [s3.first_step] | g ≤ 3 and total_cap untouched by construction [s.gain_bounds]; the opening step follows the RFC's listed order (v, Ω, counts, r, g, step) [s3.first_state] |
| s4 replay | 0 differing bytes | gate strands | as n4 | as n4 | as n4 |
| s5 P0 | byte-identical without the block | as n6 | as n6 | as n6 | as n6 |

Noise above 0.20 is outside s1's scope: the stand-in's speed was 0.84× at 0.40.

### P2 `pace` (mechanism)

| bar | pass value | how measured | null it must reject | competent null (nulls.py) | reachability check |
|---|---|---|---|---|---|
| p1a pace reads the past | stance t's `pace.rbar` equals R̄ recomputed from events 1..t, and no later event (including one paying decision t) changes stance t | blocks stream, strand recompute and a perturbed-future copy | a look-ahead pace (R̄ after event t + 1): differs at every rewarded event | a batch-average pace over the event file: stance 100 moves when later events change | the pace step runs before compile; the strand is append-only |
| p1b offered stakes do nothing | 0 pace changes over fuzzed offered stakes (goal cues, deadlines, declared amounts, non-reward inputs); and in each fuzzed stream an accepted win of 1 (goal `other`, event 1,000) moves R̄ there by exactly 1 − 0.5^(1/6) (within 2e-6) | `fuzz` over pace.yaml with those inputs varied, seeds 0-9, each with the positive control | a pace that adds the turn's offered stakes: changes; a constant pace: the control does not move it | a constant pace: 0 changes (passes v1), the control fails | pace reads R̄ only (S57: the reward on offer did not move speed); the control is the event form |
| p1c pace tracks the reward rate | Spearman(pace, block reward rate) ≥ 0.7 on blocks; on the bursts streams (all 0.5 an hour): mean R̄ over hours 304-599 within 0.02 between steady and bursty, R̄'s range on bursty ≤ 1.0, and R̄ at the last spaced event within 1e-4 of k r / (1 − 0.5^(3/6)) | blocks stream; bursts streams | a random pace: about 0 | the current-reward pace: Spearman 0.266 on blocks, range 4.0 on bursty | stand-in 0.776 at half-life 6 h, 50-h blocks; 0.499996 vs 0.5 [p1c.equal_average]; range 0.401 [p1c.bursty_range]; 0.558741 [p1c.spaced] |
| p6 loop gain | at G < 1 R̄ after 400 h within 1e-4 of the exact transient closed form; at G ≥ 1 pinned at the binding cap, never above `cap`; and after the burst (12 zero-hour wins of 4) R̄ never passes 4, is 4 from the 10th win, and recovers as 4 × 0.5^(h/6) over 24 hours (within 2e-6) | loop stream, five κ values, two a_cap; burst stream | an uncapped R̄: 5.2369 after the burst (v1's loop clipped payments at 4, so it could not pass 4) | the cap applied when R̄ is read, not when it is stored: the recovery is off by up to 0.875 | closed forms 1.2632, 1.5000, 2.2500 [p6.fixed_point]; event form equals the stand-in's average on hourly events [p6.trajectory, within 1.1e-5]; pin 3.6000 = p × a_cap 12 [p6.pinned]; with a_cap 20, p × a_cap = 6 > cap, so the pin is cap 4 by the clip; [p6.burst], [p6.recovery] |

P2's cap, decay closed form, replay and P0 run inside p6, n4 and n6 (every fixture's strands replay; every fixture has a
copy without the block).

### P3 dispositions (mechanism)

| bar | pass value | how measured | null it must reject | competent null (nulls.py) | reachability check |
|---|---|---|---|---|---|
| o1 E settles at the expectile | \|mean over individuals 0-4 of sign(o) × (Ē − τ_o b − (1 − τ_o) a)\| ≤ 0.01, Ē = mean E over wins 10,001-20,000; and on the two-point-gaps stream every individual's E equals the closed form (decay 0.5^(h/48), then the asymmetric step) at every event within 1e-5 | optimism.yaml, two-point stream at a fixed clock (`elapsed_hours` 0); two-point-gaps stream (gaps 0.5-3 h) | a gene-blind E (one rate): misses by tanh(\|o\|)/2 per individual, sign-aligned so it cannot cancel; individuals with \|o\| < 0.02 are excluded | the asymmetric step without E's decay: exact at elapsed 0 (passes v1), off by 0.175 on varied gaps | the stationary mean is the expectile exactly (the drift is linear in E on (a, b)) [o1.stationary]; stand-in 1.1613 vs 1.1667 [o1.stand_in]; per-individual window noise is about 0.005, so only the pooled error is gated |
| o2 the gene's sign is readable | sign(E_z − E_twin0) = sign(z) for 100 % of individuals 0-99; and each individual's E (and its spread-0 twin's) over wins 1-3,000 equals the closed form at every event within 1e-5, wins above and below E alike | optimism.yaml vs its spread-0 copy on one two-point stream, E averaged over wins 2,001-3,000 | spread 0: sign 0; a shuffled gene: about 50 % | one shared E plus 0.01 × sign(z): the sign is readable (passes v1), the trajectory is off by 0.285 | coupling: E_z ≥ E_0 at every step when z > 0 [o2.coupling, 100/100 at s 0.1 and 0.3]; stand-in 100/100 |
| o3 twin distance rises with outcomes | mean §5.4 distance over 20 seed pairs at spread 0.3 ≥ 0.001; the distance is 0 at every event up to and including each pair's earliest win; the released `pursue` goal differs between the twins at ≥ 1 event in ≥ 10 of 20 pairs | pairs stream, every other spread 0 | spread 0: distance exactly 0; a seed-dependent 1e-6 on one odds | a static per-seed interest bias (0.05 z nats): distance 0.0059 but 0.0166 before any outcome | before the earliest win both twins have E 0, so every field matches; then different z give different E (o2), E sets d, d sets afterglow, afterglow enters `pursue` (glow ≠ 0 in the fixture); the gene in Python with stand-in draws (nulls.py reference): 0.0032, 0 before the earliest win, 19 of 20 pairs |
| o4 0 habit breaks | 0; ≥ 50 bound stances | pairs stream | the habit-free twin binds | a pursue picker that forgets the busy rule: pursues `task` on busy events; with the rule written in it passes (a property bar, as n5) | as n5 |
| o5 P0 | spread 0 gives today's bytes | p0.py over the copies | a planted change | spread 0 storing `optimism: 0`: the state bytes differ | spread 0 stores no gene |

### P4 valence and the omission let-down (simulation; omission = mechanism)

| bar | pass value | how measured | null it must reject | competent null (nulls.py) | reachability check |
|---|---|---|---|---|---|
| v1 expectations shape valence | two lives with the same final win m 1 after a 24 h gap, one with E built by 20 earlier wins, one with E 0: final valence accumulators differ; their push ratio (built / zero) equals 1 − E × 0.5^(24/48) within 1e-4; the mood's odds of `up` are lower in the life with E built | valence.yaml | outcome-sized valence (evidence = m): identical | valence = reward minus the running mean of past rewards: the accumulators differ (passes v1), the push ratio is 0 | the 24 h gap rounds the mood accumulators to 0 (half-life 1 h) while E keeps 0.5^(24/48); both lives' previous stance pursued `other` (E 0), so the pushes are (1 − k) × w_p × d and their ratio is d_A / d_B = 0.292894 [v1.ratio] |
| v2 an expected win moves valence | a win with d 0 moves the valence accumulator beyond its inertia carry by an amount > 0; that move after 10 wins / after 3 wins equals E10 / E3 within 1e-4 (the evidence is w_e × E); the no-block twin's move is 0 and its odds of `up` are lower on the same events | valence.yaml: a win equal to E after 10 and after 3 wins; valence-noblock.yaml | today's `effects.surprise`: 0 | a fixed valence bonus on every win: move ratio 1 (want 1.141742) | the evidence term w_e × E is in the formula; E10 0.999024, E3 0.875 [v2.linearity] |
| v3 forgetting | push left j events after an impulse / the impulse = 0.61^j within 1e-4 for j = 1 … 5, so ≤ 0.09 at j = 5 | valence.yaml at inertia 0.61 | inertia 0.9: 0.59; an accumulator erased after the impulse: 0 | inertia 0.5: 0.031 at j = 5 (passes v1), 0.5 at j = 1 | closed forms 0.61, 0.3721, 0.2270, 0.1385, 0.0845 [v3.five_back, v3.lags] |
| om1 omission closed form | (E, d) at every event equal the closed form within 1e-6 in three cases after a stance that pursued `task` (window 2 h): an event at 1 h then one at 2 h (fires at expiry, not before), an event at 3 h then another (fires once), a win at 1 h then an event at 3 h (never fires) | valence.yaml: three lives, 10 wins, the pursuing stance, then the case | today's format (no window): E unchanged | a deadline timer that ignores wins inside the window: off by 0.856 | the rule's arithmetic [om1.closed_form, om1.cases, om1.fires] |

### P6 `rest` (part 2; simulation of S, mechanism of the downscale)

| bar | pass value | how measured | null it must reject | competent null (nulls.py) | reachability check |
|---|---|---|---|---|---|
| r0 order kept, weights contracted | at every rest line the learned weights equal round(ρΔ, 6) with \|·\| < prune set to 0 (within 1e-6); 0 strict order inversions; the top level unchanged; no strict order lost where a weight survives the prune; and 20 short lives (29 × 0.25 h active, S 0.329 < θ) get no rest line and no change at the gap | reversal stream, every rest line; reversal seeds 0-19 cut to 30 events, then the gap | Δ reset to 0 at a rest line: every strict order lost; rest as identity | halving the weights at every idle gap, whatever the pressure: the short controls change by 0.5 | x → ρx is increasing [r0.inversions, 0 in 40,000]; S 0.556 after the reversal's 14.75 active hours, 0.329 after 7.25 [r0.pressure]; stand-in 200/200 |
| r1 plasticity returns, the lesson stays | median re-learning turns with rest ≤ 0.5 × without, at ρ 0.5; and phase A's lesson (`mid` strictly on top) holds at the stance right after the gap in ≥ 90 % of lives | the reversal stream's draws without its lock turns (rule 2: r4's lock habit also delays re-learning; the locked ratio is reported), seeds 0-199 | ρ 1: ratio 1 | a full reset of the learned weights at the gap: ratio 0.182 (passes v1), the lesson kept in 0 of 200 | stand-in 11 → 5 (ratio 0.4545) [r1.relearn]; ρ 0.75 gives 8 and is outside the bar; the RFC's rest in Python keeps the lesson in 200 of 200 (nulls.py reference) |
| r2 lineage drift | each generation closes with one idle event (its rest line comes before absorption); probe distance without rest ≥ 0.1 after generation 1; with rest ≤ 0.8 × without after generations 1 and 2; with rest ≤ without after 10 | lineage stream | RFC 0001's absorbing lineage without rest: 0.460; rest as identity: equal distances | pruning small weights only (no downscale): ratio 1.0 | stand-in [r2.stand_in]: 0.243 vs 0.454 (0.54) after 1, 0.454 vs 0.631 (0.72) after 2, 0.660 vs 0.666 after 10 (the distance saturates by generation 10) |
| r3 replay | 0 differing bytes, rest lines included | rest strands | a strand with a rest line removed: verify exit 1 | as n4 | rest lines are logged |
| r4 0 habit breaks under conflict | 0 breaks; ≥ 50 bound stances; ≥ 50 binding conflicts (lock events whose learned weights put `mid` or `high` above `low`); the habit-free twin breaks at least once | reversal stream | as n5 | the lock as a host-side filter (the stance still releases the learned level): 2,437 conflicts released against the lock | the Python learner on the frozen draws exercises 2,437 conflicts |
| r5 P0 | byte-identical without the block | as n6 | as n6 | as n6 | as n6 |

### P7 `load` (part 2; simulation)

| bar | pass value | how measured | null it must reject | competent null (nulls.py) | reachability check |
|---|---|---|---|---|---|
| l1 three patterns, each by its own channel | three pairwise distinct L trajectories (differ by > 1e-6 at ≥ 1 event); criticism: L equals the closed form at every event within 1e-5; hold: every increment (L_t − L_(t−1) × 0.5^(h/72)) is 0 or κ_h × h, 40-50 of them, the last before event 56; failure: every increment > 0; and with a load effect on caution (load-effects.yaml), P(careful) at the peak of L above its start, for the hold and failure patterns | patterns streams; the hold and failure patterns again on load-effects.yaml | block absent: all three 0 | an aversive-event counter (κ(w − θ) per criticism or failure, held moods ignored): distinct (passes v1), the hold channel never adds | each pattern adds through a different term (hits, non-habituating evidence, the hold term) [l1.criticism] |
| l2 recovery | L after the 4 criticisms = 1.0 within 1e-6 (the induction, checked by the closed form); and L after quiet gaps = L0 × 0.5^(Σh/hl) within 1e-5 | load.yaml, a load then quiet events | a clock-free load: unchanged; a load never induced | +1 per criticism: the decay is right (passes v1), the induction is 4.0 | [l2.induction]; identity [l2.recovery] |
| l3 cap | L ≤ L_cap at every event; the criticism pattern equals the closed form up to the hit at which the cap starts to bind (hit 40) and repeated hits pin at the cap; then quiet gaps of 1, 5, 12, 24 h give 4 × 0.5^(Σh/72) within 1e-5 | patterns stream 1, then 4 quiet events | an uncapped L: steady state 4.454 > 4 | a saturating counter without decay: within the cap (passes v1), pre-cap growth off by 1.313 | steady state κ(a − θ)/(1 − 0.5^(6/72)) above the cap, so the clip binds [l1.repeated_hits]; [l1.criticism], [l3.recovery] |
| l4 replay | 0 differing bytes | load strands | as n4 | as n4 | as n4 |
| l5 P0 | byte-identical without the block | as n6 | as n6 | as n6 | as n6 |

The fourth pattern (an inadequate response with compensatory over-activity) needs two interacting channels and has no
bar.

## Bars v2 (repaired before any engine measurement)

An independent review graded the 40 bars of `fixtures/bars-v1.md` (12 sound, 10 weak, 18 vacuous) by naming, for each, a
degenerate policy or null that passes it; the 14 reviewed by running the policy all held. No engine with an RFC 0002 block
had produced any number for these bars, so they were repaired under rule 1 (a bar is never edited after a number is seen).
Three kinds of repair, as the review ranked them: (a) positive activity and two-sided correctness (exact or bounded
magnitudes, coverage, retention); (b) every advertised null executable and failing inside its own fixture; (c) a causal
effect on a decision where a bar checked only a number. Every bar also gained a competent plain-Python null (`nulls.py`,
the column above); `nulls.py` exits 0 with every one rejected. No pass value was relaxed. Per bar (verdict: the review's):

| bar | verdict | what changed | why |
|---|---|---|---|
| n1 | vacuous | exact wins on a 9-cell grid and the post-event deficit, beside the ratio | win = amount × deficit gave ratio 4 with the wrong arithmetic |
| n2 | vacuous | exact signed rewards on 5 overshooting cells and a zero cell | win = −amount passed the sign test |
| n3 | weak | a second probe with success rates set apart (0.5904, 0.2), odds magnitudes, released goals | every probe had s = 1, so pursuit that ignores s passed |
| n7 | weak | the `env` control through the same transports as the refused sources; MCP refusal tested apart on both tools | the row's control went through MCP, which refuses every amount |
| n8 | weak | the ceiling on the absent stream too; answered reports must pay | a 10-line threshold rule and a quota scheduler passed |
| n9 | vacuous | wins ≤ 0 (not < 0) count; coverage of both goals per seed; a context at-cap bound | never pursuing passed |
| n10 | vacuous | a positive τ 0 baseline, deficits recomputed from the logged inputs, τ 2 service coverage | two arms with no growth compared 0 with 0 |
| n11 | vacuous | contact before the absence and after the return (new stream `return`); the window-1e6 twin gated | permanently disabled contact passed |
| s1 | weak | constant-gain twins on the engine: the gate must need strictly fewer re-learning turns than any of them within its movement budget | a constant gain was an untested stronger control |
| s2 | vacuous | phase A learned and phase B re-learned in ≥ 90 % of lives | frozen learning moved 0 and passed |
| s3 | vacuous | every life's opening paid step against the closed form, (v, r, g) included; the cap read before recentring | zero steps respected every cap; and v1's script compared each recentred change with the cap, which a correct step exceeds by up to a third |
| p1b | vacuous | an accepted-reward control in every fuzzed stream | a constant pace passed |
| p1c | vacuous | equal-average steady / bursty / spaced streams (new stream `bursts`) | a current-reward pace can rank with block rates |
| p6 | weak | an admissible burst (12 zero-hour wins of 4) and the recovery (new stream `burst`) | with payments clipped at 4 the uncapped null could not pass 4 |
| o1 | weak | varied gaps (new stream `two-point-gaps`), each individual's E exact at every event | with elapsed 0, E's decay never acted |
| o2 | vacuous | each individual's whole E trajectory, both directions | E_z = E_0 + 0.01 sign(z) passed |
| o3 | vacuous | a distance floor, zero distance before outcomes, changed released goals | a 1e-6 odds offset passed |
| v1 | vacuous | the push ratio against the closed form, the mood's odds | any history offset passed "differ" |
| v2 | weak | the move proportional to E, the released odds against the no-block twin | E could be stored and left out of the mood |
| v3 | vacuous | the residual at lags 1-5, two-sided | erasing the accumulator passed ≤ 0.09 |
| om1 | weak | before, at and past expiry, a win inside the window, a repeat | firing at the next event after a pursuit passed |
| r0 | vacuous | exact contraction, strict distinctions, a short-activity control below threshold | rest as identity passed |
| r1 | weak | the phase-A lesson kept after the gap; measured on the reversal draws without lock turns | a full reset re-learns fast and forgets everything; and the lock turns r4 needs also delay re-learning (rule 2): with them a faithful learner gives 6 / 11 = 0.545, without them 5 / 11, the research stand-in's number |
| r2 | vacuous | a closing idle event per generation, a positive baseline, ≥ 20 % reduction after generations 1 and 2 | rest as identity passed "≤"; at generation 10 the distance saturates |
| r4 | weak | ≥ 50 bound stances and ≥ 50 binding conflicts; the habit-free twin gated | the bar never required a conflict |
| l1 | vacuous | each channel's amplitude and time course, and a decision the load reaches (variant `load-effects`) | three arbitrary increments passed |
| l2 | vacuous | the induction's value | a load never induced passed |
| l3 | vacuous | pre-cap growth up to the cap, and the recovery after it | jumping to the cap passed |

Kept as v1 (the review found them sound): n4, n5, n6, s4, s5, p1a, o4, o5, r3, r5, l4, l5. Their pass values and
measurements are unchanged; they gained the competent-null column, and the scripts now gate the null their rows already
named (replay: a changed input, a removed line and two swapped lines refused; habits: the habit-free twin breaks at least
once). `pending engine`: none (every bar rejects its competent null in `nulls.py`). `nulls.py` also runs the RFC's own
rules in Python as references (they never gate): they pass every bar they cover except n10 (0.798 → 0.569, not halved) and
n11 (2 of 100 seeds pursue once on the second absent day). Those two pass values are kept as frozen; if the engine
matches its stand-in, they fail, and under rule 1 that is a finding about the design, not a reason to move the bar.

## What this benchmark cannot see

- Whether a reading, an amount or a reward is true: a forged env label passes every bar.
- Whether a host acts on the stance or the pace level, or a model writes in it.
- Natural lives: every life here comes from a seeded generator, and sequential turns of one life are correlated.
- Other personas, longer lives, other hardware (bytes match wherever the floating-point maths agrees, persona doc §5.3).
- Whether the block is necessary: a host can rewrite a mechanism in its own code (n9 and n11 reject the threshold rules a
  harness writes by default, not a host that reimplements the success-weighted pursuit); the bars show the engine does what the
  RFC says, not that nothing else can.
- Whether any of this is like anything from the inside. No bar here can see it, and none claims it.

## How the scripts express the bars before the engine exists

The scripts were written against the RFC text before any engine had the blocks; where the spec above leaves a choice, the
choice is fixed here and frozen with the rest:

- **Scripted hosts.** fortnight, absent, return, flip, reversal and lineage depend on the stance the host reads (a report
  is sent when the stance pursues `report`), so their files hold the host's seeded draws; the scripts build the events from
  them, turn by turn, over one `live` process per life.
- **Two goals.** A drives block needs 2-7 goals, so every "one goal" fixture declares a second goal no event pays.
- **needs1** grows its deficit at `per_hour` 0.1, so 8 h and 2 h reach 0.8 and 0.2 (n1). The stance carries no win, so n1
  reads the win through E (rate 0.5 from E 0) and n2 through wanting (a setback of weight 0.5). n9 runs on the fortnight at
  τ 0 (needs2), as its row says; the win of each satisfaction is recomputed in Python from the stance-reported deficits.
- **n7** (v2) sends the refused sources and the `env` control through `persona turn` and through `live`; MCP's refusal of
  any amount is checked apart, on `probbit_persona_turn` and `probbit_live_event`.
- **n8, n9, n10, n11** share `needs.fortnight_metrics` and the `gate_*` functions with `nulls.py`, whose needs world is the
  same scripted host with RFC 0002 A.3's deficits.
- **s1** counts turns into phase B until the top level is the praised one, censored at T; s2 compares the mean movement
  per (T, noise). The constant-gain twins are the gate's step with g fixed: rate and step_cap times g, total_cap unchanged.
- **p1b** varies offered stakes (cues, deadlines, progress, novelty on both goals) over seeds 0-9 of a perturbed copy of
  the blocks stream; `fuzz` checks rules, not pace values. p1c correlates per event. **p6**: a new individual starts at
  R̄ 0, so three zero-hour wins bring it to r0 + 0.2 and the closed form starts from the engine's R̄; with a_cap 20 the
  host clips each payment at win_max 4, a win above it being an error (docs/persona.md section 2.9).
- **o3**'s distance (section 5.4) includes the `pursue` odds, where the gene acts through afterglow.
- **v1, v2, v3** read the mood accumulator a_t = k d a_(t−1) + (1 − k) e_t (persona doc section 3, step 3) as ratios, so
  its scale cancels; v2 reads the valence evidence as the accumulator's move beyond the inertia carry at elapsed 0; v3
  keeps the stance on the unpaid goal (habit `go_other`), so no E term follows the impulse.
- **r0** finds the rest line as a strand line with the top-level key `rest` (the RFC names the line, not its shape); the
  gap event carries no feedback, so the last phase-A decision is unpaid. **r2**: RFC 0001's lineage block does not exist
  yet, so the host absorbs each generation's learned weights into the next prior (`logw`); the distance is between the
  resting odds of generation 0 and generations 1, 2 and 10.
- **`drive: {"n": 4, "m": 3}`** is quoted in needs1 and needs2: the persona reader refuses a bare `n` key (YAML 1.1
  reads it as false), so the RFC's schema example needs the same quoting, or the build renames the key.
- **load.yaml**'s weights (1 each) and hold (above 1, 0.01 an hour, on the two-level mood `strain`) are test values;
  load-effects.yaml adds `effects: {caution: 1}`.
- Field names (`needs.<id>.deficit`, `needs.<id>.term`, `needs.success.<goal>`, `pace.rbar`, `load.l`, `drives.expect`,
  `drives.surprise`, `surprise.<trait>.{v, r, g}`, `dispositions.optimism`, `mood.valence.odds`) follow the RFC. When the
  engine names a field differently, the script is fixed and the change is logged below; a pass value never moves.
- **Choices the RFC leaves open**, fixed here: the rest pressure S starts at 0 for a new individual (r0's short control and
  every rest bar assume it); within one event every need grows before any satisfaction is paid (the recomputed wins of n8
  and n9 assume it; their signs do not depend on it).
- **r1** plays the reversal draws twice: with the lock turns (r0 and r4 read those lives) and without them (r1's ratio
  and retention), so the habit r4 measures does not slow the re-learning r1 measures (rule 2).
- **s3** reads the cap where the RFC puts it, on the step before recentring (docs/persona.md section 2.8): the recentred
  changes of a learned trait come from a step within ±g_max × step_cap exactly when their range is ≤ 2 × g_max × step_cap.

## Changes after the freeze

- **Bars v2** (this commit): 28 bars repaired before any engine measurement, `nulls.py` added, four streams and seven
  variants added (every v1 stream and variant regenerates byte for byte), `bars.json` and `bars.md` rewritten with a
  version marker, the v1 copies kept as `bars-v1.json` and `bars-v1.md`, MANIFEST regenerated. Section "Bars v2" lists
  every change; no pass value was relaxed.
- **Bars v2, r1's lives and s3's cap** (before any engine measurement), found by the same dry run, against the `arith_0002.py`
  learner played through `rest.py` and `gate.py`: r1 is measured on the reversal draws without the lock turns (with them
  the learner re-learns in 6 turns after rest instead of 5, 6 / 11 = 0.545 against the pass value 0.5, because the lock
  habit of r4 shares the stream: rule 2; without them 5 / 11 = 0.4545, the number the bar's reachability check cites; the
  locked ratio is reported); s3 checks the cap on the step before recentring, as the RFC states it (v1's script compared
  each recentred change with g_max × step_cap, which a correct gated step exceeds in 968 of 1,800 lives of the dry run);
  v2's no-block twin is gated at a move within 2e-6 of 0, not exactly 0 (its six-decimal accumulator leaves a residue of
  1e-6). Pass values and bar text unchanged.
- **Bars v2, n7's script** (before any engine measurement): the `persona turn` control moved from 1 h to 5 h of growth. At
  1 h the deficit is 0.1 and the amount 0.2 overshoots it back to 0.1, so a correct engine shows no change and would have
  failed the bar. Found by a dry run of `needs.py` against a Python model of RFC 0002 A.3-A.4 (not the engine); the bar's
  pass value and text are unchanged.
- **Wave 1a, n7's MCP control** (2026-10-16, on the engine's real bytes): `needs.py` sent the persona to both MCP tools as
  YAML text in `persona`, and the tools take a persona document (JSON) or `persona_path`, so both refused it as "must be an
  object" before reading the event: the 2/2 was vacuous (an engine without the MCP refusal passed it). The script now sends
  `persona_path`; on the wave 1a engine both tools refuse at `arguments.inputs.needs` and `arguments.event.needs`, and the
  same event without the amount is accepted. MANIFEST regenerated for `needs.py` only; pass values and bar text unchanged.
