# Embedding probbit: the `probbit-persona` library, with std or on `no_std`

The CLI's document layer is a library crate, `probbit-persona`: the JSON and YAML readers, the persona layer (parse, build,
lint, prove, bound, hostlint, the turn, `display.glyphs`), `run` (a probbit-ir JSON program), `router` and `evaluate` (feature
`decide`), and the strand writer and its checks (`strand`). The CLI, probbit-wasm and a firmware run the same code, so a
document or a strand line written by one is the one the others write.

| feature | default | what it gives |
|---|---|---|
| `std` | yes | builds on std (the CLI, probbit-wasm, any host program) |
| `nostd` | no | builds `no_std` + `alloc` for a target without an operating system; the std subset comes from `probbit-nostd` |
| `decide` | yes | `router` (`probbit decide`) and `evaluate` (the decision-API adapter), through probbit-decide |

probbit-core, probbit-ir and probbit-decide have the same `std` (default) and `nostd` features. A host program takes the
defaults:

```toml
[dependencies]
probbit-persona = "0.14"
```

## On `no_std`

```toml
[dependencies]
probbit-persona = { version = "0.14", default-features = false, features = ["nostd"] }  # add "decide" for router/evaluate
probbit-core = { version = "0.14", default-features = false, features = ["nostd"] }
probbit-ir = { version = "0.14", default-features = false, features = ["nostd"] }
```

The firmware supplies a global allocator (for example `esp-alloc`) and, because `probbit-nostd` builds 64-bit atomics from
`portable-atomic`'s critical-section fallback, a `critical-section` implementation (the HAL's). Checked on
`xtensa-esp32s3-none-elf` (the esp toolchain, `-Zbuild-std=core,alloc`):

```sh
cargo +esp check -p probbit-persona --no-default-features --features nostd --target xtensa-esp32s3-none-elf -Zbuild-std=core,alloc
```

What changes without an operating system:

- **The facade.** `probbit-nostd` is the subset of `std` the crates use, built from `core`, `alloc`, `hashbrown`, `libm` and
  `portable-atomic`. Each crate imports it as `std` on `nostd`, so the code is the same code. Float functions that live in
  `std` (`exp`, `ln`, `powf`, `tanh`, ...) come from `libm` through the `FloatExt` trait; `powi` is computed as compiler-rt's
  `__powidf2` computes it on the host. `print!`/`println!` go to a console hook: `probbit_nostd::io::set_print(fn(&str))`.
- **The clock.** `probbit_core::rt::Instant` is the clock you install: `probbit_core::rt::set_clock(fn() -> f64)` returning
  milliseconds (a firmware passes its system timer). Before any call that measures time, install it.
- **No threads.** `rt::sequential()` is on by default: every chain, gate pass and polish chain runs in order on the calling
  thread, as probbit-wasm runs. At fixed work the documents are the ones the CLI writes at any `--threads` (timings and
  `telemetry.threads` aside). Set `probbit_ir::GATE_THREADS` to 1 as probbit-wasm does. One core calls the engine.
- **Documents arrive as strings.** There is no file system: `persona::load(path)` returns an error. Read the document with
  `persona::parse_doc(text, is_json)` and build it with `persona::build(&doc)` (or `build_boxed`, below).
- **No usage telemetry.** `sys::usage()` and `sys::nice()` return `None`, so `process_cpu_ms`, `peak_rss_mb` and `nice` are
  `null` in telemetry.

## Stack budget: the boxed setup path

A small target runs the engine on a thread with a fixed stack (tens of KB). The by-value setup calls move `Persona` (about
1.5 KB), `State` (about 1 KB) and `Live` through their frames, and the persona reader recursed through itself once per layer of
the document (`display`, `bound`, `expectation`, the lowered blocks) with all of its readers inlined into that frame. The
heap-returning calls keep each of them on the heap from the moment it is built:

```rust
use probbit_persona::{persona, strand::{self, Clock, Live}};
let doc = persona::parse_doc(TEXT, false)?;                              // Json: a 32-byte value; the tree is on the heap
let p = persona::build_boxed(&doc)?;                                     // Box<Persona>
let st = persona::init_boxed(&p, None, true, &eng);                      // Box<State>
// a fresh strand: the header goes to `out` in pieces of at most 1 KB (write each, yield between them), not as one String
let live = Live::start_to(p, &doc, st, Clock::Real(probbit_core::rt::Instant::now()), &strand::engine(), 1024, &mut out);
// or: let (live, header) = Live::start_boxed(p, &doc, st, clock, &engine);  // Box<Live> and the header whole
// a stored strand: let live = strand::resume_boxed(&text, &p, st, clock)?;  // Box<Live>
// or from the last checkpoint without the whole text: strand::resume_boxed_from(&header, Some(&cp), line, trailing, &p, st, clock, &eng)?
// a replay from the header: let r = strand::Replay::open_boxed(&header)?;  // Box<Replay>, its Live boxed in it
// each event: live.event(&ev, &eng)?  -> (stance, line); the turn inside passes its state boxed (persona::turn_boxed)
```

