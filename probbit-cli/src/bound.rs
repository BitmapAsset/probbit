//! RFC 0004 `bound` (docs/rfcs/0004-bound.md): per learned trait, epsilon, the largest change in total variation that any event
//! sequence can make to that trait's odds at rest, computed from the persona file by probing the vertices of the learned-weight
//! box (mechanism: arithmetic over declared numbers; nothing here writes a state). Also the audit of a recorded strand against it
//! and the `erase` control line's effect on a state.
use super::*;

/// The `bound` block: every field optional
pub struct Bound { pub rest_hours: Option<f64>, pub max_epsilon: Option<f64>, pub seeds: Option<Vec<u64>>, pub count: Vec<String> }
const SEED_MAX: u64 = 2_147_483_647;
const MAX_SEEDS: usize = 10_000;
/// More searched coordinates than this in a trait's component: the coarse ceiling, never sampled vertices
const MAX_COORDS: usize = 12;
const SEEDS_MSG: &str = "a range a-b or a list of whole numbers in 0..2147483647 (a <= b, at most 10000 seeds)";

fn whole(x: f64) -> Option<u64> { (x >= 0.0 && x.fract() == 0.0 && x <= SEED_MAX as f64).then_some(x as u64) }
/// `a-b` or `a,b,c` (the `--seeds` flag and a `seeds` string)
pub fn seeds_text(s: &str) -> Option<Vec<u64>> {
    let v: Vec<u64> = match s.split_once('-') {
        Some((a, b)) => { let (a, b) = (a.trim().parse::<u64>().ok()?, b.trim().parse::<u64>().ok()?); if a > b || b > SEED_MAX { return None } (a..=b).take(MAX_SEEDS + 1).collect() }
        None => s.split(',').map(|x| x.trim().parse::<u64>().ok().filter(|x| *x <= SEED_MAX)).collect::<Option<Vec<u64>>>()? };
    (!v.is_empty() && v.len() <= MAX_SEEDS).then_some(v)
}
fn seeds_of(j: &Json, path: &str) -> R<Vec<u64>> {
    let v = match j { Json::Str(s) => seeds_text(s), Json::Arr(a) => a.iter().map(|x| x.as_f64().and_then(whole)).collect::<Option<Vec<u64>>>(), _ => None };
    v.filter(|v| !v.is_empty() && v.len() <= MAX_SEEDS).ok_or_else(|| perr(path, SEEDS_MSG))
}
/// The strict reader (refusal rows BO-R1 to BO-R6), against the persona built without the block
pub fn spec(b: &Json, p: &Persona) -> R<Bound> {
    let kv = keys(b, "bound", &["rest_hours", "max_epsilon", "seeds", "count", "comment"], &[])?; // BO-R1
    need(p.learning.is_some(), "bound", "a bound block needs a learning block (it certifies the learned weights)")?; // BO-R4
    let f = |k: &str, ok: &dyn Fn(f64) -> bool, msg: &str| -> R<Option<f64>> { match get(kv, k) { None | Some(Json::Null) => Ok(None),
        Some(x) => x.as_f64().filter(|x| ok(*x)).map(Some).ok_or_else(|| perr(&at("bound", k), msg)) } };
    let rest_hours = f("rest_hours", &|h| h > 0.0 && h <= 1e6, "a number of hours in (0, 1000000]")?; // BO-R5
    let max_epsilon = f("max_epsilon", &|e| e > 0.0 && e < 1.0, "a number in (0, 1)")?; // BO-R2
    let seeds = match get(kv, "seeds") { None | Some(Json::Null) => None, Some(s) => Some(seeds_of(s, "bound.seeds")?) }; // BO-R3
    let mut count: Vec<String> = vec![]; // BO-R6
    for (i, c) in list(get(kv, "count"), "bound.count")?.iter().enumerate() { let path = ix("bound.count", i);
        let c = c.as_str().ok_or_else(|| perr(&path, "a mood without half_life_hours, deadlines or needs"))?;
        let ok = p.var(c).is_some_and(|v| v.mood && v.half_life.is_none()) || (c == "deadlines" && p.drives.is_some()) || (c == "needs" && p.has_needs());
        need(ok, &path, "a mood without half_life_hours, deadlines or needs")?; need(!count.iter().any(|x| x == c), &path, "listed twice")?; count.push(c.to_string()); }
    Ok(Bound { rest_hours, max_epsilon, seeds, count })
}
/// The block of a document (None without one)
pub fn of_doc(doc: &Json, p: &Persona) -> R<Option<Bound>> { doc.get("bound").map(|b| spec(b, p)).transpose() }
/// Default rest: 30 × the longest declared half-life in hours (moods, pace, drives), 24 without one
pub fn default_rest(p: &Persona) -> f64 {
    let mut h: Vec<f64> = p.vars.iter().filter_map(|v| v.half_life).collect();
    if let Some(pc) = &p.pace { h.push(pc.hl); }
    if let Some(d) = &p.drives { h.extend([d.hl_w, d.hl_a, d.hl_e]); }
    h.into_iter().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))).map_or(24.0, |x| 30.0 * x)
}

