//! `pbit run`: execute a general pbit-ir program (JSON wire format v1, docs/pbit-ir-json.md) on the virtual p-bit processor.
//! Instructions (`--op`): `decide` (default: exact enumeration if the feasible set is <= --exact-limit, else the exact
//! frontier DP if the program allows it (--frontier-states, 0 = off), else 4 gated chains),
//! `exact` (enumeration only), `sample` (chains + gate, never enumerate). What-if = `"clamp"` on a variable.
use crate::json::{num, obj, str as jstr, InErr, Json};
use pbit_ir::*;

pub struct Prog { pub m: Model, pub vars: Vec<String>, pub values: Vec<String>, /// R19 P1.3(a): what the compile pass removed
    pub compiled: Compiled }

/// JSON (pbit_ir 1) -> Model. See docs/pbit-ir-json.md; strict: every field type-checked, unknown fields, duplicate ids / value
/// names and empty domains rejected, with a structured error (its "Input contract").
pub fn from_json(j: &Json) -> Result<Prog, InErr> {
    use crate::json::{arr, at, count, fields, ix, keyed, limit, names, number, opt, req, schema, text, value, weight};
    fields(j, "", &["pbit_ir", "values", "vars", "pairs", "caps", "all_different", "implies", "tables", "precedes", "linear", "start", "comment"])?;
    if let Some(c) = opt(j, "comment") { text(c, "comment")?; }
    let ver = number(opt(j, "pbit_ir").ok_or_else(|| schema("pbit_ir", "missing \"pbit_ir\": 1"))?, "pbit_ir")?;
    if ver != 1.0 { return Err(value("pbit_ir", format!("unsupported pbit_ir version {ver} (this build reads 1)"))); }
    let values: Vec<String> = names(req(j, "values", "")?, "values")?.into_iter().map(str::to_string).collect(); let k = values.len();
    if k == 0 { return Err(value("values", "no values (the alphabet must not be empty)")); }
    if k > 65535 { return Err(limit("values", format!("{k} values; at most 65535"))); }
    let vindex: std::collections::HashMap<&str, usize> = values.iter().enumerate().map(|(q, v)| (v.as_str(), q)).collect();
    let vidx = |s: &str, path: &str| vindex.get(s).copied().ok_or_else(|| value(path, format!("unknown value {s}")));
    let vs = arr(req(j, "vars", "")?, "vars")?; let n = vs.len(); if n == 0 { return Err(value("vars", "no vars")); }
    // Ids are looked up in a hash index; `vars.contains` + linear `position` made parsing O(n^2) (8.3 s of an 8.4 s call
    // on 100,000 variables)
    let mut vars: Vec<String> = vec![]; let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::with_capacity(n);
    let (mut h, mut allowed, mut clamp) = (vec![0.0; n * k], vec![true; n * k], vec![None; n]);
    for (i, v) in vs.iter().enumerate() { let vp = ix("vars", i);
        fields(v, &vp, &["id", "h", "allowed", "forbid", "clamp"])?;
        let id = match opt(v, "id") { Some(x) => text(x, &at(&vp, "id"))?.to_string(), None => format!("x{i}") };
        if index.insert(id.clone(), i).is_some() { return Err(value(&at(&vp, "id"), format!("duplicate var id {id}"))); }
        if let Some(al) = opt(v, "allowed") { let ap = at(&vp, "allowed"); let al = names(al, &ap)?;
            if al.is_empty() { return Err(value(&ap, format!("var {id}: \"allowed\" is empty (a variable needs at least one value)"))); }
            for q in 0..k { allowed[i * k + q] = false; } for (q, a) in al.iter().enumerate() { allowed[i * k + vidx(a, &ix(&ap, q))?] = true; } }
        if let Some(fb) = opt(v, "forbid") { let fp = at(&vp, "forbid"); for (q, a) in names(fb, &fp)?.iter().enumerate() { allowed[i * k + vidx(a, &ix(&fp, q))?] = false; } }
        if let Some(hh) = opt(v, "h") { let hp = at(&vp, "h"); for (val, x) in keyed(hh, &hp)? { let xp = at(&hp, val);
            h[i * k + vidx(val, &xp)?] = weight(x, &xp)?; } }
        if let Some(c) = opt(v, "clamp") { let cp = at(&vp, "clamp"); let q = vidx(c.as_str().ok_or_else(|| schema(&cp, format!("var {id}: clamp must be a value name")))?, &cp)?;
            if !allowed[i * k + q] { return Err(value(&cp, format!("var {id}: clamp to a forbidden value"))); } clamp[i] = Some(q); }
        if !(0..k).any(|q| allowed[i * k + q]) { return Err(value(&vp, format!("var {id}: no allowed value"))); }
        vars.push(id);
    }
    let xidx = |s: &Json, path: &str| { let s = text(s, path)?; index.get(s).copied().ok_or_else(|| value(path, format!("unknown var {s}"))) };
    let mut pairs = vec![];
    if let Some(ps) = opt(j, "pairs") { for (q, p) in arr(ps, "pairs")?.iter().enumerate() { let pp = ix("pairs", q);
        fields(p, &pp, &["i", "j", "potts", "table"])?;
        let (a, b) = (xidx(req(p, "i", &pp)?, &at(&pp, "i"))?, xidx(req(p, "j", &pp)?, &at(&pp, "j"))?);
        if a == b { return Err(value(&at(&pp, "j"), "a pair needs two different variables")); }
        let c = match (opt(p, "potts"), opt(p, "table")) {
            (Some(w), None) => Coupling::Potts(weight(w, &at(&pp, "potts"))?),
            (None, Some(t)) => { let tp = at(&pp, "table"); let rows = arr(t, &tp)?; if rows.len() != k { return Err(schema(&tp, format!("table must be k x k = {k} x {k}"))); }
                let mut tab = Vec::with_capacity(k * k);
                for (r, row) in rows.iter().enumerate() { let rp = ix(&tp, r); let row = arr(row, &rp)?; if row.len() != k { return Err(schema(&rp, format!("table rows must have k = {k} entries"))); }
                    for (c, x) in row.iter().enumerate() { tab.push(weight(x, &ix(&rp, c))?); } }
                Coupling::Table(tab) }
            (Some(_), Some(_)) => return Err(schema(&pp, "a pair has \"potts\" or \"table\", not both")),
            (None, None) => return Err(schema(&pp, "pair needs \"potts\" or \"table\"")) };
        pairs.push(Pair { i: a, j: b, c });
    } }
    let mut caps = vec![];
    if let Some(cs) = opt(j, "caps") { for (q, c) in arr(cs, "caps")?.iter().enumerate() { let cp = ix("caps", q);
        fields(c, &cp, &["limit", "min", "value", "vars", "members"])?;
        // R19.5 (P2.1, IR v2 cardinality): "limit" = at most, "min" = at least (value form only), both = a range (exactly k: equal)
        let lim = match opt(c, "limit") { Some(x) => Some(count(x, &at(&cp, "limit"))?), None => None };
        let min = match opt(c, "min") { Some(x) => Some(count(x, &at(&cp, "min"))?), None => None };
        if lim.is_none() && min.is_none() { return Err(schema(&cp, "cap needs \"limit\" (at most) and/or \"min\" (at least)")); }
        let mut members = vec![];
        match (opt(c, "members"), opt(c, "value")) {
            (Some(ms), None) => { let mp = at(&cp, "members"); if opt(c, "vars").is_some() { return Err(schema(&at(&cp, "vars"), "\"vars\" goes with \"value\", not \"members\"")); }
                if min.is_some() { return Err(schema(&at(&cp, "min"), "\"min\" goes with \"value\" (at least min of the vars take it), not \"members\"")); }
                for (r, mb) in arr(ms, &mp)?.iter().enumerate() { let rp = ix(&mp, r); let q = mb.as_arr().filter(|q| q.len() == 2).ok_or_else(|| schema(&rp, "cap member must be [var, value]"))?;
                    let vp = ix(&rp, 1); members.push((xidx(&q[0], &ix(&rp, 0))?, vidx(text(&q[1], &vp)?, &vp)?)); } }
            (None, Some(val)) => { let vp = at(&cp, "value"); let qv = vidx(text(val, &vp)?, &vp)?;
                let over: Vec<usize> = match opt(c, "vars") { Some(vv) => { let xp = at(&cp, "vars"); names(vv, &xp)?.iter().enumerate()
                    .map(|(r, x)| index.get(*x).copied().ok_or_else(|| value(&ix(&xp, r), format!("unknown var {x}")))).collect::<Result<_, _>>()? }, None => (0..n).collect() };
                members.extend(over.into_iter().filter(|&i| allowed[i * k + qv]).map(|i| (i, qv)));
                // at least `min` of the M variables able to take the value <=> at most |M| - min of them take another value:
                // one more cap over their other allowed values (every tier, the sampler and the gate run it unchanged)
                if let Some(kmin) = min { let mut mi: Vec<usize> = members.iter().map(|&(i, _)| i).collect(); mi.sort_unstable(); mi.dedup();
                    if kmin > mi.len() { return Err(value(&at(&cp, "min"), format!("min {kmin} exceeds the {} variables that can take {} (no plan satisfies it)", mi.len(), values[qv]))); }
                    let mut others = vec![]; for &i in &mi { for w in 0..k { if w != qv && allowed[i * k + w] { others.push((i, w)); } } }
                    caps.push(Cap { weights: vec![], members: others, limit: mi.len() - kmin }); } }
            (Some(_), Some(_)) => return Err(schema(&cp, "a cap has \"members\" or \"value\", not both")),
            (None, None) => return Err(schema(&cp, "cap needs \"members\" or \"value\"")) }
        if let Some(lim) = lim { caps.push(Cap { weights: vec![], members, limit: lim }); }
    } }
    // R19.5 (P2.1, IR v2 constructs lowered to caps, so every tier runs them unchanged):
    // all_different {"vars": [ids]} = per value, at most one of those variables takes it;
    // implies {"if": {"var", "value"}, "then": {"var", "in": [values]}} = at most one of (x = a) and (y outside `in`) holds.
    if let Some(ad) = opt(j, "all_different") { for (q, c) in arr(ad, "all_different")?.iter().enumerate() { let cp = ix("all_different", q);
        fields(c, &cp, &["vars"])?; let xp = at(&cp, "vars"); let ns = names(req(c, "vars", &cp)?, &xp)?;
        let mut over = vec![]; for (r, x) in ns.iter().enumerate() { let i = index.get(*x).copied().ok_or_else(|| value(&ix(&xp, r), format!("unknown var {x}")))?;
            if over.contains(&i) { return Err(value(&ix(&xp, r), format!("duplicate var {x}"))); } over.push(i); }
        if over.len() < 2 { return Err(value(&xp, "all_different needs at least two variables")); }
        for v in 0..k { let mem: Vec<(usize, usize)> = over.iter().filter(|&&i| allowed[i * k + v]).map(|&i| (i, v)).collect(); if mem.len() >= 2 { caps.push(Cap { weights: vec![], members: mem, limit: 1 }); } } } }
    if let Some(im) = opt(j, "implies") { for (q, c) in arr(im, "implies")?.iter().enumerate() { let cp = ix("implies", q);
        fields(c, &cp, &["if", "then"])?; let (ip, tp) = (at(&cp, "if"), at(&cp, "then")); let (ci, ct) = (req(c, "if", &cp)?, req(c, "then", &cp)?);
        fields(ci, &ip, &["var", "value"])?; fields(ct, &tp, &["var", "in"])?;
        let x = xidx(req(ci, "var", &ip)?, &at(&ip, "var"))?; let vp = at(&ip, "value"); let a = vidx(text(req(ci, "value", &ip)?, &vp)?, &vp)?;
        let y = xidx(req(ct, "var", &tp)?, &at(&tp, "var"))?; if x == y { return Err(value(&at(&tp, "var"), "an implication needs two different variables")); }
        let sp = at(&tp, "in"); let mut ins = vec![false; k]; for (r, w) in names(req(ct, "in", &tp)?, &sp)?.iter().enumerate() { ins[vidx(w, &ix(&sp, r))?] = true; }
        let mut mem = vec![(x, a)]; for w in 0..k { if !ins[w] && allowed[y * k + w] { mem.push((y, w)); } }
        if allowed[x * k + a] && mem.len() >= 2 { caps.push(Cap { weights: vec![], members: mem, limit: 1 }); } } }
    // tables {"vars": [1-3 distinct ids], "forbid" | "allow": [[value per var], ...]} (hard): a forbidden tuple = a cap with
    // limit arity - 1 over its (var, value) pairs; `allow` forbids every other tuple of the vars' allowed values (<= 100,000)
    if let Some(ts) = opt(j, "tables") { for (q, c) in arr(ts, "tables")?.iter().enumerate() { let cp = ix("tables", q);
        fields(c, &cp, &["vars", "forbid", "allow"])?; let xp = at(&cp, "vars"); let ns = names(req(c, "vars", &cp)?, &xp)?;
        let mut over = vec![]; for (r, x) in ns.iter().enumerate() { let i = index.get(*x).copied().ok_or_else(|| value(&ix(&xp, r), format!("unknown var {x}")))?;
            if over.contains(&i) { return Err(value(&ix(&xp, r), format!("duplicate var {x}"))); } over.push(i); }
        if over.is_empty() || over.len() > 3 { return Err(value(&xp, "a table constrains 1 to 3 variables")); }
        let (key, tl) = match (opt(c, "forbid"), opt(c, "allow")) { (Some(t), None) => ("forbid", t), (None, Some(t)) => ("allow", t),
            (Some(_), Some(_)) => return Err(schema(&cp, "a table has \"forbid\" or \"allow\", not both")), (None, None) => return Err(schema(&cp, "table needs \"forbid\" or \"allow\"")) };
        let tp = at(&cp, key); let mut listed: Vec<Vec<usize>> = vec![];
        for (r, t) in arr(tl, &tp)?.iter().enumerate() { let rp = ix(&tp, r); let row = arr(t, &rp)?; // a tuple may repeat a value
            if row.len() != over.len() { return Err(schema(&rp, format!("a tuple needs one value per table var ({})", over.len()))); }
            listed.push(row.iter().enumerate().map(|(e, w)| { let ep = ix(&rp, e); vidx(text(w, &ep)?, &ep) }).collect::<Result<_, _>>()?); }
        let doms: Vec<Vec<usize>> = over.iter().map(|&i| (0..k).filter(|&v| allowed[i * k + v]).collect()).collect();
        let bad: Vec<Vec<usize>> = if key == "forbid" { listed } else {
            let total: usize = doms.iter().map(Vec::len).product(); if total > 100_000 { return Err(limit(&tp, format!("{total} tuples to enumerate; at most 100,000"))); }
            let mut all: Vec<Vec<usize>> = vec![vec![]]; for d in &doms { all = all.into_iter().flat_map(|t| d.iter().map(move |&v| { let mut u = t.clone(); u.push(v); u })).collect(); }
            let set: std::collections::HashSet<Vec<usize>> = listed.into_iter().collect(); all.into_iter().filter(|t| !set.contains(t)).collect() };
        for t in bad { if t.iter().zip(&over).all(|(&v, &i)| allowed[i * k + v]) {
            caps.push(Cap { weights: vec![], members: over.iter().zip(&t).map(|(&i, &v)| (i, v)).collect(), limit: over.len() - 1 }); } } } }
    // precedes {"before": x, "after": y, "gap": g (integer, default 1)}: the (job, slot) pattern, values read as ordered slots
    // (their order in `values`): slot(y) >= slot(x) + g; every violating (slot x, slot y) pair is forbidden (a cap, limit 1)
    if let Some(pr) = opt(j, "precedes") { for (q, c) in arr(pr, "precedes")?.iter().enumerate() { let cp = ix("precedes", q);
        fields(c, &cp, &["before", "after", "gap"])?;
        let (x, y) = (xidx(req(c, "before", &cp)?, &at(&cp, "before"))?, xidx(req(c, "after", &cp)?, &at(&cp, "after"))?);
        if x == y { return Err(value(&at(&cp, "after"), "a precedence needs two different variables")); }
        let g = match opt(c, "gap") { Some(v) => count(v, &at(&cp, "gap"))?, None => 1 };
        for a in 0..k { for b in 0..k { if b < a + g && allowed[x * k + a] && allowed[y * k + b] { caps.push(Cap { weights: vec![], members: vec![(x, a), (y, b)], limit: 1 }); } } } } }
    // R19.7 (P2.1) linear {"terms": [[var, value, weight], ...], "limit": L}: sum of weight x [var = value] <= L, weights whole
    // numbers 0..=1,000,000 (0, or a value the var cannot take: term dropped), each (var, value) once. One WEIGHTED cap: members
    // are never replicated, so every tier, the sampler and the gate read the weights (knapsack: one per budget; bin packing: one per bin)
    if let Some(ls) = opt(j, "linear") { for (q, c) in arr(ls, "linear")?.iter().enumerate() { let cp = ix("linear", q);
        fields(c, &cp, &["terms", "limit"])?; let lim = count(req(c, "limit", &cp)?, &at(&cp, "limit"))?;
        let tp = at(&cp, "terms"); let (mut members, mut weights, mut seen) = (vec![], vec![], std::collections::HashSet::new());
        for (r, t) in arr(req(c, "terms", &cp)?, &tp)?.iter().enumerate() { let rp = ix(&tp, r);
            let e = t.as_arr().filter(|e| e.len() == 3).ok_or_else(|| schema(&rp, "a term is [var, value, weight]"))?;
            let i = xidx(&e[0], &ix(&rp, 0))?; let vp = ix(&rp, 1); let v = vidx(text(&e[1], &vp)?, &vp)?;
            let wp = ix(&rp, 2); let w = count(&e[2], &wp)?; if w > MAX_CAP_WEIGHT { return Err(limit(&wp, format!("weight {w}; at most {MAX_CAP_WEIGHT}"))); }
            if !seen.insert((i, v)) { return Err(value(&rp, format!("duplicate term ({}, {})", vars[i], values[v]))); }
            if w > 0 && allowed[i * k + v] { members.push((i, v)); weights.push(w); } }
        if !members.is_empty() { caps.push(Cap { members, limit: lim, weights }); } } }
    // R19 P1.3(a), inference compiler: zero pairs and never-binding caps dropped, constant / separable tables folded into the
    // unaries (`pbit_ir::compile_parts`: no odds, log Z or plan changes; kept, they hid independence from the exact tiers)
    // R19.6 (P2.1) warm start {"var": "value", ...}: every variable once, each on an allowed (and clamped) value, every cap
    // (constructs included, before the compile pass) within its limit; else exit 2. Chain 0 starts from it (`Model::start`).
    let start = match opt(j, "start") { None => None, Some(st) => {
        let mut x = vec![usize::MAX; n];
        for (var, val) in keyed(st, "start")? { let vp = at("start", var);
            let i = *index.get(var).ok_or_else(|| value(&vp, format!("unknown var {var}")))?;
            let q = vidx(val.as_str().ok_or_else(|| schema(&vp, format!("the value of {var} must be a value name")))?, &vp)?;
            if !allowed[i * k + q] || clamp[i].is_some_and(|c| c != q) { return Err(value(&vp, format!("{var} = {} is not allowed", values[q]))); }
            x[i] = q; }
        if let Some(i) = x.iter().position(|&q| q == usize::MAX) { return Err(value("start", format!("var {} has no value (a warm start gives every variable)", vars[i]))); }
        for (c, cp) in caps.iter().enumerate() { let load: usize = cp.members.iter().enumerate().filter(|&(_, &(i, v))| x[i] == v).map(|(t, _)| cp.w(t)).sum();
            if load > cp.limit { return Err(value("start", format!("infeasible: lowered cap #{c} holds {load} > limit {}", cp.limit))); } }
        Some(x) } };
    let (pairs, caps, compiled) = compile_parts(k, &mut h, pairs, caps);
    let mut m = Model::new(n, k, h, allowed, clamp, pairs, caps).map_err(|e| value("", e))?; m.start = start;
    Ok(Prog { m, vars, values, compiled })
}

