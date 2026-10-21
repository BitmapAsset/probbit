# RFC 0009: surprise-driven live state

Status: **DRAFT (lab result, not implemented)** · Target: none yet · Changes if adopted: docs/persona.md (format 1, additive),
`probbit live`, `probbit monitor`, `live verify` · Depends on: RFC 0001 (senses), RFC 0003 (reafference and consolidation,
draft, not yet in this repository), RFC 0004 (the bound)

Mechanism and simulation only: every key below is arithmetic over logged events, a sampler's output or a typed judge's
score, recomputable from the strand. The numbers come from two lab waves on the 0.11 engine (`152d005`): N1 (the first
draft) and N1-b (a constrained schedule search and a second growth rule). This text merges N1-b's proposed changes into
N1's draft. The measured results are summarised in [docs/results/0.11](../results/0.11/README.md#lab-results-not-yet-in-the-engine).

## Summary

Four additions, each optional and off by default, so a persona without them produces the same bytes as today:

1. `trajectory`: the stance computed coarse-to-fine. The engine's own program for the event is sampled along a beta
   schedule (β0 → 1) with categorical p-bits; each step emits per-variable marginals, the last step matches the exact solve,
   and the committed stance is still the exact `--op decide` answer. The steps are a display path, never a second answer.
2. `senses[].novelty`: a number sense whose reading is the novelty of the incoming message (a typed judge's 0..1 score
   against the previous 8 messages), judged by the existing sense step against a learned normal (RFC 0001).
3. The two-way channel: engine → model gets one coarse-state line (≤ 90 characters) before `stance.block`; model → engine
   gets a reafferent reading of the reply's register (RFC 0003 block A).
4. `growth`: a bounded rule that switches on dormant sense slots ("cells") inside a declared budget, with consolidation
   (RFC 0003 block B) deciding which cells persist.

## 1. `trajectory` (persona `engine.trajectory`)

