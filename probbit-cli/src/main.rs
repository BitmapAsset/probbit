//! `probbit` — a virtual probabilistic (p-bit) processor for joint decisions: exact where structure allows, sampled with
//! published diagnostics otherwise.
//!
//! JSON problem in on stdin, JSON decision out on stdout. Zero external crates.
//!
//!   probbit decide [--budget-ms N] [--seed N] [--exact-limit N] [--exact-ms N] [--frontier-states N] [--polish-ms N] [--polish-sweeps N] [--mode auto|exact|sample] [--sweeps N] [--collective on|off] [--cluster on|off] [--cycles on|off] [--chains N] [--threads N] [--cpu-limit PCT] [--mem-limit-mb N] [--priority low|normal] [--max-input-mb N] [--pretty]
//!   probbit demo   [--tasks N] [--seed N] [--hard] # emit an agent-routing problem as JSON; --hard = tight quotas + strong affinity
//!   probbit ir [--max-input-mb N]                # emit the problem in probbit-ir v0 (the hardware-facing text form)
//!   probbit run [--op decide|exact|sample] [--budget-ms N] [--deadline-ms N] [--seed N] [--exact-limit N] [--exact-ms N] [--frontier-states N] [--polish-ms N] [--polish-sweeps N] [--sweeps N] [--collective on|off] [--cluster on|off] [--cycles on|off] [--chains N] [--threads N] [--cpu-limit PCT] [--mem-limit-mb N] [--priority low|normal] [--max-input-mb N] [--pretty]  # a general probbit-ir JSON program
//!   probbit evaluate [run's flags] [--program]  # the decision-API adapter: a System One request + the judge's answers + rules -> the joint answer
//!   --progress [MS] (decide, run, evaluate): one JSONL telemetry line on stderr every MS ms (default 100) while the decision runs
//!   probbit stats [--sweeps N]                  # the processor's spec sheet: machine, build, effective controls + source, measured updates/s
//!   probbit persona init|turn|replay|explain|diff|lint|fuzz|prove|check|compile|describe PERSONA [flags]  # the individuality layer (docs/persona.md)
//!   probbit mcp                                 # a Model Context Protocol server on stdio (tools probbit_decide, probbit_run, probbit_stats, probbit_demo, probbit_evaluate, probbit_persona_init, probbit_persona_turn, probbit_persona_fuzz)
//!   probbit version
//!   probbit <command> --help | -h               # usage, every flag with its default, exit codes
//!   probbit --help | -h                          # the usage above (stdout, exit 0); at a terminal the hero screen
//!   --top, --summary (decide, run), --live (demo), --plain: see `probbit <command> --help`; visuals go to stderr (theme.rs)
mod evaluate;
mod fuzz;
mod json;
mod live;
mod mcp;
mod monitor;
mod persona;
mod router;
mod run;
mod sys;
mod theme;
mod tui;
mod yaml;
use json::{num, obj, str as jstr, Json};
pub(crate) use router::{demo_doc, from_json, item_bars, DEMO_TPL};
use probbit_decide::*;
use std::io::{Read, Write};

const VERSION: &str = env!("CARGO_PKG_VERSION");


/// Every stderr line goes through here. `eprintln!` panicked when stderr was a pipe whose reader had gone (`probbit decide --bogus
/// 2>&1 >/dev/null | true`, or `--progress` into `head -c 1`: exit 134 with panic = abort, the decision lost to a log line). A
/// failed write to stderr is ignored: the command keeps its stdout and its exit code.
fn err_line(s: &str) { let _ = writeln!(std::io::stderr(), "{s}"); }
fn fail(msg: &str) -> ! { tui::top_stop(); err_line(&format!("probbit: {msg}")); exit(2) }
/// Exit with `code`, releasing a strand's writer lock this process holds first (live.rs `Lock`; `std::process::exit` skips `Drop`)
fn exit(code: i32) -> ! { live::release_held(); std::process::exit(code) }
/// `--budget-ms inf` / `1e300` aborted (exit 134: the deadline Duration overflowed) and `NaN` never stopped sampling
fn budget_ms(args: &[String]) -> f64 {
    let b: f64 = arg(args, "--budget-ms", 200.0);
    if !(b.is_finite() && (0.0..=1e9).contains(&b)) { fail("--budget-ms must be a number of milliseconds from 0 to 1e9") } b
}
/// `--polish-ms inf` / `NaN` hung `probbit decide` and `probbit run` (the polish loops while elapsed < the budget) and `-7` ran
/// (echoed in telemetry); the same range as --budget-ms, else exit 2
/// R19.6 (P2.1) `--deadline-ms N` (`probbit run` only): a whole-call wall-clock target, see `run::run`.
fn deadline_ms(args: &[String]) -> Option<f64> {
    args.iter().any(|a| a == "--deadline-ms").then(|| { let x: f64 = arg(args, "--deadline-ms", 0.0);
        if !(x.is_finite() && x > 0.0 && x <= 1e9) { fail("--deadline-ms must be a number of milliseconds above 0, at most 1e9") } x })
}
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
/// Every stdout document goes through here. `println!` panicked on a closed pipe (`probbit demo --tasks 3000 | head -c 20`:
/// "failed printing to stdout: Broken pipe", exit 134 with panic = abort); a reader that went away is not an error, so a broken
/// pipe is ignored and the command keeps its normal exit code.
fn emit_raw(s: &str) {
    let mut o = std::io::stdout().lock();
    if let Err(e) = o.write_all(s.as_bytes()).and_then(|_| o.flush()) {
        if e.kind() != std::io::ErrorKind::BrokenPipe { err_line(&format!("probbit: cannot write the output: {e}")); exit(2) } }
}
fn emit(s: &str) { emit_raw(&format!("{s}\n")) }

/// Bad input (JSON, schema, values): ONE structured error object on stdout, a human line on stderr, exit 2.
fn bad_input(what: &str, e: json::InErr) -> ! {
    err_line(&format!("probbit: {what}{}{}", if e.path.is_empty() { String::new() } else { format!("{}: ", e.path) }, e.msg));
    emit(&json::write(&e.to_json(), false)); exit(2)
}
/// Emit a decision; returns its exit code. Every number in it must be finite (docs/probbit-ir-json.md "Numeric contract"). A
/// gate diagnostic that could not be estimated (R-hat / bound infinite: too few samples, chains stuck at different values) is
/// allowed only on a refusal (exit 3): it prints as null and is named in `gate.non_finite`. Any other non-finite number = no
/// answer: ONE structured `numeric` error on stdout, exit 3 (it used to print `null`, or a literal `inf`, with exit 0).
/// How a decision is printed (decide, run): `--pretty`; `--summary` = the compact document (`summary_doc`); with both and a
/// terminal on stderr (and no NO_COLOR / --plain / PROBBIT_THEME=plain), the summary box there too.
pub(crate) struct View { pretty: bool, summary: bool, cmd: &'static str, boxed: Option<theme::Theme> }
impl View {
    fn new(args: &[String], cmd: &'static str) -> View {
        let (pretty, summary) = (args.iter().any(|a| a == "--pretty"), args.iter().any(|a| a == "--summary"));
        View { pretty, summary, cmd, boxed: if pretty && summary { theme::stderr(args) } else { None } }
    }
}
/// `--summary`: the answer without its per-item tables (`plan`, `odds` / `marginals`, `released` / `escalated` lists,
/// `release_reason`, `top_plans`), every other field as is, plus `"summary": 1`, `counts` (items, released, escalated) and the
/// items an agent should look at first: `worst_released` (up to 5: on a sampled answer the largest error bars, on an exact one
/// the smallest gap between the top two odds) and `worst_escalated` (up to 5, largest error bars), each with its id, plan value
/// and that value's probability `p`, top two odds, release reason and error bar (`bar` = z x max(MCSE short, MCSE long), the number the gate compares with
/// `tv_tol`; null where it is not estimable). Same verdict and exit code as the full document.
fn summary_doc(doc: &Json, bars: &[f64], cmd: &str) -> Json {
    let Json::Obj(v) = doc else { return doc.clone() };
    let ids = |k: &str| doc.get(k).and_then(Json::as_arr).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>());
    let (released, escalated) = (ids("released"), ids("escalated"));
    let odds = doc.get(if cmd == "decide" { "odds" } else { "marginals" }); let plan = doc.get("plan"); let reasons = doc.get("release_reason");
    let pos: std::collections::HashMap<&str, usize> = plan.or(odds).and_then(Json::as_obj).map_or_else(Default::default, |o| o.iter().enumerate().map(|(i, (k, _))| (k.as_str(), i)).collect());
    let bar = |id: &str| pos.get(id).and_then(|&i| bars.get(i)).copied();
    let margin = |id: &str| odds.and_then(|o| o.get(id)).and_then(Json::as_obj).map_or(1.0, |o| { let p: Vec<f64> = o.iter().filter_map(|(_, x)| x.as_f64()).collect();
        p.first().copied().unwrap_or(1.0) - p.get(1).copied().unwrap_or(0.0) });
    let key = |id: &str| if bars.is_empty() { margin(id) } else { bar(id).map_or(f64::INFINITY, |b| if b.is_nan() { f64::NEG_INFINITY } else { -b }) };
    let item = |id: &str| { let mut f = vec![("id", jstr(id))];
        if let Some(x) = plan.and_then(|p| p.get(id)) { f.push(("value", x.clone()));
            // the plan's own value and its probability: the joint plan need not hold each item's most likely value
            if let Some(q) = x.as_str().and_then(|w| odds.and_then(|o| o.get(id)).and_then(|o| o.get(w))) { f.push(("p", q.clone())); } }
        f.push(("odds", Json::Obj(odds.and_then(|o| o.get(id)).and_then(Json::as_obj).map_or(vec![], |o| o.iter().take(2).cloned().collect()))));
        if let Some(r) = reasons.and_then(|r| r.get(id)) { f.push(("reason", r.clone())); }
        if !bars.is_empty() { f.push(("bar", bar(id).filter(|b| b.is_finite()).map_or(Json::Null, num))); }
        obj(f) };
    let worst = |l: &[String]| { let mut l: Vec<&String> = l.iter().collect(); l.sort_by(|a, b| key(a).total_cmp(&key(b))); Json::Arr(l.into_iter().take(5).map(|id| item(id)).collect()) };
    let n = if cmd == "decide" { doc.get("tasks").cloned() } else { doc.get("program").and_then(|p| p.get("vars")).cloned() };
    let mut counts = vec![(if cmd == "decide" { "tasks" } else { "vars" }, n.unwrap_or(Json::Null))];
    if let (Some(r), Some(e)) = (&released, &escalated) { counts.push(("released", num(r.len() as f64))); counts.push(("escalated", num(e.len() as f64))); }
    // counts go right after the verdict (and its tier / reason)
    let mut out: Vec<(String, Json)> = vec![]; let mut after_verdict = false;
    for (k, x) in v {
        if after_verdict && k != "tier" && k != "reason" { out.push(("counts".into(), obj(std::mem::take(&mut counts)))); after_verdict = false; }
        match k.as_str() {
            "plan" | "odds" | "marginals" | "release_reason" | "top_plans" | "escalated" => {}
            "released" => {
                if let Some(r) = &released { out.push(("worst_released".into(), worst(r))); }
                if let Some(e) = &escalated { out.push(("worst_escalated".into(), worst(e))); } }
            _ => { out.push((k.clone(), x.clone()));
                if k == "engine" { out.push(("summary".into(), num(1.0))); }
                if k == "verdict" { after_verdict = true; } } } }
    if after_verdict { out.push(("counts".into(), obj(counts))); }
    Json::Obj(out)
}
fn finish(mut doc: Json, code: i32, view: &View, bars: &[f64]) -> i32 {
    tui::top_stop();
    let mut bad = vec![]; json::non_finite(&doc, "", &mut bad);
    if !bad.is_empty() {
        if code == 3 && bad.iter().all(|p| p.starts_with("gate.")) {
            if let Json::Obj(v) = &mut doc { if let Some((_, Json::Obj(g))) = v.iter_mut().find(|(k, _)| k == "gate") {
                g.push(("non_finite".to_string(), Json::Arr(bad.iter().map(|p| jstr(&p["gate.".len()..])).collect()))); } }
        } else {
            let e = json::InErr { code: "numeric", path: bad[0].clone(), msg: format!("{} computed quantit{} not finite (first: {}); no answer is emitted", bad.len(), if bad.len() == 1 { "y is" } else { "ies are" }, bad[0]) };
            err_line(&format!("probbit: numeric failure: {}", e.msg)); emit(&json::write(&e.to_json(), false)); return 3;
        }
    }
    if view.summary { doc = summary_doc(&doc, bars, view.cmd); }
    emit(&json::write(&doc, view.pretty));
    if let Some(th) = view.boxed { let _ = std::io::stderr().write_all(tui::summary_box(th, view.cmd, &doc).as_bytes()); }
    code
}
/// stdin -> a parsed document, or `bad_input`
fn read_doc(args: &[String]) -> Json { json::parse(&read_stdin(args)).unwrap_or_else(|e| bad_input(if e.code == "schema" { "" } else { "bad input: " }, e)) }


