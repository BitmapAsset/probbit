//! `probbit persona`: the individuality layer (docs/persona.md). A persona is a small document (YAML subset or JSON) of traits
//! with priors, moods with inertia, soft couplings, per-turn evidence, history features and habits (hard rules in the probbit-ir
//! vocabulary). Every turn, persona + the individual's state + the turn's inputs compile to ONE probbit-ir program, run in
//! process by `probbit run`'s own instruction (`--op decide` at fixed work); the answer decodes to the stance: a level per trait
//! with exact odds, the habits in force and the ones that changed the stance, a refusal when the engine cannot vouch, a one-line
//! why and a short stance line a host puts into any model's prompt. The model writes the text; the persona decides how.
//!
//! A turn is a pure function of (persona, state, inputs, engine version): every document is canonical JSON (keys sorted, the
//! canonical number rule `canon_num`) and the reference implementation (the R30 prototype, Python) gives the same documents
//! byte for byte on the examples (`tests/persona.rs`). The engine is a parameter (`Engine`): the CLI and `probbit mcp` run
//! `run::run` on this process's threads, the browser module without threads; the answer is the same at fixed work.
use crate::json::{self, at, ix, InErr, Json};
use probbit_core::rt::Instant;
use std::collections::HashMap;

const RULE_KEYS: [&str; 6] = ["caps", "implies", "tables", "precedes", "linear", "all_different"];
const RESERVED_LEVELS: [&str; 9] = ["true", "false", "null", "yes", "no", "on", "off", "y", "n"];
const RESERVED_INPUTS: [&str; 1] = ["elapsed_hours"];
/// The engine's chain limit (main.rs `MAX_CHAINS`): a persona asking for more is refused when it is read
const MAX_CHAINS: f64 = 100_000.0;

type R<T> = Result<T, InErr>;
/// A persona, state or input error: `{"error": {"code": "persona", "path", "message"}}`, exit 2
pub fn perr(path: &str, msg: impl Into<String>) -> InErr { InErr { code: "persona", path: path.to_string(), msg: msg.into() } }
fn need(c: bool, path: &str, msg: &str) -> R<()> { if c { Ok(()) } else { Err(perr(path, msg)) } }

// ------------------------------------------------------------------------------------------- canonical JSON + hashing
/// Python's `repr(float)`: the shortest digits that read back (an exact tie between two goes to the even one), plain for
/// decimal exponents -5 < e < 16 (`0.0001`, `123.25`), else `1e-05` / `1.5e+16`. Equal to Python on 300,015 test doubles.
pub fn py_repr(x: f64) -> String {
    let e = format!("{x:e}"); let n = e.split_once('e').map_or(0, |(m, _)| m.trim_start_matches('-').replace('.', "").len());
    // Rust breaks an exact tie between two shortest candidates upward; Python (dtoa mode 0) to even = the correctly rounded n digits
    let c = format!("{:.*e}", n.max(1) - 1, x); let e = if c.parse::<f64>().ok() == Some(x) { c } else { e };
    let (m, ex) = e.split_once('e').unwrap_or((&e, "0")); let neg = m.starts_with('-'); let d: String = m.trim_start_matches('-').replace('.', "");
    let exp: i32 = ex.parse().unwrap_or(0); let decpt = exp + 1; let mut s = String::from(if neg { "-" } else { "" });
    if decpt <= -4 || decpt > 16 { s.push_str(&d[..1]); if d.len() > 1 { s.push('.'); s.push_str(&d[1..]); } s.push_str(&format!("e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())); }
    else if decpt <= 0 { s.push_str("0."); s.push_str(&"0".repeat((-decpt) as usize)); s.push_str(&d); }
    else if decpt as usize >= d.len() { s.push_str(&d); s.push_str(&"0".repeat(decpt as usize - d.len())); s.push_str(".0"); }
    else { s.push_str(&d[..decpt as usize]); s.push('.'); s.push_str(&d[decpt as usize..]); }
    s
}
/// The canonical number (docs/persona.md §5.2): a whole number of magnitude <= 2^53 prints as an integer (`1`, `-2`, `0`),
/// any other number as Python's repr (`0.43543`, `1e-05`). So `1`, `1.0` and `1e0` are one number in every digest.
fn canon_num(x: f64, s: &mut String) {
    if !x.is_finite() { s.push_str("null"); } else if x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_992.0 { s.push_str(&format!("{}", x as i64)); } else { s.push_str(&py_repr(x)); }
}
/// Python's `json.dumps(..., ensure_ascii=False)` string escapes
fn esc(t: &str, s: &mut String) {
    s.push('"');
    for c in t.chars() { match c { '"' => s.push_str("\\\""), '\\' => s.push_str("\\\\"), '\n' => s.push_str("\\n"), '\r' => s.push_str("\\r"), '\t' => s.push_str("\\t"),
        '\u{8}' => s.push_str("\\b"), '\u{c}' => s.push_str("\\f"), c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)), c => s.push(c) } }
    s.push('"');
}
fn write(j: &Json, sort: bool, s: &mut String) {
    match j {
        Json::Null => s.push_str("null"), Json::Bool(b) => s.push_str(if *b { "true" } else { "false" }), Json::Num(x) => canon_num(*x, s), Json::Str(t) => esc(t, s),
        Json::Arr(v) => { s.push('['); for (k, x) in v.iter().enumerate() { if k > 0 { s.push(','); } write(x, sort, s); } s.push(']'); }
        Json::Obj(v) => { let mut o: Vec<&(String, Json)> = v.iter().collect(); if sort { o.sort_by(|a, b| a.0.cmp(&b.0)); }
            s.push('{'); for (k, (key, x)) in o.into_iter().enumerate() { if k > 0 { s.push(','); } esc(key, s); s.push(':'); write(x, sort, s); } s.push('}'); }
    }
}
/// Canonical JSON: keys sorted (by code point), no whitespace, UTF-8, canonical numbers. Every persona document is printed so.
pub fn canon(j: &Json) -> String { let mut s = String::new(); write(j, true, &mut s); s }
/// A program's text: insertion order (the compiler's fixed order), compact, canonical numbers. `engine.program` is its sha256.
pub fn text(j: &Json) -> String { let mut s = String::new(); write(j, false, &mut s); s }
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d,
        0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut m = data.to_vec(); let bits = (data.len() as u64).wrapping_mul(8); m.push(0x80); while m.len() % 64 != 56 { m.push(0); } m.extend_from_slice(&bits.to_be_bytes());
    for c in m.chunks(64) {
        let mut w = [0u32; 64]; for t in 0..16 { w[t] = u32::from_be_bytes([c[4 * t], c[4 * t + 1], c[4 * t + 2], c[4 * t + 3]]); }
        for t in 16..64 { let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3); let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1); }
        let mut v = h;
        for t in 0..64 { let (a, b, cc, d, e, f, g, hh) = (v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]);
            let t1 = hh.wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25)).wrapping_add((e & f) ^ (!e & g)).wrapping_add(K[t]).wrapping_add(w[t]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add((a & b) ^ (a & cc) ^ (b & cc));
            v = [t1.wrapping_add(t2), a, b, cc, d.wrapping_add(t1), e, f, g]; }
        for (x, y) in h.iter_mut().zip(v) { *x = x.wrapping_add(y); }
    }
    let mut out = [0u8; 32]; for (i, x) in h.iter().enumerate() { out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes()); } out
}
pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
/// "sha256:<hex>" of a text
pub fn digest_of(t: &str) -> String { format!("sha256:{}", hex(&sha256(t.as_bytes()))) }
/// "sha256:<hex>" of a document's canonical JSON
pub fn sha(j: &Json) -> String { digest_of(&canon(j)) }

/// Python's `round(x, n)` (correctly rounded, ties to even on the exact binary value: Rust's fixed-precision formatting rounds
/// the same way; equal on 300,015 test doubles)
fn round_to(x: f64, n: usize) -> f64 { format!("{x:.n$}").parse().unwrap_or(x) }
/// Weights and odds: rounded to 6 decimals, -0 -> 0
pub(crate) fn r6(x: f64) -> f64 { let v = round_to(x, 6); if v == 0.0 { 0.0 } else { v } }
/// A number as Python's `%s` prints it in a message (bounds are whole numbers or Python floats)
fn pyn(x: f64) -> String { let mut s = String::new(); canon_num(x, &mut s); s }
/// A standard normal number that is a pure function of its parts: Box-Muller on the first 16 bytes of
/// sha256("probbit-persona/1|" + parts joined by "|")
fn normal(parts: &[&str]) -> f64 {
    let h = sha256(format!("probbit-persona/1|{}", parts.join("|")).as_bytes());
    let u = |o: usize| ((u64::from_be_bytes(h[o..o + 8].try_into().unwrap()) >> 11) as f64 + 0.5) / 9_007_199_254_740_992.0;
    let (u1, u2) = (u(0), u(8)); (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}
/// The turn's engine seed: the first 4 bytes of sha256("probbit-persona/1|turn|<name>|<seed>|<turn>"), 31 bits
pub fn turn_seed(name: &str, seed: u64, turn: u64) -> u64 {
    let h = sha256(format!("probbit-persona/1|turn|{name}|{seed}|{turn}").as_bytes()); (u32::from_be_bytes([h[0], h[1], h[2], h[3]]) & 0x7FFF_FFFF) as u64
}
/// Token estimate without a tokenizer: the larger of chars / 4 and 4/3 tokens per word
pub fn est_tokens(s: &str) -> usize {
    if s.is_empty() { return 0; }
    let (c, w) = (s.chars().count() as f64, s.split(crate::yaml::space).filter(|w| !w.is_empty()).count() as f64);
    ((c / 4.0).ceil() as usize).max((w * 4.0 / 3.0).ceil() as usize)
}
fn centered(n: usize) -> Vec<f64> { if n == 1 { vec![0.0] } else { (0..n).map(|i| (2.0 * i as f64 / (n - 1) as f64) - 1.0).collect() } }

// --------------------------------------------------------------------------------------------------------- the persona
#[derive(Clone)]
struct Var { id: String, mood: bool, levels: Vec<String>, base: Vec<f64>, spread: f64, say: Vec<String>, fallback: String, vouch: f64, hold: Option<String>, inertia: f64, half_life: Option<f64> }
#[derive(Clone, Copy, PartialEq)]
enum Kind { Flag, Level, Number }
/// target (a trait / mood id or `step.<name>`) -> per-level (per-slot) log-weights, in the document's order
type Eff = Vec<(String, Vec<f64>)>;
#[derive(Clone)]
struct Input { id: String, kind: Kind, react: f64, say: Json, levels: Vec<String>, default: Json, min: f64, max: f64, effects: Eff, by_level: Vec<(String, Eff)> }
#[derive(Clone)]
struct Hist { id: String, of: String, streak: bool, window: usize, cap: usize, say: String, effects: Eff }
#[derive(Clone)]
struct Habit { id: String, when: Vec<(String, Json)>, then: Vec<(String, Json)>, rules: Vec<(String, Vec<Json>)>, say: String, priority: f64 }
/// engine settings (docs/persona.md §2.7)
#[derive(Clone)]
struct Eng { op: String, sweeps: usize, polish_sweeps: usize, exact_limit: u64, chains: usize, twin: bool, positional: bool, yields: bool }
/// bounded learning (docs/persona.md §2.8): `from` = the reward flag, then (optional) the correction flag; the traits whose learned
/// deltas move; the step and total caps (natural-log weights); `credit` = action-addressed feedback, when declared
#[derive(Clone)]
struct Learning { from: Vec<String>, traits: Vec<String>, rate: f64, step_cap: f64, total_cap: f64, credit: Option<Credit> }
/// `learning.credit` (§2.8): at most `max_open` decisions await an outcome; a payment's eligibility halves per `half_life_events`
/// of delay, and an open decision expires after 4 half-lives
#[derive(Clone)]
struct Credit { max_open: usize, half_life: f64 }
/// Learned deltas or credits: per learned trait, one number per level (in `learning.traits` order)
type Table = Vec<(String, Vec<f64>)>;
#[derive(Clone)]
pub struct Persona { pub name: String, pub version: String, pub seed: u64, pub digest: String, vars: Vec<Var>, steps: Vec<String>, slots: Vec<String>, say_order: bool,
    prefer: Eff, couplings: Vec<(String, String, Vec<Vec<f64>>)>, inputs: Vec<Input>, history: Vec<Hist>, habits: Vec<Habit>, eng: Eng, max_tokens: usize, order: Vec<String>, prefix: String,
    learning: Option<Learning>, drives: Option<Drives>, reward: Option<Vec<(String, Option<Vec<String>>)>>, senses: Option<Senses>, why_past: Option<bool> }
impl Persona {
    /// `line.why_past` (§2.11): `why` names what the learned weights came from; default on with a senses block, off without
    fn why_past(&self) -> bool { self.why_past.unwrap_or(self.senses.is_some()) }
    /// A senses block (§2.11)
    pub fn has_senses(&self) -> bool { self.senses.is_some() }
    /// A `learning.credit` block (§2.8)
    fn credit(&self) -> Option<&Credit> { self.learning.as_ref().and_then(|l| l.credit.as_ref()) }
    /// A `learning.credit` block (§2.8): `await`, `for` and `cancel` are read on every event
    pub fn has_credit(&self) -> bool { self.credit().is_some() }
    /// A `needs` block (RFC 0002 Design A): `needs` amounts are read on every event, each with a `satisfy_from` source
    pub fn has_needs(&self) -> bool { self.drives.as_ref().is_some_and(|d| d.needs.is_some()) }
    fn var(&self, id: &str) -> Option<&Var> { self.vars.iter().find(|v| v.id == id) }
    fn input(&self, id: &str) -> Option<&Input> { self.inputs.iter().find(|x| x.id == id) }
    fn habit(&self, id: &str) -> &Habit { self.habits.iter().find(|h| h.id == id).expect("habit id") }
    fn habit_index(&self, id: &str) -> usize { self.habits.iter().position(|h| h.id == id).unwrap_or(0) }
    /// `on_conflict: yield` (else fallback)
    pub fn yields(&self) -> bool { self.eng.yields }
}

fn get<'a>(kv: &'a [(String, Json)], k: &str) -> Option<&'a Json> { kv.iter().find(|(x, _)| x == k).map(|(_, v)| v) }
/// Python's truthiness: null, false, 0, "", [] and {} are false
fn truthy(j: &Json) -> bool { match j { Json::Null => false, Json::Bool(b) => *b, Json::Num(x) => *x != 0.0, Json::Str(s) => !s.is_empty(), Json::Arr(v) => !v.is_empty(), Json::Obj(v) => !v.is_empty() } }
/// `d.get(k) or default`: a false value counts as absent
fn get_t<'a>(kv: &'a [(String, Json)], k: &str) -> Option<&'a Json> { get(kv, k).filter(|v| truthy(v)) }
/// An object whose fields are all in `allowed` (each once), with the `required` ones present and not null
fn keys<'a>(o: &'a Json, path: &str, allowed: &[&str], required: &[&str]) -> R<&'a [(String, Json)]> {
    let kv = o.as_obj().ok_or_else(|| perr(path, "must be a mapping"))?;
    for (n, (k, _)) in kv.iter().enumerate() {
        if !allowed.contains(&k.as_str()) { let mut a = allowed.to_vec(); a.sort_unstable(); return Err(perr(&at(path, k), format!("unknown field (allowed: {})", a.join(", ")))); }
        if kv[..n].iter().any(|(k2, _)| k2 == k) { return Err(perr(&at(path, k), "duplicate field")); } }
    for r in required { if get(kv, r).map_or(true, Json::is_null) { return Err(perr(&at(path, r), "required")); } }
    Ok(kv)
}
fn num(x: Option<&Json>, path: &str, lo: Option<f64>, hi: Option<f64>, integer: bool) -> R<f64> {
    let Some(&Json::Num(v)) = x else { return Err(perr(path, "must be a finite number")) };
    if integer && v.fract() != 0.0 { return Err(perr(path, "must be a whole number")); }
    if let Some(lo) = lo { need(v >= lo, path, &format!("must be >= {}", pyn(lo)))?; }
    if let Some(hi) = hi { need(v <= hi, path, &format!("must be <= {}", pyn(hi)))?; }
    Ok(v)
}
fn num_or(kv: &[(String, Json)], k: &str, default: f64, path: &str, lo: Option<f64>, hi: Option<f64>, integer: bool) -> R<f64> {
    num(Some(get(kv, k).unwrap_or(&Json::Num(default))), &at(path, k), lo, hi, integer)
}
fn is_ident(s: &str) -> bool { let b = s.as_bytes(); !b.is_empty() && b.len() <= 48 && b[0].is_ascii_lowercase() && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_') }
fn ident(x: Option<&Json>, path: &str, what: &str) -> R<String> {
    match x { Some(Json::Str(s)) if is_ident(s) => Ok(s.clone()), _ => Err(perr(path, format!("{what} must match [a-z][a-z0-9_]* (<= 48 chars)"))) }
}
fn list<'a>(x: Option<&'a Json>, path: &str) -> R<&'a [Json]> { match x { None => Ok(&[]), Some(j) if !truthy(j) => Ok(&[]), Some(Json::Arr(a)) => Ok(a), Some(_) => Err(perr(path, "must be a list")) } }
fn strs(x: &Json) -> Option<Vec<String>> { x.as_arr()?.iter().map(|s| s.as_str().map(str::to_string)).collect() }

fn var_spec(v: &Json, path: &str, mood: bool) -> R<Var> {
    const T: [&str; 10] = ["id", "levels", "prior", "logw", "spread", "say", "fallback", "vouch", "hold", "comment"];
    let allowed: Vec<&str> = T.iter().copied().chain(if mood { vec!["inertia", "half_life_hours"] } else { vec![] }).collect();
    let kv = keys(v, path, &allowed, &["id", "levels"])?;
    let id = ident(get(kv, "id"), &at(path, "id"), "id")?;
    let lp = at(path, "levels");
    let lv = match get(kv, "levels") { Some(Json::Arr(a)) if (2..=7).contains(&a.len()) => a, _ => return Err(perr(&lp, "2 to 7 levels")) };
    let mut levels: Vec<String> = vec![];
    for (i, x) in lv.iter().enumerate() { let l = ident(Some(x), &ix(&lp, i), "level")?;
        need(!RESERVED_LEVELS.contains(&l.as_str()), &ix(&lp, i), "reserved word (YAML would not read it as a string)")?; levels.push(l); }
    need((1..levels.len()).all(|i| !levels[..i].contains(&levels[i])), &lp, "levels must be distinct")?;
    need(!(get(kv, "prior").is_some() && get(kv, "logw").is_some()), path, "give prior OR logw, not both")?;
    let n = levels.len();
    let base: Vec<f64> = if let Some(pr) = get(kv, "prior") {
        let pp = at(path, "prior"); let a = pr.as_arr().filter(|a| a.len() == n).ok_or_else(|| perr(&pp, "one probability per level"))?;
        let pr: Vec<f64> = a.iter().enumerate().map(|(i, x)| num(Some(x), &ix(&pp, i), Some(0.0), None, false)).collect::<R<_>>()?;
        let s = pr.iter().fold(0.0, |a, b| a + b); need(s > 0.0, &pp, "must not be all zero")?;
        pr.iter().map(|x| (x / s).max(1e-6).ln()).collect()
    } else if let Some(lw) = get(kv, "logw") {
        let lp = at(path, "logw"); let a = lw.as_arr().filter(|a| a.len() == n).ok_or_else(|| perr(&lp, "one log-weight per level"))?;
        a.iter().enumerate().map(|(i, x)| num(Some(x), &ix(&lp, i), Some(-50.0), Some(50.0), false)).collect::<R<_>>()?
    } else { vec![0.0; n] };
    let say = match get(kv, "say") { None => vec![String::new(); n], Some(s) => strs(s).filter(|s| s.len() == n).ok_or_else(|| perr(&at(path, "say"), "one string per level"))? };
    let fallback = match get(kv, "fallback") { None | Some(Json::Null) => levels[(0..n).fold(0, |b, i| if base[i] > base[b] { i } else { b })].clone(),
        Some(f) => f.as_str().filter(|f| levels.iter().any(|l| l == f)).ok_or_else(|| perr(&at(path, "fallback"), "must be one of the levels"))?.to_string() };
    let vouch = num_or(kv, "vouch", 0.0, path, Some(0.0), Some(1.0), false)?;
    let hold = match get(kv, "hold") { None | Some(Json::Null) => None, Some(h) => {
        let h = h.as_str().filter(|h| levels.iter().any(|l| l == h)).ok_or_else(|| perr(&at(path, "hold"), "must be one of the levels"))?;
        need(vouch > 0.0, &at(path, "hold"), "needs vouch > 0")?; Some(h.to_string()) } };
    let spread = num_or(kv, "spread", 0.0, path, Some(0.0), Some(5.0), false)?;
    let (mut inertia, mut half_life) = (0.0, None);
    if mood { inertia = num_or(kv, "inertia", 0.5, path, Some(0.0), Some(0.999), false)?;
        half_life = match get(kv, "half_life_hours") { None | Some(Json::Null) => None, x => Some(num(x, &at(path, "half_life_hours"), Some(0.001), None, false)?) }; }
    Ok(Var { id, mood, levels, base, spread, say, fallback, vouch, hold, inertia, half_life })
}
/// An effect on a target: a number (an ordinal shift: w x the centred level index), a list or a mapping of per-level log-weights
fn effect(e: &Json, path: &str, levels: &[String]) -> R<Vec<f64>> {
    let n = levels.len();
    match e {
        Json::Num(_) => { let w = num(Some(e), path, Some(-50.0), Some(50.0), false)?; Ok(centered(n).iter().map(|c| w * c).collect()) }
        Json::Arr(a) => { need(a.len() == n, path, &format!("one log-weight per level ({n})"))?; a.iter().enumerate().map(|(i, x)| num(Some(x), &ix(path, i), Some(-50.0), Some(50.0), false)).collect() }
        Json::Obj(kv) => { let mut out = vec![0.0; n];
            for (k, x) in kv { let i = levels.iter().position(|l| l == k).ok_or_else(|| perr(&at(path, k), format!("not a level of the target ({})", levels.join(", "))))?;
                out[i] = num(Some(x), &at(path, k), Some(-50.0), Some(50.0), false)?; }
            Ok(out) }
        _ => Err(perr(path, "an effect is a number (ordinal shift), a list or a mapping of per-level log-weights")),
    }
}

/// A persona file: `.json` = JSON, anything else the YAML subset. -> (persona, the document)
pub fn load(path: &str) -> R<(Persona, Json)> {
    let b = std::fs::read(path).map_err(|e| perr(path, format!("cannot read the persona file: {e}")))?;
    let t = String::from_utf8(b).map_err(|e| perr(path, format!("not UTF-8 (byte {})", e.utf8_error().valid_up_to())))?;
    let doc = parse_doc(&t, path.ends_with(".json")).map_err(|e| perr(path, e))?;
    Ok((build(&doc)?, doc))
}
/// Text -> document (JSON, or the YAML subset); Err = the reader's message
pub fn parse_doc(t: &str, is_json: bool) -> Result<Json, String> {
    let t = t.strip_prefix('\u{feff}').unwrap_or(t);
    if is_json { json::parse(t).map_err(|e| format!("not JSON: {}", e.msg)) } else { crate::yaml::load(t).map_err(|e| format!("line {}: {}", e.line, e.msg)) }
}

/// The strict validator: document -> persona (every field type-checked, unknown fields refused at their path).
pub fn build(doc: &Json) -> R<Persona> {
    const TOP: [&str; 17] = ["probbit_persona", "identity", "traits", "moods", "couplings", "inputs", "history", "habits", "agenda", "engine", "line", "learning", "drives", "reward_from", "senses", "needs", "comment"];
    let kv = keys(doc, "", &TOP, &["probbit_persona", "identity", "traits"])?;
    if get(kv, "senses").is_some() { return build_senses(doc); }
    // N-R1: a need's reduction pays a goal, so the block needs a drives block
    need(get(kv, "needs").is_none() || get(kv, "drives").is_some(), "needs", "a needs block needs a drives block (a need's win pays one of its goals)")?;
    if get(kv, "drives").is_some() { return build_drives(doc, &[]); }
    need(matches!(get(kv, "probbit_persona"), Some(Json::Num(x)) if *x == 1.0), "probbit_persona", "must be 1")?;
    let idn = keys(get(kv, "identity").unwrap(), "identity", &["name", "version", "seed", "summary"], &["name", "version"])?;
    let name = get(idn, "name").and_then(Json::as_str).filter(|n| { let b = n.as_bytes(); !b.is_empty() && b.len() <= 64 && b[0].is_ascii_alphanumeric() && b.iter().all(|c| c.is_ascii_alphanumeric() || b" _.-".contains(c)) })
        .ok_or_else(|| perr("identity.name", "1-64 chars: letters, digits, space, _ . -"))?.to_string();
    let version = get(idn, "version").and_then(Json::as_str).filter(|v| !v.is_empty()).ok_or_else(|| perr("identity.version", "a non-empty string"))?.to_string();
    let seed = num_or(idn, "seed", 0.0, "identity", Some(0.0), Some(9_007_199_254_740_992.0), true)? as u64;
    let digest = sha(doc);
    let traits = match get(kv, "traits") { Some(Json::Arr(a)) if !a.is_empty() => a, _ => return Err(perr("traits", "a non-empty list")) };
    let mut vars: Vec<Var> = vec![];
    for (i, t) in traits.iter().enumerate() { vars.push(var_spec(t, &ix("traits", i), false)?); }
    for (i, m) in list(get(kv, "moods"), "moods")?.iter().enumerate() { vars.push(var_spec(m, &ix("moods", i), true)?); }
    need((1..vars.len()).all(|i| vars[..i].iter().all(|v| v.id != vars[i].id)), "traits", "trait and mood ids must be distinct")?;
    let vlev = |id: &str| vars.iter().find(|v| v.id == id).map(|v| v.levels.clone());
    // agenda
    let mut steps: Vec<String> = vec![]; let mut say_order = false; let mut prefer_doc = None;
    if let Some(ag) = get(kv, "agenda").filter(|a| !a.is_null()) {
        let akv = keys(ag, "agenda", &["steps", "prefer", "say_order", "comment"], &["steps"])?;
        let st = get(akv, "steps").and_then(Json::as_arr).filter(|s| (2..=8).contains(&s.len())).ok_or_else(|| perr("agenda.steps", "2 to 8 steps"))?;
        for (i, s) in st.iter().enumerate() { steps.push(ident(Some(s), &ix("agenda.steps", i), "step")?); }
        need((1..steps.len()).all(|i| !steps[..i].contains(&steps[i])), "agenda.steps", "distinct steps")?;
        say_order = get(akv, "say_order").map_or(true, truthy); prefer_doc = get(akv, "prefer").filter(|p| !p.is_null());
    }
    let slots: Vec<String> = (0..steps.len()).map(|i| format!("s{i}")).collect();
    let effects_map = |e: &Json, path: &str| -> R<Eff> {
        let ekv = e.as_obj().ok_or_else(|| perr(path, "a mapping of target -> effect"))?; let mut out: Eff = vec![];
        for (k, x) in ekv { let p = at(path, k);
            let v = if let Some(s) = k.strip_prefix("step.") { need(steps.iter().any(|x| x == s), &p, &format!("unknown agenda step {k:?}"))?;
                if let Json::Num(_) = x { let w = num(Some(x), &p, Some(-50.0), Some(50.0), false)?; centered(slots.len()).iter().map(|c| -w * c).collect() } else { effect(x, &p, &slots)? } }
                else { let lv = vlev(k).ok_or_else(|| perr(&p, format!("unknown trait or mood {k:?}")))?; effect(x, &p, &lv)? };
            match out.iter_mut().find(|(t, _)| t == k) { Some(slot) => slot.1 = v, None => out.push((k.clone(), v)) } }
        Ok(out) };
    let prefer = match prefer_doc { None => vec![], Some(pd) => {
        let pkv = pd.as_obj().ok_or_else(|| perr("agenda.prefer", "a mapping of agenda step -> effect"))?;
        let mut m: Vec<(String, Json)> = vec![];
        for (k, x) in pkv { let k = if k.starts_with("step.") { k.clone() } else { format!("step.{k}") };
            match m.iter_mut().find(|(t, _)| *t == k) { Some(s) => s.1 = x.clone(), None => m.push((k, x.clone())) } }
        effects_map(&Json::Obj(m), "agenda.prefer")? } };
    // couplings
    let mut couplings = vec![];
    for (i, c) in list(get(kv, "couplings"), "couplings")?.iter().enumerate() { let path = ix("couplings", i);
        let ckv = keys(c, &path, &["vars", "align", "table", "comment"], &["vars"])?;
        let vs = strs(get(ckv, "vars").unwrap()).filter(|v| v.len() == 2 && v[0] != v[1]).ok_or_else(|| perr(&at(&path, "vars"), "two distinct ids"))?;
        let (la, lb) = match (vlev(&vs[0]), vlev(&vs[1])) { (Some(a), Some(b)) => (a, b), _ => return Err(perr(&at(&path, "vars"), "trait or mood ids")) };
        need(get(ckv, "align").is_some() != get(ckv, "table").is_some(), &path, "exactly one of align / table")?;
        let mut tab = vec![vec![0.0; lb.len()]; la.len()];
        if get(ckv, "align").is_some() { let w = num(get(ckv, "align"), &at(&path, "align"), Some(-50.0), Some(50.0), false)?; let (ca, cb) = (centered(la.len()), centered(lb.len()));
            for x in 0..la.len() { for y in 0..lb.len() { tab[x][y] = w * ca[x] * cb[y]; } } }
        else { let tp = at(&path, "table"); let tkv = get(ckv, "table").and_then(Json::as_obj).ok_or_else(|| perr(&tp, "level of a -> {level of b: weight}"))?;
            for (x, row) in tkv { let xi = la.iter().position(|l| l == x).ok_or_else(|| perr(&at(&tp, x), format!("not a level of {}", vs[0])))?;
                let rkv = row.as_obj().ok_or_else(|| perr(&at(&tp, x), "a mapping level of b -> weight"))?;
                for (y, w) in rkv { let yp = at(&at(&tp, x), y); let yi = lb.iter().position(|l| l == y).ok_or_else(|| perr(&yp, format!("not a level of {}", vs[1])))?;
                    tab[xi][yi] = num(Some(w), &yp, Some(-50.0), Some(50.0), false)?; } } }
        couplings.push((vs[0].clone(), vs[1].clone(), tab)); }
    // inputs
    let mut inputs: Vec<Input> = vec![];
    for (i, x) in list(get(kv, "inputs"), "inputs")?.iter().enumerate() { let path = ix("inputs", i);
        let xkv = keys(x, &path, &["id", "kind", "levels", "default", "effects", "reactivity_spread", "max", "say", "comment"], &["id", "kind"])?;
        let id = ident(get(xkv, "id"), &at(&path, "id"), "id")?;
        need(!RESERVED_INPUTS.contains(&id.as_str()), &at(&path, "id"), "reserved input name")?;
        need(!inputs.iter().any(|y| y.id == id), &at(&path, "id"), "input ids must be distinct")?;
        let kind = match get(xkv, "kind").and_then(Json::as_str) { Some("flag") => Kind::Flag, Some("level") => Kind::Level, Some("number") => Kind::Number, _ => return Err(perr(&at(&path, "kind"), "flag | level | number")) };
        let react = num_or(xkv, "reactivity_spread", 0.0, &path, Some(0.0), Some(3.0), false)?;
        let say = get(xkv, "say").cloned().unwrap_or_else(|| Json::Str(id.replace('_', " ")));
        need(match &say { Json::Str(_) => true, Json::Obj(v) => v.iter().all(|(_, s)| s.as_str().is_some()), _ => false }, &at(&path, "say"), "a string (or for a level input, a mapping level -> string)")?;
        let mut inp = Input { id, kind, react, say, levels: vec![], default: Json::Null, min: 0.0, max: 1.0, effects: vec![], by_level: vec![] };
        match kind {
            Kind::Level => { let lp = at(&path, "levels");
                let lv = get(xkv, "levels").and_then(Json::as_arr).filter(|l| l.len() >= 2).ok_or_else(|| perr(&lp, "a level input needs 2+ levels"))?;
                for (j, l) in lv.iter().enumerate() { let l = ident(Some(l), &ix(&lp, j), "level")?; need(!RESERVED_LEVELS.contains(&l.as_str()), &ix(&lp, j), "reserved word")?; inp.levels.push(l); }
                inp.default = get(xkv, "default").cloned().unwrap_or_else(|| Json::Str(inp.levels[0].clone()));
                need(inp.default.as_str().is_some_and(|d| inp.levels.iter().any(|l| l == d)), &at(&path, "default"), "one of the levels")?;
                if let Some(eff) = get_t(xkv, "effects") { let ep = at(&path, "effects");
                    for (l, e) in eff.as_obj().ok_or_else(|| perr(&ep, "level -> {target: effect}"))? {
                        need(inp.levels.contains(l), &at(&ep, l), "not a level of this input")?; inp.by_level.push((l.clone(), effects_map(e, &at(&ep, l))?)); } } }
            Kind::Flag => { inp.default = Json::Bool(get(xkv, "default").is_some_and(truthy)); if let Some(e) = get_t(xkv, "effects") { inp.effects = effects_map(e, &at(&path, "effects"))?; } }
            Kind::Number => { inp.max = num_or(xkv, "max", 1.0, &path, Some(1e-9), None, false)?;
                inp.default = Json::Num(num_or(xkv, "default", 0.0, &path, Some(0.0), Some(inp.max), false)?);
                if let Some(e) = get_t(xkv, "effects") { inp.effects = effects_map(e, &at(&path, "effects"))?; } }
        }
        inputs.push(inp); }
    // history
    let mut history: Vec<Hist> = vec![];
    for (i, h) in list(get(kv, "history"), "history")?.iter().enumerate() { let path = ix("history", i);
        let hkv = keys(h, &path, &["id", "of", "kind", "window", "cap", "effects", "say", "comment"], &["id", "of", "kind"])?;
        let id = ident(get(hkv, "id"), &at(&path, "id"), "id")?;
        need(!inputs.iter().any(|x| x.id == id), &at(&path, "id"), "clashes with an input id")?;
        need(!history.iter().any(|x| x.id == id), &at(&path, "id"), "history ids must be distinct")?;
        let of = get(hkv, "of").and_then(Json::as_str).filter(|o| inputs.iter().any(|x| x.id == *o && x.kind == Kind::Flag)).ok_or_else(|| perr(&at(&path, "of"), "a flag input"))?.to_string();
        let streak = match get(hkv, "kind").and_then(Json::as_str) { Some("streak") => true, Some("recent") => false, _ => return Err(perr(&at(&path, "kind"), "streak | recent")) };
        let window = num_or(hkv, "window", 5.0, &path, Some(1.0), Some(100.0), true)? as usize; let cap = num_or(hkv, "cap", 5.0, &path, Some(1.0), Some(100.0), true)? as usize;
        let say = match get(hkv, "say") { None => id.replace('_', " "), Some(Json::Str(s)) => s.clone(), Some(_) => return Err(perr(&at(&path, "say"), "a string")) };
        let effects = match get_t(hkv, "effects") { Some(e) => effects_map(e, &at(&path, "effects"))?, None => vec![] };
        history.push(Hist { id, of, streak, window, cap, say, effects }); }
    // habits
    let mut habits: Vec<Habit> = vec![];
    for (i, hb) in list(get(kv, "habits"), "habits")?.iter().enumerate() { habits.push(habit_spec(hb, &ix("habits", i), &inputs, &history, &vars, &steps)?); }
    need((1..habits.len()).all(|i| habits[..i].iter().all(|h| h.id != habits[i].id)), "habits", "habit ids must be distinct")?;
    // engine
    let mut eng = Eng { op: "decide".into(), sweeps: 2000, polish_sweeps: 200, exact_limit: 2_000_000, chains: 4, twin: true, positional: true, yields: false };
    if let Some(e) = get_t(kv, "engine") { let ekv = keys(e, "engine", &["op", "sweeps", "polish_sweeps", "exact_limit", "chains", "twin", "values", "on_conflict", "comment"], &[])?;
        for (k, x) in ekv { let p = at("engine", k);
            match k.as_str() {
                "op" => eng.op = x.as_str().filter(|o| ["decide", "sample"].contains(o)).ok_or_else(|| perr(&p, "decide | sample"))?.to_string(),
                "twin" => eng.twin = if let Json::Bool(b) = x { *b } else { return Err(perr(&p, "true | false")) },
                "values" => eng.positional = match x.as_str() { Some("positional") => true, Some("semantic") => false, _ => return Err(perr(&p, "positional | semantic")) },
                "on_conflict" => eng.yields = match x.as_str() { Some("fallback") => false, Some("yield") => true, _ => return Err(perr(&p, "fallback | yield")) },
                "comment" => {}
                "polish_sweeps" => eng.polish_sweeps = num(Some(x), &p, Some(0.0), Some(1e9), true)? as usize,
                "sweeps" => eng.sweeps = num(Some(x), &p, Some(1.0), Some(1e9), true)? as usize,
                "exact_limit" => eng.exact_limit = num(Some(x), &p, Some(1.0), Some(1e9), true)? as u64,
                _ => eng.chains = num(Some(x), &p, Some(1.0), Some(MAX_CHAINS), true)? as usize,
            } } }
    // line
    let lkv: &[(String, Json)] = match get_t(kv, "line") { Some(l) => keys(l, "line", &["max_tokens", "order", "prefix", "why_past", "comment"], &[])?, None => &[] };
    let why_past = match get(lkv, "why_past") { None | Some(Json::Null) => None, Some(Json::Bool(b)) => Some(*b), Some(_) => return Err(perr("line.why_past", "true | false")) };
    let order = match get_t(lkv, "order") { None => vars.iter().filter(|v| !v.mood).map(|v| v.id.clone()).collect(), Some(o) => strs(o).ok_or_else(|| perr("line.order", "a list of trait ids"))? };
    for k in &order { need(vars.iter().any(|v| v.id == *k), "line.order", &format!("unknown trait or mood {k:?}"))?; }
    let max_tokens = num_or(lkv, "max_tokens", 40.0, "line", Some(8.0), Some(400.0), true)? as usize;
    let prefix = match get(lkv, "prefix") { None => "Stance: ".to_string(), Some(Json::Str(s)) => s.clone(), Some(_) => return Err(perr("line.prefix", "a string")) };
    let learning = match get(kv, "learning").filter(|l| !l.is_null()) { None => None, Some(l) => Some(learning_spec(l, &vars, &inputs)?) };
    let reward = match get(kv, "reward_from").filter(|x| !x.is_null()) { None => None, Some(rf) => Some(reward_spec(rf, &learning.as_ref().map_or(vec![], |l| l.from.clone()), &inputs)?) };
    let p = Persona { name, version, seed, digest, vars, steps, slots, say_order, prefer, couplings, inputs, history, habits, eng, max_tokens, order, prefix, learning, drives: None, reward, senses: None, why_past };
    // the fallback stance must obey every unconditional habit (checked once, here)
    let mut fb: HashMap<String, String> = p.vars.iter().map(|v| (v.id.clone(), v.fallback.clone())).collect();
    for (i, s) in p.steps.iter().enumerate() { fb.insert(format!("step.{s}"), p.slots[i].clone()); }
    let (mut rules, mut owners) = (empty_rules(), vec![]);
    for h in p.habits.iter().filter(|h| h.when.is_empty()) { habit_rules(&p, h, &fb, &mut rules, &mut owners); }
    let owners = ordered_owners(owners); let bad = violations(&fb, &rules, &values_of(&p));
    if !bad.is_empty() { let mut ids: Vec<&str> = bad.iter().map(|&i| owners[i].as_str()).collect(); ids.sort_unstable(); ids.dedup();
        return Err(perr("traits", format!("the fallback stance breaks unconditional habit(s): {}", ids.join(", ")))); }
    Ok(p)
}

/// One habit, checked against the persona's inputs, history features, traits, moods and agenda steps. A character property
/// (`persona fuzz` / `prove`, §5.6) has the same shape and is read by the same function.
fn habit_spec(hb: &Json, path: &str, inputs: &[Input], history: &[Hist], vars: &[Var], steps: &[String]) -> R<Habit> {
    let hkv = keys(hb, path, &["id", "when", "then", "rules", "say", "priority", "comment"], &["id"])?;
    let id = ident(get(hkv, "id"), &at(path, "id"), "id")?;
    need(get(hkv, "then").is_some() || get(hkv, "rules").is_some(), path, "a habit needs then and/or rules")?;
    let wp = at(path, "when"); let when: Vec<(String, Json)> = match get_t(hkv, "when") { None => vec![], Some(w) => w.as_obj().ok_or_else(|| perr(&wp, "a mapping input -> condition"))?.to_vec() };
    for (k, c) in &when { check_cond(k, c, &at(&wp, k), inputs, history)?; }
    let tp = at(path, "then"); let then: Vec<(String, Json)> = match get_t(hkv, "then") { None => vec![], Some(t) => t.as_obj().ok_or_else(|| perr(&tp, "a mapping trait -> allowed levels"))?.to_vec() };
    for (k, r) in &then { let lv = vars.iter().find(|v| v.id == *k).ok_or_else(|| perr(&at(&tp, k), "unknown trait or mood"))?; check_restr(r, &at(&tp, k), &lv.levels)?; }
    let rp = at(path, "rules"); let mut rules: Vec<(String, Vec<Json>)> = vec![];
    if let Some(r) = get_t(hkv, "rules") { for (k, lst) in r.as_obj().ok_or_else(|| perr(&rp, "a mapping of IR rule lists"))? {
        need(RULE_KEYS.contains(&k.as_str()), &at(&rp, k), &format!("one of {}", RULE_KEYS.join(", ")))?;
        let l = lst.as_arr().filter(|l| !l.is_empty()).ok_or_else(|| perr(&at(&rp, k), "a non-empty list"))?;
        for (j, r) in l.iter().enumerate() { check_rule(k, r, &ix(&at(&rp, k), j), vars, steps)?; }
        rules.push((k.clone(), l.to_vec())); } }
    let say = match get(hkv, "say") { None => String::new(), Some(Json::Str(s)) => s.clone(), Some(_) => return Err(perr(&at(path, "say"), "a string")) };
    let priority = num_or(hkv, "priority", 0.0, path, Some(-100.0), Some(100.0), true)?;
    Ok(Habit { id, when, then, rules, say, priority })
}
/// The `learning` block (docs/persona.md §2.8): `from` = 1-2 distinct flag inputs (the reward, then the correction), `traits` =
/// distinct trait ids (moods do not learn), `rate`, `step_cap` and `total_cap` positive, with step_cap <= total_cap <= 50; `credit`
/// (optional): `max_open` a whole number 1-64 (default 8), `half_life_events` 1-10000 (default 32)
fn learning_spec(l: &Json, vars: &[Var], inputs: &[Input]) -> R<Learning> {
    let kv = keys(l, "learning", &["from", "traits", "rate", "step_cap", "total_cap", "credit", "comment"], &["from", "traits", "rate", "step_cap", "total_cap"])?;
    let ids = |k: &str, max: usize, ok: &dyn Fn(&str) -> bool, what: &str| -> R<Vec<String>> { let p = at("learning", k);
        let v = strs(get(kv, k).unwrap()).filter(|v| !v.is_empty() && v.len() <= max).ok_or_else(|| perr(&p, format!("a list of 1 to {max} ids")))?;
        for (i, x) in v.iter().enumerate() { need(ok(x), &ix(&p, i), what)?; need(!v[..i].contains(x), &ix(&p, i), "listed twice")?; }
        Ok(v) };
    let from = ids("from", 2, &|x| inputs.iter().any(|i| i.id == x && i.kind == Kind::Flag), "not a flag input of this persona")?;
    let traits = ids("traits", vars.len(), &|x| vars.iter().any(|v| v.id == x && !v.mood), "not a trait of this persona (moods do not learn)")?;
    let pos = |k: &str, hi: f64| -> R<f64> { let p = at("learning", k); let x = num(get(kv, k), &p, Some(0.0), Some(hi), false)?; need(x > 0.0, &p, "must be > 0")?; Ok(x) };
    let (rate, total_cap) = (pos("rate", 50.0)?, pos("total_cap", 50.0)?); let step_cap = pos("step_cap", total_cap)?;
    let credit = match get(kv, "credit") { None | Some(Json::Null) => None, Some(c) => { let ck = keys(c, "learning.credit", &["max_open", "half_life_events", "comment"], &[])?;
        Some(Credit { max_open: num_or(ck, "max_open", 8.0, "learning.credit", Some(1.0), Some(64.0), true)? as usize,
            half_life: num_or(ck, "half_life_events", 32.0, "learning.credit", Some(1.0), Some(10000.0), false)? }) } };
    Ok(Learning { from, traits, rate, step_cap, total_cap, credit })
}
/// `reward_from` (docs/persona.md §2.10): the sources a reward-bearing input may come from -> per reward-bearing input (`bearing`:
/// the learning block's flags, then `goals.<id>.win` per goal of a drives block) its allowed source kinds (None = any). A list of
/// kinds (`human`, `env`) or `any` covers every reward-bearing input; a mapping names each one with its own list or `any`.
fn reward_spec(rf: &Json, bearing: &[String], inputs: &[Input]) -> R<Vec<(String, Option<Vec<String>>)>> {
    need(!bearing.is_empty(), "reward_from", "this persona has no reward-bearing input (a learning block's flags or a drives block's wins)")?;
    need(!inputs.iter().any(|x| x.id == "src"), "reward_from", "`src` names an event's source here: no input may have that id")?;
    let kinds = |x: &Json, path: &str| -> R<Option<Vec<String>>> {
        if x.as_str() == Some("any") { return Ok(None); }
        let v = strs(x).filter(|v| !v.is_empty()).ok_or_else(|| perr(path, "a list of source kinds (human, env) or any"))?;
        for (i, k) in v.iter().enumerate() {
            need(k != "self", &ix(path, i), "a reward never comes from the individual itself")?;
            need(k == "human" || k == "env", &ix(path, i), "human | env (the clock issues no rewards)")?;
            need(!v[..i].contains(k), &ix(path, i), "listed twice")?; }
        Ok(Some(v)) };
    if rf.as_obj().is_none() { let k = kinds(rf, "reward_from")?; return Ok(bearing.iter().map(|b| (b.clone(), k.clone())).collect()); }
    let allowed: Vec<&str> = bearing.iter().map(String::as_str).chain(["comment"]).collect(); let required: Vec<&str> = bearing.iter().map(String::as_str).collect();
    let kv = keys(rf, "reward_from", &allowed, &required)?;
    bearing.iter().map(|b| Ok((b.clone(), kinds(get(kv, b).unwrap(), &at("reward_from", b))?))).collect()
}
/// An event's `src` -> its kind: `human[:id]`, `env[:id]` (id: 1-64 of A-Z a-z 0-9 _ . - @ / :), `self` or `clock`; None = malformed
pub fn src_kind(s: &str) -> Option<&'static str> {
    let (k, id) = match s.split_once(':') { Some((k, id)) => (k, Some(id)), None => (s, None) };
    let ok = |id: &str| (1..=64).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.-@/:".contains(&c));
    match (k, id) { ("human", i) if i.map_or(true, ok) => Some("human"), ("env", i) if i.map_or(true, ok) => Some("env"), ("self", None) => Some("self"), ("clock", None) => Some("clock"), _ => None }
}
/// Reward provenance (G1, §2.10): with `reward_from`, a reward-bearing input that is on (a learning flag true, a goal's win > 0)
/// needs the event's `src` to be one of its allowed kinds; `self`, a missing or an undeclared source refuses the event whole
fn provenance(rw: &[(String, Option<Vec<String>>)], raw: &[(String, Json)]) -> R<()> {
    const FORM: &str = "a source: human[:id] | env[:id] | self | clock (id: 1-64 of A-Z a-z 0-9 _ . - @ / :)";
    let kind = match get(raw, "src") { None => None, Some(Json::Str(s)) => Some(src_kind(s).ok_or_else(|| perr("inputs.src", FORM))?), Some(_) => return Err(perr("inputs.src", FORM)) };
    for (k, allowed) in rw {
        let on = match k.strip_prefix("goals.").and_then(|r| r.strip_suffix(".win")) {
            Some(g) => get(raw, "goals").and_then(|x| x.get(g)).and_then(|x| x.get("win")).and_then(Json::as_f64).is_some_and(|m| m > 0.0),
            None => matches!(get(raw, k), Some(Json::Bool(true))) };
        let Some(allowed) = allowed.as_ref().filter(|_| on) else { continue };
        match kind {
            None => return Err(perr("inputs.src", format!("{k} is a reward: the event needs a src ({} allowed by reward_from)", allowed.join(" or ")))),
            Some("self") => return Err(perr("inputs.src", format!("{k} from the individual itself (src self) is refused: a reward comes from {}", allowed.join(" or ")))),
            Some(kd) if !allowed.iter().any(|a| a == kd) => return Err(perr("inputs.src", format!("{k} from {kd} is refused: reward_from allows {}", allowed.join(" or ")))),
            _ => {} } }
    Ok(())
}
/// Validate the source of a generated fixture without changing it or running inference.
/// Normal turns use this same check; callers cannot use this to authorize production evidence.
pub fn check_provenance(p: &Persona, raw: &Json) -> R<()> {
    let raw = raw.as_obj().ok_or_else(|| perr("inputs", "must be an object"))?;
    if let Some(rw) = &p.reward { provenance(rw, raw)?; }
    // readings (§2.11, S2): each one's source, its own or the event's, of a kind its sense reads from
    if let Some(sn) = &p.senses { src_form(raw)?; sense_step(sn, &SState::zero(&p.name, sn, p.seed), raw, get(raw, "src"), true)?; }
    // a cancel (§2.8, B-R5) comes from env or a person
    if p.credit().is_some() && get(raw, "cancel").is_some() { src_form(raw)?; cancel_src(get(raw, "src"))?; }
    // a need's amount (RFC 0002 N-E2): its source, its own or the event's, of a kind in its satisfy_from
    if let Some((d, nb)) = p.drives.as_ref().and_then(|d| d.needs.as_ref().map(|nb| (d, nb))) { src_form(raw)?;
        let nd = NState { genes: Json::Null, deficit: vec![0.0; nb.items.len()], success: vec![nb.init; nb.items.len()] }; needs_step(d, nb, &nd, &[], raw, get(raw, "src"), true)?; }
    Ok(())
}
/// A persona whose events need a source: rewards (`reward_from`), readings (a senses block), cancels (a credit block) or needs
/// amounts (a needs block)
pub fn has_reward_sources(p: &Persona) -> bool { p.reward.is_some() || p.senses.is_some() || p.credit().is_some() || p.has_needs() }
fn check_cond(k: &str, c: &Json, path: &str, inputs: &[Input], history: &[Hist]) -> R<()> {
    let range = |c: &Json| -> R<()> { if let Json::Num(_) = c { return Ok(()); }
        let kv = c.as_obj().filter(|kv| !kv.is_empty() && kv.iter().all(|(k, _)| k == "at_least" || k == "at_most")).ok_or_else(|| perr(path, "a number (at least) or {at_least, at_most}"))?;
        for (k, x) in kv { num(Some(x), &at(path, k), None, None, false)?; } Ok(()) };
    if let Some(x) = inputs.iter().find(|x| x.id == k) { return match x.kind {
        Kind::Flag => need(matches!(c, Json::Bool(_)), path, "true | false"),
        Kind::Level => { let l: Option<Vec<String>> = match c { Json::Str(s) => Some(vec![s.clone()]), _ => strs(c) };
            need(l.is_some_and(|l| !l.is_empty() && l.iter().all(|v| x.levels.contains(v))), path, &format!("a level or list of levels of {k}")) }
        Kind::Number => range(c) }; }
    if history.iter().any(|h| h.id == k) { return range(c); }
    Err(perr(path, "unknown input or history feature"))
}
fn check_restr(r: &Json, path: &str, levels: &[String]) -> R<()> {
    let all_levels = |x: &Json| strs(x).is_some_and(|v| !v.is_empty() && v.iter().all(|l| levels.contains(l)));
    if let Json::Arr(_) = r { return need(all_levels(r), path, "a non-empty list of levels"); }
    let kv = r.as_obj().filter(|kv| kv.len() == 1).ok_or_else(|| perr(path, "a list of levels, or one of {not: [...]}, {at_most: L}, {at_least: L}"))?;
    let (k, x) = (&kv[0].0, &kv[0].1);
    match k.as_str() {
        "not" => need(all_levels(x) && { let mut v = strs(x).unwrap(); v.sort(); v.dedup(); v.len() < levels.len() }, &at(path, "not"), "levels, not all of them"),
        "at_most" | "at_least" => need(x.as_str().is_some_and(|l| levels.iter().any(|v| v == l) || ["prev", "prev-1", "prev+1"].contains(&l)), &at(path, k), "a level, prev, prev-1 or prev+1"),
        _ => Err(perr(path, format!("unknown restriction {k:?}"))),
    }
}
/// A habit's raw IR rule, checked as strictly as `probbit run` will read it (so a bad rule fails when the persona is read, not
/// on the turns it is in force)
fn check_rule(k: &str, r: &Json, path: &str, vars: &[Var], steps: &[String]) -> R<()> {
    let var = |x: Option<&Json>, p: &str| -> R<String> { match x.and_then(Json::as_str) {
        Some(s) if vars.iter().any(|v| v.id == s) || s.strip_prefix("step.").is_some_and(|t| steps.iter().any(|x| x == t)) => Ok(s.to_string()),
        _ => Err(perr(p, format!("unknown trait, mood or step {}", x.map_or("null".into(), canon)))) } };
    let lev = |x: &str, val: Option<&Json>, p: &str| -> R<()> {
        let ok = match (vars.iter().find(|v| v.id == x), val.and_then(Json::as_str)) { (Some(v), Some(l)) => v.levels.iter().any(|y| y == l),
            (None, Some(l)) => (0..steps.len()).any(|i| l == format!("s{i}")), _ => false };
        need(ok, p, &format!("{} is not a value of {x}", val.map_or("null".into(), canon))) };
    let terms = |x: Option<&Json>, p: &str| -> R<Vec<Json>> { x.and_then(Json::as_arr).map(<[Json]>::to_vec).ok_or_else(|| perr(p, "must be a list")) };
    match k {
        "caps" => { let kv = keys(r, path, &["limit", "members"], &["members"]).map_err(|e| if e.msg == "required" { perr(path, "persona caps use members: [[var, level], ...]") } else { e })?;
            num(Some(get(kv, "limit").unwrap_or(&Json::Num(0.0))), &at(path, "limit"), Some(0.0), Some(1000.0), true)?;
            for (j, m) in terms(get(kv, "members"), &at(path, "members"))?.iter().enumerate() { let mp = ix(&at(path, "members"), j);
                let m = m.as_arr().filter(|m| m.len() == 2).ok_or_else(|| perr(&mp, "[var, level]"))?; lev(&var(Some(&m[0]), &mp)?, Some(&m[1]), &mp)?; } }
        "implies" => { let kv = keys(r, path, &["if", "then"], &["if", "then"])?;
            let i = keys(get(kv, "if").unwrap(), &at(path, "if"), &["var", "value"], &["var", "value"])?; let t = keys(get(kv, "then").unwrap(), &at(path, "then"), &["var", "in"], &["var", "in"])?;
            let a = var(get(i, "var"), &at(path, "if.var"))?; lev(&a, get(i, "value"), &at(path, "if.value"))?;
            let b = var(get(t, "var"), &at(path, "then.var"))?; need(a != b, &at(path, "then.var"), "an implication needs two different variables")?;
            for x in terms(get(t, "in"), &at(path, "then.in"))? { lev(&b, Some(&x), &at(path, "then.in"))?; } }
        "tables" => { let kv = keys(r, path, &["vars", "forbid", "allow"], &["vars"])?;
            let vs: Vec<String> = terms(get(kv, "vars"), &at(path, "vars"))?.iter().map(|x| var(Some(x), &at(path, "vars"))).collect::<R<_>>()?;
            need((1..=3).contains(&vs.len()) && get(kv, "forbid").is_some() != get(kv, "allow").is_some(), path, "1-3 vars and exactly one of forbid / allow")?;
            need((1..vs.len()).all(|i| !vs[..i].contains(&vs[i])), &at(path, "vars"), "distinct vars")?;
            let f = if get(kv, "forbid").is_some() { "forbid" } else { "allow" };
            for tup in terms(get(kv, f), &at(path, f))? { let t = tup.as_arr().filter(|t| t.len() == vs.len()).ok_or_else(|| perr(path, "tuples with one level per var"))?;
                for (x, val) in vs.iter().zip(t) { lev(x, Some(val), path)?; } } }
        "precedes" => { let kv = keys(r, path, &["before", "after", "gap"], &["before", "after"])?;
            for x in [get(kv, "before"), get(kv, "after")] { need(x.and_then(Json::as_str).and_then(|s| s.strip_prefix("step.")).is_some_and(|s| steps.iter().any(|t| t == s)), path, "precedes orders agenda steps (step.<name>)")?; }
            need(get(kv, "before") != get(kv, "after"), path, "a precedence needs two different steps")?;
            if let Some(g) = get(kv, "gap").filter(|g| !g.is_null()) { num(Some(g), &at(path, "gap"), Some(0.0), Some(100.0), true)?; } }
        "linear" => { let kv = keys(r, path, &["terms", "limit"], &["terms", "limit"])?;
            for (j, t) in terms(get(kv, "terms"), &at(path, "terms"))?.iter().enumerate() { let tp = ix(&at(path, "terms"), j);
                let t = t.as_arr().filter(|t| t.len() == 3).ok_or_else(|| perr(&tp, "[var, level, weight]"))?;
                lev(&var(Some(&t[0]), &tp)?, Some(&t[1]), &tp)?; num(Some(&t[2]), &ix(&tp, 2), Some(0.0), Some(1_000_000.0), true)?; }
            num(get(kv, "limit"), &at(path, "limit"), Some(0.0), Some(9_007_199_254_740_992.0), true)?; }
        _ => { let kv = keys(r, path, &["vars"], &["vars"])?; let vs: Vec<String> = terms(get(kv, "vars"), &at(path, "vars"))?.iter().map(|x| var(Some(x), &at(path, "vars"))).collect::<R<_>>()?;
            need(vs.len() >= 2 && (1..vs.len()).all(|i| !vs[..i].contains(&vs[i])), &at(path, "vars"), "at least two distinct variables")?; }
    }
    Ok(())
}

// ----------------------------------------------------------------------------------------------------- drives (§2.9)
const GOAL_SIGNALS: [&str; 6] = ["progress", "novelty", "cue", "setback", "win", "deadline_hours"];
const COND_SIGNALS: [&str; 5] = ["deadline_hours", "want", "glow", "expect", "since_pursued"];
/// A goal of the `drives` block: one level of the `pursue` variable
#[derive(Clone)]
struct Goal { id: String, say: String, interest_spread: f64, reactivity_spread: f64, floor: f64, starve_after: Option<f64>, expect_by: Option<f64> }
/// A goal condition of a habit (`goal.<id>.<signal>` or `since_pursued.<id>`), lowered to a flag input the turn sets: the goal's
/// value of the signal within [lo, hi]
#[derive(Clone)]
struct GoalCond { id: String, goal: usize, sig: String, lo: f64, hi: f64 }
/// The `drives` block (docs/persona.md §2.9). It lowers to 0.7.0 constructs when the persona is read: one more trait `pursue` (levels
/// = the goal ids, prior = the interests), number inputs for the aggregates (`drv_want`, `drv_glow`, `drv_surprise`, `drv_letdown`),
/// one flag input per goal condition (`drv_c<N>`) and a habit `starve_<goal>` per `starve_after`. The drive itself is state.
#[derive(Clone)]
struct Drives { goals: Vec<Goal>, hl_w: f64, cap_w: f64, drain: f64, sig: [f64; 4], hl_a: f64, cap_a: f64, rate: f64, hl_e: f64, pe_cap: f64, win_max: f64,
    w_want: f64, w_glow: f64, w_deadline: f64, tau: f64, kappa: f64, synthetic: Vec<String>, conds: Vec<GoalCond>,
    liking: f64, liking_spread: f64, valence: Option<(String, f64, f64)>, needs: Option<Needs> }
impl Drives {
    /// A goal with `expect_by_hours` (RFC 0002 A.4) keeps an omission window; without one the state has no `window` map
    fn windows(&self) -> bool { self.goals.iter().any(|g| g.expect_by.is_some()) }
}
/// One need of the `needs` block (RFC 0002 A.1): a deficit over a setpoint in [0, cap] that grows per stamped hour and per unit of a
/// number sense's raw reading, and that an amount from a `satisfy_from` source reduces, paying the bound goal
#[derive(Clone)]
struct Need { id: String, say: String, unit: String, cap: f64, weight: f64, per_hour: f64, reading: Option<(usize, f64)>, src: Vec<String>, amount: f64, bind: usize,
    horizon: f64, forecast: f64, weight_spread: f64, growth_spread: f64 }
/// The `needs` block: D(x) = (Σ w_i g_i x_i^n)^(1/m); the pursue term's weight `want` and box `cap`; the success rate's step and start;
/// the win cap; the needs in declared order
#[derive(Clone)]
struct Needs { n: f64, m: f64, want: f64, cap: f64, rate: f64, init: f64, r_cap: f64, items: Vec<Need> }
/// The needs state (RFC 0002 A.3): genes, then per need (declared order) its deficit and its bound goal's success rate
#[derive(Clone)]
struct NState { genes: Json, deficit: Vec<f64>, success: Vec<f64> }
/// The drive state of an individual: genes, then per goal (declared order) wanting, afterglow, expectation, the last turn's prediction
/// error, the deadline (hours left), the turns since it was pursued and the omission window (hours left; RFC 0002 A.4); with a needs
/// block the needs state. `omit` (the expectation of each goal whose window fired this turn) and `ev` (the expectation of the goal
/// the previous stance pursued, for `drv_expect`) belong to the turn and are not stored.
#[derive(Clone)]
struct DState { genes: Json, want: Vec<f64>, glow: Vec<f64>, expect: Vec<f64>, surprise: Vec<f64>, deadline: Vec<Option<f64>>, since: Vec<u64>, window: Vec<Option<f64>>,
    nd: Option<NState>, omit: Vec<Option<f64>>, ev: f64 }
impl DState {
    fn zero(p: &Persona, d: &Drives, seed: u64) -> DState {
        let n = d.goals.len();
        let nd = d.needs.as_ref().map(|nb| NState { genes: need_genes(p, nb, seed), deficit: vec![0.0; nb.items.len()], success: vec![nb.init; nb.items.len()] });
        DState { genes: drive_genes(p, d, seed), want: vec![0.0; n], glow: vec![0.0; n], expect: vec![0.0; n], surprise: vec![0.0; n], deadline: vec![None; n], since: vec![0; n],
            window: vec![None; n], nd, omit: vec![None; n], ev: 0.0 }
    }
    fn gene(&self, k: &str, id: &str) -> Option<f64> { self.genes.get(k).and_then(|o| o.get(id)).and_then(Json::as_f64) }
    fn to_json(&self, d: &Drives) -> Json {
        let per = |f: &dyn Fn(usize) -> Json| Json::Obj(d.goals.iter().enumerate().map(|(i, g)| (g.id.clone(), f(i))).collect());
        let mut v = vec![("genes".into(), self.genes.clone()), ("want".into(), per(&|i| Json::Num(self.want[i]))), ("glow".into(), per(&|i| Json::Num(self.glow[i]))),
            ("expect".into(), per(&|i| Json::Num(self.expect[i]))), ("deadline".into(), per(&|i| self.deadline[i].map_or(Json::Null, Json::Num))),
            ("since".into(), per(&|i| Json::Num(self.since[i] as f64))), ("surprise".into(), per(&|i| Json::Num(self.surprise[i])))];
        if d.windows() { v.push(("window".into(), self.windows_json(d))); }
        Json::Obj(v)
    }
    /// The omission windows (RFC 0002 A.4): per goal with `expect_by_hours` the hours left, or null when closed
    fn windows_json(&self, d: &Drives) -> Json {
        Json::Obj(d.goals.iter().enumerate().filter(|(_, g)| g.expect_by.is_some()).map(|(i, g)| (g.id.clone(), self.window[i].map_or(Json::Null, Json::Num))).collect())
    }
    /// `state.drives`, strictly: the genes of the seed, one entry per goal of the persona in each map, every accumulator in its box
    fn read(p: &Persona, d: &Drives, j: &Json, seed: u64) -> R<DState> {
        let mut ks: Vec<&str> = vec!["genes", "want", "glow", "expect", "surprise", "deadline", "since"]; if d.windows() { ks.push("window"); }
        let kv = keys(j, "state.drives", &ks, &ks)?;
        need(canon(get(kv, "genes").unwrap()) == canon(&drive_genes(p, d, seed)), "state.drives.genes", "drive genes do not match the seed")?;
        let per = |k: &str, ok: &dyn Fn(&Json) -> bool, msg: String| -> R<Vec<Json>> { let path = at("state.drives", k);
            let o = get(kv, k).and_then(Json::as_obj).filter(|o| o.len() == d.goals.len() && d.goals.iter().all(|g| get(o, &g.id).is_some_and(ok))).ok_or_else(|| perr(&path, msg))?;
            Ok(d.goals.iter().map(|g| get(o, &g.id).unwrap().clone()).collect()) };
        // the state keeps 6 decimals, so a value clamped to a cap with more decimals can round up to half a unit past it
        let within = |lo: f64, hi: f64| move |x: &Json| x.as_f64().is_some_and(|v| v >= lo - 5e-7 && v <= hi + 5e-7);
        let nums = |v: Vec<Json>| v.iter().map(|x| x.as_f64().unwrap()).collect::<Vec<f64>>();
        let want = nums(per("want", &within(0.0, d.cap_w), format!("one number per goal in [0, {}]", pyn(d.cap_w)))?);
        let glow = nums(per("glow", &within(0.0, d.cap_a), format!("one number per goal in [0, {}]", pyn(d.cap_a)))?);
        let expect = nums(per("expect", &within(0.0, d.win_max), format!("one number per goal in [0, {}]", pyn(d.win_max)))?);
        let surprise = nums(per("surprise", &within(-d.pe_cap, d.pe_cap), format!("one number per goal within ±{}", pyn(d.pe_cap)))?);
        let deadline = per("deadline", &|x: &Json| x.is_null() || within(0.0, 1e6)(x), "one number of hours (0 to 1e6) or null per goal".into())?.iter().map(Json::as_f64).collect();
        let since = per("since", &|x: &Json| x.as_f64().is_some_and(|v| v >= 0.0 && v.fract() == 0.0 && v <= 9_007_199_254_740_992.0), "one whole number >= 0 per goal".into())?.iter().map(|x| x.as_f64().unwrap() as u64).collect();
        // the omission windows (RFC 0002 A.4): per goal with expect_by_hours, hours left in [0, expect_by_hours] or null
        let mut window = vec![None; d.goals.len()];
        if d.windows() { let wo = get(kv, "window").and_then(Json::as_obj).filter(|o| o.len() == d.goals.iter().filter(|g| g.expect_by.is_some()).count())
                .ok_or_else(|| perr("state.drives.window", "one entry per goal with expect_by_hours"))?;
            for (i, g) in d.goals.iter().enumerate() { let Some(eb) = g.expect_by else { continue }; let wp = at("state.drives.window", &g.id);
                window[i] = match get(wo, &g.id) { Some(Json::Null) => None, Some(Json::Num(x)) if *x >= 0.0 && *x <= eb + 5e-7 => Some(*x),
                    _ => return Err(perr(&wp, format!("hours left in [0, {}] (expect_by_hours) or null", pyn(eb)))) }; } }
        let n = d.goals.len();
        Ok(DState { genes: get(kv, "genes").unwrap().clone(), want, glow, expect, surprise, deadline, since, window, nd: None, omit: vec![None; n], ev: 0.0 })
    }
}
impl NState {
    fn to_json(&self, d: &Drives, nb: &Needs) -> Json {
        let per = |f: &dyn Fn(usize) -> Json| Json::Obj(nb.items.iter().enumerate().map(|(i, x)| (x.id.clone(), f(i))).collect());
        Json::Obj(vec![("genes".into(), self.genes.clone()), ("deficit".into(), per(&|i| Json::Num(self.deficit[i]))),
            ("success".into(), Json::Obj(nb.items.iter().enumerate().map(|(i, x)| (d.goals[x.bind].id.clone(), Json::Num(self.success[i]))).collect()))])
    }
    /// `state.needs`, strictly (RFC 0002 A.5): the genes of the seed, the persona's needs and bound goals exactly, each deficit in
    /// [0, cap], each success rate in [0, 1]
    fn read(p: &Persona, d: &Drives, nb: &Needs, j: &Json, seed: u64) -> R<NState> {
        let kv = keys(j, "state.needs", &["genes", "deficit", "success"], &["genes", "deficit", "success"])?;
        need(canon(get(kv, "genes").unwrap()) == canon(&need_genes(p, nb, seed)), "state.needs.genes", "need genes do not match the seed")?;
        let ids: Vec<&str> = nb.items.iter().map(|x| x.id.as_str()).collect(); let goals: Vec<&str> = nb.items.iter().map(|x| d.goals[x.bind].id.as_str()).collect();
        let dk = get(kv, "deficit").and_then(Json::as_obj).filter(|o| o.len() == ids.len() && ids.iter().all(|i| get(o, i).is_some()))
            .ok_or_else(|| perr("state.needs.deficit", format!("one deficit per need of the persona ({})", ids.join(", "))))?;
        let sk = get(kv, "success").and_then(Json::as_obj).filter(|o| o.len() == goals.len() && goals.iter().all(|g| get(o, g).is_some()))
            .ok_or_else(|| perr("state.needs.success", format!("one success rate per bound goal ({})", goals.join(", "))))?;
        let mut st = NState { genes: get(kv, "genes").unwrap().clone(), deficit: vec![], success: vec![] };
        // the state keeps 6 decimals: a value clamped to a bound can round half a unit past it
        for (x, g) in nb.items.iter().zip(&goals) {
            st.deficit.push(get(dk, &x.id).and_then(Json::as_f64).filter(|v| *v >= 0.0 && *v <= x.cap + 5e-7).ok_or_else(|| perr(&at("state.needs.deficit", &x.id), format!("a deficit in [0, {}] (cap)", pyn(x.cap))))?);
            st.success.push(get(sk, g).and_then(Json::as_f64).filter(|v| *v >= 0.0 && *v <= 1.0 + 5e-7).ok_or_else(|| perr(&at("state.needs.success", g), "a success rate in [0, 1]"))?); }
        Ok(st)
    }
}
/// The need genes (RFC 0002 A.3): per need a multiplier exp(N(0, s²)) on its weight and one on its growth, from the persona hash
/// family (name, seed, "need", id, "weight" | "growth"), 4 decimals; a spread of 0 stores no gene (multiplier 1)
fn need_genes(p: &Persona, nb: &Needs, seed: u64) -> Json {
    let s = seed.to_string();
    let g = |f: &dyn Fn(&Need) -> f64, k: &str| Json::Obj(nb.items.iter().filter(|x| f(x) > 0.0).map(|x| (x.id.clone(), Json::Num(r6(round_to((f(x) * normal(&[&p.name, &s, "need", &x.id, k])).exp(), 4))))).collect());
    Json::Obj(vec![("weight".into(), g(&|x| x.weight_spread, "weight")), ("growth".into(), g(&|x| x.growth_spread, "growth"))])
}
/// The drive genes: a standing interest offset per goal (`interest_spread`) and a gain on its wanting evidence and afterglow
/// (`reactivity_spread`), from the persona hash family: a function of name, seed and goal id
fn drive_genes(p: &Persona, d: &Drives, seed: u64) -> Json {
    let s = seed.to_string();
    let interest = d.goals.iter().filter(|g| g.interest_spread > 0.0).map(|g| (g.id.clone(), Json::Num(r6(round_to(g.interest_spread * normal(&[&p.name, &s, "interest", &g.id]), 4))))).collect();
    let gain = d.goals.iter().filter(|g| g.reactivity_spread > 0.0).map(|g| (g.id.clone(), Json::Num(r6(round_to((g.reactivity_spread * normal(&[&p.name, &s, "goal", &g.id])).exp(), 4))))).collect();
    let mut v = vec![("interest".into(), Json::Obj(interest)), ("gain".into(), Json::Obj(gain))];
    // the liking gene (RFC 0002 Design E): a multiplier exp(N(0, s²)) on afterglow.liking; a spread of 0 stores none
    if d.liking_spread > 0.0 { v.push(("liking".into(), Json::Num(r6(round_to((d.liking_spread * normal(&[&p.name, &s, "liking"])).exp(), 4))))); }
    Json::Obj(v)
}
/// One block of drive settings: (field, default, lo, hi, must be > 0); the `signals` sub-block of `wanting` is returned unread
fn drive_block<'a>(dkv: &'a [(String, Json)], k: &str, f: &[(&str, f64, f64, f64, bool)], sub: Option<&str>) -> R<(Vec<f64>, Option<&'a Json>)> {
    let path = at("drives", k);
    let bkv: &[(String, Json)] = match get(dkv, k) { None | Some(Json::Null) => &[], Some(b) => {
        let allowed: Vec<&str> = f.iter().map(|x| x.0).chain(sub).chain(["comment"]).collect(); keys(b, &path, &allowed, &[])? } };
    let v = f.iter().map(|&(n, dv, lo, hi, pos)| { let x = num_or(bkv, n, dv, &path, Some(lo), Some(hi), false)?; if pos { need(x > 0.0, &at(&path, n), "must be > 0")?; } Ok(x) }).collect::<R<Vec<f64>>>()?;
    Ok((v, sub.and_then(|s| get(bkv, s)).filter(|x| !x.is_null())))
}
/// A goal condition `key` (`goal.<id>.<signal>` or `since_pursued.<id>`) with range `rng` -> (goal index, signal, lo, hi); with a
/// needs block also `need.<id>.deficit` (RFC 0002 A.3, a number condition) -> (the need's index, "deficit", lo, hi)
fn parse_goal_cond(key: &str, rng: &Json, path: &str, goals: &[Goal], needs: &[String]) -> R<(usize, String, f64, f64)> {
    let parts: Vec<&str> = key.split('.').collect();
    let gi = if parts[0] == "need" && !needs.is_empty() {
        need(parts.len() == 3 && parts[2] == "deficit", path, "a need condition is need.<id>.deficit")?;
        needs.iter().position(|x| x == parts[1]).ok_or_else(|| perr(path, format!("unknown need {:?}", parts[1])))?
    } else {
        let (goal, sig) = if parts[0] == "since_pursued" && parts.len() == 2 { (parts[1], "since_pursued") } else {
            need(parts[0] == "goal" && parts.len() == 3, path, "a goal condition is goal.<id>.<signal> or since_pursued.<id>")?; (parts[1], parts[2]) };
        let gi = goals.iter().position(|g| g.id == goal).ok_or_else(|| perr(path, format!("unknown goal {goal:?}")))?;
        need(COND_SIGNALS.contains(&sig), path, &format!("signal must be one of {}", COND_SIGNALS.join(", ")))?; gi
    };
    let sig = if parts[0] == "need" && !needs.is_empty() { "deficit" } else if parts[0] == "since_pursued" { "since_pursued" } else { parts[2] };
    let (lo, hi) = match rng { Json::Num(t) => (*t, f64::INFINITY), _ => {
        let kv = rng.as_obj().filter(|kv| !kv.is_empty() && kv.iter().all(|(k, _)| k == "at_least" || k == "at_most")).ok_or_else(|| perr(path, "a number (at least) or {at_least, at_most}"))?;
        for (k, x) in kv { num(Some(x), &at(path, k), None, None, false)?; }
        (get(kv, "at_least").and_then(Json::as_f64).unwrap_or(f64::NEG_INFINITY), get(kv, "at_most").and_then(Json::as_f64).unwrap_or(f64::INFINITY)) } };
    Ok((gi, sig.to_string(), lo, hi))
}
/// A goal condition as text: `goal.chores.deadline_hours <= 24`, `since_pursued.chores >= 12`, `goal.fun.want in [0.5, 2]`
fn cond_text(d: &Drives, c: &GoalCond) -> String {
    let key = if c.sig == "since_pursued" { format!("since_pursued.{}", d.goals[c.goal].id) } else if c.sig == "deficit" { format!("need.{}.deficit", d.needs.as_ref().map_or("", |nb| nb.items[c.goal].id.as_str())) }
        else { format!("goal.{}.{}", d.goals[c.goal].id, c.sig) };
    match (c.lo.is_finite(), c.hi.is_finite()) { (true, false) => format!("{key} >= {}", pyn(c.lo)), (false, true) => format!("{key} <= {}", pyn(c.hi)),
        (true, true) => format!("{key} in [{}, {}]", pyn(c.lo), pyn(c.hi)), _ => key }
}
/// A goal condition `key` with range `rng` -> the id of its flag input (one per distinct goal, signal and range)
fn goal_cond(key: &str, rng: &Json, path: &str, goals: &[Goal], needs: &[String], conds: &mut Vec<GoalCond>, ins: &mut Vec<Json>, origins: &mut Vec<String>) -> R<String> {
    let (gi, sig, lo, hi) = parse_goal_cond(key, rng, path, goals, needs)?;
    if let Some(c) = conds.iter().find(|c| c.goal == gi && c.sig == sig && c.lo == lo && c.hi == hi) { return Ok(c.id.clone()); }
    let id = format!("drv_c{}", conds.len());
    synth(ins, origins, Json::Obj(vec![("id".into(), Json::Str(id.clone())), ("kind".into(), Json::Str("flag".into())), ("say".into(), Json::Str(key.into()))]), path)?;
    conds.push(GoalCond { id: id.clone(), goal: gi, sig, lo, hi }); Ok(id)
}
/// An id of the lowering's synthetic inputs (drv_want, drv_glow, drv_surprise, drv_letdown, drv_c<N>)
fn is_drv(k: &str) -> bool { ["drv_want", "drv_glow", "drv_surprise", "drv_letdown", "drv_expect", "drv_omit"].contains(&k) || k.strip_prefix("drv_c").is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())) }
/// A synthetic input of the lowering (refused when the persona declares an input of that id); `origin` = the drives path it comes from
fn synth(ins: &mut Vec<Json>, origins: &mut Vec<String>, spec: Json, origin: &str) -> R<()> {
    let id = spec.get("id").and_then(Json::as_str).unwrap_or("").to_string();
    need(!ins.iter().any(|x| x.get("id").and_then(Json::as_str) == Some(id.as_str())), "inputs", &format!("reserved input id {id}"))?;
    ins.push(spec); origins.push(origin.to_string()); Ok(())
}
/// The `needs` block (RFC 0002 A.1), read strictly: the rows N-R1 to N-R6 refuse it at the field's path. `senses` = the persona's
/// senses (a need may grow per reading of a number sense); `win_max` bounds the win a need can pay.
fn needs_spec(nj: &Json, goals: &[Goal], senses: &[Sense], win_max: f64) -> R<Needs> {
    let kv = keys(nj, "needs", &["drive", "pursue", "success", "r_cap", "items", "comment"], &[])?;
    let six = |x: f64, path: &str| need(r6(x) == x, path, "at most 6 decimals");
    let pos = |x: Option<&Json>, path: &str| -> R<f64> { let v = num(x, path, None, None, false)?; need(v > 0.0, path, "must be > 0")?; six(v, path)?; Ok(v) };
    // N-R1: drive with n > m > 1 (required)
    let dk = keys(get(kv, "drive").ok_or_else(|| perr("needs.drive", "required: {\"n\": n, \"m\": m} with n > m > 1"))?, "needs.drive", &["n", "m", "comment"], &["n", "m"])?;
    let (n, m) = (num(get(dk, "n"), "needs.drive.n", None, None, false)?, num(get(dk, "m"), "needs.drive.m", None, None, false)?);
    need(n > m && m > 1.0, "needs.drive", "n > m > 1 (the deprivation scaling)")?; six(n, "needs.drive.n")?; six(m, "needs.drive.m")?;
    let pk: &[(String, Json)] = match get(kv, "pursue") { None | Some(Json::Null) => &[], Some(b) => keys(b, "needs.pursue", &["want", "cap", "comment"], &[])? };
    let sk: &[(String, Json)] = match get(kv, "success") { None | Some(Json::Null) => &[], Some(b) => keys(b, "needs.success", &["rate", "init", "comment"], &[])? };
    let want = pos(Some(get(pk, "want").unwrap_or(&Json::Num(2.0))), "needs.pursue.want")?;
    let cap = pos(Some(get(pk, "cap").unwrap_or(&Json::Num(3.0))), "needs.pursue.cap")?;
    let rate = num_or(sk, "rate", 0.2, "needs.success", None, None, false)?; need(rate > 0.0 && rate <= 1.0, "needs.success.rate", "in (0, 1]")?; six(rate, "needs.success.rate")?;
    let init = num_or(sk, "init", 1.0, "needs.success", Some(0.0), Some(1.0), false)?; six(init, "needs.success.init")?;
    let r_cap = pos(Some(get(kv, "r_cap").ok_or_else(|| perr("needs.r_cap", "required: |win| <= r_cap"))?), "needs.r_cap")?;
    need(r_cap <= win_max, "needs.r_cap", &format!("at most drives.expectation.win_max ({}): a need's win joins the goal's wins", pyn(win_max)))?;
    let il = match get(kv, "items") { Some(Json::Arr(a)) if (1..=8).contains(&a.len()) => a, _ => return Err(perr("needs.items", "1 to 8 needs")) };
    let mut items: Vec<Need> = vec![];
    for (i, x) in il.iter().enumerate() { let path = ix("needs.items", i);
        let xk = keys(x, &path, &["id", "say", "unit", "cap", "weight", "per_hour", "per_reading", "satisfy_from", "amount", "bind", "horizon_hours", "forecast_per_hour",
            "weight_spread", "growth_spread", "comment"], &["id", "unit", "cap", "weight", "per_hour", "satisfy_from", "amount", "bind"])?;
        let id = ident(get(xk, "id"), &at(&path, "id"), "id")?;
        need(id.len() <= 32, &at(&path, "id"), "at most 32 characters (a need reserves the input id need_<id>)")?;
        need(!items.iter().any(|y| y.id == id), &at(&path, "id"), "need ids must be distinct")?;
        let say = match get(xk, "say") { None => id.replace('_', " "), Some(Json::Str(t)) => t.clone(), Some(_) => return Err(perr(&at(&path, "say"), "a string")) };
        let unit = get(xk, "unit").and_then(Json::as_str).filter(|u| (1..=24).contains(&u.chars().count())).ok_or_else(|| perr(&at(&path, "unit"), "a unit of 1-24 characters"))?.to_string();
        let (cp, wp, ap) = (at(&path, "cap"), at(&path, "weight"), at(&path, "amount"));
        let (ncap, weight, amount) = (pos(get(xk, "cap"), &cp)?, pos(get(xk, "weight"), &wp)?, pos(get(xk, "amount"), &ap)?);
        need(amount <= ncap, &ap, "at most the need's cap")?;
        let hp = at(&path, "per_hour"); let per_hour = num(get(xk, "per_hour"), &hp, None, None, false)?; need(per_hour >= 0.0, &hp, "must be >= 0")?; six(per_hour, &hp)?;
        let reading = match get(xk, "per_reading") { None | Some(Json::Null) => None, Some(r) => { let rp = at(&path, "per_reading");
            let rk = keys(r, &rp, &["sense", "rate", "comment"], &["sense", "rate"])?; let sid = get(rk, "sense").and_then(Json::as_str).unwrap_or("");
            let si = senses.iter().position(|y| y.id == sid).ok_or_else(|| perr(&at(&rp, "sense"), format!("not a declared sense ({})", if senses.is_empty() { "the persona has no senses block".to_string() } else { senses.iter().map(|y| y.id.as_str()).collect::<Vec<_>>().join(", ") })))?;
            need(!senses[si].flag, &at(&rp, "sense"), "a number sense (a need grows per unit of its raw reading)")?;
            let r8 = num(get(rk, "rate"), &at(&rp, "rate"), None, None, false)?; need(r8 >= 0.0, &at(&rp, "rate"), "must be >= 0")?; six(r8, &at(&rp, "rate"))?; Some((si, r8)) } };
        let fp = at(&path, "satisfy_from");
        let src = strs(get(xk, "satisfy_from").unwrap()).filter(|v| !v.is_empty()).ok_or_else(|| perr(&fp, "required: a list of source kinds: env, human"))?;
        for (j, k) in src.iter().enumerate() { let q = ix(&fp, j);
            need(k != "self", &q, "an amount never comes from the individual itself")?; need(k != "clock", &q, "the clock is not a source of amounts: env or human")?;
            need(k == "env" || k == "human", &q, "env | human")?; need(!src[..j].contains(k), &q, "listed twice")?; }
        let bp = at(&path, "bind"); let gid = get(xk, "bind").and_then(Json::as_str).unwrap_or("");
        let bind = goals.iter().position(|g| g.id == gid).ok_or_else(|| perr(&bp, format!("unknown goal (goals: {})", goals.iter().map(|g| g.id.as_str()).collect::<Vec<_>>().join(", "))))?;
        need(!items.iter().any(|y| y.bind == bind), &bp, "one need per goal (another need binds it)")?;
        need(goals[bind].expect_by.is_some(), &bp, &format!("the bound goal {gid} needs expect_by_hours (unanswered pursuits would never extinguish)"))?;
        let horizon = num_or(xk, "horizon_hours", 0.0, &path, Some(0.0), Some(72.0), false)?; six(horizon, &at(&path, "horizon_hours"))?;
        let forecast = num_or(xk, "forecast_per_hour", per_hour, &path, Some(0.0), None, false)?; six(forecast, &at(&path, "forecast_per_hour"))?;
        let spread = |k: &str| -> R<f64> { let v = num_or(xk, k, 0.0, &path, Some(0.0), Some(3.0), false)?; six(v, &at(&path, k))?; Ok(v) };
        items.push(Need { id, say, unit, cap: ncap, weight, per_hour, reading, src, amount, bind, horizon, forecast, weight_spread: spread("weight_spread")?, growth_spread: spread("growth_spread")? }); }
    Ok(Needs { n, m, want, cap, rate, init, r_cap, items })
}
/// A persona with a `drives` block: the block read strictly, then lowered (see `Drives`) and read as a 0.7.0 persona; the digest is
/// the whole document's, the block included. A goal floor needs `pursue` free of multi-variable rules (the lift's soundness, §2.9).
fn build_drives(doc: &Json, senses: &[Sense]) -> R<Persona> {
    let kv = doc.as_obj().unwrap(); let drv = get(kv, "drives").unwrap();
    let dkv = keys(drv, "drives", &["goals", "wanting", "afterglow", "expectation", "pursue", "effects", "learn_from_surprise", "valence", "comment"], &[])?;
    let gl = match get(dkv, "goals") { Some(Json::Arr(a)) if (2..=7).contains(&a.len()) => a, _ => return Err(perr("drives.goals", "2 to 7 goals (pursue is one variable, 2-7 levels)")) };
    let (mut goals, mut interest): (Vec<Goal>, Vec<f64>) = (vec![], vec![]); let mut starve: Vec<(Option<f64>, f64)> = vec![]; // (starve_after, priority)
    for (i, g) in gl.iter().enumerate() { let p = ix("drives.goals", i);
        let gk = keys(g, &p, &["id", "say", "interest", "interest_spread", "reactivity_spread", "floor", "starve_after", "priority", "expect_by_hours", "comment"], &["id"])?;
        let id = ident(get(gk, "id"), &at(&p, "id"), "id")?;
        need(!RESERVED_LEVELS.contains(&id.as_str()), &at(&p, "id"), "reserved word (YAML would not read it as a string)")?;
        need(!goals.iter().any(|x| x.id == id), &at(&p, "id"), "goal ids must be distinct")?;
        let say = match get(gk, "say") { None => id.replace('_', " "), Some(Json::Str(s)) => s.clone(), Some(_) => return Err(perr(&at(&p, "say"), "a string")) };
        interest.push(num_or(gk, "interest", 1.0, &p, Some(1e-6), Some(1e6), false)?);
        let interest_spread = num_or(gk, "interest_spread", 0.0, &p, Some(0.0), Some(5.0), false)?;
        let reactivity_spread = num_or(gk, "reactivity_spread", 0.0, &p, Some(0.0), Some(3.0), false)?;
        let floor = num_or(gk, "floor", 0.0, &p, Some(0.0), Some(0.5), false)?;
        let sa = match get(gk, "starve_after") { None | Some(Json::Null) => None, x => Some(num(x, &at(&p, "starve_after"), Some(1.0), Some(10000.0), true)?) };
        starve.push((sa, num_or(gk, "priority", 0.0, &p, Some(-100.0), Some(100.0), true)?));
        // RFC 0002 A.4 (N-R6): the omission window, > 0 hours
        let expect_by = match get(gk, "expect_by_hours") { None | Some(Json::Null) => None, x => { let ep = at(&p, "expect_by_hours"); let v = num(x, &ep, None, Some(1e6), false)?;
            need(v > 0.0, &ep, "must be > 0 (hours until an expected win counts as omitted)")?; Some(v) } };
        goals.push(Goal { id, say, interest_spread, reactivity_spread, floor, starve_after: sa, expect_by }); }
    need(goals.iter().fold(0.0, |a, g| a + g.floor) <= 0.5 + 1e-9, "drives.goals", "floors must sum to at most 0.5")?;
    let (w, sigs) = drive_block(dkv, "wanting", &[("half_life_hours", 12.0, 0.0, 1e6, true), ("cap", 3.0, 0.0, 1e6, true), ("drain", 0.8, 0.0, 1.0, false)], Some("signals"))?;
    let sk: &[(String, Json)] = match sigs { None => &[], Some(s) => keys(s, "drives.wanting.signals", &["progress", "novelty", "cue", "setback", "comment"], &[])? };
    let mut sig = [0.0; 4];
    for (i, (k, dv)) in [("progress", 0.6), ("novelty", 0.5), ("cue", 0.4), ("setback", 0.5)].iter().enumerate() { sig[i] = num_or(sk, k, *dv, "drives.wanting.signals", Some(-50.0), Some(50.0), false)?; }
    let (a, _) = drive_block(dkv, "afterglow", &[("half_life_hours", 3.0, 0.0, 1e6, true), ("cap", 2.0, 0.0, 1e6, true), ("liking", 0.0, 0.0, 1e6, false), ("liking_spread", 0.0, 0.0, 3.0, false)], None)?;
    let (e, _) = drive_block(dkv, "expectation", &[("rate", 0.5, 0.0, 1.0, false), ("half_life_hours", 48.0, 0.0, 1e6, true), ("pe_cap", 1.0, 0.0, 1e6, true), ("win_max", 2.0, 0.0, 1e6, true)], None)?;
    let (pw, _) = drive_block(dkv, "pursue", &[("want", 1.0, -50.0, 50.0, false), ("glow", -0.5, -50.0, 50.0, false), ("deadline", 1.5, -50.0, 50.0, false), ("deadline_tau_hours", 24.0, 0.0, 1e6, true)], None)?;
    let kappa = num_or(dkv, "learn_from_surprise", 0.0, "drives", Some(0.0), Some(1.0), false)?;
    let eff: Vec<(String, Json)> = match get(dkv, "effects") { None => vec![], Some(x) if !truthy(x) => vec![], Some(x) => x.as_obj()
        .filter(|o| o.iter().all(|(k, _)| ["wanting", "afterglow", "surprise", "omission", "comment"].contains(&k.as_str()))).ok_or_else(|| perr("drives.effects", "a mapping with wanting / afterglow / surprise / omission"))?.to_vec() };
    // RFC 0002 Design E (E-R1, E-R2): valence on one declared mood from expected value and prediction errors, both weights declared
    let moods: Vec<String> = list(get(kv, "moods"), "moods")?.iter().filter_map(|m| m.get("id").and_then(Json::as_str).map(str::to_string)).collect();
    let valence = match get(dkv, "valence") { None | Some(Json::Null) => None, Some(vj) => {
        need(get_t(&eff, "surprise").is_none(), "drives.valence", "drives.valence replaces drives.effects.surprise: declare one of them")?;
        let vk = keys(vj, "drives.valence", &["mood", "expect", "surprise", "comment"], &["mood", "expect", "surprise"])?;
        let mood = get(vk, "mood").and_then(Json::as_str).filter(|m| moods.iter().any(|x| x == m)).ok_or_else(|| perr("drives.valence.mood", format!("a declared mood ({})", if moods.is_empty() { "none".to_string() } else { moods.join(", ") })))?.to_string();
        let (we, wp) = (num(get(vk, "expect"), "drives.valence.expect", None, Some(50.0), false)?, num(get(vk, "surprise"), "drives.valence.surprise", None, Some(50.0), false)?);
        need(we > 0.0, "drives.valence.expect", "must be > 0")?; need(wp > 0.0, "drives.valence.surprise", "must be > 0")?;
        need(we < wp, "drives.valence", "expect < surprise (the expectation weight below the prediction-error weight)")?; Some((mood, we, wp)) } };
    // the lowered document
    let mut d2: Vec<(String, Json)> = kv.iter().filter(|(k, _)| k != "drives" && k != "reward_from" && k != "needs").cloned().collect();
    let slot = |d2: &mut Vec<(String, Json)>, k: &str| -> usize { match d2.iter().position(|(x, _)| x == k) { Some(i) => i, None => { d2.push((k.into(), Json::Null)); d2.len() - 1 } } };
    for k in ["traits", "moods"] { if let Some(Json::Arr(a)) = get(&d2, k) { need(!a.iter().any(|t| t.get("id").and_then(Json::as_str) == Some("pursue")), "traits", "`pursue` is reserved when a drives block is present")?; } }
    let s = |x: &str| Json::Str(x.to_string());
    let tids: Vec<String> = match get(&d2, "traits") { Some(Json::Arr(a)) => a.iter().filter_map(|t| t.get("id").and_then(Json::as_str).map(str::to_string)).collect(), _ => vec![] };
    if let Some(Json::Arr(a)) = d2.iter_mut().find(|(k, _)| k == "traits").map(|(_, v)| v) {
        a.push(Json::Obj(vec![("id".into(), s("pursue")), ("levels".into(), Json::Arr(goals.iter().map(|g| s(&g.id)).collect())), ("prior".into(), Json::Arr(interest.iter().map(|x| Json::Num(*x)).collect())),
            ("say".into(), Json::Arr(goals.iter().map(|g| Json::Str(format!("pursue: {}: {}", g.id, g.say))).collect()))])); }
    let li = slot(&mut d2, "line"); if d2[li].1.is_null() { d2[li].1 = Json::Obj(vec![]); }
    if let Json::Obj(lv) = &mut d2[li].1 { if get_t(lv, "order").map_or(true, |o| strs(o).is_some()) { // a bad order is left for the reader to refuse
        let order: Vec<String> = get_t(lv, "order").and_then(strs).unwrap_or(tids);
        let order = Json::Arr(std::iter::once(s("pursue")).chain(order.iter().filter(|x| *x != "pursue").map(|x| s(x))).collect());
        match lv.iter_mut().find(|(k, _)| k == "order") { Some(o) => o.1 = order, None => lv.push(("order".into(), order)) } } }
    let mut ins: Vec<Json> = list(get(&d2, "inputs"), "inputs")?.to_vec(); let n_decl = ins.len();
    let (mut origins, mut conds): (Vec<String>, Vec<GoalCond>) = (vec![], vec![]);
    let synthetic_num = |ins: &mut Vec<Json>, origins: &mut Vec<String>, id: &str, max: f64, say: &str, effects: Json, origin: &str| synth(ins, origins,
        Json::Obj(vec![("id".into(), s(id)), ("kind".into(), s("number")), ("max".into(), Json::Num(max)), ("say".into(), s(say)), ("effects".into(), effects)]), origin);
    if let Some(x) = get_t(&eff, "wanting") { synthetic_num(&mut ins, &mut origins, "drv_want", w[1], "wanting", x.clone(), "drives.effects.wanting")?; }
    if let Some(x) = get_t(&eff, "afterglow") { synthetic_num(&mut ins, &mut origins, "drv_glow", a[1], "afterglow", x.clone(), "drives.effects.afterglow")?; }
    if let Some(x) = get_t(&eff, "surprise") {
        let skv = x.as_obj().ok_or_else(|| perr("drives.effects.surprise", "a mapping target -> number (ordinal shift)"))?;
        let mut neg = vec![]; for (k, v) in skv { let Json::Num(v) = v else { return Err(perr(&at("drives.effects.surprise", k), "a number (ordinal shift)")) }; neg.push((k.clone(), Json::Num(-v))); }
        synthetic_num(&mut ins, &mut origins, "drv_surprise", e[2], "a win above expectation", x.clone(), "drives.effects.surprise")?;
        synthetic_num(&mut ins, &mut origins, "drv_letdown", e[2], "a win below expectation", Json::Obj(neg), "drives.effects.surprise")?; }
    // Design E: valence's prediction-error weight on drv_surprise / drv_letdown, its expectation weight on drv_expect (box [0, win_max])
    if let Some((mood, we, wp)) = &valence { let o = |w: f64| Json::Obj(vec![(mood.clone(), Json::Num(w))]);
        synthetic_num(&mut ins, &mut origins, "drv_surprise", e[2], "a win above expectation", o(*wp), "drives.valence")?;
        synthetic_num(&mut ins, &mut origins, "drv_letdown", e[2], "a win below expectation", o(-*wp), "drives.valence")?;
        synthetic_num(&mut ins, &mut origins, "drv_expect", e[3], "expected value of the pursued goal", o(*we), "drives.valence")?; }
    // A.4: an omitted win's let-down, lowered as drv_omit (box [0, pe_cap]); the id is reserved with any window
    let windows = goals.iter().any(|g| g.expect_by.is_some());
    if windows { for (j, x) in ins[..n_decl].iter().enumerate() { need(x.get("id").and_then(Json::as_str) != Some("drv_omit"), &format!("inputs[{j}].id"), "reserved for the omission let-down (a goal has expect_by_hours)")?; } }
    if let Some(x) = get_t(&eff, "omission") { synthetic_num(&mut ins, &mut origins, "drv_omit", e[2], "an expected win that did not come", x.clone(), "drives.effects.omission")?; }
    // RFC 0002 A.1-A.3: the needs block, read strictly; each need lowers to the number input need_<id> (box [-pursue.cap, +pursue.cap])
    // carrying its term into its bound goal's level of `pursue`
    let needs = match get(kv, "needs") { None => None, Some(nj) => Some(needs_spec(nj, &goals, senses, e[3])?) };
    let need_ids: Vec<String> = needs.as_ref().map_or(vec![], |nb| nb.items.iter().map(|x| x.id.clone()).collect());
    if let Some(nb) = &needs {
        for (j, x) in ins[..n_decl].iter().enumerate() { if let Some(id) = x.get("id").and_then(Json::as_str) {
            need(!nb.items.iter().any(|y| format!("need_{}", y.id) == id), &format!("inputs[{j}].id"), "reserved by a need (need_<id>)")?; } }
        for (i, x) in nb.items.iter().enumerate() {
            synthetic_num(&mut ins, &mut origins, &format!("need_{}", x.id), nb.cap, &x.say, Json::Obj(vec![("pursue".into(), Json::Obj(vec![(goals[x.bind].id.clone(), Json::Num(1.0))]))]), &ix("needs.items", i))?; } }
    let mut habits: Vec<Json> = list(get(&d2, "habits"), "habits")?.to_vec();
    for (i, h) in habits.iter_mut().enumerate() {
        let Json::Obj(hkv) = h else { continue }; let Some((_, Json::Obj(w))) = hkv.iter_mut().find(|(k, _)| k == "when") else { continue };
        if !w.iter().any(|(k, _)| k.contains('.') || is_drv(k) || need_ids.iter().any(|x| format!("need_{x}") == *k)) { continue; }
        let mut nw: Vec<(String, Json)> = vec![];
        for (k, v) in w.iter() { need(!is_drv(k) || ins[..n_decl].iter().any(|x| x.get("id").and_then(Json::as_str) == Some(k.as_str())), &format!("habits[{i}].when.{k}"), "reserved for the drives block")?;
            need(!need_ids.iter().any(|x| format!("need_{x}") == *k), &format!("habits[{i}].when.{k}"), "reserved for the needs block (a rule reads need.<id>.deficit)")?;
            if k.contains('.') { let f = goal_cond(k, v, &format!("habits[{i}].when.{k}"), &goals, &need_ids, &mut conds, &mut ins, &mut origins)?;
            if !nw.iter().any(|(x, _)| *x == f) { nw.push((f, Json::Bool(true))); } } else { nw.push((k.clone(), v.clone())); } }
        *w = nw; }
    for (g, (sa, pri)) in goals.iter().zip(&starve) { let Some(n) = sa else { continue };
        let f = goal_cond(&format!("since_pursued.{}", g.id), &Json::Obj(vec![("at_least".into(), Json::Num(*n))]), &format!("drives.goals.{}.starve_after", g.id), &goals, &need_ids, &mut conds, &mut ins, &mut origins)?;
        habits.push(Json::Obj(vec![("id".into(), Json::Str(format!("starve_{}", g.id))), ("when".into(), Json::Obj(vec![(f, Json::Bool(true))])),
            ("then".into(), Json::Obj(vec![("pursue".into(), Json::Arr(vec![s(&g.id)]))])), ("say".into(), Json::Str(format!("pursue {} now (overdue)", g.id))), ("priority".into(), Json::Num(*pri))])); }
    if !habits.is_empty() { let hi = slot(&mut d2, "habits"); d2[hi].1 = Json::Arr(habits); }
    if let Some(Json::Arr(f)) = get(&d2, "learning").and_then(|l| l.get("from")) { for (i, x) in f.iter().enumerate() { let Some(k) = x.as_str() else { continue };
        need(!is_drv(k) || ins[..n_decl].iter().any(|y| y.get("id").and_then(Json::as_str) == Some(k)), &format!("learning.from[{i}]"), "reserved for the drives block")?; } }
    let synthetic: Vec<String> = ins[n_decl..].iter().map(|x| x.get("id").and_then(Json::as_str).unwrap_or("").to_string()).collect();
    let ii = slot(&mut d2, "inputs"); d2[ii].1 = Json::Arr(ins);
    // an error in a synthetic input is reported at the drives path it comes from
    let mut p = build(&Json::Obj(d2)).map_err(|e| { for (j, o) in origins.iter().enumerate() { let pre = format!("inputs[{}]", n_decl + j);
        if let Some(rest) = e.path.strip_prefix(&pre).filter(|r| r.is_empty() || r.starts_with('.') || r.starts_with('[')) { return perr(&format!("{o}{}", rest.strip_prefix(".effects").unwrap_or("")), e.msg); } } e })?;
    p.digest = sha(doc);
    if goals.iter().any(|g| g.floor > 0.0) { for h in &p.habits { for (k, lst) in &h.rules { for r in lst { let vs = rule_vars(k, r);
        need(!(vs.iter().any(|v| v == "pursue") && vs.iter().any(|v| v != "pursue")), &format!("habits.{}", h.id), "a goal floor needs `pursue` free of multi-variable rules (use then: or a coupling)")?; } } } }
    // reward_from, read here: its reward-bearing inputs include the goals' wins
    if let Some(rf) = get(kv, "reward_from").filter(|x| !x.is_null()) { let mut bearing = p.learning.as_ref().map_or(vec![], |l| l.from.clone());
        bearing.extend(goals.iter().map(|g| format!("goals.{}.win", g.id))); p.reward = Some(reward_spec(rf, &bearing, &p.inputs)?); }
    // the need terms' box is [-pursue.cap, +pursue.cap]: the one lowered input that can push a goal down
    if let Some(nb) = &needs { for x in p.inputs.iter_mut().filter(|x| nb.items.iter().any(|y| format!("need_{}", y.id) == x.id)) { x.min = -nb.cap; } }
    p.drives = Some(Drives { goals, hl_w: w[0], cap_w: w[1], drain: w[2], sig, hl_a: a[0], cap_a: a[1], rate: e[0], hl_e: e[1], pe_cap: e[2], win_max: e[3],
        w_want: pw[0], w_glow: pw[1], w_deadline: pw[2], tau: pw[3], kappa, synthetic, conds, liking: a[2], liking_spread: a[3], valence, needs });
    Ok(p)
}
/// D(x) = (Σ_i w_i g_w,i x_i^n)^(1/m) over the needs (RFC 0002 A.3)
fn need_drive(nb: &Needs, nd: &NState, xs: &[f64]) -> f64 {
    nb.items.iter().zip(xs).fold(0.0, |a, (x, v)| a + x.weight * need_gene(nd, "weight", &x.id) * v.powf(nb.n)).powf(1.0 / nb.m)
}
/// A need's gene (`weight` or `growth`): its multiplier, 1 without one
fn need_gene(nd: &NState, k: &str, id: &str) -> f64 { nd.genes.get(k).and_then(|o| o.get(id)).and_then(Json::as_f64).unwrap_or(1.0) }
/// `xs` with entry `j` set to `v`
fn with(xs: &[f64], j: usize, v: f64) -> Vec<f64> { let mut y = xs.to_vec(); y[j] = v; y }
/// The pursue terms (RFC 0002 A.3) on a stepped state: per need (declared order) the forecast x̂ = min(x + g_gr × forecast_per_hour ×
/// τ, cap) and term = clip(want × s × (D(x̂) − D(x̂ with x̂_j ← |x̂_j − K|)), ±pursue.cap), 6 decimals; none without a needs block
fn need_terms(d: &Drives, ds: &DState) -> Vec<(f64, f64)> {
    let (Some(nb), Some(nd)) = (&d.needs, &ds.nd) else { return vec![] };
    let xh: Vec<f64> = nb.items.iter().enumerate().map(|(i, x)| r6((nd.deficit[i] + need_gene(nd, "growth", &x.id) * x.forecast * x.horizon).min(x.cap))).collect();
    nb.items.iter().enumerate().map(|(j, x)| { let red = need_drive(nb, nd, &xh) - need_drive(nb, nd, &with(&xh, j, (xh[j] - x.amount).abs()));
        (xh[j], r6((nb.want * nd.success[j] * red).max(-nb.cap).min(nb.cap))) }).collect()
}
/// The needs step (RFC 0002 A.3), after the sense step and before the drive step. The event's `needs` amounts are checked first (N-E1
/// a declared need, an amount > 0 and at most its cap; N-E2 a `satisfy_from` source, the amount's own `src` or the event's; N-E3 never
/// with the bound goal's own win; N-E4 no engine-owned id; `check` false: fuzz and prove, whose events stand for accepted ones). Then
/// two passes, each line rounded to 6 decimals: every need grows, x ← clip(x + g_gr × (per_hour × h + rate × reading), 0, cap)
/// (reading = its sense's raw value this event, else 0); then per satisfied need in declared order r = clip(D(x) − D(x with x_j ←
/// |x_j − a|), ±r_cap) and x_j ← |x_j − a|. A win r > 0 or a setback min(−r, 1) (added to the host's, at most 1) joins the bound
/// goal's signals. -> (the stepped needs state, the event's inputs for the drive step: without `needs`, the goals joined)
fn needs_step(d: &Drives, nb: &Needs, nd: &NState, read: &[Option<SRead>], raw: &[(String, Json)], src: Option<&Json>, check: bool) -> R<(NState, Vec<(String, Json)>)> {
    for x in &nb.items { let k = format!("need_{}", x.id); need(get(raw, &k).is_none(), &format!("inputs.{k}"), "the engine computes a need's term from its deficit (N-E4)")?; }
    if d.windows() { need(get(raw, "drv_omit").is_none(), "inputs.drv_omit", "the engine computes the omission let-down (N-E4)")?; }
    let goals_in = get_t(raw, "goals"); let mut amt: Vec<Option<f64>> = vec![None; nb.items.len()];
    if let Some(nj) = get_t(raw, "needs") {
        let nkv = nj.as_obj().ok_or_else(|| perr("inputs.needs", "a mapping need id -> amount (a number, or {amount, src})"))?;
        let ids = || nb.items.iter().map(|x| x.id.as_str()).collect::<Vec<_>>().join(", ");
        for (k, (nid, v)) in nkv.iter().enumerate() { let path = format!("inputs.needs.{nid}");
            need(!nkv[..k].iter().any(|(x, _)| x == nid), &path, "a need satisfied twice in one event")?;
            let i = nb.items.iter().position(|x| x.id == *nid).ok_or_else(|| perr(&path, format!("not a declared need (needs: {})", ids())))?; let x = &nb.items[i];
            let (a, own) = match v { Json::Obj(o) => { for (key, _) in o { need(key == "amount" || key == "src", &at(&path, key),
                    if key == "deficit" { "the engine owns the deficit (N-E4)" } else { "an amount is a number or {amount, src}" })?; }
                    (get(o, "amount").cloned(), get(o, "src").cloned()) }
                _ => (Some(v.clone()), None) };
            // N-E1: a number > 0, at most the need's cap
            let a = match a { Some(Json::Num(a)) if a.is_finite() && a > 0.0 && a <= x.cap => a, _ => return Err(perr(&path, format!("an amount: a number > 0 and at most the need's cap ({})", pyn(x.cap)))) };
            // N-E2: a satisfy_from source: the amount's own or the event's
            if check { let sp = at(&path, "src"); let from = x.src.join(" or "); let bad = "a source: human[:id] | env[:id] (id: 1-64 of A-Z a-z 0-9 _ . - @ / :)";
                match own.as_ref().or(src) {
                    None => return Err(perr(&sp, format!("an amount for {nid} needs a source: its own src or the event's ({from})"))),
                    Some(Json::Str(t)) => match src_kind(t) {
                        None => return Err(perr(&sp, bad)),
                        Some("self") => return Err(perr(&sp, format!("an amount for {nid} from the individual itself (src self) is refused: amounts come from {from}"))),
                        Some("clock") => return Err(perr(&sp, format!("an amount for {nid} from the clock is refused: amounts come from {from}"))),
                        Some(kd) if !x.src.iter().any(|y| y == kd) => return Err(perr(&sp, format!("an amount for {nid} from {kd} is refused: {nid} is satisfied from {from}"))),
                        _ => {} },
                    Some(_) => return Err(perr(&sp, bad)) } }
            // N-E3: one outcome, one payment
            let gid = &d.goals[x.bind].id;
            need(!goals_in.and_then(|g| g.get(gid)).and_then(|q| q.get("win")).is_some_and(truthy), &path, &format!("an amount for {nid} and a win of its goal {gid} in one event (one outcome, one payment)"))?;
            amt[i] = Some(a); } }
    // pass 1: every need grows by the stamped hours and its sense's raw reading
    let h = num(Some(get(raw, "elapsed_hours").unwrap_or(&Json::Num(0.0))), "inputs.elapsed_hours", Some(0.0), None, false)?;
    let mut ns = nd.clone();
    for (i, x) in nb.items.iter().enumerate() {
        let (reading, rate) = x.reading.map_or((0.0, 0.0), |(si, r)| (read.get(si).and_then(Option::as_ref).and_then(|q| q.raw.as_f64()).unwrap_or(0.0), r));
        ns.deficit[i] = r6((nd.deficit[i] + need_gene(nd, "growth", &x.id) * (x.per_hour * h + rate * reading)).max(0.0).min(x.cap)); }
    // pass 2: each satisfaction pays, in declared order (DECISIONS-R111 item 2)
    let mut paid: Vec<(usize, f64)> = vec![];
    for (j, x) in nb.items.iter().enumerate() { let Some(a) = amt[j] else { continue };
        let after = with(&ns.deficit, j, (ns.deficit[j] - a).abs());
        let r = r6((need_drive(nb, &ns, &ns.deficit) - need_drive(nb, &ns, &after)).max(-nb.r_cap).min(nb.r_cap));
        ns.deficit[j] = r6(after[j].max(0.0).min(x.cap)); if r != 0.0 { paid.push((x.bind, r)); } }
    let mut out: Vec<(String, Json)> = raw.iter().filter(|(k, _)| k != "needs").cloned().collect();
    if paid.is_empty() { return Ok((ns, out)); }
    // the wins and setbacks join the goals' signals (a malformed `goals` is left for the drive step to refuse)
    let mut g: Vec<(String, Json)> = match goals_in { None => vec![], Some(Json::Obj(o)) => o.clone(), Some(_) => return Ok((ns, out)) };
    for (b, r) in paid { let gid = d.goals[b].id.clone();
        let mut sg: Vec<(String, Json)> = match get(&g, &gid) { None => vec![], Some(Json::Obj(o)) => o.clone(), Some(_) => continue };
        let (key, v) = if r > 0.0 { ("win", Json::Num(r)) } else { let add = (-r).min(1.0);
            match get(&sg, "setback") { None => ("setback", Json::Num(add)), Some(Json::Num(y)) if (0.0..=1.0).contains(y) => ("setback", Json::Num(r6((y + add).min(1.0)))), Some(_) => continue } };
        match sg.iter_mut().find(|(k, _)| k == key) { Some(e) => e.1 = v, None => sg.push((key.into(), v)) }
        match g.iter_mut().find(|(k, _)| *k == gid) { Some(e) => e.1 = Json::Obj(sg), None => g.push((gid, Json::Obj(sg))) } }
    match out.iter_mut().find(|(k, _)| k == "goals") { Some(e) => e.1 = Json::Obj(g), None => out.push(("goals".into(), Json::Obj(g))) }
    Ok((ns, out))
}
/// The omission let-down's learning rate α⁻ (RFC 0002 A.4): `rate` in this release (Design D's optimism gene will set it)
fn omission_rate(d: &Drives) -> f64 { d.rate }
/// The drive update of one turn (§4 of the design, steps 1-4): decay by the idle hours, then a win (prediction error against the
/// expectation, Rescorla-Wagner update, consummation drain, afterglow from the positive error and, with `afterglow.liking`, from the
/// win itself; a full win meets the deadline; a win closes the goal's omission window), else an open window counts down by the hours
/// and at 0 fires the omission let-down once (RFC 0002 A.4: d = −min(E, pe_cap), E ← E + α⁻ × (0 − E), the window closed); then the
/// wanting evidence; per goal in declared order, each number capped and rounded to 6 decimals. With a needs block each bound goal's
/// success rate then steps (A.3): s + rate × (1 − s) on a win, s × (1 − rate) when its window fired. `pg` = the goal the previous
/// stance pursued: its expectation after the decay is the turn's expected value (Design E's `drv_expect`).
fn drive_step(d: &Drives, ds: &DState, raw: &[(String, Json)], pg: Option<usize>) -> R<DState> {
    let n = d.goals.len(); let mut ev: Vec<Vec<(String, Option<f64>)>> = vec![vec![]; n];
    if let Some(gj) = get_t(raw, "goals") {
        let gkv = gj.as_obj().ok_or_else(|| perr("inputs.goals", "a mapping goal -> signals"))?;
        for (gid, sj) in gkv { let p = format!("inputs.goals.{gid}");
            let gi = d.goals.iter().position(|g| g.id == *gid).ok_or_else(|| perr(&p, format!("unknown goal (goals: {})", d.goals.iter().map(|g| g.id.as_str()).collect::<Vec<_>>().join(", "))))?;
            let skv = sj.as_obj().ok_or_else(|| perr(&p, "a mapping signal -> value"))?; let mut e: Vec<(String, Option<f64>)> = vec![];
            for (k, x) in skv { let sp = at(&p, k);
                need(GOAL_SIGNALS.contains(&k.as_str()), &sp, &format!("one of {}", GOAL_SIGNALS.join(", ")))?;
                let v = match (k.as_str(), x) {
                    ("cue", Json::Bool(b)) | ("novelty", Json::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
                    ("cue", _) => return Err(perr(&sp, "true | false")),
                    ("win", _) => Some(num(Some(x), &sp, Some(0.0), Some(d.win_max), false)?),
                    ("deadline_hours", Json::Null) => None,
                    ("deadline_hours", _) => Some(num(Some(x), &sp, Some(0.0), Some(1e6), false)?),
                    _ => Some(num(Some(x), &sp, Some(0.0), Some(1.0), false)?) };
                match e.iter_mut().find(|(y, _)| y == k) { Some(slot) => slot.1 = v, None => e.push((k.clone(), v)) } }
            ev[gi] = e; } }
    let h = num(Some(get(raw, "elapsed_hours").unwrap_or(&Json::Num(0.0))), "inputs.elapsed_hours", Some(0.0), None, false)?;
    let (dw, da, de) = (0.5f64.powf(h / d.hl_w), 0.5f64.powf(h / d.hl_a), 0.5f64.powf(h / d.hl_e));
    let mut ns = ds.clone(); ns.omit = vec![None; n]; ns.ev = 0.0; let mut won = vec![false; n];
    // Design E: the liking gain on the win itself (0 = afterglow from the positive error alone)
    let lg = if d.liking > 0.0 { d.liking * ds.genes.get("liking").and_then(Json::as_f64).unwrap_or(1.0) } else { 0.0 };
    for (i, g) in d.goals.iter().enumerate() {
        let e = &ev[i]; let sv = |k: &str| e.iter().find(|(x, _)| x == k).and_then(|(_, v)| *v).unwrap_or(0.0);
        let k = ds.gene("gain", &g.id).unwrap_or(1.0);
        let (mut want, mut glow, mut ex) = (ds.want[i] * dw, ds.glow[i] * da, ds.expect[i] * de);
        let mut dl = ds.deadline[i].map(|x| (x - h).max(0.0));
        if let Some((_, v)) = e.iter().find(|(x, _)| x == "deadline_hours") { dl = *v; }
        if pg == Some(i) { ns.ev = r6(ex.max(0.0).min(d.win_max)); }
        let (mut delta, m) = (0.0, sv("win")); let mut wnd = ds.window[i];
        if m > 0.0 { delta = (m - ex).min(d.pe_cap).max(-d.pe_cap); ex += d.rate * (m - ex); want *= 1.0 - d.drain * m.min(1.0);
            glow += k * if lg > 0.0 { lg * m + delta.max(0.0) } else { delta.max(0.0) }; if m >= 1.0 { dl = None; } wnd = None; won[i] = true; }
        else if let Some(w) = wnd { let w = (w - h).max(0.0);
            if w <= 0.0 { ns.omit[i] = Some(ex); delta = 0.0 - ex.min(d.pe_cap); ex += omission_rate(d) * (0.0 - ex); wnd = None; } else { wnd = Some(w); } }
        want += k * ["progress", "novelty", "cue", "setback"].iter().zip(d.sig).fold(0.0, |a, (s, w)| a + w * sv(s));
        ns.want[i] = r6(want.max(0.0).min(d.cap_w)); ns.glow[i] = r6(glow.max(0.0).min(d.cap_a)); ns.expect[i] = r6(ex.max(0.0).min(d.win_max));
        ns.deadline[i] = dl.map(r6); ns.surprise[i] = r6(delta); ns.window[i] = wnd.map(r6); }
    if let (Some(nb), Some(nd)) = (&d.needs, ns.nd.as_mut()) { for (j, x) in nb.items.iter().enumerate() { let s = nd.success[j];
        if won[x.bind] { nd.success[j] = r6((s + nb.rate * (1.0 - s)).min(1.0)); } else if ns.omit[x.bind].is_some() { nd.success[j] = r6((s * (1.0 - nb.rate)).max(0.0)); } } }
    Ok(ns)
}
/// What a stepped drive state gives the compiler: `pursue`'s drive terms (genes, wanting, afterglow, deadline urgency), the turn's
/// summed prediction error (clipped to ±pe_cap) and its share of the learning sign
struct DriveCtx { terms: Vec<(&'static str, Vec<f64>)>, learn_sign: f64, surprise: f64, omit: f64 }
fn drive_ctx(d: &Drives, ds: &DState) -> DriveCtx {
    let urg = |dl: Option<f64>| dl.map_or(0.0, |x| 1.0 / (1.0 + x / d.tau));
    let per = |f: &dyn Fn(usize) -> f64| (0..d.goals.len()).map(f).collect::<Vec<f64>>();
    let terms = vec![("genes", per(&|i| ds.gene("interest", &d.goals[i].id).unwrap_or(0.0))), ("wanting", per(&|i| r6(d.w_want * ds.want[i]))),
        ("afterglow", per(&|i| r6(d.w_glow * ds.glow[i]))), ("deadline", per(&|i| r6(d.w_deadline * urg(ds.deadline[i]))))];
    let s = ds.surprise.iter().fold(0.0, |a, b| a + b).min(d.pe_cap).max(-d.pe_cap);
    // RFC 0002 A.4: an omission joins the summed surprise but not the learning sign (it is about the stance that opened the window)
    let sl = if ds.omit.iter().any(Option::is_some) { ds.surprise.iter().zip(&ds.omit).filter(|(_, o)| o.is_none()).fold(0.0, |a, (b, _)| a + b).min(d.pe_cap).max(-d.pe_cap) } else { s };
    let omit = r6(ds.omit.iter().flatten().fold(0.0, |a, b| a + b).min(d.pe_cap));
    DriveCtx { terms, learn_sign: r6(d.kappa * sl), surprise: r6(s), omit }
}
/// The turn's inputs as the lowered persona reads them: the host's (without `goals`), then the drive aggregates and goal conditions
fn host_inputs(d: &Drives, ds: &DState, c: &DriveCtx, raw: &[(String, Json)]) -> R<Vec<(String, Json)>> {
    for k in &d.synthetic { need(get(raw, k).is_none(), &format!("inputs.{k}"), "reserved for the drives block")?; }
    let mut x: Vec<(String, Json)> = raw.iter().filter(|(k, _)| k != "goals").cloned().collect();
    let max = |v: &[f64]| v.iter().fold(f64::NEG_INFINITY, |a, b| a.max(*b)); let terms = need_terms(d, ds);
    for id in &d.synthetic { let v = match id.as_str() { "drv_want" => Json::Num(r6(max(&ds.want))), "drv_glow" => Json::Num(r6(max(&ds.glow))),
            "drv_surprise" => Json::Num(r6(c.surprise.max(0.0))), "drv_letdown" => Json::Num(r6((-c.surprise).max(0.0))),
            "drv_expect" => Json::Num(ds.ev), "drv_omit" => Json::Num(c.omit),
            _ if id.starts_with("need_") => { let j = d.needs.as_ref().and_then(|nb| nb.items.iter().position(|x| format!("need_{}", x.id) == *id)).unwrap_or(0);
                Json::Num(terms.get(j).map_or(0.0, |t| t.1)) }
            _ => { let Some(q) = d.conds.iter().find(|q| q.id == *id) else { continue }; let g = q.goal;
                let v = match q.sig.as_str() { "deadline_hours" => ds.deadline[g], "want" => Some(ds.want[g]), "glow" => Some(ds.glow[g]), "expect" => Some(ds.expect[g]),
                    "deficit" => ds.nd.as_ref().map(|nd| nd.deficit[g]), _ => Some(ds.since[g] as f64) };
                Json::Bool(v.is_some_and(|v| v >= q.lo && v <= q.hi)) } };
        x.push((id.clone(), v)); }
    Ok(x)
}
/// A turn's state and inputs as the compiler sees them: with a senses block the sense step first (the readings of `senses`
/// against the normals, `src` the event's source, `check` the provenance check), then with a drives block the stepped drives and
/// the host inputs plus the drive aggregates; else the state and inputs as given
fn prep(p: &Persona, st: &State, raw: &[(String, Json)], src: Option<&Json>, check: bool) -> R<(State, Vec<(String, Json)>)> {
    let mut s2 = st.clone(); let mut x = raw.to_vec();
    if let (Some(sn), Some(ss)) = (&p.senses, &st.senses) { let (ns, host) = sense_step(sn, ss, &x, src, check)?; s2.senses = Some(ns); x = host; }
    let (Some(d), Some(ds)) = (&p.drives, &st.drives) else { return Ok((s2, x)) };
    // RFC 0002 A.3: the needs step, after the sense step (it reads the raw readings) and before the drive step (its wins join it)
    let mut ds1 = ds.clone();
    if let (Some(nb), Some(nd)) = (&d.needs, &ds.nd) { let read = s2.senses.as_ref().map_or(&[][..], |ss| &ss.read[..]);
        let (nn, host) = needs_step(d, nb, nd, read, &x, src, check)?; ds1.nd = Some(nn); x = host; }
    // Design E: the goal the previous stance pursued (its `since` is 0 after the turn; none before the first turn)
    let pg = if st.turn > 0 { ds.since.iter().position(|n| *n == 0) } else { None };
    let ns = drive_step(d, &ds1, &x, pg)?; let host = host_inputs(d, &ns, &drive_ctx(d, &ns), &x)?;
    s2.drives = Some(ns); Ok((s2, host))
}
/// An event's `src`, read (not compiled) with `reward_from`, a senses block or a credit block: checked (`check`), taken out of the
/// inputs and returned; without any of them it stays an undeclared input
fn take_src(p: &Persona, raw: Vec<(String, Json)>, check: bool) -> R<(Option<Json>, Vec<(String, Json)>)> {
    if p.reward.is_none() && p.senses.is_none() && p.credit().is_none() && !p.has_needs() { return Ok((None, raw)); }
    if check { match &p.reward { Some(rw) => provenance(rw, &raw)?, None => src_form(&raw)? } }
    let src = get(&raw, "src").cloned();
    Ok((src, raw.into_iter().filter(|(k, _)| k != "src").collect()))
}

// ------------------------------------------------------------------------------------- action-addressed feedback (§2.8)
/// The event keys of a credit block, read (not compiled): `await` (the host will report this decision's outcome), `for` (the
/// decision a reward or correction pays: a turn, or {turn, stance} with the paid stance's digest) and `cancel` (the open decisions
/// no outcome will come for); `given` = the keys as given, echoed in the stance's inputs
#[derive(Default)]
struct Pay { wait: bool, pays: Option<(u64, Option<String>)>, cancel: Option<Vec<u64>>, given: Vec<(String, Json)> }
const PAY_KEYS: [&str; 3] = ["await", "for", "cancel"];
/// "sha256:" and 64 lower-case hex digits
fn is_digest(s: &str) -> bool { s.len() == 71 && s.starts_with("sha256:") && s[7..].bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) }
/// A stance's digest (§2.8): sha256 of the stance document's canonical JSON without `credit`, `state_digest` and `timing`. The
/// state remembers it (an open decision's, the previous stance's) without hashing itself, and `for: {turn, stance}` binds it.
pub fn stance_digest(doc: &Json) -> String {
    match doc { Json::Obj(v) => sha(&Json::Obj(v.iter().filter(|(k, _)| !["credit", "state_digest", "timing"].contains(&k.as_str())).cloned().collect())), _ => sha(doc) }
}
/// With a credit block: `await`, `for` and `cancel` taken out of the inputs and read (B-R5: a malformed value refuses the event);
/// without one they stay undeclared inputs (listed in `ignored`)
fn take_pay(p: &Persona, raw: Vec<(String, Json)>) -> R<(Option<Pay>, Vec<(String, Json)>)> {
    let Some(c) = p.credit() else { return Ok((None, raw)) };
    let whole = |x: &Json| x.as_f64().filter(|v| *v >= 0.0 && v.fract() == 0.0 && *v <= 9_007_199_254_740_992.0).map(|v| v as u64);
    let list = format!("a list of at most {} turns (max_open), each a whole number >= 0", c.max_open);
    let mut k = Pay::default();
    for (key, x) in raw.iter().filter(|(key, _)| PAY_KEYS.contains(&key.as_str())) {
        match key.as_str() {
            "await" => k.wait = match x { Json::Bool(b) => *b, _ => return Err(perr("inputs.await", "true | false (true: the host will report this decision's outcome)")) },
            "for" => k.pays = Some(match x {
                Json::Obj(_) => { let o = keys(x, "inputs.for", &["turn", "stance"], &["turn", "stance"])?;
                    let n = whole(get(o, "turn").unwrap()).ok_or_else(|| perr("inputs.for.turn", "a turn: a whole number >= 0"))?;
                    let s = get(o, "stance").and_then(Json::as_str).filter(|s| is_digest(s)).ok_or_else(|| perr("inputs.for.stance", "the paid stance's digest (sha256:<64 hex>: its credit.stance)"))?;
                    (n, Some(s.to_string())) }
                _ => (whole(x).ok_or_else(|| perr("inputs.for", "the decision this feedback pays: a turn (a whole number >= 0) or {turn, stance}"))?, None) }),
            _ => { let a = x.as_arr().filter(|a| a.len() <= c.max_open).ok_or_else(|| perr("inputs.cancel", list.clone()))?;
                k.cancel = Some(a.iter().enumerate().map(|(i, n)| whole(n).ok_or_else(|| perr(&ix("inputs.cancel", i), "a turn: a whole number >= 0"))).collect::<R<Vec<u64>>>()?); } }
        k.given.push((key.clone(), x.clone())); }
    Ok((Some(k), raw.into_iter().filter(|(key, _)| !PAY_KEYS.contains(&key.as_str())).collect()))
}
/// B-R5: a cancel says no outcome will come, so it comes from env or a person: never from the individual, the clock or nowhere
fn cancel_src(src: Option<&Json>) -> R<()> {
    match src.and_then(Json::as_str).and_then(src_kind) { Some("env" | "human") => Ok(()),
        _ => Err(perr("inputs.src", "a cancel needs a source: env[:id] or human[:id] (never the individual itself or the clock)")) }
}
/// What the credit step settled before compile, for the stance's `credit` object: the payment (turn, eligibility, sign, absorbed,
/// the paid stance's digest) and the decisions closed (by a control line, expired, cancelled, paid), in that order
struct Settled { paid: Option<(u64, f64, f64, bool, String)>, closed: Vec<(u64, &'static str)> }
/// The credit step (§2.8, RFC 0001 B.2), after the drive step and before compile; t = this turn. Open decisions with t − n − 1 >=
/// 4 x half_life_events expire, the cancels close theirs (B-R4 against the open set after expiry), then feedback (a learning flag
/// on, or with drives κ x surprise != 0) pays one decision: the one `for` names (an open one, or the previous stance t − 1), else
/// t − 1 when no other decision is open (B-R1, B-R2, B-R3; B-R6 for a digest-bound `for`). The paid credit (the open decision's,
/// or the state's for t − 1) and its eligibility e = 0.5^((t − n − 1) / half_life_events), 6 decimals, go to compile's learner.
/// Anything refused is refused whole, before any change. -> (the state compile reads, what was settled)
fn credit_step(p: &Persona, st: &State, host: &[(String, Json)], k: &Pay, src: Option<&Json>, check: bool) -> R<(State, Settled)> {
    let (c, l) = (p.credit().unwrap(), p.learning.as_ref().unwrap()); let t = st.turn;
    if k.cancel.is_some() && check { cancel_src(src)?; }
    // feedback, read as compile reads it (the inputs are checked before it)
    let (vals, _, _, _) = resolve_inputs(p, st, host)?;
    let on = |id: &String| matches!(get(&vals, id), Some(Json::Bool(true)));
    let flags = l.from.iter().enumerate().fold(0.0, |s, (i, id)| if on(id) { s + if i == 0 { 1.0 } else { -1.0 } } else { s });
    let ks = match (&p.drives, &st.drives) { (Some(d), Some(ds)) => Some(drive_ctx(d, ds).learn_sign), _ => None };
    let feedback = l.from.iter().any(on) || ks.is_some_and(|x| x != 0.0);
    let sign = match ks { Some(x) => (flags + x).clamp(-1.0, 1.0), None => flags };
    // B-R3: `for` names the decision a reward or a correction pays
    if k.pays.is_some() && !feedback { return Err(perr("inputs.for", "this event carries no feedback: `for` names the decision a reward or a correction pays")); }
    let prev = t.checked_sub(1);
    let paid = if feedback { k.pays.as_ref().map(|x| x.0).or(prev) } else { None };
    let turns = |o: &[Open]| if o.is_empty() { "none".to_string() } else { o.iter().map(|x| x.turn.to_string()).collect::<Vec<_>>().join(", ") };
    // a control line's closures are reported now; then expiry, then the cancels (B-R4) against the open set after it
    let mut ns = st.clone(); let mut closed: Vec<(u64, &'static str)> = ns.control_closed.drain(..).map(|n| (n, "control")).collect();
    let (keep, gone): (Vec<Open>, Vec<Open>) = ns.open.drain(..).partition(|o| (t.saturating_sub(o.turn + 1) as f64) < 4.0 * c.half_life);
    closed.extend(gone.iter().map(|o| (o.turn, "expired"))); ns.open = keep;
    let cancel = k.cancel.as_deref().unwrap_or(&[]);
    for (i, n) in cancel.iter().enumerate() { let path = ix("inputs.cancel", i);
        need(!cancel[..i].contains(n), &path, "listed twice")?;
        need(paid != Some(*n), &path, &format!("turn {n} is the decision this event's feedback pays"))?;
        need(ns.open.iter().any(|o| o.turn == *n), &path, &format!("turn {n} is not open (open: {})", turns(&ns.open)))?; }
    for n in cancel { ns.open.retain(|o| o.turn != *n); closed.push((*n, "cancelled")); }
    let Some(n) = paid else { return Ok((ns, Settled { paid: None, closed })) };
    // B-R1: without `for`, feedback pays the previous stance only while no other decision is open
    if k.pays.is_none() { let others: Vec<Open> = ns.open.iter().filter(|o| Some(o.turn) != prev).cloned().collect();
        need(others.is_empty(), "inputs.for", &format!("feedback without `for` while other decisions are open ({}): name the decision it pays", turns(&others)))?; }
    // B-R2: an open decision, or the previous stance
    let (credit, digest) = match ns.open.iter().position(|o| o.turn == n) {
        Some(i) => { let o = ns.open.remove(i); closed.push((n, "paid")); (o.credit, o.stance) }
        None if Some(n) == prev => (st.credit.clone(), st.prev_stance.clone().unwrap_or_default()),
        None => return Err(perr("inputs.for", format!("turn {n} is neither open nor the previous stance ({}): paid, cancelled, expired, evicted, never opened or still to come (open: {})",
            prev.map_or("none".to_string(), |x| x.to_string()), turns(&ns.open)))) };
    // B-R6: a digest-bound `for` pays the stance it names
    if let Some(s) = k.pays.as_ref().and_then(|x| x.1.as_ref()) { need(*s == digest, "inputs.for.stance", &format!("turn {n}'s stance is {digest}, not {s}"))?; }
    let e = if Some(n) == prev { 1.0 } else { r6(0.5f64.powf((t - n - 1) as f64 / c.half_life)) };
    let absorbed = credit.iter().all(|(_, v)| v.iter().all(|x| *x == 0.0));
    ns.credit = credit; ns.elig = e;
    Ok((ns, Settled { paid: Some((n, e, sign, absorbed, digest)), closed }))
}
/// After the turn (§2.8): the keys echoed in the stance's inputs, its `credit` object (paid, closed, open, this stance's digest),
/// an awaited decision opened (beyond max_open the oldest is evicted), the state remembering this stance's digest
fn settle(p: &Persona, j: &mut Json, mut ns: State, k: &Pay, s: Settled) -> State {
    let c = p.credit().unwrap(); let t = ns.turn - 1;
    let Json::Obj(v) = j else { return ns };
    if let Some((_, Json::Obj(ins))) = v.iter_mut().find(|(key, _)| key == "inputs") { ins.extend(k.given.iter().cloned()); }
    let digest = stance_digest(&Json::Obj(v.clone())); let mut closed = s.closed;
    if k.wait { ns.open.push(Open { turn: t, credit: ns.credit.clone(), stance: digest.clone() });
        while ns.open.len() > c.max_open { closed.push((ns.open.remove(0).turn, "evicted")); } }
    ns.prev_stance = Some(digest.clone()); ns.elig = 1.0; ns.seal(p);
    let n = |x: u64| Json::Num(x as f64);
    let paid = s.paid.map_or(Json::Null, |(turn, e, sign, absorbed, stance)| Json::Obj(vec![("turn".into(), n(turn)), ("e".into(), Json::Num(e)), ("sign".into(), Json::Num(sign)),
        ("absorbed".into(), Json::Bool(absorbed)), ("stance".into(), Json::Str(stance))]));
    v.push(("credit".into(), Json::Obj(vec![("paid".into(), paid),
        ("closed".into(), Json::Arr(closed.iter().map(|(x, a)| Json::Obj(vec![("turn".into(), n(*x)), ("as".into(), Json::Str(a.to_string()))])).collect())),
        ("open".into(), Json::Arr(ns.open.iter().map(|o| n(o.turn)).collect())), ("stance".into(), Json::Str(digest))])));
    if let Some((_, d)) = v.iter_mut().find(|(key, _)| key == "state_digest") { *d = Json::Str(ns.digest.clone()); }
    ns
}
/// The fuzzer's payments (§2.8, a credit block only): `await`, then `for` and `cancel` as kinds that `resolve_payments` turns
/// into turns against the state an event runs on. -> (key, kinds)
pub fn payment_alphabet(p: &Persona) -> Vec<(String, Vec<Json>)> {
    if p.credit().is_none() { return vec![]; }
    let k = |v: &[&str]| v.iter().map(|x| Json::Str(x.to_string())).collect::<Vec<Json>>();
    vec![("await".into(), vec![Json::Bool(true)]), ("for".into(), k(&["late", "reordered", "previous", "closed", "unknown", "mismatch"])), ("cancel".into(), k(&["oldest", "unknown"]))]
}
/// A fuzz event's payment kinds resolved against the state it runs on: `for` names the oldest open decision (late), the newest
/// (reordered), the previous stance, a past turn no longer open (a duplicate, or a payment after a cancel or expiry), a turn to
/// come (unknown), or the oldest open decision bound to another stance's digest (mismatch); `cancel` the oldest open decision
/// or a turn to come. A kind with nothing to name is left out; every other input is as given.
pub fn resolve_payments(p: &Persona, st: &State, e: &[(String, Json)]) -> Vec<(String, Json)> {
    if p.credit().is_none() { return e.to_vec(); }
    let (t, open) = (st.turn, &st.open); let n = |x: u64| Json::Num(x as f64);
    e.iter().filter_map(|(k, x)| { let kind = x.as_str().unwrap_or("");
        let v = match (k.as_str(), kind) {
            ("for", "late") => open.first().map(|o| n(o.turn)),
            ("for", "reordered") => open.last().filter(|_| open.len() > 1).map(|o| n(o.turn)),
            ("for", "previous") => t.checked_sub(1).map(n),
            ("for", "closed") => (0..t.saturating_sub(1)).rev().find(|x| !open.iter().any(|o| o.turn == *x)).map(n),
            ("for", "unknown") | ("cancel", "unknown") => Some(if k == "for" { n(t + 3) } else { Json::Arr(vec![n(t + 5)]) }),
            ("for", "mismatch") => open.first().map(|o| Json::Obj(vec![("turn".into(), n(o.turn)), ("stance".into(), Json::Str(format!("sha256:{}", "0".repeat(64))))])),
            ("cancel", "oldest") => open.first().map(|o| Json::Arr(vec![n(o.turn)])),
            _ => Some(x.clone()) };
        v.map(|v| (k.clone(), v)) }).collect()
}
/// Is `id` a learning block's reward or correction flag (§2.8)?
pub fn learning_flag(p: &Persona, id: &str) -> bool { p.learning.as_ref().is_some_and(|l| l.from.iter().any(|f| f == id)) }
/// The open decisions of a state with a credit block (§2.8): (turn, the eligibility a payment at the next turn would get); none
/// without one
pub fn open_decisions(p: &Persona, st: &State) -> Option<Vec<(u64, f64)>> {
    let c = p.credit()?;
    Some(st.open.iter().map(|o| (o.turn, if o.turn + 1 == st.turn { 1.0 } else { r6(0.5f64.powf(st.turn.saturating_sub(o.turn + 1) as f64 / c.half_life)) })).collect())
}
/// The stance document of a drives turn: `pursue` leaves `stance` for its own object (goal, p, odds, released, say, the floor lift),
/// `drives` = the values this turn used, `inputs` = the host's (the goals as given, the synthetic inputs left out)
fn stance_drives(j: &mut Json, d: &Drives, ds: &DState, lift: &[(String, f64)], given: &[(String, Json)]) {
    let Json::Obj(v) = j else { return };
    let mut pz = Json::Null;
    if let Some((_, Json::Obj(s))) = v.iter_mut().find(|(k, _)| k == "stance") { if let Some(i) = s.iter().position(|(k, _)| k == "pursue") { pz = s.remove(i).1; } }
    let goal = pz.get("level").and_then(Json::as_str).unwrap_or("").to_string(); let f = |k: &str| pz.get(k).cloned().unwrap_or(Json::Null);
    let say = d.goals.iter().find(|g| g.id == goal).map_or(String::new(), |g| g.say.clone());
    let pursue = Json::Obj(vec![("goal".into(), Json::Str(goal)), ("p".into(), f("p")), ("odds".into(), f("odds")), ("released".into(), f("released")), ("say".into(), Json::Str(say)),
        ("lift".into(), Json::Obj(lift.iter().map(|(k, x)| (k.clone(), Json::Num(*x))).collect()))]);
    let dj = ds.to_json(d); let mut drives: Vec<(String, Json)> = ["want", "glow", "expect", "surprise", "deadline", "since"].iter().map(|k| (k.to_string(), dj.get(k).cloned().unwrap_or(Json::Null))).collect();
    // RFC 0002 A.6: per goal with expect_by_hours its omission window (hours left, or null)
    if d.windows() { drives.push(("window".into(), ds.windows_json(d))); }
    if let Some((_, Json::Obj(ins))) = v.iter_mut().find(|(k, _)| k == "inputs") { ins.retain(|(k, _)| !d.synthetic.contains(k));
        if let Some(g) = get_t(given, "goals") { ins.push(("goals".into(), g.clone())); }
        if d.needs.is_some() { if let Some(g) = get_t(given, "needs") { ins.push(("needs".into(), g.clone())); } } }
    v.push(("pursue".into(), pursue)); v.push(("drives".into(), Json::Obj(drives)));
    // RFC 0002 A.6: per need its deficit after the event, the forecast the term read and the term
    if let (Some(nb), Some(nd)) = (&d.needs, &ds.nd) { let t = need_terms(d, ds);
        v.push(("needs".into(), Json::Obj(nb.items.iter().enumerate().map(|(i, x)| (x.id.clone(), Json::Obj(vec![("deficit".into(), Json::Num(nd.deficit[i])),
            ("forecast".into(), Json::Num(t[i].0)), ("term".into(), Json::Num(t[i].1))]))).collect()))); }
}
/// The goals the habits in force allow `pursue` to take (its one-variable tables)
fn pursue_allowed(levels: &[String], tables: &[Json]) -> Vec<bool> {
    let mut keep = vec![true; levels.len()];
    for r in tables.iter().filter(|r| r.get("vars").and_then(strs).is_some_and(|v| v == ["pursue"])) {
        let has = |k: &str, l: &str| r.get(k).and_then(Json::as_arr).is_some_and(|ts| ts.iter().any(|t| t.as_arr().and_then(|t| t.first()).and_then(Json::as_str) == Some(l)));
        for (i, l) in levels.iter().enumerate() { if r.get("allow").is_some() { keep[i] &= has("allow", l); } else { keep[i] &= !has("forbid", l); } } }
    keep
}
/// The floor lift (§2.9): the smallest L >= 0 per floor goal s the habits allow (to 1e-6, plus a 1e-5 margin) with
/// exp(h_s + L_s + lo_s) / (exp(h_s + L_s + lo_s) + sum over allowed g != s of exp(h_g + L_g + hi_g)) >= floor_s, where lo / hi are
/// the least / most `pursue`'s couplings can add to a goal over every level of each partner. Sound when no multi-variable rule
/// mentions `pursue` (checked when the persona is read): then the bound is below the exact marginal.
fn floor_lifts(p: &Persona, d: &Drives, field: &[f64], allowed: &[bool]) -> Vec<f64> { lift_iter(p, d, field, field, allowed) }
/// The lift's iteration with goal s's own field read from `own` and every other goal's from `other`. The lift grows with the
/// others' fields and lifts and shrinks with the goal's own, so with `own` at a box's least and `other` at its most every round
/// bounds the turn's round from above (`prove`)
fn lift_iter(p: &Persona, d: &Drives, own: &[f64], other: &[f64], allowed: &[bool]) -> Vec<f64> {
    let n = own.len(); let (mut lo, mut hi) = (vec![0.0; n], vec![0.0; n]);
    for (a, b, tab) in &p.couplings {
        let rows: Vec<Vec<f64>> = if a == "pursue" { tab.clone() } else if b == "pursue" { (0..n).map(|i| tab.iter().map(|r| r[i]).collect()).collect() } else { continue };
        for (i, r) in rows.iter().enumerate() { lo[i] += r.iter().fold(f64::INFINITY, |x, y| x.min(*y)); hi[i] += r.iter().fold(f64::NEG_INFINITY, |x, y| x.max(*y)); } }
    let mut l = vec![0.0; n]; let idx: Vec<usize> = (0..n).filter(|&i| d.goals[i].floor > 0.0 && allowed[i]).collect();
    for _ in 0..60 { let mut moved = false;
        for &s in &idx { let f = d.goals[s].floor;
            // log-sum-exp over the other goals (shifted by their max): finite for every finite field. exp() of the raw fields
            // underflowed below about -745 nats (no lift: the floor goal fell to odds 0) and overflowed above about +709 (an
            // infinite lift: the turn fell back).
            let xs: Vec<f64> = (0..n).filter(|&g| g != s && allowed[g]).map(|g| other[g] + l[g] + hi[g]).collect(); if xs.is_empty() { continue; }
            let m = xs.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            let lse = m + xs.iter().map(|x| (x - m).exp()).sum::<f64>().ln();
            let need = (f / (1.0 - f)).ln() + lse - own[s] - lo[s] + 1e-5; let need = (need * 1e6).ceil() / 1e6;
            if need > l[s] + 1e-9 { l[s] = need; moved = true; } }
        if !moved { break; } }
    l.into_iter().map(r6).collect()
}

// ----------------------------------------------------------------------------------------------------- senses (§2.11)
/// A sense of the `senses` block (docs/persona.md §2.11): a reading the host measures, in declared units with a named source,
/// judged against a normal the individual learns from its own readings (mean, variance, count), anchored to a declared prior
#[derive(Clone)]
struct Sense { id: String, flag: bool, lo: f64, hi: f64, log1p: bool, unit: String, src: Vec<String>, prior_mean: f64, prior_var: f64, drift_cap: f64, var_cap: f64,
    half_life: f64, zcap: f64, dead_zone: f64, var_floor: f64, say: String, gain_spread: f64, threshold_spread: f64, habituation_spread: f64 }
/// A sense condition of a habit (`sense.<id>.raw` or `sense.<id>.z`), lowered to a flag input the sense step sets: true when the
/// sense was read this event and its raw reading (a flag: equal to `flag`; a number: within [lo, hi]) or its z is within [lo, hi]
#[derive(Clone)]
struct SenseCond { id: String, sense: usize, z: bool, flag: Option<bool>, lo: f64, hi: f64 }
/// The `senses` block. It lowers when the persona is read: per sense two number inputs `sense_<id>_up` and `sense_<id>_down` (the
/// z beyond the dead zone, each way, 0..zcap, carrying the effects and the effects negated) and one flag input per condition
/// (`sense_<id>_c<N>`). The normals are state.
#[derive(Clone)]
struct Senses { list: Vec<Sense>, conds: Vec<SenseCond>, synthetic: Vec<String> }
impl Senses {
    /// the sense whose `sense_<id>_up` / `sense_<id>_down` input this is
    fn of_input(&self, id: &str) -> Option<usize> { self.list.iter().position(|s| id.strip_prefix("sense_").and_then(|r| r.strip_prefix(s.id.as_str())).is_some_and(|r| r == "_up" || r == "_down")) }
    /// an id the block reserves: `sense_<id>_*` for every sense
    fn reserved(&self, id: &str) -> bool { reserved_by(&self.list, id) }
}
fn reserved_by(list: &[Sense], id: &str) -> bool { list.iter().any(|s| id.strip_prefix("sense_").and_then(|r| r.strip_prefix(s.id.as_str())).is_some_and(|r| r.starts_with('_'))) }
/// One sense of the block, read strictly (the lint rows of §2.11)
fn sense_spec(sj: &Json, path: &str) -> R<Sense> {
    const K: [&str; 19] = ["id", "kind", "unit", "range", "scale", "src", "prior", "drift_cap", "var_cap", "half_life_events", "zcap", "dead_zone", "var_floor", "say", "effects",
        "gain_spread", "threshold_spread", "habituation_spread", "comment"];
    let kv = keys(sj, path, &K, &["id", "kind", "unit", "prior", "drift_cap"])?;
    let id = ident(get(kv, "id"), &at(path, "id"), "id")?;
    need(id.len() <= 32, &at(path, "id"), "at most 32 characters (a sense reserves the input ids sense_<id>_*)")?;
    let flag = match get(kv, "kind").and_then(Json::as_str) { Some("flag") => true, Some("number") => false, _ => return Err(perr(&at(path, "kind"), "flag | number")) };
    let unit = get(kv, "unit").and_then(Json::as_str).filter(|u| (1..=24).contains(&u.chars().count())).ok_or_else(|| perr(&at(path, "unit"), "a unit of 1-24 characters"))?.to_string();
    let rp = at(path, "range");
    let (lo, hi) = if flag { need(get(kv, "range").is_none(), &rp, "a flag has no range (its normal is a rate)")?; (0.0, 1.0) } else {
        let a = get(kv, "range").and_then(Json::as_arr).filter(|a| a.len() == 2).ok_or_else(|| perr(&rp, "required for a number sense: [lo, hi]"))?;
        let (lo, hi) = (num(Some(&a[0]), &ix(&rp, 0), Some(-1e15), Some(1e15), false)?, num(Some(&a[1]), &ix(&rp, 1), Some(-1e15), Some(1e15), false)?);
        need(lo < hi, &rp, "lo < hi")?; (lo, hi) };
    let log1p = match get(kv, "scale") { None => false, Some(Json::Str(x)) if x == "linear" => false, Some(Json::Str(x)) if x == "log1p" => true, _ => return Err(perr(&at(path, "scale"), "linear | log1p")) };
    if log1p { need(!flag, &at(path, "scale"), "log1p needs a number sense (a flag's normal is a rate)")?; need(lo >= 0.0, &at(path, "scale"), "log1p needs a range low >= 0")?; }
    let sp = at(path, "src");
    let src = match get(kv, "src") { None => vec!["env".to_string()], Some(x) => { let v = strs(x).filter(|v| !v.is_empty()).ok_or_else(|| perr(&sp, "a list of source kinds: env, human"))?;
        for (i, k) in v.iter().enumerate() { let p = ix(&sp, i);
            need(k != "self", &p, "a reading never comes from the individual itself")?; need(k != "clock", &p, "the clock is not an instrument: readings come from env or human")?;
            need(k == "env" || k == "human", &p, "env | human")?; need(!v[..i].contains(k), &p, "listed twice")?; }
        v } };
    let six = |x: f64| r6(x) == x;
    let pp = at(path, "prior"); let pk = keys(get(kv, "prior").unwrap(), &pp, &["mean", "var", "comment"], &["mean", "var"])?;
    let (slo, shi) = if log1p { (lo.ln_1p(), hi.ln_1p()) } else { (lo, hi) };
    let prior_mean = num(get(pk, "mean"), &at(&pp, "mean"), None, None, false)?;
    need(prior_mean >= slo && prior_mean <= shi, &at(&pp, "mean"), &format!("within the scaled range [{}, {}]{}", pyn(r6(slo)), pyn(r6(shi)), if log1p { ", ln(1 + range)" } else { "" }))?;
    let prior_var = num(get(pk, "var"), &at(&pp, "var"), None, Some(1e12), false)?; need(prior_var > 0.0, &at(&pp, "var"), "must be > 0")?;
    need(six(prior_mean) && six(prior_var), &pp, "at most 6 decimals (the prior anchors both caps)")?;
    let drift_cap = num(get(kv, "drift_cap"), &at(path, "drift_cap"), None, Some(1e12), false)?; need(drift_cap > 0.0, &at(path, "drift_cap"), "must be > 0")?;
    need(six(drift_cap), &at(path, "drift_cap"), "a cap has at most 6 decimals")?;
    let var_cap = match get(kv, "var_cap") { None => r6(4.0 * prior_var), x => { let v = num(x, &at(path, "var_cap"), None, Some(1e12), false)?;
        need(v >= prior_var, &at(path, "var_cap"), "must be >= prior.var")?; need(six(v), &at(path, "var_cap"), "a cap has at most 6 decimals")?; v } };
    let half_life = num_or(kv, "half_life_events", 10.0, path, Some(1.0), Some(10000.0), false)?;
    let zcap = num_or(kv, "zcap", 6.0, path, Some(1.0), Some(20.0), false)?;
    let dead_zone = num_or(kv, "dead_zone", 0.5, path, Some(0.0), None, false)?; need(dead_zone < zcap, &at(path, "dead_zone"), "must be < zcap")?;
    let var_floor = num_or(kv, "var_floor", 0.01, path, Some(0.0), Some(1e12), false)?; need(var_floor > 0.0, &at(path, "var_floor"), "must be > 0")?;
    let say = match get(kv, "say") { None => id.replace('_', " "), Some(Json::Str(s)) => s.clone(), Some(_) => return Err(perr(&at(path, "say"), "a string")) };
    let spread = |k: &str| num_or(kv, k, 0.0, path, Some(0.0), Some(3.0), false);
    Ok(Sense { id, flag, lo, hi, log1p, unit, src, prior_mean, prior_var, drift_cap, var_cap, half_life, zcap, dead_zone, var_floor, say,
        gain_spread: spread("gain_spread")?, threshold_spread: spread("threshold_spread")?, habituation_spread: spread("habituation_spread")? })
}
/// A sense condition `key` (`sense.<id>.raw` or `sense.<id>.z`) with condition `c` -> (sense, z?, a flag's value, lo, hi)
fn parse_sense_cond(key: &str, c: &Json, path: &str, list: &[Sense]) -> R<(usize, bool, Option<bool>, f64, f64)> {
    let parts: Vec<&str> = key.split('.').collect();
    need(parts.len() == 3 && parts[0] == "sense", path, "a sense condition is sense.<id>.raw or sense.<id>.z")?;
    let si = list.iter().position(|s| s.id == parts[1]).ok_or_else(|| perr(path, format!("unknown sense {:?}", parts[1])))?;
    let z = match parts[2] { "raw" => false, "z" => true, _ => return Err(perr(path, "a rule reads sense.<id>.raw or sense.<id>.z, never the normal (mean, var, n)")) };
    if !z && list[si].flag { let Json::Bool(b) = c else { return Err(perr(path, "true | false (a flag sense's reading)")) }; return Ok((si, false, Some(*b), 0.0, 0.0)); }
    let (lo, hi) = match c { Json::Num(t) => (*t, f64::INFINITY), _ => {
        let kv = c.as_obj().filter(|kv| !kv.is_empty() && kv.iter().all(|(k, _)| k == "at_least" || k == "at_most")).ok_or_else(|| perr(path, "a number (at least) or {at_least, at_most}"))?;
        for (k, x) in kv { num(Some(x), &at(path, k), None, None, false)?; }
        (get(kv, "at_least").and_then(Json::as_f64).unwrap_or(f64::NEG_INFINITY), get(kv, "at_most").and_then(Json::as_f64).unwrap_or(f64::INFINITY)) } };
    Ok((si, z, None, lo, hi))
}
/// A sense condition as text: `sense.tool_fail.raw = true`, `sense.tempo.raw >= 20`, `sense.tempo.z in [1, 3]`
fn sense_cond_text(sn: &Senses, c: &SenseCond) -> String {
    let key = format!("sense.{}.{}", sn.list[c.sense].id, if c.z { "z" } else { "raw" });
    if let Some(b) = c.flag { return format!("{key} = {b}"); }
    match (c.lo.is_finite(), c.hi.is_finite()) { (true, false) => format!("{key} >= {}", pyn(c.lo)), (false, true) => format!("{key} <= {}", pyn(c.hi)),
        (true, true) => format!("{key} in [{}, {}]", pyn(c.lo), pyn(c.hi)), _ => key }
}
/// A persona with a `senses` block: the block read strictly, then lowered (see `Senses`) and read as a persona without it (a drives
/// block, if any, lowers next); the digest is the whole document's, the block included
fn build_senses(doc: &Json) -> R<Persona> {
    let kv = doc.as_obj().unwrap();
    let sl = match get(kv, "senses") { Some(Json::Arr(a)) if (1..=24).contains(&a.len()) => a, _ => return Err(perr("senses", "a list of 1 to 24 senses")) };
    let mut senses: Vec<Sense> = vec![];
    for (i, x) in sl.iter().enumerate() { let s = sense_spec(x, &ix("senses", i))?; need(!senses.iter().any(|y| y.id == s.id), &at(&ix("senses", i), "id"), "sense ids must be distinct")?; senses.push(s); }
    let mut d2: Vec<(String, Json)> = kv.iter().filter(|(k, _)| k != "senses").cloned().collect();
    let slot = |d2: &mut Vec<(String, Json)>, k: &str| -> usize { match d2.iter().position(|(x, _)| x == k) { Some(i) => i, None => { d2.push((k.into(), Json::Null)); d2.len() - 1 } } };
    // a taken sense_ id: the block reserves sense_<id>_* for every sense
    let mut ins: Vec<Json> = list(get(&d2, "inputs"), "inputs")?.to_vec(); let n_decl = ins.len();
    for (j, x) in ins.iter().enumerate() { if let Some(id) = x.get("id").and_then(Json::as_str) { need(!reserved_by(&senses, id), &format!("inputs[{j}].id"), "reserved by a sense (sense_<id>_*)")?; } }
    for (j, h) in list(get(&d2, "history"), "history")?.iter().enumerate() { for k in ["id", "of"] {
        if let Some(id) = h.get(k).and_then(Json::as_str) { need(!reserved_by(&senses, id), &format!("history[{j}].{k}"), "reserved by a sense (sense_<id>_*)")?; } } }
    if let Some(Json::Arr(f)) = get(&d2, "learning").and_then(|l| l.get("from")) { for (i, x) in f.iter().enumerate() {
        if let Some(k) = x.as_str() { need(!reserved_by(&senses, k), &format!("learning.from[{i}]"), "reserved for the senses block")?; } } }
    let s = |x: &str| Json::Str(x.to_string()); let mut origins: Vec<String> = vec![];
    for (i, sv) in senses.iter().enumerate() {
        let ep = format!("senses[{i}].effects");
        let eff: Vec<(String, Json)> = match sl[i].get("effects") { None => vec![], Some(x) if !truthy(x) => vec![],
            Some(x) => x.as_obj().ok_or_else(|| perr(&ep, "a mapping target -> number (an ordinal shift per unit of z)"))?.to_vec() };
        let mut neg = vec![]; for (k, v) in &eff { let Json::Num(w) = v else { return Err(perr(&at(&ep, k), "a number (an ordinal shift per unit of z)")) }; neg.push((k.clone(), Json::Num(-w))); }
        for (dir, e) in [("up", eff), ("down", neg)] {
            let mut spec = vec![("id".to_string(), s(&format!("sense_{}_{dir}", sv.id))), ("kind".into(), s("number")), ("max".into(), Json::Num(sv.zcap)), ("say".into(), s(&sv.say))];
            if !e.is_empty() { spec.push(("effects".into(), Json::Obj(e))); }
            ins.push(Json::Obj(spec)); origins.push(ep.clone()); } }
    // the habits' sense conditions -> one flag input per distinct (sense, raw or z, condition)
    let mut conds: Vec<SenseCond> = vec![];
    let mut habits: Vec<Json> = list(get(&d2, "habits"), "habits")?.to_vec();
    for (i, h) in habits.iter_mut().enumerate() {
        let Json::Obj(hkv) = h else { continue }; let Some((_, Json::Obj(w))) = hkv.iter_mut().find(|(k, _)| k == "when") else { continue };
        if !w.iter().any(|(k, _)| k.starts_with("sense.") || reserved_by(&senses, k)) { continue; }
        let mut nw: Vec<(String, Json)> = vec![];
        for (k, v) in w.iter() { let cp = format!("habits[{i}].when.{k}");
            need(!reserved_by(&senses, k), &cp, "reserved for the senses block (a rule reads sense.<id>.raw or sense.<id>.z)")?;
            if !k.starts_with("sense.") { nw.push((k.clone(), v.clone())); continue; }
            let (si, z, flag, lo, hi) = parse_sense_cond(k, v, &cp, &senses)?;
            let id = match conds.iter().find(|c| c.sense == si && c.z == z && c.flag == flag && c.lo == lo && c.hi == hi) { Some(c) => c.id.clone(), None => {
                let id = format!("sense_{}_c{}", senses[si].id, conds.iter().filter(|c| c.sense == si).count());
                ins.push(Json::Obj(vec![("id".into(), s(&id)), ("kind".into(), s("flag")), ("say".into(), s(k))])); origins.push(cp.clone());
                conds.push(SenseCond { id: id.clone(), sense: si, z, flag, lo, hi }); id } };
            if !nw.iter().any(|(x, _)| *x == id) { nw.push((id, Json::Bool(true))); } }
        *w = nw; }
    if !habits.is_empty() { let hi = slot(&mut d2, "habits"); d2[hi].1 = Json::Arr(habits); }
    let synthetic: Vec<String> = ins[n_decl..].iter().map(|x| x.get("id").and_then(Json::as_str).unwrap_or("").to_string()).collect();
    let ii = slot(&mut d2, "inputs"); d2[ii].1 = Json::Arr(ins);
    // an error in a synthetic input is reported at the senses path it comes from
    // a drives block lowers next, with the senses at hand (a need may grow per reading of a number sense, RFC 0002 A.1)
    let lowered = if get(&d2, "drives").is_some() { build_drives(&Json::Obj(d2), &senses) } else { build(&Json::Obj(d2)) };
    let mut p = lowered.map_err(|e| { for (j, o) in origins.iter().enumerate() { let pre = format!("inputs[{}]", n_decl + j);
        if let Some(rest) = e.path.strip_prefix(&pre).filter(|r| r.is_empty() || r.starts_with('.') || r.starts_with('[')) { return perr(&format!("{o}{}", rest.strip_prefix(".effects").unwrap_or("")), e.msg); } } e })?;
    p.digest = sha(doc);
    p.senses = Some(Senses { list: senses, conds, synthetic });
    Ok(p)
}
/// The sense genes (§2.11): a gain on the effects, an offset on the dead zone and a multiplier on the half-life per sense, from the
/// persona hash family (a function of name, seed and sense id); a spread of 0 stores no gene (gain 1, offset 0, multiplier 1)
fn sense_genes(name: &str, sn: &Senses, seed: u64) -> Json {
    let s = seed.to_string();
    let g = |f: &dyn Fn(&Sense) -> f64, k: &str, exp: bool| Json::Obj(sn.list.iter().filter(|x| f(x) > 0.0).map(|x| { let v = f(x) * normal(&[name, &s, "sense", &x.id, k]);
        (x.id.clone(), Json::Num(r6(round_to(if exp { v.exp() } else { v }, 4)))) }).collect());
    Json::Obj(vec![("gain".into(), g(&|x| x.gain_spread, "gain", true)), ("threshold".into(), g(&|x| x.threshold_spread, "threshold", false)),
        ("habituation".into(), g(&|x| x.habituation_spread, "habituation", true))])
}
/// A reading as this turn judged it: the reading and its source as given, z against the normal before the update, zd beyond the dead
/// zone, z0 against the prior, and the mean and count it was judged against (the sense step's output for the stance and `why`)
#[derive(Clone)]
struct SRead { raw: Json, src: Json, z: f64, zd: f64, z0: f64, mean: f64, n: u64 }
/// The sense state of an individual: genes, then per sense (declared order) the normal's mean, variance and count; `read` is this
/// turn's readings (not stored)
#[derive(Clone)]
struct SState { genes: Json, mean: Vec<f64>, var: Vec<f64>, n: Vec<u64>, read: Vec<Option<SRead>> }
impl SState {
    fn zero(name: &str, sn: &Senses, seed: u64) -> SState {
        let n = sn.list.len(); SState { genes: sense_genes(name, sn, seed), mean: sn.list.iter().map(|s| s.prior_mean).collect(), var: sn.list.iter().map(|s| s.prior_var).collect(), n: vec![0; n], read: vec![None; n] }
    }
    fn gene(&self, k: &str, id: &str) -> Option<f64> { self.genes.get(k).and_then(|o| o.get(id)).and_then(Json::as_f64) }
    fn to_json(&self, sn: &Senses) -> Json {
        Json::Obj(vec![("genes".into(), self.genes.clone()), ("norm".into(), Json::Obj(sn.list.iter().enumerate().map(|(i, s)| (s.id.clone(),
            Json::Obj(vec![("mean".into(), Json::Num(self.mean[i])), ("var".into(), Json::Num(self.var[i])), ("n".into(), Json::Num(self.n[i] as f64))]))).collect()))])
    }
    /// `state.senses`, strictly (S4): the genes of the seed, one normal per sense of the persona, each mean within prior ± drift_cap,
    /// each variance within [0, var_cap], each count a whole number up to 1e6
    fn read(name: &str, sn: &Senses, j: &Json, seed: u64) -> R<SState> {
        let kv = keys(j, "state.senses", &["genes", "norm"], &["genes", "norm"])?;
        need(canon(get(kv, "genes").unwrap()) == canon(&sense_genes(name, sn, seed)), "state.senses.genes", "sense genes do not match the seed")?;
        let ids: Vec<&str> = sn.list.iter().map(|s| s.id.as_str()).collect();
        let nk = get(kv, "norm").and_then(Json::as_obj).filter(|o| o.len() == sn.list.len() && ids.iter().all(|i| get(o, i).is_some()))
            .ok_or_else(|| perr("state.senses.norm", format!("one normal per sense of the persona ({})", ids.join(", "))))?;
        let n = sn.list.len(); let mut st = SState { genes: get(kv, "genes").unwrap().clone(), mean: vec![0.0; n], var: vec![0.0; n], n: vec![0; n], read: vec![None; n] };
        // the state keeps 6 decimals: a value clamped to a bound can round half a unit past it
        for (i, s) in sn.list.iter().enumerate() { let p = at("state.senses.norm", &s.id);
            let o = keys(get(nk, &s.id).unwrap(), &p, &["mean", "var", "n"], &["mean", "var", "n"])?;
            let (lo, hi) = (s.prior_mean - s.drift_cap, s.prior_mean + s.drift_cap);
            st.mean[i] = get(o, "mean").and_then(Json::as_f64).filter(|m| *m >= lo - 5e-7 && *m <= hi + 5e-7).ok_or_else(|| perr(&at(&p, "mean"), format!("within prior.mean ± drift_cap: [{}, {}]", pyn(r6(lo)), pyn(r6(hi)))))?;
            st.var[i] = get(o, "var").and_then(Json::as_f64).filter(|v| *v >= 0.0 && *v <= s.var_cap + 5e-7).ok_or_else(|| perr(&at(&p, "var"), format!("within [0, var_cap = {}]", pyn(s.var_cap))))?;
            st.n[i] = get(o, "n").and_then(Json::as_f64).filter(|x| *x >= 0.0 && x.fract() == 0.0 && *x <= 1e6).ok_or_else(|| perr(&at(&p, "n"), "a whole number from 0 to 1000000"))? as u64; }
        Ok(st)
    }
}
/// An event's `src` read with a senses block (on every event): a well-formed source or none
fn src_form(raw: &[(String, Json)]) -> R<()> {
    match get(raw, "src") { None => Ok(()), Some(Json::Str(s)) if src_kind(s).is_some() => Ok(()),
        Some(_) => Err(perr("inputs.src", "a source: human[:id] | env[:id] | self | clock (id: 1-64 of A-Z a-z 0-9 _ . - @ / :)")) }
}
/// Is a sense condition on, given this turn's readings? A condition on a sense not read this event is false (S5)
fn cond_on(c: &SenseCond, read: &[Option<SRead>]) -> bool {
    let Some(r) = &read[c.sense] else { return false };
    match c.flag { Some(b) => r.raw == Json::Bool(b), None => { let v = if c.z { r.z } else { r.raw.as_f64().unwrap_or(f64::NAN) }; v >= c.lo && v <= c.hi } }
}
/// The sense step (§2.11, RFC 0001 A.3), before the drive step: every reading checked first (S1 declared and in range, S2 an
/// accepted source; `check` false: fuzz and prove, whose events stand for accepted ones), then per declared sense with a reading,
/// in declared order, z against the normal before the update, zd beyond the dead zone, and the normal's update (each line rounded
/// to 6 decimals). -> (the stepped sense state, the turn's inputs: the event's without `senses`, then the lowering's inputs)
fn sense_step(sn: &Senses, ss: &SState, raw: &[(String, Json)], src: Option<&Json>, check: bool) -> R<(SState, Vec<(String, Json)>)> {
    for k in &sn.synthetic { need(get(raw, k).is_none(), &format!("inputs.{k}"), "reserved for the senses block (the engine computes it from the readings)")?; }
    let n = sn.list.len(); let mut got: Vec<Option<(Json, Json, f64)>> = vec![None; n];
    if let Some(sj) = get_t(raw, "senses") {
        let skv = sj.as_obj().ok_or_else(|| perr("inputs.senses", "a mapping sense id -> reading"))?;
        for (k, (sid, r)) in skv.iter().enumerate() { let path = format!("inputs.senses.{sid}");
            need(!skv[..k].iter().any(|(x, _)| x == sid), &path, "a sense read twice in one event")?;
            let i = sn.list.iter().position(|s| s.id == *sid).ok_or_else(|| perr(&path, format!("not a declared sense (senses: {})", sn.list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", "))))?;
            let s = &sn.list[i];
            let (v, own) = match r { Json::Obj(_) => { let o = keys(r, &path, &["v", "src"], &["v"])?; (get(o, "v").unwrap().clone(), get(o, "src").cloned()) } _ => (r.clone(), None) };
            let vp = if own.is_some() || matches!(r, Json::Obj(_)) { at(&path, "v") } else { path.clone() };
            // S1: declared, in range; nothing is clipped
            let x = if s.flag { match v { Json::Bool(b) => if b { 1.0 } else { 0.0 }, _ => return Err(perr(&vp, "a flag sense reads true | false")) } } else {
                let range = format!("a number in [{}, {}] ({})", pyn(s.lo), pyn(s.hi), s.unit);
                let y = match v { Json::Num(y) if y.is_finite() => y, _ => return Err(perr(&vp, range)) }; need(y >= s.lo && y <= s.hi, &vp, &range)?;
                if s.log1p { r6(y.ln_1p()) } else { r6(y) } };
            // S2: provenance; a bare value takes the event's src
            let sj2 = own.clone().or_else(|| src.cloned());
            if check { let sp = if own.is_some() { at(&path, "src") } else { "inputs.src".to_string() };
                let allowed = s.src.join(" or ");
                match &sj2 {
                    None => return Err(perr(&sp, format!("a reading of {sid} needs a source: the event's src or its own ({allowed})"))),
                    Some(Json::Str(t)) => match src_kind(t) {
                        None => return Err(perr(&sp, "a source: human[:id] | env[:id] (id: 1-64 of A-Z a-z 0-9 _ . - @ / :)")),
                        Some("self") => return Err(perr(&sp, format!("a reading of {sid} from the individual itself (src self) is refused: readings come from {allowed}"))),
                        Some("clock") => return Err(perr(&sp, format!("a reading of {sid} from the clock is refused: readings come from {allowed}"))),
                        Some(kd) if !s.src.iter().any(|a| a == kd) => return Err(perr(&sp, format!("a reading of {sid} from {kd} is refused: {sid} reads from {allowed}"))),
                        _ => {} },
                    Some(_) => return Err(perr(&sp, "a source: human[:id] | env[:id] (id: 1-64 of A-Z a-z 0-9 _ . - @ / :)")) } }
            got[i] = Some((v, sj2.unwrap_or(Json::Null), x)); } }
    let mut ns = ss.clone(); ns.read = vec![None; n];
    for (i, s) in sn.list.iter().enumerate() {
        let Some((v, sj, x)) = &got[i] else { continue }; let x = *x;
        let (gh, gt) = (ss.gene("habituation", &s.id).unwrap_or(1.0), ss.gene("threshold", &s.id).unwrap_or(0.0));
        let h = r6((s.half_life * gh).max(1.0).min(10000.0));
        let a = r6(1.0 - 0.5f64.powf(1.0 / h));
        let (mean, var) = (ss.mean[i], ss.var[i]);
        let z = r6(((x - mean) / (var + s.var_floor).sqrt()).max(-s.zcap).min(s.zcap));
        let d = r6((s.dead_zone + gt).max(0.0).min(s.zcap - 0.000001));
        let zd = if z == 0.0 { 0.0 } else { r6(z.signum() * (z.abs() - d).max(0.0)) };
        ns.mean[i] = r6((mean + a * (x - mean)).max(s.prior_mean - s.drift_cap).min(s.prior_mean + s.drift_cap));
        ns.var[i] = r6(((1.0 - a) * (var + a * (x - mean) * (x - mean))).max(0.0).min(s.var_cap));
        ns.n[i] = (ss.n[i] + 1).min(1_000_000);
        let z0 = r6(((x - s.prior_mean) / (s.prior_var + s.var_floor).sqrt()).max(-s.zcap).min(s.zcap));
        ns.read[i] = Some(SRead { raw: v.clone(), src: sj.clone(), z, zd, z0, mean, n: ss.n[i] }); }
    let mut out: Vec<(String, Json)> = raw.iter().filter(|(k, _)| k != "senses").cloned().collect();
    out.extend(lowered_inputs(sn, &ns.read));
    Ok((ns, out))
}
/// The lowering's inputs from a turn's readings: per sense `sense_<id>_up` = max(zd, 0) and `sense_<id>_down` = max(-zd, 0), then
/// every condition flag
fn lowered_inputs(sn: &Senses, read: &[Option<SRead>]) -> Vec<(String, Json)> {
    let mut out = vec![];
    for (i, s) in sn.list.iter().enumerate() { let zd = read[i].as_ref().map_or(0.0, |r| r.zd);
        out.push((format!("sense_{}_up", s.id), Json::Num(r6(zd.max(0.0))))); out.push((format!("sense_{}_down", s.id), Json::Num(r6((-zd).max(0.0))))); }
    for c in &sn.conds { out.push((c.id.clone(), Json::Bool(cond_on(c, read)))); }
    out
}
/// `n readings` (1 reading)
fn readings(n: u64) -> String { if n == 1 { "1 reading".into() } else { format!("{n} readings") } }
/// `why`'s normal clause (§2.11), at most one: the read sense with the largest |gain x zd| > 0 (ties: declared order), "far" at
/// |z| >= 2, above or below by sign; failing that, the read sense with the largest |z0| - |z| >= 1 (z0 against the prior), whose
/// reading its normal now makes usual. -> (the sense, the clause)
fn normal_clause(sn: &Senses, ss: &SState) -> Option<(usize, String)> {
    let mut best: Option<(usize, f64)> = None;
    for (i, r) in ss.read.iter().enumerate() { let Some(r) = r else { continue };
        let m = r6(ss.gene("gain", &sn.list[i].id).unwrap_or(1.0) * r.zd).abs(); if m > 0.0 && best.map_or(true, |b| m > b.1) { best = Some((i, m)); } }
    if let Some((i, _)) = best { let (r, s) = (ss.read[i].as_ref().unwrap(), &sn.list[i]);
        return Some((i, format!("{} {}{} its normal (z {:+.1} over {})", s.say, if r.z.abs() >= 2.0 { "far " } else { "" }, if r.z > 0.0 { "above" } else { "below" }, r.z, readings(r.n)))); }
    for (i, r) in ss.read.iter().enumerate() { let Some(r) = r else { continue };
        let g = r6(r.z0.abs() - r.z.abs()); if g >= 1.0 && best.map_or(true, |b| g > b.1) { best = Some((i, g)); } }
    best.map(|(i, _)| { let (r, s) = (ss.read[i].as_ref().unwrap(), &sn.list[i]);
        (i, format!("{} usual for it now (z {:+.1} over {}, {:+.1} against its prior)", s.say, r.z, readings(r.n), r.z0)) })
}
/// The stance document of a sensed turn: `senses` = per sense read its reading, source, z, zd and the mean and count it was judged
/// against; `noticed` = the normal clause's sense (or null); `inputs` = the host's (the readings as given, the lowering's left out)
fn stance_senses(j: &mut Json, sn: &Senses, ss: &SState, given: &[(String, Json)]) {
    let Json::Obj(v) = j else { return };
    if let Some((_, Json::Obj(ins))) = v.iter_mut().find(|(k, _)| k == "inputs") { ins.retain(|(k, _)| !sn.synthetic.contains(k));
        if let Some(g) = get_t(given, "senses") { ins.push(("senses".into(), g.clone())); } }
    let read = Json::Obj(sn.list.iter().zip(&ss.read).filter_map(|(s, r)| r.as_ref().map(|r| (s.id.clone(), Json::Obj(vec![("raw".into(), r.raw.clone()), ("src".into(), r.src.clone()),
        ("z".into(), Json::Num(r.z)), ("zd".into(), Json::Num(r.zd)), ("mean".into(), Json::Num(r.mean)), ("n".into(), Json::Num(r.n as f64))])))).collect());
    let noticed = normal_clause(sn, ss).map_or(Json::Null, |(i, _)| Json::Str(sn.list[i].id.clone()));
    v.push(("senses".into(), read)); v.push(("noticed".into(), noticed));
}
/// The lowering's inputs again, from a stance document's `senses` (what `breaks` reads)
fn doc_sense_inputs(sn: &Senses, doc: &Json) -> Option<Vec<(String, Json)>> {
    let ds = doc.get("senses")?; let mut read: Vec<Option<SRead>> = vec![];
    for s in &sn.list { read.push(match ds.get(&s.id) { None => None, Some(r) => Some(SRead { raw: r.get("raw")?.clone(), src: Json::Null, z: r.get("z")?.as_f64()?, zd: r.get("zd")?.as_f64()?,
        z0: 0.0, mean: 0.0, n: 0 }) }); }
    Some(lowered_inputs(sn, &read))
}
/// The gain an input's effects carry for this individual: a sense's gain gene for its `sense_<id>_up/down`, else the input's
/// reactivity gene (1 without one)
fn input_gain(p: &Persona, st: &State, id: &str) -> f64 {
    if let (Some(sn), Some(ss)) = (&p.senses, &st.senses) { if let Some(i) = sn.of_input(id) { return ss.gene("gain", &sn.list[i].id).unwrap_or(1.0); } }
    st.react(id).unwrap_or(1.0)
}

// ------------------------------------------------------------------------------------------------------------- rules
fn empty_rules() -> Vec<Vec<Json>> { vec![vec![]; RULE_KEYS.len()] }
/// Indices (in emission order: RULE_KEYS, then list order) of the rules a complete plan breaks
fn violations(plan: &HashMap<String, String>, rules: &[Vec<Json>], values: &[String]) -> Vec<usize> {
    let g = |x: Option<&Json>| x.and_then(Json::as_str).and_then(|k| plan.get(k)).map(String::as_str);
    let pos = |v: Option<&str>| v.and_then(|v| values.iter().position(|x| x == v)).map_or(-1.0, |p| p as f64);
    let lim = |r: &Json, k: &str| r.get(k).and_then(Json::as_f64);
    let (mut bad, mut idx) = (vec![], 0);
    for (ki, key) in RULE_KEYS.iter().enumerate() { for r in &rules[ki] {
        let broken = match *key {
            "caps" => { let n = r.get("members").and_then(Json::as_arr).map_or(0, |m| m.iter().filter(|m| m.as_arr().is_some_and(|m| g(m.first()).is_some() && g(m.first()) == m[1].as_str())).count());
                n as f64 > lim(r, "limit").unwrap_or(0.0) }
            "implies" => { let (i, t) = (r.get("if"), r.get("then"));
                g(i.and_then(|i| i.get("var"))).is_some() && g(i.and_then(|i| i.get("var"))) == i.and_then(|i| i.get("value")).and_then(Json::as_str)
                    && !t.and_then(|t| t.get("in")).and_then(Json::as_arr).is_some_and(|ins| ins.iter().any(|x| x.as_str().is_some() && x.as_str() == g(t.and_then(|t| t.get("var"))))) }
            "tables" => { let tup: Vec<Option<&str>> = r.get("vars").and_then(Json::as_arr).map_or(vec![], |v| v.iter().map(|x| g(Some(x))).collect());
                let has = |k: &str| r.get(k).and_then(Json::as_arr).is_some_and(|l| l.iter().any(|t| t.as_arr().is_some_and(|t| t.len() == tup.len() && t.iter().zip(&tup).all(|(a, b)| a.as_str().is_some() && a.as_str() == *b))));
                if r.get("forbid").is_some() { has("forbid") } else { !has("allow") } }
            "precedes" => pos(g(r.get("after"))) < pos(g(r.get("before"))) + lim(r, "gap").unwrap_or(1.0),
            "linear" => r.get("terms").and_then(Json::as_arr).map_or(0.0, |t| t.iter().filter_map(Json::as_arr).filter(|t| g(t.first()).is_some() && g(t.first()) == t[1].as_str()).map(|t| t[2].as_f64().unwrap_or(0.0)).fold(0.0, |a, b| a + b)) > lim(r, "limit").unwrap_or(0.0),
            _ => { let got: Vec<Option<&str>> = r.get("vars").and_then(Json::as_arr).map_or(vec![], |v| v.iter().map(|x| g(Some(x))).collect()); (1..got.len()).any(|i| got[..i].contains(&got[i])) }
        };
        if broken { bad.push(idx); } idx += 1; } }
    bad
}
fn resolve_level(spec: &str, levels: &[String], prev: &str) -> usize {
    if let Some(i) = levels.iter().position(|l| l == spec) { return i; }
    let i = levels.iter().position(|l| l == prev).unwrap_or(0) as i64;
    (i + match spec { "prev-1" => -1, "prev+1" => 1, _ => 0 }).clamp(0, levels.len() as i64 - 1) as usize
}
/// The levels a `then` restriction allows; `prev`, `prev-1` and `prev+1` read `pl`, the previous turn's level
fn allowed<'a>(levels: &'a [String], r: &Json, pl: &str) -> Vec<&'a String> {
    match r {
        Json::Arr(a) => levels.iter().filter(|l| a.iter().any(|x| x.as_str() == Some(l.as_str()))).collect(),
        _ => { let (rk, x) = &r.as_obj().unwrap()[0];
            match rk.as_str() {
                "not" => levels.iter().filter(|l| !x.as_arr().unwrap().iter().any(|y| y.as_str() == Some(l.as_str()))).collect(),
                "at_most" => levels[..=resolve_level(x.as_str().unwrap(), levels, pl)].iter().collect(),
                _ => levels[resolve_level(x.as_str().unwrap(), levels, pl)..].iter().collect() } } }
}
/// Habit h's IR rules (semantic level names) appended to `out` (lists in RULE_KEYS order) and its id to `owners` once per rule.
/// `prev` = the previous turn's levels (`prev`, `prev-1`, `prev+1` restrictions read them; a missing one = the fallback level).
fn habit_rules(p: &Persona, h: &Habit, prev: &HashMap<String, String>, out: &mut [Vec<Json>], owners: &mut Vec<(String, usize)>) {
    let ti = RULE_KEYS.iter().position(|k| *k == "tables").unwrap();
    for (k, r) in &h.then {
        let v = p.var(k).expect("then target"); let pl = prev.get(k).cloned().unwrap_or_else(|| v.fallback.clone());
        let keep = allowed(&v.levels, r, &pl);
        out[ti].push(Json::Obj(vec![("vars".into(), Json::Arr(vec![Json::Str(k.clone())])), ("allow".into(), Json::Arr(keep.into_iter().map(|l| Json::Arr(vec![Json::Str(l.clone())])).collect()))]));
        owners.push((h.id.clone(), ti));
    }
    for (ki, key) in RULE_KEYS.iter().enumerate() {
        for r in h.rules.iter().filter(|(k, _)| k == key).flat_map(|(_, l)| l) {
            let mut rr = r.clone();
            if let Json::Obj(v) = &mut rr { if *key == "precedes" && !v.iter().any(|(k, _)| k == "gap") { v.push(("gap".into(), Json::Num(1.0))); }
                if *key == "caps" && !v.iter().any(|(k, _)| k == "limit") { v.push(("limit".into(), Json::Num(0.0))); } }
            out[ki].push(rr); owners.push((h.id.clone(), ki)); }
    }
}
/// The owners in emission order (RULE_KEYS, then list order), as `violations` counts
fn ordered_owners(owners: Vec<(String, usize)>) -> Vec<String> { (0..RULE_KEYS.len()).flat_map(|k| owners.iter().filter(move |o| o.1 == k).map(|o| o.0.clone())).collect() }
/// The value alphabet with level names (semantic): every level in order of first appearance, then the agenda slots
fn values_of(p: &Persona) -> Vec<String> {
    let mut vals: Vec<String> = vec![]; for v in &p.vars { for l in &v.levels { if !vals.contains(l) { vals.push(l.clone()); } } }
    let extra: Vec<String> = p.slots.iter().filter(|s| !vals.contains(s)).cloned().collect(); vals.extend(extra); vals
}

// ----------------------------------------------------------------------------------------------------------- the state
/// The state between turns (docs/persona.md §5.1): plain JSON a host stores.
#[derive(Clone)]
pub struct State { pub seed: u64, pub turn: u64, persona: [String; 3], genes: Json, mood: Vec<(String, Vec<f64>)>, history: Vec<(String, Vec<bool>)>, prev: HashMap<String, String>,
    agenda: Option<Vec<String>>, rest: Option<Vec<(String, String)>>, learned: Table, credit: Table, drives: Option<DState>, senses: Option<SState>,
    learned_from: Option<Vec<(String, u64, u64)>>, open: Vec<Open>, prev_stance: Option<String>, control_closed: Vec<u64>, elig: f64, pub digest: String }
/// A decision awaiting its outcome (§2.8, `open_credit`): its turn, its credit (onehot(level) − odds per learned trait it released)
/// and its stance's digest
#[derive(Clone)]
struct Open { turn: u64, credit: Table, stance: String }
/// `learned_from` (§2.11): per learned trait the feedback turns that moved it, (rewards, corrections), with a learning block and
/// `line.why_past` on (None otherwise)
fn learned_from0(p: &Persona) -> Option<Vec<(String, u64, u64)>> {
    p.learning.as_ref().filter(|_| p.why_past()).map(|l| l.traits.iter().map(|t| (t.clone(), 0, 0)).collect())
}
fn table(t: &Table) -> Json { Json::Obj(t.iter().map(|(k, v)| (k.clone(), Json::Arr(v.iter().map(|x| Json::Num(*x)).collect()))).collect()) }
/// A zero table over the learned traits (none without a learning block)
fn zeros(p: &Persona) -> Table { p.learning.as_ref().map_or(vec![], |l| l.traits.iter().map(|t| (t.clone(), vec![0.0; p.var(t).unwrap().levels.len()])).collect()) }
/// The learner (docs/persona.md §2.8). `sign` is this turn's feedback (+1 reward, -1 correction, 0 none: nothing moves); `credit`
/// is the paid stance's onehot(level) - odds per learned trait (the previous stance's without a credit block). The step sign ·
/// rate · credit is clipped to ±step_cap per level, scaled by the payment's eligibility `e` (1 for the previous stance, so the
/// step is 0.8's) and recentred to sum 0 (it moves weight between levels; zero credit moves nothing), added, and every delta is
/// clipped to ±total_cap. It returns nothing but the delta table, which compile adds to trait unaries: habits and allowed levels
/// are not reachable from here, so learning can never remove a rule.
fn learn(l: &Learning, credit: &Table, sign: f64, e: f64, delta: &Table) -> Table {
    if sign == 0.0 { return delta.clone(); }
    delta.iter().zip(credit).map(|((t, d), (_, g))| {
        let step: Vec<f64> = g.iter().map(|x| e * (sign * l.rate * x).clamp(-l.step_cap, l.step_cap)).collect();
        let mean = step.iter().fold(0.0, |a, b| a + b) / step.len() as f64;
        (t.clone(), d.iter().zip(&step).map(|(a, x)| r6((a + (x - mean)).clamp(-l.total_cap, l.total_cap))).collect()) }).collect()
}
fn genes(p: &Persona, seed: u64) -> Json {
    let s = seed.to_string();
    let shift = p.vars.iter().filter(|v| v.spread > 0.0).map(|v| (v.id.clone(), Json::Num(r6(round_to(v.spread * normal(&[&p.name, &s, "shift", &v.id]), 4))))).collect();
    let react = p.inputs.iter().filter(|x| x.react > 0.0).map(|x| (x.id.clone(), Json::Num(r6(round_to((x.react * normal(&[&p.name, &s, "react", &x.id])).exp(), 4))))).collect();
    Json::Obj(vec![("shift".into(), Json::Obj(shift)), ("react".into(), Json::Obj(react))])
}
impl State {
    fn shift(&self, id: &str) -> Option<f64> { self.genes.get("shift").and_then(|s| s.get(id)).and_then(Json::as_f64) }
    fn react(&self, id: &str) -> Option<f64> { self.genes.get("react").and_then(|s| s.get(id)).and_then(Json::as_f64) }
    fn history_of(&self, of: &str) -> &[bool] { self.history.iter().find(|(k, _)| k == of).map_or(&[], |(_, v)| v) }
    /// The state document without its digest
    fn body(&self, p: &Persona) -> Json {
        let mut prev: Vec<(String, Json)> = p.vars.iter().filter_map(|v| self.prev.get(&v.id).map(|l| (v.id.clone(), Json::Str(l.clone())))).collect();
        if let Some(a) = &self.agenda { prev.push(("agenda".into(), Json::Arr(a.iter().map(|s| Json::Str(s.clone())).collect()))); }
        let mut v = vec![("probbit_persona_state".to_string(), Json::Num(1.0)),
            ("persona".into(), Json::Obj(vec![("name".into(), Json::Str(self.persona[0].clone())), ("version".into(), Json::Str(self.persona[1].clone())), ("digest".into(), Json::Str(self.persona[2].clone()))])),
            ("seed".into(), Json::Num(self.seed as f64)), ("turn".into(), Json::Num(self.turn as f64)), ("genes".into(), self.genes.clone()),
            ("mood".into(), Json::Obj(self.mood.iter().map(|(k, m)| (k.clone(), Json::Arr(m.iter().map(|x| Json::Num(*x)).collect()))).collect())),
            ("history".into(), Json::Obj(self.history.iter().map(|(k, h)| (k.clone(), Json::Arr(h.iter().map(|b| Json::Bool(*b)).collect()))).collect())), ("prev".into(), Json::Obj(prev))];
        if let Some(r) = &self.rest { v.push(("rest".into(), Json::Obj(r.iter().map(|(k, l)| (k.clone(), Json::Str(l.clone()))).collect()))); }
        if p.learning.is_some() { v.push(("learned".into(), table(&self.learned))); v.push(("credit".into(), table(&self.credit))); }
        if let (Some(d), Some(ds)) = (&p.drives, &self.drives) { v.push(("drives".into(), ds.to_json(d)));
            // RFC 0002 A.3: the needs state, with a needs block
            if let (Some(nb), Some(nd)) = (&d.needs, &ds.nd) { v.push(("needs".into(), nd.to_json(d, nb))); } }
        if let (Some(sn), Some(ss)) = (&p.senses, &self.senses) { v.push(("senses".into(), ss.to_json(sn))); }
        if let Some(lf) = &self.learned_from { v.push(("learned_from".into(), Json::Obj(lf.iter().map(|(t, r, c)| (t.clone(),
            Json::Obj(vec![("rewards".into(), Json::Num(*r as f64)), ("corrections".into(), Json::Num(*c as f64))]))).collect()))); }
        // a credit block (§2.8): the decisions awaiting an outcome, the previous stance's digest, the decisions a control line closed
        if p.credit().is_some() {
            v.push(("open_credit".into(), Json::Arr(self.open.iter().map(|o| Json::Obj(vec![("turn".into(), Json::Num(o.turn as f64)), ("credit".into(), table(&o.credit)),
                ("stance".into(), Json::Str(o.stance.clone()))])).collect())));
            v.push(("prev_stance".into(), self.prev_stance.as_ref().map_or(Json::Null, |s| Json::Str(s.clone()))));
            v.push(("control_closed".into(), Json::Arr(self.control_closed.iter().map(|t| Json::Num(*t as f64)).collect()))); }
        Json::Obj(v)
    }
    pub fn to_json(&self, p: &Persona) -> Json { let mut b = self.body(p); if let Json::Obj(v) = &mut b { v.push(("digest".into(), Json::Str(self.digest.clone()))); } b }
    fn seal(&mut self, p: &Persona) { self.digest = sha(&self.body(p)); }
    /// This state with every credit 0 and (a credit block, §2.8) every open decision closed: a control line's (§5.7), feedback
    /// after it credits no stance before it; the next stance lists the closed decisions (as `control`). The same state when there
    /// is nothing to clear (no learning block, or a stance that released no learned trait, and no decision open)
    pub fn zero_credit(&self, p: &Persona) -> State {
        if self.credit.iter().all(|(_, c)| c.iter().all(|x| *x == 0.0)) && self.open.is_empty() { return self.clone(); }
        let mut s = self.clone(); for (_, c) in s.credit.iter_mut() { c.iter_mut().for_each(|x| *x = 0.0); }
        let closed: Vec<u64> = s.open.drain(..).map(|o| o.turn).collect(); s.control_closed.extend(closed); s.seal(p); s
    }
    /// A state document -> State, for this persona: the format, the persona digest, the state's own digest and the genes are
    /// checked (in that order), then every field's type (a well-formed state never fails those).
    pub fn read(p: &Persona, j: &Json) -> R<State> {
        let kv = j.as_obj().ok_or_else(|| perr("state", "must be a JSON object"))?;
        need(matches!(get(kv, "probbit_persona_state"), Some(Json::Num(x)) if *x == 1.0), "state", "not a probbit persona state (format 1)")?;
        let pd = get(kv, "persona").and_then(|x| x.get("digest")).and_then(Json::as_str).ok_or_else(|| perr("state.persona.digest", "missing"))?;
        if pd != p.digest { return Err(perr("state.persona.digest", format!("the persona file changed since this state was made ({} != {}); re-init", &pd[..pd.len().min(19)], &p.digest[..19]))); }
        let body = Json::Obj(kv.iter().filter(|(k, _)| k != "digest").cloned().collect());
        need(get(kv, "digest").and_then(Json::as_str) == Some(sha(&body).as_str()), "state.digest", "state was edited or corrupted")?;
        need(p.drives.is_none() || get(kv, "drives").is_some(), "state.drives", "required with a drives block")?;
        need(p.senses.is_none() || get(kv, "senses").is_some(), "state.senses", "required with a senses block")?;
        let keeps_from = learned_from0(p).is_some();
        need(!keeps_from || get(kv, "learned_from").is_some(), "state.learned_from", "required with a learning block and line.why_past")?;
        const SK: [&str; 19] = ["probbit_persona_state", "persona", "seed", "turn", "genes", "mood", "history", "prev", "rest", "digest", "learned", "credit", "drives", "senses", "learned_from",
            "open_credit", "prev_stance", "control_closed", "needs"];
        let mut sk: Vec<&str> = SK[..10].to_vec();
        if p.learning.is_some() { sk.extend(&SK[10..12]); }
        if p.drives.is_some() { sk.push(SK[12]); }
        if p.senses.is_some() { sk.push(SK[13]); }
        if keeps_from { sk.push(SK[14]); }
        if p.credit().is_some() { sk.extend(&SK[15..18]); for k in &SK[15..18] { need(get(kv, k).is_some(), &at("state", k), "required with a credit block")?; } }
        if p.has_needs() { sk.push(SK[18]); need(get(kv, "needs").is_some(), "state.needs", "required with a needs block")?; }
        keys(j, "state", &sk, &["persona", "seed", "turn", "genes", "mood", "history", "prev", "digest"])
            .map_err(|e| if e.msg.starts_with("unknown field") && (e.path == "state.learned" || e.path == "state.credit") { perr(&e.path, "this persona has no learning block") }
                else if e.msg.starts_with("unknown field") && e.path == "state.drives" { perr(&e.path, "this persona has no drives block") }
                else if e.msg.starts_with("unknown field") && e.path == "state.senses" { perr(&e.path, "this persona has no senses block") }
                else if e.msg.starts_with("unknown field") && ["state.open_credit", "state.prev_stance", "state.control_closed"].contains(&e.path.as_str()) { perr(&e.path, "this persona has no credit block (learning.credit)") }
                else if e.msg.starts_with("unknown field") && e.path == "state.learned_from" { perr(&e.path, "this persona keeps no learned_from (a learning block with line.why_past)") }
                else if e.msg.starts_with("unknown field") && e.path == "state.needs" { perr(&e.path, "this persona has no needs block") } else { e })?;
        let whole = |k: &str| num(get(kv, k), &at("state", k), Some(0.0), Some(9_007_199_254_740_992.0), true).map(|x| x as u64);
        let (seed, turn) = (whole("seed")?, whole("turn")?);
        let g = get(kv, "genes").unwrap(); need(canon(g) == canon(&genes(p, seed)), "state.genes", "genes do not match the seed")?;
        let pk = keys(get(kv, "persona").unwrap(), "state.persona", &["name", "version", "digest"], &["name", "version", "digest"])?;
        let persona = ["name", "version", "digest"].map(|k| get(pk, k).and_then(Json::as_str).unwrap_or("").to_string());
        let mut mood = vec![]; let mk = get(kv, "mood").and_then(Json::as_obj).ok_or_else(|| perr("state.mood", "must be an object"))?;
        for v in p.vars.iter().filter(|v| v.mood) { let mp = at("state.mood", &v.id);
            let a = get(mk, &v.id).and_then(Json::as_arr).filter(|a| a.len() == v.levels.len() && a.iter().all(|x| x.as_f64().is_some())).ok_or_else(|| perr(&mp, "one number per level"))?;
            mood.push((v.id.clone(), a.iter().map(|x| x.as_f64().unwrap()).collect())); }
        need(mk.len() == mood.len(), "state.mood", "one accumulator per mood of the persona")?;
        let mut history = vec![]; let hk = get(kv, "history").and_then(Json::as_obj).ok_or_else(|| perr("state.history", "must be an object"))?;
        for (k, h) in hk { need(p.history.iter().any(|x| x.of == *k), &at("state.history", k), "not a flag a history feature reads")?;
            let b: Option<Vec<bool>> = h.as_arr().and_then(|a| a.iter().map(|x| if let Json::Bool(b) = x { Some(*b) } else { None }).collect());
            history.push((k.clone(), b.ok_or_else(|| perr(&at("state.history", k), "a list of true / false"))?)); }
        let level_map = |x: &Json, path: &str| -> R<Vec<(String, String)>> { let o = x.as_obj().ok_or_else(|| perr(path, "must be an object"))?; let mut out = vec![];
            for (k, l) in o { if k == "agenda" && path == "state.prev" { continue; }
                let v = p.var(k).ok_or_else(|| perr(&at(path, k), "not a trait or mood of the persona"))?;
                let l = l.as_str().filter(|l| v.levels.iter().any(|x| x == l)).ok_or_else(|| perr(&at(path, k), "not a level of it"))?; out.push((k.clone(), l.to_string())); }
            Ok(out) };
        let pv = get(kv, "prev").unwrap(); let prev: HashMap<String, String> = level_map(pv, "state.prev")?.into_iter().collect();
        let agenda = match pv.get("agenda") { None => None, Some(a) => { let a = strs(a).filter(|a| { let mut s = a.clone(); s.sort(); let mut t = p.steps.clone(); t.sort(); s == t }).ok_or_else(|| perr("state.prev.agenda", "the persona's agenda steps"))?; Some(a) } };
        let rest = match get(kv, "rest") { None => None, Some(r) => Some(level_map(r, "state.rest")?) };
        // learned deltas within ±total_cap and credits within ±1, one number per level of each learned trait: the box `prove` covers
        let tab = |k: &str, cap: f64| -> R<Table> { let Some(l) = &p.learning else { return Ok(vec![]) }; let path = at("state", k);
            let o = get(kv, k).and_then(Json::as_obj).ok_or_else(|| perr(&path, "required: an object of learned traits"))?;
            need(o.len() == l.traits.len(), &path, "one entry per learned trait")?;
            l.traits.iter().map(|t| { let n = p.var(t).unwrap().levels.len(); let tp = at(&path, t);
                let a = get(o, t).and_then(Json::as_arr).filter(|a| a.len() == n && a.iter().all(|x| x.as_f64().is_some_and(|x| x.abs() <= cap + 5e-7))).ok_or_else(|| perr(&tp, format!("one number per level, each within ±{}", pyn(cap))))?;
                Ok((t.clone(), a.iter().map(|x| x.as_f64().unwrap()).collect())) }).collect() };
        let (learned, credit) = (tab("learned", p.learning.as_ref().map_or(0.0, |l| l.total_cap))?, tab("credit", 1.0)?);
        let drives = match &p.drives { None => None, Some(d) => { let mut ds = DState::read(p, d, get(kv, "drives").unwrap(), seed)?;
            if let Some(nb) = &d.needs { ds.nd = Some(NState::read(p, d, nb, get(kv, "needs").unwrap(), seed)?); } Some(ds) } };
        let senses = match &p.senses { None => None, Some(sn) => Some(SState::read(&p.name, sn, get(kv, "senses").unwrap(), seed)?) };
        // learned_from: per learned trait {rewards, corrections}, whole numbers >= 0
        let learned_from = match learned_from0(p) { None => None, Some(z) => { let o = get(kv, "learned_from").and_then(Json::as_obj).filter(|o| o.len() == z.len())
                .ok_or_else(|| perr("state.learned_from", "one entry per learned trait"))?;
            Some(z.iter().map(|(t, _, _)| { let tp = at("state.learned_from", t); let e = keys(get(o, t).ok_or_else(|| perr(&tp, "required"))?, &tp, &["rewards", "corrections"], &["rewards", "corrections"])?;
                let w = |k: &str| num(get(e, k), &at(&tp, k), Some(0.0), Some(9_007_199_254_740_992.0), true).map(|x| x as u64);
                Ok((t.clone(), w("rewards")?, w("corrections")?)) }).collect::<R<Vec<_>>>()?) } };
        // a credit block (§2.8): at most max_open open decisions, turns increasing and before this state's turn, credits within ±1, a
        // stance digest each; the previous stance's digest (null before any turn); the decisions a control line closed
        let (mut open, mut prev_stance, mut control_closed) = (vec![], None, vec![]);
        if let (Some(c), Some(l)) = (p.credit(), &p.learning) {
            let turn_at = |x: &Json, path: &str, after: Option<u64>| -> R<u64> { let n = num(Some(x), path, Some(0.0), Some(9_007_199_254_740_992.0), true)? as u64;
                need(n < turn, path, "a turn before this state's turn")?; need(after.map_or(true, |a| n > a), path, "turns increasing")?; Ok(n) };
            let oa = get(kv, "open_credit").and_then(Json::as_arr).filter(|a| a.len() <= c.max_open).ok_or_else(|| perr("state.open_credit", format!("a list of at most {} open decisions (max_open)", c.max_open)))?;
            for (i, o) in oa.iter().enumerate() { let op = ix("state.open_credit", i); let ok = keys(o, &op, &["turn", "credit", "stance"], &["turn", "credit", "stance"])?;
                let n = turn_at(get(ok, "turn").unwrap(), &at(&op, "turn"), open.last().map(|x: &Open| x.turn))?;
                let cp = at(&op, "credit"); let co = get(ok, "credit").and_then(Json::as_obj).filter(|o| o.len() == l.traits.len()).ok_or_else(|| perr(&cp, "one entry per learned trait"))?;
                let credit = l.traits.iter().map(|t| { let lv = p.var(t).unwrap().levels.len();
                    let a = get(co, t).and_then(Json::as_arr).filter(|a| a.len() == lv && a.iter().all(|x| x.as_f64().is_some_and(|x| x.abs() <= 1.0 + 5e-7))).ok_or_else(|| perr(&at(&cp, t), "one number per level, each within ±1"))?;
                    Ok((t.clone(), a.iter().map(|x| x.as_f64().unwrap()).collect())) }).collect::<R<Table>>()?;
                let stance = get(ok, "stance").and_then(Json::as_str).filter(|s| is_digest(s)).ok_or_else(|| perr(&at(&op, "stance"), "a stance digest (sha256:<64 hex>)"))?.to_string();
                open.push(Open { turn: n, credit, stance }); }
            prev_stance = match get(kv, "prev_stance").unwrap() { Json::Null if turn == 0 => None, Json::Str(s) if turn > 0 && is_digest(s) => Some(s.clone()),
                _ => return Err(perr("state.prev_stance", "the previous stance's digest (sha256:<64 hex>); null before any turn")) };
            let ca = get(kv, "control_closed").and_then(Json::as_arr).filter(|a| a.len() <= c.max_open).ok_or_else(|| perr("state.control_closed", format!("a list of at most {} turns", c.max_open)))?;
            for (i, x) in ca.iter().enumerate() { let n = turn_at(x, &ix("state.control_closed", i), control_closed.last().copied())?; control_closed.push(n); } }
        Ok(State { seed, turn, persona, genes: g.clone(), mood, history, prev, agenda, rest, learned, credit, drives, senses, learned_from, open, prev_stance, control_closed, elig: 1.0,
            digest: get(kv, "digest").and_then(Json::as_str).unwrap_or("").to_string() })
    }
}

// -------------------------------------------------------------------------------------------------------- the engine
/// One engine call's flags (`probbit run --op OP --seed S --sweeps N --polish-sweeps M --exact-limit X --chains C`)
pub struct Flags { pub op: String, pub seed: u64, pub sweeps: usize, pub polish_sweeps: usize, pub exact_limit: u64, pub chains: usize }
/// The engine: a program and its flags -> the answer document `probbit run` prints (or `{"verdict": "error", "reason"}`)
pub type Engine<'a> = &'a dyn Fn(&Json, &Flags) -> Json;
/// `probbit run`'s instruction on a program, in process: `run::from_json` on the document (no text round trip), the CLI's
/// defaults for every flag the persona does not set (collective, cluster, cycles on; frontier states 4096), the resource controls
/// given (they never change an answer at fixed work), and the numeric contract of `probbit run` (main.rs `finish`). The polish is
/// `polish_sweeps` sweeps (fixed work); 0 = no polish (never the wall-clock one: a turn stays a pure function of its inputs).
pub fn run_program(prog: &Json, f: &Flags, threads: usize, cpu_pct: u32, mem_mb: usize) -> Json {
    let edoc = |r: String| Json::Obj(vec![("verdict".into(), Json::Str("error".into())), ("reason".into(), Json::Str(r))]);
    let mut p = match crate::run::from_json(prog) { Ok(p) => p, Err(e) => return edoc(format!("{} at {}: {}", e.code, e.path, e.msg)) };
    p.m.collective = true; p.m.cluster = true; p.m.cycles = true;
    let rows = if mem_mb == 0 { 0 } else { f.chains.checked_mul(2 * p.m.n + 8).map_or(64, |d| (mem_mb.saturating_mul(1_048_576) / d).max(64)) };
    let (doc, code) = crate::run::run(&p, &f.op, 200.0, f.seed, f.exact_limit, 0.0, f.polish_sweeps, f.sweeps, probbit_ir::FRONTIER_MAX_STATES, f.chains, threads, cpu_pct, (mem_mb, rows), None, None, &mut vec![]);
    let mut bad = vec![]; json::non_finite(&doc, "", &mut bad);
    if !bad.is_empty() && !(code == 3 && bad.iter().all(|b| b.starts_with("gate."))) { return edoc(format!("numeric at {}: {} computed quantities not finite", bad[0], bad.len())); }
    doc
}
fn flags(p: &Persona, st: &State) -> Flags {
    Flags { op: p.eng.op.clone(), seed: turn_seed(&p.name, st.seed, st.turn), sweeps: p.eng.sweeps, polish_sweeps: p.eng.polish_sweeps, exact_limit: p.eng.exact_limit, chains: p.eng.chains }
}

// --------------------------------------------------------------------------------------------------------- compile
#[derive(Clone)]
struct Meta { vals: Vec<(String, Json)>, hist: Vec<(String, f64)>, ignored: Vec<String>, active: Vec<String>, conditional: Vec<String>, active_inputs: Vec<String>,
    rules: Vec<Vec<Json>>, owners: Vec<String>, new_mood: Vec<(String, Vec<f64>)>, new_learned: Table, contrib: Vec<Vec<(String, Vec<f64>)>>, field: Vec<Vec<f64>>, values: Vec<String>, twin: bool,
    lift: Vec<(String, f64)>, learn_sign: f64 }
#[derive(Default, Clone)]
struct Opts { no_inertia: bool, clamps: Vec<(String, String)>, twin: Option<bool>, exclude: Vec<String>, hist: Option<Vec<(String, f64)>> }

/// Inputs -> (values per declared input, history features, elapsed hours, ignored ids); a bad value is a persona error
#[allow(clippy::type_complexity)]
fn resolve_inputs(p: &Persona, st: &State, raw: &[(String, Json)]) -> R<(Vec<(String, Json)>, Vec<(String, f64)>, f64, Vec<String>)> {
    let mut ignored: Vec<String> = raw.iter().map(|(k, _)| k.clone()).filter(|k| p.input(k).is_none() && !RESERVED_INPUTS.contains(&k.as_str())).collect(); ignored.sort();
    let mut vals = vec![];
    for x in &p.inputs { let path = format!("inputs.{}", x.id); let v = get(raw, &x.id).cloned().unwrap_or_else(|| x.default.clone());
        let v = match x.kind {
            Kind::Flag => { need(matches!(v, Json::Bool(_)), &path, "true | false")?; v }
            Kind::Level => { need(v.as_str().is_some_and(|s| x.levels.iter().any(|l| l == s)), &path, &format!("one of {}", x.levels.join(", ")))?; v }
            Kind::Number => Json::Num(num(Some(&v), &path, Some(x.min), Some(x.max), false)?) };
        vals.push((x.id.clone(), v)); }
    let elapsed = num(Some(get(raw, "elapsed_hours").unwrap_or(&Json::Num(0.0))), "inputs.elapsed_hours", Some(0.0), None, false)?;
    let mut hist = vec![];
    for h in &p.history { let mut seq = st.history_of(&h.of).to_vec(); seq.push(matches!(get(&vals, &h.of), Some(Json::Bool(true))));
        let n = if h.streak { seq.iter().rev().take_while(|&&b| b).count() } else { seq[seq.len().saturating_sub(h.window)..].iter().filter(|&&b| b).count() };
        hist.push((h.id.clone(), n.min(h.cap) as f64)); }
    Ok((vals, hist, elapsed, ignored))
}
fn cond_true(p: &Persona, k: &str, c: &Json, vals: &[(String, Json)], hist: &[(String, f64)]) -> bool {
    let v = get(vals, k).cloned().unwrap_or_else(|| Json::Num(hist.iter().find(|(h, _)| h == k).map_or(0.0, |x| x.1)));
    match p.input(k).map(|x| x.kind) {
        Some(Kind::Flag) => v == *c,
        Some(Kind::Level) => match c { Json::Str(s) => v.as_str() == Some(s), Json::Arr(a) => a.iter().any(|x| x.as_str().is_some() && x.as_str() == v.as_str()), _ => false },
        _ => { let x = v.as_f64().unwrap_or(0.0);
            match c { Json::Num(t) => x >= *t, _ => x >= c.get("at_least").and_then(Json::as_f64).unwrap_or(f64::NEG_INFINITY) && x <= c.get("at_most").and_then(Json::as_f64).unwrap_or(f64::INFINITY) } } }
}
fn jstrs(v: &[String]) -> Json { Json::Arr(v.iter().map(|s| Json::Str(s.clone())).collect()) }

/// persona + state + inputs -> (the turn's probbit-ir program, meta). A pure function; every weight rounded to 6 decimals.
fn compile(p: &Persona, st: &State, raw: &[(String, Json)], o: &Opts) -> R<(Json, Meta)> {
    let (vals, mut hist, elapsed, ignored) = resolve_inputs(p, st, raw)?;
    if let Some(h) = &o.hist { hist = h.clone(); } // `prove`: history feature values given, not derived from the state
    let (nv, ns) = (p.vars.len(), p.steps.len());
    let tix = |t: &str| -> usize { match t.strip_prefix("step.") { Some(s) => nv + p.steps.iter().position(|x| x == s).unwrap(), None => p.vars.iter().position(|v| v.id == t).unwrap() } };
    let mut contrib: Vec<Vec<(String, Vec<f64>)>> = vec![vec![]; nv + ns];
    let mut mood_ev: Vec<Option<Vec<f64>>> = p.vars.iter().map(|v| v.mood.then(|| vec![0.0; v.levels.len()])).collect();
    // the learned deltas after this turn's feedback (the reward flag +1, the correction flag -1) on the previous stance's credit
    // with a drives block (§2.9): the turn's drive context, read from the (stepped) drive state; its surprise joins the learning sign
    let dctx = match (&p.drives, &st.drives) { (Some(d), Some(ds)) => Some(drive_ctx(d, ds)), _ => None };
    let mut learn_sign = 0.0;
    let new_learned = match &p.learning { None => vec![], Some(l) => {
        let sign = l.from.iter().enumerate().fold(0.0, |s, (i, id)| if matches!(get(&vals, id), Some(Json::Bool(true))) { s + if i == 0 { 1.0 } else { -1.0 } } else { s });
        let sign = match &dctx { Some(c) => (sign + c.learn_sign).clamp(-1.0, 1.0), None => sign }; learn_sign = sign;
        learn(l, &st.credit, sign, st.elig, &st.learned) } };
    for (i, v) in p.vars.iter().enumerate() { contrib[i].push(("prior".into(), v.base.clone()));
        if let Some(g) = st.shift(&v.id) { contrib[i].push(("genes".into(), centered(v.levels.len()).iter().map(|c| g * c).collect())); }
        if let Some((_, d)) = new_learned.iter().find(|(t, d)| *t == v.id && d.iter().any(|x| *x != 0.0)) { contrib[i].push(("learned".into(), d.clone())); } }
    for (j, s) in p.steps.iter().enumerate() { contrib[nv + j].push(("prior".into(), vec![0.0; ns]));
        if let Some((_, pf)) = p.prefer.iter().find(|(t, _)| *t == format!("step.{s}")) { contrib[nv + j].push(("prefer".into(), pf.clone())); } }
    let mut active_inputs = vec![];
    let mut push = |t: &str, src: String, w: Vec<f64>, contrib: &mut Vec<Vec<(String, Vec<f64>)>>| { let i = tix(t);
        match mood_ev.get_mut(i).and_then(Option::as_mut) { Some(ev) => for (a, b) in ev.iter_mut().zip(&w) { *a += b; }, None => contrib[i].push((src, w)) } };
    for x in &p.inputs { let v = get(&vals, &x.id).unwrap(); let g = input_gain(p, st, &x.id);
        let (eff, scale): (&[(String, Vec<f64>)], f64) = match x.kind {
            Kind::Flag => if *v == Json::Bool(true) { (&x.effects, 1.0) } else { (&[], 0.0) },
            Kind::Number => (&x.effects, v.as_f64().unwrap()),
            Kind::Level => (x.by_level.iter().find(|(l, _)| Some(l.as_str()) == v.as_str()).map_or(&[][..], |(_, e)| &e[..]), 1.0) };
        if !eff.is_empty() && scale != 0.0 { active_inputs.push(x.id.clone()); }
        for (t, vec) in eff { push(t, format!("input:{}", x.id), vec.iter().map(|y| g * scale * y).collect(), &mut contrib); } }
    for h in &p.history { let n = hist.iter().find(|(k, _)| *k == h.id).unwrap().1.min(h.cap as f64);
        if n != 0.0 && !h.effects.is_empty() { active_inputs.push(h.id.clone()); }
        for (t, vec) in &h.effects { push(t, format!("history:{}", h.id), vec.iter().map(|y| n * y).collect(), &mut contrib); } }
    let mut new_mood = vec![];
    for (i, v) in p.vars.iter().enumerate().filter(|(_, v)| v.mood) {
        let k = if o.no_inertia { 0.0 } else { v.inertia };
        let decay = match v.half_life { Some(hl) if elapsed > 0.0 => 0.5f64.powf(elapsed / hl), _ => 1.0 };
        let prev = &st.mood.iter().find(|(id, _)| *id == v.id).unwrap().1;
        let m: Vec<f64> = prev.iter().zip(mood_ev[i].as_ref().unwrap()).map(|(a, e)| r6(k * decay * a + (1.0 - k) * e)).collect();
        if m.iter().any(|&x| x != 0.0) { contrib[i].push(("mood_accumulator".into(), m.clone())); }
        new_mood.push((v.id.clone(), m)); }
    // habits in force and their rules
    let active: Vec<&Habit> = p.habits.iter().filter(|h| !o.exclude.contains(&h.id) && h.when.iter().all(|(k, c)| cond_true(p, k, c, &vals, &hist))).collect();
    let (mut rules, mut owners) = (empty_rules(), vec![]);
    let mut prev = st.prev.clone(); for v in &p.vars { prev.entry(v.id.clone()).or_insert_with(|| v.fallback.clone()); }
    for h in &active { habit_rules(p, h, &prev, &mut rules, &mut owners); }
    let owners = ordered_owners(owners);
    // the program: values, vars (traits and moods, then the agenda steps), pairs, rules; positional = level i -> l<i>
    let pos_vals = p.eng.positional; let sem = values_of(p);
    let lmax = p.vars.iter().map(|v| v.levels.len()).max().unwrap_or(0);
    let values: Vec<String> = if pos_vals { (0..lmax).map(|i| format!("l{i}")).chain(p.slots.iter().cloned()).collect() } else { sem.clone() };
    let name_of = |var: &str, val: &str| -> String { let id = var.strip_prefix("free.").unwrap_or(var);
        match p.var(id) { Some(v) if pos_vals => format!("l{}", v.levels.iter().position(|l| l == val).unwrap_or(0)), _ => val.to_string() } };
    // `pursue`'s drive terms after its prior, learned, input and history contributions: genes, wanting, afterglow, deadline (§3 step 2b)
    let pix = dctx.as_ref().and_then(|_| p.vars.iter().position(|v| v.id == "pursue"));
    if let (Some(c), Some(i)) = (&dctx, pix) { for (src, vec) in &c.terms { if vec.iter().any(|x| *x != 0.0) { contrib[i].push((src.to_string(), vec.clone())); } } }
    let total = |c: &[(String, Vec<f64>)], n: usize| -> Vec<f64> { let mut tot = vec![0.0; n]; for (_, vec) in c { for (a, b) in tot.iter_mut().zip(vec) { *a += b; } } tot.into_iter().map(r6).collect() };
    let mut field: Vec<Vec<f64>> = (0..nv + ns).map(|i| total(&contrib[i], if i < nv { p.vars[i].levels.len() } else { ns })).collect();
    // the floor lift (§2.9): on the rounded field, over the levels the habits in force allow; then the field is summed again
    let mut lift = vec![];
    if let (Some(d), Some(i)) = (&p.drives, pix) { if d.goals.iter().any(|g| g.floor > 0.0) {
        let ti = RULE_KEYS.iter().position(|k| *k == "tables").unwrap();
        let l = floor_lifts(p, d, &field[i], &pursue_allowed(&p.vars[i].levels, &rules[ti]));
        if l.iter().any(|x| *x != 0.0) { contrib[i].push(("floor".into(), l.clone())); field[i] = total(&contrib[i], l.len()); }
        lift = d.goals.iter().zip(&l).filter(|(_, x)| **x != 0.0).map(|(g, x)| (g.id.clone(), *x)).collect(); } }
    let mut pv = vec![];
    for (i, v) in p.vars.iter().enumerate() {
        let mut var = vec![("id".to_string(), Json::Str(v.id.clone())), ("allowed".into(), Json::Arr(v.levels.iter().map(|l| Json::Str(name_of(&v.id, l))).collect())),
            ("h".into(), Json::Obj(v.levels.iter().zip(&field[i]).filter(|(_, w)| **w != 0.0).map(|(l, w)| (name_of(&v.id, l), Json::Num(*w))).collect()))];
        if let Some((_, c)) = o.clamps.iter().find(|(t, _)| *t == v.id) { var.push(("clamp".into(), Json::Str(name_of(&v.id, c)))); }
        pv.push(Json::Obj(var)); }
    for (j, s) in p.steps.iter().enumerate() {
        pv.push(Json::Obj(vec![("id".into(), Json::Str(format!("step.{s}"))), ("allowed".into(), jstrs(&p.slots)),
            ("h".into(), Json::Obj(p.slots.iter().zip(&field[nv + j]).filter(|(_, w)| **w != 0.0).map(|(l, w)| (l.clone(), Json::Num(*w))).collect()))])); }
    let k = values.len(); let vpos = |v: &str| values.iter().position(|x| x == v).unwrap();
    let mut pairs = vec![];
    for (a, b, tab) in &p.couplings {
        let (la, lb) = (&p.var(a).unwrap().levels, &p.var(b).unwrap().levels); let mut t = vec![vec![0.0; k]; k];
        for (x, l1) in la.iter().enumerate() { for (y, l2) in lb.iter().enumerate() { let w = r6(tab[x][y]); if w != 0.0 || !pos_vals { t[vpos(&name_of(a, l1))][vpos(&name_of(b, l2))] = w; } } }
        pairs.push((a.clone(), b.clone(), t)); }
    let pair_json = |i: &str, j: &str, t: &[Vec<f64>]| Json::Obj(vec![("i".into(), Json::Str(i.into())), ("j".into(), Json::Str(j.into())),
        ("table".into(), Json::Arr(t.iter().map(|r| Json::Arr(r.iter().map(|x| Json::Num(*x)).collect())).collect()))]);
    let comment = format!("probbit persona {} {} seed {} turn {}; active habits: {}", p.name, p.version, st.seed, st.turn,
        if active.is_empty() { "none".to_string() } else { active.iter().map(|h| h.id.as_str()).collect::<Vec<_>>().join(", ") });
    let use_twin = o.twin.unwrap_or(p.eng.twin);
    let mut twin_vars = vec![];
    if use_twin { for v in &pv { let Json::Obj(f) = v else { continue }; twin_vars.push(Json::Obj(f.iter().filter(|(k, _)| k != "clamp")
        .map(|(k, x)| if k == "id" { (k.clone(), Json::Str(format!("free.{}", x.as_str().unwrap()))) } else { (k.clone(), x.clone()) }).collect())); } }
    pv.extend(twin_vars);
    let mut prog = vec![("probbit_ir".to_string(), Json::Num(1.0)), ("comment".into(), Json::Str(comment)), ("values".into(), jstrs(&values)), ("vars".into(), Json::Arr(pv))];
    if !pairs.is_empty() { let mut pj: Vec<Json> = pairs.iter().map(|(a, b, t)| pair_json(a, b, t)).collect();
        if use_twin { pj.extend(pairs.iter().map(|(a, b, t)| pair_json(&format!("free.{a}"), &format!("free.{b}"), t))); }
        prog.push(("pairs".into(), Json::Arr(pj))); }
    for (ki, key) in RULE_KEYS.iter().enumerate() {
        let mut lst: Vec<Json> = vec![];
        if *key == "all_different" && ns > 0 { lst.push(Json::Obj(vec![("vars".into(), Json::Arr(p.steps.iter().map(|s| Json::Str(format!("step.{s}"))).collect()))])); }
        lst.extend(rules[ki].iter().map(|r| rename(r, key, &name_of)));
        if *key == "all_different" && ns > 0 && use_twin { lst.push(Json::Obj(vec![("vars".into(), Json::Arr(p.steps.iter().map(|s| Json::Str(format!("free.step.{s}"))).collect()))])); }
        if !lst.is_empty() { prog.push((key.to_string(), Json::Arr(lst))); } }
    let conditional = active.iter().filter(|h| !h.when.is_empty()).map(|h| h.id.clone()).collect();
    let meta = Meta { vals, hist, ignored, active: active.iter().map(|h| h.id.clone()).collect(), conditional, active_inputs, rules, owners, new_mood, new_learned, contrib, field, values: sem, twin: use_twin, lift, learn_sign };
    Ok((Json::Obj(prog), meta))
}
/// A habit rule with its level names renamed to the program's values (positional: l<i>)
fn rename(r: &Json, key: &str, name_of: &dyn Fn(&str, &str) -> String) -> Json {
    let Json::Obj(v) = r else { return r.clone() };
    let s = |x: &Json| x.as_str().unwrap_or("").to_string();
    let pair = |m: &Json| -> Json { match m.as_arr() { Some([a, b]) => Json::Arr(vec![a.clone(), Json::Str(name_of(&s(a), &s(b)))]), _ => m.clone() } };
    Json::Obj(v.iter().map(|(k, x)| { let y = match (key, k.as_str()) {
        ("caps", "members") => Json::Arr(x.as_arr().unwrap_or(&[]).iter().map(pair).collect()),
        ("implies", "if") => { let var = s(x.get("var").unwrap_or(&Json::Null)); Json::Obj(x.as_obj().unwrap_or(&[]).iter().map(|(a, b)| (a.clone(), if a == "value" { Json::Str(name_of(&var, &s(b))) } else { b.clone() })).collect()) }
        ("implies", "then") => { let var = s(x.get("var").unwrap_or(&Json::Null)); Json::Obj(x.as_obj().unwrap_or(&[]).iter().map(|(a, b)| (a.clone(), if a == "in" { Json::Arr(b.as_arr().unwrap_or(&[]).iter().map(|y| Json::Str(name_of(&var, &s(y)))).collect()) } else { b.clone() })).collect()) }
        ("tables", "forbid" | "allow") => { let vars: Vec<String> = r.get("vars").and_then(Json::as_arr).map_or(vec![], |a| a.iter().map(s).collect());
            Json::Arr(x.as_arr().unwrap_or(&[]).iter().map(|t| Json::Arr(t.as_arr().unwrap_or(&[]).iter().zip(&vars).map(|(y, var)| Json::Str(name_of(var, &s(y)))).collect())).collect()) }
        ("linear", "terms") => Json::Arr(x.as_arr().unwrap_or(&[]).iter().map(|t| match t.as_arr() { Some([a, b, w]) => Json::Arr(vec![a.clone(), Json::Str(name_of(&s(a), &s(b))), w.clone()]), _ => t.clone() }).collect()),
        _ => x.clone() }; (k.clone(), y) }).collect())
}

// ---------------------------------------------------------------------------------------------------------- decode
struct Entry { level: String, p: f64, odds: Vec<(String, f64)>, released: bool, unheld: Option<Vec<(String, f64)>> }
/// A stance document under construction (docs/persona.md §4)
struct Out { status: String, verdict: Json, tier: Json, program: String, stance: Vec<(String, Entry)>, mood: Vec<(String, Entry)>, active: Vec<String>, bound: Vec<String>,
    violations: usize, conflict: Vec<String>, yielded: Vec<String>, unsure: Vec<String>, escalate: Option<String>, inputs: Vec<(String, Json)>, ignored: Vec<String>,
    line: String, why: String, agenda: Option<Vec<String>>, state_digest: String, held: Vec<String>, timing: Option<Json>, lift: Vec<(String, f64)> }
impl Out {
    fn to_json(&self, p: &Persona, st: &State) -> Json {
        let e = |m: &[(String, Entry)]| Json::Obj(m.iter().map(|(k, e)| { let odds = |o: &[(String, f64)]| Json::Obj(o.iter().map(|(l, x)| (l.clone(), Json::Num(*x))).collect());
            let mut f = vec![("level".to_string(), Json::Str(e.level.clone())), ("p".into(), Json::Num(e.p)), ("odds".into(), odds(&e.odds)), ("released".into(), Json::Bool(e.released))];
            if let Some(u) = &e.unheld { f.push(("odds_unheld".into(), odds(u))); } (k.clone(), Json::Obj(f)) }).collect());
        let s = |x: &str| Json::Str(x.to_string());
        let mut v = vec![("probbit_persona_turn".to_string(), Json::Num(1.0)), ("persona".into(), Json::Obj(vec![("name".into(), s(&p.name)), ("version".into(), s(&p.version)), ("digest".into(), s(&p.digest))])),
            ("seed".into(), Json::Num(st.seed as f64)), ("turn".into(), Json::Num(st.turn as f64)), ("status".into(), s(&self.status)),
            ("engine".into(), Json::Obj(vec![("verdict".into(), self.verdict.clone()), ("tier".into(), self.tier.clone()), ("program".into(), s(&self.program))])),
            ("stance".into(), e(&self.stance)), ("mood".into(), e(&self.mood)),
            ("habits".into(), Json::Obj(vec![("active".into(), jstrs(&self.active)), ("bound".into(), jstrs(&self.bound)), ("violations".into(), Json::Num(self.violations as f64)),
                ("conflict".into(), jstrs(&self.conflict)), ("yielded".into(), jstrs(&self.yielded))])),
            ("unsure".into(), jstrs(&self.unsure)), ("held".into(), jstrs(&self.held)), ("escalate".into(), self.escalate.as_deref().map_or(Json::Null, s)),
            ("inputs".into(), Json::Obj(self.inputs.clone())), ("ignored".into(), jstrs(&self.ignored)), ("line".into(), s(&self.line)),
            ("line_tokens".into(), Json::Num(est_tokens(&self.line) as f64)), ("why".into(), s(&self.why)), ("state_digest".into(), s(&self.state_digest))];
        if let Some(a) = &self.agenda { v.push(("agenda".into(), jstrs(a))); }
        if let Some(t) = &self.timing { v.push(("timing".into(), t.clone())); }
        Json::Obj(v)
    }
}
/// Python's `max(range(n), key=...)`: the first index with the largest key
fn argmax(n: usize, key: impl Fn(usize) -> f64) -> usize { (0..n).fold(0, |b, i| if key(i) > key(b) { i } else { b }) }
/// The individual's resting stance per trait: the state's `rest` (a no-evidence turn's joint plan, computed at init), else the
/// argmax of prior + genes
fn baseline(p: &Persona, st: &State) -> HashMap<String, String> {
    if let Some(r) = st.rest.as_ref().filter(|r| !r.is_empty()) { return r.iter().cloned().collect(); }
    p.vars.iter().map(|v| { let g = st.shift(&v.id).unwrap_or(0.0); let w: Vec<f64> = v.base.iter().zip(centered(v.levels.len())).map(|(b, c)| b + g * c).collect();
        (v.id.clone(), v.levels[argmax(w.len(), |i| round_to(w[i], 9))].clone()) }).collect()
}
/// The stance line: phrases by priority (0 bound habits, 1 triggered habits, 2 unsure notes, 3 traits off their resting level,
/// 4 traits at rest, 5 agenda order), cut from the lowest priority until it fits line.max_tokens (estimated)
#[allow(clippy::too_many_arguments)]
fn line(p: &Persona, stance: &HashMap<String, String>, active: &[String], bound: &[String], unsure: &[String], order_steps: &[String], vouched: &HashMap<String, bool>, conditional: &[String], base: &HashMap<String, String>) -> String {
    let mut items: Vec<(u8, usize, String)> = vec![];
    for (i, h) in bound.iter().enumerate() { let s = &p.habit(h).say; if !s.is_empty() { items.push((0, i, s.clone())); } }
    for (i, h) in active.iter().enumerate() { let s = &p.habit(h).say; if !bound.contains(h) && conditional.contains(h) && !s.is_empty() { items.push((1, i, s.clone())); } }
    for (i, t) in unsure.iter().enumerate() { let v = p.var(t).unwrap(); items.push((2, i, format!("unsure about {t}: {}", v.hold.as_ref().map_or("ask".into(), |h| format!("keep it {h}"))))); }
    for (pos, t) in p.order.iter().enumerate() {
        let Some(l) = stance.get(t) else { continue }; if !vouched.get(t).copied().unwrap_or(false) || unsure.contains(t) { continue; }
        let v = p.var(t).unwrap(); let s = &v.say[v.levels.iter().position(|x| x == l).unwrap()];
        if !s.is_empty() { items.push((if Some(l) != base.get(t) { 3 } else { 4 }, pos, s.clone())); } }
    if !order_steps.is_empty() && p.say_order { items.push((5, 0, format!("order: {}", order_steps.join(" > ")))); }
    let habit_text: Vec<String> = items.iter().filter(|it| it.0 <= 1).map(|it| it.2.clone()).collect();
    items.retain(|it| it.0 < 3 || it.0 > 4 || !habit_text.iter().any(|h| h.contains(&it.2)));
    items.sort_by_key(|it| (it.0, it.1));
    let mut keep: Vec<String> = vec![]; for it in items { if !keep.contains(&it.2) { keep.push(it.2); } }
    loop { let text = format!("{}{}", p.prefix, if keep.is_empty() { "neutral.".to_string() } else { format!("{}.", keep.join("; ")) });
        if est_tokens(&text) <= p.max_tokens || keep.is_empty() { return text; } keep.pop(); }
}
/// An input's push on one variable in words: the direction ("humour down"), not a level name (the push can point at a level the
/// stance does not take)
fn direction_word(p: &Persona, var: &str, vec: &[f64]) -> Option<(String, f64)> {
    p.var(var)?; let d = vec.iter().zip(centered(vec.len())).fold(0.0, |a, (x, c)| a + x * c);
    (d.abs() >= 0.15).then(|| (format!("{var} {}", if d > 0.0 { "up" } else { "down" }), d.abs()))
}
/// `why` (§4): the bound habits; with a senses block the normal clause (§2.11); the two strongest live inputs with the two traits
/// each pushes hardest (the senses' own inputs left out); with `line.why_past` and a learning block the learned clause; the engine
/// status if not ok
fn why(p: &Persona, st: &State, meta: &Meta, bound: &[String], status: &str, stance: &[(String, Entry)], learned_from: Option<&[(String, u64, u64)]>) -> String {
    let mut parts: Vec<String> = p.habits.iter().filter(|h| bound.contains(&h.id)).map(|h| format!("{} (habit)", if h.say.is_empty() { h.id.replace('_', " ") } else { h.say.clone() })).collect();
    if let (Some(sn), Some(ss)) = (&p.senses, &st.senses) { if let Some((_, c)) = normal_clause(sn, ss) { parts.push(c); } }
    if let (Some(d), Some(ds)) = (&p.drives, &st.drives) {
        // RFC 0002 A.6: at most one need clause, for the pursued goal when a need binds it and its term is the largest positive one
        if let (Some(nb), Some(nd), Some((_, e))) = (&d.needs, &ds.nd, stance.iter().find(|(k, _)| k == "pursue").filter(|(_, e)| e.released)) {
            let t = need_terms(d, ds); let best = (0..t.len()).filter(|&j| t[j].1 > 0.0).fold(None, |b: Option<usize>, j| if b.map_or(true, |b| t[j].1 > t[b].1) { Some(j) } else { b });
            if let Some(j) = best.filter(|&j| d.goals[nb.items[j].bind].id == e.level) {
                parts.push(format!("{} deficit {}, above its setpoint", nb.items[j].say, pyn(round_to(nd.deficit[j], 2)))); } }
        // A.4: the omission clause when a window fired this event
        let fired: Vec<&str> = d.goals.iter().zip(&ds.omit).filter(|(_, o)| o.is_some()).map(|(g, _)| g.say.as_str()).collect();
        if !fired.is_empty() { parts.push(format!("an expected win did not come ({})", fired.join(", "))); } }
    let mut srcs: Vec<(f64, String)> = vec![];
    let ids: Vec<&String> = p.inputs.iter().map(|x| &x.id).chain(p.history.iter().map(|h| &h.id)).collect();
    for id in ids { if !meta.active_inputs.contains(id) { continue; }
        if p.senses.as_ref().is_some_and(|sn| sn.synthetic.contains(id)) { continue; }
        if p.has_needs() && id.starts_with("need_") && p.drives.as_ref().is_some_and(|d| d.synthetic.contains(id)) { continue; } // the need clause says it
        let (eff, scale, say): (&[(String, Vec<f64>)], f64, String) = if let Some(x) = p.input(id) {
            let v = get(&meta.vals, id).unwrap();
            let eff = if x.kind == Kind::Level { x.by_level.iter().find(|(l, _)| Some(l.as_str()) == v.as_str()).map_or(&[][..], |(_, e)| &e[..]) } else { &x.effects[..] };
            let scale = if x.kind == Kind::Number { v.as_f64().unwrap() } else { 1.0 };
            let mut say = match &x.say { Json::Obj(m) => v.as_str().and_then(|l| get(m, l)).and_then(Json::as_str).unwrap_or(id).to_string(), s => s.as_str().unwrap_or(id).to_string() };
            if x.kind == Kind::Number { say = format!("{say} {:.1}", v.as_f64().unwrap()); }
            (eff, scale, say)
        } else { let h = p.history.iter().find(|h| h.id == *id).unwrap(); let n = meta.hist.iter().find(|(k, _)| k == id).unwrap().1;
            (&h.effects[..], n.min(h.cap as f64), format!("{} {}", h.say, n as i64)) };
        let mut words: Vec<(String, f64)> = eff.iter().filter_map(|(t, vec)| direction_word(p, t, &vec.iter().map(|y| scale * y).collect::<Vec<_>>())).collect();
        words.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let tot = words.iter().fold(0.0, |a, w| a + w.1);
        if !words.is_empty() { srcs.push((tot, format!("{say} -> {}", words.iter().take(2).map(|w| w.0.as_str()).collect::<Vec<_>>().join(", ")))); } }
    srcs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    parts.extend(srcs.into_iter().take(2).map(|s| s.1));
    // the learned clause (§2.11): the released learned trait with the largest |Δ| at its level, if >= 0.05, and what moved it
    if let (Some(l), Some(lf), true) = (&p.learning, learned_from, p.why_past()) {
        let mut best: Option<(f64, String)> = None; let n = |k: u64, w: &str| if k == 1 { format!("1 {w}") } else { format!("{k} {w}s") };
        for t in &l.traits { let Some((_, e)) = stance.iter().find(|(k, _)| k == t).filter(|(_, e)| e.released) else { continue };
            let Some(li) = p.var(t).and_then(|v| v.levels.iter().position(|x| *x == e.level)) else { continue };
            let d = meta.new_learned.iter().find(|(k, _)| k == t).map_or(0.0, |(_, d)| d[li]);
            if d.abs() >= 0.05 && best.as_ref().map_or(true, |b| d.abs() > b.0) { let (_, r, c) = lf.iter().find(|(k, _, _)| k == t).cloned().unwrap_or_default();
                best = Some((d.abs(), format!("learned: {t} {d:+.2} on {} ({}, {})", e.level, n(r, "reward"), n(c, "correction")))); } }
        if let Some((_, c)) = best { parts.push(c); } }
    if status != "ok" { parts.push(format!("engine: {status}")); }
    if parts.is_empty() { "baseline (no live evidence)".into() } else { parts.join("; ") }
}
/// The engine's value back to a level name (positional l<i> -> the level)
fn back(p: &Persona, var: &str, val: &str) -> String {
    let id = var.strip_prefix("free.").unwrap_or(var);
    match (p.eng.positional, p.var(id), val.strip_prefix('l').and_then(|d| d.parse::<usize>().ok())) { (true, Some(v), Some(i)) if i < v.levels.len() && val[1..].bytes().all(|b| b.is_ascii_digit()) => v.levels[i].clone(), _ => val.to_string() }
}
/// An engine probability as the prototype read it: `probbit run`'s printed number (rounded to 6 decimals, json.rs `write`), then r6
fn printed(x: f64) -> f64 { r6(if x.fract() == 0.0 { x } else { (x * 1e6).round() / 1e6 }) }

/// The engine's answer -> (stance document, new state)
fn decode(p: &Persona, st: &State, program: &str, meta: &Meta, doc: &Json, conflict: Option<&[String]>, yielded: Option<&[String]>) -> (Out, State) {
    let verdict = doc.get("verdict").cloned().unwrap_or(Json::Null); let vs = verdict.as_str().unwrap_or("");
    let plan: HashMap<String, String> = doc.get("plan").and_then(Json::as_obj).map_or_else(HashMap::new, |o| o.iter().map(|(k, v)| (k.clone(), back(p, k, v.as_str().unwrap_or("")))).collect());
    let released: Vec<&str> = doc.get("released").and_then(Json::as_arr).map_or(vec![], |a| a.iter().filter_map(Json::as_str).collect());
    let mut status = match vs { "exact" | "diagnostics_passed" => "ok", "partial" => "partial", _ => "refused" }.to_string();
    let has_plan = !plan.is_empty() && p.vars.iter().all(|v| plan.contains_key(&v.id));
    let (mut stance, mut mood, mut vouched) = (vec![], vec![], HashMap::new());
    for v in &p.vars {
        let lvl = if has_plan { plan[&v.id].clone() } else { v.fallback.clone() };
        let odds: Vec<(String, f64)> = if has_plan { let m = doc.get("marginals").and_then(|m| m.get(&v.id)).and_then(Json::as_obj);
            v.levels.iter().map(|l| (l.clone(), m.and_then(|m| m.iter().find(|(k, _)| back(p, &v.id, k) == *l)).and_then(|(_, x)| x.as_f64()).map_or(0.0, printed))).collect() }
            else { v.levels.iter().map(|l| (l.clone(), if *l == v.fallback { 1.0 } else { 0.0 })).collect() };
        let rel = has_plan && (vs == "exact" || released.contains(&v.id.as_str()) || vs == "diagnostics_passed"); vouched.insert(v.id.clone(), rel);
        let e = Entry { p: odds.iter().find(|(l, _)| *l == lvl).map_or(0.0, |x| x.1), level: lvl, odds, released: rel, unheld: None };
        if v.mood { mood.push((v.id.clone(), e)); } else { stance.push((v.id.clone(), e)); } }
    let (mut order_steps, mut steps_vouched) = (vec![], false);
    if !p.steps.is_empty() {
        if has_plan { order_steps = p.steps.clone(); order_steps.sort_by_key(|s| plan.get(&format!("step.{s}")).and_then(|x| p.slots.iter().position(|y| y == x)).unwrap_or(usize::MAX));
            steps_vouched = vs == "exact" || vs == "diagnostics_passed" || p.steps.iter().all(|s| released.contains(&format!("step.{s}").as_str())); }
        else { order_steps = p.steps.clone(); } }
    if !has_plan { status = "fallback".into(); }
    let unsure: Vec<String> = stance.iter().filter(|(t, e)| { let v = p.var(t).unwrap(); v.vouch > 0.0 && has_plan && e.odds.iter().fold(f64::NEG_INFINITY, |a, x| a.max(x.1)) < v.vouch }).map(|(t, _)| t.clone()).collect();
    let mut bound: Vec<String> = vec![];
    if meta.twin && has_plan && p.vars.iter().all(|v| plan.contains_key(&format!("free.{}", v.id))) {
        let free: HashMap<String, String> = plan.iter().filter_map(|(k, x)| k.strip_prefix("free.").map(|k| (k.to_string(), x.clone()))).collect();
        for i in violations(&free, &meta.rules, &meta.values) { if !bound.contains(&meta.owners[i]) { bound.push(meta.owners[i].clone()); } }
        bound.sort_by_key(|h| meta.active.iter().position(|a| a == h)); }
    let mut flat: HashMap<String, String> = stance.iter().chain(mood.iter()).map(|(k, e)| (k.clone(), e.level.clone())).collect();
    for (i, s) in order_steps.iter().enumerate() { flat.insert(format!("step.{s}"), p.slots[i].clone()); }
    let viol = if has_plan { violations(&flat, &meta.rules, &meta.values).len() } else { 0 };
    let conflict = conflict.unwrap_or(&[]); let yielded = yielded.unwrap_or(&[]);
    let escalate = if status == "fallback" && !conflict.is_empty() { Some(format!("habits conflict here (engine proved no stance obeys them all): {}", conflict.join(" + "))) }
        else if status == "fallback" { let r = doc.get("reason").filter(|r| truthy(r)).map_or(String::new(), |r| format!(": {}", r.as_str().map_or_else(|| canon(r), str::to_string).chars().take(160).collect::<String>()));
            Some(format!("no stance: engine {}{r}", verdict.as_str().unwrap_or("None"))) }
        else if !yielded.is_empty() { Some(format!("habit conflict resolved by priority: {} yielded", yielded.join(" + "))) }
        else if status == "partial" || status == "refused" { let mut unv: Vec<String> = p.vars.iter().filter(|v| !vouched[&v.id]).map(|v| v.id.clone()).collect();
            if !p.steps.is_empty() && !steps_vouched { unv.push("agenda".into()); }
            Some(format!("engine {vs}: unvouched: {}", if unv.is_empty() { "none".to_string() } else { unv.join(", ") })) }
        else { None };
    let line_active: Vec<String> = meta.active.iter().filter(|h| !conflict.contains(h)).cloned().collect();
    let levels: HashMap<String, String> = stance.iter().map(|(k, e)| (k.clone(), e.level.clone())).collect();
    let ln = line(p, &levels, &line_active, &bound, &unsure, if steps_vouched { &order_steps } else { &[] }, &vouched, &meta.conditional, &baseline(p, st));
    // the new state
    let mut ns = st.clone(); ns.turn = st.turn + 1; ns.mood = meta.new_mood.clone(); ns.learned = meta.new_learned.clone();
    // learned_from (§2.11): a feedback turn that moved a learned trait counts for it, a reward or a correction by the sign
    if let Some(lf) = ns.learned_from.as_mut() { for (t, r, c) in lf.iter_mut() {
        let moved = st.learned.iter().find(|(k, _)| k == t).map(|x| &x.1) != meta.new_learned.iter().find(|(k, _)| k == t).map(|x| &x.1);
        if moved && meta.learn_sign > 0.0 { *r += 1; } else if moved && meta.learn_sign < 0.0 { *c += 1; } } }
    let w = why(p, st, meta, &bound, &status, &stance, ns.learned_from.as_deref());
    // the credit the next turn's feedback reads: onehot(level) - odds of each learned trait this stance released (else 0)
    ns.credit = meta.new_learned.iter().map(|(t, d)| (t.clone(), match stance.iter().find(|(k, _)| k == t) { Some((_, e)) if e.released =>
        e.odds.iter().map(|(l, x)| r6(if *l == e.level { 1.0 } else { 0.0 } - x)).collect(), _ => vec![0.0; d.len()] })).collect();
    let mut seen: Vec<&str> = vec![];
    for h in &p.history { if seen.contains(&h.of.as_str()) { continue; } seen.push(&h.of); // one entry per flag and turn, however many features read it
        let keep = p.history.iter().filter(|x| x.of == h.of).map(|x| x.window.max(x.cap)).max().unwrap_or(1).max(1);
        let mut seq = st.history_of(&h.of).to_vec(); seq.push(matches!(get(&meta.vals, &h.of), Some(Json::Bool(true))));
        let seq = seq[seq.len().saturating_sub(keep)..].to_vec();
        match ns.history.iter_mut().find(|(k, _)| *k == h.of) { Some(x) => x.1 = seq, None => ns.history.push((h.of.clone(), seq)) } }
    ns.prev = flat.iter().filter(|(k, _)| !k.starts_with("step.")).map(|(k, v)| (k.clone(), v.clone())).collect();
    ns.agenda = (!p.steps.is_empty()).then(|| order_steps.clone());
    // turns since each goal was pursued: 0 for the goal a vouched stance pursues, +1 for the others
    if let (Some(d), Some(ds), Some((_, e))) = (&p.drives, ns.drives.as_mut(), stance.iter().find(|(k, _)| k == "pursue")) {
        let pursued = (status == "ok" || status == "partial") && e.released;
        for (i, g) in d.goals.iter().enumerate() { ds.since[i] = if pursued && g.id == e.level { 0 } else { ds.since[i] + 1 };
            // RFC 0002 A.4: a released stance that pursues a goal with expect_by_hours opens its window, if not already open
            if let (true, Some(w)) = (pursued && g.id == e.level, g.expect_by) { if ds.window[i].is_none() { ds.window[i] = Some(r6(w)); } } } }
    ns.seal(p);
    let out = Out { status, verdict: verdict.clone(), tier: doc.get("tier").cloned().unwrap_or(Json::Null), program: program.to_string(), stance, mood, active: meta.active.clone(), bound,
        violations: viol, conflict: conflict.to_vec(), yielded: yielded.to_vec(), unsure, escalate, inputs: meta.vals.iter().cloned().chain(meta.hist.iter().map(|(k, x)| (k.clone(), Json::Num(*x)))).collect(),
        ignored: meta.ignored.clone(), line: ln, why: w, agenda: (!p.steps.is_empty()).then_some(order_steps), state_digest: ns.digest.clone(), held: vec![], timing: None, lift: meta.lift.clone() };
    (out, ns)
}

// ------------------------------------------------------------------------------------------------------------ turns
fn solve(p: &Persona, st: &State, prog: &Json, eng: Engine, calls: &mut usize) -> Json { *calls += 1; eng(prog, &flags(p, st)) }
fn ms(t: Instant) -> f64 { t.elapsed().as_secs_f64() * 1e3 }
/// A minimal set of active habits that cannot hold together (deletion filter: drop each habit in turn, keep it only if the rest
/// becomes feasible without it); [] when the rules without any habit are already infeasible
fn habit_conflict(p: &Persona, st: &State, raw: &[(String, Json)], active: &[String], o: &Opts, eng: Engine, calls: &mut usize) -> R<Vec<String>> {
    let mut feasible = |ex: &[String]| -> R<bool> { let mut oo = Opts { no_inertia: o.no_inertia, twin: Some(false), hist: o.hist.clone(), ..Default::default() }; oo.exclude = o.exclude.iter().chain(ex).cloned().collect();
        let (prog, _) = compile(p, st, raw, &oo)?; Ok(solve(p, st, &prog, eng, calls).get("verdict").and_then(Json::as_str) != Some("infeasible")) };
    if !feasible(active)? { return Ok(vec![]); }
    let mut core: Vec<String> = active.to_vec();
    for h in active { let trial: Vec<String> = core.iter().filter(|x| *x != h).cloned().collect();
        let ex: Vec<String> = active.iter().filter(|x| !trial.contains(x)).cloned().collect(); if !feasible(&ex)? { core = trial; } }
    Ok(core)
}
/// One persona turn: (stance document, new state). A pure function of (persona, state, inputs, engine version). `timing` adds a
/// non-canonical `timing` object (compile / engine / decode ms, engine calls, the 1-minute load average).
pub fn turn(p: &Persona, st: &State, raw: &Json, no_inertia: bool, eng: Engine, timing: bool) -> R<(Json, State)> { turn_src(p, st, raw, no_inertia, eng, timing, true) }
/// `turn` without the provenance check (§2.10), for `fuzz` and `prove`: they search over stances, not over sources, so their events
/// carry no `src` and stand for events of an accepted source
pub fn turn_any_source(p: &Persona, st: &State, raw: &Json, no_inertia: bool, eng: Engine, timing: bool) -> R<(Json, State)> { turn_src(p, st, raw, no_inertia, eng, timing, false) }
fn turn_src(p: &Persona, st: &State, raw: &Json, no_inertia: bool, eng: Engine, timing: bool, check: bool) -> R<(Json, State)> {
    let raw: Vec<(String, Json)> = match raw { Json::Null => vec![], Json::Obj(v) => v.clone(), _ => return Err(perr("inputs", "must be a JSON object")) };
    for (n, (k, _)) in raw.iter().enumerate() { if raw[..n].iter().any(|(k2, _)| k2 == k) { return Err(perr(&format!("inputs.{k}"), "duplicate input")); } }
    // reward provenance (§2.10), first: with `reward_from` (or a senses block, §2.11), `src` is read here (not compiled, so not
    // `ignored`) and comes back in the stance's inputs; without either `src` is an undeclared input as before
    let (src, raw) = take_src(p, raw, check)?;
    // with a credit block (§2.8) `await`, `for` and `cancel` are read here too (not compiled, so not `ignored`) and come back in
    // the stance's inputs
    let (pay, raw) = take_pay(p, raw)?;
    // a senses block steps the normals and a drives block the drives before the turn, which runs on the stepped state with the
    // lowering's inputs; then the credit step settles what this turn's feedback pays
    let given = raw; let (st2, raw) = prep(p, st, &given, src.as_ref(), check)?;
    let (st2, settled) = match &pay { Some(k) => { let (s, d) = credit_step(p, &st2, &raw, k, src.as_ref(), check)?; (s, Some(d)) } None => (st2, None) };
    let st = &st2;
    let t0 = Instant::now(); let mut calls = 0;
    let o = Opts { no_inertia, ..Default::default() };
    let (mut prog, mut meta) = compile(p, st, &raw, &o)?; let mut ptext = digest_of(&text(&prog));
    let compile_ms = ms(t0); let t1 = Instant::now(); let mut doc = solve(p, st, &prog, eng, &mut calls);
    let (mut conflict, mut yielded): (Option<Vec<String>>, Option<Vec<String>>) = (None, None);
    if doc.get("verdict").and_then(Json::as_str) == Some("infeasible") && !meta.active.is_empty() {
        let c = habit_conflict(p, st, &raw, &meta.active, &o, eng, &mut calls)?; conflict = Some(c.clone());
        if p.eng.yields && !c.is_empty() { let mut y: Vec<String> = vec![]; let mut c = c;
            loop { let mut rank = c.clone(); rank.sort_by(|a, b| { let (ha, hb) = (p.habit(a), p.habit(b)); ha.priority.partial_cmp(&hb.priority).unwrap().then(p.habit_index(b).cmp(&p.habit_index(a))) });
                y.push(rank[0].clone());
                let oo = Opts { no_inertia, exclude: y.clone(), ..Default::default() }; let (pr, me) = compile(p, st, &raw, &oo)?; prog = pr; meta = me; ptext = digest_of(&text(&prog));
                doc = solve(p, st, &prog, eng, &mut calls);
                if doc.get("verdict").and_then(Json::as_str) != Some("infeasible") { conflict = None; break; }
                c = habit_conflict(p, st, &raw, &meta.active, &oo, eng, &mut calls)?; conflict = Some(c.clone()); if c.is_empty() { break; } }
            yielded = Some(y); } }
    let mut engine_ms = ms(t1);
    let (mut out, mut ns) = decode(p, st, &ptext, &meta, &doc, conflict.as_deref(), yielded.as_deref()); let lift = out.lift.clone();
    // the persona's vouch rule: unsure traits with a hold level are held there (a second, clamped solve), consistent with every habit
    let holds: Vec<(String, String)> = out.unsure.iter().filter_map(|t| { let v = p.var(t).unwrap(); let e = &out.stance.iter().find(|(k, _)| k == t).unwrap().1;
        v.hold.as_ref().filter(|h| **h != e.level).map(|h| (t.clone(), h.clone())) }).collect();
    let mut held = vec![];
    if !holds.is_empty() {
        let o2 = Opts { no_inertia, clamps: holds.clone(), ..Default::default() }; let (prog2, meta2) = compile(p, st, &raw, &o2)?;
        let te = Instant::now(); let doc2 = solve(p, st, &prog2, eng, &mut calls); engine_ms += ms(te);
        if matches!(doc2.get("verdict").and_then(Json::as_str), Some("exact" | "diagnostics_passed")) {
            let (mut out2, ns2) = decode(p, st, &digest_of(&text(&prog2)), &meta2, &doc2, None, None);
            out2.unsure = out.unsure.clone();
            for (t, _) in &holds { let u = out.stance.iter().find(|(k, _)| k == t).map(|(_, e)| e.odds.clone()); if let Some(e) = out2.stance.iter_mut().find(|(k, _)| k == t) { e.1.unheld = u; } }
            out2.bound = out.bound.clone(); held = holds.iter().map(|(t, _)| t.clone()).collect(); held.sort();
            let levels: HashMap<String, String> = out2.stance.iter().map(|(k, e)| (k.clone(), e.level.clone())).collect();
            let vouched: HashMap<String, bool> = out2.stance.iter().map(|(k, e)| (k.clone(), e.released)).collect();
            out2.line = line(p, &levels, &meta2.active, &out.bound, &out.unsure, out2.agenda.as_deref().unwrap_or(&[]), &vouched, &meta2.conditional, &baseline(p, st));
            out = out2; ns = ns2; } }
    out.held = held; out.lift = lift;
    if timing { let r3 = |x: f64| Json::Num((x * 1e3).round() / 1e3);
        out.timing = Some(Json::Obj(vec![("compile_ms".into(), r3(compile_ms)), ("engine_ms".into(), r3(engine_ms)), ("decode_ms".into(), r3((ms(t0) - compile_ms - engine_ms).max(0.0))), ("turn_ms".into(), r3(ms(t0))),
            ("engine_calls".into(), Json::Num(calls as f64)), ("holds".into(), Json::Num(out.held.len() as f64)), ("load_avg_1m".into(), crate::sys::loadavg().map_or(Json::Null, |l| Json::Num((l * 100.0).round() / 100.0)))])); }
    let mut j = out.to_json(p, st);
    if let (Some(sn), Some(ss)) = (&p.senses, &st.senses) { stance_senses(&mut j, sn, ss, &given); }
    if let (Some(d), Some(ds)) = (&p.drives, &st.drives) { stance_drives(&mut j, d, ds, &out.lift, &given); }
    if let (Some(s), Json::Obj(v)) = (src, &mut j) { if let Some((_, Json::Obj(ins))) = v.iter_mut().find(|(k, _)| k == "inputs") { ins.push(("src".into(), s)); } }
    if let (Some(k), Some(d)) = (&pay, settled) { ns = settle(p, &mut j, ns, k, d); }
    Ok((j, ns))
}

/// The normals of a state with a senses block (§2.11), per sense in declared order: (id, mean, count, drift_cap); none without one
pub fn normals(p: &Persona, st: &State) -> Vec<(String, f64, u64, f64)> {
    match (&p.senses, &st.senses) { (Some(sn), Some(ss)) => sn.list.iter().enumerate().map(|(i, s)| (s.id.clone(), ss.mean[i], ss.n[i], s.drift_cap)).collect(), _ => vec![] }
}
/// The deficits of a state with a needs block (RFC 0002 A.3), per need in declared order: (id, deficit, cap); none without one
pub fn deficits(p: &Persona, st: &State) -> Vec<(String, f64, f64)> {
    match (p.drives.as_ref().and_then(|d| d.needs.as_ref()), st.drives.as_ref().and_then(|ds| ds.nd.as_ref())) {
        (Some(nb), Some(nd)) => nb.items.iter().zip(&nd.deficit).map(|(x, v)| (x.id.clone(), *v, x.cap)).collect(), _ => vec![] }
}
/// A new individual: genes from the seed, an empty mood and history, the fallback levels as `prev`, and (with `rest`) the
/// resting stance: this individual's joint plan with no live evidence and a neutral mood (one engine call).
pub fn init(p: &Persona, seed: Option<u64>, rest: bool, eng: Engine) -> State {
    let seed = seed.unwrap_or(p.seed);
    let mut st = State { seed, turn: 0, persona: [p.name.clone(), p.version.clone(), p.digest.clone()], genes: genes(p, seed),
        mood: p.vars.iter().filter(|v| v.mood).map(|v| (v.id.clone(), vec![0.0; v.levels.len()])).collect(),
        history: { let mut h: Vec<(String, Vec<bool>)> = vec![]; for x in &p.history { if !h.iter().any(|(k, _)| *k == x.of) { h.push((x.of.clone(), vec![])); } } h },
        prev: p.vars.iter().map(|v| (v.id.clone(), v.fallback.clone())).collect(), agenda: (!p.steps.is_empty()).then(|| p.steps.clone()), rest: None,
        learned: zeros(p), credit: zeros(p), drives: p.drives.as_ref().map(|d| DState::zero(p, d, seed)), senses: p.senses.as_ref().map(|sn| SState::zero(&p.name, sn, seed)),
        learned_from: learned_from0(p), open: vec![], prev_stance: None, control_closed: vec![], elig: 1.0, digest: String::new() };
    if rest { if let Ok((prog, _)) = compile(p, &st, &[], &Opts { twin: Some(false), ..Default::default() }) {
        let doc = eng(&prog, &flags(p, &st));
        if let Some(plan) = doc.get("plan").and_then(Json::as_obj) {
            st.rest = Some(p.vars.iter().filter_map(|v| plan.iter().find(|(k, _)| *k == v.id).map(|(_, x)| (v.id.clone(), back(p, &v.id, x.as_str().unwrap_or(""))))).collect()); } } }
    st.seal(p); st
}
/// A script: a JSON list of per-turn input objects, or {"turns": [...]}; a turn may be {"inputs": {...}, ...}
pub fn script(j: &Json) -> R<Vec<Json>> {
    let turns = match j { Json::Arr(a) => a, Json::Obj(_) => j.get("turns").and_then(Json::as_arr).ok_or_else(|| perr("script", "a JSON list of per-turn input objects (or {\"turns\": [...]})"))?,
        _ => return Err(perr("script", "a JSON list of per-turn input objects (or {\"turns\": [...]})")) };
    turns.iter().enumerate().map(|(i, t)| { let t = if let Some(x) = t.get("inputs") { x } else { t };
        if matches!(t, Json::Obj(_) | Json::Null) { Ok(t.clone()) } else { Err(perr(&format!("script.turns[{i}]"), "must be an object of inputs")) } }).collect()
}
/// The tools `probbit_persona_init` / `probbit_persona_turn` (`probbit mcp`, probbit-wasm ops 4 / 5): stateless, the persona (an
/// inline document, or with `files` a path) and the state go in, the state comes back. init -> the state; turn -> {stance, state}.
pub fn tool(name: &str, args: &[(String, Json)], eng: Engine, files: bool) -> R<Json> {
    let get = |k: &str| args.iter().find(|(x, _)| x == k).map(|(_, v)| v).filter(|v| !v.is_null());
    let p = match (get("persona"), get("persona_path")) {
        (Some(d), None) => { if !matches!(d, Json::Obj(_)) { return Err(perr("arguments.persona", "must be an object (the JSON form of a persona file)")); } build(d)? }
        (None, Some(_)) if !files => return Err(perr("arguments.persona_path", "no files here: give the persona document as persona")),
        (None, Some(path)) => load(path.as_str().ok_or_else(|| perr("arguments.persona_path", "a file path"))?)?.0,
        _ => return Err(perr("arguments", "give exactly one of persona (a document) or persona_path")) };
    let known: &[&str] = if name == "probbit_persona_init" { &["persona", "persona_path", "seed"] } else { &["persona", "persona_path", "state", "inputs", "flags"] };
    let mut extra: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).filter(|k| !known.contains(k)).collect(); extra.sort_unstable();
    if let Some(k) = extra.first() { return Err(perr(&format!("arguments.{k}"), "unknown argument")); }
    if name == "probbit_persona_init" {
        let seed = match get("seed") { None => None, Some(s) => Some(s.as_f64().filter(|x| x.fract() == 0.0 && (0.0..=9_007_199_254_740_992.0).contains(x)).ok_or_else(|| perr("arguments.seed", "an integer from 0 to 2^53"))? as u64) };
        return Ok(init(&p, seed, true, eng).to_json(&p)); }
    let st = get("state").filter(|s| matches!(s, Json::Obj(_))).ok_or_else(|| perr("arguments.state", "required: the state from probbit_persona_init or the previous turn"))?;
    let st = State::read(&p, st)?;
    let inputs = get("inputs").cloned().unwrap_or(Json::Obj(vec![])); if !matches!(inputs, Json::Obj(_)) { return Err(perr("arguments.inputs", "must be an object")); }
    let fl = match get("flags") { None => vec![], Some(Json::Obj(v)) => v.clone(), Some(_) => return Err(perr("arguments.flags", "known flags: timing, no_inertia")) };
    if let Some((k, _)) = fl.iter().find(|(k, _)| k != "timing" && k != "no_inertia") { return Err(perr(&format!("arguments.flags.{k}"), "known flags: timing, no_inertia")); }
    let on = |k: &str| -> R<bool> { match fl.iter().find(|(x, _)| x == k) { None => Ok(false), Some((_, Json::Bool(b))) => Ok(*b), Some(_) => Err(perr(&format!("arguments.flags.{k}"), "true or false")) } };
    let (stance, ns) = turn(&p, &st, &inputs, on("no_inertia")?, eng, on("timing")?)?;
    Ok(Json::Obj(vec![("stance".into(), stance), ("state".into(), ns.to_json(&p))]))
}
/// S7 (§2.11): the MCP tools are the agent's own channel, and readings come from the host's instruments: an event (`key`: the
/// argument that holds its inputs) that carries readings for a persona with a senses block is refused
pub fn mcp_refuses_readings(args: &[(String, Json)], key: &str) -> R<()> {
    let get = |k: &str| args.iter().find(|(x, _)| x == k).map(|(_, v)| v).filter(|v| !v.is_null());
    if !get(key).and_then(|e| e.get("senses")).is_some_and(truthy) { return Ok(()); }
    let p = match (get("persona"), get("persona_path")) { (Some(d), None) => build(d).ok(), (None, Some(Json::Str(f))) => load(f).ok().map(|x| x.0), _ => None };
    if p.is_some_and(|p| p.senses.is_some()) {
        return Err(perr(&format!("arguments.{key}.senses"), "readings come from the host's instruments, never through the agent's channel: MCP refuses an event that carries readings (the host sends them with probbit live or persona turn)")); }
    Ok(())
}
/// §2.8: a cancel says no outcome will come for a decision, so an agent could cancel to dodge a correction: through the MCP tools
/// (the agent's own channel) an event that carries `cancel` is refused for a persona with a credit block; a persona without one
/// lists it in stance.ignored
pub fn mcp_refuses_cancel(args: &[(String, Json)], key: &str) -> R<()> {
    let get = |k: &str| args.iter().find(|(x, _)| x == k).map(|(_, v)| v).filter(|v| !v.is_null());
    if get(key).and_then(|e| e.get("cancel")).is_none() { return Ok(()); }
    let p = match (get("persona"), get("persona_path")) { (Some(d), None) => build(d).ok(), (None, Some(Json::Str(f))) => load(f).ok().map(|x| x.0), _ => None };
    if p.is_some_and(|p| p.credit().is_some()) {
        return Err(perr(&format!("arguments.{key}.cancel"), "a cancel comes from the host or a person, never through the agent's channel: MCP refuses an event that carries cancel (the host sends it with probbit live or persona turn)")); }
    Ok(())
}
/// RFC 0002 (security review): a need's amount comes from the host, from a `satisfy_from` source, so an agent could satisfy its
/// own needs: through the MCP tools (the agent's own channel) an event that carries `needs` amounts is refused for a persona with a
/// needs block; a persona without one lists it in stance.ignored
pub fn mcp_refuses_amounts(args: &[(String, Json)], key: &str) -> R<()> {
    let get = |k: &str| args.iter().find(|(x, _)| x == k).map(|(_, v)| v).filter(|v| !v.is_null());
    if !get(key).and_then(|e| e.get("needs")).is_some_and(truthy) { return Ok(()); }
    let p = match (get("persona"), get("persona_path")) { (Some(d), None) => build(d).ok(), (None, Some(Json::Str(f))) => load(f).ok().map(|x| x.0), _ => None };
    if p.is_some_and(|p| p.has_needs()) {
        return Err(perr(&format!("arguments.{key}.needs"), "a need's amount comes from the host's satisfy_from sources, never through the agent's channel: MCP refuses an event that carries needs amounts (the host sends them with probbit live or persona turn)")); }
    Ok(())
}
/// `init` at `seed`, then every turn of the script -> (the stance documents, the final state)
pub fn replay(p: &Persona, seed: Option<u64>, turns: &[Json], no_inertia: bool, eng: Engine, timing: bool) -> R<(Vec<Json>, State)> {
    let mut st = init(p, seed, true, eng); let mut out = vec![];
    for (i, t) in turns.iter().enumerate() { let (o, ns) = turn(p, &st, t, no_inertia, eng, timing).map_err(|e| perr(&format!("script.turns[{i}].{}", e.path), e.msg))?; out.push(o); st = ns; }
    Ok((out, st))
}
/// `compile`: the turn's program (insertion order; its sha256 is the stance's `engine.program`)
pub fn program(p: &Persona, st: &State, raw: &Json) -> R<String> {
    let raw: Vec<(String, Json)> = match raw { Json::Null => vec![], Json::Obj(v) => v.clone(), _ => return Err(perr("inputs", "must be a JSON object")) };
    let (src, raw) = if p.senses.is_some() || p.credit().is_some() || p.has_needs() { take_src(p, raw, true)? } else { (None, raw) };
    let (pay, raw) = take_pay(p, raw)?;
    let (st, raw) = prep(p, st, &raw, src.as_ref(), true)?;
    let st = match &pay { Some(k) => credit_step(p, &st, &raw, k, src.as_ref(), true)?.0, None => st };
    Ok(text(&compile(p, &st, &raw, &Opts::default())?.0))
}

/// Static check: for every conditional habit and every pair of them (with every unconditional one), and every previous level of
/// the traits their `prev` restrictions read: is there any stance that obeys them all? -> the contradictions with their resolution.
/// Conservative: a pair whose conditions cannot both hold for any input is still reported (the author decides).
pub fn lint(p: &Persona, eng: Engine) -> Vec<Json> {
    let uncond: Vec<&Habit> = p.habits.iter().filter(|h| h.when.is_empty()).collect(); let cond: Vec<&Habit> = p.habits.iter().filter(|h| !h.when.is_empty()).collect();
    let mut combos: Vec<Vec<&Habit>> = cond.iter().map(|a| vec![*a]).collect();
    for i in 0..cond.len() { for j in i + 1..cond.len() { combos.push(vec![cond[i], cond[j]]); } }
    let mut found: Vec<(Vec<String>, Vec<Json>)> = vec![];
    for combo in combos {
        let hs: Vec<&Habit> = uncond.iter().copied().chain(combo.iter().copied()).collect();
        let mut rel: Vec<String> = hs.iter().flat_map(|h| h.then.iter().filter(|(_, r)| r.as_obj().is_some_and(|o| o.iter().any(|(_, x)| x.as_str().is_some_and(|s| s.starts_with("prev"))))).map(|(k, _)| k.clone())).collect();
        rel.sort(); rel.dedup();
        let mut q = p.clone(); q.habits = hs.iter().map(|h| Habit { when: vec![], ..(*h).clone() }).collect();
        let dims: Vec<&Vec<String>> = rel.iter().map(|k| &p.var(k).unwrap().levels).collect(); let total: usize = dims.iter().map(|d| d.len()).product();
        for c in 0..total { // itertools.product order: the last trait varies fastest
            let mut idx = vec![0; dims.len()]; let mut r = c; for d in (0..dims.len()).rev() { idx[d] = r % dims[d].len(); r /= dims[d].len(); }
            let mut st = init(p, Some(p.seed), false, eng); for (k, &i) in rel.iter().zip(&idx) { st.prev.insert(k.clone(), p.var(k).unwrap().levels[i].clone()); }
            let Ok((prog, _)) = compile(&q, &st, &[], &Opts { twin: Some(false), ..Default::default() }) else { continue };
            if eng(&prog, &flags(&q, &st)).get("verdict").and_then(Json::as_str) == Some("infeasible") {
                let key: Vec<String> = combo.iter().map(|h| h.id.clone()).collect();
                let when = Json::Obj(rel.iter().zip(&idx).map(|(k, &i)| (k.clone(), Json::Str(p.var(k).unwrap().levels[i].clone()))).collect());
                match found.iter_mut().find(|f| f.0 == key) { Some(f) => f.1.push(when), None => found.push((key, vec![when])) } } }
    }
    found.into_iter().map(|(hs, when)| {
        let res = if p.eng.yields && hs.len() > 1 { let mut lo = hs.clone(); lo.sort_by(|a, b| p.habit(a).priority.partial_cmp(&p.habit(b).priority).unwrap().then(p.habit_index(b).cmp(&p.habit_index(a))));
            let tie = p.habit(&lo[0]).priority == p.habit(&lo[1]).priority; format!("yield: {} yields{}", lo[0], if tie { " (priority tie: the later-declared habit yields)" } else { "" }) }
            else { "fallback: no stance on such turns (status fallback, escalate)".to_string() };
        Json::Obj(vec![("habits".into(), jstrs(&hs)), ("when_prev".into(), Json::Arr(when)), ("resolution".into(), Json::Str(res))]) }).collect()
}

/// `lint` for a drives block (§2.9): every habit that can exclude a floor goal. A habit outranks a floor: on a turn it is in force
/// and excludes the goal (for some previous level, with `prev` restrictions), the goal's odds are 0. None without a block.
pub fn floor_notes(p: &Persona) -> Option<Vec<Json>> {
    let (d, v) = (p.drives.as_ref()?, p.var("pursue")?); let mut out = vec![];
    for (gi, g) in d.goals.iter().enumerate().filter(|(_, g)| g.floor > 0.0) { for h in &p.habits {
        let by_then = h.then.iter().filter(|(k, _)| k == "pursue").any(|(_, r)| v.levels.iter().any(|pl| !allowed(&v.levels, r, pl).iter().any(|l| **l == g.id)));
        let by_rule = h.rules.iter().filter(|(k, _)| k == "tables").flat_map(|(_, l)| l).any(|r| !pursue_allowed(&v.levels, std::slice::from_ref(r))[gi]);
        if by_then || by_rule { out.push(Json::Obj(vec![("goal".into(), Json::Str(g.id.clone())), ("floor".into(), Json::Num(g.floor)), ("habit".into(), Json::Str(h.id.clone())),
            ("note".into(), Json::Str("a habit outranks a floor: on turns this habit is in force the goal's odds are 0".into()))])); } } }
    Some(out)
}
/// `lint` for a needs block (RFC 0002, CLI surface): each need whose horizon lets the forecast pass K/2 before the deficit does (the
/// term turns positive while a satisfaction would still overshoot, unless the deficit grows to K/2 first), and every habit that can
/// exclude a bound goal (on turns it is in force the need's term cannot win `pursue`, and the deficit keeps growing). At growth gene
/// 1. None without a block.
pub fn needs_notes(p: &Persona) -> Option<Vec<Json>> {
    let d = p.drives.as_ref()?; let nb = d.needs.as_ref()?; let v = p.var("pursue")?; let mut out = vec![]; let s = |x: &str| Json::Str(x.to_string());
    for x in &nb.items { let lead = r6(x.forecast * x.horizon); if lead <= 0.0 { continue; } let half = r6(x.amount / 2.0);
        out.push(Json::Obj(vec![("need".into(), s(&x.id)), ("horizon_hours".into(), Json::Num(x.horizon)), ("forecast_per_hour".into(), Json::Num(x.forecast)),
            ("half_amount".into(), Json::Num(half)), ("term_positive_from".into(), Json::Num(r6((half - lead).max(0.0)))),
            ("note".into(), s("the forecast can pass K/2 before the deficit does: from this deficit the term is positive, and a satisfaction before the deficit reaches K/2 overshoots (a setback)"))])); }
    for x in &nb.items { let (gi, g) = (x.bind, &d.goals[x.bind]); for h in &p.habits {
        let by_then = h.then.iter().filter(|(k, _)| k == "pursue").any(|(_, r)| v.levels.iter().any(|pl| !allowed(&v.levels, r, pl).iter().any(|l| **l == g.id)));
        let by_rule = h.rules.iter().filter(|(k, _)| k == "tables").flat_map(|(_, l)| l).any(|r| !pursue_allowed(&v.levels, std::slice::from_ref(r))[gi]);
        if by_then || by_rule { out.push(Json::Obj(vec![("need".into(), s(&x.id)), ("goal".into(), s(&g.id)), ("habit".into(), s(&h.id)),
            ("note".into(), s("a habit excludes a bound goal: on turns this habit is in force the need's term cannot win pursue, and its deficit keeps growing"))])); } } }
    Some(out)
}
/// Unary goal-field bounds for one individual, before floor lifts and couplings.
fn floor_fields(p: &Persona, d: &Drives, seed: u64) -> (Vec<f64>, Vec<f64>) {
    let (n, v) = (d.goals.len(), p.var("pursue").unwrap()); let span = |w: f64, cap: f64| ((w * cap).min(0.0), (w * cap).max(0.0));
    let (sw, sa, sd) = (span(d.w_want, d.cap_w), span(d.w_glow, d.cap_a), span(d.w_deadline, 1.0));
    let lcap = p.learning.as_ref().filter(|l| l.traits.iter().any(|t| t == "pursue")).map_or(0.0, |l| l.total_cap);
    let eff = |e: &Eff| e.iter().find(|(t, _)| t == "pursue").map(|(_, w)| w.clone());
    let (gj, dg) = (genes(p, seed), drive_genes(p, d, seed)); let react = |id: &str| gj.get("react").and_then(|r| r.get(id)).and_then(Json::as_f64).unwrap_or(1.0);
    let gene = |i: usize| dg.get("interest").and_then(|o| o.get(&d.goals[i].id)).and_then(Json::as_f64).unwrap_or(0.0);
    let mut lo: Vec<f64> = (0..n).map(|i| v.base[i] + gene(i) - lcap + sw.0 + sa.0 + sd.0).collect(); let mut hi: Vec<f64> = (0..n).map(|i| v.base[i] + gene(i) + lcap + sw.1 + sa.1 + sd.1).collect();
    let mut add = |opts: Vec<Vec<f64>>| { for i in 0..n { lo[i] += opts.iter().map(|w| w[i]).fold(f64::INFINITY, f64::min); hi[i] += opts.iter().map(|w| w[i]).fold(f64::NEG_INFINITY, f64::max); } };
    for x in &p.inputs { let g = react(&x.id); match x.kind {
        Kind::Flag => if let Some(e) = eff(&x.effects) { add(vec![vec![0.0; n], e.iter().map(|w| g * w).collect()]); },
        Kind::Number => if let Some(e) = eff(&x.effects) { add(vec![vec![0.0; n], e.iter().map(|w| g * x.max * w).collect()]); },
        Kind::Level => add(x.levels.iter().map(|l| x.by_level.iter().find(|(b, _)| b == l).and_then(|(_, e)| eff(e)).map_or(vec![0.0; n], |e| e.iter().map(|w| g * w).collect())).collect()) } }
    for h in &p.history { if let Some(e) = eff(&h.effects) { add(vec![vec![0.0; n], e.iter().map(|w| h.cap as f64 * w).collect()]); } }
    (lo, hi)
}
/// `lint` for floors (§2.9): a floor goal whose lift can reach 50 on some turn of the given individuals: the lift's iteration with
/// the goal's unary at its least and every other goal's at its most over everything the persona can feed `pursue` (prior, the
/// seed's drive genes, learned ±total_cap, every input and history effect at its extremes, the drive box)
pub fn floor_reach(p: &Persona, seeds: &[u64]) -> Vec<Json> {
    let Some(d) = &p.drives else { return vec![] };
    if !d.goals.iter().any(|g| g.floor > 0.0) { return vec![]; }
    let n = d.goals.len(); let mut most = vec![0.0f64; n];
    for &seed in seeds { let (lo, hi) = floor_fields(p, d, seed);
        for (m, l) in most.iter_mut().zip(lift_iter(p, d, &lo, &hi, &vec![true; n])) { *m = m.max(l); } }
    d.goals.iter().zip(most).filter(|(g, l)| g.floor > 0.0 && *l >= 50.0).map(|(g, l)| Json::Obj(vec![("goal".into(), Json::Str(g.id.clone())), ("floor".into(), Json::Num(g.floor)),
        ("lift_max".into(), Json::Num(r6(l))), ("note".into(), Json::Str("the floor's lift can reach 50 or more: it outweighs everything the persona can say for or against this goal".into()))])).collect()
}
/// `prove` for a drives block's floors (§2.9, design §5.2): each floor holds by construction of the odds lift, on every turn whose
/// habits allow the goal (the soundness condition, no multi-variable rule over `pursue`, is checked when the persona is read);
/// with the habits that can exclude it. Extreme numeric ranges are unknown. None without floors.
pub fn floor_verdicts(p: &Persona, seeds: &[u64]) -> Option<Vec<Json>> {
    let d = p.drives.as_ref()?; let notes = floor_notes(p)?; if !d.goals.iter().any(|g| g.floor > 0.0) { return None; }
    // Keep the construction verdict within the normal exponential range. Extreme ranges use the log-space lift at
    // runtime, but this structural proof does not certify their floating-point rounding over every possible turn.
    let normal = !seeds.is_empty() && seeds.iter().all(|&seed| {
        let (mut lo, mut hi) = floor_fields(p, d, seed);
        for (a, b, tab) in &p.couplings {
            let rows: Vec<Vec<f64>> = if a == "pursue" { tab.clone() } else if b == "pursue" { (0..lo.len()).map(|i| tab.iter().map(|r| r[i]).collect()).collect() } else { continue };
            for (i, r) in rows.iter().enumerate() { lo[i] += r.iter().copied().fold(f64::INFINITY, f64::min); hi[i] += r.iter().copied().fold(f64::NEG_INFINITY, f64::max); }
        }
        lo.iter().chain(&hi).all(|x| x.is_finite() && *x >= f64::MIN_POSITIVE.ln() && *x <= f64::MAX.ln())
    });
    Some(d.goals.iter().filter(|g| g.floor > 0.0).map(|g| { let by: Vec<String> = notes.iter().filter(|n| n.get("goal").and_then(Json::as_str) == Some(g.id.as_str()))
        .filter_map(|n| n.get("habit").and_then(Json::as_str).map(str::to_string)).collect();
        let mut fields = vec![("goal".into(), Json::Str(g.id.clone())), ("floor".into(), Json::Num(g.floor)),
            ("verdict".into(), Json::Str(if normal { "held_by_construction" } else { "unknown" }.into())), ("excluded_by".into(), jstrs(&by))];
        if !normal { fields.push(("reason".into(), Json::Str("goal field bounds leave the normal exponential range; the numeric floor guarantee is not certified".into()))); }
        Json::Obj(fields) }).collect())
}
/// `lint`'s decoding check (a warning, not an error): the stance is the joint plan, the most likely stance as a whole, so on
/// coupled traits a trait's planned level can differ from its own most likely level (its marginal mode). Probes: each seed's
/// individual at rest, then a quiet turn and every declared input alone (numbers at their maximum). -> per trait where that
/// happens on a vouched, not unsure, turn: how many probe turns, out of how many, and the earliest one.
pub fn plan_warnings(p: &Persona, seeds: &[u64], eng: Engine) -> Vec<Json> {
    let mut probes: Vec<Vec<(String, Json)>> = vec![vec![]];
    for (id, _, vals) in alphabet(p, &[1.0], &[]) { for v in vals {
        let v = match (id.strip_prefix("sense."), &p.senses) { (Some(sid), Some(sn)) => { let src = sn.list.iter().find(|x| x.id == sid).map_or("env".to_string(), |x| x.src[0].clone());
            Json::Obj(vec![("v".into(), v), ("src".into(), Json::Str(src))]) } _ => v };
        let v = match (id.strip_prefix("need."), p.drives.as_ref().and_then(|d| d.needs.as_ref())) { (Some(nid), Some(nb)) => { let src = nb.items.iter().find(|x| x.id == nid).map_or("env".to_string(), |x| x.src[0].clone());
            Json::Obj(vec![("amount".into(), v), ("src".into(), Json::Str(src))]) } _ => v };
        probes.push(vec![(id.clone(), v)]); } }
    let traits: Vec<&Var> = p.vars.iter().filter(|v| !v.mood).collect();
    let (mut found, mut total): (Vec<(usize, Option<Json>)>, usize) = (vec![(0, None); traits.len()], 0);
    for &seed in seeds { let st = init(p, Some(seed), true, eng);
        for e in &probes { let Ok((doc, _)) = turn(p, &st, &event_json(e), false, eng, false) else { continue }; total += 1;
            let unsure = doc.get("unsure").and_then(Json::as_arr).unwrap_or(&[]).to_vec();
            for (i, v) in traits.iter().enumerate() { let Some(x) = doc.get("stance").and_then(|s| s.get(&v.id)) else { continue };
                if x.get("released") != Some(&Json::Bool(true)) || unsure.contains(&Json::Str(v.id.clone())) { continue; }
                let plan = x.get("level").and_then(Json::as_str).unwrap_or(""); let odds = x.get("odds").and_then(Json::as_obj).unwrap_or(&[]);
                let pp = odds.iter().find(|(l, _)| l == plan).and_then(|(_, q)| q.as_f64()).unwrap_or(0.0);
                let (mut mode, mut pm) = (plan, pp); for (l, q) in odds { if q.as_f64().unwrap_or(0.0) > pm { (mode, pm) = (l.as_str(), q.as_f64().unwrap_or(0.0)); } }
                if pm <= pp + 1e-9 { continue; } found[i].0 += 1;
                if found[i].1.is_none() { found[i].1 = Some(Json::Obj(vec![("seed".into(), Json::Num(seed as f64)), ("inputs".into(), event_json(e)),
                    ("plan".into(), Json::Str(plan.into())), ("p_plan".into(), Json::Num(pp)), ("mode".into(), Json::Str(mode.into())), ("p_mode".into(), Json::Num(pm))])); } } } }
    traits.iter().zip(found).filter(|(_, f)| f.0 > 0).map(|(v, (n, example))| Json::Obj(vec![("trait".into(), Json::Str(v.id.clone())), ("turns".into(), Json::Num(n as f64)),
        ("of".into(), Json::Num(total as f64)), ("example".into(), example.unwrap_or(Json::Null)),
        ("note".into(), Json::Str("planned level differs from the trait's own most likely level (the stance is the most likely stance as a whole); a habit pins the level, a `vouch` floor with a `hold` holds it when the odds are low".into()))])).collect()
}

/// A stance document's per-variable entries: its traits, then (drives, §4) `pursue` with its goal as the level
fn entries(doc: &Json) -> Vec<(String, Json)> {
    let mut v: Vec<(String, Json)> = doc.get("stance").and_then(Json::as_obj).map_or(vec![], <[(String, Json)]>::to_vec);
    if let Some(pz) = doc.get("pursue") { let f = |k: &str| pz.get(k).cloned().unwrap_or(Json::Null); v.push(("pursue".into(), Json::Obj(vec![("level".into(), f("goal")), ("odds".into(), f("odds"))]))); }
    v
}
/// Total-variation distance between two odds objects over the same levels (None if their levels differ)
fn tv(oa: &[(String, Json)], ob: &[(String, Json)]) -> Option<f64> {
    let mut ka: Vec<&String> = oa.iter().map(|(k, _)| k).collect(); let mut kb: Vec<&String> = ob.iter().map(|(k, _)| k).collect(); ka.sort(); kb.sort();
    (ka == kb).then(|| 0.5 * oa.iter().fold(0.0, |s, (k, p)| s + (p.as_f64().unwrap_or(0.0) - ob.iter().find(|(l, _)| l == k).and_then(|(_, q)| q.as_f64()).unwrap_or(0.0)).abs()))
}
/// Mean total-variation distance between two individuals' per-trait odds over turns and shared traits, and the fraction of
/// (turn, trait) where their levels differ (0 / 0 for identical individuals)
pub fn distance(a: &[Json], b: &[Json]) -> Json {
    let (mut tvs, mut dis): (Vec<f64>, Vec<f64>) = (vec![], vec![]); let mut per: Vec<(String, Vec<f64>)> = vec![];
    for (x, y) in a.iter().zip(b) { let (sa, sb) = (entries(x), entries(y));
        for (t, ea) in &sa { let Some(eb) = get(&sb, t) else { continue }; let (oa, ob) = (ea.get("odds").and_then(Json::as_obj).unwrap_or(&[]), eb.get("odds").and_then(Json::as_obj).unwrap_or(&[]));
            let Some(d) = tv(oa, ob) else { continue };
            tvs.push(d); match per.iter_mut().find(|(k, _)| k == t) { Some(e) => e.1.push(d), None => per.push((t.clone(), vec![d])) }
            dis.push(if ea.get("level") == eb.get("level") { 0.0 } else { 1.0 }); } }
    let mean = |v: &[f64]| r6(v.iter().fold(0.0, |a, b| a + b) / v.len() as f64);
    if tvs.is_empty() { return Json::Obj(vec![("tv".into(), Json::Null), ("level_disagreement".into(), Json::Null), ("per_trait".into(), Json::Obj(vec![]))]); }
    per.sort_by(|x, y| x.0.cmp(&y.0));
    Json::Obj(vec![("tv".into(), Json::Num(mean(&tvs))), ("level_disagreement".into(), Json::Num(mean(&dis))), ("per_trait".into(), Json::Obj(per.iter().map(|(k, v)| (k.clone(), Json::Num(mean(v)))).collect()))])
}
/// `diff`: two individuals (another seed, or another persona) over one script
pub fn diff(p: &Persona, sa: u64, q: &Persona, sb: u64, turns: &[Json], eng: Engine) -> R<Json> {
    let (ta, _) = replay(p, Some(sa), turns, false, eng, false)?; let (tb, _) = replay(q, Some(sb), turns, false, eng, false)?;
    // the raw total-variation sum over the shared traits (as `distance`, unrounded): the first turn with the largest one
    let tvsum = |i: usize| { let eb = entries(&tb[i]); entries(&ta[i]).iter().filter_map(|(t, ea)| {
        tv(ea.get("odds")?.as_obj()?, get(&eb, t)?.get("odds")?.as_obj()?) }).fold(0.0, |a, b| a + b) };
    let worst = argmax(ta.len().min(tb.len()), tvsum);
    let who = |p: &Persona, s: u64| Json::Obj(vec![("persona".into(), Json::Str(p.name.clone())), ("seed".into(), Json::Num(s as f64)), ("genes".into(), genes(p, s))]);
    let line_of = |t: &[Json]| t.get(worst).and_then(|x| x.get("line")).cloned().unwrap_or(Json::Null);
    Ok(Json::Obj(vec![("a".into(), who(p, sa)), ("b".into(), who(q, sb)), ("turns".into(), Json::Num(turns.len() as f64)), ("distance".into(), distance(&ta, &tb)),
        ("most_different_turn".into(), Json::Num(worst as f64)), ("line_a".into(), line_of(&ta)), ("line_b".into(), line_of(&tb))]))
}
/// `describe`: the persona's traits, moods, inputs, habits and agenda
pub fn describe(p: &Persona) -> Json {
    let s = |x: &str| Json::Str(x.to_string());
    let mut d = Json::Obj(vec![("name".into(), s(&p.name)), ("version".into(), s(&p.version)), ("digest".into(), s(&p.digest)), ("seed".into(), Json::Num(p.seed as f64)),
        ("traits".into(), Json::Obj(p.vars.iter().filter(|v| !v.mood && !lowered(p, &v.id)).map(|v| (v.id.clone(), Json::Obj(vec![("levels".into(), jstrs(&v.levels)), ("say".into(), jstrs(&v.say))]))).collect())),
        ("moods".into(), Json::Obj(p.vars.iter().filter(|v| v.mood).map(|v| (v.id.clone(), jstrs(&v.levels))).collect())),
        ("inputs".into(), Json::Obj(p.inputs.iter().filter(|x| !lowered(p, &x.id)).map(|x| (x.id.clone(), Json::Obj(match x.kind { Kind::Level => vec![("kind".into(), s("level")), ("levels".into(), jstrs(&x.levels))],
            Kind::Flag => vec![("kind".into(), s("flag"))], Kind::Number => vec![("kind".into(), s("number"))] }))).collect())),
        ("habits".into(), Json::Arr(p.habits.iter().map(|h| s(&h.id)).collect())), ("agenda".into(), jstrs(&p.steps))]);
    // the learning block (§2.8), when there is one: what moves the learned deltas, which traits, and the caps; the credit block's
    // settings when declared
    if let (Json::Obj(v), Some(l)) = (&mut d, &p.learning) { let mut lv = vec![("from".into(), jstrs(&l.from)), ("traits".into(), jstrs(&l.traits)),
        ("rate".into(), Json::Num(l.rate)), ("step_cap".into(), Json::Num(l.step_cap)), ("total_cap".into(), Json::Num(l.total_cap))];
        if let Some(c) = &l.credit { lv.push(("credit".into(), Json::Obj(vec![("max_open".into(), Json::Num(c.max_open as f64)), ("half_life_events".into(), Json::Num(c.half_life)),
            ("expires_after".into(), Json::Num(r6(4.0 * c.half_life)))]))); }
        v.push(("learning".into(), Json::Obj(lv))); }
    // the drives block (§2.9), when there is one: the goals (pursue's levels) in order, their phrases, floors and starve_after
    if let (Json::Obj(v), Some(dr)) = (&mut d, &p.drives) { let per = |f: &dyn Fn(&Goal) -> Option<Json>| Json::Obj(dr.goals.iter().filter_map(|g| f(g).map(|x| (g.id.clone(), x))).collect());
        v.push(("drives".into(), Json::Obj(vec![("goals".into(), Json::Arr(dr.goals.iter().map(|g| s(&g.id)).collect())), ("say".into(), per(&|g| Some(s(&g.say)))),
            ("floor".into(), per(&|g| (g.floor > 0.0).then_some(Json::Num(g.floor)))), ("starve_after".into(), per(&|g| g.starve_after.map(Json::Num))),
            ("learn_from_surprise".into(), Json::Num(dr.kappa))])));
        // RFC 0002 (only with their keys): the omission windows (A.4), valence's weights and the liking gain (Design E), and the
        // input ids they reserve
        let Some((_, Json::Obj(dv))) = v.last_mut() else { unreachable!() }; let mut reserved: Vec<String> = vec![];
        if dr.windows() { dv.push(("expect_by_hours".into(), per(&|g| g.expect_by.map(Json::Num)))); reserved.push("drv_omit".into()); }
        if let Some((mood, we, wp)) = &dr.valence { dv.push(("valence".into(), Json::Obj(vec![("mood".into(), s(mood)), ("expect".into(), Json::Num(*we)), ("surprise".into(), Json::Num(*wp))])));
            reserved.push("drv_expect".into()); }
        if dr.liking > 0.0 || dr.liking_spread > 0.0 { dv.push(("liking".into(), Json::Obj(vec![("gain".into(), Json::Num(dr.liking)), ("spread".into(), Json::Num(dr.liking_spread))]))); }
        if !reserved.is_empty() { dv.push(("reserved".into(), jstrs(&reserved))); }
        // the needs block (RFC 0002 A.1): the drive's n and m, the pursue term's weight and box, the success rate, the win cap; per
        // need its unit, cap, weight, growth, sources, amount, bound goal, horizon, spreads and the input id it reserves
        if let Some(nb) = &dr.needs { let n = Json::Num;
            v.push(("needs".into(), Json::Obj(vec![("drive".into(), Json::Obj(vec![("n".into(), n(nb.n)), ("m".into(), n(nb.m))])),
                ("pursue".into(), Json::Obj(vec![("want".into(), n(nb.want)), ("cap".into(), n(nb.cap))])), ("success".into(), Json::Obj(vec![("rate".into(), n(nb.rate)), ("init".into(), n(nb.init))])),
                ("r_cap".into(), n(nb.r_cap)),
                ("items".into(), Json::Obj(nb.items.iter().map(|x| (x.id.clone(), Json::Obj(vec![("say".into(), s(&x.say)), ("unit".into(), s(&x.unit)), ("cap".into(), n(x.cap)),
                    ("weight".into(), n(x.weight)), ("per_hour".into(), n(x.per_hour)),
                    ("per_reading".into(), x.reading.map_or(Json::Null, |(si, r)| Json::Obj(vec![("sense".into(), s(&p.senses.as_ref().map_or(String::new(), |sn| sn.list[si].id.clone()))), ("rate".into(), n(r))]))),
                    ("satisfy_from".into(), jstrs(&x.src)), ("amount".into(), n(x.amount)), ("bind".into(), s(&dr.goals[x.bind].id)), ("horizon_hours".into(), n(x.horizon)),
                    ("forecast_per_hour".into(), n(x.forecast)), ("spreads".into(), Json::Obj(vec![("weight".into(), n(x.weight_spread)), ("growth".into(), n(x.growth_spread))])),
                    ("reserved".into(), s(&format!("need_{}", x.id)))]))).collect()))]))); } }
    // reward_from (§2.10), when declared: per reward-bearing input its allowed source kinds, or "any"
    if let (Json::Obj(v), Some(rw)) = (&mut d, &p.reward) { v.push(("reward_from".into(), Json::Obj(rw.iter().map(|(k, a)| (k.clone(), a.as_ref().map_or(s("any"), |a| jstrs(a)))).collect()))); }
    // the senses block (§2.11), when there is one: per sense its unit, range, scale, sources, prior, caps, spreads, the input ids it
    // reserves and the conditions the habits read
    if let (Json::Obj(v), Some(sn)) = (&mut d, &p.senses) { let n = Json::Num;
        v.push(("senses".into(), Json::Obj(sn.list.iter().enumerate().map(|(i, x)| { let mut f = vec![("kind".to_string(), s(if x.flag { "flag" } else { "number" })), ("unit".into(), s(&x.unit))];
            if !x.flag { f.push(("range".into(), Json::Arr(vec![n(x.lo), n(x.hi)]))); }
            let mut reserved = vec![format!("sense_{}_up", x.id), format!("sense_{}_down", x.id)]; reserved.extend(sn.conds.iter().filter(|c| c.sense == i).map(|c| c.id.clone()));
            f.extend([("scale".to_string(), s(if x.log1p { "log1p" } else { "linear" })), ("src".into(), jstrs(&x.src)),
                ("prior".into(), Json::Obj(vec![("mean".into(), n(x.prior_mean)), ("var".into(), n(x.prior_var))])), ("drift_cap".into(), n(x.drift_cap)), ("var_cap".into(), n(x.var_cap)),
                ("half_life_events".into(), n(x.half_life)), ("zcap".into(), n(x.zcap)), ("dead_zone".into(), n(x.dead_zone)), ("var_floor".into(), n(x.var_floor)), ("say".into(), s(&x.say)),
                ("spreads".into(), Json::Obj(vec![("gain".into(), n(x.gain_spread)), ("threshold".into(), n(x.threshold_spread)), ("habituation".into(), n(x.habituation_spread))])),
                ("reserved".into(), jstrs(&reserved)),
                ("conditions".into(), Json::Arr(sn.conds.iter().filter(|c| c.sense == i).map(|c| s(&sense_cond_text(sn, c))).collect()))]);
            (x.id.clone(), Json::Obj(f)) }).collect()))); }
    d
}
/// A trait or input the drives block adds when it is lowered (`pursue`, the drive aggregates and goal conditions): not the author's
fn lowered(p: &Persona, id: &str) -> bool { p.drives.as_ref().is_some_and(|d| id == "pursue" || d.synthetic.iter().any(|x| x == id)) || sense_input(p, id) }
/// An input the senses block adds when it is lowered (`sense_<id>_up/down`, the condition flags): not the host's
fn sense_input(p: &Persona, id: &str) -> bool { p.senses.as_ref().is_some_and(|s| s.synthetic.iter().any(|x| x == id)) }
/// `check`: valid, with its digest and sizes (one engine call: the resting stance)
pub fn check(p: &Persona, eng: Engine) -> R<Json> {
    let st = init(p, None, true, eng); let (prog, _) = compile(p, &st, &[], &Opts::default())?;
    let n = |x: usize| Json::Num(x as f64); let len = |k: &str| prog.get(k).and_then(Json::as_arr).map_or(0, <[Json]>::len);
    Ok(Json::Obj(vec![("ok".into(), Json::Bool(true)), ("name".into(), Json::Str(p.name.clone())), ("version".into(), Json::Str(p.version.clone())), ("digest".into(), Json::Str(p.digest.clone())),
        ("traits".into(), n(p.vars.iter().filter(|v| !v.mood && !lowered(p, &v.id)).count())), ("moods".into(), n(p.vars.iter().filter(|v| v.mood).count())),
        ("inputs".into(), n(p.inputs.iter().filter(|x| !lowered(p, &x.id)).count())),
        ("history".into(), n(p.history.len())), ("habits".into(), n(p.habits.len())), ("steps".into(), n(p.steps.len())), ("program_vars".into(), n(len("vars"))), ("program_values".into(), n(len("values")))]
        .into_iter().chain(p.drives.as_ref().map(|d| ("goals".to_string(), n(d.goals.len())))).chain(p.drives.as_ref().and_then(|d| d.needs.as_ref()).map(|nb| ("needs".to_string(), n(nb.items.len()))))
        .chain(p.senses.as_ref().map(|sn| ("senses".to_string(), n(sn.list.len())))).collect()))
}
/// `explain`: a turn in words: per trait and mood every contribution to its field, the joint odds, the habit-free twin's level
/// where it differs, the program's size and digest, the line and the why
pub fn explain(p: &Persona, st: &State, raw: &Json, eng: Engine) -> R<String> {
    let rawv: Vec<(String, Json)> = match raw { Json::Null => vec![], Json::Obj(v) => v.clone(), _ => return Err(perr("inputs", "must be a JSON object")) };
    let (src, rawv) = if p.senses.is_some() || p.credit().is_some() || p.has_needs() { take_src(p, rawv, true)? } else { (None, rawv) };
    let (pay, rawv) = take_pay(p, rawv)?;
    let (st2, host) = prep(p, st, &rawv, src.as_ref(), true)?;
    let st2 = match &pay { Some(k) => credit_step(p, &st2, &host, k, src.as_ref(), true)?.0, None => st2 };
    let (prog, meta) = compile(p, &st2, &host, &Opts::default())?; let doc = eng(&prog, &flags(p, st)); let (out, _) = turn(p, st, raw, false, eng, false)?;
    let plan: HashMap<String, String> = doc.get("plan").and_then(Json::as_obj).map_or_else(HashMap::new, |o| o.iter().map(|(k, v)| (k.clone(), back(p, k, v.as_str().unwrap_or("")))).collect());
    let g = |k: &str| out.get(k).cloned().unwrap_or(Json::Null); let s = |j: &Json| j.as_str().map_or_else(|| canon(j), str::to_string);
    let mut l = vec![format!("persona {} {}  seed {}  turn {}  engine {} ({})  status {}", p.name, p.version, st.seed, st.turn, doc.get("verdict").map_or("None".into(), s), doc.get("tier").map_or("None".into(), s), s(&g("status")))];
    let inputs = g("inputs"); let hist = |k: &str| p.history.iter().any(|h| h.id == k);
    l.push(format!("inputs: {}", inputs.as_obj().unwrap_or(&[]).iter().map(|(k, v)| format!("{k}={}", match v { Json::Bool(b) => if *b { "True".into() } else { "False".into() },
        Json::Num(x) if hist(k) => format!("{}", *x as i64), Json::Num(x) => py_repr(*x), x => s(x) })).collect::<Vec<_>>().join(", ")));
    let names = |j: &Json| j.as_arr().map_or(String::new(), |a| a.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(", "));
    let ign = names(&g("ignored")); if !ign.is_empty() { l.push(format!("ignored inputs: {ign}")); }
    let hb = g("habits"); let (a, b) = (names(hb.get("active").unwrap_or(&Json::Null)), names(hb.get("bound").unwrap_or(&Json::Null)));
    l.push(format!("habits active: {} | bound (changed the stance vs the habit-free twin): {}", if a.is_empty() { "-" } else { &a }, if b.is_empty() { "-" } else { &b }));
    for (i, v) in p.vars.iter().enumerate() {
        let e = if p.drives.is_some() && v.id == "pursue" { let pz = g("pursue"); let f = |k: &str| pz.get(k).cloned().unwrap_or(Json::Null); // its own object (§4)
            Json::Obj(vec![("level".into(), f("goal")), ("p".into(), f("p")), ("odds".into(), f("odds"))]) }
            else { g(if v.mood { "mood" } else { "stance" }).get(&v.id).cloned().unwrap_or(Json::Null) };
        let lvl = e.get("level").map_or(String::new(), s);
        let twin = if meta.twin { plan.get(&format!("free.{}", v.id)).filter(|t| **t != lvl).map_or(String::new(), |t| format!("   habit-free twin: {t}")) } else { String::new() };
        l.push(format!("{} {} -> {} (p {:.3}){twin}", if v.mood { "mood" } else { "trait" }, v.id, lvl, e.get("p").and_then(Json::as_f64).unwrap_or(0.0)));
        let row = |vec: &[f64], fmt: &dyn Fn(f64) -> String| v.levels.iter().zip(vec).map(|(l, x)| format!("{l} {}", fmt(*x))).collect::<Vec<_>>().join("  ");
        for (src, vec) in &meta.contrib[i] { if vec.iter().any(|x| x.abs() > 5e-7) { l.push(format!("    {src:<24} {}", row(vec, &|x| format!("{x:+.2}")))); } }
        l.push(format!("    {:<24} {}", "= field (log-weights)", row(&meta.field[i], &|x| format!("{x:+.2}"))));
        let odds: Vec<f64> = v.levels.iter().map(|lv| e.get("odds").and_then(|o| o.get(lv)).and_then(Json::as_f64).unwrap_or(0.0)).collect();
        l.push(format!("    {:<24} {}", "odds (exact, joint)", row(&odds, &|x| format!("{x:.3}")))); }
    for (a, b, _) in &p.couplings { l.push(format!("coupling {a} ~ {b}")); }
    if !p.steps.is_empty() { l.push(format!("agenda: {}", names(&g("agenda")).replace(", ", " > "))); }
    let cnt: Vec<String> = ["pairs"].iter().chain(RULE_KEYS.iter()).filter_map(|k| prog.get(k).and_then(Json::as_arr).filter(|a| !a.is_empty()).map(|a| format!("{k} {}", a.len()))).collect();
    let pd = s(g("engine").get("program").unwrap_or(&Json::Null));
    l.push(format!("program: {} vars, {} values, {}; {}", prog.get("vars").and_then(Json::as_arr).map_or(0, <[Json]>::len), prog.get("values").and_then(Json::as_arr).map_or(0, <[Json]>::len), cnt.join(", "), &pd[..pd.len().min(23)]));
    l.push(format!("line ({} tokens est.): {}", g("line_tokens").as_f64().unwrap_or(0.0) as i64, s(&g("line")))); l.push(format!("why: {}", s(&g("why"))));
    Ok(l.join("\n"))
}

// ------------------------------------------------------------------------------------------- character properties (§5.6)
/// A character property: a habit-shaped rule (`when` / `then` / `rules`) an individual's stance must never break. `persona fuzz`
/// searches event scripts for a turn that breaks it; `persona prove` decides it for every event sequence where it can.
#[derive(Clone)]
pub struct Prop { pub id: String, pub rule: Json, habit: Habit }
/// A broken `then` entry: the trait (or mood), the level the stance has, the levels the property allows. Raw `rules` that break
/// are one entry with `var` = "rules".
pub struct Broken { pub var: String, pub level: String, pub allowed: Vec<String> }
/// A property document: one rule, a list of rules, or {"props": [...]}. A rule's `id` is optional: `one` names a single rule,
/// list entries default to prop1, prop2, ...; ids must be distinct.
pub fn props(p: &Persona, doc: &Json, path: &str, one: &str) -> R<Vec<Prop>> {
    let (list, single) = match doc { Json::Arr(a) => (a.to_vec(), false), Json::Obj(kv) if get(kv, "props").is_some() => {
            need(kv.len() == 1, path, "{\"props\": [...]} takes no other field")?;
            (get(kv, "props").and_then(Json::as_arr).ok_or_else(|| perr(&at(path, "props"), "a list of rules"))?.to_vec(), false) }
        Json::Obj(_) => (vec![doc.clone()], true), _ => return Err(perr(path, "a rule {when, then}, a list of rules or {\"props\": [...]}")) };
    need(!list.is_empty(), path, "at least one rule")?;
    let mut out: Vec<Prop> = vec![];
    for (i, r) in list.iter().enumerate() {
        let rp = if single { path.to_string() } else { ix(path, i) };
        let Json::Obj(kv) = r else { return Err(perr(&rp, "a rule is a mapping {when, then} (habit syntax)")) };
        let id = if get(kv, "id").is_some() { None } else if single { Some(one.to_string()) } else { Some(format!("prop{}", i + 1)) };
        let mut full = kv.clone(); if let Some(id) = &id { full.insert(0, ("id".into(), Json::Str(id.clone()))); }
        // a sense condition (§2.11) reads the flag of the persona's own condition on the same sense, signal and range
        if let (Some(sn), Some((_, Json::Obj(w)))) = (&p.senses, full.iter_mut().find(|(k, _)| k == "when")) {
            let mut nw: Vec<(String, Json)> = vec![];
            for (k, c) in w.iter() { let cp = at(&at(&rp, "when"), k);
                need(!sn.reserved(k), &cp, "reserved for the senses block (a rule reads sense.<id>.raw or sense.<id>.z)")?;
                if !k.starts_with("sense.") { nw.push((k.clone(), c.clone())); continue; }
                let (si, z, flag, lo, hi) = parse_sense_cond(k, c, &cp, &sn.list)?;
                let f = sn.conds.iter().find(|q| q.sense == si && q.z == z && q.flag == flag && q.lo == lo && q.hi == hi).ok_or_else(|| perr(&cp, format!("a property reads the sense conditions the persona's habits use: {}",
                    if sn.conds.is_empty() { "none".to_string() } else { sn.conds.iter().map(|q| sense_cond_text(sn, q)).collect::<Vec<_>>().join(", ") })))?;
                if !nw.iter().any(|(x, _)| *x == f.id) { nw.push((f.id.clone(), Json::Bool(true))); } }
            *w = nw; }
        // a goal condition (drives, §2.9) reads the flag of the persona's own condition on the same goal, signal and range
        if let (Some(d), Some((_, Json::Obj(w)))) = (&p.drives, full.iter_mut().find(|(k, _)| k == "when")) {
            let mut nw: Vec<(String, Json)> = vec![];
            for (k, c) in w.iter() { let cp = at(&at(&rp, "when"), k);
                need(!d.synthetic.contains(k), &cp, "reserved for the drives block")?;
                if !k.contains('.') { nw.push((k.clone(), c.clone())); continue; }
                let need_ids: Vec<String> = d.needs.as_ref().map_or(vec![], |nb| nb.items.iter().map(|x| x.id.clone()).collect());
                let (gi, sig, lo, hi) = parse_goal_cond(k, c, &cp, &d.goals, &need_ids)?;
                let f = d.conds.iter().find(|q| q.goal == gi && q.sig == sig && q.lo == lo && q.hi == hi).ok_or_else(|| perr(&cp, format!("a property reads the goal conditions the persona's habits use: {}",
                    if d.conds.is_empty() { "none".to_string() } else { d.conds.iter().map(|q| cond_text(d, q)).collect::<Vec<_>>().join(", ") })))?;
                if !nw.iter().any(|(x, _)| *x == f.id) { nw.push((f.id.clone(), Json::Bool(true))); } }
            *w = nw; }
        let habit = habit_spec(&Json::Obj(full), &rp, &p.inputs, &p.history, &p.vars, &p.steps)?;
        need(!out.iter().any(|q| q.id == habit.id), &at(&rp, "id"), "property ids must be distinct")?;
        let rule = Json::Obj(kv.iter().filter(|(k, _)| k != "id" && k != "comment" && k != "say" && k != "priority").cloned().collect());
        out.push(Prop { id: habit.id.clone(), rule, habit });
    }
    Ok(out)
}
/// A stance document's entry for a trait or mood; with a drives block `pursue` is its own object, the goal as the level
pub fn doc_entry(p: &Persona, doc: &Json, id: &str) -> Option<Json> {
    if p.drives.is_some() && id == "pursue" { let pz = doc.get("pursue")?; let f = |k: &str| pz.get(k).cloned().unwrap_or(Json::Null);
        return Some(Json::Obj(vec![("level".into(), f("goal")), ("odds".into(), f("odds")), ("released".into(), f("released"))])); }
    doc.get("stance").and_then(|s| s.get(id)).or_else(|| doc.get("mood").and_then(|m| m.get(id))).cloned()
}
/// The inputs a turn's conditions read, from its stance document: the host's and (drives) the aggregates and goal conditions
/// again, from the drive values the turn used
fn doc_inputs(p: &Persona, doc: &Json) -> Option<Vec<(String, Json)>> {
    let mut inputs = doc.get("inputs").and_then(Json::as_obj)?.to_vec();
    if let Some(sn) = &p.senses { inputs.retain(|(k, _)| k != "senses"); inputs.extend(doc_sense_inputs(sn, doc)?); }
    let Some(d) = &p.drives else { return Some(inputs) };
    let dj = doc.get("drives")?;
    let per = |k: &str| -> Option<Vec<Json>> { let o = dj.get(k)?; d.goals.iter().map(|g| o.get(&g.id).cloned()).collect() };
    let nums = |k: &str| -> Option<Vec<f64>> { per(k)?.iter().map(Json::as_f64).collect() };
    // the needs' deficits from the stance's `needs` (what a need.<id>.deficit condition reads; RFC 0002 A.6)
    let nd = match &d.needs { None => None, Some(nb) => { let nj = doc.get("needs")?;
        Some(NState { genes: Json::Null, deficit: nb.items.iter().map(|x| nj.get(&x.id).and_then(|e| e.get("deficit")).and_then(Json::as_f64)).collect::<Option<Vec<f64>>>()?, success: vec![0.0; nb.items.len()] }) } };
    let n = d.goals.len();
    let ds = DState { genes: Json::Null, want: nums("want")?, glow: nums("glow")?, expect: nums("expect")?, surprise: nums("surprise")?,
        deadline: per("deadline")?.iter().map(Json::as_f64).collect(), since: nums("since")?.iter().map(|x| *x as u64).collect(), window: vec![None; n], nd, omit: vec![None; n], ev: 0.0 };
    host_inputs(d, &ds, &drive_ctx(d, &ds), &inputs).ok()
}
/// Does this turn's stance break the property? `before` is the state the turn ran on (its `prev` levels resolve `prev`
/// restrictions), `doc` the turn's stance document. None: it holds, it is not in force, or the turn has no vouched stance (status
/// refused or fallback: the host then uses the habits-only line; unreleased traits of a partial turn are not checked).
pub fn breaks(p: &Persona, pr: &Prop, before: &State, doc: &Json) -> Option<Vec<Broken>> {
    let status = doc.get("status").and_then(Json::as_str)?; if status != "ok" && status != "partial" { return None; }
    let inputs = doc_inputs(p, doc)?;
    if !pr.habit.when.iter().all(|(k, c)| cond_true(p, k, c, &inputs, &[])) { return None; }
    let entry = |id: &str| doc_entry(p, doc, id);
    let mut out = vec![];
    for (k, r) in &pr.habit.then {
        let v = p.var(k)?; let e = entry(k)?; if e.get("released") != Some(&Json::Bool(true)) { continue; }
        let lvl = e.get("level").and_then(Json::as_str)?; let pl = before.prev.get(k).cloned().unwrap_or_else(|| v.fallback.clone());
        let ok = allowed(&v.levels, r, &pl);
        if !ok.iter().any(|l| l.as_str() == lvl) { out.push(Broken { var: k.clone(), level: lvl.to_string(), allowed: ok.into_iter().cloned().collect() }); } }
    if !pr.habit.rules.is_empty() && status == "ok" {
        let mut flat: HashMap<String, String> = p.vars.iter().filter_map(|v| entry(&v.id).and_then(|e| e.get("level").and_then(Json::as_str).map(|l| (v.id.clone(), l.to_string())))).collect();
        if let Some(a) = doc.get("agenda").and_then(Json::as_arr) { for (i, s) in a.iter().enumerate() { if let (Some(s), Some(slot)) = (s.as_str(), p.slots.get(i)) { flat.insert(format!("step.{s}"), slot.clone()); } } }
        let (mut rules, mut owners) = (empty_rules(), vec![]);
        habit_rules(p, &Habit { then: vec![], ..pr.habit.clone() }, &before.prev, &mut rules, &mut owners);
        if !violations(&flat, &rules, &values_of(p)).is_empty() { out.push(Broken { var: "rules".into(), level: String::new(), allowed: vec![] }); } }
    (!out.is_empty()).then_some(out)
}
/// The odds mass the stance puts on levels the property's `then` entries forbid (the largest over its entries; 0 for raw rules):
/// what the fuzzer's beam climbs
pub fn pressure(p: &Persona, pr: &Prop, before: &State, doc: &Json) -> f64 {
    let entry = |id: &str| doc_entry(p, doc, id);
    pr.habit.then.iter().filter_map(|(k, r)| { let v = p.var(k)?; let e = entry(k)?; let odds = e.get("odds")?;
        let pl = before.prev.get(k).cloned().unwrap_or_else(|| v.fallback.clone()); let ok = allowed(&v.levels, r, &pl);
        Some(v.levels.iter().filter(|l| !ok.contains(l)).map(|l| odds.get(l).and_then(Json::as_f64).unwrap_or(0.0)).fold(0.0, |a, b| a + b)) })
        .fold(0.0, f64::max)
}
/// The event alphabet: per declared input its non-default values (a flag: true; a level input: its other levels; a number: `grid`
/// x its max, without its default), then `elapsed_hours` (`hours`) when a mood has a half-life. -> (id, kind, values)
pub fn alphabet(p: &Persona, grid: &[f64], hours: &[f64]) -> Vec<(String, &'static str, Vec<Json>)> {
    let host = |x: &&Input| !p.drives.as_ref().is_some_and(|d| d.synthetic.contains(&x.id)) && !sense_input(p, &x.id); // the drives' and senses' own inputs are not the host's
    let mut out: Vec<(String, &'static str, Vec<Json>)> = p.inputs.iter().filter(host).map(|x| match x.kind {
        Kind::Flag => (x.id.clone(), "flag", vec![Json::Bool(true)]),
        Kind::Level => (x.id.clone(), "level", x.levels.iter().filter(|l| Some(l.as_str()) != x.default.as_str()).map(|l| Json::Str(l.clone())).collect()),
        Kind::Number => { let mut v: Vec<f64> = vec![]; for g in grid { let n = r6(g * x.max); if Some(n) != x.default.as_f64() && !v.contains(&n) { v.push(n); } }
            (x.id.clone(), "number", v.into_iter().map(Json::Num).collect()) } }).filter(|e| !e.2.is_empty()).collect();
    let pos_hours = || { let mut h: Vec<Json> = vec![]; for x in hours.iter().filter(|h| **h > 0.0) { if !h.contains(&Json::Num(*x)) { h.push(Json::Num(*x)); } } h };
    if (p.vars.iter().any(|v| v.mood && v.half_life.is_some()) || p.drives.is_some()) && hours.iter().any(|h| *h > 0.0) { out.push(("elapsed_hours".into(), "hours", pos_hours())); }
    // drives (§2.9): per goal a win on the grid x win_max, a cue, a deadline on the hours (`goal.<id>.<signal>`; `event_json` nests them)
    if let Some(d) = &p.drives { for g in &d.goals {
        let mut w: Vec<Json> = vec![]; for x in grid { let n = r6(x * d.win_max); if n > 0.0 && !w.contains(&Json::Num(n)) { w.push(Json::Num(n)); } }
        if !w.is_empty() { out.push((format!("goal.{}.win", g.id), "goal", w)); }
        out.push((format!("goal.{}.cue", g.id), "goal", vec![Json::Bool(true)]));
        let h = pos_hours(); if !h.is_empty() { out.push((format!("goal.{}.deadline_hours", g.id), "goal", h)); } }
        // RFC 0002: windows expire on the stamped hours (each window's length joins the idle hours); per need its amounts on the
        // grid x (0, cap] (`need.<id>`; `event_json` nests them)
        if d.windows() { let mut h: Vec<Json> = out.iter().find(|e| e.0 == "elapsed_hours").map_or(vec![], |e| e.2.clone());
            for g in &d.goals { if let Some(w) = g.expect_by { if !h.contains(&Json::Num(w)) { h.push(Json::Num(w)); } } }
            match out.iter_mut().find(|e| e.0 == "elapsed_hours") { Some(e) => e.2 = h, None => out.push(("elapsed_hours".into(), "hours", h)) } }
        if let Some(nb) = &d.needs { for x in &nb.items { let mut v: Vec<Json> = vec![];
            for g in grid { let a = r6(g * x.cap); if a > 0.0 && !v.contains(&Json::Num(a)) { v.push(Json::Num(a)); } }
            if !v.is_empty() { out.push((format!("need.{}", x.id), "need", v)); } } } }
    // senses (§2.11): per sense its readings (`sense.<id>`; `event_json` nests them): a flag both ways, a number on the grid x its
    // range and at every threshold a raw condition reads
    if let Some(sn) = &p.senses { for (i, x) in sn.list.iter().enumerate() {
        let vals: Vec<Json> = if x.flag { vec![Json::Bool(true), Json::Bool(false)] } else { let mut v: Vec<f64> = vec![];
            for g in grid { let y = r6(x.lo + g * (x.hi - x.lo)); if !v.contains(&y) { v.push(y); } }
            for c in sn.conds.iter().filter(|c| c.sense == i && !c.z) { for t in [c.lo, c.hi] { if t.is_finite() && t >= x.lo && t <= x.hi && !v.contains(&t) { v.push(t); } } }
            v.into_iter().map(Json::Num).collect() };
        out.push((format!("sense.{}", x.id), "sense", vals)); } }
    out
}
/// The omission windows' lengths (RFC 0002 A.4), each once, in the goals' order: the idle hours at which a pursued goal's window
/// fires (the fuzzer's expiring-window pattern); none without a window
pub fn window_hours(p: &Persona) -> Vec<f64> {
    let mut v: Vec<f64> = vec![]; for g in p.drives.iter().flat_map(|d| d.goals.iter()) { if let Some(w) = g.expect_by { if !v.contains(&w) { v.push(w); } } } v
}
/// A sense's readings for the fuzzer's patterns: (the alphabet id, its readings, the range ends) per sense
pub fn sense_patterns(p: &Persona, grid: &[f64]) -> Vec<(String, Vec<Json>, (Json, Json))> {
    let Some(sn) = &p.senses else { return vec![] };
    alphabet(p, grid, &[]).into_iter().filter(|(_, k, _)| *k == "sense").map(|(id, _, vals)| { let s = &sn.list[sn.list.iter().position(|x| format!("sense.{}", x.id) == id).unwrap()];
        let ends = if s.flag { (Json::Bool(false), Json::Bool(true)) } else { (Json::Num(s.lo), Json::Num(s.hi)) }; (id, vals, ends) }).collect()
}
/// The thresholds the rules and the habits read on number inputs, on the grid's scale (a fraction of the input's max), each once,
/// in ascending order: the values at which a condition flips, so the search can put an input exactly there
pub fn thresholds(p: &Persona, props: &[Prop]) -> Vec<f64> {
    let mut out: Vec<f64> = vec![];
    for w in p.habits.iter().map(|h| &h.when).chain(props.iter().map(|q| &q.habit.when)) { for (k, c) in w {
        // the host's number inputs (the drives' own inputs are not on the search's alphabet)
        let Some(x) = p.input(k).filter(|x| x.kind == Kind::Number && !p.drives.as_ref().is_some_and(|d| d.synthetic.contains(&x.id)) && !sense_input(p, &x.id)) else { continue };
        let ts: Vec<f64> = match c { Json::Num(t) => vec![*t], _ => ["at_least", "at_most"].iter().filter_map(|b| c.get(b).and_then(Json::as_f64)).collect() };
        for t in ts { if t >= 0.0 && t <= x.max { let g = t / x.max; if !out.contains(&g) { out.push(g); } } } } }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap()); out
}
/// An event of the alphabet as turn inputs: `goal.<id>.<signal>` entries nested into `goals`, `sense.<id>` into `senses` and
/// `need.<id>` into `needs` (after the host's inputs)
pub fn event_json(e: &[(String, Json)]) -> Json {
    let mut v: Vec<(String, Json)> = vec![]; let mut goals: Vec<(String, Json)> = vec![]; let mut senses: Vec<(String, Json)> = vec![]; let mut needs: Vec<(String, Json)> = vec![];
    for (k, x) in e { if let Some(sid) = k.strip_prefix("sense.") { senses.push((sid.to_string(), x.clone())); continue; }
        if let Some(nid) = k.strip_prefix("need.") { needs.push((nid.to_string(), x.clone())); continue; }
        match k.strip_prefix("goal.").and_then(|r| r.split_once('.')) {
        Some((g, sig)) => match goals.iter_mut().find(|(id, _)| id == g) { Some((_, Json::Obj(o))) => o.push((sig.to_string(), x.clone())), _ => goals.push((g.to_string(), Json::Obj(vec![(sig.to_string(), x.clone())]))) },
        None => v.push((k.clone(), x.clone())) } }
    if !goals.is_empty() { v.push(("goals".into(), Json::Obj(goals))); }
    if !senses.is_empty() { v.push(("senses".into(), Json::Obj(senses))); }
    if !needs.is_empty() { v.push(("needs".into(), Json::Obj(needs))); }
    Json::Obj(v)
}
/// The input assignments that put the property in force: one per combination of its input conditions (a level list gives one
/// per level; a number the smallest grid value in range). History conditions are left to the search. [] = no input can.
pub fn forcing(p: &Persona, pr: &Prop, grid: &[f64]) -> Vec<Vec<(String, Json)>> {
    let mut bases: Vec<Vec<(String, Json)>> = vec![vec![]];
    for (k, c) in &pr.habit.when {
        if let Some((sn, q)) = p.senses.as_ref().and_then(|sn| sn.conds.iter().find(|q| q.id == *k).map(|q| (sn, q))) { // a sense condition: a raw one by its reading
            let s = &sn.list[q.sense]; if q.z { continue; } // a z condition depends on the normal: left to the search
            let v = match q.flag { Some(b) => Json::Bool(b), None => { let mut v: Vec<f64> = grid.iter().map(|g| r6(s.lo + g * (s.hi - s.lo))).filter(|v| *v >= q.lo && *v <= q.hi).collect();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap()); let b = q.lo.max(s.lo).min(s.hi);
                match v.first() { Some(x) => Json::Num(*x), None if b >= q.lo && b <= q.hi => Json::Num(b), None => continue } } };
            let key = format!("sense.{}", s.id);
            bases = bases.iter().map(|b| { let mut nb = b.clone(); if !nb.iter().any(|(x, _)| *x == key) { nb.push((key.clone(), v.clone())); } nb }).collect(); continue; }
        if let Some(q) = p.drives.as_ref().and_then(|d| d.conds.iter().find(|q| q.id == *k)) { // a goal condition: a deadline puts it in force at once
            let v = if q.hi.is_finite() { q.hi } else { q.lo.max(0.0) };
            if q.sig != "deadline_hours" || !(v >= q.lo && v <= q.hi && (0.0..=1e6).contains(&v)) { continue; }
            let key = format!("goal.{}.deadline_hours", p.drives.as_ref().unwrap().goals[q.goal].id);
            bases = bases.iter().map(|b| { let mut nb = b.clone(); nb.push((key.clone(), Json::Num(v))); nb }).collect(); continue; }
        let Some(x) = p.input(k) else { continue };
        let opts: Vec<Json> = match x.kind {
            Kind::Flag => vec![c.clone()],
            Kind::Level => match c { Json::Arr(a) => a.to_vec(), _ => vec![c.clone()] },
            Kind::Number => { let (lo, hi) = match c { Json::Num(t) => (*t, f64::INFINITY), _ => (c.get("at_least").and_then(Json::as_f64).unwrap_or(f64::NEG_INFINITY), c.get("at_most").and_then(Json::as_f64).unwrap_or(f64::INFINITY)) };
                let mut v: Vec<f64> = grid.iter().map(|g| r6(g * x.max)).filter(|v| *v >= lo && *v <= hi).collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                if v.is_empty() { let b = lo.max(0.0).min(x.max); if b >= lo && b <= hi { v.push(b); } }
                v.into_iter().take(1).map(Json::Num).collect() } };
        bases = bases.iter().flat_map(|b| opts.iter().map(move |o| { let mut nb = b.clone(); nb.push((k.clone(), o.clone())); nb })).collect();
    }
    bases
}

// ---------------------------------------------------------------------------------------------- persona prove (§5.6)
/// Is the property's condition `cp` inside a habit's condition `ch` on the same input or history feature?
fn within(p: &Persona, k: &str, cp: &Json, ch: &Json) -> bool {
    let iv = |c: &Json| match c { Json::Num(t) => (*t, f64::INFINITY), _ => (c.get("at_least").and_then(Json::as_f64).unwrap_or(f64::NEG_INFINITY), c.get("at_most").and_then(Json::as_f64).unwrap_or(f64::INFINITY)) };
    let set = |c: &Json| -> Vec<String> { match c { Json::Str(s) => vec![s.clone()], _ => strs(c).unwrap_or_default() } };
    match p.input(k).map(|x| x.kind) {
        Some(Kind::Flag) => cp == ch,
        Some(Kind::Level) => set(cp).iter().all(|l| set(ch).contains(l)),
        _ => { let ((a1, b1), (a2, b2)) = (iv(cp), iv(ch)); a1 >= a2 && b1 <= b2 } }
}
/// The habits that imply the property by their text, among those `usable` allows: in force whenever it is (their conditions follow
/// from its own), and their `then` restrictions allow no level it forbids, for every previous level (per restricted trait one habit
/// that does it alone if there is one, else all of them together). None when they do not.
fn implying(p: &Persona, pr: &Prop, usable: &dyn Fn(&Habit) -> bool) -> Option<Vec<String>> {
    if pr.habit.then.is_empty() || !pr.habit.rules.is_empty() { return None; }
    let sure: Vec<&Habit> = p.habits.iter().filter(|h| usable(h) && h.when.iter().all(|(k, ch)| pr.habit.when.iter().any(|(k2, cp)| k2 == k && within(p, k, cp, ch)))).collect();
    let mut used: Vec<String> = vec![];
    for (k, r) in &pr.habit.then {
        let v = p.var(k)?; let hs: Vec<&Habit> = sure.iter().copied().filter(|h| h.then.iter().any(|(t, _)| t == k)).collect();
        let implies = |hs: &[&Habit]| v.levels.iter().all(|pl| { let mut inter: Vec<&String> = v.levels.iter().collect();
            for h in hs { let a = allowed(&v.levels, &h.then.iter().find(|(t, _)| t == k).unwrap().1, pl); inter.retain(|l| a.contains(l)); }
            let ap = allowed(&v.levels, r, pl); inter.iter().all(|l| ap.contains(l)) });
        let pick: Vec<&Habit> = match hs.iter().find(|h| implies(&[**h])) { Some(h) => vec![*h], None if !hs.is_empty() && implies(&hs) => hs.clone(), None => return None };
        for h in pick { if !used.contains(&h.id) { used.push(h.id.clone()); } } }
    Some(used)
}
/// Held by construction -> the habits it follows from. Under `on_conflict: fallback` a turn whose habits conflict has no stance, so
/// implying habits are enough. Under `yield` a habit counts when no turn where the property is in force ever drops it: whether a
/// turn's rules conflict, and which habit yields, is a function of the habits in force and the previous levels (not of genes or
/// moods), so one scan of every cell decides it.
pub fn by_construction(p: &Persona, pr: &Prop, eng: Engine) -> Option<Vec<String>> {
    let hs = implying(p, pr, &|_| true)?;
    if !p.yields() { return Some(hs); }
    let st0 = init(p, Some(p.seed), false, eng); let mut seen: std::collections::HashSet<String> = Default::default();
    let (mut dropped, mut calls, mut bad): (Vec<String>, usize, bool) = (vec![], 0, false);
    cells(p, pr, &mut |raw, _, hist, prev, _| {
        let mut st = st0.clone(); st.prev = prev.clone(); let base = Opts { twin: Some(false), hist: Some(hist.to_vec()), ..Default::default() };
        let Ok((_, meta)) = compile(p, &st, raw, &base) else { bad = true; return false };
        // the yields are a function of the rules in force and their owners
        if !seen.insert(format!("{}|{}", canon(&Json::Arr(meta.rules.iter().map(|l| Json::Arr(l.clone())).collect())), meta.owners.join(","))) { return true; }
        match first_answer(p, &st, raw, &base, eng, &mut calls) { Ok((_, _, _, ex)) => { for h in ex { if !dropped.contains(&h) { dropped.push(h); } } true } Err(_) => { bad = true; false } } });
    if bad { return None; }
    implying(p, pr, &|h| !dropped.contains(&h.id))
}
/// The turn's answer before holds, as `turn` reaches it: the solve with every habit in force and, under `on_conflict: yield`, the re-solves
/// that drop the lowest-ranked habit of each conflict -> (program, answer, meta, the habits dropped)
fn first_answer(p: &Persona, st: &State, raw: &[(String, Json)], base: &Opts, eng: Engine, calls: &mut usize) -> R<(Json, Json, Meta, Vec<String>)> {
    let (mut prog, mut meta) = compile(p, st, raw, base)?; let mut doc = solve(p, st, &prog, eng, calls); let mut ex: Vec<String> = vec![];
    let infeasible = |d: &Json| d.get("verdict").and_then(Json::as_str) == Some("infeasible");
    if infeasible(&doc) && !meta.active.is_empty() && p.eng.yields {
        let mut cf = habit_conflict(p, st, raw, &meta.active, base, eng, calls)?;
        while !cf.is_empty() {
            let mut rank = cf.clone(); rank.sort_by(|a, b| { let (ha, hb) = (p.habit(a), p.habit(b)); ha.priority.partial_cmp(&hb.priority).unwrap().then(p.habit_index(b).cmp(&p.habit_index(a))) });
            ex.push(rank[0].clone()); let o = Opts { exclude: ex.clone(), ..base.clone() }; (prog, meta) = compile(p, st, raw, &o)?; doc = solve(p, st, &prog, eng, calls);
            if !infeasible(&doc) { break; }
            cf = habit_conflict(p, st, raw, &meta.active, &o, eng, calls)?; } }
    Ok((prog, doc, meta, ex))
}
/// The variables a habit's raw rule links (they must sit in one component of the program)
fn rule_vars(key: &str, r: &Json) -> Vec<String> {
    let s = |x: Option<&Json>| x.and_then(Json::as_str).map(str::to_string);
    match key {
        "caps" => r.get("members").and_then(Json::as_arr).map_or(vec![], |m| m.iter().filter_map(|m| s(m.as_arr().and_then(|a| a.first()))).collect()),
        "implies" => [s(r.get("if").and_then(|i| i.get("var"))), s(r.get("then").and_then(|t| t.get("var")))].into_iter().flatten().collect(),
        "linear" => r.get("terms").and_then(Json::as_arr).map_or(vec![], |t| t.iter().filter_map(|t| s(t.as_arr().and_then(|a| a.first()))).collect()),
        "precedes" => [s(r.get("before")), s(r.get("after"))].into_iter().flatten().collect(),
        _ => r.get("vars").and_then(Json::as_arr).map_or(vec![], |a| a.iter().filter_map(|x| s(Some(x))).collect()) }
}
/// The ids in `k`'s component of every turn's program: linked by a coupling, by any habit's raw rule, or (agenda steps) by the
/// agenda's all_different. The joint plan is the best plan of each component on its own, so nothing outside this component (no mood
/// outside it either) can move `k`'s level.
fn component(p: &Persona, k: &str) -> Vec<String> {
    let mut links: Vec<(String, String)> = p.couplings.iter().map(|(a, b, _)| (a.clone(), b.clone())).collect();
    for h in &p.habits { for (key, lst) in &h.rules { for r in lst { let ids = rule_vars(key, r); for w in ids.windows(2) { links.push((w[0].clone(), w[1].clone())); } } } }
    let steps: Vec<String> = p.steps.iter().map(|s| format!("step.{s}")).collect(); for w in steps.windows(2) { links.push((w[0].clone(), w[1].clone())); }
    let mut comp = vec![k.to_string()]; let mut grew = true;
    while grew { grew = false; for (a, b) in &links { let (ia, ib) = (comp.contains(a), comp.contains(b)); if ia != ib { comp.push(if ia { b.clone() } else { a.clone() }); grew = true; } } }
    comp
}
/// A plan's score in a program: the sum of its unaries and pair log-weights over the program's own variables (the habit-free
/// twin's `free.` copies left out). The engine's plan on the exact tier is the highest-scoring one that obeys the rules.
fn score(prog: &Json, plan: &Json) -> f64 {
    let vals: Vec<&str> = prog.get("values").and_then(Json::as_arr).map_or(vec![], |a| a.iter().filter_map(Json::as_str).collect());
    let val = |var: &str| if var.starts_with("free.") { None } else { plan.get(var).and_then(Json::as_str) };
    let mut s = 0.0;
    for v in prog.get("vars").and_then(Json::as_arr).unwrap_or(&[]) { if let Some(x) = v.get("id").and_then(Json::as_str).and_then(val) { s += v.get("h").and_then(|h| h.get(x)).and_then(Json::as_f64).unwrap_or(0.0); } }
    for q in prog.get("pairs").and_then(Json::as_arr).unwrap_or(&[]) {
        let (Some(a), Some(b)) = (q.get("i").and_then(Json::as_str).and_then(val), q.get("j").and_then(Json::as_str).and_then(val)) else { continue };
        let (Some(ia), Some(ib)) = (vals.iter().position(|x| *x == a), vals.iter().position(|x| *x == b)) else { continue };
        s += q.get("table").and_then(|t| t.as_arr()).and_then(|t| t.get(ia)).and_then(|r| r.as_arr()).and_then(|r| r.get(ib)).and_then(Json::as_f64).unwrap_or(0.0); }
    s
}
/// Every cell of the turns where the property is in force: the inputs (flags, levels; a number at each threshold a habit or the
/// property reads and at the midpoint of each open interval between them, with its half-width), the history values that fit the
/// turn's own flags, and the previous levels of the traits a prev-relative restriction in force reads (the rest at their fallback).
/// f(inputs, half-widths, history, prev, prev-relative traits) returns false to stop.
#[allow(clippy::type_complexity)]
fn cells(p: &Persona, pr: &Prop, f: &mut dyn FnMut(&[(String, Json)], &[(String, f64)], &[(String, f64)], &HashMap<String, String>, &[String]) -> bool) {
    let thresholds = |id: &str| -> Vec<f64> { let mut t = vec![];
        for w in p.habits.iter().map(|h| &h.when).chain(std::iter::once(&pr.habit.when)) { for (_, c) in w.iter().filter(|(k, _)| k == id) {
            match c { Json::Num(x) => t.push(*x), _ => for b in ["at_least", "at_most"] { if let Some(x) = c.get(b).and_then(Json::as_f64) { t.push(x); } } } } }
        t };
    let in_force = |k: &str, v: &Json| pr.habit.when.iter().filter(|(x, _)| x == k).all(|(_, c)| cond_true(p, k, c, &[(k.to_string(), v.clone())], &[(k.to_string(), v.as_f64().unwrap_or(0.0))]));
    let sort = |v: &mut Vec<f64>| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v.dedup(); };
    let mut axes: Vec<(String, Vec<(Json, f64)>)> = vec![];
    for x in &p.inputs { let vals: Vec<(Json, f64)> = match x.kind {
            Kind::Flag => vec![(Json::Bool(false), 0.0), (Json::Bool(true), 0.0)],
            Kind::Level => x.levels.iter().map(|l| (Json::Str(l.clone()), 0.0)).collect(),
            // a condition flips at a threshold or nowhere: each threshold in range is a point, each interval between them (the
            // minimum, 0 but a need term's -pursue.cap, and the maximum close the ends) its midpoint with its half-width
            Kind::Number => { let mut th: Vec<f64> = thresholds(&x.id).into_iter().filter(|t| *t >= x.min && *t <= x.max).collect(); sort(&mut th);
                let mut pts = th.clone(); pts.push(x.min); pts.push(x.max); sort(&mut pts); let mut v = vec![];
                for (i, t) in pts.iter().enumerate() { if th.contains(t) { v.push((Json::Num(*t), 0.0)); }
                    if let Some(u) = pts.get(i + 1) { v.push((Json::Num((t + u) / 2.0), (u - t) / 2.0)); } }
                v } };
        axes.push((x.id.clone(), vals.into_iter().filter(|(v, _)| in_force(&x.id, v)).collect())); }
    let fallback: HashMap<String, String> = p.vars.iter().map(|v| (v.id.clone(), v.fallback.clone())).collect();
    let prevrel = |h: &Habit| -> Vec<String> { h.then.iter().filter(|(_, r)| r.as_obj().is_some_and(|o| o.iter().any(|(_, x)| x.as_str().is_some_and(|s| s.starts_with("prev"))))).map(|(k, _)| k.clone()).collect() };
    let pick = |dims: &[usize], c: usize| -> Vec<usize> { let mut idx = vec![0; dims.len()]; let mut r = c; for d in (0..dims.len()).rev() { idx[d] = r % dims[d]; r /= dims[d]; } idx };
    let dims: Vec<usize> = axes.iter().map(|a| a.1.len()).collect();
    for c in 0..dims.iter().product::<usize>() {
        let idx = pick(&dims, c);
        let raw: Vec<(String, Json)> = axes.iter().zip(&idx).map(|((k, v), &i)| (k.clone(), v[i].0.clone())).collect();
        let hw: Vec<(String, f64)> = axes.iter().zip(&idx).map(|((k, v), &i)| (k.clone(), v[i].1)).collect();
        // history values that fit the turn's own flag (streak: the flag on <=> at least 1; recent: the flag on -> at least 1)
        let hax: Vec<(String, Vec<f64>)> = p.history.iter().map(|h| { let on = get(&raw, &h.of) == Some(&Json::Bool(true));
            let top = if h.streak { h.cap } else { h.cap.min(h.window) };
            let vals: Vec<f64> = if on { (1..=top).map(|x| x as f64).collect() } else if h.streak { vec![0.0] } else { (0..=top.min(h.window - 1)).map(|x| x as f64).collect() };
            (h.id.clone(), vals.into_iter().filter(|x| in_force(&h.id, &Json::Num(*x))).collect()) }).collect();
        let hdims: Vec<usize> = hax.iter().map(|a| a.1.len()).collect();
        for hc in 0..hdims.iter().product::<usize>() {
            let hi = pick(&hdims, hc);
            let hist: Vec<(String, f64)> = hax.iter().zip(&hi).map(|((k, v), &i)| (k.clone(), v[i])).collect();
            let mut rel: Vec<String> = p.habits.iter().filter(|h| h.when.iter().all(|(k, c)| cond_true(p, k, c, &raw, &hist))).flat_map(prevrel).chain(prevrel(&pr.habit)).collect();
            rel.sort(); rel.dedup();
            let pdims: Vec<usize> = rel.iter().map(|k| p.var(k).unwrap().levels.len()).collect();
            for pc in 0..pdims.iter().product::<usize>() {
                let mut prev = fallback.clone(); for (k, i) in rel.iter().zip(pick(&pdims, pc)) { prev.insert(k.clone(), p.var(k).unwrap().levels[i].clone()); }
                if !f(&raw, &hw, &hist, &prev, &rel) { return; } } } }
}
/// A drive coordinate of a `prove` cell by name (None for a host input): the largest wanting / afterglow, the summed prediction
/// error above / below expectation, a goal condition as text
fn drive_name(p: &Persona, id: &str) -> Option<String> {
    let d = p.drives.as_ref().filter(|d| d.synthetic.iter().any(|x| x == id))?;
    Some(match id { "drv_want" => "want (largest)".into(), "drv_glow" => "glow (largest)".into(), "drv_surprise" => "surprise (above expectation)".into(),
        "drv_letdown" => "surprise (below expectation)".into(), "drv_expect" => "expected value (the pursued goal)".into(), "drv_omit" => "omitted expected win".into(),
        _ if id.starts_with("need_") && d.needs.is_some() => format!("{} term", id.strip_prefix("need_").unwrap_or(id)),
        _ => d.conds.iter().find(|q| q.id == id).map_or_else(|| id.to_string(), |q| cond_text(d, q)) })
}
/// A cell's inputs: the host's, and (drives) the drive coordinates by name
fn cell_inputs(p: &Persona, raw: &[(String, Json)]) -> (Json, Option<Json>) {
    let host = Json::Obj(raw.iter().filter(|(k, _)| drive_name(p, k).is_none()).cloned().collect());
    (host, p.drives.as_ref().map(|_| Json::Obj(raw.iter().filter_map(|(k, v)| drive_name(p, k).map(|n| (n, v.clone()))).collect())))
}
/// `prove` for one individual: Ok((cells, engine calls)) when the property holds on every turn of every event sequence; Err((the
/// cell that failed, why)) when the bound cannot decide it (an unknown, not a counterexample).
///
/// The bound. A turn's program is fixed by the turn's inputs, its history feature values, the previous levels and the mood
/// accumulators. The accumulators a_t = k d a_(t-1) + (1 - k) e_t (a_0 = 0, 0 < d <= 1) stay in a box: each component between
/// min(0, smallest e) and max(0, largest e) over every input and history value, widened for the 6-decimal rounding. Inputs,
/// history values and previous levels take finitely many cells (`cells`). Per cell, with the accumulators at the cell's own reference,
/// the best plan whose level the property allows must beat the best plan it forbids by more than the most the box (moods in the
/// property trait's component) and the number intervals can move the two scores apart, plus the rounding. Each held trait
/// (`vouch` + `hold`) adds the turn's re-solve with it clamped; a conflict is resolved as the turn resolves it (yield, or fallback:
/// a fallback turn has no stance to check). The engine runs the turn's own program (with its habit-free twin, as the turn does), and
/// every answer the bound reads must be exact.
pub fn prove_seed(p: &Persona, pr: &Prop, seed: u64, eng: Engine) -> Result<(usize, usize), (Json, String)> {
    if !pr.habit.rules.is_empty() { return Err((Json::Null, "raw rules are not covered by the bound".into())); }
    let st0 = init(p, Some(seed), false, eng); let gain = |id: &str| input_gain(p, &st0, id);
    let eff_of = |e: &Eff, t: &str| e.iter().find(|(x, _)| x == t).map(|(_, v)| v.clone());
    let mut boxes: Vec<(String, Vec<f64>, Vec<f64>)> = vec![];
    for v in p.vars.iter().filter(|v| v.mood) { let n = v.levels.len(); let (mut lo, mut hi) = (vec![0.0; n], vec![0.0; n]);
        let mut add = |opts: Vec<Vec<f64>>| { for l in 0..n { lo[l] += opts.iter().map(|w| w[l]).fold(f64::INFINITY, f64::min); hi[l] += opts.iter().map(|w| w[l]).fold(f64::NEG_INFINITY, f64::max); } };
        for x in &p.inputs { let g = gain(&x.id);
            match x.kind {
                Kind::Flag => if let Some(e) = eff_of(&x.effects, &v.id) { add(vec![vec![0.0; n], e.iter().map(|w| g * w).collect()]); },
                Kind::Number => if let Some(e) = eff_of(&x.effects, &v.id) { let lo = if x.min < 0.0 { e.iter().map(|w| g * x.min * w).collect() } else { vec![0.0; n] };
                    add(vec![lo, e.iter().map(|w| g * x.max * w).collect()]); },
                Kind::Level => add(x.levels.iter().map(|l| x.by_level.iter().find(|(b, _)| b == l).and_then(|(_, e)| eff_of(e, &v.id)).map_or(vec![0.0; n], |e| e.iter().map(|w| g * w).collect())).collect()) } }
        for h in &p.history { if let Some(e) = eff_of(&h.effects, &v.id) { add(vec![vec![0.0; n], e.iter().map(|w| h.cap as f64 * w).collect()]); } }
        let m = 5e-7 / (1.0 - v.inertia) + 1e-9;
        boxes.push((v.id.clone(), lo.iter().map(|x| x.min(0.0) - m).collect(), hi.iter().map(|x| x.max(0.0) + m).collect())); }
    let holdable: Vec<(String, String)> = p.vars.iter().filter(|v| !v.mood && v.vouch > 0.0).filter_map(|v| v.hold.clone().map(|h| (v.id.clone(), h))).collect();
    let comps: Vec<Vec<String>> = pr.habit.then.iter().map(|(k, _)| component(p, k)).collect();
    let nv = p.vars.len() + p.steps.len(); let eps = 2e-6 * nv as f64 + 1e-9;
    let verdict = |d: &Json| d.get("verdict").and_then(Json::as_str).unwrap_or("").to_string();
    let (mut n, mut calls) = (0usize, 0usize); let mut res = Ok(());
    cells(p, pr, &mut |raw, hw, hist, prev, rel| {
        n += 1;
        let mut st = st0.clone(); st.prev = prev.clone();
        let cell = || { let (host, drv) = cell_inputs(p, raw); let mut c = vec![("inputs".into(), host), ("history".into(), Json::Obj(hist.iter().map(|(k, x)| (k.clone(), Json::Num(*x))).collect())),
            ("prev".into(), Json::Obj(rel.iter().map(|k| (k.clone(), Json::Str(prev[k].clone()))).collect())),
            ("number_halfwidth".into(), Json::Obj(hw.iter().filter(|(_, w)| *w > 0.0).map(|(k, w)| (drive_name(p, k).unwrap_or_else(|| k.clone()), Json::Num(*w))).collect()))];
            if let Some(dv) = drv { c.push(("drives".into(), dv)); } Json::Obj(c) };
        let fail = |why: String| (cell(), why);
        let mut check = || -> Result<(), (Json, String)> {
            let base = Opts { hist: Some(hist.to_vec()), ..Default::default() };
            let (prog0, doc0, meta0, excl) = first_answer(p, &st, raw, &base, eng, &mut calls).map_err(|e| fail(e.msg))?;
            if verdict(&doc0) == "infeasible" { return Ok(()); } // a fallback turn: no stance to check
            if verdict(&doc0) != "exact" { return Err(fail(format!("the engine answered {}, not exact", verdict(&doc0)))); }
            for q in 0..1usize << holdable.len() {
                let clamps: Vec<(String, String)> = holdable.iter().enumerate().filter(|(i, _)| q >> i & 1 == 1).map(|(_, x)| x.clone()).collect();
                // the turn's re-solve of held traits compiles every habit in force (no yield) with the clamps; the turn uses it when the
                // engine vouches for it
                let (prog, doc, meta) = if q == 0 { (prog0.clone(), doc0.clone(), meta0.clone()) } else {
                    let (pg, me) = compile(p, &st, raw, &Opts { clamps: clamps.clone(), ..base.clone() }).map_err(|e| fail(e.msg))?; calls += 1; let dc = eng(&pg, &flags(p, &st)); let v = verdict(&dc);
                    match v.as_str() { "exact" => (pg, dc, me), "diagnostics_passed" => return Err(fail("a held re-solve answered diagnostics_passed, not exact".into())), _ => continue } };
                let ex = if q == 0 { excl.clone() } else { vec![] };
                let plan = doc.get("plan").cloned().unwrap_or(Json::Null);
                let level = |id: &str| plan.get(id).and_then(Json::as_str).map(|x| back(p, id, x)).unwrap_or_default();
                for ((k, rr), comp) in pr.habit.then.iter().zip(&comps) {
                    let v = p.var(k).unwrap(); let pl = st.prev.get(k).cloned().unwrap_or_else(|| v.fallback.clone()); let ok = allowed(&v.levels, rr, &pl);
                    if let Some((_, h)) = clamps.iter().find(|(t, _)| t == k) { if !ok.contains(&h) { return Err(fail(format!("{k} held at {h}"))); } continue; }
                    let lv = level(k); if !ok.iter().any(|l| **l == lv) { return Err(fail(format!("{k} {lv} with the moods at the cell's reference"))); }
                    let s_a = score(&prog, &plan); let mut s_not = f64::NEG_INFINITY;
                    for l in v.levels.iter().filter(|l| !ok.contains(l)) {
                        let mut cl = clamps.clone(); cl.push((k.clone(), l.clone()));
                        let (pg, _) = compile(p, &st, raw, &Opts { clamps: cl, exclude: ex.clone(), twin: Some(false), ..base.clone() }).map_err(|e| fail(e.msg))?; calls += 1;
                        let dc = eng(&pg, &flags(p, &st)); let vd = verdict(&dc);
                        match vd.as_str() { "infeasible" => continue, "exact" => {} _ => return Err(fail(format!("a clamped solve answered {vd}, not exact"))) }
                        s_not = s_not.max(score(&pg, dc.get("plan").unwrap_or(&Json::Null))); }
                    if s_not == f64::NEG_INFINITY { continue; } // no stance with a forbidden level obeys the rules
                    let mut bound = eps;
                    for (m, lo, hi) in boxes.iter().filter(|b| comp.contains(&b.0)) { let r = &meta.new_mood.iter().find(|(id, _)| id == m).unwrap().1; let zl = p.var(m).unwrap().levels.iter().position(|x| *x == level(m)).unwrap_or(0);
                        bound += hi.iter().zip(r).map(|(u, a)| u - a).fold(f64::NEG_INFINITY, f64::max) + r[zl] - lo[zl]; }
                    // learned deltas (§2.8) of traits in the component: anywhere in ±total_cap per level, whatever the feedback was
                    if let Some(lr) = &p.learning { for (t, r) in meta.new_learned.iter().filter(|(t, _)| comp.contains(t)) { let zl = p.var(t).unwrap().levels.iter().position(|x| *x == level(t)).unwrap_or(0);
                        bound += r.iter().map(|a| lr.total_cap - a).fold(f64::NEG_INFINITY, f64::max) + r[zl] + lr.total_cap; } }
                    // drives (§2.9): `pursue`'s wanting, afterglow and deadline terms anywhere in their boxes (W in [0, cap_W], A in [0, cap_A],
                    // urgency in [0, 1]) and the floor lift anywhere from 0 to the most it reaches over them
                    if let (Some(d), Some(pi)) = (&p.drives, p.vars.iter().position(|v| v.id == "pursue")) { if comp.iter().any(|c| c == "pursue") {
                        let n = d.goals.len(); let span = |w: f64, cap: f64| ((w * cap).min(0.0), (w * cap).max(0.0));
                        let (sw, sa, sd) = (span(d.w_want, d.cap_w), span(d.w_glow, d.cap_a), span(d.w_deadline, 1.0));
                        let (dlo, dhi) = (sw.0 + sa.0 + sd.0 - 3e-6, sw.1 + sa.1 + sd.1 + 3e-6);
                        let part = |srcs: &[&str]| -> Vec<f64> { let mut v = vec![0.0; n]; for (_, w) in meta.contrib[pi].iter().filter(|(s, _)| srcs.contains(&s.as_str())) { for (a, b) in v.iter_mut().zip(w) { *a += b; } } v };
                        let (dref, lref) = (part(&["wanting", "afterglow", "deadline"]), part(&["floor"]));
                        let mut lift_hi = vec![0.0; n];
                        if d.goals.iter().any(|g| g.floor > 0.0) {
                            // the field the lift reads, widened by all that moves it in the cell: the drive box, learning, number intervals
                            let lcap = p.learning.as_ref().filter(|l| l.traits.iter().any(|t| t == "pursue")).map_or(0.0, |l| l.total_cap);
                            let lr = meta.new_learned.iter().find(|(t, _)| t == "pursue").map_or(vec![0.0; n], |x| x.1.clone());
                            let mut nw = vec![0.0; n];
                            for x in p.inputs.iter().filter(|x| x.kind == Kind::Number) { let w = hw.iter().find(|(k, _)| *k == x.id).map_or(0.0, |h| h.1);
                                if let Some((_, e)) = x.effects.iter().find(|(t, _)| t == "pursue") { for (a, y) in nw.iter_mut().zip(e) { *a += w * gain(&x.id) * y.abs(); } } }
                            let f0: Vec<f64> = (0..n).map(|g| meta.field[pi][g] - lref[g] - dref[g] - lr[g]).collect();
                            let fmin: Vec<f64> = (0..n).map(|g| f0[g] + dlo - lcap - nw[g] - 2e-6).collect(); let fmax: Vec<f64> = (0..n).map(|g| f0[g] + dhi + lcap + nw[g] + 2e-6).collect();
                            let ti = RULE_KEYS.iter().position(|k| *k == "tables").unwrap();
                            lift_hi = lift_iter(p, d, &fmin, &fmax, &pursue_allowed(&p.vars[pi].levels, &meta.rules[ti])); }
                        let zl = p.vars[pi].levels.iter().position(|x| *x == level("pursue")).unwrap_or(0);
                        let r: Vec<f64> = (0..n).map(|g| dref[g] + lref[g]).collect();
                        bound += (0..n).map(|g| dhi + lift_hi[g] + 1e-6 - r[g]).fold(f64::NEG_INFINITY, f64::max) + r[zl] - dlo; } }
                    for x in p.inputs.iter().filter(|x| x.kind == Kind::Number) { let w = hw.iter().find(|(k, _)| *k == x.id).map_or(0.0, |h| h.1); if w == 0.0 { continue; }
                        for (t, e) in &x.effects { if p.var(t).is_some_and(|v| v.mood) || !comp.contains(t) { continue; }
                            let zl = match t.strip_prefix("step.") { Some(_) => p.slots.iter().position(|s| Some(s.as_str()) == plan.get(t).and_then(Json::as_str)), None => p.var(t).and_then(|v| v.levels.iter().position(|x| *x == level(t))) }.unwrap_or(0);
                            bound += w * gain(&x.id) * e.iter().map(|y| (e[zl] - y).abs()).fold(0.0, f64::max); } }
                    if s_a - s_not <= bound { return Err(fail(format!("{k}: the score gap {:.4} is within the bound {:.4}", s_a - s_not, bound))); } }
            }
            Ok(()) };
        match check() { Ok(()) => true, Err(e) => { res = Err(e); false } } });
    res.map(|_| (n, calls))
}

#[cfg(test)]
mod tests {
    //! Bounded learning (docs/persona.md §2.8): the update rule against the stance documents, the caps, rules that feedback cannot
    //! remove, and the strict reading of the block and of the state fields
    use super::*;
    use probbit_core::Philox4x32;

    fn run(prog: &Json, f: &Flags) -> Json { run_program(prog, f, 1, 100, 0) }
    /// `j` at a path of keys (null when missing)
    fn g<'a>(j: &'a Json, ks: &[&str]) -> &'a Json { static NULL: Json = Json::Null; ks.iter().try_fold(j, |x, k| x.get(k)).unwrap_or(&NULL) }
    const DOC: &str = r#"{"probbit_persona":1,"identity":{"name":"L","version":"1","seed":3},
        "traits":[{"id":"verbosity","levels":["terse","short","full"],"logw":[0.2,0.5,0.1],"spread":0.4},{"id":"humour","levels":["none","light","playful"],"logw":[0,0.3,0.1]}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.5,"half_life_hours":6}],"couplings":[{"vars":["valence","humour"],"align":0.8}],
        "inputs":[{"id":"praise","kind":"flag","effects":{"valence":0.6}},{"id":"criticism","kind":"flag","effects":{"valence":-0.6}},{"id":"loss","kind":"flag","effects":{"valence":-1.0}}],
        "habits":[{"id":"no_jokes_on_loss","when":{"loss":true},"then":{"humour":["none"]}}]LEARN}"#;
    const LEARN: &str = r#","learning":{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":0.6}"#;
    fn persona(learning: &str) -> R<Persona> { build(&json::parse(&DOC.replace("LEARN", learning)).unwrap()) }
    fn inputs(pr: bool, cr: bool, loss: bool) -> Json { json::parse(&format!(r#"{{"praise":{pr},"criticism":{cr},"loss":{loss}}}"#)).unwrap() }
    fn err<T>(r: R<T>) -> String { r.err().map(|e| format!("{}: {}", e.path, e.msg)).unwrap_or_default() }

    /// Check both signs at the rounded cap and refusal beyond half a unit of the sixth decimal.
    #[test]
    fn state_cap_rounding_has_a_bounded_tolerance() {
        let p = persona(&LEARN.replace("0.6", "0.1234567").replace("0.2", "0.1234567")).unwrap();
        for sign in [-1.0, 1.0] { for (value, ok) in [(0.123457, true), (0.123458, false)] {
            let mut st = init(&p, None, true, &run); st.learned[0].1[0] = sign * value; st.seal(&p);
            assert_eq!(State::read(&p, &st.to_json(&p)).is_ok(), ok, "learned {sign} * {value}");
        } }
        let p = build(&json::parse(&ADV.replace(r#""learn_from_surprise":0.5"#, r#""learn_from_surprise":0.5,"wanting":{"cap":0.1234567}"#)).unwrap()).unwrap();
        assert_eq!(p.drives.as_ref().unwrap().cap_w, 0.1234567);
        for (value, ok) in [(0.123457, true), (0.123458, false), (-0.000001, false)] {
            let mut st = init(&p, None, true, &run); st.drives.as_mut().unwrap().want[0] = value; st.seal(&p);
            assert_eq!(State::read(&p, &st.to_json(&p)).is_ok(), ok, "want {value}");
        }
    }

    /// Every turn's learned deltas equal the rule applied to the previous stance document's levels and odds (released traits; others 0):
    /// Δ += clip(sign · rate · (onehot − odds), ±step_cap), recentred to sum 0, clipped to ±total_cap, 6 decimals; no feedback
    /// (or reward and correction together) moves nothing; the deltas sum to 0 until a cap clips; a habit-forced level gives no credit
    #[test]
    fn learning_follows_the_update_rule_on_the_stance_documents() {
        let p = persona(LEARN).unwrap(); let l = p.learning.clone().unwrap(); let mut st = init(&p, None, true, &run);
        let mut r = Philox4x32::new(45, 1); let (mut delta, mut credit) = (zeros(&p), zeros(&p)); let (mut moved, mut forced, mut capped) = (0, 0, [false; 2]);
        for t in 0..400 { let (pr, cr, loss) = (r.below(3) == 0, r.below(5) == 0, r.below(6) == 0);
            let sign = if pr { 1.0 } else { 0.0 } - if cr { 1.0 } else { 0.0 };
            if sign != 0.0 { for (((_, d), (_, c)), cap) in delta.iter_mut().zip(&credit).zip(capped.iter_mut()) {
                let step: Vec<f64> = c.iter().map(|x| (sign * l.rate * x).max(-l.step_cap).min(l.step_cap)).collect(); let mean = step.iter().sum::<f64>() / step.len() as f64;
                let raw: Vec<f64> = d.iter().zip(&step).map(|(a, x)| a + (x - mean)).collect(); *cap |= raw.iter().any(|x| x.abs() > l.total_cap);
                *d = raw.iter().map(|x| r6(x.max(-l.total_cap).min(l.total_cap))).collect(); } }
            let (doc, ns) = turn(&p, &st, &inputs(pr, cr, loss), false, &run, false).unwrap();
            assert_eq!(canon(&table(&ns.learned)), canon(&table(&delta)), "turn {t}");
            if ns.learned != st.learned { moved += 1; }
            for ((k, d), cap) in ns.learned.iter().zip(capped) { assert!(d.iter().all(|x| x.abs() <= l.total_cap));
                if !cap { assert!(d.iter().sum::<f64>().abs() < 1e-5, "turn {t} {k}: {d:?}"); } }
            if sign == 0.0 || st.credit.iter().all(|(_, c)| c.iter().all(|x| *x == 0.0)) { assert_eq!(ns.learned, st.learned, "turn {t}: no feedback or no credit moves nothing"); }
            credit = l.traits.iter().map(|k| { let e = g(&doc, &["stance", k]); let lv = g(e, &["level"]).as_str().unwrap();
                (k.clone(), if *g(e, &["released"]) == Json::Bool(true) { g(e, &["odds"]).as_obj().unwrap().iter().map(|(x, q)| r6(if x == lv { 1.0 } else { 0.0 } - q.as_f64().unwrap())).collect() }
                    else { vec![0.0; 3] }) }).collect();
            assert_eq!(canon(&table(&ns.credit)), canon(&table(&credit)), "turn {t}");
            if loss { forced += 1; assert_eq!(ns.credit[1].1, vec![0.0; 3], "a habit-forced humour level carries no credit"); }
            st = ns; }
        assert!(moved > 100 && forced > 30 && capped.iter().any(|c| *c), "moved {moved}, forced {forced}, capped {capped:?}");
        assert!(st.learned.iter().any(|(_, d)| d.iter().any(|x| x.abs() > 0.3)), "the deltas moved: {:?}", st.learned);
    }

    /// Feedback that rewards humour every time it is light or playful and corrects it when it is none, on a script where every
    /// third turn reports a loss: the learned humour deltas reach the cap, humour rises on the other turns, and no loss turn is
    /// ever anything but humour none (the habit is a rule of the program; the deltas are unaries). The rule credits the level the
    /// stance took, so the joke level taken most (light) gains
    #[test]
    fn feedback_cannot_learn_a_rule_away() {
        let p = persona(LEARN).unwrap(); let tc = p.learning.as_ref().unwrap().total_cap; let mut st = init(&p, Some(5), true, &run);
        let (mut prev_humour, mut script, mut joke_odds) = (String::from("none"), vec![], vec![]);
        for t in 0..1500 { let loss = t % 3 == 2; let joke = prev_humour != "none"; script.push(inputs(joke, !joke, loss));
            let (doc, ns) = turn(&p, &st, &script[t], false, &run, false).unwrap(); let h = g(&doc, &["stance", "humour"]);
            assert_eq!(*g(&doc, &["habits", "violations"]), Json::Num(0.0));
            if loss { assert_eq!(g(h, &["level"]).as_str(), Some("none"), "turn {t}"); } else { joke_odds.push(1.0 - g(h, &["odds", "none"]).as_f64().unwrap()); }
            prev_humour = g(h, &["level"]).as_str().unwrap().to_string(); st = ns; }
        let d = &st.learned[1].1; assert!(d.iter().any(|x| (x.abs() - tc).abs() < 1e-9), "humour deltas at the cap: {d:?}");
        // the same individual (same name and seed: same genes) without the learning block, on the identical inputs
        let (twin, _) = replay(&persona("").unwrap(), Some(5), &script, false, &run, false).unwrap();
        let twin_odds: Vec<f64> = twin.iter().enumerate().filter(|(t, _)| t % 3 != 2).map(|(_, d)| 1.0 - g(d, &["stance", "humour", "odds", "none"]).as_f64().unwrap()).collect();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        assert!(mean(&joke_odds) > mean(&twin_odds) + 0.1, "the odds of a joke off loss turns: learner {} vs no learning {}", mean(&joke_odds), mean(&twin_odds));
    }

    /// The block is read strictly; a persona without it keeps 0.6.0's documents (no learned / credit fields, refused if present);
    /// a state's deltas outside ±total_cap are refused
    #[test]
    fn the_learning_block_and_state_fields_are_strict() {
        let bad = |l: &str| err(persona(&format!(r#","learning":{{{l}}}"#)));
        let base = r#""from":["praise","criticism"],"traits":["verbosity"],"rate":0.5,"step_cap":0.2,"total_cap":0.6"#;
        assert_eq!(bad(&format!(r#"{base},"spread":1"#)), "learning.spread: unknown field (allowed: comment, credit, from, rate, step_cap, total_cap, traits)");
        assert_eq!(bad(r#""from":["praise"],"traits":["verbosity"],"rate":0.5,"step_cap":0.2"#), "learning.total_cap: required");
        assert_eq!(bad(&base.replace(r#"["praise","criticism"]"#, r#"["praise","valence"]"#)), "learning.from[1]: not a flag input of this persona");
        assert_eq!(bad(&base.replace(r#"["verbosity"]"#, r#"["valence"]"#)), "learning.traits[0]: not a trait of this persona (moods do not learn)");
        assert_eq!(bad(&base.replace(r#"["verbosity"]"#, r#"["humour","humour"]"#)), "learning.traits[1]: listed twice");
        assert_eq!(bad(&base.replace(r#"["praise","criticism"]"#, r#"["praise","criticism","loss"]"#)), "learning.from: a list of 1 to 2 ids");
        assert_eq!(bad(&base.replace(r#""step_cap":0.2"#, r#""step_cap":0.7"#)), "learning.step_cap: must be <= 0.6");
        assert_eq!(bad(&base.replace(r#""rate":0.5"#, r#""rate":0"#)), "learning.rate: must be > 0");
        // no block: the state has no learned / credit fields and refuses them
        let p = persona("").unwrap(); let j = init(&p, None, true, &run).to_json(&p);
        assert!(j.get("learned").is_none() && j.get("credit").is_none());
        let mut kv: Vec<(String, Json)> = j.as_obj().unwrap().iter().filter(|(k, _)| k != "digest").cloned().collect(); kv.push(("learned".into(), Json::Obj(vec![])));
        let body = Json::Obj(kv.clone()); kv.push(("digest".into(), Json::Str(sha(&body))));
        assert_eq!(err(State::read(&p, &Json::Obj(kv))), "state.learned: this persona has no learning block");
        assert!(describe(&p).get("learning").is_none(), "describe without a block is 0.6.0's");
        // with the block: zero tables at init; a delta beyond the cap is refused even with a recomputed digest
        let q = persona(LEARN).unwrap(); let st = init(&q, None, true, &run); let j = st.to_json(&q);
        assert_eq!(canon(describe(&q).get("learning").unwrap()), r#"{"from":["praise","criticism"],"rate":0.5,"step_cap":0.2,"total_cap":0.6,"traits":["verbosity","humour"]}"#);
        assert_eq!(canon(j.get("learned").unwrap()), r#"{"humour":[0,0,0],"verbosity":[0,0,0]}"#); assert!(State::read(&q, &j).is_ok());
        let mut s2 = st.clone(); s2.learned[0].1 = vec![0.7, -0.35, -0.35]; s2.seal(&q);
        assert_eq!(err(State::read(&q, &s2.to_json(&q))), "state.learned.verbosity: one number per level, each within ±0.6");
    }

    // ------------------------------------------------------------------------------------------------ drives (§2.9)
    /// A test individual with wants: four goals (fun, craft, chores, safety), a safety floor, a must-do deadline habit, two
    /// starvation habits, learning on pursue, pursue coupled to caution
    const ADV: &str = r#"{"probbit_persona":1,"identity":{"name":"Drifter","version":"0.1.0","seed":0,"summary":"a test individual with wants"},
        "traits":[{"id":"initiative","levels":["answer","recommend","drive"],"prior":[0.3,0.5,0.2],"spread":0.4},{"id":"caution","levels":["bold","measured","careful"],"prior":[0.3,0.5,0.2],"spread":0.4,"say":["move fast","","verify every step"]},
            {"id":"humour","levels":["none","dry","playful"],"prior":[0.3,0.5,0.2],"spread":0.5},{"id":"verbosity","levels":["terse","tight","full"],"prior":[0.3,0.5,0.2],"spread":0.5}],
        "moods":[{"id":"valence","levels":["down","even","up"],"prior":[0.2,0.5,0.3],"spread":0.4,"inertia":0.5,"half_life_hours":8}],
        "couplings":[{"vars":["valence","humour"],"align":0.8},{"vars":["pursue","caution"],"table":{"safety":{"careful":0.8,"bold":-0.8},"fun":{"bold":0.6,"careful":-0.4}}}],
        "inputs":[{"id":"praise","kind":"flag","effects":{"valence":0.8}},{"id":"criticism","kind":"flag","effects":{"valence":-0.6}},{"id":"security","kind":"flag","effects":{"caution":1.5,"humour":-1.0,"pursue":{"safety":1.0}}},
            {"id":"failure","kind":"flag","effects":{"valence":-1.0,"caution":1.0,"humour":-1.5}},{"id":"load","kind":"number","max":4,"say":"machine load","effects":{"verbosity":-0.4,"initiative":-0.3}}],
        "habits":[{"id":"security_careful","when":{"security":true},"then":{"caution":["careful"]},"say":"careful under security","priority":3},{"id":"no_jokes_on_failure","when":{"failure":true},"then":{"humour":["none"]},"say":"no jokes on a miss"},
            {"id":"chores_due","when":{"goal.chores.deadline_hours":{"at_most":24}},"then":{"pursue":["chores"]},"say":"the chore is due","priority":2}],
        "engine":{"on_conflict":"yield"},"learning":{"from":["praise","criticism"],"traits":["humour","pursue"],"rate":0.5,"step_cap":0.2,"total_cap":1.0},
        "drives":{"goals":[{"id":"fun","say":"play","interest":2.0,"interest_spread":0.5,"reactivity_spread":0.5},{"id":"craft","say":"build","interest":1.0,"interest_spread":0.5,"reactivity_spread":0.5},
            {"id":"chores","say":"do the chores","interest":0.7,"interest_spread":0.3,"starve_after":12},{"id":"safety","say":"check the locks","interest":0.5,"interest_spread":0.3,"floor":0.1,"starve_after":24,"priority":1}],
            "effects":{"wanting":{"initiative":0.6,"verbosity":-0.3,"caution":-0.3},"afterglow":{"humour":0.6,"valence":0.8},"surprise":{"valence":0.5}},"learn_from_surprise":0.5}}"#;
    fn adv() -> Persona { build(&json::parse(ADV).unwrap()).unwrap() }
    fn drv(p: &Persona) -> Drives { p.drives.clone().unwrap() }
    fn step(p: &Persona, ds: &DState, ev: &str) -> DState { drive_step(p.drives.as_ref().unwrap(), ds, json::parse(ev).unwrap().as_obj().unwrap(), None).unwrap() }
    fn win(goal: &str, m: f64, h: f64) -> String { format!(r#"{{"goals":{{"{goal}":{{"win":{m}}}}},"elapsed_hours":{h}}}"#) }

    /// Habituation and recovery in closed form: the same win repeated (rate a) gives a prediction error of (1 - a)^n; after two
    /// expectation half-lives idle the same win gives 1 - E/4; afterglow decays by its half-life over any split of the idle hours;
    /// a full win drains wanting by `drain`
    #[test]
    fn drives_follow_their_closed_forms() {
        let (p, c) = (adv(), 1); let d = drv(&p); assert_eq!(d.goals[c].id, "craft");
        let mut ds = DState::zero(&p, &d, 0); let mut seq = vec![];
        for n in 0..12 { ds = step(&p, &ds, &win("craft", 1.0, 0.0)); seq.push(ds.surprise[c]);
            assert!((ds.surprise[c] - (1.0 - d.rate).powi(n)).abs() < 1e-5, "win {n}: {}", ds.surprise[c]); }
        assert!(seq[11] < 0.001 && seq.windows(2).all(|w| w[0] >= w[1]), "{seq:?}");
        let mut ds = DState::zero(&p, &d, 0); for _ in 0..10 { ds = step(&p, &ds, &win("craft", 1.0, 0.0)); }
        let e = ds.expect[c]; ds = step(&p, &ds, &win("craft", 1.0, 2.0 * d.hl_e));
        assert!((ds.surprise[c] - (1.0 - 0.25 * e)).abs() < 1e-5, "recovery {} vs {}", ds.surprise[c], 1.0 - 0.25 * e);
        let mut ds = step(&p, &DState::zero(&p, &d, 0), &win("craft", 1.2, 0.0)); let a1 = ds.glow[c]; assert!(a1 > 0.0);
        let gaps = [0.5, 1.0, 2.5, 4.0, 0.25]; for h in gaps { ds = step(&p, &ds, &format!(r#"{{"elapsed_hours":{h}}}"#)); }
        assert!((ds.glow[c] - a1 * 0.5f64.powf(gaps.iter().sum::<f64>() / d.hl_a)).abs() < 2e-6, "afterglow {} vs {a1}", ds.glow[c]);
        let mut ds = DState::zero(&p, &d, 0); for _ in 0..5 { ds = step(&p, &ds, r#"{"goals":{"craft":{"cue":true,"progress":1}}}"#); }
        let w = ds.want[c]; ds = step(&p, &ds, &win("craft", 1.0, 0.0)); assert!((ds.want[c] - w * (1.0 - d.drain)).abs() < 2e-6, "drain {} vs {w}", ds.want[c]);
    }

    /// 20,000 random drive steps (signals, wins up to win_max, idle hours) never leave the box: W in [0, cap], A in [0, cap],
    /// E in [0, win_max], |prediction error| <= pe_cap
    #[test]
    fn drive_accumulators_stay_in_their_box() {
        let p = adv(); let d = drv(&p); let mut ds = DState::zero(&p, &d, 9); let mut r = Philox4x32::new(57, 1);
        let unit = |r: &mut Philox4x32| r.below(1_000_001) as f64 / 1e6; let (mut hit_w, mut hit_a) = (false, false);
        for _ in 0..20_000 { let h = [0.0, 0.0, 0.1, 1.0, 30.0][r.below(5)];
            let mut g = vec![]; for goal in &d.goals { if r.below(10) < 6 { let m = if r.below(3) == 2 { 2.0 * unit(&mut r) } else { 0.0 };
                let (pr, nv, cue, sb) = (unit(&mut r), unit(&mut r), r.below(2) == 0, unit(&mut r));
                g.push(format!(r#""{}":{{"progress":{pr},"novelty":{nv},"cue":{cue},"setback":{sb},"win":{m}}}"#, goal.id)); } }
            ds = step(&p, &ds, &format!(r#"{{"elapsed_hours":{h},"goals":{{{}}}}}"#, g.join(",")));
            for i in 0..d.goals.len() { assert!((0.0..=d.cap_w).contains(&ds.want[i]) && (0.0..=d.cap_a).contains(&ds.glow[i]) && (0.0..=d.win_max).contains(&ds.expect[i]) && ds.surprise[i].abs() <= d.pe_cap);
                hit_w |= ds.want[i] == d.cap_w; hit_a |= ds.glow[i] == d.cap_a; } }
        assert!(hit_w && hit_a, "the caps were reached");
    }

    /// The drive genes are a function of name, seed and goal id (another seed, another individual; the same seed, the same one);
    /// the persona digest covers the block
    #[test]
    fn drive_genes_and_the_digest() {
        let p = adv(); let d = drv(&p);
        let (g1, g2) = (canon(&drive_genes(&p, &d, 1)), canon(&drive_genes(&p, &d, 2))); assert_ne!(g1, g2); assert_eq!(g1, canon(&drive_genes(&adv(), &d, 1)));
        assert_eq!(p.digest, sha(&json::parse(ADV).unwrap()));
        assert_ne!(p.digest, build(&json::parse(&ADV.replace(r#""interest":2.0"#, r#""interest":2.5"#)).unwrap()).unwrap().digest);
    }

    /// `describe` and `check` show the author's persona: no `pursue` among the traits, no lowering inputs, a `drives` object; the
    /// synthesised starvation habits are habits in force and are listed
    #[test]
    fn describe_and_check_show_the_authors_persona() {
        let p = adv(); let d = describe(&p);
        assert_eq!(canon(d.get("drives").unwrap()), r#"{"floor":{"safety":0.1},"goals":["fun","craft","chores","safety"],"learn_from_surprise":0.5,"say":{"chores":"do the chores","craft":"build","fun":"play","safety":"check the locks"},"starve_after":{"chores":12,"safety":24}}"#);
        assert!(d.get("traits").unwrap().get("pursue").is_none() && d.get("inputs").unwrap().as_obj().unwrap().len() == 5);
        assert_eq!(canon(d.get("habits").unwrap()), r#"["security_careful","no_jokes_on_failure","chores_due","starve_chores","starve_safety"]"#);
        let c = check(&p, &run).unwrap(); assert_eq!((g(&c, &["traits"]), g(&c, &["inputs"]), g(&c, &["goals"])), (&Json::Num(4.0), &Json::Num(5.0), &Json::Num(4.0)));
        assert!(check(&persona("").unwrap(), &run).unwrap().get("goals").is_none(), "no block, no goals field");
    }

    /// `lint` lists every habit that can exclude a floor goal: here the two starvation habits (they force another goal) and the
    /// must-do habit; none for a persona without a block
    #[test]
    fn lint_lists_the_habits_that_outrank_a_floor() {
        let n = floor_notes(&adv()).unwrap(); let hs: Vec<&str> = n.iter().map(|x| x.get("habit").and_then(Json::as_str).unwrap()).collect();
        assert_eq!(hs, ["chores_due", "starve_chores"], "{}", canon(&Json::Arr(n.clone())));
        assert!(n.iter().all(|x| x.get("goal").and_then(Json::as_str) == Some("safety")));
        assert!(floor_notes(&persona("").unwrap()).is_none());
        // a floor against a 30-unit input effect and the widest interests: its lift can reach 50
        assert!(floor_reach(&adv(), &[0, 1, 2]).is_empty());
        let q = build(&json::parse(&ADV.replace(r#""pursue":{"safety":1.0}"#, r#""pursue":{"fun":30.0}"#).replace(r#""interest":0.5,"#, r#""interest":0.000001,"#).replace(r#""interest":2.0,"#, r#""interest":1000000,"#)).unwrap()).unwrap();
        let r = floor_reach(&q, &[0]); assert_eq!(r.len(), 1); assert_eq!(r[0].get("goal").and_then(Json::as_str), Some("safety"));
        assert!(r[0].get("lift_max").and_then(Json::as_f64).unwrap() >= 50.0, "{}", canon(&r[0]));
    }

    /// `diff` counts `pursue` as one more variable: two individuals of the drives persona differ on it, and a persona without the
    /// block keeps its traits and nothing else
    #[test]
    fn diff_counts_pursue() {
        let p = adv(); let s: Vec<Json> = (0..12).map(|i| json::parse(&format!(r#"{{"goals":{{"craft":{{"cue":true,"win":{}}}}},"elapsed_hours":1}}"#, i % 3)).unwrap()).collect();
        let d = diff(&p, 1, &p, 2, &s, &run).unwrap(); let per = g(&d, &["distance", "per_trait"]);
        assert!(per.get("pursue").and_then(Json::as_f64).is_some_and(|x| x > 0.0), "{}", canon(&d));
        let q = persona("").unwrap(); let d = diff(&q, 1, &q, 2, &[Json::Null, Json::Null], &run).unwrap(); assert!(g(&d, &["distance", "per_trait"]).get("pursue").is_none());
    }

    /// Hard rules and floors at the extremes: after 40 turns that cue, praise and reward fun and no other goal, the learned pursue weight for fun
    /// is at its cap and fun's wanting near its cap, yet safety keeps at least its floor (the lift is active); with the chore due
    /// under security, pursue is chores with odds 1 and caution careful, no habit broken
    #[test]
    fn habits_and_floors_hold_at_the_extremes() {
        let p = adv(); let mut st = init(&p, Some(11), true, &run);
        for i in 0..40 { let m = if i % 3 == 1 { 2.0 } else { 0.0 };
            let ev = json::parse(&format!(r#"{{"goals":{{"fun":{{"cue":true,"progress":1,"novelty":true,"win":{m}}}}},"praise":true,"elapsed_hours":0.1}}"#)).unwrap();
            st = turn(&p, &st, &ev, false, &run, false).unwrap().1; }
        assert_eq!(st.learned.iter().find(|(t, _)| t == "pursue").unwrap().1[0], 1.0, "fun at the learner's cap");
        assert!(st.drives.as_ref().unwrap().want[0] >= 2.0, "fun's wanting near its cap");
        let (out, _) = turn(&p, &st, &json::parse(r#"{"elapsed_hours":0.1}"#).unwrap(), false, &run, false).unwrap();
        assert!(g(&out, &["pursue", "odds", "safety"]).as_f64().unwrap() >= 0.1 && g(&out, &["pursue", "lift", "safety"]).as_f64().unwrap() > 0.0, "{}", canon(g(&out, &["pursue"])));
        assert!(g(&out, &["stance", "pursue"]).is_null() && g(&out, &["drives", "want", "fun"]).as_f64().unwrap() >= 2.0);
        let (out, ns) = turn(&p, &st, &json::parse(r#"{"goals":{"chores":{"deadline_hours":5}},"security":true,"elapsed_hours":0.1}"#).unwrap(), false, &run, false).unwrap();
        assert_eq!(g(&out, &["pursue", "goal"]).as_str(), Some("chores")); assert_eq!(*g(&out, &["pursue", "odds", "chores"]), Json::Num(1.0));
        assert_eq!(g(&out, &["stance", "caution", "level"]).as_str(), Some("careful")); assert_eq!(*g(&out, &["habits", "violations"]), Json::Num(0.0));
        assert!(g(&out, &["line"]).as_str().unwrap().contains("pursue: chores: do the chores"), "{}", canon(g(&out, &["line"])));
        let since = &ns.drives.as_ref().unwrap().since; assert_eq!(since[2], 0, "chores pursued"); assert!(since.iter().enumerate().all(|(i, s)| i == 2 || *s > 0));
    }
    /// The floor over many individuals: 20 seeds driven to the extremes (fun wanted to its cap, learned to the learner's cap),
    /// then a quiet turn: safety's odds are at least its floor for every one of them
    #[test]
    fn the_floor_holds_for_many_individuals() {
        let p = adv();
        for seed in 0..20 { let mut st = init(&p, Some(seed), true, &run);
            for i in 0..30 { let m = if i % 3 == 1 { 2.0 } else { 0.0 };
                let ev = json::parse(&format!(r#"{{"goals":{{"fun":{{"cue":true,"progress":1,"novelty":true,"win":{m}}}}},"praise":true,"elapsed_hours":0.1}}"#)).unwrap();
                st = turn(&p, &st, &ev, false, &run, false).unwrap().1; }
            let (out, _) = turn(&p, &st, &json::parse(r#"{"elapsed_hours":0.1}"#).unwrap(), false, &run, false).unwrap();
            assert!(g(&out, &["pursue", "odds", "safety"]).as_f64().unwrap() >= 0.1, "seed {seed}: {}", canon(g(&out, &["pursue"]))); }
    }

    /// The block, the goal signals and the drive state are read strictly; a persona without the block refuses a `drives` state field
    #[test]
    fn the_drives_block_and_state_are_strict() {
        let bad = |from: &str, to: &str| { assert!(ADV.contains(from), "{from}"); err(build(&json::parse(&ADV.replace(from, to)).unwrap())) };
        assert_eq!(bad(r#""learn_from_surprise":0.5"#, r#""learn_from_surprise":1.5"#), "drives.learn_from_surprise: must be <= 1");
        assert_eq!(bad(r#""from":["praise","criticism"]"#, r#""from":["praise","drv_c0"]"#), "learning.from[1]: reserved for the drives block");
        assert_eq!(bad(r#""floor":0.1"#, r#""floor":0.6"#), "drives.goals[3].floor: must be <= 0.5");
        assert_eq!(bad(r#""id":"craft""#, r#""id":"fun""#), "drives.goals[1].id: goal ids must be distinct");
        assert_eq!(bad(r#""learn_from_surprise":0.5"#, r#""learn_from_surprise":0.5,"spice":1"#), "drives.spice: unknown field (allowed: afterglow, comment, effects, expectation, goals, learn_from_surprise, pursue, wanting)");
        assert_eq!(bad(r#""learn_from_surprise":0.5"#, r#""wanting":{"drain":2}"#), "drives.wanting.drain: must be <= 1");
        assert_eq!(bad(r#""learn_from_surprise":0.5"#, r#""afterglow":{"cap":0}"#), "drives.afterglow.cap: must be > 0");
        assert_eq!(bad(r#""surprise":{"valence":0.5}"#, r#""surprise":{"valence":[0,0,1]}"#), "drives.effects.surprise.valence: a number (ordinal shift)");
        assert_eq!(bad(r#""afterglow":{"humour":0.6"#, r#""afterglow":{"spirit":0.6"#), "drives.effects.afterglow.spirit: unknown trait or mood \"spirit\"");
        assert_eq!(bad(r#""goal.chores.deadline_hours""#, r#""goal.chores.hours""#), "habits[2].when.goal.chores.hours: signal must be one of deadline_hours, want, glow, expect, since_pursued");
        assert_eq!(bad(r#""goal.chores.deadline_hours""#, r#""goal.dishes.deadline_hours""#), "habits[2].when.goal.dishes.deadline_hours: unknown goal \"dishes\"");
        assert_eq!(bad(r#""id":"initiative""#, r#""id":"pursue""#), "traits: `pursue` is reserved when a drives block is present");
        assert_eq!(bad(r#""say":"no jokes on a miss"}"#, r#""say":"no jokes on a miss","rules":{"implies":[{"if":{"var":"pursue","value":"fun"},"then":{"var":"caution","in":["measured","careful"]}}]}}"#),
            "habits.no_jokes_on_failure: a goal floor needs `pursue` free of multi-variable rules (use then: or a coupling)");
        let p = adv(); let st = init(&p, None, true, &run); let t = |ev: &str| err(turn(&p, &st, &json::parse(ev).unwrap(), false, &run, false));
        assert_eq!(t(r#"{"goals":{"dishes":{"win":1}}}"#), "inputs.goals.dishes: unknown goal (goals: fun, craft, chores, safety)");
        assert_eq!(t(r#"{"goals":{"fun":{"win":3}}}"#), "inputs.goals.fun.win: must be <= 2");
        assert_eq!(t(r#"{"goals":{"fun":{"cue":1}}}"#), "inputs.goals.fun.cue: true | false");
        assert_eq!(t(r#"{"goals":{"fun":{"joy":1}}}"#), "inputs.goals.fun.joy: one of progress, novelty, cue, setback, win, deadline_hours");
        assert_eq!(t(r#"{"drv_want":1}"#), "inputs.drv_want: reserved for the drives block");
        assert!(State::read(&p, &st.to_json(&p)).is_ok());
        let reseal = |f: &dyn Fn(&mut DState)| { let mut s = st.clone(); f(s.drives.as_mut().unwrap()); s.seal(&p); err(State::read(&p, &s.to_json(&p))) };
        assert_eq!(reseal(&|d| d.want[0] = 3.5), "state.drives.want: one number per goal in [0, 3]");
        assert_eq!(reseal(&|d| d.surprise[1] = -1.5), "state.drives.surprise: one number per goal within ±1");
        assert_eq!(reseal(&|d| d.genes = drive_genes(&adv(), &drv(&adv()), 12)), "state.drives.genes: drive genes do not match the seed");
        let mut s = st.clone(); s.drives = None; s.seal(&p); assert_eq!(err(State::read(&p, &s.to_json(&p))), "state.drives: required with a drives block");
        let q = persona("").unwrap(); let j = init(&q, None, true, &run).to_json(&q);
        let mut kv: Vec<(String, Json)> = j.as_obj().unwrap().iter().filter(|(k, _)| k != "digest").cloned().collect(); kv.push(("drives".into(), Json::Obj(vec![])));
        let body = Json::Obj(kv.clone()); kv.push(("digest".into(), Json::Str(sha(&body))));
        assert_eq!(err(State::read(&q, &Json::Obj(kv))), "state.drives: this persona has no drives block");
    }

    /// Properties (§5.6) over a drives persona: goal conditions read the persona's own condition flags, `pursue` is read from its
    /// own object, the must-do property is held by construction, and the lowering's inputs stay the engine's
    #[test]
    fn properties_read_goal_conditions_and_pursue() {
        let p = adv(); let rule = |t: &str| json::parse(t).unwrap(); let pr = |t: &str| props(&p, &rule(t), "--never", "never");
        let must = pr(r#"{"when":{"goal.chores.deadline_hours":{"at_most":24}},"then":{"pursue":["chores"]}}"#).unwrap();
        assert_eq!(by_construction(&p, &must[0], &run), Some(vec!["chores_due".to_string()]));
        assert_eq!(canon(&Json::Arr(floor_verdicts(&p, &[0]).unwrap())), r#"[{"excluded_by":["chores_due","starve_chores"],"floor":0.1,"goal":"safety","verdict":"held_by_construction"}]"#);
        assert!(floor_verdicts(&persona("").unwrap(), &[0]).is_none());
        assert_eq!(err(pr(r#"{"when":{"goal.chores.deadline_hours":{"at_most":12}},"then":{"pursue":["chores"]}}"#)),
            "--never.when.goal.chores.deadline_hours: a property reads the goal conditions the persona's habits use: goal.chores.deadline_hours <= 24, since_pursued.chores >= 12, since_pursued.safety >= 24");
        assert_eq!(err(pr(r#"{"when":{"drv_want":1},"then":{"pursue":["chores"]}}"#)), "--never.when.drv_want: reserved for the drives block");
        let bad = ADV.replace(r#""when":{"failure":true}"#, r#""when":{"drv_c0":true}"#); assert_ne!(bad, ADV);
        assert_eq!(err(build(&json::parse(&bad).unwrap())), "habits[1].when.drv_c0: reserved for the drives block");
        // a due chore under security: the must-do holds, "security -> pursue safety" breaks on pursue
        let st = init(&p, Some(0), true, &run);
        let (doc, _) = turn(&p, &st, &json::parse(r#"{"goals":{"chores":{"deadline_hours":1}},"security":true}"#).unwrap(), false, &run, false).unwrap();
        assert!(breaks(&p, &must[0], &st, &doc).is_none());
        let sec = pr(r#"{"when":{"security":true},"then":{"pursue":["safety"]}}"#).unwrap();
        let b = breaks(&p, &sec[0], &st, &doc).unwrap(); assert_eq!((b[0].var.as_str(), b[0].level.as_str(), b[0].allowed.clone()), ("pursue", "chores", vec!["safety".to_string()]));
        assert!(pressure(&p, &sec[0], &st, &doc) > 0.5);
        // the fuzz grammar: goal signals (win on the grid x win_max, cue, deadline on the hours), nested into `goals`; forcing sets the deadline
        let a = alphabet(&p, &[0.0, 0.5, 1.0], &[1.0, 12.0]); let ids: Vec<&str> = a.iter().map(|x| x.0.as_str()).collect();
        assert!(ids.contains(&"elapsed_hours") && ids.contains(&"goal.fun.cue") && ids.contains(&"goal.safety.deadline_hours") && !ids.iter().any(|x| x.starts_with("drv_")), "{ids:?}");
        assert_eq!(canon(&Json::Arr(a.iter().find(|x| x.0 == "goal.fun.win").unwrap().2.clone())), "[1,2]");
        let e: Vec<(String, Json)> = vec![("security".into(), Json::Bool(true)), ("goal.fun.cue".into(), Json::Bool(true)), ("goal.safety.deadline_hours".into(), Json::Num(1.0)), ("goal.fun.win".into(), Json::Num(2.0))];
        assert_eq!(canon(&event_json(&e)), r#"{"goals":{"fun":{"cue":true,"win":2},"safety":{"deadline_hours":1}},"security":true}"#);
        assert_eq!(forcing(&p, &must[0], &[0.0, 0.5, 1.0]), vec![vec![("goal.chores.deadline_hours".to_string(), Json::Num(24.0))]]);
    }
    /// `prove` with drives: `pursue`'s wanting, afterglow and deadline terms move it by up to 5.5 here (wanting 0..3, afterglow
    /// -1..0, deadline 0..1.5), so a 2.5 margin at rest is not a proof (a script breaks it); a 6.5 margin is
    const BOX: &str = r#"{"probbit_persona":1,"identity":{"name":"Boxer","version":"0.1.0","seed":0},"traits":[{"id":"caution","levels":["bold","measured","careful"],"prior":[0.3,0.5,0.2]}],
        "inputs":[{"id":"security","kind":"flag","effects":{"pursue":{"safety":2.5}}}],"drives":{"goals":[{"id":"fun","interest":1.0},{"id":"safety","interest":1.0}],"wanting":{"cap":3}}}"#;
    // ------------------------------------------------------------------------------------------------ senses (§2.11)
    /// A test individual with two senses (a flag at the defaults of the RFC's arithmetic, a number), a habit on the flag's raw
    /// reading, learning on caution and reward_from
    const SEN: &str = r#"{"probbit_persona":1,"identity":{"name":"Sensor","version":"0.1.0","seed":0},
        "traits":[{"id":"caution","levels":["bold","measured","careful"],"prior":[0.4,0.4,0.2],"spread":0.4},{"id":"verbosity","levels":["terse","short","full"],"prior":[0.2,0.4,0.4]}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.5,"half_life_hours":6}],
        "inputs":[{"id":"praise","kind":"flag","effects":{"valence":0.5}},{"id":"criticism","kind":"flag","effects":{"valence":-0.5}}],
        "senses":[{"id":"tool_fail","kind":"flag","unit":"event","prior":{"mean":0.05,"var":0.05},"drift_cap":0.25,"say":"tool failures","effects":{"caution":0.8,"valence":-0.4}},
            {"id":"tempo","kind":"number","unit":"messages","range":[0,200],"prior":{"mean":1,"var":1},"drift_cap":2,"effects":{"verbosity":-0.4}}],
        "habits":[{"id":"no_bold_after_failure","when":{"sense.tool_fail.raw":true},"then":{"caution":{"not":["bold"]}}},
            {"id":"short_when_busy","when":{"sense.tempo.raw":{"at_least":20}},"then":{"verbosity":{"at_most":"short"}}}],
        "learning":{"from":["praise","criticism"],"traits":["caution"],"rate":0.3,"step_cap":0.1,"total_cap":0.4},"reward_from":["human","env"]}"#;
    fn sen() -> Persona { build(&json::parse(SEN).unwrap()).unwrap() }
    fn sen_with(from: &str, to: &str) -> R<Persona> { assert!(SEN.contains(from), "{from}"); build(&json::parse(&SEN.replacen(from, to, 1)).unwrap()) }
    /// One flag sense at the RFC's defaults (prior 0.05/0.05, drift_cap 0.25, var_cap 0.2, half-life 10, zcap 6, dead zone 0.5,
    /// var_floor 0.01, spreads 0), with `extra` fields
    fn flag_sense(extra: &str) -> (Senses, SState) {
        let doc = SEN.replacen(r#""prior":{"mean":0.05,"var":0.05},"drift_cap":0.25"#, &format!(r#""prior":{{"mean":0.05,"var":0.05}},"drift_cap":0.25{extra}"#), 1);
        let p = build(&json::parse(&doc).unwrap()).unwrap(); let sn = p.senses.clone().unwrap(); let ss = SState::zero(&p.name, &sn, 0); (sn, ss)
    }
    /// Readings of the flag sense from `ss`: (the state after them, per reading (z, zd))
    fn feed(sn: &Senses, mut ss: SState, xs: &[bool]) -> (SState, Vec<(f64, f64)>) {
        let mut out = vec![];
        for x in xs { let ev = json::parse(&format!(r#"{{"senses":{{"tool_fail":{x}}},"src":"env"}}"#)).unwrap(); let raw = ev.as_obj().unwrap().to_vec();
            let (ns, _) = sense_step(sn, &ss, &raw, get(&raw, "src"), true).unwrap(); let r = ns.read[0].clone().unwrap(); out.push((r.z, r.zd)); ss = ns; }
        (ss, out)
    }

    /// RFC 0001 A.3 against its arithmetic, computed independently in Python: habituation (zd 3.378359 -> 1.027525 from reading 6 on, the
    /// ratio 0.304 at reading 20), surprise (6.0 vs 1.75286 after 40 quiet / 40 readings at rate 0.3: 3.42 at zcap 6, 1.71 at zcap
    /// 3), the closed form of n identical readings within 1e-5 over 12 readings, the variance cap after alternating range ends
    #[test]
    fn the_sense_step_reproduces_the_rfc_arithmetic() {
        let (sn, ss) = flag_sense("");
        let (end, zs) = feed(&sn, ss.clone(), &[true; 200]);
        assert_eq!((zs[0].0, zs[0].1, zs[19].0, zs[19].1, zs[199].0), (3.878359, 3.378359, 1.527525, 1.027525, 1.527525));
        assert!(zs[5..].iter().all(|z| z.1 == 1.027525), "from reading 6 on the push stays at its floor");
        let ratio = zs[19].1 / zs[0].1; assert!((ratio - 0.304).abs() < 5e-4 && ratio <= 0.35, "habituation ratio {ratio}");
        assert_eq!((end.mean[0], end.var[0], end.n[0]), (0.3, 0.2, 200), "the drift and variance caps hold a chronic failure");
        let quiet: Vec<bool> = (0..40).map(|_| false).chain([true]).collect(); let failing: Vec<bool> = (0..40).map(|k| [0, 3, 6].contains(&(k % 10))).chain([true]).collect();
        for (zcap, zq, want) in [("", 6.0, 3.423), (r#","zcap":3"#, 3.0, 1.711)] { let (sn, ss) = flag_sense(zcap);
            let (q, f) = (feed(&sn, ss.clone(), &quiet).1, feed(&sn, ss, &failing).1); let (a, b) = (q[40].0, f[40].0);
            assert_eq!((a, b), (zq, 1.75286)); assert!(((a / b) - want).abs() < 5e-4, "surprise ratio {} at {zcap}", a / b); }
        // the closed form before the caps: a number sense with wide caps, 12 readings of 0.5 from a normal at 0.4
        let doc = SEN.replacen(r#""prior":{"mean":1,"var":1},"drift_cap":2"#, r#""prior":{"mean":0.4,"var":0.05},"drift_cap":10,"var_cap":10"#, 1);
        let p = build(&json::parse(&doc).unwrap()).unwrap(); let sn = p.senses.clone().unwrap(); let mut ss = SState::zero(&p.name, &sn, 0);
        let a = r6(1.0 - 0.5f64.powf(0.1)); assert_eq!(a, 0.066967);
        for n in 1..=12 { let raw = json::parse(r#"{"senses":{"tempo":0.5},"src":"env"}"#).unwrap().as_obj().unwrap().to_vec(); ss = sense_step(&sn, &ss, &raw, get(&raw, "src"), true).unwrap().0;
            let closed = 0.5 + (1.0 - a).powi(n) * (0.4 - 0.5); assert!((ss.mean[1] - closed).abs() < 1e-5, "reading {n}: {} vs {closed}", ss.mean[1]); }
        assert_eq!(ss.mean[1], 0.456471);
        // the variance cap: 200 readings alternating at the range ends, then a failure: the spread at most doubles
        let (sn, ss) = flag_sense(""); let alt: Vec<bool> = (0..200).map(|k| k % 2 == 1).chain([true]).collect(); let (st, zs) = feed(&sn, ss, &alt);
        assert_eq!((st.var[0] <= 0.2, zs[200].0), (true, 1.527525)); assert!(((zs[200].0 / 3.878359) - 0.394).abs() < 5e-4);
    }

    /// 100,000 adversarial readings (the range ends in blocks of 7) never move the mean out of prior ± drift_cap nor the variance out
    /// of [0, var_cap]; the count stops at 1e6 (here: 100,000)
    #[test]
    fn adversarial_readings_stay_inside_the_caps() {
        let (sn, mut ss) = flag_sense(""); let (mut lo, mut hi) = (0.05f64, 0.05f64);
        let ev = |x: bool| json::parse(&format!(r#"{{"senses":{{"tool_fail":{x}}},"src":"env"}}"#)).unwrap().as_obj().unwrap().to_vec();
        let (on, off) = (ev(true), ev(false));
        for k in 0..100_000 { let raw = if (k / 7) % 3 != 0 { &on } else { &off }; ss = sense_step(&sn, &ss, raw, get(raw, "src"), true).unwrap().0;
            lo = lo.min(ss.mean[0]); hi = hi.max(ss.mean[0]); assert!((0.0..=0.2).contains(&ss.var[0]), "var {}", ss.var[0]); }
        assert!(lo >= -0.2 && hi <= 0.3, "mean in [{lo}, {hi}]"); assert_eq!((r6(lo), hi, ss.n[0]), (0.030778, 0.3, 100_000));
    }

    /// The lint rows of §2.11: each refused when the persona is read, at its path
    #[test]
    fn every_lint_row_is_refused_at_its_path() {
        let bad = |from: &str, to: &str| err(sen_with(from, to));
        // a taken sense_ id
        assert_eq!(bad(r#"{"id":"criticism","kind":"flag""#, r#"{"id":"sense_tool_fail_x","kind":"flag""#), "inputs[1].id: reserved by a sense (sense_<id>_*)");
        // an effect on an unknown target
        assert_eq!(bad(r#""effects":{"caution":0.8,"valence":-0.4}"#, r#""effects":{"spirit":0.8}"#), "senses[0].effects.spirit: unknown trait or mood \"spirit\"");
        assert_eq!(bad(r#""effects":{"caution":0.8,"valence":-0.4}"#, r#""effects":{"caution":[0,0,1]}"#), "senses[0].effects.caution: a number (an ordinal shift per unit of z)");
        // src naming self or clock
        assert_eq!(bad(r#""unit":"event","#, r#""unit":"event","src":["self"],"#), "senses[0].src[0]: a reading never comes from the individual itself");
        assert_eq!(bad(r#""unit":"event","#, r#""unit":"event","src":["env","clock"],"#), "senses[0].src[1]: the clock is not an instrument: readings come from env or human");
        // prior.mean outside the scaled range; prior.var <= 0; drift_cap <= 0; var_cap < prior.var; dead_zone >= zcap
        assert_eq!(bad(r#""prior":{"mean":0.05,"var":0.05}"#, r#""prior":{"mean":1.5,"var":0.05}"#), "senses[0].prior.mean: within the scaled range [0, 1]");
        assert_eq!(bad(r#""prior":{"mean":0.05,"var":0.05}"#, r#""prior":{"mean":0.05,"var":0}"#), "senses[0].prior.var: must be > 0");
        assert_eq!(bad(r#""drift_cap":0.25"#, r#""drift_cap":0"#), "senses[0].drift_cap: must be > 0");
        assert_eq!(bad(r#""drift_cap":0.25"#, r#""drift_cap":0.25,"var_cap":0.04"#), "senses[0].var_cap: must be >= prior.var");
        assert_eq!(bad(r#""drift_cap":0.25"#, r#""drift_cap":0.25,"zcap":2,"dead_zone":2"#), "senses[0].dead_zone: must be < zcap");
        // log1p on a flag or a negative range low
        assert_eq!(bad(r#""unit":"event","#, r#""unit":"event","scale":"log1p","#), "senses[0].scale: log1p needs a number sense (a flag's normal is a rate)");
        assert_eq!(bad(r#""range":[0,200]"#, r#""range":[-1,200],"scale":"log1p""#), "senses[1].scale: log1p needs a range low >= 0");
        // a cap with more than 6 decimals
        assert_eq!(bad(r#""drift_cap":0.25"#, r#""drift_cap":0.2500001"#), "senses[0].drift_cap: a cap has at most 6 decimals");
        assert_eq!(bad(r#""drift_cap":0.25"#, r#""drift_cap":0.25,"var_cap":0.2000001"#), "senses[0].var_cap: a cap has at most 6 decimals");
        // the schema: unknown fields, a number without a range, ids, kinds, counts
        assert_eq!(bad(r#""unit":"event","#, r#""unit":"event","norm":"host","#), "senses[0].norm: unknown field (allowed: comment, dead_zone, drift_cap, effects, gain_spread, habituation_spread, half_life_events, id, kind, prior, range, say, scale, src, threshold_spread, unit, var_cap, var_floor, zcap)");
        assert_eq!(bad(r#""range":[0,200],"#, ""), "senses[1].range: required for a number sense: [lo, hi]");
        assert_eq!(bad(r#""id":"tempo""#, r#""id":"tool_fail""#), "senses[1].id: sense ids must be distinct");
        assert_eq!(bad(r#""id":"tempo""#, r#""id":"a_very_long_sense_id_of_33_chars_x""#), "senses[1].id: at most 32 characters (a sense reserves the input ids sense_<id>_*)");
        assert!(err(build(&json::parse(&SEN.replace(r#""senses":["#, r#""senses":[],"x":["#)).unwrap())).starts_with(""), "the parser still reads");
        assert_eq!(err(build(&json::parse(r#"{"probbit_persona":1,"identity":{"name":"X","version":"1"},"traits":[{"id":"a","levels":["x","y"]}],"senses":[]}"#).unwrap())), "senses: a list of 1 to 24 senses");
    }

    /// S1: a reading for an undeclared sense, a number outside its range or a flag that is not true/false refuses the event at
    /// inputs.senses.<id>; nothing is clipped. S2: each reading's source kind must be in its `src`; missing, malformed, self and
    /// clock refuse the event whole. A refused event changes nothing (the turn is an error; the state is the caller's).
    #[test]
    fn s1_s2_readings_are_declared_in_range_and_sourced() {
        let p = sen(); let st = init(&p, Some(1), true, &run); let t = |ev: &str| err(turn(&p, &st, &json::parse(ev).unwrap(), false, &run, false));
        assert_eq!(t(r#"{"senses":{"latency":3},"src":"env"}"#), "inputs.senses.latency: not a declared sense (senses: tool_fail, tempo)");
        assert_eq!(t(r#"{"senses":{"tempo":201},"src":"env"}"#), "inputs.senses.tempo: a number in [0, 200] (messages)");
        assert_eq!(t(r#"{"senses":{"tempo":-1},"src":"env"}"#), "inputs.senses.tempo: a number in [0, 200] (messages)");
        assert_eq!(t(r#"{"senses":{"tool_fail":1},"src":"env"}"#), "inputs.senses.tool_fail: a flag sense reads true | false");
        assert_eq!(t(r#"{"senses":{"tool_fail":{"v":"yes","src":"env"}}}"#), "inputs.senses.tool_fail.v: a flag sense reads true | false");
        assert_eq!(t(r#"{"senses":{"tool_fail":true}}"#), "inputs.src: a reading of tool_fail needs a source: the event's src or its own (env)");
        assert_eq!(t(r#"{"senses":{"tool_fail":true},"src":"self"}"#), "inputs.src: a reading of tool_fail from the individual itself (src self) is refused: readings come from env");
        assert_eq!(t(r#"{"senses":{"tool_fail":true},"src":"clock"}"#), "inputs.src: a reading of tool_fail from the clock is refused: readings come from env");
        assert_eq!(t(r#"{"senses":{"tool_fail":true},"src":"human:a"}"#), "inputs.src: a reading of tool_fail from human is refused: tool_fail reads from env");
        assert_eq!(t(r#"{"senses":{"tool_fail":{"v":true,"src":"env:"}}}"#), "inputs.senses.tool_fail.src: a source: human[:id] | env[:id] (id: 1-64 of A-Z a-z 0-9 _ . - @ / :)");
        assert_eq!(t(r#"{"senses":{"tempo":3},"src":7}"#), "inputs.src: a source: human[:id] | env[:id] | self | clock (id: 1-64 of A-Z a-z 0-9 _ . - @ / :)");
        assert_eq!(t(r#"{"senses":[1]}"#), "inputs.senses: a mapping sense id -> reading");
        // accepted: a bare value takes the event's src, a reading may carry its own; self and clock may send events without readings
        for ev in [r#"{"senses":{"tool_fail":true,"tempo":{"v":3,"src":"env:chat"}},"src":"env:runner"}"#, r#"{"senses":{"tool_fail":{"v":false,"src":"env"}},"src":"self"}"#, r#"{"src":"self"}"#, r#"{"src":"clock","elapsed_hours":3}"#] {
            let (doc, _) = turn(&p, &st, &json::parse(ev).unwrap(), false, &run, false).unwrap_or_else(|e| panic!("{ev}: {}", e.msg));
            assert!(g(&doc, &["ignored"]).as_arr().unwrap().is_empty(), "{ev}: senses and src are read, not ignored"); }
        let (doc, _) = turn(&p, &st, &json::parse(r#"{"senses":{"tool_fail":true,"tempo":{"v":3,"src":"env:chat"}},"src":"env:runner"}"#).unwrap(), false, &run, false).unwrap();
        assert_eq!(canon(g(&doc, &["senses"])), r#"{"tempo":{"mean":1,"n":0,"raw":3,"src":"env:chat","z":1.990074,"zd":1.490074},"tool_fail":{"mean":0.05,"n":0,"raw":true,"src":"env:runner","z":3.878359,"zd":3.378359}}"#);
        assert_eq!(canon(g(&doc, &["inputs", "senses"])), r#"{"tempo":{"src":"env:chat","v":3},"tool_fail":true}"#);
    }

    /// S3: the engine owns the normal: a host cannot send the lowering's inputs or a pre-judged z, and z, the normals and the
    /// genes come from the logged readings only (two individuals fed the same readings have the same normals). S5: no rule reads
    /// the normal, and a condition on a sense not read this event is false. S6: normals move only on readings.
    #[test]
    fn s3_s5_s6_the_engine_owns_the_normal() {
        let p = sen(); let st = init(&p, Some(1), true, &run); let t = |ev: &str| err(turn(&p, &st, &json::parse(ev).unwrap(), false, &run, false));
        assert_eq!(t(r#"{"sense_tool_fail_up":6}"#), "inputs.sense_tool_fail_up: reserved for the senses block (the engine computes it from the readings)");
        assert_eq!(t(r#"{"sense_tool_fail_c0":true}"#), "inputs.sense_tool_fail_c0: reserved for the senses block (the engine computes it from the readings)");
        assert_eq!(t(r#"{"senses":{"tool_fail":{"v":true,"z":6}},"src":"env"}"#), "inputs.senses.tool_fail.z: unknown field (allowed: src, v)");
        let a = init(&p, Some(1), true, &run); let b = init(&p, Some(2), true, &run); let ev = json::parse(r#"{"senses":{"tool_fail":true,"tempo":40},"src":"env"}"#).unwrap();
        let (sa, sb) = (turn(&p, &a, &ev, false, &run, false).unwrap().1, turn(&p, &b, &ev, false, &run, false).unwrap().1);
        assert_eq!(canon(g(&sa.to_json(&p), &["senses", "norm"])), canon(g(&sb.to_json(&p), &["senses", "norm"])), "zero spreads: the normal is the readings'");
        // S5: a rule never reads the normal; an unread sense's condition is false (the habit is not in force)
        assert_eq!(err(sen_with(r#""when":{"sense.tool_fail.raw":true}"#, r#""when":{"sense.tool_fail.mean":{"at_least":0.2}}"#)), "habits[0].when.sense.tool_fail.mean: a rule reads sense.<id>.raw or sense.<id>.z, never the normal (mean, var, n)");
        assert_eq!(err(sen_with(r#""when":{"sense.tool_fail.raw":true}"#, r#""when":{"sense.latency.raw":true}"#)), "habits[0].when.sense.latency.raw: unknown sense \"latency\"");
        assert_eq!(err(sen_with(r#""when":{"sense.tool_fail.raw":true}"#, r#""when":{"sense.tool_fail.raw":1}"#)), "habits[0].when.sense.tool_fail.raw: true | false (a flag sense's reading)");
        let (quiet, _) = turn(&p, &st, &json::parse(r#"{"senses":{"tempo":3},"src":"env"}"#).unwrap(), false, &run, false).unwrap();
        assert!(g(&quiet, &["habits", "active"]).as_arr().unwrap().is_empty(), "tool_fail unread: its condition is false");
        let (fail, _) = turn(&p, &st, &json::parse(r#"{"senses":{"tool_fail":true},"src":"env"}"#).unwrap(), false, &run, false).unwrap();
        assert_eq!(canon(g(&fail, &["habits", "active"])), r#"["no_bold_after_failure"]"#);
        // S6: idle hours, events without readings and a cleared credit leave the normals as they were
        let (_, s1) = turn(&p, &st, &json::parse(r#"{"senses":{"tool_fail":true,"tempo":30},"src":"env"}"#).unwrap(), false, &run, false).unwrap();
        let (_, s2) = turn(&p, &s1, &json::parse(r#"{"elapsed_hours":720,"praise":true,"src":"human:a"}"#).unwrap(), false, &run, false).unwrap();
        let s3 = s2.zero_credit(&p);
        assert_eq!(canon(g(&s1.to_json(&p), &["senses"])), canon(g(&s3.to_json(&p), &["senses"])), "no clock decay");
    }

    /// S4: a state is refused, even with a recomputed digest, if a mean leaves prior ± drift_cap, a variance leaves [0, var_cap],
    /// n is not a whole number up to 1e6, the sense set differs from the persona's or a gene misses its seed
    #[test]
    fn s4_a_state_out_of_its_anchors_is_refused() {
        let p = sen_with(r#""effects":{"verbosity":-0.4}}"#, r#""effects":{"verbosity":-0.4},"gain_spread":0.5}"#).unwrap(); let st = init(&p, Some(4), true, &run);
        assert!(State::read(&p, &st.to_json(&p)).is_ok());
        let reseal = |f: &dyn Fn(&mut SState)| { let mut s = st.clone(); f(s.senses.as_mut().unwrap()); s.seal(&p); err(State::read(&p, &s.to_json(&p))) };
        assert_eq!(reseal(&|s| s.mean[0] = 0.31), "state.senses.norm.tool_fail.mean: within prior.mean ± drift_cap: [-0.2, 0.3]");
        assert_eq!(reseal(&|s| s.mean[1] = -1.5), "state.senses.norm.tempo.mean: within prior.mean ± drift_cap: [-1, 3]");
        assert_eq!(reseal(&|s| s.var[0] = 0.21), "state.senses.norm.tool_fail.var: within [0, var_cap = 0.2]");
        assert_eq!(reseal(&|s| s.var[1] = -0.01), "state.senses.norm.tempo.var: within [0, var_cap = 4]");
        assert_eq!(reseal(&|s| s.n[0] = 1_000_001), "state.senses.norm.tool_fail.n: a whole number from 0 to 1000000");
        assert_eq!(reseal(&|s| s.genes = sense_genes("Sensor", &p.senses.clone().unwrap(), 5)), "state.senses.genes: sense genes do not match the seed");
        let body = |j: &Json| Json::Obj(j.as_obj().unwrap().iter().filter(|(k, _)| k != "digest").cloned().collect());
        let edit = |f: &dyn Fn(&mut Vec<(String, Json)>)| { let mut kv: Vec<(String, Json)> = body(&st.to_json(&p)).as_obj().unwrap().to_vec();
            if let Some((_, Json::Obj(sv))) = kv.iter_mut().find(|(k, _)| k == "senses") { if let Some((_, Json::Obj(nv))) = sv.iter_mut().find(|(k, _)| k == "norm") { f(nv); } }
            let b = Json::Obj(kv.clone()); kv.push(("digest".into(), Json::Str(sha(&b)))); err(State::read(&p, &Json::Obj(kv))) };
        assert_eq!(edit(&|n| { n.retain(|(k, _)| k != "tempo"); }), "state.senses.norm: one normal per sense of the persona (tool_fail, tempo)");
        assert_eq!(edit(&|n| { n.push(("latency".into(), json::parse(r#"{"mean":0,"var":1,"n":0}"#).unwrap())); }), "state.senses.norm: one normal per sense of the persona (tool_fail, tempo)");
        assert_eq!(edit(&|n| { if let Some((_, Json::Obj(t))) = n.iter_mut().find(|(k, _)| k == "tempo") { t.iter_mut().for_each(|(k, v)| if k == "n" { *v = Json::Num(2.5) }); } }), "state.senses.norm.tempo.n: a whole number from 0 to 1000000");
        // without the block a state refuses a senses field; with it the field is required
        let q = persona(""); let q = q.unwrap(); let j = init(&q, None, true, &run).to_json(&q);
        let mut kv: Vec<(String, Json)> = j.as_obj().unwrap().iter().filter(|(k, _)| k != "digest").cloned().collect(); kv.push(("senses".into(), Json::Obj(vec![])));
        let b = Json::Obj(kv.clone()); kv.push(("digest".into(), Json::Str(sha(&b))));
        assert_eq!(err(State::read(&q, &Json::Obj(kv))), "state.senses: this persona has no senses block");
        let mut s = st.clone(); s.senses = None; s.seal(&p); assert_eq!(err(State::read(&p, &s.to_json(&p))), "state.senses: required with a senses block");
    }

    /// Genes come from name, seed and sense id: editing an unrelated sense (its prior, caps, effects, spreads) or adding one keeps
    /// every other sense's genes and every trait's; another seed is another individual
    #[test]
    fn sense_genes_survive_an_unrelated_edit() {
        let spreads = r#","gain_spread":0.3,"threshold_spread":0.3,"habituation_spread":0.3"#;
        let base = SEN.replace(r#""say":"tool failures""#, &format!(r#""say":"tool failures"{spreads}"#)).replace(r#""drift_cap":2"#, &format!(r#""drift_cap":2{spreads}"#));
        let p = build(&json::parse(&base).unwrap()).unwrap();
        let q = build(&json::parse(&base.replace(r#""prior":{"mean":1,"var":1},"drift_cap":2"#, r#""prior":{"mean":5,"var":4},"drift_cap":3"#).replace(r#""effects":{"verbosity":-0.4}"#, r#""effects":{"verbosity":-1.0}"#)).unwrap()).unwrap();
        let (gp, gq) = (sense_genes(&p.name, p.senses.as_ref().unwrap(), 7), sense_genes(&q.name, q.senses.as_ref().unwrap(), 7));
        for k in ["gain", "threshold", "habituation"] { assert_eq!(g(&gp, &[k, "tool_fail"]), g(&gq, &[k, "tool_fail"]), "{k}"); assert_eq!(g(&gp, &[k, "tempo"]), g(&gq, &[k, "tempo"]), "{k}: the edit is not a gene"); }
        assert_eq!(canon(&genes(&p, 7)), canon(&genes(&q, 7)), "trait genes too");
        assert_ne!(canon(&gp), canon(&sense_genes(&p.name, p.senses.as_ref().unwrap(), 8)), "another seed, another individual");
        assert_ne!(p.digest, q.digest, "the edit changes the persona digest");
    }

    /// A habit on `sense.tool_fail.raw` holds by construction for the rule with the same condition; a rule with a condition the
    /// habits do not read is refused (a property reads the persona's own conditions)
    #[test]
    fn a_habit_on_a_raw_reading_holds_by_construction() {
        let p = sen(); let pr = |t: &str| props(&p, &json::parse(t).unwrap(), "--never", "never");
        let rule = pr(r#"{"when":{"sense.tool_fail.raw":true},"then":{"caution":{"not":["bold"]}}}"#).unwrap();
        assert_eq!(by_construction(&p, &rule[0], &run), Some(vec!["no_bold_after_failure".to_string()]));
        let busy = pr(r#"{"when":{"sense.tempo.raw":{"at_least":20}},"then":{"verbosity":["terse","short"]}}"#).unwrap();
        assert_eq!(by_construction(&p, &busy[0], &run), Some(vec!["short_when_busy".to_string()]));
        assert_eq!(err(pr(r#"{"when":{"sense.tool_fail.z":{"at_least":2}},"then":{"caution":{"not":["bold"]}}}"#)),
            "--never.when.sense.tool_fail.z: a property reads the sense conditions the persona's habits use: sense.tool_fail.raw = true, sense.tempo.raw >= 20");
        assert_eq!(err(pr(r#"{"when":{"sense_tool_fail_c0":true},"then":{"caution":["careful"]}}"#)), "--never.when.sense_tool_fail_c0: reserved for the senses block (a rule reads sense.<id>.raw or sense.<id>.z)");
        // the fuzzer's grammar: readings nested into `senses`; forcing puts the raw condition in force by its reading
        let a = alphabet(&p, &[0.0, 0.5, 1.0], &[]); let tf = a.iter().find(|x| x.0 == "sense.tool_fail").unwrap(); assert_eq!(canon(&Json::Arr(tf.2.clone())), "[true,false]");
        assert_eq!(canon(&Json::Arr(a.iter().find(|x| x.0 == "sense.tempo").unwrap().2.clone())), "[0,100,200,20]");
        assert!(!a.iter().any(|x| x.0.starts_with("sense_")), "the lowering's inputs are not on the alphabet");
        assert_eq!(canon(&event_json(&[("praise".into(), Json::Bool(true)), ("sense.tempo".into(), Json::Num(20.0))])), r#"{"praise":true,"senses":{"tempo":20}}"#);
        assert_eq!(forcing(&p, &busy[0], &[0.0, 0.5, 1.0]), vec![vec![("sense.tempo".to_string(), Json::Num(100.0))]]);
        // prove reads the lowering's inputs as boxes: the rule is proved for every event sequence where it is not held outright
        assert!(prove_seed(&p, &rule[0], 0, &run).is_ok());
    }

    /// The stance's `why` names the past: the normal clause (far above / above / usual for it now) and, with learning, the
    /// learned clause and its counts in `learned_from`; `line.why_past: false` keeps the learned clause out
    #[test]
    fn why_names_the_normal_and_what_the_weights_came_from() {
        let p = sen(); let mut st = init(&p, Some(0), true, &run);
        let ev = |s: &str| json::parse(s).unwrap();
        let (d, _) = turn(&p, &st, &ev(r#"{"senses":{"tool_fail":true},"src":"env"}"#), false, &run, false).unwrap();
        assert_eq!((g(&d, &["noticed"]).as_str(), g(&d, &["why"]).as_str().unwrap().starts_with("tool failures far above its normal (z +3.9 over 0 readings)")), (Some("tool_fail"), true), "{}", canon(&d));
        // beyond the drift cap a reading never becomes usual: tempo 30 stays far above its normal (mean <= 1 + 2)
        let mut s30 = st.clone(); for _ in 0..30 { s30 = turn(&p, &s30, &ev(r#"{"senses":{"tempo":30},"src":"env"}"#), false, &run, false).unwrap().1; }
        let (d, _) = turn(&p, &s30, &ev(r#"{"senses":{"tempo":30},"src":"env"}"#), false, &run, false).unwrap();
        assert!(g(&d, &["why"]).as_str().unwrap().starts_with("tempo far above its normal (z +6.0 over 30 readings)"), "{}", g(&d, &["why"]).as_str().unwrap());
        // within it the normal follows: tempo 2.5 repeated becomes usual for this individual (|z0| - |z| >= 1)
        let (d, _) = turn(&p, &st, &ev(r#"{"senses":{"tempo":2.5},"src":"env"}"#), false, &run, false).unwrap();
        assert!(g(&d, &["why"]).as_str().unwrap().starts_with("tempo above its normal (z +1.5 over 0 readings)"), "{}", g(&d, &["why"]).as_str().unwrap());
        for _ in 0..30 { st = turn(&p, &st, &ev(r#"{"senses":{"tempo":2.5},"src":"env","criticism":true}"#), false, &run, false).unwrap().1; }
        let (d, _) = turn(&p, &st, &ev(r#"{"senses":{"tempo":2.5},"src":"env"}"#), false, &run, false).unwrap();
        assert!(g(&d, &["why"]).as_str().unwrap().contains("tempo usual for it now (z "), "{}", g(&d, &["why"]).as_str().unwrap());
        assert!(g(&d, &["why"]).as_str().unwrap().contains("learned: caution "), "{}", g(&d, &["why"]).as_str().unwrap());
        let lf = g(&st.to_json(&p), &["learned_from", "caution"]).clone(); assert_eq!((g(&lf, &["rewards"]), g(&lf, &["corrections"]).as_f64().unwrap() > 0.0), (&Json::Num(0.0), true), "{}", canon(&lf));
        let q = sen_with(r#""reward_from":["human","env"]"#, r#""reward_from":["human","env"],"line":{"why_past":false}"#).unwrap();
        assert!(init(&q, Some(0), true, &run).to_json(&q).get("learned_from").is_none(), "why_past off: no learned_from");
        // without the block every document is as before: no senses, noticed or learned_from
        let o = persona(LEARN).unwrap(); let so = init(&o, Some(0), true, &run); let (od, _) = turn(&o, &so, &inputs(true, false, false), false, &run, false).unwrap();
        assert!(od.get("senses").is_none() && od.get("noticed").is_none() && so.to_json(&o).get("learned_from").is_none());
    }

    #[test]
    fn prove_puts_the_drives_in_their_box() {
        let rule = json::parse(r#"{"when":{"security":true},"then":{"pursue":["safety"]}}"#).unwrap();
        let p = build(&json::parse(BOX).unwrap()).unwrap(); let pr = props(&p, &rule, "--never", "never").unwrap();
        let (cell, why) = prove_seed(&p, &pr[0], 0, &run).unwrap_err();
        assert_eq!((canon(&cell), why.as_str()), (r#"{"drives":{},"history":{},"inputs":{"security":true},"number_halfwidth":{},"prev":{}}"#.to_string(), "pursue: the score gap 2.5000 is within the bound 5.5000"));
        let script = json::parse(r#"[{"goals":{"fun":{"deadline_hours":1}}},{"goals":{"safety":{"win":1}}},{"goals":{"safety":{"win":2}}},{"goals":{"fun":{"cue":true}},"security":true}]"#).unwrap();
        let mut st = init(&p, Some(0), true, &run); let mut last = None;
        for ev in script.as_arr().unwrap() { let (doc, ns) = turn(&p, &st, ev, false, &run, false).unwrap(); last = breaks(&p, &pr[0], &st, &doc); st = ns; }
        assert_eq!(last.map(|b| b[0].level.clone()), Some("fun".to_string()), "the drives out-want the 2.5 margin");
        let q = build(&json::parse(&BOX.replace("2.5", "6.5")).unwrap()).unwrap();
        assert_eq!(prove_seed(&q, &props(&q, &rule, "--never", "never").unwrap()[0], 0, &run).ok(), Some((1, 2)));
    }

    // ------------------------------------------------------------- action-addressed feedback (§2.8, RFC 0001 B.2 and B.3)
    const CREDIT: &str = r#","learning":{"from":["praise","criticism"],"traits":["verbosity","humour"],"rate":0.5,"step_cap":0.2,"total_cap":0.6,"credit":{"max_open":3,"half_life_events":32}}"#;
    /// One turn on the event `ev`: (stance, new state), or the refusal as "path: message"
    fn tn(p: &Persona, st: &State, ev: &str) -> Result<(Json, State), String> { turn(p, st, &json::parse(ev).unwrap(), false, &run, false).map_err(|e| format!("{}: {}", e.path, e.msg)) }
    fn opened(st: &State) -> Vec<u64> { st.open.iter().map(|o| o.turn).collect() }

    /// The RFC arithmetic: a payment `age` events late (age = t − n − 1) gets e = 0.5^(age / 32), 6 decimals (1, 0.937084, 0.805245,
    /// 0.5 and 0.063869 at ages 0, 3, 10, 32 and 127) and moves the 0.8 step scaled by e; at age 128 the decision has expired
    /// (closed as expired before anything is paid) and paying it is refused (B-R2)
    #[test]
    fn eligibility_halves_per_half_life_and_a_decision_expires_after_four() {
        let p = persona(CREDIT).unwrap(); let l = p.learning.clone().unwrap();
        for (age, e) in [(0u64, 1.0), (3, 0.937084), (10, 0.805245), (32, 0.5), (127, 0.063869)] {
            let mut st = tn(&p, &init(&p, Some(1), true, &run), r#"{"await":true}"#).unwrap().1; let c0 = st.open[0].credit.clone();
            assert!(c0.iter().any(|(_, v)| v.iter().any(|x| *x != 0.0)), "the awaited stance carries credit");
            for _ in 0..age { st = tn(&p, &st, "{}").unwrap().1; }
            let (doc, ns) = tn(&p, &st, r#"{"praise":true,"for":0}"#).unwrap();
            assert_eq!(g(&doc, &["credit", "paid", "e"]).as_f64(), Some(e), "age {age}");
            assert_eq!(canon(&table(&ns.learned)), canon(&table(&learn(&l, &c0, 1.0, e, &st.learned))), "age {age}: the step scaled by e");
            assert_eq!((canon(g(&doc, &["credit", "closed"])), ns.open.len()), (r#"[{"as":"paid","turn":0}]"#.to_string(), 0), "age {age}");
        }
        let mut st = tn(&p, &init(&p, Some(1), true, &run), r#"{"await":true}"#).unwrap().1;
        for _ in 0..128 { st = tn(&p, &st, "{}").unwrap().1; }
        assert_eq!(opened(&st), [0], "open until an event 128 late");
        let (doc, ns) = tn(&p, &st, "{}").unwrap(); assert_eq!(canon(g(&doc, &["credit", "closed"])), r#"[{"as":"expired","turn":0}]"#); assert!(ns.open.is_empty());
        let e = tn(&p, &st, r#"{"praise":true,"for":0}"#).err().unwrap(); assert!(e.starts_with("inputs.for: turn 0 is neither open nor the previous stance (128)"), "{e}");
    }

    /// At most max_open decisions wait (a fourth `await` evicts the oldest, which can no longer be paid), and every refusal row
    /// B-R1 to B-R6 refuses its event whole at its path; a payment of a habit-forced stance moves nothing and says `absorbed`
    #[test]
    fn eviction_every_refusal_row_and_absorbed() {
        let p = persona(CREDIT).unwrap(); let mut st = init(&p, Some(2), true, &run); let mut docs = vec![];
        for _ in 0..4 { let (d, s) = tn(&p, &st, r#"{"await":true}"#).unwrap(); docs.push(d); st = s; }
        assert_eq!((canon(g(&docs[3], &["credit", "closed"])), canon(g(&docs[3], &["credit", "open"]))), (r#"[{"as":"evicted","turn":0}]"#.to_string(), "[1,2,3]".to_string()));
        let st = tn(&p, &st, "{}").unwrap().1; // turn 5: open 1, 2, 3; the previous stance is 4
        let s5 = tn(&p, &st, r#"{"praise":true,"for":1}"#).unwrap().1; let s6 = tn(&p, &s5, r#"{"cancel":[2],"src":"env:executor"}"#).unwrap().1; // turn 7: open 3
        let z = "0".repeat(64); let d3 = g(&docs[3], &["credit", "stance"]).as_str().unwrap().to_string();
        let rows: Vec<(&State, String, &str)> = vec![
            (&st, r#"{"praise":true}"#.into(), "inputs.for: feedback without `for` while other decisions are open (1, 2, 3)"),          // B-R1
            (&st, r#"{"praise":true,"for":0}"#.into(), "inputs.for: turn 0 is neither open nor the previous stance (4)"),               // B-R2 evicted
            (&st, r#"{"praise":true,"for":9}"#.into(), "inputs.for: turn 9 is neither open"),                                           // B-R2 future
            (&s6, r#"{"praise":true,"for":1}"#.into(), "inputs.for: turn 1 is neither open"),                                           // B-R2 paid
            (&s6, r#"{"praise":true,"for":2}"#.into(), "inputs.for: turn 2 is neither open"),                                           // B-R2 cancelled
            (&s6, r#"{"praise":true,"for":4}"#.into(), "inputs.for: turn 4 is neither open"),                                           // B-R2 never opened
            (&st, r#"{"for":1}"#.into(), "inputs.for: this event carries no feedback"),                                                 // B-R3
            (&st, r#"{"cancel":[0],"src":"env"}"#.into(), "inputs.cancel[0]: turn 0 is not open (open: 1, 2, 3)"),                      // B-R4 not open
            (&st, r#"{"cancel":[1,1],"src":"env"}"#.into(), "inputs.cancel[1]: listed twice"),                                          // B-R4 twice
            (&st, r#"{"praise":true,"for":1,"cancel":[1],"src":"env"}"#.into(), "inputs.cancel[0]: turn 1 is the decision this event's feedback pays"), // B-R4 paid now
            (&st, r#"{"await":"yes"}"#.into(), "inputs.await: "), (&st, r#"{"await":1}"#.into(), "inputs.await: "),                     // B-R5
            (&st, r#"{"praise":true,"for":1.5}"#.into(), "inputs.for: "), (&st, r#"{"praise":true,"for":-1}"#.into(), "inputs.for: "),
            (&st, r#"{"praise":true,"for":"1"}"#.into(), "inputs.for: "), (&st, r#"{"praise":true,"for":{"turn":1}}"#.into(), "inputs.for"),
            (&st, r#"{"praise":true,"for":{"turn":1,"stance":"1"}}"#.into(), "inputs.for.stance: "),
            (&st, r#"{"cancel":1,"src":"env"}"#.into(), "inputs.cancel: "), (&st, r#"{"cancel":[1,2,3,4],"src":"env"}"#.into(), "inputs.cancel: "),
            (&st, r#"{"cancel":[0.5],"src":"env"}"#.into(), "inputs.cancel[0]: "), (&st, r#"{"cancel":[1]}"#.into(), "inputs.src: a cancel needs a source"),
            (&st, r#"{"cancel":[1],"src":"self"}"#.into(), "inputs.src: a cancel needs a source"), (&st, r#"{"cancel":[1],"src":"clock"}"#.into(), "inputs.src: a cancel needs a source"),
            (&s6, format!(r#"{{"praise":true,"for":{{"turn":3,"stance":"sha256:{z}"}}}}"#), "inputs.for.stance: turn 3's stance is sha256:"), // B-R6
        ];
        for (s, ev, want) in &rows { let e = tn(&p, s, ev).map(|_| "accepted".to_string()).unwrap_or_else(|e| e); assert!(e.starts_with(want), "{ev}: {e}"); }
        // the digest-bound payment of the stance it names is accepted and echoes it
        let (doc, ns) = tn(&p, &s6, &format!(r#"{{"praise":true,"for":{{"turn":3,"stance":"{d3}"}}}}"#)).unwrap();
        assert_eq!((g(&doc, &["credit", "paid", "stance"]).as_str(), g(&doc, &["credit", "paid", "turn"]).as_f64(), opened(&ns)), (Some(d3.as_str()), Some(3.0), vec![]));
        // a habit-forced stance carries no credit: paying it moves nothing (absorbed), and learned_from does not count it
        let q = persona(&CREDIT.replace(r#""traits":["verbosity","humour"]"#, r#""traits":["humour"]"#)).unwrap();
        let s1 = tn(&q, &init(&q, Some(3), true, &run), r#"{"loss":true,"await":true}"#).unwrap().1; let s2 = tn(&q, &s1, "{}").unwrap().1;
        let (doc, s3) = tn(&q, &s2, r#"{"praise":true,"for":0}"#).unwrap();
        assert_eq!((g(&doc, &["credit", "paid", "absorbed"]), &s3.learned), (&Json::Bool(true), &s2.learned));
    }

    /// The credit state is read strictly: at most max_open open decisions, turns increasing and before the state's turn, credits
    /// within ±1, stance digests; prev_stance a digest once a turn has run; without the block the fields are refused
    #[test]
    fn the_credit_state_is_strict() {
        let p = persona(CREDIT).unwrap(); let mut st = init(&p, Some(4), true, &run);
        for ev in [r#"{"await":true}"#, r#"{"await":true}"#, "{}"] { st = tn(&p, &st, ev).unwrap().1; }
        assert_eq!((opened(&st), State::read(&p, &st.to_json(&p)).is_ok()), (vec![0, 1], true));
        let bad = |f: &dyn Fn(&mut State)| { let mut s = st.clone(); f(&mut s); s.seal(&p); err(State::read(&p, &s.to_json(&p))) };
        let cases: Vec<(String, &str)> = vec![(bad(&|s| s.open[0].credit[0].1[0] = 1.5), "state.open_credit[0].credit"), (bad(&|s| s.open[1].turn = 0), "state.open_credit[1].turn"),
            (bad(&|s| s.open[1].turn = 3), "state.open_credit[1].turn"), (bad(&|s| s.open[0].stance = "sha256:1".into()), "state.open_credit[0].stance"),
            (bad(&|s| { let o = s.open[1].clone(); for k in 0..2 { s.open.push(Open { turn: o.turn + 1 + k, ..o.clone() }); } s.turn += 2; }), "state.open_credit"),
            (bad(&|s| s.prev_stance = None), "state.prev_stance"), (bad(&|s| s.control_closed = vec![2, 1]), "state.control_closed[1]")];
        for (e, want) in &cases { assert!(e.starts_with(want), "{want}: {e}"); }
        // the same fields for a persona without the block are refused by name
        let o = persona(LEARN).unwrap(); let Json::Obj(v) = st.to_json(&p) else { unreachable!() };
        let mut v: Vec<(String, Json)> = v.into_iter().filter(|(k, _)| k != "digest").map(|(k, x)| if k == "persona" { (k, json::parse(&canon(&x).replace(&p.digest, &o.digest)).unwrap()) } else { (k, x) }).collect();
        let d = sha(&Json::Obj(v.clone())); v.push(("digest".into(), Json::Str(d)));
        let e = err(State::read(&o, &Json::Obj(v))); assert!(e.contains("this persona has no credit block (learning.credit)"), "{e}");
    }

    /// e = 1 is 0.8 (RFC 0001 B.2, B.4): on every shipped learning persona, a copy with a credit block whose feedback names the
    /// previous stance (`for: t − 1`), and the same copy unaddressed, learn exactly what the persona without the block learns: the
    /// same Δ and learned_from after every turn
    #[test]
    fn paying_the_previous_stance_by_name_learns_as_0_8() {
        let ex = |f: &str| format!("{}/../examples/{f}", env!("CARGO_MANIFEST_DIR"));
        let with = |doc: &Json, c: Option<Json>| { let Json::Obj(v) = doc else { unreachable!() };
            build(&Json::Obj(v.iter().map(|(k, x)| (k.clone(), if k != "learning" { x.clone() } else {
                let mut l: Vec<(String, Json)> = x.as_obj().unwrap().iter().filter(|(k, _)| k != "credit").cloned().collect();
                if let Some(c) = &c { l.push(("credit".into(), c.clone())); } Json::Obj(l) })).collect())).unwrap() };
        let go = |p: &Persona, st: &State, e: &[(String, Json)]| turn_any_source(p, st, &event_json(e), false, &run, false).unwrap().1;
        for (i, f) in ["agent-harness/adaptive.json", "agent-harness/guarded.json", "agent-harness/addressed.json", "persona/sensed.yaml", "persona/dispatch-policy.yaml"].iter().enumerate() {
            let doc = load(&ex(f)).unwrap().1; let (old, new) = (with(&doc, None), with(&doc, Some(json::parse(r#"{"max_open":8,"half_life_events":32}"#).unwrap())));
            assert!(old.credit().is_none() && new.credit().is_some(), "{f}");
            let alpha = alphabet(&old, &[0.5, 1.0], &[0.5, 6.0]); let mut r = Philox4x32::new(77, i as u64); let seed = Some(i as u64);
            let (mut so, mut sa, mut sb) = (init(&old, seed, true, &run), init(&new, seed, true, &run), init(&new, seed, true, &run)); let mut moved = 0;
            for t in 0..160u64 {
                let mut e: Vec<(String, Json)> = vec![];
                for (id, kind, vals) in &alpha { if r.f64() < if learning_flag(&old, id) { 0.5 } else if *kind == "flag" { 0.3 } else { 0.2 } { e.push((id.clone(), vals[r.below(vals.len())].clone())); } }
                let mut named = e.clone();
                if t > 0 && e.iter().any(|(k, x)| learning_flag(&old, k) && *x == Json::Bool(true)) { named.push(("for".into(), Json::Num((t - 1) as f64))); }
                let (no, na, nb) = (go(&old, &so, &e), go(&new, &sa, &named), go(&new, &sb, &e));
                for s in [&na, &nb] { assert_eq!((canon(&table(&s.learned)), &s.learned_from), (canon(&table(&no.learned)), &no.learned_from), "{f} turn {t}"); }
                if no.learned != so.learned { moved += 1; }
                (so, sa, sb) = (no, na, nb); }
            assert!(moved >= 5, "{f}: feedback moved the deltas on {moved} turns"); // a one-sided learner reaches total_cap in 8 steps
        }
    }

    // ---------------------------------------------------------------- RFC 0002 wave 1a: needs, the omission let-down, valence
    /// bench/body's needs2 as JSON: `context` grows per reading of the number sense `tokens` (amount 0.6, env), `contact` 0.04 an hour
    /// (amount 0.8, human); bound to compact and report, each with a one-hour window; quiet hours exclude report
    const NEED: &str = r#"{"probbit_persona":1,"identity":{"name":"Body needs2","version":"1.0.0","seed":0},"traits":[{"id":"tone","levels":["plain","warm"]}],
        "inputs":[{"id":"quiet","kind":"flag","say":"quiet hours"}],
        "senses":[{"id":"tokens","kind":"number","unit":"token_share","range":[0,1],"src":["env"],"prior":{"mean":0.06,"var":0.01},"drift_cap":0.5}],
        "drives":{"goals":[{"id":"work","interest":1.0},{"id":"compact","interest":0.6,"expect_by_hours":1},{"id":"report","interest":0.6,"expect_by_hours":1}],"pursue":{"glow":0}},
        "needs":{"drive":{"n":4,"m":3},"pursue":{"want":2,"cap":3},"success":{"rate":0.2,"init":1},"r_cap":1,"items":[
          {"id":"context","say":"context budget","unit":"token_share","cap":1,"weight":1,"per_hour":0,"per_reading":{"sense":"tokens","rate":1},"satisfy_from":["env"],"amount":0.6,"bind":"compact","forecast_per_hour":0.06},
          {"id":"contact","unit":"share","cap":1,"weight":1,"per_hour":0.04,"satisfy_from":["human"],"amount":0.8,"bind":"report"}]},
        "habits":[{"id":"quiet_hours","when":{"quiet":true},"then":{"pursue":{"not":["report"]}}}]}"#;
    fn need2() -> Persona { build(&json::parse(NEED).unwrap()).unwrap() }
    fn need_with(from: &str, to: &str) -> R<Persona> { assert!(NEED.contains(from), "{from}"); build(&json::parse(&NEED.replacen(from, to, 1)).unwrap()) }
    /// bench/body's needs1: one need (0.1 an hour, amount 0.2, env) bound to `refuel` (window 1 h), drives at their defaults
    const NEED1: &str = r#"{"probbit_persona":1,"identity":{"name":"Body needs1","version":"1.0.0","seed":0},"traits":[{"id":"tone","levels":["plain","warm"]}],
        "drives":{"goals":[{"id":"refuel","expect_by_hours":1},{"id":"idle"}]},
        "needs":{"drive":{"n":4,"m":3},"r_cap":1,"items":[{"id":"fuel","unit":"share","cap":1,"weight":1,"per_hour":0.1,"satisfy_from":["env"],"amount":0.2,"bind":"refuel"}]}}"#;
    fn need1() -> Persona { build(&json::parse(NEED1).unwrap()).unwrap() }
    /// bench/body's valence.yaml as JSON: valence on the mood `valence` (expect 0.2, surprise 0.6, inertia 0.61, half-life 1 h); `task`
    /// has a two-hour window; the habits pick the pursued goal
    const VAL: &str = r#"{"probbit_persona":1,"identity":{"name":"Body valence","version":"1.0.0","seed":0},"traits":[{"id":"tone","levels":["plain","warm"]}],
        "moods":[{"id":"valence","levels":["down","even","up"],"inertia":0.61,"half_life_hours":1}],
        "inputs":[{"id":"go_task","kind":"flag"},{"id":"go_other","kind":"flag"}],
        "drives":{"goals":[{"id":"task","expect_by_hours":2},{"id":"other"}],"valence":{"mood":"valence","expect":0.2,"surprise":0.6}},
        "habits":[{"id":"go_task","when":{"go_task":true},"then":{"pursue":["task"]}},{"id":"go_other","when":{"go_other":true},"then":{"pursue":["other"]}}],
        "engine":{"on_conflict":"yield"}}"#;
    fn val_with(from: &str, to: &str) -> R<Persona> { assert!(VAL.contains(from), "{from}"); build(&json::parse(&VAL.replacen(from, to, 1)).unwrap()) }
    fn evj(t: &str) -> Json { json::parse(t).unwrap() }
    /// One turn that must be accepted -> (the stance, the state after it)
    fn ok_turn(p: &Persona, st: &State, ev: &str) -> (Json, State) { turn(p, st, &evj(ev), false, &run, false).unwrap_or_else(|e| panic!("{ev}: {}: {}", e.path, e.msg)) }
    /// The drive D(x) and the reduction r = D(x) - D(x with x_j <- |x_j - a|) of RFC 0002 A.3, n 4, m 3, unit weights
    fn drive_of(xs: &[f64]) -> f64 { xs.iter().map(|x| x.powf(4.0)).sum::<f64>().powf(1.0 / 3.0) }

    /// RFC 0002 A.3's closed forms on the engine: the pay ratio 2.0229 (amount 0.2 at deficits 0.8 and 0.2), the overshoot -0.117 at
    /// deficit 0 (a setback, read through wanting), the deficit after |x - a|; the x > K/2 sign rule of the reduction on the 0.01 grid
    /// and of the pursue term at horizon 0 on the same grid
    #[test]
    fn needs_pay_the_closed_forms() {
        let p = need1(); let st = init(&p, Some(0), true, &run);
        let win = |h: f64| { let (doc, ns) = ok_turn(&p, &st, &format!(r#"{{"elapsed_hours":{h},"needs":{{"fuel":0.2}},"src":"env"}}"#));
            (g(&doc, &["drives", "expect", "refuel"]).as_f64().unwrap(), g(&doc, &["drives", "want", "refuel"]).as_f64().unwrap(), g(&doc, &["needs", "fuel", "deficit"]).as_f64().unwrap(), ns) };
        let r = |x: f64, a: f64| r6(drive_of(&[x]) - drive_of(&[(x - a).abs()]));
        let (e8, _, x8, s8) = win(8.0); let (e2, _, x2, _) = win(2.0); let (e0, w0, x0, _) = win(0.0);
        assert_eq!((r(0.8, 0.2), r(0.2, 0.2), r(0.0, 0.2)), (0.236595, 0.116961, -0.116961));
        assert!((2.0 * e8 - 0.236595).abs() <= 2e-6 && (2.0 * e2 - 0.116961).abs() <= 2e-6, "the win through E (rate 0.5, two roundings): {e8}, {e2}");
        assert_eq!(round_to(r(0.8, 0.2) / r(0.2, 0.2), 4), 2.0229); assert_eq!(round_to(r(0.0, 0.2), 3), -0.117);
        assert_eq!((x8, x2, x0, e0), (0.6, 0.0, 0.2, 0.0), "the deficit after is |x - a|; an overshoot pays no win");
        assert!((w0 - 0.5 * 0.116961).abs() <= 1e-6, "the overshoot is a setback of 0.116961 (wanting weight 0.5): {w0}");
        assert_eq!(g(&s8.to_json(&p), &["needs", "success", "refuel"]).as_f64(), Some(1.0), "a win at s 1 keeps s 1");
        // the sign rule: sign(D(x) - D(|x - K|)) = sign(x - K/2) for one need, x and K on the 0.01 grid
        let (d, nb) = (p.drives.as_ref().unwrap(), p.drives.as_ref().unwrap().needs.as_ref().unwrap()); let nd = st.drives.as_ref().unwrap().nd.clone().unwrap();
        let sgn = |v: f64, tol: f64| (v > tol) as i32 - (v < -tol) as i32; let mut bad = 0;
        for i in 0..=100 { for k in 1..=100 { let (x, kk) = (i as f64 / 100.0, k as f64 / 100.0);
            if sgn(need_drive(nb, &nd, &[x]) - need_drive(nb, &nd, &[(x - kk).abs()]), 1e-12) != sgn(x - kk / 2.0, 1e-12) { bad += 1; } } }
        assert_eq!(bad, 0, "grid points where the reduction's sign is not sign(x - K/2)");
        // at horizon 0 the term (want 2, s 1, K 0.2) is positive exactly when x > K/2
        for i in 0..=100 { let x = i as f64 / 100.0; let mut ds = st.drives.clone().unwrap(); ds.nd.as_mut().unwrap().deficit[0] = x;
            let t = need_terms(d, &ds)[0]; assert_eq!((t.0, sgn(t.1, 0.0)), (x, sgn(x - 0.1, 1e-12)), "term at x {x}: {}", t.1);
            assert_eq!(t.1, r6((2.0 * (drive_of(&[x]) - drive_of(&[(x - 0.2).abs()]))).clamp(-3.0, 3.0)), "the term's closed form at x {x}"); }
    }

    /// Two passes per event (RFC 0002 A.3, as reworded after the freeze): every need grows, then each satisfaction pays in declared
    /// order; the deficits do not depend on the order, the wins only through D over the other needs, and a win joins its goal's
    /// signals as any win does (E, s); the forecast reads forecast_per_hour x horizon
    #[test]
    fn the_needs_step_grows_every_need_then_pays_in_order() {
        let p = need2(); let st = init(&p, Some(3), true, &run);
        // 15 h: contact grows to 0.6, context by the reading 0.5; then both are satisfied in one event
        let (doc, ns) = ok_turn(&p, &st, r#"{"elapsed_hours":15,"senses":{"tokens":0.5},"needs":{"context":{"amount":0.6,"src":"env:compactor"},"contact":{"amount":0.8,"src":"human:principal"}},"src":"env:host"}"#);
        let (xc, xk) = (0.5f64, 0.6f64);
        let r_ctx = r6(drive_of(&[xc, xk]) - drive_of(&[(xc - 0.6).abs(), xk])); let after = [r6((xc - 0.6f64).abs()), xk];
        let r_con = r6(drive_of(&after) - drive_of(&[after[0], (xk - 0.8f64).abs()]));
        assert_eq!((g(&doc, &["needs", "context", "deficit"]).as_f64(), g(&doc, &["needs", "contact", "deficit"]).as_f64()), (Some(0.1), Some(0.2)));
        assert!((g(&doc, &["drives", "expect", "compact"]).as_f64().unwrap() * 2.0 - r_ctx).abs() <= 2e-6, "compact's win {r_ctx}");
        assert!((g(&doc, &["drives", "expect", "report"]).as_f64().unwrap() * 2.0 - r_con).abs() <= 2e-6, "report's win {r_con}");
        assert!(r_ctx > 0.0 && r_con > 0.0 && (r_con - r6(drive_of(&[xc, xk]) - drive_of(&[xc, (xk - 0.8f64).abs()]))).abs() > 1e-3, "report's win reads context after its pay: {r_con}");
        assert_eq!(g(&ns.to_json(&p), &["needs", "success"]).get("compact").and_then(Json::as_f64), Some(1.0));
        // a reading grows context by rate x raw, clipped to cap; per_hour 0 means hours alone do not
        let (d1, _) = ok_turn(&p, &st, r#"{"elapsed_hours":5,"senses":{"tokens":0.7},"src":"env"}"#); let (d2, _) = ok_turn(&p, &st, r#"{"elapsed_hours":30,"src":"env"}"#);
        assert_eq!((g(&d1, &["needs", "context", "deficit"]).as_f64(), g(&d1, &["needs", "contact", "deficit"]).as_f64()), (Some(0.7), Some(0.2)));
        assert_eq!((g(&d2, &["needs", "context", "deficit"]).as_f64(), g(&d2, &["needs", "contact", "deficit"]).as_f64()), (Some(0.0), Some(1.0)), "contact clipped at its cap");
        // the forecast: x + forecast_per_hour x horizon (horizon 0 here: the deficit); with horizon 2 context reads 0.12 ahead
        assert_eq!(g(&d1, &["needs", "context", "forecast"]).as_f64(), Some(0.7));
        let q = need_with(r#""forecast_per_hour":0.06"#, r#""forecast_per_hour":0.06,"horizon_hours":2"#).unwrap();
        let (d3, _) = ok_turn(&q, &init(&q, Some(3), true, &run), r#"{"elapsed_hours":5,"senses":{"tokens":0.7},"src":"env"}"#);
        assert_eq!(g(&d3, &["needs", "context", "forecast"]).as_f64(), Some(0.82));
        let t = g(&d3, &["needs", "context", "term"]).as_f64().unwrap(); assert_eq!(t, r6(2.0 * (drive_of(&[0.82, 0.2]) - drive_of(&[0.22, 0.2]))));
        // the stance's term is the pursue field's: ln(odds_compact / odds_work) = ln 0.6 + term_compact (glow 0, no other signal)
        let (o_c, o_w) = (g(&d3, &["pursue", "odds", "compact"]).as_f64().unwrap(), g(&d3, &["pursue", "odds", "work"]).as_f64().unwrap());
        assert!(((o_c / o_w).ln() - (0.6f64.ln() + t)).abs() < 1e-3, "the term enters pursue at weight 1: {o_c} / {o_w}, term {t}");
    }

    /// A.4 on the engine: after a released stance pursues `task` (window 2 h) the window counts down by the stamped hours, fires once
    /// at 0 without a win (d = -min(E, pe_cap), E <- E (1 - rate), `why` says so), and a win inside it closes it; the omitted d joins
    /// the summed surprise and drv_omit but not the learning sign; a bound need's success rate falls by (1 - rate) when its window
    /// fires and rises by rate (1 - s) on a win
    #[test]
    fn the_omission_window_opens_counts_down_fires_once_and_closes_on_a_win() {
        let p = val_with(r#""valence":{"mood":"valence","expect":0.2,"surprise":0.6}"#, r#""valence":{"mood":"valence","expect":0.2,"surprise":0.6},"learn_from_surprise":0.5,"effects":{"omission":{"tone":0.4}}"#).unwrap();
        let mut st = init(&p, Some(0), true, &run);
        for _ in 0..10 { st = ok_turn(&p, &st, r#"{"goals":{"task":{"win":1}},"go_task":true,"src":"env"}"#).1; }
        let (d10, s10) = ok_turn(&p, &st, r#"{"go_task":true,"src":"env"}"#);
        let e_open = g(&d10, &["drives", "expect", "task"]).as_f64().unwrap(); assert_eq!(g(&d10, &["drives", "window", "task"]).as_f64(), Some(2.0));
        let omission = |e: f64, case: &[(f64, Option<f64>)]| -> Vec<(f64, f64)> { let (mut e, mut w, mut out) = (e, Some(2.0f64), vec![]);
            for (h, m) in case { e *= 0.5f64.powf(h / 48.0); let mut dd = 0.0;
                if let Some(m) = m { dd = (m - e).clamp(-1.0, 1.0); e += 0.5 * (m - e); w = None; }
                else if let Some(x) = w { let x = (x - h).max(0.0); if x <= 0.0 { dd = -e.min(1.0); e *= 0.5; w = None; } else { w = Some(x); } }
                e = r6(e); out.push((e, r6(dd))); } out };
        for case in [vec![(1.0, None), (1.0, None)], vec![(3.0, None), (3.0, None)], vec![(1.0, Some(1.0)), (3.0, None)]] {
            let mut s = s10.clone(); let want = omission(e_open, &case); let mut fired = 0;
            for (k, (h, m)) in case.iter().enumerate() {
                let ev = match m { Some(m) => format!(r#"{{"elapsed_hours":{h},"goals":{{"task":{{"win":{m}}}}},"go_other":true,"src":"env"}}"#), None => format!(r#"{{"elapsed_hours":{h},"go_other":true,"src":"env"}}"#) };
                let (doc, ns) = ok_turn(&p, &s, &ev); let (e, dd) = (g(&doc, &["drives", "expect", "task"]).as_f64().unwrap(), g(&doc, &["drives", "surprise", "task"]).as_f64().unwrap());
                assert!((e - want[k].0).abs() <= 1e-6 && (dd - want[k].1).abs() <= 1e-6, "{case:?} event {k}: ({e}, {dd}) vs {:?}", want[k]);
                let why = g(&doc, &["why"]).as_str().unwrap_or("").to_string();
                if dd < 0.0 && m.is_none() { fired += 1; assert!(why.contains("an expected win did not come (task)"), "{why}");
                    // drv_omit = min(omitted E, pe_cap); the learning sign leaves the omission out, the summed surprise keeps it
                    let c = drive_ctx(p.drives.as_ref().unwrap(), ns.drives.as_ref().unwrap()); assert_eq!((c.omit, c.surprise, c.learn_sign), (r6(-dd), dd, 0.0)); }
                else { assert!(!why.contains("did not come"), "{why}"); }
                assert_eq!(g(&doc, &["drives", "window", "task"]).is_null(), dd < 0.0 || m.is_some() || g(&s.to_json(&p), &["drives", "window", "task"]).is_null(), "{case:?} event {k}");
                s = ns; }
            assert_eq!(fired, if case[0].1.is_some() { 0 } else { 1 }, "{case:?}: fires once, never after a win");
        }
        // the success rate of a bound need: x 0.8 per window fired, + 0.2 (1 - s) per win
        let q = need1(); let mut s = init(&q, Some(0), true, &run); let mut sr = vec![];
        for ev in [r#"{"elapsed_hours":3,"src":"env"}"#, r#"{"elapsed_hours":2,"src":"env"}"#, r#"{"elapsed_hours":2,"src":"env"}"#, r#"{"elapsed_hours":5,"needs":{"fuel":0.2},"src":"env"}"#] {
            let (doc, ns) = ok_turn(&q, &s, ev); s = ns; sr.push((g(&s.to_json(&q), &["needs", "success", "refuel"]).as_f64().unwrap(), g(&doc, &["pursue", "goal"]).as_str().unwrap().to_string())); }
        assert_eq!(sr[0].1, "refuel", "the need's term pursues refuel at deficit 0.3: {sr:?}");
        assert_eq!(sr.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1.0, 0.8, 0.64, r6(0.64 + 0.2 * 0.36)], "{sr:?}");
    }

    /// Design E: the push of valence is (1 - k) x (w_e x E of the goal the previous stance pursued + w_p x d) on top of k x the carry:
    /// an expected win (d 0) moves it by w_e x E (the move after 10 and 3 wins keeps E10 / E3), and an impulse fades as 0.61^j
    /// (0.0845 five events back, half-life 1.402 events); liking adds k x liking x m to afterglow; the genes stay with the seed
    #[test]
    fn valence_follows_expected_value_and_prediction_errors() {
        let p = val_with("", "").unwrap(); let push = |s: &State| { let a = &s.mood.iter().find(|(k, _)| k == "valence").unwrap().1; a[2] - a[0] };
        let mv = |wins: usize| { let mut s = init(&p, Some(0), true, &run);
            for _ in 0..wins { s = ok_turn(&p, &s, r#"{"goals":{"task":{"win":1}},"go_task":true,"src":"env"}"#).1; }
            let (doc, s1) = ok_turn(&p, &s, r#"{"go_task":true,"src":"env"}"#); let e = g(&doc, &["drives", "expect", "task"]).as_f64().unwrap();
            let (_, s2) = ok_turn(&p, &s1, &format!(r#"{{"goals":{{"task":{{"win":{e}}}}},"go_task":true,"src":"env"}}"#)); (push(&s2) - 0.61 * push(&s1), e) };
        let ((m10, e10), (m3, e3)) = (mv(10), mv(3)); assert_eq!((e10, e3), (0.999024, 0.875));
        assert!(m10 > 0.0 && ((m10 / m3) - (e10 / e3)).abs() <= 1e-4, "{m10} / {m3} vs {e10} / {e3}");
        let mut s = ok_turn(&p, &init(&p, Some(0), true, &run), r#"{"goals":{"task":{"win":1}},"go_other":true,"src":"env"}"#).1; let p0 = push(&s); let mut left = vec![];
        for _ in 0..5 { s = ok_turn(&p, &s, r#"{"go_other":true,"src":"env"}"#).1; left.push(push(&s) / p0); }
        for (j, v) in left.iter().enumerate() { assert!((v - 0.61f64.powi(j as i32 + 1)).abs() <= 1e-4, "lag {}: {v}", j + 1); }
        assert_eq!((round_to(0.61f64.powi(5), 4), round_to(0.5f64.ln() / 0.61f64.ln(), 3)), (0.0845, 1.402));
        // liking: afterglow += k (liking m + max(d, 0)); 0 is today's afterglow; the gene is exp(spread z), 4 decimals
        let l = val_with(r#""valence":"#, r#""afterglow":{"liking":0.5,"liking_spread":0.3},"valence":"#).unwrap(); let sl = init(&l, Some(5), true, &run);
        let gene = g(&sl.to_json(&l), &["drives", "genes", "liking"]).as_f64().unwrap(); assert_eq!(gene, r6(round_to((0.3 * normal(&[&l.name, "5", "liking"])).exp(), 4)));
        let (dl, _) = ok_turn(&l, &sl, r#"{"goals":{"task":{"win":0.4}},"src":"env"}"#); let (d0, _) = ok_turn(&p, &init(&p, Some(5), true, &run), r#"{"goals":{"task":{"win":0.4}},"src":"env"}"#);
        assert_eq!((g(&d0, &["drives", "glow", "task"]).as_f64(), g(&dl, &["drives", "glow", "task"]).as_f64()), (Some(0.4), Some(r6(0.5 * gene * 0.4 + 0.4))));
        assert!(g(&init(&p, Some(5), true, &run).to_json(&p), &["drives", "genes", "liking"]).is_null(), "spread 0 stores no gene");
    }

    /// The persona refusals of RFC 0002 A.5 (N-R1 to N-R6) and Design E (E-R1, E-R2): each at its path, when the persona is read
    #[test]
    fn every_needs_and_valence_row_is_refused_at_its_path() {
        let bad = |from: &str, to: &str| err(need_with(from, to));
        // N-R1: n > m > 1; a drives block; 1-8 items; distinct ids; a taken need_<id> id
        assert_eq!(bad(r#""drive":{"n":4,"m":3}"#, r#""drive":{"n":3,"m":4}"#), "needs.drive: n > m > 1 (the deprivation scaling)");
        assert_eq!(bad(r#""drive":{"n":4,"m":3}"#, r#""drive":{"n":4,"m":1}"#), "needs.drive: n > m > 1 (the deprivation scaling)");
        assert_eq!(err(build(&json::parse(&NEED1.replace(r#""drives":{"goals":[{"id":"refuel","expect_by_hours":1},{"id":"idle"}]},"#, "")).unwrap())), "needs: a needs block needs a drives block (a need's win pays one of its goals)");
        assert_eq!(bad(r#""items":["#, r#""items":[],"x":["#).split(':').next(), Some("needs.x"));
        assert_eq!(err(build(&json::parse(&NEED1.replace(r#""items":[{"id":"fuel""#, r#""items":[],"y":[{"id":"fuel""#)).unwrap())).split(':').next(), Some("needs.y"));
        let one = r#"{"id":"fuel","unit":"share","cap":1,"weight":1,"per_hour":0.1,"satisfy_from":["env"],"amount":0.2,"bind":"refuel"}"#;
        assert_eq!(err(build(&json::parse(&NEED1.replace(&format!("[{one}]"), "[]")).unwrap())), "needs.items: 1 to 8 needs");
        let nine: Vec<String> = (0..9).map(|i| one.replace(r#""id":"fuel""#, &format!(r#""id":"fuel{i}""#))).collect();
        assert_eq!(err(build(&json::parse(&NEED1.replace(&format!("[{one}]"), &format!("[{}]", nine.join(",")))).unwrap())), "needs.items: 1 to 8 needs");
        assert_eq!(bad(r#"{"id":"contact","#, r#"{"id":"context","#), "needs.items[1].id: need ids must be distinct");
        assert_eq!(bad(r#"{"id":"quiet","kind":"flag","say":"quiet hours"}"#, r#"{"id":"quiet","kind":"flag","say":"quiet hours"},{"id":"need_contact","kind":"flag"}"#), "inputs[1].id: reserved by a need (need_<id>)");
        assert_eq!(bad(r#"{"id":"contact","#, r#"{"id":"a_need_id_of_thirty_three_chars_x","#), "needs.items[1].id: at most 32 characters (a need reserves the input id need_<id>)");
        // N-R2: bind an unknown goal; two needs bound to one goal
        assert_eq!(bad(r#""bind":"report""#, r#""bind":"chat""#), "needs.items[1].bind: unknown goal (goals: work, compact, report)");
        assert_eq!(bad(r#""bind":"report""#, r#""bind":"compact""#), "needs.items[1].bind: one need per goal (another need binds it)");
        // N-R3: satisfy_from missing, empty, self or clock
        assert_eq!(bad(r#""satisfy_from":["human"],"#, ""), "needs.items[1].satisfy_from: required");
        assert_eq!(bad(r#""satisfy_from":["human"]"#, r#""satisfy_from":[]"#), "needs.items[1].satisfy_from: required: a list of source kinds: env, human");
        assert_eq!(bad(r#""satisfy_from":["human"]"#, r#""satisfy_from":["self"]"#), "needs.items[1].satisfy_from[0]: an amount never comes from the individual itself");
        assert_eq!(bad(r#""satisfy_from":["human"]"#, r#""satisfy_from":["human","clock"]"#), "needs.items[1].satisfy_from[1]: the clock is not a source of amounts: env or human");
        // N-R4: per_reading of an undeclared or flag sense; a negative per_hour or rate
        assert_eq!(bad(r#""sense":"tokens""#, r#""sense":"latency""#), "needs.items[0].per_reading.sense: not a declared sense (tokens)");
        assert_eq!(err(build(&json::parse(&NEED.replace(r#""drift_cap":0.5}]"#, r#""drift_cap":0.5},{"id":"fail","kind":"flag","unit":"event","src":["env"],"prior":{"mean":0.05,"var":0.05},"drift_cap":0.25}]"#)
            .replace(r#""sense":"tokens""#, r#""sense":"fail""#)).unwrap())), "needs.items[0].per_reading.sense: a number sense (a need grows per unit of its raw reading)");
        assert_eq!(err(build(&json::parse(&NEED1.replace(r#""per_hour":0.1,"#, r#""per_hour":0.1,"per_reading":{"sense":"tokens","rate":1},"#)).unwrap())), "needs.items[0].per_reading.sense: not a declared sense (the persona has no senses block)");
        assert_eq!(bad(r#""per_hour":0.04"#, r#""per_hour":-0.04"#), "needs.items[1].per_hour: must be >= 0");
        assert_eq!(bad(r#""rate":1}"#, r#""rate":-1}"#), "needs.items[0].per_reading.rate: must be >= 0");
        // N-R5: positive caps, weights, amounts, want, pursue.cap, r_cap; amount <= cap; horizon in [0, 72]; the success rate and start; 6 decimals
        assert_eq!(bad(r#""cap":1,"weight":1,"per_hour":0.04"#, r#""cap":0,"weight":1,"per_hour":0.04"#), "needs.items[1].cap: must be > 0");
        assert_eq!(bad(r#""weight":1,"per_hour":0.04"#, r#""weight":-1,"per_hour":0.04"#), "needs.items[1].weight: must be > 0");
        assert_eq!(bad(r#""amount":0.8"#, r#""amount":0"#), "needs.items[1].amount: must be > 0");
        assert_eq!(bad(r#""amount":0.8"#, r#""amount":1.5"#), "needs.items[1].amount: at most the need's cap");
        assert_eq!(bad(r#""want":2"#, r#""want":0"#), "needs.pursue.want: must be > 0");
        assert_eq!(bad(r#""want":2,"cap":3"#, r#""want":2,"cap":-3"#), "needs.pursue.cap: must be > 0");
        assert_eq!(bad(r#""r_cap":1"#, r#""r_cap":0"#), "needs.r_cap: must be > 0");
        assert_eq!(bad(r#""r_cap":1"#, r#""r_cap":3"#), "needs.r_cap: at most drives.expectation.win_max (2): a need's win joins the goal's wins");
        assert_eq!(bad(r#""bind":"report""#, r#""bind":"report","horizon_hours":73"#), "needs.items[1].horizon_hours: must be <= 72");
        assert_eq!(bad(r#""bind":"report""#, r#""bind":"report","horizon_hours":-1"#), "needs.items[1].horizon_hours: must be >= 0");
        assert_eq!(bad(r#""rate":0.2,"init":1"#, r#""rate":0,"init":1"#), "needs.success.rate: in (0, 1]");
        assert_eq!(bad(r#""rate":0.2,"init":1"#, r#""rate":1.5,"init":1"#), "needs.success.rate: in (0, 1]");
        assert_eq!(bad(r#""rate":0.2,"init":1"#, r#""rate":0.2,"init":1.2"#), "needs.success.init: must be <= 1");
        assert_eq!(bad(r#""amount":0.8"#, r#""amount":0.8000001"#), "needs.items[1].amount: at most 6 decimals");
        assert_eq!(bad(r#""per_hour":0.04"#, r#""per_hour":0.0400001"#), "needs.items[1].per_hour: at most 6 decimals");
        // N-R6: a bound goal without expect_by_hours; expect_by_hours <= 0
        assert_eq!(bad(r#"{"id":"report","interest":0.6,"expect_by_hours":1}"#, r#"{"id":"report","interest":0.6}"#), "needs.items[1].bind: the bound goal report needs expect_by_hours (unanswered pursuits would never extinguish)");
        assert_eq!(bad(r#"{"id":"report","interest":0.6,"expect_by_hours":1}"#, r#"{"id":"report","interest":0.6,"expect_by_hours":0}"#), "drives.goals[2].expect_by_hours: must be > 0 (hours until an expected win counts as omitted)");
        // a need condition reads need.<id>.deficit only
        assert_eq!(bad(r#""when":{"quiet":true}"#, r#""when":{"need.contact.term":0.5}"#), "habits[0].when.need.contact.term: a need condition is need.<id>.deficit");
        assert_eq!(bad(r#""when":{"quiet":true}"#, r#""when":{"need.sleep.deficit":0.5}"#), "habits[0].when.need.sleep.deficit: unknown need \"sleep\"");
        assert_eq!(bad(r#""when":{"quiet":true}"#, r#""when":{"need_contact":0.5}"#), "habits[0].when.need_contact: reserved for the needs block (a rule reads need.<id>.deficit)");
        // E-R1: valence with effects.surprise; an unknown mood. E-R2: 0 < w_e < w_p; liking >= 0; liking_spread 0..3
        let vb = |from: &str, to: &str| err(val_with(from, to));
        assert_eq!(vb(r#""valence":{"#, r#""effects":{"surprise":{"valence":0.6}},"valence":{"#), "drives.valence: drives.valence replaces drives.effects.surprise: declare one of them");
        assert_eq!(vb(r#""mood":"valence","expect""#, r#""mood":"spirit","expect""#), "drives.valence.mood: a declared mood (valence)");
        assert_eq!(vb(r#""expect":0.2"#, r#""expect":0"#), "drives.valence.expect: must be > 0");
        assert_eq!(vb(r#""surprise":0.6"#, r#""surprise":-0.6"#), "drives.valence.surprise: must be > 0");
        assert_eq!(vb(r#""expect":0.2"#, r#""expect":0.6"#), "drives.valence: expect < surprise (the expectation weight below the prediction-error weight)");
        assert_eq!(vb(r#""valence":{"#, r#""afterglow":{"liking":-0.5},"valence":{"#), "drives.afterglow.liking: must be >= 0");
        assert_eq!(vb(r#""valence":{"#, r#""afterglow":{"liking_spread":3.5},"valence":{"#), "drives.afterglow.liking_spread: must be <= 3");
        // drv_expect and drv_omit are reserved only with their blocks: a persona without them may declare and use them
        assert_eq!(vb(r#"{"id":"go_other","kind":"flag"}"#, r#"{"id":"go_other","kind":"flag"},{"id":"drv_omit","kind":"flag"}"#), "inputs[2].id: reserved for the omission let-down (a goal has expect_by_hours)");
        assert_eq!(vb(r#"{"id":"go_other","kind":"flag"}"#, r#"{"id":"go_other","kind":"flag"},{"id":"drv_expect","kind":"flag"}"#), "inputs: reserved input id drv_expect");
        let free = VAL.replace(r#","expect_by_hours":2"#, "").replace(r#","valence":{"mood":"valence","expect":0.2,"surprise":0.6}"#, "")
            .replace(r#"{"id":"go_other","kind":"flag"}"#, r#"{"id":"go_other","kind":"flag"},{"id":"drv_expect","kind":"flag"},{"id":"drv_omit","kind":"flag"}"#)
            .replace(r#""when":{"go_other":true}"#, r#""when":{"drv_omit":true,"drv_expect":true}"#);
        assert!(build(&json::parse(&free).unwrap()).is_ok(), "{}", err(build(&json::parse(&free).unwrap())));
    }

    /// The event refusals of RFC 0002 A.5 (N-E1 to N-E4): each refuses the event whole at its path; a refused event changes
    /// nothing (the run equals the run without it, stance and state bytes)
    #[test]
    fn every_needs_event_row_refuses_the_event_whole() {
        let p = need2(); let st0 = init(&p, Some(7), true, &run); let t = |st: &State, ev: &str| err(turn(&p, st, &evj(ev), false, &run, false));
        let st = ok_turn(&p, &st0, r#"{"elapsed_hours":6,"senses":{"tokens":0.4},"src":"env"}"#).1;
        let rows: Vec<(&str, &str, String)> = vec![
            ("N-E1 undeclared", r#"{"needs":{"sleep":0.2},"src":"env"}"#, "inputs.needs.sleep: not a declared need (needs: context, contact)".into()),
            ("N-E1 zero", r#"{"needs":{"context":0},"src":"env"}"#, "inputs.needs.context: an amount: a number > 0 and at most the need's cap (1)".into()),
            ("N-E1 above cap", r#"{"needs":{"context":1.5},"src":"env"}"#, "inputs.needs.context: an amount: a number > 0 and at most the need's cap (1)".into()),
            ("N-E1 not a number", r#"{"needs":{"context":"0.5"},"src":"env"}"#, "inputs.needs.context: an amount: a number > 0 and at most the need's cap (1)".into()),
            ("N-E1 twice", r#"{"needs":{"context":0.2,"context":0.3},"src":"env"}"#, "inputs.needs.context: a need satisfied twice in one event".into()),
            ("N-E2 missing", r#"{"needs":{"context":0.2}}"#, "inputs.needs.context.src: an amount for context needs a source: its own src or the event's (env)".into()),
            ("N-E2 self", r#"{"needs":{"context":{"amount":0.2,"src":"self"}},"src":"env"}"#, "inputs.needs.context.src: an amount for context from the individual itself (src self) is refused: amounts come from env".into()),
            ("N-E2 clock", r#"{"needs":{"context":0.2},"src":"clock"}"#, "inputs.needs.context.src: an amount for context from the clock is refused: amounts come from env".into()),
            ("N-E2 kind", r#"{"needs":{"contact":0.2},"src":"env"}"#, "inputs.needs.contact.src: an amount for contact from env is refused: contact is satisfied from human".into()),
            ("N-E2 malformed", r#"{"needs":{"context":{"amount":0.2,"src":"env:"}},"src":"env"}"#, "inputs.needs.context.src: a source: human[:id] | env[:id] (id: 1-64 of A-Z a-z 0-9 _ . - @ / :)".into()),
            ("N-E3", r#"{"needs":{"context":0.2},"goals":{"compact":{"win":1}},"src":"env"}"#, "inputs.needs.context: an amount for context and a win of its goal compact in one event (one outcome, one payment)".into()),
            ("N-E4 term", r#"{"need_context":1,"src":"env"}"#, "inputs.need_context: the engine computes a need's term from its deficit (N-E4)".into()),
            ("N-E4 omit", r#"{"drv_omit":0.5,"src":"env"}"#, "inputs.drv_omit: the engine computes the omission let-down (N-E4)".into()),
            ("N-E4 deficit", r#"{"needs":{"context":{"amount":0.2,"deficit":0.1,"src":"env"}}}"#, "inputs.needs.context.deficit: the engine owns the deficit (N-E4)".into()),
            ("N-E4 shape", r#"{"needs":[0.2],"src":"env"}"#, "inputs.needs: a mapping need id -> amount (a number, or {amount, src})".into())];
        let tail = [r#"{"elapsed_hours":1,"needs":{"context":0.6},"src":"env"}"#, r#"{"elapsed_hours":2,"needs":{"contact":{"amount":0.8,"src":"human:a"}},"src":"env"}"#, r#"{"elapsed_hours":1,"senses":{"tokens":0.9},"src":"env"}"#];
        let run_of = |st: &State, extra: Option<&str>| { let mut s = st.clone(); let mut out = String::new();
            for ev in extra.into_iter().chain(tail) { match turn(&p, &s, &evj(ev), false, &run, false) { Ok((doc, ns)) => { out.push_str(&canon(&doc)); s = ns; } Err(_) => out.push_str("refused") } }
            (out.replacen("refused", "", 1), canon(&s.to_json(&p))) };
        let without = run_of(&st, None);
        for (row, ev, want) in &rows { assert_eq!(&t(&st, ev), want, "{row}"); assert_eq!(run_of(&st, Some(ev)), without, "{row}: the run equals the run without it"); }
        // accepted: a bare amount takes the event's src, an amount its own; the inputs list them; MCP's channel is refused apart
        let (doc, _) = ok_turn(&p, &st, r#"{"needs":{"context":0.6,"contact":{"amount":0.8,"src":"human:a"}},"src":"env:host"}"#);
        assert_eq!(canon(g(&doc, &["inputs", "needs"])), r#"{"contact":{"amount":0.8,"src":"human:a"},"context":0.6}"#); assert!(g(&doc, &["ignored"]).as_arr().unwrap().is_empty());
        let args = |k: &str, ev: &str| vec![("persona".to_string(), json::parse(NEED).unwrap()), (k.to_string(), evj(ev))];
        assert_eq!(err(mcp_refuses_amounts(&args("inputs", r#"{"needs":{"context":0.2},"src":"env"}"#), "inputs")).split(':').next(), Some("arguments.inputs.needs"));
        assert_eq!(err(mcp_refuses_amounts(&args("event", r#"{"needs":{"context":0.2},"src":"env"}"#), "event")).split(':').next(), Some("arguments.event.needs"));
        assert!(mcp_refuses_amounts(&args("event", r#"{"senses":{"tokens":0.2},"src":"env"}"#), "event").is_ok());
        // without the block `needs` is an undeclared input, listed in ignored
        let (d1, _) = ok_turn(&adv(), &init(&adv(), Some(0), true, &run), r#"{"needs":{"context":0.2}}"#); assert_eq!(canon(g(&d1, &["ignored"])), r#"["needs"]"#);
    }

    /// A state is refused, even with a recomputed digest, when a deficit leaves [0, cap], a success rate leaves [0, 1], a window
    /// leaves [0, expect_by_hours] or null, the need set differs from the persona's, or a gene misses its seed (RFC 0002 A.5)
    #[test]
    fn a_needs_state_out_of_its_box_is_refused() {
        let p = need_with(r#""bind":"report""#, r#""bind":"report","weight_spread":0.4,"growth_spread":0.2"#).unwrap(); let st = init(&p, Some(4), true, &run);
        assert!(State::read(&p, &st.to_json(&p)).is_ok());
        let set = |f: &dyn Fn(&mut State)| { let mut s = st.clone(); f(&mut s); s.seal(&p); err(State::read(&p, &s.to_json(&p))) };
        fn nd(s: &mut State) -> &mut NState { s.drives.as_mut().unwrap().nd.as_mut().unwrap() }
        assert_eq!(set(&|s| nd(s).deficit[1] = 1.000001), "state.needs.deficit.contact: a deficit in [0, 1] (cap)");
        assert_eq!(set(&|s| nd(s).deficit[0] = -0.000001), "state.needs.deficit.context: a deficit in [0, 1] (cap)");
        assert_eq!(set(&|s| nd(s).success[0] = 1.000001), "state.needs.success.compact: a success rate in [0, 1]");
        assert_eq!(set(&|s| nd(s).success[1] = -0.5), "state.needs.success.report: a success rate in [0, 1]");
        assert_eq!(set(&|s| s.drives.as_mut().unwrap().window[1] = Some(1.000001)), "state.drives.window.compact: hours left in [0, 1] (expect_by_hours) or null");
        assert_eq!(set(&|s| s.drives.as_mut().unwrap().window[2] = Some(-1.0)), "state.drives.window.report: hours left in [0, 1] (expect_by_hours) or null");
        assert!(set(&|s| s.drives.as_mut().unwrap().window[2] = Some(0.5)).is_empty(), "inside the box");
        assert_eq!(set(&|s| nd(s).genes = json::parse(r#"{"weight":{},"growth":{}}"#).unwrap()), "state.needs.genes: need genes do not match the seed");
        // an edited document with a recomputed digest
        let edit = |f: &dyn Fn(&mut Json)| { let mut j = st.to_json(&p); f(&mut j);
            if let Json::Obj(o) = &mut j { let body = Json::Obj(o.iter().filter(|(k, _)| k != "digest").cloned().collect()); o.iter_mut().find(|(k, _)| k == "digest").unwrap().1 = Json::Str(sha(&body)); }
            err(State::read(&p, &j)) };
        let obj = |j: &mut Json, k: &str| -> Vec<(String, Json)> { j.get(k).and_then(Json::as_obj).unwrap().to_vec() };
        let put = |j: &mut Json, k: &str, v: Json| { if let Json::Obj(o) = j { o.iter_mut().find(|(x, _)| x == k).unwrap().1 = v; } };
        assert_eq!(edit(&|j| { let mut n = obj(j, "needs"); n[1].1 = json::parse(r#"{"context":0}"#).unwrap(); put(j, "needs", Json::Obj(n)); }), "state.needs.deficit: one deficit per need of the persona (context, contact)");
        assert_eq!(edit(&|j| { let mut n = obj(j, "needs"); n[2].1 = json::parse(r#"{"compact":1,"work":1}"#).unwrap(); put(j, "needs", Json::Obj(n)); }), "state.needs.success: one success rate per bound goal (compact, report)");
        assert_eq!(edit(&|j| { if let Json::Obj(o) = j { o.retain(|(k, _)| k != "needs"); } }), "state.needs: required with a needs block");
        assert_eq!(edit(&|j| { let mut d = obj(j, "drives"); d.retain(|(k, _)| k != "window"); put(j, "drives", Json::Obj(d)); }), "state.drives.window: required");
        // a persona without the block refuses the state field; the genes survive an unrelated edit (another need, another goal)
        let q = need1(); let sq = init(&q, Some(0), true, &run); let mut j = sq.to_json(&q);
        if let Json::Obj(o) = &mut j { o.retain(|(k, _)| k != "needs"); let body = Json::Obj(o.iter().filter(|(k, _)| k != "digest").cloned().collect()); o.iter_mut().find(|(k, _)| k == "digest").unwrap().1 = Json::Str(sha(&body)); }
        assert_eq!(err(State::read(&q, &j)), "state.needs: required with a needs block");
        let (a, _) = (adv(), 0); let mut ja = init(&a, Some(0), true, &run).to_json(&a);
        if let Json::Obj(o) = &mut ja { let at = o.len() - 1; o.insert(at, ("needs".into(), json::parse(r#"{"genes":{},"deficit":{},"success":{}}"#).unwrap()));
            let body = Json::Obj(o.iter().filter(|(k, _)| k != "digest").cloned().collect()); o.iter_mut().find(|(k, _)| k == "digest").unwrap().1 = Json::Str(sha(&body)); }
        assert_eq!(err(State::read(&a, &ja)), "state.needs: this persona has no needs block");
        let other = need_with(r#""bind":"report""#, r#""bind":"report","weight_spread":0.4,"growth_spread":0.2,"horizon_hours":3"#).unwrap()
            .clone(); let edited = build(&json::parse(&NEED.replacen(r#""bind":"report""#, r#""bind":"report","weight_spread":0.4,"growth_spread":0.2"#, 1).replace(r#"{"id":"work","interest":1.0}"#, r#"{"id":"work","interest":0.9},{"id":"chat"}"#)).unwrap()).unwrap();
        for q in [&other, &edited] { assert_eq!(canon(g(&init(q, Some(4), true, &run).to_json(q), &["needs", "genes"])), canon(g(&st.to_json(&p), &["needs", "genes"])), "genes are a function of name, seed and id"); }
        let gw = g(&st.to_json(&p), &["needs", "genes", "weight", "contact"]).as_f64().unwrap(); assert_eq!(gw, r6(round_to((0.4 * normal(&[&p.name, "4", "need", "contact", "weight"])).exp(), 4)));
        assert!(g(&st.to_json(&p), &["needs", "genes", "weight", "context"]).is_null(), "spread 0 stores no gene");
    }
}
