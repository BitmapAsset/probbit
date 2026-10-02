//! The visuals: the hero screen (`probbit`, `probbit --help` at a terminal; stdout), the `--top` monitor (decide, run), `probbit demo
//! --live` and the `--summary --pretty` box (stderr). Each draws only with a `Theme` (see `theme` for when there is none) and
//! none of them writes a decision document: stdout is produced exactly as without them.
use crate::json::{num, obj, str as jstr, Json};
use crate::theme::{self, Col, Depth, Theme, CYAN, DIM, GREEN, GREY, MAGENTA, RESET, YELLOW};
use probbit_decide::{Chain, GroupPairs, Problem, Samples, GATE};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 1234567 -> "1,234,567"
pub fn group(n: u64) -> String {
    let s = n.to_string(); let mut o = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() { if i > 0 && (s.len() - i) % 3 == 0 { o.push(','); } o.push(c); } o
}
/// 70.8e6 -> "70.8 M", 512 -> "512"
pub fn si(x: f64) -> String {
    if x >= 1e9 { format!("{:.1} G", x / 1e9) } else if x >= 1e6 { format!("{:.1} M", x / 1e6) } else if x >= 1e3 { format!("{:.1} k", x / 1e3) } else { format!("{x:.0}") }
}
/// Visible columns of `s` (escape sequences skipped; every glyph drawn here is one column wide).
fn width(s: &str) -> usize {
    let mut n = 0; let mut esc = false;
    for c in s.chars() { if esc { if c.is_ascii_alphabetic() { esc = false; } } else if c == '\x1b' { esc = true; } else { n += 1; } } n
}
/// `s` cut to `w` visible columns (escapes kept; a reset closes a cut line).
fn clip(s: &str, w: usize) -> String {
    if width(s) <= w { return s.to_string(); }
    let (mut o, mut n, mut esc) = (String::new(), 0, false);
    for c in s.chars() { if esc { o.push(c); if c.is_ascii_alphabetic() { esc = false; } continue; }
        if c == '\x1b' { esc = true; o.push(c); continue; }
        if n == w { break; } o.push(c); n += 1; }
    o.push_str(RESET); o
}
fn pad(s: &str, w: usize) -> String { let s = clip(s, w); let n = width(&s); format!("{s}{}", " ".repeat(w - n)) }
/// stderr, unbuffered; a reader that went away is ignored (as `err_line` in main.rs)
fn err_write(s: &str) { let mut e = std::io::stderr().lock(); let _ = e.write_all(s.as_bytes()); let _ = e.flush(); }
fn paint(th: Option<Theme>, c: Col, s: &str) -> String { th.map_or_else(|| s.to_string(), |t| t.paint(c, s)) }
fn bold(th: Option<Theme>, c: Col, s: &str) -> String { th.map_or_else(|| s.to_string(), |t| t.bold(c, s)) }
/// UTC calendar date of a Unix time
fn date(t: u64) -> String {
    let z = (t / 86_400) as i64 + 719_468; let era = z.div_euclid(146_097); let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153; let d = doy - (153 * mp + 2) / 5 + 1; let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", yoe + era * 400 + (m <= 2) as i64, m, d)
}
fn now_s() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) }

