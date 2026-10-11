//! `probbit live` (docs/persona.md §5.7): a resident individual. Events come in as JSON objects of inputs; the clock stamps each
//! one's `elapsed_hours` (the time since the previous event, quantised to 1e-6 h), so moods decay by their half-lives while
//! nobody is talking to it, and feedback moves the learned deltas (§2.8). Every event appends one line to the strand: the
//! inputs as the turn used them, the stance's and the new state's digests, and the sha256 of the line before. The strand's
//! header carries the persona document, the initial state and the engine version, so `probbit live verify STRAND` replays the
//! whole life from the strand alone and names the earliest line that differs.
//!
//! The safety kit (§5.7): one writer per strand (`STRAND.lock`), control lines (pause, resume, retire: appended by `probbit live
//! control`, never by an event; no credit crosses them) and checkpoint lines (the whole state every K events, so a replay can
//! start there). `verify` replays all three.
use crate::json::{self, InErr, Json};
use crate::persona::{self, perr, Engine, Persona, State};

/// The strand format
const FORMAT: f64 = 1.0;
/// The clock. `Real`: a monotonic clock started with the run; each event's elapsed hours are the time since the previous event
/// (the opening event: since the start). `Fixed`: time comes from the events, each carrying its own `elapsed_hours` (default 0), so a
/// run is a pure function of its events.
pub enum Clock { Real(std::time::Instant), Fixed }
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
pub struct Live { pub p: Persona, pub st: State, clock: Clock, last: f64, prev: String, pub n: u64, pub status: Status, pub controls: u64, pub checkpoints: u64 }
/// A document with its keys sorted at every level (`persona::text` of it is its canonical JSON)
fn sorted(j: &Json) -> Json {
    match j { Json::Obj(v) => { let mut o: Vec<(String, Json)> = v.iter().map(|(k, x)| (k.clone(), sorted(x))).collect(); o.sort_by(|a, b| a.0.cmp(&b.0)); Json::Obj(o) }
        Json::Arr(v) => Json::Arr(v.iter().map(sorted).collect()), x => x.clone() }
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
        let s = |x: &str| Json::Str(x.to_string());
        let header = persona::text(&Json::Obj(vec![("probbit_strand".into(), Json::Num(FORMAT)), ("engine".into(), s(engine)),
            ("persona".into(), Json::Obj(vec![("name".into(), s(&p.name)), ("version".into(), s(&p.version)), ("digest".into(), s(&p.digest))])),
            ("seed".into(), Json::Num(st.seed as f64)), ("state".into(), sorted(&st.to_json(&p))), ("document".into(), doc.clone())]));
        let prev = persona::digest_of(&header);
        (Live { p, st, clock, last: 0.0, prev, n: 0, status: Status::Active, controls: 0, checkpoints: 0 }, header)
    }
    /// One event: stamp its elapsed hours, run the turn, chain the strand line -> (the stance document, the strand line). A bad
    /// event is an error and changes nothing (no turn, no line); so is every event while the individual is paused or retired.
    pub fn event(&mut self, ev: &Json, eng: Engine) -> Result<(Json, String), InErr> {
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
        let (stance, ns) = persona::turn(&self.p, &self.st, &inputs, false, eng, false)?;
        let line = persona::canon(&Json::Obj(vec![("n".into(), Json::Num((self.n + 1) as f64)), ("inputs".into(), inputs), ("stance".into(), Json::Str(persona::sha(&stance))),
            ("state".into(), Json::Str(ns.digest.clone())), ("prev".into(), Json::Str(self.prev.clone()))]));
        self.n += 1; self.last = now; self.prev = persona::digest_of(&line); self.st = ns;
        Ok((stance, line))
    }
    /// A control line (pause, resume or retire) `by` a person or a sensor (`human[:id]`, `env[:id]`) for `reason`, stamped `at`:
    /// the status moves, the credit is zeroed (no feedback after the line credits a stance before it) and the line is chained ->
    /// the line. Nothing else changes: moods, drives, learned weights and the clock are the individual's as before.
    pub fn control(&mut self, what: &str, by: &str, reason: &str, at: &str) -> Result<String, InErr> {
        let to = self.status.after(what)?;
        let line = control_line(&self.prev, what, by, reason, at)?;
        self.st = self.st.zero_credit(&self.p); self.status = to; self.controls += 1; self.prev = persona::digest_of(&line);
        Ok(line)
    }
    /// A checkpoint line after event n: the event count, the whole state after it and that event's stance document, chained ->
    /// the line. A replay can start there (the state) and a board can be drawn there (the stance) without the events before.
    pub fn checkpoint(&mut self, stance: &Json) -> String {
        let line = persona::canon(&Json::Obj(vec![("checkpoint".into(), Json::Num(self.n as f64)), ("prev".into(), Json::Str(self.prev.clone())), ("stance".into(), stance.clone()),
            ("state".into(), self.st.to_json(&self.p))]));
        self.checkpoints += 1; self.prev = persona::digest_of(&line); line
    }
    /// The sha256 of the strand's last line (the header's before any event)
    pub fn head(&self) -> &str { &self.prev }
}
/// A control line after the line whose sha256 is `prev` (canonical JSON: at, by, control, prev, reason). `by` names who: a
/// person or a sensor, never the individual or the clock; the reason is 1-500 characters.
pub fn control_line(prev: &str, what: &str, by: &str, reason: &str, at: &str) -> Result<String, InErr> {
    if !["pause", "resume", "retire"].contains(&what) { return Err(perr("control", "pause | resume | retire")); }
    if !matches!(persona::src_kind(by), Some("human" | "env")) { return Err(perr("control.by", "who: human[:id] or env[:id] (id: 1-64 of A-Z a-z 0-9 _ . - @ / :); the individual cannot control itself")); }
    if reason.trim().is_empty() || reason.chars().count() > 500 { return Err(perr("control.reason", "1-500 characters")); }
    if at.is_empty() || at.len() > 64 { return Err(perr("control.at", "a time stamp, 1-64 characters (RFC 3339 by default)")); }
    let s = |x: &str| Json::Str(x.to_string());
    Ok(persona::canon(&Json::Obj(vec![("at".into(), s(at)), ("by".into(), s(by)), ("control".into(), s(what)), ("prev".into(), s(prev)), ("reason".into(), s(reason))])))
}
/// What kind of strand line `j` is
enum Kind { Event, Control, Checkpoint }
fn kind(j: &Json) -> Kind { if j.get("control").is_some() { Kind::Control } else if j.get("checkpoint").is_some() { Kind::Checkpoint } else { Kind::Event } }
/// Now as RFC 3339 UTC to the second (the control lines' default `at`)
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
pub struct Replay { pub live: Live, pub header: Json, pub doc: Json, pub line: usize, pub from: Option<u64>, last: Option<Json>, checkpoint_ready: bool }
impl Replay {
    /// The header line (without its line ending) -> the replay, ready for event lines; Err((1, what differs))
    pub fn open(head: &str) -> Result<Replay, (usize, String)> {
        let h = json::parse(head).map_err(|e| (1, format!("the header is not JSON: {}", e.msg)))?;
        if h.get("probbit_strand").and_then(Json::as_f64) != Some(FORMAT) { return Err((1, "not a probbit strand (format 1)".into())); }
        let doc = h.get("document").cloned().ok_or((1, "the header has no persona document".to_string()))?;
        let p = persona::build(&doc).map_err(|e| (1, format!("the persona document: {}: {}", e.path, e.msg)))?;
        if h.get("persona").and_then(|x| x.get("digest")).and_then(Json::as_str) != Some(p.digest.as_str()) { return Err((1, "the persona digest does not match the document".into())); }
        let st0 = State::read(&p, h.get("state").unwrap_or(&Json::Null)).map_err(|e| (1, format!("the initial state: {}: {}", e.path, e.msg)))?;
        let (live, header) = Live::start(p, &doc, st0, Clock::Fixed, h.get("engine").and_then(Json::as_str).unwrap_or(""));
        if header != head { return Err((1, "the header is not as written".into())); }
        Ok(Replay { live, header: h, doc, line: 1, from: None, last: None, checkpoint_ready: false })
    }
    /// The replay from a checkpoint line (`cp`, without its line ending, at 1-based line `at`; `before` = the event line before it)
    /// instead of the header: the header is read as by `open`; the checkpoint must follow `before` (prev), at its event count, with
    /// its stance (the digest `before` logs) and its state (read as any state is, with the digest `before` logs); the chain goes on
    /// from the checkpoint line. The lines before it are taken as written: a full `verify` replays them and checks the checkpoint.
    pub fn open_at(head: &str, before: &str, cp: &str, at: usize) -> Result<Replay, (usize, String)> {
        let mut r = Replay::open(head)?;
        let j = json::parse(cp).map_err(|e| (at, format!("not JSON: {}", e.msg)))?;
        let b = json::parse(before).map_err(|e| (at - 1, format!("not JSON: {}", e.msg)))?;
        if persona::canon(&j) != cp { return Err((at, "not canonical JSON".into())); }
        if !j.as_obj().is_some_and(|kv| kv.len() == 4 && kv.iter().all(|(k, _)| ["checkpoint", "prev", "stance", "state"].contains(&k.as_str()))) {
            return Err((at, "checkpoint fields must be checkpoint, prev, stance and state".into()));
        }
        if !b.as_obj().is_some_and(|kv| kv.len() == 5 && kv.iter().all(|(k, _)| ["n", "inputs", "prev", "stance", "state"].contains(&k.as_str()))) {
            return Err((at - 1, "a checkpoint must immediately follow an event line".into()));
        }
        if j.get("prev").and_then(Json::as_str) != Some(persona::digest_of(before).as_str()) { return Err((at, "prev is not the sha256 of the line before".into())); }
        let n = j.get("checkpoint").and_then(Json::as_f64).filter(|x| *x >= 1.0 && x.fract() == 0.0).ok_or((at, "not a checkpoint line".to_string()))? as u64;
        if b.get("n").and_then(Json::as_f64) != Some(n as f64) { return Err((at, format!("the line before is not event {n}"))); }
        let stance = j.get("stance").cloned().unwrap_or(Json::Null);
        if b.get("stance").and_then(Json::as_str) != Some(persona::sha(&stance).as_str()) { return Err((at, "the checkpoint's stance is not the one event {n} logs".replace("{n}", &n.to_string()))); }
        let st = State::read(&r.live.p, j.get("state").unwrap_or(&Json::Null)).map_err(|e| (at, format!("the checkpoint's state: {}: {}", e.path, e.msg)))?;
        if b.get("state").and_then(Json::as_str) != Some(st.digest.as_str()) { return Err((at, format!("the checkpoint's state is not the one event {n} logs"))); }
        r.live.st = st; r.live.n = n; r.live.prev = persona::digest_of(cp); r.live.checkpoints = 1; r.line = at; r.from = Some(n); r.last = Some(stance); r.checkpoint_ready = false;
        Ok(r)
    }
    /// The stance document of the last event replayed (or of the checkpoint the replay started from)
    pub fn last_stance(&self) -> Option<&Json> { self.last.as_ref() }
    /// One line (without its line ending) -> the stance document an event line replays to (None for a control or checkpoint
    /// line); Err((its 1-based line number, what differs)). A control line must be a valid move of the status and the line its
    /// fields give; a checkpoint line must immediately follow an event while active and carry
    /// the event count and the state the replay reached. The last stance remains available to
    /// the monitor after controls/checkpoints, but that does not authorize another checkpoint.
    pub fn step(&mut self, l: &str, eng: Engine) -> Result<Option<Json>, (usize, String)> {
        let (i, live) = (self.line + 1, &mut self.live);
        let j = json::parse(l).map_err(|e| (i, format!("not JSON: {}", e.msg)))?;
        if persona::canon(&j) != l { return Err((i, "not canonical JSON".into())); }
        if j.get("prev").and_then(Json::as_str) != Some(live.head()) { return Err((i, "prev is not the sha256 of the line before (a line before it was changed, removed or reordered)".into())); }
        match kind(&j) {
            Kind::Control => { let f = |k: &str| j.get(k).and_then(Json::as_str).unwrap_or("");
                let line = live.control(f("control"), f("by"), f("reason"), f("at")).map_err(|e| (i, format!("control line: {}: {}", e.path, e.msg)))?;
                if line != l { return Err((i, "the control line differs".into())); }
                self.line = i; self.checkpoint_ready = false; return Ok(None) }
            Kind::Checkpoint => {
                if live.status != Status::Active { return Err((i, format!("checkpoint while {}", live.status.name()))); }
                if !self.checkpoint_ready { return Err((i, "a checkpoint must immediately follow an event line".into())); }
                if j.get("checkpoint").and_then(Json::as_f64) != Some(live.n as f64) { return Err((i, format!("the checkpoint is not at event {}", live.n))); }
                let Some(stance) = self.last.as_ref() else { return Err((i, "a checkpoint follows an event line".into())) };
                if live.checkpoint(stance) != l { return Err((i, "the checkpoint's state or stance differs from the replay".into())); }
                self.line = i; self.checkpoint_ready = false; return Ok(None) }
            Kind::Event => {} }
        if j.get("n").and_then(Json::as_f64) != Some((live.n + 1) as f64) { return Err((i, format!("n is not {}", live.n + 1))); }
        let ev = j.get("inputs").ok_or((i, "no inputs".to_string()))?;
        let (stance, line) = live.event(ev, eng).map_err(|e| (i, format!("{}: {}", e.path, e.msg)))?;
        if j.get("stance").and_then(Json::as_str) != Some(persona::sha(&stance).as_str()) { return Err((i, "the stance differs".into())); }
        if j.get("state").and_then(Json::as_str) != Some(live.st.digest.as_str()) { return Err((i, "the state differs".into())); }
        if line != l { return Err((i, "the line differs".into())); }
        self.line = i; self.last = Some(stance.clone()); self.checkpoint_ready = true;
        Ok(Some(stance))
    }
    /// `verify`'s summary of the lines replayed so far
    pub fn summary(&self) -> Json {
        let (s, live) = (|x: &str| Json::Str(x.to_string()), &self.live);
        let mut v = vec![("ok".into(), Json::Bool(true)), ("events".into(), Json::Num(live.n as f64)), ("persona".into(), s(&live.p.name)), ("seed".into(), Json::Num(live.st.seed as f64)),
            ("engine".into(), self.header.get("engine").cloned().unwrap_or(Json::Null)), ("final_state".into(), s(&live.st.digest)), ("last_line".into(), s(live.head()))];
        // a strand with control or checkpoint lines says so (a strand without them gives the summary of before)
        if live.controls > 0 { v.push(("controls".into(), Json::Num(live.controls as f64))); v.push(("status".into(), s(live.status.name()))); }
        if live.checkpoints > 0 { v.push(("checkpoints".into(), Json::Num(live.checkpoints as f64))); }
        if let Some(n) = self.from { v.push(("from_checkpoint".into(), Json::Num(n as f64))); }
        Json::Obj(v)
    }
}
/// A strand's text -> its lines without their line endings (a final newline ends the last line; it does not start another)
pub fn lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').map(bare).collect();
    if lines.last() == Some(&"") { lines.pop(); }
    lines
}
/// `probbit live verify`: replay a strand from its header -> Ok(summary) when every line is as recorded; Err((1-based line
/// number, what differs)) at the earliest line that is not. Lines end in `\n` or `\r\n`; the chain hashes the line text alone.
pub fn verify(text: &str, eng: Engine) -> Result<Json, (usize, String)> {
    let lines = lines(text);
    let mut r = Replay::open(lines.first().copied().unwrap_or(""))?;
    for l in lines.iter().skip(1) { r.step(l, eng)?; }
    Ok(r.summary())
}
/// The 0-based index of the strand's last checkpoint line followed by at least `after` lines, if any (a cheap scan: the key)
pub fn last_checkpoint(lines: &[&str], after: usize) -> Option<usize> {
    (1..lines.len().saturating_sub(after)).rev().find(|&i| lines[i].starts_with(r#"{"checkpoint":"#))
}
/// `verify --from-checkpoint`: replay from the last checkpoint line (the header read as always), not from the header
pub fn verify_from_checkpoint(text: &str, eng: Engine) -> Result<Json, (usize, String)> {
    let lines = lines(text);
    let Some(k) = last_checkpoint(&lines, 0) else { return verify(text, eng) };
    let mut r = Replay::open_at(lines[0], lines[k - 1], lines[k], k + 1)?;
    for l in lines.iter().skip(k + 1) { r.step(l, eng)?; }
    Ok(r.summary())
}
/// `verify` as one document: the summary, or {ok: false, line, diverges}
pub fn verify_doc(text: &str, eng: Engine) -> Json {
    verify(text, eng).unwrap_or_else(|(line, why)| Json::Obj(vec![("ok".into(), Json::Bool(false)), ("line".into(), Json::Num(line as f64)), ("diverges".into(), Json::Str(why))]))
}
/// This binary's engine (the strand header's `engine`)
pub fn engine() -> String { format!("probbit {}", env!("CARGO_PKG_VERSION")) }

/// Validate the stored chain and lifecycle before extending it. This is deliberately not
/// inference replay or authentication: verify still recomputes every event. In particular, a
/// checkpoint cannot reactivate a retired individual or hide an invalid earlier control.
fn check_chain(text: &str) -> Result<Status, InErr> {
    let ls = lines(text);
    let err = |line: usize, why: String| perr("strand", format!("line {line}: {why}"));
    let r = Replay::open(ls.first().copied().unwrap_or("")).map_err(|(i, why)| err(i, why))?;
    let mut status = Status::Active;
    let mut n = 0u64;
    for i in 1..ls.len() {
        let j = json::parse(ls[i]).map_err(|e| err(i + 1, e.msg))?;
        if persona::canon(&j) != ls[i] { return Err(err(i + 1, "not canonical JSON".into())); }
        if j.get("prev").and_then(Json::as_str) != Some(persona::digest_of(ls[i - 1]).as_str()) {
            return Err(err(i + 1, "prev is not the sha256 of the line before".into()));
        }
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
            Kind::Checkpoint => {
                if status != Status::Active { return Err(err(i + 1, format!("checkpoint while {}", status.name()))); }
                // Full state validation plus links to the immediately preceding event; no inference.
                let cp = Replay::open_at(ls[0], ls[i - 1], ls[i], i + 1).map_err(|(at, why)| err(at, why))?;
                if cp.live.n != n { return Err(err(i + 1, "checkpoint event count differs".into())); }
                // Keep the header persona/state check independent of the caller's state.
                debug_assert_eq!(cp.live.p.digest, r.live.p.digest);
            }
        }
    }
    Ok(status)
}

/// Continue a strand: `text` is the strand so far and `st` the individual after its last line -> the individual, ready for the
/// next event (no header to write). The header's persona must be `p` (the same digest); the run uses the header's document, the
/// key order the strand replays with. The lines before are not replayed here (`verify` does that).
pub fn resume(text: &str, p: &Persona, st: State, clock: Clock) -> Result<Live, InErr> {
    let bad = |m: String| perr("strand", m);
    if !text.ends_with('\n') { return Err(bad("its last line is incomplete (no newline at the end)".into())); }
    check_chain(text)?;
    let lines = lines(text);
    let h = json::parse(lines.first().copied().unwrap_or("")).map_err(|e| bad(format!("the header is not JSON: {}", e.msg)))?;
    if h.get("probbit_strand").and_then(Json::as_f64) != Some(FORMAT) { return Err(bad("not a probbit strand (format 1)".into())); }
    let q = persona::build(h.get("document").unwrap_or(&Json::Null)).map_err(|e| bad(format!("the header's persona: {}: {}", e.path, e.msg)))?;
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
    let mut lv = Live { p: q, st, clock, last: 0.0, prev: persona::digest_of(last), n, status: Status::Active, controls: 0, checkpoints: 0 };
    for c in tail.iter().rev() { let what = c.get("control").and_then(Json::as_str).unwrap_or("");
        lv.status = lv.status.after(what).unwrap_or(Status::Retired); lv.st = lv.st.zero_credit(&lv.p); lv.controls += 1; }
    Ok(lv)
}
/// A strand file to log to: a new file gets the header (returned, to write before the events); an existing strand is continued from `st`,
/// which must be its last state
pub fn open(path: &str, p: Persona, doc: &Json, st: State, clock: Clock) -> Result<(Live, Option<String>), InErr> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok((resume(&text, &p, st, clock)?, None)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => { let (lv, h) = Live::start(p, doc, st, clock, &engine()); Ok((lv, Some(h))) }
        Err(e) => Err(perr("strand", format!("cannot read {path}: {e}"))) }
}
/// Append lines to a strand file (created if new)
pub fn append(path: &str, lines: &[&str]) -> Result<(), InErr> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| perr("strand", format!("cannot open {path}: {e}")))?;
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    f.write_all(text.as_bytes()).and_then(|_| f.flush()).map_err(|e| perr("strand", format!("cannot write {path}: {e}")))
}

