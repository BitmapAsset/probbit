# RFC 0014: Integrity, the drift from the declared self

Status: implemented (library and strand line), off by default; engine 0.14.0-dev on a draft PR · Changes: docs/persona.md
(the `identity` block's self keys, additive and outside the digest), one engine-gated field on event lines · Depends on:
RFC 0004 (the allowance is its epsilon), RFC 0013 (the Mirror tab, where the per-trait table is to be shown)

## Summary

Per event, one exact number and its decomposition, from the state and the persona document alone: how far the individual's
resting odds have moved from those of the self its persona declares, and how much of the persuadability certificate's
allowance that movement uses. Labelled **mechanism**: arithmetic over declared numbers, replayable from the strand,
recomputed by `live verify`. It adds nothing to the state and changes no turn.

## The declaration

Four optional keys in the existing `identity` block (the "self keys"):

- `keep: [habit ids]`: the habits that are identity-bearing.
- `warn: <share>`: a share of the allowance in (0, 1], default 0.8.
- `cue: <text>`: a short line (1-120 characters) in the persona's own words, handed to the host when a trait reaches `warn`.
  The engine never writes cue text of its own.
- `traits: [trait ids]`: the traits measured; default the learned traits (`learning.traits`), else every trait.

Declaring any of them turns integrity on. The keys are read against the persona built without them and stay out of the
persona digest (as RFC 0004's `bound`, amendment A1): every state is byte for byte as without them. A persona that declares
none of them emits nothing, so every existing strand, stance and corpus is unchanged.

## The numbers

- **The rest probe** (RFC 0004's): one event of `bound.rest_hours` idle hours (default 30 × the longest declared half-life,
  24 without one), no inputs.
- **The rested individual**: the current state with the channels of the hour at the fresh individual's values (moods with
  `half_life_hours`, the pace average, the load, the rest pressure, the eligibility trace, the credit awaiting feedback and
  its open decisions, the levels and stance taken last, the agenda's step) and everything that persists as it is (learned
  deltas, history, moods without a half-life, drives, senses, the surprise gates, genes). The credit is of the hour: it is
  what the next feedback would act on, zeroed by every control line, and a rest probe has no feedback.
- **drift**, per trait: the total variation, half the L1 distance over the trait's levels, between the probe's odds for the
  rested individual and for the fresh individual of the same seed (the declared persona at birth); `drift` at the top is the
  sum over the traits. The fresh individual is its own rested state, so its drift is 0 exactly. Total variation is chosen over the
  plain L1 because it is the certificate's own unit: `bound_used` is then a ratio of like quantities, and the theorem of
  RFC 0004 reads directly as `bound_used ≤ 1` (up to the residue r of decayed channels and the 1e-6 rounding of amendment A7).
- **allowance**, per trait: epsilon at the individual's own seed, computed once per seed from the persona's `bound` block
  (the same vertex search as `persona bound --audit`); null for a trait that is not learned or without a `bound` block.
- **bound_used** = drift / allowance (0 without an allowance). **used** (the dial) = the summed drift over the traits with an
  allowance, divided by the summed allowance (0 without one).
- **crossed**: the traits whose `bound_used` reached `warn`; **cue**: `identity.cue` when `crossed` is not empty, else null.
- **habits_held**: of the `keep` habits, `held` (bound this event) and `not_fired`.

Every number is rounded to 6 decimals.

## Where it appears

- The live stance document gets `integrity`; the event line carries the same document as its `integrity` field. Both stance
  digests leave it out (it is computed after the turn, from the state the turn produced, so the state's remembered stance
  digest and `for` credit binding are unchanged).
- `live verify` recomputes it on every event line and refuses an edited value naming the line ("the integrity differs").
  `resume` takes it as written, as it takes `deviation` and `glyph`.
- Engine-gated: a persona with self keys is refused by an engine before 0.14.0 as an unknown field, which `live verify`
  reports as `unsupported` (exit 5), never as a divergence.

## Cost

One extra exact solve per event at most, the rest probe of the rested individual, cached on the rested state's body without
`turn` and `digest` (a rested state has no open decision, so nothing in it ages in turns); the fresh individual's
probe and the allowances once per seed. Because the rested state leaves out the channels of the hour, the cache hits on every
event that moves only moods, the last stance or the agenda, and misses when something that persists moved (a learning step,
a credit, a history window, a drive). Measured (docs/embedding.md "Integrity on a body"): on a body's recorded day of 1,160
events it still missed on every event (a persisting channel moves each event while the drift stays 0.000000), +40 % of a
turn on the host and a turn's stack (about 21 KB on the `nostd` build) per miss. The bar of this RFC (2 % of a turn, 8 KB)
is not met; a cache keyed on the rest probe's program, as the expectation's cache keys its members, is the next step.

## Not in this build

- The cost bar: a rest probe whose cache hits on a body's ordinary event (see Cost).

- Appending the cue to the stance's `line` (the line is inside the stance digest the state remembers); the host appends
  `integrity.cue` itself.
- The monitor's integrity dial and the Mirror's per-trait table.
