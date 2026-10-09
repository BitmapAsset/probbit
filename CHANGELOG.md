# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## 0.8.0 - 2026-10-09

### Added
- **Drives** (docs/persona.md §2.9): persona: optional `drives` block. Goals with wanting, afterglow and an expectation per
  goal; a win moves an individual by its prediction error (magnitude minus expectation), so repeated equal wins stop moving it;
  a new `pursue` output says which goal gets the next unit of effort, with exact odds, under habits; goal floors and
  `starve_after` keep goals from being out-wanted or starved. A persona without the block gives byte-identical documents.
  Read strictly (`probbit_persona` and `probbit_persona_state` stay 1): 2-7 goals (interest, gene spreads, floor, starve_after,
  priority), wanting / afterglow / expectation / pursue weights, effects of the strongest wanting, the strongest afterglow and
  the turn's prediction error, `learn_from_surprise`. Goal signals ride in the inputs under `goals` (progress, novelty, cue,
  setback, win, deadline_hours). Each turn: decay by clock hours, the win's prediction error and Rescorla-Wagner update,
  consumption, afterglow, wanting evidence, caps (6 decimals); `pursue` is one more variable whose field adds the drive terms;
  a goal floor is a lift of that field, sound when no multi-variable rule names `pursue` (such a persona is refused); habits
  read `goal.<id>.deadline_hours|want|glow|expect` and `since_pursued.<id>`, and `starve_after` adds a `starve_<goal>` habit;
  with a learning block the prediction error joins the learning sign. The state gains `drives` (genes per seed, the values per
  goal), checked on read. The stance gains `pursue` and `drives`, the line a `pursue: <goal>: <say>` clause.
- Drives on every surface: `persona turn / replay / explain / describe / check / compile / diff`, `live` events with `goals`
  (strands log them, `live verify` replays them), MCP, the Python wrapper and the browser module. `lint` lists the habits
  that can exclude a floor goal and the floors whose lift can reach 50; `prove` reads the drives as boxes and reports goal
  floors as held by construction; `fuzz` adds goal signals to its events.
- `bench/drives_adversary.py` and BENCHMARKS.md §9: an adversary that wants one goal, over N individuals x 10,000 turns through
  `probbit live`. On the drives fixture, 100 x 10,000 turns: 0 habit breaks, every must-do turn (6,743) pursued the due chore,
  the safety floor of 0.1 held on every turn its habits allowed (least odds 0.141), and every individual ends in the state the
  Python prototype of the design reaches on the same events (100 of 100 digests).

- **`probbit monitor STRAND`** (docs/persona.md §5.8, probbit-cli/src/monitor.rs): watch an individual's inner state as live
  horizontal bars. It replays the strand with the rules of `live verify` (`live::Replay`, now shared by both), recomputing every
  stance document from the strand alone, and draws the latest event: each trait's levels with their odds (the level taken
  highlighted, its phrase), the moods with a sparkline of the last 50 events, the event's inputs and history features, the habits
  in force (the bound ones marked, the violations counter), the learned deltas within their cap, the drives when a document
  carries them, and the stance line. `--follow` replays appended lines within a second (a truncated or rotated strand is replayed
  from the start, with a warning), `--once` prints one frame, `--plain` draws in plain ASCII. `probbit monitor --demo` plays the
  tutor's scripted week (the week `live --demo week` writes, seed 2, through a temporary file it removes at once), paced 1 s per
  hour and each night in 2 s. Exit 0, 1 at a line that differs (named in the frame), 2 for a bad flag or a file that is not a
  strand. It changes no file and sends nothing; no dependencies.
- **`probbit monitor STRAND --serve [--port N] [--open]`** (also with `--demo`): the same board as a page in the browser. A server
  on 127.0.0.1 (std `TcpListener`; it binds no other address, and refuses requests addressed to another host) follows the strand
  and serves one page embedded in the binary (styles and script inline; it fetches no fonts, styles or scripts), its server-sent
  events (the layout, the latest frame, then a frame per event, a heartbeat every 15 s) and `/doc/N`, event N's stance document
  as `probbit live` printed it. The URL is the line on stdout; `--open` serves the same and starts the default browser, best
  effort. `probbit monitor --demo --open` plays the week over and over in the browser.

- **The safety kit in the engine** (docs/persona.md §2.10, §5.7). Four guards an autonomous loop needs, each tested; personas
  without the new key and strands without the new lines give 0.8.0's documents and strands, byte for byte.
  - **Reward provenance**: an event may say who produced it (`src`: `human[:id]`, `env[:sensor]`, `self`, `clock`), and a
    persona may declare `reward_from` (a list of `human` / `env`, `any`, or one per reward-bearing input: the learning flags and
    `goals.<id>.win`). A reward from `src: self`, without a source or from an undeclared one is refused whole, as a bad event
    is; `src` is read (not `ignored`), echoed in the stance's inputs and logged in the strand. A run equals the same run with
    its refused events removed (P3: 40 individuals x 300 random events). A closed self-reward loop (300 events of praise and a
    win from `src: self`) is refused 300 times and leaves the individual in its initial state, byte for byte.
  - **One writer per strand**: `live --strand` takes `STRAND.lock` (created exclusively with the writer's pid and start; a
    lock whose process is gone is taken over) before it reads the strand and holds it to its exit; a second writer exits 4
    and changes nothing; every append first checks the lock is still the writer's. `probbit_live_event` takes it for its
    append. Two concurrent writers on one state and strand now give one exit 0 and one exit 4, and the strand verifies.
  - **Control lines**: `probbit live control STRAND pause|resume|retire --by human:ID --reason TEXT [--at TIME]` appends a
    chained control line under the lock. While paused or retired every event is refused (code `paused` / `retired`, exit 4,
    nothing written); retire is final; no credit crosses a control line (feedback after a resume credits no stance from before
    the pause) and nothing else moves. `verify` replays them and reports `controls` and `status`. Not an MCP tool, by design.
  - **Checkpoints**: every K-th event (`--checkpoint-every K`, default 1,000; 0 = none) a checkpoint line carries the event
    count, that event's stance and the whole state. `verify` checks each against the replay; `verify --from-checkpoint`
    replays from the last one; `monitor` (`--once`, and the first read of `--follow` / `--serve`) starts there and draws its
    event at once, and shows a paused or retired status next to its badge.
- Exit code 4 for `live` and `live control`: the writer lock is held by another writer, or the individual is paused or retired.

### Changed
- MCP: the input schemas of `probbit_persona_turn` (`inputs`) and `probbit_live_event` (`event`) declare `goals`, so a client
  that checks arguments against them passes a drives persona's goal signals.

## 0.7.0 - 2026-10-06