/// R19.6 (P2.1): `phases` on every `pbit run` answer, `[{"phase": name, "ms": wall}]` in order: parse (stdin JSON), compile
/// (validation, lowering, compile pass), exact (exact tiers; on a sampled answer the time before the sampler), sample (chains
/// incl. their start search), gate, polish (measured wall), total (parse + compile + the answer's `ms`). The run's internal
/// `exact_phase_ms` / `polish_wall_ms` telemetry entries are moved here.
pub fn phases(doc: &mut Json, parse: f64, compile: f64, deadline: Option<f64>) {
    let f = |o: Option<&Json>, k: &str| o.and_then(|o| o.get(k)).and_then(Json::as_f64);
    let total = f(Some(&*doc), "ms").unwrap_or(0.0); let t = doc.get("telemetry");
    let (exact, sample, gate, polish) = match f(t, "exact_phase_ms") { Some(e) => (e, f(t, "sample_ms").unwrap_or(0.0), f(t, "gate_ms").unwrap_or(0.0), f(t, "polish_wall_ms").unwrap_or(0.0)), None => (total, 0.0, 0.0, 0.0) };
    let Json::Obj(v) = doc else { return };
    if let Some((_, Json::Obj(t))) = v.iter_mut().find(|(k, _)| k == "telemetry") { t.retain(|(k, _)| k != "exact_phase_ms" && k != "polish_wall_ms"); }
    v.push(("phases".into(), Json::Arr([("parse", parse), ("compile", compile), ("exact", exact), ("sample", sample), ("gate", gate), ("polish", polish), ("total", parse + compile + total)]
        .iter().map(|&(p, x)| obj(vec![("phase", jstr(p)), ("ms", num(x))])).collect())));
    if let Some(d) = deadline { v.push(("deadline".into(), obj(vec![("ms", num(d)), ("met", Json::Bool(parse + compile + total <= d))]))); }
}

