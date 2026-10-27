//! The strand writer and its checks (docs/persona.md §5.7), with no file I/O: the pure half of `probbit live`. Events come in as
//! JSON objects of inputs; the clock stamps each one's `elapsed_hours` (the time since the previous event, quantised to 1e-6 h),
//! so moods decay by their half-lives while nobody is talking to it, and feedback moves the learned deltas (§2.8). Every event
//! appends one line to the strand: the inputs as the turn used them, the stance's and the new state's digests, and the sha256 of
//! the line before. The strand's header carries the persona document, the initial state and the engine version, so `verify`
//! replays the whole life from the strand alone and names the earliest line that differs, or the earliest line with a field or
//! line kind this engine does not know (`unsupported`, a newer engine's strand: not verified, and not a divergence).
//!
//! Control lines (pause, resume, retire: appended by `probbit live control`, never by an event; no credit crosses them) and
//! checkpoint lines (the whole state every K events, so a replay can start there) are written here; `verify` replays all three.
//! The writer returns each line as a `String` and the caller stores it (the CLI appends to a file under a writer lock,
//! probbit-cli/src/live.rs; a firmware writes flash). Every function here takes the strand as text, so it runs on `no_std`.
use crate::json::{self, InErr, Json};
use crate::persona::{self, perr, Engine, Persona, State};
#[allow(unused_imports)]
use std::prelude::rust_2021::*;

/// The strand format
pub const FORMAT: f64 = 1.0;
/// The clock. `Real`: a monotonic clock started with the run; each event's elapsed hours are the time since the previous event
/// (the opening event: since the start). `Fixed`: time comes from the events, each carrying its own `elapsed_hours` (default 0), so a
/// run is a pure function of its events.
pub enum Clock { Real(probbit_core::rt::Instant), Fixed }
/// Hours quantised to 1e-6 h (3.6 ms): the value the turn uses is the value the strand logs
fn q6(h: f64) -> f64 { persona::r6(h) }

/// A strand's life as its control lines leave it: events are answered only while `Active`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status { Active, Paused, Retired }
impl Status {
    pub fn name(self) -> &'static str { match self { Status::Active => "active", Status::Paused => "paused", Status::Retired => "retired" } }
    /// The status a control line leads to: pause an active individual, resume a paused one, retire either; nothing after retire
    pub fn after(self, what: &str) -> Result<Status, InErr> {
        match (what, self) {
            (_, Status::Retired) => Err(refusal(Status::Retired)),
            ("pause", Status::Active) => Ok(Status::Paused), ("resume", Status::Paused) => Ok(Status::Active), ("retire", _) => Ok(Status::Retired),
            // RFC 0004 amendment A2: `erase` returns the learned weights to zero and leaves the status as it was
            ("erase", s) => Ok(s),
            ("pause", _) => Err(perr("control", "already paused")), ("resume", _) => Err(perr("control", "not paused (resume follows a pause)")),
            _ => Err(perr("control", "pause | resume | retire")) }
    }
}
/// The refusal of an event (or a control line) by a paused or retired individual: code `paused` / `retired` (`live` exits 4)
fn refusal(s: Status) -> InErr {
    let msg = if s == Status::Retired { "the individual is retired: every event is refused, for good (probbit live control ... retire)" }
        else { "the individual is paused: every event is refused until `probbit live control STRAND resume`" };
    InErr { code: s.name(), path: "strand".into(), msg: msg.into() }
}
/// The default number of events between two checkpoint lines
pub const CHECKPOINT_EVERY: u64 = 1000;

/// A live individual: the persona, the current state, the clock, the strand's chain and its status (control lines)
pub struct Live { pub p: Persona, pub st: State, clock: Clock, last: f64, prev: String, pub n: u64, pub status: Status, pub controls: u64, pub checkpoints: u64, pub rests: u64,
    /// RFC 0009: the expectation's memory (a fold of the logged events) and the last event's exact trajectory (derived, never logged)
    pub expect: Option<persona::ExpectMem>, pub traj: Option<Json>,
    /// The strand's engine is before 0.14.0 (DEV_R6_SINCE): the stance's `deviation` keeps full precision, as that engine wrote it
    pub dev_full: bool,
    /// RFC 0009: the members' exact marginals by program (`persona::ExpectCache`, on by default; off gives the same bytes)
    pub expect_cache: persona::ExpectCache }
