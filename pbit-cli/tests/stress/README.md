# Frozen stress corpus (R19 P0.3)

Inputs on which the sampled gate has released wrong answers (R19.1: every family but chain100 red). Since R19.2 the
collective moves (`--collective on`, the default) answer all seven correctly at defaults and at fixed work; `--collective off`
reproduces the failures. Frozen: never edit or regenerate them, and never tune a
threshold on them (see CONTRIBUTING). Each has an oracle that does NOT use pbit's own inference (`pbit-cli/tests/stress.rs`).

| File | Front-end | Source | Oracle |
|---|---|---|---|
| `asymmetric-router32-w0.2-h0.03.json` | `pbit decide` | External review 2026-09-30, E2 | occupancy count DP over k = #tasks on B (closed form: sum_k C(32,k) exp(0.2 [C(k,2)+C(32-k,2)] + h k)); P(B) = 0.8698133938587349 at h = 0.06 |
| `asymmetric-router32-w0.2-h0.06.json` | `pbit decide` | Review E2/E3 (12/100 and 14/100 seeds: whole answers passed the gate, all false) | same |
| `heterogeneous-router32.json` | `pbit decide` | Review E5 (2/16 false whole answers, 64 wrong released tasks) | count DP with per-task scores (elementary symmetric polynomials, O(n^3)) |
| `ferro12.json` | `pbit run --op sample` | Review E4 (complete graph, Potts 10: zero bound, true error 0.5) | brute-force enumeration (2^12) |
| `ferro12-w0.5-redundantcaps.json`, `ferro12-w1.0-redundantcaps.json` | `pbit run --op sample` | Review E4 (redundant caps: max TV 0.496875, bound 0.001928, all 12 released) | brute-force enumeration (caps checked) |
| `chain100.json` | `pbit run --op sample` | Review E11 (path, Potts 1.2; exact in < 1 ms by sum-product vs 290 ms sampled) | transfer matrix (forward-backward, log space) |

`--op sample` because `pbit run`'s default `--op decide` answers the IR inputs exactly (enumeration / frontier).

R19.4: the inference compiler answers the three router inputs (tier `occupancy`) and chain100 (tier `forest`) exactly at
defaults; `stress.rs` checks those exact answers against the same oracles and runs the router families with `--mode sample`
as well, so the sampler + gate path stays under test.