/// The rest probe: one event of `h` idle hours, no inputs -> per learned trait its odds in level order
pub fn rest_odds(p: &Persona, st: &State, h: f64, eng: Engine) -> R<Vec<Vec<f64>>> {
    let (doc, _) = turn(p, st, &Json::Obj(vec![("elapsed_hours".into(), Json::Num(h))]), false, eng, false)?;
    let l = p.learning.as_ref().expect("learning");
    Ok(l.traits.iter().map(|t| { let o = doc.get("stance").and_then(|s| s.get(t)).and_then(|x| x.get("odds"));
        p.var(t).unwrap().levels.iter().map(|lv| o.and_then(|o| o.get(lv)).and_then(Json::as_f64).unwrap_or(0.0)).collect() }).collect())
}
pub fn tv(a: &[f64], b: &[f64]) -> f64 { 0.5 * a.iter().zip(b).fold(0.0, |s, (x, y)| s + (x - y).abs()) }
pub fn with_learned(p: &Persona, st: &State, w: &[Vec<f64>]) -> State { let mut s = st.clone();
    for (k, (_, d)) in s.learned.iter_mut().enumerate() { *d = w[k].clone(); } s.seal(p); s }

/// A searched coordinate besides the trait's own corner: a learned weight of another trait in the component (trait, level),
/// box ±c; or a counted mood's accumulator (mood slot, level), box [lo, hi]
#[derive(Clone)]
pub enum Coord { W(usize, usize), M(usize, usize, f64, f64) }
/// One learned trait's channels at rest (the scope rule): its component, the searched coordinates, the enumerated history
/// flags (flag, stored length), the residue in nats of decayed channels, and every channel this build neither searches,
/// decays nor enumerates (named at its path)
pub struct Scope { pub coords: Vec<Coord>, pub flags: Vec<(String, usize)>, pub d_r: f64, pub uncounted: Vec<String>, pub comp: Vec<String>, pub others: usize }
/// The largest one-event effect a mood's accumulator can take: every declared input effect on it at its extreme
fn mood_reach(p: &Persona, id: &str) -> f64 { mood_box(p, None, id).iter().map(|(lo, hi)| lo.abs().max(hi.abs())).fold(0.0, f64::max) }
/// Per level of a mood, the least and most one-event effect on its accumulator (inputs at their extremes, the seed's gains when a
/// state is given, history features at 0 and their cap), widened to include 0: the accumulator, an inertia average of such
/// effects from 0, stays inside
fn mood_box(p: &Persona, st: Option<&State>, id: &str) -> Vec<(f64, f64)> {
    let n = p.var(id).map_or(0, |v| v.levels.len()); let mut bx = vec![(0.0f64, 0.0f64); n];
    let on = |e: &Eff, l: usize| e.iter().filter(|(k, _)| k == id).map(|(_, v)| v.get(l).copied().unwrap_or(0.0)).sum::<f64>();
    for x in &p.inputs { let g = st.map_or(1.0, |s| input_gain(p, s, &x.id).max(1.0));
        for (l, b) in bx.iter_mut().enumerate() { let ch: Vec<f64> = match x.kind {
                Kind::Flag => vec![0.0, on(&x.effects, l)], Kind::Number => vec![0.0, x.min * on(&x.effects, l), x.max * on(&x.effects, l)],
                Kind::Level => std::iter::once(0.0).chain(x.by_level.iter().map(|(_, e)| on(e, l))).collect() };
            b.0 += g * ch.iter().copied().fold(0.0, f64::min); b.1 += g * ch.iter().copied().fold(0.0, f64::max); } }
    for h in &p.history { for (l, b) in bx.iter_mut().enumerate() { let y = h.cap as f64 * on(&h.effects, l); b.0 += y.min(0.0); b.1 += y.max(0.0); } }
    bx
}
pub fn scope(p: &Persona, b: &Bound, t: &str, rest: f64, st: Option<&State>) -> Scope {
    let l = p.learning.as_ref().unwrap(); let comp = component(p, t);
    let mut coords: Vec<Coord> = vec![]; let mut others = 0;
    for (k2, x) in l.traits.iter().enumerate() { if x != t && comp.contains(x) { others += 1; for i in 0..p.var(x).unwrap().levels.len() { coords.push(Coord::W(k2, i)); } } }
    let (mut d_r, mut uncounted) = (0.0, vec![]); let nt = p.vars.iter().filter(|v| !v.mood).count();
    for (slot, v) in p.vars.iter().filter(|v| v.mood).enumerate() { if !comp.contains(&v.id) { continue; }
        match v.half_life { Some(hl) => d_r += 2.0 * v.inertia * 0.5f64.powf(rest / hl) * mood_reach(p, &v.id),
            None if b.count.contains(&v.id) => { for (i, (lo, hi)) in mood_box(p, st, &v.id).into_iter().enumerate() { coords.push(Coord::M(slot, i, lo, hi)); } }
            None => uncounted.push(format!("moods[{}]", p.vars.iter().position(|w| w.id == v.id).unwrap() - nt)) } }
    // history (enumerated): every stored sequence of a flag a `recent` feature reads, when the feature's effects reach the component
    // or a habit's `when` names it; a streak is 0 at the rest probe
    let mut flags: Vec<(String, usize)> = vec![];
    for h in p.history.iter().filter(|h| !h.streak) {
        let reaches = h.effects.iter().any(|(k, _)| comp.contains(k)) || p.habits.iter().any(|hb| hb.when.iter().any(|(k, _)| *k == h.id));
        if !reaches { continue; } let len = h.window.saturating_sub(1);
        match flags.iter_mut().find(|(f, _)| *f == h.of) { Some(e) => e.1 = e.1.max(len), None => flags.push((h.of.clone(), len)) } }
    for (i, h) in p.habits.iter().enumerate() { let prev_rel = h.rules.iter().flat_map(|(_, r)| r).any(|r| canon(r).contains("\"prev"));
        if prev_rel && h.rules.iter().flat_map(|(k, r)| r.iter().map(move |r| rule_vars(k, r))).flatten().any(|x| comp.contains(&x)) { uncounted.push(format!("habits[{i}]")); } }
    for v in p.vars.iter().filter(|v| v.hold.is_some() && comp.contains(&v.id)) { uncounted.push(format!("{}.hold", v.id)); }
    if let Some(pc) = &p.pace { if pc.effects.iter().any(|(k, _)| comp.contains(k)) { d_r += 2.0 * 0.5f64.powf(rest / pc.hl) * pace_shift(pc); } }
    if p.drives.is_some() && (comp.iter().any(|x| x == "pursue") || t == "pursue") { uncounted.push(if b.count.iter().any(|x| x == "deadlines") { "drives (counted; not yet searched by this build)".into() } else { "drives".into() }); }
    if p.has_needs() && (comp.iter().any(|x| x == "pursue") || t == "pursue") { uncounted.push(if b.count.iter().any(|x| x == "needs") { "needs (counted; not yet searched by this build)".into() } else { "needs".into() }); }
    Scope { coords, flags, d_r, uncounted, comp, others }
}
/// Every enumerated history at the rest probe, as copies of `st0` (one when nothing is enumerated); None above 64 combinations
pub fn bases(sc: &Scope, st0: &State) -> Option<Vec<State>> {
    let bits: usize = sc.flags.iter().map(|(_, n)| *n).sum(); if bits > 6 { return None; }
    Some((0..1u64 << bits).map(|m| { let mut s = st0.clone(); let mut b = 0;
        for (f, n) in &sc.flags { let seq: Vec<bool> = (0..*n).map(|i| m >> (b + i) & 1 == 1).collect(); b += n;
            match s.history.iter_mut().find(|(k, _)| k == f) { Some(e) => e.1 = seq, None => s.history.push((f.clone(), seq)) } }
        s }).collect())
}
/// The proper non-empty level sets of an n-level trait, as bit masks
fn sets(n: usize) -> impl Iterator<Item = u32> { 1..(1u32 << n) - 1 }
/// The vertex search at one seed, learned box ±c: the largest p_S − p0_S over level sets S of trait `k` (own corner c_S), the
/// vertices of the other searched coordinates and the enumerated histories -> (value, S mask, probes)
pub fn search(p: &Persona, k: usize, coords: &[Coord], c: f64, bases: &[State], p0: &[f64], rest: f64, eng: Engine) -> R<(f64, u32, usize)> {
    let lk = p0.len(); let (mut best, mut arg, mut probes) = (f64::NEG_INFINITY, 0u32, 0usize);
    for base in bases { for s in sets(lk) { for vx in 0..(1u64 << coords.len()) {
        let mut st = base.clone(); for (_, d) in st.learned.iter_mut() { d.iter_mut().for_each(|x| *x = 0.0); }
        st.learned[k].1 = (0..lk).map(|i| if s >> i & 1 == 1 { c } else { -c }).collect();
        for (bit, co) in coords.iter().enumerate() { let up = vx >> bit & 1 == 1;
            match co { Coord::W(k2, i) => st.learned[*k2].1[*i] = if up { c } else { -c }, Coord::M(m, i, lo, hi) => st.mood[*m].1[*i] = r6(if up { *hi } else { *lo }) } }
        st.seal(p);
        let odds = rest_odds(p, &st, rest, eng)?; probes += 1;
        let ps: f64 = (0..lk).filter(|i| s >> i & 1 == 1).map(|i| odds[k][i] - p0[i]).sum();
        if ps > best + 1e-12 { best = ps; arg = s; } } } }
    Ok((r6(best.max(0.0)), arg, probes))
}

