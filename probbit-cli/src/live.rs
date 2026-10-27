//! `probbit live` (docs/persona.md §5.7): a resident individual. Events come in as JSON objects of inputs; the clock stamps each
//! one's `elapsed_hours` (the time since the previous event, quantised to 1e-6 h), so moods decay by their half-lives while
//! nobody is talking to it, and feedback moves the learned deltas (§2.8). Every event appends one line to the strand: the
//! inputs as the turn used them, the stance's and the new state's digests, and the sha256 of the line before. The strand's
//! header carries the persona document, the initial state and the engine version, so `probbit live verify STRAND` replays the
//! whole life from the strand alone and names the earliest line that differs, or the earliest line with a field or line kind
//! this engine does not know (`unsupported`, a newer engine's strand: not verified, and not a divergence).
//!
//! The safety kit (§5.7): one writer per strand (`STRAND.lock`), control lines (pause, resume, retire: appended by `probbit live
//! control`, never by an event; no credit crosses them) and checkpoint lines (the whole state every K events, so a replay can
//! start there). `verify` replays all three.
//!
//! The pure half (the line writers, `Live`, `Replay`, `verify`, the chain checks) is `probbit_persona::strand`, so a firmware or
//! another runtime writes and checks the same strand; this file is the CLI's half: strand files, the writer lock, the tools of
//! `probbit mcp` and the demo week.
pub use probbit_persona::strand::*;
use crate::json::{self, InErr, Json};
use crate::persona::{self, perr, Engine, Persona, State};

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

/// `probbit live control STRAND seal` (§5.7, "Segments"): a host decides when to seal. Under the writer lock, the newest segment
/// of STRAND (`NAME.strand`, `NAME.0001.strand`, ...) is replayed (from its last checkpoint, else from its start); refused unless
/// its last line is an event line (a seal follows an event); then the sealing checkpoint is appended to it and the next segment
/// written: its header (`segment` {index, previous, sealed}) and the sealing line -> {ok, control: seal, segment, file, line, head}
pub fn seal_cmd(path: &str, eng: Engine) -> Result<Json, InErr> {
    let lock = Lock::take(path)?;
    let mut k = 0u64; while std::path::Path::new(&segment_name(path, k + 1)).exists() { k += 1; }
    let cur = segment_name(path, k);
    let text = std::fs::read_to_string(&cur).map_err(|e| perr("strand", format!("cannot read {cur}: {e}")))?;
    if !text.ends_with('\n') { return Err(perr("strand", format!("{cur}: its last line is incomplete (no newline at the end)"))); }
    let ls = lines(&text);
    let last = ls.last().and_then(|l| json::parse(l).ok()).filter(|_| ls.len() > 1);
    if !last.as_ref().is_some_and(|j| matches!(kind(j), Kind::Event)) { return Err(perr("strand", format!("{cur}: its last line is not an event line (a seal follows an event)"))); }
    let at = |(i, why): (usize, String)| perr("strand", format!("{cur}: line {i}: {why}"));
    let (mut r, from) = match last_checkpoint(&ls, 0).filter(|&c| c >= 2).and_then(|c| Replay::open_at(ls[0], ls[c - 1], ls[c], c + 1).ok().map(|r| (r, c + 1))) {
        Some(x) => x, None => Replay::open_lines(&ls).map_err(at)? };
    for l in &ls[from..] { r.step(l, eng).map_err(at)?; }
    if r.live.expect.as_ref().is_some_and(|m| !m.is_canonical()) { return Err(perr("strand", "before engine 0.14.0 a checkpoint does not carry the expectation's memory, so a segment of a persona with an expectation block could not start from its sealed line")); }
    let stance = r.last_stance().cloned().ok_or_else(|| perr("strand", "no event replayed"))?;
    let cp = r.live.checkpoint(&stance);
    let next = segment_name(path, k + 1);
    let pname = std::path::Path::new(&cur).file_name().map_or_else(|| cur.clone(), |f| f.to_string_lossy().into_owned());
    let engine = r.header.get("engine").and_then(Json::as_str).unwrap_or("").to_string();
    let h = segment_header(&r.live, &r.doc, &engine, k + 1, &pname, &persona::digest_of(&cp));
    if !lock.held() { return Err(locked(None, &format!("{path}.lock"))); }
    append(&cur, &[&cp])?; append(&next, &[&h, &cp])?;
    let s = |x: &str| Json::Str(x.to_string());
    Ok(Json::Obj(vec![("ok".into(), Json::Bool(true)), ("control".into(), s("seal")), ("segment".into(), Json::Num((k + 1) as f64)), ("file".into(), s(&next)),
        ("line".into(), Json::Num((ls.len() + 1) as f64)), ("head".into(), s(&persona::digest_of(&cp)))]))
}

