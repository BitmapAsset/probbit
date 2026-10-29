# The amplitude register demo (research, RFC 0011)

One decision, shown twice. A fictional tutor reduced to its humour component lives the same 17 events twice: once on the
default tiers (`thermal.yaml`, the thermal face) and once on the amplitude register (`register.yaml`), an exact simulated
qubit register on a CPU, run with the long sweep (`tf-linear-64x16`) and exact amplification onto the plans its habits allow.
No quantum hardware is involved and no speedup is claimed: every number below is arithmetic, exact and replayable, simulated
on a CPU.

Two opposed drives pull humour: praise raises it (with valence and emoji), criticism, a correction, lowers it. The life
(`life.jsonl`, seed 1, fixed clock) opens with a run of corrections; at **event 9** praise and a correction arrive together.

| event 9 (strand line 10), humour | none | light | playful | top two gap |
|---|---|---|---|---|
| thermal (default tiers) | 0.059462 | 0.466563 | 0.473975 | **0.007412** |
| register, default sweep (`tf-linear-16x4`, for comparison) | 0.013180 | 0.566151 | 0.420670 | 0.145481 |
| register, long sweep (`tf-linear-64x16`, this copy) | 0.007171 | 0.249010 | 0.743818 | **0.494808** |

On the same decision the thermal odds of the top two humour levels sit 0.007412 apart (a near tie: the drives cancel), and
the simulated register separates them by 0.494808: it commits to `playful`, the thermal plurality (the default sweep, four
times shorter, leans the other way, to `light`, by 0.145481). It does not invent a preference the weights do not hold; a
slower sweep keeps more of the register's amplitude on the most probable plans (docs/rfc/0011-amplitude-register.md,
"Schedules and the indifferent set" and "The demo"). Events 2 and 11 are near ties of the same kind (praise and a correction
together: thermal gaps 0.033093 and 0.048852, register gaps 0.656343 and 0.725300); the register is not always the more
decisive face (event 14: thermal gap 0.072051, register gap 0.009221).

## Files

| file | what |
|---|---|
| `register.yaml` | the persona with `engine: {tier: amplitude, amplify: true, schedule: tf-linear-64x16}` |
| `thermal.yaml` | the same persona without the engine block (`amplify` and `schedule` need the tier, so all three go) |
| `life.jsonl` | the 17 events |
| `register.strand`, `thermal.strand` | the two recorded lives (`probbit live verify` checks each); event 9 is line 10 of both |

Record and check them again:

```sh
probbit live examples/persona/demo/register.yaml --seed 1 --clock fixed --events examples/persona/demo/life.jsonl --strand register.strand
probbit live examples/persona/demo/thermal.yaml --seed 1 --clock fixed --events examples/persona/demo/life.jsonl --strand thermal.strand
probbit live verify register.strand
probbit monitor register.strand --open     # the Mood Monitor (docs/persona.md, section 5.8); thermal.strand beside it
```

The register strand's documents carry the receipts: `engine.tier` `amplitude`, `engine.schedule_name` `tf-linear-64x16`,
`engine.amplify` (iterations, phases, the mass before and after), `feasible_mass` 1.000000 and the `state` digest of the final
amplitudes at 6 decimals.

**Caveat.** The long sweep replays on the recording (std) build only for now: the no_std build's math library differs from the
platform's in the last bits, and over four times the layers a 6-decimal odd can flip. The default schedule stays the one for
every recorded strand; an engine without the tier (0.13.0) refuses `register.strand` (exit 5) and verifies `thermal.strand`.