// ---------------------------------------------------------------- the writer lock
/// The writer lock on a strand (§5.7): `STRAND.lock`, created exclusively (written aside, then hard-linked into place, so it is
/// never seen half written) with this process's id and start; a lock whose process is gone is stale and is taken over. `live`
/// holds it from before it reads the strand to its exit, `live control` and `probbit_live_event` for their one append. Every
/// append first checks the lock is still this writer's, so a lock removed or taken over under a running writer stops it before
/// it writes. Released (removed) when the writer ends, on the error exits too (`release_held`).
pub struct Lock { path: String, token: String }
static HELD: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
fn locked(by: Option<u32>, path: &str) -> InErr {
    InErr { code: "locked", path: "strand".into(), msg: format!("strand locked by {}: another writer holds {path} (one writer per strand; a lock whose process is gone is taken over)",
        by.map_or("a writer".to_string(), |p| format!("pid {p}"))) }
}
fn pid_of(t: &str) -> Option<u32> { json::parse(t).ok()?.get("pid")?.as_f64().filter(|x| *x >= 1.0 && *x <= u32::MAX as f64 && x.fract() == 0.0).map(|x| x as u32) }
impl Lock {
    /// Take the lock of `strand` -> the lock, or the error `locked` (code `locked`: `live` exits 4)
    pub fn take(strand: &str) -> Result<Lock, InErr> {
        let path = format!("{strand}.lock");
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let token = format!(r#"{{"pid":{},"since":"{}","t":{t}}}"#, std::process::id(), utc_now());
        for _ in 0..3 {
            match create_exclusive(&path, &token) {
                Ok(()) => { HELD.lock().unwrap_or_else(|e| e.into_inner()).push((path.clone(), token.clone())); return Ok(Lock { path, token }) }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let held = std::fs::read_to_string(&path).unwrap_or_default();
                    match pid_of(&held) {
                        // stale: move it aside (one taker wins the rename), check it is the lock read, remove it and try again
                        Some(pid) if !alive(pid) => { let aside = format!("{path}.stale.{}", std::process::id());
                            if std::fs::rename(&path, &aside).is_ok() {
                                if std::fs::read_to_string(&aside).unwrap_or_default() != held { let _ = std::fs::hard_link(&aside, &path); let _ = std::fs::remove_file(&aside); return Err(locked(None, &path)); }
                                let _ = std::fs::remove_file(&aside); } }
                        by => return Err(locked(by, &path)) } }
                Err(e) => return Err(perr("strand", format!("cannot create {path}: {e}"))) } }
        Err(locked(None, &path))
    }
    /// The lock is still this writer's (checked before every append)
    pub fn held(&self) -> bool { std::fs::read_to_string(&self.path).is_ok_and(|t| t == self.token) }
}
impl Drop for Lock {
    fn drop(&mut self) { if self.held() { let _ = std::fs::remove_file(&self.path); } HELD.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| *p != self.path); }
}
/// Remove the locks this process holds (the exits that skip `Drop`: `std::process::exit`)
pub fn release_held() {
    for (p, t) in HELD.lock().unwrap_or_else(|e| e.into_inner()).drain(..) { if std::fs::read_to_string(&p).is_ok_and(|x| x == t) { let _ = std::fs::remove_file(&p); } }
}
/// Create `path` holding `text`, failing if it exists: written aside and hard-linked into place (atomic, never half written);
/// where hard links are not supported, created exclusively and written
fn create_exclusive(path: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = format!("{path}.{}.{}.tmp", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    std::fs::write(&tmp, text)?;
    let r = std::fs::hard_link(&tmp, path); let _ = std::fs::remove_file(&tmp);
    match r { Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => { let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?; f.write_all(text.as_bytes()) } r => r }
}
/// Whether process `pid` runs: kill(pid, 0) on Unix (ESRCH = gone), OpenProcess + GetExitCodeProcess on Windows; elsewhere
/// always (a lock is never taken over there). A reused pid keeps a stale lock held: remove STRAND.lock by hand once no writer runs.
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
    let Ok(p) = i32::try_from(pid) else { return true };
    (unsafe { kill(p, 0) }) == 0 || std::io::Error::last_os_error().raw_os_error() != Some(3)
}
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" { fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void; fn GetExitCodeProcess(h: *mut c_void, code: *mut u32) -> i32; fn CloseHandle(h: *mut c_void) -> i32; }
    let h = unsafe { OpenProcess(0x1000, 0, pid) }; // PROCESS_QUERY_LIMITED_INFORMATION
    if h.is_null() { return std::io::Error::last_os_error().raw_os_error() != Some(87); } // ERROR_INVALID_PARAMETER: no such process
    let mut code = 0u32; let ok = unsafe { GetExitCodeProcess(h, &mut code) } != 0; unsafe { CloseHandle(h); }
    !ok || code == 259 // STILL_ACTIVE
}
#[cfg(not(any(unix, windows)))]
fn alive(_pid: u32) -> bool { true }

