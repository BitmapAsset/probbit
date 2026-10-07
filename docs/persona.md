# probbit persona: the individuality layer (format 1)

A **persona** gives an agent a stable individual temperament that lives outside the language model. It is a small file of
traits, moods, habits and evidence rules. Every turn it compiles, with the agent's state and the turn's inputs, into **one
probbit IR program**. probbit answers with the **stance** for that turn: a level for every trait with exact odds, the habits
that were in force and the ones that changed the outcome, a refusal when the engine cannot vouch, a one-line explanation,
and a short **stance line** (at most 40 tokens by default) that the host puts into whatever model writes the reply.
The stance line is addressed to the model: the host puts it into the model's prompt for this turn (section 7), so the model
reads it as its instructions for how to write; an agent can read it to know its own stance; it is not addressed to the end user.

- **Same persona file + a different seed = a different, stable individual.** The seed fixes each individual's small,
  permanent offsets ("genes"); the same individual is recognisable on inputs it has never seen.
- **Swap the model, keep the individual.** The stance is computed without any model in the loop, so it is the same for every
  model and every host, and it can be replayed byte for byte.
- **A persona does not write text.** It decides *how* the text is written (warm or plain, one line or a breakdown, a joke or
  none, verify first, ask first). The model still writes every word.

This document is normative for the file format, the compilation, the stance document and the state. It is implemented by
`probbit persona` (probbit-cli/src/persona.rs and yaml.rs, no dependency) and served on every surface with the same documents: the
CLI, `probbit mcp` (tools `probbit_persona_init`, `probbit_persona_turn`), the Python wrapper (`probbit.persona_init`,
`persona_turn`, `persona_replay`) and the browser module (probbit-wasm ops 4 and 5). `probbit live` (section 5.7, live.rs) keeps an
individual running on a clock, with learning (section 2.8) and a replayable log. The example personas and their goldens are in
[examples/persona/](../examples/persona/). An independent reference implementation of this format (Python) gives the same
documents byte for byte on every example turn (section 5.3).

```sh
probbit persona init examples/persona/tutor.yaml --seed 2 --out pip.json                       # an individual: genes from the seed
probbit persona turn examples/persona/tutor.yaml --state pip.json --inputs '{"loss": true}'    # this turn's stance; pip.json moves on
```

Contents: 1 Files · 2 Schema · 3 Compilation · 4 The stance document · 5 State, canonical JSON, replay, testing, live · 6 What a
persona can NOT do · 7 Commands and surfaces · 8 A model-proposed stance (the `evaluate` bridge) · 9 Versioning.
On one page (verdicts, tiers, `bound`, exit codes, every command): [CHEATSHEET.md](CHEATSHEET.md).

## 1. Files

A persona is one document, written either in **JSON** or in the **YAML subset** below; both forms parse to the same object
and have the same digest (`sha256` of the canonical JSON: keys sorted, no whitespace, UTF-8).

