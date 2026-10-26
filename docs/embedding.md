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
  `persona::parse_doc(text, is_json)` and build it with `persona::build(&doc)`.
- **No usage telemetry.** `sys::usage()` and `sys::nice()` return `None`, so `process_cpu_ms`, `peak_rss_mb` and `nice` are
  `null` in telemetry.

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
