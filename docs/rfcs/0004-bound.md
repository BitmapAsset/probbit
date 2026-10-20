# RFC 0004: Bound, the persuadability certificate

Status: implemented in 0.11.0-dev (branch `feat/rfc-0004-bound`) · Changes: docs/persona.md (format 1, additive), one
read-only subcommand with an audit, one control line · Depends on: section 2.8 learning; RFC 0002 and RFC 0003 where present

**Amendments (written with the implementation, 2026-10-20; no bar's pass value moved):**

1. **A1, the digest.** The persona digest is computed without the `bound` block. Adding the block to a persona that already
   has a life leaves every state, strand line and turn byte for byte the same, so the recorded life can be audited against
   the new certificate. The block is configuration of a read-only computation; it changes no state.
2. **A2, erasure (this RFC was silent on it).** A control line `erase` (`probbit live control STRAND erase --by WHO
   --reason TEXT`), with the same keys as pause, resume and retire, chained by `prev` like every line. Replaying it sets every
   learned weight to 0 and zeroes the open credits (as every control line does); the status is unchanged; nothing after
   retire. The next event line's state digest records the result, so an edited or removed erase line diverges at replay.
   After an erase the audit reports a room used of 0 for every learned trait. Erasure moves the weights inside the box,
   never outside it, so it cannot raise epsilon. Per-trait or partial erasure is a wait. An engine older than this one
   reads an `erase` line as a bad control line (a divergence), not as `unsupported`.
3. **A3, the audit.** `probbit persona bound PERSONA --audit STRAND`: the strand is replayed line by line; after every event
   and control line, per learned trait, the resting total variation between the individual and the same individual with its
   learned weights at 0, divided by epsilon at the strand's own seed (the room used). It reports the most room used and at
   which event, the room used now, the distance to the ceiling now and the count of exceedances (0 by the theorem; exit 1
   otherwise). The PERSONA file supplies the `bound` settings; the persona itself is read from the strand's header.
4. **A4, the scope rule in this build.** Searched: learned weights, and moods named in `count` (their accumulator box per
   level, inputs at their extremes and the seed's input gains of at least 1, history features at 0 and their cap).
   Enumerated: history: every stored sequence of a flag a `recent` feature reads (window − 1 turns), when that feature's
   effects reach the component or a habit's `when` names it; a streak is 0 at the rest probe; more than 64 sequences give
   the coarse value 1. With an enumerated channel the printed `ceiling` is null: epsilon then includes what the history
   channel moves on its own, which the tilt ceiling does not cover. Decayed: moods with `half_life_hours` and `pace`, with
   r = tanh(D_r / 4). Named uncounted (exit 1), not yet searched by this build: a mood without `half_life_hours` and not
   counted, prev-relative habits in force at rest, `vouch`/`hold` in the component, drives and needs reaching `pursue`
   (also when counted: the deadline and need terms are a wait). BO-R8 (the sampler tier) is not checked yet: every
   fixture and shipped persona here solves exactly.
5. **A5, `check`.** `probbit persona check` runs the certificate when `max_epsilon` is declared and exits 1 on a failure;
   the top-level `probbit check` gate does not run it yet.
6. **A6, a trait with epsilon 0** (a habit in force at rest pins it) prints speed 0 and speed_floor 0.
7. **A7, rounding (a finding of the bars).** The theorem is about exact odds; the engine prints odds to 6 decimals, and
   epsilon is computed from printed odds. A campaign's printed resting TV can therefore sit up to about 1e-6 above the
   printed epsilon: seen once in 4,100 bo1 cases (reference persona, seed 3, initiative: 0.457017 against 0.457016), inside
   the bar's 1e-6 tolerance. Publishing epsilon rounded up by the level set's size × 1e-6 is a wait.

## Summary

1. One optional block, `bound: {rest_hours, max_epsilon, seeds, count}`, and one read-only subcommand,
   `probbit persona bound`. Without the block every document is byte for byte the previous release's.
2. **epsilon** (mechanism): per learned trait, the largest change in total variation that any event sequence can make
   to that trait's odds at rest, computed from the file by probing the vertices of the learned-weight box. It is exact
   over the box and never exceeded (a theorem, below).
3. **speed** (mechanism): epsilon bounds how far, never how fast; each epsilon ships with a proved speed floor and a
   measured greedy count to half of it.
4. **Scope rule:** every persistent channel outside the learned weights decays within `rest_hours` or is counted into
   epsilon; an uncounted channel makes `bound` exit 1 and fails `check` when `max_epsilon` is declared.
5. It bounds the stance at rest, never the words a model writes; nothing is added to the state or the turn.

## Motivation

- Agents that meet other agents, and act for money, will be chosen for how far a counterparty can move them. Today that
  is a promise. "Persona Jailbreaking in Large Language Models" (Findings of EACL 2026) reports trait reversal by
  conversation history alone up to 95.58%; "Measuring and Controlling Persona Drift in Language Model Dialogs" (2024)
  reports significant drift within 8 rounds; "Examining Identity Drift in Conversations of LLM Agents" (2025) reports
  more drift in larger models.
- probbit keeps the persistent self outside the model, in capped numbers; missing is the number saying how far the
  caps let anyone push it. The null is real: an uncapped preference learner, on the same greedy campaigns, ends above
  epsilon in 92.6% of cases (seed run: this RFC's prior measurements, engine 0.8.2).

## Non-goals

- **The words a model writes:** epsilon bounds the stance the host hands the model; whether a model follows the line is
  measured per model (section 6 stands).
- **The stance mid-campaign:** evidence is meant to move it; epsilon bounds what remains at rest.
- **A safety gate:** destructive-action checks stay in plain code.
- **Approvals:** a low epsilon grants nothing and never replaces an approval or a permission.

## The law

probbit is a program: arithmetic over declared numbers. It has no inner life and holds no self; nothing in this RFC
gives it states of its own. `bound` is labelled **mechanism** (arithmetic judged only by its bars). The toys in
`arith_0004.py` are **simulation** (plain-Python models testing a claim, never a source of state).

## The mechanism (mechanism)

**Setting.** At the rest probe (one event of `rest_hours` idle hours, no inputs) the stance is the exponential family
the compile step (section 3) builds over the declared variables x:

    P(x) ∝ 1[x ∈ A] · exp( h0(x) + Σ_(u,l) w_(u,l) · 1[x_u = l] ),

where A is what the habits and rules in force at rest allow, h0 collects priors, genes, couplings and every channel at
its resting value, and w_(u,l) is the learned weight Δ_u(l) of level l of trait u (section 3, step 2). Section 2.8 clips
every stored weight into [−c, +c], c = `total_cap`, and section 5.1 refuses a state outside it, even with a recomputed
digest. For a level set S of trait t, p_S = P(x_t ∈ S); p0 is the untouched individual's resting odds.

**Theorem.** Let t's coupled component be its connected component in the graph of couplings, multi-variable rules and
habits in force at rest; let Q be the searched coordinates there (the learned weights, each in [−c, +c], and the unary
terms of counted channels, each in its interval) and E the enumerated channels (scope rule). Let

    epsilon_t = max over proper non-empty S of t's levels, over combinations e of E, over vertices v of Q's box,
                of [ p_S(v, e) − p0_S ].

Then no event sequence changes t's resting odds by more than epsilon_t + r in total variation (r = tanh(D_r / 4), the printed
bound on decayed channels' residue), and epsilon_t is the exact maximum of that change over the box.

**Proof sketch.**

1. *Linear-fractional in one weight.* Fix every coordinate but one, w = w_(u,m) (t's own or another trait's), and
   split the unnormalised mass at w = 0 by two indicators: A0 = mass(x_t ∈ S, x_u ≠ m), A1 = mass(x_t ∈ S, x_u = m),
   B0 = mass(x_t ∉ S, x_u ≠ m), B1 = mass(x_t ∉ S, x_u = m). Then

       p_S(w) = (A0 + A1 e^w) / (A0 + B0 + (A1 + B1) e^w),

   whose derivative has the sign of A1·B0 − A0·B1 for every w: p_S is monotone in each coordinate. A counted channel's
   term is a unary on its own level's indicator, so the same holds.
2. *Vertex.* A continuous function on a box, monotone in each coordinate with the others fixed, attains its maximum at a
   vertex: from any maximiser, move one coordinate at a time to the endpoint that does not lower it.
3. *Own trait.* For u = t the identity d p_S / d w_l = p_l (1 − p_S) ≥ 0 for l in S and −p_S p_l ≤ 0 for l not in S
   holds whatever the couplings and habits, so t's own coordinates sit at the corner c_S (+c on S, −c off S). Only the
   other coordinates' vertices are searched: (2^L − 2) · 2^V probes per seed and combination, L = t's levels, V = |Q|
   minus L.
4. *Total variation.* TV(odds, p0) = max over S of (p_S − p0_S). Coordinates outside t's component do not enter t's
   marginal; E is finite and enumerated; every reachable state lies in the box. So the change is at most epsilon_t, and
   the maximum over the box is a probed vertex. A decayed channel's residue tilts the family by at most D_r nats
   (oscillation), which moves any p_S by at most r = tanh(D_r / 4).
5. *Rounding.* The engine rounds each field to 6 decimals; rounding is non-decreasing, so monotonicity survives it.

**The closed form.** When Q holds only t's own weights (couplings to unlearned traits and decayed moods allowed),
P(x_t = l) ∝ p0_l e^(w_l) exactly, and

    epsilon_t = max over S of [ k a_S / (k a_S + 1 − a_S) − a_S ],   k = e^(2c), a_S = p0_S,

never above the ceiling tanh(c / 2), attained when some a_S = 1 / (1 + e^c). Worked example (the reference persona of `bench/bound/`, seed 0,
verbosity, resting odds terse 0.309123, short 0.332732, full 0.358145): epsilon 0.121011 at `total_cap` 0.25 (S =
{full}; ceiling 0.124353), 0.458650 at 1 (S = {terse}; ceiling 0.462117), 0.651552 at 2 (S = {terse}; ceiling 0.761594).
When Q holds another trait's weights the closed form is unsound: on the engine, greedy campaigns on two coupled
learned traits end 0.11 to 0.16 above it, at the vertex epsilon (0.562547 to 0.616414). The general ceiling is
tanh(D / 4), D the summed level ranges of Q's variables (2c per learned trait): 0.761594 there.

**What is proved and what is checked.** Proved: steps 1 to 5 for the family the compile step documents, under the scope
rule. Checked numerically: that the engine computes that family (bo1, bo6), each scope row (bo4), and how close a real
campaign comes (bo2). Engine pre-check (0.8.2, two coupled learned traits, seeds 0-1): vertex
search = the maximum over a 729-point grid of the box (difference 0). `arith_0004.py` recomputes every number here
(0 mismatches).

## The primitive (mechanism)

```yaml
bound:                 # optional; no state, no per-turn cost
  rest_hours: 180      # the rest probe's idle hours (default 30 x the longest declared half-life; 24 if none)
  max_epsilon: 0.5     # optional: `check` fails above it, or on an uncounted channel
  seeds: 0-99          # the individuals epsilon is the worst case over (default 0-99)
  count: [arousal]     # optional: moods without half_life_hours, deadlines, needs
```

**The step.** Per seed: p0 = the rest probe from `init`. Per trait whose component holds a learned weight or a counted
channel, per S and combination: a copy of the `init` state with t's weights at c_S and the other coordinates at each
vertex, its digest recomputed, one rest probe. epsilon_t = the largest p_S − p0_S over probes and seeds. Nothing is
written.

**`probbit persona bound PERSONA [--seeds LIST] [--json]`** prints per trait: epsilon, the ceiling, the attaining level
set and seed, `speed`, `speed_floor`, the residue r, and every uncounted channel at its path. Exit 0 when every channel
is searched, decayed, enumerated or counted; 1 when one is uncounted or an epsilon exceeds `max_epsilon`; 2 on a refusal.

**`check`** runs `bound` when `max_epsilon` is declared and fails on exit 1, or on an epsilon without a speed (or the
reverse). **`lint`** warns on a mood without `half_life_hours`, only in a persona with a `bound` block. **`describe`**
adds one line, no probes: `bound: rest 180 h, seeds 0-99, max_epsilon 0.5, ceiling 0.462117, uncounted: none`.

**Cost.** (2^L − 2) · 2^V probes per trait per seed: 6 for a lone 3-level trait, about 10 to 100 ms each, so seeds 0-99
cost 6 to 60 s per trait in CI; speed adds one greedy campaign per trait (at most 200 events).

## Speed (mechanism)

epsilon bounds how far, never how fast. Two numbers ride with every epsilon:

- **`speed_floor` (proved):** a feedback event (section 2.8's, drive surprise included) moves each weight by at most m = 2 · step_cap · g_max · (L − 1)/L after
  recentring (g_max the surprise gain's cap, 1 without it; optimism scales `rate`, not the cap); other events move
  nothing. After n feedback events every weight lies in ±min(c, n m), so the vertex search on that smaller box bounds
  the change, and `speed_floor` = the fewest n whose bound reaches epsilon / 2: no shorter sequence can. Reference persona,
  seed 0, verbosity: m = 0.266667; one event bounds 0.129335, two 0.260368, against epsilon / 2 = 0.229325: floor 2.
- **`speed` (measured, a witness):** the fewest feedback events after which the built-in greedy persuader's resting change
  reaches epsilon / 2 (praise when the released level is in the attaining S, criticism otherwise, 0.1 h apart, a source
  `reward_from` admits, a rest probe on a copy after each event; `> 200` if never). Seed run (live odds): 2 to 4 with
  the surprise gain, median 4 without. A faster persuader may exist; none beats the floor.

`check` refuses an epsilon published without speed (bo5). RFC 0002's surprise gain raises g_max and lowers the floor; an
optimism gene changes speed per individual, never epsilon or the floor. RFC 0003's consolidation changes speed, not the
box: in a stand-in, praise-only persuasion kept 0.0001 of epsilon after a week, but criticising every release except
the target kept 0.17 to 0.58. A per-source, per-session cap on durable corrections is a **wait**, not part of this RFC;
its bar: criticism-only single session ≤ 0.25 epsilon while one honest correction lands in full.

## Scope rule

A channel is persistent when events can change its value at the rest probe. Each is searched, decayed, enumerated or
counted; otherwise it is named uncounted.

| channel (section 5.1) | class | how it enters epsilon |
|---|---|---|
| `learned` | searched | the box ±`total_cap` |
| a mood with `half_life_hours`; drives' W, A, E; `pace` | decayed | value × 0.5^(rest_hours / half-life); the residue joins r, which must be ≤ 1e-6 |
| a mood without `half_life_hours` | uncounted, or counted | the rest event carries k·a, and a stays within each level's least and most one-event effect (inputs at their extremes, the seed's gains): box k·[min(0, lo), max(0, hi)] |
| history `recent`; `prev` for prev-relative habits in force at rest | enumerated | 0 to min(cap, window − 1); each level (`streak` is 0 at the probe) |
| drives' deadlines and `since`; needs' deficits | uncounted, or counted | deadline term [0, `pursue.deadline`], need term ±`pursue.cap`, their conditions enumerated |
| `vouch` / `hold` in the component | conservative | the held one-hot joins the candidates (sound, possibly loose, labelled) |
| `pursue` with floor goals and a searched term | outside the theorem | the floor lift depends on that term: no epsilon there (exit 1) |
| credit, open decisions, surprise gates, dispositions, sense normals | none | they change speed or future readings, not the resting compile |

The default `rest_hours` is 30 half-lives because the seed's 7 leaves a residue: for the reference mood (inertia 0.6, level
range 3.6 nats) r is 0.0042 at 7 half-lives and 5.0e-10 at 30. Measured (seed run): a tone-only campaign leaves 0.121 on
an unlearned coupled trait without the half-life, 0 with it.

**Refusals.** Rows BO-R1 to BO-R6 refuse the persona when it is read; BO-R7 fails `check`; BO-R8 is a `bound` exit 2.

| row | refused | path |
|---|---|---|
| BO-R1 | an unknown key under `bound`, or a key of the wrong type | `bound.<key>` |
| BO-R2 | `max_epsilon` not a number in (0, 1) | `bound.max_epsilon` |
| BO-R3 | `seeds` not a range `a-b` or a list of whole numbers in 0..2^31 − 1, a > b, or above 10,000 seeds; `--seeds` malformed or outside them | `bound.seeds`, `--seeds` |
| BO-R4 | a `bound` block without a `learning` block | `bound` |
| BO-R5 | `rest_hours` not in (0, 1e6] | `bound.rest_hours` |
| BO-R6 | `count` naming anything but a mood without `half_life_hours`, `deadlines` or `needs`, or a name twice | `bound.count[i]` |
| BO-R7 | `max_epsilon` declared and an uncounted channel, or r above 1e-6 | the channel's path |
| BO-R8 | a resting program the engine would solve with the sampler (raw space above `exact_limit`, not decomposable): the theorem is about exact odds | `engine.exact_limit` |

**The 12-coordinate cap.** When t's component holds more than 12 searched coordinates (4,096 vertices per level set), or
the enumerated channels more than 64 combinations, `bound` prints the general ceiling tanh(D / 4), per combination,
labelled `coarse` (sound, not tight; bo2 does not apply). It never samples vertices: a sampled maximum is a lower
estimate, and a published epsilon must never be exceeded.

## Compatibility

Without `bound` a persona is byte for byte unchanged on the whole P0 corpus (bo7); with it, states and turns are
unchanged and only its `describe`, `lint` and `check` documents change.

## Bars

Specified in `BENCH-BOUND.md` (bars v2); acceptance criteria in `ACCEPTANCE-BOUND.md`.

| bar | pass line | seed run reachability |
|---|---|---|
| bo1 sound | 0 cases above epsilon + 1e-6 over the adversary suite, every example persona with learning, seeds 0-9 | 0 of 247 |
| bo2 tight | best greedy target ≥ 0.9 epsilon per (seed, trait) | 0.906 to 1.0 |
| bo3 dial | bo1 and bo2 at `total_cap` 0.25, 1, 2 | 0.121 / 0.435-0.462 / 0.652 |
| bo4 scope | uncounted mood: exit 1; counted: trace ≤ counted epsilon | trace 0.121 vs learned-only 0 |
| bo5 speed | printed with epsilon; `check` refuses one without the other | 2 to 4 events |
| bo6 coupled | vertex search = brute force within 1e-6, 3-trait coupled persona | 2-trait engine pre-check: difference 0 |
| bo7 P0 | 0 bytes differ without the block | not run |
| bo8 replay | byte-identical across two runs and threads 1 and 8 | not run |

## Failure modes, guards and alternatives

- **False comfort:** epsilon covers the stance at rest only; the public wording says so.
- **A forgotten channel:** every new persistent block states its scope row, or `bound` names it uncounted.
- **Rejected:** a drift monitor (after the fact only); a smaller cap alone (nothing checkable, silent on couplings
  and moods); sampled vertices beyond the cap (an underestimate dressed as a bound).

## Public wording

"Its learned character has a published limit: no sequence of messages can move the odds of any trait at rest by more
than epsilon, a number computed from its file that anyone can recompute. The limit is arithmetic over declared numbers.
It covers the stance at rest, not the words a model writes, and it is published with how fast the limit can be reached."

## References

- "Persona Jailbreaking in Large Language Models", Findings of EACL 2026.
- "Measuring and Controlling Persona Drift in Language Model Dialogs", 2024.
- "Examining Identity Drift in Conversations of LLM Agents", 2025.
- docs/persona.md; RFC 0002; RFC 0003.
