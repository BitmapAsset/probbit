# RFC 0001: Senses and reward

Status: draft for review · Target: probbit 0.9.0 · Changes: docs/persona.md (format 1, additive)

## Summary

1. Two optional blocks (`senses`, `learning.credit`) and four event keys (`senses`, `await`, `for`, `cancel`). Without the
   blocks every document is byte for byte 0.8's.
2. `senses`: the host sends measured readings in declared units with a named source; the engine keeps a learned normal per
   reading and turns each reading into a capped z-score against it, which moves moods and traits like any input.
3. `why` names the past: the normal a reading was judged against, and what the learned weights came from.
4. `learning.credit`: a reward names the decision it pays for (`for: <n>`); late, reordered and cancelled outcomes credit
   the right decision or are refused.
5. Moods and drives stay what they are: capped, replayable numbers that change behaviour for printed reasons.

## Motivation

Two measured facts (our tests, synthetic fixtures, probbit 0.8):

- **The past changes decisions but is never named.** One 4-trait persona, one seed, two scripted lives of 40 host-measured
  events, the same final message: two different stances, a third copy replayed byte for byte, 0 habit breaks. After a 720-hour
  gap and 8 calm events the two still differed (mean odds distance 0.154), yet both `why` lines were the same text, since `why`
  cites only present inputs and history inside its window. With scheduled process measurements, 40/40 life turns differed and
  a blind classifier separated the lives on 400/400 lines, yet taken levels converged after an idle interval: moods relax,
  history rolls out of its window, and only learned weights persist, which only feedback moves.
- **Late outcomes pay the wrong decision.** 0.8 credits a reward to the stance just before it. One act-or-ask policy, 5
  individuals × 300 events, outcomes 1–3 events late: 36–39 % of rewards credited a decision at a different level, and the
  share of `act` in the last 100 events fell from 0.60 to 0.38 (delay 3). Dropping mismatched rewards restores 0.59 but
  discards 40–41 % of them; one outstanding action per context removes the hole and serialises the agent.

## Non-goals

Reading text; idle actions; time-of-day curves; one individual's stance as another's reading; host-normalised readings;
authenticated readings; an exploration policy; any claim about inner states, which no test can see.

## Design A: senses

### A.1 Schema

```yaml
senses:                          # optional, 1-24 senses; with the block, `src` is read on every event
  - id: tool_fail                # at most 32 characters; reserves the input ids sense_tool_fail_*
    kind: flag                   # flag (the normal is a rate) | number (then `range: [lo, hi]` is required)
    unit: event                  # 1-24 characters, in the digest: two units are two personas
    scale: linear                # linear | log1p (the normal is kept on ln(1 + reading); numbers with range low >= 0)
    src: [env]                   # env and/or human; default [env]
    prior: {mean: 0.05, var: 0.05}   # required: the normal at init, anchor of both caps (scaled units)
    drift_cap: 0.25              # required: the mean stays within prior.mean ± drift_cap
    var_cap: 0.2                 # default 4 × prior.var: the variance stays within [0, var_cap]
    half_life_events: 10         # default 10 (1-10000)
    zcap: 6                      # default 6 (1-20)
    dead_zone: 0.5               # default 0.5 (0 <= dead_zone < zcap)
    var_floor: 0.01              # default 0.01 (> 0)
    say: tool failures           # default: the id, _ as spaces
    effects: {caution: 0.8, valence: -0.4}   # ordinal shift per unit of z beyond the dead zone; negative z pushes back
    gain_spread: 0.3             # gene: gain exp(N(0, s²)) on effects; default 0, range 0..3 (all three)
    threshold_spread: 0.3        # gene: offset N(0, s²) on dead_zone; default 0
    habituation_spread: 0.3      # gene: multiplier exp(N(0, s²)) on half_life_events; default 0
```

