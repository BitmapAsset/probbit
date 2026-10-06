# probbit cheat sheet (0.7.0)

The CLI's own help and docs, shortened; `probbit <command> --help` has every flag.

## Verdicts (`decide`, `run`, `evaluate`; a persona turn's `engine.verdict`)
| verdict | means | exit |
|---|---|---|
| `exact` | odds are exact; `tier` says how | 0 |
| `diagnostics_passed` | sampled; every released item passed the diagnostics gate (not a proof) | 0 |
| `partial` | act on `released`, escalate the rest | 0 |
| `refused` | the sampler cannot vouch for the answer; escalate | 3 |
| `infeasible` | no plan satisfies the rules (a proof) | 1 |

`declined` (exit 3): `decide --mode exact` or `run --op exact` stopped before an answer.

## Tiers (`tier`, the method that answered)
The exact tiers run before the sampler:
- `enumerate`: every feasible plan listed (up to `--exact-limit`, default 2,000,000);
- `frontier`: a dynamic program over loads, when groups share few resources (`--frontier-states`, default 4096);
- `occupancy` / `forest` / `components`: independent parts solved one by one (two-value groups by a count dynamic
  program, cap-free trees by sum-/max-product, a mix);
- `sample`: constraint-preserving Gibbs sampling + the diagnostics gate (`decide` omits `tier` when it samples).

## A persona stance
- `line`: the stance line. The host puts it into the model's prompt for this turn; an agent can read it to know its own
  stance; it is not addressed to the end user.
- `habits.active`: habits in force this turn. `habits.bound`: the active habits the habit-free twin breaks, i.e. the ones
  that changed the stance. Empty = no habit had to change anything this turn.
- `status`: `ok` (exact or diagnostics_passed), `partial`, `refused` (the line: just the habits), `fallback`.

## Exit codes
- `decide`, `run`, `evaluate`: 0 answer (exact | diagnostics_passed | partial), 1 infeasible (a proof), 2 bad input (one
  `{"error"}` object on stdout) or bad flag (stderr), 3 refused / declined / non-finite result.
- `persona`: 0 done; 1 lint found an unresolved contradiction or (with rules) a counterexample, fuzz found a counterexample or
  prove left a rule unknown; 2 bad persona / state / inputs / script / rule, or bad flag.
- `live`: 0 done; 1 verify: a line differs; 2 a bad persona, state, event or flag.

## `probbit persona <sub> PERSONA`
| sub | does |
|---|---|
| `init [--seed N] [--out STATE]` | a new individual (genes from the seed, resting stance) |
| `turn --state STATE [--inputs JSON\|FILE]` | the stance on stdout; the new state replaces STATE |
| `replay [--seed N] --script JSON\|FILE` | init, then every turn of the script: one stance per line |
| `explain [--seed N] --script ... --turn K` | turn K in words: every contribution, the odds, the twin |
| `diff [--seed A] [--other P2] [--seed2 B] --script ...` | distance between two individuals |
| `lint [--never RULE \| --props FILE]` | contradicting habits; with rules: prove, then fuzz the unknown ones |
| `fuzz (--never RULE \| --props FILE) [--seeds 0-99]` | the shortest event script whose stance breaks a rule |
| `prove (--never RULE \| --props FILE) [--seeds 0-99]` | held by construction, proved for every event sequence, or unknown |
| `check` | valid? digest and sizes |
| `compile --state STATE [--inputs ...]` | the turn's probbit-ir program |
| `describe` | traits, moods, inputs, habits, agenda |

## `probbit live`
| command | does |
|---|---|
| `live PERSONA [--seed N \| --state FILE] [--strand FILE] [--events FILE]` | JSONL events in, one stance per event out |
| `live PERSONA --demo week [--seed N] [--plain]` | a scripted week: learning to a cap, a night, rules that hold |
| `live verify STRAND` | replay a strand; the earliest line that differs |

## Testing a character
- `lint`: contradicting habits; with `--props`, every rule proved or fuzzed;
- `fuzz`: "none found" is evidence over the scripts searched, not a proof;
- `prove`: `held by construction` (a habit implies it), `proved for every event sequence` (a sound bound) or `unknown`;
- `live verify`: a strand replays byte for byte.
