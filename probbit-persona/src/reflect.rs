//! RFC 0013, self-report (docs/persona.md §5.8 "Self-report"): a canonical account of a window of a strand's events, computed
//! from the strand's lines alone: no turn is run, nothing is solved, nothing is drawn at random. Per event line the reflection
//! reads what the line logs (the inputs, the expectation's receipt, the glyph); the z of each receipt is recomputed from the
//! expectation's memory, a fold of the logged receipts (`persona::expect_step`'s mean and variance, nothing else); per
//! checkpoint line in the window it reads the stance and the state the line carries (the levels taken and their odds, the
//! moods, the drives, the habits that bound, the learned weights); control, rest and reflection lines are counted. A section
//! the window has no line for is `null` (absent, not invented): a window without a checkpoint line reports no levels.
//!
//! The document (`Reflection::doc`) is canonical JSON with every number at 6 decimals and `digest` = the sha256 of its
//! canonical text without `digest`; `summary` is 6 to 12 sentences made by fixed templates from its numbers, part of the
//! document. Cost: one pass over the lines from the fold's start (the last checkpoint carrying the expectation's memory before
//! the window, or the header), each event line parsed once, each checkpoint line in the window parsed once.
use crate::json::{self, InErr, Json};
use crate::persona::{self, r6};
#[allow(unused_imports)]
use std::prelude::rust_2021::*;

/// The document's format
pub const FORMAT: f64 = 1.0;
/// The surprises reported: the K events with the largest |z|
pub const TOP_K: usize = 5;
/// The turning points listed per mood (all are counted)
pub const TURNS_MAX: usize = 8;
/// The samples of a mood's path the document carries (the last ones)
pub const PATH_MAX: usize = 50;
/// The engine from which a strand may carry reflection lines
pub const REFLECT_SINCE: (u64, u64, u64) = (0, 14, 0);
/// z at or above this registers (the expectation's own threshold)
const DEV_Z: f64 = persona::DEV_Z;
/// The expectation's prior variance and variance floor (persona.rs)
const PRIOR_VAR: f64 = 0.04;
const VAR_FLOOR: f64 = 1e-4;

/// Which events a reflection covers
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Window { All, Last(u64), SinceCheckpoint }
impl Window {
    /// The window a reflection document names (its `window.last_n` and `window.since_checkpoint`)
    pub fn of(doc: &Json) -> Option<Window> {
        let w = doc.get("window")?;
        if w.get("since_checkpoint") == Some(&Json::Bool(true)) { return Some(Window::SinceCheckpoint); }
        match w.get("last_n") { Some(Json::Num(x)) if *x >= 1.0 && x.fract() == 0.0 => Some(Window::Last(*x as u64)), Some(Json::Null) => Some(Window::All), _ => None }
    }
}
/// A reflection: the canonical document (with `digest`) and its digest
pub struct Reflection { pub doc: Json, pub digest: String }
impl Reflection {
    /// The canonical text
    pub fn text(&self) -> String { persona::canon(&self.doc) }
    /// The summary's sentences
    pub fn summary(&self) -> Vec<String> { self.doc.get("summary").and_then(Json::as_arr).map_or(vec![], |v| v.iter().filter_map(|s| s.as_str().map(String::from)).collect()) }
}