Why these defaults. `half_life_events` 10: a reading 10 events old counts half, so the normal follows a changed rate halfway
within 10 readings. `zcap` 6: a failure after 40 quiet readings scores 3.4× the same failure after 40 readings at rate 0.3;
at zcap 3 the quiet case saturates and the ratio drops to 1.7. `dead_zone` 0.5: readings within half a standard deviation
move nothing, so a steady room stays quiet. `var_floor` 0.01 keeps z finite when the variance collapses, and moves z under
10 % while the variance is ≥ 0.05. `var_cap` 4 × prior.var: the spread at most doubles, so it alone never shrinks a z below
about half. `src` [env]: readings come from instruments unless people are named. Spreads 0, as every gene in the format.
24 senses keep the step small (it is linear in senses); 32-character ids keep `sense_<id>_down` within the 48-character
identifier limit. `prior` and `drift_cap` have no default: no unit-free anchor is safe.

Refused on read (exit 2): a taken `sense_` id, an effect on an unknown target, `src` naming `self` or `clock`, `prior.mean`
outside the scaled range, `prior.var` ≤ 0, `drift_cap` ≤ 0, `var_cap` < `prior.var`, `dead_zone` ≥ `zcap`, `log1p` on a flag
or a negative range low, a cap with more than 6 decimals.

### A.2 Events

```json
{"senses": {"tool_fail": true, "tool_latency": {"v": 2.4, "src": "env:runner"}}, "src": "env:host", "elapsed_hours": 0.2}
```

A bare value takes the event's `src`. Without the block, `senses` is an undeclared input in `ignored`, as in 0.8.

### A.3 The sense step

After validation and before the drive step, per declared sense with a reading, in declared order (genes g_gain, g_thr,
g_hab; each line rounded to 6 decimals):

```
x     = reading (flag: 1 or 0), or ln(1 + reading) for log1p
h     = clip(half_life_events × g_hab, 1, 10000)
a     = 1 − 0.5^(1/h)
z     = clip((x − mean) / sqrt(var + var_floor), −zcap, +zcap)        # against the past, before the update
d     = clip(dead_zone + g_thr, 0, zcap − 0.000001)
zd    = sign(z) × max(|z| − d, 0)
mean' = clip(mean + a × (x − mean), prior.mean − drift_cap, prior.mean + drift_cap)
var'  = clip((1 − a) × (var + a × (x − mean)²), 0, var_cap)              # the mean before the update
n'    = min(n + 1, 1000000)
```

The turn adds g_gain × zd times each effect (an ordinal shift, section 2.3) to its target's field or mood evidence (section
3), as a number input does. Lowering follows drives' `surprise`: reserved number inputs `sense_<id>_up` = max(zd, 0) carrying
the effects and `sense_<id>_down` = max(−zd, 0) carrying them negated, both in [0, zcap], gain g_gain. Genes, with section
5.4's z: `round(exp(gain_spread × z(name, seed, "sense", id, "gain")), 4)`, `round(threshold_spread × z(…, "threshold"), 4)`,
`round(exp(habituation_spread × z(…, "habituation")), 4)`; spread 0 means gain 1, offset 0, multiplier 1 and no stored gene,
as in 0.8. State: `senses: {genes: {gain: {id: g}, threshold: {…}, habituation: {…}}, norm: {id: {mean, var, n}}}`, each
`norm` starting at the prior with n = 0.

At the defaults (flag prior 0.05/0.05) the same failure every event gives zd 3.38, then 1.03 from reading 6 on: the push
falls to 0.30× and stays there, because the drift cap keeps a chronic failure from becoming normal.

### A.4 Rules the engine enforces

- **S1 Declared, in range.** A reading for an undeclared sense, a number outside its range or a flag that is not true/false
  refuses the event at `inputs.senses.<id>`; nothing is clipped.
- **S2 Provenance.** Each reading's source kind must be in its `src`; missing, malformed, `self` and `clock` refuse the event
  whole, and a refused event changes nothing.
- **S3 The engine owns the normal.** Hosts send raw readings; z, normals and genes come from logged readings only.
- **S4 Anchored.** A state is refused, even with a recomputed digest, if a mean leaves prior ± drift_cap, a variance leaves
  [0, var_cap], n is not a whole number up to 1e6, the sense set differs from the persona's or a gene misses its seed.
