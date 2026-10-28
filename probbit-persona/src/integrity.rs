//! RFC 0014 integrity (docs/rfcs/0014-integrity.md): per turn, how far the individual has drifted from the self its persona
//! declares, exact from the state and the persona document (mechanism: arithmetic over declared numbers; nothing here writes a
//! state). Off by default: only a persona whose `identity` block declares one of the self keys (`keep`, `warn`, `cue`, `traits`)
//! gets it, and those keys stay out of the digest (as RFC 0004's `bound`, amendment A1), so every state is as without them.
//!
//! - `drift` per trait: the total variation (half the L1 over the trait's levels) between the odds at the rest probe (one event of
//!   the rest hours idle, no inputs: RFC 0004's probe) of the current individual rested, and of the same seed's fresh individual
//!   (the declared persona); summed over the traits. Rested (`rested`): the channels of the hour (moods with `half_life_hours`,
//!   the pace average, the load, the credit awaiting feedback, the last levels and stance, the agenda's step) at the fresh
//!   individual's values, everything that persists as it is (learned deltas, history, counted moods, drives, the senses' normals,
//!   the gates): the drift
//!   measures what stays, not the mood of the hour, and the rest probe's cache hits whenever only the hour moved.
//! - `allowance` per trait: RFC 0004's epsilon at the individual's own seed when the persona declares a `bound` block and the
//!   trait is learned (null otherwise); `bound_used` = drift / allowance (0 without an allowance).
//! - `used`: the summed drift over the traits with an allowance as a share of the summed allowance (0 without one): the dial.
//! - `habits_held`: of the habits `identity.keep` names, those that bound this turn (`held`) and those that did not (`not_fired`).
//! - `cue`: the persona's own `identity.cue`, when a trait's `bound_used` reaches `identity.warn` (null otherwise).
use super::*;

/// The self keys of the `identity` block
pub const KEYS: [&str; 4] = ["keep", "warn", "cue", "traits"];
/// The declared self: identity-bearing habits, the warn share, the cue, the traits measured, the rest probe's hours and the
/// `bound` block (when declared) that gives each learned trait its allowance
#[derive(Clone)]
pub struct Ident { pub keep: Vec<String>, pub warn: f64, pub cue: Option<String>, pub traits: Vec<String>, pub rest: f64, pub bound: Option<bound::Bound> }
/// Whether an `identity` block declares a self key
pub fn declared(idn: &Json) -> bool { idn.as_obj().is_some_and(|kv| kv.iter().any(|(k, _)| KEYS.contains(&k.as_str()))) }
/// The strict reader of the self keys (`idn`: the whole `identity` block), against the persona built without them; `doc` is the
/// whole document (its `bound` block, when present, gives the allowances)
pub fn spec(idn: &Json, doc: &Json, p: &Persona) -> R<Ident> {
    let kv = idn.as_obj().ok_or_else(|| perr("identity", "must be a mapping"))?;
    let mut keep: Vec<String> = vec![];
    for (i, h) in list(get(kv, "keep"), "identity.keep")?.iter().enumerate() { let path = ix("identity.keep", i);
        let h = h.as_str().filter(|h| p.habits.iter().any(|x| x.id == *h)).ok_or_else(|| perr(&path, "a habit id of this persona"))?;
        need(!keep.iter().any(|x| x == h), &path, "listed twice")?; keep.push(h.to_string()); }
    let warn = num_or(kv, "warn", 0.8, "identity", Some(0.0), Some(1.0), false)?;
    need(warn > 0.0, "identity.warn", "a share of the allowance in (0, 1]")?;
    let cue = match get(kv, "cue") { None | Some(Json::Null) => None,
        Some(c) => Some(c.as_str().filter(|c| !c.trim().is_empty() && c.chars().count() <= 120 && !c.contains(['\n', '\r'])).ok_or_else(|| perr("identity.cue", "a short line of text (1-120 characters, one line)"))?.to_string()) };
    let mut traits: Vec<String> = vec![];
    for (i, t) in list(get(kv, "traits"), "identity.traits")?.iter().enumerate() { let path = ix("identity.traits", i);
        let t = t.as_str().filter(|t| p.var(t).is_some_and(|v| !v.mood)).ok_or_else(|| perr(&path, "a trait id of this persona"))?;
        need(!traits.iter().any(|x| x == t), &path, "listed twice")?; traits.push(t.to_string()); }
    if traits.is_empty() { traits = match &p.learning { Some(l) => l.traits.clone(), None => p.vars.iter().filter(|v| !v.mood).map(|v| v.id.clone()).collect() }; }
    let bound = bound::of_doc(doc, p)?;
    let rest = bound.as_ref().and_then(|b| b.rest_hours).unwrap_or_else(|| bound::default_rest(p));
    Ok(Ident { keep, warn, cue, traits, rest, bound })
}