`build`, `init`, `Live::start` and `resume` stay as they were (thin wrappers over the boxed calls) for hosts with a large stack.
The header the pieces make is `Live::start`'s header byte for byte, and `strand::write_header` returns its sha256 (the first
line's `prev`), so the chain is the same however the host cuts it; neither the line nor a copy of the document is held whole.
The YAML reader is bounded like the JSON one: block and flow collections nested deeper than 256 are a `limit` error (the JSON
reader already refused them); on a stack of tens of KB keep documents a few levels deep, as every example is.

The stack each call needs, measured by a host proxy: probbit-persona `nostd` on x86_64 (release, `lto = "fat"`), each call
run from one frame on a thread whose free stack is painted with a pattern first and scanned after (the high-water below the
calling frame), and a counting allocator for the heap peak above what was live before the call. The persona is a 12 KB body
persona (traits, moods, senses, drives, needs, expectation, display; its header line is 11,985 bytes), read from YAML.

| call (before → after) | Xtensa frame, B | stack high-water, B | heap peak, B |
|---|---:|---:|---:|
| `parse_doc` (YAML) | 128 → 128 | 4,760 → 5,048 | 93,411 → 93,411 |
| `build` → `build_boxed` | 3,312 → 400 (its reader `build_base` 1,280) | **39,024 → 12,080** | 267,736 → 269,232 |
| `init` → `init_boxed` | 1,184 → 624 (+ `init` 1,184) | 9,240 → 9,240 | 69,365 → 69,365 |
| `Live::start` → `Live::start_to` / `start_boxed` | 1,888 → 160 / 96 (`write_header` 448) | 9,872 → 4,560 | 87,482 → 19,181 |
| **setup: parse + build + init + start** | | **39,056 → 12,112** | 332,669 → 334,165 |
| `resume` → `resume_boxed` | 4,880 → 960 → 896 (`check_chain` 6,320 → 96 + `check_chain_head` 384) | 55,328 → 29,264 (30,560 measured again with the phase rows added) → **15,792** | 397,428 → 398,924 → 356,129 |
| one event (`Live::event`) | 2,688 → 2,672 → 960 (`turn_src` 6,160 → 528; its steps `turn_prep` 448, `turn_solve` 1,312, `turn_doc` 704) | 31,904 → 31,824 → **13,808** | 112,990 → 112,990 → 88,149 |
| the solve alone (in the event) | | 6,856 | 29,009 |
| a checkpoint line | 160 | 5,408 → **3,264** (the key sort in place) | 42,594 |

