# Pre-registration

Extracted verbatim from the lab brief of 2026-10-21 (and, where the brief points at them, the run log's bars block and the
RFC draft); the brief's protocol and infrastructure lines (paths, run instructions, monitoring) are omitted. Names of
recorded lanes of a private deployment are replaced in place by a bracketed note; nothing else is changed. The bars
below were written before the run they judge and were not edited after.

## From the lab brief, section "Pre-registration"

## Pre-registration (copy this section VERBATIM into LOOP.md §1 before the first registered run; never edit it after; deviations go in LOOP.md §5)

Arms, all evaluated on the same 1,000 episodes:
- `A`: reproduction (expect median +0.8394; survival 0.9220).
- `A-t0`: tonic 0 (expect median +0.1199).
- `T0-P0`: tonic 0, phasic 0.
- `T0-D0`: tonic 0, drives.valence off.
- `T0-S0`: tonic 0, fail_valence 0, damage_valence 0.
- `T0-all`: tonic 0, phasic 0, drives.valence off, fail_valence 0, damage_valence 0.
- `A-P0`: tonic 0.8, phasic 0 (does the main clause depend on the phasic term).
- `A-S0`: tonic 0.8, fail_valence 0, damage_valence 0.

Measured per arm: median per-episode Spearman(valence, distance) with null (i) 97.5th percentile, share of episodes
≥ 0.3, survival, prioritisation accuracy, median Spearman(pressure, −distance). All in `tables/arms_v2res.tsv` and
`tables/tracking_v2res.tsv`, with the v-run tables' columns plus the knob columns.

Bars:
- R1 reproduction: the `A` and `A-t0` rows equal the v-run's `tracking_vrun.tsv` / `arms_vrun.tsv` values to every
  printed decimal, persona hashes equal. If R1 fails, stop the matrix, write the diff to LOOP.md §5 and `BLOCKED`.
- R2 decision path untouched: every arm's survival, prioritisation accuracy and pressure median equal `A`'s exactly
  (valence is off the policy's path). A difference is a finding, not a failure: report it.
- R3 attribution: name, before reading any `T0-*` number, the three candidate carriers (phasic, drives.valence, sense
  effects). The carrier is the smallest set of knobs whose removal from `A-t0` gives a median ≤ +0.10; `T0-all` is
  expected at the null level (|median| ≤ null (i) 97.5th percentile + 0.02). If `T0-all` stays above +0.10, the residual
  comes from something not on the list (valence inertia against the world's autocorrelation, the measurement's level-index
  mapping): then run one extra pre-named arm, `T0-all-i0` (`T0-all` with the valence mood's `inertia` 0), and report.
- R4 one pass: one run per arm, judged as registered; nothing is tuned; a failed bar stays failed.
