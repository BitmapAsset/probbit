//! `pbit run`: execute a general pbit-ir program (JSON wire format v1, docs/pbit-ir-json.md) on the virtual p-bit processor.
//! Instructions (`--op`): `decide` (default: exact enumeration if the feasible set is <= --exact-limit, else the exact
//! frontier DP if the program allows it (--frontier-states, 0 = off), else 4 gated chains),
//! `exact` (enumeration only), `sample` (chains + gate, never enumerate). What-if = `"clamp"` on a variable.
use crate::json::{num, obj, str as jstr, Json};
use pbit_ir::*;

pub struct Prog { pub m: Model, pub vars: Vec<String>, pub values: Vec<String> }

fn names(j: Option<&Json>, what: &str) -> Result<Vec<String>, String> {
    j.and_then(Json::as_arr).ok_or(format!("{what} must be an array of strings"))?.iter().map(|v| v.as_str().map(str::to_string).ok_or(format!("{what} must be strings"))).collect()
}

/// JSON (pbit_ir 1) -> Model. See docs/pbit-ir-json.md.
pub fn from_json(j: &Json) -> Result<Prog, String> {
    if j.get("pbit_ir").and_then(Json::as_f64) != Some(1.0) { return Err("missing \"pbit_ir\": 1".into()); }
    let values = names(j.get("values"), "\"values\"")?; let k = values.len(); if k == 0 { return Err("no values".into()); }
    let vidx = |s: &str| values.iter().position(|v| v == s).ok_or(format!("unknown value {s}"));
    let vs = j.get("vars").and_then(Json::as_arr).ok_or("missing \"vars\" array")?; let n = vs.len(); if n == 0 { return Err("no vars".into()); }
    // Ids are looked up in a hash index; `vars.contains` + linear `position` made parsing O(n^2) (8.3 s of an 8.4 s call
    // on 100,000 variables)
    let mut vars: Vec<String> = vec![]; let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::with_capacity(n);
    let (mut h, mut allowed, mut clamp) = (vec![0.0; n * k], vec![true; n * k], vec![None; n]);
    for (i, v) in vs.iter().enumerate() {
        let id = v.get("id").and_then(Json::as_str).map(str::to_string).unwrap_or_else(|| format!("x{i}"));
        if index.insert(id.clone(), i).is_some() { return Err(format!("duplicate var id {id}")); }
        if let Some(al) = v.get("allowed").filter(|a| !a.is_null()) { for q in 0..k { allowed[i * k + q] = false; } for a in names(Some(al), "\"allowed\"")? { allowed[i * k + vidx(&a)?] = true; } }
        if let Some(fb) = v.get("forbid").filter(|a| !a.is_null()) { for a in names(Some(fb), "\"forbid\"")? { allowed[i * k + vidx(&a)?] = false; } }
        if let Some(hh) = v.get("h").filter(|a| !a.is_null()) { for (val, x) in hh.as_obj().ok_or(format!("var {id}: \"h\" must be an object"))? {
            h[i * k + vidx(val)?] = x.as_f64().ok_or(format!("var {id}: h[{val}] is not a number"))?; } }
        if let Some(c) = v.get("clamp").filter(|a| !a.is_null()) { let q = vidx(c.as_str().ok_or(format!("var {id}: clamp must be a value"))?)?;
            if !allowed[i * k + q] { return Err(format!("var {id}: clamp to a forbidden value")); } clamp[i] = Some(q); }
        if !(0..k).any(|q| allowed[i * k + q]) { return Err(format!("var {id}: no allowed value")); }
        vars.push(id);
    }
    let xidx = |s: &Json| s.as_str().and_then(|s| index.get(s).copied()).ok_or(format!("unknown var {s:?}"));
    let mut pairs = vec![];
    for p in j.get("pairs").and_then(Json::as_arr).unwrap_or(&[]) {
        let (a, b) = (xidx(p.get("i").ok_or("pair without \"i\"")?)?, xidx(p.get("j").ok_or("pair without \"j\"")?)?);
        let c = if let Some(w) = p.get("potts").and_then(Json::as_f64) { Coupling::Potts(w) } else if let Some(t) = p.get("table").and_then(Json::as_arr) {
            let mut tab = vec![]; for row in t { let r = row.as_arr().ok_or("table rows must be arrays")?; if r.len() != k { return Err("table rows must have k entries".into()); }
                for x in r { tab.push(x.as_f64().ok_or("table entries must be numbers")?); } }
            if tab.len() != k * k { return Err("table must be k x k".into()); } Coupling::Table(tab) } else { return Err("pair needs \"potts\" or \"table\"".into()) };
        pairs.push(Pair { i: a, j: b, c });
    }
    let mut caps = vec![];
    for c in j.get("caps").and_then(Json::as_arr).unwrap_or(&[]) {
        let lim = c.get("limit").and_then(Json::as_f64).ok_or("cap without \"limit\"")?; if lim < 0.0 || lim.fract() != 0.0 { return Err("cap limit must be a non-negative integer".into()); }
        let mut members = vec![];
        if let Some(ms) = c.get("members").and_then(Json::as_arr) { for mb in ms { let q = mb.as_arr().filter(|q| q.len() == 2).ok_or("cap member must be [var, value]")?;
            members.push((xidx(&q[0])?, vidx(q[1].as_str().ok_or("cap member value must be a string")?)?)); } }
        else if let Some(val) = c.get("value").and_then(Json::as_str) { let q = vidx(val)?;
            let over: Vec<usize> = match c.get("vars") { Some(vv) if !vv.is_null() => vv.as_arr().ok_or("cap \"vars\" must be an array")?.iter().map(|x| xidx(x)).collect::<Result<_, _>>()?, _ => (0..n).collect() };
            members.extend(over.into_iter().filter(|&i| allowed[i * k + q]).map(|i| (i, q))); }
        else { return Err("cap needs \"members\" or \"value\"".into()); }
        caps.push(Cap { members, limit: lim as usize });
    }
    Ok(Prog { m: Model::new(n, k, h, allowed, clamp, pairs, caps)?, vars, values })
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
pub fn run(p: &Prog, op: &str, budget: f64, seed: u64, exact_limit: u64, polish_ms: f64, polish_sweeps: usize, sweeps: usize, fr_states: usize, chains: usize, threads: usize, cpu_pct: u32, mem: (usize, usize), exact_ms: Option<f64>) -> (Json, i32) {
    let m = &p.m; let t0 = std::time::Instant::now();
    // --exact-ms (opt-in) = a hard wall-clock stop for the exact tiers below, from t0 (see main.rs `exact_ms`)
    let hard = exact_ms.map(|x| t0 + std::time::Duration::from_secs_f64(x / 1e3));
    let mut out: Vec<(&str, Json)> = vec![("engine", jstr(&format!("pbit {}", env!("CARGO_PKG_VERSION")))), ("op", jstr(op)),
        ("program", obj(vec![("vars", num(m.n as f64)), ("values", num(m.k as f64)), ("pairs", num(m.pairs.len() as f64)), ("caps", num(m.caps.len() as f64))]))];
    let ms = |t0: std::time::Instant| num(t0.elapsed().as_secs_f64() * 1e3);
    if op == "decide" || op == "exact" {
        let lim = if op == "exact" { u64::MAX / 128 } else { exact_limit };
        // When the raw space exceeds the enumeration limit, the frontier DP goes FIRST: enumeration would spend its
        // whole node budget before declining (measured 169 ms / 1.04 s on 80 / 300-var team rosters; frontier 0.9 / 56 ms)
        let space: f64 = (0..m.n).map(|i| m.cand_count(i) as f64).product();
        let fr = |first: bool| if op == "decide" && fr_states > 0 && (space > lim as f64) == first { exact_frontier_until(m, fr_states, hard) } else { None };
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
            if op == "exact" { let why = if hard.is_some_and(|h| std::time::Instant::now() >= h) { "enumeration stopped at --exact-ms; raise it, or use --op decide or sample" } else { "enumeration node budget exhausted; use --op decide or sample" };
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
    }
    // Only a run whose exact tiers could run can have hit the cap (was also true under --op sample)
    let ts = std::time::Instant::now(); let reached = op != "sample" && hard.is_some_and(|h| ts >= h);
    // Under `decide` (wall-clock budget, non-partition program) the chains' feasible-start search stops at HALF of the
    // budget, and if a chain finds no start the exact search runs again, past its gap budget, to the end of --budget-ms: its
    // first plan is also the proof of infeasibility. An earlier version gave the exact tier half the budget BEFORE sampling, which
    // cost feasible programs sampler time (200-job schedules at 5 s: 174 -> 97 and 157 -> 0 released). `--sweeps N`: no fallback.
    let fallback = op == "decide" && sweeps == 0 && !m.is_partition();
    let s = match sample_on_starting(m, chains, threads, sweeps, if sweeps > 0 { None } else { Some(budget) }, seed, false, cpu_pct, mem.1, if fallback { 0.5 } else { 1.0 }) { Some(s) => s, None => {
        // Matching (quota-shaped programs) proves infeasibility; the bounded search for other programs does not
        if m.is_partition() { out.extend([("verdict", jstr("infeasible")), ("reason", jstr("no assignment satisfies every rule (capacitated matching)")), ("ms", ms(t0))]); return (obj(out), 1); }
        if fallback { if let (Some(e), _) = exact_until(m, 5, exact_limit, Some(t0 + std::time::Duration::from_secs_f64(budget / 1e3))) {
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
    let g = gate_stats(m, &s); let gate_ms = tg.elapsed().as_secs_f64() * 1e3; let rel = g.certified_tasks(&GATE); let whole = g.certified(&GATE);
    let nrel = rel.iter().filter(|&&r| r).count(); let verdict = if whole { "certified" } else if nrel > 0 { "partial" } else { "refused" };
    let mask: Vec<bool> = if whole { vec![true; m.n] } else { rel };
    // plan polish (as `pbit decide`): anneal the chains' best plan through beta 2 -> 32; never worse than that plan
    // --polish-sweeps N = fixed-work polish (deterministic); --polish-ms is wall-clock, so its plan can vary run to run
    // The polish runs on --threads workers (it used 4 threads whatever --threads said)
    let (plw, px) = if polish_sweeps > 0 || polish_ms > 0.0 { anneal_on(m, Some(&s.best.1), &[2.0, 4.0, 8.0, 16.0, 32.0], if polish_sweeps > 0 { 0.0 } else { polish_ms }, polish_sweeps, 4, threads, seed).unwrap_or((s.best.0, s.best.1.clone())) }
        else { (s.best.0, s.best.1.clone()) };
    out.extend([("verdict", jstr(verdict)), ("tier", jstr("sample")), ("plan", plan(p, &px)), ("plan_logw", num(plw)), ("sampled_best_logw", num(s.best.0)), ("violations", num(m.violations(&px) as f64)),
        ("marginals", marginals(p, &s.marg)), ("released", ids(p, &mask, true)), ("escalated", ids(p, &mask, false)),
        ("gate", obj(vec![("rhat", num(g.rhat)), ("tv_bound", num(g.tv_bound(&GATE))), ("tv_tol", num(GATE.tv_tol)), ("frozen_saturated_caps", num(g.frozen as f64)), ("frozen_escalated", num(g.escalate.iter().filter(|&&e| e).count() as f64)), ("min_batches", num(g.min_batches as f64)),
            ("samples", num(s.n as f64)), ("sweeps", num(s.sweeps as f64)), ("chains", num(chains as f64)), ("budget_ms", num(budget)), ("seed", num(seed as f64))])),
        ("telemetry", obj([vec![("chains", num(chains as f64)), ("threads", num(threads.clamp(1, chains) as f64)), ("sweeps", num(s.sweeps as f64)), ("site_updates_per_s", num((s.sweeps as f64 * m.n as f64 / sample_s).round())),
            ("sample_ms", num(sample_s * 1e3)), ("gate_ms", num(gate_ms)), ("polish_ms", num(if polish_sweeps > 0 { 0.0 } else { polish_ms })), ("polish_sweeps", num(polish_sweeps as f64)),
            ("process_cpu_ms", crate::sys::usage().map_or(Json::Null, |u| num(u.0))), ("peak_rss_mb", crate::sys::usage().map_or(Json::Null, |u| num(u.1))), ("nice", crate::sys::nice().map_or(Json::Null, |v| num(v as f64))), ("cpu_limit_pct", num(cpu_pct as f64)),
            ("mem_limit_mb", num(mem.0 as f64)), ("traj_rows", num(s.traj.iter().map(|tr| tr.len() / m.n).sum::<usize>() as f64))], crate::exact_cap(exact_ms, reached)].concat())), ("ms", ms(t0))]);
    let code = if verdict == "refused" { 3 } else { 0 };
    (obj(out), code)
}