// ---------------- the hero screen ----------------
/// `probbit` / `probbit --help` at a terminal: wordmark, the spec line (`spec_line`), the status line, three commands to try and
/// the usage block, in 9 lines. Plain text without a theme (NO_COLOR, --plain, PROBBIT_THEME=plain).
pub fn hero(th: Option<Theme>) -> String {
    const MARK: [[&str; 7]; 2] = [["█▀█", "█▀▀", "█▀█", "█▀▄", "█▀▄", "█", "▀█▀"], ["█▀▀", "█  ", "█▄█", "█▄▀", "█▄▀", "█", " █ "]];
    let hues = [CYAN, CYAN, CYAN, CYAN, CYAN, MAGENTA, YELLOW];
    let mark = |r: usize| (0..7).map(|k| bold(th, hues[k], MARK[r][k])).collect::<Vec<_>>().join(" ");
    let dot = paint(th, DIM, "·");
    let spec = spec_line().split(" · ").map(|x| paint(th, GREY, x)).collect::<Vec<_>>().join(&format!(" {dot} "));
    let status = format!("{} {dot} {} {dot} {}", bold(th, GREEN, "PROCESSOR ONLINE"), bold(th, CYAN, VERSION), paint(th, YELLOW, "exact → sample → gate"));
    let tries = [("probbit demo", "watch 300 agent tasks get routed, live"), ("probbit demo | probbit decide --summary --pretty", "one JSON verdict + a boxed summary"),
        ("claude mcp add probbit -- probbit mcp", "hand the processor to an agent (MCP over stdio)")];
    let mut o = format!(" {}   {status}\n {}   {spec}\n\n", mark(0), mark(1));
    for (k, (cmd, what)) in tries.iter().enumerate() {
        o += &format!(" {}  {}{}{}\n", if k == 0 { bold(th, MAGENTA, "try") } else { "   ".into() }, bold(th, CYAN, cmd), " ".repeat(51 - cmd.len()), paint(th, GREY, what)); }
    o += &format!("\n {} probbit decide|run [flags] < doc.json {dot} probbit demo [--live] {dot} probbit stats {dot} probbit ir {dot} probbit mcp {dot} probbit version\n", bold(th, MAGENTA, "usage"));
    o += &format!("       probbit <command> --help: every flag, its default, the exit codes {dot} {}\n", paint(th, GREY, "no colour: NO_COLOR, --plain or PROBBIT_THEME=plain"));
    o
}
/// os/arch · logical cpus · target features · the last measured site updates/s and its date (`self_test_cached`).
fn spec_line() -> String {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get()); let feats = crate::target_features();
    let rate = match self_test_cached() { Some((r, t)) => format!("{} site updates/s (self-test {} UTC)", si(r), date(t)), None => "updates/s not measured".into() };
    format!("{}/{} · {cores} logical cpus · {} · {rate}", std::env::consts::OS, std::env::consts::ARCH, if feats.is_empty() { "baseline".into() } else { feats.join(" ") })
}
/// `$XDG_CACHE_HOME/probbit/stats.json`, else `~/.cache/probbit/stats.json`, else `%LOCALAPPDATA%\probbit\stats.json`
fn cache_file() -> Option<std::path::PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    Some(var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache"))).or_else(|| var("LOCALAPPDATA"))?.join("probbit").join("stats.json"))
}
/// The cached self-test (updates/s, Unix time), re-measured when missing, unreadable, from another version or a day old.
fn self_test_cached() -> Option<(f64, u64)> {
    let path = cache_file(); let now = now_s(); let me = format!("probbit {VERSION}");
    if let Some(j) = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| crate::json::parse(&s).ok()) {
        let f = |k: &str| j.get(k).and_then(Json::as_f64);
        if let (Some(e), Some(r), Some(t)) = (j.get("engine").and_then(Json::as_str), f("site_updates_per_s"), f("unix_s")) {
            if e == me && r > 0.0 && t >= 0.0 && now >= t as u64 && now - (t as u64) < 86_400 { return Some((r, t as u64)); } } }
    let r = self_test()?;
    if let Some(p) = path { if let Some(d) = p.parent() { let _ = std::fs::create_dir_all(d); }
        let doc = obj(vec![("engine", jstr(&me)), ("unix_s", num(now as f64)), ("site_updates_per_s", num(r.round())), ("program", jstr("400-spin ring, IR sampler, 4 chains, 30 ms"))]);
        let _ = std::fs::write(&p, crate::json::write(&doc, false) + "\n"); }
    Some((r, now))
}
/// `probbit stats`'s self-test program (400-spin ring, 4 chains) on a 30 ms wall-clock budget: under 50 ms in all.
fn self_test() -> Option<f64> {
    let prog = crate::run::from_json(&crate::json::parse(&crate::ring(400)).ok()?).ok()?;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(4);
    let t0 = Instant::now(); let s = probbit_ir::sample_on(&prog.m, 4, threads, 0, Some(30.0), 1, false, 100, 0)?;
    Some(s.sweeps as f64 * 400.0 / t0.elapsed().as_secs_f64())
}