### Added
- **Bounded learning** (docs/persona.md §2.8): an optional persona block `learning: {from: [reward, correction], traits, rate,
  step_cap, total_cap}`, read strictly (`probbit_persona` stays 1). With it the state keeps `learned` (a weight Δ per level of each
  learned trait, 0 at `init`) and `credit` (the previous stance's onehot(level) − odds; 0 for a level a habit or a hold forced),
  both checked on read (Δ within ±total_cap, credit within ±1). On a turn whose reward flag is on (correction: sign −1) each
  learned trait's step is clip(sign x rate x credit, ±step_cap) per level, recentred to sum 0, added, and Δ is clipped to
  ±total_cap (6 decimals); Δ is added to the trait's unary weights when the turn compiles. The learner's whole output is the Δ
  table: habits and allowed levels are rules of every program, so every stance obeys every habit in force whatever Δ is. A
  persona without the block gives every document byte for byte as 0.6.0 did (the three example goldens are unchanged).
- **`prove` covers learning**: the learned weights of traits linked to the rule's trait stay in their box (±total_cap per
  level) whatever the feedback, and the bound includes it; verdicts unchanged. Soundness tests: every event sequence of tiny
  learning personas (brute force, learned deltas included), a targeted case the learned box decides, and a mutation that drops
  the learned term from the bound, which the tests catch (0.6.0's mutation checks still fail as they should).
- **`probbit live`** (docs/persona.md §5.7, probbit-cli/src/live.rs): a resident individual. JSONL events in (`--events FILE` or
  stdin), one stance per event out. `--clock real` (default) stamps each event's `elapsed_hours` from a monotonic clock,
  quantised to 1e-6 h, so moods decay by their half-lives between events; `--clock fixed` reads them from the events (a pure
  function of the events). `--seed N` or `--state FILE` (rewritten after every event). `--strand FILE` logs the life: a header
  (engine version, persona name / version / digest, seed, the initial state as canonical JSON, the persona document in its own
  key order), then per event the inputs as used, the stance digest, the new state's digest and the sha256 of the line before. A
  strand is never rewritten, just appended to: `--strand` on an existing strand continues it from the state after its last line,
  and a life continued over runs is the strand of one run, byte for byte. `--events FILE --watch` follows a file as lines are appended.
- **`probbit live verify STRAND`**: replays a strand from its header alone and prints `{"ok": true, events, persona, seed,
  engine, final_state, last_line}` (exit 0) or `{"ok": false, line, diverges}` for the earliest line that differs (exit 1). The
  hashes cover each line's text without its line ending, so a `\r\n` copy verifies.
- **`probbit live PERSONA --demo week [--seed N] [--strand FILE] [--plain]`**: one individual's scripted week on the fixed clock
  (events hourly 09:00-15:00, quiet nights): praise for short answers until the learned verbosity weights reach their cap, an
  upset and a quiet night, then a campaign that praises every joke while every third hour reports a failure. A persona without a
  learning block gets the demo's (the opening line says so; the strand records the document). Bars on stderr at a colour
  terminal (1 s per hour, nights fast-forwarded), else one line per event. The tutor, seed 2: 50 events, the verbosity cap on
  day 1 at 15:00, valence back to its resting level after a 17-hour night, P(joke) off failure turns 0.57 on day 1 and 0.89 on
  days 3-7, 13 failure turns with no joke, 0 rule breaks, a strand that verifies; the same bytes every run.
- MCP tools `probbit_live_event` (one event: the persona, the state or a seed, the event, optionally a strand file to log to ->
  `{stance, state}`) and `probbit_live_verify` (a strand's text or path -> verify's document; a divergence is an answer): ten
  tools. Python: `probbit.live_event(...)` and `probbit.live_verify(...)` (the same documents, through the CLI).
- `probbit persona describe` lists the learning block when there is one.
- `bench/live_learning.py` (stdlib, drives the binary) and BENCHMARKS.md §8: an adversary that praises every joke and
  criticises every joke-free stance, failures included, over 10,000 turns on each of seeds 0-9 of the tutor: 0 rule breaks and
  no joke on any of the 33,330 failure turns, while the learned humour weights reach their cap within 8-26 turns; a 10,000-event
  strand verifies in 3.3 s; how far learning moves an individual is set by `total_cap`, not by time in use (at 0.25 all 20
  learners stay nearer their own initial self than any of 99 siblings; at 1, 9 of 20 have a sibling as near or nearer).
- Tests: mood decay against the closed form on the fixed clock; caps; an adversary over 2,000 turns (no rule broken, humour
  learned to its cap); identity against the 99 siblings at two caps; strands replayed, continued, changed (divergence at the
  changed line), converted to `\r\n`; one header per individual (from `init` or a state file); the demo deterministic; the MCP
  and Python live surfaces.
- docs/persona.md §2.8 Learning and §5.7 Live (what decays, what learns, what can never change, how far learning moves an
  individual, the strand, verify, `--watch`, the demo); README "Live"; docs/agents.md the two tools.

### Changed
- Version 0.7.0 (Cargo manifests, npm package, installers' examples, the bench workflow's tag); the golden test is
  `stdout_matches_the_0_7_0_goldens` (the documents are unchanged).
- docs/persona.md: compilation step 2 adds the learned weights, the state (§5.1) shows `learned` and `credit`, `prove`'s bound
  (§5.6) includes the learned box, §9 lists the strand's format number.

## 0.6.0 - 2026-10-06

### Added
- **`probbit persona fuzz`, character testing** (docs/persona.md): a rule in habit syntax (`--never '{when: {...}, then:
  {...}}'`, or several in `--props FILE`) and a population of individuals (`--seeds 0-99`). It searches event scripts built from
  the persona's declared inputs (flags, levels, numbers on `--grid`, idle hours on `--hours`): random scripts, then a beam guided
  by the exact odds (the next events that put the most odds on a level the rule forbids). It shrinks each individual's shortest
  counterexample by delta debugging and prints it with the breaking turn's levels, odds, why and stance line and the exact
  `probbit persona replay` / `explain` commands that reproduce it. Human report or `--json` (`probbit_persona_fuzz: 1`).
  Deterministic: Philox (probbit-core) keyed by `--fuzz-seed`; byte-identical output for the same inputs on any number of
  threads (timing on stderr). Exit 0 nothing found, 1 a counterexample, 2 bad input. It tests the stance a host gets, not the
  words a model writes; "none found" is evidence, not a proof.
- **`probbit persona prove`**: the same rules, decided for every event sequence where it can be. One verdict per rule over the
  population: `held by construction` (the habits in force whenever the rule applies imply it; under `on_conflict: yield` a habit
  counts when no conflict on such a turn ever drops it), `proved for every event sequence` (a sound bound per individual: the mood accumulators
  stay in a box, inputs, history values and previous levels fall into finitely many cells, and in every cell the best allowed
  stance beats the best forbidden one by more than the box can move them), or `unknown` (the earliest cell the bound cannot
  decide, with the `fuzz` command to search it). Human report or `--json` (`probbit_persona_prove: 1`). Exit 0 every rule held
  or proved, 1 some unknown, 2 bad input. Soundness tests: every event sequence of tiny random personas, the fuzzer on random
  personas, and mutations of the bound that the tests catch. It covers the stance, not the model's words.
- `probbit persona lint --props FILE` (or `--never RULE`): the contradictions as before, then every rule proved, and the ones
  the bound leaves unknown fuzzed; the lint document gets `props` and `broken`; exit 1 when a rule breaks (CI-friendly).
- MCP tool `probbit_persona_fuzz` (the eighth tool of `probbit mcp`): `probbit persona fuzz --json`'s document for a persona
  (inline or a path) and `never` / `props`, byte for byte, in the server's process. Python: `probbit.persona_fuzz(...)` and
  `probbit.persona_prove(...)` return the `--json` documents (a counterexample or an unknown rule is an answer).
- docs/persona.md §5.6 "Testing a character": what a property is, both commands, the verdicts and the bound, the limits.
- `probbit-cli/tests/fixtures/persona/tutor-0.5.0.yaml`: the tutor as it shipped in 0.5.0, byte for byte, with the pinned
  fuzz report on it (one upset message, `{"sentiment":"negative"}`, gives seed 1 a playful stance).
- `probbit persona lint` warnings: a trait whose planned level differs from its own most likely level (its marginal mode) on a
  probe turn (each individual of `--seeds` at rest, a quiet turn, then every declared input alone) is listed under `warnings`
  with the count and the earliest case. A warning leaves the exit code alone, and decoding is unchanged (the stance is still the
  joint plan); docs/persona.md §5.5 gives the options (a habit, a `vouch` floor with a `hold`, or the trait's `odds`).
- README "Test a character": the fuzz counterexample on the 0.5.0 tutor and `held by construction` on the tutor as it ships now.

### Changed
- Version 0.6.0 (Cargo manifests, npm package, installers' examples, the bench workflow's tag); the golden test is
  `stdout_matches_the_0_6_0_goldens` (the documents are unchanged).
- **The tutor example** (examples/persona/tutor.yaml, persona version 1.1.0) has one habit more, `no_play_when_upset` (`when:
  {sentiment: negative}`, `then: {humour: {at_most: light}}`). In 0.5.0 `persona fuzz` found 67 of seeds 0-99 playful with an
  upset learner (37 of them after the single message `{"sentiment": "negative"}`), and the shipped workday golden itself had
  seed 1 playful on turn 11 ("still upset"); that turn is now light, and `persona prove` says the rule is held by construction.
  The tutor's digest and goldens change; its genes (individuals) do not. The habit fixes the stance the host gets, not the words
  a model writes.
- `why` names an input's push by its direction (`learner upset -> valence down, humour down`) where it named the trait's
  lowest or highest level ("humour none" printed next to a playful stance read as a contradiction). In the ops engineer's and
  the trader assistant's goldens nothing but `why` changes.
- docs/persona.md §5.3: the byte-for-byte parity with the reference implementation is stated for the 0.5.0 documents; the 0.6.0
  goldens are regenerated from this implementation.
- brand: the agent rabbit mark (visor + lightning ear) replaces the 0.5.0 mark (docs/brand/, the hero, the social card, the
  playground's favicon).

## 0.5.0 - 2026-10-05

### Added
- **`probbit persona`, the individuality layer** (docs/persona.md): a persona file (YAML subset or JSON: traits with priors,
  moods with inertia, couplings, per-turn inputs, history features, habits as hard rules in the probbit-ir vocabulary) + an
  individual's state + a turn's inputs compile to one probbit-ir program, run in process at fixed work; the stance (levels with
  exact odds, habits in force and bound, refusals, a why and a stance line of at most 40 estimated tokens) and the next state are
  canonical JSON. Subcommands `init`, `turn`, `replay`, `explain`, `diff`, `lint`, `check`, `compile`, `describe`. A strict YAML
  subset reader (probbit-cli/src/yaml.rs) and a strict validator (one `{"error": {"code": "persona", ...}}` object, exit 2). Three
  example personas with goldens in examples/persona/; the documents equal an independent reference implementation's.
- **MCP tools `probbit_persona_init` and `probbit_persona_turn`** in `probbit mcp` (stateless; the CLI's documents).
- **Python `probbit.persona_init`, `persona_turn`, `persona_replay`** (python/probbit.py; tests python/test_persona.py).
- **probbit-wasm ops 4 (persona init) and 5 (persona turn)**; the playground's "Meet three individuals from one persona".
- `probbit persona ... --timing` reports the 1-minute load average next to its timings (`sys::loadavg`).

### Changed
- `probbit mcp` lists seven tools (the two persona tools after the five before; their order and contracts are unchanged).

- **Renamed: pbit → probbit everywhere** (crates, binary, env vars, MCP tools, Python module, npm package). 'p-bit' remains the
  term for the probabilistic bit. No behaviour changes.
  - Crates `probbit-core`, `probbit-ir`, `probbit-decide`, `probbit-cli` (binary `probbit`), `probbit-wasm`; Rust paths
    `probbit_core::` etc.
  - Environment variables `PROBBIT_THEME`, `PROBBIT_CONFIG`, `PROBBIT_CHAINS`, `PROBBIT_THREADS`, `PROBBIT_CPU_LIMIT`,
    `PROBBIT_MEM_LIMIT_MB`, `PROBBIT_PRIORITY`; installer variables `PROBBIT_VERSION`, `PROBBIT_INSTALL_DIR`,
    `PROBBIT_DOWNLOAD_BASE`, ...; config file `probbit.json`; cache directory `$XDG_CACHE_HOME/probbit`.
  - MCP server `probbit`, tools `probbit_decide`, `probbit_run`, `probbit_stats`, `probbit_demo`, `probbit_evaluate`.
  - The IR's version key `probbit_ir` and the evaluate request's namespaced `probbit` block; the `engine` field reads
    `probbit 0.5.0`. The documents are otherwise byte for byte the 0.4.0 documents (proven on the 7 golden inputs and every
    example; the golden test is now `stdout_matches_the_0_5_0_goldens`).
  - Python module `probbit.py` (`ProbbitError`, ...), npm package and bin `probbit`, release assets `probbit-<tag>-<target>`,
    the playground's `probbit.wasm` / `probbit-wasm.js` (C ABI `probbit_*`, clock import `probbit.now_ms`).
  - The terminal hero screen spells probbit (its command column is 6 characters wider to fit the longer name).
  - Repository `github.com/BitmapAsset/probbit` (the old URLs redirect); the IR schema `$id` now names it (it named a wrong owner).
- Migration: rename the binary, env vars, MCP tool names, `"pbit_ir": 1` → `"probbit_ir": 1` and the evaluate `pbit` block →
  `probbit` in your documents and scripts; nothing else changes.

## 0.4.0 - unreleased

### Added
- **`pbit evaluate`**, the decision-API adapter: a System One request (`model`, `state`, `questions` keyed by id, types `noul`,
  `choice` and `score`, `images`; the shape published by TypeSafe's OpenAPI 0.2.0 and the Workers AI `clef` schemas) plus an
  optional `pbit` block (`judge`: the judge's System One response; or `weights` / `logw` per question; `floor`; `rules`: the pbit-ir
  constructs over question ids). It compiles to a pbit-ir program (one variable per question, log-weight = ln max(p, 1e-6)), runs
  it as `pbit run` does with every `pbit run` flag, and answers in the judge's response shape (`model`, `answers`, `usage`) plus
  the `pbit run` document; each answer carries a `pbit` object (`value`, `p`, `judge`, `changed`, `released`). `--program` prints
  the compiled program. Same exit codes, error objects, `--summary`, `--pretty` and terminal rules as `pbit run`; a rule naming an
  option its question does not have is a `value` error at the rule's path (the program's value names are shared by all questions).
  docs/pbit-ir-json.md "Decision API"; tests in `pbit-cli/tests/evaluate.rs`.
- **MCP tool `pbit_evaluate`** in `pbit mcp` (the same contract; its input schema carries the pbit-ir rule definitions).
- **Python `pbit.evaluate(request, judge=None)`**: the judge is a callable (returning a System One response or per-question
  probabilities) or the URL of a System-One-compatible server, called with `urllib` (a key from an environment variable the
  caller names; the Workers AI REST envelope is unwrapped); `PbitJudgeError` for judge failures. `python/mock_judge.py`: a stdlib
  System One server for tests and demos.
- **Single-threaded engine path and an injected clock** (`pbit_core::rt`): the engine's `Instant` (std's where it exists; on
  wasm32-unknown-unknown a millisecond clock the embedder installs) and a per-thread switch that runs every parallel section of
  the command paths (chains, gate passes, polish, the exact tiers' deep-stack thread) in order on the calling thread. At fixed
  work the answers equal those on any number of threads.
- **`pbit-wasm`** (not published; no dependencies): `decide`, `run`, `evaluate` and `demo` as JSON-in / JSON-out functions for
  wasm32-unknown-unknown over a small C ABI, running the CLI's own front-end code without threads (830,233-byte module).
  **`playground/index.html`**: one static page, no server or framework, that runs the 300-task demo and `evaluate` with an
  editable JSON box; `playground/build.sh` builds the module; CI job `wasm` builds it and runs it under Node.

### Changed
- `pbit decide`'s pipeline moved from `main.rs` to `router.rs` (the browser build shares it); output unchanged (golden tests).

## 0.3.0 - 2026-10-02

### Added
- **Hero screen.** At a terminal, `pbit` and `pbit --help` print a 9-line screen and exit 0: the wordmark, the status line
  `PROCESSOR ONLINE · 0.3.0 · exact → sample → gate`, a spec line (os/arch, logical CPUs, target features and the site
  updates/s of a 30 ms self-test, cached for a day in `$XDG_CACHE_HOME/pbit/stats.json` or `~/.cache/pbit/stats.json`), three
  commands to try and the usage. Piped, `pbit --help` prints the usage as before (now listing the new flags and `pbit mcp`)
  and a bare `pbit` still exits 2.
- **`pbit demo --live`**, the default when stdout and stderr are both terminals: the router story inside the binary (no
  cargo needed). The queue and its rules, what a per-task argmax router breaks, the exact tiers and what they decline, then
  a field of p-bits (one per task and worker, half-block cells sized to the terminal) drawn from the current states of four
  real chains, the gate re-run every 200 sweeps on the samples so far, the ladder refused → partial → passed, a 3-line verdict
  stamp, and the command that gives the final look as JSON: the chains are recorded exactly as `pbit decide --sweeps 3200
  --polish-ms 0` records them (unit test `live_stepper_matches_sample_on`). Deterministic under `--seed` (fixed work per
  frame); 20 frames per second; `--tasks` defaults to 300 in this mode; stdout, when not a terminal, gets the problem
  document. The renderer took 2.0-2.4% of the process CPU on an Apple M4.
- **`--top`** (`decide`, `run`): a monitor on stderr redrawn at 10 Hz (tier, sweeps against the budget, site updates/s, CPU,
  peak RSS, the gate once it ran), erased at exit. Only with a terminal on stderr; `--top --progress` is a flag error.
- **`--summary`** (`decide`, `run`): the answer without its per-item tables: verdict, `counts`, the gate, telemetry and the
  items to look at first (`worst_released`, `worst_escalated`: up to 5 each, with plan value, its probability, the top two
  odds, the release reason and the error bar). Same verdict and exit code. With `--pretty` and a terminal on stderr, a boxed
  summary there too. docs/pbit-ir-json.md "Summary".
- **`pbit mcp`**: a Model Context Protocol server on stdio (hand-written JSON-RPC 2.0, no crates). Checked against the MCP
  specification revision 2026-07-28: requests with per-request metadata are served statelessly (`server/discover`,
  `tools/list`, `tools/call`, `ping`), and `initialize` serves the handshake revisions 2025-11-25 back to 2024-11-05. Tools
  `pbit_decide`, `pbit_run`, `pbit_stats`, `pbit_demo` take the commands' own documents (plus a `flags` object) and return
  the commands' own JSON. Tested by a dependency-free Python client (`python/test_mcp.py`); docs/agents.md "MCP: one line per
  agent" (Claude Code, Codex CLI, Cursor, mcporter, LangChain).
- `--plain` on every command and `PBIT_THEME=neon|plain`; `NO_COLOR` is honoured. One palette (`theme.rs`: cyan #00F0FF,
  magenta #FF2A6D, yellow #F9F002 in 256 colours, with a 16-colour fallback) for the hero, `--top`, `--live` and the summary
  box; at a terminal, `install.sh` ends with the hero screen.
- `pbit_decide::Chain::set_group_pairs`, for callers that step router chains themselves.

### Changed
- `pbit decide --mode exact` that cannot answer (stopped by `--exact-ms`, or the feasible set too large) prints a `declined`
  document and exits 3, as `pbit run --op exact` does. It printed a line on stderr and exited 2 with an empty stdout.
- One leading UTF-8 byte-order mark on stdin is skipped. Windows PowerShell 5.1 adds one to every pipe, so `pbit demo | pbit
  decide` exited 2 there (`bad JSON: unexpected character`).
- Python wrapper: a boolean switch passes the bare flag (`pretty=True` -> `--pretty`, `summary=True` -> `--summary`; False
  leaves it out). It became `--pretty on`, exit 2. `collective`, `cluster` and `cycles` still map to `on` / `off`.
- Crates: the path dependencies carry `version = "0.3.0"` and the workspace declares `rust-version = "1.78"`; `cargo
  publish --dry-run -p pbit-core -p pbit-ir -p pbit-decide -p pbit-cli` passes (nothing is published). The npm wrapper
  (`npm/package.json`), the installers' examples and the benchmark workflow point at v0.3.0.
- release.yml: static musl archives for x86_64 and aarch64 Linux, the Windows binary links the C runtime statically (no
  VCRUNTIME140.dll), and the macOS Intel build is smoke-tested under Rosetta 2. CI builds these four targets on every push
  and pull request (no tag, no release).
- `run_deadline_ms_bounds_the_whole_call` allows 1,800 ms when `CI` is set (a 600 ms deadline took 990 ms on the macOS
  Intel runner; the deadline is a target, not a hard bound on a slow CPU).
- Documentation: "First five minutes" and an install table in the README; the instruction set lists every construct
  (`min`, `all_different`, `implies`, `tables`, `precedes`, `linear`; cost budgets are expressible in `pbit run`
  programs); budget semantics; modelling notes (one variable per entity instead of `implies` chains; plan or odds);
  docs/pbit-ir-json.md without internal round tags; USE-CASES says `pbit ir` reads router documents only; the flow figure's
  source drops retired wording.

Every document of `decide`, `run`, `stats`, `demo` and `ir` is unchanged: stdout is byte-identical to 0.2.1 apart from the
version and the timings (golden digests in `stdout_matches_the_0_2_1_goldens`; the engine's own golden digests are
unchanged).

## 0.2.1 - 2026-10-01

### Fixed (found by the 2026-10-01 independent review)
- `pbit decide` aborted (exit 134, empty stdout) on a router document with 65,536 or more workers. Workers are now limited to
  65,535 (the IR's value limit), a `limit` error with exit 2, and `Problem::lower_until` declines instead of panicking.
- `--chains 576460752303423488` aborted `decide`, `run` and `stats` (exit 134) and `--chains 1000000000` exhausted memory.
  `--chains` is now 1..=100,000 and `--threads` 1..=1,024 from any source (flag, `PBIT_*`, `pbit.json`), else exit 2; the
  memory-limit arithmetic is checked.
- A failed write to stderr (a pipe whose reader had gone) aborted the process (exit 134): a bad flag lost its exit code, and
  `--progress` into a reader that stopped lost the whole decision. Every stderr line now ignores a failed write.
- Invalid UTF-8 on stdin and an unreadable stdin (`pbit decide < /`) exited 2 with nothing on stdout. Both are now ONE
  `schema` error object on stdout, like any bad input.
- The router sampler's two-group move stored worker indices and bucket loads in 8 bits: with more than 255 workers in a group a
  task could be placed on a worker it does not allow and the sampled odds were wrong (the gate refused those runs). Such groups
  now skip the move, which keeps the chain exact; loads add without wrapping, and caps of t or more (legal up to 2^53) no longer
  truncate in its 32-bit arithmetic. Golden digests unchanged.
- Small documents could allocate gigabytes (a 0.6 MB program peaked at 4.1-6.3 GB, a 30 KB precedence at 1.45 GB). New `limit`
  errors, checked before the allocation: at most 20,000,000 (variable, value) pairs (n x k; tasks x workers on the router), at
  most 100,000 slot pairs per `precedes` (as for `tables`), at most 20,000,000 cap members in total; `--max-input-mb N`
  (`decide`, `run`, `ir`; default 256, 0 = no limit) bounds stdin. `precedes` loops over allowed slots only (same caps).
- Escaped UTF-16 surrogate pairs (`"\ud83d\ude00"`, how Python's `json.dumps` writes an emoji) decoded to two U+FFFD, so ids
  came back changed and two emoji ids collided. A pair is now one character, and a lone surrogate is a `schema` error. The
  Python wrapper reads pbit's output as UTF-8 whatever the locale.
- `pbit-core`: the multispin and heat-bath lattices had `pub` fields, so safe code could reach undefined behaviour in the
  `unsafe` fast kernels (a wrapped `l * l` length check; heat-bath couplings off ±1 read past the lookup table). The fields are
  private now: build a lattice with `random` or the checked `new` (exact lengths; ±1 values for the heat-bath), read it with
  `l()`, `w()` / `s()`, `jr()`, `jd()`; the length checks no longer wrap. Kernel output and speed unchanged.
- `cargo clippy` failed with default lints (and so never reached `pbit-cli`); it now passes. The seeded generators keep the
  literal 6.283185307 (allowed with a reason), because golden digests and published instances depend on its exact bits.
- `pbit --help` and `pbit -h` print the usage on stdout and exit 0 (they exited 2 with the usage on stderr).
- `pbit.json`: an unknown key (`{"thread": 3}`) or a non-object was ignored silently; now exit 2. `decide` and `run` name the
  config file in `telemetry.config` when there is one (it can change the answer of an otherwise identical command).
- Process telemetry (`process_cpu_ms`, `peak_rss_mb`) is read only on 64-bit Unix, where the `getrusage` layout is known
  (`null` elsewhere; 32-bit Linux or BSD would have read garbage).
- Two wall-clock tests tolerate slow CI runners: `acc1_tv_le_001_within_10ms` allows 25 ms when `CI` is set, and the sudoku
  test's passing sampled run gets 300 ms instead of 60.

## 0.2.0 - 2026-10-01

### Changed (breaking for malformed input)
- Strict input contract for both JSON front-ends (`pbit decide` / `pbit ir` router documents, `pbit run` pbit-ir programs):
  every field type-checked when present, unknown fields rejected, duplicate ids / value names / object keys rejected, empty
  domains rejected, only finite numbers accepted, nesting limited to 256. Bad input now exits 2 with ONE structured object
  `{"error":{"code":"schema|value|limit","path","message"}}` on stdout (plus a human line on stderr); flag errors are unchanged.
  Before, `"allowed": "A"` was ignored (every scored worker allowed), a cap object in place of the caps array was dropped,
  duplicate value names emitted duplicate keys, and `1e309` aborted the process (exit 134). Valid documents give identical
  answers (8/8 fixed-work outputs byte-identical after stripping timings); parsing a 100,000-task document costs +7.7%
  (`pbit ir`, 461 -> 496 ms median of 7, Apple M4, load ~5.9). See docs/pbit-ir-json.md "Input contract".

### Changed (trust contract)
- The sampled whole-answer verdict `certified` is retired: it is now `diagnostics_passed` (same test). Library: `Verdict::DiagnosticsPassed`,
  `Gate::diagnostics_passed`, `Gate::released_tasks`, `Anytime::passed_at_ms` (were `Certified`, `certified`, `certified_tasks`,
  `certified_at_ms`; the last renamed together with the crate doc of `pbit` and the agent_router / anytime / frontier_exact example labels). Vocabulary: `exact` and
  `infeasible` are proof-grade under the score model; `diagnostics_passed` and `partial` are sampled and gated; `refused` escalates.
- Every sampled answer carries gate/2: `gate.version` ("gate/2"), `gate.assumptions`, `gate.mcse_tv_short` / `mcse_tv_long` (both
  batch-means MCSEs), `gate.min_batches_long` (the long pass's batch count, now also required to be >= 8: the 0.1.0 gate checked only the short pass;
  measured: 2 of 5 partial answers at 300 sweeps per chain (12 / 15 released) become refused, 0 change at 1,000+ sweeps and at the
  200 ms default on the 300-task demo, 2,674 vs 2,701 released over 19 runs; on the max-cut oracle suite (`maxcut_oracle`, 144 runs)
  72 -> 61 whole answers and 1,346 -> 929 released spins, 0 false before and after: every 500-sweep run now refuses, 7 long batches), `gate.worst`
  (the worst item's per-chain means and kept rows) and a per-item `release_reason` (`whole_answer_gate`, `item_gate`, or the first
  failed check: `frozen`, `rhat`, `batches`, `batch_stability`, `item_bound`).
- README claims rewritten: no "certified joint decisions", odds are Monte-Carlo estimates under the supplied score model (not
  predictive calibration), the 64 kernel replicas share one random draw per site, the naive-router baseline is a rule-ignoring
  argmax over synthetic scores.
- Example printouts (re-run on the 0.2.0 build): `agent_router` (the README's first demo), `dispatch`, `router_bench`, `transfer`, `maxcut_oracle`,
  `colouring_oracle` and `schedule_oracle` still printed the retired word (`CERTIFIED` / `certified`) and `agent_router` said
  "gate promised <= 0.05"; they now print `DIAGNOSTICS PASSED` / `passed` / "gate tolerance 0.05". Labels only: no logic or
  number changed. Comments that describe 0.1.0 history keep the old word.

### Added (Python wrapper, `--help`, schema)
- `python/pbit.py`: a zero-dependency (stdlib, Python >= 3.9) subprocess wrapper: `run` / `exact` / `sample` (pbit-ir programs),
  `decide` / `demo` (router documents), keyword flags (`budget_ms=200` -> `--budget-ms 200`, booleans -> on/off), `deadline_ms`
  (passed to the CLI, plus a kill backstop), typed errors (`PbitInputError` with code / path / message for exit 2, flag errors as
  code "flag"; `PbitNumericError`; `PbitTimeout`; `PbitError`); `infeasible` (exit 1) and `refused` / `declined` (exit 3) are
  answers, not exceptions. `python/examples/` (router, knapsack, agent planner); `python/test_pbit.py` (9 tests) runs inside
  `cargo test` (CLI test `python_wrapper_tests_pass`; skipped only without python3 >= 3.9).
- `pbit <command> --help` / `-h` (decide, run, demo, ir, stats, version): usage, every accepted flag with its meaning and
  default, the precedence of controls, and the exit codes (stdout, exit 0). The flag list is the one the parser enforces
  (`flags_of`), so help and parser cannot drift (test `every_command_has_help`). Before, `--help` after a command was an
  unknown-flag error (exit 2).
- `docs/pbit-ir.schema.json`: a hand-written JSON Schema (draft 2020-12) of the pbit-ir wire format with three runnable
  examples. Test `ir_schema_matches_the_parser`: every object's property list equals the parser's own known-field list
  (read from the CLI's "unknown field" error for an injected probe, 11 shapes), every schema example answers `exact`, and
  every example program and stress input parses.

### Added (collective moves)
- `--collective on|off` for `pbit decide` and `pbit run` (default **on**; `Problem::collective` / `Model::collective` in the
  libraries, default off there): per sweep, a global flip of every free variable with exactly two candidate values and, for
  k > 2, a swap of two value labels everywhere; each a symmetric involution with a Metropolis accept and every cap checked.
  The router and IR samplers stay bit-identical with it on. The gate output reports `gate.collective`. Effect: the frozen
  stress corpus (the external review's counterexamples) goes from 2-3/20 false whole answers per family to 0/20, 0 wrong released items;
  seeds 1..100: 12/100 and 14/100 false -> 0/100 (BENCHMARKS "Known failure modes"). Sampled answers change for the same seed
  (the moves consume random draws); `--collective off` reproduced 0.1.x sampling when this shipped; since `--cluster` and
  `--cycles` became default-on, that needs `--collective off --cluster off --cycles off` *(inferred from the earlier check)*.
- `--cluster on` is the DEFAULT (calibration, below); `--cycles on` is the DEFAULT (below). `--cluster on` = a Wolff cluster move over the non-negative Potts bonds (boundary
  bonds cancel against the proposal ratio; tables and negative couplings enter the accept as plain terms) and `--cycles on` =
  three-cycle rotations that keep every value's count (connect capacity-saturated states no pairwise swap connects). Both
  samplers stay bit-identical with any combination on. Fixed-work cost (1 thread, Apple M4, load ~10-15): cluster +0% on the
  300-task demo, +93.6% on the 32-task one-group router, +26.7% on ferro12; cycles +10.7% / +1.9% / +4.1% (demo / router /
  chain100); the cluster figures were measured before the move became a probability-1/2 mixture per sweep (about half
  expected, unmeasured). That change fixed a composition hazard: attempted every sweep next to the global flip, the two
  cancelled on ferro12 (2/10 false whole answers; BENCHMARKS "Known failure modes"). Default-on decisions wait for the
  calibration corpus.

### Added (inference compiler)
- Exact-zero factors are eliminated before tier selection (`pbit run` / IR JSON lowering): a pair whose every entry is exactly
  0 (`potts: 0`, an all-zero table) changes no state's log-weight, but it linked its ends and hid independence from the exact
  tiers. Review case E17 input (32 independent variables + all 496 pairs as `potts: 0`): sampled 115.7 ms (single run, this machine;
  the reviewer: 118.5 ms) -> exact frontier, median 0.051 ms (N = 7, Apple M4, load ~6), the same as without the pairs (0.053 ms).
  Constant and separable tables (t[a][b] = u[a] + w[b] exactly in f64) are folded into the two variables' unaries
  (`pbit_ir::compile_parts` / `Model::compiled`, counted as `compiled.tables_folded`): log Z, odds and plans unchanged up to
  float summation order; 32 independent variables + 496 constant tables (0.5): sampled 385.7 ms -> exact frontier 0.045 ms
  (medians N = 5, load ~15), log Z = the table-free log Z + 248. `pbit decide` runs its components tier on the compiled lowered
  program. Tests `zero_factors_do_not_hide_independence`, `compile_pass_preserves_the_distribution` (200 random programs,
  enumeration of compiled vs original, 1e-12).
- Redundant capacity constraints are dropped in the same pass: a cap whose limit reaches its number of distinct member
  variables can never bind (each variable holds one value). The external review's ferro12 redundant-caps input: identical odds to the cap-free
  program; sampling 51.40 -> 45.06 ms (-12.3%) at `--sweeps 20000 --threads 1`, medians of N = 5 alternating, load ~8.5. The
  moves-off stress counts for it are unchanged (3/20 false: those chains move, inside one wrong mode). Test
  `redundant_caps_are_dropped_binding_caps_kept`.
- Every `pbit run` answer carries `compiled` {pairs_dropped, caps_dropped, tables_folded}: what the compile pass removed (E17 input: 496 / 0;
  ferro12 redundant-caps input: 0 / 2).
- Components + forest exact tier (`pbit run`, `exact_components_until` in pbit-ir): connected components of
  pairs and caps are solved one by one and recombined (log Z adds, odds per component); cap-free trees by log-space
  sum-product + max-product, other components by enumeration or the frontier DP. `tier: "forest"` / `"components"` +
  `components` {count, forest, enumerate, frontier}. The external review's chain100 / chain1000 at defaults: sampled 281.9 / 281.3 ms
  (`diagnostics_passed`) -> exact 0.055 / 0.390 ms (medians N = 5, Apple M4, load 3.6-11); four independent capped rings
  (non-partition, 2^32 raw space): 385.5 ms sampled -> 0.057 ms exact. Tests `components_and_forest_match_enumeration`
  (400 random programs vs enumeration, 1e-9; chain1000 log Z vs a transfer matrix) and `stress_chain100_forest_tier`.
  Sampled answers on such programs become exact answers (a behaviour change for `decide`; `--op sample` is unchanged).
- Occupancy-count DP inside the components tier: a component whose members take two values, with one identical
  symmetric coupling on every pair (complete graph; or no pairs) and caps that count every member able to take their value,
  is solved in O(n^2) in log space (odds clamped into [0, 1]; n <= 2048). `pbit decide` now runs the components tier on its
  lowered program (before whole-program enumeration when the raw space exceeds `--exact-limit`, else after the frontier;
  `--frontier-states 0` = off): the external review's asymmetric-router32 h0.03 / h0.06 and heterogeneous-router32 at defaults: sampled
  `diagnostics_passed` 428.8 / 426.4 / 423.4 ms -> exact (tier `occupancy`) 0.092 / 0.087 / 0.086 ms (medians N = 5, Apple M4,
  load 8-18). Tests `occupancy_count_dp_matches_enumeration` (300 random programs incl. near misses that must NOT be counted,
  infeasible caps; n = 512 vs the closed form, 1e-9) and `stress_router_occupancy_tier` (CLI vs the stress oracle).
  The router stress families also run with `--mode sample` so the gate is still exercised (20/20, 0 false).
- `--cycles on` is the DEFAULT (CLI, both commands; library `Model::cycles` / `Problem::cycles` stay off), and the
  rotations are attempted only on programs with k > 2 values and at least one cap (IR and router alike, still bit-identical):
  on two-value or cap-free programs the answer is byte-identical to `--cycles off` at fixed work (ferro12, chain100,
  asymmetric router, 2,000 sweeps, seed 3). Evidence: a new holdout family, saturated quotas (12 variables, 3 values, 2 allowed
  per variable, every value capped at 4 = n/3, so no single-variable change is ever feasible): seed 20261005 sampler-forced
  20/20 refused with cycles off, 16/20 whole (0 false) with cycles on (diagnostic); validated on the untouched seed 20261006
  at the new defaults: all 11 families 20/20 whole, 0 false, 0 wrong, 0 refused (control `--cycles off`: saturated quotas
  20/20 refused, every other family unchanged); frozen corpus 0 / 0 / 0. Cost at defaults (median sweeps in 200 ms, N = 5
  alternating, Apple M4, load ~6.5): 300-task demo -13.7% (the only input of the four where the move runs); ferro12 / chain100 /
  asymmetric router -20.2% / +2.3% / +11.8% are load noise (identical code path, see above). Sampled answers on k > 2 capped
  programs change for the same seed; `--cycles off` reproduces the sampling before this move.
- IR cardinality (the first new construct): a `value` cap takes `min` (at least) alone or with `limit` (range; equal =
  exactly k), lowered to an extra at-most cap over the members' other values, so every tier runs it unchanged. `min` above the
  variables able to take the value -> exit 2 (`value`); `min` with `members` -> exit 2 (`schema`); a cap with neither `limit`
  nor `min` -> exit 2 (`schema`). v1 programs are unchanged (golden digests hold). Tests: brute force (6 x 3, three
  cardinality caps) + 3 contract cases.
- IR `all_different` and `implies`: lowered to caps (per value at most one of the listed variables; at most one
  of x = a and y outside `in`), so every tier, the sampler and the gate run them unchanged. Brute-force test (5 x 4) and 3
  contract cases; sampler at defaults 5/5 `diagnostics_passed`, max TV 0.0010.
- IR `tables`: forbidden / allowed tuples over 1-3 variables, lowered to caps (limit arity - 1 per forbidden
  tuple; an allow-list forbids the complement, at most 100,000 tuples). Brute-force test (4 x 3) + 3 contract cases.
- IR `precedes`: the (job, slot) precedence pattern, slot(after) >= slot(before) + gap with values read as
  ordered slots, lowered to pair caps (O(k^2) per precedence). Brute-force test (4 jobs x 5 slots) + 2 contract cases.
  Known failure (found during development): a feasible 20-job x 30-slot chain was refused (no feasible start; `pbit run` 15.2 s at defaults).
  Fixed before release (the "Arc-consistent start" entry below).
- `pbit run --deadline-ms N`: whole-call wall-clock target (exact tiers N/4, sampler 0.6 and polish 0.1 of the
  rest, gate in the reserve); `deadline: {ms, met}` in the answer; met 15/15 at N = 500 / 1000 / 2000 on the 20 x 30 chain.
- `pbit run` answers end with `phases`: parse / compile / exact / sample / gate / polish / total wall ms.
- IR `start` warm start: `{"var": "value"}` over every variable, validated (allowed, clamps, every cap and
  construct) or exit 2; chain 0 starts from it, the other chains search as before; exact tiers ignore it. `Model::start` in the
  library (ignored if infeasible). 9 contract cases + 2 tests.
- Arc-consistent start + bounded exact tiers (the known failure above): programs whose two-variable forbid caps prune under
  root arc consistency start with a MAC search (AC-3 + forward checking on full caps); others are unchanged. `pbit run
  --op decide` with a wall-clock budget stops the exact tiers before the sampler at `--budget-ms` when the raw space exceeds
  `--exact-limit` (new `telemetry.exact_budget_reached`), and the no-start fallback stops hard at the end of the sampling
  budget or half a budget after it starts, whichever is later. 20 x 30 precedence chain at defaults: 15.2 s -> 0.54 s, chains start, gate refuses honestly at 200 ms
  (`diagnostics_passed` at 3000 ms, max TV 0.0112 vs an independent chain DP). Behaviour change: a program over
  `--exact-limit` whose exact answer needs longer than `--budget-ms` is now sampled (use `--op exact` or a larger budget).
- IR `linear` rules (linear <=): `{"terms": [[var, value, weight], ...], "limit": L}` = sum of weight x [var = value]
  <= L, whole-number weights 0..=1,000,000 (knapsack, bin packing, multi-dimensional budgets). Lowered to one WEIGHTED cap
  (`Cap.weights`; `Cap::new` for unit caps; library users constructing `Cap { .. }` literals add `weights: vec![]`), never by
  replicating members. Every exact tier, start search (incl. MAC forward checking), sampler move, the compile pass and the gate
  read the weights; the frontier and occupancy-count DPs decline weighted programs. Unweighted programs: identical answers
  (every golden test unchanged). Tests: 300 random programs vs brute force (1e-9), sampler vs enumeration with every move on,
  a CLI brute-force test and 10 contract cases.
- Program families (BENCHMARKS 4.4-4.10, USE-CASES rows; stdlib bench scripts with independent oracles and a
  classical baseline each): knapsack (`bench/knapsack.py`, log-space DP; n = 20-80), bin packing (`bench/binpacking.py`, DP over
  bin-load vectors; n = 12-36 x 3 bins), Ising-MRF denoising (`bench/denoise.py`, transfer matrix + ICM; 8 x 12), tree inference
  (`bench/tree_infer.py`, sum-product; n = 200 / 1,000), budgeted selection with correlations (`bench/portfolio.py`, chain x budget
  DP; n = 30 / 60), 3-SAT counting (`bench/sat.py`, brute force + WalkSAT; n = 14 / 18: exact; the sampler refuses 16/16),
  agentic tool-call planner (`bench/agent_planner.py`, brute force; 6-7 steps), assignment with side constraints
  (`bench/assign_side.py`, brute force; n = 8 / 12). Totals over the sampled answers: 0 released items outside 0.05 of the
  oracle, 0 false `diagnostics_passed`. Examples `examples/knapsack-20.json`, `denoise-8x12.json`, `agent-plan-6.json` (tests
  `run_knapsack_example_matches_dp`, `run_ising_denoise_matches_brute_force`, `run_agent_plan_example_is_exact`). Not covered:
  MaxSAT (no soft ternary terms in v1), ILP baselines (no scipy on the measuring machine).
- Known failures found during development (BENCHMARKS / docs "Limits in v1"; neither gives a wrong answer): (1) dense n x k domains:
  k = 65,535 with 3 allowed values per variable, n = 200, `--op sample` peaks at 3,584 MB and takes 1.85 s at a 200 ms budget
  (the polish clones the model per temperature; gate 0.9-1.0 s; polish FIXED: `Chain::beta` carries the
  temperature, no clones, same plans bit for bit: n = 200 polish 549.7 -> 50.1 ms at its 50 ms budget, peak RSS 3,444-3,584 ->
  2,322 MB; sparse domains stay a v1 limit); (2) gate false refusal: on quota programs whose caps sum to n,
  members forced BY COUNTING trip the `frozen` rule (saturated-k3: 8/120 sampler answers over 6 seeds, chains fine);
  `#[ignore]`d red test `stress_saturated_forced_by_counting_is_not_frozen`. (2) FIXED (`pbit_ir::forced_values`:
  on partition programs a variable every chain held at one value is proved forced by the capacitated matching and treated as
  a constant): 8/120 -> 0/120 refused on the same seeds, 0 false; `--cycles off` control still refuses 20/20; untouched
  holdout 20261011 0 false / 0 wrong / 0 refused; the red test is un-ignored.
- `bench/calibrate.py` family `v2-constructs`: 7 variables x 4 values with an at-least cap, an all_different, an
  implication, a forbid table and a precedence; the brute-force oracle checks each construct by its direct meaning (not by
  pbit's cap lowering). Untouched seed 20261007 (5 programs x seeds 1..4): defaults 20/20 `exact`, sampler forced 20/20
  `diagnostics_passed`, 140 released, 0 false, 0 wrong, 0 refused.
- Never-moved rule sharpened: partition programs (the router; cap-free programs) now escalate per connected component
  of the free variables, like every other program, instead of every variable (`partition_programs_escalate_only_the_stuck_
  component`: two independent quota pools, one stuck: only that pool is escalated). Measured: `router_bench` family A 6/16
  passed, 1,390 -> 1,380 released (wall-clock noise; router programs are one component through their caps), 0 wrong; seed
  20261006 with every move off, old vs new binary: identical counts in all 11 families (0 false, 0 wrong); corpus 0 / 0 / 0.
  `bench/calibrate.py`: `PBIT_BIN` env var to score another binary.
- `bench/calibrate.py`: five harder holdout families from their own random stream (three ferro clusters with
  mixed-sign bridges, rare modes behind a cap, 3-value clusters, a 150-variable 4-value chain with a transfer-matrix oracle, an
  8-value alphabet with caps) and `--families`. Untouched seed 20261004, all 10 families + the frozen corpus, defaults and
  sampler forced: 0 false whole answers, 0 wrong released items, 0 refused (gate/3 unchanged). Moves-off control: the three
  cluster families refuse (20/20, 20/20, 7/20 not whole), still 0 false. Per-family table in BENCHMARKS "Known failure modes".
- `pbit run` (op decide): when the raw space exceeds `--exact-limit` the components tier now runs BEFORE the
  whole-program frontier DP (as `pbit decide` already did). 100,000 independent two-value variables: frontier 4,441.9 ms ->
  forest 32.5 ms (medians N = 5, Apple M4, load ~2.4; same log Z 73283.230992, identical odds; MAP differs only on exact-tie
  variables, same `plan_logw`); a 100,000-variable path is unchanged (forest ~34 ms). Tier names on such inputs move
  `frontier` -> `forest` / `components`. Test `run_components_tier_goes_before_the_frontier_over_the_limit`.

### Added (stress corpus)
- A frozen stress corpus (`pbit-cli/tests/stress/`, 7 inputs from the 2026-09-30 external review) with independent oracles (occupancy count DP,
  brute force, transfer matrix) and a "Known failure modes" section in BENCHMARKS: on 5 of 7 inputs 2-3 of 20 seeds pass the gate
  with every released item wrong. Those gate tests were `#[ignore]`d (red) until the collective moves and gate/3 (below).

### Changed (gate cost at many chains)
- The gate's chain-disagreement check uses sign projections from 128 chains (k <= 12) instead of every pair of chains,
  and its per-chain passes (both batch-means passes, the frozen check, the projections) run on a bounded pool of `--threads`
  workers instead of one OS thread per chain (`pbit_ir::pool_map`, `pbit_ir::GATE_THREADS`, which the CLI sets from
  `--threads`). Statistics are bit-identical (test `gate_pool_is_bit_identical`; old vs new binary same bytes at fixed
  `--sweeps`, 4 to 3,000 chains). `pbit demo --tasks 24 --seed 1 | pbit decide --mode sample --chains N`, two binaries
  interleaved, N = 5 medians, Apple M4, load 6.2-7.2: gate 1,000 chains 54.0 -> 15.9 ms, 10,000 chains 2,255.7 -> 15.8 ms
  (peak RSS 840 -> 368 MB), 100,000 chains: no answer in 90 s -> whole call 583 ms (gate 88.5 ms; the sampling phase now
  overruns a 200 ms budget there, BENCHMARKS §6).
- `pbit decide` lowers the router program for its components tier only under `--mode auto` with `--frontier-states` > 0, and the
  lowering stops when `--exact-ms` passes (`Problem::lower_until`, clock read per group member). One 3,000-task
  group: `--mode sample` 884.6 -> 618.3 ms, `--exact-ms 0` 885.6 -> 614.9 ms, `--exact-ms 50` 898.1 -> 741.0 ms (N = 5 medians).
  Outputs unchanged (byte-identical at fixed `--sweeps`).

### Changed (calibration on a frozen-threshold holdout)
- `bench/calibrate.py` (stdlib only): the frozen stress corpus and seeded holdout families (rare modes in weakly bridged
  ferro clusters, heterogeneous groups under a binding cap, 3-value constrained cycles, a 7-value alphabet, varying group
  sizes), each scored against a brute-force / count-DP / transfer-matrix oracle, at defaults and with the sampler forced;
  the header records commit, gate version, the gate thresholds, machine, load and every command. CONTRIBUTING: thresholds are
  frozen before a holdout and never retuned on it.
- Holdout seed 20261001 (gate/3 thresholds frozen at 54c9a16, `--op sample`, 5 programs x seeds 1..4 per family): rare-modes
  8 whole answers, ALL 8 false (96 wrong released items, 12 refused); group-sizes 12/20 refused; the other three families
  20/20, 0 false. The global flip maps (A,A) <-> (B,B) of two clusters but never reaches (A,B). Diagnostic on the same seed
  with `--cluster on`: rare-modes 20/20, 0 false; group-sizes 0 refused. **`--cluster` is now on by default** (`pbit run`
  and `pbit decide`); validated on the UNTOUCHED holdout seed 20261002 at the new defaults: every family 20/20, 0 false, 0
  wrong, 0 refused (control, same seed with `--cluster off`: rare-modes 9/9 false, 108 wrong; group-sizes 8 refused). The
  frozen corpus at the new defaults: 0 false, 0 wrong, 0 refused in 14 family-modes (ferro12-rc-w1.0 sampler had 1/20
  refused before). Cost (earlier fixed-work numbers, unchanged move): +48.4% per sweep on the 32-task one-group router,
  +24.6% on ferro12, -0.4% on the 300-task demo, +1.3% on chain100; at defaults the budget is wall-clock, so it shows as fewer
  sweeps: median sweeps in 200 ms (sampler forced, N = 5 alternating on/off, Apple M4, load ~3): asymmetric-router32 h0.06
  -20.4%, ferro12 -2.4%, 300-task demo -2.1%, chain100 within noise (+14.2%; the off runs spread 118k-190k). Sampled answers
  change for the same seed (the move consumes draws); `--cluster off` reproduces the sampling before this move.

### Changed (gate/3)
- `gate.version` is "gate/3": every released item also needs its own indicator split-R-hat (`Gate::rhat_task`, computed since
  0.1.0 but unused) below `ITEM_RHAT_MAX` = 1.05, and a whole answer needs every variable's; new `release_reason` `item_rhat`,
  new gate fields `item_rhat_max` (finite max) and `item_rhat_infinite` (count). It catches chains that hold different values
  of a variable while their log-weight traces agree. Measured: 0 change on `maxcut_oracle` (144 runs, deterministic) and on
  1,000 iid bits (150 runs); it does not change the moves-off stress counts (every chain in the same wrong mode is invisible to
  any within-run statistic: BENCHMARKS "Known failure modes"). New example `iid_gate` (review case E6) and a `COLLECTIVE=1` knob on
  the pbit-ir oracle examples (the libraries still default the moves off; the CLI defaults them on).
- The frozen rule's "a free variable that never changed value in any chain" test now also runs on partition programs (the
  router, and every program without capacity constraints, which counts as a partition: max-cut, ferro12; it returned before
  reaching it): such a run is refused whole, as on other programs. Moves-off ferro12 at fixed work: 3/20 false -> 0/20 (20 refused). Conservative by design (the gate
  cannot tell "certain" from "stuck"). Measured on `router_bench` (wall clock, collective off): family A 7 -> 6 of 16 runs
  passed, 1,565 -> 1,370 released (one correct 200-task run, maxTV 0.0178, refused), 0 wrong before and after; families B and C
  unchanged; the 300 / 1,000 / 3,000-task demos at CLI defaults are unaffected (frozen 0; 5/5, 3/3 passed, 3/3 partial with
  every escalation `item_bound`). Two golden gate digests re-pinned (their `frozen` counts now include the never-moved tasks).
- Occupancy-count R-hat (`Gate::rhat_occ`, `pbit_ir::rhat_occupancy`): per capacity constraint, split-R-hat of its load trace
  across chains; a variable is released only if every constraint it belongs to reads < 1.05 (reason `item_rhat`); gate fields
  `occupancy_rhat_max` / `occupancy_rhat_infinite`. It sees chains that hold different counts on a worker while each member's
  indicator still mixes (16 variables, 7 vs 9 ones per row on two chains: indicator R-hat < 1, occupancy R-hat infinite; test
  `occupancy_rhat_sees_count_modes_the_indicators_miss`). Measured on the night suites (same harness as above): no change beyond
  the never-moved rule (scheduling, which has caps, 428 -> 428; router_bench 1,375 family-A released incl. the c2 refusal; 0 false).
- `gate.mode_transitions` (global_flips, label_swaps, chains_without): accepted collective moves per chain, counted in both
  samplers without consuming draws (`Samples::moves`, `Chain::moves`; the router and IR samplers report identical counts). It
  is a reading, not a release rule: on ferro12 the flip is accepted every sweep (h = 0); on the 300-task demo (no two-value
  groups) 0 moves are accepted and the answer still passes; `--collective off` on the h0.06 stress input reports 0 transitions
  next to its known false whole answer (`--sweeps 4000 --seed 5 --polish-ms 0`: every chain at P(A) ~0.997 vs the true 0.1302,
  item_rhat_max 1.0003, chains_without 4), which is the case the reading exists for.

### Fixed (router enumeration on near-saturated groups)
- Default `pbit decide` on a near-saturated 3,000-task group spent 199.9 s in the router enumeration before declining: every
  search node summed the affinity over all group mates. The enumeration now keeps per-(group, worker) counts of assigned mates and
  a memo of the same sequential affinity fold per (task, worker): O(workers) per node, bit-identical sums (golden digests
  unchanged, new test `enumeration_affinity_memo_is_bit_identical`, old vs new binary same answers). Default decide, N = 5
  medians, Apple M4: 3,000 tasks at 91% fill 199.9 s (N = 1) -> 2.70 s; 1,000 at 91% 19.63 -> 0.73 s; 1,000 at 77% 2.56 ->
  0.39 s; 500 at 63% 1.14 -> 0.30 s (BENCHMARKS §6). The node budget itself is unchanged (64 x `--exact-limit`). Cost, found
  by an old-vs-new A/B: small-group exact inputs are 5-22% slower (default decide, N = 9, load 4.8: 12-task demo
  7.17 -> 8.01 ms, 24-task 26.5 -> 28.0 ms; full enumeration `--mode exact`, N = 5, load 3.5: 14-task demo 1,455 -> 1,773 ms;
  same answers); the sampled path is unchanged.
- `pbit decide` sampling at many chains: the wall-clock budget is now a deadline per worker (the r-th chain of a worker stops at
  call start + (r + 1) x budget / ceil(chains / threads)) instead of a slice each chain timed for itself, so a chain's overrun
  is charged to the next ones. 100,000 chains on a 200 ms budget: sampling 354-432 -> 269-318 ms, call 498-572 -> 405-460 ms
  (medians of N = 5 in two interleaved runs, loads 3.5-4.2); at that setting no chain sweeps (`sweeps` 0: the starts are the answer, refused);
  4 chains unchanged. Same chains and output shape; late chains sweep less. The rest of the overrun is the per-chain build.
  The IR sampler (`pbit run`) is unchanged.

### Fixed (numeric validity)
- Scores / weights must be finite with |x| <= 1e9 (`limit` error); the printer no longer emits a literal `inf` for |x| > ~1.8e302
  (|x| >= 1e15 prints in exponent form); a non-finite computed number never ships with an answer (`numeric` error, exit 3;
  infinite gate diagnostics only on a refusal, listed in `gate.non_finite`); telemetry rates no longer divide by zero.
- The frontier DP (`tier: "frontier"`) now runs in log space: its per-layer-scaled linear weights underflowed past ~745 nats
  and returned NaN marginals labelled `exact` (weights of +-1000 suffice), which aborted the CLI (exit 134). Printed odds and
  log Z unchanged on the demos; +9-12% wall on the largest DP layers.
- Seeded 10,000-document fuzz test of both front-ends (CLI test `fuzz_inputs_never_crash_and_outputs_stay_finite_json`).

## 0.1.0 - 2026-09-30

Initial public release.

- `pbit-core`: a Philox4x32-10 counter RNG (reproducible, stream-splittable) and two p-bit
  kernels, a bit-sliced multispin kernel (64 replicas per `u64`) and an SoA `f32` heat-bath.
- `pbit-ir`: the instruction set of the virtual processor. Categorical variables with
  allowed / forbidden / forced values, unary weights, pairwise couplings and at-most-k caps;
  three tiers: exact (enumeration, a frontier dynamic program, minimum-remaining-values search),
  a constraint-preserving Gibbs sampler, and the certification gate.
- `pbit-decide`: the assignment-router front-end (tasks to workers under quotas, allowed sets,
  clamps and group affinity), lowered bit-identically onto `pbit-ir`; anytime mode; annealed
  plan polish; exact-oracle instance families for calibration.
- `pbit` CLI: `demo`, `decide`, `run`, `ir`, `stats` and `version`. JSON in, JSON out. Exit codes
  0 plan, 1 infeasible, 2 bad input, 3 refused. Processor controls from flags, `PBIT_*`
  environment variables or `pbit.json`: threads, chains, CPU limit, memory limit, priority,
  progress, fixed-work sweeps, exact-tier time cap, frontier cap, wall-clock budgets.
- Zero external crates: a fresh checkout builds offline with stable Rust.
