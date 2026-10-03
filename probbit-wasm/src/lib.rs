//! probbit-wasm: probbit in a browser (wasm32-unknown-unknown), without threads, on the page's clock. JSON in, JSON out, with the
//! CLI's own documents: `decide` (a router document, as `probbit decide`), `run` (a probbit-ir program, as `probbit run`), `evaluate` (a
//! System One request, as `probbit evaluate`), `demo` (as `probbit demo`) and the persona ops `persona_init` / `persona_turn` (the
//! arguments of the MCP tools probbit_persona_init / probbit_persona_turn, with the persona inline; docs/persona.md). Flags go in an
//! optional `flags` object, as in `probbit mcp`:
//! `{"workers": [...], "tasks": [...], "flags": {"sweeps": 3200, "polish_ms": 0}}`.
//!
//! The code is the CLI's (probbit-cli's json.rs, run.rs, router.rs, evaluate.rs, persona.rs, yaml.rs and sys.rs, included below) run with
//! `probbit_core::rt::set_sequential(true)`: every chain, gate pass and polish chain runs in order on the calling thread. At fixed
//! work the documents equal the CLI's at any `--threads` (timings and `telemetry.threads` aside; tests/no_threads.rs).
//! Not here: `--summary`, `--pretty`, `--top`, `--progress`, `--cpu-limit`, `--priority` and the config file (an unknown flag is
//! an error). `threads` above 1 is accepted only off wasm (the native tests' comparison runs).
//!
//! JavaScript calls the C ABI at the end (playground/index.html): `probbit_alloc(len)` -> a buffer for the UTF-8 input;
//! `probbit_call(op, ptr, len)` -> the output's length (op 0 decide, 1 run, 2 evaluate, 3 demo, 4 persona init, 5 persona turn; the
//! input buffer is consumed);
//! `probbit_out_ptr()` / `probbit_out_code()` -> the output bytes and the CLI's exit code (0 answer, 1 infeasible, 2 bad input or flag,
//! 3 refused / declined). The clock is the import `probbit.now_ms` (`performance.now()`). No dependencies (no wasm-bindgen).
#[allow(dead_code)]
#[path = "../../probbit-cli/src/json.rs"]
mod json;
#[allow(dead_code)]
#[path = "../../probbit-cli/src/run.rs"]
mod run;
#[allow(dead_code)]
#[path = "../../probbit-cli/src/router.rs"]
mod router;
#[allow(dead_code)]
#[path = "../../probbit-cli/src/evaluate.rs"]
mod evaluate;
#[allow(dead_code, unused_imports)]
#[path = "../../probbit-cli/src/sys.rs"]
mod sys;
#[allow(dead_code)]
#[path = "../../probbit-cli/src/persona.rs"]
mod persona;
#[allow(dead_code)]
#[path = "../../probbit-cli/src/yaml.rs"]
mod yaml;
/// The CLI's `--top` monitor hooks (probbit-cli/src/tui.rs): nothing is drawn here.
mod tui {
    pub fn tier(_: &'static str) {}
    pub fn gate_seen(_: f64, _: f64, _: usize, _: usize) {}
}
use json::{num, str as jstr, InErr, Json};
use probbit_core::rt::Instant;
use router::item_bars;

/// The telemetry fields of `--exact-ms` (as probbit-cli/src/main.rs).
fn exact_cap(xms: Option<f64>, reached: bool) -> Vec<(&'static str, Json)> {
    xms.map_or(vec![], |x| vec![("exact_ms", num(x)), ("exact_ms_reached", Json::Bool(reached))])
}
/// No configuration file in a browser (the CLI echoes probbit.json / $PROBBIT_CONFIG here).
fn config_echo() -> Vec<(&'static str, Json)> { vec![] }

/// `probbit decide` on a router document (+ optional `flags`) -> (the decision JSON, the CLI's exit code).
pub fn decide(input: &str) -> (String, i32) { call(input, "decide") }
/// `probbit run` on a probbit-ir program (+ optional `flags`).
pub fn run(input: &str) -> (String, i32) { call(input, "run") }
/// `probbit evaluate` on a System One request (+ optional `probbit` block and `flags`).
pub fn evaluate(input: &str) -> (String, i32) { call(input, "evaluate") }
/// `probbit demo`: `{"tasks": 300, "seed": 7, "hard": false}` (each optional; defaults 12, 7, false) -> the router document, as
/// `probbit demo --tasks N --seed S [--hard]` prints it.
pub fn demo(input: &str) -> (String, i32) {
    let go = || -> Result<String, Json> {
        let j = json::parse(input).map_err(|e| e.to_json())?;
        let o = json::fields(&j, "", &["tasks", "seed", "hard"]).map_err(|e| e.to_json())?;
        let int = |k: &str, d: usize| o.iter().find(|(x, _)| x == k).map_or(Ok(d), |(_, v)| json::count(v, k).map_err(|e| e.to_json()));
        let hard = match o.iter().find(|(x, _)| x == "hard") { None => false, Some((_, Json::Bool(b))) => *b, Some(_) => return Err(json::schema("hard", "must be true or false").to_json()) };
        let tasks = int("tasks", 12)?; if tasks == 0 { return Err(json::value("tasks", "at least 1").to_json()); }
        Ok(json::write(&router::demo_doc(tasks, int("seed", 7)? as u64, hard), true))
    };
    match go() { Ok(s) => (s, 0), Err(e) => (json::write(&e, false), 2) }
}

/// `probbit persona init` (MCP tool probbit_persona_init): `{"persona": {...}, "seed": 2}` -> the individual's state, exactly the CLI's
/// document (`probbit persona init PERSONA --seed 2`). Without threads; resource controls at the CLI's defaults (they never change it).
pub fn persona_init(input: &str) -> (String, i32) { persona_call(input, "probbit_persona_init") }
/// `probbit persona turn` (MCP tool probbit_persona_turn): `{"persona": {...}, "state": {...}, "inputs": {...}, "flags": {"timing": true}}`
/// -> `{"stance": ..., "state": ...}`, the stance and next state exactly as the CLI prints and writes them.
pub fn persona_turn(input: &str) -> (String, i32) { persona_call(input, "probbit_persona_turn") }
fn persona_call(input: &str, tool: &str) -> (String, i32) {
    let eng = |prog: &Json, f: &persona::Flags| on_threads(1, || persona::run_program(prog, f, 1, 100, 1024));
    let r = json::parse(input).map_err(|e| persona::perr(&e.path, e.msg)).and_then(|j| match j { Json::Obj(v) => persona::tool(tool, &v, &eng, false),
        _ => Err(persona::perr("", "the arguments must be a JSON object")) });
    match r { Ok(d) => (persona::canon(&d), 0), Err(e) => (persona::canon(&e.to_json()), 2) }
}

/// The flags of one call, with the CLI's defaults and ranges.
struct Flags { budget: f64, budget_given: bool, seed: u64, exact_limit: u64, exact_ms: Option<f64>, fr_states: usize, polish_ms: f64, polish_sweeps: usize,
    sweeps: usize, collective: bool, cluster: bool, cycles: bool, chains: usize, threads: usize, mem_mb: usize, mode: String, deadline_ms: Option<f64>, program: bool }
/// A flag error: `{"error": {"code": "flag", "path": "flags.<name>", "message"}}`, exit 2 (the CLI prints a stderr line instead).
fn flag_err(k: &str, msg: &str) -> Json { InErr { code: "flag", path: format!("flags.{k}"), msg: msg.to_string() }.to_json() }
fn flags(f: Option<&Json>, cmd: &str) -> Result<Flags, Json> {
    let mut o = Flags { budget: 200.0, budget_given: false, seed: 7, exact_limit: 2_000_000, exact_ms: None, fr_states: probbit_ir::FRONTIER_MAX_STATES, polish_ms: 50.0,
        polish_sweeps: 0, sweeps: 0, collective: true, cluster: true, cycles: true, chains: 4, threads: 1, mem_mb: 1024,
        mode: if cmd == "decide" { "auto" } else { "decide" }.to_string(), deadline_ms: None, program: false };
    let kv = match f { None | Some(Json::Null) => return Ok(o), Some(Json::Obj(v)) => v, Some(_) => return Err(flag_err("", "\"flags\" must be an object")) };
    let ms = |k: &str, x: &Json, lo_open: bool| match x.as_f64() { Some(v) if v.is_finite() && v <= 1e9 && (v > 0.0 || (!lo_open && v == 0.0)) => Ok(v),
        _ => Err(flag_err(k, if lo_open { "milliseconds above 0, at most 1e9" } else { "milliseconds from 0 to 1e9" })) };
    let int = |k: &str, x: &Json, lo: u64, hi: u64| match x.as_f64() { Some(v) if v.fract() == 0.0 && v >= lo as f64 && v <= hi as f64 => Ok(v as u64),
        _ => Err(flag_err(k, &format!("an integer from {lo} to {hi}"))) };
    let on = |k: &str, x: &Json| match x { Json::Bool(b) => Ok(*b), _ => Err(flag_err(k, "true or false")) };
    let max = 1u64 << 53;
    for (k, x) in kv {
        match (k.as_str(), cmd) {
            ("budget_ms", _) => { o.budget = ms(k, x, false)?; o.budget_given = true; }
            ("seed", _) => o.seed = int(k, x, 0, max)?,
            ("exact_limit", _) => o.exact_limit = int(k, x, 0, max)?,
            ("exact_ms", _) => o.exact_ms = Some(ms(k, x, false)?),
            ("frontier_states", _) => o.fr_states = int(k, x, 0, max)? as usize,
            ("polish_ms", _) => o.polish_ms = ms(k, x, false)?,
            ("polish_sweeps", _) => o.polish_sweeps = int(k, x, 0, max)? as usize,
            ("sweeps", _) => o.sweeps = int(k, x, 0, max)? as usize,
            ("collective", _) => o.collective = on(k, x)?,
            ("cluster", _) => o.cluster = on(k, x)?,
            ("cycles", _) => o.cycles = on(k, x)?,
            ("chains", _) => o.chains = int(k, x, 1, 100_000)? as usize,
            ("threads", _) => { o.threads = int(k, x, 1, 1024)? as usize;
                if cfg!(all(target_family = "wasm", target_os = "unknown")) && o.threads > 1 { return Err(flag_err(k, "this build has no threads (wasm32-unknown-unknown): 1")); } }
            ("mem_limit_mb", _) => o.mem_mb = int(k, x, 0, max)? as usize,
            ("mode", "decide") => { o.mode = x.as_str().filter(|m| ["auto", "exact", "sample"].contains(m)).ok_or_else(|| flag_err(k, "auto, exact or sample"))?.to_string(); }
            ("op", "run" | "evaluate") => { o.mode = x.as_str().filter(|m| ["decide", "exact", "sample"].contains(m)).ok_or_else(|| flag_err(k, "decide, exact or sample"))?.to_string(); }
            ("deadline_ms", "run" | "evaluate") => o.deadline_ms = Some(ms(k, x, true)?),
            ("program", "evaluate") => o.program = on(k, x)?,
            _ => return Err(flag_err(k, &format!("unknown flag for {cmd} here (see the probbit-wasm crate docs)"))),
        }
    }
    if o.deadline_ms.is_some() && o.sweeps > 0 { return Err(flag_err("deadline_ms", "needs a wall-clock budget: drop sweeps")); }
    Ok(o)
}
/// (--mem-limit-mb, rows per chain), as probbit-cli/src/main.rs `mem_limit`.
fn mem_rows(mb: usize, chains: usize, n_vars: usize) -> (usize, usize) {
    (mb, if mb == 0 { 0 } else { chains.checked_mul(2 * n_vars + 8).map_or(64, |d| (mb.saturating_mul(1_048_576) / d).max(64)) })
}
/// The numeric contract (as probbit-cli/src/main.rs `finish`): a non-finite gate diagnostic only on a refusal, named in
/// `gate.non_finite`; any other non-finite number = no answer, one `numeric` error object, exit 3.
fn finish(mut doc: Json, code: i32) -> (Json, i32) {
    let mut bad = vec![]; json::non_finite(&doc, "", &mut bad);
    if bad.is_empty() { return (doc, code); }
    if code == 3 && bad.iter().all(|p| p.starts_with("gate.")) {
        if let Json::Obj(v) = &mut doc { if let Some((_, Json::Obj(g))) = v.iter_mut().find(|(k, _)| k == "gate") {
            g.push(("non_finite".to_string(), Json::Arr(bad.iter().map(|p| jstr(&p["gate.".len()..])).collect()))); } }
        return (doc, 3);
    }
    let e = InErr { code: "numeric", path: bad[0].clone(), msg: format!("{} computed quantit{} not finite (first: {}); no answer is emitted", bad.len(), if bad.len() == 1 { "y is" } else { "ies are" }, bad[0]) };
    (e.to_json(), 3)
}
/// Run `f` without threads (threads == 1, the browser) or on `threads` scoped threads (native only), then restore the setting.
fn on_threads<T>(threads: usize, f: impl FnOnce() -> T) -> T {
    let prev = probbit_core::rt::set_sequential(threads <= 1); probbit_ir::GATE_THREADS.store(threads, std::sync::atomic::Ordering::Relaxed);
    let r = f(); probbit_core::rt::set_sequential(prev); r
}
/// `probbit run`'s instruction on a parsed program, `phases` appended (main.rs `run_program`).
fn run_prog(mut p: run::Prog, f: &Flags, tp: Instant, parse_ms: f64, compile_ms: f64) -> (Json, i32) {
    p.m.collective = f.collective; p.m.cluster = f.cluster; p.m.cycles = f.cycles; let mut bars = vec![];
    let (mut doc, code) = on_threads(f.threads, || run::run(&p, &f.mode, f.budget, f.seed, f.exact_limit, f.polish_ms, f.polish_sweeps, f.sweeps, f.fr_states, f.chains, f.threads, 100,
        mem_rows(f.mem_mb, f.chains, p.m.n), f.exact_ms, f.deadline_ms.map(|d| (tp, d, f.budget_given)), &mut bars));
    run::phases(&mut doc, parse_ms, compile_ms, f.deadline_ms); (doc, code)
}
fn call(input: &str, cmd: &str) -> (String, i32) {
    let tp = Instant::now();
    let go = || -> Result<(Json, i32), Json> {
        let mut j = json::parse(input).map_err(|e| e.to_json())?;
        let fl = match &mut j { Json::Obj(v) => v.iter().position(|(k, _)| k == "flags").map(|i| v.remove(i).1), _ => None };
        let f = flags(fl.as_ref(), cmd)?;
        let tc = Instant::now(); let parse_ms = (tc - tp).as_secs_f64() * 1e3;
        Ok(match cmd {
            "decide" => {
                let mut n = router::from_json(&j).map_err(|e| e.to_json())?; n.p.collective = f.collective; n.p.cluster = f.cluster; n.p.cycles = f.cycles;
                let o = router::Opts { budget: f.budget, seed: f.seed, exact_limit: f.exact_limit, polish_ms: f.polish_ms, polish_sweeps: f.polish_sweeps, fr_states: f.fr_states,
                    sweeps: f.sweeps, mode: f.mode.clone(), chains: f.chains, threads: f.threads, cpu_pct: 100, xms: f.exact_ms };
                let (doc, code, _) = on_threads(f.threads, || router::decide(&n, &o, &|| mem_rows(f.mem_mb, f.chains, n.p.t))); finish(doc, code) }
            "run" => { let p = run::from_json(&j).map_err(|e| e.to_json())?; let compile_ms = tc.elapsed().as_secs_f64() * 1e3;
                let (doc, code) = run_prog(p, &f, tp, parse_ms, compile_ms); finish(doc, code) }
            _ => { let r = evaluate::compile(&j).map_err(|e| e.to_json())?; if f.program { return Ok((r.program, 0)); }
                let p = run::from_json(&r.program).map_err(|e| evaluate::locate(e).to_json())?; let compile_ms = tc.elapsed().as_secs_f64() * 1e3;
                let (doc, code) = run_prog(p, &f, tp, parse_ms, compile_ms); finish(evaluate::respond(&r, doc), code) }
        })
    };
    let (doc, code) = go().unwrap_or_else(|e| (e, 2));
    (json::write(&doc, false), code)
}

// ---------------- the C ABI (JavaScript) ----------------
thread_local! { static OUT: std::cell::RefCell<(Vec<u8>, i32)> = const { std::cell::RefCell::new((Vec::new(), 0)) }; }
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
#[link(wasm_import_module = "probbit")]
extern "C" { fn now_ms() -> f64; }
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
fn page_clock() -> f64 { unsafe { now_ms() } }

/// A buffer of `len` bytes for `probbit_call`'s input.
#[no_mangle]
pub extern "C" fn probbit_alloc(len: usize) -> *mut u8 { let mut v = Vec::<u8>::with_capacity(len.max(1)); let p = v.as_mut_ptr(); std::mem::forget(v); p }
/// Run op 0 decide, 1 run, 2 evaluate, 3 demo, 4 persona init, 5 persona turn on the UTF-8 JSON at `ptr` (`len` bytes); returns the output's length (read it at
/// `probbit_out_ptr()`, valid until the next call; the exit code at `probbit_out_code()`).
///
/// # Safety
/// `ptr` must come from `probbit_alloc(len)` with `len` bytes written; the buffer is freed here.
#[no_mangle]
pub unsafe extern "C" fn probbit_call(op: u32, ptr: *mut u8, len: usize) -> usize {
    let input = Vec::from_raw_parts(ptr, len, len.max(1));
    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    probbit_core::rt::set_clock(page_clock);
    let (out, code) = match std::str::from_utf8(&input) {
        Err(e) => (json::write(&json::schema("", format!("bad JSON: invalid UTF-8 at byte {}", e.valid_up_to())).to_json(), false), 2),
        Ok(s) => match op { 0 => decide(s), 1 => run(s), 2 => evaluate(s), 3 => demo(s), 4 => persona_init(s), 5 => persona_turn(s), _ => (json::write(&json::value("", format!("unknown op {op}")).to_json(), false), 2) } };
    let n = out.len(); OUT.with(|o| *o.borrow_mut() = (out.into_bytes(), code)); n
}
/// The last output's bytes.
#[no_mangle]
pub extern "C" fn probbit_out_ptr() -> *const u8 { OUT.with(|o| o.borrow().0.as_ptr()) }
/// The last output's exit code (as the CLI's).
#[no_mangle]
pub extern "C" fn probbit_out_code() -> i32 { OUT.with(|o| o.borrow().1) }