/// A flag that is present must parse (exit 2 otherwise). It used to fall back to the default silently: `--sweeps 1e4`
/// ran in wall-clock mode (not the fixed-work, deterministic mode asked for) and `--seed x` ran seed 7.
fn arg<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    match args.iter().position(|a| a == name) { None => default, Some(k) => match args.get(k + 1) {
        Some(v) => v.parse().unwrap_or_else(|_| fail(&format!("{name}: cannot read {v:?} (integer flags take plain digits, e.g. 10000 not 1e4)"))),
        None => fail(&format!("{name} needs a value")) } }
}
/// Flags taking a value, per command (the controls are shared by decide, run and stats).
const CONTROLS: [&str; 5] = ["--chains", "--threads", "--cpu-limit", "--mem-limit-mb", "--priority"];
/// Per command: (flags taking a value, switches); `check_flags` enforces exactly these and `help` lists exactly these (R19.8).
fn flags_of(cmd: &str) -> (Vec<&'static str>, Vec<&'static str>) {
    match cmd {
        "decide" => ([&SEARCH[..], &CONTROLS[..], &["--mode", "--max-input-mb"]].concat(), vec!["--pretty", "--progress", "--summary", "--top", "--plain"]),
        "run" => ([&SEARCH[..], &CONTROLS[..], &["--op", "--deadline-ms", "--max-input-mb"]].concat(), vec!["--pretty", "--progress", "--summary", "--top", "--plain"]),
        "evaluate" => ([&SEARCH[..], &CONTROLS[..], &["--op", "--deadline-ms", "--max-input-mb"]].concat(), vec!["--pretty", "--progress", "--summary", "--top", "--plain", "--program"]),
        "stats" => ([&CONTROLS[..], &["--sweeps"]].concat(), vec!["--pretty", "--plain"]),
        "demo" => (vec!["--tasks", "--seed"], vec!["--hard", "--live", "--plain"]),
        "ir" => (vec!["--max-input-mb"], vec!["--plain"]),
        _ => (vec![], vec![]),
    }
}
/// One line per flag for `help` (R19.8, P3.1); `help` panics on a flag without a line (test `every_command_has_help`).
const FLAG_HELP: [(&str, &str); 29] = [
    ("--budget-ms", "N    sampler wall-clock budget, ms (default 200)"), ("--seed", "N    random seed (default 7)"),
    ("--exact-limit", "N    plans the exact enumeration may count before it declines (default 2000000)"),
    ("--exact-ms", "N    wall-clock cap on the exact tiers that run before the sampler (opt-in; absent = no cap)"),
    ("--frontier-states", "N    state cap of the frontier DP and the components tier (default 4096; 0 = off)"),
    ("--polish-ms", "N    plan polish budget, ms (default 50; 0 = off)"), ("--polish-sweeps", "N    fixed-work plan polish (deterministic)"),
    ("--sweeps", "N    fixed work per chain instead of the wall-clock budget (with --polish-ms 0: a pure function of input + seed)"),
    ("--collective", "on|off    global two-value flips + label swaps (default on)"), ("--cluster", "on|off    Wolff cluster move (default on)"),
    ("--cycles", "on|off    three-cycle rotations (default on)"), ("--chains", "N    sampler chains, 1..=100000 (default 4)"),
    ("--threads", "N    worker threads for the chains, the gate and the polish, 1..=1024 (default min(cores, 4))"),
    ("--cpu-limit", "PCT    per-thread duty cycle 1..100 (default 100)"), ("--mem-limit-mb", "N    cap on stored samples (default 1024; 0 = none)"),
    ("--priority", "low|normal    OS scheduling priority (default normal)"), ("--mode", "auto|exact|sample    which tiers may answer (default auto)"),
    ("--op", "decide|exact|sample    decide = exact tiers, then the sampler; exact / sample = only those (default decide)"),
    ("--deadline-ms", "N    whole-call deadline (exact N/4, then sampler + polish); answer has `deadline` + `phases`"),
    ("--pretty", "indented JSON"), ("--progress", "[MS]    one JSONL telemetry line on stderr every MS ms (default 100)"),
    ("--tasks", "N    tasks to generate (default 12)"), ("--hard", "tight quotas + strong affinity"),
    ("--max-input-mb", "N    largest stdin document read, MB (default 256; 0 = no limit)"),
    ("--summary", "compact answer: verdict, counts, gate, the 5 worst released and escalated items, telemetry (no per-item tables);\n      with --pretty and a terminal on stderr, also a boxed summary there"),
    ("--top", "live monitor on stderr while it runs (tier, sweeps, updates/s, CPU, peak RSS, gate; 10 Hz), erased at exit;\n      only with a terminal on stderr; stdout unchanged"),
    ("--live", "the router story on stderr: exact tiers, then a live field of p-bits from the real chains, the gate, the verdict\n      (default when stdout and stderr are both terminals; --tasks defaults to 300; stdout, if not a terminal, gets the JSON problem)"),
    ("--plain", "no colour, no animation (as NO_COLOR=1 or PROBBIT_THEME=plain); output otherwise unchanged"),
    ("--program", "print the compiled probbit-ir program instead of running it (`probbit evaluate --program | probbit run` = the same answer)"),
];
/// `probbit <command> --help` / `-h` (R19.8, P3.1): what it does, every flag it accepts, exit codes. None = not a command.
fn help(cmd: &str) -> Option<String> {
    let (what, usage) = match cmd {
        "decide" => ("Route tasks to workers: a router document (workers + caps, tasks + allowed + scores, affinity; README) on stdin,\n  a JSON decision on stdout. Exact where structure allows, else sampled with the diagnostics gate.", "probbit decide [flags] < router.json"),
        "run" => ("Run a probbit-ir program (variables, values, scores, rules; docs/probbit-ir-json.md) on stdin; JSON answer on stdout.", "probbit run [flags] < program.json"),
        "evaluate" => ("The decision-API adapter: a System One request (questions noul | choice | score) plus a \"probbit\" block (the judge's\n  answers, rules over question ids) on stdin; the rule-abiding joint answer in the judge's response shape, odds per question\n  and the gate's verdict on stdout. Every `probbit run` flag applies. docs/probbit-ir-json.md \"Decision API\".", "probbit evaluate [flags] < request.json"),
        "demo" => ("Emit a synthetic agent-routing document (JSON) for `probbit decide`.", "probbit demo [flags] > router.json"),
        "ir" => ("Print a router document as probbit-ir v0 text (the hardware-facing form).", "probbit ir < router.json"),
        "stats" => ("The processor's spec sheet: machine, build, effective controls + their source, measured updates/s.", "probbit stats [flags]"),
        "mcp" => ("A Model Context Protocol server on stdio (JSON-RPC 2.0, one message per line; logs on stderr). Tools probbit_decide,\n  probbit_run, probbit_stats, probbit_demo, probbit_evaluate: the commands' own JSON in and out. Exits when stdin closes. docs/agents.md.", "probbit mcp"),
        "persona" => (PERSONA_HELP, "probbit persona <init|turn|replay|explain|diff|lint|fuzz|prove|check|compile|describe> PERSONA [flags]"),
        "live" => (LIVE_HELP, "probbit live PERSONA [--seed N | --state FILE] [--strand FILE] [--events FILE [--watch]] [--clock real|fixed] [--checkpoint-every K] | probbit live PERSONA --demo week [--seed N] [--strand FILE] [--plain] | probbit live verify STRAND [--from-checkpoint] | probbit live control STRAND pause|resume|retire --by WHO --reason TEXT [--at TIME]"),
        "version" => ("Print the version.", "probbit version"), _ => return None };
    let (vals, sw) = flags_of(cmd); let mut h = format!("usage: {usage}\n  {what}\n");
    if !vals.is_empty() || !sw.is_empty() { h.push_str("flags:\n"); }
    for f in vals.iter().chain(&sw) {
        let d = if cmd == "stats" && *f == "--sweeps" { "N    sweeps of the throughput measurement (default 2000)" }
            else { FLAG_HELP.iter().find(|(k, _)| k == f).unwrap_or_else(|| panic!("no help line for {f}")).1 };
        h.push_str(&format!("  {f} {d}\n")); }
    if ["decide", "run", "evaluate", "stats"].contains(&cmd) { h.push_str("controls: flag > PROBBIT_* environment > probbit.json > default.\n"); }
    if cmd == "live" { h.push_str("exit: 0 done (live: every event answered; verify: every line replays; control: the line appended), 1 verify: a line differs\n  (its number and what), 2 a bad persona, state, event (one {\"error\"} object on stdout per bad event; the run goes on), control\n  line or flag, 4 refused: the strand's writer lock is held by another writer, or the individual is paused or retired (one\n  {\"error\"} object per refused event; nothing is written).\n"); }
    if cmd == "persona" { h.push_str("exit: 0 done (every turn status, refusals and fallbacks included, is an answer; fuzz: no counterexample found; prove: every rule\n  held or proved), 1 lint found an unresolved contradiction or (with rules) a counterexample, fuzz found a counterexample or prove left a rule\n  unknown, 2 bad persona / state / inputs / script / rule (one {\"error\"}\n  object on stdout, code \"persona\") or bad flag (stderr).\n"); }
    if ["decide", "run", "evaluate"].contains(&cmd) { h.push_str("exit: 0 answer (exact | diagnostics_passed | partial), 1 infeasible (a proof), 2 bad input (one {\"error\"} object\n  on stdout) or bad flag (stderr), 3 refused / declined / non-finite result.\n"); }
    Some(h)
}
const SEARCH: [&str; 11] = ["--budget-ms", "--seed", "--exact-limit", "--exact-ms", "--frontier-states", "--polish-ms", "--polish-sweeps", "--sweeps", "--collective", "--cluster", "--cycles"];
/// `--collective on|off` (default on, R19 P1.1): the sampler's global two-value flip (`Problem::collective` / `Model::collective`).
fn collective_arg(args: &[String]) -> bool {
    match arg(args, "--collective", "on".to_string()).as_str() { "on" => true, "off" => false, _ => fail("--collective must be on or off") }
}
/// `--cluster on|off` (DEFAULT ON since R19.4, P1.4: the frozen-threshold holdout's rare-modes family gave 8/8 false whole
/// answers without it and 0/20 with it; R19 P1.1(d)): the sampler's Wolff cluster move (`Problem::cluster` / `Model::cluster`).
fn cluster_arg(args: &[String]) -> bool {
    match arg(args, "--cluster", "on".to_string()).as_str() { "on" => true, "off" => false, _ => fail("--cluster must be on or off") }
}
/// `--cycles on|off` (default on since R19.5, P1.4 calibration): the sampler's three-cycle rotations (`Problem::cycles` /
/// `Model::cycles`; attempted only with k > 2 values and caps).
fn cycles_arg(args: &[String]) -> bool {
    match arg(args, "--cycles", "on".to_string()).as_str() { "on" => true, "off" => false, _ => fail("--cycles must be on or off") }
}
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
        else { fail(&format!("unknown argument {a:?} for `probbit {}` (run `probbit` for usage)", args[0])) } }
}
/// Optional config file: `$PROBBIT_CONFIG`, else `./probbit.json` if present (keys chains, threads, cpu_limit, mem_limit_mb, priority).
fn config_path() -> Option<String> { std::env::var("PROBBIT_CONFIG").ok().or_else(|| std::path::Path::new("probbit.json").exists().then(|| "probbit.json".to_string())) }
const CONFIG_KEYS: [&str; 5] = ["chains", "threads", "cpu_limit", "mem_limit_mb", "priority"];
fn config() -> Option<json::Json> {
    let path = config_path()?;
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| fail(&format!("config {path}: {e}")));
    let c = json::parse(&src).unwrap_or_else(|e| fail(&format!("config {path}: {}", e.msg)));
    // An unknown key (`{"thread": 3}`) or a non-object was ignored silently: exit 2, as for the input documents' unknown fields
    match &c { Json::Obj(kv) => if let Some((k, _)) = kv.iter().find(|(k, _)| !CONFIG_KEYS.contains(&k.as_str())) { fail(&format!("config {path}: unknown key \"{k}\" (known: {})", CONFIG_KEYS.join(", "))) },
        _ => fail(&format!("config {path}: must be a JSON object")) }
    Some(c)
}
/// The config file in the telemetry of `decide` and `run` (absent without one): it can change the answer of an otherwise
/// identical command (`chains` is part of the determinism key), and nothing in the output said it was there.
pub(crate) fn config_echo() -> Vec<(&'static str, Json)> { config_path().map_or(vec![], |p| vec![("config", jstr(&p))]) }
/// A config-file value as text (numbers must be whole; anything else fails the caller's parse).
fn cfg_val(key: &str) -> Option<String> {
    let c = config()?; let v = c.get(key)?;
    Some(if let Some(x) = v.as_f64() { if x.fract() == 0.0 && x >= 0.0 { format!("{}", x as u64) } else { format!("{x}") } } else { v.as_str().map_or("?".into(), str::to_string) })
}
/// Processor-style control: CLI flag > `PROBBIT_*` environment variable > config file > default; must be in 1..=max.
fn ctl(args: &[String], name: &str, env: &str, default: usize, max: usize) -> usize { ctl_in(args, name, env, default, 1, max) }
fn ctl_min(args: &[String], name: &str, env: &str, default: usize, min: usize) -> usize { ctl_in(args, name, env, default, min, usize::MAX) }
fn ctl_in(args: &[String], name: &str, env: &str, default: usize, min: usize, max: usize) -> usize {
    let v = args.iter().position(|a| a == name).and_then(|k| args.get(k + 1)).map(|v| v.to_string()).or_else(|| std::env::var(env).ok())
        .or_else(|| cfg_val(&name[2..].replace('-', "_")));
    match v { None => default, Some(v) => match v.parse::<usize>() { Ok(n) if n >= min && n <= max => n,
        _ => fail(&if max == usize::MAX { format!("{name} / {env} must be an integer >= {min}") } else { format!("{name} / {env} must be an integer from {min} to {max}") }) } }
}
/// Upper bounds of --chains and --threads (from any source): `--chains 576460752303423488` aborted (`capacity overflow`, exit 134)
/// and `--chains 1000000000` exhausted memory. 100,000 chains is the largest count measured (BENCHMARKS §6); the sampler uses
/// at most `chains` threads, so 1,024 covers any machine.
const MAX_CHAINS: usize = 100_000;
const MAX_THREADS: usize = 1024;
/// The default memory cap (MB). Unbounded, sample memory grew ~190 MB per second of budget on the 300-task demo,
/// so a long `--budget-ms` could exhaust a small machine; 1024 MB never thins below ~8 s there (inferred from rows per chain).
const DEFAULT_MEM_LIMIT_MB: usize = 1024;
/// `--mem-limit-mb N` (+ PROBBIT_MEM_LIMIT_MB, config key mem_limit_mb; default now 1024, 0 = unbounded) caps the sample
/// buffers that grow with the budget (per-chain trajectory n x 2 bytes + trace 8 bytes per kept sweep) at N MB in total by thinning
/// (`probbit_ir::record_row`), never below 64 rows per chain. Fixed costs (program, JSON, marginals) come on top. -> (N, rows per chain)
fn mem_limit(args: &[String], chains: usize, n_vars: usize) -> (usize, usize) {
    let mb = ctl_min(args, "--mem-limit-mb", "PROBBIT_MEM_LIMIT_MB", DEFAULT_MEM_LIMIT_MB, 0);
    (mb, if mb == 0 { 0 } else { chains.checked_mul(2 * n_vars + 8).map_or(64, |d| (mb.saturating_mul(1_048_576) / d).max(64)) })
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
    probbit_ir::PROGRESS_SWEEPS.store(0, Relaxed); probbit_ir::PROGRESS_ON.store(true, Relaxed);
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let h = std::thread::spawn(move || { let t0 = std::time::Instant::now(); let (mut lt, mut ls) = (0.0f64, 0u64);
        while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(std::time::Duration::from_millis(every)) {
            let (t, s) = (t0.elapsed().as_secs_f64(), probbit_ir::PROGRESS_SWEEPS.load(Relaxed)); let u = sys::usage();
            let line = obj(vec![("event", jstr("progress")), ("ms", num((t * 1e3).round())), ("sweeps", num(s as f64)), ("site_updates_per_s", num(if t > lt { ((s - ls) as f64 * n_vars as f64 / (t - lt)).round() } else { 0.0 })),
                ("process_cpu_ms", u.map_or(Json::Null, |u| num(u.0.round()))), ("peak_rss_mb", u.map_or(Json::Null, |u| num(u.1)))]);
            err_line(&json::write(&line, false)); (lt, ls) = (t, s); } });
    Some(Monitor { tx: Some(tx), h: Some(h) })
}
/// `--max-input-mb N` (decide, run, ir; default 256, 0 = no limit): the largest stdin document read. Parsing costs up to ~9x
/// the input in memory (a 200 MB numeric array peaked at 1.78 GB), so a larger input is a `limit` error before it is parsed.
const DEFAULT_MAX_INPUT_MB: usize = 256;
/// stdin -> text, or `bad_input`: ONE error object on stdout, exit 2. An unreadable stdin (`probbit decide < /`) and invalid UTF-8
/// went to `fail` (a stderr line, stdout empty), although both are document content, not flags.
fn read_stdin(args: &[String]) -> String {
    let mb: usize = arg(args, "--max-input-mb", DEFAULT_MAX_INPUT_MB); let cap = mb.saturating_mul(1_048_576); let mut b = Vec::new();
    if let Err(e) = std::io::stdin().lock().take(if mb == 0 { u64::MAX } else { (cap as u64).saturating_add(1) }).read_to_end(&mut b) {
        bad_input("", json::schema("", format!("cannot read stdin: {e}"))) }
    if mb > 0 && b.len() > cap { bad_input("bad input: ", json::limit("", format!("stdin holds more than --max-input-mb {mb} MB"))) }
    // One leading UTF-8 byte-order mark is skipped (RFC 8259 lets a parser ignore it): Windows PowerShell 5.1 adds one to every
    // pipe into a native program, so `probbit demo | probbit decide` exited 2 there (`unexpected character` at byte 0)
    if b.starts_with(&[0xEF, 0xBB, 0xBF]) { b.drain(..3); }
    String::from_utf8(b).unwrap_or_else(|e| bad_input("", json::schema("", format!("bad JSON: invalid UTF-8 at byte {}", e.utf8_error().valid_up_to()))))
}
/// --priority low|normal (flag > PROBBIT_PRIORITY > config > normal), applied before any chain thread starts.
fn priority(args: &[String]) {
    match args.iter().position(|a| a == "--priority").and_then(|k| args.get(k + 1)).cloned().or_else(|| std::env::var("PROBBIT_PRIORITY").ok()).or_else(|| cfg_val("priority")).as_deref() {
        None | Some("normal") => {}, Some("low") => { if sys::set_low().is_none() { fail("--priority low: setpriority failed"); } },
        Some(_) => fail("--priority / PROBBIT_PRIORITY must be low or normal") }
}
/// The processor controls shared by `probbit decide` and `probbit run`: (chains, threads, cpu_limit_pct).
/// R19.8: `--threads` also bounds the gate's per-chain passes (`probbit_ir::GATE_THREADS`; one thread per chain before).
fn controls(args: &[String]) -> (usize, usize, u32) {
    let r = (ctl(args, "--chains", "PROBBIT_CHAINS", 4, MAX_CHAINS), ctl(args, "--threads", "PROBBIT_THREADS", std::thread::available_parallelism().map_or(1, |n| n.get()).min(4), MAX_THREADS),
     { let c = ctl(args, "--cpu-limit", "PROBBIT_CPU_LIMIT", 100, usize::MAX); if c > 100 { fail("--cpu-limit / PROBBIT_CPU_LIMIT must be 1..=100"); } c as u32 });
    probbit_ir::GATE_THREADS.store(r.1, std::sync::atomic::Ordering::Relaxed); r
}

fn decide_cmd(args: &[String]) {
    let (v, w) = flags_of("decide"); check_flags(args, &v, &w);
    let budget: f64 = budget_ms(args); let seed: u64 = arg(args, "--seed", 7);
    let exact_limit: u64 = arg(args, "--exact-limit", 2_000_000); let polish_ms: f64 = polish_ms_arg(args);
    // Any other --mode value (`foo`, `EXACT`, or `--pretty` swallowed as the value) used to run the sampler silently (exit 0)
    let mode: String = arg(args, "--mode", "auto".to_string()); let view = View::new(args, "decide");
    if !["auto", "exact", "sample"].contains(&mode.as_str()) { fail("--mode must be auto, exact or sample") }
    // --polish-sweeps N = fixed-work polish (deterministic); parsed here, so a bad value exits 2 before sampling
    let polish_sweeps: usize = arg(args, "--polish-sweeps", 0);
    let fr_states: usize = arg(args, "--frontier-states", FRONTIER_MAX_STATES); let sweeps: usize = arg(args, "--sweeps", 0);
    let (chains, threads, cpu_pct) = controls(args); priority(args); let top = top_wanted(args);
    let j = read_doc(args);
    let mut n = from_json(&j).unwrap_or_else(|e| bad_input("bad problem: ", e)); n.p.collective = collective_arg(args); n.p.cluster = cluster_arg(args); n.p.cycles = cycles_arg(args); let p = &n.p;
    let _mon = monitor(args, p.t);
    if let Some(th) = top { tui::tier("exact tiers"); tui::top_start(th, tui::TopSpec { cmd: "decide", vars: p.t, chains, threads: threads.clamp(1, chains), budget_ms: budget, sweeps, deadline_ms: None }); }
    let o = router::Opts { budget, seed, exact_limit, polish_ms, polish_sweeps, fr_states, sweeps, mode, chains, threads, cpu_pct, xms: exact_ms(args) };
    let (doc, code, bars) = router::decide(&n, &o, &|| mem_limit(args, chains, p.t));
    let c = finish(doc, code, &view, &bars); if c != 0 { std::process::exit(c); }
}
/// `--top` asked for and drawable (a terminal on stderr, not plain); exit 2 with `--progress`, which writes the same stream.
fn top_wanted(args: &[String]) -> Option<theme::Theme> {
    if !args.iter().any(|a| a == "--top") { return None; }
    if args.iter().any(|a| a == "--progress") { fail("--top and --progress both write to stderr; use one"); }
    theme::stderr(args)
}

/// Where a control's value comes from (same precedence as `ctl`): "flag" > "env" > "file" > "default".
fn source(args: &[String], name: &str, env: &str) -> &'static str {
    if args.iter().any(|a| a == name) { "flag" } else if std::env::var(env).is_ok() { "env" } else if cfg_val(&name[2..].replace('-', "_")).is_some() { "file" } else { "default" }
}
/// The SIMD features this binary was built with (the ones the kernels can use), in `probbit stats` order.
pub(crate) fn target_features() -> Vec<&'static str> {
    [("sse4.2", cfg!(target_feature = "sse4.2")), ("avx", cfg!(target_feature = "avx")), ("avx2", cfg!(target_feature = "avx2")), ("fma", cfg!(target_feature = "fma")),
        ("bmi2", cfg!(target_feature = "bmi2")), ("avx512f", cfg!(target_feature = "avx512f")), ("neon", cfg!(target_feature = "neon")), ("dotprod", cfg!(target_feature = "dotprod"))]
        .iter().filter(|f| f.1).map(|f| f.0).collect()
}
/// The self-test program of `probbit stats` and the hero screen: an `n`-spin ring with a weak field (probbit-ir JSON).
pub(crate) fn ring(n: usize) -> String {
    format!("{{\"probbit_ir\": 1, \"values\": [\"-\", \"+\"], \"vars\": [{}], \"pairs\": [{}]}}",
        (0..n).map(|i| format!("{{\"id\": \"s{i}\", \"h\": {{\"+\": {}}}}}", 0.1 * ((i % 7) as f64 - 3.0))).collect::<Vec<_>>().join(","),
        (0..n).map(|i| format!("{{\"i\": \"s{i}\", \"j\": \"s{}\", \"table\": [[0.4, -0.4], [-0.4, 0.4]]}}", (i + 1) % n)).collect::<Vec<_>>().join(","))
}
/// Rows per chain under the default memory cap (`mem_limit` without flag, variable or config)
pub(crate) fn default_max_rows(chains: usize, n_vars: usize) -> usize { chains.checked_mul(2 * n_vars + 8).map_or(64, |d| (DEFAULT_MEM_LIMIT_MB.saturating_mul(1_048_576) / d).max(64)) }
/// `probbit stats` — the processor's spec sheet: machine, build target features, effective controls and where each came
/// from, and a measured self-test (400-spin ring, 4 chains x `--sweeps` fixed work, IR sampler) at 1 thread and at the effective
/// thread count, after one discarded warm-up pass. Nothing is applied (e.g. --priority low is reported, not set).
fn stats_cmd(args: &[String]) {
    let (v, w) = flags_of("stats"); check_flags(args, &v, &w);
    let (chains, threads, cpu) = controls(args); let sweeps: usize = arg(args, "--sweeps", 2000).max(1);
    let prio = args.iter().position(|a| a == "--priority").and_then(|k| args.get(k + 1)).cloned().or_else(|| std::env::var("PROBBIT_PRIORITY").ok()).or_else(|| cfg_val("priority")).unwrap_or_else(|| "normal".into());
    if !["low", "normal"].contains(&prio.as_str()) { fail("--priority / PROBBIT_PRIORITY must be low or normal") } // a bad value used to be echoed
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let feats: Vec<Json> = target_features().into_iter().map(jstr).collect();
    let n = 400; let prog = run::from_json(&json::parse(&ring(n)).unwrap()).unwrap();
    let rate = |t: usize| { let t0 = std::time::Instant::now(); let s = probbit_ir::sample_on(&prog.m, chains, t, sweeps, None, 1, false, cpu as u32, 0).unwrap();
        (s.sweeps as f64 * n as f64 / t0.elapsed().as_secs_f64()).round() };
    rate(threads); let (r1, rt) = (rate(1), rate(threads)); // first pass discarded: page faults + clock ramp made 1 thread look slow (4.27x on 4 threads)
    let ctl_obj = |v: Json, name: &str, env: &str| obj(vec![("value", v), ("source", jstr(source(args, name, env)))]);
    let doc = obj(vec![("engine", jstr(&format!("probbit {VERSION}"))),
        ("machine", obj(vec![("os", jstr(std::env::consts::OS)), ("arch", jstr(std::env::consts::ARCH)), ("logical_cpus", num(cores as f64))])),
        ("build", obj(vec![("target_features", Json::Arr(feats)), ("debug_assertions", Json::Bool(cfg!(debug_assertions)))])),
        ("controls", obj(vec![("chains", ctl_obj(num(chains as f64), "--chains", "PROBBIT_CHAINS")), ("threads", ctl_obj(num(threads as f64), "--threads", "PROBBIT_THREADS")),
            ("cpu_limit_pct", ctl_obj(num(cpu as f64), "--cpu-limit", "PROBBIT_CPU_LIMIT")), ("priority", ctl_obj(jstr(&prio), "--priority", "PROBBIT_PRIORITY")),
            ("mem_limit_mb", ctl_obj(num(ctl_min(args, "--mem-limit-mb", "PROBBIT_MEM_LIMIT_MB", DEFAULT_MEM_LIMIT_MB, 0) as f64), "--mem-limit-mb", "PROBBIT_MEM_LIMIT_MB")),
            ("config_file", config_path().map_or(Json::Null, |f| jstr(&f)))])),
        ("self_test", obj(vec![("program", jstr("400-spin ring, IR sampler, fixed work")), ("chains", num(chains as f64)), ("sweeps_per_chain", num(sweeps as f64)),
            ("site_updates_per_s_1_thread", num(r1)), ("threads", num(threads.clamp(1, chains) as f64)), ("site_updates_per_s", num(rt)), ("speedup", num((rt / r1 * 100.0).round() / 100.0))])),
        ("process_cpu_ms", sys::usage().map_or(Json::Null, |u| num(u.0))), ("peak_rss_mb", sys::usage().map_or(Json::Null, |u| num(u.1)))]);
    emit(&json::write(&doc, args.iter().any(|a| a == "--pretty")));
}

