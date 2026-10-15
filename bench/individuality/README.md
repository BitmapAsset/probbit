# Benchmark: individuality, senses and credit (`bench/individuality/`)

This benchmark defines "done" for RFC 0001 ([docs/rfcs/0001-senses-and-reward.md](../../docs/rfcs/0001-senses-and-reward.md))
with numbers a test can see. It runs on Python 3.9 (standard library only) and the probbit binary, in under 60 s on one
machine; `run.py` exits 1 when any gated bar fails. Every number it prints carries its scope: the persona, the seeds and the
events.

```
PROBBIT_BIN=target/release/probbit python3 bench/individuality/run.py --only a,b,d,e   # writes result.json next to run.py
PROBBIT_BIN=target/release/probbit python3 bench/individuality/p0.py check             # P0: byte-identical documents
```

**This release** ships senses (RFC 0001 Design A), action-addressed feedback (Design B) and metrics a, b, c, d and e. Metric f
needs a lineage budget (Design C); `run.py` skips it with a note. Metric a's frozen streams carry no `await` or `for` keys (the
fixture was frozen before Design B, so its rewards pay the stance just before them); `twins_addressed.py` re-measures it with
the credit block and addressed feedback added at run time.

**Freezing.** The fixtures, generators and bars are committed with their sha256 in `fixtures/MANIFEST` before any run.
`run.py` checks every listed file before it measures anything (exit 2 when one differs). Any later change is logged at the end
of this file with its reason, and the old result stays in the history.

## Layout

```
bench/individuality/
  run.py            the metrics -> result.json; exit 0 every gated bar passes, 1 one fails, 2 setup error
  common.py         binary lookup (PROBBIT_BIN, else PATH), canonical JSON, live/turn/verify wrappers, TV distance,
                    percentile bootstrap (2,000 resamples, random.Random(20261014))
  p0.py             P0: the documents of personas without the new blocks, recorded with the released 0.8.2 binary
  fixtures/
    sensed.yaml          4 traits, 2 moods, 4 senses, 2 habits, learning on caution, reward_from
    sensed-nogenes.yaml  the same with every sense gene spread at 0
    lives.py             the seeded generator of the streams; its outputs are committed in lives/ and checked against MANIFEST
    bars.json            the gated bars
    gen.py, p0/          the P0 corpus: personas, scripts, the recorded outputs and strands
    dispatch.yaml        metric c: the shipped dispatch policy with a credit block; dispatch-noblock.yaml without it
    credit_draws.py      metric c's seeded draws per decision, committed in credit/ and checked against MANIFEST
    bars-c.json          metric c's gated bars
    sensed-plain.yaml    metric a's reported control: the senses as plain 0.8 number inputs
  twins.py  habituation.py  credit.py  replay.py  habits.py
  twins_addressed.py   metric a re-measured with Design B (and the control), on the frozen fixture and bars
```

## Shared fixtures

**`sensed.yaml`** (a copy of `examples/persona/sensed.yaml`). Traits `caution` (bold, measured, careful), `initiative`
(follow, suggest, lead), `verbosity` (terse, short, full), `warmth` (cool, neutral, warm); moods `valence`, `arousal`
(half-life 6 h). Senses, all with gene spreads 0.3:

| sense | kind, unit, range, scale | prior (mean, var) | drift_cap | effects per unit of z |
|---|---|---|---|---|
| `tool_fail` | flag, event | 0.05, 0.05 | 0.25 | caution +0.8, valence −0.4 |
| `tool_latency` | number, seconds, 0..3600, log1p | 0.405, 0.1 | 0.7 | arousal +0.5, verbosity −0.3 |
| `tempo` | number, messages in 10 min, 0..200, linear | 1, 1 | 2 | verbosity −0.4, arousal +0.3 |
| `silence` | number, hours, 0..1e6, log1p | 0.69, 0.5 | 1.0 | warmth +0.4, initiative +0.3 |

Habits: `no_bold_after_failure` (`when: {sense.tool_fail.raw: true}`, `then: {caution: {not: [bold]}}`) and
`short_when_busy` (`when: {sense.tempo.raw: {at_least: 20}}`, `then: {verbosity: {at_most: short}}`). Learning on `caution`
(rate 0.3, step_cap 0.1, total_cap 0.4); `reward_from: [human, env]`. **Host action** (the gated decision): caution bold →
"go ahead", measured → "show the plan, then go", careful → "show the diff and wait".