/// `probbit persona bound`: the certificate document and whether every channel is covered and every epsilon within max_epsilon
pub fn publish(p: &Persona, b: &Bound, seeds_flag: Option<Vec<u64>>, eng: Engine) -> R<(Json, bool)> {
    let l = p.learning.as_ref().ok_or_else(|| perr("bound", "a bound block needs a learning block (it certifies the learned weights)"))?;
    let rest = b.rest_hours.unwrap_or_else(|| default_rest(p)); let c = l.total_cap;
    let seeds = seeds_flag.or_else(|| b.seeds.clone()).unwrap_or_else(|| (0..100).collect());
    let g_max = p.surprise.as_ref().map_or(1.0, |s| s.g_max);
    let mut rows = vec![]; let mut all_ok = true;
    let base: Vec<(State, Vec<Vec<f64>>)> = seeds.iter().map(|s| { let st = init(p, Some(*s), true, eng); let o = rest_odds(p, &st, rest, eng)?; Ok((st, o)) }).collect::<R<_>>()?;
    for (k, t) in l.traits.iter().enumerate() {
        let sc = scope(p, b, t, rest, None); let lv = &p.var(t).unwrap().levels; let n_coords = lv.len() + sc.coords.len();
        let d = 2.0 * c * (1 + sc.others) as f64 + sc.coords.iter().map(|x| if let Coord::M(_, _, lo, hi) = x { hi - lo } else { 0.0 }).sum::<f64>();
        let ceiling = r6((d / 4.0).tanh()); let r = (sc.d_r / 4.0).tanh();
        let per_seed: Vec<Option<(Scope, Vec<State>)>> = base.iter().map(|(st0, _)| { let sc = scope(p, b, t, rest, Some(st0)); let bs = bases(&sc, st0)?; Some((sc, bs)) }).collect();
        let coarse = n_coords > MAX_COORDS || per_seed.iter().any(Option::is_none);
        let (mut eps, mut arg, mut at_seed, mut probes) = (0.0f64, 0u32, seeds[0], 0usize);
        if coarse { eps = if per_seed.iter().any(Option::is_none) || !sc.flags.is_empty() { 1.0 } else { ceiling }; } else {
            for (si, ((_, p0), ps)) in base.iter().zip(&per_seed).enumerate() { let (sc, bs) = ps.as_ref().unwrap();
                let (e, s, n) = search(p, k, &sc.coords, c, bs, &p0[k], rest, eng)?; probes += n;
                if e > eps { eps = e; arg = s; at_seed = seeds[si]; } } }
        // speed_floor (proved): after n feedback events every weight lies in ±min(c, n m); the fewest n whose vertex bound reaches eps / 2
        let m = 2.0 * l.step_cap * g_max * (lv.len() - 1) as f64 / lv.len() as f64;
        let si = seeds.iter().position(|s| *s == at_seed).unwrap_or(0);
        let floor = if coarse { None } else if eps <= 0.0 { Some(0) } else { let (sc, bs) = per_seed[si].as_ref().unwrap(); let mut n = 1u64; loop { let cn = (n as f64 * m).min(c);
            let (e, _, _) = search(p, k, &sc.coords, cn, bs, &base[si].1[k], rest, eng)?; if e >= eps / 2.0 - 1e-12 || cn >= c { break Some(n) } n += 1; } };
        let speed = if coarse { None } else if eps <= 0.0 { Some(0) } else { greedy(p, k, arg, &base[si].0, &base[si].1[k], eps / 2.0, rest, eng) };
        let over = b.max_epsilon.is_some_and(|me| eps > me);
        let ok = sc.uncounted.is_empty() && !over && (b.max_epsilon.is_none() || r <= 1e-6); all_ok &= ok;
        let s = |x: &str| Json::Str(x.to_string());
        rows.push(Json::Obj(vec![("trait".into(), s(t)), ("epsilon".into(), Json::Num(eps)), ("ceiling".into(), if sc.flags.is_empty() { Json::Num(ceiling) } else { Json::Null }), ("enumerated".into(), Json::Arr(sc.flags.iter().map(|(f, n)| s(&format!("history {f}: {n} turns"))).collect())), ("coarse".into(), Json::Bool(coarse)),
            ("level_set".into(), Json::Arr(lv.iter().enumerate().filter(|(i, _)| arg >> i & 1 == 1).map(|(_, x)| s(x)).collect())), ("seed".into(), Json::Num(at_seed as f64)),
            ("speed".into(), speed.map_or_else(|| if coarse { Json::Null } else { s("> 200") }, |n| Json::Num(n as f64))), ("speed_floor".into(), floor.map_or(Json::Null, |n| Json::Num(n as f64))),
            ("r".into(), Json::Num(if r == 0.0 { 0.0 } else { format!("{r:.3e}").parse().unwrap_or(r) })),
            ("component".into(), Json::Arr(sc.comp.iter().map(|x| s(x)).collect())), ("probes".into(), Json::Num(probes as f64)),
            ("uncounted".into(), Json::Arr(sc.uncounted.iter().map(|x| s(x)).collect())), ("over_max_epsilon".into(), Json::Bool(over))]));
    }
    // the scope rule, persona-wide: a mood without half_life_hours that is not counted is a persistent channel nothing bounds
    let nt = p.vars.iter().filter(|v| !v.mood).count();
    let global: Vec<String> = p.vars.iter().enumerate().filter(|(_, v)| v.mood && v.half_life.is_none() && !b.count.contains(&v.id)).map(|(i, _)| format!("moods[{}]", i - nt)).collect();
    all_ok &= global.is_empty();
    let doc = Json::Obj(vec![("persona".into(), Json::Str(p.name.clone())), ("digest".into(), Json::Str(p.digest.clone())), ("rest_hours".into(), Json::Num(rest)),
        ("uncounted".into(), Json::Arr(global.iter().map(|x| Json::Str(x.clone())).collect())),
        ("seeds".into(), Json::Str(ranges(&seeds))), ("max_epsilon".into(), b.max_epsilon.map_or(Json::Null, Json::Num)), ("total_cap".into(), Json::Num(c)),
        ("claim".into(), Json::Str(CLAIM.into())), ("traits".into(), Json::Arr(rows)), ("ok".into(), Json::Bool(all_ok))]);
    Ok((doc, all_ok))
}
/// Seeds as ranges: 0-9,12
fn ranges(s: &[u64]) -> String { let mut out: Vec<String> = vec![]; let mut i = 0;
    while i < s.len() { let mut j = i; while j + 1 < s.len() && s[j + 1] == s[j] + 1 { j += 1; } out.push(if j > i { format!("{}-{}", s[i], s[j]) } else { s[i].to_string() }); i = j + 1; }
    out.join(",") }
