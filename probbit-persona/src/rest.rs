//! RFC 0015 the rest pass (memory consolidation): at rest, when a host decides (standby, night, idle), the expectation's memory
//! (RFC 0009) is consolidated from what the events since the last pass left in it, and one `consolidation` line logs what moved.
//! Mechanism only, and exact: everything the pass reads is a fold of the logged lines inside the expectation's canonical memory
//! (engine 0.14.0 on), so a checkpoint carries it and `live verify` re-runs the pass from the lines before the line.
//!
//! The window (per remembered context, since the last pass): `a` its visits decayed by the expectation's half-life (the part of
//! its count the window added), `b` the same visits decayed by the rest pass's `half_life`, `k` the visits, `zs` the sum of the
//! visits' one-sided surprise z+ = r6(max(z, 0)) (z as the deviation computed it: the logged prediction against the learned
//! normal before the event; a cold event adds 0). The pass, in order:
//! 1. re-fold: each count becomes r6(max(0, count - a + b)) (the window re-weighted with the rest half-life; the default
//!    half-life is the expectation's own, and the count stays);
//! 2. surprise: with `surprise_gain` g > 0, each count is weighed by f = 1 + g min(zs/k, 1.5)/1.5 (contexts that kept
//!    surprising keep more weight) and the counts scaled back to their sum before (r6 each);
//! 3. merge: with `bucket` B < 3, the contexts whose key is the same with each declared number input in B bins (instead of the
//!    waking key's thirds) become one: the largest count's inputs (ties: key order), the counts summed;
//! 4. the learned deltas times `decay` (r6 each; 1 leaves the state as it is).
//!
//! The curiosity fold (RFC 0016) and the learned normal are left as they are. The line: `{at, consolidation {after, before,
//! decay, events, merged {count, keys}, moved}, n, prev, state}` with the memory's digests before and after (sha256 of its
//! checkpoint form), the window's event count, the merged-away keys, sum |count after - count before| over every key, and the
//! state digest after.
use crate::json::Json;
use crate::persona::{self, canon, digest_of, r6, ExpectMem, Kind as InKind, Persona, State, DEV_Z};
#[allow(unused_imports)]
use std::prelude::rust_2021::*;