**Two stimulus streams** (`fixtures/lives.py`, 60 events each, 0.5 h apart, one stream per room shared by every individual):
- Room A, quiet and praised: tool failures at rate 0.02, latency 0.1–0.3 s, tempo 0–2, silence 0.5 h; every 6th event
  `praise` from `human:a` (its readings keep the instrument's source, `env:host`).
- Room B, harsh: failures at rate 0.3, latency 1–8 s, tempo 5–30, silence 0.5 h with a 9 h silence every 10th event; every
  7th event `criticism` from `human:b`.

Both lives then get one event with `elapsed_hours: 720` and no readings, and 8 calm events at room A's medians, so moods relax
and history features roll out of their window. **Probes**: 20 events, each with a reading for every sense (a seeded subset of
tool_fail both ways × latency 0.2, 2, 8 s × tempo 1, 10 × silence 0.5, 30 h), each answered by `persona turn` from the same
saved post-life state, so probes never see each other. **Seed control**: individuals 0–39; pairs (0, 1), (2, 3) … (38, 39)
share a room.

## Metric a: twins distinguishability

Measures whether one individual's life moves its decisions as far as being a different individual does.

- **Statistics.** Per seed s: `d_life(s)` = share of the 20 probes whose host action differs between life A and life B;
  `l_life(s)` = share of (probe, trait) whose taken level differs; `tv_life(s)` = mean over probes and traits of the total
  variation distance between the odds (docs/persona.md section 5.4). Per room, the same over the 20 seed pairs: `d_seed_A`,
  `tv_seed_A`, `d_seed_B`, `tv_seed_B`. Means over seeds, with bootstrap 95 % intervals over seeds.
- **Pass bars.** a1: mean `d_life` ≥ 0.30, and the lower end of its interval ≥ 0.20. a2: mean `tv_life` ≥ the smaller of
  `tv_seed_A` and `tv_seed_B` (in at least one room a different life moves an individual as far as a different seed does).
- **Reported, not gated.** `tv_seed_B / tv_seed_A`.
- **Script.** `twins.py`: 80 `live` runs (`--clock fixed --strand`, `--state` written at the end), 1,600 probe turns.

## Metric b: habituation and the named past

Measures whether the same reading lands differently after a learned normal, and whether `why` says so.

- **Fixture.** `sensed-nogenes.yaml` for b1 and b2; streams: 20 identical tool failures from init (habituation), and 40 quiet
  readings vs 40 readings at rate 0.3 (failures at 0, 3 and 6 of every 10), each followed by one failure (surprise); b3 runs
  `sensed.yaml` over individuals 0–99; b4 and b5 read metric a's probe documents.
- **Statistics.** b1: `senses.tool_fail.zd` at reading 20 over reading 1, and the engine's z, zd, judged mean and count, and
  the normal after each reading (mean, variance, count) against an independent Python implementation of RFC section A.3 (every
  reading of every sense in the habituation, surprise and room streams, to 6 decimals). b2: z of the failure after the quiet
  stream over z after the rate-0.3 stream. b3: across individuals 0–99 with genes, the share whose zd at reading 20 is below
  reading 1. b4: of the divergent decisions of metric a (a probe whose host action differs gives two decisions), the share
  whose `why` holds a past clause its document backs: `noticed` names a sense read this turn and the clause equals the RFC A.5
  template rendered in Python from `senses`, or the learned clause equals its template rendered from `learned_from`. b5: of
  the divergent probes, the share where a cited value (z, mean or n of the normal clause, or Δ of the learned clause) differs
  between the two lives.
- **Pass bars.** b1 ≤ 0.35 (0.30 by the RFC arithmetic) with 0 differing values; b2 ≥ 2 (3.4 by the arithmetic); b3 = 100 %;
  b4 ≥ 95 %; b5 ≥ 95 %.
- **Script.** `habituation.py`.

## Metric c: credit accuracy (with RFC Design B)

Measures whether a late, reordered or cancelled outcome credits the decision it was for: an act-or-ask policy, 10 individuals
× 300 events, outcomes delivered on time and up to 6 events late (10 % cancelled, 5 % sent twice, 2 % paying a turn never
opened), against an independent Python learner. Bars: c1 ≥ 99 % of the applied step equal to the oracle's, c2 = 100 % of
events with the oracle's Δ, c3 ≥ 99 % of open outcomes accepted, c4 = 100 % of duplicates, late payments and unknown turns
refused, each refused run byte-identical to the run without the refused events.

- **Fixture.** `fixtures/dispatch.yaml` is the shipped dispatch policy (`examples/persona/dispatch-policy.yaml`) with a credit
  block (`max_open` 8, `half_life_events` 32); `fixtures/dispatch-noblock.yaml` is the same without it, for variants (iii)
  and (iv). `fixtures/credit_draws.py` draws, per individual from `random.Random(1000 + seed)`, each decision's blast radius
  (0.05 to 0.65: no habit binds), its outcome draw, its delay, and whether it is cancelled, sent twice or joined by a payment
  of a turn never opened; the draws are committed as `fixtures/credit/draws-<seed>.jsonl`.
- **Host.** `credit.py` drives one `probbit live --checkpoint-every 1` per run event by event: it reads each stance's action,
  decides the outcome (good when the draw is below 0.8 after `act`, 0.4 after `ask`) and queues it. One message rides on
  an event; a second one due on the same event moves to the next (the older decision goes ahead). A second copy of an outcome
  is due one event after the original; for a cancelled decision the second copy is an outcome sent after the cancel. A payment
  of a turn never opened names turn k + 1000. A refused event is followed by the same decision without its message, so
  each run makes 300 decisions. Five runs per individual: stream 1, and on stream 2 (i) addressed, (ii) unaddressed, (iii)
  0.8 semantics (cancels not sent), (iv) the 0.8 workaround.
- **Statistics.** The oracle replays RFC section B.2 from the stance documents and the events, over all five runs: c1 weighs
  each accepted payment by the oracle's applied step (and requires the engine's `paid` turn and `e` to match), c2 compares Δ
  after every accepted event, c3 counts the outcomes of runs stream 1 and (i) whose decision was open on arrival, c4 the
  duplicates, outcomes after a cancel and unknown turns of (i) and the ambiguous unaddressed feedback of (ii), and every run
  is re-run without its refused events.

