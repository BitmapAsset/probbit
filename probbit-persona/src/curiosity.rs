//! RFC 0016 curiosity (docs/rfcs/0016-curiosity.md): a declared drive fed by the measured surprise, and the self-started idle
//! event (`stir`) it can gate. Mechanism only: the drive's level is arithmetic over two numbers the expectation block (RFC 0009)
//! already measures, and it moves the stance through one engine-owned number input, `cur_level`, as `pace` moves it through its
//! two. Off by default: a persona without the `curiosity` block gives every document, state and strand as before.
//!
//! - `surprise_recent` s: an exponentially weighted average of |z| (the deviation's z, read from the logged prediction as the
//!   expectation's normal reads it) with a half-life in events; it starts at `quiet_z`.
//! - `failed_contexts` F: of the contexts the expectation remembers (its input combos), those whose last K visits all missed
//!   (|z| > `miss_z` on each).
//! - level = clip(quiet x max(0, 1 - s / quiet_z) + failed x F, 0, cap), at 6 decimals: it rises while nothing new arrives (s
//!   low for long) and while contexts keep failing, and falls when a surprising event raises s.
//!
//! The drive's state (s, the per-context miss runs, the hours since the last stir, the last event's asleep flag) is a fold of the
//! logged lines inside the expectation's canonical memory (engine 0.14.0 on), so checkpoints carry it and a resume or a replay
//! from a checkpoint reads it back. `stir_due` answers from that state and the idle hours since the last event.
use super::*;

/// The engine-owned number input the drive lowers to (reserved only with the block)
pub const CUR_ID: &str = "cur_level";
/// The input key that marks a self-started event (reserved only with a `stir` block): its value is the stir's class
pub const STIR_KEY: &str = "stir";

/// The declared drive
#[derive(Clone, Debug)]
pub struct Curio { pub hl: f64, pub quiet_z: f64, pub miss_z: f64, pub last_k: usize, pub w_quiet: f64, pub w_failed: f64, pub cap: f64, pub stir: Option<StirSpec>,
    /// the ordinal shift per unit of the level, per trait or mood (the lowered input's effects)
    pub effects: Vec<(String, f64)> }
impl Curio {
    /// The largest shift the drive can put on the field of the variables in `comp`: sum of |effects| x cap (RFC 0004's residual)
    pub fn shift(&self, comp: &[String]) -> f64 { r6(self.effects.iter().filter(|(k, _)| comp.contains(k)).fold(0.0, |a, (_, w)| a + w.abs()) * self.cap) }
}
/// The self-start: due when the level is at least `threshold`, at most one per `every_min` minutes; while the last event's
/// `asleep` flag was on, only with `when_asleep` (flag, minutes, class), whose class never speaks
#[derive(Clone, Debug)]
pub struct StirSpec { pub threshold: f64, pub every_min: f64, pub class: String, pub speaks: bool, pub asleep: Option<(String, f64, String)> }
impl StirSpec {
    /// The classes an event may carry under `stir`: the awake one, then the asleep one
    pub fn classes(&self) -> Vec<&str> { let mut v = vec![self.class.as_str()]; if let Some((_, _, c)) = &self.asleep { if c != &self.class { v.push(c); } } v }
}

