//! `probbit persona fuzz` (docs/persona.md §5.6): search event scripts for the shortest one that makes an individual's stance
//! break a character property (a habit-shaped rule the stance must never break), shrink it, and print it with the breaking
//! turn's stance and odds and the command that replays it.
//!
//! The search: random scripts over the persona's declared inputs (flags, levels, numbers on a grid, idle hours), then a beam
//! guided by the exact odds (from each kept state, the next events whose turn puts the most odds on a level the rule forbids), then
//! delta debugging (drop events, then single inputs, while the rule still breaks). Its randomness is Philox (probbit-core) keyed by
//! `--fuzz-seed` and the individual's seed, and each individual is searched on its own, so the output is byte-identical for the
//! same inputs whatever the number of threads. It tests the stance a host gets, not the words a model writes.
use crate::json::Json;
use crate::persona::{self, Broken, Persona, Prop, State};
use probbit_core::Philox4x32;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// The engine, shared by the search's threads
pub type SyncEngine<'a> = &'a (dyn Fn(&Json, &persona::Flags) -> Json + Sync);
type Event = Vec<(String, Json)>;

/// The search settings (the flags of `probbit persona fuzz`)
pub struct Search { pub seeds: Vec<u64>, pub fuzz_seed: u64, pub scripts: usize, pub depth: usize, pub beam: usize, pub grid: Vec<f64>, pub hours: Vec<f64>, pub threads: usize }
/// One individual's shortest counterexample to one property, shrunk: the script, the breaking turn's stance document, what broke
pub struct Found { pub script: Vec<Event>, pub doc: Json, pub broken: Vec<Broken> }