/// What kind a canonical strand line is, from its first bytes (`reflect` reads lines `verify` checks; it does not check them)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K { Header, Event, Control, Checkpoint, Rest, Reflection }
fn kind_of(l: &str) -> K {
    if l.starts_with("{\"probbit_strand\":") { K::Header } else if l.starts_with("{\"checkpoint\":") { K::Checkpoint } else if l.starts_with("{\"at\":") { K::Control }
    else if l.starts_with("{\"prev\":") { let t = l.get(80..).unwrap_or(""); if t.starts_with("\",\"reflection\":") { K::Reflection } else { K::Rest } }
    else { K::Event }
}
/// True for a reflection line (canonical: `{"prev": ..., "reflection": ...}`)
pub fn is_reflection_line(l: &str) -> bool { kind_of(l) == K::Reflection }
fn bad(msg: impl Into<String>) -> InErr { json::value("reflect", msg) }
fn n6(x: f64) -> Json { Json::Num(r6(x)) }
fn cnt(x: usize) -> Json { Json::Num(x as f64) }
fn f6(x: f64) -> String { format!("{:.6}", r6(x)) }
/// The declared ids and levels of a document's list (`traits`, `moods`)
fn declared(doc: &Json, key: &str) -> Vec<(String, Vec<String>)> {
    doc.get(key).and_then(Json::as_arr).map_or(vec![], |v| v.iter().filter_map(|t| Some((t.get("id")?.as_str()?.to_string(),
        t.get("levels")?.as_arr()?.iter().filter_map(|l| l.as_str().map(String::from)).collect()))).collect())
}
/// Where odds sit along the levels, 0 (the lowest) to 1 (the highest): the monitor's sparkline position
fn position(levels: &[String], odds: &Json) -> f64 {
    let ps: Vec<f64> = levels.iter().map(|l| odds.get(l).and_then(Json::as_f64).unwrap_or(0.0)).collect();
    let (t, k) = (ps.iter().sum::<f64>(), ps.len());
    if k < 2 || t <= 0.0 { return 0.0; }
    ps.iter().enumerate().map(|(i, p)| p * i as f64).sum::<f64>() / t / (k - 1) as f64
}
/// An input counts as set when it is true or a non-zero number (the history and the sensed readings are not counted)
fn set_keys(inputs: &Json) -> Vec<String> {
    inputs.as_obj().map_or(vec![], |kv| kv.iter().filter(|(k, v)| !matches!(k.as_str(), "elapsed_hours" | "src" | "senses")
        && match v { Json::Bool(b) => *b, Json::Num(x) => *x != 0.0, _ => false }).map(|(k, _)| k.clone()).collect())
}
fn by_count(m: &[(String, usize)]) -> Vec<(String, usize)> { let mut v = m.to_vec(); v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0))); v }
fn bump(m: &mut Vec<(String, usize)>, k: &str) { match m.iter_mut().find(|x| x.0 == k) { Some(x) => x.1 += 1, None => m.push((k.to_string(), 1)) } }
fn obj_counts(m: &[(String, usize)]) -> Json { Json::Obj(m.iter().map(|(k, c)| (k.clone(), cnt(*c))).collect()) }
fn odds_obj(levels: &[String], o: &Json) -> Json { Json::Obj(levels.iter().map(|l| (l.clone(), n6(o.get(l).and_then(Json::as_f64).unwrap_or(0.0)))).collect()) }

/// Per trait (and `pursue`): the levels taken, the odds at the first and last samples, the smallest and largest p of the level taken
struct Tr { id: String, levels: Vec<String>, taken: Vec<(String, usize)>, first: Option<Json>, last: Option<Json>, pmin: f64, pmax: f64 }
/// Per mood: its position at each sample (event n, value)
struct Md { id: String, levels: Vec<String>, path: Vec<(u64, f64)> }

/// The reflection of the last `n` events of a strand (`None`: every event)
pub fn reflect(lines: &[&str], n: Option<u64>) -> Result<Reflection, InErr> { reflect_window(lines, n.map_or(Window::All, Window::Last)) }
/// The reflection of the events after the strand's last checkpoint line (the checkpoint itself is the first sample)
pub fn reflect_since_checkpoint(lines: &[&str]) -> Result<Reflection, InErr> { reflect_window(lines, Window::SinceCheckpoint) }
/// `reflect` over a strand's text
pub fn reflect_text(text: &str, w: Window) -> Result<Reflection, InErr> { reflect_window(&crate::strand::lines(text), w) }

