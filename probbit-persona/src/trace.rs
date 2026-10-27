//! `turn_trace` (feature, off by default): where one event's time and bytes go, by phase, so a host measures the same breakdown
//! on its own hardware (docs/embedding.md "The turn's phases"). With the feature off every hook is an empty inline function.
//!
//! `Live::event` opens a trace and closes it on its way out; between, each phase is a segment of the installed clock
//! (`probbit_core::rt` on `nostd`). After the event `last()` holds per phase the microseconds spent in it and, when the host
//! installed a probe (`set_probe`), the largest value the probe returned at the end of one of its segments: a host reads its own
//! counter there (its allocator's peak since the last call, the stack's high-water). Turns outside `Live::event` are not traced.
//! The parse of the event line and the append of the strand line are the host's, before and after the call.
#[allow(unused_imports)]
use std::prelude::rust_2021::*;

/// A phase of one event
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// the event's inputs read and stamped (elapsed hours)
    Input = 0,
    /// RFC 0009: the deviation and the trajectory (exact solves on the state before the event)
    Expect = 1,
    /// the steps before the compiler: provenance, payments, rest, senses / needs / drives / pace, credit, the surprise gate
    Prep = 2,
    /// the persona's program for the event (and for a yield or a hold)
    Compile = 3,
    /// the engine's calls
    Solve = 4,
    /// the engine's answer read back into a stance and the next state
    Decode = 5,
    /// the stance document: its blocks, credit, glyph and digest
    Document = 6,
    /// the strand line (and a rest line due before it) and its digest
    Line = 7,
    /// RFC 0009: the expectation's memory after the event
    Fold = 8,
}
/// The number of phases
pub const PHASES: usize = 9;
/// The phases' names, in `Phase` order
pub const NAMES: [&str; PHASES] = ["input", "expect", "prep", "compile", "solve", "decode", "document", "line", "fold"];

/// One event's breakdown: per phase (indexed by `Phase as usize`) the microseconds and the largest probe value; the event's
/// total microseconds and the engine calls of the turn; RFC 0009: the expectation's programs answered from its cache
/// (`expect_cached`) and solved (`expect_solved`)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TurnTrace { pub us: [u64; PHASES], pub bytes: [u64; PHASES], pub total_us: u64, pub solves: u32, pub expect_cached: u32, pub expect_solved: u32 }

/// A resume's walk (`strand::resume_from_record`, `strand::resume_boxed_from`): the lines read after its start, the event lines
/// applied to a record that was behind (each one turn without the expectation's member solves) and their microseconds, and the
/// whole call's microseconds (with the feature off: the counts, zero microseconds)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResumeTrace { pub lines: u64, pub applied: u64, pub applied_us: u64, pub total_us: u64 }

