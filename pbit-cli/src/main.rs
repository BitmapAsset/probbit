//! `pbit` — a virtual probabilistic (p-bit) processor for certified joint decisions.
//!
//! JSON problem in on stdin, JSON decision out on stdout. Zero external crates.
//!
//!   pbit decide [--budget-ms N] [--seed N] [--exact-limit N] [--exact-ms N] [--frontier-states N] [--polish-ms N] [--polish-sweeps N] [--mode auto|exact|sample] [--sweeps N] [--chains N] [--threads N] [--cpu-limit PCT] [--mem-limit-mb N] [--priority low|normal] [--pretty]
//!   pbit demo   [--tasks N] [--seed N] [--hard] # emit an agent-routing problem as JSON; --hard = tight quotas + strong affinity
//!   pbit ir                                   # emit the problem in pbit-ir v0 (the hardware-facing text form)
//!   pbit run [--op decide|exact|sample] [--budget-ms N] [--seed N] [--exact-limit N] [--exact-ms N] [--frontier-states N] [--polish-ms N] [--polish-sweeps N] [--sweeps N] [--chains N] [--threads N] [--cpu-limit PCT] [--mem-limit-mb N] [--priority low|normal] [--pretty]  # a general pbit-ir JSON program
//!   --progress [MS] (decide, run): one JSONL telemetry line on stderr every MS ms (default 100) while the decision runs
//!   pbit stats [--sweeps N]                  # the processor's spec sheet: machine, build, effective controls + source, measured updates/s
//!   pbit version
mod json;
mod run;
mod sys;
use json::{num, obj, str as jstr, Json};
use pbit_decide::*;
use std::io::{Read, Write};

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Named { p: Problem, tasks: Vec<String>, workers: Vec<String> }

fn fail(msg: &str) -> ! { eprintln!("pbit: {msg}"); std::process::exit(2) }
/// `--budget-ms inf` / `1e300` aborted (exit 134: the deadline Duration overflowed) and `NaN` never stopped sampling
fn budget_ms(args: &[String]) -> f64 {
    let b: f64 = arg(args, "--budget-ms", 200.0);
    if !(b.is_finite() && (0.0..=1e9).contains(&b)) { fail("--budget-ms must be a number of milliseconds from 0 to 1e9") } b
}
/// `--polish-ms inf` / `NaN` hung `pbit decide` and `pbit run` (the polish loops while elapsed < the budget) and `-7` ran
/// (echoed in telemetry); the same range as --budget-ms, else exit 2
fn polish_ms_arg(args: &[String]) -> f64 {
    let x: f64 = arg(args, "--polish-ms", 50.0);
    if !(x.is_finite() && (0.0..=1e9).contains(&x)) { fail("--polish-ms must be a number of milliseconds from 0 to 1e9") } x
}
/// `--exact-ms N` (opt-in; absent = no cap, the output unchanged): a wall-clock cap on the exact tiers that run BEFORE
/// the sampler (enumeration / MRV search and the frontier DP, together, from the start of the decision). Past it they decline
/// as at --exact-limit: `decide` / `--mode auto` go on to the sampler with the full --budget-ms, `--op exact` / `--mode exact`
/// decline. Without it a loose program's exact tier counted plans to --exact-limit first (1.66 s at 3,000 vars, 200 ms budget).
fn exact_ms(args: &[String]) -> Option<f64> {
    args.iter().any(|a| a == "--exact-ms").then(|| { let x: f64 = arg(args, "--exact-ms", 0.0);
        if !(x.is_finite() && (0.0..=1e9).contains(&x)) { fail("--exact-ms must be a number of milliseconds from 0 to 1e9") } x })
}
/// Telemetry fields of a sampled answer when --exact-ms was given (none otherwise): the cap and whether the exact tiers hit it.
pub(crate) fn exact_cap(xms: Option<f64>, reached: bool) -> Vec<(&'static str, Json)> {
    xms.map_or(vec![], |x| vec![("exact_ms", num(x)), ("exact_ms_reached", Json::Bool(reached))])
}
/// Every stdout document goes through here. `println!` panicked on a closed pipe (`pbit demo --tasks 3000 | head -c 20`:
/// "failed printing to stdout: Broken pipe", exit 134 with panic = abort); a reader that went away is not an error, so a broken
/// pipe is ignored and the command keeps its normal exit code.
fn emit_raw(s: &str) {
    let mut o = std::io::stdout().lock();
    if let Err(e) = o.write_all(s.as_bytes()).and_then(|_| o.flush()) {
        if e.kind() != std::io::ErrorKind::BrokenPipe { eprintln!("pbit: cannot write the output: {e}"); std::process::exit(2) } }
}
fn emit(s: &str) { emit_raw(&format!("{s}\n")) }