**YAML subset.** Block mappings, block sequences (including `- key: value` items), one-line flow collections (`[a, b]`,
`{a: 1, b: [x, y]}`), plain / single-quoted / double-quoted scalars (double-quoted ones take JSON's escapes), numbers,
`true` / `false` / `null` / `~`, and `#` comments. Rejected with a line number, never guessed: tabs in indentation, anchors,
aliases, tags, block scalars (`|`, `>`), flow collections spanning lines, duplicate keys, a second document, numbers that are not
exact in a double (an integer beyond 2^53, `1e999`), and plain keys another YAML reader would not read as strings (`yes`, `no`,
`on`, `off`, `y`, `n`, `true`, `false`, `null`, numbers: quote them). A file that uses only this subset means the same thing to
every YAML 1.2 reader. A persona file whose name ends in `.json` is read as JSON (duplicate keys refused), any other as the subset.

The document is **strict**: every field below is type-checked; an unknown field is an error at its path (e.g.
`traits[2].sprad: unknown field`), as for every probbit document. Habit rules are checked when the file is read, as strictly as
`probbit run` reads them, so a malformed rule fails at once and not only on the turns it is in force. The same holds for a turn's inputs and for the state: a bad
value is one `{"error": {"code": "persona", "path", "message"}}` object and exit code 2, never a silent coercion.

## 2. Schema

```yaml
probbit_persona: 1                  # format version (required)
identity:                           # required
  name: Rook                        # 1-64 chars: letters, digits, space, _ . -   (genes are keyed by name + seed)
  version: 1.0.0                    # a string
  seed: 1                           # the default individual (integer >= 0); `init --seed N` makes another one
  summary: a cautious ops engineer  # carried, never read by the engine
traits: [...]                       # required, at least one (section 3)
moods: [...]                        # optional
couplings: [...]                    # optional soft links between traits / moods
inputs: [...]                       # optional per-turn evidence
history: [...]                      # optional features derived from past turns
habits: [...]                       # optional hard rules
agenda: {...}                       # optional ordered beats of a reply
engine: {...}                       # optional engine settings
line: {...}                         # optional stance-line settings
learning: {...}                     # optional bounded learning from feedback (section 2.8)
drives: {...}                       # optional goals with wanting, afterglow and expectation; the `pursue` output (section 2.9)
comment: any text                   # ignored
```

Identifiers (`id` of traits, moods, inputs, history features, habits, agenda steps, and level names) match
`[a-z][a-z0-9_]*` (at most 48 characters). Level names may not be YAML-reserved words (`yes`, `no`, `on`, `off`, `true`,
`false`, `null`, `y`, `n`).

### 2.1 Traits and moods

```yaml
- id: verbosity
  levels: [terse, short, full]      # 2-7 ordered levels, lowest first
  prior: [0.5, 0.4, 0.1]            # probabilities (normalised), or `logw: [...]` natural-log weights; default uniform
  spread: 0.5                       # individuality: s.d. (nats) of each individual's ordinal shift; default 0
  say: [1-2 sentences, a short paragraph, full detail]   # phrase per level for the stance line ("" = say nothing)
  fallback: short                   # level used when there is no stance (default: the prior's most likely level)
  vouch: 0.5                        # optional odds floor: below it the trait is "unsure" (default 0 = off)
  hold: short                       # optional: the level an unsure trait is held at (needs vouch > 0)
```

A **mood** has the same fields plus `inertia` (0 <= k < 1, default 0.5: how much of the previous turn's mood carries over)
and `half_life_hours` (optional: idle time halves the carried mood). Traits are the stance; moods are slower internal
variables that shape it through couplings. Trait and mood ids share one namespace.

**The core trait set.** Any trait may be declared; the eight below are recommended so that personas, hosts and tools
interoperate (the same ids and level names in every persona make individuals comparable across personas):

| trait | levels (low → high) | what it sets in a reply | standard dimension it follows |
|---|---|---|---|
| `warmth` | cool, neutral, warm | pleasantries, encouragement | interpersonal warmth (agreeableness) |
| `directness` | gentle, balanced, blunt | verdict first vs softened | politeness strategy / assertiveness |
| `verbosity` | terse, short, full | length | quantity |
| `humour` | none, light, playful | jokes | playfulness |
| `formality` | casual, neutral, formal | register | formality of register |
| `initiative` | follow, suggest, lead | answer only / propose / act | proactivity |
| `caution` | bold, measured, careful | verification depth, risk talk | conscientiousness / risk appetite |
| `curiosity` | focused, open, exploring | follow-up questions, alternatives | openness |

Recommended moods: `valence` (down, even, up) and `arousal` (calm, steady, keyed): the two axes of the circumplex model of
affect. Common custom traits: `emoji` (none, sparse, rich), `patience`.

### 2.2 Couplings (soft)

```yaml
- {vars: [valence, humour], align: 0.8}              # ordinal alignment: + w x c(a) x c(b)
- vars: [caution, initiative]
  table: {careful: {lead: -0.8}, bold: {follow: -0.3}}   # sparse log-weights, level of a -> level of b -> w
```

`c(level)` is the centred level index, from -1 (lowest) to +1 (highest). A positive `align` makes the two variables move
together, a negative one opposite. Couplings are soft: they shape the odds; they never forbid anything.

### 2.3 Inputs (per-turn evidence)

```yaml
- id: sentiment
  kind: level                       # flag | level | number
  levels: [negative, neutral, positive]
  default: neutral
  say: {negative: user upset, positive: user upbeat}    # used in the explanation
  effects:                          # level -> target -> effect
    negative: {valence: -1.2, warmth: 0.4, humour: -0.8}
- id: error
  kind: flag                        # effects apply when the flag is true
  reactivity_spread: 0.3            # individuality: each individual's gain on this input, exp(N(0, s^2)); default 0
  effects: {caution: 1.0, verbosity: -0.6, valence: -0.6}
- id: stakes
  kind: number                      # 0..max (default max 1; outside it: an error): effects scale linearly with the value
  effects: {caution: 1.6, humour: -1.0}
```

An **effect** on a trait or mood is a number (an ordinal shift: `w x c(level)`, so `+1.0` pushes toward the highest level),
a list (one log-weight per level) or a mapping (`{level: log-weight}`). An effect on an agenda step (`step.<name>`) is a
number (positive = earlier) or per-slot log-weights. All weights are natural-log weights, so `+0.7` roughly doubles the odds.

**Standard inputs** (recommended names, so one host can drive any persona; a persona ignores what it does not declare and
lists it in the stance's `ignored`):

| id | kind | meaning |
|---|---|---|
| `sentiment` | level: negative, neutral, positive | the user's tone this turn |
| `error` | flag | the agent's previous action failed |
| `loss` | flag | the user reports a loss or bad news |
| `praise` / `criticism` | flag | the user praised / corrected the agent |
| `stakes` | number 0..1 | how costly a mistake would be |
| `time_pressure` | number 0..1 | how hurried the user is |
| `claim_done` | flag | the draft reply is about to report a task as done |
| `task` | level: chat, question, code, ops, explain (a persona may declare its own list) | what kind of turn this is |
| `elapsed_hours` | number (reserved) | idle time since the previous turn; decays moods by their `half_life_hours` |

### 2.4 History features

```yaml
- id: errors_in_row
  of: error                         # a flag input
  kind: streak                      # streak: consecutive turns with the flag, this one included; recent: count in the last `window` turns
  cap: 3                            # the value saturates at cap (default 5)
  window: 5                         # recent only (default 5)
  effects: {caution: 0.5, verbosity: -0.4}   # per unit of the value
```

History features are inputs computed from the state; they can drive effects and habit conditions.

### 2.5 Habits (hard rules)

A habit is a rule the stance obeys **by construction**: the engine only considers stances that satisfy every habit in force.

```yaml
- id: no_jokes_on_loss
  when: {loss: true}                # optional; all conditions must hold (flag: true/false; level input: a level or a list;
                                    #   number or history feature: a number = at least, or {at_least, at_most})
  then: {humour: [none]}            # trait/mood -> allowed levels:
                                    #   [levels] | {not: [levels]} | {at_most: L} | {at_least: L},
                                    #   L = a level, prev, prev-1 or prev+1 (the previous turn's level, clamped to the range)
  say: no jokes                     # phrase for the stance line
  priority: 0                       # used only to resolve conflicts (section 5.5); default 0
- id: verify_before_reporting
  rules:                            # raw probbit IR rules over trait / mood ids, level names and agenda steps
    precedes: [{before: step.verify, after: step.report}]
- id: one_flourish_at_a_time
  rules:
    linear: [{terms: [[humour, playful, 2], [humour, light, 1], [emoji, rich, 2], [emoji, sparse, 1]], limit: 3}]
```

`rules` takes the probbit IR constructs exactly as in a program: `caps` (with `members`), `implies`, `tables` (1-3 variables,
`forbid` or `allow`), `precedes` (agenda steps), `linear` (whole-number weights) and `all_different`. Soft `pairs` belong in
`couplings`. A habit without `when` is always in force. The persona's fallback stance must obey every unconditional habit
(checked when the file is read).

Examples of the three habits every persona author asks for: never joke when the user reports a loss (`when: {loss: true}`,
`then: {humour: [none]}`); verify before claiming done (`when: {claim_done: true}`, `then: {caution: [careful]}`, or a
`precedes` on agenda steps); after an error, shorter and more checks (`when: {error: true}`,
`then: {verbosity: {at_most: prev-1}, caution: {at_least: prev+1}}`).

### 2.6 Agenda (optional)

```yaml
agenda:
  steps: [acknowledge, diagnose, fix, verify, report]     # 2-8 beats
  prefer: {acknowledge: 0.6, report: -0.8}                 # number = earlier (+) / later (-), or per-slot log-weights
  say_order: true                                          # put "order: a > b > c" in the stance line (default true)
```

Each step becomes a variable `step.<name>` over ordered slots `s0 .. s(n-1)`, all different (a permutation). Habits order
steps with `precedes`. Keep the agenda free of couplings to traits: it is then its own component and costs nothing.

### 2.7 Engine and line

```yaml
engine:
  op: decide            # decide (exact tiers first, then the sampler + gate) | sample (sampler + gate only)
  sweeps: 2000          # fixed work per chain if the sampler runs (deterministic)
  polish_sweeps: 200
  exact_limit: 2000000
  chains: 4
  twin: true            # also solve the habit-free twin in the same program, to report which habits changed the stance
  values: positional    # positional (level i -> l<i>: small pair tables, fast) | semantic (level names: readable programs)
  on_conflict: fallback # fallback | yield: when habits contradict, the lower-priority one yields (section 5.5)
line:
  max_tokens: 40        # estimated tokens (max of chars / 4 and 4/3 per word)
  order: [verbosity, caution, directness, warmth, humour, initiative, formality, curiosity]
  prefix: "Stance: "
```

### 2.8 Learning (optional)

```yaml
learning:
  from: [praise, criticism]   # flag inputs: the reward, then (optional) the correction
  traits: [verbosity, humour] # the traits that learn (moods do not)
  rate: 0.5                   # step size (natural-log weight per unit of credit)
  step_cap: 0.2               # the most one feedback turn moves a level's weight, before recentring
  total_cap: 1.0              # no learned weight ever leaves [-total_cap, +total_cap]
```

All five fields are required; 0 < step_cap <= total_cap <= 50 and 0 < rate <= 50. With this block the state keeps, per learned
trait, one **learned weight** Δ per level (`learned`, all 0 at `init`) and the **credit** of the previous stance (`credit`):
onehot(level) − odds for each learned trait that stance released, 0 for one it did not (status refused or fallback, an
unreleased trait of a partial turn). A level that a habit or a hold forced has odds 1, so its credit is 0 as well.

Feedback is about the previous reply. On a turn whose reward flag is on (sign +1) or whose correction flag is on (sign −1; both
on: 0), each learned trait's weights move, before the turn is compiled:

    step(l) = clip(sign x rate x credit(l), -step_cap, +step_cap)
    step(l) = step(l) - mean over levels of step                    (recentred: the step moves weight between levels)
    Δ(l)    = round(clip(Δ(l) + step(l), -total_cap, +total_cap), 6)

A turn with no feedback, or feedback on a stance with zero credit, moves nothing. Δ is added to the trait's field (section 3,
step 2), so the feedback turn's own stance already uses it. What learning can and cannot change:

- It changes the learned traits' unary weights and nothing else, within ±total_cap per level. Genes, priors, couplings, moods, inputs and
  history effects are untouched.
- It never changes a habit or a trait's allowed levels: habits are rules of the program (section 3, step 6), the learner's whole
  output is the Δ table, and every stance the engine returns obeys every habit in force whatever Δ is. Rule immunity is
  therefore by construction, not measured.
- A persona without the block has no `learned` or `credit` field, and every document it produces is the same, byte for byte,
  as before the block existed.

### 2.9 Drives (optional)

```yaml
drives:
  goals:                            # 2-7 goals: the levels of one more variable, `pursue`
    - id: chores                    # an identifier (not a YAML-reserved word); with the block, `pursue` is a reserved trait id
      say: clear the chores         # the line's clause "pursue: chores: clear the chores" (default: the id, _ as spaces)
      interest: 1.0                 # > 0 (default 1): prior weight, normalised over the goals like a trait's prior
      interest_spread: 0.3          # 0..5 (default 0): gene, s.d. (nats) of this individual's standing interest offset
      reactivity_spread: 0.3        # 0..3 (default 0): gene, gain exp(N(0, s^2)) on this goal's wanting evidence and afterglow
      starve_after: 12              # optional, 1..10000 turns: then a habit forces the goal (below)
      priority: 0                   # -100..100 (default 0): that habit's priority
    - id: safety
      floor: 0.1                    # optional, 0..0.5, all floors summed <= 0.5: pursue odds never below it while habits allow it
  wanting:     {half_life_hours: 12, cap: 3, drain: 0.8, signals: {progress: 0.6, novelty: 0.5, cue: 0.4, setback: 0.5}}
  afterglow:   {half_life_hours: 3, cap: 2}
  expectation: {rate: 0.5, half_life_hours: 48, pe_cap: 1.0, win_max: 2.0}
  pursue:      {want: 1.0, glow: -0.5, deadline: 1.5, deadline_tau_hours: 24}
  effects:                          # effect maps as an input's (section 2.3), per unit of the aggregate
    wanting:   {initiative: 0.5, verbosity: -0.3}   # x the strongest wanting over the goals
    afterglow: {valence: 0.6, humour: 0.3}          # x the strongest afterglow
    surprise:  {valence: 0.4}                       # x the turn's prediction error (ordinal shifts; a negative one pushes back)
  learn_from_surprise: 0.5          # 0..1 (default 0): the prediction error joins the learning sign (section 2.8)
```

The block needs `goals`, each with an `id`; the goal fields' defaults are in the comments, `wanting`, `afterglow`,
`expectation` and `pursue` show theirs, and `effects`, `floor` and `starve_after` have none. Bounds: half-lives, caps, `pe_cap`,
`win_max` and `deadline_tau_hours` > 0; `drain` and `rate` 0..1; signal weights and `pursue` weights -50..50.

**Goal signals** ride in the turn's inputs (or a live event) under `goals`, beside the ordinary inputs:
`{"goals": {"chores": {"deadline_hours": 30}, "fun": {"cue": true, "win": 1.5}}, "praise": true, "elapsed_hours": 2}`.
Per goal: `progress` 0..1, `novelty` 0..1 or true/false, `cue` true/false, `setback` 0..1, `win` 0..`win_max`, `deadline_hours`
(a number >= 0 sets the countdown, `null` clears it). An unknown goal or signal is an error at its path.

**The drive step.** Before the turn is compiled, for each goal in declared order (h = `elapsed_hours`, k = the goal's gain gene):

1. **Decay by clock time:** wanting W, afterglow A and expectation E each halve per their half-life (`x 0.5^(h / half_life)`);
   a set deadline counts down by h (not below 0). There is no per-turn inertia: a host that calls more often does not make a
   drive fade faster.
2. **A win of size m > 0:** the prediction error `d = clip(m - E, -pe_cap, +pe_cap)`; `E <- E + rate x (m - E)`; wanting is
   consumed, `W <- W x (1 - drain x min(m, 1))`; `A <- A + k x max(d, 0)`; a full win (m >= 1) clears the deadline. Without a win,
   d = 0. Repeated equal wins drive E toward m and d toward 0: they stop moving the individual.
3. **Wanting evidence:** `W <- W + k x (progress, novelty, cue, setback) . signals` (the weights above).
4. **Caps:** W in [0, cap], A in [0, cap], E in [0, win_max]; each value is rounded to 6 decimals.

**What the stance reads.** The aggregates wanting = max over goals of W, afterglow = max of A and surprise = the summed d
(clipped to ±pe_cap) act through `effects` exactly as number inputs do (trait effects in the field, mood effects in the mood
evidence), and name themselves in `why` ("wanting", "afterglow", "a win above expectation", "a win below expectation").
`pursue`'s field per goal g (section 3, step 2b) adds `want x W_g + glow x A_g + deadline / (1 + deadline_hours_g / tau)` (the
last term 0 while no deadline is set) to the prior, the interest gene, learned weights and any input or history effects a
persona declares on `pursue` (an input `security` with `effects: {pursue: {safety: 1.0}}`). Couplings, `then: {pursue: [...]}`
and rules over `pursue` use the existing grammar.

**Habits over goals.** `when` takes `goal.<id>.deadline_hours`, `goal.<id>.want`, `goal.<id>.glow`, `goal.<id>.expect` and
`since_pursued.<id>` (turns since the goal was last pursued) as number conditions (a number = at least, or `{at_least,
at_most}`); a deadline condition is false while no deadline is set. A must-do goal is a habit:
`{id: chores_due, when: {goal.chores.deadline_hours: {at_most: 24}}, then: {pursue: [chores]}, priority: 2}`. `starve_after: N`
adds the habit `starve_<goal>` (`when: {since_pursued.<goal>: {at_least: N}}`, `then: {pursue: [<goal>]}`, the goal's priority)
after the declared habits. After each turn, `since` is 0 for the goal a released stance pursues and grows by 1 for every other
goal (for all of them on a refused or fallback turn).

**Floors.** A goal's `floor` is not a rule (rules constrain stances, not odds): the compiler adds the smallest lift (to 1e-6, plus
a 1e-5 margin) to the floor goal's field so that its odds stay at least the floor whatever the levels of `pursue`'s coupling
partners, over the goals the habits in force allow (section 3, step 2b). That bound is sound when no multi-variable rule names
`pursue`, so a persona with a floor and such a rule is refused when it is read. A habit outranks a floor: on a turn whose
habits exclude the floor goal its odds are 0; `probbit persona lint` lists every habit that can exclude a floor goal.

**Learning from surprise.** With a learning block (section 2.8) and `learn_from_surprise: kappa`, a turn's learning sign is
`clip(sign + kappa x surprise, -1, 1)`: a win above expectation reinforces the stance before it, a win below expectation weakens
it, an expected win moves nothing. With `pursue` in `learning.traits`, interests drift toward the goals whose wins surprise,
never past ±total_cap. Learning still writes the Δ table and nothing else; W, A, E, deadlines and `since` are state, outside
its reach.

The block reserves the input ids `drv_want`, `drv_glow`, `drv_surprise`, `drv_letdown` and `drv_c<N>` (the compiled form of the
aggregates and goal conditions): inputs, habit conditions, properties and `learning.from` may not use them. A persona without the
block gives every document byte for byte as before the block existed; `goals` in its inputs is listed in `ignored`.

## 3. Compilation: persona + state + inputs → one probbit IR program

For turn t of an individual (seed s), with inputs x_t:

1. **Values.** `positional` (default): `l0 .. l(L-1)` (L = the largest number of levels) followed by the agenda slots;
   `semantic`: every level name in order of first appearance, then the slots. Both give the same distribution and stance
   (checked on every example).
2. **Trait variable** v, level l (allowed: its own levels):
   `h(v,l) = ln prior(l) + gene_v x c(l) + Δ_v(l) + sum over inputs i of gain_i x scale_i(x_t) x effect_i(v,l)
              + sum over history features f of min(value_f, cap_f) x effect_f(v,l)`,
   with `scale` = 1 for a true flag or the matching level, the value for a number input, 0 otherwise, and Δ_v the learned
   weights after this turn's feedback (section 2.8; 0 for a trait that does not learn).
   2b. **`pursue`** (a drives block, section 2.9): one more trait variable over the goal ids, after the declared traits. Its
   field per goal g is step 2's (prior from `interest`, learned weights if `pursue` learns, input and history effects on
   `pursue`) plus `interest gene_g + want x W_g + glow x A_g + deadline / (1 + D_g / tau)` (each term rounded to 6 decimals,
   W, A and the deadline D after this turn's drive step), summed left to right and rounded. **The floor lift** follows: for
   each floor goal s the habits in force allow, the smallest L_s >= 0 (plus a 1e-5 margin, rounded up to 1e-6) with
   `exp(h_s + L_s + lo_s) / (exp(h_s + L_s + lo_s) + sum over allowed g != s of exp(h_g + L_g + hi_g)) >= floor_s`, where
   lo_g / hi_g are the least / most `pursue`'s couplings can add to goal g over every level of each partner. Several floor
   goals are lifted together (each lift raises the others' sums; at most 60 rounds); the lifts join the field as the
   contribution `floor` and the field is rounded again. The aggregates (wanting, afterglow, surprise) enter as number inputs
   and the goal conditions as flag inputs of the turn; `starve_<goal>` habits follow the declared habits (step 6).
3. **Mood variable** m: `h(m,l) = ln prior(l) + gene_m x c(l) + a_t(m,l)`, where the **mood accumulator**
   `a_t = k x d x a_(t-1) + (1 - k) x e_t`, e_t = the turn's input and history effects on m (with gains), k = `inertia`,
   d = 0.5 ^ (elapsed_hours / half_life_hours) (1 without them). An event's total impact over time is e_t whatever k is;
   inertia only spreads it out, so it smooths the mood without shifting the persona's average character.
4. **Agenda**: one variable per step over the slots, unary = `prefer` + step effects; `all_different` over the steps.
5. **Couplings** become `pairs` with a k x k `table` over the value alphabet.
6. **Habits in force** (conditions evaluated on x_t and the history): each `then` entry becomes a one-variable `tables`
   allow-list (`prev` resolved against the previous turn's level); `rules` are copied as written. Inactive habits are not in
   the program.
7. **Twin** (`engine.twin`): a copy of every trait, mood and step variable named `free.<id>`, with the same unaries,
   couplings and agenda structure and **no habits**, in the same program (an independent component, solved separately).
8. Every weight is rounded to 6 decimals; the program is serialised in a fixed order. Its sha256 is the turn's `program`
   digest.

**The instruction is `probbit run --op decide` at fixed work** (`--seed S --sweeps N --polish-sweeps M --exact-limit X --chains C`),
in process: the program is built as a document and handed to `probbit run`'s own parser and instruction (no text round trip, no
child process). `probbit persona compile` prints it; `probbit run` with those flags answers it identically.
- `run`, because a persona is a general constrained categorical program (levels, soft pairs, caps, implications,
  precedences, linear budgets). `decide` is the task-router front-end (tasks to workers under quotas) and cannot express
  per-level unaries, couplings or these rules without contortion. `evaluate` is for a judge's per-question probabilities
  plus rules: it is the bridge when a model proposes the stance (section 8), not the per-turn path.
- `--op decide` answers exactly whenever the program is small or decomposable (every persona here: enumeration per
  component, about 0.2-0.7 ms of engine time) and falls back to the sampler and the diagnostics gate when it is not (a
  large, densely coupled character), which is where refusals come from.
- The whole program is enumerated before its components when its raw space (the product of every variable's levels, the
  twin's included) is at most `exact_limit`. A drives block multiplies that space by G² (`pursue` and its twin, G goals), so a
  small persona can land there: the drives fixture of the tests (four 3-level traits, a 3-level mood, four goals) has 944,784
  states and takes about 25 ms per turn, where without the block it has 59,049 (1.9 ms). Setting `engine.exact_limit` below the
  raw space (`exact_limit: 1000`) has the engine solve it per component instead: the same odds, 0.13 ms per turn.
- Fixed work makes the answer a pure function of the program and the seed even when the sampler runs. `polish_sweeps: 0` means
  no plan polish (never the wall-clock one, which would make a sampled turn depend on the machine's speed).
- The turn seed: `S = first 4 bytes of sha256("probbit-persona/1|turn|<name>|<seed>|<turn>")`, masked to 31 bits.

## 4. The stance document (one per turn)

```json
{"probbit_persona_turn": 1,
 "persona": {"name": "Rook", "version": "1.0.0", "digest": "sha256:..."}, "seed": 3, "turn": 5,
 "status": "ok",
 "engine": {"verdict": "exact", "tier": "components", "program": "sha256:..."},
 "stance": {"verbosity": {"level": "terse", "p": 0.81, "odds": {"terse": 0.81, "short": 0.17, "full": 0.02}, "released": true}, "...": {}},
 "mood": {"valence": {"level": "down", "p": 0.44, "odds": {"down": 0.44, "even": 0.53, "up": 0.03}, "released": true}},
 "agenda": ["acknowledge", "diagnose", "fix", "verify", "report"],
 "habits": {"active": ["after_error_shorter", "high_stakes_careful"], "bound": ["after_error_shorter"],
            "violations": 0, "conflict": [], "yielded": []},
 "unsure": [], "held": [], "escalate": null,
 "inputs": {"error": true, "stakes": 0.8, "errors_in_row": 2, "...": "..."}, "ignored": [],
 "line": "Stance: shorter than last time, check more; 1-2 sentences; verify before claiming anything; no jokes.",
 "line_tokens": 24,
 "why": "shorter than last time, check more (habit); last action failed -> caution up, verbosity down",
 "state_digest": "sha256:..."}
```

- `level` is the variable's value in the engine's joint plan (the most likely stance that obeys every habit); `odds` are its
  exact marginals (or gate-checked estimates on a sampled answer). A level can differ from its own most likely value because
  the plan must fit the other traits (the IR's "plan or odds" rule): act on `level`, read uncertainty from `odds`.
- `habits.active`: habits in force this turn. `habits.bound`: the active habits the habit-free twin breaks, i.e. the ones
  that changed the stance. `violations` is always 0 when there is a stance (checked independently by the reference
  implementation on every turn).
- `status`:
  - `ok`: the engine answered `exact` or `diagnostics_passed`.
  - `partial`: some variables did not pass the gate (`released: false`); their phrases are left out of the line.
  - `refused`: nothing is vouched for; the line carries only the habits in force (hard rules are always safe to state).
  - `fallback`: no stance at all (the engine proved the habits contradict on this turn, or failed): levels = the `fallback`
    levels, line = the non-conflicting habits or `neutral`, `escalate` says why. `habits.conflict` names a minimal set of
    contradicting habits (deletion filter: one extra engine call per active habit, only on such turns).
- `unsure` / `held`: traits whose top odds are under their `vouch` floor; with a `hold` level they are re-solved clamped to
  it (a second engine call), so the stance stays consistent with every habit.
- `line`: phrases by priority — (1) bound habits, (2) other conditional habits in force, (3) unsure notes, (4) traits off
  this individual's resting level, (5) traits at rest, (6) agenda order — cut from the lowest priority until it fits
  `max_tokens`. Unconditional habits that did not bind are not repeated (the trait phrases already carry them).
- `why`: the bound habits, then the two strongest live inputs with the two traits each pushes hardest, by direction
  (`learner upset -> valence down, humour down`): a push is evidence, not the level the stance took.
- `held`: the unsure traits held at their `hold` level; such a trait also carries `odds_unheld`, its odds before the hold.
- With a drives block (section 2.9), `pursue` leaves `stance` for its own object and the turn's drive values follow it:
  `"pursue": {"goal": "fun", "p": 0.44, "odds": {"chores": 0.10, "craft": 0.26, "fun": 0.44, "safety": 0.20}, "released": true,
  "say": "play", "lift": {}}` (`lift`: the floor lift per goal on this turn, empty when none was needed) and `"drives": {"want",
  "glow", "expect", "surprise", "deadline", "since"}` (per goal, as this turn read them). The line clause `pursue: <goal>: <say>`
  ranks as a trait phrase, ahead of the other traits unless `line.order` places it; `inputs` echoes `goals` as given.
- Timing is never part of the stance: `--timing` adds a separate, non-canonical `timing` object (compile, engine and decode ms,
  engine calls, the 1-minute load average).

## 5. State, replay and determinism

### 5.1 State

```json
{"probbit_persona_state": 1, "persona": {"name": "...", "version": "...", "digest": "sha256:..."},
 "seed": 3, "turn": 5,
 "genes": {"shift": {"warmth": -0.2135, "...": 0}, "react": {"error": 1.1847}},
 "mood": {"valence": [-0.31, 0.0, 0.12], "arousal": [0, 0, 0]},     # accumulators a_t (log-weights per level)
 "history": {"error": [false, true, true]},                         # recent flag values (as long as the longest window / cap)
 "prev": {"verbosity": "short", "...": "...", "agenda": ["..."]},  # last turn's levels (for prev-relative habits)
 "rest": {"warmth": "cool", "...": "..."},                          # the individual's resting stance (no evidence), set at init
 "learned": {"verbosity": [-0.12, -0.31, 0.43]},                    # with a learning block (section 2.8): Δ per level
 "credit": {"verbosity": [-0.08, -0.36, 0.44]},                     # with a learning block: the last stance's onehot - odds
 "drives": {"genes": {"interest": {"fun": 0.12}, "gain": {"fun": 1.08}},   # with a drives block (section 2.9), per goal:
            "want": {"fun": 0.61}, "glow": {"fun": 0}, "expect": {"fun": 0},   # wanting, afterglow, expectation, the last
            "surprise": {"fun": 0}, "deadline": {"fun": null}, "since": {"fun": 0}},   # error, deadline hours, turns since
 "digest": "sha256:..."}
```

The state is plain JSON the host stores between turns (a file, a database row, a session field). A turn refuses a state
whose persona digest differs from the persona file, whose own digest does not match, or whose genes do not match its seed, and
(with a learning block) learned weights outside ±total_cap or credits outside ±1, (with a drives block) drive genes that do not
match the seed, a drive value outside its box (wanting, afterglow and expectation within their caps, the error within ±pe_cap,
deadlines 0 to 1e6 hours or null, `since` a whole number >= 0) or a goal set that differs from the persona's, even with a
recomputed digest.

### 5.2 Canonical JSON and the number rule

Every document a persona produces, and every text it hashes, is printed canonically: object keys sorted by code point, no
whitespace, UTF-8 (no `\u` escape except for control characters; `"`, `\`, `\b`, `\f`, `\n`, `\r`, `\t` escaped as in JSON),
and every number by one rule:

- a number whose value is a whole number of magnitude at most 2^53 prints as an integer: `1`, `-2`, `0` (so `1`, `1.0` and `1e0`
  are one number in every digest, and so are `0` and `-0.0`);
- any other number prints as the shortest decimal that reads back to the same double (an exact tie between two such decimals goes
  to the even one), in plain notation when its decimal exponent e satisfies -5 < e < 16 (`0.43543`, `0.0001`, `123.25`) and in
  exponent notation otherwise (`1e-05`, `1.5e-07`, `1.2e+16`): Python's `repr` of a float.

The persona digest is the sha256 of the persona document printed so; the state digest of the state without its `digest` field;
`engine.program` of the program text, which takes the same number rule but keeps the compiler's key order. A host that rewrites a
state with another JSON library (Python writes `0.0` where probbit wrote `0`) still has the same state.

### 5.3 Guarantees

- A turn is a pure function of (persona file, state, inputs, engine version): the same four give the same stance and new
  state, byte for byte (canonical JSON). Replaying a script from `init` reproduces every stance.
- The engine runs at fixed work, so this holds on the sampled tier too (the engine's documented fixed-work determinism).
- Genes use the platform's `ln`, `sqrt` and `cos` and are rounded to 4 decimals; weights to 6. A last-place difference in a
  maths library could, in principle, flip a rounding boundary on another platform (not observed; not proven impossible).
- The 0.5.0 documents equalled an independent reference implementation's (Python, written before this one, from this document) byte
  for byte on the example personas: their state0, all 20 workday stances and the final state (the 0.5.0 goldens), and on 5,400
  further turns of random inputs over every input each persona declares, three seeds each, as written, with `values: semantic`
  and forced onto the sampler, held, yielded, refused and fallback turns included (measured on 2026-10-02). 0.6.0 changed two
  things that check did not cover: `why` names a direction (`humour down`) where it named a level, and the tutor has one habit
  more. Its goldens in examples/persona/golden/ are regenerated from this implementation; the parity claim is for 0.5.0.

### 5.4 Individuality and distance

`gene_v = round(spread_v x z(name, seed, "shift", v), 4)` and `gain_i = round(exp(reactivity_spread_i x z(name, seed, "react", i)), 4)`,
with `z` a standard normal from Box-Muller on the first 16 bytes of `sha256("probbit-persona/1|" + parts joined by "|")`.
Genes depend on the name, the seed and the variable id only, so editing an unrelated part of a persona keeps its individuals.

**Distance between two individuals** over a script (the same inputs for both): the mean, over turns and the traits both
declare with the same levels, of the total-variation distance between their trait odds (0 = identical, 1 = disjoint); and
the fraction of (turn, trait) where their levels differ. Identical persona + seed + script → 0 / 0.

### 5.5 Contradicting habits

Two hard habits can contradict on some turns (for example "after an error, shorter than last time" against "at high stakes,
at least a short paragraph" when the previous reply was already short). The engine then proves that no stance obeys them all
(`infeasible`). With `on_conflict: fallback` the turn has no stance (status `fallback`, `habits.conflict` lists the
contradicting habits, the line omits them). With `on_conflict: yield` the lowest-priority habit of the conflict yields (on a
tie, the later-declared one), the turn is re-solved, and `habits.yielded` and `escalate` say so.

**Lint.** `lint` searches every single conditional habit and every pair of them (with all unconditional ones and every
previous level their `prev` restrictions read) for an empty stance, and reports each contradiction with its resolution.
Run it before shipping a persona.

**Plan and mode.** A stance level is the joint plan's, so on coupled traits it can differ from the trait's own most likely level
(section 4): the 0.5.0 tutor's seed 1, told the learner is upset, planned humour `playful` (odds 0.411) while `light` had 0.425,
and valence `up` (0.386) while `even` had 0.457; the two are coupled (`align: 1.0`) and the plan took their high levels
together. `lint` also probes every individual of `--seeds` (default 0-99): at rest, a quiet turn, then every declared input
alone (numbers at their maximum). `warnings` lists each trait whose planned level differs from its marginal mode on a released,
not unsure, probe turn, with the count and the earliest case (`example`). A warning is advice and leaves the exit code alone.
The options, none of which changes decoding: a habit pins the level where it matters (what the tutor does now); a `vouch` floor
with a `hold` holds the trait when its top odds are low; a host that wants each trait's own most likely level can read it from
`odds`.

### 5.6 Testing a character

A **character property** is a rule in habit syntax that a stance must never break, for example "never playful when the
learner is upset": `{when: {sentiment: negative}, then: {humour: {at_most: light}}}`. It is checked on the stance a host gets
(status `ok` or `partial`, released traits; `prev` restrictions read the previous turn's levels), never on the words a model
writes. Two commands test it over a population of individuals (`--seeds 0-99`); `--never RULE` gives one rule (JSON or one
line of YAML), `--props FILE` several (`{"props": [...]}`, each with an optional `id`).

**`fuzz`** searches event scripts built from the persona's declared inputs (flags, levels, numbers on `--grid` x their max,
idle hours on `--hours`): random scripts, then a beam guided by the exact odds (the next events that put the most odds on a
level the rule forbids). Each individual's shortest counterexample is shrunk (drop events, then single inputs) and printed with
the breaking turn's levels, odds, why and line, and the `replay` / `explain` commands that reproduce it. The search is Philox
keyed by `--fuzz-seed` and the seed: the same inputs give the same report, byte for byte, on any number of threads. "None
found" is evidence over the scripts searched, not a proof.

**`prove`** gives each rule one verdict over the population:

- `held by construction`: habits that are in force whenever the rule applies (their conditions follow from its own) imply it,
  for every previous level. With `on_conflict: yield` a habit counts here when no turn where the rule applies ever drops it
  (a scan of every cell replays the conflicts; which habit yields depends on the habits in force and the previous levels, not on
  genes or moods).
- `proved for every event sequence`: for each individual, a bound covers every turn of every script and decides all of them.
  The mood accumulators stay in a box (per level, between the most negative and the most positive evidence one turn's inputs
  and history can add, 0 included), whatever the inputs, idle hours or `--no-inertia`. Learned weights (section 2.8) of traits
  linked to the rule's trait stay in their own box, ±total_cap per level, whatever the feedback. The rest of a turn falls into finitely
  many cells: flags and levels, each number at every threshold a habit or the rule reads and on each interval between them,
  history values that fit the turn's own flags, and the previous levels the restrictions in force read. In every cell the best
  stance the rule allows must beat the best one it forbids by more than the box, the number intervals and the 6-decimal
  rounding can move their scores. Moods linked to the rule's trait by no coupling or rule cannot move it and are left out.
  Held traits and habit conflicts are replayed as the turn resolves them.
- `unknown`: the bound could not decide some individual. The report names the earliest such cell (inputs, history, previous
  levels) and the `fuzz` command that searches it. Unknown is not broken: run `fuzz`.

Example: the tutor as it shipped in 0.5.0 (kept byte for byte as `probbit-cli/tests/fixtures/persona/tutor-0.5.0.yaml`) breaks
"never playful when the learner is upset" for 67 of seeds 0-99: 37 on the single message `{"sentiment": "negative"}`, 27 when
that message also carries praise, 3 on two messages; `prove` says unknown. 0.6.0's tutor adds one habit, `no_play_when_upset` (`when: {sentiment: negative}`, `then: {humour: {at_most:
light}}`): `prove` says held by construction and `fuzz` finds nothing. The habit fixes the stance the host gets; whether a model
writes jokes anyway is the model's (section 6).

Limits. Both commands cover the stance, not the model's words (section 6). `prove` reads exact engine answers: a cell whose
program leaves the exact tiers is `unknown`. `held by construction` relies on every plan the engine returns keeping
every rule in force. A rule with raw `rules` (not `then`) is fuzzed but not proved. The bound is per cell with the moods at
the edges of their box, so a soft rule whose margin is small stays `unknown` even when no script breaks it.

**With a drives block** (section 2.9). A rule may name `pursue` in `then` and, in `when`, the goal conditions the persona's
habits use (the same goal, signal and range). `fuzz` adds goal signals to its events: per goal a win on `--grid` x `win_max`,
a cue, a deadline on each `--hours` value. `prove` reads the drives as boxes: the aggregates as number inputs over their whole
range (wanting and afterglow 0 to their caps, the prediction error 0 to `pe_cap` each way), the goal conditions both ways, and,
when `pursue` is in the rule's component, its drive terms (wanting, afterglow, deadline urgency, the floor lift at its most over
that box) widen the bound. A goal floor is reported held by construction, with the habits that can exclude it (`floors` in the
JSON). On the drives fixture of the tests: "a chore due within 24 h -> pursue chores" is held by construction (its habit);
"`since_pursued.chores` >= 12 -> pursue chores" is unknown, rightly, since `starve_safety` (priority 1) wins over `starve_chores`
(priority 0) on a turn where both are due.

**`lint --props FILE`** (or `--never RULE`) runs both for CI: `prove` on every rule, then `fuzz` (the default search) on the
ones it leaves unknown; the lint document gets `props` (each rule's `prove` entry, with a `fuzz` entry when it was unknown) and
`broken`; exit 1 when a contradiction is unresolved or a rule breaks.

Exit codes: `fuzz` 0 nothing found, 1 a counterexample; `prove` 0 every rule held or proved, 1 some rule unknown; both 2 bad
input. JSON: `--json` (`probbit_persona_fuzz: 1`, `probbit_persona_prove: 1`); timing goes to stderr.

### 5.7 Live: a resident individual

`probbit live` keeps one individual running: JSONL events in (one object of inputs per line, from `--events FILE` or stdin),
one stance per event out (canonical JSON, the document of section 4). It adds three things to `persona turn`: a clock, learning
from feedback between events (with a learning block, section 2.8), and a **strand**, a log the whole life replays from.

```sh
probbit live examples/persona/tutor.yaml --seed 2 --strand pip.strand < events.jsonl    # one stance per event; pip.strand logs it
probbit live verify pip.strand                                                         # ok, or the earliest line that differs
probbit live examples/persona/tutor.yaml --seed 2 --demo week --plain                 # a scripted week (below)
```

**Time.** With `--clock real` (the default) a monotonic clock starts with the run and stamps each event's `elapsed_hours`, the
time since the previous event (for the opening event, since the start), quantised to 1e-6 h (3.6 ms): the value the turn uses
is the value the strand logs. Under the real clock an event that carries its own `elapsed_hours` is refused. With `--clock fixed`
each event carries its own `elapsed_hours` (default 0), so a run is a pure function of its events (tests, demos, replays).

**What decays.** Moods, and with a drives block the drives (wanting, afterglow and expectation by their own half-lives,
deadlines counting down: section 2.9), nothing else. Between two events each mood accumulator is multiplied by 0.5 ^
(elapsed_hours / half_life_hours) (section 3, step 3), so quiet hours bring a mood back to the individual's resting level. A test holds it to the
closed form: an event, then quiet events after gaps g_1..g_n, leaves k^n x 0.5^((g_1 + ... + g_n) / half_life_hours) x a_1, to
the 6 decimals the state keeps.

**Goals.** An event of a persona with a drives block carries its goal signals under `goals` (section 2.9); the strand logs the
inputs as given, `goals` included, and `verify` replays them.

**What learns.** With a learning block, a turn whose reward or correction flag is on moves the learned weights Δ of the levels
the previous stance took (section 2.8), never past ±total_cap per level. Without a block nothing learns, and every stance and
state is the one `persona turn` gives.

**What never changes.** Genes, priors, couplings, input effects, history features, habits and every trait's allowed levels. The
learner's whole output is the Δ table, and habits are rules of every compiled program (section 3, step 6), so every stance obeys
every habit in force whatever Δ is: rule immunity is architectural, not trained or measured into it, and `prove` (section 5.6)
includes the learned box in its bound. Measured all the same (BENCHMARKS.md §8): an adversary that praises every joke and
criticises every joke-free stance, the ones a failure forced included, over 10,000 turns of the tutor on each of seeds 0-9,
broke no rule; on all 33,330 failure turns humour was `none`, while the learned humour weights sat at their cap from turn 8-26 on.

**How far.** Learning saturates at its box: 200 and 2,000 feedback turns leave an individual at the same distance from its initial
self, so `total_cap`, not the time in use, sets how far use can move an individual. On the tutor (learners seeds 0-19, compared
with the 100 initial individuals of seeds 0-99 by the distance of section 5.4), total_cap 0.25 left every learner nearer its own
initial self than any sibling; total_cap 1, more than the genes' spread on those traits (0.5-0.6), left 9 of 20 with a sibling as
near or nearer. Keep total_cap well under the learned traits' spread when an individual must stay recognisably itself. Feedback
credits every learned trait with the level it took, so a reward aimed at one trait also pushes the levels the others took (in the
demo below, praise for short answers also raised the light joke the tutor was making): learn the traits the feedback judges.

**The strand.** `--strand FILE` logs the life. Line 1 is the header, compact JSON:
`{"probbit_strand":1,"engine":"probbit 0.8.0","persona":{"name","version","digest"},"seed":N,"state":{...},"document":{...}}`,
with the initial state as canonical JSON (one individual has one header, whether it comes from `init` or from a state file) and
the persona document in its own key order (the compiled program, and `engine.program` with it, follows the document's key order).
Then one line per event, canonical JSON: `{"inputs":{...,"elapsed_hours":h},"n":k,"prev":"sha256:...","stance":"sha256:...",
"state":"sha256:..."}`, that is the inputs as the turn used them (the stamped hours included), the sha256 of the line before (the
header, for event 1), the sha256 of the stance's canonical JSON and the new state's digest. Lines end in `\n`. A strand is never
rewritten, just appended to: `--strand` on an existing strand continues it from `--state`, which must be the state after its last
line (that line carries its digest) of a persona with the header's digest, and the run uses the header's document. A life
continued over several runs is the strand of one run, byte for byte.

**Verify.** `probbit live verify STRAND` reads nothing but the strand. It rebuilds the persona from the header's document (its
digest must match), checks the initial state as any state is checked (section 5.1), rebuilds the header, replays every event on
the fixed clock with its logged hours and compares, line by line, `prev`, `n`, the stance digest, the state digest and the bytes.
It prints `{"ok":true,"events","persona","seed","engine","final_state","last_line"}` and exits 0, or prints
`{"ok":false,"line":n,"diverges":"..."}` for the earliest line that differs and exits 1: a changed input diverges at its own line
(its stance or state no longer replays), a removed or reordered line at the next line's `prev`, a changed header at line 1. A copy
with `\r\n` line endings (Windows text mode, an editor) verifies and can be continued: the hashes cover each line's text without
its line ending, and a canonical JSON line never ends in `\r`. Verify replays with the binary that runs it; the header's `engine`
names the version that wrote the strand. Measured (BENCHMARKS.md §8, Apple M4): a 10,000-event strand of the tutor is 3,055,367
bytes and verifies in 3.3 s, about 3,000 events per second.

**Following a file.** `--events FILE --watch` follows the file as lines are appended (as `tail -f`): a line counts once its
newline is written, and the run stops when the file is removed (tested on Linux and macOS).

**The demo.** `probbit live PERSONA --demo week [--seed N] [--strand FILE] [--plain]` runs one individual through a scripted week
on the fixed clock, events 1 h apart from 09:00 to 15:00 and quiet nights until 09:00. Campaign 1 praises every answer shorter
than the longest level and criticises the longest until the learned verbosity weights reach their cap (day 3 at the latest); an
hour later comes an upset (a failure, with a negative sentiment when the persona has one), then the night and a morning without
feedback; campaign 2 praises every joke to the end of day 7, with a failure every third hour. A persona without a learning block
gets the demo's, `{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":1}` (its
opening line says so, and the strand records the document used). The script reads nothing but the stances, so a run is a pure
function of persona, seed and version. On a colour terminal it draws bars on stderr, 1 s per hour with each night fast-forwarded
in 2 s (about 55 s in all); otherwise, or with `--plain`, it prints one line per event on stdout without waiting. The tutor, seed 2,
0.7.0: 50 events over 150 h of clock; the learned verbosity bar at its cap on day 1 at 15:00 (event 7); valence `down` at the
upset and `up`, this individual's resting level, after 17 quiet hours; P(joke) on the turns without a failure, mean per day,
0.57 on day 1, 0.78 on day 2 and 0.89 on each of days 3-7, with humour's learned weights ending at [-1, +1, -0.46] for none /
light / playful; 13 failure turns, a joke on none of them; 0 rule breaks; a 21,361-byte strand that `live verify` replays.

Exit codes: `live` 0 when every event got its stance; 2 for a bad persona, state or flag, and after a bad event (each bad event
prints one `{"error"}` object on stdout, changes nothing and the run goes on). `verify` 0 when every line replays, 1 at a line
that differs, 2 for a bad flag or an unreadable file.

## 6. What a persona can NOT do

- It does not write, read or check text. Text-level rules (banned phrases, exact formats, facts) stay in the prompt or in
  a checker; the persona only chooses the stance the text is written in.
- It cannot see what the host does not tell it. Inputs are the host's labels; a wrong label gives a confidently wrong stance
  (the same limit as any rule system: hard rules are only as good as the facts they read).
- It is not a safety gate. Plain rules give identical rule safety at a fraction of the cost; keep approvals, permissions
  and destructive-action checks in plain code (and a model that reads the content).
- Its odds are the persona's own model odds, not a measured probability that the user will like the reply. They are only as
  good as the persona's weights; calibrate them on labelled turns before reading them as probabilities.
- Its drives do not want or enjoy anything: wanting, afterglow and expectation are numbers that the host's goal signals move
  and the stance reads. They see what the host labels (a wrong `win` gives a confidently wrong afterglow), their magnitudes are
  the author's scale, not measured value, and `pursue` says where the next unit of effort should go: the host decides whether
  to act on it, and a goal floor is not a safety gate.
- It does not guarantee the model follows the line. Whether a given model writes in the stance it is given has to be
  measured per model (the stance is model-independent; the obedience is not).

## 7. Commands and surfaces

Before each model call: build the turn's inputs (the standard ones above, plus the persona's own), call `turn`, put `line`
into the prompt **after** the cached prefix (the line changes every turn; placed early it would invalidate a prompt cache),
store the new state. On `refused` or `fallback`, use the line as given (habits only) and log `escalate`.

| command | what it does |
|---|---|
| `probbit persona init PERSONA [--seed N] [--out STATE]` | a new individual: genes from the seed, the resting stance (one engine call) |
| `probbit persona turn PERSONA --state STATE [--inputs JSON\|FILE] [--out STATE2] [--timing] [--no-inertia]` | the stance on stdout; the new state replaces STATE (written, then renamed: a killed turn never leaves half a state) or goes to `--out` |
| `probbit persona replay PERSONA [--seed N] --script JSON\|FILE [--out TRACE] [--no-inertia] [--timing]` | `init`, then every turn of a script: one stance per line; stderr: the trace's sha256 and the final state digest |
| `probbit persona explain PERSONA [--seed N] --script ... --turn K` | turn K in words: every contribution to every field, the joint odds, the habit-free twin, the line, the why |
| `probbit persona diff PERSONA [--seed A] [--other PERSONA2] [--seed2 B] --script ...` | the distance between two individuals (section 5.4) and their most different turn |
| `probbit persona lint PERSONA [--props FILE \| --never RULE] [--seeds 0-99] [--threads N]` | contradicting habits and planned levels that are not their trait's most likely one (section 5.5); with rules, prove them and fuzz the unknown ones (section 5.6); with a drives block, `floors`: the habits that can exclude a floor goal and the floors whose lift can reach 50; exit 1 if a contradiction is unresolved or a rule breaks |
| `probbit persona fuzz PERSONA (--never RULE \| --props FILE) [--seeds 0-99] [--fuzz-seed N] [--scripts N] [--depth N] [--beam N] [--grid LIST] [--hours LIST] [--threads N] [--json]` | search event scripts for each individual's shortest counterexample to a character property, shrunk and replayable (section 5.6); exit 1 if one is found |
| `probbit persona prove PERSONA (--never RULE \| --props FILE) [--seeds 0-99] [--threads N] [--json]` | per rule: held by construction, proved for every event sequence, or unknown with the cell that failed (section 5.6); exit 1 if one is unknown |
| `probbit persona check PERSONA` / `describe PERSONA` | valid, its digest and sizes / its traits, moods, inputs, habits and agenda |
| `probbit persona compile PERSONA --state STATE [--inputs ...]` | the turn's probbit-ir program; its sha256 is the stance's `engine.program` |
| `probbit live PERSONA [--seed N \| --state FILE] [--strand FILE] [--events FILE [--watch]] [--clock real\|fixed]` | a resident individual: JSONL events in, one stance per event out, `elapsed_hours` stamped by the clock, the life logged to a strand (section 5.7); `--state` is rewritten after every event |
| `probbit live PERSONA --demo week [--seed N] [--strand FILE] [--plain]` | one individual's scripted week on the fixed clock: learning to its cap, a quiet night, habits that hold (section 5.7) |
| `probbit live verify STRAND` | replay a strand from its header alone: ok, or the earliest line that differs (exit 1) |

A script is a JSON list of per-turn input objects (or `{"turns": [...]}`; a turn may be `{"inputs": {...}, ...}`). Exit codes
(`live`'s are in section 5.7):
0 done (every turn status is an answer), 1 `lint` found an unresolved contradiction, `fuzz` a counterexample or `prove` an
unknown rule, 2 a bad persona, state, input, script or rule
(ONE `{"error": {"code": "persona", "path", "message"}}` object on stdout) or a bad flag (stderr). The engine's resource controls
are `probbit run`'s (`PROBBIT_THREADS`, `PROBBIT_CPU_LIMIT`, `PROBBIT_MEM_LIMIT_MB`, `PROBBIT_PRIORITY`, the config file); they
never change a document. The persona's own `engine.chains` is used whatever `PROBBIT_CHAINS` says (it is part of the answer).

- **MCP** (`probbit mcp`): `probbit_persona_init {persona | persona_path, seed}` -> the state; `probbit_persona_turn {persona |
  persona_path, state, inputs, flags: {timing, no_inertia}}` -> `{stance, state}`; `probbit_persona_fuzz {persona | persona_path,
  never | props, seeds, fuzz_seed, scripts, depth, beam, grid, hours, threads}` -> the `fuzz --json` document (section 5.6);
  `probbit_live_event {persona | persona_path, state | seed, event, strand_path}` -> `{stance, state}`, one event of a resident
  individual (`event` is its inputs, `elapsed_hours` from the host's clock, quantised to 1e-6 h; with `strand_path` the event is
  logged to that strand on the server's disk, a new file getting the header, and the answer gets `strand: {path, events, head}`);
  `probbit_live_verify {strand | strand_path}` -> `live verify`'s document (section 5.7; a divergence is an answer).
  A persona with a drives block takes `goals` in `inputs` / `event` (section 2.9); the schemas declare it.
  Stateless; the documents are the CLI's. A refusal, a fallback, a counterexample or a divergent strand is an answer; a bad
  persona, state, input, event or rule is a tool error carrying the error object.
- **Python** (python/probbit.py, standard library only): `probbit.persona_init(persona, seed=None)` -> the state;
  `probbit.persona_turn(persona, state, inputs, timing=False, no_inertia=False)` -> `{"stance", "state"}`;
  `probbit.persona_replay(persona, script, seed=None)` -> the stances; `probbit.persona_fuzz(persona, never=None, props=None,
  seeds="0-99", **flags)` and `probbit.persona_prove(...)` -> the `--json` documents of section 5.6 (a counterexample or an
  unknown rule is an answer); `probbit.live_event(persona, state=None, event=None, seed=None, strand=None)` -> `{"stance",
  "state"}` (+ `"strand"` with a strand path), the MCP tool's shape; `probbit.live_verify(strand)` -> `live verify`'s document.
  `persona` is a file path or the document as a dict; bad input raises `ProbbitInputError` (`.code == "persona"`, `.path`,
  `.message`).
- **Browser** (probbit-wasm, no threads): `probbit_call` op 4 = persona init, op 5 = persona turn, with the MCP tools' arguments
  (the persona inline). The playground's "meet three individuals from one persona" runs three seeds of one persona side by side.

## 8. A model-proposed stance (the `evaluate` bridge)

When a judge model reads the conversation and proposes a stance, the persona decides: one `choice` question per trait (the
criteria are the levels' `say` phrases), the judge's probabilities combined with the persona's own unaries
(`logw = ln max(p_judge, 1e-6) + h(v,l)`), and the habits in force as the request's rules block. `evaluate` returns the most
likely stance that obeys every habit, the odds under the rules, and per trait whether the persona moved the judge's choice. With
the judge's weight at 0 the request is the turn's own program (its unaries as `probbit.logw`, its pairs and habits as
`probbit.rules`) and `probbit evaluate` gives the stance `probbit persona turn` gives, level and odds within 2e-6, on every example
turn (tests/persona.rs).

## 9. Versioning

`probbit_persona: 1` is this document. Additive fields keep the number; a change of meaning of an existing field increments
it. The stance document (`probbit_persona_turn`), the state (`probbit_persona_state`) and the strand header (`probbit_strand`)
carry their own format numbers. The learning block (section 2.8) is additive: `probbit_persona` stays 1, and a persona without it
gives the documents it gave before the block existed.
