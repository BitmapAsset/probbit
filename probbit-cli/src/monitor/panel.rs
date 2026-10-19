//! The instrument panel's words (docs/persona.md §5.8): one pure map from a field's reading at this event, and its reading at the
//! event before, to a fixed word line `<label>: <level>, <size>, <direction>` (plain ASCII apart from the persona's names) and a dial value in [0, 1]. The words are bucketed
//! (the number the dial shows is printed beside them, never in them), so a field's words change exactly when its level, size
//! bucket or direction bucket changes; every word is true of the state at that event or of its change since the event before.
//! Mechanism and simulation: the words describe arithmetic over the replayed document and state, nothing more.
use crate::json::{self, Json};

/// Size words, from a magnitude in [0, 1]: [0, 1/3), [1/3, 2/3), [2/3, 1]
pub const SIZES: [&str; 3] = ["slightly", "clearly", "strongly"];
/// The odds of a chosen level: slightly under 0.5, clearly under 0.8, strongly from 0.8
const P_CUTS: [f64; 2] = [0.5, 0.8];
/// Smallest change of a level's position (mean level index over levels - 1) that reads as leaning, rising or falling
const POS_STEP: f64 = 0.02;
/// Smallest change of a number, as a share of its cap, that reads as rising or falling
const NUM_STEP: f64 = 0.01;
/// Smallest learned delta that counts, and smallest change of a learned spread
const LEARN_STEP: f64 = 0.005;
/// A gene under this size reads as even
const GENE_EVEN: f64 = 0.05;
/// A sense's z within this reads as at its normal
const Z_AT: f64 = 0.5;
const FIRST: &str = "first event";
const HOLDING: &str = "holding";
const UNDECLARED: &str = "not declared in this persona";
const ROOM: &str = "not computed (E1 pending)";