Three columns where a row has them: before the boxed setup, after it, and after the boxed turn and resume. The turn's steps
(`turn_prep`, `turn_solve`, `turn_doc`, the state steps, the decoder) are out of line and pass the state boxed, so the deepest
frame is one step's; the expectation's programs (`program_json`) compile the same way. `resume` opens the header once: the chain
check builds its persona on the heap, writes the header again in pieces against the stored line (no `Live`, no whole line) and
`resume` continues with that persona instead of building it a second time; the expectation's fold reads one line at a time out
of line. The heap peak of setup and of `resume` is the reader's (the document tree and the lowered documents); the event's
(88,149 B) is the live set of a turn: the expectation's exact solves, then the program as a document, the engine's answer and
the stance under construction (per phase, the peak above the event's start: the expectation's solves 66,879, prep 20,221,
compile 73,957, the solve 88,149, decode 76,529, document 60,295; the program tree is held from the compiler to the engine and
the engine's answer to the decoder).

**Size the engine stack from the turn** (and from `resume` for a host that boots on a stored strand), not from setup: setup, one
event and `resume` now need about the same (12,112, 13,808 and 15,792 B on the proxy), so the largest of the three plus the
host's own frames is the thread's stack.

### The turn's phases (`turn_trace`)

With the feature `turn_trace` (off by default; without it the hooks are empty inline functions) `Live::event` records per
phase the microseconds spent and the largest value of a probe the host installs, so a firmware reads the same breakdown on
its own clock and counters:

```rust
use probbit_persona::trace::{self, Phase};
fn probe(_: Phase) -> u64 { /* e.g. the allocator's peak since the last call, reset; or the task's stack high-water */ 0 }
trace::set_probe(Some(probe));          // optional: called at the end of each phase's segment
let (stance, line) = live.event(&ev, &eng)?;
let t = trace::last();                  // t.us[Phase::Solve as usize], t.bytes[..], t.total_us, t.solves
```

The phases: `input` (the event stamped), `expect` (the deviation and the trajectory: exact solves on the state before the
event), `prep` (provenance, payments, rest, senses / needs / drives / pace, credit, the surprise gate), `compile`, `solve`
(the engine's calls), `decode`, `document` (the stance's blocks, credit, glyph, digest), `line` (the strand line and a rest
line due before it) and `fold` (the expectation's memory). The parse of the event line and the store of the strand line are
the host's, before and after the call. `probbit live` built with `--features turn_trace` prints one `trace` line per event
on stderr when `PROBBIT_TURN_TRACE` is set (with the parse and the append it timed); stdout and the strand are unchanged.

On the proxy (the body persona's last recorded event, x86_64, median of 21 runs) the turn takes 1,249 µs: expect 727, compile
130, solve 117, decode 102, document 97, prep 49, line 13, fold 4 (the host's parse of the event line and its append to a buffer
are about 1 µs each). The solve is 9.4% of it; the expectation's exact solves are 58%. Stack high-water and heap peak per phase
on the proxy: expect 15,520 / 66,879, prep 14,880 / 20,221, decode 12,464 / 76,529, solve 11,544 / 88,149, compile 8,816 /
73,957, document 6,192 / 60,295, fold 4,592 / 35,154, line 4,088 / 35,745, input 3,424 / 1,005 (with the feature compiled in,
which adds about 1.7 KB to the event's stack: 15,520 against 13,808 without it).

### The expectation's cache

`expect` (RFC 0009, a persona with an expectation block) solves, every event, the event's own program, the empty program and
one program per remembered combination (exact tier each). `Live` keeps the non-twin exact marginals of each program it solved
(`Live::expect_cache`, a `persona::ExpectCache`, on by default), keyed by the sha256 of the program's structure without its
`comment` (the only line that names the turn and the seed; each number by its bits, -0 as 0, as the program's text writes
it). The exact tier's marginals are a function of the program's model alone, so a row is the value the solve would return:
the stance, its `deviation`, every strand line and digest are the same with the cache on or off (`ExpectCache::new(false)`).
After each event only the rows that event used are kept: at most `max_combos + 1`. Decay never invalidates a row (marginals
do not read counts); a row stops being used when its program changes, which happens whenever the state the program is built
from moves (moods, needs, senses: every event of most lives; the state's digest itself carries the turn, so it is never a
key). The cache skips solves, not builds (the next section removes most of the build). A life whose programs move
every event gets no hit; after an event without one the cache is not consulted for the next 1, 2, 4, ... 32 events (a hit
resets the pause), so such a life keys two events in 34. `turn_trace` counts `expect_cached` and `expect_solved` per event.

On the proxy (median of 3–7 runs, cache off → on, `expect`'s share of the turn): a life of one event repeated 500 times
(the expectation fixture persona, max_combos 16): 52.2% → 37.4%, 95.2% of programs from the cache; 500 events of many
combos: 80.9% → 81.2% (max_combos 16) and 93.6% → 93.7% (64), no hit; the body persona's recorded day (134 events, max_combos
6): 74.2% → 74.5%, 0.7% of programs from the cache. The rows' heap on the proxy (x86_64): 13,771 B at most for the body
persona (7 rows), 46,471 B (17 rows) and 178,327 B (65 rows) for the fixture persona at max_combos 16 and 64; a 32-bit target
holds about two thirds of that (its `String` and `Vec` headers are 12 bytes, not 24). `ExpectCache::bytes` reports it on the
target. A host short of heap at a large `max_combos` turns the cache off; the bytes do not move. With the cache the body
persona's turn on the proxy takes 13,840 B of stack (13,808 before) and its heap peak is 89,990 B (88,153 before: the rows
the individual holds between events).

### The expectation's members: one program text per event

Measured before (proxy, the body persona's recorded day, 134 events, max_combos 6; `turn_trace`'s `expect_build_us` and
`expect_solve_us`): 6.63 programs per event, each about 105 µs to build (prep 40, compile 61) and 110 µs to solve (`from_json`
47, the tier ladder with its answer document 36.5, the document's finite check 10, the cache key's hash over the text). The
exact solve itself is the small part; the program as text and back is about half of every member.

A member's program is the event's own program A_t with another `h`: the persona's values, variables, pairs and steps are the
same, and so are the rules whenever the habits in force are A_t's. So, with the cache on (`Live`'s default):

- A_t is built as before (its text and the compiler's meta) and read once into the model's parts before the compile pass
  (`run::from_json_parts`), on the first program the cache does not answer.
- Every other member (the empty program, each remembered combination) runs prep, credit and the gate, then the compiler's
  arithmetic without writing its text (the field per variable and the rules in force). With A_t's rules, its model is A_t's
  parts with its own `h` (each level at the value index its name has in the text, zeros where the text omits a weight, the
  twins copied, each weight checked as the text's is), then the compile pass and `Model::new` as `from_json` runs them
  (`run::Parts::model_with`): the model the engine would build from its text. A_t's own field mapped this way is checked
  against the `h` its text carries before any member uses the mapping.
- The model is solved by the exact tier `run` answers a `decide` with (`run::enumerated`): over `exact_limit` raw plans the
  components tier, within it the whole-program enumeration, the same calls with the same arguments, the same verdicts, the
  same finite check; the marginals are read as from the answer document. Another tier (one of those declining, or `sweeps: 0`
  over the limit, where `run` stops on the wall clock), or other rules in force: the member's text is written from its meta
  (no second build) and run by the engine, as before.
- The cache keys these programs by A_t's text without its comment and its vars' `h`, then the program's field: equal keys,
  equal texts, as the cache's own key.

No member's text, digest or marginals is in a stance or a strand line (only the deviation's numbers, from the marginals), and
the marginals are the same numbers: every stance, strand line and digest is the same as with the cache off, which is the
reference path (`ExpectCache::new(false)`, `persona::deviation`: every member's text built and run by the engine). The engine
argument is not called for these members; the deviation already reads members as the engine's exact tier (the cache returns
rows without calling it), and every engine in the repository is `persona::run_program`.

On the proxy (median of 3–5 runs, before → after, `expect`'s share of the turn, cache on): the body persona's recorded day
74.6% → 67.7% (`expect` 1,467 → 1,047 µs per event; per member build 105 → 84 µs, solve 110 → 65 µs); its two recorded events
50.6% → 41.8%; the fixture persona, one event repeated 500 times, 37.1% → 35.2%; 500 events of many combinations 81.6% →
81.9% (most of its members have other habits in force than A_t: they keep the engine's path, without a second build). What is
left per member on the body is its prep (about 40 µs: the senses, needs and drives step on the member's own readings) and the
compiler's arithmetic, then the components tier; with M members per event this is the floor of any exact path that solves
every member. The body persona's turn on the proxy takes 13,824 B of stack (13,840 before) and its heap peak is unchanged
(89,990 B, at the turn's solve; inside `expect` the peak rises from 68,476 to 87,214 B: A_t's parts and one member's model).

### The expectation's members: what the deviation reuses

The event's own program A_t was compiled and solved twice per event: once by the turn, once as the deviation's own member.
Since the turn's first compile reads the same state and inputs as the deviation's A_t, `Live::event` now runs the turn first
and, with the cache on, the turn hands the deviation its first program's compile meta and that answer's exact marginals
(`persona::turn_at`, `persona::deviation_at`). The two programs are the same text when no rest line is due before the event,
`src` is taken out of the inputs alike (a `reward_from` without a senses, credit or needs block keeps it in the turn only) and
the persona's op is `decide`; the answer's marginals are the deviation's when its verdict is exact (an answer without an exact
verdict means no deviation, either way). Any other event: the deviation builds and solves A_t itself, as before. A_t's text is
written again from its meta (no prep, no compile) only to read the members' skeleton and key. `ExpectCache::with_reuse(false,
_)` keeps the old order (A_t built and solved by the deviation); `with_reuse(true, true)` builds and solves A_t a second time
as well and compares the two, text for text and bit for bit (a test's instrument). The phases' order in `turn_trace` changes:
the turn's phases come before `expect`.

The member path's cost after that: prep copies only what it steps and reads (the senses and drive states it replaces are not
copied first; a member's copy leaves out the persona's name, the agenda, the resting stance, the learned-from counts and the
digest); the compiler's contributions carry no source name for a member (`Opts::meta_only`: the names exist for the turn's
`why`); and the members share one model (`run::Members`: A_t's parts moved into it, the compile pass's folds of separable
tables kept apart and replayed per member in the pass's order, checked bit for bit on A_t's own `h`), so no second copy of the
pairs is built per member; A_t's text is dropped once it is read. The trajectory runs before the turn, the deviation after
it. Every stance, strand line and digest is the same as before (and as the cache-off reference).

On the proxy (median of 3–5 runs, cache on, `expect`'s share of the turn, before → after): the body persona's recorded day
67.7% → 63.9% (`expect` 1,042 → 894 µs per event, -14%; the rest of the turn about 500 µs; 5.63 member programs per event,
each about 79 µs to build and 69 µs to solve); its two recorded events 43.3% → 26.3%; the fixture persona, one event repeated,
35.8% → 32.9%; 500 events of many combinations 81.9% → 81.7%. Each step: A_t once 65.2%, the prep copy 64.7%, no source names
64.4%, one model 63.8%. Stack: the body persona's turn 13,584 B (13,824 before). Heap (the body persona's two recorded events, the
last one): the deviation now runs while the turn's stance document and next state are held (about 36.7 KB), so the event's
peak is 97,583 B, inside `expect` (89,990 before, at the turn's solve; inside `expect` 87,214); the deviation's own peak above
what it finds held fell from 86,209 to 60,857 B (one model instead of A_t's parts and a member's model; a member that keeps the
engine's path still builds its text and the engine's answer). A host short of heap keeps the old order with
`ExpectCache::with_reuse(false, false)`: the deviation before the turn, A_t built and solved by it, the same bytes; the body
persona's event peak is then 89,990 B, at the turn's solve (inside `expect` 69,332 B), at the cost of A_t's second build and solve.

### The expectation's members: the order with the reuse, and what is held

With the cache reusing the turn's program (the default), `Live::event` runs the deviation in two parts around the turn
(`persona::deviation_pre`, `persona::deviation_post`):

1. **Before the turn** (nothing of the turn held): every member but A_t is solved from the pre-event state, E_t first (when E_t
   has no exact answer there is no deviation and no other member is solved), then the remembered combos in the mixture's
   order. Each member's exact marginals are kept as a row of f64 in the order of its layout (about 0.4 KB per member on the
   body persona; the layout is kept only as its sha256: each variable's id and its levels' names in order). The cache keeps
   only this event's rows through the turn.
2. **The turn**, which hands A_t's compile meta and exact marginals (T3-b).
3. **After the turn**: A_t's layout must hash to the held one; the rows are named again from it (the same names, values and
   order), A_t's row goes into the cache, and the mixture runs as before. A layout that differs solves the members again; an
   event the turn does not hand A_t for (a rest due, another op) builds and solves A_t after the turn.

The members share one model, read without any program text: the skeleton comes from E_t's text without its pairs (a few KB),
and the pairs are the persona's couplings read as `run::from_json` reads the text's (r6 of each table entry at its values'
indices, each weight checked as the text's). A member whose rules in force differ from E_t's (other habits, rules on inputs)
takes the same model with its own caps: its rules lowered as `from_json` lowers a text's (from its text without the pairs),
the compile pass's never-binding caps dropped, `Model::new` on the same parts (`run::Members::set_caps`, no copy of the pairs);
its cache key is the skeleton's, its rules' and its field's. The components tier's split of that model (the components, their
kinds, the sub-models of the components with caps or cycles) does not read `h`, so it is computed once per caps and reused by
every member (`probbit_ir::exact_components_split`, each sub-model's `h` copied in; the same tiers on the same numbers).

Every stance, strand line and digest is the same as with the old order and as with the cache off
(`the_members_before_the_turn_move_no_byte`; the IR test `one_split_over_many_h_is_bit_identical` for the split).

On the proxy (cache on; the body persona: its recorded day for time, its last recorded event for heap; one node session):

| | T3-b head | members before the turn | every member a model | the split once |
|---|---:|---:|---:|---:|
| `expect` share of the turn, recorded day | 64.1% | 63.2% | 57.9% | 57.5% |
| per member: build / solve (µs) | 80.7 / 71.0 | 71.7 / 65.7 | 66.9 / 41.6 | 67.5 / 40.5 |
| members on the engine's path, recorded day | 253 | 159 | 0 | 0 |
| event heap peak (B) | 97,583 | 88,844 | 88,844 | 88,844 |
| heap inside `expect` (B) | 97,583 | 72,568 | 55,920 | 55,920 |
| turn stack (B) | 13,584 | 13,504 | 13,552 | 13,552 |

The event's heap peak is now the turn's solve (its own allocations as before, plus about 1.2 KB of held rows); inside `expect`
the peak is the members' (no program text, no answer document). `ExpectCache::with_reuse(false, false)` keeps the old order
(the whole deviation before the turn, A_t built and solved by it): event peak 89,990 B, inside `expect` 72,003 B, same bytes;
the reuse is the better default now on both. On the fixture persona's 500 varied events the members are solved before the
turn knows whether A_t has an exact answer, so events without one solve their members for nothing (about 3.6 programs per
event); the model path still brings that life's share from 81.7% to 74.8% (its steady life 33.2% → 19.4%).

Silicon numbers (the same calls on the target's engine thread) are measured by the firmware and will follow; the x86_64 proxy
and an Xtensa build differ in frame layout, so read the proxy as the shape and the ratio, not the target's bytes.

## The strand writer, and how the host checks it

`probbit_persona::strand` has no file I/O: each call returns the line, and the caller stores it (the CLI appends to a file under
a writer lock, `probbit-cli/src/live.rs`; a firmware writes flash).

```rust
use probbit_persona::{json, persona, strand::{self, Clock, Live}};

// how a turn runs its program: in order on this thread (as the CLI's tests and probbit-wasm run it)
let engine = |prog: &json::Json, f: &persona::Flags| persona::run_program(prog, f, 1, 100, 0);
let doc = persona::parse_doc(PERSONA_YAML, false)?;          // Err(String): the reader's message
let p = persona::build(&doc)?;                               // Err(json::InErr): path, code, message
let st = persona::init(&p, Some(seed), true, &engine);
let (mut live, header) = Live::start(p, &doc, st, Clock::Fixed, &strand::engine());
store(&header);
let (stance, line) = live.event(&json::parse(r#"{"praise": true, "elapsed_hours": 0.5}"#)?, &engine)?;
store(&line);
if live.n % strand::CHECKPOINT_EVERY == 0 { store(&live.checkpoint(&stance)); }
```

- `Clock::Fixed`: each event carries its own `elapsed_hours`, so the strand is a pure function of its events. `Clock::Real`
  holds an `rt::Instant` (the installed clock on `nostd`).
- `live.control(what, by, reason, at)` writes pause, resume and retire lines; on `nostd` the caller passes `at` (there is no
  `strand::utc_now`).
- Every line carries `prev`, the sha256 of the line before, so the strand is one chain from its header.

The host reads the strand back (over serial, from flash) and checks it with the same code: `probbit live verify STRAND`, or
`strand::verify(&text, &engine)` in a program. Verify replays every event from the header's persona and initial state and
names the earliest line that differs (a changed, removed or reordered line, or a stance or state digest that the replay does
not reproduce), or the earliest field this engine does not know (`unsupported`: not verified, and not a divergence).
`strand::Replay` steps a strand line by line, and `strand::resume(text, &p, st, clock)` continues one from its last state.

A `nostd` build takes its transcendentals (`ln`, `exp`) from the `libm` crate, which can differ from the host's libm in the
last bits (2 ULP measured on one deviation). Every number a strand line carries is rounded (6 decimals, or 5 significant
digits for the deviation receipt), so the device's strand and the host's replay have the same bytes; from engine 0.14.0 this
includes the stance's `deviation` on checkpoint lines (`strand::DEV_R6_SINCE`). A strand whose header names an earlier engine
keeps its full-precision deviation, and verify accepts it when it equals the replay at 6 decimals.

**The engine string.** A strand's header names the engine that wrote it, and the rules a replay applies follow that name, not
the replaying engine: from `probbit 0.14.0` on (a `-dev` or other suffix ignored: `probbit 0.14.0-dev` is 0.14.0) a strand gets
the rounded deviation, the canonical fold of the expectation's memory and `expect` on checkpoints; an earlier name keeps the
earlier rules, byte for byte. A firmware passes its engine string to `Live::start` (the header's `engine`) and should pass the
version of the crate it builds with (`strand::engine()` gives `probbit ` + that version; a development build says `probbit 0.14.0-dev`),
so a strand it writes replays on the host under the same rules. A strand that keeps naming an older engine keeps the older
fold, and its boot folds the expectation over every line (no checkpoint carries the memory).

An engine older than the strand verifies what it can read: it does not compare engine names. At the first line with a field it
does not know it stops with `unsupported by this engine: FIELD` (exit 5: a newer engine's strand, not verified, and not a
divergence), so 0.13.0 answers exit 5 at the first checkpoint that carries `expect`. A 0.14.0 strand without such a line (no
expectation block, or no checkpoint yet) has nothing 0.13.0 does not know, and 0.13.0 replays it under its own rules (ok while
those give the same bytes). A segment file after the first (`NAME.0001.strand`, ...) is not readable before 0.14.0: its
second line chains into the previous file, and 0.13.0 reports a divergence at line 2 (exit 1) before it reaches the header's
`segment` key. Verify strands of 0.14.0 with 0.14.0 or later.

### Sizing a strand on flash: checkpoints and segments

A strand grows by one event line per event and one checkpoint line every K events (`live.checkpoint`), and it never shrinks.
With E bytes per event line, C per checkpoint line and H for the header, n events take about H + n x (E + C / K) bytes. The
expectation fixture persona of the CLI's tests (a 6,312-byte header, a 10-input tutor with an `expectation` block, 500 events
over many input combos) measures E <= 498 B and, from engine 0.14.0 (the checkpoint carries the expectation's memory),
C = 6,369 B with `max_combos` 16 and C = 14,121 B with `max_combos` 64, the memory's largest window (63 named combos); before
0.14.0 the same checkpoint was 3,888 B and the memory it did not carry had grown to 387 combos. At K = 50 and one event a
minute that is about 37 KB an hour: 1 MB lasts about 27 hours. A larger persona scales both: one measured at about 0.85 KB per
event and 8.2 KB per checkpoint fills 1 MB in 3.5 to 4 hours at K = 50 and its event rate.

Segments bound the file a life is stored in: `--segment-every S` (or a host's own seal: a checkpoint line, then
`strand::segment_header(&live, &doc, engine, k, previous_name, &sha256_of_that_line)` as the first line of a new file and the
checkpoint line as its second) costs one header and one repeated checkpoint per segment, so a segment file is about
H + C + S x (E + C / K) bytes (for the fixture at S = 500, K = 50: about 325 KB). A host that keeps the newest two segments and
deletes the older ones holds a life of any length in about twice that; `verify --chain` then reports the first missing file
(exit 6) instead of a divergence, and each kept segment still verifies on its link. Resume works on the newest segment alone:
`strand::resume` reads its header, its sealed line (the state and, from 0.14.0, the expectation's memory) and the lines after
it: a boot parses and hashes the lines of that one segment, folds the expectation's memory over the lines after its last
checkpoint only, and never solves a stored event again.

### Resume on a body: the call and the three recoveries

A body keeps three things between boots: the persona document (it rides in the strand's header line, which also names the
engine and the initial state), **the byte offset of the last checkpoint line** it wrote, and **its last state record** (the
state after the event it last answered, in its own small log). On boot it reads the header line, the checkpoint line at the
stored offset and then the lines after it one at a time, and calls

```rust
// trailing: an iterator over the lines after the checkpoint (each without its line ending), read from flash as it goes
let live = strand::resume_boxed_from(&header, Some(&checkpoint), checkpoint_line_number, trailing, &p, record, clock, &eng)?;
// before the first checkpoint: resume_boxed_from(&header, None, 1, lines_after_the_header, ...)
```

The header is checked as `verify` checks it (the persona built once on the heap, the header written again in pieces against
the stored line); the checkpoint line by its own fields (canonical JSON, its state and the expectation's memory read, an event
count), its link to the line before taken on the host's offset, as a segment's sealed line is taken on its link; every line
after it is chain-checked as `resume` checks it. The whole strand is never loaded, and nothing before the checkpoint is read
again. The three recoveries:

1. **The record is the strand's last state** (the normal boot): the walk reads the trailing lines, finds the record at the last
   event line and returns the individual; no turn runs.
2. **The record is k lines behind** (a reboot between a line and its record): the record is found at the line that logs it and
   the k event lines after it are applied (each its turn's main solve, not the expectation's member solves; docs/persona.md
   §5.7 "Resume from a record"). The result is the individual the unbroken life has, byte for byte; nothing needs the host to
   resync. A record older than the checkpoint is refused (`state`): resume from the whole strand with
   `strand::resume_from_record(text, ...)`, which starts at the last checkpoint before the record.
3. **No record** (a first boot on a copied strand, a lost state log): replay from the header with `strand::Replay::open_boxed`
   (every event solved again, the cost of `verify`), or start at the checkpoint's state: a checkpoint line carries the whole
   state, so a record taken from it (`State::read` of its `state`) resumes at once and applies the lines after it.

The cost on the host proxy (probbit-persona `nostd` on x86_64, the 12 KB body persona; the strand: 50 events, one checkpoint,
then T event lines; record last; median of 7): `resume_boxed_from` = **1.46 ms fixed + 13.4 µs per trailing line** (T = 0, 58,
200, 449: 1,459, 2,328, 4,284, 7,456 µs), against the whole-text `resume_boxed` at 2.3 ms + about 15 µs for every line of the
strand. The fixed part is the header's check and the persona's build plus the checkpoint's state and memory read. An applied
line (recovery 2) costs about 612 µs where the turn costs 2,580 µs on that life (0.24 of a turn: the expectation's member
solves, most of this persona's turn, are not run). A body measured the whole-text resume at about 488 ms fixed + 8 ms per
line, so its boot cost now follows the lines since its last checkpoint, not the length of its life (silicon numbers for the
new call are the body's to measure, with `turn_trace`: `trace::last_resume()` gives the lines walked, the lines applied and
their µs).

Stack on the proxy (the same harness as the table above, 20 KB thread run too): `Replay::open_boxed` and its steps over a
500-event strand **16,192 B** (the by-value `Replay::open` was 24,752 B: it built the persona and the `Live` by value and held
the `Live` in the replay; `open` is now `open_boxed` moved out of its box, 16,736 B with the replay in the caller's frame);
`resume_from_record` with a record 1, 5 or 49 lines behind **16,080 B** (13,648 with the record last); `resume_boxed_from` a
checkpoint + 58 lines **12,656 B** (the whole-text `resume_boxed` of the same strand 15,072 B). All three ran on a 20 KB thread.

### Self-report in standby (RFC 0013)

A body that idles can give an account of its recent events without running a turn: `reflect::reflect_window(&lines, w)` reads
the strand's lines and returns the canonical `reflection` document (docs/rfcs/0013-self-report.md); `reflect::line(prev, &r)`
is the line to append (`{"prev", "reflection"}`), which `live verify` recomputes. No solve, no randomness, no model: one pass
over the lines from the expectation fold's start (the last checkpoint before the window carrying the expectation's memory),
each event line parsed once and each checkpoint line in the window parsed once, so the cost is O(lines read), dominated by
the checkpoint lines' size. Measured on the x86_64 host (release, LTO; median of 21 calls), with the stack high-water and the
heap peak of one call on the `nostd` build (the stack painted below the caller and scanned after the call, as for the setup
path above):

| Strand | Window | Lines | Host µs (std) | Host µs (`nostd`) | Stack high-water | Heap peak |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| a recorded day, 1,160 events, 23 checkpoints (0.99 MB) | last 50 | 1,184 | 664 | 601 | 5,552 B | 217,674 B |
| the same | last 200 | 1,184 | 1,299 | 1,248 | 5,552 B | 223,787 B |
| the same | all | 1,184 | 5,303 | 5,283 | 5,552 B | 251,144 B |
| the tutor with an expectation block, 500 events, checkpoint every 50 | last 50 | 511 | 335 | 344 | 6,568 B | 128,292 B |
| the same | all | 511 | 1,221 | 1,320 | 6,568 B | 140,547 B |

The heap peak is the parsed checkpoint lines (a checkpoint line of the recorded day is about 20 KB of JSON, held parsed while
it is read); a body short of heap reflects a window with fewer checkpoint lines in it. The document is byte for byte the same
from the std and `nostd` builds and the CLI (6/6 windows over the two strands). Not measured on silicon.

### Measured on one board (ESP32-S3)

First numbers from one ESP32-S3 at 240 MHz, the engine on one core shared with a 16 fps renderer, with the boxed setup path
above: the engine task's stack high-water for the setup plus one to three turns was 17,612 to 18,096 bytes (28,304 bytes for
the setup alone before the boxed constructors); the strand header was 11,985 bytes, byte for byte the host's; one stance turn
took 0.60 to 0.93 s with 17 to 23 KB of transient heap per event; the engine's PSRAM use was 50 to 112 KB during setup and run;
the internal heap's floor was 30,216 bytes through the header write. These are one board's measurements, not bounds.

## The host hooks

`probbit_persona::hooks` lets a host see the decision pipeline: `set_monitor(tier, gate_seen)` (the CLI's `--top` view) and
`set_config_echo(f)` (the telemetry fields that name the configuration in effect). Unset, the monitor calls do nothing and
the echo is empty.
