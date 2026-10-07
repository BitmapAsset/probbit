//! `probbit monitor STRAND` (docs/persona.md §5.8): watch an individual's inner state as live horizontal bars. The monitor
//! replays the strand with `live::Replay`, the rules `probbit live verify` uses, so it recomputes every stance document from
//! the strand alone (no other log, any strand, any machine) and says whether the replay checks. It draws the latest event: the
//! stance (each trait's levels as segments with their exact odds, the level taken highlighted, the phrase it says), the moods
//! with a sparkline of the last 50 events, the senses (the event's inputs and the history features), the habits in force and
//! the ones that bound, the learned deltas, the drives when the documents carry them (feature-detected), and the stance line.
//! Nothing is written or sent anywhere.
use crate::json::{self, Json};
use crate::live::Replay;
use crate::persona::{self, Engine};
use crate::theme::{self, Col, Theme, CYAN, DIM, GREEN, GREY, MAGENTA, YELLOW};
use std::collections::VecDeque;
use std::io::{Read, Seek, Write};

/// Moods (warm amber), wanting (orange), a line that diverges or a prediction error below zero (red), the shades of the levels
/// not taken
const AMBER: Col = Col(214, 33);
const ORANGE: Col = Col(208, 33);
const RED: Col = Col(196, 91);
const SHADE: [Col; 2] = [Col(244, 37), Col(239, 90)];
/// Events a mood's sparkline covers
const SPARK: usize = 50;
/// Cells of a level bar and of a number bar
const BAR: usize = 20;
const NBAR: usize = 8;

const HELP: &str = "usage: probbit monitor STRAND [--follow] [--once] [--plain] [--fps N]\n  Watch an individual's inner state (docs/persona.md §5.8): replays the strand (`probbit live --strand` writes it) with the\n  rules of `probbit live verify`, recomputing every stance document from the strand alone, and draws the latest event as\n  horizontal bars: the stance (each trait's levels with their exact odds, the level taken highlighted, the phrase it says), the\n  moods with a sparkline of the last 50 events, the senses (the event's inputs, the history features), the habits in force and\n  the ones that bound, the learned deltas, the drives when the documents carry them, and the stance line. Read-only: nothing is\n  written or sent anywhere.\nflags:\n  --follow    keep watching: lines appended to the strand are replayed and drawn within a second (a line counts once its\n              newline is written); a truncated or rotated strand is replayed from the start, with a warning; Ctrl-C quits\n  --once      one frame on stdout, then exit (the default without --follow)\n  --plain     ASCII only, no colour (NO_COLOR=1: no colour; PROBBIT_THEME=plain or TERM=dumb: as --plain)\n  --fps N     with --follow, at most N redraws per second (1-60, default 10)\nexit: 0 every line replays, 1 a line differs (the frame names it and shows the event before it), 2 a bad flag, or a file that\n  cannot be read or is not a strand.\n";

fn strs(j: Option<&Json>) -> Vec<String> { j.and_then(Json::as_arr).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() }

/// A trait or a mood: its id, levels and per-level phrases, in the persona's order
struct Var { id: String, levels: Vec<String>, say: Vec<String> }
/// A declared input: its id, kind (flag | level | number) and, for a number, its max
struct Input { id: String, kind: String, max: f64 }
/// What the layout reads from the strand's header
struct Meta { name: String, version: String, seed: u64, individual: String, engine: String, traits: Vec<Var>, moods: Vec<Var>, inputs: Vec<Input>,
    history: Vec<(String, f64)>, learning: Option<(Vec<String>, f64)>, caps: (f64, f64) }
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
            caps: (cap("wanting", 3.0), cap("afterglow", 2.0)) }
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
/// Where the odds sit along the levels, 0 (the first) to 1 (the last): the mean level index over (levels - 1)
fn position(ps: &[f64]) -> f64 {
    let (t, k) = (ps.iter().sum::<f64>(), ps.len());
    if k < 2 || t <= 0.0 { return 0.0; }
    ps.iter().enumerate().map(|(i, p)| p * i as f64).sum::<f64>() / t / (k - 1) as f64
}

