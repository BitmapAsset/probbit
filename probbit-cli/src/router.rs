//! `probbit decide`, the router front-end: the router document -> `probbit_decide::Problem` (the strict input contract), and the decision
//! pipeline (the exact tiers, then the gated sampler and the polish) as one function that returns the document and its exit code.
//! main.rs parses the flags and prints it; probbit-wasm runs the same code in a browser.
use crate::json::{self, num, obj, str as jstr, Json};
use crate::{config_echo, exact_cap, sys, tui};
use probbit_core::rt::Instant;
use probbit_decide::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Named { pub(crate) p: Problem, pub(crate) tasks: Vec<String>, pub(crate) workers: Vec<String> }

/// JSON problem -> Problem. See README "Problem format"; the strict contract (types, unknown fields, duplicates, empty domains,
/// structured errors) is docs/probbit-ir-json.md "Input contract".
pub(crate) fn from_json(j: &Json) -> Result<Named, json::InErr> {
    use json::{arr, at, fields, ix, limit, opt, req, schema, text, value, weight};
    fields(j, "", &["workers", "tasks", "affinity", "comment"])?;
    if let Some(c) = opt(j, "comment") { text(c, "comment")?; }
    let ws = arr(req(j, "workers", "")?, "workers")?;
    // Workers are the lowered program's values, at most 65,535 (`probbit_ir::Model::new`, as `probbit run`'s `values`): 65,536 workers
    // aborted `probbit decide` in the lowering (exit 134, empty stdout)
    if ws.len() > 65535 { return Err(limit("workers", format!("{} workers; at most 65535", ws.len()))); }
    // Ids go through hash indexes (worker / task duplicate checks, worker and group lookups were linear scans: O(n^2))
    use std::collections::{HashMap, HashSet};
    let mut workers = vec![]; let mut cap = vec![]; let mut windex: HashMap<String, usize> = HashMap::new();
    for (wi, w) in ws.iter().enumerate() { let wp = ix("workers", wi); fields(w, &wp, &["id", "cap", "capacity"])?;
        let id = text(req(w, "id", &wp)?, &at(&wp, "id"))?;
        if opt(w, "cap").is_some() && opt(w, "capacity").is_some() { return Err(schema(&wp, format!("worker {id}: give \"cap\" or \"capacity\", not both"))); }
        let ck = if opt(w, "capacity").is_some() { "capacity" } else { "cap" };
        let c = json::count(opt(w, ck).ok_or_else(|| schema(&at(&wp, "cap"), format!("worker {id}: missing \"cap\"")))?, &at(&wp, ck))?;
        if windex.insert(id.to_string(), workers.len()).is_some() { return Err(value(&at(&wp, "id"), format!("duplicate worker id {id}"))); }
        workers.push(id.to_string()); cap.push(c); }
    let a = workers.len(); if a == 0 { return Err(value("workers", "no workers")); }
    let widx = |id: &str| windex.get(id).copied();
    let ts = arr(req(j, "tasks", "")?, "tasks")?;
    let t = ts.len(); if t == 0 { return Err(value("tasks", "no tasks")); }
    // Dense (task, worker) tables (as `probbit run`'s n x k): a small document must not ask for gigabytes
    if t.saturating_mul(a) > json::MAX_DENSE { return Err(limit("tasks", format!("{t} tasks x {a} workers = {} (task, worker) pairs; at most {}", t * a, json::MAX_DENSE))); }
    let (mut h, mut allowed, mut group, mut clamp, mut tasks) = (vec![f64::NEG_INFINITY; t * a], vec![false; t * a], vec![usize::MAX; t], vec![None; t], vec![]);
    let mut gids: Vec<String> = vec![]; let mut gindex: HashMap<String, usize> = HashMap::new(); let mut tseen: HashSet<String> = HashSet::with_capacity(t);
    for (i, tk) in ts.iter().enumerate() { let tp = ix("tasks", i);
        fields(tk, &tp, &["id", "scores", "allowed", "group", "clamp", "text"])?;
        if let Some(x) = opt(tk, "text") { text(x, &at(&tp, "text"))?; }
        let id = match opt(tk, "id") { Some(x) => text(x, &at(&tp, "id"))?.to_string(), None => format!("task{i}") };
        if !tseen.insert(id.clone()) { return Err(value(&at(&tp, "id"), format!("duplicate task id {id}"))); }
        let sp = at(&tp, "scores");
        let sc = json::keyed(opt(tk, "scores").ok_or_else(|| schema(&sp, format!("task {id}: missing \"scores\" object")))?, &sp)?;
        // `allowed` must be an array of distinct worker ids: `"allowed": "A"` (a string) used to be ignored, enabling every scored worker
        let ap = at(&tp, "allowed");
        let allow: Option<Vec<&str>> = opt(tk, "allowed").map(|v| json::names(v, &ap)).transpose()?;
        if allow.as_ref().is_some_and(|al| al.is_empty()) { return Err(value(&ap, format!("task {id}: \"allowed\" is empty (a task needs at least one worker)"))); }
        for (wid, v) in sc { let k = widx(wid).ok_or_else(|| value(&at(&sp, wid), format!("task {id}: unknown worker {wid} in scores")))?;
            let x = weight(v, &at(&sp, wid))?;
            if allow.as_ref().map_or(true, |al| al.contains(&wid.as_str())) { h[i * a + k] = x; allowed[i * a + k] = true; } }
        if let Some(al) = &allow { for (q, wid) in al.iter().enumerate() { let k = widx(wid).ok_or_else(|| value(&ix(&ap, q), format!("task {id}: unknown worker {wid} in allowed")))?;
            if !allowed[i * a + k] { h[i * a + k] = 0.0; allowed[i * a + k] = true; } } }
        if !(0..a).any(|k| allowed[i * a + k]) { return Err(value(&tp, format!("task {id}: no allowed worker with a score"))); }
        if let Some(g) = opt(tk, "group") { let gs = match g { Json::Str(s) => s.clone(), Json::Num(x) => format!("{x}"), _ => return Err(schema(&at(&tp, "group"), format!("task {id}: group must be a string or number"))) };
            group[i] = match gindex.get(&gs) { Some(&k) => k, None => { gindex.insert(gs.clone(), gids.len()); gids.push(gs); gids.len() - 1 } }; }
        if let Some(c) = opt(tk, "clamp") { let cp = at(&tp, "clamp"); let cid = c.as_str().ok_or_else(|| schema(&cp, format!("task {id}: clamp must be a worker id")))?;
            let k = widx(cid).ok_or_else(|| value(&cp, format!("task {id}: unknown clamp worker {cid}")))?;
            if !allowed[i * a + k] { return Err(value(&cp, format!("task {id}: clamp to {cid} but {cid} is not allowed"))); } clamp[i] = Some(k); }
        tasks.push(id);
    }
    for v in h.iter_mut() { if *v == f64::NEG_INFINITY { *v = 0.0; } } // disallowed entries never read; keep finite
    let lam = opt(j, "affinity").map(|x| weight(x, "affinity")).transpose()?.unwrap_or(0.0);
    if lam < 0.0 { return Err(value("affinity", "affinity must be >= 0")); }
    Ok(Named { p: Problem { t, a, h, allowed, cap, group, lam, clamp, block_moves: false, pair_swaps: false, collective: false, cluster: false, cycles: false }, tasks, workers })
}