/// One field's reading at an event
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    /// A trait (`mood` false) or a mood: the levels, the level taken (None: not reported), released, the odds in level order
    Level { levels: Vec<String>, level: Option<String>, released: bool, odds: Vec<f64>, mood: bool },
    /// A need (RFC 0002 A): its deficit after the event, its cap, its pursue term
    Need { deficit: f64, cap: f64, term: f64 },
    /// A learned trait's surprise gate: its gain g (1 = closed) and g_max
    Gate { g: f64, g_max: f64 },
    /// Pace: the stance's level, the average and its cap
    Pace { level: String, rbar: f64, cap: f64 },
    /// A disposition gene on a signed scale: optimism o, patience ln p, the volatility offset
    Gene { gene: String, x: f64 },
    /// A learned trait: its levels, the learned deltas in level order, total_cap
    Learned { levels: Vec<String>, deltas: Vec<f64>, cap: f64 },
    /// The decisions awaiting an outcome, and max_open
    Credit { open: usize, max: usize },
    /// The habits in force, the ones that bound, the habits the persona declares
    Habits { active: usize, bound: usize, declared: usize },
    /// A sense: its z when read this event
    Sense { z: Option<f64> },
    /// A block the persona does not declare
    Undeclared,
    /// Room left in the learning bound (RFC 0004): not computed until the engine has it
    Room,
}
/// A field at an event: its stable key (`stance:verbosity`, `mood:pace`, `pace`, …), its label and its reading
#[derive(Clone, Debug, PartialEq)]
pub struct Reading { pub key: String, pub label: String, pub field: Field }
/// A field's words at an event: the three buckets, the line, the dial (None: no number) and the number shown beside the words
#[derive(Clone, Debug, PartialEq)]
pub struct Word { pub key: String, pub label: String, pub level: String, pub size: &'static str, pub dir: String, pub text: String, pub dial: Option<f64>, pub value: String }

fn size_of(m: f64) -> &'static str { if m < 1.0 / 3.0 { SIZES[0] } else if m < 2.0 / 3.0 { SIZES[1] } else { SIZES[2] } }
fn size_of_p(p: f64) -> &'static str { if p < P_CUTS[0] { SIZES[0] } else if p < P_CUTS[1] { SIZES[1] } else { SIZES[2] } }
fn gene_size(x: f64) -> &'static str { let a = x.abs(); if a < GENE_EVEN { "" } else if a < 0.25 { SIZES[0] } else if a < 0.75 { SIZES[1] } else { SIZES[2] } }
fn z_size(z: f64) -> &'static str { let a = z.abs(); if a < 1.0 { SIZES[0] } else if a < 2.0 { SIZES[1] } else { SIZES[2] } }
/// rising / falling / holding by a change against its step
fn trend(d: f64, step: f64) -> &'static str { if d >= step - 1e-12 { "rising" } else if d <= -step + 1e-12 { "falling" } else { HOLDING } }
/// Where the odds sit along the levels, 0 (first level) to 1 (last)
fn position(ps: &[f64]) -> f64 {
    let (t, k) = (ps.iter().sum::<f64>(), ps.len());
    if k < 2 || t <= 0.0 { return 0.0; }
    ps.iter().enumerate().map(|(i, p)| p * i as f64).sum::<f64>() / t / (k - 1) as f64
}
fn r2(x: f64) -> String { format!("{x:.2}") }
fn signed(x: f64) -> String { format!("{x:+.2}") }
/// A count or cap without decimals when whole
fn whole(x: f64) -> String { if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{x}") } }

/// The line: `<label>: <level>, <size>, <direction>`, empty parts left out
pub fn line(label: &str, level: &str, size: &str, dir: &str) -> String {
    let parts: Vec<&str> = [level, size, dir].into_iter().filter(|x| !x.is_empty()).collect();
    format!("{label}: {}", parts.join(", "))
}
fn gene_level(gene: &str, x: f64) -> &'static str {
    if x.abs() < GENE_EVEN { return "even"; }
    match (gene, x > 0.0) { ("optimism", true) => "learns more from better than expected", ("optimism", false) => "learns more from worse than expected",
        ("patience", true) => "longer credit memory", ("patience", false) => "shorter credit memory",
        (_, true) => "surprise gate opens later", (_, false) => "surprise gate opens sooner" }
}
fn count_level(n: usize) -> String { if n == 0 { "none open".into() } else { format!("{n} open") } }
fn learned_level(levels: &[String], ds: &[f64]) -> String {
    if ds.iter().all(|d| d.abs() < LEARN_STEP) { return "none yet".into(); }
    let (mut at, mut best) = (0, f64::NEG_INFINITY);
    for (i, d) in ds.iter().enumerate() { if *d > best { best = *d; at = i; } }
    format!("toward {}", levels.get(at).cloned().unwrap_or_else(|| format!("level {at}")))
}
fn spread(ds: &[f64]) -> f64 { if ds.is_empty() { 0.0 } else { ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - ds.iter().cloned().fold(f64::INFINITY, f64::min) } }

/// The words of one field at this event, given its reading at the event before (None: the first event this replay shows, or a
/// field the event before did not have)
pub fn word(now: &Reading, prev: Option<&Reading>) -> Word {
    let before = prev.filter(|p| p.key == now.key).map(|p| &p.field);
    let (level, size, dir, dial, value): (String, &'static str, String, Option<f64>, String) = match &now.field {
        Field::Level { levels, level, released, odds, mood } => {
            let pick = level.as_ref().and_then(|l| levels.iter().position(|x| x == l));
            let p = pick.and_then(|i| odds.get(i)).copied().unwrap_or(0.0);
            let lv = match (level, released) { (None, _) => "not reported".to_string(), (Some(_), false) => "not released".to_string(), (Some(l), true) => l.clone() };
            let sz = if level.is_some() && *released { size_of_p(p) } else { "" };
            let d = match before { Some(Field::Level { odds: o, .. }) => {
                let dp = position(odds) - position(o);
                if *mood { trend(dp, POS_STEP).to_string() }
                else if dp >= POS_STEP - 1e-12 { format!("leaning toward {}", levels.last().map_or("", String::as_str)) }
                else if dp <= -POS_STEP + 1e-12 { format!("leaning toward {}", levels.first().map_or("", String::as_str)) }
                else { HOLDING.to_string() } }
                _ => FIRST.to_string() };
            (lv, sz, d, (level.is_some() && *released).then_some(p), format!("p {}", r2(p)))
        }
        Field::Need { deficit, cap, term } => {
            let m = (deficit / cap.max(1e-12)).clamp(0.0, 1.0);
            let d = match before { Some(Field::Need { deficit: x, .. }) => trend((deficit - x) / cap.max(1e-12), NUM_STEP).to_string(), _ => FIRST.to_string() };
            ((if *term > 0.0 { "pressing" } else { "not pressing" }).to_string(), if *deficit <= 0.0 { "" } else { size_of(m) }, d, Some(m), format!("deficit {}/{}", r2(*deficit), whole(*cap)))
        }
        Field::Gate { g, g_max } => {
            let m = ((g - 1.0) / (g_max - 1.0).max(1e-12)).clamp(0.0, 1.0);
            let d = match before { Some(Field::Gate { g: x, .. }) => trend(g - x, NUM_STEP).to_string(), _ => FIRST.to_string() };
            ((if *g > 1.0 { "gate open" } else { "gate closed" }).to_string(), if *g > 1.0 { size_of(m) } else { "" }, d, Some(m), format!("g {}", r2(*g)))
        }
        Field::Pace { level, rbar, cap } => {
            let m = (rbar / cap.max(1e-12)).clamp(0.0, 1.0);
            let d = match before { Some(Field::Pace { rbar: x, .. }) => trend((rbar - x) / cap.max(1e-12), NUM_STEP).to_string(), _ => FIRST.to_string() };
            (level.clone(), if *rbar <= 0.0 { "" } else { size_of(m) }, d, Some(m), format!("rbar {}/{}", r2(*rbar), whole(*cap)))
        }
        // patience is read on ln p (its words and dial), and shown as the gene p itself
        Field::Gene { gene, x } => (gene_level(gene, *x).to_string(), gene_size(*x), "fixed".to_string(), Some(x.abs().min(1.0)),
            if gene == "patience" { format!("{gene} x{}", r2(x.exp())) } else { format!("{gene} {}", signed(*x)) }),
        Field::Learned { levels, deltas, cap } => {
            let big = deltas.iter().fold(0.0f64, |a, d| a.max(d.abs()));
            let lv = learned_level(levels, deltas);
            let d = match before { Some(Field::Learned { deltas: o, .. }) => trend(spread(deltas) - spread(o), LEARN_STEP).to_string(), _ => FIRST.to_string() };
            let m = (big / cap.max(1e-12)).clamp(0.0, 1.0);
            let sz = if lv == "none yet" { "" } else { size_of(m) };
            let top = deltas.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            (lv, sz, d, Some(m), format!("{}/{}", signed(if top.is_finite() { top } else { 0.0 }), r2(*cap)))
        }
        Field::Credit { open, max } => {
            let m = (*open as f64 / (*max).max(1) as f64).clamp(0.0, 1.0);
            let d = match before { Some(Field::Credit { open: o, .. }) => trend(*open as f64 - *o as f64, 1.0).to_string(), _ => FIRST.to_string() };
            (count_level(*open), if *open == 0 { "" } else { size_of(m) }, d, Some(m), format!("{open}/{max}"))
        }
        Field::Habits { active, bound, declared } => {
            let m = (*active as f64 / (*declared).max(1) as f64).clamp(0.0, 1.0);
            let d = match before { Some(Field::Habits { active: a, .. }) => trend(*active as f64 - *a as f64, 1.0).to_string(), _ => FIRST.to_string() };
            (format!("{active} in force and {bound} bound"), if *active == 0 { "" } else { size_of(m) }, d, Some(m), format!("{active}/{declared}"))
        }
        Field::Sense { z } => match z {
            None => ("not read".to_string(), "", String::new(), None, String::new()),
            Some(z) => ("read".to_string(), z_size(*z), (if *z >= Z_AT { "above its normal" } else if *z <= -Z_AT { "below its normal" } else { "at its normal" }).to_string(),
                Some((z.abs() / 3.0).min(1.0)), format!("z {z:+.1}")),
        },
        Field::Undeclared => (UNDECLARED.to_string(), "", String::new(), None, String::new()),
        Field::Room => (ROOM.to_string(), "", String::new(), None, String::new()),
    };
    let text = line(&now.label, &level, size, &dir);
    Word { key: now.key.clone(), label: now.label.clone(), level, size, dir, text, dial, value }
}
/// Every field's words at this event, the event before's readings matched by key
pub fn words(now: &[Reading], prev: &[Reading]) -> Vec<Word> { now.iter().map(|r| word(r, prev.iter().find(|p| p.key == r.key))).collect() }

#[cfg(test)]
/// The word space of a field: every level × size × direction its words can take (the faithful bar enumerates it)
pub fn space(field: &Field) -> (Vec<String>, Vec<&'static str>, Vec<String>) {
    let s3 = || SIZES.to_vec();
    let num_dirs = || vec![FIRST.to_string(), HOLDING.to_string(), "rising".into(), "falling".into()];
    match field {
        Field::Level { levels, mood, .. } => {
            let mut lv = levels.clone(); lv.extend(["not released".to_string(), "not reported".to_string()]);
            let mut sz = s3(); sz.push("");
            let dirs = if *mood { num_dirs() } else { vec![FIRST.into(), HOLDING.into(), format!("leaning toward {}", levels.last().map_or("", String::as_str)), format!("leaning toward {}", levels.first().map_or("", String::as_str))] };
            (lv, sz, dirs)
        }
        Field::Need { .. } => (vec!["pressing".into(), "not pressing".into()], { let mut s = s3(); s.push(""); s }, num_dirs()),
        Field::Gate { .. } => (vec!["gate open".into(), "gate closed".into()], { let mut s = s3(); s.push(""); s }, num_dirs()),
        Field::Pace { .. } => (vec!["slow".into(), "steady".into(), "brisk".into()], { let mut s = s3(); s.push(""); s }, num_dirs()),
        Field::Gene { gene, .. } => (vec![gene_level(gene, 1.0).into(), gene_level(gene, -1.0).into(), "even".into()], { let mut s = s3(); s.push(""); s }, vec!["fixed".into()]),
        Field::Learned { levels, .. } => { let mut lv: Vec<String> = levels.iter().map(|l| format!("toward {l}")).collect(); lv.push("none yet".into()); (lv, { let mut s = s3(); s.push(""); s }, num_dirs()) }
        Field::Credit { max, .. } => ((0..=*max).map(count_level).collect(), { let mut s = s3(); s.push(""); s }, num_dirs()),
        Field::Habits { declared, .. } => ((0..=*declared).flat_map(|a| (0..=a).map(move |b| format!("{a} in force and {b} bound"))).collect(), { let mut s = s3(); s.push(""); s }, num_dirs()),
        Field::Sense { .. } => (vec!["read".into(), "not read".into()], { let mut s = s3(); s.push(""); s }, vec![String::new(), "above its normal".into(), "below its normal".into(), "at its normal".into()]),
        Field::Undeclared => (vec![UNDECLARED.into()], vec![""], vec![String::new()]),
        Field::Room => (vec![ROOM.into()], vec![""], vec![String::new()]),
    }
}
#[cfg(test)]
/// Every fixed word the panel can print (labels and level names come from the persona file and are not in it)
pub fn vocabulary() -> Vec<String> {
    let mut v: Vec<String> = SIZES.iter().map(|s| s.to_string()).collect();
    v.extend([FIRST, HOLDING, "rising", "falling", "leaning toward", "fixed", "above its normal", "below its normal", "at its normal", "not released", "not reported",
        "pressing", "not pressing", "gate open", "gate closed", "even", "none yet", "toward", "none open", "open", "in force and", "bound", "read", "not read", UNDECLARED, ROOM,
        "deficit", "g", "rbar", "p", "z", "needs", "surprise gate", "pace", "dispositions", "credit", "habits", "room left in the bound", "learned", "need", "sense", "mood", "stance", "average reward"].map(String::from));
    for g in ["optimism", "patience", "volatility"] { for x in [1.0, -1.0, 0.0] { v.push(gene_level(g, x).to_string()); } v.push(g.to_string()); }
    v
}
/// The words as the page's frame carries them
pub fn to_json(ws: &[Word]) -> Json {
    Json::Arr(ws.iter().map(|w| json::obj(vec![("key", json::str(&w.key)), ("label", json::str(&w.label)), ("level", json::str(&w.level)), ("size", json::str(w.size)),
        ("dir", json::str(&w.dir)), ("text", json::str(&w.text)), ("dial", w.dial.map_or(Json::Null, |x| json::num((x * 1e6).round() / 1e6))), ("value", json::str(&w.value))])).collect())
}

/// The law's banned words (whole word, case-insensitive): the panel and its docs say none of them
#[cfg(test)]
pub const BANNED: [&str; 10] = ["alive", "living", "feel", "feels", "feeling", "feelings", "soul", "conscious", "sentient", "suffer"];
/// Whole-word, case-insensitive hits of the banned list in `s`
#[cfg(test)]
pub fn law_hits(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| BANNED.contains(w)).map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lv(levels: &[&str], mood: bool) -> Field { Field::Level { levels: levels.iter().map(|s| s.to_string()).collect(), level: None, released: true, odds: vec![], mood } }
    /// One of each field kind, with the level sets of the personas the panel runs on (Pepe's, Hermes's, the examples')
    fn kinds() -> Vec<Field> {
        let mut v = vec![];
        for ls in [&["soft", "direct", "blunt"][..], &["calm", "steady", "keyed"], &["down", "even", "up"], &["inspect", "execute"], &["calm", "steady", "urgent"], &["follow", "suggest", "lead"]] {
            v.push(lv(ls, false)); v.push(lv(ls, true));
            v.push(Field::Learned { levels: ls.iter().map(|s| s.to_string()).collect(), deltas: vec![], cap: 0.25 });
        }
        v.extend([Field::Need { deficit: 0.0, cap: 1.0, term: 0.0 }, Field::Gate { g: 1.0, g_max: 3.0 }, Field::Pace { level: "steady".into(), rbar: 0.0, cap: 4.0 },
            Field::Credit { open: 0, max: 8 }, Field::Habits { active: 0, bound: 0, declared: 11 }, Field::Sense { z: None }, Field::Undeclared, Field::Room]);
        for g in ["optimism", "patience", "volatility"] { v.push(Field::Gene { gene: g.into(), x: 0.0 }); }
        v
    }