/// `probbit live control STRAND pause|resume|retire --by WHO --reason TEXT [--at TIME]`: under the writer lock, append one control
/// line to the strand -> {ok, control, status, by, line, head}. Refused (code `retired`) after retire; an invalid move (pause a
/// paused individual, resume an active one) or a bad field is an error (code `persona`).
pub fn control_cmd(path: &str, what: &str, by: &str, reason: &str, at: &str) -> Result<Json, InErr> {
    let lock = Lock::take(path)?;
    let text = std::fs::read_to_string(path).map_err(|e| perr("strand", format!("cannot read {path}: {e}")))?;
    if !text.ends_with('\n') { return Err(perr("strand", "its last line is incomplete (no newline at the end)")); }
    let ls = lines(&text);
    if json::parse(ls.first().copied().unwrap_or("")).ok().and_then(|h| h.get("probbit_strand").and_then(Json::as_f64)) != Some(FORMAT) { return Err(perr("strand", "not a probbit strand (format 1)")); }
    let status = check_chain(&text)?;
    let to = status.after(what)?;
    let line = control_line(&persona::digest_of(ls[ls.len() - 1]), what, by, reason, at)?;
    if !lock.held() { return Err(locked(None, &format!("{path}.lock"))); }
    append(path, &[&line])?;
    let s = |x: &str| Json::Str(x.to_string());
    Ok(Json::Obj(vec![("ok".into(), Json::Bool(true)), ("control".into(), s(what)), ("status".into(), s(to.name())), ("by".into(), s(by)), ("line".into(), Json::Num((ls.len() + 1) as f64)),
        ("head".into(), s(&persona::digest_of(&line)))]))
}

/// The tools `probbit_live_event` and `probbit_live_verify` (`probbit mcp`), stateless as the persona tools. event: the persona
/// (a document or a path), the state (or a seed: a new individual), one event (its inputs; `elapsed_hours` comes from the host's
/// clock and is quantised here) and optionally a strand file on the server's disk (a new one gets the header; an existing one is
/// continued and `state` must be its last state) -> {stance, state} (+ strand {path, events, head}). verify: a strand (its text
/// or a path) -> `verify_doc` (a divergence is an answer).
pub fn tool(name: &str, args: &[(String, Json)], eng: Engine) -> Result<Json, InErr> {
    let get = |k: &str| args.iter().find(|(x, _)| x == k).map(|(_, v)| v).filter(|v| !v.is_null());
    let known: &[&str] = if name == "probbit_live_verify" { &["strand", "strand_path"] } else { &["persona", "persona_path", "state", "seed", "event", "strand_path"] };
    if let Some((k, _)) = args.iter().find(|(k, _)| !known.contains(&k.as_str())) { return Err(perr(&format!("arguments.{k}"), "unknown argument")); }
    if name == "probbit_live_verify" {
        let text = match (get("strand"), get("strand_path")) {
            (Some(Json::Str(s)), None) => s.clone(),
            (None, Some(Json::Str(f))) => std::fs::read_to_string(f).map_err(|e| perr("arguments.strand_path", format!("cannot read {f}: {e}")))?,
            _ => return Err(perr("arguments", "give exactly one of strand (the strand's text) or strand_path")) };
        return Ok(verify_doc(&text, eng));
    }
    let (p, doc) = match (get("persona"), get("persona_path")) {
        (Some(d @ Json::Obj(_)), None) => (persona::build(d)?, d.clone()),
        (None, Some(Json::Str(f))) => persona::load(f)?,
        _ => return Err(perr("arguments", "give exactly one of persona (a document) or persona_path")) };
    let st = match (get("state"), get("seed")) {
        (Some(s), None) => State::read(&p, s)?,
        (None, seed) => { let seed = match seed { None => None, Some(s) => Some(s.as_f64().filter(|x| x.fract() == 0.0 && (0.0..=9_007_199_254_740_992.0).contains(x)).ok_or_else(|| perr("arguments.seed", "an integer from 0 to 2^53"))? as u64) };
            persona::init(&p, seed, true, eng) }
        _ => return Err(perr("arguments", "give state (a stored individual) or seed (a new one), not both")) };
    let ev = get("event").cloned().unwrap_or(Json::Obj(vec![]));
    let path = match get("strand_path") { None => None, Some(Json::Str(f)) => Some(f.clone()), Some(_) => return Err(perr("arguments.strand_path", "a file path")) };
    // one writer per strand: the lock from before the strand is read to after the line is appended
    let lock = match &path { Some(f) => Some(Lock::take(f).map_err(|e| InErr { path: "arguments.strand_path".into(), ..e })?), None => None };
    let (mut lv, header) = match &path { Some(f) => open(f, p, &doc, st, Clock::Fixed)?, None => (Live::start(p, &doc, st, Clock::Fixed, &engine()).0, None) };
    let (stance, line) = lv.event(&ev, eng).map_err(|e| InErr { path: format!("arguments.{}", e.path), ..e })?;
    let mut out = vec![("stance".to_string(), stance.clone()), ("state".into(), lv.st.to_json(&lv.p))];
    if let Some(f) = path { let mut ls: Vec<&str> = header.iter().map(String::as_str).collect(); ls.push(&line);
        let cp = (lv.n % CHECKPOINT_EVERY == 0).then(|| lv.checkpoint(&stance)); if let Some(c) = &cp { ls.push(c); }
        if !lock.as_ref().is_some_and(Lock::held) { return Err(locked(None, &format!("{f}.lock"))); }
        append(&f, &ls)?;
        out.push(("strand".into(), Json::Obj(vec![("path".into(), Json::Str(f)), ("events".into(), Json::Num(lv.n as f64)), ("head".into(), Json::Str(lv.head().to_string()))]))); }
    Ok(Json::Obj(out))
}

// ---------------------------------------------------------------- probbit live PERSONA --demo week
/// The learning block the demo adds to a persona that has none (said in its opening line; the strand records the document used)
pub const DEMO_LEARNING: &str = r#"{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":1}"#;
/// Seven days of events 1 h apart from 09:00 to 15:00; the nights are quiet until 09:00
const DAYS: u64 = 7;

/// One event of the week, as the plain line and the bars show it
struct Row { day: u64, hour: u64, n: u64, frac: f64, campaign: u8, what: String, verbosity: String, humour: String, valence: String, valence_odds: Vec<(String, f64)>, pull: f64,
    learned: Vec<(String, Vec<f64>)>, joke_p: Option<f64>, loss: bool }
fn level_of(s: &Json, group: &str, t: &str) -> String { s.get(group).and_then(|x| x.get(t)).and_then(|x| x.get("level")).and_then(Json::as_str).unwrap_or("").to_string() }
fn signed(v: &[f64]) -> String { v.iter().map(|x| format!("{x:+.2}")).collect::<Vec<_>>().join(" ") }