#[cfg(feature = "turn_trace")]
mod on {
    use super::*;
    use probbit_core::rt::Instant;
    use std::cell::RefCell;
    struct S { open: Option<(Instant, u64)>, cur: Phase, acc: TurnTrace, last: TurnTrace, probe: Option<fn(Phase) -> u64> }
    thread_local! { static T: RefCell<S> = RefCell::new(S { open: None, cur: Phase::Input, acc: TurnTrace::default(), last: TurnTrace::default(), probe: None }); }
    fn us(t0: Instant) -> u64 { (t0.elapsed().as_secs_f64() * 1e6) as u64 }
    /// The breakdown of the last traced event on this thread (zeros before the first)
    pub fn last() -> TurnTrace { T.with(|t| t.borrow().last) }
    /// Install (or remove) the probe: called with the phase at the end of each of its segments; its value is kept per phase as
    /// the largest seen in the event. Its own time is not charged to the phase.
    pub fn set_probe(f: Option<fn(Phase) -> u64>) { T.with(|t| t.borrow_mut().probe = f) }
    pub(crate) fn begin() { T.with(|t| { let mut t = t.borrow_mut(); t.acc = TurnTrace::default(); t.cur = Phase::Input; t.open = Some((Instant::now(), 0)); }) }
    /// End the current segment and start one of `ph` -> the phase that was current (a call into the engine restores it after)
    pub(crate) fn enter(ph: Phase) -> Phase {
        T.with(|t| { let mut g = t.borrow_mut(); let Some((t0, at)) = g.open else { return ph };
            let (prev, probe) = (g.cur, g.probe); let now = us(t0); g.acc.us[prev as usize] += now.saturating_sub(at);
            if ph == Phase::Solve { g.acc.solves += 1; }
            drop(g);
            let b = probe.map(|f| f(prev));
            let mut g = t.borrow_mut(); if let Some(b) = b { let x = &mut g.acc.bytes[prev as usize]; *x = (*x).max(b); }
            g.open = Some((t0, us(t0))); g.cur = ph; prev })
    }
    thread_local! { static RT: RefCell<ResumeTrace> = RefCell::new(ResumeTrace::default()); }
    /// The last resume's walk on this thread (zeros before the first)
    pub fn last_resume() -> ResumeTrace { RT.with(|r| *r.borrow()) }
    pub(crate) fn resumed(t: ResumeTrace) { RT.with(|r| *r.borrow_mut() = t) }
    /// A clock read for the resume's counts
    pub(crate) struct Stopwatch(Instant);
    impl Stopwatch { pub(crate) fn start() -> Stopwatch { Stopwatch(Instant::now()) } pub(crate) fn us(&self) -> u64 { us(self.0) } }
    pub(crate) fn expect_cache(hits: u32, solves: u32) { T.with(|t| { let mut g = t.borrow_mut(); if g.open.is_some() { g.acc.expect_cached += hits; g.acc.expect_solved += solves; } }) }
    pub(crate) fn end() {
        let cur = T.with(|t| t.borrow().cur); enter(cur);
        T.with(|t| { let mut g = t.borrow_mut(); if let Some((t0, _)) = g.open.take() { g.acc.total_us = us(t0); g.last = g.acc; } })
    }
}
#[cfg(feature = "turn_trace")]
pub use on::{last, last_resume, set_probe};
#[cfg(feature = "turn_trace")]
pub(crate) use on::{begin, end, enter, expect_cache, resumed, Stopwatch};
#[cfg(not(feature = "turn_trace"))]
#[inline(always)]
pub(crate) fn begin() {}
#[cfg(not(feature = "turn_trace"))]
#[inline(always)]
pub(crate) fn enter(ph: Phase) -> Phase { ph }
#[cfg(not(feature = "turn_trace"))]
#[inline(always)]
pub(crate) fn end() {}
#[cfg(not(feature = "turn_trace"))]
#[inline(always)]
pub(crate) fn expect_cache(_hits: u32, _solves: u32) {}
#[cfg(not(feature = "turn_trace"))]
#[inline(always)]
pub(crate) fn resumed(_t: ResumeTrace) {}
#[cfg(not(feature = "turn_trace"))]
pub(crate) struct Stopwatch;
#[cfg(not(feature = "turn_trace"))]
impl Stopwatch { #[inline(always)] pub(crate) fn start() -> Stopwatch { Stopwatch } #[inline(always)] pub(crate) fn us(&self) -> u64 { 0 } }

impl TurnTrace {
    /// As JSON: `{"us": {phase: µs}, "bytes": {phase: value}, "total_us": n, "solves": n, "expect_cached": n, "expect_solved": n}`
    /// (phases in `Phase` order)
    pub fn to_json(&self) -> crate::json::Json {
        use crate::json::Json;
        let row = |v: &[u64; PHASES]| Json::Obj(NAMES.iter().zip(v).map(|(k, x)| (k.to_string(), Json::Num(*x as f64))).collect());
        Json::Obj(vec![("us".into(), row(&self.us)), ("bytes".into(), row(&self.bytes)), ("total_us".into(), Json::Num(self.total_us as f64)), ("solves".into(), Json::Num(self.solves as f64)),
            ("expect_cached".into(), Json::Num(self.expect_cached as f64)), ("expect_solved".into(), Json::Num(self.expect_solved as f64))])
    }
}