/// The window of the rest pass inside the expectation's memory: events since the last pass, per context [a, b, k, zs]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RestWin { pub since: u64, pub w: Vec<(String, [f64; 4])> }
impl RestWin {
    /// The window as the memory's `window` key: {combos: per remembered combo in key order [a, b, k, zs], since}
    pub(crate) fn to_json(&self, keys: &[&str]) -> Json {
        let row = |k: &str| self.w.iter().find(|e| e.0 == k).map_or([0.0; 4], |e| e.1);
        Json::Obj(vec![("combos".into(), Json::Arr(keys.iter().map(|k| Json::Arr(row(k).iter().map(|x| Json::Num(*x)).collect())).collect())),
            ("since".into(), Json::Num(self.since as f64))])
    }
    /// A checkpoint's `window` for the memory's combos (`keys`, key order) -> the window; Err(what is wrong)
    pub(crate) fn read(j: &Json, keys: &[String]) -> Result<RestWin, String> {
        let kv = j.as_obj().filter(|kv| kv.len() == 2).ok_or("window: an object {combos, since}")?;
        let f = |k: &str| kv.iter().find(|(x, _)| x == k).map(|(_, v)| v).ok_or(format!("window: no {k}"));
        let since = f("since")?.as_f64().filter(|x| *x >= 0.0 && x.fract() == 0.0 && *x <= 9_007_199_254_740_992.0).ok_or("window.since: a whole number >= 0")? as u64;
        let rows = f("combos")?.as_arr().filter(|r| r.len() == keys.len()).ok_or("window.combos: one row per combo")?;
        let mut w = vec![];
        for (k, r) in keys.iter().zip(rows) {
            let x: Vec<f64> = r.as_arr().filter(|r| r.len() == 4).map(|r| r.iter().filter_map(Json::as_f64).filter(|x| x.is_finite() && *x >= 0.0 && r6(*x) == *x).collect()).unwrap_or_default();
            if x.len() != 4 { return Err("window.combos: each [a, b, k, zs], numbers >= 0 at 6 decimals".into()); }
            if x.iter().any(|v| *v != 0.0) { w.push((k.clone(), [x[0], x[1], x[2], x[3]])); } }
        Ok(RestWin { since, w })
    }
    /// One event folded in (after the memory's own step): every row decays (`d` the expectation's factor, `db` the pass's), the
    /// event's context gains a visit and its z+
    pub(crate) fn step(&mut self, key: &str, d: f64, db: f64, zp: f64) {
        for e in &mut self.w { e.1[0] *= d; e.1[1] *= db; }
        match self.w.iter_mut().find(|e| e.0 == key) { Some(e) => { e.1[0] += 1.0; e.1[1] += 1.0; e.1[2] += 1.0; e.1[3] += zp; } None => self.w.push((key.to_string(), [1.0, 1.0, 1.0, zp])) }
        self.since += 1;
    }
    /// The canonical rounding and the memory's prune: rows at 6 decimals, a context the memory dropped is dropped here
    pub(crate) fn settle(&mut self, combos: &[(String, f64, Json)]) {
        for e in &mut self.w { for x in e.1.iter_mut() { *x = r6(*x); } }
        self.w.retain(|e| combos.iter().any(|c| c.0 == e.0));
    }
}
/// The merge key: the waking key (`combo_key`) with each declared number input in `bins` bins instead of thirds
fn merge_key(p: &Persona, raw: &[(String, Json)], bins: u8) -> String {
    if bins >= 3 { return persona::combo_key(p, raw); }
    let mut kv: Vec<(String, Json)> = raw.iter().filter(|(k, v)| k != "elapsed_hours" && !matches!(v, Json::Null | Json::Bool(false)) && !matches!(v, Json::Num(x) if *x == 0.0))
        .map(|(k, v)| (k.clone(), match (p.input(k), v) {
            (Some(i), Json::Num(x)) if i.kind == InKind::Number && i.max > i.min => Json::Str(format!("bin{}of{bins}", (((x - i.min) / (i.max - i.min)) * f64::from(bins)).floor().clamp(0.0, f64::from(bins - 1)) as i64)),
            _ => v.clone() })).collect();
    kv.sort_by(|a, b| a.0.cmp(&b.0)); canon(&Json::Obj(kv))
}
/// The pass on the memory alone -> the line's `consolidation` object; Err without a rest block or before the canonical fold.
/// Leaves the window empty (the next pass reads the events after this one).
#[inline(never)]
pub fn consolidate_mem(p: &Persona, mem: &mut ExpectMem) -> Result<Json, String> {
    let ex = p.expect.as_ref().ok_or("the persona has no expectation block")?;
    let rp = ex.rest.as_ref().ok_or("the persona declares no expectation.rest block (the rest pass is off)")?;
    if !mem.canon { return Err("a rest pass needs a strand of engine 0.14.0 or later".into()); }
    let before = digest_of(&canon(&mem.to_json()));
    let b0: Vec<(String, f64)> = mem.combos.iter().map(|c| (c.0.clone(), c.1)).collect();
    let win = mem.win.take().unwrap_or_default();
    // 1. the window re-folded with the rest half-life
    for c in &mut mem.combos { if let Some(e) = win.w.iter().find(|e| e.0 == c.0) { c.1 = r6((c.1 - e.1[0] + e.1[1]).max(0.0)); } }
    // 2. contexts that kept surprising keep more weight (the total kept)
    if rp.surprise_gain > 0.0 {
        let f: Vec<f64> = mem.combos.iter().map(|c| win.w.iter().find(|e| e.0 == c.0).filter(|e| e.1[2] > 0.0)
            .map_or(1.0, |e| 1.0 + rp.surprise_gain * (e.1[3] / e.1[2]).min(DEV_Z) / DEV_Z)).collect();
        let t: f64 = mem.combos.iter().map(|c| c.1).sum(); let s: f64 = mem.combos.iter().zip(&f).map(|(c, x)| c.1 * x).sum();
        if s > 0.0 { for (c, x) in mem.combos.iter_mut().zip(&f) { c.1 = r6(c.1 * x * t / s); } }
    }
    // 3. near-duplicate contexts merged (the same key with the numbers in `bucket` bins)
    let mut merged: Vec<String> = vec![];
    if rp.bucket < 3 {
        let mk: Vec<String> = mem.combos.iter().map(|c| merge_key(p, c.2.as_obj().unwrap_or(&[]), rp.bucket)).collect();
        let mut keep = vec![true; mem.combos.len()];
        for i in 0..mem.combos.len() {
            if !keep[i] { continue; }
            let g: Vec<usize> = (i..mem.combos.len()).filter(|&j| keep[j] && mk[j] == mk[i]).collect();
            if g.len() < 2 { continue; }
            // the group's head: the largest count (ties: key order)
            let h = *g.iter().min_by(|&&x, &&y| mem.combos[y].1.partial_cmp(&mem.combos[x].1).unwrap_or(core::cmp::Ordering::Equal).then(mem.combos[x].0.cmp(&mem.combos[y].0))).unwrap_or(&i);
            let sum = g.iter().fold(0.0, |a, &j| a + mem.combos[j].1);
            for &j in &g { if j != h { keep[j] = false; merged.push(mem.combos[j].0.clone()); } }
            mem.combos[h].1 = r6(sum);
        }
        let mut it = keep.iter(); mem.combos.retain(|_| *it.next().unwrap_or(&true));
    }
    mem.combos.retain(|c| c.1 > 0.0);
    mem.win = Some(RestWin::default());
    merged.sort();
    let after = digest_of(&canon(&mem.to_json()));
    let mut moved = 0.0;
    for (k, x) in &b0 { moved += (mem.combos.iter().find(|c| &c.0 == k).map_or(0.0, |c| c.1) - x).abs(); }
    for c in &mem.combos { if !b0.iter().any(|b| b.0 == c.0) { moved += c.1; } }
    let s = |x: &str| Json::Str(x.to_string());
    Ok(Json::Obj(vec![("after".into(), s(&after)), ("before".into(), s(&before)), ("decay".into(), Json::Num(rp.decay)), ("events".into(), Json::Num(win.since as f64)),
        ("merged".into(), Json::Obj(vec![("count".into(), Json::Num(merged.len() as f64)), ("keys".into(), Json::Arr(merged.into_iter().map(Json::Str).collect()))])),
        ("moved".into(), Json::Num(r6(moved)))]))
}
/// Step 4: the learned deltas times the pass's `decay` (r6 each), in place (no state by value on a frame); 1 leaves the state
#[inline(never)]
pub fn decay_learned(p: &Persona, st: &mut State) {
    let Some(rp) = p.expect.as_ref().and_then(|e| e.rest.as_ref()) else { return };
    if rp.decay >= 1.0 { return; }
    for (_, d) in st.learned.iter_mut() { for x in d.iter_mut() { let y = r6(rp.decay * *x); *x = if y == 0.0 { 0.0 } else { y }; } }
    st.seal(p);
}
/// The `consolidation` line after the line whose sha256 is `prev` (canonical JSON: at, consolidation, n, prev, state)
pub fn line(prev: &str, at: &str, n: u64, c: Json, state: &str) -> String {
    let s = |x: &str| Json::Str(x.to_string());
    canon(&Json::Obj(vec![("at".into(), s(at)), ("consolidation".into(), c), ("n".into(), Json::Num(n as f64)), ("prev".into(), s(prev)), ("state".into(), s(state))]))
}
/// The first field (dotted path) where the line `got` differs from the line the replay wrote, `want`
pub fn first_field(want: &str, got: &str) -> String {
    fn walk(a: &Json, b: &Json, path: &str) -> Option<String> {
        match (a, b) {
            (Json::Obj(x), Json::Obj(y)) => {
                for (k, v) in x { let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    match y.iter().find(|(j, _)| j == k) { None => return Some(p), Some((_, w)) => if let Some(d) = walk(v, w, &p) { return Some(d); } } }
                y.iter().find(|(k, _)| !x.iter().any(|(j, _)| j == k)).map(|(k, _)| if path.is_empty() { k.clone() } else { format!("{path}.{k}") }) }
            _ => (canon(a) != canon(b)).then(|| path.to_string()) }
    }
    match (crate::json::parse(want), crate::json::parse(got)) { (Ok(a), Ok(b)) => walk(&a, &b, "").unwrap_or_else(|| "the bytes".into()), _ => "the line".into() }
}

