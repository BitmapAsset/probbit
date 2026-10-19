# RFC 0002: The neuromodulators and the body

Status: draft for review · Target: probbit 0.10.0 (part 1), a follow-on release (part 2) · Changes: docs/persona.md (format
1, additive) · Depends on: RFC 0001 (senses, action-addressed feedback)

**Changes after the freeze** (2026-10-16; the text is not a bar, no pass value moved):

1. A.3 runs in two passes per event: every need grows in declared order, then each satisfaction pays in declared order. The
   frozen draft said "per need in declared order", which left the order of growth and payment across needs open; the bench
   scripts had to pick one. The deficits after an event do not depend on the order, and sign(r) never does.
2. Part 2: `pressure.s` starts at 0 and `pressure.pending` at false when the individual is made (`persona init`).
3. A.1's example quotes `"n"`: the YAML reader refuses a bare `n` key (YAML 1.1 reads it as false).
4. Found by the build (wave 1a): `r_cap` above `drives.expectation.win_max` is refused with N-R5 (a need's win joins its
   goal's wins, which are at most `win_max`), and one need satisfied twice in one event is refused with N-E1. `drv_expect`
   reads the same E the turn's surprise is measured against (after the event's decay, before its win).
5. Found by the build (wave 1b): P-R3 also refuses a habit condition on `pace_up` or `pace_down`, and an event that sends either
   is refused at `inputs.<id>` (the engine sets both from R̄, as N-E4 does for the needs' inputs). The stance's `surprise` map is
   `{}` once every gate is closed; precision weighting reads each sense's variance after the event's sense step; the pace clause
   prints R̄ to 2 decimals.

## Summary

1. Six optional blocks or keys in part 1 (`needs`, `goals[].expect_by_hours`, `surprise`, `pace`, `dispositions`,
   `drives.valence` with `afterglow.liking`) and two in part 2 (`rest`, `load`). Without them every document is byte for
   byte 0.9's.
2. **Mechanism:** `needs` (a deficit over a setpoint; its reduction is the win); the omission let-down, which `needs`
   depends on (an expected win that does not come lowers the expectation); `surprise` (a volatility-gated step size);
   `pace` (average reward sets a pace level); `dispositions` (optimism, patience and volatility genes).
3. **Simulation:** `drives.valence` and the liking gain; part 2's rest pressure and `load`. The downscale at a rest line
   is a **mechanism**.
4. Every new number is capped, rounded to 6 decimals and recomputed by `live verify`. No block touches `total_cap` or
   `drift_cap`, adds a solved variable, or runs between events.

## Motivation

Three measured facts (our map of the format against 52 published mechanisms, and small stand-in simulations, not the
engine):

- **Wins are sized by the host.** Of 52 mechanisms the literature names for reward, uncertainty and regulation, the format
  has 6, a part of 18 and none of 28. The largest missing piece is regulation over setpoints: no variable has a deficit
  that grows with use or time and whose reduction is the reward (Sterling 2012; Keramati & Gutkin 2014).
- **An omitted win teaches nothing.** In §2.9 a goal without a win has prediction error 0, so a goal pursued and never paid
  keeps its expectation. Dopamine neurons are depressed when reward is less than predicted (Schultz 2016), and an omitted
  expected reward invigorates the next response (Amsel 1979). On a stand-in with the principal absent for five days, pursuit
  weighted by a fixed answer probability reached 15 contact pursuits a day; weighted by a learned one, 3, 0, 0, 0, 0.
- **How the drive enters pursuit decides whether a need becomes a nuisance.** On a stand-in fortnight, pursuing
  by the marginal drive ∂D/∂x made 9.5 contact pursuits per waking day and paid negative wins; pursuing by the expected
  drive reduction made 1.13 at the same weight and none negative at horizon 0. A 2 h forecast cut the hours a deficit sat at
  its cap from 0.39 to 0.04 a day with no overshoot; an 8 h one overshot 0.85 times a day.

State held outside the model cannot drift with the context: an agent holding its needs in the prompt turned numbers into
words because models "struggle with interpreting numerical values" (Wang et al. 2024), and every evaluated agent drifted
from a system-prompt goal under long contexts (Arike et al. 2025).

## Non-goals

Idle actions (nothing runs between events: growth and decay use the next event's stamped hours); an exploration policy or
information bonus (RFC 0001 non-goal; Aston-Jones & Cohen 2005; Wilson et al. 2014); a time-of-day term (Murray et al.
2009); any claim about inner states, which no test can see. **Waits**, each for its own RFC with its own bar:
controllability from addressed outcomes (Maier & Seligman 2016; the success weighting of `needs` is its simplest form); the
opponent after-state and relief (Solomon & Corbit 1974; Seymour et al. 2005); per-source reliability and partner preference
(Behrens et al. 2008; Young & Wang 2004: either must never discount the principal's corrections); mood biasing how outcomes
are valued (Eldar & Niv 2015: mood and value oscillate at f ≥ 1.2); replay for credit (Mattar & Daw 2018; Schaul et al.
2016: replaying the same feedback double-counts it without a model to replay into); sensitization (no primary arithmetic
fetched). **Rejected:** spontaneous recovery, renewal and context-tagged extinction of corrected behaviour (Rankin et al.
2009; Bouton 2004). The clean forgetting of a corrected stance is a safety property and stays.

## Design A: needs and the omission let-down (mechanism)

Reward is the reduction of distance to setpoints, D(H) = (Σ|h* − h|ⁿ)^(1/m) with n > m > 1, computed from the sensed outcome
(Keramati & Gutkin 2014); regulation anticipates need (Sterling 2012); contact and information behave as needs (Tomova et
al. 2020; Matthews et al. 2019; Kidd & Hayden 2015).

### A.1 Schema

```yaml
needs:                              # optional; needs a drives block; 1-8 items
  drive: {"n": 4, "m": 3}           # required, n > m > 1 (the stand-in's values; "n" quoted, YAML 1.1 reads a bare n as false)
  pursue: {want: 2, cap: 3}         # the pursue term: weight, and |term| <= cap nats (the stand-in's values)
  success: {rate: 0.2, init: 1}     # the bound goal's success rate s: its step per win or omission (the stand-in's), its start
  r_cap: 1                          # required: |win| <= r_cap
  items:
    - id: context                   # at most 32 characters; reserves the input id need_context
      say: context budget           # default: the id, _ as spaces
      unit: token_share             # 1-24 characters, in the digest
      cap: 1                        # the deficit stays in [0, cap]
      weight: 1                     # w > 0
      per_hour: 0                   # growth per stamped hour (>= 0)
      per_reading: {sense: tokens, rate: 1}   # optional: growth per unit of a declared number sense's raw reading
      satisfy_from: [env]           # required: env and/or human
      amount: 0.6                   # K: the typical satisfying amount, 0 < K <= cap (used by the forecast)
      bind: compact                 # the goal the reduction pays; one need per goal
      horizon_hours: 0              # τ, 0-72 (default 0)
      forecast_per_hour: 0          # the growth per hour the forecast assumes (default per_hour; >= 0)
      weight_spread: 0              # gene: multiplier exp(N(0, s²)) on weight; 0..3, default 0
      growth_spread: 0              # gene: multiplier exp(N(0, s²)) on per_hour and per_reading.rate; default 0
drives:
  goals:
    - id: compact
      expect_by_hours: 1            # new, optional, > 0 hours: the omission window (required on a goal a need binds)
```

Why these defaults. n = 4, m = 3 give deprivation scaling: the same amount 0.2 pays 2.02 times as much at deficit 0.8 as at
0.2, and −0.117 at deficit 0 (an overshoot is a setback). `horizon_hours` 0: anticipation is opt-in, because a forecast past
what growth allows overshoots. `per_reading` reads `raw`, never the normal (RFC 0001 rule S5). There is no half-life on a
deficit: regulation's "stick" does not adapt (Sterling 2012); the win flows into afterglow, the brief "carrot".

### A.2 Events

```json
{"needs": {"context": {"amount": 0.6, "src": "env:compactor"}}, "src": "env:host"}
```

A bare number takes the event's `src`. `needs` is read, not compiled, and comes back in the stance's `inputs`. Without the
block, `needs` is an undeclared input in `ignored`, as today.

### A.3 The needs step

After the sense step and before the drive step, in two passes (genes g_w, g_gr; h = `elapsed_hours`; every line rounded to
6 decimals):

```
pass 1, every need in declared order:
x     = clip(x + g_gr × (per_hour × h + per_reading.rate × reading), 0, cap)   # reading = the sense's raw value this event
D(x)  = (Σ_i w_i g_w,i × x_i^n)^(1/m)                                      # over all needs
pass 2, every need satisfied this event (need j, amount a), in declared order:
r     = clip(D(x) − D(x with x_j ← |x_j − a|), −r_cap, +r_cap)
x_j   = |x_j − a|, then clip to [0, cap]
```

r > 0 becomes a `win` of size r for the bound goal, r < 0 a `setback` of min(−r, 1); both join the event's goal signals,
so the drive step treats them as any win or setback. **The pursue term** (section 3, step 2b) for the bound goal g:

```
x̂     = x + g_gr × forecast_per_hour × τ                                 # the forecast: a declared growth rate (0.10.1: not clipped at cap)
term  = clip(want × s_g × (D(x̂) − D(x̂ with x̂_j ← |x̂_j − K|)), −cap, +cap)
s_g   ← s_g + success.rate × (1 − s_g) on a win of g, and k_g ← 0
s_g   ← s_g × max(1 − k_g × success.rate, 0) when g's window fires, k_g ← k_g + 1 first   # 0.10.1: the k-th omission in a run (A.4)
```

Amended in 0.10.1 (R126; bench/body n10 and n11 failed on 0.10.0 as design findings, BENCHMARKS.md §12). The forecast was
`min(x + …, cap)`: at the cap x̂ = x, so a horizon added nothing once the deficit had reached it, and with two needs at their
caps the compact term (2 × (D(1, 1) − D(0.4, 1)) = 0.503) stayed under the interest gap ln(1/0.6) = 0.511 (the nulls.py
reference: every at-cap hour it kept on `work` had both deficits at 1 and s = 1). The deficit stays clipped; only the forecast
reads the growth the horizon brings. And an omission lowered s by a constant factor, so two omissions on the first away day
left s = 0.485, enough to pursue once more the next morning (2 of 100 lives, n11). Now the k-th omission of a run multiplies
s by 1 − k × rate: a run of 1/rate unanswered pursuits extinguishes pursuit, and the same life's third pursuit sees s = 0.364
(a geometric run, (1 − rate)^k, left 0.388 and a field margin of 0.004 on the engine: R126 cycle B-E1, not kept). The run k_g is state
(`needs.misses: {goal: k}`, only for k > 0; a state is refused when a run is not a whole number in [1, 10⁶]).

It is lowered as the number input `need_<id>` (box [−cap, +cap]) with effect `{pursue: {<bind>: 1}}`. Pursuit is
weighted by the goal's learned success, so unanswered pursuits extinguish themselves through the omission let-down (A.4).
The weight is a success rate s ∈ [0, 1], not §2.9's E: E is a win size and starts at 0, so weighting by it leaves a need
unpursued until its goal has paid by another path. At τ = 0 the term is positive exactly when x > K/2, the deficits at
which the satisfaction pays a positive win. Rules may read `need.<id>.deficit` as a number condition (lowered like goal
conditions), so a must-do habit can fire at a deficit.

State: `needs: {genes: {weight: {id: g}, growth: {id: g}}, deficit: {id: x}, success: {goal: s}}`, each deficit 0 and each
s at `success.init` when the individual is made. Genes:
`round(exp(weight_spread × z(name, seed, "need", id, "weight")), 4)` and the same with `"growth"`; spread 0 means
multiplier 1 and no stored gene.

### A.4 The omission let-down

Omission of an expected reward is a negative prediction error (Schultz 2016) that invigorates the next response (Amsel
1979). A goal with `expect_by_hours` keeps a window, `drives.window: {goal: hours | null}`:

- **opens** after a released stance pursues the goal, if not already open, at `expect_by_hours`;
- **counts down** by h in the drive step's decay, like a deadline;
- **closes** on a win of the goal (m > 0);
- **fires** at an event where it reaches 0 without a win: `d = −min(E, pe_cap)`, `E ← E + α⁻ × (0 − E)` (α⁻ = `rate`, or
  the optimism gene's rate, Design D), once; the window closes and `why` says `an expected win did not come`.

The omitted d joins the turn's summed surprise (so `effects.surprise` and `drv_letdown` act on it) and the optional
`drives.effects.omission` map, lowered as the number input `drv_omit` = min(Σ omitted E, pe_cap) (default: no effects).
It does **not** join the learning sign (question 1), and it lowers the bound need's success rate (A.3), by the factor 1 − k × rate
(at least 0) at the k-th omission in a run without a win (0.10.1).

### A.5 Refusals

Refused when the persona is read (exit 2, at the field's path):

| row | refused |
|---|---|
| N-R1 | `drive` without n > m > 1; a needs block without a drives block; an empty block or more than 8 items; two needs with one id; a taken `need_<id>` id |
| N-R2 | `bind` naming an unknown goal, or two needs bound to one goal |
| N-R3 | `satisfy_from` missing, empty, or naming `self` or `clock` |
| N-R4 | `per_reading` naming an undeclared or flag sense; a negative `per_hour` or `rate` |
| N-R5 | `cap`, `weight`, `amount`, `want`, `pursue.cap` or `r_cap` ≤ 0; `amount` > `cap`; `horizon_hours` outside [0, 72]; `success.rate` outside (0, 1] or `success.init` outside [0, 1]; a number with more than 6 decimals |
| N-R6 | a need whose bound goal has no `expect_by_hours` (unanswered pursuits would never extinguish); `expect_by_hours` ≤ 0 |

An event is refused whole (one `{"error"}` at its path, no turn, no state change):

| row | refused | path |
|---|---|---|
| N-E1 | an amount for an undeclared need, not a number > 0, or above the need's `cap` | `inputs.needs.<id>` |
| N-E2 | an amount whose source kind is not in `satisfy_from`: missing, malformed, `self`, `clock` | `inputs.needs.<id>.src` |
| N-E3 | an amount and its bound goal's `win` in one event (one outcome, one payment) | `inputs.needs.<id>` |
| N-E4 | a `need_*` or `drv_omit` input, or a deficit or window sent as an input (the engine owns them) | `inputs.<key>` |

A state is refused, even with a recomputed digest, if a deficit leaves [0, cap], a success rate leaves [0, 1], a window
leaves [0, `expect_by_hours`] or null, the need set differs from the persona's, or a gene misses its seed.

### A.6 The stance and `why`

The stance gains `needs: {id: {deficit, forecast, term}}` and, per goal with a window, `window` (hours left or null).
`why` gains at most one need clause, for the pursued goal when a need binds it and its term is the largest positive one:
`context budget deficit 0.8, above its setpoint`; and the omission clause when a window fired this event.

**Public:** "A need is a number with a setpoint: use or time grows its deficit, and an event that reduces it pays a win
sized by how much it reduced the drive. It changes which goal the stance pursues." For the omission: "Valence moves with
expected value and with outcomes better or worse than expected, including expected wins that do not come."

## Design B: surprise (mechanism)

Unexpected uncertainty raises learning (Yu & Dayan 2005; Behrens et al. 2007); a reduced Bayesian delta rule makes the rate
a function of change-point evidence and run length (Nassar et al. 2010); a slow volatility average needs a run of surprises,
which a real change produces and noise rarely does (Behrens et al. 2007's shape). The stand-in showed it: a one-step z gate
drifted 2.98× in a stable phase, no setting of a Bayesian-hazard gate met both halves of the bar, and the slow average met
both on held-out seeds.

```yaml
surprise:                     # optional; needs a learning block
  volatility_rate: 0.3        # a: the volatility average's weight per feedback (half-life 1.943 events)
  v0: 0.45                    # the volatility at which the gate opens, 0 <= v0 < 1
  r_ref: 4                    # the run length at which the gain is 1
  r_max: 20                   # cap on the run length
  g_max: 3                    # 1 < g_max <= 3
  precision: false            # with senses: each sense's effect gain × (1 − var / var_cap)
```

The defaults are the values the stand-in passed with. Inside the credit step (B.2 of RFC 0001), for each learned trait
when the paid stance's taken level l* has credit > 0 and sign ≠ 0 (state per trait: v, r̂, counts per level):

```
mb      = (plus(l*) − minus(l*)) / (plus(l*) + minus(l*) + 2)
p_same  = clip((1 + sign × mb) / 2, 0.01, 0.99)
v       = (1 − a) × v + a × (1 − p_same)
Ω       = clip((v − v0') / (1 − v0'), 0, 1)                        # v0' = clip(v0 + volatility gene, 0, 0.999999)
counts  = counts × (1 − Ω), then plus(l*) or minus(l*) += |sign|
r̂       = min((1 − Ω) × (r̂ + 1) + Ω, r_max)
g       = clip((1 + Ω × r̂) / (r̂ + 1) × (r_ref + 1), 1, g_max)
step(l) = e × clip(sign × rate × g × c(l), −step_cap × g, +step_cap × g), recentred
Δ(l)    = round(clip(Δ(l) + step(l), −total_cap, +total_cap), 6)
```

g never goes below 1, so the gate never slows learning below today's rate; `total_cap` is untouched, so the learned box
`prove` uses is unchanged. A new learner starts at v = 0, r̂ = 1 (g = 2.5) and reaches g = 1 after a few consistent feedback
turns. Precision weighting (Yu & Dayan 2005) is opt-in because it changes every sensed document's bytes. State: `surprise:
{trait: {v, r, g, counts: [[plus, minus], …]}}`. Stance: `surprise: {trait: g}` for traits whose g > 1; `why`: `learning
fast: feedback changed` while any g > 1.

The bar is stated for feedback noise ≤ 0.20: at 0.40 the stand-in's speed was 0.84× of the fixed rate, outside s1.

**Public:** "After a run of surprising outcomes it learns faster, and slower again once things settle."

## Design C: pace (mechanism)

The opportunity cost of time sets the latency of every action, τ* = √(C_v/R̄) (Niv et al. 2007); recent average reward sped
responses (t = 6.91) while the reward on offer did not (t = 1.82) (Beierholm et al. 2013).

```yaml
pace:                          # optional; needs reward_from (section 2.10)
  half_life_hours: 6           # the stand-in's value
  cap: 4                       # R̄ stays in [0, cap]
  r0: 1                        # the average at which the effects are 0
  levels: [slow_below, brisk_above]   # required, no default; 0 <= slow_below < brisk_above <= cap
  effects: {initiative: e_i, verbosity: e_v}   # required, no default: ordinal shift per unit of (R̄ − r0); recommended e_i > 0, e_v < 0
  pace_spread: 0               # gene: gain exp(N(0, s²)) on effects; 0..3, default 0
```

The pace step runs after the drive step (h = `elapsed_hours`; r_t = the sum of this event's accepted wins, the needs'
included, plus 1 per reward flag on; corrections add nothing):

```
R̄ = round(clip(R̄ × 0.5^(h / half_life_hours) + (1 − 0.5^(1 / half_life_hours)) × r_t, 0, cap), 6)
```

Stance t reads R̄ after event t; the outcome of turn t's action, carried by a later event, moves only later stances.
On hourly events this is the stand-in's average exactly: a host acting a = base × (1 + κ × (R̄ − r0)) times an hour, each
action paying with probability p, settles at R* = p × base × (1 − κ × r0) / (1 − G) while the loop gain G = p × κ × base <
1 (1.2632, 1.5000, 2.2500 at G = 0.24, 0.60, 0.84), and runs to the cap that binds when G ≥ 1 (in the stand-in, p × the
host's action cap: 3.6000). Lowered as `pace_up` = max(R̄ − r0, 0) and `pace_down` = max(r0 − R̄, 0), gain g_pace =
`round(exp(pace_spread × z(name, seed, "pace", "gain")), 4)`. Stance `pace: {level, rbar}` (slow / steady / brisk by
`levels`) for the host's scheduler; `why`: `pace brisk: average reward <R̄> an hour` when not steady. State `pace: {rbar}`.

The engine cannot see the loop (p is the world's), so `lint` reports the largest field shift pace can cause (Σ|effects| ×
max(cap − r0, r0)) and warns over 1, and `describe` prints the closed form. Caution is not a recommended target, so
verification habits keep their soft odds.

**Public:** "Average reward over the last hours sets the pace of the next actions; a single reward does not."

## Design D: dispositions (mechanism)

People learn more from better- than from worse-than-expected outcomes, with individual differences (Lefebvre et al. 2017);
genes shift gain and loss learning in opposite directions (Frank et al. 2009); dopamine neurons settle at different
expectiles (Dabney et al. 2020); serotonin sets patience and the discount (Miyazaki et al. 2014; Schweighofer et al. 2008).

```yaml
dispositions:                 # optional; each spread 0..3, default 0
  optimism_spread: 0          # o = s × z: α⁺ = rate × e^o, α⁻ = rate × e^−o
  patience_spread: 0          # p = exp(s × z): multiplier on credit half_life_events and deadline_tau_hours
  volatility_spread: 0        # offset s × z on surprise.v0
```

```
§2.8 step:   rate × e^(sign × o), then the step_cap clip
§2.9 E:      E ← E + α × (m − E), α = min(rate × e^(+o), 1) if m > E, min(rate × e^(−o), 1) if m < E
patience:    half_life_events × p, clipped to [1, 10000] (expiry at 4 half-lives); deadline_tau_hours × p
```

With wins b and a at equal odds, E settles at the expectile τb + (1 − τ)a, τ = α⁺/(α⁺ + α⁻). On one stream an individual
with z > 0 keeps E at or above its z = 0 twin's at every step (the update is increasing in E and α⁺, decreasing in α⁻).
Genes `round(o, 4)`, `round(p, 4)`, `round(s × z, 4)` from `z(name, seed, "disposition", <gene>)`; state
`dispositions: {optimism, patience, volatility}`; spread 0 stores nothing. No `why` clause; `describe` prints the genes.

**Public:** "Each individual learns at its own rate from better- and worse-than-expected outcomes; the rates are genes."

## Design E: valence and liking (simulation)

Momentary valence follows expectations and prediction errors, both weights positive, the expectation weight below the
prediction-error weight, per-event forgetting 0.61 ± 0.30, r² = 0.47 ± 0.21, replicated in 18,420 people (Rutledge et al.
2014); liking is a gain separate from wanting (Berridge & Kringelbach 2015).

```yaml
drives:
  valence: {mood: valence, expect: w_e, surprise: w_p}   # replaces effects.surprise; 0 < w_e < w_p, both declared
  afterglow: {half_life_hours: 3, cap: 2, liking: 0, liking_spread: 0}   # λ, default 0 = today
```

Valence evidence on the named mood = w_e × E of the goal the previous stance pursued + w_p × the turn's surprise,
lowered as `drv_expect` (box [0, win_max]) and the existing `drv_surprise`/`drv_letdown`. The mood inertia k plays the
forgetting factor: at k = 0.61 an event 5 events back keeps 0.0845 of its push (half-life 1.402 events). Liking:
afterglow grows by k_gene × (λ_gene × m + max(d, 0)), λ_gene = λ × exp(liking_spread × z(…, "liking")). Weights carry no
default: the sources fix their signs and order only. `drv_expect` is reserved only with `valence`.

**Public:** "Valence moves with expected value and with outcomes better or worse than expected, including expected wins that
do not come."

## Refusals for Designs B–E and part 2

Refused when the persona is read (exit 2, at the field's path); a refused persona never runs:

| row | refused |
|---|---|
| V-R1 | a `surprise` block without a learning block; `precision: true` without a senses block |
| V-R2 | `volatility_rate` outside (0, 1); `v0` outside [0, 1); `r_ref` or `r_max` below 1; `g_max` outside (1, 3] |
| P-R1 | a `pace` block without `reward_from` (no self-reward can move pace) |
| P-R2 | `cap` or `half_life_hours` ≤ 0; `r0` outside (0, cap); `levels` not ordered within [0, cap]; missing `levels` or `effects` |
| P-R3 | a pace effect on `pursue` or on an unknown trait or mood; a taken `pace_up` or `pace_down` id |
| D-R1 | a disposition spread outside 0..3 |
| D-R2 | `volatility_spread` without `surprise`; `optimism_spread` without learning and without drives; `patience_spread` without a credit block and without drives |
| E-R1 | `drives.valence` together with `drives.effects.surprise`; an unknown mood |
| E-R2 | w_e ≤ 0, w_p ≤ 0 or w_e ≥ w_p (the order Rutledge et al. 2014 found); a negative `liking`; `liking_spread` outside 0..3 |
| W-R1 | `rest` without a learning block; `downscale` outside [0.5, 1); `prune` < 0; `threshold` outside (0, 1); a rate or `gap_hours` ≤ 0 |
| L-R1 | `load` with `cap`, `half_life_hours` or `kappa` ≤ 0; a `from` key that is not a learning flag, a declared sense or `expired`; a `hold` on an unknown mood |

A state is refused, even with a recomputed digest, when a field below leaves its range.

## State fields

| field | block | range | rounding | enters at step | bounded by |
|---|---|---|---|---|---|
| `needs.deficit.<id>` | needs | [0, cap] | 6 decimals | 2 | `cap` |
| `need_<id>` (input) | needs | [−pursue.cap, +pursue.cap] | 6 | 6 (field of `pursue`) | `pursue.cap` |
| `needs.genes.weight/growth` | needs | > 0 | 4 | 2 | spread ≤ 3 |
| `needs.success.<goal>` | needs | [0, 1] | 6 | 3 (win, omission) | 0 and 1 |
| `drives.window.<goal>` | expect_by_hours | [0, expect_by_hours] or null | 6 | 3, 8 | `expect_by_hours` |
| `drv_omit` (input) | expect_by_hours | [0, pe_cap] | 6 | 6 | `pe_cap` |
| `surprise.<trait>.v` | surprise | [0, 1] | 6 | 5 | the average of values in [0.01, 0.99] |
| `surprise.<trait>.r` | surprise | [1, r_max] | 6 | 5 | `r_max` |
| `surprise.<trait>.g` | surprise | [1, g_max] | 6 | 5 | `g_max` ≤ 3 |
| `surprise.<trait>.counts` | surprise | ≥ 0 per level and sign | 6 | 5 | discounted by (1 − Ω) |
| `pace.rbar` | pace | [0, cap] | 6 | 4 | `cap` |
| `pace_up`, `pace_down` (inputs) | pace | [0, cap − r0], [0, r0] | 6 | 6 | `cap`, `r0` |
| `dispositions.optimism/patience/volatility` | dispositions | gene | 4 | 3, 5, 6 | spread ≤ 3; rates ≤ 1; half-life ≤ 10000 |
| `drv_expect` (input) | valence | [0, win_max] | 6 | 6 (mood evidence) | `win_max` |
| `drives.genes.liking` | afterglow.liking | > 0 | 4 | 3 | spread ≤ 3; afterglow `cap` |
| `pressure.s`, `pressure.pending` | rest (part 2) | [0, 1], true/false | 6 | rest line | 1 |
| `load.l`, `load_level` (input) | load (part 2) | [0, L_cap] | 6 | after 4; 6 | `L_cap` |

## The step order inside an event

| # | step | new in this RFC | reads | writes |
|---|---|---|---|---|
| 0 | validation and refusals (§2.10, RFC 0001 S1–S2 and B-R1–B-R6, N-E1–N-E4) | needs rows | the event | nothing on refusal |
| 1 | sense step (§2.11) | precision factor (B) | readings | normals |
| 2 | **needs step** (A.3) | yes | stamped h, raw readings, amounts | deficits; wins and setbacks of bound goals |
| 3 | drive step (§2.9): decay, wins, **omission** (A.4), wanting, caps | windows, α± (D), liking (E) | goal signals | W, A, E, windows |
| 4 | **pace step** (C) | yes | stamped h, accepted wins and reward flags | R̄ |
| 5 | credit step (RFC 0001 B.2) with the **surprise gain** (B) and α± (D) | gain, α± | feedback, open decisions | Δ, counts, v, r̂, g |
| 6 | compile (§3): `need_*`, `pace_*`, `drv_expect`, `drv_omit` as number inputs | yes | the state after 1–5 | the program |
| 7 | solve, stance, `why` | clauses | | the stance |
| 8 | after the turn: `since`, **windows open** for the pursued goal, `await` | windows | the released stance | state |

Part 2 adds a **rest line** before step 1 when its conditions hold, and the **load step** after step 4.

## Part 2 (follow-on): rest and load

**`rest` (simulation of the pressure, mechanism of the downscale).** Pressure rises over active hours and falls over idle
gaps (process S; Daan et al. 1984; Borbély et al. 2016); synaptic strength is renormalized in proportion, the strong
surviving relatively (Tononi & Cirelli 2014); consolidation needs low acetylcholine, so no encoding at the same time
(Hasselmo 2006); fatigue is the value of offline computation (Agrawal et al. 2022).

```yaml
rest: {rise_per_hour: r_up, fall_per_hour: r_down, gap_hours: g, threshold: θ, downscale: ρ, prune: ε}   # all required; ρ in [0.5, 1)
```

Daan et al. 1984's human rates (rise 0.055/h, fall 0.238/h, ×0.888 per 30 min) are an example, not a default.

```
h <  gap_hours:  S ← 1 − (1 − S) × e^(−r_up × h)
h >= gap_hours:  if S >= θ: rest line (Δ ← round(ρ × Δ, 6); |Δ| < ε ← 0); then S ← S × e^(−r_down × h)
```

A rest line is its own strand line, chained like a control line, and never shares an event with a learning step: if the
event carries feedback, the line waits for the next event without it (`pressure.pending`). x → ρx is increasing, so the
order of a trait's learned weights is kept. The state key `rest` is taken (§5.1, the resting stance), so the state is
`pressure: {s, pending}`; `pressure.s` starts at 0 and `pressure.pending` at false when the individual is made. Stand-in: a capped learner re-learned a reversal in 5 turns instead of 11 at ρ = 0.5 (8 at 0.75);
a floor exempting reinforced levels gave back a third of the gain and protected nothing extra.

**Public:** "During idle time it scales its learned weights down in proportion, keeping their order."

**`load` (simulation).** Wear accumulates when responses repeat, fail to adapt or fail to shut off (McEwen 1998; McEwen
2006), and can leave a lasting set-point deviation (Koob & Le Moal 2001).

```yaml
load: {half_life_hours: hl, kappa: κ, theta: θ, cap: L_cap,          # all required
       from: {criticism: w_c, sense_tool_fail: w_f, expired: w_x},   # the aversive evidence a_t, weights >= 0
       effects: {caution: e_c, humour: e_h},                         # recommended e_c > 0, e_h < 0
       hold: {mood: <id>, above: θ_m, per_hour: κ_h}}                # optional: a mood held above θ_m adds per hour
```

```
L = round(clip(L × 0.5^(h / hl) + κ × max(a_t − θ, 0) + κ_h × h × [mood accumulator > θ_m], 0, L_cap), 6)
```

Lowered as `load_level` (box [0, L_cap]). One accumulator expresses three of the four patterns (repeated hits, lack of
adaptation, prolonged response); the fourth, an inadequate response with compensatory over-activity, needs two
interacting channels and is out of scope. State `load: {l}`.

**Public:** "Repeated aversive events raise a slow load that relaxes over days."

## Invariants

| invariant | statement |
|---|---|
| P0 | No new block or key: every stance, state, program and fuzz/prove/lint/describe document is byte-identical to 0.9; new reserved ids are reserved only with their block |
| Replay | A turn stays a pure function of (persona, state, inputs, engine version); every new field comes from logged inputs and stamped hours by fixed arithmetic rounded to 6 decimals, genes from name, seed and id; rest lines are logged; `live verify` recomputes all of them |
| Caps | as in the state-field table; Δ within ±total_cap and normals within drift_cap are untouched; the step ≤ g_max × step_cap |
| Refusal | a refused event changes nothing: the run equals the run without it, byte for byte |
| Proof coverage | every new channel enters the field as a bounded number input, so `prove` boxes it and habits stay rules of every program; the learned box is unchanged |
| Lineage | RFC 0001's drift budget bounds generations; `rest` lowers drift (bar r2); no new block absorbs learned weights into a prior |

## CLI and MCP surface

| surface | change |
|---|---|
| `persona describe` | needs (unit, cap, weight, growth, sources, amount, bind, horizon, n and m, the reserved id), windows, surprise settings, pace (closed form, levels, effects), disposition spreads, valence weights |
| `persona turn`, `live` | event key `needs`; stance keys `needs`, `window`, `surprise`, `pace`; clauses in `why` |
| `persona lint` | pace's largest field shift (warn over 1); a horizon whose forecast can pass K/2 before the deficit does; habits that exclude a bound goal |
| `fuzz`, `prove` | `prove` boxes `need_*`, `pace_*`, `drv_expect`, `drv_omit`, `load_level`; `fuzz` adds amounts on `--grid` × (0, cap], repeated and overshooting satisfactions, expiring windows, reward bursts for pace, alternating feedback for the gate |
| `live verify` | recomputes every new field and rest line; summary adds `needs: {id: {deficit, at_cap_hours}}`, `pace: {rbar, max}`, `surprise: {trait: {g_max_seen}}` |
| `monitor` | per need a bar over [0, cap] and its window; R̄ with an r0 tick; g per learned trait; an alert when a deficit sits at its cap |
| MCP | `probbit_persona_turn` and `probbit_live_event` refuse an event that carries `needs` amounts |

## Security review

| input path | who may write it | refused | what a mislabelled value can move, and the cap |
|---|---|---|---|
| a need's amount | the host, from a `satisfy_from` kind | `self`, `clock`, missing or malformed sources, MCP (N-E2, N-R3) | one deficit (to [0, cap]) and one win ≤ r_cap; then E by at most rate × r_cap, s by `success.rate`, afterglow by k × r_cap, R̄ by (1 − 0.5^(1/hl)) × r_cap |
| per-reading growth | a sense's sources (RFC 0001 rule S2) | as RFC 0001 | the deficit by rate × the range's top, clipped to cap |
| stamped hours | the clock (`--clock real`) or the host (`fixed`) | an event's own hours under the real clock | growth to cap; at most one omission per open window (|d| ≤ pe_cap); pace decay; part 2: one rest line per gap (ρ ≥ 0.5) |
| reward-bearing values in pace | `reward_from` kinds only (a pace block requires it) | as §2.10 | R̄ ≤ cap; the field shift ≤ the lint bound |
| feedback into the gate | as §2.8 and §2.10 | as RFC 0001 | g ≤ 3: a forged run of contrary feedback triples the step at most; Δ stays within ±total_cap |
| genes | name, seed, id | a state whose genes miss the seed | nothing at run time |
| the state | the host's store | every new field outside its box, even with a recomputed digest | nothing: refused |

`self` and `clock` are refused for every reward-bearing input as in RFC 0001 §2.10; an event with no reward on is accepted
from any source. A source is the host's word: a forged `env` label passes every check. Signed rewards stay RFC 0001
Design C's.

## Migration

Existing personas, states and strands are untouched. Adding a block changes the persona digest: a new individual on a new
strand. A 0.9 binary refuses the new blocks as unknown fields and refuses a rest line as an unknown line kind, so it never
replays them wrongly.

## Test plan

- Unit: every closed form this RFC states (pay ratio, overshoot, the x > K/2 sign rule, inertia, volatility
  half-life, expectile, pace fixed points and trajectory, gain bounds, rest order, load recovery); one test per row.
- Property: P0 over the RFC 0001 corpus; refusal over 40 individuals × 300 random events mixing sources, amounts and
  windows; replay, and a life split across runs equal to one run; genes unchanged when an unrelated need is edited.
- Fuzz/prove: the example persona's rules held or proved over seeds 0–99 with every new box; acceptance: `bench/body/`.

## Failure modes and guards

| failure | guard |
|---|---|
| A mis-declared need becomes a motive the stance obeys | success-weighted pursuit with extinction (N-R6), `pursue.cap`, ceilings in the bench (n8, n11), habits as rules |
| A need never acts (weighted by a win size that starts at 0) | the success rate starts at `success.init`; bar n10 and the non-vacuity check of n8 |
| A need pursues a principal who is away | the omission lowers E per unanswered pursuit; bar n11 |
| A forecast overshoots | horizon default 0, capped at 72; `lint`; bar n10 |
| Pace runs away | R̄ cap, effect gene bounded, `lint` loop warning, bar p6 |
| The gate amplifies noise | slow volatility average; g ≥ 1 and ≤ 3; bar s2 |
| A pessimistic individual over-learns corrections | `step_cap` and `total_cap` bound every step |
| An omission counted as feedback refuses events | omissions stay out of the learning sign |
| Downscaling erases what was taught | ρ ≥ 0.5, order kept by construction (r0), rest never with a learning step |
| A self-reward inflates pace or a deficit | pace requires `reward_from`; `satisfy_from` refuses `self` and `clock`; MCP refuses amounts |

## Alternatives considered

| alternative | why not |
|---|---|
| pursuit by the marginal drive ∂D/∂x | 9.5 contact pursuits per waking day and negative wins on the stand-in |
| a fixed answer probability in the pursue term | 15 contact pursuits a day once the principal was away |
| a one-step z gate; Bayesian-hazard gates | 2.98× stable-phase drift; no hazard setting met both halves of the bar |
| a keep-floor for reinforced levels at a rest line | a third of the gain lost, nothing protected |
| host-sized wins for needs | the reduction would leave the log, and deprivation scaling with it |
| pace from the current event's reward | the reward on offer is what did not move speed (Beierholm et al. 2013) |

## Open questions for contributors

1. Should an omission join the learning sign? **Recommended: no**: it is about the stance that opened the window, and as
   feedback it would trip B-R1 whenever another decision is open.
2. Should the surprise gate also scale each goal's expectation rate? **Recommended: not in part 1**: only the
   learned-weight learner was tested, and an E gate has no bar.
3. Where should the success rate start, and how fast should it move? **Recommended: start 1, rate 0.2** (the stand-in's
   learning step for answered and unanswered pursuits): a need can act before its goal has paid, and each unanswered
   pursuit removes a fifth of what is left. n8, n10 and n11 were measured on stand-ins, so they run on the engine
   before this is final, frozen as written.
4. Should a rest line be its own strand line? **Recommended: yes**: `verify` replays it and an older binary refuses it.
5. Should habits read `need.<id>.deficit`? **Recommended: yes**, as number conditions, so a must-do habit can fire at a
   deficit while the soft term stays capped.

## References

Research the designs and their numbers draw on, cited above by author and year.

- Agrawal et al. 2022, Psychol Rev
- Amsel 1979, Current Contents
- Arike et al. 2025, arXiv 2505.02709
- Aston-Jones & Cohen 2005, Annu Rev Neurosci
- Behrens et al. 2007, Nat Neurosci
- Behrens et al. 2008, Nature
- Beierholm et al. 2013, Neuropsychopharmacology
- Berridge & Kringelbach 2015, Neuron
- Borbély et al. 2016, J Sleep Res
- Bouton 2004, Learn Mem
- Daan et al. 1984, Am J Physiol
- Dabney et al. 2020, Nature
- Eldar & Niv 2015, Nat Commun
- Frank et al. 2009, Nat Neurosci
- Hasselmo 2006, Curr Opin Neurobiol
- Keramati & Gutkin 2014, eLife
- Kidd & Hayden 2015, Neuron
- Koob & Le Moal 2001, Neuropsychopharmacology
- Lefebvre et al. 2017, Nat Hum Behav
- Maier & Seligman 2016, Psychol Rev
- Mattar & Daw 2018, Nat Neurosci
- Matthews et al. 2019, Ann N Y Acad Sci
- McEwen 1998, N Engl J Med
- McEwen 2006, Dialogues Clin Neurosci
- Miyazaki et al. 2014, Curr Biol
- Murray et al. 2009
- Nassar et al. 2010, J Neurosci
- Niv et al. 2007, Psychopharmacology
- Rankin et al. 2009, Neurobiol Learn Mem
- Rutledge et al. 2014, PNAS
- Schaul et al. 2016, ICLR
- Schultz 2016, Dialogues Clin Neurosci
- Schweighofer et al. 2008, J Neurosci
- Seymour et al. 2005, Nat Neurosci
- Solomon & Corbit 1974, Psychol Rev
- Sterling 2012, Physiol Behav
- Tomova et al. 2020, Nat Neurosci
- Tononi & Cirelli 2014, Neuron
- Wang et al. 2024, arXiv 2412.06435
- Wilson et al. 2014, J Exp Psychol Gen
- Young & Wang 2004, Nat Neurosci
- Yu & Dayan 2005, Neuron