/// The engine from which the stance's `deviation` (on checkpoint lines) is rounded to 6 decimals: a transcendental's last bits
/// differ between platforms' libm (macOS, glibc, the `libm` crate of a `nostd` build), and 6 decimals make the line's bytes,
/// and so the chain, the same on all of them
pub const DEV_R6_SINCE: (u64, u64, u64) = (0, 14, 0);
/// A strand header's engine `probbit X.Y.Z` (a `-pre` or `+build` suffix ignored) -> (X, Y, Z); None for any other name
pub fn engine_version(engine: &str) -> Option<(u64, u64, u64)> {
    let v = engine.strip_prefix("probbit ")?.split(['-', '+']).next()?;
    let mut it = v.split('.').map(|x| x.parse::<u64>().ok());
    let r = (it.next()??, it.next()??, it.next()??);
    if it.next().is_some() { None } else { Some(r) }
}
/// A line with its stance's `deviation` self, pred and input at 6 decimals, as canonical JSON (None when it is not JSON): a
/// checkpoint of a strand before 0.14.0 verifies when it is the replay's at the 6 decimals 0.14.0 writes (the libm's last bits
/// set aside); the stance digest excludes `deviation` and the event line's receipt is checked exactly
fn dev_r6(l: &str) -> Option<String> {
    fn field<'a>(j: &'a mut Json, k: &str) -> Option<&'a mut Json> { match j { Json::Obj(v) => v.iter_mut().find(|(x, _)| x == k).map(|(_, y)| y), _ => None } }
    let mut j = json::parse(l).ok()?;
    if let Some(d) = field(&mut j, "stance").and_then(|s| field(s, "deviation")) {
        for k in ["self", "pred", "input"] { if let Some(Json::Num(x)) = field(d, k) { *x = persona::r6(*x); } } }
    Some(persona::canon(&j))
}
/// One line of a strand without its line ending: strands are written with `\n`; a `\r\n` copy (Windows text mode, an editor)
/// reads the same, since a canonical JSON line never ends in `\r` itself
fn bare(l: &str) -> &str { l.strip_suffix('\r').unwrap_or(l) }
impl Live {
    /// Start a life from `st` (an `init` state or a stored one) -> (the individual, the strand's header line). The header keeps
    /// the persona document in its own key order (the compiled program follows it, and `engine.program` with it) and the
    /// state as canonical JSON (keys sorted), so a state read from a file and the same state from `init` give one header; the
    /// header is compact JSON, not key-sorted.
    pub fn start(p: Persona, doc: &Json, st: State, clock: Clock, engine: &str) -> (Live, String) {
        let (lv, header) = Live::start_boxed(Box::new(p), doc, Box::new(st), clock, engine); (*lv, header)
    }
    /// `start` on the heap: the persona and the state come in boxed and the individual goes out boxed, so the setup path of a
    /// small-stack target holds none of them by value (docs/embedding.md "Stack budget"). The header is `start`'s, byte for byte.
    #[inline(never)]
    pub fn start_boxed(p: Box<Persona>, doc: &Json, st: Box<State>, clock: Clock, engine: &str) -> (Box<Live>, String) {
        let mut b = Vec::new();
        let lv = Live::start_to(p, doc, st, clock, engine, HEADER_CHUNK, &mut |c: &[u8]| b.extend_from_slice(c));
        // the pieces are the bytes of one UTF-8 text cut at fixed offsets: whole again, it is that text
        (lv, String::from_utf8(b).unwrap_or_default())
    }
    /// `start_boxed` with the header handed to `out` in pieces of at most `chunk` bytes (the last one shorter) instead of returned
    /// whole: a host writes each piece to its store and may yield between pieces (a flash write of the whole line can outlast a
    /// watchdog). The pieces end to end are `start`'s header (without its `\n`); a piece may end inside a UTF-8 character.
    #[inline(never)]
    pub fn start_to(p: Box<Persona>, doc: &Json, st: Box<State>, clock: Clock, engine: &str, chunk: usize, out: &mut dyn FnMut(&[u8])) -> Box<Live> {
        let prev = write_header(&p, doc, &st, engine, chunk, out);
        let dev_full = engine_version(engine).is_some_and(|v| v < DEV_R6_SINCE);
        let expect = p.expect.as_ref().map(|_| new_mem(dev_full));
        Live::boxed(p, st, clock, prev, 0, expect, dev_full)
    }
    /// The individual on the heap from its parts (one place builds a `Live`: `start_to` and `resume_boxed`), written in place:
    /// the persona and the state are copied from their boxes into the new one, so no frame holds a `Live` (`Box::new` of the
    /// value built it on the stack first: a 1.7 KB frame on Xtensa)
    #[inline(never)]
    fn boxed(p: Box<Persona>, st: Box<State>, clock: Clock, prev: String, n: u64, expect: Option<persona::ExpectMem>, dev_full: bool) -> Box<Live> {
        use core::mem::MaybeUninit; use core::ptr::{addr_of_mut, copy_nonoverlapping};
        // every field is written below: this pattern stops compiling when a field is added and not written
        let _all = |l: &Live| { let Live { p: _, st: _, clock: _, last: _, prev: _, n: _, status: _, controls: _, checkpoints: _, rests: _, expect: _, traj: _, dev_full: _, expect_cache: _ } = l; };
        let mut b: Box<MaybeUninit<Live>> = Box::new(MaybeUninit::uninit());
        let l = b.as_mut_ptr();
        // SAFETY: `l` points to an allocation for one `Live`; each field is written once, through raw field pointers (no reference
        // to the uninitialised value); the persona and the state are moved by a bitwise copy out of their boxes, which are then
        // freed as `MaybeUninit` (their contents are not dropped: they live on in the new box)
        unsafe {
            copy_nonoverlapping(&*p as *const Persona, addr_of_mut!((*l).p), 1);
            copy_nonoverlapping(&*st as *const State, addr_of_mut!((*l).st), 1);
            drop(Box::from_raw(Box::into_raw(p) as *mut MaybeUninit<Persona>)); drop(Box::from_raw(Box::into_raw(st) as *mut MaybeUninit<State>));
            addr_of_mut!((*l).clock).write(clock); addr_of_mut!((*l).last).write(0.0); addr_of_mut!((*l).prev).write(prev); addr_of_mut!((*l).n).write(n);
            addr_of_mut!((*l).status).write(Status::Active); addr_of_mut!((*l).controls).write(0); addr_of_mut!((*l).checkpoints).write(0); addr_of_mut!((*l).rests).write(0);
            addr_of_mut!((*l).expect).write(expect); addr_of_mut!((*l).traj).write(None); addr_of_mut!((*l).dev_full).write(dev_full);
            addr_of_mut!((*l).expect_cache).write(persona::ExpectCache::new(true));
            Box::from_raw(Box::into_raw(b) as *mut Live)
        }
    }
    /// One event: stamp its elapsed hours, run the turn, chain the strand line -> (the stance document, the strand line). A bad
    /// event is an error and changes nothing (no turn, no line); so is every event while the individual is paused or retired.
    pub fn event(&mut self, ev: &Json, eng: Engine) -> Result<(Json, String), InErr> {
        use crate::trace::{enter, Phase};
        crate::trace::begin();
        if self.status != Status::Active { return Err(refusal(self.status)); }
        let Json::Obj(kv) = ev else { return Err(perr("event", "must be a JSON object of inputs")) };
        let mut kv: Vec<(String, Json)> = kv.iter().filter(|(k, _)| k != "elapsed_hours").cloned().collect();
        let given = ev.get("elapsed_hours");
        let (elapsed, now) = match &self.clock {
            Clock::Real(t0) => { if given.is_some() { return Err(perr("event.elapsed_hours", "the clock stamps it (--clock fixed reads it from the events)")); }
                let now = t0.elapsed().as_secs_f64() / 3600.0; (q6((now - self.last).max(0.0)), now) }
            Clock::Fixed => { let e = match given { None => 0.0, Some(Json::Num(x)) if x.is_finite() && *x >= 0.0 => *x, _ => return Err(perr("event.elapsed_hours", "a number >= 0")) }; (q6(e), self.last) } };
        kv.push(("elapsed_hours".into(), Json::Num(elapsed)));
        let inputs = Json::Obj(kv);
        enter(Phase::Expect);
        // RFC 0009: the deviation and the trajectory are read from the state before the event (effects {}: the turn is as without
        // them). The trajectory before the turn (nothing of the turn held while it solves), the deviation after it: with the
        // cache on the turn hands it the event's own program (A_t) and its exact answer
        let traj = persona::trajectory(&self.p, &self.st, &inputs);
        let want = self.expect.is_some() && self.expect_cache.reuses();
        let (mut stance, ns, at) = persona::turn_at(&self.p, &self.st, &inputs, eng, want)?;
        let sd = persona::stance_sha(&stance);
        enter(Phase::Expect);
        let cache = &mut self.expect_cache;
        let dev = self.expect.as_ref().and_then(|m| persona::deviation_at(&self.p, &self.st, &inputs, m, eng, cache, at));
        if self.expect.is_some() { if let Json::Obj(v) = &mut stance { v.push(("deviation".into(), dev.as_ref().map_or(Json::Null, |d| d.to_json(self.dev_full)))); } }
        // §2.18: the glyph reads the finished document, `deviation` included (the surprise modifier)
        persona::set_glyph(&self.p, &mut stance);
        // RFC 0002 part 2: a rest line due before this event is its own line, chained before the event line (the turn applied it)
        enter(Phase::Line);
        let (rest, prev) = match persona::rest_line_digest(&self.p, &self.st, &inputs) {
            Some((r, rd)) => { let l = rest_line(&self.prev, r, &rd); let d = persona::digest_of(&l); (Some(l), d) }
            None => (None, self.prev.clone()) };
        let mut lj = vec![("n".into(), Json::Num((self.n + 1) as f64)), ("inputs".into(), inputs.clone()), ("stance".into(), Json::Str(sd)),
            ("state".into(), Json::Str(ns.digest.clone())), ("prev".into(), Json::Str(prev))];
        if let Some(d) = &dev { lj.push(("deviation".into(), d.receipt())); }
        if let Some(g) = stance.get("glyph") { lj.push(("glyph".into(), g.clone())); }
        let line = persona::canon(&Json::Obj(lj));
        enter(Phase::Fold);
        if let Some(m) = self.expect.as_mut() { persona::expect_step(&self.p, m, &inputs, dev.as_ref().and_then(persona::Deviation::logged_pred)); }
        self.traj = traj;
        enter(Phase::Line);
        self.n += 1; self.last = now; self.prev = persona::digest_of(&line); self.st = *ns;
        let line = match &rest { Some(r) => { self.rests += 1; format!("{r}\n{line}") } None => line };
        crate::trace::end();
        Ok((stance, line))
    }
    /// A control line (pause, resume or retire) `by` a person or a sensor (`human[:id]`, `env[:id]`) for `reason`, stamped `at`:
    /// the status moves, the credit is zeroed (no feedback after the line credits a stance before it) and the line is chained ->
    /// the line. Nothing else changes: moods, drives, learned weights and the clock are the individual's as before.
    pub fn control(&mut self, what: &str, by: &str, reason: &str, at: &str) -> Result<String, InErr> {
        let to = self.status.after(what)?;
        let line = control_line(&self.prev, what, by, reason, at)?;
        self.st = self.st.zero_credit(&self.p); if what == "erase" { self.st = self.st.erased(&self.p); } self.status = to; self.controls += 1; self.prev = persona::digest_of(&line);
        Ok(line)
    }
    /// A checkpoint line after event n: the event count, the whole state after it and that event's stance document (from 0.14.0,
    /// with an expectation block, also the expectation's memory: `expect`), chained -> the line. A replay can start there (the
    /// state) and a board can be drawn there (the stance) without the events before.
    pub fn checkpoint(&mut self, stance: &Json) -> String {
        let mut kv = vec![("checkpoint".into(), Json::Num(self.n as f64)), ("prev".into(), Json::Str(self.prev.clone())), ("stance".into(), stance.clone()),
            ("state".into(), self.st.to_json(&self.p))];
        // from 0.14.0 the expectation's memory (the canonical fold) rides on the line, so a replay and a resume can start here
        if let Some(m) = self.expect.as_ref().filter(|m| m.is_canonical()) { kv.push(("expect".into(), m.to_json())); }
        let line = persona::canon(&Json::Obj(kv));
        self.checkpoints += 1; self.prev = persona::digest_of(&line); line
    }
    /// The sha256 of the strand's last line (the header's before any event)
    pub fn head(&self) -> &str { &self.prev }
}
/// The header piece size `start_boxed` collects with (any size gives the same bytes)
pub const HEADER_CHUNK: usize = 1024;
/// A sink that cuts the text into pieces of `cap` bytes for `out` and hashes it on the way
struct Pieces<'a> { buf: Vec<u8>, cap: usize, out: &'a mut dyn FnMut(&[u8]), h: persona::Sha256 }
impl Pieces<'_> {
    fn put(&mut self, mut b: &[u8]) {
        self.h.update(b);
        while !b.is_empty() { let k = (self.cap - self.buf.len()).min(b.len()); self.buf.extend_from_slice(&b[..k]); b = &b[k..];
            if self.buf.len() == self.cap { (self.out)(&self.buf); self.buf.clear(); } }
    }
}
impl persona::Sink for Pieces<'_> {
    fn push(&mut self, c: char) { let mut t = [0u8; 4]; self.put(c.encode_utf8(&mut t).as_bytes()) }
    fn push_str(&mut self, t: &str) { self.put(t.as_bytes()) }
}
/// The strand's header line for `p`, `doc`, `st` and `engine` (`Live::start`'s, without its `\n`), handed to `out` in pieces of
/// at most `chunk` bytes (a 0 is taken as 1) -> its sha256 (`sha256:` + hex), the first line's `prev`. Neither the line nor a
/// copy of the document is held whole: the buffer is one piece.
pub fn write_header(p: &Persona, doc: &Json, st: &State, engine: &str, chunk: usize, out: &mut dyn FnMut(&[u8])) -> String {
    write_header_seg(p, doc, st, engine, None, chunk, out)
}
/// `write_header` of a segment (docs/persona.md §5.7, "Segments"): the header with one more key at its end, `segment` = {index,
/// previous, sealed} (canonical JSON); None writes `write_header`'s bytes
pub fn write_header_seg(p: &Persona, doc: &Json, st: &State, engine: &str, segment: Option<&Json>, chunk: usize, out: &mut dyn FnMut(&[u8])) -> String {
    use persona::Sink;
    let cap = chunk.max(1);
    let mut w = Pieces { buf: Vec::with_capacity(cap), cap, out, h: persona::Sha256::new() };
    let s = |x: &str| Json::Str(x.to_string());
    // the keys in `Live::start`'s order; values through the canonical writer (the state with its keys sorted, the rest as given)
    w.push_str("{\"probbit_strand\":"); persona::write_to(&Json::Num(FORMAT), false, &mut w);
    w.push_str(",\"engine\":"); persona::write_to(&s(engine), false, &mut w);
    w.push_str(",\"persona\":"); persona::write_to(&Json::Obj(vec![("name".into(), s(&p.name)), ("version".into(), s(&p.version)), ("digest".into(), s(&p.digest))]), false, &mut w);
    w.push_str(",\"seed\":"); persona::write_to(&Json::Num(st.seed as f64), false, &mut w);
    w.push_str(",\"state\":"); persona::write_to(&st.to_json(p), true, &mut w);
    w.push_str(",\"document\":"); persona::write_to(doc, false, &mut w);
    if let Some(sg) = segment { w.push_str(",\"segment\":"); persona::write_to(sg, true, &mut w); }
    w.push('}');
    if !w.buf.is_empty() { (w.out)(&w.buf); }
    format!("sha256:{}", persona::hex(&w.h.finish()))
}
/// A control line after the line whose sha256 is `prev` (canonical JSON: at, by, control, prev, reason). `by` names who: a
/// person or a sensor, never the individual or the clock; the reason is 1-500 characters.
pub fn control_line(prev: &str, what: &str, by: &str, reason: &str, at: &str) -> Result<String, InErr> {
    if !["pause", "resume", "retire", "erase"].contains(&what) { return Err(perr("control", "pause | resume | retire")); }
    if !matches!(persona::src_kind(by), Some("human" | "env")) { return Err(perr("control.by", "who: human[:id] or env[:id] (id: 1-64 of A-Z a-z 0-9 _ . - @ / :); the individual cannot control itself")); }
    if reason.trim().is_empty() || reason.chars().count() > 500 { return Err(perr("control.reason", "1-500 characters")); }
    if at.is_empty() || at.len() > 64 { return Err(perr("control.at", "a time stamp, 1-64 characters (RFC 3339 by default)")); }
    let s = |x: &str| Json::Str(x.to_string());
    Ok(persona::canon(&Json::Obj(vec![("at".into(), s(at)), ("by".into(), s(by)), ("control".into(), s(what)), ("prev".into(), s(prev)), ("reason".into(), s(reason))])))
}
/// A rest line (RFC 0002 part 2) after the line whose sha256 is `prev` (canonical JSON: prev, rest, state): what the line did
/// (downscale, prune, the pressure, the learned weights after it, each gate as kept) and the digest of the state after it. It is
/// written only together with the event it precedes, and that event's line chains to it.
fn rest_line(prev: &str, rest: Json, state: &str) -> String {
    persona::canon(&Json::Obj(vec![("prev".into(), Json::Str(prev.into())), ("rest".into(), rest), ("state".into(), Json::Str(state.into()))]))
}
/// What kind of strand line `j` is
pub enum Kind { Event, Control, Checkpoint, Rest }
pub fn kind(j: &Json) -> Kind { if j.get("control").is_some() { Kind::Control } else if j.get("checkpoint").is_some() { Kind::Checkpoint } else if j.get("rest").is_some() { Kind::Rest } else { Kind::Event } }
/// The header's keys, as this engine writes them (a segment's header has one more at its end: `segment`)
const HEADER: [&str; 6] = ["probbit_strand", "engine", "persona", "seed", "state", "document"];
/// The divergence of a segment whose second line is not the line its header links
pub const SEALED: &str = "the line after a segment's header is not its sealed line (its sha256 is not the header's `sealed`)";
/// A header's `segment`: exactly {index: a whole number >= 1, previous: a file name (1-255 characters, no `/` or `\\`), sealed: a
/// sha256}
fn check_segment(sg: &Json) -> Result<(), String> {
    let kv = sg.as_obj().filter(|kv| kv.len() == 3).ok_or("an object {index, previous, sealed}")?;
    let f = |k: &str| kv.iter().find(|(x, _)| x == k).map(|(_, v)| v);
    f("index").and_then(Json::as_f64).filter(|x| *x >= 1.0 && x.fract() == 0.0 && *x <= 9_007_199_254_740_992.0).ok_or("index: a whole number >= 1")?;
    f("previous").and_then(Json::as_str).filter(|s| !s.is_empty() && s.len() <= 255 && !s.contains(['/', '\\'])).ok_or("previous: the previous segment's file name")?;
    f("sealed").and_then(Json::as_str).filter(|s| s.len() == 71 && s.starts_with("sha256:") && s[7..].bytes().all(|c| c.is_ascii_hexdigit())).ok_or("sealed: a sha256")?;
    Ok(())
}
/// The file name of segment `k` of the strand `base` (§5.7): `NAME.strand` for 0, `NAME.000k.strand` after it (four digits at least);
/// a base without `.strand` gets `.000k` appended
pub fn segment_name(base: &str, k: u64) -> String {
    if k == 0 { return base.to_string(); }
    match base.strip_suffix(".strand") { Some(stem) => format!("{stem}.{k:04}.strand"), None => format!("{base}.{k:04}") }
}
/// The header of segment `index` that continues `lv` after its sealed line (whose sha256 is `sealed`; `previous` is the file name
/// of the segment it seals), for the persona document `doc` and the strand's `engine` -> the header line. The new file is this
/// header, then the sealed line itself (byte for byte), then the events: the chain goes on across the files unchanged.
pub fn segment_header(lv: &Live, doc: &Json, engine: &str, index: u64, previous: &str, sealed: &str) -> String {
    let sg = Json::Obj(vec![("index".into(), Json::Num(index as f64)), ("previous".into(), Json::Str(previous.into())), ("sealed".into(), Json::Str(sealed.into()))]);
    let mut b = Vec::new(); write_header_seg(&lv.p, doc, &lv.st, engine, Some(&sg), HEADER_CHUNK, &mut |c: &[u8]| b.extend_from_slice(c));
    String::from_utf8(b).unwrap_or_default()
}
/// The earliest key of a line after the header that its kind does not have (the line's keys in canonical order): a line kind
/// this engine does not know (a line without `control` or `checkpoint` is read as an event line, so `{"prev": ..., "nap": ...}`
/// gives `nap`) or a field it does not know on a known one
fn unknown_key(j: &Json) -> Option<&str> {
    let known: &[&str] = match kind(j) { Kind::Event => &["deviation", "glyph", "inputs", "n", "prev", "stance", "state"], Kind::Control => &["at", "by", "control", "prev", "reason"],
        Kind::Checkpoint => &["checkpoint", "expect", "prev", "stance", "state"], Kind::Rest => &["prev", "rest", "state"] };
    j.as_obj()?.iter().map(|(k, _)| k.as_str()).find(|k| !known.contains(k))
}
/// The start of a replay's `why` that is not a divergence: the line has a field or a line kind this engine does not know (a
/// strand from a newer engine). Nothing from that line on was checked: the strand is not verified, and not shown to differ.
/// `verify` reports it as `unsupported` (exit 5), never as `diverges`; a changed value of a field this engine knows still
/// diverges.
pub const UNSUPPORTED: &str = "unsupported by this engine: ";
/// The `why` of an unsupported field: UNSUPPORTED, then its path (a persona field as the persona validator names it, a state
/// field under `state.`, otherwise its path in the strand line)
fn unsupported(path: &str) -> String { format!("{UNSUPPORTED}{path}") }
/// The field path of an unsupported `why`; None for a divergence
pub fn unsupported_field(why: &str) -> Option<&str> { why.strip_prefix(UNSUPPORTED) }
/// A persona, state or event error for a field this engine does not know (the validators' "unknown field" at its path) -> the
/// unsupported `why`; any other error -> `what` + the error, a divergence
fn read_err(e: InErr, what: &str) -> String { if e.msg.starts_with("unknown field") { unsupported(&e.path) } else { format!("{what}{}: {}", e.path, e.msg) } }
/// Now as RFC 3339 UTC to the second (the control lines' default `at`); `std` only (a firmware passes its own `at`)
#[cfg(feature = "std")]
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (z, t) = ((secs / 86_400) as i64 + 719_468, secs % 86_400); // days since 0000-03-01 (civil-from-days)
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097)); let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153; let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t % 3600 / 60, t % 60)
}

