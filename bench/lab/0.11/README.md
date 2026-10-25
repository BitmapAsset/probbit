# Lab reproduction kits for 0.11 (Result 2: viability and graded need pressure)

Mechanism and simulation. Each kit is the reproduction subset of one lab run behind BENCHMARKS.md §13 and
`docs/results/0.11/`: the harness, the frozen configuration, the tables the run wrote, the run's own replay record, and the
bars as registered before the run (`PREREGISTRATION.md`, extracted verbatim from the lab brief). Each `rerun.sh` regenerates
the tables twice from the configuration with a probbit binary you give it and checks them byte for byte against the run's
own sha256 manifest. No API calls, no network, no model.

| kit | the run | reproduces | rerun (16-vCPU Ubuntu VM) |
|---|---|---|---|
| [`v011-viability/`](v011-viability/) | the v-run, 0.11 development build `152d005`, 2026-10-21 | §13 bars V1-V4 and V7, nulls, tracking; Table 3's 0.11 column; figures a-c (0.11 halves) | 27 min 44 s |
| [`i3sim-needs-viability/`](i3sim-needs-viability/) | I3-sim, 0.10.1-dev `a5d9c26`, 2026-10-20 | Table 3's 0.10.1 column, the v-run's reference numbers; figures a-c (0.10.1 halves); the 240 simulated strands of V5 | 51 min 37 s |
| [`v2res-valence-residual/`](v2res-valence-residual/) | the V2 residual follow-up, `152d005`, 2026-10-21 | §13's residual paragraph (eight paired arms), Section 5.3 of the report | 34 min 58 s |

All three regenerate byte-identical with probbit 0.12.1 (the release binary), as measured on 2026-10-25; the sha256 of every
table is in each kit's README.

```bash
cd bench/lab/0.11/<kit>
python3 -m venv venv && venv/bin/pip install numpy==2.0.2
PROBBIT=$(command -v probbit) experiments/rerun.sh venv/bin/python
```

## Not in these kits

- **Recorded strands of a private deployment.** V5 of the v-run also replayed seven recorded strands (196 events), V6
  audited two, and the host acceptance check H1 read them. Those strands, and the tables that name their lanes, are not
  published. The rest of V5 (the 240 simulated strands) is in `i3sim-needs-viability/`.
- **V6's campaign harness.** `v011-viability/v6/` carries the recorded comparison of the 247 campaigns with the engine's
  epsilon; the scripts that ran the campaigns are not published. The public bound bench re-measures the bound on its own
  registered campaigns: `PROBBIT_BIN=target/release/probbit PROBBIT_THREADS=1 python3 bench/bound/run.py`.
- **E1, the bound in the engine** (`e1-bound-in-engine-20261020`, Result 1's V6 row): not published as a kit; that row is
  reproduced from this repository by the bound bench above (`bench/bound/BENCH.md`).

## Not yet published

- **N1** (`n1-surprise-live-state-20261021`) and **N1-b** (`n1b-live-state-constrained-20261021`), behind Result 4 and
  figures d, e and f of `docs/results/0.11/`. Their fixtures are derived from recorded strands of a private deployment;
  publishing them needs synthetic fixtures and regenerated tables, and that decision has not been made.
