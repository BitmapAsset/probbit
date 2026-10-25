# Pre-registration

Extracted verbatim from the lab brief of 2026-10-20 (and, where the brief points at them, the run log's bars block and the
RFC draft); the brief's protocol and infrastructure lines (paths, run instructions, monitoring) are omitted. Names of
recorded lanes of a private deployment are replaced in place by a bracketed note; nothing else is changed. The bars
below were written before the run they judge and were not edited after.

## From the lab brief, section "Bars"

## Bars (write `LOOP.md` with these BEFORE any run; never adjust after)
- n1: tracking — median per-episode Spearman(pressure, distance-to-bound of the worst resource) ≥ 0.3 in arm A (report the
  valence correlation too, no bar).
- n2: survival — arm A survival rate exceeds arm B's with a 95% bootstrap CI over episodes excluding 0 (1,000 episodes per
  arm, seeds fixed), and A's collected value ≥ 0.8 × C's.
- n3: prioritisation accuracy in arm A ≥ 0.7 and above arm B's with CI.
- n4: the loop's best configuration beats its own baseline on held-out seeds, or the loop reports no improvement.
- n5: replayability — `experiments/rerun.sh` reproduces every table byte-identically, twice; strands of the first 20
  episodes verify with `probbit live verify`.
- n6: the engine binary is unmodified (clone head sha + binary sha recorded); every engine field used is named with its
  source line in the docs.
Report failed bars as failed with the number. A null (A ≈ B) is a result: it would say the shipped state does not yet carry
need information usefully, which is what RFC 0008 must then add.
