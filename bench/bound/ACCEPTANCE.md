# Acceptance criteria: RFC 0004 `bound` (implementation wave)

The wave is done when every criterion below holds on a clean checkout, in CI, with the commands named. Each criterion is
a yes or no; "mostly" is no. Block label: **mechanism** (arithmetic judged only by its bars).

## A. Schema and refusals

- **A1.** `docs/persona.md` format 1 accepts the optional block `bound: {rest_hours, max_epsilon, seeds, count}` with the
  defaults of the RFC (rest 30 × the longest declared half-life, 24 h if none; seeds 0-99; no `max_epsilon`; no `count`).
- **A2.** Rows BO-R1 to BO-R6 refuse the persona when it is read, one `{"error"}` at the row's path, exit 2, for every
  command that reads a persona; one golden per row under `tests/golden/bound/refusals/`.
- **A3.** BO-R7 makes `check` exit 1 naming the channel's path; BO-R8 makes `bound` exit 2 naming `engine.exact_limit`.
  `--seeds` malformed or outside the declared seeds exits 2 at `--seeds`.
- **A4.** A state written by any command is unchanged by the block: the state and stance schemas differ by 0 bytes
  (`bound` adds no field and no per-turn work).

## S. The subcommand

- **S1.** `probbit persona bound PERSONA [--seeds LIST] [--json]` prints, per trait whose component holds a learned
  weight or a counted channel: epsilon, the ceiling, the attaining level set and seed, `speed`, `speed_floor`, the
  residue r, the label (`exact`, `coarse` or `conservative`), and every uncounted channel at its path. The `--json`
  document has a published schema; text and JSON carry the same numbers, rounded to 6 decimals.
- **S2.** Exit codes: 0 when every channel is searched, decayed, enumerated or counted; 1 when a channel is uncounted or
  an epsilon exceeds `max_epsilon`; 2 on a refusal.
- **S3.** Probes edit copies of the `init` state (digest recomputed) and run one rest probe each; no file is written and
  no state the host holds is read.
- **S4.** The own trait's coordinates sit at c_S; only the other searched coordinates take every vertex; the probe count
  per trait per seed is (2^L − 2) · 2^V times the enumerated combinations, printed with `--json`.
- **S5.** Above 12 searched coordinates in a component, or 64 enumerated combinations, the trait is printed with the
  ceiling tanh(D / 4) labelled `coarse`; no code path samples vertices (a test asserts it).
- **S6.** `speed` runs the greedy persuader of the RFC (at most 200 events; `> 200` printed when unreached); `speed_floor`
  runs the vertex search on the shrunk box ±min(c, n m), m = 2 · step_cap · g_max · (L − 1)/L.

## C. Gate, lint, describe

- **C1.** `check` runs `bound` when `max_epsilon` is declared and fails on exit 1, and its document validator rejects a
  bound document with epsilon but no speed, or speed but no epsilon (unit test).
- **C2.** `lint` warns on every mood without `half_life_hours`, only in a persona with a `bound` block, at the mood's path.
- **C3.** `describe` adds one line, computed without probes:
  `bound: rest <h> h, seeds <a-b>, max_epsilon <x | none>, ceiling <tanh(c/2)>, uncounted: <paths | none>`.

## D. Goldens

- **D1.** Goldens for `bound` (text and `--json`) on `ref.yaml` at `total_cap` 0.25, 1 and 2, `ref-nohalf.yaml`,
  `ref-nohalf-counted.yaml`, the four `scope-*` fixtures, `coupled3.yaml`, `tutor-learn.yaml` and the three shipped
  examples with learning, seeds 0-9.
- **D2.** The reference golden reproduces the RFC's worked example: seed 0 verbosity epsilon 0.121011, 0.458650 and 0.651552,
  ceilings 0.124353, 0.462117 and 0.761594, `speed_floor` 2 at `total_cap` 1.
- **D3.** `bench/bound/arith_0004.py` reports 57 checks, 0 mismatches.

## E. P0 and replay

- **E1 (bo7).** A persona without `bound` is byte for byte unchanged on the whole P0 corpus of the previous release
  (every shipped persona and example × seeds × the committed scripts, plus `describe`, `lint`, `fuzz` and `prove`
  documents): 0 outputs differ; a planted 1-byte change is detected.
- **E2 (bo8).** `bound` is a pure function of the file and the seeds: byte-identical text and `--json` across two runs
  and across `PROBBIT_THREADS` 1 and 8, on every bo1 fixture.

## F. Bars

- **F1.** `bench/bound/run.py` passes bo1 to bo8 as specified in `BENCH-BOUND.md`, with every pass line frozen in
  `fixtures/MANIFEST` before any engine with the block ran them (`manifest_check.py` exits 0).
- **F2.** `nulls.py` exits 0: every competent null (the uncapped counter, the clipped counter, the drift monitor, the
  released-levels check, the time-seeded sampled search) fails its bar; every engine null is rejected.
- **F3.** Runtime: seeds 0-99 cost at most 60 s per lone 3-level trait in CI with `PROBBIT_THREADS=1`; the bench prints
  wall time and probe counts.

## G. Law sweep

- **G1.** The law sweep (the method of `law_check_0004.py`) reports 0 hits on the RFC, the bench, the acceptance file,
  the docs changes, the CHEATSHEET, the man page and the CHANGELOG entry: none of the banned words, every block and bar
  labelled mechanism or simulation, and no people, chats, machines, internal paths or run names.
- **G2.** Every public sentence about the block says the limit covers the stance at rest, never the words a model writes,
  and that speed is published with it.

## H. Docs

- **H1.** `docs/persona.md` section 2.8 gains one paragraph pointing to the bound (the box is what `bound` searches), and a
  new section 2.16 `bound` holds: the schema, the theorem and its proof sketch, the closed form and when it applies, the
  scope table, the refusal rows, the 12-coordinate cap, speed and its floor.
- **H2.** `docs/rfcs/0004-bound.md` is the reviewed RFC; `CHEATSHEET.md` gains the subcommand and the block; the man page
  documents `probbit persona bound` with its exit codes; `CHANGELOG.md` has one entry naming the block, the subcommand,
  the gate, the lint warning and the describe line.
- **H3.** The public wording of the RFC is used unchanged wherever the feature is described.

## Not in this wave

The per-source, per-session cap on durable corrections (a wait, with its own bar); a prompt-persona comparison arm (needs a
model run); counting rules for blocks added after this RFC (each new persistent block states its scope row in its own RFC).