- **S5 No rule reads the normal.** `when` takes `sense.<id>.raw` (true/false for a flag, declared units for a number) and
  `sense.<id>.z` (A.3's z, before dead zone and gain), never the mean or variance; a condition on a sense not read this event
  is false. A rule that must hold whatever the history reads `raw`.
- **S6 No clock decay.** Normals move only on readings; silence and control lines leave them.
- **S7 Not through the agent's channel.** MCP tools refuse events that carry readings.

`prove` reads `sense_<id>_up/down` as boxes [0, zcap] × gain and each sense condition both ways, so a habit on
`sense.<id>.raw` holds by construction for a rule with the same condition. `fuzz` adds readings: flags both ways, numbers on
`--grid` × range, one reading repeated 1, 5 and 20 times, and alternating range ends.

### A.5 The stance and `why`

The stance gains `senses` (per sense read: `raw`, `src`, `z`, `zd`, and the `mean` and `n` it was judged against) and
`noticed` (the normal clause's sense, or null); `inputs` echoes `senses`. `why` becomes: bound habits; the normal clause; up to
two other live inputs (the 0.8 rule, sense inputs excluded); the learned clause; the engine status if not ok.

- **Normal clause**, at most one: the read sense with the largest |g_gain × zd| > 0 (ties: declared order),
  `tool failures far above its normal (z +6.0 over 40 readings)`, "far" at |z| ≥ 2 (two standard deviations), above or below
  by sign. Failing that, the read sense with the largest |z0| − |z| ≥ 1 (the smallest gap worth citing), z0 being the reading
  judged against the prior: `tool failures usual for it now (z +0.9 over 40 readings, +3.9 against its prior)`.
- **Learned clause**, at most one, with a learning block and `line.why_past` (default true with `senses`, false without, so 0.8
  documents keep their bytes): the released learned trait with the largest |Δ| at its taken level, if ≥ 0.05 (odds × 1.05),
  `learned: caution +0.31 on careful (0 rewards, 4 corrections)`. The counts are state: `learned_from`, per learned trait,
  the feedback turns that moved it, by sign.

z prints with one signed decimal and Δ with two, rounded half to even from the stored value.

## Design B: action-addressed feedback

### B.1 Schema and event keys

```yaml
learning:                        # the 0.8 block, unchanged
  from: [praise, criticism]
  traits: [action]
  rate: 0.3
  step_cap: 0.05
  total_cap: 0.4
  credit:                        # optional
    max_open: 8                  # decisions awaiting an outcome at once (1-64)
    half_life_events: 32         # eligibility halves per 32 events of delay (1-10000)
```

`max_open` 8 covers 8 tool results in flight and keeps the state to a few KB. `half_life_events` 32: a reward 3 events late
keeps 0.94 of its step, 10 events late 0.81. A decision expires after 4 half-lives (128 events), where a payment would move at
most 1/16 of `step_cap`.

Event keys: `await: true` (the host will act on this turn's stance and report its outcome); `for: n` (this feedback pays the
decision whose stance has `turn` n); `cancel: [n, …]` (no outcome will come; needs an env or human `src`). They are read, not
compiled, and echoed in `inputs`.

### B.2 The credit step

After the drive step (its surprise joins the sign), before compile; t is this event's turn. Feedback means a `learning.from`
flag is on, or (with drives) κ × surprise ≠ 0. B.3 is checked against the open set after this event's expiry and cancels.

```
expire  each open n with t − n − 1 ≥ 4 × half_life_events                  (closed: expired)
close   each n in cancel                                                      (closed: cancelled)
if feedback:
  sign    = the 0.8 sign: +1 reward, −1 correction, 0 both; with drives clip(sign + κ × surprise, −1, 1)
  paid    = for if given, else t − 1
  c       = open[paid].credit, or the state's credit when paid = t − 1 is not open
  e       = 0.5^((t − paid − 1) / half_life_events), rounded to 6 decimals    (1 for the previous stance)
  step(l) = e × clip(sign × rate × c(l), −step_cap, +step_cap)
  step(l) = step(l) − mean over levels of step
  Δ(l)    = round(clip(Δ(l) + step(l), −total_cap, +total_cap), 6)
  close   paid                                                                (closed: paid)
-- compile and solve with Δ --
credit  = onehot(level) − odds per released learned trait (the 0.8 rule)
if await: open[t] = {credit, stance digest}; beyond max_open close the oldest  (closed: evicted)
```

Eligibility scales the capped step, so a stale reward always moves less than a fresh one, and at e = 1 the step is 0.8's
exactly. Paying a stance a habit forced moves nothing and is marked `absorbed`. A control line closes every open decision
(closed: control): in 0.8 no credit crosses one. State: `open_credit: [{turn, credit, stance}]` (turns increasing, at most
`max_open`, credits within ±1). Stance: `credit: {paid: {turn, e, sign, absorbed, stance} | null, closed: [{turn, as}],
open: [turns]}`.

### B.3 Refusals

Refused whole (one `{"error"}` at its path, no turn, no state change; exit 2 for `persona turn`, at the end of a `live` run):

- **B-R1** feedback without `for` while a decision other than t − 1 is open;
- **B-R2** `for` naming a decision neither open nor t − 1 (paid, cancelled, expired, evicted, never opened, future);
- **B-R3** `for` on an event without feedback;
- **B-R4** `cancel` naming a decision not open, twice, or the one `for` pays;
- **B-R5** malformed: `for` not a whole number, `cancel` not a list of at most `max_open` whole numbers, `await` not
  true/false, a `cancel` without an env or human `src`.

`reward_from` checks run before these, unchanged.

### B.4 Coexistence with "previous stance"

Without `for`, feedback pays t − 1, as in 0.8, whenever that is unambiguous: no decision is open, or only t − 1 is. A host
that never sends `await` keeps 0.8 learning exactly; only the new keys appear. Without the block, `await`, `for` and `cancel`
are undeclared inputs in `ignored` (`live` warns on stderr), so P0 holds.

## Design C (follow-on, not in 0.9.0): receipts and a lineage drift budget

Receipts: signers in `reward_from` (`{praise: {kinds: [env], signers: [ci]}}`) and `receipt: {id, signer, sig}` on the event,
signing the persona digest, seed, `for`, the paid stance digest and the flags; since Design B pays a decision once, a replayed
receipt fails B-R2. Open: a signature scheme under the zero-dependency rule (a shared-key HMAC puts the key in `verify`) and
key rotation. Drift budget: `lineage: {m0_digest, probe_set, max_drift}` in a persona whose priors absorbed a parent's learned
weights; `verify --lineage M0` refuses a generation whose probe odds sit further than `max_drift` from M0's. In our tests one
policy absorbing its learned weights each generation moved 0.460 in 10 generations, against about 0.20 for one life. Open:
what counts as a generation, and where the check runs.

## Invariants

- **P0.** No new block: every stance, state, compiled program and fuzz/prove/lint/describe document is byte-identical to 0.8;
  a strand differs only in its header's `engine` string and the hashes chained from it; every 0.8 strand verifies.
- **Replay.** A turn stays a pure function of (persona, state, inputs, engine version). Normals, `open_credit` and
  `learned_from` are state derived from logged inputs by fixed arithmetic rounded to 6 decimals; genes come from name, seed
  and id; `live verify` recomputes them.
- **Caps.** Mean within prior ± drift_cap; variance within [0, var_cap]; Δ within ±total_cap; at most `max_open` open; e ≤ 1.
- **Refusal.** A refused event changes nothing: the run equals the run without it, byte for byte.
- **Proof coverage.** Rules proved on 0.8 stay proved: the learned box is unchanged, sense pushes enter as boxes.

## CLI and MCP surface

| surface | change |
|---|---|
| `persona describe` | senses (unit, range, scale, sources, prior, caps, spreads, reserved ids) and the credit block |
| `persona turn`, `live` | event keys `senses`, `await`, `for`, `cancel`; stance keys `senses`, `noticed`, `credit` |
| `live verify` | recomputes normals and open decisions; summary adds `normals: {id: {mean, n, max_move_50}}` and `open` |
| `live verify --actions FILE` | lines `{"turn", "action": {trait: level}}`: the earliest turn whose host action differs from the released level |
| `monitor` | per sense the normal as a tick over the range, the reading, z, n; an alert when a mean moved over drift_cap/2 within 50 events; open decisions with e |
| `fuzz`, `prove` | A.4; `fuzz` adds late, reordered, duplicated, cancelled and unknown payments |
| MCP | `probbit_persona_turn` and `probbit_live_event` refuse readings and `cancel` |

`prove --witness` is independent of this RFC and gets its own proposal.

## Migration

Existing personas, states and strands are untouched. A 0.8 strand continues under 0.9.0 with its header unchanged, and 0.9.0
appends the lines 0.8 would. Adding a block changes the persona digest: a new individual on a new strand (retire the old one
with `live control retire`). A 0.8 binary refuses the new blocks as unknown fields and never replays them wrongly.

## Test plan

- Unit: closed form (n identical readings: mean_n = x + (1 − a)^n (mean_0 − x), within 1e-5 before caps); habituation (zd
  at reading 20 ≤ 0.35 × reading 1; 0.30 at defaults); surprise ratio ≥ 2 (3.4); 100,000 adversarial readings inside the drift
  cap; var_cap; eligibility; every lint and refusal row.
- Property: P0 over every golden and a recorded corpus of 7,200 shipped-persona turns; refusal over 40 individuals × 300
  random events mixing sources, readings and payments; replay, and a life split across runs equal to one run; genes
  unchanged when an unrelated sense is edited.
- Fuzz/prove: the example sensed persona's rules held or proved over seeds 0–99; fuzz with readings and payments finds nothing.
- Acceptance: `bench/individuality/` (twins, habituation and named past, credit accuracy, replay, habit breaks), each metric
  with a pass bar.

## Failure modes and guards

| failure | guard |
|---|---|
| A normal is taught: a chronic failure drifts into "normal" | drift cap anchored to the prior (push floor zd ≈ 1.0 at defaults); habits read raw readings; `monitor` and `verify` flag a mean that moved over drift_cap/2 within 50 events |
| Alternating extremes make every reading look ordinary | var_cap: the spread at most doubles |
| The agent supplies its own readings | `self` refused; MCP refuses readings; hosts set `src` in their own code |
| A forged env label | the honest limit: a source is the host's word, logged and replayed, not shown true; Design C covers rewards, readings stay unauthenticated |
| Unit drift (ms vs s) | unit in the digest; out-of-range readings refused |
| A stale reward pays an older self | eligibility; expiry after 4 half-lives |
| Outcomes never settled | `max_open`, expiry, `open` in every stance |
| A duplicated or replayed payment | the decision is closed: B-R2 |
| The agent cancels to dodge a correction | `cancel` needs env or human; MCP refuses it |
| A restored state repeats turn numbers | `paid.stance` names the paid stance's digest for the host to compare |

## Alternatives considered

- Senses as plain number inputs (0.8): no normal, no habituation, no per-reading provenance, no past in `why`.
- Host-computed z: the normal leaves the log, and a host can hand in a pre-judged surprise.
- Clock decay of normals: silence would erase the durable difference.
- A familiarity ramp for new individuals: the declared prior is already the starting normal; widen `prior.var` instead.
- Every turn opens a decision: unaddressed feedback would almost always be ambiguous; `await` keeps 0.8 hosts unchanged.
- Credit spread over recent stances without addressing: still misattributes.
- Refusing `await` at `max_open`: blocks the agent's turn on bookkeeping; eviction is recorded instead.

## Open questions for contributors

1. Should open decisions survive pause and resume? Closing them matches 0.8, but outcomes landing during a pause are lost.
2. Should `for` carry the stance digest (`for: {turn, stance}`) so a restored state cannot pay a different decision?
3. Should eligibility decay by events, by clock hours, or not at all until expiry?
4. Should `human` readings teach a normal, or only move the stance?
5. Should 0.9.0 ship `persona migrate OLD NEW --state S` to carry moods, history and learned weights into a persona that only
   adds blocks?