/// gate/2+ fields shared by `pbit run` and `pbit decide`: version, assumptions, both batch-means MCSEs (max over variables of
/// 0.5 x sum of per-value standard errors), the long pass's batch count, the largest finite per-variable indicator R-hat and the
/// count of variables whose R-hat is infinite (gate/3; finite by construction), and the worst variable's per-chain means and kept rows
/// (chains that agree on a wrong mode show here as equal means: the gate cannot see a mode no chain visits). Plus the per-item
/// `release_reason` object.
pub fn gate2(g: &Gate, s: &Samples, k: usize, ids: &[String], values: &[String]) -> (Vec<(&'static str, Json)>, Json) {
    let n = ids.len(); let w = g.worst_task.min(n - 1);
    let v = (0..k).max_by(|&a, &b| s.marg[w * k + a].total_cmp(&s.marg[w * k + b])).unwrap_or(0);
    let worst = obj(vec![("id", jstr(&ids[w])), ("value", jstr(&values[v])), ("pooled_mean", num(s.marg[w * k + v])),
        ("chain_means", Json::Arr(s.chain_marg.iter().map(|cm| num(cm[w * k + v])).collect())), ("chain_rows", Json::Arr(s.traj.iter().map(|tr| num((tr.len() / n) as f64)).collect()))]);
    let fields = vec![("version", jstr(GATE_VERSION)), ("assumptions", Json::Arr(GATE_ASSUMPTIONS.iter().map(|a| jstr(a)).collect())),
        ("mcse_tv_short", num(g.sig_tv_max)), ("mcse_tv_long", num(g.sig_tv_long.iter().cloned().fold(0.0, f64::max))), ("min_batches_long", num(g.min_batches_long as f64)),
        ("item_rhat_max", num(g.rhat_task.iter().cloned().filter(|r| r.is_finite()).fold(0.0, f64::max))), ("item_rhat_infinite", num(g.rhat_task.iter().filter(|r| !r.is_finite()).count() as f64)),
        ("occupancy_rhat_max", num(g.rhat_occ.iter().cloned().filter(|r| r.is_finite()).fold(0.0, f64::max))), ("occupancy_rhat_infinite", num(g.rhat_occ.iter().filter(|r| !r.is_finite()).count() as f64)), ("worst", worst)];
    // R19 P1.2: accepted collective moves (jumps between mirror / label-permuted modes), when the sampler tracked them per chain
    let mut fields = fields; if !s.moves.is_empty() && s.moves.len() == s.traj.len() {
        fields.push(("mode_transitions", obj(vec![("global_flips", num(s.moves.iter().map(|m| m[0]).sum::<u64>() as f64)), ("label_swaps", num(s.moves.iter().map(|m| m[1]).sum::<u64>() as f64)),
            ("chains_without", num(s.moves.iter().filter(|m| m[0] + m[1] == 0).count() as f64))]))); }
    (fields, Json::Obj(ids.iter().zip(g.release_reasons(&GATE)).map(|(id, r)| (id.clone(), jstr(r))).collect()))
}
/// The components / forest tier's answer (`exact_components_until`): tier `forest` when every component is a cap-free tree,
/// else `components`, with the per-tier component counts; exit 1 if one component has no feasible assignment.
fn comp_doc(p: &Prog, c: &CompExact, mut out: Vec<(&str, Json)>, t0: std::time::Instant) -> (Json, i32) {
    let m = &p.m; let ms = num(t0.elapsed().as_secs_f64() * 1e3);
    let parts = obj(vec![("count", num(c.components as f64)), ("forest", num(c.forest as f64)), ("occupancy", num(c.occupancy as f64)), ("enumerate", num(c.enumerated as f64)), ("frontier", num(c.frontier as f64))]);
    if c.infeasible { out.extend([("verdict", jstr("infeasible")), ("reason", jstr("no assignment satisfies every rule (one independent component has no feasible assignment)")), ("components", parts), ("ms", ms)]); return (obj(out), 1); }
    out.extend([("verdict", jstr("exact")), ("tier", jstr(if c.forest == c.components { "forest" } else if c.occupancy == c.components { "occupancy" } else { "components" })), ("plan", plan(p, &c.map)), ("plan_logw", num(c.map_logw)), ("violations", num(m.violations(&c.map) as f64)),
        ("marginals", marginals(p, &c.marg)), ("released", ids(p, &vec![true; m.n], true)), ("escalated", Json::Arr(vec![])), ("logz", num(c.logz)),
        ("top_plans", Json::Arr(vec![obj(vec![("p", num((c.map_logw - c.logz).exp())), ("plan", plan(p, &c.map))])])), ("components", parts), ("ms", ms)]);
    (obj(out), 0)
}
fn plan(p: &Prog, x: &[usize]) -> Json { Json::Obj(p.vars.iter().enumerate().map(|(i, id)| (id.clone(), jstr(&p.values[x[i]]))).collect()) }
fn marginals(p: &Prog, mg: &[f64]) -> Json {
    let k = p.m.k;
    Json::Obj(p.vars.iter().enumerate().map(|(i, id)| { let mut row: Vec<(usize, f64)> = (0..k).filter(|&q| p.m.allowed[i * k + q]).map(|q| (q, mg[i * k + q])).collect();
        row.sort_by(|u, v| v.1.partial_cmp(&u.1).unwrap()); (id.clone(), Json::Obj(row.into_iter().map(|(q, x)| (p.values[q].clone(), num(x))).collect())) }).collect())
}
fn ids(p: &Prog, mask: &[bool], want: bool) -> Json { Json::Arr(p.vars.iter().enumerate().filter(|(i, _)| mask[*i] == want).map(|(_, id)| jstr(id)).collect()) }

/// Runs the instruction; returns (decision document, exit code: 0 answer, 1 infeasible, 3 refused).
/// `sweeps > 0` = fixed work per chain instead of the wall-clock budget: with `polish_ms == 0` or `polish_sweeps > 0` the answer
/// is then a pure function of (program, seed) — byte-identical across runs and machines with the same float semantics.
/// `chains` / `threads`: resource controls (fixed sweeps => identical answer for any thread count).
/// `mem`: the memory cap (--mem-limit-mb, rows per chain) — see main.rs `mem_limit`.
#[allow(clippy::too_many_arguments)]
pub fn run(p: &Prog, op: &str, budget: f64, seed: u64, exact_limit: u64, polish_ms: f64, polish_sweeps: usize, sweeps: usize, fr_states: usize, chains: usize, threads: usize, cpu_pct: u32, mem: (usize, usize), exact_ms: Option<f64>, deadline: Option<(std::time::Instant, f64, bool)>) -> (Json, i32) {
    let m = &p.m; let t0 = std::time::Instant::now();
    // --exact-ms (opt-in) = a hard wall-clock stop for the exact tiers below, from t0 (see main.rs `exact_ms`)
    let space: f64 = (0..m.n).map(|i| m.cand_count(i) as f64).product();
    // R19.6: without --exact-ms, `decide` on a wall-clock budget stops the exact tiers before the sampler at --budget-ms when the
    // raw space exceeds --exact-limit (such an enumeration may count plans up to the limit and decline: ~8 s at a 200 ms budget
    // on a 20-job x 30-slot precedence program); past it they decline as at --exact-ms and the sampler gets its full budget.
    // Fixed work (--sweeps) and spaces within the limit are unchanged.
    let auto = deadline.is_none() && exact_ms.is_none() && op == "decide" && sweeps == 0 && space > exact_limit as f64;
    let hard = exact_ms.map(|x| t0 + std::time::Duration::from_secs_f64(x / 1e3)).or(auto.then(|| t0 + std::time::Duration::from_secs_f64(budget / 1e3)));
    // R19.6 (P2.1) `--deadline-ms D` (whole call, counted from before parsing; wall-clock sampling only): the exact tiers stop at
    // D / 4 under `decide` (D under `exact`), min'd with --exact-ms; then the sampler gets 0.6 and the polish at most 0.1 of the
    // time left (the sampler at most --budget-ms if that flag is given too); the rest is the gate's reserve, which is not
    // bounded: the answer reports `deadline.met`.
    let dl_end = deadline.map(|(t, d, _)| t + std::time::Duration::from_secs_f64(d / 1e3));
    let hard = match deadline { Some((t, d, _)) => { let stop = t + std::time::Duration::from_secs_f64(if op == "exact" { d } else { d / 4.0 } / 1e3); Some(hard.map_or(stop, |h| h.min(stop))) } None => hard };
    let mut out: Vec<(&str, Json)> = vec![("engine", jstr(&format!("pbit {}", env!("CARGO_PKG_VERSION")))), ("op", jstr(op)),
        ("program", obj(vec![("vars", num(m.n as f64)), ("values", num(m.k as f64)), ("pairs", num(m.pairs.len() as f64)), ("caps", num(m.caps.len() as f64))]))];
    out.push(("compiled", obj(vec![("pairs_dropped", num(p.compiled.pairs_dropped as f64)), ("caps_dropped", num(p.compiled.caps_dropped as f64)), ("tables_folded", num(p.compiled.tables_folded as f64))])));
    let ms = |t0: std::time::Instant| num(t0.elapsed().as_secs_f64() * 1e3);
    if op == "decide" || op == "exact" {
        let lim = if op == "exact" { u64::MAX / 128 } else { exact_limit };
        // When the raw space exceeds the enumeration limit, the frontier DP goes FIRST: enumeration would spend its
        // whole node budget before declining (measured 169 ms / 1.04 s on 80 / 300-var team rosters; frontier 0.9 / 56 ms)
        let fr = |first: bool| if op == "decide" && fr_states > 0 && (space > lim as f64) == first { exact_frontier_until(m, fr_states, hard) } else { None };
        // R19 P1.3(b, c), inference compiler: the connected components of pairs + caps solved one by one (cap-free trees by
        // sum-/max-product, the rest by enumeration, and under `decide` the frontier DP); log Z adds up. Tried before the
        // whole-program enumeration when the raw space exceeds the limit (that enumeration would decline), else after it.
        // `--frontier-states 0` switches it off under `decide` (the earlier path); `exact` uses it with enumeration only.
        let comp = || if op == "exact" || fr_states > 0 { exact_components_until(m, lim, if op == "exact" { 0 } else { fr_states }, hard) } else { None };
        // R19.5: over the limit the components tier goes BEFORE the frontier DP: its decline is cheap (union-find + count_spec;
        // one non-tree, non-occupancy component declines before any solve) and 100,000 independent two-value variables took
        // 4,430 ms in the whole-program frontier DP vs 29.2 ms as 100,000 components
        if space > lim as f64 { if let Some(c) = comp() { return comp_doc(p, &c, out, t0); } }
        let mut f = fr(true);
        if f.is_none() {
            if let Some(e) = exact_within(m, 5, lim, None, hard).0 {
                if e.n_feasible == 0 { out.extend([("verdict", jstr("infeasible")), ("reason", jstr("no assignment satisfies every rule")), ("ms", ms(t0))]); return (obj(out), 1); }
                let best = &e.top[0].1;
                out.extend([("verdict", jstr("exact")), ("tier", jstr("enumerate")), ("plan", plan(p, best)), ("plan_logw", num(m.logw(best))), ("violations", num(m.violations(best) as f64)),
                    ("marginals", marginals(p, &e.marg)), ("released", ids(p, &vec![true; m.n], true)), ("escalated", Json::Arr(vec![])), ("n_feasible", num(e.n_feasible as f64)), ("logz", num(e.logz)),
                    ("top_plans", Json::Arr(e.top.iter().map(|(pr, x)| obj(vec![("p", num(*pr)), ("plan", plan(p, x))])).collect())), ("ms", ms(t0))]);
                return (obj(out), 0);
            }
            if op == "exact" { if space <= lim as f64 { if let Some(c) = comp() { return comp_doc(p, &c, out, t0); } }
                let why = if hard.is_some_and(|h| std::time::Instant::now() >= h) { "enumeration stopped at --exact-ms; raise it, or use --op decide or sample" } else { "enumeration node budget exhausted; use --op decide or sample" };
                out.extend([("verdict", jstr("declined")), ("reason", jstr(why)), ("ms", ms(t0))]); return (obj(out), 3); }
            f = fr(false);
        }
        // Frontier DP (partition programs whose components share caps thinly): exact odds + an exact MAP plan
        if let Some(f) = f {
            out.extend([("verdict", jstr("exact")), ("tier", jstr("frontier")), ("plan", plan(p, &f.map)), ("plan_logw", num(f.map_logw)), ("violations", num(m.violations(&f.map) as f64)),
                ("marginals", marginals(p, &f.marg)), ("released", ids(p, &vec![true; m.n], true)), ("escalated", Json::Arr(vec![])), ("logz", num(f.logz)),
                ("top_plans", Json::Arr(vec![obj(vec![("p", num((f.map_logw - f.logz).exp())), ("plan", plan(p, &f.map))])])), ("frontier_states", num(f.max_states as f64)), ("ms", ms(t0))]);
            return (obj(out), 0);
        }
        if space <= lim as f64 { if let Some(c) = comp() { return comp_doc(p, &c, out, t0); } } // enumeration declined on its node budget
    }
    // Only a run whose exact tiers could run can have hit the cap (was also true under --op sample)
    let ts = std::time::Instant::now(); let reached = op != "sample" && hard.is_some_and(|h| ts >= h);
    let (budget, polish_ms) = match (deadline, dl_end) { (Some((_, _, explicit)), Some(end)) => {
        let left = end.saturating_duration_since(ts).as_secs_f64() * 1e3;
        if left <= 0.0 { out.extend([("verdict", jstr("refused")), ("reason", jstr("--deadline-ms passed before the sampler could start; raise it")), ("ms", ms(t0))]); return (obj(out), 3); }
        (if explicit { budget.min(0.6 * left) } else { 0.6 * left }, polish_ms.min(0.1 * left)) }
        _ => (budget, polish_ms) };
    // Under `decide` (wall-clock budget, non-partition program) the chains' feasible-start search stops at HALF of the
    // budget, and if a chain finds no start the exact search runs again, past its gap budget, to the end of --budget-ms: its
    // first plan is also the proof of infeasibility. An earlier version gave the exact tier half the budget BEFORE sampling, which
    // cost feasible programs sampler time (200-job schedules at 5 s: 174 -> 97 and 157 -> 0 released). `--sweeps N`: no fallback.
    let fallback = op == "decide" && sweeps == 0 && !m.is_partition();
    let s = match sample_on_starting(m, chains, threads, sweeps, if sweeps > 0 { None } else { Some(budget) }, seed, false, cpu_pct, mem.1, if fallback { 0.5 } else { 1.0 }) { Some(s) => s, None => {
        // Matching (quota-shaped programs) proves infeasibility; the bounded search for other programs does not
        if m.is_partition() { out.extend([("verdict", jstr("infeasible")), ("reason", jstr("no assignment satisfies every rule (capacitated matching)")), ("ms", ms(t0))]); return (obj(out), 1); }
        // R19.6: the fallback stops HARD at the end of the sampling phase's budget, or half a budget after it starts if that is
        // later (it used to be a soft deadline from the start of the call, consulted only between plans: a program with many plans
        // ran on to --exact-limit, ~7 s past a 200 ms budget). The half budget: on a loaded machine 1,024 chains' wall-clock slices
        // overran the 4 s phase, the fallback started past its end and refused a program it enumerates in ~0.3 s.
        let end = (ts + std::time::Duration::from_secs_f64(budget / 1e3)).max(std::time::Instant::now() + std::time::Duration::from_secs_f64(budget / 2e3));
        if fallback { if let (Some(e), _) = exact_within(m, 5, exact_limit, Some(end), Some(end)) {
            if e.n_feasible == 0 { out.extend([("verdict", jstr("infeasible")), ("reason", jstr("no assignment satisfies every rule")), ("ms", ms(t0))]); return (obj(out), 1); }
            let best = &e.top[0].1; // a feasible program no chain could start in time, enumerated within the budget
            out.extend([("verdict", jstr("exact")), ("tier", jstr("enumerate")), ("plan", plan(p, best)), ("plan_logw", num(m.logw(best))), ("violations", num(m.violations(best) as f64)),
                ("marginals", marginals(p, &e.marg)), ("released", ids(p, &vec![true; m.n], true)), ("escalated", Json::Arr(vec![])), ("n_feasible", num(e.n_feasible as f64)), ("logz", num(e.logz)),
                ("top_plans", Json::Arr(e.top.iter().map(|(pr, x)| obj(vec![("p", num(*pr)), ("plan", plan(p, x))])).collect())), ("ms", ms(t0))]);
            return (obj(out), 0); } }
        // The hint depends on the op. Under `decide` the bounded exact tier has already given up (now after
        // max(64 x limit, 1e8) checks without a plan), so the proof of infeasibility an earlier binary gave here in 0.13-4.3 s on
        // hard 3-colourings now needs the unbounded `--op exact`
        let hint = if op == "decide" { "no feasible start found within the search budget, and the bounded exact search (run again to the end of --budget-ms) found no plan (not a proof of infeasibility); --op exact searches exhaustively and can prove infeasibility, or raise --budget-ms" }
            else { "no feasible start found within the search budget (not a proof of infeasibility); try --op decide" };
        out.extend([("verdict", jstr("refused")), ("reason", jstr(hint)), ("ms", ms(t0))]); return (obj(out), 3); } };
    let sample_s = ts.elapsed().as_secs_f64(); let tg = std::time::Instant::now();
    let g = gate_stats(m, &s); let gate_ms = tg.elapsed().as_secs_f64() * 1e3; let rel = g.released_tasks(&GATE); let whole = g.diagnostics_passed(&GATE);
    let nrel = rel.iter().filter(|&&r| r).count(); let verdict = if whole { "diagnostics_passed" } else if nrel > 0 { "partial" } else { "refused" };
    let mask: Vec<bool> = if whole { vec![true; m.n] } else { rel };
    // plan polish (as `pbit decide`): anneal the chains' best plan through beta 2 -> 32; never worse than that plan
    // --polish-sweeps N = fixed-work polish (deterministic); --polish-ms is wall-clock, so its plan can vary run to run
    // The polish runs on --threads workers (it used 4 threads whatever --threads said)
    let tpol = std::time::Instant::now();
    let (plw, px) = if polish_sweeps > 0 || polish_ms > 0.0 { anneal_on(m, Some(&s.best.1), &[2.0, 4.0, 8.0, 16.0, 32.0], if polish_sweeps > 0 { 0.0 } else { polish_ms }, polish_sweeps, 4, threads, seed).unwrap_or((s.best.0, s.best.1.clone())) }
        else { (s.best.0, s.best.1.clone()) };
    let polish_wall = tpol.elapsed().as_secs_f64() * 1e3;
    let (g2, reasons) = gate2(&g, &s, m.k, &p.vars, &p.values);
    out.extend([("verdict", jstr(verdict)), ("tier", jstr("sample")), ("plan", plan(p, &px)), ("plan_logw", num(plw)), ("sampled_best_logw", num(s.best.0)), ("violations", num(m.violations(&px) as f64)),
        ("marginals", marginals(p, &s.marg)), ("released", ids(p, &mask, true)), ("escalated", ids(p, &mask, false)), ("release_reason", reasons),
        ("gate", obj([vec![("rhat", num(g.rhat)), ("tv_bound", num(g.tv_bound(&GATE))), ("tv_tol", num(GATE.tv_tol)), ("frozen_saturated_caps", num(g.frozen as f64)), ("frozen_escalated", num(g.escalate.iter().filter(|&&e| e).count() as f64)), ("min_batches", num(g.min_batches as f64)),
            ("samples", num(s.n as f64)), ("sweeps", num(s.sweeps as f64)), ("chains", num(chains as f64)), ("budget_ms", num(budget)), ("seed", num(seed as f64)), ("collective", Json::Bool(m.collective)), ("cluster", Json::Bool(m.cluster)), ("cycles", Json::Bool(m.cycles))], g2].concat())),
        ("telemetry", obj([vec![("chains", num(chains as f64)), ("threads", num(threads.clamp(1, chains) as f64)), ("sweeps", num(s.sweeps as f64)), ("site_updates_per_s", num(if sample_s > 0.0 { (s.sweeps as f64 * m.n as f64 / sample_s).round() } else { 0.0 })),
            ("sample_ms", num(sample_s * 1e3)), ("gate_ms", num(gate_ms)), ("polish_ms", num(if polish_sweeps > 0 { 0.0 } else { polish_ms })), ("polish_sweeps", num(polish_sweeps as f64)),
            ("process_cpu_ms", crate::sys::usage().map_or(Json::Null, |u| num(u.0))), ("peak_rss_mb", crate::sys::usage().map_or(Json::Null, |u| num(u.1))), ("nice", crate::sys::nice().map_or(Json::Null, |v| num(v as f64))), ("cpu_limit_pct", num(cpu_pct as f64)),
            ("mem_limit_mb", num(mem.0 as f64)), ("traj_rows", num(s.traj.iter().map(|tr| tr.len() / m.n).sum::<usize>() as f64))], crate::exact_cap(exact_ms, reached),
            if auto && reached { vec![("exact_budget_reached", Json::Bool(true))] } else { vec![] },
            vec![("exact_phase_ms", num((ts - t0).as_secs_f64() * 1e3)), ("polish_wall_ms", num(polish_wall))]].concat())), ("ms", ms(t0))]);
    let code = if verdict == "refused" { 3 } else { 0 };
    (obj(out), code)
}