/// The tools `probbit_live_event` and `probbit_live_verify` (`probbit mcp`), stateless as the persona tools. event: the persona
/// (a document or a path), the state (or a seed: a new individual), one event (its inputs; `elapsed_hours` comes from the host's
/// clock and is quantised here) and optionally a strand file on the server's disk (a new one gets the header; an existing one is
/// continued and `state` must be its last state) -> {stance, state} (+ strand {path, events, head}). verify: a strand (its text
/// or a path) -> `verify_doc` (a divergence is an answer, and so is an unsupported strand).
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

    /// RFC 0009 (effects {}): the expectation block leaves every stance and state digest of the life as without it; its strand
    /// lines carry the deviation receipt, `verify` recomputes it (an edited digit diverges as "the deviation differs"), `resume`
    /// folds the same memory, the bound -log w(c_t) holds term by term, and the cold start reads 0 until the first non-time input
    #[test]
    fn expectation_receipts_replay_and_bound() {
        let off = json::parse(DOC).unwrap(); let mut on = off.clone();
        if let Json::Obj(v) = &mut on { v.push(("expectation".into(), Json::Obj(vec![]))); v.push(("engine".into(), json::parse(r#"{"trajectory":{"k":5,"beta0":0.3}}"#).unwrap())); }
        let (po, pn) = (persona::build(&off).unwrap(), persona::build(&on).unwrap()); assert_eq!(po.digest, pn.digest, "the blocks stay out of the digest");
        let evs: Vec<Json> = (0..24).map(|t| if t < 3 { json::parse(r#"{"elapsed_hours":1}"#).unwrap() } else { ev(t) }).collect();
        let run_arm = |p: &Persona, d: &Json| { let (mut live, header) = Live::start(p.clone(), d, persona::init(p, Some(2), true, &run), Clock::Fixed, "probbit test");
            let mut text = format!("{header}\n"); let mut docs = vec![];
            for e in &evs { let (s, l) = live.event(e, &run).unwrap(); text += &l; text.push('\n'); docs.push((s, live.traj.clone())); } (text, docs) };
        let ((t0, d0), (t1, d1)) = (run_arm(&po, &off), run_arm(&pn, &on));
        for (a, b) in t0.lines().skip(1).zip(t1.lines().skip(1)) { let (a, b) = (json::parse(a).unwrap(), json::parse(b).unwrap());
            assert_eq!((a.get("stance"), a.get("state")), (b.get("stance"), b.get("state"))); assert!(a.get("deviation").is_none() && b.get("deviation").is_some()); }
        for (k, ((s0, _), (s1, tj))) in d0.iter().zip(&d1).enumerate() {
            assert_eq!(persona::stance_sha(s1), persona::sha(s0)); let dv = s1.get("deviation").unwrap(); let g = |x: &str| dv.get(x).and_then(Json::as_f64).unwrap();
            assert_eq!(dv.get("cold"), Some(&Json::Bool(k < 3)), "event {k}"); if k < 3 { assert_eq!(g("pred"), 0.0); } else { assert!(g("pred") > 1e-6, "event {k}: {dv:?}"); }
            assert_eq!(tj.as_ref().and_then(|t| t.get("steps")).and_then(Json::as_arr).map(|a| a.len()), Some(5)); }
        assert_eq!(verify(&t1, &run).map(|d| d.get("events").cloned()), Ok(Some(Json::Num(24.0))));
        // one edited digit of a logged deviation diverges at its line
        let ls: Vec<&str> = t1.lines().collect(); let at = ls.iter().position(|l| l.contains(r#""pred":0."#) && !l.contains("cold")).unwrap();
        let k = ls[at].find(r#""pred":0."#).unwrap() + 10; let mut bad = ls[at].to_string(); let c = bad.as_bytes()[k]; bad.replace_range(k..k + 1, if c == b'9' { "8" } else { "9" });
        let mut tt: Vec<String> = ls.iter().map(|l| l.to_string()).collect(); tt[at] = bad; let tampered = tt.join("\n") + "\n";
        assert_eq!(verify(&tampered, &run).err().map(|e| e.1), Some("the deviation differs".to_string()));
        // resume folds the same memory: the next event's line is the one the continuous life writes
        let cut: String = ls[..20].iter().map(|l| format!("{l}\n")).collect();
        let st20 = { let mut r = Replay::open(ls[0]).unwrap(); for l in &ls[1..20] { r.step(l, &run).unwrap(); } r.live.st.clone() };
        let mut lv = resume(&cut, &pn, st20, Clock::Fixed).unwrap(); let (_, l20) = lv.event(&evs[19], &run).unwrap(); assert_eq!(l20, ls[20]);
        // the bound (P9), term by term: a member combo reads at most -ln w(c_t) per variable; a new one at most self_i - ln w_empty
        let mut mem = persona::ExpectMem::new(); let mut st = persona::init(&pn, Some(2), true, &run);
        for e in &evs { let d = persona::deviation(&pn, &st, e, &mem, &run).unwrap();
            for (_, x, y) in &d.terms { assert!(*x <= if d.member { d.input } else { y + d.input } + 1e-9, "{d:?}"); }
            persona::expect_step(&pn, &mut mem, e, d.logged_pred()); st = persona::turn(&pn, &st, e, false, &run, false).unwrap().1; }
    }

    /// 0.14.0, the canonical fold (docs/persona.md §5.7): a checkpoint of a persona with an expectation block carries the memory
    /// (`expect`), so `verify --from-checkpoint` and `resume` start there: the life with a checkpoint after every 7th event gives
    /// the stances, the state and the memory of the life without them, verifies from its header and from its last checkpoint to
    /// one summary, and a resume writes the continuous life's next line; the memory never holds more than max_combos - 1 named
    /// combos; an edited memory diverges at its checkpoint; a 0.13.0 strand keeps the four-key checkpoint and the refusal
    #[test]
    fn a_checkpoint_carries_the_expectation_memory() {
        let mut on = json::parse(DOC).unwrap();
        if let Json::Obj(v) = &mut on { v.push(("expectation".into(), json::parse(r#"{"max_combos":3}"#).unwrap())); }
        let p = persona::build(&on).unwrap();
        let evs: Vec<Json> = (0..60).map(|t| json::parse(&format!(r#"{{"praise":{},"criticism":{},"loss":{},"elapsed_hours":0.5}}"#, t % 2 == 1, t % 3 == 1, t % 5 == 4)).unwrap()).collect();
        let life = |engine: &str, every: u64| { let (mut lv, head) = Live::start(p.clone(), &on, persona::init(&p, Some(2), true, &run), Clock::Fixed, engine);
            let (mut text, mut out) = (format!("{head}\n"), vec![]);
            for e in &evs { let (s, l) = lv.event(e, &run).unwrap(); text += &l; text.push('\n'); if every > 0 && lv.n % every == 0 { text += &lv.checkpoint(&s); text.push('\n'); } out.push(s); }
            (text, out, lv) };
        let ((_, s0, l0), (cp, s1, l1)) = (life("probbit 0.14.0", 0), life("probbit 0.14.0", 7));
        assert_eq!(s0, s1, "checkpoints move no stance"); let mem = |l: &Live| l.expect.as_ref().map(persona::ExpectMem::to_json);
        assert_eq!(l0.st.digest, l1.st.digest); assert_eq!(mem(&l0), mem(&l1));
        let ls: Vec<&str> = cp.lines().collect(); let cps: Vec<usize> = (0..ls.len()).filter(|&i| ls[i].starts_with(r#"{"checkpoint":"#)).collect(); assert_eq!(cps.len(), 8);
        for &i in &cps { let j = json::parse(ls[i]).unwrap(); let m = persona::ExpectMem::read(&p, j.get("expect").expect("expect on a 0.14.0 checkpoint")).unwrap();
            assert!(m.to_json().get("combos").and_then(Json::as_arr).unwrap().len() <= 3, "{}", ls[i]); }
        let (full, fast) = (verify(&cp, &run).unwrap(), verify_from_checkpoint(&cp, &run).unwrap());
        assert_eq!(fast.get("from_checkpoint"), Some(&Json::Num(56.0)));
        for k in ["events", "final_state", "last_line"] { assert_eq!(full.get(k), fast.get(k), "{k}"); }
        // resume from the text before the last event (the last checkpoint in it) and the state there: the continuous life's line
        let cut: String = ls[..ls.len() - 1].iter().map(|l| format!("{l}\n")).collect();
        let st = { let mut r = Replay::open(ls[0]).unwrap(); for l in &ls[1..ls.len() - 1] { r.step(l, &run).unwrap(); } r.live.st.clone() };
        let mut lv = resume(&cut, &p, st, Clock::Fixed).unwrap(); let (_, last) = lv.event(&evs[59], &run).unwrap();
        assert_eq!(last, ls[ls.len() - 1]); assert_eq!(mem(&lv), mem(&l1));
        // an edited memory (the chain re-made after it) diverges at its checkpoint in a full verify
        let at = cps[3]; let mut j = json::parse(ls[at]).unwrap(); let n = j.get("expect").and_then(|e| e.get("n")).and_then(Json::as_f64).unwrap();
        put(&mut j, &["expect", "n"], Json::Num(n + 1.0)); let mut tt: Vec<String> = ls.iter().map(|l| l.to_string()).collect(); tt[at] = persona::canon(&j);
        assert_eq!(verify(&rechain(tt, at), &run).unwrap_err(), (at + 1, "the checkpoint's state or stance differs from the replay".to_string()));
        // before 0.14.0: four keys, and a replay from a checkpoint refuses
        let (old, _, _) = life("probbit 0.13.0", 7);
        assert!(old.lines().filter(|l| l.starts_with(r#"{"checkpoint":"#)).all(|l| !l.contains(r#""expect":"#)));
        assert!(verify_from_checkpoint(&old, &run).unwrap_err().1.contains("expectation block reads every event"));
        assert_eq!(verify(&old, &run).map(|d| d.get("checkpoints").cloned()), Ok(Some(Json::Num(8.0))));
    }

    /// Event t of the S1 lives on the expectation fixture persona (tests/fixtures/monitor/tutor-expect.yaml): a hash of t picks the
    /// sentiment, six flags and two numbers' thirds, so the lives see many input combos
    fn s1_event(t: usize) -> Json {
        let h = (t as u64).wrapping_mul(2654435761) >> 7; let b = |k: u64| (h >> k) & 1 == 1;
        json::parse(&format!(r#"{{"sentiment":"{}","confused":{},"error":{},"loss":{},"praise":{},"criticism":{},"claim_done":{},"stakes":{},"time_pressure":{},"elapsed_hours":{}}}"#,
            ["negative", "neutral", "positive"][(h % 3) as usize], b(2), b(3), b(4) && b(5), b(6), b(7) && b(8), b(9) && b(10), [0.1, 0.5, 0.9][((h >> 11) % 3) as usize],
            [0.0, 0.5, 0.9][((h >> 13) % 3) as usize], [0.1, 0.5, 2.0][((h >> 15) % 3) as usize])).unwrap()
    }
    /// Segments with an expectation block (0.14.0, docs/persona.md §5.7): 500 events of the expectation fixture persona logged as one
    /// file with a checkpoint after every 50th, and as segments of 50 events (each file: a header linking the previous one, the
    /// sealed line, the events, its own sealed line): every stance document and the final state are the same, the lines joined
    /// (each segment's header and repeated sealed line set aside) are the one file's, each segment verifies from its link to the
    /// final state of the one file, a resume in the newest segment writes the one file's next line, and a segment whose second
    /// line is not its header's sealed line diverges at line 2
    #[test]
    fn segments_of_an_expectation_life_equal_one_file() {
        let (p, doc) = persona::load(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/monitor/tutor-expect.yaml")).unwrap(); assert!(p.expect.is_some());
        let start = || Live::start(p.clone(), &doc, persona::init(&p, Some(2), true, &run), Clock::Fixed, "probbit 0.14.0");
        let ((mut a, ha), (mut b, hb)) = (start(), start());
        let (mut one, mut segs, mut sa, mut sb) = (format!("{ha}\n"), vec![format!("{hb}\n")], vec![], vec![]);
        for t in 0..500 {
            let (s, l) = a.event(&s1_event(t), &run).unwrap(); one += &l; one.push('\n'); if a.n % 50 == 0 { one += &a.checkpoint(&s); one.push('\n'); } sa.push(persona::canon(&s));
            let (s, l) = b.event(&s1_event(t), &run).unwrap(); let cur = segs.last_mut().unwrap(); *cur += &l; cur.push('\n');
            if b.n % 50 == 0 { let c = b.checkpoint(&s); *cur += &c; cur.push('\n'); let k = segs.len() as u64;
                let h = segment_header(&b, &doc, "probbit 0.14.0", k, &segment_name("life.strand", k - 1), &persona::digest_of(&c)); segs.push(format!("{h}\n{c}\n")); }
            sb.push(persona::canon(&s)); }
        assert_eq!(sa, sb, "every stance document"); assert_eq!(a.st.digest, b.st.digest); assert_eq!(segs.len(), 11);
        let mut joined: Vec<&str> = segs[0].lines().collect(); for t in &segs[1..] { joined.extend(t.lines().skip(2)); }
        assert_eq!(joined, one.lines().collect::<Vec<_>>(), "the segments are the one file");
        let fin = verify(&one, &run).unwrap(); assert_eq!(fin.get("events"), Some(&Json::Num(500.0)));
        for (k, t) in segs.iter().enumerate() { let v = verify(t, &run).unwrap_or_else(|e| panic!("segment {k}: {e:?}"));
            if k > 0 { assert_eq!(v.get("segment"), Some(&Json::Num(k as f64))); } if k == 10 { assert_eq!(v.get("final_state"), fin.get("final_state")); } }
        let mut r = resume(&segs[10], &p, b.st.clone(), Clock::Fixed).unwrap(); let e = s1_event(500);
        assert_eq!(r.event(&e, &run).unwrap().1, a.event(&e, &run).unwrap().1, "a resume in the newest segment");
        let mut l3: Vec<&str> = segs[3].lines().collect(); let other = segs[4].lines().nth(1).unwrap(); l3[1] = other;
        assert_eq!(verify(&(l3.join("\n") + "\n"), &run).unwrap_err(), (2, SEALED.to_string()));
    }

    /// S1 measurements (ignored: run with --ignored --nocapture): the expectation fixture persona, 500 events, a checkpoint every
    /// 50: checkpoint line bytes (max_combos 16 and 64), and resume's time at 166 and 500 events, 0.13.0 (fold from the header)
    /// against 0.14.0 (fold from the last checkpoint), with the full replay for scale
    #[test]
    #[ignore]
    fn s1_measure() {
        let y = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/monitor/tutor-expect.yaml")).unwrap();
        let doc0 = probbit_persona::yaml::load(&y).unwrap_or_else(|e| panic!("{}", e.msg));
        let ev = s1_event;
        for mc in [16usize, 64] {
            let mut d = doc0.clone(); put(&mut d, &["expectation"], json::parse(&format!(r#"{{"max_combos":{mc}}}"#)).unwrap()); let p = persona::build(&d).unwrap();
            for engine in ["probbit 0.13.0", "probbit 0.14.0"] {
                let (mut lv, head) = Live::start(p.clone(), &d, persona::init(&p, Some(2), true, &run), Clock::Fixed, engine);
                let (mut text, mut ev_b, mut cp_max, mut sts) = (format!("{head}\n"), 0usize, 0usize, vec![]);
                for t in 0..500 { let (s, l) = lv.event(&ev(t), &run).unwrap(); ev_b = ev_b.max(l.len()); text += &l; text.push('\n');
                    if lv.n % 50 == 0 { let c = lv.checkpoint(&s); cp_max = cp_max.max(c.len()); text += &c; text.push('\n'); } if lv.n == 166 || lv.n == 500 { sts.push((text.clone(), lv.st.clone())); } }
                let named = lv.expect.as_ref().map(|m| m.to_json().get("combos").and_then(Json::as_arr).map_or(0, |a| a.len())).unwrap_or(0);
                println!("S1 max_combos={mc} engine={engine}: header {} B, event line max {ev_b} B, checkpoint line max {cp_max} B, memory entries at 500: {named}", head.len());
                for (n, (t, st)) in [166usize, 500].into_iter().zip(&sts) {
                    let t0 = std::time::Instant::now(); let r = resume(t, &p, st.clone(), Clock::Fixed).unwrap(); let dt = t0.elapsed(); drop(r);
                    let t1 = std::time::Instant::now(); let v = verify(t, &run).unwrap(); let dv = t1.elapsed(); assert_eq!(v.get("events"), Some(&Json::Num(n as f64)));
                    let t2 = std::time::Instant::now(); let f = verify_from_checkpoint(t, &run); let df = t2.elapsed();
                    println!("S1   n={n}: resume {:.1} ms, verify from header {:.1} ms, verify --from-checkpoint {} ({:.1} ms)", dt.as_secs_f64() * 1e3, dv.as_secs_f64() * 1e3,
                        if f.is_ok() { "ok" } else { "refused" }, df.as_secs_f64() * 1e3); }
            }
        }
    }

    /// §2.18: a persona with two moods and a habit that binds on a loss
    const GDOC: &str = r#"{"probbit_persona":1,"identity":{"name":"G","version":"1","seed":5},
        "traits":[{"id":"humour","levels":["none","light","playful"],"logw":[0,1.0,0.8]},{"id":"caution","levels":["bold","measured","careful"],"logw":[0.3,0.4,0]}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.5,"half_life_hours":6},{"id":"pressure","levels":["calm","steady","keyed"],"inertia":0.5,"half_life_hours":3}],
        "couplings":[{"vars":["pressure","caution"],"align":0.8}],
        "inputs":[{"id":"praise","kind":"flag","effects":{"valence":1.5}},{"id":"loss","kind":"flag","effects":{"valence":-1.5,"pressure":1.2}},{"id":"rush","kind":"flag","effects":{"pressure":1.5}}],
        "habits":[{"id":"no_jokes_on_loss","when":{"loss":true},"then":{"humour":["none"]}}]}"#;
    const GLYPHS: &str = r#"{"glyphs":{"moods":{"valence":["down","even","up"],"pressure":["calm","steady","keyed"]},
        "table":{"down":["😔","😕","😣"],"even":["😌","🙂","😬"],"up":["😊","😄","🤩"]},"surprise":{"glyph":"👀","z_at_least":2.0},"habits":{"no_jokes_on_loss":"🛟"}}}"#;
    fn with_display(doc: &Json, extra: &[(&str, &str)]) -> Json {
        let mut d = doc.clone(); if let Json::Obj(v) = &mut d { for (k, x) in extra { v.push((k.to_string(), json::parse(x).unwrap())); } } d
    }
    fn gev(t: usize) -> Json {
        let (praise, loss, rush) = (t % 3 == 1, t % 7 == 5, t % 4 == 2);
        json::parse(&format!(r#"{{"praise":{praise},"loss":{loss},"rush":{rush},"elapsed_hours":{}}}"#, if t % 9 == 8 { "7.5" } else { "0.2" })).unwrap()
    }
    fn glife(p: &Persona, d: &Json, evs: &[Json]) -> (String, Vec<Json>) {
        let (mut live, header) = Live::start(p.clone(), d, persona::init(p, Some(1), true, &run), Clock::Fixed, "probbit test");
        let (mut text, mut docs) = (format!("{header}\n"), vec![]);
        for e in evs { let (s, l) = live.event(e, &run).unwrap(); text += &l; text.push('\n'); docs.push(s); }
        (text, docs)
    }

    /// §2.18 (effects {}): `display.glyphs` stays out of the digest and moves nothing: every stance and state digest of the life
    /// is the one without it and each document is the one without it plus `glyph` and `glyph_why`. The glyph follows the rule
    /// order, the strand line logs it, `verify` recomputes it (an edited glyph diverges as "the glyph differs"), a checkpoint
    /// strand verifies from the checkpoint and `resume` continues the same bytes
    #[test]
    fn glyphs_are_receipts_and_move_nothing() {
        let off = json::parse(GDOC).unwrap(); let on = with_display(&off, &[("display", GLYPHS)]);
        let (po, pn) = (persona::build(&off).unwrap(), persona::build(&on).unwrap()); assert_eq!(po.digest, pn.digest, "display stays out of the digest");
        let evs: Vec<Json> = (0..40).map(gev).collect();
        let ((t0, d0), (t1, d1)) = (glife(&po, &off, &evs), glife(&pn, &on, &evs));
        for (a, b) in t0.lines().skip(1).zip(t1.lines().skip(1)) { let (a, b) = (json::parse(a).unwrap(), json::parse(b).unwrap());
            assert_eq!((a.get("stance"), a.get("state")), (b.get("stance"), b.get("state"))); assert!(a.get("glyph").is_none() && b.get("glyph").is_some()); }
        let (mut habit, mut cells) = (0, std::collections::BTreeSet::new());
        for (s0, s1) in d0.iter().zip(&d1) {
            assert!(s0.get("glyph").is_none());
            let Json::Obj(v) = s1 else { panic!() }; let bare = Json::Obj(v.iter().filter(|(k, _)| k != "glyph" && k != "glyph_why").cloned().collect());
            assert_eq!(persona::canon(&bare), persona::canon(s0), "the document is the one without the block, plus the glyph");
            let (g, why) = (s1.get("glyph").and_then(Json::as_str).unwrap(), s1.get("glyph_why").and_then(Json::as_str).unwrap());
            let active = s1.get("habits").and_then(|h| h.get("active")).and_then(Json::as_arr).unwrap();
            if active.iter().any(|b| b.as_str() == Some("no_jokes_on_loss")) { assert_eq!((g, why), ("🛟", "habit no_jokes_on_loss")); habit += 1; continue; }
            let lv = |m: &str| s1.get("mood").and_then(|x| x.get(m)).and_then(|e| e.get("level")).and_then(Json::as_str).unwrap().to_string();
            let (va, pr) = (lv("valence"), lv("pressure"));
            let row = ["down", "even", "up"].iter().position(|x| *x == va).unwrap(); let col = ["calm", "steady", "keyed"].iter().position(|x| *x == pr).unwrap();
            let want = [["😔", "😕", "😣"], ["😌", "🙂", "😬"], ["😊", "😄", "🤩"]][row][col];
            assert_eq!((g, why), (want, format!("mood valence={va} pressure={pr}").as_str())); cells.insert(want); }
        assert!(habit > 0 && cells.len() >= 3, "the life must exercise the habit rule and several cells: {habit} {cells:?}");
        assert_eq!(verify(&t1, &run).map(|d| d.get("events").cloned()), Ok(Some(Json::Num(40.0))));
        // an edited glyph, the chain re-made after it, diverges at its line
        let ls: Vec<String> = t1.lines().map(str::to_string).collect();
        let mut j = json::parse(&ls[7]).unwrap(); let was = j.get("glyph").and_then(Json::as_str).unwrap().to_string();
        put(&mut j, &["glyph"], Json::Str(if was == "🤩" { "😊" } else { "🤩" }.into())); let mut tt = ls.clone(); tt[7] = persona::canon(&j);
        assert_eq!(verify(&rechain(tt, 8), &run).unwrap_err(), (8, "the glyph differs".to_string()));
        // a removed glyph diverges too (the line's other fields are as the replay gives them)
        let mut j = json::parse(&ls[3]).unwrap(); if let Json::Obj(v) = &mut j { v.retain(|(k, _)| k != "glyph"); } let mut tt = ls.clone(); tt[3] = persona::canon(&j);
        assert_eq!(verify(&rechain(tt, 4), &run).unwrap_err(), (4, "the glyph differs".to_string()));
        // checkpoints: the strand with one verifies in full and from the checkpoint
        let (mut live, header) = Live::start(pn.clone(), &on, persona::init(&pn, Some(1), true, &run), Clock::Fixed, "probbit test"); let mut text = format!("{header}\n");
        for (k, e) in evs.iter().enumerate() { let (s, l) = live.event(e, &run).unwrap(); text += &l; text.push('\n'); if k == 19 { text += &live.checkpoint(&s); text.push('\n'); } }
        assert!(verify(&text, &run).is_ok()); assert_eq!(verify_from_checkpoint(&text, &run).map(|d| d.get("from_checkpoint").cloned()), Ok(Some(Json::Num(20.0))));
        // resume continues the same bytes
        let cut: String = ls[..21].iter().map(|l| format!("{l}\n")).collect();
        let st20 = { let mut r = Replay::open(&ls[0]).unwrap(); for l in &ls[1..21] { r.step(l, &run).unwrap(); } r.live.st.clone() };
        let mut lv = resume(&cut, &pn, st20, Clock::Fixed).unwrap(); let (_, l21) = lv.event(&evs[20], &run).unwrap(); assert_eq!(l21, ls[21]);
    }

    /// §2.18: the habit rule reads the habit in force (`habits.active`), not `habits.bound`: with an input that already takes the
    /// stance where the habit points, the habit applies but does not bind, and its glyph still shows on every event it applies;
    /// on the others the table's cell
    #[test]
    fn the_habit_glyph_reads_the_habit_in_force() {
        let off = json::parse(&GDOC.replace(r#""effects":{"valence":-1.5,"pressure":1.2}"#, r#""effects":{"valence":-1.5,"pressure":1.2,"humour":-8}"#)).unwrap();
        let on = with_display(&off, &[("display", GLYPHS)]); let pn = persona::build(&on).unwrap();
        let evs: Vec<Json> = (0..40).map(gev).collect();
        let (text, docs) = glife(&pn, &on, &evs);
        let (mut applies, mut not_bound) = (0, 0);
        for (d, e) in docs.iter().zip(&evs) {
            let has = |k: &str| d.get("habits").and_then(|h| h.get(k)).and_then(Json::as_arr).unwrap().iter().any(|b| b.as_str() == Some("no_jokes_on_loss"));
            let (g, why) = (d.get("glyph").and_then(Json::as_str).unwrap(), d.get("glyph_why").and_then(Json::as_str).unwrap());
            assert_eq!(has("active"), e.get("loss") == Some(&Json::Bool(true)));
            if has("active") { applies += 1; if !has("bound") { not_bound += 1; } assert_eq!((g, why), ("🛟", "habit no_jokes_on_loss")); }
            else { assert!(why.starts_with("mood ") && g != "🛟", "{why}"); } }
        assert!(applies > 0 && not_bound == applies, "the habit must apply without binding: {applies} {not_bound}");
        assert!(verify(&text, &run).is_ok());
    }

    /// §2.18, the cost of the glyph: the strands named in PROBBIT_GLYPH_COST (`:`-separated) are replayed from their headers'
    /// documents without and with the `display` block in PROBBIT_GLYPH_DISPLAY (a JSON file), the two arms interleaved per event
    /// (order alternating), 5 passes; prints p50 and p95 of `Live::event` per arm in ms. Release build:
    /// `cargo test --release -p probbit-cli --bin probbit glyph_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn glyph_cost() {
        let (Ok(paths), Ok(dp)) = (std::env::var("PROBBIT_GLYPH_COST"), std::env::var("PROBBIT_GLYPH_DISPLAY")) else { return };
        let display = json::parse(&std::fs::read_to_string(dp).unwrap()).unwrap();
        let (mut off_ms, mut on_ms) = (vec![], vec![]);
        for path in paths.split(':') {
            let text = std::fs::read_to_string(path).unwrap(); let ls = lines(&text);
            let h = json::parse(ls[0]).unwrap(); let doc = h.get("document").unwrap().clone();
            let mut on = doc.clone(); if let Json::Obj(v) = &mut on { v.push(("display".into(), display.clone())); }
            let evs: Vec<Json> = ls[1..].iter().filter_map(|l| json::parse(l).ok()).filter(|j| matches!(kind(j), Kind::Event)).map(|j| j.get("inputs").unwrap().clone()).collect();
            let start = |d: &Json| { let p = persona::build(d).unwrap(); let st = State::read(&p, h.get("state").unwrap()).unwrap(); Live::start(p, d, st, Clock::Fixed, "probbit test").0 };
            for _pass in 0..5 {
                let (mut a, mut b) = (start(&doc), start(&on));
                for (k, e) in evs.iter().enumerate() {
                    let one = |lv: &mut Live, out: &mut Vec<f64>| { let t = std::time::Instant::now(); lv.event(e, &run).unwrap(); out.push(t.elapsed().as_secs_f64() * 1e3); };
                    if k % 2 == 0 { one(&mut a, &mut off_ms); one(&mut b, &mut on_ms); } else { one(&mut b, &mut on_ms); one(&mut a, &mut off_ms); } } } }
        let q = |xs: &mut Vec<f64>, f: f64| { xs.sort_by(|a, b| a.partial_cmp(b).unwrap()); xs[((xs.len() as f64 * f).ceil() as usize).clamp(1, xs.len()) - 1] };
        let (n, p50_off, p95_off, p50_on, p95_on) = (off_ms.len(), q(&mut off_ms, 0.5), q(&mut off_ms, 0.95), q(&mut on_ms, 0.5), q(&mut on_ms, 0.95));
        println!("GLYPH_COST {{\"events_per_arm\":{n},\"off\":{{\"p50_ms\":{p50_off:.4},\"p95_ms\":{p95_off:.4}}},\"on\":{{\"p50_ms\":{p50_on:.4},\"p95_ms\":{p95_on:.4}}},\"p95_diff_ms\":{:.4}}}", p95_on - p95_off);
    }

    /// §2.18: with an expectation block the surprise modifier follows the deviation it reads (registers and z >= z_at_least),
    /// after the base glyph, and `glyph_why` names the z; a floor no event reaches gives the bare base glyph
    #[test]
    fn the_surprise_modifier_reads_the_deviation() {
        let off = json::parse(GDOC).unwrap();
        let on = with_display(&off, &[("display", GLYPHS), ("expectation", "{}")]);
        let pn = persona::build(&on).unwrap();
        let evs: Vec<Json> = (0..60).map(|t| if t % 17 == 16 { json::parse(r#"{"loss":true,"rush":true,"praise":true,"elapsed_hours":0.1}"#).unwrap() } else { json::parse(r#"{"elapsed_hours":0.1}"#).unwrap() }).collect();
        let (text, docs) = glife(&pn, &on, &evs);
        let mut fired = 0;
        for d in &docs { let (g, why) = (d.get("glyph").and_then(Json::as_str).unwrap(), d.get("glyph_why").and_then(Json::as_str).unwrap());
            let dv = d.get("deviation").unwrap(); let z = dv.get("z").and_then(Json::as_f64);
            let on_ = dv.get("registers") == Some(&Json::Bool(true)) && z.is_some_and(|z| z >= 2.0);
            assert_eq!(g.ends_with('👀'), on_, "{why}"); assert_eq!(why.contains(" +surprise z="), on_);
            if on_ { fired += 1; assert_eq!(g.chars().count(), 2); } else { assert_eq!(g.chars().count(), 1); } }
        assert!(fired > 0, "the rare event must register");
        assert!(verify(&text, &run).is_ok());
        let high = with_display(&off, &[("display", &GLYPHS.replace(r#""z_at_least":2.0"#, r#""z_at_least":1000"#)), ("expectation", "{}")]);
        let (_, hd) = glife(&persona::build(&high).unwrap(), &high, &evs);
        assert!(hd.iter().all(|d| d.get("glyph").and_then(Json::as_str).unwrap().chars().count() == 1));
    }

    /// §2.18: `display.glyphs` is read strictly (the lint rules): two moods of the persona with their levels each once, one row
    /// per level, one glyph per column, every glyph one code point that shows on its own (optionally followed by U+FE0F), known
    /// habits, no repeated key
    #[test]
    fn display_glyphs_are_read_strictly() {
        let off = json::parse(GDOC).unwrap();
        let err = |g: &str| persona::build(&with_display(&off, &[("display", g)])).err().map(|e| (e.path, e.msg));
        let ok = |g: &str| persona::build(&with_display(&off, &[("display", g)])).map(|p| p.glyphs.is_some()).map_err(|e| e.msg);
        assert_eq!(ok(GLYPHS), Ok(true)); assert_eq!(ok("{}"), Ok(false)); assert_eq!(ok(r#"{"glyphs":null}"#), Ok(false));
        let swap = |a: &str, b: &str| GLYPHS.replacen(a, b, 1);
        assert_eq!(err(&swap(r#""pressure":["calm","steady","keyed"]"#, r#""humour":["none","light","playful"]"#)).map(|e| e.0), Some("display.glyphs.moods.humour".into()));
        assert_eq!(err(&swap(r#""pressure":["calm","steady","keyed"]"#, r#""pressure":["calm","steady"]"#)).map(|e| e.0), Some("display.glyphs.moods.pressure".into()));
        assert_eq!(err(&swap(r#""pressure":["calm","steady","keyed"]"#, r#""pressure":["calm","calm","keyed"]"#)).map(|e| e.0), Some("display.glyphs.moods.pressure".into()));
        assert_eq!(err(&swap(r#","pressure":["calm","steady","keyed"]"#, "")).map(|e| e.0), Some("display.glyphs.moods".into()));
        assert_eq!(err(&swap(r#""up":["😊","😄","🤩"]"#, r#""up":["😊","😄"]"#)).map(|e| e.0), Some("display.glyphs.table.up".into()));
        assert_eq!(err(&swap(r#","up":["😊","😄","🤩"]"#, "")).map(|e| e.0), Some("display.glyphs.table.up".into()));
        assert_eq!(err(&swap(r#""up":"#, r#""upp":"#)).map(|e| e.0), Some("display.glyphs.table.upp".into()));
        // one code point followed by the emoji presentation selector U+FE0F is one glyph (🛠️, ❤️, ☺️)
        for good in ["🛠\u{fe0f}", "❤\u{fe0f}", "☺\u{fe0f}", "☺"] { assert_eq!(ok(&swap("😬", good)), Ok(true), "{good:?}"); }
        assert_eq!(ok(&swap(r#""glyph":"👀""#, "\"glyph\":\"👁\u{fe0f}\"")), Ok(true));
        // ZWJ sequences, the text selector, a lone or doubled selector, skin tones, flags, keycaps (1️⃣ and the bare mark), two
        // glyphs, a code point after the selector, empty, a space: refused at the cell
        for bad in ["👨\u{200d}💻", "👩\u{200d}💻", "☺\u{fe0e}", "\u{fe0f}", "🙂\u{fe0f}\u{fe0f}", "🙂\u{fe0f}🙂", "👍\u{1f3fd}", "\u{1f1fa}", "1\u{fe0f}\u{20e3}", "1\u{20e3}", "🙂🙂", "", " ", "\u{1f3fd}"] {
            let g = swap("😬", bad); assert_eq!(err(&g), Some(("display.glyphs.table.even[2]".into(), "one Unicode code point, optionally followed by U+FE0F (no ZWJ, skin-tone, flag, keycap, tag or other combining sequence)".into())), "{bad:?}"); }
        assert_eq!(err(&swap(r#""glyph":"👀""#, r#""glyph":"👀👀""#)).map(|e| e.0), Some("display.glyphs.surprise.glyph".into()));
        assert_eq!(err(&swap(r#""z_at_least":2.0"#, r#""z_at_least":-1"#)).map(|e| e.0), Some("display.glyphs.surprise.z_at_least".into()));
        assert_eq!(err(&swap(r#""no_jokes_on_loss":"🛟""#, r#""no_such":"🛟""#)).map(|e| e.0), Some("display.glyphs.habits.no_such".into()));
        assert_eq!(err(&swap(r#""no_jokes_on_loss":"🛟""#, r#""no_jokes_on_loss":"🛟","no_jokes_on_loss":"🔒""#)).map(|e| e.0), Some("display.glyphs.habits.no_jokes_on_loss".into()));
        assert_eq!(err(&swap(r#""surprise":"#, r#""shock":"#)).map(|e| e.0), Some("display.glyphs.shock".into()));
        assert_eq!(err(r#"{"glyph":{}}"#).map(|e| e.0), Some("display.glyph".into()));
        assert_eq!(err(r#"{"glyphs":{"table":{}}}"#).map(|e| e.0), Some("display.glyphs.moods".into()));
    }

    /// RFC 0009: the trajectory at beta = 1 is the exact tier's answer (to 1e-9), and the joint entropy of p_beta is non-increasing
    /// in beta (dH/dbeta = -beta Var(log w) <= 0), checked by enumerating the joint of a small program
    #[test]
    fn exact_trajectory_matches_and_sharpens() {
        let p = persona::build(&json::parse(DOC).unwrap()).unwrap(); let st = persona::init(&p, Some(2), true, &run);
        for t in 0..6 { let prog = persona::program_json(&p, &st, &ev(t)).unwrap();
            let tj = persona::trajectory_program(&prog, 8, 0.4, 2_000_000).unwrap(); let steps = tj.get("steps").and_then(Json::as_arr).unwrap();
            let d = run(&prog, &persona::Flags { op: "decide".into(), seed: 1, sweeps: 10, polish_sweeps: 0, exact_limit: 2_000_000, chains: 4 });
            for (id, row) in steps[7].get("marginals").and_then(Json::as_obj).unwrap() { for (l, x) in row.as_obj().unwrap() {
                let y = d.get("marginals").and_then(|m| m.get(id)).and_then(|r| r.get(l)).and_then(Json::as_f64).unwrap(); assert!((x.as_f64().unwrap() - y).abs() <= 1e-9); } }
            let m = crate::run::from_json(&prog).unwrap().m; let mut last = f64::INFINITY;
            for s in steps { let b = s.get("beta").and_then(Json::as_f64).unwrap();
                let e = probbit_ir::exact(&m.scaled(b), 1 << 20, 2_000_000).unwrap(); assert_eq!(e.top.len() as u64, e.n_feasible);
                let h = e.top.iter().fold(0.0, |a, (lw, _)| { let q = (lw - e.logz).exp(); if q > 0.0 { a - q * (lw - e.logz) } else { a } });
                assert!(h <= last + 1e-9, "joint entropy rose at beta {b}"); last = h; } }
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

    /// `j` with `v` at `path` (objects only; the last key replaced, or appended when missing)
    fn put(j: &mut Json, path: &[&str], v: Json) {
        let Json::Obj(kv) = j else { panic!("not an object at {path:?}") };
        match path {
            [k] => match kv.iter_mut().find(|(x, _)| x == k) { Some((_, x)) => *x = v, None => kv.push((k.to_string(), v)) },
            [k, rest @ ..] => put(&mut kv.iter_mut().find(|(x, _)| x == k).unwrap_or_else(|| panic!("no {k}")).1, rest, v),
            [] => unreachable!() }
    }
    /// The lines re-chained from index `from` on (each `prev` the sha256 of the line before), as the strand's text
    fn rechain(mut ls: Vec<String>, from: usize) -> String {
        for i in from.max(1)..ls.len() { let mut j = json::parse(&ls[i]).unwrap(); put(&mut j, &["prev"], Json::Str(persona::digest_of(&ls[i - 1]))); ls[i] = persona::canon(&j); }
        ls.join("\n") + "\n"
    }
    /// Cross-platform parity of the deviation (0.14.0): a strand of 0.13.0 with an expectation block and checkpoint lines (the
    /// stance's `deviation` at full precision, as that engine wrote it) verifies as recorded, and verifies with a deviation's last
    /// bits changed as another platform's libm gives them (the chain re-made after it); a change at the 6th decimal still
    /// diverges. From 0.14.0 every number of the deviation is at 6 decimals, so the line is the same on every libm. The fixture
    /// was recorded by the 0.13.0 release build (Linux x86_64): `probbit live` on this module's DOC with `"expectation": {}`,
    /// `--seed 2 --clock fixed --checkpoint-every 4`, 16 events (3 time-only, then `ev(3..16)`).
    #[test]
    fn the_deviation_is_the_same_on_every_libm() {
        assert_eq!(engine_version("probbit 0.13.0"), Some((0, 13, 0))); assert_eq!(engine_version("probbit 0.14.0-dev"), Some((0, 14, 0)));
        assert_eq!(engine_version("probbit test"), None); assert_eq!(engine_version("probbit 0.14"), None); assert_eq!(engine_version("other 0.13.0"), None);
        const F: &str = include_str!("../tests/fixtures/strands/expectation-0.13.0.strand");
        let ok = verify(F, &run).unwrap(); assert_eq!((ok.get("engine"), ok.get("checkpoints")), (Some(&Json::Str("probbit 0.13.0".into())), Some(&Json::Num(4.0))));
        let ls: Vec<String> = F.lines().map(str::to_string).collect();
        let dev = |l: &str, k: &str| json::parse(l).unwrap().get("stance").and_then(|s| s.get("deviation")).and_then(|d| d.get(k)).and_then(Json::as_f64);
        let at = ls.iter().position(|l| dev(l, "pred").is_some_and(|x| persona::r6(x) != x)).expect("a checkpoint with a full-precision deviation");
        let pred = dev(&ls[at], "pred").unwrap();
        let with = |x: f64| { let mut j = json::parse(&ls[at]).unwrap(); put(&mut j, &["stance", "deviation", "pred"], Json::Num(x)); let mut tt = ls.clone(); tt[at] = persona::canon(&j); rechain(tt, at) };
        // 2 ULP (the `libm` crate of a nostd build against the platform's libm): verifies
        let other = with(f64::from_bits(pred.to_bits() + 2)); assert_ne!(other, F);
        assert_eq!(verify(&other, &run).map(|d| d.get("events").cloned()), Ok(ok.get("events").cloned()));
        // a change the 6 decimals show: diverges at that line
        assert_eq!(verify(&with(pred + 1e-5), &run).unwrap_err(), (at + 1, "the checkpoint's state or stance differs from the replay".to_string()));
        // from 0.14.0: the same life logs every number of the deviation at 6 decimals and verifies; 0.13.0 logged more
        let h = json::parse(&ls[0]).unwrap(); let doc = h.get("document").unwrap().clone(); let p = persona::build(&doc).unwrap();
        let evs: Vec<Json> = ls[1..].iter().filter_map(|l| json::parse(l).ok()).filter(|j| matches!(kind(j), Kind::Event)).map(|j| j.get("inputs").unwrap().clone()).collect();
        let life = |engine: &str| { let st = State::read(&p, h.get("state").unwrap()).unwrap(); let (mut lv, head) = Live::start(p.clone(), &doc, st, Clock::Fixed, engine); let mut text = format!("{head}\n");
            for e in &evs { let (s, l) = lv.event(e, &run).unwrap(); text += &l; text.push('\n'); if lv.n % 4 == 0 { text += &lv.checkpoint(&s); text.push('\n'); } } text };
        let (old, new) = (life("probbit 0.13.0"), life("probbit 0.14.0"));
        assert_eq!(old, F.replace("\r\n", "\n"), "0.13.0's bytes, as recorded (a Windows checkout has CRLF)");
        let nums = |t: &str| t.lines().filter_map(|l| json::parse(l).ok()?.get("stance")?.get("deviation").cloned()).flat_map(|d| ["self", "pred", "input", "z"].map(|k| d.get(k).and_then(Json::as_f64))).flatten().collect::<Vec<f64>>();
        let n = nums(&new); assert_eq!(n.len(), 16); assert!(n.iter().all(|x| persona::r6(*x) == *x), "{n:?}"); assert!(nums(&old).iter().any(|x| persona::r6(*x) != *x));
        assert_eq!(verify(&new, &run).map(|d| d.get("checkpoints").cloned()), Ok(Some(Json::Num(4.0))));
        // a 0.14.0 checkpoint is checked byte for byte: a rounded number's last bits changed diverge
        let ln: Vec<String> = new.lines().map(str::to_string).collect(); let c = ln.iter().position(|l| dev(l, "pred").is_some_and(|x| x > 0.0)).unwrap();
        let mut j = json::parse(&ln[c]).unwrap(); put(&mut j, &["stance", "deviation", "pred"], Json::Num(f64::from_bits(dev(&ln[c], "pred").unwrap().to_bits() + 2))); let mut tt = ln.clone(); tt[c] = persona::canon(&j);
        assert_eq!(verify(&rechain(tt, c), &run).unwrap_err().0, c + 1);
    }

    /// docs/persona.md §5.7, "Unsupported": a header key, a persona field, a state field, a line kind or a line field this engine
    /// does not know (a newer engine's strand, its chain whole) is `unsupported by this engine: FIELD` at the line that has it,
    /// never a divergence; `verify_doc` (the MCP tool's document) is {ok, line, unsupported, engine, strand_engine}. A tampered
    /// strand still diverges at its line: a changed input, a removed line, a changed known header value, and an unknown field
    /// added without the chain that a writer would give it (the chain is checked past an unsupported line).
    #[test]
    fn a_field_this_engine_does_not_know_is_unsupported_not_a_divergence() {
        let (text, _) = strand(6); let ls: Vec<String> = text.lines().map(str::to_string).collect(); assert_eq!(ls.len(), 7);
        let unsupported_at = |t: &str, line: usize, field: &str| {
            assert_eq!(verify(t, &run).unwrap_err(), (line, format!("unsupported by this engine: {field}")), "{field}");
            let want = format!(r#"{{"engine":"{}","line":{line},"ok":false,"strand_engine":"probbit test","unsupported":"{field}"}}"#, engine());
            assert_eq!(persona::canon(&verify_doc(t, &run)), want, "{field}: the document");
        };
        let diverges_at = |t: &str, line: usize| { let (l, why) = verify(t, &run).unwrap_err(); assert_eq!(l, line, "{why}"); assert_eq!(unsupported_field(&why), None, "{why}");
            assert!(verify_doc(t, &run).get("diverges").is_some()); };
        let header = |f: &dyn Fn(&mut Json)| { let mut h = json::parse(&ls[0]).unwrap(); f(&mut h); persona::text(&h) };
        let load = json::parse(r#"{"half_life_hours":72,"kappa":0.5,"theta":1,"cap":4,"from":{"criticism":1,"sense_tool_fail":1},"effects":{"caution":0.5}}"#).unwrap();
        // the header: a persona block a newer engine might add (`sleep`) and a field in a known block, each with the persona digest of its document
        for (path, field) in [(vec!["document", "sleep"], "sleep"), (vec!["document", "learning", "pace"], "learning.pace")] {
            let h = header(&|h| { put(h, &path, load.clone()); let d = persona::sha(h.get("document").unwrap()); put(h, &["persona", "digest"], Json::Str(d)); });
            let mut t: Vec<String> = ls.clone(); t[0] = h; let t = rechain(t, 1);
            unsupported_at(&t, 1, field);
            // the same header written over the old one, the chain left as it was: tampered, line 2's prev
            let mut bent = ls.clone(); bent[0] = t.lines().next().unwrap().to_string(); diverges_at(&(bent.join("\n") + "\n"), 2);
        }
        // a header key, and a state field (the state's digest recomputed, as a writer's would be)
        let h = header(&|h| put(h, &["clock"], Json::Str("fixed".into()))); let mut t = ls.clone(); t[0] = h; unsupported_at(&rechain(t, 1), 1, "clock");
        let h = header(&|h| { put(h, &["state", "sleep"], Json::Num(0.0)); let mut s = h.get("state").unwrap().clone(); if let Json::Obj(v) = &mut s { v.retain(|(k, _)| k != "digest"); }
            put(h, &["state", "digest"], Json::Str(persona::sha(&s))); });
        let mut t = ls.clone(); t[0] = h; unsupported_at(&rechain(t, 1), 1, "state.sleep");
        // a line kind (a `nap` line) at the end, and in the middle with the lines after it re-chained
        let rest = |prev: &str| persona::canon(&json::parse(&format!(r#"{{"prev":"{prev}","nap":{{"rho":0.5}}}}"#)).unwrap());
        let mut t = ls.clone(); t.push(rest(&persona::digest_of(&ls[6]))); unsupported_at(&rechain(t, 7), 8, "nap");
        let mut mid = ls.clone(); mid.insert(3, rest("")); let mid = rechain(mid, 3); unsupported_at(&mid, 4, "nap");
        // a field on an event line
        let mut t = ls.clone(); let mut j = json::parse(&t[4]).unwrap(); put(&mut j, &["why"], Json::Str("a newer field".into())); t[4] = persona::canon(&j); unsupported_at(&rechain(t, 5), 5, "why");
        // tampered: a changed input diverges at its line; a removed line at the next line's prev; a changed known header value at line 1
        diverges_at(&text.replacen(r#""praise":false"#, r#""praise":true"#, 1), 2);
        let mut t = ls.clone(); t.remove(3); diverges_at(&(t.join("\n") + "\n"), 4);
        diverges_at(&text.replacen(r#""rate":0.5"#, r#""rate":0.6"#, 1), 1);
        // tampered around an unsupported line: a rest line with a wrong prev; a line removed after it; the rest line itself edited
        let mut t = ls.clone(); t.push(rest(&persona::digest_of(&ls[5]))); diverges_at(&(t.join("\n") + "\n"), 8);
        let mut t: Vec<String> = mid.lines().map(str::to_string).collect(); t.remove(5); diverges_at(&(t.join("\n") + "\n"), 6);
        diverges_at(&mid.replacen(r#""rho":0.5"#, r#""rho":0.9"#, 1), 5);
        // `live --strand` (resume) and `live control` refuse to continue it, saying why (exit 2)
        let mut t = ls.clone(); t.push(rest(&persona::digest_of(&ls[6]))); let e = check_chain(&rechain(t, 7)).unwrap_err();
        assert_eq!(e.msg, "line 8: unsupported by this engine: nap");
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

    // ------------------------------------------------------------------ senses (docs/persona.md §2.11)
    /// A sensed individual: a flag sense from env, a number sense from env or a person, learning and reward_from
    const SENSED: &str = r#"{"probbit_persona":1,"identity":{"name":"Sensor","version":"0.1.0","seed":0},
        "traits":[{"id":"caution","levels":["bold","measured","careful"],"prior":[0.4,0.4,0.2],"spread":0.4},{"id":"verbosity","levels":["terse","short","full"],"prior":[0.2,0.4,0.4],"spread":0.4}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.5,"half_life_hours":6}],
        "inputs":[{"id":"praise","kind":"flag","effects":{"valence":0.5}},{"id":"criticism","kind":"flag","effects":{"valence":-0.5}}],
        "senses":[{"id":"tool_fail","kind":"flag","unit":"event","prior":{"mean":0.05,"var":0.05},"drift_cap":0.25,"effects":{"caution":0.8,"valence":-0.4},"gain_spread":0.3,"threshold_spread":0.3,"habituation_spread":0.3},
            {"id":"tempo","kind":"number","unit":"messages","range":[0,200],"src":["env","human"],"prior":{"mean":1,"var":1},"drift_cap":2,"effects":{"verbosity":-0.4},"gain_spread":0.3}],
        "habits":[{"id":"no_bold_after_failure","when":{"sense.tool_fail.raw":true},"then":{"caution":{"not":["bold"]}}}],
        "learning":{"from":["praise","criticism"],"traits":["caution"],"rate":0.3,"step_cap":0.1,"total_cap":0.4},"reward_from":["human","env"]}"#;
    fn sensed() -> (Persona, Json) { let doc = json::parse(SENSED).unwrap(); (persona::build(&doc).unwrap(), doc) }
    /// A random event and whether the rules accept it: sources (env, a person, self, the clock, none, malformed), readings
    /// (declared, from the event's source or their own, out of range, of an undeclared sense, of the wrong type), rewards, hours
    fn sensed_rnd(r: &mut Philox4x32) -> (Json, bool) {
        let srcs: [(Option<&str>, Option<&str>); 6] = [(Some("env:runner"), Some("env")), (Some("human:a"), Some("human")), (Some("self"), Some("self")), (Some("clock"), Some("clock")), (None, None), (Some("env:"), None)];
        let (src, kind) = srcs[r.below(6) as usize];
        let mut ok = !(src.is_some() && kind.is_none());
        let mut e = vec![("elapsed_hours".to_string(), Json::Num([0.0, 0.25, 1.0, 30.0][r.below(4) as usize]))];
        if r.below(4) == 0 { e.push(("praise".into(), Json::Bool(true))); ok &= matches!(kind, Some("env" | "human")); }
        let env_only = |k: Option<&str>| k == Some("env"); let env_human = |k: Option<&str>| matches!(k, Some("env" | "human"));
        let rd = match r.below(8) {
            0 | 1 => None,
            2 => { ok &= env_only(kind); Some(format!(r#"{{"tool_fail":{}}}"#, r.below(2) == 0)) }
            3 => { ok &= env_human(kind); Some(format!(r#"{{"tempo":{}}}"#, r.below(41))) }
            4 => { ok &= env_only(kind) && env_human(kind); Some(format!(r#"{{"tool_fail":true,"tempo":{}}}"#, 5 * r.below(9))) }
            5 => Some(format!(r#"{{"tool_fail":{{"v":{},"src":"env:own"}}}}"#, r.below(2) == 0)),
            6 => { ok = false; Some(["{\"tempo\":250}", "{\"latency\":3}", "{\"tool_fail\":1}", "{\"tempo\":{\"v\":3,\"src\":\"self\"}}"][r.below(4) as usize].to_string()) }
            _ => Some(format!(r#"{{"tempo":{{"v":{},"src":"human:b"}},"tool_fail":{{"v":true,"src":"env"}}}}"#, r.below(30))) }; // each reading its own source
        if let Some(x) = rd { e.push(("senses".into(), json::parse(&x).unwrap())); }
        if let Some(s) = src { e.push(("src".into(), Json::Str(s.into()))); }
        (Json::Obj(e), ok)
    }

    /// P3s, refusal invariance (RFC 0001): 40 individuals x 300 random events mixing sources, readings, `self`, the clock and
    /// out-of-range or undeclared readings: every run equals, strand and final state byte for byte, the same run with its
    /// refused events removed; the rules the generator labels by construction are the rules the engine applies
    #[test]
    fn p3s_a_run_equals_the_run_without_its_refused_readings() {
        let (p, doc) = sensed(); let mut r = Philox4x32::new(97, 5); let (mut refused_all, mut events_all) = (0, 0);
        for seed in 0..40u64 {
            let evs: Vec<(Json, bool)> = (0..300).map(|_| sensed_rnd(&mut r)).collect();
            let go = |seq: &[&Json]| { let (mut lv, h) = Live::start(p.clone(), &doc, persona::init(&p, Some(seed), true, &run), Clock::Fixed, "t"); let mut text = format!("{h}\n"); let mut refused = vec![];
                for (i, e) in seq.iter().enumerate() { match lv.event(e, &run) { Ok((_, l)) => { text += &l; text.push('\n'); } Err(_) => refused.push(i) } } (text, persona::canon(&lv.st.to_json(&lv.p)), refused) };
            let all: Vec<&Json> = evs.iter().map(|x| &x.0).collect(); let kept: Vec<&Json> = evs.iter().filter(|x| x.1).map(|x| &x.0).collect();
            let (a, b) = (go(&all), go(&kept));
            let want: Vec<usize> = evs.iter().enumerate().filter(|(_, x)| !x.1).map(|(i, _)| i).collect();
            assert_eq!(a.2, want, "individual {seed}: the refused events are the ones the rules refuse");
            assert_eq!((&a.0, &a.1), (&b.0, &b.1), "individual {seed}"); assert!(b.2.is_empty());
            refused_all += a.2.len(); events_all += evs.len(); }
        assert!(refused_all > 2000 && refused_all < 10000, "{refused_all} of {events_all} refused");
        eprintln!("p3s: 40 individuals x 300 events: {refused_all} of {events_all} refused, every run equal to its run without them");
    }

    /// `verify` recomputes the normals: its summary gains `normals` (mean, count, the largest move of a mean within 50 events)
    /// with a senses block and not without; a life split over two runs (state + strand continuation) is the strand of one run
    #[test]
    fn verify_recomputes_the_normals_and_a_split_life_is_one_life() {
        let (p, doc) = sensed(); let mut r = Philox4x32::new(11, 2);
        let evs: Vec<Json> = (0..120).map(|_| sensed_rnd(&mut r)).filter(|x| x.1).map(|x| x.0).collect();
        let one = |seq: &[Json], st: State| { let (mut lv, h) = Live::start(p.clone(), &doc, st, Clock::Fixed, "t"); let mut text = format!("{h}\n");
            for e in seq { let (_, l) = lv.event(e, &run).unwrap(); text += &l; text.push('\n'); } (text, lv) };
        let (text, lv) = one(&evs, persona::init(&p, Some(3), true, &run));
        let v = verify(&text, &run).unwrap(); let nm = v.get("normals").unwrap();
        let fail = nm.get("tool_fail").unwrap(); assert!(fail.get("max_move_50").and_then(Json::as_f64).unwrap() > 0.0 && fail.get("n").and_then(Json::as_f64).unwrap() > 10.0, "{}", persona::canon(nm));
        assert_eq!(fail.get("mean"), lv.st.to_json(&p).get("senses").and_then(|s| s.get("norm")).and_then(|n| n.get("tool_fail")).and_then(|t| t.get("mean")));
        assert!(verify(&strand(5).0, &run).unwrap().get("normals").is_none(), "no block, the summary of before");
        // split at event 37: the first run's state, then the rest on the same strand
        let k = 37; let (head, lv1) = one(&evs[..k], persona::init(&p, Some(3), true, &run));
        let mut lv2 = resume(&head, &p, lv1.st.clone(), Clock::Fixed).unwrap(); let mut text2 = head.clone();
        for e in &evs[k..] { let (_, l) = lv2.event(e, &run).unwrap(); text2 += &l; text2.push('\n'); }
        assert_eq!(text2, text, "a life continued over two runs is the strand of one run");
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
    /// RFC 0004 amendment A2: `erase` is a chained control line; replay zeroes the learned weights (the next event's state
    /// digest records it), the status is unchanged, an edited erase line diverges at the next line, and the audit reads 0
    fn erase_control_line() {
        let (p, doc) = kit(None); let mut r = Philox4x32::new(7, 7);
        let (mut lv, h) = Live::start(p.clone(), &doc, persona::init(&p, Some(2), true, &run), Clock::Fixed, "t"); let mut text = format!("{h}\n");
        let praise = json::parse(r#"{"praise":true}"#).unwrap();
        for _ in 0..6 { text += &lv.event(&rnd(&mut r, &[None]), &run).unwrap().1; text.push('\n'); text += &lv.event(&praise, &run).unwrap().1; text.push('\n'); }
        assert!(persona::bound::learned_of(&lv.st).iter().flatten().any(|x| *x != 0.0), "praise moved the learned weights");
        assert_eq!(lv.control("erase", "self", "x", "t0").err().map(|e| e.path).as_deref(), Some("control.by"), "the individual cannot erase itself");
        text += &lv.control("erase", "human:owner", "forget the campaign", "2026-10-20T08:00:00Z").unwrap(); text.push('\n');
        assert_eq!(lv.status, Status::Active); assert!(persona::bound::learned_of(&lv.st).iter().flatten().all(|x| *x == 0.0));
        let n = lines(&text).len(); text += &lv.event(&rnd(&mut r, &[None]), &run).unwrap().1; text.push('\n');
        let v = verify(&text, &run).unwrap(); assert_eq!(v.get("final_state").and_then(Json::as_str), Some(lv.st.digest.as_str()));
        assert_eq!(verify(&text.replacen("forget the campaign", "forget the campaigns", 1), &run).unwrap_err().0, n + 1, "an edited erase line diverges at the next line");
        assert!(verify(&text.replacen(r#""control":"erase""#, r#""control":"pause""#, 1), &run).is_err(), "erase turned into pause: the replay refuses");
        let b = persona::bound::Bound { rest_hours: Some(24.0), max_epsilon: None, seeds: None, count: vec![] };
        let (a, ok) = bound_audit(&text, &b, &run).unwrap(); assert!(ok);
        let rows = a.get("traits").and_then(Json::as_arr).unwrap(); assert!(rows.iter().all(|t| t.get("room_used_now").and_then(Json::as_f64) == Some(0.0)));
        assert!(rows.iter().any(|t| t.get("room_used_max").and_then(Json::as_f64).unwrap() > 0.0), "the life used some room before the erase");
    }
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

    /// RFC 0002 part 2: a rest line is its own strand line before its event; `verify` recomputes it, and refuses the strand
    /// with it removed (the chain, or rechained: missing), edited, or with one where none is due
    #[test]
    fn verify_recomputes_rest_lines() {
        let doc = json::parse(r#"{"probbit_persona":1,"identity":{"name":"R","version":"1","seed":0},"traits":[{"id":"approach","levels":["low","mid","high"]}],
            "inputs":[{"id":"praise","kind":"flag"},{"id":"criticism","kind":"flag"}],"learning":{"from":["praise","criticism"],"traits":["approach"],"rate":0.5,"step_cap":0.2,"total_cap":1},
            "rest":{"rise_per_hour":0.3,"fall_per_hour":0.238,"gap_hours":4,"threshold":0.5,"downscale":0.5,"prune":0.05}}"#).unwrap();
        let p = persona::build(&doc).unwrap(); let st = persona::init(&p, Some(1), true, &run);
        let (mut live, header) = Live::start(p, &doc, st, Clock::Fixed, "probbit test");
        let mut text = format!("{header}\n");
        for t in 0..12 { let ev = json::parse(&if t == 11 { r#"{"elapsed_hours":8}"#.to_string() } else { format!(r#"{{"elapsed_hours":1,"praise":{}}}"#, t > 0) }).unwrap();
            text.push_str(&live.event(&ev, &run).unwrap().1); text.push('\n'); }
        let ls: Vec<String> = text.lines().map(str::to_string).collect();
        let at = ls.iter().position(|l| l.starts_with(r#"{"prev":"#) && l.contains(r#","rest":{"#)).expect("a rest line"); assert_eq!(at, ls.len() - 2);
        let ok = verify(&text, &run).unwrap(); assert_eq!(ok.get("rest_lines").and_then(Json::as_f64), Some(1.0), "{}", persona::canon(&ok));
        let mut t = ls.clone(); t.remove(at); assert_eq!(verify(&(t.join("\n") + "\n"), &run).unwrap_err().0, at + 1);
        let mut t = ls.clone(); t.remove(at); let r = rechain(t, at); assert_eq!(verify(&r, &run).unwrap_err(), (at + 1, "a rest line is due before this event and missing".into()));
        let mut t = ls.clone(); t[at] = t[at].replacen(r#""downscale":0.5"#, r#""downscale":0.6"#, 1); let r = rechain(t, at + 1);
        assert_eq!(verify(&r, &run).unwrap_err(), (at + 1, "the rest line differs".into()));
        let t = ls[..=at].join("\n") + "\n"; assert_eq!(verify(&t, &run).unwrap_err(), (at + 1, "a rest line must be followed by its event line".into()));
        let mut t = ls.clone(); let extra = t[at].clone(); t.insert(3, extra); let r = rechain(t, 3); assert_eq!(verify(&r, &run).unwrap_err(), (4, "a rest line where none is due".into()));
    }

}
