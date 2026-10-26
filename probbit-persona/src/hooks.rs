//! The host's hooks into the decision pipeline: the CLI's `--top` monitor (the tier that is running, what the gate saw) and its
//! configuration echo (`telemetry.config`, the probbit.json in effect). Unset, the monitor calls do nothing and the echo is empty,
//! which is what probbit-wasm and a firmware want. The CLI installs its own at start (probbit-cli/src/main.rs).
#[allow(unused_imports)]
use std::prelude::rust_2021::*;
use crate::json::Json;
use std::sync::atomic::{AtomicUsize, Ordering};

// fn pointers as usize: an atomic store works on every target the engine builds for, with or without std
static TIER: AtomicUsize = AtomicUsize::new(0);
static GATE_SEEN: AtomicUsize = AtomicUsize::new(0);
static CONFIG_ECHO: AtomicUsize = AtomicUsize::new(0);

/// Install the monitor: `tier` gets the name of each tier as it starts, `gate_seen` the gate's R-hat, its TV bound, the released
/// variables and all of them.
pub fn set_monitor(tier: fn(&'static str), gate_seen: fn(f64, f64, usize, usize)) {
    TIER.store(tier as usize, Ordering::Relaxed); GATE_SEEN.store(gate_seen as usize, Ordering::Relaxed);
}
/// Install the configuration echo: the telemetry fields that name the configuration in effect.
pub fn set_config_echo(f: fn() -> Vec<(&'static str, Json)>) { CONFIG_ECHO.store(f as usize, Ordering::Relaxed) }

pub fn tier(t: &'static str) {
    let p = TIER.load(Ordering::Relaxed);
    if p != 0 { let f: fn(&'static str) = unsafe { core::mem::transmute(p) }; f(t) }
}
pub fn gate_seen(rhat: f64, tv: f64, released: usize, n: usize) {
    let p = GATE_SEEN.load(Ordering::Relaxed);
    if p != 0 { let f: fn(f64, f64, usize, usize) = unsafe { core::mem::transmute(p) }; f(rhat, tv, released, n) }
}
pub fn config_echo() -> Vec<(&'static str, Json)> {
    let p = CONFIG_ECHO.load(Ordering::Relaxed);
    if p == 0 { return vec![]; }
    let f: fn() -> Vec<(&'static str, Json)> = unsafe { core::mem::transmute(p) }; f()
}