fn odds(n: &Named, marg: &[f64]) -> Json {
    Json::Obj(n.tasks.iter().enumerate().map(|(i, id)| { let mut row: Vec<(usize, f64)> = (0..n.p.a).filter(|&k| n.p.allowed[i * n.p.a + k]).map(|k| (k, marg[i * n.p.a + k])).collect();
        row.sort_by(|u, v| v.1.partial_cmp(&u.1).unwrap());
        (id.clone(), Json::Obj(row.into_iter().map(|(k, m)| (n.workers[k].clone(), num(m))).collect())) }).collect())
}
fn plan(n: &Named, x: &[usize]) -> Json { Json::Obj(n.tasks.iter().enumerate().map(|(i, id)| (id.clone(), jstr(&n.workers[x[i]]))).collect()) }
fn ids(n: &Named, mask: &[bool], want: bool) -> Json { Json::Arr(n.tasks.iter().enumerate().filter(|(i, _)| mask[*i] == want).map(|(_, id)| jstr(id)).collect()) }

/// R19 P1.3(b-d), the inference compiler's components tier on the lowered router program (`probbit decide`, mode auto,
/// `--frontier-states` > 0): independent groups solved one by one (two-worker groups with uniform affinity and whole-worker
/// caps by the occupancy-count DP, cap-free trees by sum-/max-product, the rest by enumeration / the frontier DP). Returns the
/// exact (or infeasible) answer and its exit code; None if the tier declines.
fn components_tier(n: &Named, lowered: &probbit_ir::Model, exact_limit: u64, fr_states: usize, hard: Option<Instant>, t0: Instant, out: &mut Vec<(&str, Json)>) -> Option<(Json, i32)> {
    let p = &n.p; if fr_states == 0 { return None; }
    let c = probbit_ir::exact_components_until(lowered, exact_limit, fr_states, hard)?;
    let parts = obj(vec![("count", num(c.components as f64)), ("forest", num(c.forest as f64)), ("occupancy", num(c.occupancy as f64)), ("enumerate", num(c.enumerated as f64)), ("frontier", num(c.frontier as f64))]);
    if c.infeasible { out.extend([("verdict", jstr("infeasible")), ("reason", jstr("no plan satisfies every rule (one independent component has no feasible plan)")), ("components", parts), ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
        return Some((obj(std::mem::take(out)), 1)); }
    out.extend([("verdict", jstr("exact")), ("tier", jstr(if c.forest == c.components { "forest" } else if c.occupancy == c.components { "occupancy" } else { "components" })), ("plan", plan(n, &c.map)), ("plan_logw", num(c.map_logw)), ("violations", num(p.violations(&c.map) as f64)),
        ("odds", odds(n, &c.marg)), ("released", ids(n, &vec![true; p.t], true)), ("escalated", Json::Arr(vec![])), ("logz", num(c.logz)),
        ("top_plans", Json::Arr(vec![obj(vec![("p", num((c.map_logw - c.logz).exp())), ("plan", plan(n, &c.map))])])), ("components", parts), ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
    Some((obj(std::mem::take(out)), 0))
}
/// The decision flags of `probbit decide` (main.rs reads them from the command line, probbit-wasm from a `flags` object).
pub(crate) struct Opts { pub(crate) budget: f64, pub(crate) seed: u64, pub(crate) exact_limit: u64, pub(crate) polish_ms: f64, pub(crate) polish_sweeps: usize,
    pub(crate) fr_states: usize, pub(crate) sweeps: usize, pub(crate) mode: String, pub(crate) chains: usize, pub(crate) threads: usize, pub(crate) cpu_pct: u32,
    pub(crate) xms: Option<f64> }
/// `probbit decide` on a parsed document: the exact tiers, then the gated sampler and the polish. -> (the decision document, its exit
/// code: 0 answer, 1 infeasible, 3 refused / declined; the per-task error bars for `--summary`). `mem` = (--mem-limit-mb, rows per
/// chain), read only when the sampler runs.
pub(crate) fn decide(n: &Named, o: &Opts, mem: &dyn Fn() -> (usize, usize)) -> (Json, i32, Vec<f64>) {
    let (budget, seed, exact_limit, polish_ms, polish_sweeps, fr_states, sweeps, mode, chains, threads, cpu_pct, xms) =
        (o.budget, o.seed, o.exact_limit, o.polish_ms, o.polish_sweeps, o.fr_states, o.sweeps, o.mode.as_str(), o.chains, o.threads, o.cpu_pct, o.xms);
    let p = &n.p;
    let t0 = Instant::now(); let hard = xms.map(|x| t0 + std::time::Duration::from_secs_f64(x / 1e3));
    let mut out: Vec<(&str, Json)> = vec![("engine", jstr(&format!("probbit {VERSION}"))), ("tasks", num(p.t as f64)), ("workers", num(p.a as f64)), ("affinity", num(p.lam))];
    // R19 P1.3: the lowered program; when its raw space exceeds --exact-limit the components tier goes BEFORE the
    // whole-program enumeration, which would spend its node budget first (the external review's router32 inputs: 154 ms -> 0.4 ms)
    // R19.8 (P2.3 item 2): lowered only when that tier can run (auto, --frontier-states > 0: `--mode sample` / `exact` paid
    // the O(group size^2) lowering for nothing) and within --exact-ms (`lower_until`; past the cap the tier is skipped, as it
    // would decline, and the whole-program tiers decline at their first clock read)
    let lowered = if mode == "auto" && fr_states > 0 { p.lower_until(hard).map(|m| m.compiled().0) } else { None };
    let comp_first = lowered.as_ref().is_some_and(|l| (0..l.n).map(|i| l.cand_count(i) as f64).product::<f64>() > exact_limit as f64);
    if let Some(l) = lowered.as_ref().filter(|_| comp_first) { if let Some((d, c)) = components_tier(n, l, exact_limit, fr_states, hard, t0, &mut out) { return (d, c, vec![]); } }
    // 1. exact enumeration when the feasible set is small enough (or forced)
    if mode == "exact" || mode == "auto" {
        let lim = if mode == "exact" { u64::MAX / 128 } else { exact_limit };
        if let Some(e) = exact_within(p, 5, lim, hard) {
            if e.n_feasible == 0 { out.push(("verdict", jstr("infeasible"))); out.push(("reason", jstr("no plan satisfies every rule (allowed sets + capacities + clamps)"))); out.push(("ms", num(t0.elapsed().as_secs_f64() * 1e3)));
                return (obj(out), 1, vec![]); }
            let best = &e.top[0].1;
            out.extend([("verdict", jstr("exact")), ("tier", jstr("enumerate")), ("plan", plan(n, best)), ("plan_logw", num(p.logw(best))), ("violations", num(p.violations(best) as f64)),
                ("odds", odds(n, &e.marg)), ("released", ids(n, &vec![true; p.t], true)), ("escalated", Json::Arr(vec![])),
                ("n_feasible", num(e.n_feasible as f64)), ("logz", num(e.logz)),
                ("top_plans", Json::Arr(e.top.iter().map(|(pr, x)| obj(vec![("p", num(*pr)), ("plan", plan(n, x))])).collect())),
                ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
            return (obj(out), 0, vec![]);
        }
        // An exact mode that cannot answer declines with a JSON document and exit 3, as `probbit run --op exact` does (it was a
        // stderr line, exit 2 and an empty stdout: a bad-input code for a decision the processor declined)
        if mode == "exact" { let why = if hard.is_some_and(|h| Instant::now() >= h) { "enumeration stopped at --exact-ms; raise it, or use --mode auto or sample" }
                else { "feasible set too large for --mode exact; use --mode auto or sample" };
            out.extend([("verdict", jstr("declined")), ("reason", jstr(why)), ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
            return (obj(out), 3, vec![]); }
    }
    // 2. frontier DP: exact odds and an exact MAP plan when worker sharing between groups is thin; declines in ms otherwise
    if mode == "auto" && fr_states > 0 { tui::tier("frontier DP"); if let Some(f) = exact_frontier_until(p, fr_states, hard) {
        out.extend([("verdict", jstr("exact")), ("tier", jstr("frontier")), ("plan", plan(n, &f.map)), ("plan_logw", num(f.map_logw)), ("violations", num(p.violations(&f.map) as f64)),
            ("odds", odds(n, &f.marg)), ("released", ids(n, &vec![true; p.t], true)), ("escalated", Json::Arr(vec![])), ("logz", num(f.logz)),
            ("top_plans", Json::Arr(vec![obj(vec![("p", num((f.map_logw - f.logz).exp())), ("plan", plan(n, &f.map))])])),
            ("frontier_states", num(f.max_states as f64)), ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
        return (obj(out), 0, vec![]);
    }
    // 2b. the components tier after the whole-program tiers (see `components_tier`)
    if let Some(l) = lowered.as_ref().filter(|_| !comp_first) { if let Some((d, c)) = components_tier(n, l, exact_limit, fr_states, hard, t0, &mut out) { return (d, c, vec![]); } }
    }
    // 3. the p-bit sampler with the certification gate, with the processor controls of `probbit run` (--chains, --threads,
    // --cpu-limit, fixed-work --sweeps); with --sweeps and --polish-ms 0 the answer is a pure function of (problem, seed, chains).
    // Only a run whose exact tiers could run can have hit the cap (was also true under --mode sample)
    let ts = Instant::now(); let reached = mode != "sample" && hard.is_some_and(|h| ts >= h);
    let (mem_mb, max_rows) = mem(); tui::tier("sampler");
    let Some(s) = sample_on(p, chains, threads, sweeps, if sweeps > 0 { None } else { Some(budget) }, seed, false, auto_group_pairs(p), cpu_pct, max_rows) else {
        out.push(("verdict", jstr("infeasible"))); out.push(("reason", jstr("no feasible initial assignment (allowed sets + capacities + clamps)"))); out.push(("ms", num(t0.elapsed().as_secs_f64() * 1e3)));
        return (obj(out), 1, vec![]); };
    let sample_s = ts.elapsed().as_secs_f64(); let tg = Instant::now(); tui::tier("gate");
    let g = gate_stats(p, &s); let gate_ms = tg.elapsed().as_secs_f64() * 1e3;
    let rel = g.released_tasks(&GATE); let nrel = rel.iter().filter(|&&r| r).count();
    let whole = g.diagnostics_passed(&GATE); tui::gate_seen(g.rhat, g.tv_bound(&GATE), if whole { p.t } else { nrel }, p.t); tui::tier("polish");
    let verdict = if whole { "diagnostics_passed" } else if nrel > 0 { "partial" } else { "refused" };
    // The default --polish-ms polish is wall-clock, so its plan can vary; --polish-sweeps (parsed above) is fixed work
    // The polish runs on --threads workers (it used 4 threads whatever --threads said)
    let (plw, px) = if polish_sweeps > 0 || polish_ms > 0.0 { polish_plan_on(p, Some(&s.best.1), if polish_sweeps > 0 { 0.0 } else { polish_ms }, polish_sweeps, threads, seed).unwrap_or((s.best.0, s.best.1.clone())) }
        else { (s.best.0, s.best.1.clone()) };
    let released_mask: Vec<bool> = if whole { vec![true; p.t] } else { rel.clone() };
    let (g2, reasons) = crate::run::gate2(&g, &s, p.a, &n.tasks, &n.workers);
    out.extend([("verdict", jstr(verdict)), ("plan", plan(n, &px)), ("plan_logw", num(plw)), ("violations", num(p.violations(&px) as f64)),
        ("odds", odds(n, &s.marg)), ("released", ids(n, &released_mask, true)), ("escalated", ids(n, &released_mask, false)), ("release_reason", reasons),
        ("gate", obj([vec![("rhat", num(g.rhat)), ("tv_bound", num(g.tv_bound(&GATE))), ("tv_tol", num(GATE.tv_tol)), ("frozen_saturated_workers", num(g.frozen as f64)),
            ("min_batches", num(g.min_batches as f64)), ("batch_ratio", num(if g.sig_tv_max > 0.0 { g.sig_tv_long.iter().cloned().fold(0.0, f64::max) / g.sig_tv_max } else { 0.0 })),
            ("samples", num(s.n as f64)), ("sweeps", num(s.sweeps as f64)), ("chains", num(chains as f64)), ("budget_ms", num(budget)), ("seed", num(seed as f64)), ("collective", Json::Bool(p.collective)), ("cluster", Json::Bool(p.cluster)), ("cycles", Json::Bool(p.cycles))], g2].concat())),
        // The same telemetry object as `probbit run`
        ("telemetry", obj([vec![("chains", num(chains as f64)), ("threads", num(threads.clamp(1, chains) as f64)), ("sweeps", num(s.sweeps as f64)), ("site_updates_per_s", num(if sample_s > 0.0 { (s.sweeps as f64 * p.t as f64 / sample_s).round() } else { 0.0 })),
            ("sample_ms", num(sample_s * 1e3)), ("gate_ms", num(gate_ms)), ("polish_ms", num(if polish_sweeps > 0 { 0.0 } else { polish_ms })), ("polish_sweeps", num(polish_sweeps as f64)),
            ("process_cpu_ms", sys::usage().map_or(Json::Null, |u| num(u.0))), ("peak_rss_mb", sys::usage().map_or(Json::Null, |u| num(u.1))), ("nice", sys::nice().map_or(Json::Null, |v| num(v as f64))), ("cpu_limit_pct", num(cpu_pct as f64)),
            ("mem_limit_mb", num(mem_mb as f64)), ("traj_rows", num(s.traj.iter().map(|tr| tr.len() / p.t).sum::<usize>() as f64))], exact_cap(xms, reached), config_echo()].concat())),
        ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
    (obj(out), if verdict == "refused" { 3 } else { 0 }, item_bars(&g))
}
/// Per item, the error bar the gate compares with `tv_tol`: z x max(MCSE short, MCSE long) (`Gate::released_tasks`).
pub(crate) fn item_bars(g: &Gate) -> Vec<f64> { g.sig_tv.iter().zip(&g.sig_tv_long).map(|(&s, &l)| GATE.z * if s.is_nan() || l.is_nan() { f64::NAN } else { s.max(l) }).collect() }

/// The demo's task templates: (text, PII, production DB, judge fit per worker in `DEMO_W` order).
pub(crate) const DEMO_TPL: [(&str, bool, bool, [f64; 6]); 8] = [
        ("draft reply: refund dispute (PII)", true, false, [2.0, 1.6, 1.0, 0.6, -1.0, 1.2]),
        ("summarize 40-page contract (PII)", true, false, [2.2, 1.8, 0.8, 0.9, -1.0, 0.5]),
        ("refactor auth middleware", false, false, [1.6, 1.2, 0.0, -0.5, 2.0, -1.0]),
        ("prod DB migration review", false, true, [1.8, 1.0, 1.2, 0.0, 1.5, 1.0]),
        ("weekly SEO report", false, false, [0.5, 1.0, 1.6, 0.5, -1.0, -1.5]),
        ("triage support inbox", false, false, [0.3, 0.8, 1.5, 1.0, -1.0, 0.0]),
        ("fix flaky CI test", false, false, [1.0, 0.8, 0.2, -0.5, 1.8, -2.0]),
        ("investor update draft", false, false, [1.8, 1.4, 0.6, 0.2, -1.0, 0.8])];
/// The demo document for `n` tasks (seeded): the bytes of `probbit demo` since 0.1.
pub(crate) fn demo_doc(n: usize, seed: u64, hard: bool) -> Json {
    const W: [(&str, usize); 6] = [("opus", 2), ("sonnet", 2), ("luna-pro", 3), ("local-gemma", 2), ("codex", 2), ("human", 3)];
    const CUST: [&str; 8] = ["acme", "globex", "initech", "umbrella", "hooli", "stark", "wayne", "wonka"];
    const TPL: [(&str, bool, bool, [f64; 6]); 8] = DEMO_TPL;
    let mut r = probbit_core::Philox4x32::new(seed, 4242);
    #[allow(clippy::approx_constant)] // 6.283185307, not TAU: the demo's scores, and every test and benchmark built on them, depend on these exact bits
    let mut g = || { let u = r.f64() + 1e-12; let v = r.f64(); (-2.0 * u.ln()).sqrt() * (6.283185307 * v).cos() };
    // quotas grow with the queue: 12 tasks -> 14 slots, 300 -> ~350. --hard: 96% full and strong affinity (near-ties get escalated)
    let scale = (n as f64 / 12.0).max(1.0) * if hard { 0.89 } else { 1.0 };
    let workers: Vec<Json> = W.iter().map(|(id, c)| obj(vec![("id", jstr(id)), ("cap", num((*c as f64 * scale).ceil().max(1.0)))])).collect();
    let mut tasks = vec![];
    for i in 0..n { let k = i % TPL.len(); let (tn, pii, prod, fit) = TPL[k];
        let allowed: Vec<Json> = W.iter().enumerate().filter(|(w, _)| !(pii && *w != 3 && *w != 5) && !(prod && (*w == 2 || *w == 3))).map(|(_, (id, _))| jstr(id)).collect();
        let scores = Json::Obj(W.iter().enumerate().map(|(w, (id, _))| (id.to_string(), num(((fit[w] + 0.7 * g()) * 100.0).round() / 100.0))).collect());
        tasks.push(obj(vec![("id", jstr(&format!("T{:03}", i))), ("text", jstr(&format!("{} {}", CUST[(i / 3) % CUST.len()], tn))), ("group", jstr(&format!("{}-wf{}", CUST[(i / 3) % CUST.len()], i / 3))),
            ("allowed", Json::Arr(allowed)), ("scores", scores)])); }
    obj(vec![("comment", jstr("probbit demo: route agent tasks to workers. Rules: PII only on local-gemma or human; prod-DB migrations never on luna-pro/local-gemma; quotas are hard; tasks in one workflow prefer one worker (affinity).")),
        ("affinity", num(if hard { 2.5 } else if n <= 12 { 0.8 } else { 1.2 })), ("workers", Json::Arr(workers)), ("tasks", Json::Arr(tasks))])
}