/// The certified claim, as a theorem about the engine's arithmetic
pub const CLAIM: &str = "no event sequence moves a learned trait's odds at rest (rest_hours idle, no inputs) by more than epsilon + r in total variation; epsilon is the exact maximum over the learned-weight box ±total_cap, computed from this file; it bounds the stance at rest, not the words a model writes";

/// `speed` (a witness): the built-in greedy persuader on a copy of the individual, praise when the released level is in S and
/// criticism otherwise, 0.1 h apart; the fewest feedback events after which the resting change reaches `half` (None: > 200)
fn greedy(p: &Persona, k: usize, s: u32, st0: &State, p0: &[f64], half: f64, rest: f64, eng: Engine) -> Option<u64> {
    let l = p.learning.as_ref()?; let t = &l.traits[k]; let lv = &p.var(t)?.levels;
    let ev = |st: &State, fb: Option<&str>| -> Option<(Json, State)> {
        for src in [None, Some("human:persuader"), Some("env:persuader")] {
            let mut kv = vec![("elapsed_hours".to_string(), Json::Num(0.1))];
            if let Some(f) = fb { kv.push((f.to_string(), Json::Bool(true))); } if let Some(x) = src { kv.push(("src".into(), Json::Str(x.into()))); }
            if let Ok(r) = turn(p, st, &Json::Obj(kv), false, eng, false) { return Some(r); } }
        None };
    let (mut doc, mut st) = ev(st0, None)?;
    for n in 1..=200u64 {
        let rel = doc.get("stance").and_then(|x| x.get(t)).and_then(|x| x.get("level")).and_then(Json::as_str).and_then(|x| lv.iter().position(|y| y == x)).unwrap_or(0);
        let fb = if s >> rel & 1 == 1 { &l.from[0] } else { l.from.get(1).unwrap_or(&l.from[0]) };
        let (d2, s2) = ev(&st, Some(fb))?; doc = d2; st = s2;
        let odds = rest_odds(p, &st, rest, eng).ok()?;
        let ps: f64 = (0..lv.len()).filter(|i| s >> i & 1 == 1).map(|i| odds[k][i] - p0[i]).sum();
        if ps >= half - 1e-12 { return Some(n); } }
    None
}