/// The `curiosity` block of a persona whose traits, moods and inputs are `ids`, `moods` and `inputs` -> (the drive, the effects
/// map its input carries). Refused at the field's path (C-R1 .. C-R3 of the RFC).
pub fn spec(cj: &Json, traits: &[String], flags: &[String]) -> R<(Curio, Json)> {
    let ck = keys(cj, "curiosity", &["half_life_events", "quiet_z", "miss_z", "last_k", "weights", "cap", "effects", "stir", "comment"], &["effects"])?;
    let f = |k: &str, d: f64| num_or(ck, k, d, "curiosity", Some(-1e6), Some(1e6), false);
    let (hl, quiet_z, miss_z, cap) = (f("half_life_events", 8.0)?, f("quiet_z", 0.5)?, f("miss_z", 1.5)?, f("cap", 2.0)?);
    need(hl >= 1.0, "curiosity.half_life_events", "must be >= 1 (events)")?;
    need(quiet_z > 0.0, "curiosity.quiet_z", "must be > 0 (the recent surprise below which nothing is new)")?;
    need(miss_z > 0.0, "curiosity.miss_z", "must be > 0 (the |z| above which a visit missed)")?;
    need(cap > 0.0, "curiosity.cap", "must be > 0")?;
    let last_k = num_or(ck, "last_k", 3.0, "curiosity", Some(1.0), Some(16.0), true)? as usize;
    let (w_quiet, w_failed) = match get(ck, "weights").filter(|x| !x.is_null()) { None => (1.0, 0.5), Some(w) => {
        let wk = keys(w, "curiosity.weights", &["quiet", "failed"], &[])?;
        (num_or(wk, "quiet", 1.0, "curiosity.weights", Some(0.0), Some(50.0), false)?, num_or(wk, "failed", 0.5, "curiosity.weights", Some(0.0), Some(50.0), false)?) } };
    let ek = get(ck, "effects").and_then(Json::as_obj).filter(|o| !o.is_empty()).ok_or_else(|| perr("curiosity.effects", "a mapping trait or mood -> ordinal shift per unit of the level"))?;
    let mut effects = vec![];
    for (k, x) in ek { let ep = at("curiosity.effects", k);
        need(k != "pursue", &ep, "the curiosity drive never moves pursue")?; need(traits.contains(k), &ep, &format!("unknown trait or mood {k:?}"))?;
        effects.push((k.clone(), num(Some(x), &ep, Some(-50.0), Some(50.0), false)?)); }
    let stir = match get(ck, "stir").filter(|x| !x.is_null()) { None => None, Some(sj) => {
        let sk = keys(sj, "curiosity.stir", &["threshold", "every_minutes", "class", "speaks", "when_asleep"], &["threshold"])?;
        let threshold = num(get(sk, "threshold"), "curiosity.stir.threshold", Some(0.0), Some(cap), false)?;
        need(threshold > 0.0, "curiosity.stir.threshold", "must be > 0 (a level of 0 never stirs)")?;
        let every_min = num_or(sk, "every_minutes", 30.0, "curiosity.stir", Some(1.0), Some(1e6), false)?;
        let class = match get(sk, "class") { None => "check".to_string(), x => ident(x, "curiosity.stir.class", "class")? };
        let speaks = match get(sk, "speaks") { None | Some(Json::Null) => true, Some(Json::Bool(b)) => *b, Some(_) => return Err(perr("curiosity.stir.speaks", "true | false")) };
        let asleep = match get(sk, "when_asleep").filter(|x| !x.is_null()) { None => None, Some(aj) => {
            let ak = keys(aj, "curiosity.stir.when_asleep", &["flag", "every_minutes", "class"], &["flag"])?;
            let fl = ident(get(ak, "flag"), "curiosity.stir.when_asleep.flag", "flag")?;
            need(flags.contains(&fl), "curiosity.stir.when_asleep.flag", "a declared flag input (true on the events of the asleep hours)")?;
            let m = num_or(ak, "every_minutes", 120.0, "curiosity.stir.when_asleep", Some(1.0), Some(1e6), false)?;
            need(m >= every_min, "curiosity.stir.when_asleep.every_minutes", "at least the awake every_minutes (asleep stirs are rarer)")?;
            let c = match get(ak, "class") { None => "glance".to_string(), x => ident(x, "curiosity.stir.when_asleep.class", "class")? };
            need(c != class || !speaks, "curiosity.stir.when_asleep.class", "the asleep class never speaks: name another class than a speaking awake one")?;
            Some((fl, m, c)) } };
        Some(StirSpec { threshold, every_min, class, speaks, asleep }) } };
    Ok((Curio { hl, quiet_z, miss_z, last_k, w_quiet, w_failed, cap, stir, effects }, Json::Obj(ek.to_vec())))
}