/// One turn's integrity: the canonical `integrity` document, its summed drift and the dial's share
#[derive(Clone, Debug, PartialEq)]
pub struct Integrity { pub doc: Json, pub drift: f64, pub used: f64 }

/// The fresh individual's odds at the rest probe and the allowances (both fixed per seed), and the last probe twice keyed: by
/// state (the rested state's body without its `turn` and digest: a turn that changes nothing else leaves the resting odds as they
/// were) and by program (the probe's first program without its comment, `prog_key` as the members' cache keys them: a rested state
/// whose senses or drives moved but whose resting program did not gives the same odds). A program row is kept only when that
/// program's answer was exact and the stance is that answer's (no yield, hold or conflict): exact marginals read the program alone.
pub struct Cache { base: Option<Box<Base>>, key: [u8; 32], odds: Vec<Vec<f64>>, prog: Option<[u8; 32]>,
    /// probes answered from the cache (by state or by program), probes solved, and of the hits those by program
    pub hits: u64, pub misses: u64, pub by_program: u64 }
impl Cache { pub const fn new() -> Cache { Cache { base: None, key: [0; 32], odds: Vec::new(), prog: None, hits: 0, misses: 0, by_program: 0 } } }
impl Default for Cache { fn default() -> Cache { Cache::new() } }