/// The plain-text certificate
pub fn text(doc: &Json) -> String {
    let n = |j: Option<&Json>| j.map_or("-".into(), |x| match x { Json::Num(v) => pyn(*v), Json::Str(s) => s.clone(), _ => "-".into() });
    let mut out = format!("bound: {} (rest {} h, seeds {}, max_epsilon {})\n", n(doc.get("persona")), n(doc.get("rest_hours")), n(doc.get("seeds")), n(doc.get("max_epsilon")));
    for r in doc.get("traits").and_then(Json::as_arr).unwrap_or(&[]) {
        let set: Vec<String> = r.get("level_set").and_then(strs).unwrap_or_default(); let unc: Vec<String> = r.get("uncounted").and_then(strs).unwrap_or_default();
        out += &format!("  {}: epsilon {}{} (ceiling {}, S {{{}}}, seed {}), speed {}, speed_floor {}, r {}, uncounted: {}\n", n(r.get("trait")), n(r.get("epsilon")),
            if r.get("coarse") == Some(&Json::Bool(true)) { " coarse" } else { "" }, n(r.get("ceiling")), set.join(", "), n(r.get("seed")), n(r.get("speed")), n(r.get("speed_floor")), n(r.get("r")),
            if unc.is_empty() { "none".into() } else { unc.join(", ") }); }
    let g: Vec<String> = doc.get("uncounted").and_then(strs).unwrap_or_default();
    out + &format!("  uncounted (persona): {}\n  claim: {CLAIM}\n  ok: {}", if g.is_empty() { "none".into() } else { g.join(", ") }, doc.get("ok") == Some(&Json::Bool(true)))
}

