//! `probbit monitor STRAND` (docs/persona.md §5.8): watch an individual's inner state as live horizontal bars. The monitor
//! replays the strand with `live::Replay`, the rules `probbit live verify` uses, so it recomputes every stance document from
//! the strand alone (no other log, any strand, any machine) and says whether the replay checks. It draws the latest event: the
//! stance (each trait's levels as segments with their exact odds, the level taken highlighted, the phrase it says), the moods
//! with a sparkline of the last 50 events, the senses (the event's inputs and the history features), the habits in force and
//! the ones that bound, the learned deltas, the drives when the documents carry them (feature-detected), and the stance line.
//! `--serve` draws the same in a browser: one page embedded in the binary, fed by server-sent events from a server on
//! 127.0.0.1. It changes no file (the demo's week goes through a temporary one, removed at once) and sends nothing anywhere.
use crate::json::{self, Json};
use crate::live::Replay;
use crate::persona::{self, Engine};
use crate::theme::{self, Col, Theme, CYAN, DIM, GREEN, GREY, MAGENTA, YELLOW};
use std::collections::VecDeque;
use std::io::{Read, Seek, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Moods (warm amber), wanting (orange), a line that diverges or a prediction error below zero (red), the shades of the levels
/// not taken
const AMBER: Col = Col(214, 33);
const ORANGE: Col = Col(208, 33);
const RED: Col = Col(196, 91);
const SHADE: [Col; 2] = [Col(244, 37), Col(239, 90)];
/// Events a mood's sparkline covers
const SPARK: usize = 50;
/// The tutor persona `--demo` plays (a copy of examples/persona/tutor.yaml; a test keeps them equal)
const TUTOR: &str = include_str!("demo-tutor.yaml");
/// Cells of a level bar and of a number bar
const BAR: usize = 20;
const NBAR: usize = 8;
/// The page `--serve` serves at `/`: one file, its styles and script inline; it fetches nothing but `/events` from its own server
const PAGE: &str = include_str!("monitor.html");
/// Seconds between heartbeats on a quiet event stream
const BEAT: u64 = 15;
/// Stance documents `/doc/N` keeps (the latest; about 2.5 KB each)
const KEEP: usize = 10_000;
/// Connections served at once (more are refused with 503)
const CONNS: usize = 64;

const HELP: &str = "usage: probbit monitor STRAND [--follow] [--once] [--plain] [--fps N] [--serve] [--open] [--port N] | probbit monitor --demo [--once] [--plain] [--serve] [--open] [--port N]\n  Watch an individual's inner state (docs/persona.md §5.8): replays the strand (`probbit live --strand` writes it) with the\n  rules of `probbit live verify`, recomputing every stance document from the strand alone, and draws the latest event as\n  horizontal bars: the stance (each trait's levels with their exact odds, the level taken highlighted, the phrase it says), the\n  moods with a sparkline of the last 50 events, the senses (the event's inputs, the history features), the habits in force and\n  the ones that bound, the learned deltas, the drives when the documents carry them, and the stance line. In the terminal, or\n  with --serve as a page in the browser. It changes no file (--demo: its week goes through a temporary one, removed at\n  once) and sends nothing anywhere.\nflags:\n  --follow    keep watching: lines appended to the strand are replayed and drawn within a second (a line counts once its\n              newline is written); a truncated or rotated strand is replayed from the start, with a warning; Ctrl-C quits\n  --once      one frame on stdout, then exit (the default without --follow)\n  --plain     plain ASCII, no colour (NO_COLOR=1: no colour; PROBBIT_THEME=plain or TERM=dumb: as --plain)\n  --fps N     with --follow, at most N redraws per second (1-60, default 10)\n  --serve     the page instead of the terminal, following the strand (or playing the demo week, over and over): its URL\n              http://127.0.0.1:PORT/ is the line on stdout. GET / the page (one file; it fetches no fonts, styles or\n              scripts), /events its server-sent events (the layout, the latest frame, then a frame per event, a heartbeat\n              every 15 s; events that land within one 100 ms poll, or while a page falls behind, come as the latest\n              frame), /doc/N event N's stance document, as `probbit live` printed it (the latest 10000 are kept). It\n              binds 127.0.0.1 and no other address (there is no --host), answers requests addressed to 127.0.0.1 or\n              localhost, and sends nothing anywhere; Ctrl-C quits\n  --open      serve the page (as --serve does) and open it in the default browser (BROWSER if set; else open on macOS,\n              start on Windows, xdg-open elsewhere); a browser that does not start fails nothing\n  --port N    with --serve or --open, the port (default 0: a free one)\n  --demo      the tutor's scripted week (as `probbit live examples/persona/tutor.yaml --seed 2 --demo week`), replayed in\n              memory: at a terminal or with --serve paced 1 s per hour, each night in 2 s (under a minute); otherwise, or\n              with --once, its last frame\nexit: 0 every line replays, 1 a line differs (the frame names it and shows the event before it), 2 a bad flag, a port that\n  cannot be had, or a file that cannot be read or is not a strand.\n";

fn strs(j: Option<&Json>) -> Vec<String> { j.and_then(Json::as_arr).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() }

/// A trait or a mood: its id, levels and per-level phrases, in the persona's order
struct Var { id: String, levels: Vec<String>, say: Vec<String> }
/// A declared input: its id, kind (flag | level | number) and, for a number, its max
struct Input { id: String, kind: String, max: f64 }
/// What the layout reads from the strand's header
struct Meta { name: String, version: String, seed: u64, individual: String, engine: String, traits: Vec<Var>, moods: Vec<Var>, inputs: Vec<Input>,
    history: Vec<(String, f64)>, learning: Option<(Vec<String>, f64)>, caps: (f64, f64), goals: Vec<String> }
impl Meta {
    fn of(r: &Replay) -> Meta {
        let d = persona::describe(&r.live.p);
        let obj = |k: &str| d.get(k).and_then(Json::as_obj).map(|o| o.to_vec()).unwrap_or_default();
        let decl = |k: &str| r.doc.get(k).and_then(Json::as_arr).map(|a| a.to_vec()).unwrap_or_default();
        let max = |id: &str| decl("inputs").iter().find(|i| i.get("id").and_then(Json::as_str) == Some(id)).and_then(|i| i.get("max")).and_then(Json::as_f64).unwrap_or(1.0);
        let cap = |b: &str, d: f64| r.doc.get("drives").and_then(|x| x.get(b)).and_then(|x| x.get("cap")).and_then(Json::as_f64).unwrap_or(d);
        Meta { name: r.live.p.name.clone(), version: r.live.p.version.clone(), seed: r.live.st.seed,
            individual: r.header.get("state").and_then(|s| s.get("digest")).and_then(Json::as_str).unwrap_or("").to_string(),
            engine: r.header.get("engine").and_then(Json::as_str).unwrap_or("").to_string(),
            traits: obj("traits").into_iter().map(|(id, t)| Var { levels: strs(t.get("levels")), say: strs(t.get("say")), id }).collect(),
            moods: obj("moods").into_iter().map(|(id, l)| Var { levels: strs(Some(&l)), say: vec![], id }).collect(),
            inputs: obj("inputs").into_iter().map(|(id, x)| Input { kind: x.get("kind").and_then(Json::as_str).unwrap_or("").to_string(), max: max(&id), id }).collect(),
            history: decl("history").iter().filter_map(|h| Some((h.get("id")?.as_str()?.to_string(), h.get("cap").and_then(Json::as_f64).unwrap_or(5.0)))).collect(),
            learning: d.get("learning").map(|l| (strs(l.get("traits")), l.get("total_cap").and_then(Json::as_f64).unwrap_or(1.0))),
            caps: (cap("wanting", 3.0), cap("afterglow", 2.0)),
            // a drives block's goals, in the order of the levels of `pursue` (describe hides that trait; its learned row needs them)
            goals: strs(d.get("drives").and_then(|x| x.get("goals"))) }
    }
}

/// One replayed event: its number, its stance document, its inputs as the strand logs them, the learned deltas after it
struct Frame { n: u64, doc: Json, given: Json, learned: Json }
/// A strand being watched: the replay, the layout, the latest frame, the moods' recent positions, the line that diverged (if any)
struct Watch { rep: Replay, meta: Meta, last: Option<Frame>, spark: Vec<VecDeque<f64>>, bad: Option<(usize, String)> }
impl Watch {
    fn open(head: &str) -> Result<Watch, (usize, String)> {
        let rep = Replay::open(head)?; let meta = Meta::of(&rep); let n = meta.moods.len();
        Ok(Watch { rep, meta, last: None, spark: vec![VecDeque::new(); n], bad: None })
    }
    /// Replay one event line; after a line that differs, nothing more is replayed
    fn feed(&mut self, l: &str, eng: Engine) {
        if self.bad.is_some() { return; }
        match self.rep.step(l, eng) {
            Ok(doc) => {
                let given = json::parse(l).ok().and_then(|j| j.get("inputs").cloned()).unwrap_or(Json::Null);
                let learned = self.rep.live.st.to_json(&self.rep.live.p).get("learned").cloned().unwrap_or(Json::Null);
                for (m, s) in self.meta.moods.iter().zip(self.spark.iter_mut()) { if s.len() == SPARK { s.pop_front(); } s.push_back(position(&odds(&doc, "mood", m))); }
                self.last = Some(Frame { n: self.rep.live.n, doc, given, learned });
            }
            Err(e) => self.bad = Some(e) }
    }
}
/// A variable's odds in its levels' order (0 for a level the document does not list)
fn odds(doc: &Json, group: &str, v: &Var) -> Vec<f64> {
    let o = doc.get(group).and_then(|g| g.get(&v.id)).and_then(|x| x.get("odds"));
    v.levels.iter().map(|l| o.and_then(|o| o.get(l)).and_then(Json::as_f64).unwrap_or(0.0)).collect()
}
/// Where the odds sit along the levels, 0 (the lowest level) to 1 (the highest): the mean level index over (levels - 1)
fn position(ps: &[f64]) -> f64 {
    let (t, k) = (ps.iter().sum::<f64>(), ps.len());
    if k < 2 || t <= 0.0 { return 0.0; }
    ps.iter().enumerate().map(|(i, p)| p * i as f64).sum::<f64>() / t / (k - 1) as f64
}

/// How a frame is drawn: the theme (None: no colour), plain ASCII, the columns a line may take
struct Style { th: Option<Theme>, ascii: bool, cols: usize }
impl Style {
    fn paint(&self, c: Col, s: &str) -> String { self.th.map_or_else(|| s.to_string(), |t| t.paint(c, s)) }
    fn bold(&self, c: Col, s: &str) -> String { self.th.map_or_else(|| s.to_string(), |t| t.bold(c, s)) }
    fn g<'a>(&self, uni: &'a str, ascii: &'a str) -> &'a str { if self.ascii { ascii } else { uni } }
}
/// Odds -> cells per level summing to `w` (largest remainder; ties to the earlier level)
fn cells(ps: &[f64], w: usize) -> Vec<usize> {
    let t = ps.iter().map(|p| p.max(0.0)).sum::<f64>();
    if t <= 0.0 { return vec![0; ps.len()]; }
    let raw: Vec<f64> = ps.iter().map(|p| p.max(0.0) / t * w as f64).collect();
    let mut c: Vec<usize> = raw.iter().map(|x| x.floor() as usize).collect();
    let mut order: Vec<usize> = (0..ps.len()).collect();
    order.sort_by(|&a, &b| (raw[b] - raw[b].floor()).total_cmp(&(raw[a] - raw[a].floor())).then(a.cmp(&b)));
    let left = w.saturating_sub(c.iter().sum());
    for &i in order.iter().take(left) { c[i] += 1; }
    c
}
/// A stacked bar: one segment per level, the level taken (`pick`) in `hue`, the others in alternating shades
fn stack(s: &Style, ps: &[f64], pick: Option<usize>, hue: Col) -> String {
    cells(ps, BAR).iter().enumerate().filter(|(_, &n)| n > 0).map(|(i, &n)| {
        if Some(i) == pick { s.paint(hue, &s.g("█", "#").repeat(n)) }
        else { let glyph = if s.th.is_some() { s.g("█", "#") } else if i % 2 == 0 { s.g("▒", "=") } else { s.g("░", "-") }; s.paint(SHADE[i % 2], &glyph.repeat(n)) } }).collect()
}
/// `level 0.37` per level, the level taken in brackets
fn labels(s: &Style, levels: &[String], ps: &[f64], pick: Option<usize>, hue: Col) -> String {
    levels.iter().zip(ps).enumerate().map(|(i, (l, p))| if Some(i) == pick { s.bold(hue, &format!("[{l} {p:.2}]")) } else { s.paint(GREY, &format!(" {l} {p:.2} ")) }).collect::<Vec<_>>().join(" ")
}
/// A number bar: `v / max` of `NBAR` cells
fn nbar(s: &Style, v: f64, max: f64, hue: Col) -> String {
    let k = ((v / max.max(1e-12)).clamp(0.0, 1.0) * NBAR as f64).round() as usize;
    format!("{}{}{}{}", s.paint(DIM, s.g("▕", "[")), s.paint(hue, &s.g("█", "#").repeat(k)), s.paint(DIM, &s.g("░", ".").repeat(NBAR - k)), s.paint(DIM, s.g("▏", "]")))
}
/// A centred bar for a signed value within ±cap: `h` cells each side of the centre mark
fn centred(s: &Style, v: f64, cap: f64, h: usize) -> String {
    let k = ((v.abs() / cap.max(1e-12)).clamp(0.0, 1.0) * h as f64).round() as usize;
    let (fill, mid) = (s.g("━", if v < 0.0 { "-" } else { "+" }), s.g("┃", "|"));
    let hue = if v < 0.0 { MAGENTA } else { GREEN };
    if v < 0.0 { format!("{}{}{}{}", " ".repeat(h - k), s.paint(hue, &fill.repeat(k)), s.paint(DIM, mid), " ".repeat(h)) }
    else { format!("{}{}{}{}", " ".repeat(h), s.paint(DIM, mid), s.paint(hue, &fill.repeat(k)), " ".repeat(h - k)) }
}
/// The moods' recent positions as a sparkline
fn spark(s: &Style, xs: &VecDeque<f64>) -> String {
    let ramp: Vec<char> = s.g("▁▂▃▄▅▆▇█", "_.:-=+*#").chars().collect();
    xs.iter().map(|x| ramp[(x.clamp(0.0, 1.0) * (ramp.len() - 1) as f64).round() as usize]).collect()
}
/// Hours as a short clock: seconds, minutes or hours
fn hours(h: f64) -> String { if h < 1.0 / 60.0 { format!("+{:.0} s", h * 3600.0) } else if h < 1.0 { format!("+{:.1} min", h * 60.0) } else { format!("+{h:.2} h") } }
/// A count or a cap: whole numbers without decimals
fn whole(x: f64) -> String { if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{x}") } }
/// A digest's leading 8 hex digits
fn short(d: &str) -> &str { let h = d.strip_prefix("sha256:").unwrap_or(d); &h[..h.len().min(8)] }

/// One frame: the lines to draw (without line endings). `note`: a warning to show (a truncated or rotated strand, an
/// incomplete last line).
fn frame(w: Option<&Watch>, bad_head: Option<&(usize, String)>, path: &str, note: Option<&str>, s: &Style) -> Vec<String> {
    let dot = s.paint(DIM, s.g("·", "|"));
    let lab = |t: &str| s.bold(MAGENTA, &format!("{t:<8}"));
    let mut out = vec![];
    let bad = bad_head.or_else(|| w.and_then(|w| w.bad.as_ref()));
    let badge = match bad { Some((n, why)) => s.bold(RED, &format!("line {n} diverges: {why}")), None => s.bold(GREEN, s.g("replay verified ✓", "replay verified [ok]")) };
    let Some(w) = w else {
        out.push(format!("{} {dot} {badge}", s.bold(CYAN, "probbit monitor")));
        if let Some(t) = note { out.push(s.paint(YELLOW, t)); }
        out.push(footer(path, "", s));
        return out;
    };
    let m = &w.meta;
    let f = w.last.as_ref();
    let mut head = format!("{} {} {dot} seed {} {dot} individual {} {dot} event {}", s.bold(CYAN, "probbit monitor"), s.bold(CYAN, &format!("{} {}", m.name, m.version)), m.seed, short(&m.individual), f.map_or(0, |f| f.n));
    if let Some(h) = f.and_then(|f| f.given.get("elapsed_hours")).and_then(Json::as_f64) { head += &format!(" {dot} {} since the event before", hours(h)); }
    out.push(format!("{head} {dot} {badge}"));
    if let Some(t) = note { out.push(s.paint(YELLOW, t)); }
    let Some(f) = f else {
        out.push(s.paint(GREY, "no events yet: the strand holds its header and no event lines"));
        out.push(footer(path, &m.engine, s));
        return out;
    };
    let doc = &f.doc;
    let idw = m.traits.iter().chain(&m.moods).map(|v| v.id.chars().count()).chain(m.learning.iter().flat_map(|l| l.0.iter().map(|t| t.chars().count()))).max().unwrap_or(4).clamp(4, 14);
    let status = doc.get("status").and_then(Json::as_str).unwrap_or("");
    // 1. the stance: one row per trait
    let mut top = true;
    for v in &m.traits {
        let Some(t) = doc.get("stance").and_then(|x| x.get(&v.id)) else { continue };
        let ps = odds(doc, "stance", v);
        let level = t.get("level").and_then(Json::as_str).unwrap_or("");
        let released = t.get("released") != Some(&Json::Bool(false));
        let pick = v.levels.iter().position(|l| l == level).filter(|_| released);
        let say = pick.and_then(|i| v.say.get(i)).filter(|x| !x.is_empty()).cloned().unwrap_or_default();
        let note = if released { say } else { s.paint(GREY, &format!("({level}: not released)")) };
        out.push(format!("{}{:<idw$}  {}  {}  {note}", lab(if top { "STANCE" } else { "" }), v.id, stack(s, &ps, pick, CYAN), labels(s, &v.levels, &ps, pick, CYAN)).trim_end().to_string());
        top = false;
    }
    if status != "ok" && !status.is_empty() {
        let esc = doc.get("escalate").and_then(Json::as_str).map_or(String::new(), |e| format!(": {e}"));
        out.push(format!("{}{}", lab(""), s.bold(YELLOW, &format!("status {status}{esc}"))));
    }
    // 2. the moods, with their recent positions
    for (k, v) in m.moods.iter().enumerate() {
        let ps = odds(doc, "mood", v);
        let level = doc.get("mood").and_then(|x| x.get(&v.id)).and_then(|x| x.get("level")).and_then(Json::as_str).unwrap_or("");
        let pick = v.levels.iter().position(|l| l == level);
        out.push(format!("{}{:<idw$}  {}  {}  {}", lab(if k == 0 { "MOODS" } else { "" }), v.id, stack(s, &ps, pick, AMBER), labels(s, &v.levels, &ps, pick, AMBER), s.paint(AMBER, &spark(s, &w.spark[k]))));
    }
    // 3. the senses: the event's inputs as the turn used them, then the history features
    let ins = doc.get("inputs");
    let val = |id: &str| ins.and_then(|i| i.get(id));
    let mut chips = vec![]; let mut nums = vec![];
    for x in &m.inputs {
        match (x.kind.as_str(), val(&x.id)) {
            ("flag", v) => { let on = v == Some(&Json::Bool(true)); chips.push(if on { s.bold(CYAN, &format!("{} {}", s.g("●", "(x)"), x.id)) } else { s.paint(GREY, &format!("{} {}", s.g("○", "( )"), x.id)) }); }
            ("number", v) => { let n = v.and_then(Json::as_f64).unwrap_or(0.0); nums.push(format!("{} {} {n:.2}", x.id, nbar(s, n, x.max, CYAN))); }
            (_, v) => { let l = v.and_then(Json::as_str).unwrap_or(""); chips.push(format!("{} {}", s.paint(GREY, &x.id), s.bold(CYAN, l))); } }
    }
    if let Some(g) = val("goals").filter(|g| !g.is_null()) { chips.push(format!("{} {}", s.paint(GREY, "goals"), persona::canon(g))); }
    out.push(format!("{}{}", lab("SENSES"), chips.join("  ")));
    if let Some(h) = f.given.get("elapsed_hours").and_then(Json::as_f64) { nums.push(format!("clock {}", hours(h))); }
    if !nums.is_empty() { out.push(format!("{}{}", lab(""), nums.join("  "))); }
    let hist: Vec<String> = m.history.iter().map(|(id, cap)| { let n = val(id).and_then(Json::as_f64).unwrap_or(0.0); format!("{id} {}/{}", whole(n), whole(*cap)) }).collect();
    if !hist.is_empty() { out.push(format!("{}{} {}", lab(""), s.paint(GREY, "history"), hist.join("  "))); }
    if let Some(ig) = doc.get("ignored").and_then(Json::as_arr).filter(|a| !a.is_empty()) { out.push(format!("{}{} {}", lab(""), s.paint(GREY, "ignored"), strs(Some(&Json::Arr(ig.to_vec()))).join(" "))); }
    // 4. the habits: in force, the ones that bound highlighted, the violations counter (0 by construction)
    let hb = doc.get("habits");
    let list = |k: &str| strs(hb.and_then(|h| h.get(k)));
    let bound = list("bound");
    let active: Vec<String> = list("active").iter().map(|h| if bound.contains(h) { s.bold(CYAN, &format!("[{h}]")) } else { s.paint(GREY, h) }).collect();
    let viol = hb.and_then(|h| h.get("violations")).and_then(Json::as_f64).unwrap_or(0.0);
    let mut hl = format!("{}{} {dot} violations {}", lab("HABITS"), if active.is_empty() { s.paint(GREY, "none in force") } else { active.join("  ") }, s.bold(if viol == 0.0 { GREEN } else { RED }, &whole(viol)));
    for k in ["yielded", "conflict"] { let l = list(k); if !l.is_empty() { hl += &format!(" {dot} {k} {}", l.join(" ")); } }
    out.push(hl);
    // 5. the learned deltas, centred within ±total_cap
    if let Some((traits, cap)) = &m.learning {
        for (k, t) in traits.iter().enumerate() {
            let levels = m.traits.iter().find(|v| &v.id == t).map(|v| v.levels.clone()).or_else(|| (t == "pursue").then(|| m.goals.clone())).unwrap_or_default();
            let ds: Vec<f64> = f.learned.get(t).and_then(Json::as_arr).map(|a| a.iter().filter_map(Json::as_f64).collect()).unwrap_or_default();
            let cellsx: Vec<String> = levels.iter().zip(&ds).map(|(l, d)| format!("{l} {} {d:+.2}", centred(s, *d, *cap, 5))).collect();
            out.push(format!("{}{t:<idw$}  {}{}", lab(if k == 0 { "LEARNED" } else { "" }), cellsx.join("  "), if k == 0 { s.paint(GREY, &format!("  (cap {}{})", s.g("±", "+/-"), whole(*cap))) } else { String::new() }));
        }
    }
    // 6. the drives, when the document carries them (feature-detected: documents without them draw no row)
    out.extend(drives(doc, m, s, idw, &lab));
    // 7. the line, and why
    if let Some(l) = doc.get("line").and_then(Json::as_str) { out.push(format!("{}{}", lab("LINE"), s.bold(CYAN, l))); }
    if let Some(y) = doc.get("why").and_then(Json::as_str).filter(|y| !y.is_empty()) { out.push(format!("{}{}", lab(""), s.paint(GREY, &format!("why: {y}")))); }
    // 8. the footer
    out.push(footer(path, &m.engine, s));
    out
}
/// The footer: the strand, the engine that wrote it and the command that replays it; the demo's week (path "") is in memory, so
/// the command that writes the same week instead
fn footer(path: &str, engine: &str, s: &Style) -> String {
    let dot = s.g("·", "|");
    let by = if engine.is_empty() { String::new() } else { format!(" {dot} written by {engine}") };
    s.paint(GREY, &if path.is_empty() { format!("strand: the demo week, in memory{by} {dot} the same week: probbit live examples/persona/tutor.yaml --seed 2 --demo week") }
        else { format!("strand {path}{by} {dot} replay: probbit live verify {path}") })
}
/// Per goal: a drive's values, from an object keyed by goal or a list in the goals' order
fn per_goal(v: Option<&Json>, goals: &[String]) -> Vec<Option<f64>> {
    match v { Some(Json::Obj(o)) => goals.iter().map(|g| o.iter().find(|(k, _)| k == g).and_then(|(_, x)| x.as_f64())).collect(),
        Some(Json::Arr(a)) => (0..goals.len()).map(|i| a.get(i).and_then(Json::as_f64)).collect(), _ => vec![None; goals.len()] }
}
/// The drives rows (0.8.0 documents with a drives block): `pursue` as a stance row, then per goal wanting, afterglow and the
/// last prediction error
fn drives(doc: &Json, m: &Meta, s: &Style, idw: usize, lab: &dyn Fn(&str) -> String) -> Vec<String> {
    let (pz, dv) = (doc.get("pursue").filter(|x| x.as_obj().is_some()), doc.get("drives").filter(|x| x.as_obj().is_some()));
    if pz.is_none() && dv.is_none() { return vec![]; }
    let mut out = vec![];
    // the goals: pursue's odds (canonical order), else the goals a drive object names
    let mut goals: Vec<String> = pz.and_then(|p| p.get("odds")).and_then(Json::as_obj).map(|o| o.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default();
    if goals.is_empty() { goals = dv.and_then(Json::as_obj).and_then(|o| o.iter().find_map(|(_, v)| v.as_obj())).map(|o| o.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default(); }
    if let Some(p) = pz {
        let v = Var { id: "pursue".into(), levels: goals.clone(), say: vec![] };
        let ps: Vec<f64> = goals.iter().map(|g| p.get("odds").and_then(|o| o.get(g)).and_then(Json::as_f64).unwrap_or(0.0)).collect();
        let pick = p.get("goal").and_then(Json::as_str).and_then(|g| goals.iter().position(|x| x == g)).filter(|_| p.get("released") != Some(&Json::Bool(false)));
        let say = p.get("say").and_then(Json::as_str).unwrap_or("");
        out.push(format!("{}{:<idw$}  {}  {}  {say}", lab("DRIVES"), v.id, stack(s, &ps, pick, ORANGE), labels(s, &v.levels, &ps, pick, ORANGE)).trim_end().to_string());
    }
    if let Some(d) = dv {
        let (want, glow, err) = (per_goal(d.get("want"), &goals), per_goal(d.get("glow"), &goals), per_goal(d.get("surprise"), &goals));
        for (i, g) in goals.iter().enumerate() {
            let mut row = format!("{}{g:<idw$} ", lab(if pz.is_none() && i == 0 { "DRIVES" } else { "" }));
            if let Some(x) = want[i] { row += &format!(" wanting {} {x:.2}", nbar(s, x, m.caps.0, ORANGE)); }
            if let Some(x) = glow[i] { row += &format!("  afterglow {} {x:.2}", nbar(s, x, m.caps.1, GREEN)); }
            if let Some(x) = err[i].filter(|x| *x != 0.0) { row += &format!("  {}", s.bold(if x < 0.0 { RED } else { GREEN }, &format!("{x:+.2} {} expectation", if x < 0.0 { "below" } else { "above" }))); }
            out.push(row);
        }
    }
    out
}

/// The page's layout (`event: meta`): the strand ("" = the demo week), the engine that wrote it, the persona and its variables
/// in the persona's order (none while there is no header that replays)
fn meta_json(w: Option<&Watch>, path: &str) -> Json {
    let s = |x: &str| Json::Str(x.to_string());
    let strs = |v: &[String]| Json::Arr(v.iter().map(|x| s(x)).collect());
    let o = |kv: Vec<(&str, Json)>| json::obj(kv);
    let mut kv = vec![("path", s(path)), ("demo", Json::Bool(path.is_empty()))];
    if let Some(m) = w.map(|w| &w.meta) {
        let vars = |vs: &[Var]| Json::Arr(vs.iter().map(|v| o(vec![("id", s(&v.id)), ("levels", strs(&v.levels)), ("say", strs(&v.say))])).collect());
        kv.extend([("engine", s(&m.engine)), ("persona", o(vec![("name", s(&m.name)), ("version", s(&m.version)), ("seed", Json::Num(m.seed as f64)), ("individual", s(&m.individual))])),
            ("traits", vars(&m.traits)), ("moods", vars(&m.moods)),
            ("inputs", Json::Arr(m.inputs.iter().map(|x| o(vec![("id", s(&x.id)), ("kind", s(&x.kind)), ("max", Json::Num(x.max))])).collect())),
            ("history", Json::Arr(m.history.iter().map(|(id, cap)| o(vec![("id", s(id)), ("cap", Json::Num(*cap))])).collect())),
            ("learning", m.learning.as_ref().map_or(Json::Null, |(t, cap)| o(vec![("traits", strs(t)), ("total_cap", Json::Num(*cap))]))),
            ("caps", o(vec![("wanting", Json::Num(m.caps.0)), ("afterglow", Json::Num(m.caps.1))])), ("goals", strs(&m.goals))]);
    }
    o(kv)
}
/// One frame for the page (`event: frame`): the latest event (its number, stance document, inputs as the strand logs them,
/// learned deltas after it), the moods' recent positions, the line that diverges and the note, if any
fn frame_json(w: Option<&Watch>, bad_head: Option<&(usize, String)>, note: Option<&str>) -> Json {
    let f = w.and_then(|w| w.last.as_ref());
    let bad = bad_head.or_else(|| w.and_then(|w| w.bad.as_ref()));
    let spark = w.map_or(Json::Obj(vec![]), |w| Json::Obj(w.meta.moods.iter().zip(&w.spark).map(|(m, xs)| (m.id.clone(), Json::Arr(xs.iter().map(|x| Json::Num(persona::r6(*x))).collect()))).collect()));
    let of = |g: fn(&Frame) -> &Json| f.map_or(Json::Null, |f| g(f).clone());
    json::obj(vec![("n", Json::Num(f.map_or(0, |f| f.n) as f64)), ("doc", of(|f| &f.doc)), ("given", of(|f| &f.given)), ("learned", of(|f| &f.learned)), ("spark", spark),
        ("diverges", bad.map_or(Json::Null, |(l, why)| json::obj(vec![("line", Json::Num(*l as f64)), ("why", json::str(why))]))), ("note", note.map_or(Json::Null, json::str))])
}

/// A strand file read as it grows: whole lines (a line counts once its newline is written). It starts over when the
/// file shrinks, disappears, or gets another header (truncated or rotated).
struct Tail { path: String, at: u64, part: Vec<u8>, head: Option<Vec<u8>>, gone: bool, seen: Option<(u64, Option<std::time::SystemTime>)> }
enum Got { Lines(Vec<String>), Restart(&'static str), Nothing }
impl Tail {
    fn new(path: &str) -> Tail { Tail { path: path.to_string(), at: 0, part: vec![], head: None, gone: false, seen: None } }
    fn reset(&mut self) { self.at = 0; self.part.clear(); self.head = None; self.seen = None; }
    fn poll(&mut self) -> Got {
        let Ok(md) = std::fs::metadata(&self.path) else {
            if self.gone { return Got::Nothing; } self.gone = true; self.reset(); return Got::Restart("the strand is gone (removed or rotated): waiting for it");
        };
        if std::mem::replace(&mut self.gone, false) { self.reset(); return Got::Restart("the strand is back: replayed from the start"); }
        let (len, mtime) = (md.len(), md.modified().ok());
        if self.seen == Some((len, mtime)) { return Got::Nothing; }
        let shrank = len < self.at + self.part.len() as u64;
        let mut f = match std::fs::File::open(&self.path) { Ok(f) => f, Err(_) => return Got::Nothing };
        // another header at the top: the file was replaced (rotated) by one at least as long
        let swapped = self.head.as_ref().is_some_and(|h| { let mut b = vec![0u8; h.len()]; f.read_exact(&mut b).is_err() || &b != h });
        if shrank || swapped { self.reset(); return Got::Restart(if shrank { "the strand shrank (truncated or rotated): replayed from the start" } else { "the strand has another header (rotated): replayed from the start" }); }
        self.seen = Some((len, mtime));
        let mut buf = vec![];
        if f.seek(std::io::SeekFrom::Start(self.at + self.part.len() as u64)).and_then(|_| f.read_to_end(&mut buf)).is_err() { return Got::Nothing; }
        self.part.extend_from_slice(&buf);
        let Some(end) = self.part.iter().rposition(|&b| b == b'\n') else { return Got::Nothing };
        let done: Vec<u8> = self.part.drain(..=end).collect();
        self.at += done.len() as u64;
        if self.head.is_none() { self.head = done.iter().position(|&b| b == b'\n').map(|e| done[..e].to_vec()); }
        Got::Lines(crate::live::lines(&String::from_utf8_lossy(&done)).into_iter().map(String::from).collect())
    }
}
/// A strand followed as it grows (`--follow`, `--serve`): the file, the replay (None until a header is in, or when it does not
/// replay), the header's error, the warning to show
struct Followed { tail: Tail, w: Option<Watch>, bad_head: Option<(usize, String)>, note: Option<&'static str> }
impl Followed {
    fn new(path: &str) -> Followed { Followed { tail: Tail::new(path), w: None, bad_head: None, note: None } }
    /// Replay what was appended since the last poll -> None: nothing new; Some(true): the strand starts over (a truncated or
    /// rotated file); Some(false): lines replayed (`each` sees the replay after every event it adds)
    fn poll(&mut self, eng: Engine, each: &mut dyn FnMut(&Watch)) -> Option<bool> {
        match self.tail.poll() {
            Got::Lines(ls) => { for l in ls { if let Some(x) = &mut self.w { let n = x.rep.live.n; x.feed(&l, eng); if x.rep.live.n != n && x.bad.is_none() { each(x); } continue; }
                    if self.bad_head.is_none() { match Watch::open(&l) { Ok(x) => self.w = Some(x), Err(e) => self.bad_head = Some(e) } } }
                Some(false) }
            Got::Restart(why) => { self.w = None; self.bad_head = None; self.note = Some(why); Some(true) }
            Got::Nothing => None }
    }
}

/// Write to stdout -> false once the reader has gone
fn out(s: &str) -> bool { let mut o = std::io::stdout().lock(); o.write_all(s.as_bytes()).and_then(|_| o.flush()).is_ok() }
/// Plain ASCII: --plain, PROBBIT_THEME set to anything but neon, TERM=dumb
fn ascii(args: &[String]) -> bool {
    args.iter().any(|a| a == "--plain") || std::env::var("PROBBIT_THEME").is_ok_and(|v| !v.is_empty() && v != "neon") || std::env::var("TERM").is_ok_and(|t| t == "dumb")
}

/// `probbit monitor STRAND [--follow] [--once] [--plain] [--fps N]`
pub fn cmd(args: &[String]) {
    if args.iter().any(|a| a == "--help" || a == "-h") { crate::emit_raw(HELP); return; }
    if args.iter().any(|a| ["--host", "--bind", "--address"].iter().any(|f| a == f || a.starts_with(&format!("{f}=")))) { crate::fail("monitor --serve binds 127.0.0.1 and no other address, so the page stays on this machine: there is no --host") }
    let path = args.get(1).filter(|a| !a.starts_with("--")).cloned();
    let mut a = vec!["monitor".to_string()]; a.extend(args.iter().skip(if path.is_some() { 2 } else { 1 }).cloned());
    crate::check_flags(&a, &["--fps", "--port"], &["--follow", "--once", "--plain", "--demo", "--serve", "--open"]);
    let has = |f: &str| args.iter().any(|a| a == f);
    // --open opens the page, so it serves it: `--open` is `--serve --open`
    let serve_ = has("--serve") || has("--open");
    let sv = if has("--serve") { "--serve" } else { "--open" };
    if has("--follow") && has("--once") { crate::fail("monitor: --once draws one frame and --follow keeps drawing: give one of them") }
    if serve_ && has("--once") { crate::fail(&format!("monitor: --once draws one frame and {sv} keeps serving: give one of them")) }
    if let Some(f) = ["--fps", "--plain"].iter().find(|f| serve_ && has(f)) { crate::fail(&format!("monitor {sv}: {f} does not apply (the page draws itself)")) }
    if !serve_ && has("--port") { crate::fail("monitor: --port goes with --serve (or --open)") }
    let fps: u32 = crate::arg(args, "--fps", 10); if !(1..=60).contains(&fps) { crate::fail("monitor: --fps N, from 1 to 60") }
    let port: u16 = crate::arg(args, "--port", 0);
    let engine = crate::persona_engine(); let eng: Engine = &engine;
    let tty = theme::stdout_is_terminal();
    let s = Style { th: theme::stdout(args), ascii: ascii(args), cols: if tty { theme::size(1).0.max(20) } else { usize::MAX } };
    if has("--demo") {
        if path.is_some() { crate::fail("monitor --demo plays its own week: give no STRAND") }
        if has("--follow") { crate::fail("monitor --demo: --follow does not apply (the demo is its own week)") }
        if serve_ { serve(port, has("--open"), &mut |hub| feed_demo(hub, eng)) }
        return demo(&s, has("--once"), eng);
    }
    let Some(path) = path else { crate::fail("monitor: the strand file comes before the flags: probbit monitor STRAND [flags] (or probbit monitor --demo)") };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| crate::fail(&format!("monitor: cannot read {path}: {e}")));
    let text = String::from_utf8_lossy(&bytes);
    // whole lines: a last line without its newline is still being written
    let (done, rest) = match text.rfind('\n') { Some(i) => (&text[..=i], &text[i + 1..]), None => ("", &text[..]) };
    let lines = crate::live::lines(done);
    let strand = |h: &str| json::parse(h).ok().and_then(|h| h.get("probbit_strand").and_then(Json::as_f64)) == Some(1.0);
    if let Some(h) = lines.first() { if !strand(h) { crate::fail(&format!("monitor: {path} is not a probbit strand (format 1)")) } }
    // --follow and --serve wait for a header still to be written; one frame needs it now
    if serve_ { serve(port, has("--open"), &mut |hub| feed_strand(&path, hub, eng)) }
    if has("--follow") { follow(&path, &s, fps, eng) }
    let head = lines.first().copied().unwrap_or(rest);
    if head.is_empty() { crate::fail(&format!("monitor: {path} is empty: not a strand")) }
    if !strand(head) { crate::fail(&format!("monitor: {path} is not a probbit strand (format 1)")) }
    let note = (!rest.is_empty() && !lines.is_empty()).then(|| format!("line {} is incomplete (no newline at its end): not replayed", lines.len() + 1));
    let (w, bad_head) = match Watch::open(head) { Ok(mut w) => { for l in lines.iter().skip(1) { w.feed(l, eng); } (Some(w), None) } Err(e) => (None, Some(e)) };
    let fr = frame(w.as_ref(), bad_head.as_ref(), &path, note.as_deref(), &s);
    out(&fr.iter().map(|l| crate::tui::clip(l, s.cols) + "\n").collect::<String>());
    if bad_head.is_some() || w.is_some_and(|w| w.bad.is_some()) { std::process::exit(1) }
}

/// Draw a frame in place at a terminal of `size` (columns, rows) -> false once the reader has gone. A frame taller than the
/// terminal would scroll on every redraw: the rows that fit are kept.
fn redraw(mut fr: Vec<String>, size: (usize, usize)) -> bool {
    fr.truncate(size.1.saturating_sub(1).max(1));
    out(&format!("\x1b[H{}\x1b[J", fr.iter().map(|l| crate::tui::clip(l, size.0.max(20)) + "\x1b[K\n").collect::<String>()))
}

/// The demo's week: the tutor's scripted week (`live::week`, seed 2) written to a temporary strand, read back and removed -> its
/// lines
fn week(eng: Engine) -> Vec<String> {
    let doc = persona::parse_doc(TUTOR, false).unwrap_or_else(|e| crate::fail(&format!("monitor --demo: the tutor: {e}")));
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let f = std::env::temp_dir().join(format!("probbit-monitor-demo-{}-{nanos}.strand", std::process::id()));
    let fp = f.to_string_lossy().to_string();
    let made = crate::live::week(&doc, Some(2), Some(&fp), None, &mut |_: &str| {}, eng);
    let text = std::fs::read_to_string(&f); let _ = std::fs::remove_file(&f);
    if let Err(e) = made { crate::fail(&format!("monitor --demo: {}: {}", e.path, e.msg)) }
    let text = text.unwrap_or_else(|e| crate::fail(&format!("monitor --demo: cannot read the week back: {e}")));
    crate::live::lines(&text).into_iter().map(String::from).collect()
}
/// Replay the week paced, 1 s per hour and each night in 2 s: `show` gets the replay before the week, before each night (with the
/// night's note) and after each event -> false once `show` returns false
fn play(lines: &[String], eng: Engine, show: &mut dyn FnMut(&Watch, Option<&str>) -> bool) -> bool {
    let mut w = Watch::open(&lines[0]).unwrap_or_else(|(n, why)| crate::fail(&format!("monitor --demo: line {n}: {why}")));
    if !show(&w, None) { return false; }
    for l in &lines[1..] {
        let h = json::parse(l).ok().and_then(|j| j.get("inputs").and_then(|i| i.get("elapsed_hours")).and_then(Json::as_f64)).unwrap_or(0.0);
        if h > 1.0 { if !show(&w, Some(&format!("night: {} quiet hours; moods decay by their half-lives", whole(h)))) { return false; }
            std::thread::sleep(Duration::from_secs(2)); }
        else { std::thread::sleep(Duration::from_secs_f64(h)); }
        w.feed(l, eng);
        if !show(&w, None) { return false; }
    }
    true
}
/// `--demo`: the week replayed: at a terminal paced (`play`), otherwise (or with --once) straight to its last frame
fn demo(s: &Style, once: bool, eng: Engine) {
    let lines = week(eng);
    if s.cols == usize::MAX || once {
        let mut w = Watch::open(&lines[0]).unwrap_or_else(|(n, why)| crate::fail(&format!("monitor --demo: line {n}: {why}")));
        for l in &lines[1..] { w.feed(l, eng); }
        out(&(frame(Some(&w), None, "", None, s).join("\n") + "\n")); return;
    }
    theme::restore_cursor_on_interrupt();
    let size = || theme::size(1);
    if !out("\x1b[?25l\x1b[2J") || !play(&lines, eng, &mut |w, note| redraw(frame(Some(w), None, "", note, &Style { cols: size().0, ..*s }), size())) { std::process::exit(0) }
    out("\x1b[?25h");
}

// ---------------------------------------------------------------- --serve
/// What the page shows, shared by the thread that replays and the connections: the layout and the latest frame (each with a
/// counter that moves when it changes; layout 0 = nothing replayed yet) and the latest stance documents (`last` = the event
/// number of the newest)
#[derive(Default)]
struct Board { meta: String, frame: String, layout: u64, seq: u64, docs: VecDeque<String>, last: u64 }
type Hub = Arc<(Mutex<Board>, Condvar)>;
fn board(hub: &Hub) -> std::sync::MutexGuard<'_, Board> { hub.0.lock().unwrap_or_else(|e| e.into_inner()) }
/// The board once the replay has published (a request that comes in while the strand is read waits for it, at most 30 s)
fn ready(hub: &Hub) -> std::sync::MutexGuard<'_, Board> {
    let mut b = board(hub); let until = std::time::Instant::now() + Duration::from_secs(30);
    while b.layout == 0 { let left = until.saturating_duration_since(std::time::Instant::now()); if left.is_zero() { break; } b = hub.1.wait_timeout(b, left).unwrap_or_else(|e| e.into_inner()).0; }
    b
}
/// Show a new frame (and the layout, if it changed) to every page; `docs`: the events replayed since the last publish, with their
/// numbers; `over`: the strand starts over (the documents kept so far go)
fn publish(hub: &Hub, meta: &Json, frame: &Json, docs: Vec<(u64, String)>, over: bool) {
    let (meta, frame) = (persona::canon(meta), persona::canon(frame));
    let mut b = board(hub);
    if over { b.docs.clear(); b.last = 0; }
    for (n, d) in docs { b.docs.push_back(d); b.last = n; if b.docs.len() > KEEP { b.docs.pop_front(); } }
    if b.meta != meta || b.layout == 0 { b.meta = meta; b.layout += 1; }
    b.frame = frame; b.seq += 1;
    hub.1.notify_all();
}
/// An event's stance document as `probbit live` prints it
fn doc_of(w: &Watch) -> (u64, String) { w.last.as_ref().map_or((0, String::new()), |f| (f.n, persona::canon(&f.doc))) }

/// `--serve`: listen on 127.0.0.1:`port` (0: a free port), say the URL on stdout, open the browser if asked, serve every
/// connection on its own thread, and run `feed` (it publishes the frames; it never returns)
fn serve(port: u16, open: bool, feed: &mut dyn FnMut(&Hub)) -> ! {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap_or_else(|e| crate::fail(&format!("monitor --serve: cannot listen on 127.0.0.1:{port}: {e}")));
    let port = l.local_addr().map_or(port, |a| a.port());
    let url = format!("http://127.0.0.1:{port}/");
    out(&format!("{url}\n"));
    crate::err_line(&format!("probbit monitor: the page is at {url} (on this machine, 127.0.0.1); Ctrl-C quits"));
    let hub: Hub = Arc::new((Mutex::new(Board::default()), Condvar::new()));
    let h = hub.clone();
    std::thread::spawn(move || {
        let open = Arc::new(AtomicUsize::new(0));
        for c in l.incoming() { match c {
            Ok(c) => { let (h, open) = (h.clone(), open.clone()); std::thread::spawn(move || { if open.fetch_add(1, Ordering::SeqCst) < CONNS { handle(c, &h, port); } else { busy(c); } open.fetch_sub(1, Ordering::SeqCst); }); }
            Err(_) => std::thread::sleep(Duration::from_millis(50)) } }
    });
    if open { browse(&url); }
    feed(&hub);
    loop { std::thread::park(); }
}
/// The strand followed for the page: every 100 ms, what was appended is replayed and published
fn feed_strand(path: &str, hub: &Hub, eng: Engine) -> ! {
    let mut fl = Followed::new(path);
    let mut fresh = true;
    loop { step(&mut fl, path, hub, eng, &mut fresh, CATCH_UP); std::thread::sleep(Duration::from_millis(100)); }
}
/// One poll of the strand for the page: what was appended is replayed and published. A long replay (a large strand read from
/// its header) is also published every `every` on the way, with a note, so the page draws while it catches up.
fn step(fl: &mut Followed, path: &str, hub: &Hub, eng: Engine, fresh: &mut bool, every: Duration) {
    let (mut docs, mut tick) = (vec![], std::time::Instant::now());
    let got = fl.poll(eng, &mut |w| { docs.push(doc_of(w));
        if tick.elapsed() >= every { tick = std::time::Instant::now();
            publish(hub, &meta_json(Some(w), path), &frame_json(Some(w), None, Some(CATCHING_UP)), std::mem::take(&mut docs), false); } });
    if got.is_some() || *fresh { publish(hub, &meta_json(fl.w.as_ref(), path), &frame_json(fl.w.as_ref(), fl.bad_head.as_ref(), fl.note), docs, got == Some(true)); *fresh = false; }
}
/// How often a long replay shows where it is, and what it says meanwhile
const CATCH_UP: Duration = Duration::from_millis(250);
const CATCHING_UP: &str = "replaying the strand from its header: the board catches up";
/// The demo week for the page, paced, over and over (5 s between the weeks; a week that starts over goes from its last event
/// straight to event 1, without the empty board in between)
fn feed_demo(hub: &Hub, eng: Engine) -> ! {
    let lines = week(eng);
    let mut start = true;
    loop {
        let mut over = true;
        play(&lines, eng, &mut |w, note| { if w.last.is_none() && !std::mem::take(&mut start) { return true; }
            let docs = if w.last.is_some() && note.is_none() { vec![doc_of(w)] } else { vec![] };
            publish(hub, &meta_json(Some(w), ""), &frame_json(Some(w), None, note), docs, std::mem::take(&mut over)); true });
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// One connection: a GET of `/`, `/events` or `/doc/N` addressed to 127.0.0.1:PORT or localhost:PORT (a page of another site
/// that resolves its name to 127.0.0.1 is refused: it would read the documents); anything else is answered with an error
fn handle(mut c: TcpStream, hub: &Hub, port: u16) {
    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = c.set_write_timeout(Some(Duration::from_secs(30)));
    let mut head = vec![]; let mut buf = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        if head.len() > 16_384 { return reply(&mut c, "431 Request Header Fields Too Large", "text/plain", b"request head too large\n"); }
        match c.read(&mut buf) { Ok(0) | Err(_) => return, Ok(k) => head.extend_from_slice(&buf[..k]) }
    }
    let text = String::from_utf8_lossy(&head);
    let mut ls = text.split("\r\n");
    let mut req = ls.next().unwrap_or("").split(' ');
    let (method, target) = (req.next().unwrap_or(""), req.next().unwrap_or(""));
    let host = ls.find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("host")).map(|(_, v)| v.trim().to_ascii_lowercase()));
    if !host.is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}")) { return reply(&mut c, "403 Forbidden", "text/plain", b"this monitor answers requests addressed to 127.0.0.1 or localhost\n"); }
    if method != "GET" { return reply(&mut c, "405 Method Not Allowed", "text/plain", b"this server answers GET\n"); }
    let path = target.split(['?', '#']).next().unwrap_or("");
    match path {
        "/" => reply(&mut c, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        "/events" => events(c, hub, Duration::from_secs(BEAT)),
        p if p.starts_with("/doc/") => {
            let b = ready(hub); let k = b.docs.len() as u64;
            let doc = p[5..].parse::<u64>().ok().filter(|&n| n >= 1 && n <= b.last && b.last - n < k).map(|n| b.docs[(k - 1 - (b.last - n)) as usize].clone() + "\n");
            let msg = format!("no document for {}: events {}..={} are kept\n", &p[5..], (b.last + 1).saturating_sub(k).max(1), b.last);
            drop(b);
            match doc { Some(d) => reply(&mut c, "200 OK", "application/json; charset=utf-8", d.as_bytes()), None => reply(&mut c, "404 Not Found", "text/plain", msg.as_bytes()) } }
        _ => reply(&mut c, "404 Not Found", "text/plain", b"not found: / is the page, /events its stream, /doc/N a stance document\n") }
}
/// The headers every answer carries: no caching, no sniffing, and for the page a policy that lets it load nothing and connect
/// to nothing but its own server
const SAFE: &str = "Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n";
fn reply(c: &mut TcpStream, status: &str, kind: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n{SAFE}Connection: close\r\n\r\n", body.len());
    let _ = c.write_all(head.as_bytes()).and_then(|_| c.write_all(body)).and_then(|_| c.flush());
}
fn busy(mut c: TcpStream) { let _ = c.set_write_timeout(Some(Duration::from_secs(5))); reply(&mut c, "503 Service Unavailable", "text/plain", b"too many connections\n"); }
/// `/events`: the layout and the latest frame, then each new frame (at most 20 a second: a page that falls behind gets the latest),
/// a new layout before the frame when the strand starts over as another, and a heartbeat comment after `beat` quiet (15 s)
fn events(mut c: TcpStream, hub: &Hub, beat: Duration) {
    let _ = c.set_nodelay(true);
    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\n{SAFE}\r\n");
    if c.write_all(head.as_bytes()).and_then(|_| c.flush()).is_err() { return; }
    let (mut layout, mut seq) = (0u64, 0u64);
    loop {
        let (meta, frame) = {
            let mut b = board(hub);
            let until = std::time::Instant::now() + beat;
            while b.layout == 0 || (b.layout == layout && b.seq == seq) {
                let left = until.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() { break; }
                b = hub.1.wait_timeout(b, left).unwrap_or_else(|e| e.into_inner()).0;
            }
            let meta = (b.layout != layout && b.layout != 0).then(|| { layout = b.layout; b.meta.clone() });
            let frame = (b.seq != seq && b.layout != 0).then(|| { seq = b.seq; b.frame.clone() });
            (meta, frame)
        };
        let mut msg = String::new();
        if let Some(m) = meta { msg += &format!("event: meta\ndata: {m}\n\n"); }
        if let Some(f) = frame { msg += &format!("event: frame\ndata: {f}\n\n"); }
        if msg.is_empty() { msg = ": heartbeat\n\n".into(); }
        if c.write_all(msg.as_bytes()).and_then(|_| c.flush()).is_err() { return; }
        std::thread::sleep(Duration::from_millis(50));
    }
}
/// `--open`: the page in the default browser (`BROWSER` if set; else macOS `open`, Windows `cmd /c start`, elsewhere `xdg-open`),
/// best effort: a browser that does not start is said on stderr and fails nothing
fn browse(url: &str) {
    use std::process::{Command, Stdio};
    let mut cmd = match std::env::var("BROWSER").ok().filter(|b| !b.trim().is_empty()) {
        Some(b) => Command::new(b.trim()),
        None if cfg!(target_os = "macos") => Command::new("open"),
        None if cfg!(windows) => { let mut c = Command::new("cmd"); c.args(["/c", "start", ""]); c }
        None => Command::new("xdg-open") };
    match cmd.arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(mut child) => { std::thread::spawn(move || { let _ = child.wait(); }); }
        Err(e) => crate::err_line(&format!("probbit monitor: --open: no browser started ({e}); open {url} by hand")) }
}

/// `--follow`: poll the strand every 100 ms, replay the lines appended, redraw (at most `fps` times a second; in place at a
/// terminal, else one frame after another, each followed by an empty line). Runs until Ctrl-C or until the reader goes away.
fn follow(path: &str, s: &Style, fps: u32, eng: Engine) -> ! {
    let tty = s.cols != usize::MAX;
    if tty { theme::restore_cursor_on_interrupt(); if !out("\x1b[?25l\x1b[2J") { std::process::exit(0) } }
    let mut fl = Followed::new(path);
    let (mut dirty, mut drawn, mut size) = (true, None::<std::time::Instant>, (0, 0));
    let gap = std::time::Duration::from_secs_f64(1.0 / fps as f64);
    loop {
        // a long replay (a large strand read from its header) is drawn on the way at a terminal, with a note
        let mut tick = std::time::Instant::now();
        let mut catch_up = |w: &Watch| if tty && tick.elapsed() >= CATCH_UP { tick = std::time::Instant::now(); let size = theme::size(1);
            redraw(frame(Some(w), None, path, Some(CATCHING_UP), &Style { cols: size.0.max(20), ..*s }), size); };
        if fl.poll(eng, &mut catch_up).is_some() { dirty = true; }
        let now = if tty { theme::size(1) } else { (0, 0) };
        if now != size { size = now; dirty = true; }
        if dirty && drawn.map_or(true, |t| t.elapsed() >= gap) {
            let cols = if tty { size.0.max(20) } else { usize::MAX };
            let fr = frame(fl.w.as_ref(), fl.bad_head.as_ref(), path, fl.note, &Style { cols, ..*s });
            if !(if tty { redraw(fr, size) } else { out(&(fr.join("\n") + "\n\n")) }) { std::process::exit(0) }
            dirty = false; drawn = Some(std::time::Instant::now());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    //! The monitor's documents are the strand's stances: replayed with `live::Replay`, each one's sha256 is the line's `stance`
    //! digest, on strands of the three example personas; a document with drives fields draws without error
    use super::*;
    use crate::live::{self, Clock, Live};

    fn run(prog: &Json, f: &persona::Flags) -> Json { persona::run_program(prog, f, 1, 100, 0) }
    /// A strand of `n` events for the persona at `path`: every declared flag on in turn, numbers 0 / 0.5 / max, level inputs
    /// through their levels, the clock 0.5 to 13 h
    fn strand(path: &str, n: usize) -> String {
        let (p, doc) = persona::load(path).unwrap();
        let d = persona::describe(&p);
        let ins: Vec<(String, Json)> = d.get("inputs").and_then(Json::as_obj).unwrap().to_vec();
        let (mut lv, header) = Live::start(p.clone(), &doc, persona::init(&p, Some(5), true, &run), Clock::Fixed, "probbit test");
        let mut text = format!("{header}\n");
        for t in 0..n {
            let mut ev = vec![("elapsed_hours".to_string(), Json::Num([0.5, 1.0, 13.0, 2.25][t % 4]))];
            for (k, (id, x)) in ins.iter().enumerate() {
                match x.get("kind").and_then(Json::as_str) {
                    Some("flag") => if (t + k) % 3 == 0 { ev.push((id.clone(), Json::Bool(true))); },
                    Some("number") => ev.push((id.clone(), Json::Num([0.0, 0.5, 1.0][(t + k) % 3]))),
                    _ => { let l = strs(x.get("levels")); if !l.is_empty() { ev.push((id.clone(), Json::Str(l[(t + k) % l.len()].clone()))); } } }
            }
            let (_, line) = lv.event(&Json::Obj(ev), &run).unwrap(); text += &line; text.push('\n');
        }
        text
    }

    /// Watch a strand line by line: every document's sha256 is its line's `stance` digest, and the replay ends where `verify` does
    fn replays_to_its_digests(text: &str, name: &str) -> Watch {
        let lines = live::lines(text);
        let mut w = Watch::open(lines[0]).unwrap();
        for l in &lines[1..] {
            w.feed(l, &run); assert!(w.bad.is_none(), "{name}: {:?}", w.bad);
            let want = json::parse(l).unwrap().get("stance").and_then(Json::as_str).unwrap().to_string();
            assert_eq!(persona::sha(&w.last.as_ref().unwrap().doc), want, "{name}: line {}", w.rep.line);
        }
        assert_eq!(w.rep.summary(), live::verify(text, &run).unwrap(), "{name}: the monitor's replay ends where verify does");
        w
    }
    /// More strands to check, named in PROBBIT_MONITOR_STRANDS (paths joined as in PATH; unset: nothing to do)
    #[test]
    fn strands_named_in_the_environment_replay_to_their_digests() {
        let Some(v) = std::env::var_os("PROBBIT_MONITOR_STRANDS") else { return };
        for p in std::env::split_paths(&v) { let w = replays_to_its_digests(&std::fs::read_to_string(&p).unwrap(), &p.display().to_string());
            eprintln!("{}: {} events, every document's sha256 is its line's stance digest", p.display(), w.rep.live.n); }
    }

    #[test]
    fn the_monitors_documents_are_the_strands_stances() {
        for name in ["tutor.yaml", "ops-engineer.yaml", "trader-assistant.yaml"] {
            let text = strand(&format!("{}/../examples/persona/{name}", env!("CARGO_MANIFEST_DIR")), 40);
            let w = replays_to_its_digests(&text, name);
            // every frame style draws it; plain ASCII with --plain
            for (ascii, th) in [(true, None), (false, None), (false, Some(Theme { depth: theme::Depth::Ansi256 }))] {
                let fr = frame(Some(&w), None, "x.strand", None, &Style { th, ascii, cols: usize::MAX });
                assert!(fr.len() > 8 && fr[0].contains(&format!("event {} ", w.rep.live.n)), "{name}: {fr:?}");
                if ascii { assert!(fr.iter().all(|l| l.is_ascii()), "{name}: {fr:?}"); }
            }
            // a changed input: the frame names the line and keeps the event before it
            let bent = text.replacen(r#""elapsed_hours":13"#, r#""elapsed_hours":12"#, 1);
            let bl = live::lines(&bent); let mut b = Watch::open(bl[0]).unwrap(); for l in &bl[1..] { b.feed(l, &run); }
            let (line, _) = b.bad.clone().unwrap(); assert_eq!(live::verify(&bent, &run).unwrap_err().0, line, "{name}");
            assert_eq!(b.last.as_ref().map(|f| f.n as usize), Some(line - 2), "{name}: the frame shows the event before the line that differs");
            assert!(frame(Some(&b), None, "x", None, &Style { th: None, ascii: true, cols: usize::MAX })[0].contains(&format!("event {} ", line - 2)), "{name}");
        }
    }

    /// Each example persona's scripted week (seed 2, as `probbit live PERSONA --seed 2 --demo week --strand` writes it) replays to
    /// its digests
    #[test]
    fn the_example_personas_demo_weeks_replay_to_their_digests() {
        for name in ["tutor.yaml", "ops-engineer.yaml", "trader-assistant.yaml"] {
            let doc = persona::parse_doc(&std::fs::read_to_string(format!("{}/../examples/persona/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap(), false).unwrap();
            let f = std::env::temp_dir().join(format!("probbit-monitor-week-{}-{name}.strand", std::process::id()));
            let fp = f.to_string_lossy().to_string(); let _ = std::fs::remove_file(&f);
            assert!(live::week(&doc, Some(2), Some(&fp), None, &mut |_: &str| {}, &run).is_ok(), "{name}: the demo week");
            let text = std::fs::read_to_string(&f).unwrap(); let _ = std::fs::remove_file(&f);
            let w = replays_to_its_digests(&text, name); assert!(w.rep.live.n >= 40, "{name}: {} events", w.rep.live.n);
        }
    }

    /// A 0.8.0 document with `pursue` and `drives` (hand-written) draws the drives rows; a document without them draws none
    #[test]
    fn drives_are_drawn_when_the_document_carries_them() {
        let text = strand(&format!("{}/../examples/persona/tutor.yaml", env!("CARGO_MANIFEST_DIR")), 3);
        let lines = live::lines(&text); let mut w = Watch::open(lines[0]).unwrap(); for l in &lines[1..] { w.feed(l, &run); }
        let s = Style { th: None, ascii: true, cols: usize::MAX };
        assert!(!frame(Some(&w), None, "x", None, &s).iter().any(|l| l.starts_with("DRIVES")));
        let Json::Obj(mut kv) = w.last.as_ref().unwrap().doc.clone() else { panic!("a document") };
        kv.push(("pursue".into(), json::parse(r#"{"goal":"ship","p":0.62,"odds":{"rest":0.38,"ship":0.62},"released":true,"say":"ship the next small piece","lift":{}}"#).unwrap()));
        kv.push(("drives".into(), json::parse(r#"{"want":{"rest":0.2,"ship":1.4},"glow":{"rest":0,"ship":0.9},"expect":{"rest":0.1,"ship":0.5},"surprise":{"rest":0,"ship":1},"deadline":{"rest":null,"ship":6},"since":{"rest":3,"ship":0}}"#).unwrap()));
        w.last.as_mut().unwrap().doc = Json::Obj(kv);
        let fr = frame(Some(&w), None, "x", None, &s);
        let at = fr.iter().position(|l| l.starts_with("DRIVES")).unwrap_or_else(|| panic!("{fr:?}"));
        assert!(fr[at].contains("[ship 0.62]") && fr[at].contains("ship the next small piece"), "{}", fr[at]);
        assert!(fr[at + 2].contains("wanting [") && fr[at + 2].contains("+1.00 above expectation"), "{fr:?}");
        // odd shapes (lists, strings, nulls) draw without a panic
        let Json::Obj(mut kv) = w.last.as_ref().unwrap().doc.clone() else { panic!() };
        kv.retain(|(k, _)| k != "drives"); kv.push(("drives".into(), json::parse(r#"{"want":[1,"x",null],"glow":"?","surprise":[-0.5]}"#).unwrap()));
        w.last.as_mut().unwrap().doc = Json::Obj(kv); let fr = frame(Some(&w), None, "x", None, &s);
        assert!(fr.iter().any(|l| l.contains("-0.50 below expectation")) || fr.iter().any(|l| l.contains("wanting")), "{fr:?}");
    }

    /// A real drives strand (the fixture persona, goal signals in the events, praise so `pursue` learns): it replays to its
    /// digests, the frame draws `pursue` and every goal's wanting and afterglow, and the learned row of `pursue` names the goals
    #[test]
    fn a_drives_strand_lights_the_drives_rows() {
        let (p, doc) = persona::load(&format!("{}/tests/fixtures/persona/drives-adversary.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let (mut lv, header) = Live::start(p.clone(), &doc, persona::init(&p, Some(4), true, &run), Clock::Fixed, "probbit test");
        let mut text = format!("{header}\n");
        for e in [r#"{"goals":{"fun":{"cue":true,"win":1.5}},"praise":true}"#, r#"{"goals":{"fun":{"win":1.5}},"elapsed_hours":2}"#,
            r#"{"goals":{"craft":{"progress":0.6}},"praise":true,"elapsed_hours":1}"#, r#"{"goals":{"chores":{"deadline_hours":5}},"security":true}"#] {
            let (_, line) = lv.event(&json::parse(e).unwrap(), &run).unwrap(); text += &line; text.push('\n');
        }
        let w = replays_to_its_digests(&text, "drives");
        assert_eq!(w.meta.goals, ["fun", "craft", "chores", "safety"]);
        let fr = frame(Some(&w), None, "x", None, &Style { th: None, ascii: true, cols: usize::MAX });
        let at = fr.iter().position(|l| l.starts_with("DRIVES")).unwrap_or_else(|| panic!("{fr:?}"));
        assert!(fr[at].contains("[chores 1.00]"), "{}", fr[at]);
        for g in ["fun", "craft", "chores", "safety"] { assert!(fr.iter().any(|l| l.trim_start().starts_with(g) && l.contains("wanting [") && l.contains("afterglow [")), "{g}: {fr:?}"); }
        let learned = fr.iter().find(|l| l.trim_start().starts_with("pursue") && !l.starts_with("DRIVES")).unwrap_or_else(|| panic!("{fr:?}"));
        assert!(["fun ", "craft ", "chores ", "safety "].iter().all(|g| learned.contains(g)), "{learned}");
        assert_eq!(meta_json(Some(&w), "x").get("goals").map(|g| strs(Some(g))), Some(w.meta.goals.clone()));
    }

    /// The page's layout lists the persona's variables in its order; a frame carries the event's document as replayed (drives
    /// fields included, as they are), the moods' recent positions, the line that diverges and the note
    #[test]
    fn the_pages_frames_carry_the_documents() {
        let text = strand(&format!("{}/../examples/persona/tutor.yaml", env!("CARGO_MANIFEST_DIR")), 5);
        let lines = live::lines(&text); let mut w = Watch::open(lines[0]).unwrap(); for l in &lines[1..] { w.feed(l, &run); }
        let m = meta_json(Some(&w), "x.strand");
        let ids: Vec<&str> = m.get("traits").and_then(Json::as_arr).unwrap().iter().filter_map(|t| t.get("id").and_then(Json::as_str)).collect();
        assert_eq!(ids, w.meta.traits.iter().map(|v| v.id.as_str()).collect::<Vec<_>>());
        assert_eq!((m.get("demo"), m.get("persona").and_then(|p| p.get("name"))), (Some(&Json::Bool(false)), Some(&Json::Str("Pip".into()))));
        let f = frame_json(Some(&w), None, None);
        assert_eq!((f.get("n"), f.get("diverges"), f.get("note")), (Some(&Json::Num(5.0)), Some(&Json::Null), Some(&Json::Null)));
        assert_eq!(persona::canon(f.get("doc").unwrap()), persona::canon(&w.last.as_ref().unwrap().doc));
        assert_eq!(f.get("spark").and_then(|s| s.get("valence")).and_then(Json::as_arr).map(<[Json]>::len), Some(5));
        let Json::Obj(mut kv) = w.last.as_ref().unwrap().doc.clone() else { panic!("a document") };
        let dv = json::parse(r#"{"want":{"ship":1.4},"surprise":{"ship":-0.5}}"#).unwrap(); kv.push(("drives".into(), dv.clone()));
        w.last.as_mut().unwrap().doc = Json::Obj(kv);
        assert_eq!(frame_json(Some(&w), None, None).get("doc").and_then(|d| d.get("drives")), Some(&dv));
        let bad = frame_json(Some(&w), Some(&(3, "the stance differs".into())), Some("a note"));
        assert_eq!((bad.get("diverges").and_then(|d| d.get("line")), bad.get("note")), (Some(&Json::Num(3.0)), Some(&Json::Str("a note".into()))));
        assert!(meta_json(None, "x").get("persona").is_none() && frame_json(None, None, None).get("doc") == Some(&Json::Null));
    }

    /// A long replay is published on the way (every `every`, with a note), then at its end without the note; a short one once
    #[test]
    fn a_long_replay_is_published_on_the_way() {
        let text = strand(&format!("{}/../examples/persona/tutor.yaml", env!("CARGO_MANIFEST_DIR")), 5);
        let f = std::env::temp_dir().join(format!("probbit-monitor-step-{}.strand", std::process::id()));
        std::fs::write(&f, &text).unwrap(); let path = f.to_string_lossy().to_string();
        for (every, publishes) in [(Duration::ZERO, 6), (Duration::from_secs(3600), 1)] {
            let hub: Hub = Arc::new((Mutex::new(Board::default()), Condvar::new()));
            let (mut fl, mut fresh) = (Followed::new(&path), true);
            step(&mut fl, &path, &hub, &run, &mut fresh, every);
            let b = board(&hub);
            assert_eq!((b.seq, b.docs.len(), b.last, b.layout), (publishes, 5, 5, 1), "{every:?}");
            assert_eq!(json::parse(&b.frame).unwrap().get("note"), Some(&Json::Null), "{every:?}: the last frame has no note");
        }
        let _ = std::fs::remove_file(&f);
    }

    /// Read a stream until `what` is in what came after byte `from`
    fn read_until(s: &mut TcpStream, got: &mut String, from: usize, what: &str) {
        let t = std::time::Instant::now();
        while !got[from..].contains(what) {
            assert!(t.elapsed() < Duration::from_secs(10), "no {what:?} within 10 s: {got}");
            let mut b = [0u8; 4096]; let k = s.read(&mut b).unwrap(); assert!(k > 0, "the stream closed: {got}");
            got.push_str(&String::from_utf8_lossy(&b[..k]));
        }
    }
    /// `/events` on a quiet board: a heartbeat comment after each quiet `beat` (15 s when served; 100 ms here), before the
    /// strand has replayed and after; a frame published meanwhile is pushed at once, the layout before it
    #[test]
    fn a_quiet_event_stream_carries_heartbeats() {
        let hub: Hub = Arc::new((Mutex::new(Board::default()), Condvar::new()));
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap(); let port = l.local_addr().unwrap().port();
        let h = hub.clone();
        std::thread::spawn(move || { if let Ok((c, _)) = l.accept() { events(c, &h, Duration::from_millis(100)); } });
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap(); s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut got = String::new();
        read_until(&mut s, &mut got, 0, ": heartbeat\n\n");
        assert!(got.starts_with("HTTP/1.1 200 OK\r\n") && got.contains("text/event-stream") && !got.contains("event:"), "{got}");
        let at = got.len();
        publish(&hub, &json::obj(vec![("path", json::str("x"))]), &json::obj(vec![("n", Json::Num(1.0))]), vec![], false);
        read_until(&mut s, &mut got, at, "event: meta\ndata: {\"path\":\"x\"}\n\nevent: frame\ndata: {\"n\":1}\n\n");
        let at = got.len(); read_until(&mut s, &mut got, at, ": heartbeat\n\n");
        assert!(!got[at..].contains("event:"), "nothing new, nothing but heartbeats: {got}");
    }

    /// The tutor `--demo` embeds is examples/persona/tutor.yaml
    #[test]
    fn the_demo_plays_the_example_tutor() {
        let ex = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/persona/tutor.yaml")).unwrap();
        assert_eq!(TUTOR.replace("\r\n", "\n"), ex.replace("\r\n", "\n"), "copy examples/persona/tutor.yaml to probbit-cli/src/demo-tutor.yaml");
    }

    #[test]
    fn cells_sum_to_the_bar() {
        for ps in [vec![0.37, 0.56, 0.07], vec![1.0, 0.0, 0.0], vec![0.333334, 0.333333, 0.333333], vec![0.0, 0.0], vec![0.05, 0.05, 0.05, 0.85]] {
            let c = cells(&ps, BAR); assert!(c.iter().sum::<usize>() == BAR || ps.iter().sum::<f64>() == 0.0, "{ps:?} {c:?}");
        }
        assert_eq!(cells(&[0.5, 0.5], 5), vec![3, 2]);
    }
}