fn step(p: &Persona, st: &State, e: &Event, eng: persona::Engine, turns: &AtomicUsize) -> (Json, State) {
    turns.fetch_add(1, Ordering::Relaxed);
    persona::turn(p, st, &Json::Obj(e.clone()), false, eng, false).expect("the fuzzer's inputs are valid")
}
/// The earliest turn of `script` (run from `st0`) whose stance breaks the property
fn breaking_turn(p: &Persona, pr: &Prop, st0: &State, script: &[Event], eng: persona::Engine, turns: &AtomicUsize) -> Option<(usize, Json, Vec<Broken>)> {
    let mut st = st0.clone();
    for (k, e) in script.iter().enumerate() { let (doc, ns) = step(p, &st, e, eng, turns);
        if let Some(b) = persona::breaks(p, pr, &st, &doc) { return Some((k, doc, b)); } st = ns; }
    None
}
/// A random event: each input takes one of its non-default values with a fixed chance (flag 1/4, level 1/2, number 2/5, idle
/// hours 3/10), else stays at its default (left out of the event)
fn random_event(r: &mut Philox4x32, alpha: &[(String, &'static str, Vec<Json>)]) -> Event {
    let mut e = vec![];
    for (id, kind, vals) in alpha { let chance = match *kind { "flag" => 0.25, "level" => 0.5, "number" => 0.4, _ => 0.3 };
        if r.f64() < chance { e.push((id.clone(), vals[r.below(vals.len())].clone())); } }
    e
}
/// The beam's moves: a quiet turn, every single input at each of its values, and every assignment that puts the property in
/// force, alone and with each single input that does not touch it
fn moves(p: &Persona, pr: &Prop, s: &Search) -> Vec<Event> {
    let atoms: Vec<Event> = persona::alphabet(p, &s.grid, &s.hours).into_iter().flat_map(|(id, _, vals)| vals.into_iter().map(move |v| vec![(id.clone(), v)])).collect();
    let mut out: Vec<Event> = vec![vec![]]; out.extend(atoms.iter().cloned());
    for b in persona::forcing(p, pr, &s.grid) { if !out.contains(&b) { out.push(b.clone()); }
        for a in &atoms { if b.iter().any(|(k, _)| *k == a[0].0) { continue; } let mut e = b.clone(); e.extend(a.iter().cloned()); if !out.contains(&e) { out.push(e); } } }
    out
}
/// Breadth by depth, `s.beam` states kept per depth (distinct states, the most pressure first): the earliest script whose last
/// turn breaks the property, at most `limit` events long
fn beam(p: &Persona, pr: &Prop, st0: &State, limit: usize, s: &Search, eng: persona::Engine, turns: &AtomicUsize) -> Option<Vec<Event>> {
    let mv = moves(p, pr, s); let mut kept: Vec<(Vec<Event>, State)> = vec![(vec![], st0.clone())];
    for _ in 0..limit {
        let mut next: Vec<(f64, Vec<Event>, State)> = vec![];
        for (script, st) in &kept { for e in &mv {
            let (doc, ns) = step(p, st, e, eng, turns); let mut sc = script.clone(); sc.push(e.clone());
            if persona::breaks(p, pr, st, &doc).is_some() { return Some(sc); }
            next.push((persona::pressure(p, pr, st, &doc), sc, ns)); } }
        next.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)); // stable: ties keep the move order
        kept.clear(); let mut seen: Vec<String> = vec![];
        for (_, sc, ns) in next {
            if kept.len() == s.beam { break; }
            if !seen.contains(&ns.digest) { seen.push(ns.digest.clone()); kept.push((sc, ns)); } }
    }
    None
}
/// Delta debugging: drop whole events, then single inputs, while some turn still breaks the property (cut after that turn)
fn shrink(p: &Persona, pr: &Prop, st0: &State, script: Vec<Event>, eng: persona::Engine, turns: &AtomicUsize) -> Found {
    let mut cur = script;
    loop {
        let mut changed = false;
        for i in 0..cur.len() { let mut c = cur.clone(); c.remove(i); if c.is_empty() { continue; }
            if let Some((k, _, _)) = breaking_turn(p, pr, st0, &c, eng, turns) { c.truncate(k + 1); cur = c; changed = true; break; } }
        if !changed { 'all: for i in 0..cur.len() { for j in 0..cur[i].len() { let mut c = cur.clone(); c[i].remove(j);
            if let Some((k, _, _)) = breaking_turn(p, pr, st0, &c, eng, turns) { c.truncate(k + 1); cur = c; changed = true; break 'all; } } } }
        if !changed { break; }
    }
    let (k, doc, broken) = breaking_turn(p, pr, st0, &cur, eng, turns).expect("a shrunk counterexample still breaks the property");
    cur.truncate(k + 1); Found { script: cur, doc, broken }
}
/// One individual: random scripts (shared by the properties), then per property the beam up to one event shorter than the
/// shortest found, then shrinking
fn individual(p: &Persona, props: &[Prop], seed: u64, s: &Search, eng: persona::Engine, turns: &AtomicUsize) -> Vec<Option<Found>> {
    let st0 = persona::init(p, Some(seed), true, eng); let alpha = persona::alphabet(p, &s.grid, &s.hours);
    let mut best: Vec<Option<Vec<Event>>> = vec![None; props.len()];
    let mut r = Philox4x32::new(s.fuzz_seed, seed);
    for _ in 0..s.scripts {
        let n = 1 + r.below(s.depth); let script: Vec<Event> = (0..n).map(|_| random_event(&mut r, &alpha)).collect();
        let mut st = st0.clone();
        for (k, e) in script.iter().enumerate() {
            if best.iter().all(|b| b.as_ref().is_some_and(|b| b.len() <= k + 1)) { break; }
            let (doc, ns) = step(p, &st, e, eng, turns);
            for (i, pr) in props.iter().enumerate() { if best[i].as_ref().map_or(true, |b| b.len() > k + 1) && persona::breaks(p, pr, &st, &doc).is_some() { best[i] = Some(script[..=k].to_vec()); } }
            st = ns; } }
    if s.beam > 0 { for (i, pr) in props.iter().enumerate() { let limit = best[i].as_ref().map_or(s.depth, |b| b.len() - 1);
        if let Some(sc) = beam(p, pr, &st0, limit, s, eng, turns) { best[i] = Some(sc); } } }
    props.iter().zip(best).map(|(pr, b)| b.map(|sc| shrink(p, pr, &st0, sc, eng, turns))).collect()
}
/// The search over every individual (threads take individuals in turn; each one's search is independent) -> per property, per
/// seed (in the order given) its counterexample, and the number of turns run
pub fn fuzz(p: &Persona, props: &[Prop], s: &Search, eng: SyncEngine) -> (Vec<Vec<Option<Found>>>, usize) {
    let (turns, next) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let slots: Vec<Mutex<Option<Vec<Option<Found>>>>> = s.seeds.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|sc| { for _ in 0..s.threads.clamp(1, s.seeds.len().max(1)) {
        std::thread::Builder::new().stack_size(8 << 20).spawn_scoped(sc, || loop {
            let i = next.fetch_add(1, Ordering::Relaxed); if i >= s.seeds.len() { break; }
            let r = individual(p, props, s.seeds[i], s, eng, &turns); *slots[i].lock().unwrap() = Some(r); }).expect("a search thread"); } });
    let mut per: Vec<Vec<Option<Found>>> = slots.into_iter().map(|m| m.into_inner().unwrap().unwrap_or_default()).collect();
    ((0..props.len()).map(|i| per.iter_mut().map(|v| v.get_mut(i).and_then(Option::take)).collect()).collect(), turns.into_inner())
}

// ------------------------------------------------------------------------------------------------------------- output
/// A rule as one-line YAML (habit syntax): `{when: {sentiment: negative}, then: {humour: {at_most: light}}}`
pub fn flow(j: &Json) -> String {
    match j {
        Json::Obj(kv) => format!("{{{}}}", kv.iter().map(|(k, v)| format!("{}: {}", plain(k), flow(v))).collect::<Vec<_>>().join(", ")),
        Json::Arr(a) => format!("[{}]", a.iter().map(flow).collect::<Vec<_>>().join(", ")),
        Json::Str(s) => plain(s), _ => persona::canon(j) }
}
fn plain(s: &str) -> String {
    let ok = s.bytes().next().is_some_and(|c| c.is_ascii_alphabetic() || c == b'_') && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"_+-.".contains(&c))
        && !["true", "false", "null", "yes", "no", "on", "off", "y", "n"].contains(&s.to_ascii_lowercase().as_str());
    if ok { s.to_string() } else { persona::canon(&Json::Str(s.to_string())) }
}
/// A shell word: as is when it needs no quoting, else single-quoted
fn word(s: &str) -> String { if !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"_./\\:-+=@,".contains(&c)) { s.to_string() } else { format!("'{}'", s.replace('\'', "'\\''")) } }
/// Seeds as ranges: 0-99, 120, 130-139
pub fn ranges(seeds: &[u64]) -> String {
    let mut out: Vec<String> = vec![]; let mut i = 0;
    while i < seeds.len() { let mut j = i; while j + 1 < seeds.len() && seeds[j + 1] == seeds[j] + 1 { j += 1; }
        out.push(if j > i { format!("{}-{}", seeds[i], seeds[j]) } else { seeds[i].to_string() }); i = j + 1; }
    out.join(",")
}
fn script_json(sc: &[Event]) -> Json { Json::Arr(sc.iter().map(|e| Json::Obj(e.clone())).collect()) }
fn commands(path: &str, seed: u64, f: &Found) -> (String, String) {
    let sc = word(&persona::canon(&script_json(&f.script)));
    (format!("probbit persona replay {} --seed {seed} --script {sc}", word(path)), format!("probbit persona explain {} --seed {seed} --script {sc} --turn {}", word(path), f.script.len() - 1))
}
fn lengths(per: &[Option<Found>]) -> Vec<(usize, usize)> {
    let mut by: Vec<(usize, usize)> = vec![];
    for f in per.iter().flatten() { match by.iter_mut().find(|x| x.0 == f.script.len()) { Some(x) => x.1 += 1, None => by.push((f.script.len(), 1)) } }
    by.sort_unstable(); by
}
/// The shortest counterexample: the fewest events, then the earliest seed in the order given
fn shortest<'a>(s: &Search, per: &'a [Option<Found>]) -> Option<(u64, &'a Found)> {
    s.seeds.iter().zip(per).filter_map(|(sd, f)| f.as_ref().map(|f| (*sd, f))).fold(None, |b: Option<(u64, &Found)>, x| match b { Some(b) if b.1.script.len() <= x.1.script.len() => Some(b), _ => Some(x) })
}
fn broken_json(b: &[Broken]) -> Json {
    Json::Arr(b.iter().map(|b| Json::Obj(vec![("var".into(), Json::Str(b.var.clone())), ("level".into(), Json::Str(b.level.clone())), ("allowed".into(), Json::Arr(b.allowed.iter().map(|l| Json::Str(l.clone())).collect()))])).collect())
}
/// The `probbit_persona_fuzz` document (canonical JSON when printed)
pub fn doc(p: &Persona, path: &str, props: &[Prop], s: &Search, res: &[Vec<Option<Found>>], turns: usize) -> Json {
    let n = |x: f64| Json::Num(x); let st = |x: &str| Json::Str(x.to_string());
    let props_j: Vec<Json> = props.iter().zip(res).map(|(pr, per)| {
        let failing: Vec<Json> = s.seeds.iter().zip(per).filter(|(_, f)| f.is_some()).map(|(sd, _)| n(*sd as f64)).collect();
        let sh = shortest(s, per).map_or(Json::Null, |(sd, f)| { let (rp, ex) = commands(path, sd, f);
            Json::Obj(vec![("seed".into(), n(sd as f64)), ("script".into(), script_json(&f.script)), ("turn".into(), n((f.script.len() - 1) as f64)), ("broken".into(), broken_json(&f.broken)),
                ("stance".into(), f.doc.clone()), ("replay".into(), st(&rp)), ("explain".into(), st(&ex))]) });
        let cex: Vec<Json> = s.seeds.iter().zip(per).filter_map(|(sd, f)| f.as_ref().map(|f| Json::Obj(vec![("seed".into(), n(*sd as f64)), ("script".into(), script_json(&f.script))]))).collect();
        Json::Obj(vec![("id".into(), st(&pr.id)), ("rule".into(), pr.rule.clone()), ("verdict".into(), st(if failing.is_empty() { "none_found" } else { "counterexample" })),
            ("individuals".into(), n(s.seeds.len() as f64)), ("failing".into(), n(failing.len() as f64)), ("failing_seeds".into(), Json::Arr(failing)),
            ("by_length".into(), Json::Obj(lengths(per).into_iter().map(|(l, c)| (l.to_string(), n(c as f64))).collect())), ("shortest".into(), sh), ("counterexamples".into(), Json::Arr(cex))]) }).collect();
    Json::Obj(vec![("probbit_persona_fuzz".into(), n(1.0)), ("persona".into(), Json::Obj(vec![("name".into(), st(&p.name)), ("version".into(), st(&p.version)), ("digest".into(), st(&p.digest))])),
        ("seeds".into(), Json::Arr(s.seeds.iter().map(|x| n(*x as f64)).collect())), ("fuzz_seed".into(), n(s.fuzz_seed as f64)),
        ("search".into(), Json::Obj(vec![("scripts".into(), n(s.scripts as f64)), ("depth".into(), n(s.depth as f64)), ("beam".into(), n(s.beam as f64)),
            ("grid".into(), Json::Arr(s.grid.iter().map(|x| n(*x)).collect())), ("hours".into(), Json::Arr(s.hours.iter().map(|x| n(*x)).collect()))])),
        ("turns".into(), n(turns as f64)), ("found".into(), Json::Bool(res.iter().any(|per| per.iter().any(Option::is_some)))), ("properties".into(), Json::Arr(props_j))])
}
fn odds_text(doc: &Json, var: &str) -> String {
    let e = doc.get("stance").and_then(|s| s.get(var)).or_else(|| doc.get("mood").and_then(|m| m.get(var)));
    e.and_then(|e| e.get("odds")).and_then(Json::as_obj).map_or(String::new(), |o| o.iter().map(|(l, x)| format!("{l} {:.3}", x.as_f64().unwrap_or(0.0))).collect::<Vec<_>>().join(", "))
}
/// The human report (stdout; deterministic: the timing goes to stderr)
pub fn human(p: &Persona, path: &str, props: &[Prop], s: &Search, res: &[Vec<Option<Found>>], turns: usize) -> String {
    let short = &p.digest[..p.digest.len().min(19)];
    let mut o = vec![format!("persona fuzz  {} {} ({short}), {} individuals (seeds {}), fuzz seed {}", p.name, p.version, s.seeds.len(), ranges(&s.seeds), s.fuzz_seed)];
    o.push(format!("search        {} random scripts of 1-{} events per individual{}; counterexamples shrunk by delta debugging", s.scripts, s.depth,
        if s.beam > 0 { format!(", then an odds-guided beam (width {})", s.beam) } else { String::new() }));
    let alpha = persona::alphabet(p, &s.grid, &s.hours);
    o.push(format!("inputs        {}", alpha.iter().map(|(id, _, v)| format!("{id} {}", v.iter().map(flow).collect::<Vec<_>>().join("/"))).collect::<Vec<_>>().join(", ")));
    let mut broke = 0;
    for (pr, per) in props.iter().zip(res) {
        o.push(String::new()); o.push(format!("{}  {}", pr.id, flow(&pr.rule)));
        match shortest(s, per) {
            None => o.push(format!("  none found  0 of {} individuals broke it (evidence, not a proof)", s.seeds.len())),
            Some((sd, f)) => { broke += 1; let failing = per.iter().filter(|f| f.is_some()).count();
                let by = lengths(per).iter().map(|(l, c)| format!("{l} event{}: {c}", if *l == 1 { "" } else { "s" })).collect::<Vec<_>>().join(", ");
                o.push(format!("  FAIL  {failing} of {} individuals break it; shortest counterexample per individual: {by}", s.seeds.len()));
                o.push(format!("  seed {sd}, {} event{}: {}", f.script.len(), if f.script.len() == 1 { "" } else { "s" }, persona::canon(&script_json(&f.script))));
                for b in &f.broken {
                    if b.var == "rules" { o.push(format!("  turn {}: the stance breaks the rule's raw rules", f.script.len() - 1)); }
                    else { o.push(format!("  turn {}: {} {} (odds {}); the rule allows {}", f.script.len() - 1, b.var, b.level, odds_text(&f.doc, &b.var), b.allowed.join(", "))); } }
                let g = |k: &str| f.doc.get(k).and_then(Json::as_str).unwrap_or("").to_string();
                o.push(format!("  why:     {}", g("why"))); o.push(format!("  line:    {}", g("line")));
                let (rp, ex) = commands(path, sd, f); o.push(format!("  replay:  {rp}")); o.push(format!("  explain: {ex}"));
                let seeds: Vec<u64> = s.seeds.iter().zip(per).filter(|(_, f)| f.is_some()).map(|(x, _)| *x).collect();
                o.push(format!("  failing seeds: {}", ranges(&seeds))); }
        }
    }
    o.push(String::new());
    o.push(format!("{} propert{}, {broke} broken; {turns} turns searched", props.len(), if props.len() == 1 { "y" } else { "ies" }));
    o.join("\n")
}
