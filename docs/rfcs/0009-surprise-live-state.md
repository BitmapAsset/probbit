# RFC 0009: surprise-driven live state

Status: **proposed, this PR** (engine side: `expectation` and the exact `engine.trajectory`, 0.12.0-dev; §2-§4 stay
draft) · Changes: docs/persona.md §2.17 (format 1, additive), `probbit live`, `probbit monitor`, `live verify`,
`probbit persona trajectory` · Depends on: RFC 0001 (senses), RFC 0003 (reafference and consolidation, draft, not yet in
this repository), RFC 0004 (the bound)

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

## 1. `trajectory` (persona `engine.trajectory`), computed exactly

This PR replaces the draft's sampled trajectory: for these programs p_beta, proportional to p^beta, is computed exactly
with `Model::scaled(beta)` and the exact tier. No lanes, no sweeps, no schedule search, zero sampling error; the draft's
sampler keys (`shape`, `lanes`, `sweeps`, `final_frac`, `q`, `burn`, `floor`) are not part of the key.

```yaml
engine:
  trajectory:     # optional; absent = no trajectory, no row, same bytes as today; outside the digest
    k: 8          # steps, 2-16
    beta0: 0.4    # first step, (0, 1); linear to beta = 1
```

The stance and the strand gain nothing: the trajectory is a pure function of the event's program, derived on demand
(`probbit monitor` row `TRAJECTORY`, the monitor's JSON frame `trajectory`, `probbit persona trajectory`), so `live verify`
needs no receipt for it. The coarse line of §3 keeps its wording.

**The sharpening property.** The joint entropy of p_beta is non-increasing in beta (dH/dbeta = -beta Var_beta(log w) <= 0, a
theorem; asserted on small programs by enumeration in the tests). A single variable's marginal entropy is not bound by it
and can rise; the draft's b2 bar (one rise allowed) is restated on the exact path, measured below. The half-way state is a
deterministic function of the final program: a picture of the same answer at a higher temperature, never a second source of
information, so it drives nothing.

**Measured (N2-ss lab, 2026-10-22):** filled in below before review.

## 1b. `expectation`: the engine's own expectation and the exact deviation of each event

docs/persona.md §2.17 has the key. Per `probbit live` event, from the state before it: A_t = the event's program; E_t = the
program with only time passing; Pi_t = a mixture over the canonical input combos the strand has seen (decayed counts,
half-life 32 events, one pseudo-count on the empty combo, at most 16 members). The engine reports `self` = sum_i
KL(A_i || E_i), `pred` = sum_i KL(A_i || Pi_i) (exact from the per-variable marginals of at most M + 1 exact solves:
mixture marginals are linear in the members' marginals), `input` = -ln w(c_t), `cold`, and a z against a learned normal.
Effects {}: the deviation moves nothing; the stance and state digests are those of the persona without the block.

**A bound by construction.** A_t is the member of its own combo, so Pi_i >= w(c_t) A_i and each variable's term is at most
-ln w(c_t); for a combo without its own member, Pi_i >= w_empty E_i and the term is at most KL(A_i || E_i) - ln w_empty.
The cap falls as an input recurs: that is the honest shape of habituation here. The reading stays above zero while other
inputs also occur, and reaches zero only if one input is all that happens.

**Gap 3 (the novelty sense rarely reads as unusual), the engine side.** The host's novelty judge reads the world; this
reading is the engine's own: non-zero on every post-cold event that moves any input, graded, bounded, and recomputed by
`live verify` from the strand. It does not replace the novelty sense of §2.

**Measured (N2-ss lab, 2026-10-22):** filled in below before review.

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
- **Open gap (failed bar g1), untouched by the 0.12.0-dev engine change.** The bar asks that a varied stream end with more cells than a repetitive one on ≥ 45 of 50
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
