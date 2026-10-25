# Pre-registration

Extracted verbatim from the lab brief of 2026-10-21 (and, where the brief points at them, the run log's bars block and the
RFC draft); the brief's protocol and infrastructure lines (paths, run instructions, monitoring) are omitted. Names of
recorded lanes of a private deployment are replaced in place by a bracketed note; nothing else is changed. The bars
below were written before the run they judge and were not edited after.

## From the lab brief, section "The pre-registered v-run"

## The pre-registered v-run (copy §3 of OPUS-JUDGE.md into LOOP.md VERBATIM before the first run; never edit it after)

Copy `../i3sim-needs-viability-20261020/experiments/` into `experiments/` here. Allowed edits, and only these: the persona
gains the RFC 0008 keys at the fixed values (pressure gain 2.0 linear; valence tonic 0.8, phasic 0.6; `viable.events 4`;
`viable.at_most = 1 − lo` per resource: 0.85 / 0.75 / 0.80) and loses every habit on `need.<id>.deficit`; the host policy
reads `viability.worst` and the need margins instead of host band widths; add arm A-t0 (tonic = 0). Binary = your PR head
(record its sha256). World = the I3-sim BASELINE world (`config_baseline.json`), not the loop-mutated one. Evaluation seeds
1000-1999, 1,000 episodes per arm, 400 steps, seed 1 persona. Bars V1–V7 exactly as registered; nulls (i) time-shuffle 100
permutations per episode, (ii) arm B, (iii) arm A-t0. One run, judged as registered; a failed bar stays failed and is
reported as failed. One re-run (-b) only for a harness bug named in LOOP.md BEFORE the bar's number is read. No gain, world
or seed change after any evaluation-seed number is seen. `experiments/rerun.sh` twice → tables byte-identical; strands
verify with `probbit live verify` (≥ 240 plus all 1,000 A strands), 0 failed. H1 ($0 string check): `block` contains every
declared trait and mood exactly once on 100% of replayed events of the five lanes ([names of recorded lanes of a private deployment omitted]
from read-only strand copies; [names of recorded lanes of a private deployment omitted]).
H2 and H3 are not part of this wave.

## From the run log, section 1: the bars block copied verbatim from the judge's verdict before any v-run

## 1. Pre-registered bars (OPUS-JUDGE.md §3, copied verbatim before any v-run; not edited after)

## 3. Pre-registered bars (copy into the v-run BRIEF unchanged)

```
RUN: I3-sim-v (viability follow-on). Engine = the 0.11 PR head (record sha256 of the binary). Harness =
i3sim-needs-viability-20261020/experiments/ copied, unchanged except: persona gains the RFC 0008 keys and loses every
habit on need.<id>.deficit; host policy reads viability.worst / need margins instead of host band widths.
World = the BASELINE world of I3-sim (not the loop-mutated world). Persona seed 1. Steps 400. elapsed_hours 0.25/step.
SAMPLE: evaluation seeds 1000-1999, 1,000 episodes per arm; arms A (stance-coupled), B (constant resting stance),
C (threshold heuristic on world levels), D (random), A-t0 (A with needs.valence.tonic = 0). No tuning runs; gains are
fixed BEFORE the run at the RFC values: needs.pressure {gain 2.0, shape linear}; needs.valence {tonic 0.8, phasic 0.6};
viable.events 4; viable.at_most = 1 - lo per resource (0.85 / 0.75 / 0.80 for lo 0.15 / 0.25 / 0.20).
NULLS: (i) time-shuffle: per episode, permute the valence (or pressure) series 100 times, take the median over episodes
of each shuffle, report the 97.5th percentile of those 100 medians; (ii) arm B for survival and prioritisation;
(iii) arm A-t0 for the valence mechanism.
BARS (all judged on A unless named):
 V1 pressure, graded, no deficit habits: median per-episode Spearman(pressure, -distance) >= 0.30 AND > null (i).
 V2 valence registers deviation (HEADLINE): median per-episode Spearman(valence, distance) >= +0.30 AND > null (i)
    AND A-t0 median <= +0.10. (Unmodified-engine reference: -0.149.)
 V3 the engine names the worst need: |strand margin - world margin| <= 1e-6 and strand viability.worst = world's
    worst-margin resource on 100% of events of all A episodes; prioritisation accuracy >= 0.9799 (reference 0.9999);
    host config holds no band width.
 V4 decisions still carried: A - B survival, 95% bootstrap CI (10,000 resamples, seed 20261021) lower bound > 0;
    A survival >= 0.515 (reference 0.559, minus the A-row CI half-width 0.044).
 V5 no regression: personas without the new keys replay every real strand ([names of recorded lanes of a private deployment omitted]
   ) and the 240 I3-sim strands to stance documents byte-identical to 0.10.1; cargo test all green.
 V6 bound holds: the E1 suite (247 campaigns) gives 0 exceedances of epsilon with need_pressure, need_tonic and
    need_phasic boxed in prove; live audit 0 exceedances on both lanes.
 V7 replay: experiments/rerun.sh twice -> all tables byte-identical; >= 240 strands (20 episodes x 4 arms x 3 configs)
    plus all 1,000 A strands verify with probbit live verify, 0 failed.
HOST ACCEPTANCE (not the discovery; gates the install):
 H1 stance.block contains every declared trait and mood exactly once as "name level" on 100% of replayed events of the
    five lanes above ($0, string check).
 H2 [names of recorded lanes of a private deployment omitted] (a read-back score on a private deployment; not part of this kit)
    [names of recorded lanes of a private deployment omitted]
 H3 48 h after install, per lane: live verify passes; margin_min_seen and lost_at reported. Descriptive, no bar.
RERUN RULE: one run, judged as registered. A failed bar stays failed and is reported as failed. One re-run (-b) only for
a harness bug named in the log BEFORE the bar's number is read; diff logged; no gain, world or seed change after any
evaluation-seed number is seen. Law sweep over every output file: 0 hits.
```


## From the RFC 0008 draft, section "Bars for the follow-on I3-sim run"

## Bars for the follow-on I3-sim run (to be fixed before it runs)

- v1: with `needs.pressure`, median Spearman(pressure, −distance) in A ≥ 0.3 with **no** habits on deficits.
- v2: with `needs.valence`, median Spearman(valence, distance) in A ≥ 0.3 (today −0.149).
- v3: the host policy reads `viability.worst` instead of host band widths; prioritisation accuracy within 0.02 of today's.
- v4: A − B survival CI still excludes 0; every strand verifies; documents of a persona without the keys are byte-identical
  to 0.10.1.