// ---------------- --top ----------------
/// What the running command is doing (`tier`), set at each tier change by decide / run; read only by the monitor.
static TIER: Mutex<&str> = Mutex::new("starting");
/// The gate's numbers once the gate has run (`gate_seen`)
static GATE_LINE: Mutex<String> = Mutex::new(String::new());
#[allow(clippy::type_complexity)]
static TOP: Mutex<Option<(std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>)>> = Mutex::new(None);
pub fn tier(t: &'static str) { if let Ok(mut g) = TIER.lock() { *g = t; } }
pub fn gate_seen(rhat: f64, tv: f64, released: usize, n: usize) {
    if let Ok(mut g) = GATE_LINE.lock() { *g = format!("R-hat {rhat:.4} · TV bound {tv:.4} (tolerance {}) · released {released}/{n}", GATE.tv_tol); }
}
/// The job `--top` watches: command, variables per sweep, chains, threads, the sampler budget, fixed sweeps per chain (0 = wall
/// clock) and the whole-call deadline (`probbit run --deadline-ms`)
pub struct TopSpec { pub cmd: &'static str, pub vars: usize, pub chains: usize, pub threads: usize, pub budget_ms: f64, pub sweeps: usize, pub deadline_ms: Option<f64> }
/// Start the `--top` monitor: a box on stderr redrawn in place at 10 Hz (tier, work and updates/s, CPU and peak RSS, the gate),
/// erased by `top_stop`. It reads the sampler's progress counter (as `--progress`) and process telemetry; the job is unchanged.
pub fn top_start(th: Theme, sp: TopSpec) {
    use std::sync::atomic::Ordering::Relaxed;
    probbit_ir::PROGRESS_SWEEPS.store(0, Relaxed); probbit_ir::PROGRESS_ON.store(true, Relaxed); theme::restore_cursor_on_interrupt();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let h = std::thread::spawn(move || {
        let t0 = Instant::now(); let mut drawn = 0usize; let (mut lt, mut ls, mut lc) = (0.0f64, 0u64, crate::sys::usage().map_or(0.0, |u| u.0)); let (mut rate, mut cpu) = (0.0, None);
        while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(Duration::from_millis(100)) {
            let t = t0.elapsed().as_secs_f64(); let s = probbit_ir::PROGRESS_SWEEPS.load(Relaxed); let u = crate::sys::usage();
            if t > lt { rate = (s - ls) as f64 * sp.vars as f64 / (t - lt); if let Some(u) = u { cpu = Some((u.0 - lc) / ((t - lt) * 1e3) * 100.0); lc = u.0; } }
            (lt, ls) = (t, s);
            let lines = top_lines(th, &sp, t, s, rate, cpu, u.map(|u| u.1));
            let mut f = if drawn > 0 { format!("\x1b[{drawn}F") } else { "\x1b[?25l".to_string() };
            for l in &lines { f += l; f += "\x1b[K\n"; } drawn = lines.len(); err_write(&f);
        }
        err_write(&if drawn > 0 { format!("\x1b[{drawn}F\x1b[J\x1b[?25h") } else { String::new() });
    });
    if let Ok(mut g) = TOP.lock() { *g = Some((tx, h)); }
}
/// Stop the monitor and erase its box (no-op without one). Called before anything else is written to stdout or stderr.
pub fn top_stop() {
    let t = TOP.lock().ok().and_then(|mut g| g.take());
    if let Some((tx, h)) = t { drop(tx); let _ = h.join(); }
}
fn bar(frac: f64, w: usize) -> String { let k = (frac.clamp(0.0, 1.0) * w as f64).round() as usize; format!("{}{}", "█".repeat(k), "░".repeat(w - k)) }
fn top_lines(th: Theme, sp: &TopSpec, t: f64, sweeps: u64, rate: f64, cpu: Option<f64>, rss: Option<f64>) -> Vec<String> {
    let w = theme::size(2).0.saturating_sub(1).clamp(44, 100); let inner = w - 4;
    let tier = TIER.lock().map(|g| *g).unwrap_or(""); let gate = GATE_LINE.lock().map(|g| g.clone()).unwrap_or_default();
    let (when, frac) = if sp.sweeps > 0 { let all = (sp.sweeps * sp.chains) as f64; (format!("{} / {} sweeps", group(sweeps), group(all as u64)), sweeps as f64 / all) }
        else if let Some(d) = sp.deadline_ms { (format!("{t:.2} s / deadline {:.2} s", d / 1e3), t * 1e3 / d) }
        else { (format!("{t:.2} s · sampler budget {:.2} s", sp.budget_ms / 1e3), t * 1e3 / sp.budget_ms.max(1.0)) };
    let rows = [("tier", th.bold(YELLOW, tier)),
        ("work", format!("{} {} · {} chains × {} vars · {} threads", th.paint(CYAN, &bar(frac, 12)), when, sp.chains, group(sp.vars as u64), sp.threads)),
        ("rate", format!("{} site updates/s · cpu {} · peak rss {}", si(rate), cpu.map_or("n/a".into(), |c| format!("{c:.0}%")), rss.map_or("n/a".into(), |r| format!("{r:.1} MB")))),
        ("gate", if gate.is_empty() { th.paint(GREY, "after the sampler") } else { gate })];
    let title = format!(" probbit {} · top ", sp.cmd);
    let mut v = vec![th.paint(MAGENTA, &format!("┌─{title}{}┐", "─".repeat(w.saturating_sub(3 + title.chars().count()))))];
    for (k, x) in rows { v.push(format!("{} {} {} {}", th.paint(MAGENTA, "│"), th.paint(GREY, &format!("{k:<5}")), pad(&x, inner - 6), th.paint(MAGENTA, "│"))); }
    v.push(th.paint(MAGENTA, &format!("└{}┘", "─".repeat(w - 2)))); v
}

// ---------------- the --summary box ----------------
fn verdict_hue(v: &str) -> Col { match v { "exact" => CYAN, "diagnostics_passed" => GREEN, "partial" => YELLOW, _ => MAGENTA } }
/// `--summary --pretty` with a terminal on stderr: the summary document as a box (verdict, counts, gate, the worst items, work).
pub fn summary_box(th: Theme, cmd: &str, s: &Json) -> String {
    let st = |j: Option<&Json>| j.and_then(Json::as_str).unwrap_or("").to_string(); let f = |j: Option<&Json>| j.and_then(Json::as_f64);
    let v = st(s.get("verdict")); let hue = verdict_hue(&v); let c = s.get("counts"); let cf = |k: &str| f(c.and_then(|c| c.get(k)));
    let (items, unit) = if cmd == "decide" { (cf("tasks"), "tasks") } else { (cf("vars"), "vars") };
    let mut rows: Vec<String> = vec![];
    let mut head = format!("{} {unit}", group(items.unwrap_or(0.0) as u64));
    if let (Some(r), Some(e)) = (cf("released"), cf("escalated")) { head += &format!(" · released {} · escalated {}", group(r as u64), group(e as u64)); }
    if let Some(x) = f(s.get("violations")) { head += &format!(" · violations {x}"); }
    if let Some(x) = f(s.get("plan_logw")) { head += &format!(" · plan log-weight {x:.3}"); }
    rows.push(head);
    if let Some(r) = s.get("reason").and_then(Json::as_str) { rows.push(th.paint(GREY, r)); }
    if let Some(g) = s.get("gate") {
        rows.push(format!("gate  R-hat {:.4} · TV bound {:.4} (tolerance {}) · {} samples × {} chains", f(g.get("rhat")).unwrap_or(f64::NAN), f(g.get("tv_bound")).unwrap_or(f64::NAN),
            f(g.get("tv_tol")).unwrap_or(GATE.tv_tol), group(f(g.get("samples")).unwrap_or(0.0) as u64), f(g.get("chains")).unwrap_or(0.0))); }
    let item = |x: &Json| { let id = st(x.get("id")); let odds = x.get("odds").and_then(Json::as_obj).map_or(String::new(), |o| o.iter().map(|(k, p)| format!("{k} {:.2}", p.as_f64().unwrap_or(0.0))).collect::<Vec<_>>().join(" / "));
        let b = f(x.get("bar")).map_or(String::new(), |b| format!(" ±{b:.3}")); format!("{id}: {odds}{b}") };
    for (key, label) in [("worst_released", "worst released"), ("worst_escalated", "escalated")] {
        if let Some(a) = s.get(key).and_then(Json::as_arr).filter(|a| !a.is_empty()) { rows.push(format!("{}  {}", th.paint(GREY, label), a.iter().take(3).map(item).collect::<Vec<_>>().join(" · "))); } }
    let t = s.get("telemetry"); let tf = |k: &str| f(t.and_then(|t| t.get(k)));
    let mut work = format!("{:.1} ms", f(s.get("ms")).unwrap_or(0.0));
    if let Some(u) = tf("site_updates_per_s") { work += &format!(" · {} site updates/s", si(u)); }
    if let Some(r) = tf("peak_rss_mb") { work += &format!(" · peak rss {r:.1} MB"); }
    rows.push(th.paint(GREY, &work));
    let w = theme::size(2).0.saturating_sub(1).clamp(44, 110); let inner = w - 4;
    let title = format!(" probbit {cmd} · {} ", v.to_uppercase());
    let mut o = format!("{}{}{}\n", th.paint(hue, "╭─"), th.bold(hue, &title), th.paint(hue, &format!("{}╮", "─".repeat(w.saturating_sub(3 + title.chars().count())))));
    for r in rows { o += &format!("{} {} {}\n", th.paint(hue, "│"), pad(&r, inner), th.paint(hue, "│")); }
    o += &th.paint(hue, &format!("╰{}╯", "─".repeat(w - 2))); o.push('\n'); o
}

// ---------------- probbit demo --live ----------------
/// Persistent router chains stepped in slices for the live view. Each chain is built (`Chain::new`, the two-group move when
/// `auto_group_pairs`) and its sweeps recorded exactly as `probbit_decide::sample_on` records them for `total` fixed sweeps (burn-in
/// `total / 10`, `record_row` with the same row cap), so after `total` sweeps `samples()` is bit-identical to that call: the
/// final look is `probbit decide --sweeps total --polish-ms 0` (test `live_stepper_matches_sample_on`).
pub struct Stepper<'a> { p: &'a Problem, pub chains: Vec<Chain<'a>>, parts: Vec<Samples>, strides: Vec<usize>, pub burn: usize, max_rows: usize }
impl<'a> Stepper<'a> {
    pub fn new(p: &'a Problem, n: usize, seed: u64, total: usize, max_rows: usize) -> Option<Self> {
        let gpt = if probbit_decide::auto_group_pairs(p) { GroupPairs::new(p) } else { None };
        let chains = (0..n).map(|c| { let mut ch = Chain::new(p, seed, c as u64)?; ch.set_group_pairs(gpt.clone()); Some(ch) }).collect::<Option<Vec<_>>>()?;
        let parts = (0..n).map(|_| Samples { chain_marg: vec![], traj: vec![vec![]], marg: vec![0.0; p.t * p.a], n: 0, plans: HashMap::new(), trace: vec![vec![]], viol: 0,
            best: (f64::NEG_INFINITY, vec![]), sweeps: 0, moves: vec![] }).collect();
        Some(Stepper { p, chains, parts, strides: vec![1; n], burn: total / 10, max_rows })
    }
    /// Every chain `m` more sweeps, one thread per chain (each chain's draws are its own, so threads change nothing).
    pub fn step(&mut self, m: usize) {
        let (p, burn, max_rows) = (self.p, self.burn, self.max_rows);
        let one = move |ch: &mut Chain, s: &mut Samples, stride: &mut usize| for _ in 0..m { ch.sweep(); s.sweeps += 1; if s.sweeps <= burn { continue; }
            for i in 0..p.t { s.marg[i * p.a + ch.x[i]] += 1.0; }
            s.viol += (p.violations(&ch.x) > 0) as usize;
            let lw = ch.logw(); probbit_ir::record_row(s, &ch.x, lw, stride, max_rows); s.n += 1;
            if lw > s.best.0 { s.best = (lw, ch.x.clone()); } };
        std::thread::scope(|sc| { for ((ch, s), stride) in self.chains.iter_mut().zip(self.parts.iter_mut()).zip(self.strides.iter_mut()) { sc.spawn(move || one(ch, s, stride)); } });
    }
    /// The pooled samples so far (as `sample_on` pools its chains).
    pub fn samples(&self) -> Samples {
        let parts = self.parts.iter().zip(&self.chains).map(|(s, ch)| Samples { chain_marg: vec![], traj: s.traj.clone(), marg: s.marg.clone(), n: s.n, plans: HashMap::new(),
            trace: s.trace.clone(), viol: s.viol, best: if s.n == 0 { (ch.logw(), ch.x.clone()) } else { s.best.clone() }, sweeps: s.sweeps, moves: vec![ch.moves] }).collect();
        probbit_ir::merge(self.p.t * self.p.a, parts)
    }
}

/// Worker hues of the field (cycled past six) as brightness ramps, dark to bright (xterm-256), and as 16-colour SGR codes.
const RAMPS: [[u8; 5]; 6] = [[23, 30, 37, 44, 51], [53, 89, 125, 161, 197], [58, 100, 142, 184, 226], [22, 28, 34, 40, 46], [94, 130, 166, 202, 208], [54, 55, 92, 129, 141]];
const HUE16: [u8; 6] = [36, 35, 33, 32, 31, 34];
/// A p-bit's share in 0..=1 -> 0 (dark: the terminal's background) or brightness 1..=5
fn level(x: f64) -> usize { if x < 0.08 { 0 } else { 1 + ((x - 0.08) / 0.92 * 4.999) as usize } }
/// SGR strings per (hue, level 1..=5): [fg, bg]
fn sgr_table(th: Theme) -> Vec<[String; 2]> {
    (0..30).map(|k| { let (w, l) = (k / 5, k % 5 + 1); match th.depth {
        Depth::Ansi256 => [format!("\x1b[38;5;{}m", RAMPS[w][l - 1]), format!("\x1b[48;5;{}m", RAMPS[w][l - 1])],
        Depth::Ansi16 => { let c = HUE16[w] + if l >= 3 { 60 } else { 0 }; [format!("\x1b[{c}m"), format!("\x1b[{}m", c + 10)] } } }).collect()
}
/// Field geometry: tasks run left to right in lanes, one p-bit row per worker (two rows per text line, half blocks), one column
/// per `per` tasks: as wide as the terminal allows (at most 64 columns) and at most `max_lines` text lines high.
fn field_layout(n: usize, a: usize, cols: usize, max_lines: usize) -> (usize, usize, usize) {
    let lines = a.div_ceil(2); let maxw = cols.saturating_sub(12).clamp(8, 64);
    let mut per = 1;
    loop { let need = n.div_ceil(per); let lanes = need.div_ceil(maxw);
        if lanes * lines <= max_lines.max(lines) || per >= n { return (lanes, need.div_ceil(lanes), per); } per += 1; }
}
/// The field as text lines: p-bit (task i, worker w) lit with worker w's hue at brightness `f[i * a + w]` (columns of `per` tasks
/// show their mean).
#[allow(clippy::too_many_arguments)]
fn field_lines(th: Theme, tab: &[[String; 2]], f: &[f64], n: usize, a: usize, lay: (usize, usize, usize), labels: &[String], dot: &str) -> Vec<String> {
    let (lanes, wid, per) = lay; let mut out = vec![];
    for lane in 0..lanes { for r in (0..a).step_by(2) {
        let mut s = if r == 0 { format!("  {} ", th.paint(GREY, &format!("{:<6}", labels.get(lane * wid * per).map_or("", |x| x.as_str())))) } else { "         ".to_string() };
        s.push_str(dot);
        let (mut cf, mut cb): (usize, usize) = (usize::MAX, usize::MAX); // current fg / bg table index (usize::MAX - 1 = default bg)
        for c in 0..wid { let col = lane * wid + c; let (lo, hi) = (col * per, ((col + 1) * per).min(n));
            let val = |w: usize| if w >= a || lo >= hi { 0.0 } else { (lo..hi).map(|i| f[i * a + w]).sum::<f64>() / (hi - lo) as f64 };
            let (t, b) = (level(val(r)), level(val(r + 1)));
            let (glyph, fg, bg) = match (t, b) { (0, 0) => (' ', usize::MAX, usize::MAX - 1), (t, 0) => ('▀', (r % 6) * 5 + t - 1, usize::MAX - 1),
                (0, b) => ('▄', ((r + 1) % 6) * 5 + b - 1, usize::MAX - 1), (t, b) => ('▀', (r % 6) * 5 + t - 1, ((r + 1) % 6) * 5 + b - 1) };
            if bg != cb { if bg == usize::MAX - 1 { s.push_str("\x1b[49m"); } else { s.push_str(&tab[bg][1]); } cb = bg; }
            if fg != cf && fg != usize::MAX { s.push_str(&tab[fg][0]); cf = fg; }
            s.push(glyph); }
        s.push_str(RESET); s.push_str(dot); out.push(s); } }
    out
}
fn verdict_of(g: &probbit_decide::Gate) -> (&'static str, usize) {
    let rel = g.released_tasks(&GATE).iter().filter(|&&r| r).count();
    if g.diagnostics_passed(&GATE) { ("PASSED", g.sig_tv.len()) } else if rel > 0 { ("PARTIAL", rel) } else { ("REFUSED", 0) }
}
fn stamp(th: Theme, word: &str, line: &str) -> String {
    let hue = match word { "PASSED" => GREEN, "PARTIAL" => YELLOW, "EXACT" => CYAN, _ => MAGENTA };
    let body = format!("  {}  {line}  ", th.bold(hue, word)); let w = width(&body);
    format!("  {}\n  {}{body}{}\n  {}\n", th.paint(hue, &format!("╔{}╗", "═".repeat(w))), th.paint(hue, "║"), th.paint(hue, "║"), th.paint(hue, &format!("╚{}╝", "═".repeat(w))))
}

/// What the live story needs from the demo generator: the document, the problem it parses to, and the seed / --hard it came from.
pub struct DemoRun<'a> { pub doc: &'a Json, pub p: &'a Problem, pub tasks: &'a [String], pub workers: &'a [String], pub seed: u64, pub hard: bool }
/// Sweeps per chain of the live story, gate looks, sweeps per frame (20 frames a second: 80 frames, ~4 s).
const LIVE_SWEEPS: usize = 3200;
const LIVE_LOOK: usize = 200;
const LIVE_FRAME: usize = 40;
/// `probbit demo --live`: the router story on stderr. The queue, the rules and what a per-task argmax router breaks; the exact
/// tiers (and what they decline); then the p-bit field: 4 chains stepped `LIVE_FRAME` sweeps per frame, every p-bit drawn from
/// the chains' current states (no synthetic animation), the gate re-run every `LIVE_LOOK` sweeps on the samples so far, the
/// verdict ladder, the 3-line stamp, and the command that reproduces the final look as JSON. Deterministic under --seed: fixed
/// work per frame; only the wall-clock pacing (20 Hz) varies.
pub fn live(th: Theme, d: &DemoRun) {
    let (p, t, a) = (d.p, d.p.t, d.p.a); let (cols, rows) = theme::size(2); let dot = th.paint(DIM, "·");
    let lab = |s: &str| th.bold(MAGENTA, &format!("{s:<7}")); let hi = |s: &str| th.bold(CYAN, s); let gr = |s: &str| th.paint(GREY, s);
    theme::restore_cursor_on_interrupt(); let t_all = Instant::now();
    let mut o = format!("\x1b[?25l\n  {}  {} {dot} {}\n\n", th.bold(CYAN, "probbit demo --live"), hi(&format!("route {t} agent tasks to {a} workers under hard rules")), gr(&format!("seed {}{}", d.seed, if d.hard { " · --hard" } else { "" })));
    let slots: usize = p.cap.iter().sum(); let groups = { let mut g: Vec<usize> = p.group.iter().copied().filter(|&g| g != usize::MAX).collect(); g.sort_unstable(); g.dedup(); g.len() };
    o += &format!("  {} {} tasks {dot} {} customer workflows {dot} {a} workers {dot} {} quota slots ({:.0}% full)\n", lab("QUEUE"), group(t as u64), group(groups as u64), group(slots as u64), 100.0 * t as f64 / slots.max(1) as f64);
    o += &format!("  {} PII only on local-gemma or human {dot} prod-DB migrations never on luna-pro or local-gemma {dot} quotas are hard {dot} affinity {}\n", lab("RULES"), p.lam);
    // the naive router: every task's best-scoring worker from the document's scores, rules ignored
    let tasks = d.doc.get("tasks").and_then(Json::as_arr).unwrap_or(&[]);
    let raw: Vec<f64> = tasks.iter().flat_map(|tk| d.workers.iter().map(move |w| tk.get("scores").and_then(|s| s.get(w)).and_then(Json::as_f64).unwrap_or(f64::NEG_INFINITY))).collect();
    let naive: Vec<usize> = (0..t).map(|i| (0..a).max_by(|&u, &v| raw[i * a + u].total_cmp(&raw[i * a + v])).unwrap_or(0)).collect();
    let (mut pii, mut prod) = (0, 0); for (i, &w) in naive.iter().enumerate() { if !p.allowed[i * a + w] { if crate::DEMO_TPL[i % crate::DEMO_TPL.len()].1 { pii += 1 } else { prod += 1 } } }
    let mut load = vec![0usize; a]; for &w in &naive { load[w] += 1; } let over: usize = (0..a).map(|w| load[w].saturating_sub(p.cap[w])).sum();
    let nv = p.violations(&naive);
    o += &format!("  {} per-task argmax router: {} {dot} {pii} PII leaks {dot} {prod} prod-DB on cheap models {dot} {over} over quota\n", lab("NAIVE"), th.bold(MAGENTA, &format!("{nv} rule violations")));
    err_write(&o);
    // the exact tiers, as `probbit decide` runs them by default
    let te = Instant::now(); let limit = 2_000_000u64; let fs = probbit_ir::FRONTIER_MAX_STATES;
    let lowered = p.lower_until(None).map(|m| m.compiled().0);
    let space = lowered.as_ref().map_or(0.0, |l| (0..l.n).map(|i| (l.cand_count(i).max(1) as f64).log10()).sum::<f64>());
    let comp_first = space > (limit as f64).log10();
    let comp = || lowered.as_ref().and_then(|l| probbit_ir::exact_components_until(l, limit, fs, None)).filter(|c| !c.infeasible).map(|c| (c.marg, c.map, "components"));
    let mut tried = vec![]; let mut exact: Option<(Vec<f64>, Vec<usize>, &str)> = None;
    if comp_first { exact = comp(); if exact.is_none() { tried.push("components"); } }
    if exact.is_none() { exact = probbit_decide::exact_within(p, 5, limit, None).filter(|e| e.n_feasible > 0).map(|e| (e.marg, e.top[0].1.clone(), "enumeration")); if exact.is_none() { tried.push("enumeration"); } }
    if exact.is_none() { exact = probbit_decide::exact_frontier_until(p, fs, None).map(|f| (f.marg, f.map, "frontier DP")); if exact.is_none() { tried.push("frontier DP"); } }
    if exact.is_none() && !comp_first { exact = comp(); if exact.is_none() { tried.push("components"); } }
    let ems = te.elapsed().as_secs_f64() * 1e3;
    let labels: Vec<String> = d.tasks.to_vec();
    let tab = sgr_table(th); let legend = (0..a).map(|w| format!("{} {}", th.paint(Col(RAMPS[w % 6][4], HUE16[w % 6] + 60), "▀"), gr(&d.workers[w]))).collect::<Vec<_>>().join("  ");
    let field_max = rows.saturating_sub(10).max(3);
    let lay = field_layout(t, a, cols, field_max); let fdot = th.paint(DIM, "▏");
    if let Some((marg, map, tier)) = exact {
        err_write(&format!("  {} 10^{space:.0} plans {dot} answered by the {} tier in {ems:.1} ms: exact odds, no sampling needed\n\n", lab("EXACT"), hi(tier)));
        let mut s = String::new(); for l in field_lines(th, &tab, &marg, t, a, lay, &labels, &fdot) { s += &l; s.push('\n'); }
        s += &format!("         {legend}\n         {}\n\n", gr("brightness = exact probability of that worker"));
        s += &stamp(th, "EXACT", &format!("all {t} tasks · proof-grade odds under the score model"));
        s += &format!("  {} {} {dot} log-weight {:.3} {dot} naive router: {nv}\n", lab("PLAN"), hi(&format!("{} rule violations", p.violations(&map))), p.logw(&map));
        s += &format!("  {} probbit demo --tasks {t} --seed {}{} | probbit decide\n\x1b[?25h\n", lab("SAME"), d.seed, if d.hard { " --hard" } else { "" });
        err_write(&s); return;
    }
    err_write(&format!("  {} 10^{space:.0} plans {dot} {} declined in {ems:.0} ms {dot} {}\n\n", lab("EXACT"), tried.join(", "), hi("the p-bit sampler takes over")));
    let max_rows = crate::default_max_rows(4, t);
    let Some(mut st) = Stepper::new(p, 4, d.seed, LIVE_SWEEPS, max_rows) else { err_write(&format!("{}\n\x1b[?25h", stamp(th, "REFUSED", "no feasible start"))); return };
    let nch = st.chains.len() as f64; let mut f = vec![0.0; t * a];
    let (mut t_samp, mut t_gate, mut t_draw, mut cpu_draw) = (Duration::ZERO, Duration::ZERO, Duration::ZERO, Some(0.0f64));
    let mut first: [Option<usize>; 3] = [None; 3]; let mut cur: Option<(&str, usize, f64, f64)> = None; let mut drawn = 0usize; let mut last = None;
    let frames = LIVE_SWEEPS / LIVE_FRAME; let looks = LIVE_SWEEPS / LIVE_LOOK - st.burn / LIVE_LOOK; let mut nlook = 0;
    let period = Duration::from_millis(50); let mut next = Instant::now();
    for fr in 1..=frames + 1 {
        let done = fr > frames; let sw = fr.min(frames) * LIVE_FRAME;
        if !done { let ts = Instant::now(); st.step(LIVE_FRAME); t_samp += ts.elapsed();
            let mut m = vec![0.0; t * a]; for ch in &st.chains { for i in 0..t { m[i * a + ch.x[i]] += 1.0 / nch; } }
            for (x, y) in f.iter_mut().zip(&m) { *x = 0.5 * *x + 0.5 * y; }
            if sw % LIVE_LOOK == 0 && sw > st.burn { let tg = Instant::now(); let s = st.samples(); let g = probbit_decide::gate_stats(p, &s); let (v, rel) = verdict_of(&g);
                let k = ["REFUSED", "PARTIAL", "PASSED"].iter().position(|x| *x == v).unwrap_or(0); if first[k].is_none() { first[k] = Some(sw); }
                cur = Some((v, rel, g.rhat, g.tv_bound(&GATE))); nlook += 1; if sw == LIVE_SWEEPS { last = Some((s, g)); } t_gate += tg.elapsed(); } }
        else if let Some((s, _)) = &last { f = s.marg.clone(); } // the last frame: the answer's odds
        let (td, cd) = (Instant::now(), crate::sys::thread_cpu_ms());
        let mut b = vec![format!("  {} {} p-bits = {t} tasks × {a} workers {dot} 4 chains {dot} sweep {} / {} {dot} {}", lab("FIELD"), group((t * a) as u64), group(sw as u64), group(LIVE_SWEEPS as u64),
            gr(if done { "final odds" } else { "slow motion: 1 frame = 40 sweeps" }))];
        b.extend(field_lines(th, &tab, &f, t, a, lay, &labels, &fdot));
        b.push(format!("         {legend}  {}", gr("· brightness = share of chains")));
        b.push(match cur { None => format!("  {} {}", lab("GATE"), gr("burn-in: the first 10% of the sweeps are discarded")),
            Some((v, rel, rh, tv)) => format!("  {} look {nlook}/{looks} {dot} R-hat {rh:.4} {dot} TV bound {} {dot} released {}/{t} {}", lab("GATE"),
                if tv <= GATE.tv_tol { th.paint(GREEN, &format!("{tv:.4} ≤ {}", GATE.tv_tol)) } else { th.paint(YELLOW, &format!("{tv:.4} > {}", GATE.tv_tol)) }, rel, th.paint(verdict_hue(match v { "PASSED" => "diagnostics_passed", "PARTIAL" => "partial", _ => "refused" }), &bar(rel as f64 / t as f64, 20))) });
        let rung = |k: usize, w: &str| { let at = first[k].map_or(String::new(), |s| gr(&format!(" @{}", group(s as u64))));
            if cur.is_some_and(|c| c.0 == w) { format!("{}{at}", th.bold([MAGENTA, YELLOW, GREEN][k], &format!("[ {w} ]"))) } else if first[k].is_some() { format!("{}{at}", th.paint([MAGENTA, YELLOW, GREEN][k], w)) } else { th.paint(DIM, w) } };
        b.push(format!("  {} {} {} {} {} {}", lab("LADDER"), rung(0, "REFUSED"), th.paint(DIM, "━━"), rung(1, "PARTIAL"), th.paint(DIM, "━━"), rung(2, "PASSED")));
        let mut s = if drawn > 0 { format!("\x1b[{drawn}F") } else { String::new() };
        for l in &b { s += &clip(l, cols.saturating_sub(1)); s += "\x1b[K\n"; } drawn = b.len(); err_write(&s); t_draw += td.elapsed();
        cpu_draw = match (cpu_draw, cd, crate::sys::thread_cpu_ms()) { (Some(acc), Some(a), Some(z)) => Some(acc + z - a), _ => None };
        if !done { next += period; let now = Instant::now(); if next > now { std::thread::sleep(next - now); } else { next = now; } }
    }
    let Some((s, g)) = last else { err_write("\x1b[?25h"); return };
    let (v, rel) = verdict_of(&g); let esc = t - rel;
    let line = match v { "PASSED" => format!("all {t} tasks released · every diagnostic passed"), "PARTIAL" => format!("{rel} of {t} tasks released · {esc} escalated to a person"), _ => format!("0 of {t} released · escalate the queue") };
    // the renderer's share: its thread CPU time (wall time where that clock is missing: an upper bound) of the process CPU
    let cpu = crate::sys::usage().map(|u| u.0); let drawms = cpu_draw.unwrap_or(t_draw.as_secs_f64() * 1e3);
    let mut o = format!("\n{}", stamp(th, v, &line));
    o += &format!("  {} {} (naive router: {nv}) {dot} plan log-weight {:.3} {dot} best plan from the chains, unpolished\n", lab("PLAN"), hi(&format!("{} rule violations", p.violations(&s.best.1))), s.best.0);
    o += &format!("  {} {} sweeps × 4 chains on 4 threads: sampling {:.0} ms, gate {nlook} looks {:.0} ms, renderer {drawms:.1} ms CPU{} {dot} {}\n", lab("WORK"), group(LIVE_SWEEPS as u64),
        t_samp.as_secs_f64() * 1e3, t_gate.as_secs_f64() * 1e3, cpu.map_or(String::new(), |c| format!(" = {:.1}% of {} ms process CPU", 100.0 * drawms / c.max(1e-9), group(c as u64))),
        gr(&format!("paced to {:.1} s at 20 frames/s", t_all.elapsed().as_secs_f64())));
    o += &format!("  {} probbit demo --tasks {t} --seed {}{} | probbit decide --sweeps {LIVE_SWEEPS} --polish-ms 0\n          {}\n", lab("SAME"), d.seed, if d.hard { " --hard" } else { "" },
        gr("the same 4 chains and the same gate as one JSON document, at full speed (default controls)"));
    o += &format!("  {} probbit demo | probbit decide --summary --pretty {dot} claude mcp add probbit -- probbit mcp {dot} probbit --help\n\x1b[?25h\n", lab("NEXT"));
    err_write(&o);
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The live view's chains are the decision's chains: stepping 4 chains in 40-sweep slices to 400 sweeps gives the samples of
    /// `sample_on` with 400 fixed sweeps (what `probbit decide --sweeps 400 --polish-ms 0` gates), bit for bit.
    #[test]
    fn live_stepper_matches_sample_on() {
        let doc = crate::demo_doc(60, 3, false); let mut n = crate::from_json(&doc).unwrap();
        n.p.collective = true; n.p.cluster = true; n.p.cycles = true;
        let rows = crate::default_max_rows(4, n.p.t);
        let mut st = Stepper::new(&n.p, 4, 3, 400, rows).unwrap(); for _ in 0..10 { st.step(40); }
        let a = st.samples(); let b = probbit_decide::sample_on(&n.p, 4, 2, 400, None, 3, false, probbit_decide::auto_group_pairs(&n.p), 100, rows).unwrap();
        assert_eq!((a.n, a.sweeps, a.viol), (b.n, b.sweeps, b.viol)); assert_eq!(a.marg, b.marg); assert_eq!(a.trace, b.trace); assert_eq!(a.traj, b.traj);
        assert_eq!(a.best, b.best); assert_eq!(a.moves, b.moves); assert_eq!(a.chain_marg, b.chain_marg);
    }
    #[test]
    fn layout_fits_and_formats() {
        let (lanes, w, per) = field_layout(300, 6, 120, 30); assert!(lanes * w * per >= 300 && w <= 64 && lanes * 3 <= 30, "{lanes} {w} {per}");
        let (lanes, w, per) = field_layout(300, 6, 80, 6); assert!(lanes * 3 <= 6 && lanes * w * per >= 300, "{lanes} {w} {per}");
        assert_eq!((group(1234567), group(12)), ("1,234,567".to_string(), "12".to_string())); assert_eq!(date(0), "1970-01-01"); assert_eq!(date(1_759_363_200), "2025-10-02");
        assert_eq!(width("\x1b[1m\x1b[38;5;51mab\x1b[0m c"), 4); assert_eq!(width(&clip("\x1b[31mabcdef\x1b[0m", 3)), 3);
    }
}