/// Per seed: the seed, the fresh individual's odds at the rest probe, the allowances, the fresh individual
type Base = (u64, Vec<Vec<f64>>, Vec<Option<f64>>, State);
/// The rest probe's inputs: one event of `id.rest` idle hours
fn rest_in(id: &Ident) -> Json { Json::Obj(vec![("elapsed_hours".into(), Json::Num(id.rest))]) }
/// Per measured trait its odds in level order, read from a stance document
fn odds_of(p: &Persona, doc: &Json, id: &Ident) -> Vec<Vec<f64>> {
    id.traits.iter().map(|t| { let o = doc.get("stance").and_then(|s| s.get(t)).and_then(|x| x.get("odds"));
        p.var(t).map_or(vec![], |v| v.levels.iter().map(|lv| o.and_then(|o| o.get(lv)).and_then(Json::as_f64).unwrap_or(0.0)).collect()) }).collect()
}
/// The rest probe: one event of `id.rest` idle hours, no inputs -> per measured trait its odds in level order
fn probe(p: &Persona, st: &State, id: &Ident, eng: Engine) -> R<Vec<Vec<f64>>> {
    let (doc, _) = turn_boxed(p, st, &rest_in(id), false, eng, false)?; Ok(odds_of(p, &doc, id))
}
/// The rest probe through the program key: the turn's steps as `turn_boxed` runs them, its first program compiled here and keyed;
/// when the key is `known` -> (key, None), else the turn finishes on that program -> (key, Some((odds, whether a row may keep them)))
#[inline(never)]
fn probe_keyed(p: &Persona, st: &State, id: &Ident, eng: Engine, known: Option<&[u8; 32]>) -> R<([u8; 32], Option<(Vec<Vec<f64>>, bool)>)> {
    let pr = turn_prep(p, st, &rest_in(id), true)?;
    let first = Box::new(compile(p, &pr.st, &pr.raw, &Opts::default())?);
    let key = prog_key(&first.0);
    if known == Some(&key) { return Ok((key, None)); }
    let sv = turn_solve(p, &pr, false, eng, false, false, Some(first))?;
    let o = &sv.out; let row = o.verdict.as_str() == Some("exact") && o.held.is_empty() && o.yielded.is_empty() && o.conflict.is_empty();
    let (doc, _) = turn_doc(p, pr, sv); Ok((key, Some((odds_of(p, &doc, id), row))))
}
/// The cache key of a rested state: its body without `turn` and `digest` (a rested state has no open decision, whose credit
/// would age in turns), hashed by structure (`prog_key`)
#[inline(never)]
fn key_of(p: &Persona, st: &State) -> [u8; 32] {
    let mut b = st.body(p); if let Json::Obj(v) = &mut b { v.retain(|(k, _)| k != "turn" && k != "digest"); } prog_key(&b)
}
/// The individual rested: the channels of the hour at the fresh individual's values `st0` (moods with a half-life, the pace
/// average, the load, the rest pressure, the eligibility trace and the credit awaiting feedback with its open decisions, the
/// levels and stance taken last, the agenda's step); what persists as it is (learned deltas, history, moods without a
/// half-life, drives, senses, the surprise gates, genes)
pub fn rested(p: &Persona, st: &State, st0: &State) -> State { *rested_boxed(p, st, st0) }
/// `rested` on the heap (the probe's path: no frame holds a `State` by value)
#[inline(never)]
pub fn rested_boxed(p: &Persona, st: &State, st0: &State) -> Box<State> {
    let mut s = Box::new(st.clone());
    for (id, acc) in s.mood.iter_mut() { if p.var(id).is_some_and(|v| v.half_life.is_some()) { if let Some((_, a0)) = st0.mood.iter().find(|(k, _)| k == id) { acc.clone_from(a0); } } }
    s.rbar = st0.rbar; s.load_l = st0.load_l; s.pressure = st0.pressure; s.elig = st0.elig;
    s.prev.clone_from(&st0.prev); s.prev_stance.clone_from(&st0.prev_stance); s.agenda.clone_from(&st0.agenda);
    s.credit.clone_from(&st0.credit); s.open.clone_from(&st0.open);
    // RFC 0016: the curiosity drive's level moves with every event (a channel of the hour): at the fresh individual's (0)
    s.cur = st0.cur; s.seal(p); s
}

