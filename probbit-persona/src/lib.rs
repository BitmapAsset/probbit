//! probbit-persona: probbit's document layer and strand writer as a library, for the CLI, the browser module and a firmware.
//!
//! - `json`, `yaml`: the reader/writer of the JSON documents and the YAML subset of persona files (no external crates).
//! - `persona`: the individuality layer (docs/persona.md): parse, build, lint, prove, bound, hostlint, the turn, `display.glyphs`.
//! - `run`: `probbit run` on a probbit-ir JSON program; `router` (feature `decide`): `probbit decide` on a router document;
//!   `evaluate` (feature `decide`): the decision-API adapter.
//! - `strand`: the strand writer (`Live`: start, event, control, checkpoint and rest lines, the `prev` chain) and its checks over
//!   a `&str` (`Replay`, `verify*`, `read_actions`, `first_difference`), with no file I/O.
//! - `sys`: the process's resource usage (getrusage on 64-bit Unix; `None` elsewhere). `hooks`: the host's monitor and config echo.
//!
//! Features: `std` (default) builds on std; `nostd` builds `no_std` + `alloc` for a target without an operating system, with
//! the std subset from probbit-nostd (docs/embedding.md). `decide` (default) adds `router` and `evaluate` (probbit-decide).
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
#[macro_use]
extern crate probbit_nostd as std;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
#[macro_use]
extern crate alloc;
#[cfg(not(any(feature = "std", feature = "nostd")))]
compile_error!("probbit-persona needs feature `std` (the default) or `nostd`");

#[cfg(feature = "decide")]
pub mod evaluate;
pub mod hooks;
pub mod json;
pub mod persona;
#[cfg(feature = "decide")]
pub mod router;
pub mod run;
pub mod strand;
pub mod sys;
pub mod yaml;
