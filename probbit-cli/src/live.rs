//! `probbit live` (docs/persona.md §5.7): a resident individual. Events come in as JSON objects of inputs; the clock stamps each
//! one's `elapsed_hours` (the time since the previous event, quantised to 1e-6 h), so moods decay by their half-lives while
//! nobody is talking to it, and feedback moves the learned deltas (§2.8). Every event appends one line to the strand: the
//! inputs as the turn used them, the stance's and the new state's digests, and the sha256 of the line before. The strand's
//! header carries the persona document, the initial state and the engine version, so `probbit live verify STRAND` replays the
//! whole life from the strand alone and names the earliest line that differs.
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

/// A live individual: the persona, the current state, the clock and the strand's chain
pub struct Live { pub p: Persona, pub st: State, clock: Clock, last: f64, prev: String, pub n: u64 }
impl Live {
    /// Start a life from `st` (an `init` state or a stored one) -> (the individual, the strand's header line). The header keeps
    /// the persona document in its own key order (the compiled program follows it, and `engine.program` with it) and the
    /// state as `to_json` writes it; it is compact JSON, not key-sorted.
    pub fn start(p: Persona, doc: &Json, st: State, clock: Clock, engine: &str) -> (Live, String) {
        let s = |x: &str| Json::Str(x.to_string());
        let header = persona::text(&Json::Obj(vec![("probbit_strand".into(), Json::Num(FORMAT)), ("engine".into(), s(engine)),
            ("persona".into(), Json::Obj(vec![("name".into(), s(&p.name)), ("version".into(), s(&p.version)), ("digest".into(), s(&p.digest))])),
            ("seed".into(), Json::Num(st.seed as f64)), ("state".into(), st.to_json(&p)), ("document".into(), doc.clone())]));
        let prev = persona::digest_of(&header);
        (Live { p, st, clock, last: 0.0, prev, n: 0 }, header)
    }
    /// One event: stamp its elapsed hours, run the turn, chain the strand line -> (the stance document, the strand line). A bad
    /// event is an error and changes nothing (no turn, no line).
    pub fn event(&mut self, ev: &Json, eng: Engine) -> Result<(Json, String), InErr> {
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
    /// The sha256 of the strand's last line (the header's before any event)
    pub fn head(&self) -> &str { &self.prev }
}

/// `probbit live verify`: replay a strand from its header -> Ok(summary) when every line is as recorded; Err((1-based line
/// number, what differs)) at the earliest line that is not
pub fn verify(text: &str, eng: Engine) -> Result<Json, (usize, String)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let lines = if lines.last() == Some(&"") { &lines[..lines.len() - 1] } else { &lines[..] };
    let head = lines.first().copied().unwrap_or("");
    let h = json::parse(head).map_err(|e| (1, format!("the header is not JSON: {}", e.msg)))?;
    if h.get("probbit_strand").and_then(Json::as_f64) != Some(FORMAT) { return Err((1, "not a probbit strand (format 1)".into())); }
    let doc = h.get("document").ok_or((1, "the header has no persona document".to_string()))?;
    let p = persona::build(doc).map_err(|e| (1, format!("the persona document: {}: {}", e.path, e.msg)))?;
    if h.get("persona").and_then(|x| x.get("digest")).and_then(Json::as_str) != Some(p.digest.as_str()) { return Err((1, "the persona digest does not match the document".into())); }
    let st0 = State::read(&p, h.get("state").unwrap_or(&Json::Null)).map_err(|e| (1, format!("the initial state: {}: {}", e.path, e.msg)))?;
    let (mut live, header) = Live::start(p, doc, st0, Clock::Fixed, h.get("engine").and_then(Json::as_str).unwrap_or(""));
    if header != head { return Err((1, "the header is not as written".into())); }
    for (i, l) in lines.iter().enumerate().skip(1) {
        let j = json::parse(l).map_err(|e| (i + 1, format!("not JSON: {}", e.msg)))?;
        if persona::canon(&j) != *l { return Err((i + 1, "not canonical JSON".into())); }
        if j.get("prev").and_then(Json::as_str) != Some(live.head()) { return Err((i + 1, "prev is not the sha256 of the line before (a line before it was changed, removed or reordered)".into())); }
        if j.get("n").and_then(Json::as_f64) != Some((live.n + 1) as f64) { return Err((i + 1, format!("n is not {}", live.n + 1))); }
        let ev = j.get("inputs").ok_or((i + 1, "no inputs".to_string()))?;
        let (stance, line) = live.event(ev, eng).map_err(|e| (i + 1, format!("{}: {}", e.path, e.msg)))?;
        if j.get("stance").and_then(Json::as_str) != Some(persona::sha(&stance).as_str()) { return Err((i + 1, "the stance differs".into())); }
        if j.get("state").and_then(Json::as_str) != Some(live.st.digest.as_str()) { return Err((i + 1, "the state differs".into())); }
        if line != *l { return Err((i + 1, "the line differs".into())); }
    }
    let s = |x: &str| Json::Str(x.to_string());
    Ok(Json::Obj(vec![("ok".into(), Json::Bool(true)), ("events".into(), Json::Num(live.n as f64)), ("persona".into(), s(&live.p.name)), ("seed".into(), Json::Num(live.st.seed as f64)),
        ("engine".into(), h.get("engine").cloned().unwrap_or(Json::Null)), ("final_state".into(), s(&live.st.digest)), ("last_line".into(), s(live.head()))]))
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

    fn strand(n: usize) -> (String, Vec<Json>) {
        let doc = json::parse(DOC).unwrap(); let p = persona::build(&doc).unwrap(); let st = persona::init(&p, Some(2), true, &run);
        let (mut live, header) = Live::start(p, &doc, st, Clock::Fixed, "probbit test");
        let (mut text, mut stances) = (format!("{header}\n"), vec![]);
        for t in 0..n { let ev = json::parse(&format!(r#"{{"praise":{},"loss":{},"elapsed_hours":{}}}"#, t % 2 == 1, t % 5 == 4, if t % 7 == 6 { "9.1234567" } else { "0.25" })).unwrap();
            let (s, line) = live.event(&ev, &run).unwrap(); text.push_str(&line); text.push('\n'); stances.push(s); }
        (text, stances)
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