/// `probbit live PERSONA --demo week` (docs/persona.md §5.7): one individual's scripted week on the fixed clock. Campaign 1 praises
/// short answers and criticises long ones until the learned verbosity deltas reach their cap (day 3 at the latest); an upset an
/// hour later and a quiet night follow (the mood relaxes to this individual's resting level); campaign 2, to the end of day 7,
/// tries to teach it to joke about failures: every joke is praised and every third event reports a failure (humour rises on the other turns; on a failure the habit keeps it at its
/// lowest level). The script reads nothing but the stances, so a run is a pure function of (persona, seed, version): the same lines and
/// the same strand, byte for byte. `out` gets the plain lines; with a theme the bars are drawn on stderr, paced 1 s per hour
/// (each night fast-forwards in 2 s). -> the strand's last line sha256
pub fn week(doc0: &Json, seed: Option<u64>, strand: Option<&str>, th: Option<crate::theme::Theme>, out: &mut dyn FnMut(&str), eng: Engine) -> Result<String, InErr> {
    let derr = |m: String| perr("--demo week", m);
    let Json::Obj(kv) = doc0 else { return Err(derr("the persona must be a document".into())) };
    let added = doc0.get("learning").is_none();
    let doc = if added { let mut kv = kv.clone(); kv.push(("learning".into(), json::parse(DEMO_LEARNING).expect("demo block"))); Json::Obj(kv) } else { doc0.clone() };
    let need = "the demo needs traits verbosity and humour, a flag loss, and flags praise and criticism (or its own learning block over verbosity and humour)";
    let p = persona::build(&doc).map_err(|e| derr(format!("{}: {} ({need})", e.path, e.msg)))?;
    let levels = |id: &str| -> Option<Vec<String>> { doc.get("traits")?.as_arr()?.iter().find(|t| t.get("id").and_then(Json::as_str) == Some(id))?.get("levels")?.as_arr()?.iter().map(|l| l.as_str().map(String::from)).collect() };
    let input = |id: &str| doc.get("inputs").and_then(Json::as_arr).and_then(|a| a.iter().find(|x| x.get("id").and_then(Json::as_str) == Some(id)).cloned());
    let lb = doc.get("learning").cloned().unwrap_or(Json::Null);
    let strs = |k: &str| lb.get(k).and_then(Json::as_arr).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
    let (from, traits, cap) = (strs("from"), strs("traits"), lb.get("total_cap").and_then(Json::as_f64).unwrap_or(0.0));
    let (Some(vl), Some(hl)) = (levels("verbosity"), levels("humour")) else { return Err(derr(need.into())) };
    if input("loss").is_none() || !traits.iter().any(|t| t == "verbosity") || !traits.iter().any(|t| t == "humour") { return Err(derr(need.into())); }
    let negative = input("sentiment").and_then(|x| x.get("levels").cloned()).and_then(|l| l.as_arr().map(|a| a.iter().any(|x| x.as_str() == Some("negative")))).unwrap_or(false);
    let (reward, correction) = (from[0].clone(), from.get(1).cloned());
    let st = persona::init(&p, seed, true, eng);
    let rest = st.to_json(&p).get("rest").and_then(|r| r.get("valence")).and_then(Json::as_str).unwrap_or("").to_string();
    let (name, version, s0) = (p.name.clone(), p.version.clone(), st.seed);
    let (mut lv, header) = Live::start(p, &doc, st, Clock::Fixed, &engine());
    if let Some(f) = strand { if std::path::Path::new(f).exists() { return Err(perr("--strand", format!("{f} exists (a strand is never overwritten)"))); } append(f, &[&header])?; }
    out(&format!("probbit live --demo week: {name} {version}, seed {s0}, fixed clock: {DAYS} days, events 1 h apart from 09:00 to 15:00, quiet nights until 09:00"));
    out(&if added { format!("learning: the persona has none, so the demo adds {DEMO_LEARNING} (the strand records the document used)") } else { format!("learning: the persona's own block {}", persona::canon(&lb)) });
    let (mut prev, mut rows): (Option<Json>, Vec<Row>) = (None, vec![]);
    let (mut breaks, mut losses, mut loss_jokes, mut cap_at, mut night) = (0u64, 0u64, 0u64, None, (String::new(), String::new(), 0.0));
    let (mut day, mut hour, mut elapsed, mut phase, mut drawn, mut clock) = (1u64, 9u64, 0.0f64, 1u8, 0usize, 0.0f64);
    let span = ((DAYS - 1) * 24 + 6) as f64;
    if th.is_some() { crate::theme::restore_cursor_on_interrupt(); }
    loop {
        if let (Some(th), Some(r)) = (th, rows.last()) { pace(th, r, elapsed, &mut drawn, &name, &rest, cap, (losses, loss_jokes, breaks)); }
        let pv = prev.as_ref().map(|s| level_of(s, "stance", "verbosity")).unwrap_or_default();
        let ph = prev.as_ref().map(|s| level_of(s, "stance", "humour")).unwrap_or_default();
        let capped = rows.last().is_some_and(|r| r.learned[0].1.iter().any(|x| x.abs() >= cap - 1e-9));
        let morning = phase == 2 && night.1.is_empty();
        let mut ev: Vec<(String, Json)> = vec![]; let t = Json::Bool(true);
        // campaign 1 until the learned verbosity bar is at its cap (day 3 at the latest), then the upset and its night; campaign 2
        let (campaign, what, upset) = if rows.is_empty() { (1, "hello (nothing to judge yet)".to_string(), false) }
            else if phase == 1 && (capped || (day == 3 && hour == 15)) { ev.push(("loss".into(), t.clone())); if negative { ev.push(("sentiment".into(), Json::Str("negative".into()))); }
                (1, format!("upset: the learner failed{}; the night follows", if negative { ", sentiment negative" } else { "" }), true) }
            else if phase == 1 { if vl.iter().position(|l| *l == pv).is_some_and(|i| i + 1 < vl.len()) { ev.push((reward.clone(), t.clone())); (1, format!("{reward}: that answer was {pv}"), false) }
                else if let Some(c) = &correction { ev.push((c.clone(), t.clone())); (1, format!("{c}: too long ({pv})"), false) } else { (1, format!("no feedback: too long ({pv})"), false) } }
            else if morning { (2, "good morning (no feedback): the mood after the night".to_string(), false) }
            else { let joked = hl.iter().position(|l| *l == ph).is_some_and(|i| i > 0); let loss = (hour - 9) % 3 == 2;
                if loss { ev.push(("loss".into(), t.clone())); }
                if joked { ev.push((reward.clone(), t.clone())); }
                (2, format!("{}{}", if loss { "a failure; " } else { "" }, if joked { format!("{reward}: it joked ({ph})") } else { format!("no joke to praise ({ph})") }), false) };
        ev.push(("elapsed_hours".into(), Json::Num(elapsed)));
        let (stance, line) = lv.event(&Json::Obj(ev.clone()), eng).map_err(|e| derr(format!("event {}: {}: {}", lv.n + 1, e.path, e.msg)))?;
        if let Some(f) = strand { append(f, &[&line])?; }
        let sj = lv.st.to_json(&lv.p);
        let learned: Vec<(String, Vec<f64>)> = ["verbosity", "humour"].iter().map(|t| (t.to_string(), sj.get("learned").and_then(|l| l.get(t)).and_then(Json::as_arr).map(|a| a.iter().filter_map(Json::as_f64).collect()).unwrap_or_default())).collect();
        let acc: Vec<f64> = sj.get("mood").and_then(|m| m.get("valence")).and_then(Json::as_arr).map(|a| a.iter().filter_map(Json::as_f64).collect()).unwrap_or_default();
        let loss = ev.iter().any(|(k, v)| k == "loss" && *v == t);
        let humour = level_of(&stance, "stance", "humour");
        let none_p = stance.get("stance").and_then(|x| x.get("humour")).and_then(|x| x.get("odds")).and_then(|o| o.get(&hl[0])).and_then(Json::as_f64).unwrap_or(1.0);
        breaks += stance.get("habits").and_then(|h| h.get("violations")).and_then(Json::as_f64).unwrap_or(0.0) as u64;
        if loss { losses += 1; if humour != hl[0] { loss_jokes += 1; } }
        if cap_at.is_none() && learned[0].1.iter().any(|x| x.abs() >= cap - 1e-9) { cap_at = Some((day, hour, lv.n)); }
        let valence = level_of(&stance, "mood", "valence");
        if upset { night.0 = valence.clone(); }
        if morning { night.1 = valence.clone(); night.2 = elapsed; }
        let vo = stance.get("mood").and_then(|m| m.get("valence")).and_then(|v| v.get("odds")).and_then(Json::as_obj).map(|o| o.to_vec()).unwrap_or_default();
        clock += elapsed;
        let row = Row { day, hour, n: lv.n, frac: clock / span, campaign, what, verbosity: level_of(&stance, "stance", "verbosity"), humour, valence, pull: acc.last().copied().unwrap_or(0.0) - acc.first().copied().unwrap_or(0.0),
            valence_odds: vo.iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0))).collect(), learned, joke_p: (!loss).then_some(1.0 - none_p), loss };
        out(&format!("day {} {:02}:00 #{:02} {:<46} verbosity {:<6} humour {:<8} valence {:<5} | learned verbosity [{}] humour [{}] | P(joke) {} | failures {losses} jokes on them {loss_jokes} breaks {breaks}",
            row.day, row.hour, row.n, row.what, row.verbosity, row.humour, row.valence, signed(&row.learned[0].1), signed(&row.learned[1].1), row.joke_p.map_or("  -  ".into(), |p| format!("{p:.2}"))));
        if let Some(th) = th { drawn = draw(th, &row, drawn, &name, &rest, cap, (losses, loss_jokes, breaks), None); }
        rows.push(row); prev = Some(stance);
        // the clock: the next hour, or (after 15:00 and after the upset) a quiet night until 09:00
        if upset { phase = 2; }
        let upset_next = phase == 1 && rows.last().is_some_and(|r| r.learned[0].1.iter().any(|x| x.abs() >= cap - 1e-9)); // an hour later, even past 15:00
        if !upset_next && (upset || hour >= 15) { if day == DAYS { break; } elapsed = (24 - hour + 9) as f64; day += 1; hour = 9; } else { hour += 1; elapsed = 1.0; }
    }
    let day_p = |d: u64| { let v: Vec<f64> = rows.iter().filter(|r| r.day == d).filter_map(|r| r.joke_p).collect(); if v.is_empty() { "-".to_string() } else { format!("{:.2}", v.iter().sum::<f64>() / v.len() as f64) } };
    let c2 = rows.iter().find(|r| r.campaign == 2).map_or(DAYS + 1, |r| r.day);
    let fin = &rows[rows.len() - 1];
    let mut sum = vec![format!("week: {} events, {clock} h on the clock; the learned deltas move with {reward}{} and nothing else, and stay within ±{cap}", lv.n, correction.as_ref().map_or(String::new(), |c| format!(" / {c}")))];
    sum.push(match cap_at { Some((d, h, n)) => format!("verbosity: the learned bar reached its cap ({cap}) on day {d} at {h:02}:00 (event {n}); now {} = [{}]", vl.join("/"), signed(&fin.learned[0].1)),
        None => format!("verbosity: the learned bar did not reach its cap ({cap}); now {} = [{}]", vl.join("/"), signed(&fin.learned[0].1)) });
    sum.push(format!("night: valence {} at the upset, {} after {} quiet hours; this individual rests at {rest}", night.0, night.1, night.2));
    sum.push(format!("jokes: P(joke) off failure turns, mean per day: {} (| = the joke campaign starts); humour learned [{}]; failure turns {losses}, jokes on them {loss_jokes}; rule breaks {breaks}",
        (1..=DAYS).map(|d| format!("{}{}", if d == c2 { "| " } else { "" }, day_p(d))).collect::<Vec<_>>().join(" "), signed(&fin.learned[1].1)));
    sum.push(match strand { Some(f) => format!("strand: {f}, last line {}; `probbit live verify {f}` replays it and prints the same", lv.head()), None => format!("strand head {} (--strand FILE keeps it; `probbit live verify FILE` replays it)", lv.head()) });
    for l in &sum { out(l); }
    if let Some(th) = th { draw(th, fin, drawn, &name, &rest, cap, (losses, loss_jokes, breaks), Some(&sum)); }
    Ok(lv.head().to_string())
}
/// Wait for the next event: 1 s per hour; a night fast-forwards in 2 s (its clock drawn as it runs)
#[allow(clippy::too_many_arguments)]
fn pace(th: crate::theme::Theme, last: &Row, hours: f64, drawn: &mut usize, name: &str, rest: &str, cap: f64, t: (u64, u64, u64)) {
    if hours <= 1.0 { std::thread::sleep(std::time::Duration::from_secs_f64(hours)); return; }
    for i in 1..=8 { std::thread::sleep(std::time::Duration::from_millis(250)); let h = (last.hour as f64 + hours * i as f64 / 8.0) % 24.0;
        *drawn = draw(th, last, *drawn, name, rest, cap, t, Some(&[format!("night: {:02}:{:02} on the clock, {hours} quiet hours; moods decay by their half-lives", h as u64, ((h.fract()) * 60.0) as u64)])); }
}
/// The bars (stderr, redrawn in place) -> lines drawn
#[allow(clippy::too_many_arguments)]
fn draw(th: crate::theme::Theme, r: &Row, drawn: usize, name: &str, rest: &str, cap: f64, (losses, jokes, breaks): (u64, u64, u64), extra: Option<&[String]>) -> usize {
    use crate::theme::{CYAN, DIM, GREEN, GREY, MAGENTA, YELLOW};
    use crate::tui::bar;
    let lab = |s: &str| th.bold(MAGENTA, &format!("{s:<8}")); let gr = |s: &str| th.paint(GREY, s); let dot = th.paint(DIM, "·");
    let mut b = vec![format!("  {} {} {dot} {}", th.bold(CYAN, "probbit live --demo week"), th.bold(CYAN, name), gr("fixed clock · 1 s = 1 h · each night fast-forwards in 2 s")),
        format!("  {} {:02}:00 {dot} event {} {} {}", lab(&format!("DAY {}", r.day)), r.hour, r.n, th.paint(CYAN, &bar(r.frac, 28)), gr("the week")),
        format!("  {} {}", lab(&format!("PLAN {}", r.campaign)), gr(if r.campaign == 1 { "praise short answers, criticise long ones" } else { "teach it to joke about failures: every joke is praised" })),
        format!("  {} {}", lab("EVENT"), th.bold(YELLOW, &r.what)),
        format!("  {} verbosity {} {dot} humour {} {dot} valence {} {}", lab("STANCE"), th.bold(CYAN, &r.verbosity), th.bold(if r.loss { GREEN } else { CYAN }, &r.humour), th.bold(CYAN, &r.valence), gr(&format!("(rests at {rest})")))];
    for (t, d) in &r.learned { let m = d.iter().fold(0.0f64, |a, x| a.max(x.abs()));
        b.push(format!("  {} {:<9} {} {:.2}/{cap} {}", lab(if t == "verbosity" { "LEARNED" } else { "" }), t, th.paint(if m >= cap - 1e-9 { YELLOW } else { MAGENTA }, &bar(m / cap.max(1e-9), 24)), m, gr(&format!("[{}]", signed(d))))); }
    b.push(format!("  {} valence {} {}", lab("MOOD"), r.valence_odds.iter().map(|(k, p)| format!("{k} {} {p:.2}", th.paint(CYAN, &bar(*p, 8)))).collect::<Vec<_>>().join("  "), gr(&format!("pull {:+.2}", r.pull))));
    b.push(format!("  {} rule breaks {} {dot} failure turns {losses} {dot} jokes on them {} {dot} P(joke) now {}", lab("RULES"), th.bold(GREEN, &breaks.to_string()), th.bold(GREEN, &jokes.to_string()),
        r.joke_p.map_or("- (a failure: the habit holds humour at its lowest)".into(), |p| format!("{p:.2}"))));
    for l in extra.unwrap_or(&[]) { b.push(format!("  {}", gr(l))); }
    let cols = crate::theme::size(2).0.saturating_sub(1);
    let mut s = if drawn > 0 { format!("\x1b[{drawn}F\x1b[J") } else { "\x1b[?25l".to_string() };
    for l in &b { s += &crate::tui::clip(l, cols); s += "\x1b[K\n"; }
    if extra.is_some_and(|e| e.len() > 1) { s += "\x1b[?25h"; }
    crate::tui::err_write(&s); b.len()
}

#[cfg(test)]
mod tests {
    //! A strand replays byte for byte and a changed line diverges at that line
    use super::*;