/// JSON problem -> Problem. See README "Problem format".
fn from_json(j: &Json) -> Result<Named, String> {
    let ws = j.get("workers").and_then(Json::as_arr).ok_or("missing \"workers\" array")?;
    // Ids go through hash indexes (worker / task duplicate checks, worker and group lookups were linear scans: O(n^2))
    use std::collections::{HashMap, HashSet};
    let mut workers = vec![]; let mut cap = vec![]; let mut windex: HashMap<String, usize> = HashMap::new();
    for w in ws { let id = w.get("id").and_then(Json::as_str).ok_or("worker without \"id\"")?;
        let c = w.get("cap").or(w.get("capacity")).and_then(Json::as_f64).ok_or(format!("worker {id}: missing \"cap\""))?;
        if c < 0.0 || c.fract() != 0.0 { return Err(format!("worker {id}: cap must be a non-negative integer")); }
        if windex.insert(id.to_string(), workers.len()).is_some() { return Err(format!("duplicate worker id {id}")); }
        workers.push(id.to_string()); cap.push(c as usize); }
    let a = workers.len(); if a == 0 { return Err("no workers".into()); }
    let widx = |id: &str| windex.get(id).copied();
    let ts = j.get("tasks").and_then(Json::as_arr).ok_or("missing \"tasks\" array")?;
    let t = ts.len(); if t == 0 { return Err("no tasks".into()); }
    let (mut h, mut allowed, mut group, mut clamp, mut tasks) = (vec![f64::NEG_INFINITY; t * a], vec![false; t * a], vec![usize::MAX; t], vec![None; t], vec![]);
    let mut gids: Vec<String> = vec![]; let mut gindex: HashMap<String, usize> = HashMap::new(); let mut tseen: HashSet<String> = HashSet::with_capacity(t);
    for (i, tk) in ts.iter().enumerate() {
        let id = tk.get("id").and_then(Json::as_str).map(|s| s.to_string()).unwrap_or_else(|| format!("task{i}"));
        if !tseen.insert(id.clone()) { return Err(format!("duplicate task id {id}")); }
        let sc = tk.get("scores").and_then(Json::as_obj).ok_or(format!("task {id}: missing \"scores\" object"))?;
        let allow: Option<Vec<&str>> = tk.get("allowed").and_then(Json::as_arr).map(|v| v.iter().filter_map(Json::as_str).collect());
        for (wid, v) in sc { let k = widx(wid).ok_or(format!("task {id}: unknown worker {wid} in scores"))?;
            let x = v.as_f64().ok_or(format!("task {id}: score for {wid} is not a number"))?;
            if allow.as_ref().map_or(true, |al| al.contains(&wid.as_str())) { h[i * a + k] = x; allowed[i * a + k] = true; } }
        if let Some(al) = &allow { for wid in al { let k = widx(wid).ok_or(format!("task {id}: unknown worker {wid} in allowed"))?;
            if !allowed[i * a + k] { h[i * a + k] = 0.0; allowed[i * a + k] = true; } } }
        if !(0..a).any(|k| allowed[i * a + k]) { return Err(format!("task {id}: no allowed worker with a score")); }
        if let Some(g) = tk.get("group").filter(|g| !g.is_null()) { let gs = match g { Json::Str(s) => s.clone(), Json::Num(x) => format!("{x}"), _ => return Err(format!("task {id}: group must be a string or number")) };
            group[i] = match gindex.get(&gs) { Some(&k) => k, None => { gindex.insert(gs.clone(), gids.len()); gids.push(gs); gids.len() - 1 } }; }
        if let Some(c) = tk.get("clamp").filter(|c| !c.is_null()) { let cid = c.as_str().ok_or(format!("task {id}: clamp must be a worker id"))?;
            let k = widx(cid).ok_or(format!("task {id}: unknown clamp worker {cid}"))?;
            if !allowed[i * a + k] { return Err(format!("task {id}: clamp to {cid} but {cid} is not allowed")); } clamp[i] = Some(k); }
        tasks.push(id);
    }
    for v in h.iter_mut() { if *v == f64::NEG_INFINITY { *v = 0.0; } } // disallowed entries never read; keep finite
    let lam = j.get("affinity").and_then(Json::as_f64).unwrap_or(0.0);
    if lam < 0.0 { return Err("affinity must be >= 0".into()); }
    Ok(Named { p: Problem { t, a, h, allowed, cap, group, lam, clamp, block_moves: false, pair_swaps: false }, tasks, workers })
}

fn odds(n: &Named, marg: &[f64]) -> Json {
    Json::Obj(n.tasks.iter().enumerate().map(|(i, id)| { let mut row: Vec<(usize, f64)> = (0..n.p.a).filter(|&k| n.p.allowed[i * n.p.a + k]).map(|k| (k, marg[i * n.p.a + k])).collect();
        row.sort_by(|u, v| v.1.partial_cmp(&u.1).unwrap());
        (id.clone(), Json::Obj(row.into_iter().map(|(k, m)| (n.workers[k].clone(), num(m))).collect())) }).collect())
}
fn plan(n: &Named, x: &[usize]) -> Json { Json::Obj(n.tasks.iter().enumerate().map(|(i, id)| (id.clone(), jstr(&n.workers[x[i]]))).collect()) }
fn ids(n: &Named, mask: &[bool], want: bool) -> Json { Json::Arr(n.tasks.iter().enumerate().filter(|(i, _)| mask[*i] == want).map(|(_, id)| jstr(id)).collect()) }