/// The reflection of window `w` of a strand's lines (its header first)
#[inline(never)]
pub fn reflect_window(lines: &[&str], w: Window) -> Result<Reflection, InErr> {
    let head = json::parse(lines.first().copied().unwrap_or("")).map_err(|e| bad(format!("line 1: the header is not JSON: {}", e.msg)))?;
    if kind_of(lines.first().copied().unwrap_or("")) != K::Header { return Err(bad("line 1 is not a strand header")); }
    let engine = head.get("engine").and_then(Json::as_str).unwrap_or("").to_string();
    let canon = !crate::strand::engine_version(&engine).is_some_and(|v| v < crate::strand::DEV_R6_SINCE);
    let doc = head.get("document").cloned().unwrap_or(Json::Null);
    let kinds: Vec<K> = lines.iter().map(|l| kind_of(l)).collect();
    let events: Vec<usize> = (1..lines.len()).filter(|&i| kinds[i] == K::Event).collect();
    // the window: [start, lines.len()) and its first event
    let (start, first_ev) = match w {
        Window::All => { let s = events.first().copied().ok_or_else(|| bad("the strand has no event line"))?; (s, 0) }
        Window::Last(k) => { if k == 0 { return Err(bad("--last: a whole number >= 1")); } let f = events.len().saturating_sub(k as usize);
            (events.get(f).copied().ok_or_else(|| bad("the strand has no event line"))?, f) }
        Window::SinceCheckpoint => { let c = (1..lines.len()).rev().find(|&i| kinds[i] == K::Checkpoint).ok_or_else(|| bad("the strand has no checkpoint line"))?;
            let f = events.iter().position(|&i| i > c).ok_or_else(|| bad("no event line after the last checkpoint"))?; (c, f) }
    };
    let win: &[usize] = &events[first_ev..];
    // the expectation's memory before the window's first event: from the last checkpoint before it that carries the memory (a
    // canonical strand: its mean and variance are the fold's, at 6 decimals), else folded from the header
    let ex = doc.get("expectation");
    let d = ex.map(|e| 0.5f64.powf(1.0 / e.get("half_life_events").and_then(Json::as_f64).unwrap_or(32.0)));
    let (mut mean, mut var) = (0.0f64, PRIOR_VAR);
    let mut fold_from = 1;
    if ex.is_some() && canon {
        if let Some(c) = (1..win[0]).rev().find(|&i| kinds[i] == K::Checkpoint && lines[i].contains(",\"expect\":")) {
            let j = json::parse(lines[c]).map_err(|e| bad(format!("line {}: {}", c + 1, e.msg)))?;
            let m = j.get("expect").ok_or_else(|| bad(format!("line {}: the checkpoint has no expect", c + 1)))?;
            mean = m.get("mean").and_then(Json::as_f64).unwrap_or(0.0); var = m.get("var").and_then(Json::as_f64).unwrap_or(PRIOR_VAR); fold_from = c + 1;
        }
    }
    let (traits_decl, moods_decl) = (declared(&doc, "traits"), declared(&doc, "moods"));
    let goals: Vec<String> = doc.get("drives").and_then(|d| d.get("goals")).and_then(Json::as_arr).map_or(vec![], |v| v.iter().filter_map(|g| g.get("id").and_then(Json::as_str).map(String::from)).collect());
    let from: Vec<String> = doc.get("learning").and_then(|l| l.get("from")).and_then(Json::as_arr).map_or(vec![], |v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect());
    let mut trs: Vec<Tr> = traits_decl.iter().map(|(id, lv)| Tr { id: id.clone(), levels: lv.clone(), taken: vec![], first: None, last: None, pmin: f64::INFINITY, pmax: f64::NEG_INFINITY }).collect();
    if !goals.is_empty() { trs.push(Tr { id: "pursue".into(), levels: goals.clone(), taken: vec![], first: None, last: None, pmin: f64::INFINITY, pmax: f64::NEG_INFINITY }); }
    let mut mds: Vec<Md> = moods_decl.iter().map(|(id, lv)| Md { id: id.clone(), levels: lv.clone(), path: vec![] }).collect();
    let (mut inputs, mut habits, mut peaks): (Vec<(String, usize)>, Vec<(String, usize)>, Vec<(String, f64, u64)>) = (vec![], vec![], vec![]);
    let (mut scored, mut registered, mut top): (usize, usize, Vec<(u64, f64, Json)>) = (0, 0, vec![]);
    let (mut rewards, mut corrections, mut cancelled) = (0usize, 0usize, 0usize);
    let (mut samples, mut drive_samples, mut learned): (usize, usize, Vec<Json>) = (0, 0, vec![]);
    let mut ctl: Vec<(String, usize)> = ["erase", "pause", "resume", "retire"].iter().map(|k| (k.to_string(), 0)).collect();
    let (mut controls, mut rests, mut reflections) = (0usize, 0usize, 0usize);
    let (mut first_n, mut last_n) = (0u64, 0u64);
    for i in fold_from.min(start)..lines.len() {
        let in_win = i >= start;
        match kinds[i] {
            K::Event => {
                let need = in_win || ex.is_some();
                if !need { continue; }
                let j = json::parse(lines[i]).map_err(|e| bad(format!("line {}: {}", i + 1, e.msg)))?;
                let n = j.get("n").and_then(Json::as_f64).unwrap_or(0.0) as u64;
                let dv = j.get("deviation").filter(|x| !x.is_null());
                let cold = dv.is_some_and(|x| x.get("cold") == Some(&Json::Bool(true)));
                let pred = dv.and_then(|x| x.get("pred")).and_then(Json::as_f64);
                if in_win {
                    if first_n == 0 { first_n = n; } last_n = n;
                    let set = set_keys(j.get("inputs").unwrap_or(&Json::Null));
                    for k in &set { bump(&mut inputs, k); }
                    let on = |k: Option<&String>| k.is_some_and(|k| set.contains(k));
                    match (on(from.first()), on(from.get(1))) { (true, true) => cancelled += 1, (true, false) => rewards += 1, (false, true) => corrections += 1, _ => {} }
                    if let (Some(dv), Some(p), false, true) = (dv, pred, cold, ex.is_some()) {
                        let z = r6(((1.0 + p).ln() - mean) / (var + VAR_FLOOR).sqrt());
                        scored += 1; if z >= DEV_Z { registered += 1; }
                        let f = |k: &str| n6(dv.get(k).and_then(Json::as_f64).unwrap_or(0.0));
                        let mut set = set.clone(); set.sort();
                        let e = Json::Obj(vec![("glyph".into(), j.get("glyph").cloned().unwrap_or(Json::Null)), ("input".into(), f("input")), ("n".into(), Json::Num(n as f64)),
                            ("pred".into(), f("pred")), ("self".into(), f("self")), ("set".into(), Json::Arr(set.into_iter().map(Json::Str).collect())), ("z".into(), Json::Num(z))]);
                        top.push((n, z, e));
                        top.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(core::cmp::Ordering::Equal).then(a.0.cmp(&b.0))); top.truncate(TOP_K);
                    }
                }
                // the fold (persona::expect_step's mean and variance): a receipt that is not cold moves them
                if let (Some(d), Some(p), false) = (d, pred, cold) {
                    let (x, a) = ((1.0 + p).ln(), 1.0 - d); let dx = x - mean;
                    mean += a * dx; var = (1.0 - a) * (var + a * dx * dx);
                    if canon { mean = r6(mean); var = r6(var); }
                }
            }
            K::Checkpoint if in_win => {
                let j = json::parse(lines[i]).map_err(|e| bad(format!("line {}: {}", i + 1, e.msg)))?;
                let n = j.get("checkpoint").and_then(Json::as_f64).unwrap_or(0.0) as u64;
                let Some(st) = j.get("stance") else { continue };
                samples += 1;
                for t in trs.iter_mut() {
                    let x = if t.id == "pursue" { st.get("pursue") } else { st.get("stance").and_then(|s| s.get(&t.id)) };
                    let Some(x) = x else { continue };
                    let lv = if t.id == "pursue" { x.get("goal") } else { x.get("level") };
                    if let Some(l) = lv.and_then(Json::as_str) { bump(&mut t.taken, l); }
                    if let Some(o) = x.get("odds") { let o = odds_obj(&t.levels, o); if t.first.is_none() { t.first = Some(o.clone()); } t.last = Some(o); }
                    if let Some(p) = x.get("p").and_then(Json::as_f64) { t.pmin = t.pmin.min(p); t.pmax = t.pmax.max(p); }
                }
                for m in mds.iter_mut() { if let Some(o) = st.get("mood").and_then(|s| s.get(&m.id)).and_then(|x| x.get("odds")) { m.path.push((n, r6(position(&m.levels, o)))); } }
                if let Some(Json::Obj(want)) = st.get("drives").and_then(|d| d.get("want")) { drive_samples += 1;
                    for (g, v) in want { let v = v.as_f64().unwrap_or(0.0);
                        match peaks.iter_mut().find(|x| &x.0 == g) { Some(x) => if v > x.1 { x.1 = v; x.2 = n; }, None => peaks.push((g.clone(), v, n)) } } }
                if let Some(b) = st.get("habits").and_then(|h| h.get("bound")).and_then(Json::as_arr) { for h in b.iter().filter_map(Json::as_str) { bump(&mut habits, h); } }
                if let Some(l) = j.get("state").and_then(|s| s.get("learned")) { learned.push(l.clone()); }
            }
            K::Control if in_win => {
                let j = json::parse(lines[i]).map_err(|e| bad(format!("line {}: {}", i + 1, e.msg)))?;
                controls += 1; if let Some(c) = j.get("control").and_then(Json::as_str) { bump(&mut ctl, c); }
            }
            K::Rest if in_win => rests += 1,
            K::Reflection if in_win => reflections += 1,
            _ => {}
        }
    }
    // the document
    let none = |b: bool, j: Json| if b { j } else { Json::Null };
    let sampled = samples > 0;
    let traits = Json::Obj(trs.iter().filter(|t| !t.taken.is_empty() || t.first.is_some()).map(|t| (t.id.clone(), Json::Obj(vec![
        ("levels".into(), obj_counts(&t.taken)), ("odds_first".into(), t.first.clone().unwrap_or(Json::Null)), ("odds_last".into(), t.last.clone().unwrap_or(Json::Null)),
        ("p_max".into(), if t.pmax.is_finite() { n6(t.pmax) } else { Json::Null }), ("p_min".into(), if t.pmin.is_finite() { n6(t.pmin) } else { Json::Null })]))).collect());
    let mood = |m: &Md| -> Json {
        let xs: Vec<f64> = m.path.iter().map(|p| p.1).collect();
        // the turning points: the samples where the first difference changes sign (a zero difference keeps the last sign)
        let (mut turns, mut sign, mut at) = (0usize, 0i8, vec![]);
        for k in 1..xs.len() { let dx = xs[k] - xs[k - 1]; if dx == 0.0 { continue; } let s = if dx > 0.0 { 1 } else { -1 };
            if sign != 0 && s != sign { turns += 1; if at.len() < TURNS_MAX { at.push(Json::Num(m.path[k - 1].0 as f64)); } } sign = s; }
        let p0 = m.path.len().saturating_sub(PATH_MAX);
        Json::Obj(vec![("end".into(), n6(*xs.last().unwrap_or(&0.0))), ("max".into(), n6(xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max))),
            ("min".into(), n6(xs.iter().cloned().fold(f64::INFINITY, f64::min))), ("path".into(), Json::Arr(m.path[p0..].iter().map(|(n, v)| Json::Arr(vec![Json::Num(*n as f64), n6(*v)])).collect())),
            ("start".into(), n6(xs[0])), ("turning_points".into(), Json::Arr(at)), ("turns".into(), cnt(turns))])
    };
    let moods = Json::Obj(mds.iter().filter(|m| !m.path.is_empty()).map(|m| (m.id.clone(), mood(m))).collect());
    let drives = Json::Obj(peaks.iter().map(|(g, v, n)| (g.clone(), Json::Obj(vec![("at".into(), Json::Num(*n as f64)), ("peak".into(), n6(*v))]))).collect());
    let net = |a: &Json, b: &Json| -> Option<Json> { Some(Json::Obj(b.as_obj()?.iter().map(|(t, v)| (t.clone(), Json::Arr(v.as_arr().unwrap_or(&[]).iter().enumerate()
        .map(|(k, x)| n6(x.as_f64().unwrap_or(0.0) - a.get(t).and_then(Json::as_arr).and_then(|w| w.get(k)).and_then(Json::as_f64).unwrap_or(0.0))).collect()))).collect())) };
    let netj = if learned.len() >= 2 { net(&learned[0], &learned[learned.len() - 1]) } else { None };
    let moved = netj.as_ref().and_then(Json::as_obj).map_or(0.0, |kv| kv.iter().flat_map(|(_, v)| v.as_arr().unwrap_or(&[]).iter().filter_map(Json::as_f64)).map(|x| x.abs()).sum::<f64>());
    let inputs_sorted = { let mut v = inputs.clone(); v.sort_by(|a, b| a.0.cmp(&b.0)); v };
    let events_n = win.len();
    let segment = head.get("segment").and_then(|s| s.get("index")).cloned().unwrap_or(Json::Null);
    let has_learning = doc.get("learning").is_some();
    let tops: Vec<Json> = top.iter().map(|t| t.2.clone()).collect();
    let window = Json::Obj(vec![("checkpoints".into(), cnt(samples)), ("engine".into(), Json::Str(engine.clone())), ("events".into(), cnt(events_n)),
        ("first".into(), Json::Num(first_n as f64)), ("last".into(), Json::Num(last_n as f64)), ("last_n".into(), match w { Window::Last(k) => Json::Num(k as f64), _ => Json::Null }),
        ("lines".into(), cnt(lines.len() - start)), ("since_checkpoint".into(), Json::Bool(w == Window::SinceCheckpoint))]);
    let mut kv: Vec<(String, Json)> = vec![
        ("controls".into(), Json::Obj(vec![("erase".into(), cnt(ctl[0].1)), ("pause".into(), cnt(ctl[1].1)), ("reflections".into(), cnt(reflections)), ("rests".into(), cnt(rests)),
            ("resume".into(), cnt(ctl[2].1)), ("retire".into(), cnt(ctl[3].1)), ("segment".into(), segment), ("zero_credit".into(), cnt(controls))])),
        ("credit".into(), none(has_learning, Json::Obj(vec![("cancelled".into(), cnt(cancelled)), ("corrections".into(), cnt(corrections)), ("rewards".into(), cnt(rewards)), ("zeroed".into(), cnt(controls))]))),
        ("drives".into(), none(drive_samples > 0, drives)),
        ("habits".into(), none(sampled, Json::Obj(vec![("bound".into(), obj_counts(&{ let mut v = habits.clone(); v.sort_by(|a, b| a.0.cmp(&b.0)); v })), ("samples".into(), cnt(samples))]))),
        ("inputs".into(), obj_counts(&inputs_sorted)),
        ("learned".into(), none(has_learning && !learned.is_empty(), Json::Obj(vec![("moved".into(), n6(moved)), ("net".into(), netj.clone().unwrap_or(Json::Null)), ("samples".into(), cnt(learned.len()))]))),
        ("moods".into(), none(sampled && !mds.iter().all(|m| m.path.is_empty()), moods)),
        ("probbit_reflection".into(), Json::Num(FORMAT)),
        ("surprise".into(), none(ex.is_some(), Json::Obj(vec![("registered".into(), cnt(registered)), ("scored".into(), cnt(scored)), ("threshold".into(), n6(DEV_Z)), ("top".into(), Json::Arr(tops))]))),
        ("traits".into(), none(sampled, traits)),
        ("window".into(), window),
    ];
    let doc0 = Json::Obj(kv.clone());
    let summary = sentences(&doc0, &trs, &mds);
    kv.push(("summary".into(), Json::Arr(summary.into_iter().map(Json::Str).collect())));
    let digest = persona::digest_of(&persona::canon(&Json::Obj(kv.clone())));
    kv.push(("digest".into(), Json::Str(digest.clone())));
    Ok(Reflection { doc: Json::Obj(kv), digest })
}