## Metric d: replay

Measures whether an individual is reproducible from its log.

- **Fixture.** The lives of metric a, and the P0 strand of the shipped tutor recorded with the released 0.8.2 binary.
- **Statistics.** Differing bytes between each strand and a third copy (the same persona, seed and events) and a split copy
  (the same life split at a seeded event into two runs: `--state` plus the strand continued); `live verify` exit codes; for the
  0.8.2 strand, verify under this build and the bytes it appends for 20 committed events against the recorded bytes.
- **Pass bars.** 0 differing bytes; every verify exit 0; the 0.8.2 strand verifies and the appended bytes are equal (P0).
- **Script.** `replay.py`.

## Metric e: zero habit breaks

Measures whether habits hold in every decision, whatever the life.

- **Fixture.** Every stance of metrics a, b and d. Control: `habits.bound` lists the habits that changed the stance, so a zero
  is not vacuous.
- **Statistics.** Stances with `habits.violations` > 0; stances that break a habit by an independent Python check (each
  habit's `when` evaluated on the logged inputs, with `sense.<id>.raw` read from the stance's `senses`, against the released
  levels); stances where a habit bound.
- **Pass bars.** 0 and 0 breaks; at least 50 bound stances.
- **Script.** `habits.py`, post-processing only.

## Metric f: lineage drift (with RFC Design C; skipped in this release)

Measures whether a lineage that absorbs learned weights into its prior stays within a declared budget: every generation over
the budget refused, no benign generation refused.

## P0: personas without the new blocks

`p0.py` recorded, before any code change and with the released 0.8.2 binary (sha256 in `fixtures/MANIFEST`), 653 outputs: 8
personas without the new blocks × seeds 0–2 × 4 committed random scripts of 100 events, through `persona replay` and `live`
(9,600 live turns: stances, final states, strands with the engine string and the hashes chained from it masked), plus `check`,
`describe`, `init`, `compile`, `lint --seeds 0-99`, and `lint`, `prove` and `fuzz` documents of the shipped rules. `p0.py
check` exits 0 when every output equals its record. One line may differ, on stderr only: a script that sends a `senses` key
to a persona without the block gets `live`'s one line naming it (scripts k=1, at their earliest such event); the check accepts
exactly that line there and counts those outputs apart.

## What this benchmark cannot see