    /// Bar (a) faithful: per field, every level × size × direction gives its own line (no two states share a field's words)
    #[test]
    fn every_bucket_of_every_field_has_its_own_words() {
        let mut total = 0;
        for f in kinds() {
            let (ls, ss, ds) = space(&f); let mut seen = std::collections::BTreeMap::new();
            for l in &ls { for s in &ss { for d in &ds {
                let t = line("x", l, s, d);
                assert!(seen.insert(t.clone(), (l.clone(), *s, d.clone())).is_none(), "{f:?}: {t:?} twice");
                total += 1;
            } } }
        }
        eprintln!("panel faithful: {total} enumerated states, 0 ambiguous");
        assert!(total > 1000);
    }
    /// The map only ever lands inside the enumerated space (so the enumeration covers every line it can print)
    #[test]
    fn the_map_lands_in_the_enumerated_space() {
        let mut x: u64 = 7; let mut rnd = || { x ^= x << 13; x ^= x >> 7; x ^= x << 17; (x % 10_000) as f64 / 10_000.0 };
        for _ in 0..4000 {
            let levels: Vec<String> = ["soft", "direct", "blunt"].iter().map(|s| s.to_string()).collect();
            let mk = |r: &mut dyn FnMut() -> f64, mood: bool| { let o = [r(), r(), r()]; let t: f64 = o.iter().sum::<f64>().max(1e-9);
                let odds: Vec<f64> = o.iter().map(|v| v / t).collect(); let k = (r() * 3.0) as usize % 3;
                Field::Level { levels: levels.clone(), level: (r() > 0.05).then(|| levels[k].clone()), released: r() > 0.1, odds, mood } };
            let pairs = vec![(mk(&mut rnd, false), mk(&mut rnd, false)), (mk(&mut rnd, true), mk(&mut rnd, true)),
                (Field::Need { deficit: (rnd() * 1.2).min(1.0), cap: 1.0, term: rnd() - 0.5 }, Field::Need { deficit: rnd(), cap: 1.0, term: 0.1 }),
                (Field::Gate { g: 1.0 + (rnd() * 2.0 * (rnd() > 0.3) as u8 as f64), g_max: 3.0 }, Field::Gate { g: 1.0 + rnd(), g_max: 3.0 }),
                (Field::Pace { level: ["slow", "steady", "brisk"][(rnd() * 3.0) as usize % 3].into(), rbar: rnd() * 4.0, cap: 4.0 }, Field::Pace { level: "steady".into(), rbar: rnd() * 4.0, cap: 4.0 }),
                (Field::Learned { levels: levels.clone(), deltas: vec![rnd() * 0.5 - 0.25, rnd() * 0.01, rnd() * 0.5 - 0.25], cap: 0.25 }, Field::Learned { levels: levels.clone(), deltas: vec![rnd() * 0.5 - 0.25, 0.0, 0.0], cap: 0.25 }),
                (Field::Credit { open: (rnd() * 9.0) as usize % 9, max: 8 }, Field::Credit { open: (rnd() * 9.0) as usize % 9, max: 8 }),
                (Field::Habits { active: (rnd() * 12.0) as usize % 12, bound: 0, declared: 11 }, Field::Habits { active: 2, bound: 1, declared: 11 }),
                (Field::Sense { z: (rnd() > 0.3).then(|| rnd() * 8.0 - 4.0) }, Field::Sense { z: None }),
                (Field::Gene { gene: ["optimism", "patience", "volatility"][(rnd() * 3.0) as usize % 3].into(), x: rnd() * 2.0 - 1.0 }, Field::Undeclared)];
            for (a, b) in pairs {
                let (ra, rb) = (Reading { key: "k".into(), label: "x".into(), field: a.clone() }, Reading { key: "k".into(), label: "x".into(), field: b });
                for w in [word(&ra, Some(&rb)), word(&ra, None)] {
                    let (ls, ss, ds) = space(&a);
                    if let Field::Habits { active, bound, .. } = a { assert_eq!(w.level, format!("{active} in force and {bound} bound")); }
                    assert!(ls.contains(&w.level) && ss.contains(&w.size) && ds.contains(&w.dir), "{a:?} -> {w:?}");
                    assert_eq!(w.text, line("x", &w.level, w.size, &w.dir));
                    assert!(w.dial.map_or(true, |d| (0.0..=1.0).contains(&d)), "{w:?}");
                }
            }
        }
    }
    /// The law on every word the panel can print: the fixed vocabulary and every enumerated line
    #[test]
    fn panel_words_obey_the_law() {
        assert_eq!(law_hits("The Soul feels; alive-ish, sentient. living"), ["soul", "feels", "alive", "sentient", "living"]);
        for w in vocabulary() { assert!(law_hits(&w).is_empty(), "{w}"); }
        let mut n = 0;
        for f in kinds() { let (ls, ss, ds) = space(&f); for l in &ls { for s in &ss { for d in &ds { let t = line("x", l, s, d); assert!(law_hits(&t).is_empty(), "{t}"); n += 1; } } } }
        for label in ["needs", "surprise gate", "pace", "dispositions", "credit", "habits", "room left in the bound"] { assert!(law_hits(label).is_empty()); }
        assert!(n > 1000);
    }
    /// Words are a function of the buckets: a small move inside a bucket changes the dial, never the words
    #[test]
    fn a_move_inside_a_bucket_keeps_the_words() {
        let lv: Vec<String> = ["terse", "tight", "full"].iter().map(|s| s.to_string()).collect();
        let at = |o: [f64; 3]| Reading { key: "stance:v".into(), label: "verbosity".into(), field: Field::Level { levels: lv.clone(), level: Some("tight".into()), released: true, odds: o.to_vec(), mood: false } };
        let (a, b, c) = (at([0.30, 0.60, 0.10]), at([0.30, 0.61, 0.09]), at([0.20, 0.60, 0.20]));
        assert_eq!(word(&b, Some(&a)).text, "verbosity: tight, clearly, holding");
        assert_eq!(word(&c, Some(&a)).text, "verbosity: tight, clearly, leaning toward full");
        assert_eq!(word(&a, None).text, "verbosity: tight, clearly, first event");
        assert_eq!(word(&Reading { key: "room".into(), label: "room left in the bound".into(), field: Field::Room }, None).text, "room left in the bound: not computed (E1 pending)");
        assert_eq!(word(&Reading { key: "need:c".into(), label: "need context budget".into(), field: Field::Need { deficit: 0.55, cap: 1.0, term: 1.1 } }, Some(&Reading { key: "need:c".into(), label: "".into(), field: Field::Need { deficit: 0.4, cap: 1.0, term: 0.2 } })).text,
            "need context budget: pressing, clearly, rising");
    }
}
