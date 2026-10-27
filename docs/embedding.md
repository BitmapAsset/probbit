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

## The host hooks

`probbit_persona::hooks` lets a host see the decision pipeline: `set_monitor(tier, gate_seen)` (the CLI's `--top` view) and
`set_config_echo(f)` (the telemetry fields that name the configuration in effect). Unset, the monitor calls do nothing and
the echo is empty.