- Whether a reading or a reward is true: a forged env label passes every metric.
- Whether a host acts on the stance, or a model writes in it.
- Natural lives: every life here comes from a seeded generator, and sequential turns of one life are correlated.
- Other personas, longer lives (none here exceeds 100 events before its probes), other hardware (timings come from one
  machine; bytes match wherever the floating-point maths agrees, docs/persona.md section 5.3).
- Whether any of this is like anything from the inside. No metric here can see it, and none claims it.

## Results

One run on an Apple M4 (development build, 10.0 s): a1 0.145 (interval 0.096–0.199) and a2 0.1307 against 0.1329 fail their
bars; b1 0.304149 with 0 of 4,522 values differing, b2 3.422977, b3 100 %, b4 100 % of 232 decisions, b5 100 % of 116 probes,
d 0 differing bytes over 240 strands, e 0 violations and 0 breaks over 9,442 stances with 72 bound pass. See BENCHMARKS.md §10.

With action-addressed feedback (development build): c1, c2, c3 and c4 100 % (8,393 payments, 15,000 events, 3,802 open
outcomes, 2,793 refusable, 50 of 50 runs byte-identical without their refused events); metric a re-measured with Design B
(`twins_addressed.py`): a1 0.1725 (interval 0.1175–0.235), a2 0.1223 against 0.1366, both still below their bars; the
plain-inputs control 0.155 and 0.0295. See BENCHMARKS.md §11.

Metric a fails its bars on this release: a1 0.1725 (interval 0.1175–0.235) against ≥ 0.30, a2 0.1223 against 0.1366.
The bars and the fixtures stay frozen and unchanged. Why they fail: the frozen fixture's readings and its feedback push the
same probes in opposite directions, so the two mechanisms cancel; and at the fixture's gene spread, two individuals differ in
odds more than one individual's two lives do. A pre-registered revision of the fixture is planned for 0.9.1; nothing will be
relabelled.

## Changes after freezing

- `p0.py` (listed in MANIFEST since the P0 record): `check` accepts the one stderr line `live` writes when a script sends a
  `senses` key to a persona without the block, only on such a script, once, at its earliest such event, and reports those
  outputs apart (`stderr_senses_warning_only`). The recorded outputs, scripts and strands are unchanged. Reason: the line is
  part of the senses release (an undeclared key is named once); it is not a document, and every document stays
  byte-identical. MANIFEST's p0.py line was updated with the change.
- `p0.py` and `fixtures/p0/EXPECTED`: the corpus now passes personas and rules by their paths relative to the repository
  root, where every command runs. Reason: a fuzz document that finds a counterexample names the persona path as given in
  its `explain` and `replay` strings, so `fuzz/adaptive-fixture` (43,726 bytes recorded from one clone) differed from a
  clone at another path while the engine's bytes were the same. EXPECTED was re-recorded with the same released 0.8.2
  binary (sha256 in MANIFEST) using relative paths: one line changed (`fuzz/adaptive-fixture`, now 43,646 bytes); every
  other output, the scripts and the strands are unchanged. MANIFEST's EXPECTED and p0.py lines were updated with the
  change.
- `p0.py` (wave 2): `check` also accepts the one stderr line `live` writes when a script sends `await`, `for` or
  `cancel` to a persona without a credit block (RFC 0001 B.4: undeclared inputs, listed in `ignored`, and `live` says
  so), on the same terms as the senses line: only where the script carries those keys, once, at its earliest such event,
  each mandated line present exactly once. The outputs equal but for these lines are reported apart as
  `stderr_warnings_only` (the key was `stderr_senses_warning_only`). EXPECTED, the scripts and the strands are
  unchanged; MANIFEST's p0.py line was updated.
- Metric c (wave 2): its fixtures, the draws generator, the draws and its bars (`fixtures/bars-c.json`, the bars of the
  metric c section above) were added to MANIFEST in their own section before they were ever run; `run.py` runs metric c.
  `fixtures/sensed-plain.yaml` (metric a's reported control: the senses replaced by plain 0.8 number inputs) was frozen in
  the same section. No earlier fixture, stream or bar changed.
- `credit.py`, changed after it had run once (no fixture, bar or gated computation changed; every gated value was the same in each
  run): the 50 runs go side by side (the host waits on each stance, so a run is latency-bound); a run that refused no
  event is not re-run (without refused events it is the same run, and metric d replays every strand); the reported
  eviction and expiry shares are given per run, with the age of the addressed payments. The numbers of the run before the change stay in
  the history: c1, c2, c3 and c4 1.0, 50/50 pairs identical.