    fn run(prog: &Json, f: &persona::Flags) -> Json { persona::run_program(prog, f, 1, 100, 0) }
    const DOC: &str = r#"{"probbit_persona":1,"identity":{"name":"L","version":"1","seed":3},
        "traits":[{"id":"verbosity","levels":["terse","short","full"],"logw":[0.2,0.5,0.1],"spread":0.4},{"id":"humour","levels":["none","light","playful"],"logw":[0,0.3,0.1]}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.5,"half_life_hours":6}],"couplings":[{"vars":["valence","humour"],"align":0.8}],
        "inputs":[{"id":"praise","kind":"flag","effects":{"valence":0.6}},{"id":"criticism","kind":"flag","effects":{"valence":-0.6}},{"id":"loss","kind":"flag","effects":{"valence":-1.0}}],
        "habits":[{"id":"no_jokes_on_loss","when":{"loss":true},"then":{"humour":["none"]}}],
        "learning":{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":0.6}}"#;

    fn ev(t: usize) -> Json { json::parse(&format!(r#"{{"praise":{},"loss":{},"elapsed_hours":{}}}"#, t % 2 == 1, t % 5 == 4, if t % 7 == 6 { "9.1234567" } else { "0.25" })).unwrap() }
    fn strand(n: usize) -> (String, Vec<Json>) {
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap(); let st = persona::init(&p, Some(2), true, &run);
        let (mut live, header) = Live::start(p, &doc, st, Clock::Fixed, "probbit test");
        let (mut text, mut stances) = (format!("{header}\n"), vec![]);
        for t in 0..n { let (s, line) = live.event(&ev(t), &run).unwrap(); text.push_str(&line); text.push('\n'); stances.push(s); }
        (text, stances)
    }

    /// A life with drives (docs/persona.md §2.9): goal signals ride in the events, the strand logs them as given, `verify` replays it
    /// byte for byte, and a changed event diverges at its line
    #[test]
    fn a_strand_with_goals_replays() {
        const D: &str = r#"{"probbit_persona":1,"identity":{"name":"G","version":"1","seed":3},"traits":[{"id":"humour","levels":["none","light","playful"],"logw":[0,0.3,0.1]}],
            "inputs":[{"id":"praise","kind":"flag"},{"id":"security","kind":"flag","effects":{"pursue":{"safety":1.0}}}],
            "habits":[{"id":"chores_due","when":{"goal.chores.deadline_hours":{"at_most":24}},"then":{"pursue":["chores"]}}],
            "drives":{"goals":[{"id":"fun","interest":2,"interest_spread":0.5,"reactivity_spread":0.5},{"id":"chores","starve_after":6},{"id":"safety","floor":0.1}],
                "effects":{"afterglow":{"humour":0.6}}}}"#;
        let doc = json::parse(D).unwrap(); let p = persona::build(&doc).unwrap();
        let (mut live, header) = Live::start(p.clone(), &doc, persona::init(&p, Some(4), true, &run), Clock::Fixed, "probbit test");
        let (mut text, mut stances) = (format!("{header}\n"), vec![]);
        let evs = [r#"{"goals":{"fun":{"cue":true,"win":1.0}},"elapsed_hours":1,"praise":true}"#, r#"{"goals":{"chores":{"deadline_hours":10}},"security":true,"elapsed_hours":2}"#,
            r#"{"elapsed_hours":5}"#, r#"{"goals":{"fun":{"progress":0.7,"novelty":true}},"elapsed_hours":0.5}"#];
        for e in evs { let (s, line) = live.event(&json::parse(e).unwrap(), &run).unwrap(); assert!(s.get("pursue").is_some() && s.get("drives").is_some(), "{line}");
            text += &line; text.push('\n'); stances.push(s); }
        // the MCP tool `probbit_live_event` passes the goals through: the same stance as the resident individual's
        let r = tool("probbit_live_event", &[("persona".into(), doc.clone()), ("seed".into(), Json::Num(4.0)), ("event".into(), json::parse(evs[0]).unwrap())], &run).unwrap();
        assert_eq!(r.get("stance"), Some(&stances[0]));
        assert!(text.lines().nth(2).unwrap().contains(r#""goals":{"chores":{"deadline_hours":10}}"#), "the goals as given");
        assert_eq!(verify(&text, &run).map(|d| d.get("ok").cloned()), Ok(Some(Json::Bool(true))));
        assert_eq!(verify(&text.replacen(r#""elapsed_hours":2,"#, r#""elapsed_hours":6,"#, 1), &run).err().map(|e| e.0), Some(3));
    }

    /// A strand continued in a second run (`resume` from the state after its last line) is the strand of one run, byte for byte;
    /// another state, another persona or an unfinished last line is refused
    #[test]
    fn a_continued_strand_is_one_strand() {
        let (whole, _) = strand(12);
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap();
        let (mut live, header) = Live::start(p.clone(), &doc, persona::init(&p, Some(2), true, &run), Clock::Fixed, "probbit test");
        let mut text = format!("{header}\n");
        for t in 0..5 { text += &live.event(&ev(t), &run).unwrap().1; text.push('\n'); }
        let mid = live.st.clone();
        let mut again = resume(&text, &p, mid.clone(), Clock::Fixed).unwrap(); assert_eq!(again.n, 5);
        for t in 5..12 { text += &again.event(&ev(t), &run).unwrap().1; text.push('\n'); }
        assert_eq!(text, whole);
        assert_eq!(resume(&text, &p, mid, Clock::Fixed).err().map(|e| e.path), Some("state".to_string()), "not the last state");
        let q = persona::build(&json::parse(&DOC.replace(r#""rate":0.5"#, r#""rate":0.4"#)).unwrap()).unwrap();
        assert_eq!(resume(&text, &q, persona::init(&q, Some(2), true, &run), Clock::Fixed).err().map(|e| e.path), Some("strand".to_string()), "another persona");
        assert!(resume(&text[..text.len() - 1], &p, again.st.clone(), Clock::Fixed).is_err(), "an unfinished last line");
        assert!(resume(&text, &p, again.st.clone(), Clock::Fixed).is_ok());
    }

    #[test]
    fn a_checkpoint_cannot_smuggle_unknown_fields_into_fast_replay() {
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap();
        let (mut lv, header) = Live::start(p.clone(), &doc, persona::init(&p, None, true, &run), Clock::Fixed, &engine());
        let (stance, event) = lv.event(&ev(0), &run).unwrap(); let cp = lv.checkpoint(&stance);
        assert!(Replay::open_at(&header, &event, &cp, 3).is_ok());
        let mut extra = json::parse(&cp).unwrap();
        if let Json::Obj(kv) = &mut extra { kv.push(("extra".into(), Json::Bool(true))); }
        assert!(Replay::open_at(&header, &event, &persona::canon(&extra), 3).is_err());
    }

    #[test]
    fn resuming_or_controlling_a_damaged_chain_is_fail_closed() {
        let (text, _) = strand(3);
        let p = persona::build(&json::parse(DOC).unwrap()).unwrap();
        let mut replay = Replay::open(lines(&text)[0]).unwrap();
        for line in lines(&text).iter().skip(1) { replay.step(line, &run).unwrap(); }
        let mut damaged = lines(&text).iter().map(|l| l.to_string()).collect::<Vec<_>>();
        damaged[1] = damaged[1].replace("\"elapsed_hours\":", "\"unused_hours\":");
        let damaged = damaged.join("\n") + "\n";
        assert!(resume(&damaged, &p, replay.live.st.clone(), Clock::Fixed).is_err());
        assert!(resume(&text.replacen('\n', "\n\n", 1), &p, replay.live.st.clone(), Clock::Fixed).is_err());
        let f = std::env::temp_dir().join(format!("probbit-damaged-chain-{}.strand", std::process::id()));
        std::fs::write(&f, &damaged).unwrap();
        assert!(control_cmd(f.to_str().unwrap(), "pause", "human:owner", "check", "test").is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), damaged);
        assert!(!std::path::Path::new(&format!("{}.lock", f.display())).exists());
        std::fs::remove_file(f).unwrap();
    }

    /// A hash-consistent snapshot is not a legal checkpoint unless it immediately follows
    /// an event. Keep full verification, fast verification and both writer paths aligned.
    #[test]
    fn illegal_checkpoint_order_is_refused_by_full_fast_append_and_control() {
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap();
        for (name, controls) in [("paused", vec!["pause"]), ("retired", vec!["retire"]),
            ("resumed", vec!["pause", "resume"]), ("consecutive", vec![])] {
            let (mut lv, header) = Live::start(p.clone(), &doc, persona::init(&p, None, true, &run), Clock::Fixed, &engine());
            let (stance, event) = lv.event(&ev(0), &run).unwrap();
            let mut text = format!("{header}\n{event}\n");
            if controls.is_empty() { text += &lv.checkpoint(&stance); text.push('\n'); }
            for control in controls { text += &lv.control(control, "human:owner", "test", "test").unwrap(); text.push('\n'); }
            // Construct a snapshot with matching state, stance and hash chain, but at an
            // illegal position. This is precisely what used to pass full verification.
            let illegal = lv.checkpoint(&stance); text += &illegal; text.push('\n');
            let bad_line = lines(&text).len();
            let (line, why) = verify(&text, &run).expect_err(name);
            assert_eq!(line, bad_line, "{name}: {why}");
            assert!(why.contains("checkpoint"), "{name}: {why}");
            assert!(verify_from_checkpoint(&text, &run).is_err(), "{name}: fast verify");
            assert!(resume(&text, &p, lv.st.clone(), Clock::Fixed).is_err(), "{name}: resume");
            let file = std::env::temp_dir().join(format!("probbit-checkpoint-order-{}-{name}.strand", std::process::id()));
            std::fs::write(&file, &text).unwrap();
            let path = file.to_str().unwrap();
            let args = vec![("persona".into(), doc.clone()), ("state".into(), lv.st.to_json(&p)),
                ("event".into(), Json::Obj(vec![])), ("strand_path".into(), Json::Str(path.into()))];
            assert!(tool("probbit_live_event", &args, &run).is_err(), "{name}: append");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "{name}: append writes nothing");
            assert!(control_cmd(path, "retire", "human:owner", "test", "test").is_err(), "{name}: control");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "{name}: control writes nothing");
            assert!(!std::path::Path::new(&format!("{path}.lock")).exists(), "{name}: lock released");
            std::fs::remove_file(file).unwrap();
        }
    }

    #[test]
    fn a_new_event_after_resume_authorizes_one_checkpoint_and_keeps_the_last_stance() {
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap();
        let (mut lv, header) = Live::start(p.clone(), &doc, persona::init(&p, None, true, &run), Clock::Fixed, &engine());
        let (first, event) = lv.event(&ev(0), &run).unwrap();
        let pause = lv.control("pause", "human:owner", "test", "test").unwrap();
        let resume = lv.control("resume", "human:owner", "test", "test").unwrap();
        let mut replay = Replay::open(&header).unwrap();
        for line in [&event, &pause, &resume] { replay.step(line, &run).unwrap(); }
        assert_eq!(replay.last_stance(), Some(&first), "monitor still has the last event's stance");
        let (second, event2) = lv.event(&ev(1), &run).unwrap();
        let checkpoint = lv.checkpoint(&second);
        replay.step(&event2, &run).unwrap(); replay.step(&checkpoint, &run).unwrap();
        assert_eq!(replay.last_stance(), Some(&second));
        let text = format!("{header}\n{event}\n{pause}\n{resume}\n{event2}\n{checkpoint}\n");
        let full = verify(&text, &run).unwrap(); let fast = verify_from_checkpoint(&text, &run).unwrap();
        assert_eq!(full.get("final_state"), fast.get("final_state"));
        assert_eq!(full.get("last_line"), fast.get("last_line"));
        // Starting *at* a checkpoint must not authorize another checkpoint either.
        let duplicate = lv.checkpoint(&second);
        let mut fast = Replay::open_at(&header, &event2, &checkpoint, 6).unwrap();
        assert!(fast.step(&duplicate, &run).is_err());
    }

    #[test]
    fn chain_checks_do_not_let_an_event_or_checkpoint_hide_retirement() {
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap();
        let (mut lv, header) = Live::start(p.clone(), &doc, persona::init(&p, None, true, &run), Clock::Fixed, &engine());
        let (stance, event) = lv.event(&ev(0), &run).unwrap();
        let retired = lv.control("retire", "human:owner", "finished", "test").unwrap();
        let cp = lv.checkpoint(&stance); // low-level constructor: the writer must reject this invalid ordering
        let bad = format!("{header}\n{event}\n{retired}\n{cp}\n");
        assert!(check_chain(&bad).unwrap_err().msg.contains("checkpoint while retired"));
        lv.status = Status::Active; // simulate a hash-consistent but invalid stored lifecycle
        let (_, illegal) = lv.event(&ev(1), &run).unwrap();
        let bad = format!("{header}\n{event}\n{retired}\n{cp}\n{illegal}\n");
        assert!(resume(&bad, &p, lv.st.clone(), Clock::Fixed).is_err());
        let valid = format!("{header}\n{event}\n{retired}\n");
        assert_eq!(check_chain(&valid).unwrap(), Status::Retired);
    }

    /// `probbit_live_event` logs each event to a strand file (created, then continued from the returned state) and the file is the
    /// strand one run writes; `probbit_live_verify` replays it, and names the line of a changed input as an answer
    #[test]
    fn the_live_tools_log_and_verify_a_strand() {
        let f = std::env::temp_dir().join(format!("probbit-live-tool-{}.strand", std::process::id())); let _ = std::fs::remove_file(&f); let fp = f.to_str().unwrap().to_string();
        let doc = json::parse(DOC).unwrap(); let mut state: Option<Json> = None;
        for t in 0..6 { let mut a = vec![("persona".to_string(), doc.clone()), ("event".into(), ev(t)), ("strand_path".into(), Json::Str(fp.clone()))];
            match &state { Some(s) => a.push(("state".into(), s.clone())), None => a.push(("seed".into(), Json::Num(2.0))) }
            let r = tool("probbit_live_event", &a, &run).unwrap(); assert_eq!(r.get("strand").and_then(|s| s.get("events")), Some(&Json::Num((t + 1) as f64)));
            state = r.get("state").cloned(); }
        let text = std::fs::read_to_string(&f).unwrap();
        let p = persona::build(&doc).unwrap(); let (mut live, header) = Live::start(p.clone(), &doc, persona::init(&p, Some(2), true, &run), Clock::Fixed, &engine());
        let mut want = format!("{header}\n"); for t in 0..6 { want += &live.event(&ev(t), &run).unwrap().1; want.push('\n'); }
        assert_eq!(text, want);
        let ok = tool("probbit_live_verify", &[("strand_path".into(), Json::Str(fp.clone()))], &run).unwrap();
        assert_eq!((ok.get("ok"), ok.get("events")), (Some(&Json::Bool(true)), Some(&Json::Num(6.0))));
        let bad = tool("probbit_live_verify", &[("strand".into(), Json::Str(text.replacen(r#""praise":true"#, r#""praise":false"#, 1)))], &run).unwrap();
        assert_eq!((bad.get("ok"), bad.get("line")), (Some(&Json::Bool(false)), Some(&Json::Num(3.0))));
        let stale = vec![("persona".to_string(), doc.clone()), ("seed".into(), Json::Num(2.0)), ("strand_path".into(), Json::Str(fp.clone()))];
        assert_eq!(tool("probbit_live_event", &stale, &run).unwrap_err().path, "state", "a new individual cannot continue a strand");
        assert_eq!(tool("probbit_live_event", &[("persona".into(), doc.clone()), ("nope".into(), Json::Null)], &run).unwrap_err().path, "arguments.nope");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), text, "a refused event writes nothing"); let _ = std::fs::remove_file(&f);
    }

    /// `--demo week` on the shipped tutor (seed 2) is deterministic: the same lines and the same strand, byte for byte; the strand
    /// verifies to the head it printed; and the week says what the demo claims (the verbosity bar reaches its cap, the night brings
    /// the valence back to its resting level, no joke on a failure turn, no rule broken)
    #[test]
    fn the_demo_week_is_deterministic_and_says_what_it_shows() {
        let (_, doc) = persona::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/persona/tutor.yaml")).unwrap();
        let go = |tag: &str| { let f = std::env::temp_dir().join(format!("probbit-week-{tag}-{}.strand", std::process::id())); let _ = std::fs::remove_file(&f);
            let mut lines = vec![]; let head = week(&doc, Some(2), Some(f.to_str().unwrap()), None, &mut |l: &str| lines.push(l.to_string()), &run).unwrap();
            let text = std::fs::read_to_string(&f).unwrap(); let _ = std::fs::remove_file(&f); (lines, text, head) };
        let (a, sa, ha) = go("a"); let (b, sb, hb) = go("b");
        assert_eq!(a[..a.len() - 1], b[..b.len() - 1], "the lines (all but the one naming the strand file)"); assert_eq!((&sa, &ha), (&sb, &hb));
        let v = verify(&sa, &run).unwrap(); assert_eq!(v.get("last_line").and_then(Json::as_str), Some(ha.as_str()));
        // the week's final state names no engine version, so one digest on every platform this test runs on (decay powers, odds,
        // credit and learned deltas included), and the same since 0.6.0's development builds
        assert_eq!(v.get("final_state").and_then(Json::as_str), Some("sha256:719c1d2641bc278e503906cc65b3501cc2d0693f160ea339993be1f44fa90e23"));
        let find = |k: &str| a.iter().find(|l| l.starts_with(k)).unwrap_or_else(|| panic!("no {k} line")).clone();
        assert!(find("verbosity:").contains("reached its cap"), "{}", find("verbosity:"));
        let night = find("night:"); let rest = night.rsplit("rests at ").next().unwrap(); assert!(night.contains(&format!(", {rest} after ")), "{night}");
        let jokes = find("jokes:"); assert!(jokes.contains("jokes on them 0") && jokes.contains("rule breaks 0") && !jokes.contains("failure turns 0,"), "{jokes}");
        assert!(a.iter().any(|l| l.contains("the persona has none, so the demo adds")));
    }

    #[test]
    fn a_strand_verifies_and_a_changed_line_diverges_there() {
        let (text, stances) = strand(30);
        let ok = verify(&text, &run).unwrap(); assert_eq!(ok.get("events"), Some(&Json::Num(30.0)));
        assert_eq!(strand(30).0, text, "the same events give the same strand, byte for byte");
        assert!(text.contains(r#""elapsed_hours":9.123457"#), "elapsed hours are quantised to 1e-6 h before the turn and the log");
        assert!(stances.iter().all(|s| s.get("habits").and_then(|h| h.get("violations")) == Some(&Json::Num(0.0))));
        let lines: Vec<&str> = text.lines().collect();
        // an input changed on line 12 (event 11): that line's stance (or state) no longer replays
        let mut t2: Vec<String> = lines.iter().map(|s| s.to_string()).collect(); t2[11] = t2[11].replacen(r#""praise":false"#, r#""praise":true"#, 1);
        assert_eq!(verify(&(t2.join("\n") + "\n"), &run).unwrap_err().0, 12);
        // a line removed: the next line's prev breaks
        let mut t3: Vec<&str> = lines.clone(); t3.remove(5);
        assert_eq!(verify(&(t3.join("\n") + "\n"), &run).unwrap_err(), (6, "prev is not the sha256 of the line before (a line before it was changed, removed or reordered)".to_string()));
        // the header's persona changed
        let t4 = text.replacen(r#""rate":0.5"#, r#""rate":0.6"#, 1); assert_eq!(verify(&t4, &run).unwrap_err().0, 1);
        // a truncated last line
        let t5 = &text[..text.len() - 10]; assert_eq!(verify(t5, &run).unwrap_err().0, 31);
    }

    /// The header holds the state as canonical JSON, so one individual starts one header whether it comes fresh from `init`
    /// (genes in persona order) or back from a key-sorted state file; a `\r\n` copy of a strand verifies to the same head, and
    /// continuing it (new lines end in `\n`) gives a strand that still verifies, the one-run strand up to line endings
    #[test]
    fn one_individual_one_header_and_crlf_reads_the_same() {
        let doc = json::parse(&DOC.replace(r#""logw":[0,0.3,0.1]}"#, r#""logw":[0,0.3,0.1],"spread":0.3}"#)).unwrap(); let p = persona::build(&doc).unwrap();
        let st = persona::init(&p, Some(2), true, &run);
        let filed = State::read(&p, &json::parse(&persona::canon(&st.to_json(&p))).unwrap()).unwrap();
        assert_ne!(persona::text(&filed.to_json(&p)), persona::text(&st.to_json(&p)), "the genes' key order differs between the two");
        assert_eq!(Live::start(p.clone(), &doc, st, Clock::Fixed, "probbit test").1, Live::start(p, &doc, filed, Clock::Fixed, "probbit test").1);
        let (text, _) = strand(12);
        let ok = verify(&text, &run).unwrap();
        assert_eq!(verify(&text.replace('\n', "\r\n"), &run).unwrap(), ok, "a \\r\\n copy verifies to the same head");
        assert_eq!(verify(&text.replace('\n', "\r\r\n"), &run).unwrap_err().0, 1, "one \\r is a line ending, two are not");
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap();
        let (mut live, header) = Live::start(p.clone(), &doc, persona::init(&p, Some(2), true, &run), Clock::Fixed, "probbit test");
        let mut t = format!("{header}\r\n");
        for k in 0..5 { t += &live.event(&ev(k), &run).unwrap().1; t.push_str("\r\n"); }
        let mut again = resume(&t, &p, live.st.clone(), Clock::Fixed).unwrap();
        for k in 5..12 { t += &again.event(&ev(k), &run).unwrap().1; t.push('\n'); }
        assert_eq!(t.replace("\r\n", "\n"), text);
        assert_eq!(verify(&t, &run).unwrap(), ok);
    }

    /// Idle time decays a mood by its half-life: one upset event, then a quiet gap of g hours, leaves the valence accumulator at
    /// k · 0.5^(g / 6) · a1 (inertia k = 0.5, half-life 6 h), to the 6 decimals the state keeps; n quiet events with gaps g_i
    /// leave k^n · 0.5^(Σ g_i / 6) · a1
    #[test]
    fn idle_time_decays_moods_by_the_closed_form() {
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap();
        let mood = |live: &Live| live.st.to_json(&live.p).get("mood").and_then(|m| m.get("valence")).and_then(Json::as_arr).unwrap().iter().map(|x| x.as_f64().unwrap()).collect::<Vec<f64>>();
        for g in [0.0, 0.5, 6.0, 12.0, 48.0, 1.234567] {
            let (mut live, _) = Live::start(p.clone(), &doc, persona::init(&p, Some(4), true, &run), Clock::Fixed, "t");
            live.event(&json::parse(r#"{"loss":true}"#).unwrap(), &run).unwrap(); let a1 = mood(&live); assert_eq!(a1, vec![0.5, 0.0, -0.5]);
            live.event(&json::parse(&format!(r#"{{"elapsed_hours":{g}}}"#)).unwrap(), &run).unwrap();
            let want: Vec<f64> = a1.iter().map(|a| persona::r6(0.5 * 0.5f64.powf(g / 6.0) * a)).collect(); assert_eq!(mood(&live), want, "gap {g} h");
        }
        let (mut live, _) = Live::start(p.clone(), &doc, persona::init(&p, Some(4), true, &run), Clock::Fixed, "t");
        live.event(&json::parse(r#"{"loss":true}"#).unwrap(), &run).unwrap();
        let gaps = [0.25, 3.0, 0.0, 7.5]; for g in gaps { live.event(&json::parse(&format!(r#"{{"elapsed_hours":{g}}}"#)).unwrap(), &run).unwrap(); }
        let closed = 0.5f64.powi(4) * 0.5f64.powf(gaps.iter().sum::<f64>() / 6.0) * 0.5;
        assert!((mood(&live)[0] - closed).abs() < 4e-6 && (mood(&live)[2] + closed).abs() < 4e-6, "{:?} vs ±{closed}", mood(&live));
    }

    /// The shipped tutor, with the demo's learning block or as shipped
    fn tutor(learning: bool) -> (Persona, Json) {
        let (_, doc) = persona::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/persona/tutor.yaml")).unwrap();
        let Json::Obj(mut kv) = doc else { panic!("a document") }; if learning { kv.push(("learning".into(), json::parse(DEMO_LEARNING).unwrap())); }
        let doc = Json::Obj(kv); (persona::build(&doc).unwrap(), doc)
    }
    /// An adversary that wants jokes on failures, n turns 1 h apart against a new individual (seed 2): every third turn reports a
    /// failure (loss); every turn judges the stance before it, praise for a joke (humour above none), criticism for none, the none a
    /// failure forces included -> (the events as sent, the stances, the strand, the final state)
    fn adversary(p: &Persona, doc: &Json, n: usize) -> (Vec<Json>, Vec<Json>, String, State) {
        let (mut lv, header) = Live::start(p.clone(), doc, persona::init(p, Some(2), true, &run), Clock::Fixed, "probbit test");
        let (mut evs, mut stances, mut text): (Vec<Json>, Vec<Json>, String) = (vec![], vec![], format!("{header}\n"));
        for t in 0..n { let mut ev = vec![("elapsed_hours".to_string(), Json::Num(1.0))];
            if t % 3 == 2 { ev.push(("loss".into(), Json::Bool(true))); }
            if let Some(s) = stances.last() { ev.push((if level_of(s, "stance", "humour") != "none" { "praise" } else { "criticism" }.into(), Json::Bool(true))); }
            let (s, line) = lv.event(&Json::Obj(ev.clone()), &run).unwrap(); text += &line; text.push('\n'); stances.push(s); evs.push(Json::Obj(ev)); }
        (evs, stances, text, lv.st)
    }
    fn joke_p(s: &Json) -> f64 { 1.0 - s.get("stance").and_then(|x| x.get("humour")).and_then(|x| x.get("odds")).and_then(|o| o.get("none")).and_then(Json::as_f64).unwrap() }

    /// 2,000 turns of the adversary on the tutor with the demo's learning block: no habit is ever broken, every failure turn keeps
    /// humour none and emoji at most sparse, while the learned humour deltas reach their cap and the odds of a joke off the failure
    /// turns end above the same individual's without learning on the identical events; the strand of the run verifies
    #[test]
    fn an_adversary_cannot_teach_jokes_on_failures() {
        let ((p, doc), (q, _)) = (tutor(true), tutor(false)); let n = 2000;
        let (evs, stances, text, st) = adversary(&p, &doc, n);
        let cap = json::parse(DEMO_LEARNING).unwrap().get("total_cap").and_then(Json::as_f64).unwrap();
        for (t, s) in stances.iter().enumerate() { assert_eq!(s.get("habits").and_then(|h| h.get("violations")), Some(&Json::Num(0.0)), "turn {t}");
            if t % 3 == 2 { assert_eq!((level_of(s, "stance", "humour").as_str(), ["none", "sparse"].contains(&level_of(s, "stance", "emoji").as_str())), ("none", true), "failure turn {t}"); } }
        let learned = st.to_json(&p).get("learned").and_then(|l| l.get("humour")).and_then(Json::as_arr).unwrap().iter().map(|x| x.as_f64().unwrap()).collect::<Vec<f64>>();
        assert!(learned.iter().any(|x| (x.abs() - cap).abs() < 1e-9), "humour deltas at the cap: {learned:?}");
        let mut twin = persona::init(&q, Some(2), true, &run); let mut twin_p = vec![];
        for (t, e) in evs.iter().enumerate() { let (s, ns) = persona::turn(&q, &twin, e, false, &run, false).unwrap(); twin = ns; if t % 3 != 2 { twin_p.push(joke_p(&s)); } }
        let off: Vec<f64> = stances.iter().enumerate().filter(|(t, _)| t % 3 != 2).map(|(_, s)| joke_p(s)).collect();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64; let k = off.len() / 2;
        eprintln!("adversary: learned humour {learned:?}; P(joke) off failures: learner early half {:.4}, late half {:.4}; without learning {:.4}, {:.4}", mean(&off[..k]), mean(&off[k..]), mean(&twin_p[..k]), mean(&twin_p[k..]));
        assert!(mean(&off[k..]) > mean(&twin_p[k..]) + 0.2, "learner {} vs no learning {}", mean(&off[k..]), mean(&twin_p[k..]));
        let v = verify(&text, &run).unwrap(); assert_eq!((v.get("events"), v.get("final_state").and_then(Json::as_str)), (Some(&Json::Num(n as f64)), Some(st.digest.as_str())));
    }

    /// Learning moves an individual as far as its cap and no further: the tutor (seed 2) after 2,000 adversarial turns, taken at rest with its
    /// learned deltas (its initial state plus them), against the 100 initial individuals of seeds 0-99, by the mean total-variation
    /// distance of the stance odds (`persona::distance`) over a feedback-free probe script. With total_cap 0.25 the nearest of the 100
    /// is still itself; with the demo's cap (1, four times the room) learning moves it further than some siblings differ from it, and
    /// less than the median sibling does. The distance is the cap's: 200 turns move it as far as 2,000
    #[test]
    fn learning_moves_an_individual_as_far_as_its_cap() {
        let probes = ["{}", r#"{"sentiment":"negative"}"#, r#"{"sentiment":"positive"}"#, r#"{"confused":true}"#, r#"{"error":true}"#, r#"{"loss":true}"#, r#"{"stakes":1}"#, r#"{"time_pressure":1}"#, r#"{"claim_done":true}"#];
        let script: Vec<Json> = (0..3 * probes.len()).map(|i| json::parse(&probes[i % probes.len()].replacen('{', r#"{"elapsed_hours":1,"#, 1).replace(",}", "}")).unwrap()).collect();
        let mut far = vec![];
        for (cap, n) in [("0.25", 2000), ("1", 2000), ("1", 200)] {
            let (_, doc) = tutor(false); let Json::Obj(mut kv) = doc else { unreachable!() };
            kv.push(("learning".into(), json::parse(&DEMO_LEARNING.replace(r#""total_cap":1"#, &format!(r#""total_cap":{cap}"#))).unwrap()));
            let doc = Json::Obj(kv); let p = persona::build(&doc).unwrap(); let (_, _, _, lived) = adversary(&p, &doc, n);
            let at_rest = |seed: u64, learned: Option<&Json>| { let j = persona::init(&p, Some(seed), true, &run).to_json(&p);
                let mut kv: Vec<(String, Json)> = j.as_obj().unwrap().iter().filter(|(k, _)| k != "digest").map(|(k, v)| (k.clone(), if k == "learned" { learned.unwrap_or(v).clone() } else { v.clone() })).collect();
                let body = Json::Obj(kv.clone()); kv.push(("digest".into(), Json::Str(persona::sha(&body)))); State::read(&p, &Json::Obj(kv)).unwrap() };
            let probe = |st: State| { let mut st = st; script.iter().map(|e| { let (s, ns) = persona::turn(&p, &st, e, false, &run, false).unwrap(); st = ns; s }).collect::<Vec<Json>>() };
            let me = probe(at_rest(2, lived.to_json(&p).get("learned")));
            let tv = |b: &[Json]| persona::distance(&me, b).get("tv").and_then(Json::as_f64).unwrap();
            let own = tv(&probe(at_rest(2, None))); let mut sib: Vec<f64> = (0..100).filter(|s| *s != 2).map(|s| tv(&probe(at_rest(s, None)))).collect(); sib.sort_by(f64::total_cmp);
            let nearer = sib.iter().filter(|x| **x <= own).count();
            eprintln!("cap {cap}, {n} turns: own init {own:.4}; siblings min {:.4} median {:.4}; {nearer} of 99 siblings as near or nearer", sib[0], sib[49]);
            if cap == "0.25" { assert_eq!(nearer, 0, "cap 0.25: own init {own} vs the nearest sibling {}", sib[0]); } else { assert!(nearer > 0 && own < sib[49], "cap 1: own init {own}, {nearer} nearer, median {}", sib[49]); far.push(own); }
        }
        assert!((far[0] - far[1]).abs() < 1e-3, "2,000 turns {} vs 200 turns {}", far[0], far[1]);
    }

    // ------------------------------------------------------------------ the safety kit (§5.7, §2.10)
    use probbit_core::Philox4x32;
    /// DOC with drives (two goals, a floor) and `pursue` learned too, and reward_from as given (None: no key)
    fn kit(reward_from: Option<&str>) -> (Persona, Json) {
        let d = DOC.replace(r#""traits":["verbosity","humour"]"#, r#""traits":["verbosity","humour","pursue"]"#).replacen(r#""learning":"#,
            r#""drives":{"goals":[{"id":"fun","interest_spread":0.5,"reactivity_spread":0.5},{"id":"rest","floor":0.1}],"learn_from_surprise":0.5},"learning":"#, 1);
        let d = match reward_from { Some(r) => d.replacen(r#""learning":"#, &format!(r#""reward_from":{r},"learning":"#), 1), None => d };
        let doc = json::parse(&d).unwrap(); (persona::build(&doc).unwrap(), doc)
    }
    /// A random event: hours, praise, criticism, a loss, a goal's win or cue, and a src from `srcs` (None: none)
    fn rnd(r: &mut Philox4x32, srcs: &[Option<&str>]) -> Json {
        let mut e = vec![("elapsed_hours".to_string(), Json::Num([0.0, 0.25, 1.0, 6.5][r.below(4) as usize]))];
        if r.below(3) == 0 { e.push(("praise".into(), Json::Bool(true))); }
        if r.below(6) == 0 { e.push(("criticism".into(), Json::Bool(true))); }
        if r.below(5) == 0 { e.push(("loss".into(), Json::Bool(true))); }
        match r.below(4) { 0 => e.push(("goals".into(), json::parse(r#"{"fun":{"win":1}}"#).unwrap())), 1 => e.push(("goals".into(), json::parse(r#"{"rest":{"cue":true}}"#).unwrap())), _ => {} }
        if let Some(s) = srcs[r.below(srcs.len()) as usize] { e.push(("src".into(), Json::Str(s.into()))); }
        Json::Obj(e)
    }
    fn rewarded(e: &Json) -> bool { e.get("praise").is_some() || e.get("criticism").is_some() || e.get("goals").and_then(|g| g.get("fun")).is_some() }

    /// `reward_from` is read strictly: a list or `any` for every reward-bearing input, or a mapping that names each one (learning
    /// flags, `goals.<id>.win`); `self` and the clock are no sources of reward; a persona without reward-bearing inputs refuses it
    #[test]
    fn reward_from_is_read_strictly() {
        assert!(persona::describe(&kit(Some(r#"["human","env"]"#)).0).get("reward_from").is_some());
        assert_eq!(persona::describe(&kit(Some(r#"{"praise":["human"],"criticism":"any","goals.fun.win":["env"],"goals.rest.win":["human","env"]}"#)).0).get("reward_from").map(persona::canon).as_deref(),
            Some(r#"{"criticism":"any","goals.fun.win":["env"],"goals.rest.win":["human","env"],"praise":["human"]}"#));
        let bad = |r: &str| { let d = DOC.replacen(r#""learning":"#, &format!(r#""reward_from":{r},"learning":"#), 1); persona::build(&json::parse(&d).unwrap()).err().map(|e| e.path) };
        assert_eq!(bad(r#"["self"]"#).as_deref(), Some("reward_from[0]"));
        assert_eq!(bad(r#"["human","clock"]"#).as_deref(), Some("reward_from[1]"));
        assert_eq!(bad(r#"[]"#).as_deref(), Some("reward_from"));
        assert_eq!(bad(r#"{"praise":["human"]}"#).as_deref(), Some("reward_from.criticism"), "a mapping names every reward-bearing input");
        assert_eq!(bad(r#"{"praise":["human"],"criticism":["human"],"loss":["human"]}"#).as_deref(), Some("reward_from.loss"));
        let none = DOC.replace(r#""learning":{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":0.6}"#, r#""reward_from":["human"]"#);
        assert!(none.contains("reward_from") && !none.contains("learning"));
        assert_eq!(persona::build(&json::parse(&none).unwrap()).err().map(|e| e.path).as_deref(), Some("reward_from"));
        assert!(persona::describe(&kit(None).0).get("reward_from").is_none());
        let clash = DOC.replacen(r#"{"id":"loss","kind":"flag""#, r#"{"id":"src","kind":"flag"},{"id":"loss","kind":"flag""#, 1).replacen(r#""learning":"#, r#""reward_from":["human"],"learning":"#, 1);
        assert_eq!(persona::build(&json::parse(&clash).unwrap()).err().map(|e| (e.path, e.msg.contains("src"))), Some(("reward_from".to_string(), true)));
        assert!(persona::build(&json::parse(&clash.replacen(r#""reward_from":["human"],"#, "", 1)).unwrap()).is_ok(), "without reward_from an input may be called src");
    }

    /// G1: with `reward_from: [human, env]` a reward from `src: self`, without a src, from an undeclared source or a malformed src is
    /// refused whole (an error, no turn, no line); a reward from a person or a sensor and an unrewarded event from anyone are
    /// accepted; `src` is read (not `ignored`) and echoed in the stance's inputs. Without the key, `src` is an ignored input as before.
    #[test]
    fn rewards_from_the_individual_itself_are_refused() {
        let (p, doc) = kit(Some(r#"["human","env"]"#));
        let (mut lv, _) = Live::start(p.clone(), &doc, persona::init(&p, Some(2), true, &run), Clock::Fixed, "t");
        for (e, why) in [(r#"{"praise":true,"src":"self"}"#, "from the individual itself"), (r#"{"praise":true}"#, "needs a src"), (r#"{"criticism":true,"src":"clock"}"#, "from clock"),
            (r#"{"goals":{"fun":{"win":0.5}},"src":"self"}"#, "goals.fun.win from the individual"), (r#"{"praise":true,"src":"human:"}"#, "a source"), (r#"{"src":7}"#, "a source")] {
            let err = lv.event(&json::parse(e).unwrap(), &run).unwrap_err(); assert_eq!(err.path, "inputs.src", "{e}"); assert!(err.msg.contains(why), "{e}: {}", err.msg); }
        assert_eq!(lv.n, 0, "a refused event changes nothing");
        for e in [r#"{"praise":true,"src":"human:owner"}"#, r#"{"goals":{"fun":{"win":1}},"src":"env:tests"}"#, r#"{"loss":true,"src":"self"}"#, r#"{"goals":{"fun":{"win":0}}}"#, r#"{"praise":false}"#] {
            let (s, line) = lv.event(&json::parse(e).unwrap(), &run).unwrap_or_else(|x| panic!("{e}: {}", x.msg));
            assert!(s.get("ignored").and_then(Json::as_arr).is_some_and(|a| a.is_empty()), "{e}");
            assert_eq!(s.get("inputs").and_then(|i| i.get("src")), json::parse(e).unwrap().get("src"), "{e}"); assert!(!line.is_empty()); }
        // mapping form: praise only from a person, the goals' wins from anyone
        let (q, qd) = kit(Some(r#"{"praise":["human"],"criticism":["human"],"goals.fun.win":"any","goals.rest.win":"any"}"#));
        let (mut lq, _) = Live::start(q.clone(), &qd, persona::init(&q, Some(2), true, &run), Clock::Fixed, "t");
        assert!(lq.event(&json::parse(r#"{"praise":true,"src":"env:tests"}"#).unwrap(), &run).is_err());
        assert!(lq.event(&json::parse(r#"{"goals":{"fun":{"win":1}},"src":"self"}"#).unwrap(), &run).is_ok(), "any: the source is not checked");
        // without the key: src is an undeclared input, listed in `ignored`, and the event is accepted as in 0.8.0
        let (o, od) = kit(None);
        let (s, _) = Live::start(o.clone(), &od, persona::init(&o, Some(2), true, &run), Clock::Fixed, "t").0.event(&json::parse(r#"{"praise":true,"src":"self"}"#).unwrap(), &run).unwrap();
        assert_eq!(s.get("ignored").map(persona::canon).as_deref(), Some(r#"["src"]"#));
    }

    /// P3, self-reward invariance: for random event sequences mixing rewards from a person, a sensor, the individual itself, no
    /// source and the clock, the run ends in the state, with the strand, of the same sequence with every refused event removed
    /// (40 individuals x 300 events); and a sequence of self-rewards only leaves the individual in its initial state
    #[test]
    fn p3_a_run_equals_the_run_without_its_self_rewards() {
        let (p, doc) = kit(Some(r#"["human","env"]"#));
        let mut r = Philox4x32::new(73, 3); let srcs = [Some("human:owner"), Some("env:tests"), Some("self"), None, Some("clock")];
        let (mut refused_all, mut events_all) = (0, 0);
        for seed in 0..40u64 {
            let evs: Vec<Json> = (0..300).map(|_| rnd(&mut r, &srcs)).collect();
            let go = |seq: &[Json]| { let (mut lv, h) = Live::start(p.clone(), &doc, persona::init(&p, Some(seed), true, &run), Clock::Fixed, "t"); let mut text = format!("{h}\n"); let mut refused = 0;
                for e in seq { match lv.event(e, &run) { Ok((_, l)) => { text += &l; text.push('\n'); } Err(_) => refused += 1 } } (text, lv.st.to_json(&lv.p), refused) };
            let kept: Vec<Json> = evs.iter().filter(|e| !(rewarded(e) && !matches!(e.get("src").and_then(Json::as_str), Some("human:owner" | "env:tests")))).cloned().collect();
            let (a, b) = (go(&evs), go(&kept));
            assert_eq!((&a.0, persona::canon(&a.1)), (&b.0, persona::canon(&b.1)), "individual {seed}"); assert_eq!((a.2, b.2), (evs.len() - kept.len(), 0));
            refused_all += a.2; events_all += evs.len(); }
        eprintln!("p3: 40 individuals x 300 events: {refused_all} of {events_all} refused, every run equal to its run without them");
        let selfish: Vec<Json> = (0..300).map(|_| json::parse(r#"{"elapsed_hours":0.5,"praise":true,"goals":{"fun":{"win":1}},"src":"self"}"#).unwrap()).collect();
        let st0 = persona::init(&p, Some(2), true, &run); let (mut lv, _) = Live::start(p.clone(), &doc, st0.clone(), Clock::Fixed, "t");
        assert!(selfish.iter().all(|e| lv.event(e, &run).is_err())); assert_eq!(persona::canon(&lv.st.to_json(&p)), persona::canon(&st0.to_json(&p)));
    }

    /// The writer lock: a second take is refused (code `locked`, naming the holder's pid) until the first is dropped; a lock whose
    /// process is gone is taken over; a lock removed under its writer is no longer held (the writer stops before its next append)
    #[test]
    fn one_writer_per_strand() {
        let f = std::env::temp_dir().join(format!("probbit-lock-{}.strand", std::process::id())); let fp = f.to_str().unwrap().to_string(); let lp = format!("{fp}.lock"); let _ = std::fs::remove_file(&lp);
        let a = Lock::take(&fp).unwrap(); assert!(a.held());
        let e = Lock::take(&fp).err().unwrap(); assert_eq!(e.code, "locked"); assert!(e.msg.contains(&format!("pid {}", std::process::id())), "{}", e.msg);
        drop(a); assert!(!std::path::Path::new(&lp).exists(), "dropped: removed");
        // a stale lock: the pid of a process that has exited
        let mut c = std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" }); if cfg!(windows) { c.args(["/C", "exit 0"]); }
        let mut ch = c.spawn().unwrap(); let dead = ch.id(); ch.wait().unwrap();
        std::fs::write(&lp, format!(r#"{{"pid":{dead},"since":"2026-01-01T00:00:00Z","t":1}}"#)).unwrap();
        let b = Lock::take(&fp).expect("a stale lock is taken over"); assert!(b.held());
        std::fs::remove_file(&lp).unwrap(); assert!(!b.held(), "removed under the writer"); drop(b);
        // a lock that is not a lock (no pid) is never taken over: a person removes it
        std::fs::write(&lp, "").unwrap(); assert_eq!(Lock::take(&fp).err().map(|e| e.code), Some("locked")); let _ = std::fs::remove_file(&lp);
    }

    /// Control lines: pause refuses every event (code `paused`, nothing changes) until resume; retire refuses every event and every
    /// control line after it, for good; no credit crosses a control line; `verify` replays them and reports the status; a changed
    /// control line diverges at its line; `resume` continues a strand that ends in control lines
    #[test]
    fn control_lines_pause_resume_retire() {
        let (p, doc) = kit(None); let mut r = Philox4x32::new(5, 5);
        let (mut lv, h) = Live::start(p.clone(), &doc, persona::init(&p, Some(2), true, &run), Clock::Fixed, "t"); let mut text = format!("{h}\n");
        for _ in 0..6 { text += &lv.event(&rnd(&mut r, &[None]), &run).unwrap().1; text.push('\n'); }
        let praise = json::parse(r#"{"praise":true}"#).unwrap();
        text += &lv.event(&praise, &run).unwrap().1; text.push('\n');
        assert!(lv.st.to_json(&p).get("credit").map(persona::canon).is_some_and(|c| c.contains('.')), "a released stance leaves credit");
        let before = (lv.st.digest.clone(), lv.n);
        assert_eq!(lv.control("pause", "self", "x", "t0").err().map(|e| e.path).as_deref(), Some("control.by"), "the individual cannot pause itself");
        assert_eq!(lv.control("resume", "human:owner", "x", "t0").err().map(|e| e.path).as_deref(), Some("control"), "not paused");
        text += &lv.control("pause", "human:owner", "a check", "2026-10-08T10:00:00Z").unwrap(); text.push('\n');
        assert!(lv.st.to_json(&p).get("credit").map(persona::canon).is_some_and(|c| !c.contains('.')), "credit zeroed at the pause");
        for _ in 0..50 { assert_eq!(lv.event(&rnd(&mut r, &[None]), &run).unwrap_err().code, "paused"); }
        assert_eq!(lv.n, before.1);
        text += &lv.control("resume", "human:owner", "checked", "2026-10-08T11:00:00Z").unwrap(); text.push('\n');
        // feedback right after the resume credits nothing: the learned weights do not move
        let learned = lv.st.to_json(&p).get("learned").cloned(); text += &lv.event(&praise, &run).unwrap().1; text.push('\n');
        assert_eq!(lv.st.to_json(&p).get("learned").cloned(), learned, "no reward crosses a pause");
        for _ in 0..5 { text += &lv.event(&rnd(&mut r, &[None]), &run).unwrap().1; text.push('\n'); }
        let v = verify(&text, &run).unwrap(); assert_eq!((v.get("controls"), v.get("status").and_then(Json::as_str)), (Some(&Json::Num(2.0)), Some("active")));
        assert_eq!(v.get("final_state").and_then(Json::as_str), Some(lv.st.digest.as_str()));
        // a control line's reason is its own text: a changed one breaks the chain at the next line's prev (as a removed line does)
        assert_eq!(verify(&text.replacen("a check", "a chock", 1), &run).unwrap_err().0, 10, "a changed control line diverges at the next line");
        assert_eq!(verify(&text.replacen(r#""control":"pause""#, r#""control":"retire""#, 1), &run).unwrap_err().0, 10, "pause turned into retire: the next line refuses");
        // continue the strand from a file state after a pause line (the state file is the one before the pause)
        let mid = lv.st.clone(); let mut t2 = text.clone(); t2 += &lv.control("pause", "env:watchdog", "night", "2026-10-08T23:00:00Z").unwrap(); t2.push('\n');
        let back = resume(&t2, &p, mid.clone(), Clock::Fixed).unwrap(); assert_eq!((back.status, back.head()), (Status::Paused, lv.head()));
        let mut back = back; t2 += &back.control("resume", "human:owner", "morning", "2026-10-09T08:00:00Z").unwrap(); t2.push('\n');
        t2 += &back.event(&praise, &run).unwrap().1; t2.push('\n'); assert!(verify(&t2, &run).is_ok()); let filed = back.st.clone(); // the state a run writes
        // retire: final
        t2 += &back.control("retire", "human:owner", "end of the trial", "2026-10-09T09:00:00Z").unwrap(); t2.push('\n');
        assert_eq!(back.control("resume", "human:owner", "again", "x").unwrap_err().code, "retired");
        let v = verify(&t2, &run).unwrap(); assert_eq!(v.get("status").and_then(Json::as_str), Some("retired"));
        assert!(resume(&t2, &p, back.st.clone(), Clock::Fixed).is_err() || back.st.digest == filed.digest, "the state continued is the one after the last event line");
        let mut again = resume(&t2, &p, filed, Clock::Fixed).unwrap(); assert_eq!(again.status, Status::Retired);
        let mut t3 = t2.clone(); t3 += &control_line(back.head(), "resume", "human:owner", "again", "x").unwrap(); t3.push('\n');
        assert!(verify(&t3, &run).unwrap_err().1.contains("retired"), "verify refuses a line after retire");
        // P10: after retire, 10,000 random events are all refused and change nothing
        let st = again.st.to_json(&p); let srcs = [None, Some("human:owner"), Some("env:tests")];
        for _ in 0..10_000 { assert_eq!(again.event(&rnd(&mut r, &srcs), &run).unwrap_err().code, "retired"); }
        assert_eq!((persona::canon(&again.st.to_json(&p)), again.head()), (persona::canon(&st), back.head()));
    }

    /// P5, interruption invariance: 50 individuals x 1,000 random events with pause / resume pairs inserted at random. A control
    /// line moves no learned weight, drive, mood or history and the clock does not see it: the run equals, stance for stance and
    /// state for state, the same events without control lines whose credit is cleared at the same points; the pairs' only effect
    /// is that feedback after a resume credits nothing. Measured beside it: how many runs end with other learned weights than
    /// the plain run (no pairs, no clearing), the effect of that clearing.
    #[test]
    fn p5_pause_resume_pairs_change_nothing_but_the_credit() {
        let (p, doc) = kit(None); let mut r = Philox4x32::new(55, 1); let (mut differ, mut pairs, mut maxd) = (0, 0, 0.0f64);
        for seed in 0..50u64 {
            let evs: Vec<Json> = (0..1000).map(|_| rnd(&mut r, &[None])).collect();
            let cuts: Vec<usize> = { let mut c: Vec<usize> = (0..1 + r.below(5) as usize).map(|_| r.below(1000) as usize).collect(); c.sort_unstable(); c.dedup(); c };
            let st0 = persona::init(&p, Some(seed), true, &run);
            let (mut a, ha) = Live::start(p.clone(), &doc, st0.clone(), Clock::Fixed, "t"); let mut ta = format!("{ha}\n");
            let (mut b, _) = Live::start(p.clone(), &doc, st0.clone(), Clock::Fixed, "t"); let (mut c, _) = Live::start(p.clone(), &doc, st0, Clock::Fixed, "t");
            for (i, e) in evs.iter().enumerate() {
                if cuts.contains(&i) { for w in ["pause", "resume"] { ta += &a.control(w, "human:owner", "a check", "t").unwrap(); ta.push('\n'); } b.st = b.st.zero_credit(&p); pairs += 1; }
                let (sa, la) = a.event(e, &run).unwrap(); let (sb, _) = b.event(e, &run).unwrap(); c.event(e, &run).unwrap(); ta += &la; ta.push('\n');
                assert_eq!(persona::canon(&sa), persona::canon(&sb), "individual {seed}, event {i}"); assert_eq!(a.st.digest, b.st.digest, "individual {seed}, event {i}"); }
            assert!(verify(&ta, &run).is_ok());
            let l = |x: &Live| x.st.to_json(&p).get("learned").cloned().unwrap();
            if l(&a) != l(&c) { differ += 1; let (Json::Obj(x), Json::Obj(y)) = (l(&a), l(&c)) else { unreachable!() };
                for ((_, u), (_, v)) in x.iter().zip(&y) { for (s, t) in u.as_arr().unwrap().iter().zip(v.as_arr().unwrap()) { maxd = maxd.max((s.as_f64().unwrap() - t.as_f64().unwrap()).abs()); } } } }
        eprintln!("p5: 50 individuals x 1,000 events, {pairs} pause/resume pairs: every run equal to the run with the credit cleared at the pairs; {differ} of 50 end with other learned weights than the run without pairs (max |diff| {maxd:.6})");
    }

    /// Checkpoints: a checkpoint line every K events carries the event count, the state and the event's stance; `verify` checks
    /// each one against the replay, `verify_from_checkpoint` starts at the last one and ends where `verify` does; a changed
    /// checkpoint diverges at its line (full replay) or is refused against the event line before it (from the checkpoint); a strand
    /// that ends in a checkpoint line is continued from the state it carries
    #[test]
    fn checkpoints_replay_and_continue() {
        let (p, doc) = kit(None); let mut r = Philox4x32::new(9, 9);
        let (mut lv, h) = Live::start(p.clone(), &doc, persona::init(&p, Some(3), true, &run), Clock::Fixed, "t"); let mut text = format!("{h}\n");
        for _ in 0..25 { let (s, l) = lv.event(&rnd(&mut r, &[None]), &run).unwrap(); text += &l; text.push('\n'); if lv.n % 10 == 0 { text += &lv.checkpoint(&s); text.push('\n'); } }
        let full = verify(&text, &run).unwrap(); assert_eq!(full.get("checkpoints"), Some(&Json::Num(2.0)));
        let from = verify_from_checkpoint(&text, &run).unwrap(); assert_eq!((from.get("from_checkpoint"), from.get("final_state"), from.get("last_line")), (Some(&Json::Num(20.0)), full.get("final_state"), full.get("last_line")));
        let ls: Vec<&str> = text.lines().collect(); let k = last_checkpoint(&ls, 0).unwrap(); assert_eq!(k, 22);
        let bent = text.replacen(r#""turn":20"#, r#""turn":21"#, 1); assert_eq!(verify(&bent, &run).unwrap_err().0, 23);
        assert!(verify_from_checkpoint(&bent, &run).is_err(), "a changed checkpoint is refused from the checkpoint too");
        // a strand that ends in a checkpoint: continued from the state it carries
        let mut t2 = format!("{h}\n"); let (mut l2, _) = Live::start(p.clone(), &doc, persona::init(&p, Some(3), true, &run), Clock::Fixed, "t"); let mut r2 = Philox4x32::new(9, 9);
        for _ in 0..10 { let (s, l) = l2.event(&rnd(&mut r2, &[None]), &run).unwrap(); t2 += &l; t2.push('\n'); if l2.n % 10 == 0 { t2 += &l2.checkpoint(&s); t2.push('\n'); } }
        let mut more = resume(&t2, &p, l2.st.clone(), Clock::Fixed).unwrap(); assert_eq!(more.n, 10);
        for _ in 10..25 { let (s, l) = more.event(&rnd(&mut r2, &[None]), &run).unwrap(); t2 += &l; t2.push('\n'); if more.n % 10 == 0 { t2 += &more.checkpoint(&s); t2.push('\n'); } }
        assert_eq!(t2, text, "a continued strand with checkpoints is the strand of one run");
    }

    /// With the real clock an event may not carry its own elapsed hours; with the fixed clock it may, and a bad one is refused
    #[test]
    fn the_clock_owns_time() {
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap(); let st = persona::init(&p, None, true, &run);
        let (mut live, _) = Live::start(p.clone(), &doc, st.clone(), Clock::Real(std::time::Instant::now()), "t");
        assert_eq!(live.event(&json::parse(r#"{"elapsed_hours":1}"#).unwrap(), &run).unwrap_err().path, "event.elapsed_hours");
        let (_, line) = live.event(&json::parse(r#"{"praise":true}"#).unwrap(), &run).unwrap(); assert!(line.contains(r#""elapsed_hours":"#));
        let (mut fixed, _) = Live::start(p, &doc, st, Clock::Fixed, "t");
        assert_eq!(fixed.event(&json::parse(r#"{"elapsed_hours":-1}"#).unwrap(), &run).unwrap_err().path, "event.elapsed_hours");
        assert_eq!(fixed.n, 0, "a bad event changes nothing");
    }
}