/// The agent-routing demo as a JSON problem: agent tasks x workers under PII / prod-DB policy, quotas and same-workflow affinity.
/// Stdout: the document (`demo_doc`). With `--live`, or by default when stdout and stderr are both terminals, the router story
/// runs on stderr (`tui::live`; --tasks defaults to 300) and stdout gets the same document only if it is not a terminal.
fn demo_cmd(args: &[String]) {
    let (v, w) = flags_of("demo"); check_flags(args, &v, &w);
    let seed: u64 = arg(args, "--seed", 7); let hard = args.iter().any(|a| a == "--hard"); let live = args.iter().any(|a| a == "--live");
    let th = theme::stderr(args); let auto = !live && th.is_some() && theme::stdout_is_terminal();
    if !(live || auto) { emit(&json::write(&demo_doc(arg(args, "--tasks", 12), seed, hard), true)); return; }
    let doc = demo_doc(arg(args, "--tasks", 300), seed, hard);
    let Some(th) = th else { err_line("probbit: --live draws on a colour terminal (stderr); no terminal, or NO_COLOR / --plain / PROBBIT_THEME=plain: printed the problem only");
        emit(&json::write(&doc, true)); return };
    let mut n = from_json(&doc).unwrap_or_else(|e| bad_input("bad problem: ", e)); n.p.collective = true; n.p.cluster = true; n.p.cycles = true; // decide's defaults
    tui::live(th, &tui::DemoRun { doc: &doc, p: &n.p, tasks: &n.tasks, workers: &n.workers, seed, hard });
    if !theme::stdout_is_terminal() { emit(&json::write(&doc, true)); }
}