/// `integrity` of the state `st` (after a turn) whose stance document is `stance` (its `habits.bound`), from a fresh cache
pub fn compute(p: &Persona, st: &State, stance: &Json, eng: Engine) -> R<Option<Integrity>> { compute_cached(p, st, stance, &mut Cache::new(), eng) }
/// `compute` through a cache: one rest-probe solve when the probe's program (without its comment) and the state's body (without
/// `turn`) both changed since the last solve, else none; the fresh individual's probe and the allowances once per seed. None
/// without the self keys.
pub fn compute_cached(p: &Persona, st: &State, stance: &Json, cache: &mut Cache, eng: Engine) -> R<Option<Integrity>> {
    let Some(id) = p.ident.as_ref() else { return Ok(None) };
    if cache.base.as_ref().map_or(true, |b| b.0 != st.seed) { cache.base = Some(base(p, st.seed, id, eng)?); cache.key = [0; 32]; cache.prog = None; }
    resting(p, st, id, cache, eng)?;
    document(stance, id, cache).map(Some)
}
/// The fresh individual of `seed`: its rest probe and the allowances (on the heap, once per seed)
#[inline(never)]
fn base(p: &Persona, seed: u64, id: &Ident, eng: Engine) -> R<Box<Base>> {
    let st0 = init_boxed(p, Some(seed), true, eng); let p0 = probe(p, &st0, id, eng)?;
    let mut eps = vec![];
    for (i, t) in id.traits.iter().enumerate() {
        let k = p.learning.as_ref().and_then(|l| l.traits.iter().position(|x| x == t));
        eps.push(match (&id.bound, &p.learning, k) {
            (Some(b), Some(l), Some(k)) => { let sc = bound::scope(p, b, t, id.rest, Some(&*st0));
                match bound::bases(p, &sc, &st0) { Some(bs) => Some(bound::search(p, k, &sc.coords, l.total_cap, &bs, &p0[i], id.rest, eng)?.0), None => None } }
            _ => None }); }
    Ok(Box::new((seed, p0, eps, *st0)))
}
/// The resting odds of `st` into `cache.odds`: from the program key while the last solved probe left a row (an equal rested state
/// would compile the same program, so the state key would answer the same: it is not built), else from the state key, else one
/// rest probe; after a solve without a row the state key is that state's, as before
#[inline(never)]
fn resting(p: &Persona, st: &State, id: &Ident, cache: &mut Cache, eng: Engine) -> R<()> {
    let rs = rested_boxed(p, st, &cache.base.as_ref().unwrap().3);
    if cache.prog.is_none() { let key = key_of(p, &rs); if key == cache.key { cache.hits += 1; return Ok(()); } cache.key = key; }
    match probe_keyed(p, &rs, id, eng, cache.prog.as_ref())? {
        (_, None) => { cache.hits += 1; cache.by_program += 1; }
        (k, Some((odds, row))) => { cache.odds = odds; cache.misses += 1;
            if row { cache.prog = Some(k); } else if cache.prog.take().is_some() { cache.key = key_of(p, &rs); } } }
    Ok(())
}
/// The integrity document from the resting odds in `cache` and the stance's bound habits
#[inline(never)]
fn document(stance: &Json, id: &Ident, cache: &Cache) -> R<Integrity> {
    let b = cache.base.as_ref().unwrap(); let (p0, eps) = (&b.1, &b.2);
    let (mut sum, mut sum_e, mut sum_d) = (0.0f64, 0.0f64, 0.0f64); let mut rows = vec![]; let mut crossed = vec![];
    for (i, t) in id.traits.iter().enumerate() {
        let d = r6(bound::tv(&cache.odds[i], &p0[i])); sum += d;
        let u = match eps[i] { Some(e) if e > 0.0 => { sum_e += e; sum_d += d; r6(d / e) } _ => 0.0 };
        if u >= id.warn { crossed.push(t.clone()); }
        rows.push((t.clone(), Json::Obj(vec![("drift".into(), Json::Num(d)), ("allowance".into(), eps[i].map_or(Json::Null, Json::Num)), ("bound_used".into(), Json::Num(u))]))); }
    let used = if sum_e > 0.0 { r6(sum_d / sum_e) } else { 0.0 };
    let bound_now: Vec<String> = stance.get("habits").and_then(|h| h.get("bound")).and_then(strs).unwrap_or_default();
    let (held, not_fired): (Vec<String>, Vec<String>) = id.keep.iter().cloned().partition(|h| bound_now.contains(h));
    let cue = if crossed.is_empty() { Json::Null } else { id.cue.as_deref().map_or(Json::Null, |c| Json::Str(c.into())) };
    let drift = r6(sum);
    let doc = Json::Obj(vec![("drift".into(), Json::Num(drift)), ("allowance".into(), Json::Num(r6(sum_e))), ("used".into(), Json::Num(used)),
        ("traits".into(), Json::Obj(rows)), ("warn".into(), Json::Num(id.warn)), ("crossed".into(), jstrs(&crossed)),
        ("habits_held".into(), Json::Obj(vec![("held".into(), jstrs(&held)), ("not_fired".into(), jstrs(&not_fired))])), ("cue".into(), cue)]);
    // canonical key order (the order the strand line is written in), so the stance's copy equals the line's
    let doc = json::parse(&canon(&doc)).map_err(|e| perr("integrity", e.msg))?;
    Ok(Integrity { doc, drift, used })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(prog: &Json, f: &Flags) -> Json { run_program(prog, f, 1, 100, 0) }
    const REF: &str = r#"{"probbit_persona":1,"identity":{"name":"I","version":"1","seed":0SELF},
        "traits":[{"id":"verbosity","levels":["terse","short","full"],"spread":0.5},{"id":"humour","levels":["none","light","playful"],"spread":0.5}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.6,"half_life_hours":6}],"couplings":[{"vars":["valence","humour"],"align":0.8}],
        "inputs":[{"id":"praise","kind":"flag"},{"id":"criticism","kind":"flag"},{"id":"loss","kind":"flag","effects":{"valence":-1.0}}],
        "habits":[{"id":"quiet_after_loss","when":{"loss":true},"then":{"verbosity":["terse","short"]}}],
        "learning":{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":1},"bound":{"seeds":"0-1","rest_hours":200}}"#;
    fn persona(s: &str) -> Persona { build(&json::parse(&REF.replace("SELF", s)).unwrap()).unwrap() }
    fn refused(s: &str) -> String { match build(&json::parse(&REF.replace("SELF", s)).unwrap()) { Err(e) => format!("{}: {}", e.path, e.msg), Ok(_) => "ok".into() } }
    #[test]
    fn self_keys_stay_out_of_the_digest_and_are_read_strictly() {
        let (a, b) = (persona(""), persona(r#","keep":["quiet_after_loss"],"warn":0.5,"cue":"hold the line""#));
        assert_eq!(a.digest, b.digest); assert!(a.ident.is_none() && b.ident.is_some());
        assert_eq!(canon(&init(&a, Some(1), true, &run).to_json(&a)), canon(&init(&b, Some(1), true, &run).to_json(&b)));
        assert_eq!(refused(r#","keep":["nope"]"#), "identity.keep[0]: a habit id of this persona");
        assert_eq!(refused(r#","warn":0"#), "identity.warn: a share of the allowance in (0, 1]");
        assert_eq!(refused(r#","cue":"""#), "identity.cue: a short line of text (1-120 characters, one line)");
        assert_eq!(refused(r#","traits":["valence"]"#), "identity.traits[0]: a trait id of this persona");
        assert_eq!(refused(r#","traits":["humour","humour"]"#), "identity.traits[1]: listed twice");
    }
    #[test]
    fn fresh_is_zero_drift_and_a_campaign_stays_under_the_allowance() {
        let p = persona(r#","keep":["quiet_after_loss"],"warn":0.5,"cue":"hold the line""#);
        let mut c = Cache::new(); let st = init(&p, Some(1), true, &run);
        let (doc, _) = turn(&p, &st, &Json::Obj(vec![]), false, &run, false).unwrap();
        let i0 = compute_cached(&p, &st, &doc, &mut c, &run).unwrap().unwrap(); assert_eq!(i0.drift, 0.0, "{}", canon(&i0.doc));
        let mut s = st; let mut cued = false;
        for n in 0..60 { let f = if n % 5 == 0 { "criticism" } else { "praise" };
            let (d, ns) = turn(&p, &s, &Json::Obj(vec![(f.into(), Json::Bool(true)), ("elapsed_hours".into(), Json::Num(0.1))]), false, &run, false).unwrap(); s = ns;
            let i = compute_cached(&p, &s, &d, &mut c, &run).unwrap().unwrap();
            assert_eq!(canon(&i.doc), canon(&compute(&p, &s, &d, &run).unwrap().unwrap().doc), "the cache gives the uncached number");
            for t in ["verbosity", "humour"] { let u = i.doc.get("traits").and_then(|x| x.get(t)).and_then(|x| x.get("bound_used")).and_then(Json::as_f64).unwrap(); assert!(u <= 1.0 + 1e-5, "{t} {u}"); }
            if i.doc.get("cue") == Some(&Json::Str("hold the line".into())) { cued = true; } }
        assert!(cued, "the campaign crosses warn 0.5 somewhere");
        assert!(c.misses >= 2);
        // moods move every loss event; the rested state does not see them: a run of losses after the campaign hits the cache
        let before = (c.hits, c.misses);
        for _ in 0..5 { let (d, ns) = turn(&p, &s, &Json::Obj(vec![("loss".into(), Json::Bool(true)), ("elapsed_hours".into(), Json::Num(0.1))]), false, &run, false).unwrap(); s = ns;
            compute_cached(&p, &s, &d, &mut c, &run).unwrap().unwrap(); }
        assert_eq!((c.hits - before.0, c.misses - before.1), (5, 0), "five mood-only events: five hits");
    }
    /// RFC 0014-b: senses that move every event (the rested state's key never repeats): through the program key every number
    /// equals the uncached computation's
    #[cfg(feature = "std")]
    #[test]
    fn the_program_key_moves_no_number_when_the_senses_move() {
        let f = concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/bound/fixtures/sensed.yaml");
        let mut d = crate::yaml::load(&std::fs::read_to_string(f).unwrap()).unwrap_or_else(|e| panic!("{}", e.msg));
        if let Json::Obj(v) = &mut d { if let Some((_, Json::Obj(i))) = v.iter_mut().find(|(k, _)| k == "identity") { i.push(("warn".into(), Json::Num(0.8))); } }
        let p = build(&d).unwrap(); let mut c = Cache::new(); let mut s = init(&p, Some(3), true, &run);
        for n in 0..40u32 { let e = format!(r#"{{"senses":{{"tool_latency":{},"tempo":{}}},"src":"env","elapsed_hours":0.1{}}}"#, 0.5 + f64::from(n % 7) * 0.37, n % 5, if n % 9 == 4 { r#","praise":true"# } else { "" });
            let (doc, ns) = turn(&p, &s, &json::parse(&e).unwrap(), false, &run, false).unwrap(); s = ns;
            let i = compute_cached(&p, &s, &doc, &mut c, &run).unwrap().unwrap();
            assert_eq!(canon(&i.doc), canon(&compute(&p, &s, &doc, &run).unwrap().unwrap().doc), "event {n}: the cache gives the uncached number"); }
        eprintln!("R14B sensed: hits {} (by program {}) misses {}", c.hits, c.by_program, c.misses);
    }
    /// B3: every repo persona (examples/persona, bench/bound/fixtures) with the self keys declared: a fresh individual drifts 0
    #[cfg(feature = "std")]
    #[test]
    fn fresh_individual_has_zero_drift_on_every_repo_persona() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
        let mut n = 0;
        for dir in ["examples/persona", "bench/bound/fixtures"] {
            let mut fs: Vec<_> = std::fs::read_dir(format!("{root}/{dir}")).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "yaml")).collect(); fs.sort();
            for f in fs { let mut d = crate::yaml::load(&std::fs::read_to_string(&f).unwrap()).unwrap_or_else(|e| panic!("{}: {}", f.display(), e.msg)); if d.get("probbit_persona").is_none() { continue; }
                if let Json::Obj(v) = &mut d { if let Some((_, Json::Obj(i))) = v.iter_mut().find(|(k, _)| k == "identity") { i.push(("warn".into(), Json::Num(0.8))); } }
                let p = build(&d).unwrap_or_else(|e| panic!("{}: {}: {}", f.display(), e.path, e.msg));
                for seed in [0u64, 7] { let st = init(&p, Some(seed), true, &run);
                    let i = compute(&p, &st, &Json::Null, &run).unwrap().unwrap();
                    eprintln!("B3 fresh\t{dir}/{}\tseed {seed}\tdrift {}\tused {}\tallowance {}", f.file_name().unwrap().to_string_lossy(), i.drift, i.used, canon(i.doc.get("allowance").unwrap()));
                    assert_eq!(i.drift, 0.0, "{}", f.display()); n += 1; } } }
        assert!(n >= 20, "{n}");
    }
}