/// The summary: 6 to 12 sentences, each from a fixed template filled with the document's numbers (mechanism language only)
fn sentences(d: &Json, trs: &[Tr], mds: &[Md]) -> Vec<String> {
    let g = |p: &[&str]| -> Option<&Json> { let mut x = d; for k in p { x = x.get(k)?; } Some(x) };
    let u = |p: &[&str]| g(p).and_then(Json::as_f64).unwrap_or(0.0) as u64;
    let mut s = vec![];
    s.push(format!("This account covers events {} to {}: {} events and {} lines read, {} of them checkpoint samples.", u(&["window", "first"]), u(&["window", "last"]),
        u(&["window", "events"]), u(&["window", "lines"]), u(&["window", "checkpoints"])));
    let ins: Vec<(String, usize)> = g(&["inputs"]).and_then(Json::as_obj).map_or(vec![], |kv| kv.iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0) as usize)).collect());
    let ins = by_count(&ins);
    s.push(if ins.is_empty() { "No input was set in this window.".to_string() } else {
        let parts: Vec<String> = ins.iter().take(3).map(|(k, c)| format!("{k} ({c} events)")).collect();
        format!("The inputs set most often were {}.", join(&parts)) });
    match g(&["surprise"]).filter(|x| !x.is_null()) {
        None => s.push("This persona has no expectation block, so no surprise was scored.".into()),
        Some(sp) => {
            s.push(format!("The expectation scored {} events and {} of them registered as surprising (z at least {}).", u(&["surprise", "scored"]), u(&["surprise", "registered"]), f6(DEV_Z)));
            if let Some(t) = sp.get("top").and_then(Json::as_arr).and_then(|v| v.first()) { let f = |k: &str| f6(t.get(k).and_then(Json::as_f64).unwrap_or(0.0));
                s.push(format!("The largest surprise was event {}: z {}, the expectation predicted {} and the event measured {}.", t.get("n").and_then(Json::as_f64).unwrap_or(0.0) as u64, f("z"), f("pred"), f("self"))); }
        }
    }
    if u(&["window", "checkpoints"]) == 0 {
        s.push("No checkpoint line falls in this window, so the levels, moods, drives and habits are not reported.".into());
    } else {
        let modal: Vec<String> = trs.iter().filter(|t| t.id != "pursue").filter_map(|t| by_count(&t.taken).first().map(|(l, c)| format!("{} {} ({})", t.id, l, c))).take(3).collect();
        if !modal.is_empty() { s.push(format!("Across the checkpoint samples the levels taken most were {}.", join(&modal))); }
        // the moods named valence and pressure first, then the others, at most two sentences
        let mut ms: Vec<&Md> = mds.iter().filter(|m| !m.path.is_empty()).collect();
        ms.sort_by_key(|m| match m.id.as_str() { "valence" => 0, "pressure" => 1, _ => 2 });
        for m in ms.into_iter().take(2) { let f = |k: &str| f6(g(&["moods", m.id.as_str(), k]).and_then(Json::as_f64).unwrap_or(0.0));
            s.push(format!("The mood {} moved from {} to {} on a scale of 0 to 1 (min {}, max {}, {} turning points).", m.id, f("start"), f("end"), f("min"), f("max"), u(&["moods", m.id.as_str(), "turns"]))); }
        let hb: Vec<(String, usize)> = g(&["habits", "bound"]).and_then(Json::as_obj).map_or(vec![], |kv| kv.iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0) as usize)).collect());
        s.push(match by_count(&hb).first() { None => "No habit bound in the checkpoint samples.".into(),
            Some((h, c)) => format!("The habit {h} bound in {c} of {} samples, and {} habits bound in all.", u(&["window", "checkpoints"]), hb.len()) });
        if let Some(Json::Obj(dv)) = g(&["drives"]) { let mut v: Vec<(&String, f64, u64)> = dv.iter().map(|(k, x)| (k, x.get("peak").and_then(Json::as_f64).unwrap_or(0.0), x.get("at").and_then(Json::as_f64).unwrap_or(0.0) as u64)).collect();
            v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal).then(a.0.cmp(b.0)));
            if let Some((k, x, n)) = v.first() { s.push(format!("The strongest want was {k} at {} (event {n}).", f6(*x))); } }
    }
    match g(&["credit"]).filter(|x| !x.is_null()) {
        None => s.push("This persona has no learning block, so no credit was settled.".into()),
        Some(_) => {
            s.push(format!("Feedback settled the credit {} times as reward and {} times as correction ({} cancelled, {} zeroed by control lines).", u(&["credit", "rewards"]),
                u(&["credit", "corrections"]), u(&["credit", "cancelled"]), u(&["credit", "zeroed"])));
            if let Some(l) = g(&["learned"]).filter(|x| !x.is_null()) { if l.get("net").is_some_and(|n| !n.is_null()) {
                s.push(format!("The learned weights moved by {} in total between the first and the last of {} samples.", f6(l.get("moved").and_then(Json::as_f64).unwrap_or(0.0)), u(&["learned", "samples"]))); } }
        }
    }
    let c = |k: &str| u(&["controls", k]);
    s.push(if c("zero_credit") == 0 { format!("No control line was applied, and {} rest lines were written.", c("rests")) }
        else { format!("Control lines applied: {} pause, {} resume, {} retire and {} erase; {} rest lines were written.", c("pause"), c("resume"), c("retire"), c("erase"), c("rests")) });
    s
}
fn join(p: &[String]) -> String {
    match p.len() { 0 => String::new(), 1 => p[0].clone(), n => format!("{} and {}", p[..n - 1].join(", "), p[n - 1]) }
}