/// `probbit run` on a parsed program (also `probbit evaluate`'s compiled one): the flags, the instruction, `phases` appended.
/// -> (document, exit code, per-variable error bars for `--summary`)
fn run_program(args: &[String], mut p: run::Prog, cmd: &'static str, tp: std::time::Instant, parse_ms: f64, compile_ms: f64) -> (Json, i32, Vec<f64>) {
    p.m.collective = collective_arg(args); p.m.cluster = cluster_arg(args); p.m.cycles = cycles_arg(args);
    priority(args); let (chains, threads, cpu_pct) = controls(args); let top = top_wanted(args);
    let op: String = arg(args, "--op", "decide".to_string()); if !["decide", "exact", "sample"].contains(&op.as_str()) { fail("--op must be decide, exact or sample"); }
    let mon = monitor(args, p.m.n); let dl = deadline_ms(args);
    if dl.is_some() && arg::<usize>(args, "--sweeps", 0) > 0 { fail("--deadline-ms needs a wall-clock budget: drop --sweeps"); }
    if let Some(th) = top { tui::tier("exact tiers"); tui::top_start(th, tui::TopSpec { cmd, vars: p.m.n, chains, threads: threads.clamp(1, chains), budget_ms: budget_ms(args), sweeps: arg(args, "--sweeps", 0), deadline_ms: dl }); }
    let mut bars = vec![];
    let (mut doc, code) = run::run(&p, &op, budget_ms(args), arg(args, "--seed", 7), arg(args, "--exact-limit", 2_000_000), polish_ms_arg(args), arg(args, "--polish-sweeps", 0), arg(args, "--sweeps", 0), arg(args, "--frontier-states", probbit_ir::FRONTIER_MAX_STATES),
        chains, threads, cpu_pct, mem_limit(args, chains, p.m.n), exact_ms(args), dl.map(|d| (tp, d, args.iter().any(|a| a == "--budget-ms"))), &mut bars); drop(mon); run::phases(&mut doc, parse_ms, compile_ms, dl);
    (doc, code, bars)
}
/// `probbit evaluate`: a System One request + `probbit` block on stdin -> the compiled program (`evaluate::compile`) -> `probbit run`'s
/// instruction with every `run` flag -> the System-One-shaped answer plus the probbit document (`evaluate::respond`). `--program`
/// prints the compiled probbit-ir program instead (`probbit evaluate --program | probbit run` gives the same answer).
fn evaluate_cmd(args: &[String]) {
    let (v, w) = flags_of("evaluate"); check_flags(args, &v, &w); let tp = std::time::Instant::now(); let j = read_doc(args);
    let tc = std::time::Instant::now(); let parse_ms = (tc - tp).as_secs_f64() * 1e3;
    let r = evaluate::compile(&j).unwrap_or_else(|e| bad_input("bad request: ", e));
    if args.iter().any(|a| a == "--program") { emit(&json::write(&r.program, args.iter().any(|a| a == "--pretty"))); return; }
    let p = run::from_json(&r.program).unwrap_or_else(|e| bad_input("bad request: ", evaluate::locate(e))); let compile_ms = tc.elapsed().as_secs_f64() * 1e3;
    let (doc, code, bars) = run_program(args, p, "evaluate", tp, parse_ms, compile_ms);
    let c = finish(evaluate::respond(&r, doc), code, &View::new(args, "evaluate"), &bars); if c != 0 { std::process::exit(c); }
}