/// The drive's fold (inside the expectation's canonical memory)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurMem { pub s: f64, pub since: Option<f64>, pub asleep: bool, pub ctx: Vec<(String, Vec<bool>)> }
impl CurMem {
    pub fn new(c: &Curio) -> CurMem { CurMem { s: c.quiet_z, ..Default::default() } }
    /// The contexts whose last K visits all missed
    pub fn failed(&self, c: &Curio) -> usize { self.ctx.iter().filter(|(_, v)| v.len() == c.last_k && v.iter().all(|m| *m)).count() }
    /// The drive's level from this fold
    pub fn level(&self, c: &Curio) -> f64 { r6((c.w_quiet * (1.0 - self.s / c.quiet_z).max(0.0) + c.w_failed * self.failed(c) as f64).clamp(0.0, c.cap)) }
    /// The stance's `drives.curiosity` (and the line's receipt): the level and its two inputs
    pub fn doc(&self, c: &Curio) -> Json {
        Json::Obj(vec![("failed_contexts".into(), Json::Num(self.failed(c) as f64)), ("level".into(), Json::Num(self.level(c))), ("surprise_recent".into(), Json::Num(self.s))])
    }
    /// One event folded in: `key` its combo, `z` its z (None while cold), `kv` its logged inputs. Before the expectation's normal
    /// reads the event (z is the deviation's z). Every number at 6 decimals.
    pub fn step(&mut self, c: &Curio, key: &str, z: Option<f64>, kv: &[(String, Json)]) {
        if let Some(z) = z { let lam = 0.5f64.powf(1.0 / c.hl); self.s = r6(lam * self.s + (1.0 - lam) * r6(z.abs()));
            let miss = r6(z.abs()) > c.miss_z;
            match self.ctx.binary_search_by(|x| x.0.as_str().cmp(key)) {
                Ok(i) => { let v = &mut self.ctx[i].1; v.push(miss); if v.len() > c.last_k { v.remove(0); } }
                Err(i) => self.ctx.insert(i, (key.to_string(), vec![miss])) } }
        let h = get(kv, "elapsed_hours").and_then(Json::as_f64).unwrap_or(0.0);
        self.since = if matches!(get(kv, STIR_KEY), Some(Json::Str(_))) && c.stir.is_some() { Some(0.0) } else { self.since.map(|s| r6(s + h)) };
        self.asleep = c.stir.as_ref().and_then(|st| st.asleep.as_ref()).is_some_and(|(fl, _, _)| matches!(get(kv, fl), Some(Json::Bool(true))));
    }
    /// The memory's contexts after the expectation's own prune: a context the expectation dropped is dropped here
    pub fn prune(&mut self, keep: &[(String, f64, Json)]) { self.ctx.retain(|(k, _)| keep.iter().any(|c| &c.0 == k)); }
    /// As a checkpoint line's `expect.curiosity`: {asleep, ctx: [[key, [miss, ...]], ...] in key order, s, since}
    pub fn to_json(&self) -> Json {
        Json::Obj(vec![("asleep".into(), Json::Bool(self.asleep)),
            ("ctx".into(), Json::Arr(self.ctx.iter().map(|(k, v)| Json::Arr(vec![Json::Str(k.clone()), Json::Arr(v.iter().map(|m| Json::Bool(*m)).collect())])).collect())),
            ("s".into(), Json::Num(self.s)), ("since".into(), self.since.map_or(Json::Null, Json::Num))])
    }
    /// A checkpoint line's `expect.curiosity` -> the fold; Err(what is wrong)
    pub fn read(c: &Curio, j: &Json) -> Result<CurMem, String> {
        let kv = j.as_obj().filter(|kv| kv.len() == 4).ok_or("curiosity: an object {asleep, ctx, s, since}")?;
        let f = |k: &str| get(kv, k).ok_or(format!("curiosity: no {k}"));
        let s = f("s")?.as_f64().filter(|x| x.is_finite() && *x >= 0.0 && r6(*x) == *x).ok_or("curiosity.s: a number >= 0 at 6 decimals")?;
        let since = match f("since")? { Json::Null => None, Json::Num(x) if x.is_finite() && *x >= 0.0 && r6(*x) == *x => Some(*x), _ => return Err("curiosity.since: null or hours >= 0 at 6 decimals".into()) };
        let Json::Bool(asleep) = f("asleep")? else { return Err("curiosity.asleep: true or false".into()) };
        let mut ctx: Vec<(String, Vec<bool>)> = vec![];
        for x in f("ctx")?.as_arr().ok_or("curiosity.ctx: a list")? {
            let Some([Json::Str(k), Json::Arr(m)]) = x.as_arr() else { return Err("curiosity.ctx: each [key, [miss, ...]]".into()) };
            if ctx.last().is_some_and(|l| l.0 >= *k) { return Err("curiosity.ctx: in key order, each key once".into()); }
            let m: Option<Vec<bool>> = m.iter().map(|b| if let Json::Bool(b) = b { Some(*b) } else { None }).collect();
            let m = m.filter(|m| !m.is_empty() && m.len() <= c.last_k).ok_or("curiosity.ctx: 1 to last_k misses (true / false)")?;
            ctx.push((k.clone(), m)); }
        Ok(CurMem { s, since, asleep: *asleep, ctx })
    }
}