/// The reflection line after the line whose sha256 is `prev`: canonical JSON {prev, reflection}
pub fn line(prev: &str, r: &Reflection) -> String { persona::canon(&Json::Obj(vec![("prev".into(), Json::Str(prev.into())), ("reflection".into(), r.doc.clone())])) }
/// A reflection line's check (`live verify`): the document it carries is the reflection of the lines before it over the window
/// it names, byte for byte -> Ok, or what differs
pub fn check_line(before: &[&str], l: &str) -> Result<(), String> {
    let j = json::parse(l).map_err(|e| format!("not JSON: {}", e.msg))?;
    let doc = j.get("reflection").ok_or("no reflection")?;
    let w = Window::of(doc).ok_or("the reflection names no window (window.last_n, window.since_checkpoint)")?;
    let r = reflect_window(before, w).map_err(|e| format!("the reflection cannot be recomputed: {}", e.msg))?;
    if persona::canon(doc) != r.text() { return Err(format!("the reflection differs from the lines before it (recomputed digest {})", r.digest)); }
    Ok(())
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    const FORBIDDEN: [&str; 4] = ["soul", "feels", "alive", "conscious"];
    fn fixture(name: &str) -> String { std::fs::read_to_string(format!("{}/../probbit-cli/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap() }
    #[test]
    fn deterministic_canonical_and_lawful() {
        for f in ["monitor/tutor-expect.strand", "monitor/tutor-week.strand", "strands/expectation-0.13.0.strand", "strands/sensed-credit-0.9.0-dev.strand"] {
            let t = fixture(f); let ls = crate::strand::lines(&t);
            for w in [Window::All, Window::Last(5), Window::Last(10_000)] {
                let a = reflect_window(&ls, w).unwrap(); let b = reflect_window(&ls, w).unwrap();
                assert_eq!(a.text(), b.text(), "{f}"); assert_eq!(persona::canon(&json::parse(&a.text()).unwrap()), a.text());
                let mut no = a.doc.clone(); if let Json::Obj(v) = &mut no { v.retain(|(k, _)| k != "digest"); }
                assert_eq!(persona::digest_of(&persona::canon(&no)), a.digest);
                let s = a.summary(); assert!((6..=12).contains(&s.len()), "{f}: {} sentences", s.len());
                for x in &s { let low = x.to_lowercase(); for w in FORBIDDEN { assert!(!low.contains(w), "{f}: {x}"); } }
            }
        }
    }
    /// The z the fold recomputes is the z each checkpoint's stance carries (the expectation's own, at that event)
    #[test]
    fn z_matches_the_checkpoints() {
        let t = fixture("monitor/tutor-expect.strand"); let ls = crate::strand::lines(&t);
        let r = reflect_window(&ls, Window::All).unwrap();
        let mut zs: Vec<(u64, f64)> = vec![];
        for l in &ls { if l.starts_with("{\"checkpoint\":") { let j = json::parse(l).unwrap();
            if let Some(z) = j.get("stance").and_then(|s| s.get("deviation")).and_then(|d| d.get("z")).and_then(Json::as_f64) { zs.push((j.get("checkpoint").unwrap().as_f64().unwrap() as u64, z)); } } }
        assert!(!zs.is_empty());
        for (n, z) in zs { let r1 = reflect_window(&ls[..ls.iter().position(|l| l.starts_with(&format!("{{\"checkpoint\":{n},"))).unwrap()], Window::Last(1)).unwrap();
            let got = r1.doc.get("surprise").and_then(|s| s.get("top")).and_then(Json::as_arr).and_then(|v| v.first()).and_then(|e| e.get("z")).and_then(Json::as_f64);
            assert_eq!(got, Some(z), "event {n}"); }
        assert!(r.doc.get("surprise").is_some());
    }
    #[test]
    fn line_checks_and_refuses_a_forgery() {
        let t = fixture("monitor/tutor-expect.strand"); let ls = crate::strand::lines(&t);
        let r = reflect(&ls, Some(10)).unwrap(); let l = line(&persona::digest_of(ls[ls.len() - 1]), &r);
        assert!(is_reflection_line(&l)); assert!(check_line(&ls, &l).is_ok());
        let forged = l.replacen("This account covers", "This account spans", 1);
        assert!(check_line(&ls, &forged).unwrap_err().contains("differs"));
        let moved = l.replacen("\"last_n\":10", "\"last_n\":9", 1);
        assert!(check_line(&ls, &moved).is_err());
    }
}