/// A flag that is present must parse (exit 2 otherwise). It used to fall back to the default silently: `--sweeps 1e4`
/// ran in wall-clock mode (not the fixed-work, deterministic mode asked for) and `--seed x` ran seed 7.
fn arg<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    match args.iter().position(|a| a == name) { None => default, Some(k) => match args.get(k + 1) {
        Some(v) => v.parse().unwrap_or_else(|_| fail(&format!("{name}: cannot read {v:?} (integer flags take plain digits, e.g. 10000 not 1e4)"))),
        None => fail(&format!("{name} needs a value")) } }
}
/// Flags taking a value, per command (the controls are shared by decide, run and stats).
const CONTROLS: [&str; 5] = ["--chains", "--threads", "--cpu-limit", "--mem-limit-mb", "--priority"];
const SEARCH: [&str; 8] = ["--budget-ms", "--seed", "--exact-limit", "--exact-ms", "--frontier-states", "--polish-ms", "--polish-sweeps", "--sweeps"];
/// Unknown arguments exit 2. They were ignored: a typo (`--budgetms 5`), or `"--budget-ms 50"` passed as ONE argument by
/// a shell that did not split it (zsh `$var`), ran the whole decision on defaults without a word. `values` take the next argument,
/// `switches` none; `--progress` takes an optional number.
/// A value flag given twice exits 2 (it used the first value and never read the second: `--budget-ms 50
/// --budget-ms nan` ran with exit 0). `--progress` too (its optional value was first-wins: `--progress 1000 --progress 10`
/// printed 0 lines in 300 ms, the reverse order 23).
fn check_flags(args: &[String], values: &[&str], switches: &[&str]) {
    let mut i = 1; let mut seen: Vec<&str> = Vec::new();
    while i < args.len() { let a = args[i].as_str();
        if values.contains(&a) { if i + 1 >= args.len() { fail(&format!("{a} needs a value")) }
            if seen.contains(&a) { fail(&format!("{a} given twice")) } seen.push(a); i += 2; }
        else if a == "--progress" && switches.contains(&a) { if seen.contains(&a) { fail(&format!("{a} given twice")) } seen.push(a);
            i += if args.get(i + 1).is_some_and(|v| v.parse::<u64>().is_ok()) { 2 } else { 1 }; }
        else if switches.contains(&a) { i += 1; }
        else { fail(&format!("unknown argument {a:?} for `pbit {}` (run `pbit` for usage)", args[0])) } }
}
/// Optional config file: `$PBIT_CONFIG`, else `./pbit.json` if present (keys chains, threads, cpu_limit, priority).
fn config() -> Option<json::Json> {
    let path = std::env::var("PBIT_CONFIG").ok().or_else(|| std::path::Path::new("pbit.json").exists().then(|| "pbit.json".to_string()))?;
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| fail(&format!("config {path}: {e}")));
    Some(json::parse(&src).unwrap_or_else(|e| fail(&format!("config {path}: bad JSON: {e}"))))
}
/// A config-file value as text (numbers must be whole; anything else fails the caller's parse).
fn cfg_val(key: &str) -> Option<String> {
    let c = config()?; let v = c.get(key)?;
    Some(if let Some(x) = v.as_f64() { if x.fract() == 0.0 && x >= 0.0 { format!("{}", x as u64) } else { format!("{x}") } } else { v.as_str().map_or("?".into(), str::to_string) })
}
/// Processor-style control: CLI flag > `PBIT_*` environment variable > config file > default; must be >= 1.
fn ctl(args: &[String], name: &str, env: &str, default: usize) -> usize { ctl_min(args, name, env, default, 1) }
fn ctl_min(args: &[String], name: &str, env: &str, default: usize, min: usize) -> usize {
    let v = args.iter().position(|a| a == name).and_then(|k| args.get(k + 1)).map(|v| v.to_string()).or_else(|| std::env::var(env).ok())
        .or_else(|| cfg_val(&name[2..].replace('-', "_")));
    match v { None => default, Some(v) => match v.parse::<usize>() { Ok(n) if n >= min => n, _ => fail(&format!("{name} / {env} must be an integer >= {min}")) } }
}
/// The default memory cap (MB). Unbounded, sample memory grew ~190 MB per second of budget on the 300-task demo,
/// so a long `--budget-ms` could exhaust a small machine; 1024 MB never thins below ~8 s there (inferred from rows per chain).
const DEFAULT_MEM_LIMIT_MB: usize = 1024;
/// `--mem-limit-mb N` (+ PBIT_MEM_LIMIT_MB, config key mem_limit_mb; default now 1024, 0 = unbounded) caps the sample
/// buffers that grow with the budget (per-chain trajectory n x 2 bytes + trace 8 bytes per kept sweep) at N MB in total by thinning
/// (`pbit_ir::record_row`), never below 64 rows per chain. Fixed costs (program, JSON, marginals) come on top. -> (N, rows per chain)
fn mem_limit(args: &[String], chains: usize, n_vars: usize) -> (usize, usize) {
    let mb = ctl_min(args, "--mem-limit-mb", "PBIT_MEM_LIMIT_MB", DEFAULT_MEM_LIMIT_MB, 0);
    (mb, if mb == 0 { 0 } else { (mb.saturating_mul(1_048_576) / (chains * (2 * n_vars + 8))).max(64) })
}
/// `--progress [MS]` monitor: a thread prints one JSONL line to stderr every MS ms (default 100) while the decision runs:
/// {"event":"progress","ms","sweeps","site_updates_per_s","process_cpu_ms","peak_rss_mb"} (sweeps = all chains so far, counted in
/// steps of 64; updates/s = since the previous line). Stdout is unchanged. Stopped (and joined) when dropped; exit paths skip it.
struct Monitor { tx: Option<std::sync::mpsc::Sender<()>>, h: Option<std::thread::JoinHandle<()>> }
impl Drop for Monitor { fn drop(&mut self) { drop(self.tx.take()); if let Some(h) = self.h.take() { let _ = h.join(); } } }
fn monitor(args: &[String], n_vars: usize) -> Option<Monitor> {
    // `--progress 0` used to become 100 silently (and `--progress 1.5` too; check_flags now rejects it as an unknown argument)
    let every: u64 = match args.get(args.iter().position(|a| a == "--progress")? + 1).and_then(|v| v.parse::<u64>().ok()) { Some(0) => fail("--progress MS must be >= 1"), Some(v) => v, None => 100 };
    use std::sync::atomic::Ordering::Relaxed;
    pbit_ir::PROGRESS_SWEEPS.store(0, Relaxed); pbit_ir::PROGRESS_ON.store(true, Relaxed);
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let h = std::thread::spawn(move || { let t0 = std::time::Instant::now(); let (mut lt, mut ls) = (0.0f64, 0u64);
        while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(std::time::Duration::from_millis(every)) {
            let (t, s) = (t0.elapsed().as_secs_f64(), pbit_ir::PROGRESS_SWEEPS.load(Relaxed)); let u = sys::usage();
            let line = obj(vec![("event", jstr("progress")), ("ms", num((t * 1e3).round())), ("sweeps", num(s as f64)), ("site_updates_per_s", num(((s - ls) as f64 * n_vars as f64 / (t - lt)).round())),
                ("process_cpu_ms", u.map_or(Json::Null, |u| num(u.0.round()))), ("peak_rss_mb", u.map_or(Json::Null, |u| num(u.1)))]);
            eprintln!("{}", json::write(&line, false)); (lt, ls) = (t, s); } });
    Some(Monitor { tx: Some(tx), h: Some(h) })
}
fn read_stdin() -> String { let mut s = String::new(); std::io::stdin().read_to_string(&mut s).unwrap_or_else(|e| fail(&format!("stdin: {e}"))); s }
/// --priority low|normal (flag > PBIT_PRIORITY > config > normal), applied before any chain thread starts.
fn priority(args: &[String]) {
    match args.iter().position(|a| a == "--priority").and_then(|k| args.get(k + 1)).cloned().or_else(|| std::env::var("PBIT_PRIORITY").ok()).or_else(|| cfg_val("priority")).as_deref() {
        None | Some("normal") => {}, Some("low") => { if sys::set_low().is_none() { fail("--priority low: setpriority failed"); } },
        Some(_) => fail("--priority / PBIT_PRIORITY must be low or normal") }
}
/// The processor controls shared by `pbit decide` and `pbit run`: (chains, threads, cpu_limit_pct).
fn controls(args: &[String]) -> (usize, usize, u32) {
    (ctl(args, "--chains", "PBIT_CHAINS", 4), ctl(args, "--threads", "PBIT_THREADS", std::thread::available_parallelism().map_or(1, |n| n.get()).min(4)),
     { let c = ctl(args, "--cpu-limit", "PBIT_CPU_LIMIT", 100); if c > 100 { fail("--cpu-limit / PBIT_CPU_LIMIT must be 1..=100"); } c as u32 })
}

