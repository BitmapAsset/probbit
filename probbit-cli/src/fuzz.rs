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
    persona::turn_any_source(p, st, &persona::event_json(e), false, eng, false).expect("the fuzzer's inputs are valid")
}
/// The earliest turn of `script` (run from `st0`) whose stance breaks the property
fn breaking_turn(p: &Persona, pr: &Prop, st0: &State, script: &[Event], eng: persona::Engine, turns: &AtomicUsize) -> Option<(usize, Json, Vec<Broken>)> {
    let mut st = st0.clone();
    for (k, e) in script.iter().enumerate() { let (doc, ns) = step(p, &st, e, eng, turns);
        if let Some(b) = persona::breaks(p, pr, &st, &doc) { return Some((k, doc, b)); } st = ns; }
    None
}
/// A random event: each input takes one of its non-default values with a fixed chance (flag 1/4, level 1/2, number 2/5, idle
/// hours 3/10, a goal signal 3/20), else stays at its default (left out of the event)
fn random_event(r: &mut Philox4x32, alpha: &[(String, &'static str, Vec<Json>)]) -> Event {
    let mut e = vec![];
    for (id, kind, vals) in alpha { let chance = match *kind { "flag" => 0.25, "level" => 0.5, "number" => 0.4, "goal" => 0.15, _ => 0.3 };
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
    let turns = AtomicUsize::new(0);
    let mut per = par(s.seeds.len(), s.threads, |i| individual(p, props, s.seeds[i], s, eng, &turns));
    ((0..props.len()).map(|i| per.iter_mut().map(|v| v.get_mut(i).and_then(Option::take)).collect()).collect(), turns.into_inner())
}
/// `f(0) .. f(n-1)` on up to `threads` threads (each takes the next index; 8 MiB stacks, as the engine's threads), in index order
fn par<T: Send>(n: usize, threads: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0); let slots: Vec<Mutex<Option<T>>> = (0..n).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|sc| { for _ in 0..threads.clamp(1, n.max(1)) {
        std::thread::Builder::new().stack_size(8 << 20).spawn_scoped(sc, || loop {
            let i = next.fetch_add(1, Ordering::Relaxed); if i >= n { break; } let r = f(i); *slots[i].lock().unwrap() = Some(r); }).expect("a search thread"); } });
    slots.into_iter().map(|m| m.into_inner().unwrap().expect("every index ran")).collect()
}

// ------------------------------------------------------------------------------------------------------------- prove
/// `probbit persona prove`'s answer for one property over the population
pub enum Verdict { Held(Vec<String>), Proved { cells: usize, calls: usize }, Unknown { proved: usize, failed: Vec<(u64, Json, String)> } }
/// Per property: held by construction (a habit implies it), else the bound per individual (persona::prove_seed): proved when every
/// individual is, else unknown with every individual the bound cannot decide (in seed order)
pub fn prove(p: &Persona, props: &[Prop], seeds: &[u64], threads: usize, eng: SyncEngine) -> Vec<Verdict> {
    props.iter().map(|pr| { if let Some(h) = persona::by_construction(p, pr, eng) { return Verdict::Held(h); }
        let res = par(seeds.len(), threads, |i| persona::prove_seed(p, pr, seeds[i], eng));
        let (mut cells, mut calls, mut failed) = (0, 0, vec![]);
        for (sd, r) in seeds.iter().zip(res) { match r { Ok((c, k)) => { cells += c; calls += k; } Err((cell, why)) => failed.push((*sd, cell, why)) } }
        if failed.is_empty() { Verdict::Proved { cells, calls } } else { Verdict::Unknown { proved: seeds.len() - failed.len(), failed } } }).collect()
}
/// The `probbit_persona_prove` document
pub fn prove_doc(p: &Persona, props: &[Prop], seeds: &[u64], out: &[Verdict]) -> Json {
    let n = |x: f64| Json::Num(x); let st = |x: &str| Json::Str(x.to_string());
    let props_j: Vec<Json> = props.iter().zip(out).map(|(pr, v)| { let mut f = vec![("id".to_string(), st(&pr.id)), ("rule".into(), pr.rule.clone()), ("individuals".into(), n(seeds.len() as f64))];
        match v {
            Verdict::Held(h) => { f.push(("verdict".into(), st("held_by_construction"))); f.push(("habits".into(), Json::Arr(h.iter().map(|x| st(x)).collect()))); }
            Verdict::Proved { cells, calls } => { f.push(("verdict".into(), st("proved"))); f.push(("cells".into(), n(*cells as f64))); f.push(("engine_calls".into(), n(*calls as f64))); }
            Verdict::Unknown { proved, failed } => { f.push(("verdict".into(), st("unknown"))); f.push(("proved_individuals".into(), n(*proved as f64)));
                f.push(("unknown".into(), Json::Arr(failed.iter().map(|(sd, cell, why)| Json::Obj(vec![("seed".into(), n(*sd as f64)), ("cell".into(), cell.clone()), ("reason".into(), st(why))])).collect()))); } }
        Json::Obj(f) }).collect();
    let mut v = vec![("probbit_persona_prove".into(), n(1.0)), ("persona".into(), Json::Obj(vec![("name".into(), st(&p.name)), ("version".into(), st(&p.version)), ("digest".into(), st(&p.digest))])),
        ("seeds".into(), Json::Arr(seeds.iter().map(|x| n(*x as f64)).collect())), ("properties".into(), Json::Arr(props_j))];
    if let Some(f) = persona::floor_verdicts(p, seeds) { v.push(("floors".into(), Json::Arr(f))); } // drives (§2.9): goal floors
    Json::Obj(v)
}
/// The human report of `prove`
pub fn prove_human(p: &Persona, path: &str, props: &[Prop], seeds: &[u64], out: &[Verdict]) -> String {
    let mut o = vec![format!("persona prove  {} {} ({}), {} individuals (seeds {})", p.name, p.version, &p.digest[..p.digest.len().min(19)], seeds.len(), ranges(seeds))];
    for (pr, v) in props.iter().zip(out) {
        o.push(String::new()); o.push(format!("{}  {}", pr.id, flow(&pr.rule)));
        match v {
            Verdict::Held(h) => o.push(format!("  held by construction  (habit{} {})", if h.len() == 1 { "" } else { "s" }, h.join(", "))),
            Verdict::Proved { cells, calls } => o.push(format!("  proved for every event sequence  ({} individuals; {cells} cells, {calls} engine calls)", seeds.len())),
            Verdict::Unknown { proved, failed } => { let (sd, cell, why) = &failed[0];
                o.push(format!("  unknown  the bound decides {proved} of {} individuals; not seed{} {}", seeds.len(), if failed.len() == 1 { "" } else { "s" }, ranges(&failed.iter().map(|f| f.0).collect::<Vec<_>>())));
                o.push(format!("  seed {sd}: {why}; cell {}", persona::canon(cell)));
                o.push(format!("  search it: probbit persona fuzz {} --seeds {} --never {}", word(path), ranges(seeds), word(&flow(&pr.rule)))); } }
    }
    for f in persona::floor_verdicts(p, seeds).unwrap_or_default() { let g = |k: &str| f.get(k).cloned().unwrap_or(Json::Null);
        let by: Vec<String> = g("excluded_by").as_arr().unwrap_or(&[]).iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
        o.push(String::new()); o.push(format!("floor  {} odds >= {}", g("goal").as_str().unwrap_or(""), persona::canon(&g("floor"))));
        if g("verdict").as_str() == Some("unknown") { o.push(format!("  unknown  {}", g("reason").as_str().unwrap_or("numeric floor guarantee not certified"))); }
        else { o.push(format!("  held by construction  (the odds lift; on turns whose habits allow it{})", if by.is_empty() { String::new() } else { format!("; can be excluded by {}", by.join(", ")) })); } }
    o.join("\n")
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
fn script_json(sc: &[Event]) -> Json { Json::Arr(sc.iter().map(|e| persona::event_json(e)).collect()) }
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
fn odds_text(p: &Persona, doc: &Json, var: &str) -> String {
    persona::doc_entry(p, doc, var).and_then(|e| e.get("odds").and_then(Json::as_obj).map(|o| o.iter().map(|(l, x)| format!("{l} {:.3}", x.as_f64().unwrap_or(0.0))).collect::<Vec<_>>().join(", "))).unwrap_or_default()
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
                    else { o.push(format!("  turn {}: {} {} (odds {}); the rule allows {}", f.script.len() - 1, b.var, b.level, odds_text(p, &f.doc, &b.var), b.allowed.join(", "))); } }
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

// --------------------------------------------------------------------------------------------------------------- MCP
/// `0-99` / `3` / `1,4,9` / `0-9,20-29`: the individuals, in the order given, each once; None when unreadable (seeds 0 to 2^53,
/// at most 100,000)
pub fn seeds(v: &str) -> Option<Vec<u64>> {
    let mut out: Vec<u64> = vec![];
    for part in v.split(',') { let (a, b) = part.split_once('-').unwrap_or((part, part));
        let (a, b): (u64, u64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        if a > b || b > 1 << 53 || b - a >= 100_000 { return None; }
        for x in a..=b { if !out.contains(&x) { out.push(x); }
            if out.len() > 100_000 { return None; } } }
    Some(out)
}
/// The MCP tool `probbit_persona_fuzz` (stateless): {persona | persona_path, never | props, seeds, fuzz_seed, scripts, depth, beam,
/// grid, hours, threads} -> the `probbit_persona_fuzz` document, `probbit persona fuzz --json`'s for the same arguments (an inline
/// persona is named PERSONA in the replay commands)
pub fn tool(args: &[(String, Json)], eng: SyncEngine) -> Result<Json, crate::json::InErr> {
    use persona::perr;
    let get = |k: &str| args.iter().find(|(x, _)| x == k).map(|(_, v)| v).filter(|v| !v.is_null());
    const KNOWN: [&str; 12] = ["persona", "persona_path", "never", "props", "seeds", "fuzz_seed", "scripts", "depth", "beam", "grid", "hours", "threads"];
    let mut extra: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).filter(|k| !KNOWN.contains(k)).collect(); extra.sort_unstable();
    if let Some(k) = extra.first() { return Err(perr(&format!("arguments.{k}"), "unknown argument")); }
    let (p, path) = match (get("persona"), get("persona_path")) {
        (Some(d @ Json::Obj(_)), None) => (persona::build(d)?, "PERSONA".to_string()),
        (Some(_), None) => return Err(perr("arguments.persona", "must be an object (the JSON form of a persona file)")),
        (None, Some(Json::Str(f))) => (persona::load(f)?.0, f.clone()),
        (None, Some(_)) => return Err(perr("arguments.persona_path", "a file path")),
        _ => return Err(perr("arguments", "give exactly one of persona (a document) or persona_path")) };
    let props = match (get("never"), get("props")) {
        // a rule: an object, or one line of the YAML subset (as `--never`)
        (Some(Json::Str(t)), None) => { let d = persona::parse_doc(&format!("never: {t}"), false).map_err(|e| perr("arguments.never", e))?;
            persona::props(&p, d.get("never").unwrap_or(&Json::Null), "arguments.never", "never")? }
        (Some(r), None) => persona::props(&p, r, "arguments.never", "never")?,
        (None, Some(d)) => persona::props(&p, d, "arguments.props", "prop1")?,
        _ => return Err(perr("arguments", "give exactly one of never (a rule) or props (a list of rules)")) };
    let whole = |k: &str, d: usize, lo: usize, hi: usize| -> Result<usize, crate::json::InErr> { match get(k) { None => Ok(d),
        Some(Json::Num(x)) if x.fract() == 0.0 && *x >= lo as f64 && *x <= hi as f64 => Ok(*x as usize), _ => Err(perr(&format!("arguments.{k}"), format!("an integer from {lo} to {hi}"))) } };
    let nums = |k: &str, d: &[f64], hi: f64| -> Result<Vec<f64>, crate::json::InErr> { match get(k) { None => Ok(d.to_vec()),
        Some(Json::Arr(a)) if !a.is_empty() => a.iter().map(|x| x.as_f64().filter(|x| *x >= 0.0 && *x <= hi)).collect::<Option<Vec<f64>>>().ok_or_else(|| perr(&format!("arguments.{k}"), format!("numbers from 0 to {hi}"))),
        _ => Err(perr(&format!("arguments.{k}"), format!("a list of numbers from 0 to {hi}"))) } };
    let sd = match get("seeds") { None => Some((0..100).collect()), Some(Json::Str(v)) => seeds(v),
        Some(Json::Arr(a)) => a.iter().map(|x| x.as_f64().filter(|x| x.fract() == 0.0 && *x >= 0.0 && *x <= 9_007_199_254_740_992.0).map(|x| x as u64)).collect::<Option<Vec<u64>>>().filter(|v| !v.is_empty() && v.len() <= 100_000), _ => None }
        .ok_or_else(|| perr("arguments.seeds", "e.g. \"0-99\", \"1,4,9\" or a list of integers (0 to 2^53, at most 100000)"))?;
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let s = Search { seeds: sd, fuzz_seed: whole("fuzz_seed", 0, 0, 1 << 53)? as u64, scripts: whole("scripts", 60, 0, 1_000_000)?, depth: whole("depth", 8, 1, 64)?, beam: whole("beam", 4, 0, 64)?,
        grid: nums("grid", &[0.0, 0.5, 1.0], 1.0)?, hours: nums("hours", &[1.0, 12.0, 48.0], 1e6)?, threads: whole("threads", cores, 1, 1024)? };
    let (res, turns) = fuzz(&p, &props, &s, eng);
    Ok(doc(&p, &path, &props, &s, &res, turns))
}

#[cfg(test)]
mod tests {
    //! `prove` never says proved (or held) where a stance breaks the property: against every event sequence of tiny random
    //! personas, against the fuzzer on random personas, and with habits as properties
    use super::*;
    use crate::json;

    fn run(prog: &Json, f: &persona::Flags) -> Json { persona::run_program(prog, f, 1, 100, 0) }
    /// A habit of a generated persona: id, `when`, `then` (JSON text) and priority
    struct H { id: String, when: String, then: String, priority: usize }
    /// A random persona: 2-3 traits (2-3 levels), a mood (inertia 0.3-0.8) coupled to each, flags f0 and f1, a level input lv (neg,
    /// neu, pos); with `big` also a number n0 and a streak of f0. 0-2 conditional habits (level lists, not, at_most / at_least with
    /// levels or prev); conflicts by fallback or yield. With `learn`, a learning block on every trait (f0 rewards, f1 corrects; rate
    /// 1-3, total cap 0.05-2, step cap 50-100% of it). -> (persona, trait ids with their level counts, habits)
    fn persona_at(r: &mut Philox4x32, big: bool, learn: bool) -> (Persona, Vec<(String, usize)>, Vec<H>) {
        let w = |r: &mut Philox4x32| ((r.f64() * 4.0 - 2.0) * 100.0).round() / 100.0;
        let lv = |n: usize| (0..n).map(|i| format!("\"l{i}\"")).collect::<Vec<_>>().join(",");
        let (mut traits, mut tl) = (vec![], vec![]);
        for t in ["a", "b", "c"].iter().take(2 + r.below(2)) { let n = 2 + r.below(2); tl.push((t.to_string(), n));
            traits.push(format!(r#"{{"id":"{t}","levels":[{}],"logw":[{}],"spread":0.4}}"#, lv(n), (0..n).map(|_| w(r).to_string()).collect::<Vec<_>>().join(","))); }
        let couplings: Vec<String> = tl.iter().map(|(t, _)| format!(r#"{{"vars":["m","{t}"],"align":{}}}"#, w(r))).collect();
        let eff = |r: &mut Philox4x32| { let mut e = vec![format!(r#""m":{}"#, w(r) * 1.5)]; for (t, _) in &tl { if r.below(2) == 0 { e.push(format!(r#""{t}":{}"#, w(r))); } } format!("{{{}}}", e.join(",")) };
        let mut inputs = vec![format!(r#"{{"id":"f0","kind":"flag","effects":{}}}"#, eff(r)), format!(r#"{{"id":"f1","kind":"flag","effects":{}}}"#, eff(r)),
            format!(r#"{{"id":"lv","kind":"level","levels":["neg","neu","pos"],"default":"neu","effects":{{"neg":{},"pos":{}}}}}"#, eff(r), eff(r))];
        let mut whens = vec![r#"{"f0":true}"#, r#"{"f1":true}"#, r#"{"lv":"neg"}"#, r#"{"lv":["neg","pos"]}"#];
        let mut history = String::new();
        if big { inputs.push(format!(r#"{{"id":"n0","kind":"number","effects":{}}}"#, eff(r)));
            history = format!(r#","history":[{{"id":"streak","of":"f0","kind":"streak","cap":3,"effects":{}}}]"#, eff(r)); whens.extend([r#"{"n0":0.5}"#, r#"{"streak":2}"#]); }
        let mut habits = vec![];
        for i in 0..r.below(3) { let (t, n) = tl[r.below(tl.len())].clone();
            let then = match r.below(4) { 0 => format!(r#"{{"{t}":["l{}"]}}"#, r.below(n)), 1 => format!(r#"{{"{t}":{{"not":["l{}"]}}}}"#, r.below(n)),
                2 => format!(r#"{{"{t}":{{"at_most":"{}"}}}}"#, ["prev".to_string(), format!("l{}", r.below(n))][r.below(2)]), _ => format!(r#"{{"{t}":{{"at_least":"{}"}}}}"#, ["prev".to_string(), format!("l{}", r.below(n))][r.below(2)]) };
            habits.push(H { id: format!("h{i}"), when: whens[r.below(whens.len())].to_string(), then, priority: r.below(2) }); }
        let hj: Vec<String> = habits.iter().map(|h| format!(r#"{{"id":"{}","when":{},"then":{},"priority":{}}}"#, h.id, h.when, h.then, h.priority)).collect();
        let mut doc = format!(r#"{{"probbit_persona":1,"identity":{{"name":"T","version":"1","seed":1}},"traits":[{}],"moods":[{{"id":"m","levels":["down","even","up"],"inertia":{}}}],"couplings":[{}],"inputs":[{}]{history},"habits":[{}],"engine":{{"on_conflict":"{}"}}}}"#,
            traits.join(","), (30 + r.below(51)) as f64 / 100.0, couplings.join(","), inputs.join(","), hj.join(","), ["fallback", "yield"][r.below(2)]);
        if learn { let tc = [0.05, 0.1, 0.25, 0.5, 1.0, 2.0][r.below(6)]; let names: Vec<String> = tl.iter().map(|(t, _)| format!("\"{t}\"")).collect(); doc.pop();
            doc.push_str(&format!(r#","learning":{{"from":["f0","f1"],"traits":[{}],"rate":{},"step_cap":{},"total_cap":{tc}}}}}"#, names.join(","), (100 + r.below(201)) as f64 / 100.0, ((50 + r.below(51)) as f64 / 100.0 * tc * 100.0).round() / 100.0)); }
        let p = persona::build(&json::parse(&doc).unwrap()).unwrap_or_else(|e| panic!("{}: {} in {doc}", e.path, e.msg));
        (p, tl, habits)
    }
    /// Three random properties (a condition on f0, f1 or lv, or none; a restriction of one trait or the mood) and every habit's own
    /// rule as a property
    fn props_of(r: &mut Philox4x32, p: &Persona, tl: &[(String, usize)], habits: &[H]) -> Vec<Prop> {
        let mut out: Vec<String> = (0..3).map(|i| { let when = [r#"{"f0":true}"#, r#"{"f1":true}"#, r#"{"lv":"neg"}"#, r#"{"lv":"pos"}"#, "{}"][r.below(5)];
            let (t, n) = if r.below(4) == 0 { ("m".to_string(), 3) } else { tl[r.below(tl.len())].clone() }; let lvl = |k: usize| if t == "m" { ["down", "even", "up"][k].to_string() } else { format!("l{k}") };
            let restr = match r.below(3) { 0 => format!(r#"{{"at_most":"{}"}}"#, lvl(r.below(n - 1))), 1 => format!(r#"{{"at_least":"{}"}}"#, lvl(1 + r.below(n - 1))), _ => format!(r#"{{"not":["{}"]}}"#, lvl(r.below(n))) };
            format!(r#"{{"id":"p{i}","when":{when},"then":{{"{t}":{restr}}}}}"#) }).collect();
        out.extend(habits.iter().map(|h| format!(r#"{{"id":"{}","when":{},"then":{}}}"#, h.id, h.when, h.then)));
        persona::props(p, &json::parse(&format!(r#"{{"props":[{}]}}"#, out.join(","))).unwrap(), "props", "prop1").unwrap_or_else(|e| panic!("{}: {}", e.path, e.msg))
    }
    /// Every event sequence over `alpha` up to `depth` events from `st`: which properties some turn breaks
    fn every_sequence(p: &Persona, prs: &[Prop], st: &State, alpha: &[Json], depth: usize, broken: &mut [bool]) {
        if depth == 0 { return; }
        for e in alpha { let (doc, ns) = persona::turn(p, st, e, false, &run, false).unwrap();
            for (i, pr) in prs.iter().enumerate() { if persona::breaks(p, pr, st, &doc).is_some() { broken[i] = true; } }
            every_sequence(p, prs, &ns, alpha, depth - 1, broken); }
    }

    /// (i) Brute force: 16 tiny personas x 2 individuals x their properties, every sequence of up to 3 events over all 12 input
    /// combinations and up to 5 over 4 of them; a property prove calls held or proved never breaks
    #[test]
    fn prove_agrees_with_every_event_sequence_of_tiny_personas() {
        let ev = |s: &str| json::parse(s).unwrap();
        let all: Vec<Json> = [false, true].iter().flat_map(|a| [false, true].iter().flat_map(move |b| ["neg", "neu", "pos"].iter().map(move |l| ev(&format!(r#"{{"f0":{a},"f1":{b},"lv":"{l}"}}"#))))).collect();
        let few: Vec<Json> = [r#"{}"#, r#"{"f0":true}"#, r#"{"lv":"neg"}"#, r#"{"f1":true,"lv":"pos"}"#].iter().map(|s| ev(s)).collect();
        let res = par(16, std::thread::available_parallelism().map_or(1, |n| n.get()), |k| {
            let mut r = Philox4x32::new(38, k as u64); let (p, tl, habits) = persona_at(&mut r, false, false); let prs = props_of(&mut r, &p, &tl, &habits);
            let mut out = vec![];
            for seed in [0u64, 7] {
                let st0 = persona::init(&p, Some(seed), true, &run); let mut broken = vec![false; prs.len()];
                every_sequence(&p, &prs, &st0, &all, 3, &mut broken); every_sequence(&p, &prs, &st0, &few, 5, &mut broken);
                for (i, pr) in prs.iter().enumerate() { let held = persona::by_construction(&p, pr, &run).is_some(); let proved = persona::prove_seed(&p, pr, seed, &run).is_ok();
                    assert!(!(broken[i] && (held || proved)), "persona {k} seed {seed}: {} {} is {} but a sequence breaks it", pr.id, persona::canon(&pr.rule), if held { "held" } else { "proved" });
                    out.push((held || proved, broken[i])); } }
            out });
        let all: Vec<(bool, bool)> = res.into_iter().flatten().collect();
        let (yes, broke) = (all.iter().filter(|x| x.0).count(), all.iter().filter(|x| x.1).count());
        assert!(yes >= 5 && broke >= 5, "the check must see both sides: {yes} held or proved, {broke} broken of {}", all.len());
    }

    /// (ii) Property-based: 16 random personas (numbers, a streak, habits) x 3 individuals x their properties: whenever the fuzzer
    /// finds a counterexample, prove did not say held or proved
    #[test]
    fn prove_never_proves_what_the_fuzzer_breaks() {
        let s = Search { seeds: vec![0, 1, 2], fuzz_seed: 5, scripts: 40, depth: 5, beam: 3, grid: vec![0.0, 0.5, 1.0], hours: vec![1.0], threads: 1 };
        let res = par(16, std::thread::available_parallelism().map_or(1, |n| n.get()), |k| {
            let mut r = Philox4x32::new(380, k as u64); let (p, tl, habits) = persona_at(&mut r, true, false); let prs = props_of(&mut r, &p, &tl, &habits);
            let (found, _) = fuzz(&p, &prs, &s, &run); let mut n = (0, 0);
            for (pr, f) in prs.iter().zip(&found) { let held = persona::by_construction(&p, pr, &run).is_some();
                for (seed, f) in s.seeds.iter().zip(f) { if f.is_none() { continue; } n.0 += 1;
                    assert!(!held, "persona {k}: {} held by construction but the fuzzer breaks it (seed {seed})", pr.id);
                    assert!(persona::prove_seed(&p, pr, *seed, &run).is_err(), "persona {k}: {} proved for seed {seed} but the fuzzer breaks it", pr.id); }
                n.1 += s.seeds.iter().filter(|sd| persona::prove_seed(&p, pr, **sd, &run).is_ok()).count(); }
            n });
        let (found, proved) = res.iter().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        assert!(found >= 10 && proved >= 10, "the check must see both sides: {found} counterexamples, {proved} proved");
    }

    /// (iv) Brute force with learning: 16 tiny personas whose traits all learn (f0 rewards, f1 corrects, large rates and caps) x 2
    /// individuals x their properties, every sequence of up to 3 events over all 12 input combinations and up to 5 over 4 of them;
    /// a property prove calls held or proved never breaks, whatever the learned deltas became
    #[test]
    fn prove_agrees_with_every_event_sequence_of_tiny_learning_personas() {
        let ev = |s: &str| json::parse(s).unwrap();
        let all: Vec<Json> = [false, true].iter().flat_map(|a| [false, true].iter().flat_map(move |b| ["neg", "neu", "pos"].iter().map(move |l| ev(&format!(r#"{{"f0":{a},"f1":{b},"lv":"{l}"}}"#))))).collect();
        let few: Vec<Json> = [r#"{"f0":true}"#, r#"{"f1":true}"#, r#"{"lv":"neg"}"#, r#"{"f0":true,"lv":"pos"}"#].iter().map(|s| ev(s)).collect();
        let res = par(16, std::thread::available_parallelism().map_or(1, |n| n.get()), |k| {
            let mut r = Philox4x32::new(45, k as u64); let (p, tl, habits) = persona_at(&mut r, false, true); let prs = props_of(&mut r, &p, &tl, &habits);
            let mut out = vec![];
            for seed in [0u64, 7] {
                let st0 = persona::init(&p, Some(seed), true, &run); let mut broken = vec![false; prs.len()];
                every_sequence(&p, &prs, &st0, &all, 3, &mut broken); every_sequence(&p, &prs, &st0, &few, 5, &mut broken);
                for (i, pr) in prs.iter().enumerate() { let held = persona::by_construction(&p, pr, &run).is_some(); let proved = persona::prove_seed(&p, pr, seed, &run).is_ok();
                    assert!(!(broken[i] && (held || proved)), "persona {k} seed {seed}: {} {} is {} but a sequence breaks it", pr.id, persona::canon(&pr.rule), if held { "held" } else { "proved" });
                    out.push((held || proved, broken[i], proved && !held)); } }
            out });
        let all: Vec<(bool, bool, bool)> = res.into_iter().flatten().collect();
        let (yes, broke, proved) = (all.iter().filter(|x| x.0).count(), all.iter().filter(|x| x.1).count(), all.iter().filter(|x| x.2).count());
        assert!(yes >= 5 && broke >= 5, "the check must see both sides: {yes} held or proved ({proved} by the bound), {broke} broken of {}", all.len());
    }

    /// (v) The learned box is what the bound needs: one trait, l0 ahead of l1 by 1 nat, both levels learning (f0 rewards, f1
    /// corrects, rate 3). With total_cap 0.2 prove proves "never l1" and no sequence breaks it; with total_cap 2 one correction
    /// after an l0 turn makes it l1, and prove says unknown (without the learned term in the bound it would say proved)
    #[test]
    fn prove_covers_the_learned_box() {
        let alpha: Vec<Json> = [r#"{}"#, r#"{"f0":true}"#, r#"{"f1":true}"#, r#"{"f0":true,"f1":true}"#].iter().map(|s| json::parse(s).unwrap()).collect();
        for (cap, breaks) in [(0.2, false), (2.0, true)] {
            let doc = format!(r#"{{"probbit_persona":1,"identity":{{"name":"T","version":"1","seed":1}},"traits":[{{"id":"a","levels":["l0","l1"],"logw":[1,0]}}],
                "inputs":[{{"id":"f0","kind":"flag"}},{{"id":"f1","kind":"flag"}}],"learning":{{"from":["f0","f1"],"traits":["a"],"rate":3,"step_cap":{cap},"total_cap":{cap}}}}}"#);
            let p = persona::build(&json::parse(&doc).unwrap()).unwrap();
            let prs = persona::props(&p, &json::parse(r#"{"id":"never_l1","then":{"a":["l0"]}}"#).unwrap(), "never", "prop1").unwrap();
            let st0 = persona::init(&p, Some(0), true, &run); let mut broken = vec![false];
            every_sequence(&p, &prs, &st0, &alpha, 4, &mut broken);
            assert_eq!(broken[0], breaks, "cap {cap}"); assert_eq!(persona::prove_seed(&p, &prs[0], 0, &run).is_ok(), !breaks, "cap {cap}");
        }
    }

    /// (iii) A habit's own rule is held by construction: every habit under on_conflict fallback, the top-ranked habit under yield
    #[test]
    fn habits_are_held_by_construction() {
        let mut checked = 0;
        for k in 0..40u64 {
            let mut r = Philox4x32::new(3800, k); let (p, tl, habits) = persona_at(&mut r, true, false); if habits.is_empty() { continue; }
            let prs = props_of(&mut r, &p, &tl, &habits);
            let top = habits.iter().enumerate().max_by(|(i, a), (j, b)| a.priority.cmp(&b.priority).then(j.cmp(i))).map(|(_, h)| h.id.clone()).unwrap();
            for pr in prs.iter().filter(|pr| habits.iter().any(|h| h.id == pr.id)) {
                if !p.yields() || pr.id == top { assert!(persona::by_construction(&p, pr, &run).is_some(), "persona {k}: habit {} is not held by construction", pr.id); checked += 1; } } }
        assert!(checked >= 20, "{checked} habits checked");
    }

    /// drives (§2.9): the search sends goal signals (deadlines, wins, cues; nested into `goals` as a host sends them): the must-do
    /// habit holds under it, and a `pursue` property a due chore outranks is broken with a script that replays
    #[test]
    fn fuzz_sends_goal_signals() {
        let p = persona::build(&json::parse(r#"{"probbit_persona":1,"identity":{"name":"Wants","version":"0.1.0","seed":0},
            "traits":[{"id":"caution","levels":["bold","measured","careful"],"prior":[0.3,0.5,0.2]}],"inputs":[{"id":"security","kind":"flag","effects":{"pursue":{"safety":2.5}}}],
            "habits":[{"id":"chores_due","when":{"goal.chores.deadline_hours":{"at_most":24}},"then":{"pursue":["chores"]},"priority":2}],
            "drives":{"goals":[{"id":"fun","interest":2.0},{"id":"chores","interest":0.5},{"id":"safety","interest":1.0,"floor":0.1}]}}"#).unwrap()).unwrap();
        let rule = |t: &str| persona::props(&p, &json::parse(t).unwrap(), "--never", "never").unwrap().remove(0);
        let props = vec![rule(r#"{"when":{"goal.chores.deadline_hours":{"at_most":24}},"then":{"pursue":["chores"]}}"#), rule(r#"{"when":{"security":true},"then":{"pursue":["safety"]}}"#)];
        let s = Search { seeds: vec![0, 1], fuzz_seed: 0, scripts: 8, depth: 5, beam: 2, grid: vec![0.0, 0.5, 1.0], hours: vec![1.0, 12.0, 48.0], threads: 2 };
        let (found, turns) = fuzz(&p, &props, &s, &run);
        assert!(turns > 50 && found[0].iter().all(Option::is_none), "the must-do held ({turns} turns)");
        let f = found[1][0].as_ref().expect("a due chore outranks security's safety");
        let sc = script_json(&f.script); assert!(persona::canon(&sc).contains(r#""goals":{"chores":{"deadline_hours":"#), "{}", persona::canon(&sc));
        let mut st = persona::init(&p, Some(0), true, &run); let mut last = None;
        for ev in sc.as_arr().unwrap() { let (doc, ns) = persona::turn(&p, &st, ev, false, &run, false).unwrap(); last = persona::breaks(&p, &props[1], &st, &doc); st = ns; }
        assert_eq!(last.map(|b| b[0].level.clone()), Some("chores".to_string()));
    }
}