const PERSONA_HELP: &str = "The individuality layer (docs/persona.md): a persona file (YAML subset or JSON: traits with priors, moods with\n  inertia, couplings, per-turn inputs, history, habits = hard rules) + an individual's state + this turn's inputs -> ONE probbit-ir\n  program, run in process (`probbit run --op decide` at fixed work) -> the stance: a level per trait with exact odds, the habits\n  in force and the ones that bound, a refusal when the engine cannot vouch, a why and a short stance line for any model's prompt.\n  Documents are canonical JSON (keys sorted); a turn is a pure function of (persona, state, inputs, version).\nsubcommands:\n  init PERSONA [--seed N] [--out STATE]          a new individual (genes from the seed, resting stance); stdout or --out\n  turn PERSONA --state STATE [--inputs JSON|FILE] [--out STATE2] [--timing] [--no-inertia]\n                                                 the stance on stdout; the new state replaces STATE (or goes to --out)\n  replay PERSONA [--seed N] --script JSON|FILE [--out TRACE] [--no-inertia] [--timing]\n                                                 init, then every turn of the script: one stance per line (JSONL)\n  explain PERSONA [--seed N] --script JSON|FILE --turn K   turn K in words: every contribution, the odds, the twin\n  diff PERSONA [--seed A] [--other PERSONA2] [--seed2 B] --script JSON|FILE   distance between two individuals\n  lint PERSONA [--never RULE | --props FILE] [--seeds 0-99] [--threads N]\n                                                 contradicting habits (every conditional habit and pair, every prev level);\n                                                 warnings: planned levels that are not their trait's most likely one;\n                                                 with rules: prove each one, fuzz the unknown ones; exit 1 on a counterexample\n  fuzz PERSONA (--never RULE | --props FILE) [--seeds 0-99] [--fuzz-seed N] [--scripts N] [--depth N] [--beam N]\n       [--grid LIST] [--hours LIST] [--threads N] [--json]\n                                                 search event scripts for the shortest one whose stance breaks a rule\n                                                 (habit syntax: {when: {...}, then: {...}}); shrunk, replayable (§5.6)\n  prove PERSONA (--never RULE | --props FILE) [--seeds 0-99] [--threads N] [--json]\n                                                 per rule: held by construction (a habit implies it), proved for every\n                                                 event sequence (a sound bound), or unknown (the cell it fails; fuzz it) (§5.6)\n  check PERSONA                                  valid? digest and sizes\n  compile PERSONA --state STATE [--inputs JSON|FILE]   the turn's probbit-ir program (its sha256 = engine.program)\n  describe PERSONA                               traits, moods, inputs, habits, agenda\n  A script is a JSON list of per-turn input objects, or {\"turns\": [...]}. Resource controls as for run (PROBBIT_THREADS, ...).";
/// The persona engine of this process: `persona::run_program` with the resource controls `probbit run` would use (flag > env >
/// config > default; resolved once), and --priority low applied if asked for. They never change an answer at fixed work.
pub(crate) fn persona_engine() -> impl Fn(&Json, &persona::Flags) -> Json {
    let a: Vec<String> = ["persona", "--chains", "4"].iter().map(|s| s.to_string()).collect(); // chains come from the persona (the prototype passes them as a flag)
    priority(&a); let (_, threads, cpu) = controls(&a); let mem = ctl_min(&a, "--mem-limit-mb", "PROBBIT_MEM_LIMIT_MB", DEFAULT_MEM_LIMIT_MB, 0);
    move |prog: &Json, f: &persona::Flags| persona::run_program(prog, f, threads, cpu, mem)
}
/// `--inputs` / `--script` / `--state`: a file, else the argument itself as JSON
fn json_arg(s: &str, what: &str) -> Json {
    let t = if std::path::Path::new(s).is_file() { std::fs::read_to_string(s).unwrap_or_else(|e| bad_input("persona: ", persona::perr(what, format!("cannot read {s}: {e}")))) } else { s.to_string() };
    json::parse(t.strip_prefix('\u{feff}').unwrap_or(&t)).unwrap_or_else(|e| bad_input("persona: ", persona::perr(what, format!("not JSON: {}", e.msg))))
}
/// Write a document: to `path` (write, then rename: a killed turn never leaves a half-written state), or to stdout
fn put(path: Option<&str>, text: &str) {
    let Some(path) = path else { emit(text); return };
    let tmp = format!("{path}.tmp-{}", std::process::id());
    if let Err(e) = std::fs::write(&tmp, format!("{text}\n")).and_then(|_| std::fs::rename(&tmp, path)) { let _ = std::fs::remove_file(&tmp); fail(&format!("persona: cannot write {path}: {e}")) }
}
/// A --seed: an integer 0..=2^53
fn seed_arg(args: &[String], name: &str) -> Option<u64> {
    args.iter().any(|a| a == name).then(|| { let s: u64 = arg(args, name, 0); if s > 1 << 53 { fail(&format!("{name} must be an integer from 0 to 2^53")) } s })
}
/// A list flag: comma-separated numbers from `lo` to `hi`
fn list_arg(args: &[String], name: &str, default: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let Some(k) = args.iter().position(|a| a == name) else { return default.to_vec() };
    let v = args.get(k + 1).map_or("", String::as_str);
    let xs: Option<Vec<f64>> = v.split(',').map(|x| x.trim().parse::<f64>().ok().filter(|x| x.is_finite() && *x >= lo && *x <= hi)).collect();
    match xs { Some(x) if !x.is_empty() => x, _ => fail(&format!("{name}: comma-separated numbers from {lo} to {hi}, e.g. {}", default.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))) }
}
/// `--seeds 0-99` / `3` / `1,4,9` / `0-9,20-29`: the individuals, in the order given, each once (default 0-99)
fn seeds_arg(args: &[String]) -> Vec<u64> {
    let Some(k) = args.iter().position(|a| a == "--seeds") else { return (0..100).collect() };
    let v = args.get(k + 1).map_or("", String::as_str); let bad = || -> ! { fail(&format!("--seeds: cannot read {v:?} (e.g. 0-99, 7, or 1,4,9; seeds 0 to 2^53, at most 100000)")) };
    fuzz::seeds(v).unwrap_or_else(|| bad())
}
/// `probbit persona fuzz PERSONA (--never RULE | --props FILE) [flags]` (docs/persona.md §5.6): exit 0 nothing found, 1 a
/// counterexample, 2 bad input
/// `--never RULE` / `--props FILE` -> the rules (fuzz, prove, lint); a bad rule is an error object, exit 2
fn rules_arg(args: &[String], p: &persona::Persona) -> Vec<persona::Prop> {
    let opt = |f: &str| -> Option<String> { args.iter().position(|x| x == f).and_then(|k| args.get(k + 1)).cloned() };
    let mut props = vec![];
    // a rule on the command line: JSON, or one line of the YAML subset (a flow mapping, read as the value of a key)
    if let Some(t) = opt("--never") { let d = json::parse(&t).or_else(|_| persona::parse_doc(&format!("never: {t}"), false).map(|d| d.get("never").cloned().unwrap_or(Json::Null)))
            .unwrap_or_else(|e| bad_input("persona: ", persona::perr("--never", e)));
        props = persona::props(p, &d, "--never", "never").unwrap_or_else(|e| bad_input("persona: ", e)); }
    if let Some(f) = opt("--props") {
        let t = std::fs::read_to_string(&f).unwrap_or_else(|e| bad_input("persona: ", persona::perr("--props", format!("cannot read {f}: {e}"))));
        let d = persona::parse_doc(&t, f.ends_with(".json")).unwrap_or_else(|e| bad_input("persona: ", persona::perr("--props", e)));
        for q in persona::props(p, &d, "--props", "prop1").unwrap_or_else(|e| bad_input("persona: ", e)) {
            if props.iter().any(|x: &persona::Prop| x.id == q.id) { bad_input("persona: ", persona::perr("--props", format!("property id {} is given twice", q.id))) }
            props.push(q); } }
    props
}
fn fuzz_cmd(args: &[String], path: &str, p: &persona::Persona, eng: fuzz::SyncEngine) {
    let props = rules_arg(args, p);
    let sub = args[1].as_str();
    if props.is_empty() { fail(&format!("persona {sub}: give a rule: --never RULE (habit syntax) or --props FILE")) }
    let bounded = |f: &str, d: usize, lo: usize, hi: usize| -> usize { let x: usize = arg(args, f, d); if x < lo || x > hi { fail(&format!("{f} must be {lo} to {hi}")) } x };
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    if sub == "prove" { let (seeds, threads) = (seeds_arg(args), bounded("--threads", cores, 1, 1024));
        let t0 = std::time::Instant::now(); let out = fuzz::prove(p, &props, &seeds, threads, eng); let secs = t0.elapsed().as_secs_f64();
        if args.iter().any(|a| a == "--json") { emit(&persona::canon(&fuzz::prove_doc(p, &props, &seeds, &out))) } else { emit(&fuzz::prove_human(p, path, &props, &seeds, &out)) }
        err_line(&format!("prove: {secs:.2} s"));
        if out.iter().any(|v| matches!(v, fuzz::Verdict::Unknown { .. })) { std::process::exit(1) }
        return; }
    let s = fuzz::Search { seeds: seeds_arg(args), fuzz_seed: arg(args, "--fuzz-seed", 0u64), scripts: bounded("--scripts", 60, 0, 1_000_000), depth: bounded("--depth", 8, 1, 64),
        beam: bounded("--beam", 4, 0, 64), grid: list_arg(args, "--grid", &[0.0, 0.5, 1.0], 0.0, 1.0), hours: list_arg(args, "--hours", &[1.0, 12.0, 48.0], 0.0, 1e6), threads: bounded("--threads", cores, 1, 1024) };
    let t0 = std::time::Instant::now(); let (res, turns) = fuzz::fuzz(p, &props, &s, eng); let secs = t0.elapsed().as_secs_f64();
    if args.iter().any(|a| a == "--json") { emit(&persona::canon(&fuzz::doc(p, path, &props, &s, &res, turns))) } else { emit(&fuzz::human(p, path, &props, &s, &res, turns)) }
    err_line(&format!("fuzz: {turns} turns in {secs:.2} s ({:.0} turns/s, {} search thread{})", turns as f64 / secs.max(1e-9), s.threads.min(s.seeds.len().max(1)), if s.threads.min(s.seeds.len().max(1)) == 1 { "" } else { "s" }));
    if res.iter().any(|per| per.iter().any(Option::is_some)) { std::process::exit(1) }
}
const LIVE_HELP: &str = "A resident individual (docs/persona.md §5.7): JSONL events (one object of inputs per line) from --events FILE or stdin,\n  one stance per event on stdout (canonical JSON). The clock stamps each event's elapsed_hours (quantised to 1e-6 h), so moods\n  decay by their half-lives between events; feedback moves the learned deltas of a persona with a learning block (§2.8).\n  --strand FILE logs the life: a header (persona document, initial state, engine version), then per event the inputs as used,\n  the stance and state digests and the sha256 of the line before. `probbit live verify STRAND` replays it.\nflags:\n  --seed N            a new individual (default: the persona's seed)\n  --state FILE        a stored individual instead; rewritten after every event\n  --strand FILE       log the life to FILE: a new file gets the header; an existing strand is continued from --state\n                      (the state after its last line); a strand is never rewritten, just appended to\n  --events FILE       read events from FILE (default stdin)\n  --watch             follow --events FILE as lines are appended (tail -f); stops when the file is removed\n  --clock real|fixed  real (default): elapsed hours from a monotonic clock started with the run; fixed: each event carries\n                      its own elapsed_hours (default 0), so the run is a pure function of its events\n  --demo week         one individual's scripted week on the fixed clock (7 days, events hourly 09:00-15:00): praise for short\n                      answers moves the learned verbosity deltas to their cap, a quiet night relaxes the mood to its resting\n                      level, a campaign praising jokes raises humour while failure turns stay joke-free; a persona without a\n                      learning block gets the demo's (said in the opening line). Bars on stderr at a terminal, paced 1 s per hour\n                      (nights fast-forward in 2 s); otherwise one line per event on stdout, no waiting. With --seed, --strand\n  --plain             no colour, no bars, no pacing (as NO_COLOR=1)\n  --checkpoint-every K  with --strand: a checkpoint line (the whole state) after every K-th event (default 1000; 0: none);\n                      `verify --from-checkpoint` and `monitor` start at the last one\nthe safety kit (docs/persona.md §5.7):\n  one writer per strand: `--strand` takes STRAND.lock (a second writer exits 4 and changes nothing; a lock whose process is\n  gone is taken over). With `reward_from` in the persona (§2.10), a reward from `src: self`, without a src or from an\n  undeclared one is refused like a bad event.\n  probbit live control STRAND pause|resume|retire --by human:ID|env:ID --reason TEXT [--at TIME]\n                      append a control line under the lock: while paused or retired every event is refused (exit 4);\n                      no credit crosses a control line; retire is final; `verify` replays them and reports the status\n  probbit live verify STRAND [--from-checkpoint]   replay from the header (every checkpoint checked), or from the last checkpoint";
/// `probbit live PERSONA [--seed N | --state FILE] [--strand FILE] [--events FILE [--watch]] [--clock real|fixed] [--checkpoint-every K]`;
/// `probbit live PERSONA --demo week`; `probbit live verify STRAND [--from-checkpoint]`; `probbit live control STRAND pause|resume|retire --by WHO --reason TEXT`
fn live_cmd(args: &[String]) {
    let engine = persona_engine(); let eng: persona::Engine = &engine;
    let opt = |f: &str| -> Option<String> { args.iter().position(|x| x == f).and_then(|k| args.get(k + 1)).cloned() };
    // an error object on stdout, a line on stderr, then the exit: 4 for a lock held by another writer or a paused / retired individual
    let refuse = |what: &str, e: json::InErr| -> ! { err_line(&format!("probbit: {what}{}: {}", e.path, e.msg)); emit(&json::write(&e.to_json(), false));
        exit(if ["locked", "paused", "retired"].contains(&e.code) { 4 } else { 2 }) };
    if args.get(1).map(String::as_str) == Some("verify") {
        let mut a = vec!["live verify".to_string()]; a.extend(args.iter().skip(3).cloned()); check_flags(&a, &[], &["--from-checkpoint"]);
        let Some(path) = args.get(2).filter(|a| !a.starts_with("--")) else { fail("live verify: the strand file: probbit live verify STRAND") };
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| fail(&format!("live verify: cannot read {path}: {e}")));
        let res = if args.iter().any(|a| a == "--from-checkpoint") { live::verify_from_checkpoint(&text, eng) } else { live::verify(&text, eng) };
        match res {
            Ok(doc) => emit(&persona::canon(&doc)),
            Err((line, why)) => { emit(&persona::canon(&obj(vec![("ok", Json::Bool(false)), ("line", num(line as f64)), ("diverges", jstr(&why))]))); std::process::exit(1) } }
        return;
    }
    if args.get(1).map(String::as_str) == Some("control") {
        let mut a = vec!["live control".to_string()]; a.extend(args.iter().skip(4).cloned()); check_flags(&a, &["--by", "--reason", "--at"], &[]);
        let usage = "live control: probbit live control STRAND pause|resume|retire --by human:ID --reason TEXT";
        let (Some(path), Some(what)) = (args.get(2).filter(|a| !a.starts_with("--")), args.get(3).filter(|a| !a.starts_with("--"))) else { fail(usage) };
        let (Some(by), Some(reason)) = (opt("--by"), opt("--reason")) else { fail(&format!("{usage} (--by and --reason are required)")) };
        match live::control_cmd(path, what, &by, &reason, &opt("--at").unwrap_or_else(live::utc_now)) { Ok(doc) => emit(&persona::canon(&doc)), Err(e) => refuse("live control: ", e) }
        return;
    }
    let Some(path) = args.get(1).filter(|a| !a.starts_with("--")) else { fail("live: the persona file goes before the flags: probbit live PERSONA [flags] (or probbit live verify STRAND, probbit live control STRAND ...)") };
    let mut a = vec!["live".to_string()]; a.extend(args.iter().skip(2).cloned()); check_flags(&a, &["--seed", "--state", "--strand", "--events", "--clock", "--demo", "--checkpoint-every"], &["--watch", "--plain"]);
    let (p, doc) = persona::load(path).unwrap_or_else(|e| bad_input("live: ", e));
    if let Some(d) = opt("--demo") {
        if d != "week" { fail(&format!("live: --demo week, not {d:?}")) }
        if let Some(f) = ["--state", "--events", "--clock", "--watch", "--checkpoint-every"].iter().find(|f| args.iter().any(|a| a == *f)) { fail(&format!("live --demo week: {f} does not apply (the demo is its own events on the fixed clock)")) }
        // the bars on stderr at a colour terminal; the plain lines on stdout unless it is that terminal too
        let th = theme::stderr(args); let lines = th.is_none() || !theme::stdout_is_terminal();
        live::week(&doc, seed_arg(args, "--seed"), opt("--strand").as_deref(), th, &mut |l: &str| if lines { emit(l) }, eng).unwrap_or_else(|e| bad_input("live: ", e));
        return;
    }
    let every: u64 = arg(args, "--checkpoint-every", live::CHECKPOINT_EVERY);
    let state_file = opt("--state"); if state_file.is_some() && opt("--seed").is_some() { fail("live: give --seed N (a new individual) or --state FILE (a stored one), not both") }
    let st = match &state_file {
        Some(f) => { let t = std::fs::read_to_string(f).unwrap_or_else(|e| bad_input("live: ", persona::perr("state", format!("cannot read {f}: {e} (make one with `probbit persona init`)"))));
            let j = json::parse(t.strip_prefix('\u{feff}').unwrap_or(&t)).unwrap_or_else(|e| bad_input("live: ", persona::perr("state", format!("not JSON: {}", e.msg))));
            persona::State::read(&p, &j).unwrap_or_else(|e| bad_input("live: ", e)) }
        None => persona::init(&p, seed_arg(args, "--seed"), true, eng) };
    let clock = match opt("--clock").as_deref() { None | Some("real") => live::Clock::Real(std::time::Instant::now()), Some("fixed") => live::Clock::Fixed, Some(c) => fail(&format!("live: --clock real|fixed, not {c:?}")) };
    let watch = args.iter().any(|a| a == "--watch"); if watch && opt("--events").is_none() { fail("live: --watch follows an --events FILE") }
    // --strand: one writer per strand (the lock, held to the exit, from before the strand is read); a new file gets the header;
    // an existing strand is continued from --state (the state after its last line)
    let strand = opt("--strand");
    let lock = strand.as_ref().map(|f| live::Lock::take(f).unwrap_or_else(|e| refuse("live: ", e)));
    if let Some(f) = &strand { if std::path::Path::new(f).exists() && state_file.is_none() { fail(&format!("live: {f} exists: continue it with --state FILE (the state after its last line), or log to a new file")) } }
    let (mut lv, header) = match &strand { Some(f) => live::open(f, p, &doc, st, clock).unwrap_or_else(|e| bad_input("live: ", e)), None => (live::Live::start(p, &doc, st, clock, &live::engine()).0, None) };
    // every append first checks the lock is still this writer's: a lock removed or taken over under it stops the run unwritten
    let log = |ls: &[&str]| if let Some(f) = &strand {
        if !lock.as_ref().is_some_and(live::Lock::held) { refuse("live: ", json::InErr { code: "locked", path: "strand".into(), msg: format!("the writer lock {f}.lock is no longer this run's (removed or taken over): stopped before writing") }) }
        live::append(f, ls).unwrap_or_else(|e| fail(&format!("live: {}", e.msg))) };
    if let Some(h) = &header { log(&[h]); }
    let (mut bad, mut refused) = (0, 0);
    let mut one = |lv: &mut live::Live, i: usize, t: &str| {
        let res = json::parse(t).map_err(|e| persona::perr("event", format!("not JSON: {}", e.msg))).and_then(|ev| lv.event(&ev, eng));
        match res {
            Ok((stance, line)) => { let cp = (every > 0 && lv.n % every == 0).then(|| lv.checkpoint(&stance));
                match &cp { Some(c) => log(&[&line, c]), None => log(&[&line]) }
                if let Some(f) = &state_file { put(Some(f), &persona::canon(&lv.st.to_json(&lv.p))); } emit(&persona::canon(&stance)); }
            Err(e) => { if ["paused", "retired"].contains(&e.code) { refused += 1 } else { bad += 1 }
                let e = json::InErr { path: format!("events[{i}].{}", e.path), ..e }; err_line(&format!("probbit: live: {}: {}", e.path, e.msg)); emit(&json::write(&e.to_json(), false)); } } };
    match (opt("--events"), watch) {
        // --watch: follow the file as lines are appended (a line counts once its newline is written); stops when the file is removed
        (Some(f), true) => { let mut r = std::io::BufReader::new(std::fs::File::open(&f).unwrap_or_else(|e| fail(&format!("live: cannot read {f}: {e}"))));
            let (mut pending, mut i) = (String::new(), 0usize);
            loop { let n = std::io::BufRead::read_line(&mut r, &mut pending).unwrap_or_else(|e| fail(&format!("live: cannot read the events: {e}")));
                if n == 0 { if !std::path::Path::new(&f).exists() { break; } std::thread::sleep(std::time::Duration::from_millis(50)); continue; }
                if !pending.ends_with('\n') { continue; }
                let l = std::mem::take(&mut pending); if !l.trim().is_empty() { one(&mut lv, i, l.trim()); } i += 1; } }
        (f, _) => { let input: Box<dyn std::io::BufRead> = match f { Some(f) => Box::new(std::io::BufReader::new(std::fs::File::open(&f).unwrap_or_else(|e| fail(&format!("live: cannot read {f}: {e}"))))),
                None => Box::new(std::io::BufReader::new(std::io::stdin())) };
            for (i, l) in std::io::BufRead::lines(input).enumerate() {
                let l = l.unwrap_or_else(|e| fail(&format!("live: cannot read the events: {e}"))); let t = l.trim(); if !t.is_empty() { one(&mut lv, i, t); } } } }
    err_line(&format!("live: {} events, strand head {}, final state {}{}", lv.n, lv.head(), lv.st.digest, if lv.status == live::Status::Active { String::new() } else { format!(", {} ({refused} events refused)", lv.status.name()) }));
    drop(lock);
    if refused > 0 { exit(4) }
    if bad > 0 { exit(2) }
}
/// `probbit persona <sub> PERSONA [flags]` (docs/persona.md)
fn persona_cmd(args: &[String]) {
    let sub = args.get(1).map_or("", String::as_str);
    let (vals, sw): (&[&str], &[&str]) = match sub {
        "init" => (&["--seed", "--out"], &[]), "turn" => (&["--state", "--inputs", "--out"], &["--timing", "--no-inertia"]),
        "replay" => (&["--seed", "--script", "--out"], &["--no-inertia", "--timing"]), "explain" => (&["--seed", "--script", "--turn"], &[]),
        "diff" => (&["--seed", "--other", "--seed2", "--script"], &[]), "compile" => (&["--state", "--inputs"], &[]), "check" | "describe" => (&[], &[]), "lint" => (&["--never", "--props", "--seeds", "--threads"], &[]),
        "fuzz" => (&["--seeds", "--never", "--props", "--fuzz-seed", "--scripts", "--depth", "--beam", "--grid", "--hours", "--threads"], &["--json"]),
        "prove" => (&["--seeds", "--never", "--props", "--threads"], &["--json"]),
        _ => fail("persona: a subcommand: init, turn, replay, explain, diff, lint, fuzz, prove, check, compile or describe (probbit persona --help)") };
    let Some(path) = args.get(2).filter(|a| !a.starts_with("--")) else { fail(&format!("persona {sub}: the persona file comes first: probbit persona {sub} PERSONA [flags]")) };
    let mut a = vec![format!("persona {sub}")]; a.extend(args[3..].iter().cloned()); check_flags(&a, vals, sw);
    let need = |f: &str| -> String { args.iter().position(|x| x == f).and_then(|k| args.get(k + 1)).cloned().unwrap_or_else(|| fail(&format!("persona {sub}: {f} is required"))) };
    let opt = |f: &str| -> Option<String> { args.iter().position(|x| x == f).and_then(|k| args.get(k + 1)).cloned() };
    let (p, _) = persona::load(path).unwrap_or_else(|e| bad_input("persona: ", e));
    let engine = persona_engine(); let eng: persona::Engine = &engine;
    let no_inertia = args.iter().any(|x| x == "--no-inertia");
    let state = |f: &str| -> persona::State { let s = need(f);
        let t = std::fs::read_to_string(&s).unwrap_or_else(|e| bad_input("persona: ", persona::perr("state", format!("cannot read {s}: {e} (make one with `probbit persona init`)"))));
        let j = json::parse(t.strip_prefix('\u{feff}').unwrap_or(&t)).unwrap_or_else(|e| bad_input("persona: ", persona::perr("state", format!("not JSON: {}", e.msg))));
        persona::State::read(&p, &j).unwrap_or_else(|e| bad_input("persona: ", e)) };
    let inputs = || opt("--inputs").map_or(Json::Obj(vec![]), |s| json_arg(&s, "inputs"));
    let turns = || persona::script(&json_arg(&need("--script"), "script")).unwrap_or_else(|e| bad_input("persona: ", e));
    match sub {
        "init" => { let st = persona::init(&p, seed_arg(args, "--seed"), true, eng); put(opt("--out").as_deref(), &persona::canon(&st.to_json(&p))); }
        "turn" => { let st = state("--state"); let (out, ns) = persona::turn(&p, &st, &inputs(), no_inertia, eng, args.iter().any(|x| x == "--timing")).unwrap_or_else(|e| bad_input("persona: ", e));
            put(Some(&opt("--out").unwrap_or_else(|| need("--state"))), &persona::canon(&ns.to_json(&p))); emit(&persona::canon(&out)); }
        "replay" => { let (trace, st) = persona::replay(&p, seed_arg(args, "--seed"), &turns(), no_inertia, eng, args.iter().any(|x| x == "--timing")).unwrap_or_else(|e| bad_input("persona: ", e));
            let text: String = trace.iter().map(|t| persona::canon(t) + "\n").collect();
            match opt("--out") { Some(o) => if let Err(e) = std::fs::write(&o, &text) { fail(&format!("persona: cannot write {o}: {e}")) }, None => emit_raw(&text) }
            err_line(&format!("replay: {} turns, trace sha256 {}, final state {}", trace.len(), persona::hex(&persona::sha256(text.as_bytes())), st.digest)); }
        "explain" => { let ts = turns(); let k: usize = arg(args, "--turn", usize::MAX); if k == usize::MAX { fail("persona explain: --turn K is required") }
            if k >= ts.len() { bad_input("persona: ", persona::perr("--turn", format!("the script has {} turns (0..{})", ts.len(), ts.len().saturating_sub(1)))) }
            let (_, st) = persona::replay(&p, seed_arg(args, "--seed"), &ts[..k], false, eng, false).unwrap_or_else(|e| bad_input("persona: ", e));
            emit(&persona::explain(&p, &st, &ts[k], eng).unwrap_or_else(|e| bad_input("persona: ", e))); }
        "diff" => { let q = opt("--other").map_or_else(|| p.clone(), |o| persona::load(&o).unwrap_or_else(|e| bad_input("persona: ", e)).0);
            let (sa, sb) = (seed_arg(args, "--seed").unwrap_or(p.seed), seed_arg(args, "--seed2").unwrap_or(q.seed));
            emit(&persona::canon(&persona::diff(&p, sa, &q, sb, &turns(), eng).unwrap_or_else(|e| bad_input("persona: ", e)))); }
        "compile" => { let st = state("--state"); emit(&persona::program(&p, &st, &inputs()).unwrap_or_else(|e| bad_input("persona: ", e))); }
        "check" => emit(&persona::canon(&persona::check(&p, eng).unwrap_or_else(|e| bad_input("persona: ", e)))),
        "describe" => emit(&persona::canon(&persona::describe(&p))),
        "fuzz" | "prove" => fuzz_cmd(args, path, &p, &engine),
        _ => { let found = persona::lint(&p, eng); let unresolved = found.iter().filter(|f| !f.get("resolution").and_then(Json::as_str).is_some_and(|r| r.starts_with("yield"))).count();
            let seeds = seeds_arg(args); let warnings = persona::plan_warnings(&p, &seeds, eng);
            let mut doc = vec![("persona", jstr(&p.name)), ("conflicts", Json::Arr(found)), ("unresolved", num(unresolved as f64)), ("warnings", Json::Arr(warnings))]; let mut broken = 0;
            // drives (§2.9): habits that outrank a goal floor, floors whose lift can reach 50
            if let Some(mut n) = persona::floor_notes(&p) { n.extend(persona::floor_reach(&p, &seeds)); doc.push(("floors", Json::Arr(n))); }
            // with rules (§5.6): prove each one, then fuzz the ones the bound leaves unknown
            if args.iter().any(|a| a == "--never" || a == "--props") {
                let props = rules_arg(args, &p); let threads = arg(args, "--threads", std::thread::available_parallelism().map_or(1, |n| n.get())).clamp(1, 1024);
                let verdicts = fuzz::prove(&p, &props, &seeds, threads, &engine);
                let unknown: Vec<persona::Prop> = props.iter().zip(&verdicts).filter(|(_, v)| matches!(v, fuzz::Verdict::Unknown { .. })).map(|(q, _)| q.clone()).collect();
                let s = fuzz::Search { seeds: seeds.clone(), fuzz_seed: 0, scripts: 60, depth: 8, beam: 4, grid: vec![0.0, 0.5, 1.0], hours: vec![1.0, 12.0, 48.0], threads };
                let (res, turns) = if unknown.is_empty() { (vec![], 0) } else { fuzz::fuzz(&p, &unknown, &s, &engine) };
                let fd = fuzz::doc(&p, path, &unknown, &s, &res, turns); let fp = fd.get("properties").and_then(Json::as_arr).unwrap_or(&[]).to_vec();
                broken = fp.iter().filter(|x| x.get("verdict").and_then(Json::as_str) == Some("counterexample")).count();
                let mut entries = fuzz::prove_doc(&p, &props, &seeds, &verdicts).get("properties").and_then(Json::as_arr).unwrap_or(&[]).to_vec();
                for e in entries.iter_mut() { let id = e.get("id").cloned();
                    if let (Json::Obj(v), Some(f)) = (e, fp.iter().find(|x| x.get("id").cloned() == id)) { v.push(("fuzz".into(), f.clone())); } }
                doc.push(("props", Json::Arr(entries))); doc.push(("broken", num(broken as f64))); }
            doc.push(("ok", Json::Bool(unresolved == 0 && broken == 0)));
            emit(&persona::canon(&obj(doc)));
            if unresolved > 0 || broken > 0 { std::process::exit(1) } }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() >= 2 && args[1..].iter().any(|a| a == "--help" || a == "-h") { if let Some(h) = help(&args[0]) { emit_raw(&h); return; } }
    // At a terminal, `probbit` (no command) and `probbit --help` print the hero screen (exit 0); piped, `--help` prints USAGE and a
    // bare `probbit` still exits 2 with USAGE on stderr, as before
    let bare = args.iter().all(|a| a == "--plain") || args.iter().all(|a| a == "--help" || a == "-h" || a == "--plain");
    if bare && theme::stdout_is_terminal() { emit_raw(&tui::hero(theme::stdout(&args))); return; }
    match args.first().map(|s| s.as_str()) {
        Some("decide") => decide_cmd(&args),
        Some("demo") => demo_cmd(&args),
        Some("ir") => { let (v, w) = flags_of("ir"); check_flags(&args, &v, &w); let j = read_doc(&args);
            let n = from_json(&j).unwrap_or_else(|e| bad_input("bad problem: ", e)); emit_raw(&probbit_decide::ir::to_ir(&n.p)); }
        Some("run") => { let (v, w) = flags_of("run"); check_flags(&args, &v, &w); let tp = std::time::Instant::now(); let j = read_doc(&args);
            let tc = std::time::Instant::now(); let parse_ms = (tc - tp).as_secs_f64() * 1e3;
            let p = run::from_json(&j).unwrap_or_else(|e| bad_input("bad program: ", e)); let compile_ms = tc.elapsed().as_secs_f64() * 1e3;
            let (doc, code, bars) = run_program(&args, p, "run", tp, parse_ms, compile_ms);
            let c = finish(doc, code, &View::new(&args, "run"), &bars); if c != 0 { std::process::exit(c); } }
        Some("evaluate") => evaluate_cmd(&args),
        Some("stats") => stats_cmd(&args),
        Some("persona") => persona_cmd(&args),
        Some("live") => live_cmd(&args),
        Some("monitor") => monitor::cmd(&args),
        Some("mcp") => { check_flags(&args, &[], &[]); mcp::serve() }
        Some("version") | Some("--version") | Some("-V") => { check_flags(&args, &[], &[]); emit(&format!("probbit {VERSION}")) }
        // `probbit --help` / `-h` asked for the usage: stdout, exit 0 (it went to stderr with exit 2, as for a missing command)
        Some("--help") | Some("-h") => emit_raw(USAGE),
        _ => { let _ = std::io::stderr().write_all(USAGE.as_bytes()); std::process::exit(2) }
    }
}
const USAGE: &str = "usage: probbit decide [--budget-ms N] [--seed N] [--exact-limit N] [--exact-ms N] [--frontier-states N] [--polish-ms N] [--polish-sweeps N] [--mode auto|exact|sample] [--sweeps N] [--collective on|off] [--cluster on|off] [--cycles on|off] [--chains N] [--threads N] [--cpu-limit PCT] [--mem-limit-mb N] [--priority low|normal] [--max-input-mb N] [--progress [MS]] [--summary] [--top] [--pretty] < problem.json\n       probbit demo [--tasks N] [--seed N] [--hard] [--live]\n       probbit ir [--max-input-mb N] < problem.json\n       probbit run [--op decide|exact|sample] [--budget-ms N] [--deadline-ms N] [--seed N] [--exact-limit N] [--exact-ms N] [--frontier-states N] [--polish-ms N] [--polish-sweeps N] [--sweeps N] [--collective on|off] [--cluster on|off] [--cycles on|off] [--chains N] [--threads N] [--cpu-limit PCT] [--mem-limit-mb N] [--priority low|normal] [--max-input-mb N] [--progress [MS]] [--summary] [--top] [--pretty] < program.json   (probbit-ir JSON v1)\n       probbit evaluate [the run flags] [--program] < request.json   (decision-API adapter: System One request + judge answers + rules)\n       probbit stats [--sweeps N] [--pretty]   (machine, build, effective controls + source, measured updates/s)\n       probbit persona init|turn|replay|explain|diff|lint|fuzz|prove|check|compile|describe PERSONA [flags]   (the individuality layer; probbit persona --help)\n       probbit live PERSONA [--seed N | --state FILE] [--strand FILE] [--events FILE [--watch]] [--clock real|fixed]   (a resident individual: JSONL events in, stances out)\n       probbit live PERSONA --demo week [--seed N] [--strand FILE] [--plain]   (a scripted week: learning to a cap, a night, rules that hold)\n       probbit live verify STRAND [--from-checkpoint]   (replay a strand; the earliest line that differs)\n       probbit live control STRAND pause|resume|retire --by WHO --reason TEXT   (a control line: pause, resume or retire an individual)\n       probbit monitor STRAND [--follow] [--once] [--plain] [--fps N] [--serve] [--open] [--port N] | probbit monitor --demo [--open]   (watch an individual's inner state live: bars in the terminal or a page on 127.0.0.1, replayed from the strand)\n       probbit mcp   (Model Context Protocol server on stdio)\n       probbit version\n       probbit <command> --help | -h   (--plain or NO_COLOR: no colour on a terminal)\n";