/// A strand replayed line by line: `verify` and `probbit monitor` (§5.8) share it, so both read a strand with one set of
/// rules. `open` checks the header and rebuilds the individual; each `step` replays one event line on the fixed clock and
/// checks it (`prev`, `n`, the stance and state digests, the bytes). After an error the replay is spent: the chain past the
/// line that differs cannot be checked.
pub struct Replay { pub live: Box<Live>, pub header: Json, pub doc: Json, pub line: usize, pub from: Option<u64>, last: Option<Json>, checkpoint_ready: bool, norms: Normals, at_cap: Vec<f64>, seen: (f64, Vec<f64>), viab: Option<(f64, Option<u64>)>, rest: Option<String> }
/// A senses block's normals over the replay (§2.11): per sense the means after the last 50 events, the largest move of a mean
/// within 50 events so far, and the latest event's move (the monitor's alert: over drift_cap / 2)
#[derive(Default)]
pub struct Normals { window: Vec<std::collections::VecDeque<f64>>, pub max_move: Vec<f64>, pub moved: Vec<f64> }
impl Normals {
    fn start(p: &Persona, st: &State) -> Normals {
        let n = persona::normals(p, st);
        Normals { window: n.iter().map(|x| std::collections::VecDeque::from(vec![x.1])).collect(), max_move: vec![0.0; n.len()], moved: vec![0.0; n.len()] }
    }
    fn track(&mut self, p: &Persona, st: &State) {
        for (k, (_, m, _, _)) in persona::normals(p, st).iter().enumerate() { let q = &mut self.window[k];
            let mv = persona::r6(q.iter().fold(0.0f64, |a, x| a.max((m - x).abs())));
            self.moved[k] = mv; self.max_move[k] = self.max_move[k].max(mv); q.push_back(*m); if q.len() > 50 { q.pop_front(); } }
    }
}
impl Replay {
    /// The header line (without its line ending) -> the replay, ready for event lines; Err((1, what differs)), or Err((1, the
    /// unsupported `why`)) for a header key, persona field or state field this engine does not know
    pub fn open(head: &str) -> Result<Replay, (usize, String)> { Replay::open_boxed(head).map(|r| *r) }
    /// `open` on the heap (docs/embedding.md "Stack budget"): the header's checks of `open`, in its order with its messages, run as
    /// the chain check runs them (the persona built on the heap, the state read into a box, the header written again piece by
    /// piece against the line, never whole), and the individual is built in its box: no frame holds a `Live`, a `Persona` or the
    /// header text. `open` is this call with the replay moved out of its box (the replay itself is small: the individual stays
    /// boxed in it).
    #[inline(never)]
    pub fn open_boxed(head: &str) -> Result<Box<Replay>, (usize, String)> {
        let (p, h, st0, prev) = open_head(head)?;
        let doc = h.get("document").cloned().unwrap_or(Json::Null);
        let dev_full = engine_version(h.get("engine").and_then(Json::as_str).unwrap_or("")).is_some_and(|v| v < DEV_R6_SINCE);
        let expect = p.expect.as_ref().map(|_| new_mem(dev_full));
        let live = Live::boxed(p, st0, Clock::Fixed, prev, 0, expect, dev_full);
        Ok(Box::new(Replay::at_header(live, h, doc)))
    }
    /// The replay of a life just started from its header
    #[inline(never)]
    fn at_header(live: Box<Live>, header: Json, doc: Json) -> Replay {
        let norms = Normals::start(&live.p, &live.st); let at_cap = vec![0.0; persona::deficits(&live.p, &live.st).len()]; let seen = (0.0, vec![0.0; persona::gate_gains(&live.p, &live.st).len()]);
        let viab = viab_start(&live);
        Replay { live, header, doc, line: 1, from: None, last: None, checkpoint_ready: false, norms, at_cap, seen, viab, rest: None }
    }
    /// The replay from a checkpoint line (`cp`, without its line ending, at 1-based line `at`; `before` = the event line before it)
    /// instead of the header: the header is read as by `open`; the checkpoint must follow `before` (prev), at its event count, with
    /// its stance (the digest `before` logs) and its state (read as any state is, with the digest `before` logs); the chain goes on
    /// from the checkpoint line. The lines before it are taken as written: a full `verify` replays them and checks the checkpoint.
    pub fn open_at(head: &str, before: &str, cp: &str, at: usize) -> Result<Replay, (usize, String)> {
        let mut r = Replay::open(head)?;
        // RFC 0009: the expectation's memory is a fold of every event line, so a strand with the block replays from its header
        // RFC 0009: before 0.14.0 the expectation's memory is a fold of every event line and no checkpoint carries it, so such a
        // strand replays from its header; from 0.14.0 the checkpoint carries it
        let canon = !r.live.dev_full;
        if r.live.p.expect.is_some() && !canon { return Err((at, "the persona's expectation block reads every event before this checkpoint: verify this strand from its header".into())); }
        let (n, stance, st, mem) = check_checkpoint(&r.live.p, canon, Some(before), cp, at)?;
        r.start_at(n, stance, st, mem, cp, at); r.live.checkpoints = 1;
        Ok(r)
    }
    /// The replay restarted at the checkpoint line `cp` (1-based line `at`): its count, stance, state and memory
    fn start_at(&mut self, n: u64, stance: Json, st: Box<State>, mem: Option<persona::ExpectMem>, cp: &str, at: usize) {
        let r = self;
        r.live.st = *st; r.live.n = n; if mem.is_some() { r.live.expect = mem; } r.live.prev = persona::digest_of(cp); r.line = at; r.from = Some(n); r.last = Some(stance); r.checkpoint_ready = false;
        r.norms = Normals::start(&r.live.p, &r.live.st); r.at_cap = vec![0.0; r.at_cap.len()]; r.seen = (0.0, vec![0.0; r.seen.1.len()]); r.viab = viab_start(&r.live);
    }
    /// The replay of a strand's `lines` from their start -> (the replay, the index of the first line still to step): the header, or
    /// for a segment (a header with `segment`) the header and its sealed line (line 2), taken on the header's link: its sha256 is
    /// the header's `sealed`, it is a valid checkpoint line and its state is the header's; the replay goes on from it
    pub fn open_lines(lines: &[&str]) -> Result<(Replay, usize), (usize, String)> {
        let mut r = Replay::open(lines.first().copied().unwrap_or(""))?;
        let Some(sg) = r.header.get("segment").cloned() else { return Ok((r, 1)) };
        let canon = !r.live.dev_full;
        if r.live.p.expect.is_some() && !canon { return Err((1, "a segment of a persona with an expectation block needs a strand of engine 0.14.0 or later".into())); }
        let cp = lines.get(1).copied().unwrap_or("");
        if Some(persona::digest_of(cp).as_str()) != sg.get("sealed").and_then(Json::as_str) { return Err((2, SEALED.into())); }
        let (n, stance, st, mem) = check_checkpoint(&r.live.p, canon, None, cp, 2)?;
        if st.digest != r.live.st.digest { return Err((2, "the sealed line's state is not the segment header's".into())); }
        r.start_at(n, stance, st, mem, cp, 2);
        Ok((r, 2))
    }
    /// The stance document of the last event replayed (or of the checkpoint the replay started from)
    pub fn last_stance(&self) -> Option<&Json> { self.last.as_ref() }
    /// The senses' normals over the replay so far (§2.11)
    pub fn normals(&self) -> &Normals { &self.norms }
    /// One line (without its line ending) -> the stance document an event line replays to (None for a control or checkpoint
    /// line); Err((its 1-based line number, what differs)). A control line must be a valid move of the status and the line its
    /// fields give; a checkpoint line must immediately follow an event while active and carry
    /// the event count and the state the replay reached. The last stance remains available to
    /// the monitor after controls/checkpoints, but that does not authorize another checkpoint. A chained line with a key its
    /// kind does not have (a newer line kind, a newer field), or an event whose inputs have a field this engine does not know,
    /// is Err((its line number, the unsupported `why`)).
    pub fn step(&mut self, l: &str, eng: Engine) -> Result<Option<Json>, (usize, String)> {
        let (i, live) = (self.line + 1, &mut self.live);
        let j = json::parse(l).map_err(|e| (i, format!("not JSON: {}", e.msg)))?;
        if persona::canon(&j) != l { return Err((i, "not canonical JSON".into())); }
        let head = self.rest.as_deref().map_or(live.head().to_string(), persona::digest_of);
        if j.get("prev").and_then(Json::as_str) != Some(head.as_str()) { return Err((i, PREV.into())); }
        if let Some(k) = unknown_key(&j) { return Err((i, unsupported(k))); }
        if self.rest.is_some() && !matches!(kind(&j), Kind::Event) { return Err((i, "a rest line must be followed by its event line".into())); }
        match kind(&j) {
            // RFC 0002 part 2: a rest line is checked with the event after it (the replay recomputes it there)
            Kind::Rest => { if live.status != Status::Active { return Err((i, format!("rest line while {}", live.status.name()))); }
                self.rest = Some(l.to_string()); self.line = i; self.checkpoint_ready = false; return Ok(None) }
            Kind::Control => { let f = |k: &str| j.get(k).and_then(Json::as_str).unwrap_or("");
                let line = live.control(f("control"), f("by"), f("reason"), f("at")).map_err(|e| (i, format!("control line: {}: {}", e.path, e.msg)))?;
                if line != l { return Err((i, "the control line differs".into())); }
                self.line = i; self.checkpoint_ready = false; return Ok(None) }
            Kind::Checkpoint => {
                if live.status != Status::Active { return Err((i, format!("checkpoint while {}", live.status.name()))); }
                if !self.checkpoint_ready { return Err((i, "a checkpoint must immediately follow an event line".into())); }
                if j.get("checkpoint").and_then(Json::as_f64) != Some(live.n as f64) { return Err((i, format!("the checkpoint is not at event {}", live.n))); }
                let Some(stance) = self.last.as_ref() else { return Err((i, "a checkpoint follows an event line".into())) };
                let line = live.checkpoint(stance);
                if line != l {
                    // before 0.14.0 the deviation is at full precision: equal at 6 decimals, the chain goes on from the logged bytes
                    if !(live.dev_full && dev_r6(&line) == dev_r6(l)) { return Err((i, "the checkpoint's state or stance differs from the replay".into())); }
                    live.prev = persona::digest_of(l);
                }
                self.line = i; self.checkpoint_ready = false; return Ok(None) }
            Kind::Event => {} }
        if j.get("n").and_then(Json::as_f64) != Some((live.n + 1) as f64) { return Err((i, format!("n is not {}", live.n + 1))); }
        let ev = j.get("inputs").ok_or((i, "no inputs".to_string()))?;
        // a needs block (RFC 0002): the stamped hours of events that found a deficit at its cap (it sat there since the event before)
        let before = persona::deficits(&live.p, &live.st); let h = ev.get("elapsed_hours").and_then(Json::as_f64).unwrap_or(0.0);
        let logged = self.rest.take();
        let (stance, both) = live.event(ev, eng).map_err(|e| (i, read_err(e, "")))?;
        let line = match (both.split_once('\n'), logged) {
            (Some((r, e)), Some(lr)) => { if r != lr { return Err((i - 1, "the rest line differs".into())); } e.to_string() }
            (Some(_), None) => return Err((i, "a rest line is due before this event and missing".into())),
            (None, Some(_)) => return Err((i - 1, "a rest line where none is due".into())),
            (None, None) => both };
        for (k, (_, x, cap)) in before.iter().enumerate() { if *x >= cap - 5e-7 { self.at_cap[k] = persona::r6(self.at_cap[k] + h); } }
        if j.get("stance").and_then(Json::as_str) != Some(persona::stance_sha(&stance).as_str()) { return Err((i, "the stance differs".into())); }
        // RFC 0009: the logged deviation is recomputed (an edited receipt diverges here)
        if json::parse(&line).ok().and_then(|x| x.get("deviation").cloned()) != j.get("deviation").cloned() { return Err((i, "the deviation differs".into())); }
        // §2.18: the logged glyph is recomputed from the replayed stance (an edited glyph diverges here)
        if stance.get("glyph") != j.get("glyph") { return Err((i, "the glyph differs".into())); }
        if j.get("state").and_then(Json::as_str) != Some(live.st.digest.as_str()) { return Err((i, "the state differs".into())); }
        if line != l { return Err((i, "the line differs".into())); }
        self.line = i; self.last = Some(stance.clone()); self.checkpoint_ready = true; self.norms.track(&self.live.p, &self.live.st);
        // RFC 0002: the largest R̄ (Design C) and per learned trait the largest gain (Design B) after any event replayed
        if let Some((r, _, _)) = persona::pace_rbar(&self.live.p, &self.live.st) { self.seen.0 = self.seen.0.max(r); }
        for (k, (_, g, _)) in persona::gate_gains(&self.live.p, &self.live.st).iter().enumerate() { if let Some(m) = self.seen.1.get_mut(k) { *m = m.max(*g); } }
        // RFC 0008 E: the smallest margin_min after any event replayed, and the event at which viability was lost
        if let (Some((seen, lost_at)), Some((_, m, lost))) = (self.viab.as_mut(), persona::viability(&self.live.p, &self.live.st)) {
            *seen = seen.min(m); if lost && lost_at.is_none() { *lost_at = Some(self.live.n); } }
        Ok(Some(stance))
    }
    /// The end of the strand: a rest line (RFC 0002 part 2) is only ever written with the event after it, so a strand that ends
    /// on one diverges at it
    pub fn end(&self) -> Result<(), (usize, String)> {
        if self.rest.is_some() { return Err((self.line, "a rest line must be followed by its event line".into())); } Ok(())
    }
    /// `verify`'s summary of the lines replayed so far: `engine` is the strand header's (the engine that wrote it), `verified_by`
    /// the engine that replayed it
    pub fn summary(&self) -> Json {
        let (s, live) = (|x: &str| Json::Str(x.to_string()), &self.live);
        let mut v = vec![("ok".into(), Json::Bool(true)), ("events".into(), Json::Num(live.n as f64)), ("persona".into(), s(&live.p.name)), ("seed".into(), Json::Num(live.st.seed as f64)),
            ("engine".into(), self.header.get("engine").cloned().unwrap_or(Json::Null)), ("verified_by".into(), Json::Str(engine())), ("final_state".into(), s(&live.st.digest)),
            ("last_line".into(), s(live.head()))];
        // a strand with control or checkpoint lines says so (a strand without them gives the summary of before)
        if live.controls > 0 { v.push(("controls".into(), Json::Num(live.controls as f64))); v.push(("status".into(), s(live.status.name()))); }
        if live.checkpoints > 0 { v.push(("checkpoints".into(), Json::Num(live.checkpoints as f64))); }
        // RFC 0002 part 2: rest lines replayed, the pressure now; the load now
        if live.rests > 0 { v.push(("rest_lines".into(), Json::Num(live.rests as f64))); }
        if let Some((sp, pend)) = persona::rest_pressure(&live.st) { v.push(("pressure".into(), Json::Obj(vec![("s".into(), Json::Num(sp)), ("pending".into(), Json::Bool(pend))]))); }
        if let Some((l, _)) = persona::load_level(&live.p, &live.st) { v.push(("load".into(), Json::Obj(vec![("l".into(), Json::Num(l))]))); }
        if let Some(n) = self.from { v.push(("from_checkpoint".into(), Json::Num(n as f64))); }
        // a segment (§5.7): its index and the digest of the sealed line its replay started from, taken on the link
        if let Some(sg) = self.header.get("segment") { v.push(("segment".into(), sg.get("index").cloned().unwrap_or(Json::Null))); v.push(("trusted_link".into(), sg.get("sealed").cloned().unwrap_or(Json::Null))); }
        // a senses block (§2.11): per sense the normal now and the largest move of its mean within 50 events
        let nm = persona::normals(&live.p, &live.st);
        if !nm.is_empty() { v.push(("normals".into(), Json::Obj(nm.iter().enumerate().map(|(k, (id, m, n, _))| (id.clone(), Json::Obj(vec![("mean".into(), Json::Num(*m)),
            ("n".into(), Json::Num(*n as f64)), ("max_move_50".into(), Json::Num(self.norms.max_move.get(k).copied().unwrap_or(0.0)))]))).collect()))); }
        // a needs block (RFC 0002): per need its deficit now and the stamped hours it sat at its cap
        let dx = persona::deficits(&live.p, &live.st);
        if !dx.is_empty() { v.push(("needs".into(), Json::Obj(dx.iter().enumerate().map(|(k, (id, x, _))| (id.clone(), Json::Obj(vec![("deficit".into(), Json::Num(*x)),
            ("at_cap_hours".into(), Json::Num(self.at_cap.get(k).copied().unwrap_or(0.0)))]))).collect()))); }
        // RFC 0002 Design C: R̄ now and the largest after any event replayed; Design B: per learned trait the largest gain seen
        if let Some((r, _, _)) = persona::pace_rbar(&live.p, &live.st) { v.push(("pace".into(), Json::Obj(vec![("rbar".into(), Json::Num(r)), ("max".into(), Json::Num(self.seen.0))]))); }
        let gg = persona::gate_gains(&live.p, &live.st);
        if !gg.is_empty() { v.push(("surprise".into(), Json::Obj(gg.iter().enumerate().map(|(k, (t, _, _))| (t.clone(),
            Json::Obj(vec![("g_max_seen".into(), Json::Num(self.seen.1.get(k).copied().unwrap_or(0.0)))]))).collect()))); }
        // RFC 0008 E: with a bound, recomputed from the logged readings and amounts
        if let Some((seen, lost_at)) = self.viab { v.push(("viability".into(), Json::Obj(vec![("margin_min_seen".into(), Json::Num(seen)), ("lost_at".into(), lost_at.map_or(Json::Null, |n| Json::Num(n as f64)))]))); }
        // a credit block (§2.8): the decisions still awaiting an outcome
        if let Some(o) = persona::open_decisions(&live.p, &live.st) { v.push(("open".into(), Json::Arr(o.iter().map(|(t, _)| Json::Num(*t as f64)).collect()))); }
        Json::Obj(v)
    }
}
/// RFC 0008 E: the viability record a replay starts from: the margin_min of its first state, and lost_at (the event count when the
/// state is already lost, as after a checkpoint); none without a bound
fn viab_start(live: &Live) -> Option<(f64, Option<u64>)> { persona::viability(&live.p, &live.st).map(|(_, m, lost)| (m, lost.then_some(live.n))) }
/// Per event line its n and its block
pub type Blocks = Vec<(u64, String)>;
/// `live verify --blocks`: replay from the header -> (the summary, per event line its n and the block its stance renders, RFC 0008 E)
pub fn verify_blocks(text: &str, eng: Engine) -> Result<(Json, Blocks), (usize, String)> {
    let lines = lines(text);
    let replay = || -> Result<(Json, Blocks), (usize, String)> {
        let (mut r, k) = Replay::open_lines(&lines)?; let mut out = vec![];
        for l in lines.iter().skip(k) { if let Some(doc) = r.step(l, eng)? { out.push((r.live.n, persona::block(&r.live.p, &doc))); } }
        Ok((r.summary(), out)) };
    replay().map_err(|e| chain_first(&lines, e))
}
/// A strand's text -> its lines without their line endings (a final newline ends the last line; it does not start another).
/// One leading UTF-8 byte-order mark is skipped, as `\r` before `\n` is: a Windows editor adds both on save, and neither is
/// part of the line text the chain hashes.
pub fn lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.strip_prefix('\u{feff}').unwrap_or(text).split('\n').map(bare).collect();
    if lines.last() == Some(&"") { lines.pop(); }
    lines
}
/// `probbit live verify`: replay a strand from its header -> Ok(summary) when every line is as recorded; Err((1-based line
/// number, what differs)) at the earliest line that is not. Lines end in `\n` or `\r\n`; the chain hashes the line text alone.
/// A line with a field or line kind this engine does not know stops the replay as unsupported, unless a later line breaks the
/// chain (`chain_first`).
pub fn verify(text: &str, eng: Engine) -> Result<Json, (usize, String)> {
    let lines = lines(text);
    let replay = || -> Result<Json, (usize, String)> {
        let (mut r, k) = Replay::open_lines(&lines)?;
        for l in lines.iter().skip(k) { r.step(l, eng)?; }
        r.end()?; Ok(r.summary()) };
    replay().map_err(|e| chain_first(&lines, e))
}
/// The divergence the hash chain shows after an unsupported line: a strand this engine cannot replay to the end is still checked
/// for what the format alone says, so an edited, removed or reordered line after it (or the unsupported line itself, edited:
/// the next line's `prev`) diverges at the earliest line that breaks the chain (not JSON, not canonical JSON, or a `prev` that is
/// not the sha256 of the line before), as in a replay. `unsupported` is left only for a strand whose chain is whole; a
/// divergence passes unchanged.
fn chain_first(lines: &[&str], (line, why): (usize, String)) -> (usize, String) {
    if unsupported_field(&why).is_none() { return (line, why); }
    for i in line.max(1)..lines.len() {
        let j = match json::parse(lines[i]) { Ok(j) => j, Err(e) => return (i + 1, format!("not JSON: {}", e.msg)) };
        if persona::canon(&j) != lines[i] { return (i + 1, "not canonical JSON".into()); }
        if j.get("prev").and_then(Json::as_str) != Some(persona::digest_of(lines[i - 1]).as_str()) { return (i + 1, PREV.into()); }
    }
    (line, why)
}
/// The divergence of a line whose `prev` is not the sha256 of the line before
const PREV: &str = "prev is not the sha256 of the line before (a line before it was changed, removed or reordered)";
/// The 0-based index of the strand's last checkpoint line followed by at least `after` lines, if any (a cheap scan: the key)
pub fn last_checkpoint(lines: &[&str], after: usize) -> Option<usize> {
    (1..lines.len().saturating_sub(after)).rev().find(|&i| lines[i].starts_with(r#"{"checkpoint":"#))
}
/// `verify --from-checkpoint`: replay from the last checkpoint line (the header read as always), not from the header
pub fn verify_from_checkpoint(text: &str, eng: Engine) -> Result<Json, (usize, String)> {
    let lines = lines(text);
    // a segment's sealed line (line 2) is where its own replay starts anyway
    let Some(k) = last_checkpoint(&lines, 0).filter(|&k| k >= 2) else { return verify(text, eng) };
    let replay = || -> Result<Json, (usize, String)> {
        let mut r = Replay::open_at(lines[0], lines[k - 1], lines[k], k + 1)?;
        for l in lines.iter().skip(k + 1) { r.step(l, eng)?; }
        r.end()?; Ok(r.summary()) };
    replay().map_err(|e| chain_first(&lines, e))
}
/// `verify` from the header, keeping the stance documents of the given turns (what `--actions` compares with)
pub fn verify_keep(text: &str, eng: Engine, keep: &[u64]) -> Result<(Json, Vec<(u64, Json)>, Persona), (usize, String)> {
    let lines = lines(text);
    let replay = || -> Result<(Json, Vec<(u64, Json)>, Persona), (usize, String)> {
        let (mut r, k) = Replay::open_lines(&lines)?; let mut kept = vec![];
        for l in lines.iter().skip(k) { if let Some(doc) = r.step(l, eng)? {
            if let Some(t) = doc.get("turn").and_then(Json::as_f64).map(|x| x as u64).filter(|t| keep.contains(t)) { kept.push((t, doc)); } } }
        r.end()?; Ok((r.summary(), kept, r.live.p.clone())) };
    replay().map_err(|e| chain_first(&lines, e))
}
/// One line of a host's action log (`live verify --actions FILE`, §5.7): the decision's turn and, per trait the host acted on,
/// the level it acted on
pub struct Action { pub line: usize, pub turn: u64, pub levels: Vec<(String, String)> }
/// A host's action log: one JSON object per line, {"turn": T, "action": {trait: level, ...}} (blank lines skipped)
pub fn read_actions(text: &str) -> Result<Vec<Action>, InErr> {
    let mut out = vec![];
    for (i, l) in text.split('\n').map(bare).enumerate() { if l.trim().is_empty() { continue; } let path = format!("actions.line{}", i + 1);
        let bad = |m: &str| perr(&path, m.to_string());
        let j = json::parse(l).map_err(|e| perr(&path, format!("not JSON: {}", e.msg)))?;
        let kv = j.as_obj().filter(|kv| kv.len() == 2 && kv.iter().all(|(k, _)| k == "turn" || k == "action")).ok_or_else(|| bad("an object {\"turn\": T, \"action\": {trait: level}}"))?;
        let turn = kv.iter().find(|(k, _)| k == "turn").and_then(|(_, x)| x.as_f64()).filter(|x| *x >= 0.0 && x.fract() == 0.0 && *x <= 9_007_199_254_740_992.0)
            .ok_or_else(|| bad("turn: a whole number >= 0 (the stance's turn)"))? as u64;
        let act = kv.iter().find(|(k, _)| k == "action").and_then(|(_, x)| x.as_obj()).filter(|a| !a.is_empty()).ok_or_else(|| bad("action: an object trait -> level"))?;
        let levels = act.iter().map(|(t, x)| x.as_str().map(|l| (t.clone(), l.to_string())).ok_or_else(|| bad("action: each level a string"))).collect::<Result<Vec<_>, InErr>>()?;
        out.push(Action { line: i + 1, turn, levels }); }
    Ok(out)
}
/// The earliest turn (then the earliest line) whose host action differs from the level its stance released -> (actions
/// checked, the difference: {turn, line, trait, action, released (null: not released or no such turn), why}); an action on a
/// trait the persona does not have is an input error
pub fn first_difference(p: &Persona, actions: &[Action], stances: &[(u64, Json)]) -> Result<(usize, Option<Json>), InErr> {
    let mut order: Vec<&Action> = actions.iter().collect(); order.sort_by_key(|a| (a.turn, a.line));
    let s = |x: &str| Json::Str(x.to_string()); let mut checked = 0;
    for a in order { checked += 1; let doc = stances.iter().find(|(t, _)| *t == a.turn).map(|x| &x.1);
        for (t, level) in &a.levels {
            let e = match doc { Some(d) => persona::doc_entry(p, d, t), None => None };
            if doc.is_some() && e.is_none() { return Err(perr(&format!("actions.line{}.action.{t}", a.line), "not a trait of the persona")); }
            let released = e.as_ref().filter(|e| e.get("released") == Some(&Json::Bool(true))).and_then(|e| e.get("level")).and_then(Json::as_str).map(str::to_string);
            let why = match (doc, &released) { (None, _) => "the strand has no stance with this turn", (_, None) => "the stance did not release this trait",
                (_, Some(r)) if r != level => "the host acted on another level", _ => continue };
            return Ok((checked, Some(Json::Obj(vec![("turn".into(), Json::Num(a.turn as f64)), ("line".into(), Json::Num(a.line as f64)), ("trait".into(), s(t)), ("action".into(), s(level)),
                ("released".into(), released.map_or(Json::Null, |r| s(&r))), ("why".into(), s(why))])))); } }
    Ok((checked, None))
}
/// `verify` as one document: the summary, or `failure`'s document
pub fn verify_doc(text: &str, eng: Engine) -> Json { verify(text, eng).unwrap_or_else(|(line, why)| failure(text, line, &why)) }
/// The document of a strand that does not verify at `line`: {ok: false, line, diverges: what differs}, or, for a field or line
/// kind this engine does not know, {ok: false, line, unsupported: the field's path, engine: this engine, strand_engine: the
/// header's `engine` (null if it names none)}
pub fn failure(text: &str, line: usize, why: &str) -> Json {
    let mut v = vec![("ok".into(), Json::Bool(false)), ("line".into(), Json::Num(line as f64))];
    match unsupported_field(why) {
        Some(f) => { v.push(("unsupported".into(), Json::Str(f.to_string()))); v.push(("engine".into(), Json::Str(engine()))); v.push(("strand_engine".into(), strand_engine(text))); }
        None => v.push(("diverges".into(), Json::Str(why.to_string()))) }
    Json::Obj(v)
}
/// The engine a strand's header names: its `engine` string, or null when the header is not JSON or has none
pub fn strand_engine(text: &str) -> Json {
    json::parse(lines(text).into_iter().next().unwrap_or("")).ok().and_then(|h| h.get("engine").cloned()).filter(|e| matches!(e, Json::Str(_))).unwrap_or(Json::Null)
}
/// This binary's engine (the strand header's `engine`)
pub fn engine() -> String { format!("probbit {}", env!("CARGO_PKG_VERSION")) }

/// Validate the stored chain and lifecycle before extending it. This is deliberately not
/// inference replay or authentication: verify still recomputes every event. In particular, a
/// checkpoint cannot reactivate a retired individual or hide an invalid earlier control.
pub fn check_chain(text: &str) -> Result<Status, InErr> { check_chain_head(text).map(|(s, _, _)| s) }
/// The header checks of `Replay::open` without a replay (the same checks, in its order, with its messages): the header read, its
/// persona built on the heap, its digest and initial state checked, and the header written again piece by piece against the line
/// (no `Live`, no whole header) -> the persona, the header, its initial state and the header's sha256
#[inline(never)]
fn open_head(head: &str) -> Result<(Box<Persona>, Json, Box<State>, String), (usize, String)> {
    let h = json::parse(head).map_err(|e| (1, format!("the header is not JSON: {}", e.msg)))?;
    if h.get("probbit_strand").and_then(Json::as_f64) != Some(FORMAT) { return Err((1, "not a probbit strand (format 1)".into())); }
    if let Some((k, _)) = h.as_obj().and_then(|kv| kv.iter().find(|(k, _)| !HEADER.contains(&k.as_str()) && k != "segment")) { return Err((1, unsupported(k))); }
    if let Some(sg) = h.get("segment") { check_segment(sg).map_err(|e| (1, format!("the header's segment: {e}")))?; }
    let doc = h.get("document").ok_or((1, "the header has no persona document".to_string()))?;
    let p = persona::build_boxed(doc).map_err(|e| (1, read_err(e, "the persona document: ")))?;
    if h.get("persona").and_then(|x| x.get("digest")).and_then(Json::as_str) != Some(p.digest.as_str()) { return Err((1, "the persona digest does not match the document".into())); }
    let st0 = read_boxed(&p, h.get("state").unwrap_or(&Json::Null)).map_err(|e| (1, read_err(e, "the initial state: ")))?;
    let (hb, mut at, mut same) = (head.as_bytes(), 0usize, true);
    let digest = write_header_seg(&p, doc, &st0, h.get("engine").and_then(Json::as_str).unwrap_or(""), h.get("segment"), HEADER_CHUNK, &mut |c: &[u8]| { same = same && hb.get(at..at + c.len()) == Some(c); at += c.len(); });
    if !same || at != hb.len() { return Err((1, "the header is not as written".into())); }
    Ok((p, h, st0, digest))
}
/// A checkpoint line's own checks (`cp`, without its line ending, at 1-based line `at`; `before` = the line before it) for persona
/// `p`, with `open_at`'s messages: canonical JSON, its four fields, right after an event line (prev), at that event's count, with
/// the stance and the state (read as any state is) that event logs -> the count, the stance and the state. No replay starts here,
/// so it holds for a persona with an expectation block too: the chain check needs the line valid, not a replay start.
#[inline(never)]
fn check_checkpoint(p: &Persona, canon: bool, before: Option<&str>, cp: &str, at: usize) -> Result<(u64, Json, Box<State>, Option<persona::ExpectMem>), (usize, String)> {
    let j = json::parse(cp).map_err(|e| (at, format!("not JSON: {}", e.msg)))?;
    // a segment's sealed line (no `before`: the event before it is in the previous segment) is taken on its link
    let b = match before { Some(before) => json::parse(before).map_err(|e| (at - 1, format!("not JSON: {}", e.msg)))?, None => Json::Null };
    if persona::canon(&j) != cp { return Err((at, "not canonical JSON".into())); }
    if let Some(k) = unknown_key(&j) { return Err((at, unsupported(k))); }
    // from 0.14.0 a persona with an expectation block also carries `expect` (the canonical fold's memory)
    let with = canon && p.expect.is_some();
    if !j.as_obj().is_some_and(|kv| kv.len() == 4 + usize::from(with) && kv.iter().all(|(k, _)| ["checkpoint", "prev", "stance", "state"].contains(&k.as_str()) || (with && k == "expect"))) {
        return Err((at, if with { "checkpoint fields must be checkpoint, expect, prev, stance and state" } else { "checkpoint fields must be checkpoint, prev, stance and state" }.into()));
    }
    // §2.18: an event line of a persona with glyphs also carries `glyph`; RFC 0009: of a persona with an expectation block, `deviation`
    let extra = |kv: &[(String, Json)]| kv.iter().filter(|(k, _)| k == "glyph" || k == "deviation").count();
    if let Some(before) = before {
        if !b.as_obj().is_some_and(|kv| kv.len() == 5 + extra(kv) && kv.iter().all(|(k, _)| ["n", "inputs", "prev", "stance", "state", "glyph", "deviation"].contains(&k.as_str()))) {
            return Err((at - 1, "a checkpoint must immediately follow an event line".into()));
        }
        if j.get("prev").and_then(Json::as_str) != Some(persona::digest_of(before).as_str()) { return Err((at, "prev is not the sha256 of the line before".into())); }
    }
    let n = j.get("checkpoint").and_then(Json::as_f64).filter(|x| *x >= 1.0 && x.fract() == 0.0).ok_or((at, "not a checkpoint line".to_string()))? as u64;
    if before.is_some() && b.get("n").and_then(Json::as_f64) != Some(n as f64) { return Err((at, format!("the line before is not event {n}"))); }
    let stance = j.get("stance").cloned().unwrap_or(Json::Null);
    if before.is_some() && b.get("stance").and_then(Json::as_str) != Some(persona::stance_sha(&stance).as_str()) { return Err((at, "the checkpoint's stance is not the one event {n} logs".replace("{n}", &n.to_string()))); }
    let st = read_boxed(p, j.get("state").unwrap_or(&Json::Null)).map_err(|e| (at, read_err(e, "the checkpoint's state: ")))?;
    if before.is_some() && b.get("state").and_then(Json::as_str) != Some(st.digest.as_str()) { return Err((at, format!("the checkpoint's state is not the one event {n} logs"))); }
    let mem = if with { Some(persona::ExpectMem::read(p, j.get("expect").unwrap_or(&Json::Null)).map_err(|e| (at, format!("the checkpoint's expect: {e}")))?) } else { None };
    Ok((n, stance, st, mem))
}
#[inline(never)]
fn read_boxed(p: &Persona, j: &Json) -> Result<Box<State>, InErr> { State::read(p, j).map(Box::new) }
/// `check_chain` -> the status, the header's persona and the header (`resume_boxed` continues with them)
#[inline(never)]
fn check_chain_head(text: &str) -> Result<(Status, Box<Persona>, Json), InErr> {
    let ls = lines(text);
    let err = |line: usize, why: String| perr("strand", format!("line {line}: {why}"));
    let (hp, h, _, _) = open_head(ls.first().copied().unwrap_or("")).map_err(|(i, why)| err(i, why))?;
    let canon = !engine_version(h.get("engine").and_then(Json::as_str).unwrap_or("")).is_some_and(|v| v < DEV_R6_SINCE);
    let mut status = Status::Active;
    let mut n = 0u64;
    let mut from = 1;
    // a segment: its second line is the sealed line, taken on its link (the header's `sealed`); the chain goes on from it
    if let Some(sg) = h.get("segment") {
        if hp.expect.is_some() && !canon { return Err(err(1, "a segment of a persona with an expectation block needs a strand of engine 0.14.0 or later".into())); }
        let cp = ls.get(1).copied().unwrap_or("");
        if Some(persona::digest_of(cp).as_str()) != sg.get("sealed").and_then(Json::as_str) { return Err(err(2, SEALED.into())); }
        let (cn, _, st, _) = check_checkpoint(&hp, canon, None, cp, 2).map_err(|(at, why)| err(at, why))?;
        if Some(st.digest.as_str()) != h.get("state").and_then(|s| s.get("digest")).and_then(Json::as_str) { return Err(err(2, "the sealed line's state is not the segment header's".into())); }
        n = cn; from = 2;
    }
    for i in from..ls.len() {
        let j = json::parse(ls[i]).map_err(|e| err(i + 1, e.msg))?;
        if persona::canon(&j) != ls[i] { return Err(err(i + 1, "not canonical JSON".into())); }
        if j.get("prev").and_then(Json::as_str) != Some(persona::digest_of(ls[i - 1]).as_str()) {
            return Err(err(i + 1, "prev is not the sha256 of the line before".into()));
        }
        if let Some(k) = unknown_key(&j) { return Err(err(i + 1, unsupported(k))); }
        match kind(&j) {
            Kind::Control => {
                let f = |k: &str| j.get(k).and_then(Json::as_str).unwrap_or("");
                status = status.after(f("control")).map_err(|e| err(i + 1, e.msg))?;
                let line = control_line(f("prev"), f("control"), f("by"), f("reason"), f("at")).map_err(|e| err(i + 1, e.msg))?;
                if line != ls[i] { return Err(err(i + 1, "the control line differs".into())); }
            }
            Kind::Event => {
                if status != Status::Active { return Err(err(i + 1, format!("event while {}", status.name()))); }
                n += 1;
                if j.get("n").and_then(Json::as_f64) != Some(n as f64) { return Err(err(i + 1, format!("n is not {n}"))); }
                let Some(Json::Obj(_)) = j.get("inputs") else { return Err(err(i + 1, "no input object".into())) };
                for k in ["state", "stance"] {
                    if !j.get(k).and_then(Json::as_str).is_some_and(|s| s.len() == 71 && s.starts_with("sha256:") && s[7..].bytes().all(|c| c.is_ascii_hexdigit())) {
                        return Err(err(i + 1, format!("invalid {k} digest")));
                    }
                }
            }
            Kind::Rest => {
                if status != Status::Active { return Err(err(i + 1, format!("rest line while {}", status.name()))); }
                if !ls.get(i + 1).and_then(|x| json::parse(x).ok()).is_some_and(|x| matches!(kind(&x), Kind::Event)) { return Err(err(i + 1, "a rest line must be followed by its event line".into())); }
            }
            Kind::Checkpoint => {
                if status != Status::Active { return Err(err(i + 1, format!("checkpoint while {}", status.name()))); }
                // Full state validation plus links to the immediately preceding event; no inference and no replay start, so a
                // persona with an expectation block (whose replay cannot start at a checkpoint) resumes after one as well
                let (cn, _, _, _) = check_checkpoint(&hp, canon, Some(ls[i - 1]), ls[i], i + 1).map_err(|(at, why)| err(at, why))?;
                if cn != n { return Err(err(i + 1, "checkpoint event count differs".into())); }
            }
        }
    }
    Ok((status, hp, h))
}

/// Continue a strand: `text` is the strand so far and `st` the individual after its last line -> the individual, ready for the
/// next event (no header to write). The header's persona must be `p` (the same digest); the run uses the header's document, the
/// key order the strand replays with. The lines before are not replayed here (`verify` does that).
pub fn resume(text: &str, p: &Persona, st: State, clock: Clock) -> Result<Live, InErr> { resume_boxed(text, p, Box::new(st), clock).map(|lv| *lv) }
/// `resume` on the heap: the state comes in boxed and the individual goes out boxed (docs/embedding.md "Stack budget")
#[inline(never)]
pub fn resume_boxed(text: &str, p: &Persona, st: Box<State>, clock: Clock) -> Result<Box<Live>, InErr> {
    let bad = |m: String| perr("strand", m);
    if !text.ends_with('\n') { return Err(bad("its last line is incomplete (no newline at the end)".into())); }
    // the chain check opens the header (its persona built once, on the heap): resume continues with that persona
    let (_, q, h) = check_chain_head(text)?;
    let lines = lines(text);
    if q.digest != p.digest { return Err(bad(format!("it is another persona's ({} != {})", &q.digest[..19], &p.digest[..19]))); }
    let last = lines[lines.len() - 1];
    // the control lines after the last event or checkpoint line (the anchor; the header without either) set the status: an
    // event or a checkpoint is only ever written while active
    let (mut a, mut tail) = (lines.len() - 1, vec![]);
    while a > 0 { let j = json::parse(lines[a]).map_err(|e| bad(format!("line {} is not JSON: {}", a + 1, e.msg)))?;
        if !matches!(kind(&j), Kind::Control) { break; } tail.push(j); a -= 1; }
    let (n, want) = if a == 0 { (0, h.get("state").and_then(|s| s.get("digest")).and_then(Json::as_str).unwrap_or("").to_string()) } else {
        let j = json::parse(lines[a]).map_err(|e| bad(format!("line {} is not JSON: {}", a + 1, e.msg)))?;
        match kind(&j) { Kind::Checkpoint => (j.get("checkpoint").and_then(Json::as_f64).unwrap_or(0.0) as u64, j.get("state").and_then(|s| s.get("digest")).and_then(Json::as_str).unwrap_or("").to_string()),
            _ => (j.get("n").and_then(Json::as_f64).unwrap_or(0.0) as u64, j.get("state").and_then(Json::as_str).unwrap_or("").to_string()) } };
    if st.digest != want { return Err(perr("state", format!("not the strand's last state ({} != {})", &st.digest[..st.digest.len().min(19)], &want[..want.len().min(19)]))); }
    // RFC 0009: the expectation's memory, folded from every event line's inputs and logged deviation; from 0.14.0 from the last
    // checkpoint's (the chain check read it), folding only the event lines after it
    let dev_full = engine_version(h.get("engine").and_then(Json::as_str).unwrap_or("")).is_some_and(|v| v < DEV_R6_SINCE);
    let expect = if q.expect.is_none() { None } else { Some(match last_checkpoint(&lines, 0).filter(|_| !dev_full) {
        Some(k) => { let j = json::parse(lines[k]).map_err(|e| bad(format!("line {} is not JSON: {}", k + 1, e.msg)))?;
            let m = persona::ExpectMem::read(&q, j.get("expect").unwrap_or(&Json::Null)).map_err(|e| bad(format!("line {}: the checkpoint's expect: {e}", k + 1)))?;
            expect_fold(&q, m, &lines[k + 1..]) }
        None => expect_fold(&q, new_mem(dev_full), &lines[1..]) }) };
    let mut lv = Live::boxed(q, st, clock, persona::digest_of(last), n, expect, dev_full);
    for c in tail.iter().rev() { let what = c.get("control").and_then(Json::as_str).unwrap_or("");
        lv.status = lv.status.after(what).unwrap_or(Status::Retired); lv.st = lv.st.zero_credit(&lv.p); lv.controls += 1; }
    Ok(lv)
}

/// Continue a strand from a state record that may be behind it (docs/persona.md §5.7, "Resume from a record"): `st` is the state
/// some line logs (an event line's `state`, a checkpoint's, or the header's), not necessarily the last; a reboot between a line
/// and its record leaves the record k lines behind. The whole strand is chain-checked as by `resume`; then the walk starts at the
/// last checkpoint at or before the record's line (the header without one), and every event line after the record's line is
/// **applied** to the record: its turn run on the logged inputs (the main solve; the expectation's member solves are not run: the
/// line's logged deviation is folded as `resume` folds it), the stance and state digests and the line's bytes checked (its logged
/// `deviation` and `glyph` taken as written: `verify` recomputes them), rest and control lines applied as the live writer applies
/// them. -> the individual after the strand's last line, as a header replay reaches it, byte for byte. Refused, with the line,
/// when the record is no state the strand logs, a line differs from its application, or the persona is another one.
#[inline(never)]
pub fn resume_from_record(text: &str, p: &Persona, st: Box<State>, clock: Clock, eng: Engine) -> Result<Box<Live>, InErr> {
    let bad = |m: String| perr("strand", m);
    if !text.ends_with('\n') { return Err(bad("its last line is incomplete (no newline at the end)".into())); }
    let (_, q, h) = check_chain_head(text)?;
    let ls = lines(text);
    // the record's line: the last whose logged state is the record (each logs its own: the state carries the turn)
    let want = format!("\"{}\"", st.digest);
    let a = (1..ls.len()).rev().find(|&i| ls[i].contains(&want)).unwrap_or(0);
    let dev_full = engine_version(h.get("engine").and_then(Json::as_str).unwrap_or("")).is_some_and(|v| v < DEV_R6_SINCE);
    // the walk starts at the last checkpoint at or before it (a segment's sealed line at the latest), whose memory it reads; the
    // header otherwise (and for an expectation block before 0.14.0, whose checkpoints do not carry the memory)
    let c = last_checkpoint(&ls[..=a.max(1).min(ls.len() - 1)], 0).filter(|_| !(dev_full && q.expect.is_some()) || h.get("segment").is_some());
    let (cp, from) = match c { Some(c) => (Some((ls[c], c + 1)), c + 1), None => (None, 1) };
    resume_walk(q, &h, ls[0], cp, ls[from..].iter().copied(), p, st, clock, eng)
}
/// The embedded resume (docs/embedding.md "Resume on a body"): continue a strand from its header line `head` (the persona
/// document, the engine and the initial state it carries), its last checkpoint line `cp` (None before the first: the walk starts
/// at the header) and the lines after that checkpoint, read one at a time from `trailing` (a host reads them from its store at the
/// checkpoint's offset), never the whole strand. `line` is the checkpoint's 1-based line number in the strand, for messages (0:
/// lines are counted from the checkpoint). The header is checked as `verify` checks it (built once, written again in pieces); the
/// checkpoint line by its own fields (canonical, its state and memory read, at an event count): its link to the line before it is
/// the host's (the offset it stored), as a segment's sealed line is taken on its link. Each trailing line is chain-checked as
/// `resume` checks it, and the record `st` may be any state logged from the checkpoint on (the event lines after it are applied, as
/// by `resume_from_record`). Cost: O(trailing lines), plus the header's check and the persona's build.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn resume_boxed_from<S: AsRef<str>>(head: &str, cp: Option<&str>, line: usize, trailing: impl IntoIterator<Item = S>, p: &Persona, st: Box<State>, clock: Clock, eng: Engine) -> Result<Box<Live>, InErr> {
    let (q, h, _, _) = open_head(head).map_err(|(i, why)| perr("strand", format!("line {i}: {why}")))?;
    let dev_full = engine_version(h.get("engine").and_then(Json::as_str).unwrap_or("")).is_some_and(|v| v < DEV_R6_SINCE);
    if cp.is_some() && dev_full && q.expect.is_some() {
        return Err(perr("strand", "before engine 0.14.0 a checkpoint does not carry the expectation's memory: resume from the whole strand"));
    }
    if cp.is_none() && h.get("segment").is_some() { return Err(perr("strand", "a segment resumes from its sealed line (line 2), not from its header")); }
    resume_walk(q, &h, head, cp.map(|c| (c, line.max(1))), trailing, p, st, clock, eng)
}
/// The walk both resumes share: from the header (`cp` None) or a checkpoint line (and its line number), over the lines after it.
/// Every line is chain-checked; the record `st` anchors at the line that logs its state; the event lines after that line are
/// applied (`apply_line`); the expectation's memory is the start's, folded over every event line.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn resume_walk<S: AsRef<str>>(q: Box<Persona>, h: &Json, head: &str, cp: Option<(&str, usize)>, trailing: impl IntoIterator<Item = S>, p: &Persona, st: Box<State>, clock: Clock, eng: Engine) -> Result<Box<Live>, InErr> {
    let sw = crate::trace::Stopwatch::start();
    let mut tr = crate::trace::ResumeTrace::default();
    let err = |line: usize, why: String| perr("strand", format!("line {line}: {why}"));
    if q.digest != p.digest { return Err(perr("strand", format!("it is another persona's ({} != {})", &q.digest[..19], &p.digest[..19]))); }
    let dev_full = engine_version(h.get("engine").and_then(Json::as_str).unwrap_or("")).is_some_and(|v| v < DEV_R6_SINCE);
    let canon = !dev_full;
    let (mut n, mut prev_line, logged, mut mem, mut at) = match cp {
        None => (0, head.to_string(), h.get("state").and_then(|s| s.get("digest")).and_then(Json::as_str).unwrap_or("").to_string(), q.expect.as_ref().map(|_| new_mem(dev_full)), 1),
        Some((c, at)) => { let (cn, _, cst, cmem) = check_checkpoint(&q, canon, None, c, at).map_err(|(i, why)| err(i, why))?;
            let mem = if q.expect.is_some() { cmem } else { None };
            (cn, c.to_string(), cst.digest.clone(), mem, at) } };
    let mut prev = persona::digest_of(&prev_line);
    // the record: anchored once the walk reaches the line that logs its state; from there on it is the individual's state
    let mut rec = Some(st);
    let mut cur: Option<Box<State>> = if rec.as_ref().is_some_and(|s| s.digest == logged) { rec.take() } else { None };
    let (mut status, mut tail, mut rest) = (Status::Active, 0u64, None::<String>);
    for l in trailing {
        let l = l.as_ref(); at += 1; tr.lines += 1;
        let j = json::parse(l).map_err(|e| err(at, format!("not JSON: {}", e.msg)))?;
        if persona::canon(&j) != l { return Err(err(at, "not canonical JSON".into())); }
        if j.get("prev").and_then(Json::as_str) != Some(prev.as_str()) { return Err(err(at, "prev is not the sha256 of the line before".into())); }
        if let Some(k) = unknown_key(&j) { return Err(err(at, unsupported(k))); }
        if rest.is_some() && !matches!(kind(&j), Kind::Event) { return Err(err(at - 1, "a rest line must be followed by its event line".into())); }
        match kind(&j) {
            Kind::Rest => { if status != Status::Active { return Err(err(at, format!("rest line while {}", status.name()))); } rest = Some(l.to_string()); }
            Kind::Control => { let f = |k: &str| j.get(k).and_then(Json::as_str).unwrap_or("");
                status = status.after(f("control")).map_err(|e| err(at, e.msg))?;
                let line = control_line(f("prev"), f("control"), f("by"), f("reason"), f("at")).map_err(|e| err(at, e.msg))?;
                if line != l { return Err(err(at, "the control line differs".into())); }
                // as the live writer: no credit crosses a control line; `erase` returns the learned weights to zero
                if let Some(s) = cur.as_mut() { let mut z = Box::new(s.zero_credit(&q)); if f("control") == "erase" { z = Box::new(z.erased(&q)); } *s = z; }
                tail += 1; }
            Kind::Checkpoint => {
                if status != Status::Active { return Err(err(at, format!("checkpoint while {}", status.name()))); }
                let (cn, _, cst, cmem) = check_checkpoint(&q, canon, Some(&prev_line), l, at).map_err(|(i, why)| err(i, why))?;
                if cn != n { return Err(err(at, "checkpoint event count differs".into())); }
                if q.expect.is_some() && cmem.is_some() { mem = cmem; }
                if cur.is_none() && rec.as_ref().is_some_and(|s| s.digest == cst.digest) { cur = rec.take(); }
                tail = 0; }
            Kind::Event => {
                if status != Status::Active { return Err(err(at, format!("event while {}", status.name()))); }
                n += 1;
                if j.get("n").and_then(Json::as_f64) != Some(n as f64) { return Err(err(at, format!("n is not {n}"))); }
                let Some(Json::Obj(_)) = j.get("inputs") else { return Err(err(at, "no input object".into())) };
                for k in ["state", "stance"] {
                    if !j.get(k).and_then(Json::as_str).is_some_and(|s| s.len() == 71 && s.starts_with("sha256:") && s[7..].bytes().all(|c| c.is_ascii_hexdigit())) {
                        return Err(err(at, format!("invalid {k} digest")));
                    }
                }
                let logged = j.get("state").and_then(Json::as_str).unwrap_or("");
                match cur.as_mut() {
                    Some(s) => { let t0 = crate::trace::Stopwatch::start();
                        *s = apply_line(&q, s, &j, l, rest.as_deref(), eng).map_err(|why| err(at, why))?;
                        tr.applied += 1; tr.applied_us += t0.us(); }
                    None => if rec.as_ref().is_some_and(|s| s.digest == logged) { cur = rec.take(); } }
                rest = None;
                if let Some(m) = mem.as_mut() { persona::expect_step(&q, m, j.get("inputs").unwrap_or(&Json::Null), logged_pred(&j)); }
                tail = 0; } }
        prev = persona::digest_of(l); prev_line.clear(); prev_line.push_str(l);
    }
    if rest.is_some() { return Err(err(at, "a rest line must be followed by its event line".into())); }
    let Some(st) = cur else {
        let d = rec.as_ref().map_or("", |s| s.digest.as_str());
        return Err(perr("state", format!("not a state this strand logs from where the resume starts ({}): neither the start's nor any line's after it", &d[..d.len().min(19)])));
    };
    let mut lv = Live::boxed(q, st, clock, prev, n, mem, dev_full);
    lv.status = status; lv.controls = tail;
    tr.total_us = sw.us(); crate::trace::resumed(tr);
    Ok(lv)
}
/// One recorded event line applied to the state before it (`resume_from_record`): the line's inputs stamped as the live writer
/// stamps them on the fixed clock (its logged `elapsed_hours`), a rest line due before it recomputed against the logged one, the
/// turn run, and the line written again from what the turn gives (n, inputs, stance digest, state digest, prev) with the logged
/// `deviation` and `glyph` -> the state after it, or what differs. Without the expectation's member solves: the deviation and
/// the glyph that reads it are the line's.
#[inline(never)]
fn apply_line(p: &Persona, st: &State, j: &Json, l: &str, rest: Option<&str>, eng: Engine) -> Result<Box<State>, String> {
    let inputs = stamp_logged(j)?;
    rest_check(p, st, &inputs, rest)?;
    // the turn from the smallest frame: its document and state only (the checks are their own frames, before and after)
    let (stance, ns) = persona::turn_boxed(p, st, &inputs, false, eng, false).map_err(|e| read_err(e, ""))?;
    line_check(j, l, inputs, &stance, &ns.digest)?;
    Ok(ns)
}
/// A logged event's inputs stamped as the live writer stamps them on the fixed clock (its `elapsed_hours`, quantised, last)
#[inline(never)]
fn stamp_logged(j: &Json) -> Result<Json, String> {
    let ev = j.get("inputs").unwrap_or(&Json::Null);
    let Json::Obj(kv) = ev else { return Err("no input object".into()) };
    let mut kv: Vec<(String, Json)> = kv.iter().filter(|(k, _)| k != "elapsed_hours").cloned().collect();
    let e = match ev.get("elapsed_hours") { None => 0.0, Some(Json::Num(x)) if x.is_finite() && *x >= 0.0 => *x, _ => return Err("event.elapsed_hours: a number >= 0".into()) };
    kv.push(("elapsed_hours".into(), Json::Num(q6(e)))); Ok(Json::Obj(kv))
}
/// A rest line due before the event (RFC 0002 part 2) recomputed against the logged one
#[inline(never)]
fn rest_check(p: &Persona, st: &State, inputs: &Json, rest: Option<&str>) -> Result<(), String> {
    match (persona::rest_line_digest(p, st, inputs), rest) {
        (Some((r, rd)), Some(lr)) => { let rp = json::parse(lr).ok().and_then(|x| x.get("prev").and_then(Json::as_str).map(str::to_string)).unwrap_or_default();
            if rest_line(&rp, r, &rd) != lr { return Err("the rest line differs".into()); } Ok(()) }
        (Some(_), None) => Err("a rest line is due before this event and missing".into()),
        (None, Some(_)) => Err("a rest line where none is due".into()),
        (None, None) => Ok(()) }
}
/// The applied event's line against the logged one: its stance and state digests, then its bytes (n, inputs, stance, state,
/// prev as the turn gives them; the logged `deviation` and `glyph` as written)
#[inline(never)]
fn line_check(j: &Json, l: &str, inputs: Json, stance: &Json, state: &str) -> Result<(), String> {
    let sd = persona::stance_sha(stance);
    if j.get("stance").and_then(Json::as_str) != Some(sd.as_str()) { return Err("the stance differs".into()); }
    if j.get("state").and_then(Json::as_str) != Some(state) { return Err("the state differs".into()); }
    let mut lj = vec![("n".into(), j.get("n").cloned().unwrap_or(Json::Null)), ("inputs".into(), inputs), ("stance".into(), Json::Str(sd)),
        ("state".into(), Json::Str(state.to_string())), ("prev".into(), j.get("prev").cloned().unwrap_or(Json::Null))];
    for k in ["deviation", "glyph"] { if let Some(x) = j.get(k) { lj.push((k.into(), x.clone())); } }
    if persona::canon(&Json::Obj(lj)) != l { return Err("the line differs".into()); }
    Ok(())
}
/// RFC 0009: an event line's logged prediction (none on a cold start), the value the expectation's memory folds
fn logged_pred(j: &Json) -> Option<f64> { j.get("deviation").filter(|d| d.get("cold").is_none()).and_then(|d| d.get("pred")).and_then(Json::as_f64) }