/// How a frame is drawn: the theme (None: no colour), ASCII only, the columns a line may take
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
/// A digest's first 8 hex digits
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
        out.push(s.paint(GREY, &format!("strand {path} {} replay: probbit live verify {path}", s.g("·", "|"))));
        return out;
    };
    let m = &w.meta;
    let f = w.last.as_ref();
    let mut head = format!("{} {} {dot} seed {} {dot} individual {} {dot} event {}", s.bold(CYAN, "probbit monitor"), s.bold(CYAN, &format!("{} {}", m.name, m.version)), m.seed, short(&m.individual), f.map_or(0, |f| f.n));
    if let Some(h) = f.and_then(|f| f.given.get("elapsed_hours")).and_then(Json::as_f64) { head += &format!(" {dot} {} since the event before", hours(h)); }
    out.push(format!("{head} {dot} {badge}"));
    if let Some(t) = note { out.push(s.paint(YELLOW, t)); }
    let Some(f) = f else {
        out.push(s.paint(GREY, "no events yet: the strand has its header only"));
        out.push(s.paint(GREY, &format!("strand {path} {dot} written by {} {dot} replay: probbit live verify {path}", m.engine)));
        return out;
    };
    let doc = &f.doc;
    let idw = m.traits.iter().chain(&m.moods).map(|v| v.id.chars().count()).chain(m.learning.iter().flat_map(|l| l.0.iter().map(|t| t.chars().count()))).max().unwrap_or(4).clamp(4, 14);
    let status = doc.get("status").and_then(Json::as_str).unwrap_or("");
    // 1. the stance: one row per trait
    let mut first = true;
    for v in &m.traits {
        let Some(t) = doc.get("stance").and_then(|x| x.get(&v.id)) else { continue };
        let ps = odds(doc, "stance", v);
        let level = t.get("level").and_then(Json::as_str).unwrap_or("");
        let released = t.get("released") != Some(&Json::Bool(false));
        let pick = v.levels.iter().position(|l| l == level).filter(|_| released);
        let say = pick.and_then(|i| v.say.get(i)).filter(|x| !x.is_empty()).cloned().unwrap_or_default();
        let note = if released { say } else { s.paint(GREY, &format!("({level}: not released)")) };
        out.push(format!("{}{:<idw$}  {}  {}  {note}", lab(if first { "STANCE" } else { "" }), v.id, stack(s, &ps, pick, CYAN), labels(s, &v.levels, &ps, pick, CYAN)).trim_end().to_string());
        first = false;
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
            let levels = m.traits.iter().find(|v| &v.id == t).map(|v| v.levels.clone()).unwrap_or_default();
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
    out.push(s.paint(GREY, &format!("strand {path} {} written by {} {} replay: probbit live verify {path}", s.g("·", "|"), m.engine, s.g("·", "|"))));
    out
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
    // the goals: pursue's odds (canonical order), else the keys of the first drive object
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

/// A strand file read as it grows: complete lines only (a line counts once its newline is written). It starts over when the
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

/// Write to stdout -> false once the reader has gone
fn out(s: &str) -> bool { let mut o = std::io::stdout().lock(); o.write_all(s.as_bytes()).and_then(|_| o.flush()).is_ok() }
/// ASCII only: --plain, PROBBIT_THEME set to anything but neon, TERM=dumb
fn ascii(args: &[String]) -> bool {
    args.iter().any(|a| a == "--plain") || std::env::var("PROBBIT_THEME").is_ok_and(|v| !v.is_empty() && v != "neon") || std::env::var("TERM").is_ok_and(|t| t == "dumb")
}

/// `probbit monitor STRAND [--follow] [--once] [--plain] [--fps N]`
pub fn cmd(args: &[String]) {
    if args.iter().any(|a| a == "--help" || a == "-h") { crate::emit_raw(HELP); return; }
    let Some(path) = args.get(1).filter(|a| !a.starts_with("--")).cloned() else { crate::fail("monitor: the strand file goes first: probbit monitor STRAND [flags]") };
    let mut a = vec!["monitor".to_string()]; a.extend(args.iter().skip(2).cloned()); crate::check_flags(&a, &["--fps"], &["--follow", "--once", "--plain"]);
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--follow") && has("--once") { crate::fail("monitor: --once draws one frame and --follow keeps drawing: give one of them") }
    let fps: u32 = crate::arg(args, "--fps", 10); if !(1..=60).contains(&fps) { crate::fail("monitor: --fps N, from 1 to 60") }
    let engine = crate::persona_engine(); let eng: Engine = &engine;
    let tty = theme::stdout_is_terminal();
    let s = Style { th: theme::stdout(args), ascii: ascii(args), cols: if tty { theme::size(1).0.max(20) } else { usize::MAX } };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| crate::fail(&format!("monitor: cannot read {path}: {e}")));
    let text = String::from_utf8_lossy(&bytes);
    // complete lines only: a last line without its newline is still being written
    let (done, rest) = match text.rfind('\n') { Some(i) => (&text[..=i], &text[i + 1..]), None => ("", &text[..]) };
    let lines = crate::live::lines(done);
    let strand = |h: &str| json::parse(h).ok().and_then(|h| h.get("probbit_strand").and_then(Json::as_f64)) == Some(1.0);
    if let Some(h) = lines.first() { if !strand(h) { crate::fail(&format!("monitor: {path} is not a probbit strand (format 1)")) } }
    // --follow waits for a header still to be written; one frame needs it now
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

/// `--follow`: poll the strand every 100 ms, replay the lines appended, redraw (at most `fps` times a second; in place at a
/// terminal, else one frame after another, each followed by an empty line). Runs until Ctrl-C or until the reader goes away.
fn follow(path: &str, s: &Style, fps: u32, eng: Engine) -> ! {
    let tty = s.cols != usize::MAX;
    if tty { theme::restore_cursor_on_interrupt(); if !out("\x1b[?25l\x1b[2J") { std::process::exit(0) } }
    let mut tail = Tail::new(path); let mut w: Option<Watch> = None; let mut bad_head: Option<(usize, String)> = None; let mut note: Option<&str> = None;
    let (mut dirty, mut drawn, mut size) = (true, None::<std::time::Instant>, (0, 0));
    let gap = std::time::Duration::from_secs_f64(1.0 / fps as f64);
    loop {
        match tail.poll() {
            Got::Lines(ls) => { for l in ls { if let Some(x) = &mut w { x.feed(&l, eng); continue; }
                    if bad_head.is_none() { match Watch::open(&l) { Ok(x) => w = Some(x), Err(e) => bad_head = Some(e) } } }
                dirty = true; }
            Got::Restart(why) => { w = None; bad_head = None; note = Some(why); dirty = true; }
            Got::Nothing => {}
        }
        let now = if tty { theme::size(1) } else { (0, 0) };
        if now != size { size = now; dirty = true; }
        if dirty && drawn.map_or(true, |t| t.elapsed() >= gap) {
            let cols = if tty { size.0.max(20) } else { usize::MAX };
            let fr = frame(w.as_ref(), bad_head.as_ref(), path, note, &Style { cols, ..*s });
            let body: String = fr.iter().map(|l| crate::tui::clip(l, cols) + if tty { "\x1b[K\n" } else { "\n" }).collect();
            if !out(&if tty { format!("\x1b[H{body}\x1b[J") } else { format!("{body}\n") }) { std::process::exit(0) }
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

    #[test]
    fn the_monitors_documents_are_the_strands_stances() {
        for name in ["tutor.yaml", "ops-engineer.yaml", "trader-assistant.yaml"] {
            let text = strand(&format!("{}/../examples/persona/{name}", env!("CARGO_MANIFEST_DIR")), 40);
            let lines = live::lines(&text);
            let mut w = Watch::open(lines[0]).unwrap();
            for l in &lines[1..] {
                w.feed(l, &run); assert!(w.bad.is_none(), "{name}: {:?}", w.bad);
                let want = json::parse(l).unwrap().get("stance").and_then(Json::as_str).unwrap().to_string();
                assert_eq!(persona::sha(&w.last.as_ref().unwrap().doc), want, "{name}: line {}", w.rep.line);
            }
            assert_eq!(w.rep.summary(), live::verify(&text, &run).unwrap(), "{name}: the monitor's replay ends where verify does");
            // every frame style draws it; ASCII only with --plain
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

    #[test]
    fn cells_sum_to_the_bar() {
        for ps in [vec![0.37, 0.56, 0.07], vec![1.0, 0.0, 0.0], vec![0.333334, 0.333333, 0.333333], vec![0.0, 0.0], vec![0.05, 0.05, 0.05, 0.85]] {
            let c = cells(&ps, BAR); assert!(c.iter().sum::<usize>() == BAR || ps.iter().sum::<f64>() == 0.0, "{ps:?} {c:?}");
        }
        assert_eq!(cells(&[0.5, 0.5], 5), vec![3, 2]);
    }
}
