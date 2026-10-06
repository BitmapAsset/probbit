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
/// `verify` as one document: the summary, or {ok: false, line, diverges}
pub fn verify_doc(text: &str, eng: Engine) -> Json {
    verify(text, eng).unwrap_or_else(|(line, why)| Json::Obj(vec![("ok".into(), Json::Bool(false)), ("line".into(), Json::Num(line as f64)), ("diverges".into(), Json::Str(why))]))
}
/// This binary's engine (the strand header's `engine`)
pub fn engine() -> String { format!("probbit {}", env!("CARGO_PKG_VERSION")) }

/// Continue a strand: `text` is the strand so far and `st` the individual after its last line -> the individual, ready for the
/// next event (no header to write). The header's persona must be `p` (the same digest); the run uses the header's document, the
/// key order the strand replays with. The lines before are not replayed here (`verify` does that).
pub fn resume(text: &str, p: &Persona, st: State, clock: Clock) -> Result<Live, InErr> {
    let bad = |m: String| perr("strand", m);
    if !text.ends_with('\n') { return Err(bad("its last line is incomplete (no newline at the end)".into())); }
    let lines: Vec<&str> = text.split('\n').filter(|l| !l.is_empty()).collect();
    let h = json::parse(lines.first().copied().unwrap_or("")).map_err(|e| bad(format!("the header is not JSON: {}", e.msg)))?;
    if h.get("probbit_strand").and_then(Json::as_f64) != Some(FORMAT) { return Err(bad("not a probbit strand (format 1)".into())); }
    let q = persona::build(h.get("document").unwrap_or(&Json::Null)).map_err(|e| bad(format!("the header's persona: {}: {}", e.path, e.msg)))?;
    if q.digest != p.digest { return Err(bad(format!("it is another persona's ({} != {})", &q.digest[..19], &p.digest[..19]))); }
    let last = lines[lines.len() - 1];
    let (n, want) = if lines.len() == 1 { (0, h.get("state").and_then(|s| s.get("digest")).and_then(Json::as_str).unwrap_or("").to_string()) } else {
        let j = json::parse(last).map_err(|e| bad(format!("its last line is not JSON: {}", e.msg)))?;
        (j.get("n").and_then(Json::as_f64).unwrap_or(0.0) as u64, j.get("state").and_then(Json::as_str).unwrap_or("").to_string()) };
    if st.digest != want { return Err(perr("state", format!("not the strand's last state ({} != {})", &st.digest[..st.digest.len().min(19)], &want[..want.len().min(19)]))); }
    Ok(Live { p: q, st, clock, last: 0.0, prev: persona::digest_of(last), n })
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
    let (mut lv, header) = match &path { Some(f) => open(f, p, &doc, st, Clock::Fixed)?, None => (Live::start(p, &doc, st, Clock::Fixed, &engine()).0, None) };
    let (stance, line) = lv.event(&ev, eng).map_err(|e| perr(&format!("arguments.{}", e.path), e.msg))?;
    let mut out = vec![("stance".to_string(), stance), ("state".into(), lv.st.to_json(&lv.p))];
    if let Some(f) = path { let mut ls: Vec<&str> = header.iter().map(String::as_str).collect(); ls.push(&line); append(&f, &ls)?;
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

/// `probbit live PERSONA --demo week` (docs/persona.md §5.7): one individual's scripted week on the fixed clock. Days 1-3 praise
/// short answers and criticise long ones (the learned verbosity deltas go to their cap); day 3 ends upset and a quiet night
/// follows (the mood relaxes to this individual's resting level); days 4-7 try to teach it to joke about failures: every joke is
/// praised and every third event reports a failure (humour rises on the other turns; on a failure the habit keeps it at its
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