/// The expectation's empty memory: the canonical fold from 0.14.0 (`dev_full` false), the fold before it otherwise
fn new_mem(dev_full: bool) -> persona::ExpectMem { if dev_full { persona::ExpectMem::new() } else { persona::ExpectMem::canonical() } }
/// RFC 0009: the expectation's memory `m` with the event lines of `lines` folded in, one line read at a time
#[inline(never)]
fn expect_fold(q: &Persona, mut m: persona::ExpectMem, lines: &[&str]) -> persona::ExpectMem {
    for l in lines.iter() { if let Ok(j) = json::parse(l) { if matches!(kind(&j), Kind::Event) {
        persona::expect_step(q, &mut m, j.get("inputs").unwrap_or(&Json::Null), logged_pred(&j)); } } }
    m
}
/// The audit of a recorded life (`probbit persona bound PERSONA --audit STRAND`): the strand replayed line by line; after every
/// event and control line, per learned trait the resting TV between the individual and the same individual with its learned
/// weights at zero, as a share of its epsilon (the room used). An exceedance is impossible by the theorem; the audit shows it
/// on the recorded life.
pub fn bound_audit(text: &str, b: &persona::bound::Bound, eng: Engine) -> Result<(Json, bool), (usize, String)> {
    let lines = lines(text);
    let (mut r, k0) = Replay::open_lines(&lines)?;
    let p = r.live.p.clone(); let (traits, cap) = persona::bound::learned_traits(&p).ok_or((1, "the strand's persona has no learning block: nothing to audit".to_string()))?;
    let rest = b.rest_hours.unwrap_or_else(|| persona::bound::default_rest(&p)); let seed = r.live.st.seed;
    let e = |x: InErr| (0usize, format!("{}: {}", x.path, x.msg));
    let st0 = persona::init(&p, Some(seed), true, eng); let p0 = persona::bound::rest_odds(&p, &st0, rest, eng).map_err(e)?;
    let mut eps = vec![]; let mut rr = vec![];
    for (k, t) in traits.iter().enumerate() { let sc = persona::bound::scope(&p, b, t, rest, Some(&st0));
        let bs = persona::bound::bases(&p, &sc, &st0).ok_or((0, format!("{t}: more than {} enumerated history combinations: the audit needs an exact epsilon", persona::bound::MAX_HISTORY)))?;
        eps.push(persona::bound::search(&p, k, &sc.coords, cap, &bs, &p0[k], rest, eng).map_err(e)?.0); rr.push((sc.d_r / 4.0).tanh()); }
    let nt = traits.len(); let (mut maxr, mut at_n, mut cur, mut exceed) = (vec![0.0f64; nt], vec![0u64; nt], vec![0.0f64; nt], 0usize);
    let (mut events, mut feedback, mut erasures) = (0u64, 0u64, 0u64); let mut last = persona::bound::learned_of(&r.live.st);
    let measure = |st: &persona::State| -> Result<Vec<f64>, (usize, String)> { let a = persona::bound::rest_odds(&p, st, rest, eng).map_err(e)?;
        let z = persona::bound::rest_odds(&p, &persona::bound::with_learned(&p, st, &persona::bound::learned_of(st).iter().map(|d| vec![0.0; d.len()]).collect::<Vec<_>>()), rest, eng).map_err(e)?;
        Ok(a.iter().zip(&z).map(|(x, y)| persona::bound::tv(x, y)).collect()) };
    for l in lines.iter().skip(k0) { if l.trim().is_empty() { continue; }
        let was_event = r.step(l, eng)?.is_some(); if was_event { events += 1; } else if l.contains(r#""control":"erase""#) { erasures += 1; }
        let now = persona::bound::learned_of(&r.live.st); if now != last { if was_event { feedback += 1; } last = now; }
        let tvs = measure(&r.live.st)?;
        for k in 0..nt { cur[k] = tvs[k]; if eps[k] > 0.0 && tvs[k] / eps[k] > maxr[k] { maxr[k] = tvs[k] / eps[k]; at_n[k] = r.live.n; } if tvs[k] > eps[k] + rr[k] + 1e-6 { exceed += 1; } } }
    let s = |x: &str| Json::Str(x.to_string());
    let rows: Vec<Json> = traits.iter().enumerate().map(|(k, t)| Json::Obj(vec![("trait".into(), s(t)), ("epsilon".into(), Json::Num(eps[k])),
        ("room_used_max".into(), Json::Num(persona::r6(maxr[k]))), ("at_event".into(), Json::Num(at_n[k] as f64)), ("room_used_now".into(), Json::Num(persona::r6(if eps[k] > 0.0 { cur[k] / eps[k] } else { 0.0 }))),
        ("distance_to_ceiling".into(), Json::Num(persona::r6(eps[k] - cur[k]))), ("tv_now".into(), Json::Num(persona::r6(cur[k])))])).collect();
    let ok = exceed == 0;
    Ok((Json::Obj(vec![("persona".into(), s(&p.name)), ("digest".into(), s(&p.digest)), ("seed".into(), Json::Num(seed as f64)), ("rest_hours".into(), Json::Num(rest)),
        ("events".into(), Json::Num(events as f64)), ("feedback_events".into(), Json::Num(feedback as f64)), ("erasures".into(), Json::Num(erasures as f64)),
        ("traits".into(), Json::Arr(rows)), ("exceedances".into(), Json::Num(exceed as f64)), ("ok".into(), Json::Bool(ok))]), ok))
}