/// The learned traits and total_cap (None without a learning block)
pub fn learned_traits(p: &Persona) -> Option<(Vec<String>, f64)> { p.learning.as_ref().map(|l| (l.traits.clone(), l.total_cap)) }
/// A state's learned weights, in `learning.traits` order
pub fn learned_of(st: &State) -> Vec<Vec<f64>> { st.learned.iter().map(|(_, d)| d.clone()).collect() }
impl State {
    /// The `erase` control line (RFC 0004 amendment A2): every learned weight back to zero, the state resealed. The open
    /// decisions' credits are zeroed by every control line already.
    pub fn erased(&self, p: &Persona) -> State { let mut s = self.clone(); for (_, d) in s.learned.iter_mut() { for x in d.iter_mut() { *x = 0.0; } } s.seal(p); s }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(prog: &Json, f: &Flags) -> Json { run_program(prog, f, 1, 100, 0) }
    /// The reference fixture's shape (one learned 3-level trait, no couplings to it): the closed form applies
    const REF: &str = r#"{"probbit_persona":1,"identity":{"name":"B","version":"1","seed":0},
        "traits":[{"id":"verbosity","levels":["terse","short","full"],"spread":0.5},{"id":"humour","levels":["none","light","playful"],"spread":0.5}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.6,"half_life_hours":6}],"couplings":[{"vars":["valence","humour"],"align":0.8}],
        "inputs":[{"id":"praise","kind":"flag"},{"id":"criticism","kind":"flag"},{"id":"loss","kind":"flag","effects":{"valence":-1.0}}],
        "learning":{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":1}BOUND}"#;
    fn doc(bound: &str) -> Json { json::parse(&REF.replace("BOUND", bound)).unwrap() }
    fn refused(bound: &str) -> String { match build(&doc(bound)) { Err(e) => format!("{}: {}", e.path, e.msg), Ok(_) => "ok".into() } }
    #[test]
    fn refusals_bo_r1_to_r6() {
        assert_eq!(refused(r#","bound":{"rest_hour":3}"#), "bound.rest_hour: unknown field (allowed: comment, count, max_epsilon, rest_hours, seeds)"); // BO-R1
        assert_eq!(refused(r#","bound":[1]"#), "bound: must be a mapping");
        assert_eq!(refused(r#","bound":{"max_epsilon":1}"#), "bound.max_epsilon: a number in (0, 1)"); // BO-R2
        assert_eq!(refused(r#","bound":{"max_epsilon":"0.5"}"#), "bound.max_epsilon: a number in (0, 1)");
        for s in [r#""9-3""#, r#""0-10000""#, r#"[1.5]"#, r#"7"#, r#""x""#, r#"[]"#] { assert_eq!(refused(&format!(r#","bound":{{"seeds":{s}}}"#)), format!("bound.seeds: {SEEDS_MSG}")); } // BO-R3
        assert_eq!(refused(r#","bound":{"rest_hours":0}"#), "bound.rest_hours: a number of hours in (0, 1000000]"); // BO-R5
        assert_eq!(refused(r#","bound":{"rest_hours":1000001}"#), "bound.rest_hours: a number of hours in (0, 1000000]");
        assert_eq!(refused(r#","bound":{"count":["valence"]}"#), "bound.count[0]: a mood without half_life_hours, deadlines or needs"); // BO-R6
        assert_eq!(refused(r#","bound":{"count":["needs"]}"#), "bound.count[0]: a mood without half_life_hours, deadlines or needs");
        assert_eq!(refused(r#","bound":{"seeds":"0-9","rest_hours":200,"max_epsilon":0.5}"#), "ok");
        let no_learning = REF.replace(r#""learning":{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":1}BOUND"#, r#""bound":{}"#);
        assert_eq!(match build(&json::parse(&no_learning).unwrap()) { Err(e) => format!("{}: {}", e.path, e.msg), Ok(_) => "ok".into() }, "bound: a bound block needs a learning block (it certifies the learned weights)"); // BO-R4
    }
    #[test]
    fn block_leaves_the_digest_and_states_unchanged() {
        let (a, b) = (build(&doc("")).unwrap(), build(&doc(r#","bound":{"seeds":"0-1"}"#)).unwrap());
        assert_eq!(a.digest, b.digest);
        assert_eq!(canon(&init(&a, Some(3), true, &run).to_json(&a)), canon(&init(&b, Some(3), true, &run).to_json(&b)));
    }
    #[test]
    fn epsilon_is_the_closed_form_for_a_lone_trait_and_never_exceeded() {
        let d = doc(r#","bound":{"seeds":"0-1","rest_hours":200}"#); let p = build(&d).unwrap(); let b = of_doc(&d, &p).unwrap().unwrap();
        let (j, ok) = publish(&p, &b, None, &run).unwrap(); assert!(ok, "{}", canon(&j));
        let rows = j.get("traits").and_then(Json::as_arr).unwrap(); let v = &rows[0];
        // closed form at the attaining seed's resting odds
        let seed = v.get("seed").and_then(Json::as_f64).unwrap() as u64; let st = init(&p, Some(seed), true, &run); let p0 = rest_odds(&p, &st, 200.0, &run).unwrap();
        let k = (2.0f64).exp(); let cf = (1..7u32).map(|s| { let a: f64 = (0..3).filter(|i| s >> i & 1 == 1).map(|i| p0[0][i]).sum(); k * a / (k * a + 1.0 - a) - a }).fold(0.0, f64::max);
        let eps = v.get("epsilon").and_then(Json::as_f64).unwrap(); assert!((eps - cf).abs() < 2e-6, "{eps} vs {cf}");
        assert!(eps <= v.get("ceiling").and_then(Json::as_f64).unwrap());
        assert!(v.get("speed_floor").and_then(Json::as_f64).unwrap() <= v.get("speed").and_then(Json::as_f64).unwrap_or(1e9));
        // a random walk of the real learner never leaves epsilon
        let mut s = init(&p, Some(seed), true, &run);
        for i in 0..40 { let f = if i % 3 == 0 { "criticism" } else { "praise" }; s = turn(&p, &s, &Json::Obj(vec![(f.into(), Json::Bool(true)), ("elapsed_hours".into(), Json::Num(0.1))]), false, &run, false).unwrap().1;
            let o = rest_odds(&p, &s, 200.0, &run).unwrap(); assert!(tv(&o[0], &p0[0]) <= eps + 1e-6); }
    }
    #[test]
    fn uncounted_mood_names_its_path() {
        let d = json::parse(&REF.replace(r#","half_life_hours":6"#, "").replace("BOUND", r#","bound":{"seeds":"0"}"#)).unwrap(); let p = build(&d).unwrap();
        let (j, ok) = publish(&p, &of_doc(&d, &p).unwrap().unwrap(), None, &run).unwrap(); assert!(!ok);
        assert_eq!(canon(j.get("traits").and_then(Json::as_arr).unwrap()[1].get("uncounted").unwrap()), r#"["moods[0]"]"#);
    }
    #[test]
    fn erase_zeroes_learned_weights() {
        let p = build(&doc("")).unwrap(); let mut s = init(&p, Some(1), true, &run);
        for _ in 0..5 { s = turn(&p, &s, &Json::Obj(vec![("praise".into(), Json::Bool(true))]), false, &run, false).unwrap().1; }
        assert!(s.learned.iter().any(|(_, d)| d.iter().any(|x| *x != 0.0)));
        let e = s.erased(&p); assert!(e.learned.iter().all(|(_, d)| d.iter().all(|x| *x == 0.0))); assert_ne!(e.digest, s.digest);
    }

    /// bo6: three pairwise-coupled 3-level learned traits; the vertex search equals the maximum over the {−c, 0, +c}^9 grid
    /// within 1e-6, no grid point lies above it, the coupled epsilon exceeds the lone-trait closed form somewhere, and the search
    /// without the partners' vertices falls below the grid (the null). Release only: `cargo test --release -- --ignored bo6`
    #[test]
    #[ignore]
    fn bo6_coupled_vertex_equals_grid() {
        let d = json::parse(r#"{"probbit_persona":1,"identity":{"name":"C3","version":"1","seed":0},
            "traits":[{"id":"a","levels":["a0","a1","a2"],"spread":0.5},{"id":"b","levels":["b0","b1","b2"],"spread":0.5},{"id":"c","levels":["c0","c1","c2"],"spread":0.5}],
            "couplings":[{"vars":["a","b"],"align":1.5},{"vars":["b","c"],"align":-1.2},{"vars":["a","c"],"align":0.9}],
            "inputs":[{"id":"praise","kind":"flag"},{"id":"criticism","kind":"flag"}],
            "learning":{"from":["praise","criticism"],"traits":["a","b","c"],"rate":0.5,"step_cap":0.2,"total_cap":1},"bound":{"seeds":"0-1","rest_hours":24}}"#).unwrap();
        let p = build(&d).unwrap(); let b = of_doc(&d, &p).unwrap().unwrap(); let mut active = false;
        for seed in 0..2u64 { let st0 = init(&p, Some(seed), true, &run); let p0 = rest_odds(&p, &st0, 24.0, &run).unwrap();
            let mut grid = vec![[f64::NEG_INFINITY; 3]; 1]; grid.clear(); let mut brute = [f64::NEG_INFINITY; 3];
            for g in 0..19683u32 { let mut x = g; let w: Vec<Vec<f64>> = (0..3).map(|_| (0..3).map(|_| { let v = (x % 3) as f64 - 1.0; x /= 3; v }).collect()).collect();
                let o = rest_odds(&p, &with_learned(&p, &st0, &w), 24.0, &run).unwrap();
                for k in 0..3 { for s in sets(3) { let ps: f64 = (0..3).filter(|i| s >> i & 1 == 1).map(|i| o[k][i] - p0[k][i]).sum(); brute[k] = brute[k].max(ps); } } }
            for k in 0..3 { let sc = scope(&p, &b, ["a", "b", "c"][k], 24.0, Some(&st0)); assert_eq!(sc.coords.len(), 6);
                let (v, _, _) = search(&p, k, &sc.coords, 1.0, &[st0.clone()], &p0[k], 24.0, &run).unwrap();
                assert!((v - r6(brute[k])).abs() <= 1e-6, "seed {seed} trait {k}: vertex {v} grid {}", brute[k]);
                let (own, _, _) = search(&p, k, &[], 1.0, &[st0.clone()], &p0[k], 24.0, &run).unwrap();
                assert!(own < r6(brute[k]) - 1e-6 || own == v, "null: own corner {own} vs grid {}", brute[k]);
                let kk = (2.0f64).exp(); let cf = (1..7u32).map(|s| { let a: f64 = (0..3).filter(|i| s >> i & 1 == 1).map(|i| p0[k][i]).sum(); kk * a / (kk * a + 1.0 - a) - a }).fold(0.0, f64::max);
                active |= v > cf + 1e-3;
                eprintln!("bo6 seed {seed} trait {k}: vertex {v} grid {:.6} own-corner {own} closed-form {cf:.6}", brute[k]); } }
        assert!(active, "the coupled epsilon never exceeds the closed form: the fixture is not coupled enough");
    }
}