fn decide_cmd(args: &[String]) {
    check_flags(args, &[&SEARCH[..], &CONTROLS[..], &["--mode"]].concat(), &["--pretty", "--progress"]);
    let budget: f64 = budget_ms(args); let seed: u64 = arg(args, "--seed", 7);
    let exact_limit: u64 = arg(args, "--exact-limit", 2_000_000); let polish_ms: f64 = polish_ms_arg(args);
    // Any other --mode value (`foo`, `EXACT`, or `--pretty` swallowed as the value) used to run the sampler silently (exit 0)
    let mode: String = arg(args, "--mode", "auto".to_string()); let pretty = args.iter().any(|a| a == "--pretty");
    if !["auto", "exact", "sample"].contains(&mode.as_str()) { fail("--mode must be auto, exact or sample") }
    // --polish-sweeps N = fixed-work polish (deterministic); parsed here, so a bad value exits 2 before sampling
    let polish_sweeps: usize = arg(args, "--polish-sweeps", 0);
    let fr_states: usize = arg(args, "--frontier-states", FRONTIER_MAX_STATES); let sweeps: usize = arg(args, "--sweeps", 0);
    let (chains, threads, cpu_pct) = controls(args); priority(args);
    let src = read_stdin(); let j = json::parse(&src).unwrap_or_else(|e| fail(&format!("bad JSON: {e}")));
    let n = from_json(&j).unwrap_or_else(|e| fail(&format!("bad problem: {e}"))); let p = &n.p;
    let _mon = monitor(args, p.t); let t0 = std::time::Instant::now();
    let xms = exact_ms(args); let hard = xms.map(|x| t0 + std::time::Duration::from_secs_f64(x / 1e3));
    let mut out: Vec<(&str, Json)> = vec![("engine", jstr(&format!("pbit {VERSION}"))), ("tasks", num(p.t as f64)), ("workers", num(p.a as f64)), ("affinity", num(p.lam))];
    // 1. exact enumeration when the feasible set is small enough (or forced)
    if mode == "exact" || mode == "auto" {
        let lim = if mode == "exact" { u64::MAX / 128 } else { exact_limit };
        if let Some(e) = exact_within(p, 5, lim, hard) {
            if e.n_feasible == 0 { out.push(("verdict", jstr("infeasible"))); out.push(("reason", jstr("no plan satisfies every rule (allowed sets + capacities + clamps)"))); out.push(("ms", num(t0.elapsed().as_secs_f64() * 1e3)));
                emit(&json::write(&obj(out), pretty)); std::process::exit(1); }
            let best = &e.top[0].1;
            out.extend([("verdict", jstr("exact")), ("tier", jstr("enumerate")), ("plan", plan(&n, best)), ("plan_logw", num(p.logw(best))), ("violations", num(p.violations(best) as f64)),
                ("odds", odds(&n, &e.marg)), ("released", ids(&n, &vec![true; p.t], true)), ("escalated", Json::Arr(vec![])),
                ("n_feasible", num(e.n_feasible as f64)), ("logz", num(e.logz)),
                ("top_plans", Json::Arr(e.top.iter().map(|(pr, x)| obj(vec![("p", num(*pr)), ("plan", plan(&n, x))])).collect())),
                ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
            emit(&json::write(&obj(out), pretty)); return;
        }
        if mode == "exact" { if hard.is_some_and(|h| std::time::Instant::now() >= h) { fail("--mode exact: the enumeration did not finish within --exact-ms; raise it, or use auto or sample"); }
            fail("feasible set too large for --mode exact; use auto or sample"); }
    }
    // 2. frontier DP: exact odds and an exact MAP plan when worker sharing between groups is thin; declines in ms otherwise
    if mode == "auto" && fr_states > 0 { if let Some(f) = exact_frontier_until(p, fr_states, hard) {
        out.extend([("verdict", jstr("exact")), ("tier", jstr("frontier")), ("plan", plan(&n, &f.map)), ("plan_logw", num(f.map_logw)), ("violations", num(p.violations(&f.map) as f64)),
            ("odds", odds(&n, &f.marg)), ("released", ids(&n, &vec![true; p.t], true)), ("escalated", Json::Arr(vec![])), ("logz", num(f.logz)),
            ("top_plans", Json::Arr(vec![obj(vec![("p", num((f.map_logw - f.logz).exp())), ("plan", plan(&n, &f.map))])])),
            ("frontier_states", num(f.max_states as f64)), ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
        emit(&json::write(&obj(out), pretty)); return;
    } }
    // 3. the p-bit sampler with the certification gate, with the processor controls of `pbit run` (--chains, --threads,
    // --cpu-limit, fixed-work --sweeps); with --sweeps and --polish-ms 0 the answer is a pure function of (problem, seed, chains).
    // Only a run whose exact tiers could run can have hit the cap (was also true under --mode sample)
    let ts = std::time::Instant::now(); let reached = mode != "sample" && hard.is_some_and(|h| ts >= h);
    let (mem_mb, max_rows) = mem_limit(args, chains, p.t);
    let Some(s) = sample_on(p, chains, threads, sweeps, if sweeps > 0 { None } else { Some(budget) }, seed, false, auto_group_pairs(p), cpu_pct, max_rows) else {
        out.push(("verdict", jstr("infeasible"))); out.push(("reason", jstr("no feasible initial assignment (allowed sets + capacities + clamps)"))); out.push(("ms", num(t0.elapsed().as_secs_f64() * 1e3)));
        emit(&json::write(&obj(out), pretty)); std::process::exit(1); };
    let sample_s = ts.elapsed().as_secs_f64(); let tg = std::time::Instant::now();
    let g = gate_stats(p, &s); let gate_ms = tg.elapsed().as_secs_f64() * 1e3;
    let rel = g.certified_tasks(&GATE); let nrel = rel.iter().filter(|&&r| r).count();
    let whole = g.certified(&GATE);
    let verdict = if whole { "certified" } else if nrel > 0 { "partial" } else { "refused" };
    // The default --polish-ms polish is wall-clock, so its plan can vary; --polish-sweeps (parsed above) is fixed work
    // The polish runs on --threads workers (it used 4 threads whatever --threads said)
    let (plw, px) = if polish_sweeps > 0 || polish_ms > 0.0 { polish_plan_on(p, Some(&s.best.1), if polish_sweeps > 0 { 0.0 } else { polish_ms }, polish_sweeps, threads, seed).unwrap_or((s.best.0, s.best.1.clone())) }
        else { (s.best.0, s.best.1.clone()) };
    let released_mask: Vec<bool> = if whole { vec![true; p.t] } else { rel.clone() };
    out.extend([("verdict", jstr(verdict)), ("plan", plan(&n, &px)), ("plan_logw", num(plw)), ("violations", num(p.violations(&px) as f64)),
        ("odds", odds(&n, &s.marg)), ("released", ids(&n, &released_mask, true)), ("escalated", ids(&n, &released_mask, false)),
        ("gate", obj(vec![("rhat", num(g.rhat)), ("tv_bound", num(g.tv_bound(&GATE))), ("tv_tol", num(GATE.tv_tol)), ("frozen_saturated_workers", num(g.frozen as f64)),
            ("min_batches", num(g.min_batches as f64)), ("batch_ratio", num(if g.sig_tv_max > 0.0 { g.sig_tv_long.iter().cloned().fold(0.0, f64::max) / g.sig_tv_max } else { 0.0 })),
            ("samples", num(s.n as f64)), ("sweeps", num(s.sweeps as f64)), ("chains", num(chains as f64)), ("budget_ms", num(budget)), ("seed", num(seed as f64))])),
        // The same telemetry object as `pbit run`
        ("telemetry", obj([vec![("chains", num(chains as f64)), ("threads", num(threads.clamp(1, chains) as f64)), ("sweeps", num(s.sweeps as f64)), ("site_updates_per_s", num((s.sweeps as f64 * p.t as f64 / sample_s).round())),
            ("sample_ms", num(sample_s * 1e3)), ("gate_ms", num(gate_ms)), ("polish_ms", num(if polish_sweeps > 0 { 0.0 } else { polish_ms })), ("polish_sweeps", num(polish_sweeps as f64)),
            ("process_cpu_ms", sys::usage().map_or(Json::Null, |u| num(u.0))), ("peak_rss_mb", sys::usage().map_or(Json::Null, |u| num(u.1))), ("nice", sys::nice().map_or(Json::Null, |v| num(v as f64))), ("cpu_limit_pct", num(cpu_pct as f64)),
            ("mem_limit_mb", num(mem_mb as f64)), ("traj_rows", num(s.traj.iter().map(|tr| tr.len() / p.t).sum::<usize>() as f64))], exact_cap(xms, reached)].concat())),
        ("ms", num(t0.elapsed().as_secs_f64() * 1e3))]);
    emit(&json::write(&obj(out), pretty));
    if verdict == "refused" { std::process::exit(3); }
}

/// Where a control's value comes from (same precedence as `ctl`): "flag" > "env" > "file" > "default".
fn source(args: &[String], name: &str, env: &str) -> &'static str {
    if args.iter().any(|a| a == name) { "flag" } else if std::env::var(env).is_ok() { "env" } else if cfg_val(&name[2..].replace('-', "_")).is_some() { "file" } else { "default" }
}
/// `pbit stats` — the processor's spec sheet: machine, build target features, effective controls and where each came
/// from, and a measured self-test (400-spin ring, 4 chains x `--sweeps` fixed work, IR sampler) at 1 thread and at the effective
/// thread count, after one discarded warm-up pass. Nothing is applied (e.g. --priority low is reported, not set).
fn stats_cmd(args: &[String]) {
    check_flags(args, &[&CONTROLS[..], &["--sweeps"]].concat(), &["--pretty"]);
    let (chains, threads, cpu) = controls(args); let sweeps: usize = arg(args, "--sweeps", 2000).max(1);
    let prio = args.iter().position(|a| a == "--priority").and_then(|k| args.get(k + 1)).cloned().or_else(|| std::env::var("PBIT_PRIORITY").ok()).or_else(|| cfg_val("priority")).unwrap_or_else(|| "normal".into());
    if !["low", "normal"].contains(&prio.as_str()) { fail("--priority / PBIT_PRIORITY must be low or normal") } // a bad value used to be echoed
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let feats: Vec<Json> = [("sse4.2", cfg!(target_feature = "sse4.2")), ("avx", cfg!(target_feature = "avx")), ("avx2", cfg!(target_feature = "avx2")), ("fma", cfg!(target_feature = "fma")),
        ("bmi2", cfg!(target_feature = "bmi2")), ("avx512f", cfg!(target_feature = "avx512f")), ("neon", cfg!(target_feature = "neon")), ("dotprod", cfg!(target_feature = "dotprod"))]
        .iter().filter(|f| f.1).map(|f| jstr(f.0)).collect();
    let n = 400; let ring = format!("{{\"pbit_ir\": 1, \"values\": [\"-\", \"+\"], \"vars\": [{}], \"pairs\": [{}]}}",
        (0..n).map(|i| format!("{{\"id\": \"s{i}\", \"h\": {{\"+\": {}}}}}", 0.1 * ((i % 7) as f64 - 3.0))).collect::<Vec<_>>().join(","),
        (0..n).map(|i| format!("{{\"i\": \"s{i}\", \"j\": \"s{}\", \"table\": [[0.4, -0.4], [-0.4, 0.4]]}}", (i + 1) % n)).collect::<Vec<_>>().join(","));
    let prog = run::from_json(&json::parse(&ring).unwrap()).unwrap();
    let rate = |t: usize| { let t0 = std::time::Instant::now(); let s = pbit_ir::sample_on(&prog.m, chains, t, sweeps, None, 1, false, cpu as u32, 0).unwrap();
        (s.sweeps as f64 * n as f64 / t0.elapsed().as_secs_f64()).round() };
    rate(threads); let (r1, rt) = (rate(1), rate(threads)); // first pass discarded: page faults + clock ramp made 1 thread look slow (4.27x on 4 threads)
    let ctl_obj = |v: Json, name: &str, env: &str| obj(vec![("value", v), ("source", jstr(source(args, name, env)))]);
    let doc = obj(vec![("engine", jstr(&format!("pbit {VERSION}"))),
        ("machine", obj(vec![("os", jstr(std::env::consts::OS)), ("arch", jstr(std::env::consts::ARCH)), ("logical_cpus", num(cores as f64))])),
        ("build", obj(vec![("target_features", Json::Arr(feats)), ("debug_assertions", Json::Bool(cfg!(debug_assertions)))])),
        ("controls", obj(vec![("chains", ctl_obj(num(chains as f64), "--chains", "PBIT_CHAINS")), ("threads", ctl_obj(num(threads as f64), "--threads", "PBIT_THREADS")),
            ("cpu_limit_pct", ctl_obj(num(cpu as f64), "--cpu-limit", "PBIT_CPU_LIMIT")), ("priority", ctl_obj(jstr(&prio), "--priority", "PBIT_PRIORITY")),
            ("mem_limit_mb", ctl_obj(num(ctl_min(args, "--mem-limit-mb", "PBIT_MEM_LIMIT_MB", DEFAULT_MEM_LIMIT_MB, 0) as f64), "--mem-limit-mb", "PBIT_MEM_LIMIT_MB")),
            ("config_file", std::env::var("PBIT_CONFIG").ok().or_else(|| std::path::Path::new("pbit.json").exists().then(|| "pbit.json".to_string())).map_or(Json::Null, |f| jstr(&f)))])),
        ("self_test", obj(vec![("program", jstr("400-spin ring, IR sampler, fixed work")), ("chains", num(chains as f64)), ("sweeps_per_chain", num(sweeps as f64)),
            ("site_updates_per_s_1_thread", num(r1)), ("threads", num(threads.clamp(1, chains) as f64)), ("site_updates_per_s", num(rt)), ("speedup", num((rt / r1 * 100.0).round() / 100.0))])),
        ("process_cpu_ms", sys::usage().map_or(Json::Null, |u| num(u.0))), ("peak_rss_mb", sys::usage().map_or(Json::Null, |u| num(u.1)))]);
    emit(&json::write(&doc, args.iter().any(|a| a == "--pretty")));
}

/// The agent-routing demo as a JSON problem: agent tasks x workers under PII / prod-DB policy, quotas and same-workflow affinity.
fn demo_cmd(args: &[String]) {
    check_flags(args, &["--tasks", "--seed"], &["--hard"]);
    const W: [(&str, usize); 6] = [("opus", 2), ("sonnet", 2), ("luna-pro", 3), ("local-gemma", 2), ("codex", 2), ("human", 3)];
    const CUST: [&str; 8] = ["acme", "globex", "initech", "umbrella", "hooli", "stark", "wayne", "wonka"];
    const TPL: [(&str, bool, bool, [f64; 6]); 8] = [
        ("draft reply: refund dispute (PII)", true, false, [2.0, 1.6, 1.0, 0.6, -1.0, 1.2]),
        ("summarize 40-page contract (PII)", true, false, [2.2, 1.8, 0.8, 0.9, -1.0, 0.5]),
        ("refactor auth middleware", false, false, [1.6, 1.2, 0.0, -0.5, 2.0, -1.0]),
        ("prod DB migration review", false, true, [1.8, 1.0, 1.2, 0.0, 1.5, 1.0]),
        ("weekly SEO report", false, false, [0.5, 1.0, 1.6, 0.5, -1.0, -1.5]),
        ("triage support inbox", false, false, [0.3, 0.8, 1.5, 1.0, -1.0, 0.0]),
        ("fix flaky CI test", false, false, [1.0, 0.8, 0.2, -0.5, 1.8, -2.0]),
        ("investor update draft", false, false, [1.8, 1.4, 0.6, 0.2, -1.0, 0.8])];
    let n: usize = arg(args, "--tasks", 12); let seed: u64 = arg(args, "--seed", 7); let hard = args.iter().any(|a| a == "--hard");
    let mut r = pbit_core::Philox4x32::new(seed, 4242);
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
    let doc = obj(vec![("comment", jstr("pbit demo: route agent tasks to workers. Rules: PII only on local-gemma or human; prod-DB migrations never on luna-pro/local-gemma; quotas are hard; tasks in one workflow prefer one worker (affinity).")),
        ("affinity", num(if hard { 2.5 } else if n <= 12 { 0.8 } else { 1.2 })), ("workers", Json::Arr(workers)), ("tasks", Json::Arr(tasks))]);
    emit(&json::write(&doc, true));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("decide") => decide_cmd(&args),
        Some("demo") => demo_cmd(&args),
        Some("ir") => { check_flags(&args, &[], &[]); let src = read_stdin(); let j = json::parse(&src).unwrap_or_else(|e| fail(&format!("bad JSON: {e}")));
            let n = from_json(&j).unwrap_or_else(|e| fail(&format!("bad problem: {e}"))); emit_raw(&pbit_decide::ir::to_ir(&n.p)); }
        Some("run") => { check_flags(&args, &[&SEARCH[..], &CONTROLS[..], &["--op"]].concat(), &["--pretty", "--progress"]); let src = read_stdin(); let j = json::parse(&src).unwrap_or_else(|e| fail(&format!("bad JSON: {e}")));
            let p = run::from_json(&j).unwrap_or_else(|e| fail(&format!("bad program: {e}")));
            priority(&args); let (chains, threads, cpu_pct) = controls(&args);
            let op: String = arg(&args, "--op", "decide".to_string()); if !["decide", "exact", "sample"].contains(&op.as_str()) { fail("--op must be decide, exact or sample"); }
            let mon = monitor(&args, p.m.n);
            let (doc, code) = run::run(&p, &op, budget_ms(&args), arg(&args, "--seed", 7), arg(&args, "--exact-limit", 2_000_000), polish_ms_arg(&args), arg(&args, "--polish-sweeps", 0), arg(&args, "--sweeps", 0), arg(&args, "--frontier-states", pbit_ir::FRONTIER_MAX_STATES),
                chains, threads, cpu_pct, mem_limit(&args, chains, p.m.n), exact_ms(&args)); drop(mon);
            emit(&json::write(&doc, args.iter().any(|a| a == "--pretty"))); if code != 0 { std::process::exit(code); } }
        Some("stats") => stats_cmd(&args),
        Some("version") | Some("--version") | Some("-V") => { check_flags(&args, &[], &[]); emit(&format!("pbit {VERSION}")) }
        _ => { let _ = std::io::stderr().write_all(b"usage: pbit decide [--budget-ms N] [--seed N] [--exact-limit N] [--exact-ms N] [--frontier-states N] [--polish-ms N] [--polish-sweeps N] [--mode auto|exact|sample] [--sweeps N] [--chains N] [--threads N] [--cpu-limit PCT] [--mem-limit-mb N] [--priority low|normal] [--progress [MS]] [--pretty] < problem.json\n       pbit demo [--tasks N] [--seed N] [--hard]\n       pbit ir < problem.json\n       pbit run [--op decide|exact|sample] [--budget-ms N] [--seed N] [--exact-limit N] [--exact-ms N] [--frontier-states N] [--polish-ms N] [--polish-sweeps N] [--sweeps N] [--chains N] [--threads N] [--cpu-limit PCT] [--mem-limit-mb N] [--priority low|normal] [--progress [MS]] [--pretty] < program.json   (pbit-ir JSON v1)\n       pbit stats [--sweeps N] [--pretty]   (machine, build, effective controls + source, measured updates/s)\n       pbit version\n"); std::process::exit(2) }
    }
}