/// A self-started idle event that is due: its class, whether it may speak, whether the individual was asleep, the level that
/// gated it, and the minutes since the last stir (None: none yet)
#[derive(Clone, Debug, PartialEq)]
pub struct Stir { pub class: String, pub speaks: bool, pub asleep: bool, pub level: f64, pub since_minutes: Option<f64> }
impl Stir {
    /// The control query's answer and the event a host feeds when it acts on it (`{"stir": class}` plus its own readings)
    pub fn to_json(&self) -> Json {
        Json::Obj(vec![("asleep".into(), Json::Bool(self.asleep)), ("class".into(), Json::Str(self.class.clone())), ("level".into(), Json::Num(self.level)),
            ("since_minutes".into(), self.since_minutes.map_or(Json::Null, |m| Json::Num(r6(m)))), ("speaks".into(), Json::Bool(self.speaks))])
    }
}
/// Is a stir due `idle_hours` after the last event? Exactly from the fold: the level at least the threshold; while asleep only
/// with `when_asleep` (its class, its rate); at most one per the declared minutes since the last stir (none yet: due)
pub fn stir_due(c: &Curio, m: &CurMem, idle_hours: f64) -> Option<Stir> {
    let sp = c.stir.as_ref()?; let level = m.level(c);
    if level < sp.threshold { return None; }
    let (every, class, speaks) = if m.asleep { let (_, e, cl) = sp.asleep.as_ref()?; (*e, cl.clone(), false) } else { (sp.every_min, sp.class.clone(), sp.speaks) };
    let since = m.since.map(|s| r6((s + idle_hours.max(0.0)) * 60.0));
    if since.is_some_and(|s| s < every) { return None; }
    Some(Stir { class, speaks, asleep: m.asleep, level, since_minutes: since })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strand::{self, Clock, Live};
    fn run(prog: &Json, f: &Flags) -> Json { run_program(prog, f, 1, 100, 0) }
    fn g<'a>(j: &'a Json, ks: &[&str]) -> &'a Json { static NULL: Json = Json::Null; ks.iter().try_fold(j, |x, k| x.get(k)).unwrap_or(&NULL) }
    const DOC: &str = r#"{"probbit_persona":1,"identity":{"name":"C","version":"1","seed":3},
        "traits":[{"id":"verbosity","levels":["terse","short","full"],"logw":[0.2,0.5,0.1]},{"id":"initiative","levels":["wait","offer","lead"],"logw":[0.3,0.2,0]}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.5,"half_life_hours":6}],
        "inputs":[{"id":"night","kind":"flag","effects":{"verbosity":-0.5}},{"id":"ping","kind":"flag","effects":{"valence":0.6}},{"id":"odd","kind":"number","max":3,"effects":{"valence":-0.8,"verbosity":0.6}}],
        "habits":[{"id":"quiet_at_night","when":{"night":true},"then":{"verbosity":["terse"],"initiative":["wait","offer"]}}],
        "expectation":{"half_life_events":8,"max_combos":6}CUR}"#;
    const CUR: &str = r#","curiosity":{"half_life_events":4,"quiet_z":0.5,"miss_z":1.5,"last_k":2,"weights":{"quiet":1.5,"failed":0.5},"cap":2,"effects":{"initiative":0.8},
        "stir":{"threshold":0.6,"every_minutes":30,"class":"check","when_asleep":{"flag":"night","every_minutes":120,"class":"glance"}}}"#;
    fn doc(c: &str) -> Json { json::parse(&DOC.replace("CUR", c)).unwrap() }
    fn refused(c: &str) -> String { match build(&doc(c)) { Err(e) => format!("{}: {}", e.path, e.msg), Ok(_) => "ok".into() } }
    fn ev(s: &str) -> Json { json::parse(s).unwrap() }
    /// A day of varied events, then a dark night of identical sensed events every 10 minutes; the host polls `stir_due` after each
    /// event and feeds the stir it is offered -> (the strand's text, the individual, per night tick the level and the stir fed)
    fn life(with_checkpoints: bool) -> (String, Box<Live>, Vec<(f64, Option<Stir>)>) {
        let d = doc(CUR); let p = build(&d).unwrap();
        let (mut lv, header) = Live::start(p.clone(), &d, init(&p, Some(2), true, &run), Clock::Fixed, "probbit 0.14.0");
        let mut text = format!("{header}\n"); let mut night = vec![];
        let mut feed = |lv: &mut Live, e: &Json, text: &mut String| { let (s, l) = lv.event(e, &run).unwrap(); text.push_str(&l); text.push('\n');
            if with_checkpoints && lv.n % 10 == 0 { text.push_str(&lv.checkpoint(&s)); text.push('\n'); } s };
        for k in 0..24 { let e = match k % 4 { 0 => r#"{"ping":true,"elapsed_hours":0.25}"#.to_string(), 1 => format!(r#"{{"odd":{},"elapsed_hours":0.25}}"#, k % 3 + 1),
            2 => r#"{"elapsed_hours":0.25}"#.to_string(), _ => r#"{"ping":true,"odd":2,"elapsed_hours":0.25}"#.to_string() };
            feed(&mut lv, &ev(&e), &mut text); }
        for _ in 0..48 { feed(&mut lv, &ev(r#"{"night":true,"elapsed_hours":0.166667}"#), &mut text);
            let lvl = crate::persona::cur_level(&lv.p, lv.expect.as_ref().unwrap()).unwrap();
            let due = lv.stir_due(0.0);
            if let Some(s) = &due { let st = feed(&mut lv, &ev(&format!(r#"{{"night":true,"stir":"{}","elapsed_hours":0}}"#, s.class)), &mut text);
                assert!(g(&st, &["habits", "active"]).as_arr().unwrap().iter().any(|h| h.as_str() == Some("quiet_at_night")), "{}", canon(&st));
                assert_eq!(g(&st, &["stance", "verbosity", "level"]).as_str(), Some("terse"), "the habit binds on the stir");
                assert_eq!(g(&st, &["drives", "curiosity", "stir"]).as_str(), Some(s.class.as_str())); }
            night.push((lvl, due)); }
        (text, Box::new(lv), night)
    }
    #[test]
    fn the_block_is_read_strictly() {
        assert_eq!(refused(""), "ok");
        assert_eq!(refused(CUR), "ok");
        assert_eq!(match build(&json::parse(&DOC.replace("CUR", CUR).replace(r#","expectation":{"half_life_events":8,"max_combos":6}"#, "")).unwrap()) { Err(e) => format!("{}: {}", e.path, e.msg), Ok(_) => "ok".into() },
            "curiosity: a curiosity block needs an expectation block (the drive reads its measured surprise)");
        assert_eq!(refused(r#","curiosity":{"effects":{"pursue":1}}"#), "curiosity.effects.pursue: the curiosity drive never moves pursue");
        assert_eq!(refused(r#","curiosity":{"effects":{"nope":1}}"#), r#"curiosity.effects.nope: unknown trait or mood "nope""#);
        assert_eq!(refused(r#","curiosity":{"effects":{}}"#), "curiosity.effects: a mapping trait or mood -> ordinal shift per unit of the level");
        assert_eq!(refused(r#","curiosity":{"effects":{"initiative":1},"quiet_z":0}"#), "curiosity.quiet_z: must be > 0 (the recent surprise below which nothing is new)");
        assert_eq!(refused(r#","curiosity":{"effects":{"initiative":1},"stir":{"threshold":0.5,"when_asleep":{"flag":"ping2"}}}"#),
            "curiosity.stir.when_asleep.flag: a declared flag input (true on the events of the asleep hours)");
        assert_eq!(refused(r#","curiosity":{"effects":{"initiative":1},"stir":{"threshold":0.5,"every_minutes":60,"when_asleep":{"flag":"night","every_minutes":30}}}"#),
            "curiosity.stir.when_asleep.every_minutes: at least the awake every_minutes (asleep stirs are rarer)");
        assert_eq!(refused(r#","curiosity":{"effects":{"initiative":1},"stir":{"threshold":3}}"#).split(':').next(), Some("curiosity.stir.threshold"));
        let d = DOC.replace("CUR", CUR).replace(r#"{"id":"ping","kind":"flag""#, r#"{"id":"cur_level","kind":"flag""#).replace(r#""ping":true"#, r#""cur_level":true"#);
        assert_eq!(build(&json::parse(&d).unwrap()).err().map(|e| e.path), Some("inputs[1].id".to_string()));
        // off by default: without the block no cur_level, no drives key, and the digest is another
        let (a, b) = (build(&doc("")).unwrap(), build(&doc(CUR)).unwrap()); assert_ne!(a.digest, b.digest);
        let (sa, _) = turn(&a, &init(&a, Some(1), true, &run), &ev(r#"{"ping":true}"#), false, &run, false).unwrap();
        assert!(sa.get("drives").is_none() && g(&sa, &["inputs", "cur_level"]).is_null(), "{}", canon(&sa));
        let (sb, _) = turn(&b, &init(&b, Some(1), true, &run), &ev(r#"{"ping":true}"#), false, &run, false).unwrap();
        assert!(g(&sb, &["inputs", "cur_level"]).is_null(), "cur_level leaves inputs: {}", canon(&sb));
        assert_eq!(turn(&b, &init(&b, Some(1), true, &run), &ev(r#"{"cur_level":1}"#), false, &run, false).err().map(|e| e.path), Some("inputs.cur_level".to_string()));
        // the expectation block outside the digest still: with and without it the curiosity persona's digest... is another persona
        let d = doc(CUR); let p = build(&d).unwrap(); let (mut lv, _) = Live::start(p.clone(), &d, init(&p, Some(2), true, &run), Clock::Fixed, "probbit 0.14.0");
        assert_eq!(lv.event(&ev(r#"{"stir":"hum"}"#), &run).err().map(|e| format!("{}: {}", e.path, e.msg)), Some("event.stir: a declared stir class: check | glance".to_string()));
    }
    #[test]
    fn a_quiet_night_stirs_at_the_declared_rate_and_the_habit_binds() {
        let (_, lv, night) = life(false);
        let stirs: Vec<(usize, &Stir)> = night.iter().enumerate().filter_map(|(i, (_, s))| s.as_ref().map(|s| (i, s))).collect();
        eprintln!("R16 night: levels {:?}", night.iter().map(|x| x.0).collect::<Vec<_>>());
        eprintln!("R16 night: stirs at ticks {:?}", stirs.iter().map(|x| x.0).collect::<Vec<_>>());
        assert!(night[0].0 < 0.6, "the night starts below the threshold: {}", night[0].0);
        assert!(stirs.len() >= 2, "the quiet night raises the drive past the threshold");
        for w in stirs.windows(2) { assert!((w[1].0 - w[0].0) * 10 >= 120, "asleep: at most one stir per 120 minutes: {:?}", stirs); }
        assert!(stirs.iter().all(|(_, s)| s.class == "glance" && !s.speaks && s.asleep));
        // awake, the same level stirs at the awake class and rate
        assert!(lv.stir_due(10.0).is_some_and(|s| s.asleep), "the last event was a night event");
    }
    #[test]
    fn the_drive_replays_resumes_and_restarts_from_a_checkpoint() {
        for cps in [false, true] {
            let (text, lv, _) = life(cps);
            let v = strand::verify(&text, &run).unwrap_or_else(|(i, w)| panic!("line {i}: {w}"));
            assert_eq!(v.get("events").and_then(Json::as_f64), Some(lv.n as f64), "{}", canon(&v));
            let ls = strand::lines(&text); let (mut r, s0) = strand::Replay::open_lines(&ls).unwrap();
            for i in s0..ls.len() { r.step_at(&ls, i, &run).unwrap(); }
            for h in [0.0, 0.5, 3.0] { assert_eq!(r.live.stir_due(h), lv.stir_due(h)); }
            assert_eq!(r.live.expect.as_ref().unwrap().cur, lv.expect.as_ref().unwrap().cur);
            let rs = strand::resume(&text, &lv.p, lv.st.clone(), Clock::Fixed).unwrap();
            assert_eq!((rs.n, rs.head(), rs.expect.as_ref().unwrap().cur.clone()), (lv.n, lv.head(), lv.expect.as_ref().unwrap().cur.clone()));
            if cps { strand::verify_from_checkpoint(&text, &run).unwrap_or_else(|(i, w)| panic!("from checkpoint: line {i}: {w}"));
                assert!(text.contains(r#""curiosity":{"asleep":"#), "the checkpoint carries the fold"); }
            // an edited receipt diverges at its line
            let i = text.find(r#""surprise_recent":"#).unwrap(); let mut t2 = text.clone(); t2.replace_range(i..i + 18, r#""surprise_recent":9"#);
            assert!(strand::verify(&t2, &run).is_err());
        }
    }
}