Proposed default schedule (the constrained search's kept point):

```yaml
engine:
  trajectory:              # optional; absent = no trajectory, no row, same bytes as today
    k: 10                  # steps, 2-16
    shape: lin             # lin | geo | pow | sig
    beta0: 0.2             # first step's beta, (0, 1)
    lanes: 128             # p-bit lanes (chains), multiple of 8
    sweeps: 62             # per lane; lanes x (burn + sweeps) = 8,448 lane-sweeps per event (fixed work: deterministic)
    final_frac: 0.7        # share of sweeps at beta = 1
    q: 0.5                 # early-step sweep weight (i+1)^q: back-loads the steps before beta = 1
    burn: 4
    floor: 1               # minimum sweeps per step before the last (>= 1)
```

The stance gains nothing per step and the strand gains nothing; the trajectory is emitted only on the monitor stream and
on `probbit live --trajectory`:

```
TRAJECTORY <event> k=<i>/<K> beta=<b> <var>=<p_l0>/<p_l1>/<p_l2> ... H=<mean entropy>
```

Seeds: `Philox4x32(turn seed, lane)`, so a trajectory is a pure function of (program, turn seed, schedule): `live verify`
recomputes it and compares the bytes (receipt key `trajectory_digest` = sha256 of the rows, per event).

**The sharpening property is a constraint, not a hope.** The schedule is chosen by minimising total-variation distance to
the exact solve at fixed cost, subject to a floor on the share of (trajectory, variable) pairs whose marginal entropy is
non-increasing step to step (one rise allowed); the floor is part of the key's contract. Measured on 1,931 held-out
trajectories (four lab personas, ≤ 11 three-level variables each), the three schedules form one frontier:

| schedule | mean TV | max TV | monotone, one rise / none | ms per event, median / p95 |
|---|---|---|---|---|
| TV-only search (N1) | 0.000485 | 0.014928 | 0.931 / 0.785 (**fails** the 0.95 bar) | 2.56 / 3.28 |
| constrained at 0.97 (N1-b, the default above) | 0.000564 | 0.020997 | 0.970 / 0.939 | 2.56 / 3.23 |
| untuned start (`8,lin,0.1,…,0.5`) | 0.000683 | 0.022152 | 0.989 / 0.971 | 2.63 / 3.30 |

The constraint was binding: 221 rejected mutations had a lower TV than the kept one. A constraint of 0.97 on the tuning
seeds gave 0.970 on held-out. Trajectories were byte-identical over two runs.

- **`floor`: keep, and say what it does not do.** At fixed cost a floor ≥ 2 moved sweeps off the β = 1 step and raised TV
  (197 tries, 0 kept). It buys monotonicity with accuracy, the same trade as the constraint; it is for owners who want a
  monotone share near 0.99 with a short schedule.
- **Publish the sharpening claim with a tolerance.** The receipt states "entropy of each variable's marginal is
  non-increasing step to step within `tol` nats". Measured for the default (one rise / none): 1e-9: 0.970 / 0.939; 1e-4:
  0.975 / 0.944; 1e-3: 0.984 / 0.958; 1e-2: 0.999 / 0.990. Proposed published form: "≥ 99 % of variables non-increasing
  within 0.01 nats with no rise allowed; ≥ 97 % within 1e-9 with one rise allowed". `tol` is a reporting key; it never feeds
  the schedule search (a threshold the optimiser can move is no constraint).
- **Gap: re-solved events.** Events whose habits contradict are re-solved by the engine (persona §5.5), and `vouch`/`hold`
  traits re-solve clamped; the trajectory is of the first program. 75 of 2,400 generated events (3.1 %) were habit-conflict
  re-solves and were skipped; hold re-solves were not counted separately.
- **Not shown:** anything about programs too large for exact enumeration.

## 2. `senses[].novelty`

```yaml
senses:
  - id: novelty
    kind: number
    unit: novelty score
    range: [0, 1]
    src: [env]                       # the host computes it; never the agent's own channel
    prior: {mean: 0.6, var: 0.05}
    drift_cap: 0.35
    half_life_events: 10
    effects: {}                      # observing first; effects only after the calibration bars of §3
```

The reading is the host's: a typed LLM judge (a 5-level score, expected index / 4) over the previous 8 messages and the new
one. The engine owns the normal: z = (x − mean) / sqrt(var + var_floor). Measured: AUC first-seen vs repeat or paraphrase
1.00 (lexical 3-gram control 0.87; 0.73 on paraphrases only) on 28 real messages and 6 planted controls; 2 of 28 real
messages registered at |z| ≥ 1.5 (7.1 %). A theme sent six times reads 0.99, 0.53, 0.05, 0.04, 0.02, 0.00, with z +1.62,
−0.08, −1.74, −1.56, −1.45, −1.37: the normal follows the run of low readings, so the engine habituates to the repetition.

- **Gap:** "surprise on almost every message" is not what the engine measures: against its learned normal most real
  messages are within 1.5 s.d.; the learned normal is what turns "every message is new" into "this message is unusually new".
- **Gap:** 28 real messages and 6 controls are a small sample; one judge; the real messages were assumed first-seen.

## 3. The two-way channel (design; pilot not run)

**Engine → model, the coarse line** (one line, ≤ 90 characters, before `stance.block`). From the trajectory's middle step
(⌈K/2⌉) per mood: `settled` if top odds ≥ 0.8, `leaning` if ≥ 0.5, else `uncertain`; then the novelty clause when
|z| ≥ 1.5:

```
state forming: valence leaning up 0.62, pressure uncertain 0.48; novel z +1.6
```

The final stance stays authoritative; the line says how settled the state was mid-schedule (a sampled estimate, labelled
"forming"). Length over every held-out event: 66-74 characters without the clause, 80-88 with "; novel z +6.0".

**Open before any pilot:** at the middle step the line reads "uncertain" on at least one mood for 90-95 % of events and
"settled" for none, with either schedule. As a per-event signal it is nearly constant. Options: read the line at a later
step (e.g. ⌈3K/4⌉), or grade against the final odds (the share of the final top odds reached mid-schedule) instead of
fixed 0.8 / 0.5 cuts. Run the bars below only after that choice.

**Model → engine, reafference** (RFC 0003 block A). After the reply is sent, the host scores its register with three typed
questions (verbosity, humour, hype; 3 levels each, expected index / 2) and sends `senses: {reply_register: {v, src:
env:host}}, act: true`. The sense is `reafferent`: the engine predicts the register from its own stance and judges the
reading against the after-own-act normal, so a reply that matches what the stance asked reads as usual and a reply that
departs from it reads as prediction error. It moves nothing at first (`effects {}`).

**Follow-on bars (written, not run):**
- c1: calibration ECE of the model's reply register vs the stance odds ≤ 0.15 with the coarse line in the prompt (0.29 /
  0.36 measured without it), on ≥ 100 replies, one benchmark run.