#[cfg(test)]
mod tests {
    use crate::json::{self, Json};
    use crate::persona::{build, canon, init, run_program, Flags};
    use crate::strand::{self, Clock, Live};
    fn run(prog: &Json, f: &Flags) -> Json { run_program(prog, f, 1, 100, 0) }
    const DOC: &str = r#"{"probbit_persona":1,"identity":{"name":"R","version":"1","seed":3},
        "traits":[{"id":"verbosity","levels":["terse","short","full"],"logw":[0.2,0.5,0.1]},{"id":"initiative","levels":["wait","offer","lead"],"logw":[0.3,0.2,0]}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.5,"half_life_hours":6}],
        "inputs":[{"id":"night","kind":"flag","effects":{"verbosity":-0.5}},{"id":"ping","kind":"flag","effects":{"valence":0.6}},{"id":"odd","kind":"number","max":3,"effects":{"valence":-0.8,"verbosity":0.6}},{"id":"praise","kind":"flag"},{"id":"criticism","kind":"flag"}],
        "habits":[{"id":"quiet_at_night","when":{"night":true},"then":{"verbosity":["terse"],"initiative":["wait","offer"]}}],
        "learning":{"from":["praise","criticism"],"traits":["verbosity"],"rate":0.5,"step_cap":0.2,"total_cap":1},
        "expectation":{"half_life_events":8,"max_combos":6REST}}"#;
    const REST: &str = r#","rest":{"half_life":24,"bucket":2,"surprise_gain":1,"decay":0.5}"#;
    fn doc(r: &str) -> Json { json::parse(&DOC.replace("REST", r)).unwrap() }
    fn ev(s: &str) -> Json { json::parse(s).unwrap() }
    fn e(k: usize) -> Json { ev(&match k % 5 { 0 => r#"{"ping":true,"elapsed_hours":0.25}"#.to_string(), 1 => format!(r#"{{"odd":{},"elapsed_hours":0.25}}"#, (k * 7) % 4),
        2 => (if k % 3 == 0 { r#"{"praise":true,"elapsed_hours":0.25}"# } else { r#"{"criticism":true,"elapsed_hours":0.25}"# }).to_string(), 3 => format!(r#"{{"odd":{}.5,"night":true,"elapsed_hours":0.5}}"#, k % 3), _ => r#"{"night":true,"elapsed_hours":0.25}"#.to_string() }) }
    /// A life of `n` events, a rest pass every `every` events (0: never), a checkpoint every `cp` (0: never) -> (text, individual)
    fn life(r: &str, n: usize, every: usize, cp: usize) -> (String, Box<Live>) {
        let d = doc(r); let p = build(&d).unwrap();
        let (mut lv, header) = Live::start(p.clone(), &d, init(&p, Some(2), true, &run), Clock::Fixed, "probbit 0.14.0");
        let mut text = format!("{header}\n");
        for k in 0..n { let (s, l) = lv.event(&e(k), &run).unwrap(); text.push_str(&l); text.push('\n');
            if cp > 0 && lv.n % cp as u64 == 0 { text.push_str(&lv.checkpoint(&s)); text.push('\n'); }
            if every > 0 && lv.n % every as u64 == 0 { text.push_str(&lv.consolidate(&format!("2026-10-28T03:{:02}:00Z", k % 60)).unwrap()); text.push('\n'); } }
        (text, Box::new(lv))
    }
    #[test]
    fn the_block_is_read_strictly_and_off_by_default() {
        let r = |x: &str| match build(&doc(x)) { Err(e) => format!("{}: {}", e.path, e.msg), Ok(_) => "ok".into() };
        assert_eq!(r(""), "ok"); assert_eq!(r(REST), "ok"); assert_eq!(r(r#","rest":{}"#), "ok");
        assert!(r(r#","rest":{"bucket":4}"#).starts_with("expectation.rest.bucket"), "{}", r(r#","rest":{"bucket":4}"#));
        assert!(r(r#","rest":{"decay":1.5}"#).starts_with("expectation.rest.decay"));
        assert!(r(r#","rest":{"nap":1}"#).starts_with("expectation.rest"));
        // without the block: the pass is refused, nothing is written, the memory has no window
        let (text, mut lv) = life("", 12, 0, 0);
        assert_eq!(lv.consolidate("t").err().map(|e| e.path), Some("rest".to_string()));
        assert!(!text.contains("consolidation") && !canon(&lv.expect.as_ref().unwrap().to_json()).contains("window"));
        // the defaults: nothing moves (the window re-folded with the expectation's own half-life, no gain, no merge, no decay)
        let (_, mut lv) = life(r#","rest":{}"#, 30, 0, 0); let st0 = lv.st.digest.clone();
        let l = lv.consolidate("t").unwrap(); let j = json::parse(&l).unwrap(); let c = j.get("consolidation").unwrap();
        assert_eq!(c.get("moved").and_then(Json::as_f64), Some(0.0), "{l}"); assert_eq!(lv.st.digest, st0);
        assert_eq!(c.get("merged").and_then(|m| m.get("count")).and_then(Json::as_f64), Some(0.0));
    }
    #[test]
    fn the_pass_replays_verifies_resumes_and_refuses_forged_lines() {
        for cp in [0, 7] {
            let (text, lv) = life(REST, 60, 10, cp);
            assert_eq!(text.matches(r#""consolidation":"#).count(), 6);
            let v = strand::verify(&text, &run).unwrap_or_else(|(i, w)| panic!("line {i}: {w}"));
            assert_eq!(v.get("events").and_then(Json::as_f64), Some(60.0), "{}", canon(&v));
            let ls = strand::lines(&text);
            let (mut r, s0) = strand::Replay::open_lines(&ls).unwrap();
            for i in s0..ls.len() { r.step_at(&ls, i, &run).unwrap_or_else(|(i, w)| panic!("line {i}: {w}")); }
            assert_eq!(canon(&r.live.expect.as_ref().unwrap().to_json()), canon(&lv.expect.as_ref().unwrap().to_json()));
            assert_eq!(r.live.st.digest, lv.st.digest);
            let rs = strand::resume(&text, &lv.p, lv.st.clone(), Clock::Fixed).unwrap();
            assert_eq!((rs.n, rs.head().to_string(), canon(&rs.expect.as_ref().unwrap().to_json())), (lv.n, lv.head().to_string(), canon(&lv.expect.as_ref().unwrap().to_json())));
            if cp > 0 { strand::verify_from_checkpoint(&text, &run).unwrap_or_else(|(i, w)| panic!("from checkpoint: line {i}: {w}"));
                assert!(text.contains(r#""window":{"#), "the checkpoint carries the window");
                // the resume from a record behind the last line: the state an event logs before the last rest line
                let k = ls.iter().rposition(|l| l.contains(r#""consolidation":"#)).unwrap() - 1;
                let rp = strand::Replay::open_lines(&ls[..=k]).map(|(mut r, s0)| { for i in s0..=k { r.step_at(&ls, i, &run).unwrap(); } r.live.st.clone() }).unwrap();
                let rr = strand::resume_from_record(&text, &lv.p, Box::new(rp), Clock::Fixed, &run).unwrap_or_else(|e| panic!("resume from record: {}", e.msg));
                assert_eq!((rr.n, rr.head().to_string(), rr.st.digest.clone(), canon(&rr.expect.as_ref().unwrap().to_json())), (lv.n, lv.head().to_string(), lv.st.digest.clone(), canon(&lv.expect.as_ref().unwrap().to_json()))); }
            // the pass moved something: merges (bucket 2), weights, the learned deltas halved
            let cs: Vec<Json> = ls.iter().filter(|l| l.contains(r#""consolidation":"#)).map(|l| json::parse(l).unwrap()).collect();
            assert!(cs.iter().any(|c| c.get("consolidation").and_then(|c| c.get("moved")).and_then(Json::as_f64).unwrap_or(0.0) > 0.0));
            eprintln!("R15 lines: {}", cs.iter().map(|c| canon(c.get("consolidation").unwrap())).collect::<Vec<_>>().join("\n"));
            // forged lines: each field edited -> refused at that line, naming the field
            let li = ls.iter().position(|l| l.contains(r#""consolidation":"#)).unwrap();
            let line = ls[li]; let j = json::parse(line).unwrap();
            let edits: Vec<(&str, String)> = vec![
                ("consolidation.moved", { let m = j.get("consolidation").unwrap().get("moved").and_then(Json::as_f64).unwrap(); line.replacen(&format!(r#""moved":{}"#, canon(&Json::Num(m))), r#""moved":9.5"#, 1) }),
                ("consolidation.decay", line.replacen(r#""decay":0.5"#, r#""decay":1"#, 1)),
                ("consolidation.events", line.replacen(r#""events":10"#, r#""events":11"#, 1)),
                ("n", line.replacen(r#""n":10"#, r#""n":9"#, 1)),
                ("state", { let s = j.get("state").and_then(Json::as_str).unwrap(); line.replacen(s, &format!("sha256:{}", "0".repeat(64)), 1) }),
                ("consolidation.after", { let s = j.get("consolidation").unwrap().get("after").and_then(Json::as_str).unwrap(); line.replacen(s, &format!("sha256:{}", "1".repeat(64)), 1) }),
            ];
            for (field, forged) in edits {
                assert_ne!(forged, line, "{field}: the edit applies");
                let t2 = text.replacen(line, &forged, 1);
                let (i, why) = strand::verify(&t2, &run).err().unwrap_or_else(|| panic!("{field}: a forged line verifies"));
                assert_eq!(i, li + 1, "{field}: {why}"); assert!(why.ends_with(&format!("at {field}")), "{field}: {why}");
            }
        }
    }
    #[test]
    fn the_reflection_reports_the_rest_pass_and_old_engines_refuse_it() {
        let (text, _) = life(REST, 30, 10, 0);
        let ls = strand::lines(&text);
        let r = crate::reflect::reflect_window(&ls, crate::reflect::Window::Last(30)).unwrap();
        let s = canon(&r.doc);
        assert!(s.contains(r#""consolidation":{"lines":3"#) && s.contains("The rest pass ran 3 times in this window"), "{s}");
        // without a rest line in the window, no key and no sentence
        let (t0, _) = life(REST, 30, 0, 0);
        let r0 = crate::reflect::reflect_window(&strand::lines(&t0), crate::reflect::Window::Last(30)).unwrap();
        assert!(!canon(&r0.doc).contains("consolidation") && !canon(&r0.doc).contains("rest pass"));
        // a consolidation line in an old engine's strand is refused (the line kind is engine-gated)
        let d = doc(REST); let p = build(&d).unwrap();
        let (mut lv, header) = Live::start(p.clone(), &d, init(&p, Some(2), true, &run), Clock::Fixed, "probbit 0.13.0");
        let (_, l) = lv.event(&e(0), &run).unwrap();
        assert!(lv.consolidate("t").is_err(), "the fold before 0.14.0 has no window: refused");
        let _ = (header, l);
    }
}
