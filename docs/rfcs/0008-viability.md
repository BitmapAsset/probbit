# RFC 0008 (draft): viability bands and need-coupled valence and pressure

Status: draft, implemented and shipped in 0.11.0 (PR #50), for review · Changes: docs/persona.md §2.12
(format 1, additive) · Depends on: RFC 0002 part 1 (needs, as shipped in 0.10.1-dev @ a5d9c26)

Mechanism and simulation only. The claim this RFC makes possible is "the individual registers deviation from its own
viability bounds and prioritises among its needs", with receipts. Nothing here is a claim of experience.

## Summary

The shipped `needs` block integrates a deficit per need from logged readings and amounts, exactly: in I3-sim (4,000
evaluation episodes, 400 steps each) the engine's deficit equalled the world's `1 − level` to 6 decimals on every event
(max reconstruction error 0.000000). What the engine does **not** carry is where the bound is, how close the individual is
to it, and a graded path from that closeness to its moods. The I3-sim host had to supply all three. This RFC adds them:

1. `viable`: a viability bound per need, a margin, a worst-need aggregate and a non-viability flag, as engine state.
2. `needs.pressure`: a graded coupling from the worst margin to a declared mood (pressure), replacing per-threshold habits.
3. `needs.valence`: a tonic term (−D(x), how far from the setpoints) and a phasic term (the drive reduction this event)
   into the valence mood, so valence registers deviation rather than only payments.
4. Flag senses as need growth (`per_event`), so a resource a tool failure damages needs no parallel number sense.
5. Receipts: stance fields, `live verify` summary fields and monitor rows for every new number.

## Measured gaps (I3-sim, evaluation seeds 1000-1999, 1,000 episodes per arm; tables in `tables/`)

| gap | what the shipped engine gave | what the host had to do | measured consequence |
|---|---|---|---|
| G1 no bound | a deficit in [0, cap] and a setpoint at 0 (persona.md l.579-670) | keep each band's lo and width in host config; rank needs by deficit / (1 − lo); detect non-viability itself | prioritisation accuracy of A 0.9999 is host arithmetic over engine deficits; the engine cannot say which need is worst |
| G2 no graded deficit → mood path | `need_<id>` acts only on `pursue` (l.636); "a rule never reads `need_<id>`" (l.641) | express pressure as 6 habits (`need.<id>.deficit ≥ θ` → pressure ≥ mid / = high) | pressure tracked the worst margin (median per-episode Spearman 0.832 in A) — but equally in arms whose decisions ignore it (B 0.880, D 0.807): the tracking is the habits' step function, set by hand-picked θ; the loop's kept mutations moved θ (see RESULTS) |
| G3 valence registers payment, not deviation | `drives.valence` = expect × E + surprise × d (l.415-430) | nothing | median Spearman(valence, distance-to-bound) in A **−0.149** (C −0.202): valence rose when resources were low, because need payments (wins) happen there; in B/D (+0.232 / +0.223) it moved with failures instead |
| G4 no pressure field | none; RFC 0002 part 2 `rest` (sleep-like pressure) is unshipped and `live verify` reads a `rest` line as unsupported | declare a persona mood `pressure` | every lane names "pressure" differently; no engine invariant ties it to need |
| G5 flag damage | `per_reading` takes number senses only (N-R4) | send a parallel number sense `damage` beside the flag `tool_fail` | two readings for one event |

The survival result stands without these (A 0.559 vs B 0.000, CI [0.528, 0.590]); the gaps are about where the
need information lives: today in the host, after this RFC in the strand.

## Design

### A. Bounds and margins (mechanism)

```yaml
needs:
  items:
    - id: budget
      cap: 1
      viable: {at_most: 0.85, events: 4}   # deficit bound x_b (0 < x_b <= cap) and the run of events beyond it that ends viability
```

Per need after the needs step: `margin = round(clip(1 − x / x_b, 0, 1), 6)` (1 at the setpoint, 0 at the bound);
`beyond` = consecutive events with x > x_b (state, capped at `events`). Aggregates: `worst` = the need with the smallest
margin (ties: declared order), `margin_min`. When some need's `beyond` reaches `events`, the state sets `viability:
"lost"` once and the stance carries it; nothing else changes (the engine never stops a host). Habits gain the conditions
`need.<id>.margin` and `needs.margin_min` (numbers, `{at_least, at_most}`). Refused when read: `at_most` outside (0, cap],
`events` outside 1..10000 (V8-R1).

### B. Graded pressure (simulation)

```yaml
needs:
  pressure: {mood: pressure, gain: 2.0, shape: linear}   # mood evidence += gain × c(1 − margin_min)
```

The named mood (2-7 levels, declared like any mood) takes `gain × (1 − margin_min)` as an ordinal shift each event, lowered
as a reserved number input `need_pressure` in [0, 1] (box), so `prove` bounds it. No threshold to pick: the I3-sim loop's
kept habit-threshold mutations are the evidence that hand-set θ is a tuning surface; a gain is one number per persona.

### C. Need valence (simulation)

```yaml
needs:
  valence: {mood: valence, tonic: 0.8, phasic: 0.6}      # 0 <= tonic, 0 <= phasic
```

Mood evidence += `−tonic × D(x)/D(cap)` + `phasic × (D(x_before) − D(x_after))` per event, where D is the shipped drive
function (l.616) and x_before is the deficit vector before this event's pass 1. The phasic term is Keramati & Gutkin's
homeostatic reward (already the size of a need's win); the tonic term is what G3 lacked: a standing push that grows with
deviation. Lowered as `need_tonic` in [0, 1] and `need_phasic` in [−r_cap, r_cap]. Coexists with `drives.valence`
(evidence adds).

### D. Flag senses as growth (mechanism)

`per_reading: {sense: tool_fail, per_event: 0.08}` on a flag sense: growth `per_event` per true reading. N-R4 relaxes to
"a number sense with `rate`, or a flag sense with `per_event`".

### E. Receipts

- Stance: `needs.<id>.{deficit, forecast, term, margin, beyond}`, `viability: {worst, margin_min, status}`; `why` gains
  `budget margin 0.12, nearest its bound` when `worst`'s margin is under 0.25.
- `live verify` summary: `"viability": {"margin_min_seen", "lost_at": n | null}` recomputed from logged readings/amounts.
- Monitor: a VIABILITY row (margin bars per need, the worst highlighted).
- Properties to test, as for RFC 0002: P3 (refused events change nothing) with bounds; a persona without the new keys gives
  every document byte for byte as 0.10; margin and pressure inputs are capped and rounded to 6 decimals; `prove` boxes for
  `need_pressure`, `need_tonic`, `need_phasic`.

## Bars for the follow-on I3-sim run (to be fixed before it runs)

- v1: with `needs.pressure`, median Spearman(pressure, −distance) in A ≥ 0.3 with **no** habits on deficits.
- v2: with `needs.valence`, median Spearman(valence, distance) in A ≥ 0.3 (today −0.149).
- v3: the host policy reads `viability.worst` instead of host band widths; prioritisation accuracy within 0.02 of today's.
- v4: A − B survival CI still excludes 0; every strand verifies; documents of a persona without the keys are byte-identical
  to 0.10.1.

## Not covered

Learning of bounds (bounds are declared, never learned); multi-agent needs; anything about experience. The welfare stance
that §4 asks for should exist before this ships.

## Implementation notes (PR #50, shipped in 0.11.0)

Decisions the draft left open, as built:

1. `viable` is per need; the aggregates (`worst`, `margin_min`) run over the needs that declare one. A needs block whose needs
   declare none has no `viability` in its stance or state. `needs.pressure` needs at least one bounded need.
2. `beyond` counts consecutive events with x > x_b after the needs step (x = x_b is margin 0 but not beyond), capped at
   `events`; `lost` is set once and never cleared.
3. `need_pressure` = round(1 − margin_min, 6) after the event; `need_tonic` = round(D(x)/D(cap), 6) with D(cap) the drive at
   every cap (genes included); `need_phasic` = round(clip(D(x before pass 1) − D(x after pass 2), ±r_cap), 6). Their effects
   are ordinal shifts with weights gain, −tonic and phasic. A weight of 0 lowers no input (the A-t0 arm of the v-run is the
   same persona with tonic 0).
4. The reserved ids are reserved only while the coupling that lowers them is declared (a 0.10.1 persona with a need called
   `pressure` still reads).
5. `stance.block` is in the document with a bound, a coupling or the new `line.block: true`, so a document without the new keys
   is unchanged; for every other persona `live verify --blocks FILE` renders the same text from the replayed stances. Format:
   traits in declared order, then moods, `name level` joined by `; `, then a newline and `viability: worst <need>, margin <m>`
   (2 decimals; `viability: lost, worst ...`; `viability: no bounds declared` for needs without a bound). `pursue` (the
   lowering's trait) is not in the block; the pursued goal is in `pursue`.
6. `live verify`'s `margin_min_seen` starts at the first state's margin_min (1 for a new individual); after a checkpoint, at
   the checkpoint's state.
7. The monitor's label `VIABILITY` is wider than its 8-column gutter, so it heads its own line (worst, margin_min, status)
   and the per-need rows follow it.