- c2: the reafferent z on replies that follow the stance has |median| ≤ 0.5; on replies forced off-stance (a seeded control
  set of 20) median |z| ≥ 1.5.
- c3: the coarse line costs ≤ 25 tokens and ≤ 20 ms per turn.

## 4. `growth` + budget

The rule reads readings only (rule v2a; the first draft keyed hits on the simulation's context id, which the engine never
has):

```yaml
growth:
  slots: [cell0, cell1, cell2, cell3]   # dormant senses declared in `senses`: the set never changes
  theta: 3.0            # routing: a reading goes to the nearest open cell if |z| < theta; else the base judges it
  k: 4                  # a cell opens when a pending candidate holds hits from >= k distinct visits ...
  window_events: 50     # ... within this many events
  visit_gap_events: 10  # a visit = run of base-judged events between z-sign changes or a gap >= this
  probation_events: 100
  keep_uses: 3          # fewer uses in probation -> pruned, and that region cannot open again for probation_events
```

A hit is a reading no open cell explains; hits pool by proximity (within theta × sqrt(0.25) of a candidate's mean). The
budget is the slot list: growth only switches a declared slot on, so the sense set, the digest and every cap are fixed when
the file is read.

- **The bound holds.** Through the engine, a growth-on arm (60 campaigns) stays at 0 of 180 (campaign, trait) cases over
  the `persona bound` epsilon (max ratio 0.944), with 81 slots opened over 53 campaigns; the 247-campaign suite stays at 0
  exceedances. Replay was byte-identical twice.
- **Open gap (failed bar g1).** The bar asks that a varied stream end with more cells than a repetitive one on ≥ 45 of 50
  seeds, and that the repetitive stream plateau on ≥ 45 of 50. The first rule: 22/50 and 6/50. Rule v2a: **40/50** (fail)
  and 49/50. Hits per visit fixed the repetitive stream's late openings. The varied stream now plateaus too (3-11 cells
  against about 40 latent contexts) for two measured reasons: rare contexts never collect 4 visits in 50 events, and cells
  swell (32 % of varied-stream cells end with variance above 1 against a reading noise of 0.25), so one cell covers several
  clusters.
- **Proposed next rule (v3, not run):** cap a cell's variance at a multiple of its opening spread (e.g. `cell_var_cap: 4x`
  the opening variance) so a cell cannot absorb its neighbours, and make k decay with the candidate's age, or use k = 3 with
  W ≥ 100, so rare recurrences can open a cell. g1 stays the bar, unchanged.

## 5. Monitor row

`probbit monitor` serves the strand with heartbeats. Add one row kind after each event's stance: `TRAJECTORY` (format above),
throttled to the last event; the page draws the K steps as a blur-to-sharp bar per variable. The row is derived; it is never
written to the strand.

## 6. Receipts in `live verify`

`live verify` recomputes per event: the trajectory rows' sha256 (with the persona's schedule), the novelty sense's z from
logged readings, the reafferent normal, and which growth slots were open (from logged readings, the visit counter, the
cooldown list and the rule's parameters); a mismatch fails verification as today's digests do.

## 7. Welfare page (docs/welfare-stance.md), one paragraph under "What the words mean here"

- **Coarse-to-fine state, novelty, growth**: the "state forming" line is the marginal odds of a sampler at an intermediate
  inverse temperature over the same program the engine solves exactly; it is a picture of a computation converging, not a
  report of an inner experience. "Novelty" is a judge's score of how much a message differs from the previous eight, and
  "registers novelty" means its z-score against a learned average exceeded a threshold. "Growth" is a declared, budgeted
  set of sense slots switched on by a counting rule; nothing grows beyond the file's declared slots, and every number can be
  recomputed from the strand.

## 8. Not claimed

No claim that the engine's state is anything but arithmetic over logged events. A "blurry" stance here means sampler
marginals at an intermediate beta, nothing more.
