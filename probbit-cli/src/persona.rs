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
fn r6(x: f64) -> f64 { let v = round_to(x, 6); if v == 0.0 { 0.0 } else { v } }
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
struct Input { id: String, kind: Kind, react: f64, say: Json, levels: Vec<String>, default: Json, max: f64, effects: Eff, by_level: Vec<(String, Eff)> }
#[derive(Clone)]
struct Hist { id: String, of: String, streak: bool, window: usize, cap: usize, say: String, effects: Eff }
#[derive(Clone)]
struct Habit { id: String, when: Vec<(String, Json)>, then: Vec<(String, Json)>, rules: Vec<(String, Vec<Json>)>, say: String, priority: f64 }
/// engine settings (docs/persona.md §2.7)
#[derive(Clone)]
struct Eng { op: String, sweeps: usize, polish_sweeps: usize, exact_limit: u64, chains: usize, twin: bool, positional: bool, yields: bool }
#[derive(Clone)]
pub struct Persona { pub name: String, pub version: String, pub seed: u64, pub digest: String, vars: Vec<Var>, steps: Vec<String>, slots: Vec<String>, say_order: bool,
    prefer: Eff, couplings: Vec<(String, String, Vec<Vec<f64>>)>, inputs: Vec<Input>, history: Vec<Hist>, habits: Vec<Habit>, eng: Eng, max_tokens: usize, order: Vec<String>, prefix: String }
impl Persona {
    fn var(&self, id: &str) -> Option<&Var> { self.vars.iter().find(|v| v.id == id) }
    fn input(&self, id: &str) -> Option<&Input> { self.inputs.iter().find(|x| x.id == id) }
    fn habit(&self, id: &str) -> &Habit { self.habits.iter().find(|h| h.id == id).expect("habit id") }
    fn habit_index(&self, id: &str) -> usize { self.habits.iter().position(|h| h.id == id).unwrap_or(0) }
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
    const TOP: [&str; 12] = ["probbit_persona", "identity", "traits", "moods", "couplings", "inputs", "history", "habits", "agenda", "engine", "line", "comment"];
    let kv = keys(doc, "", &TOP, &["probbit_persona", "identity", "traits"])?;
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
        let mut inp = Input { id, kind, react, say, levels: vec![], default: Json::Null, max: 1.0, effects: vec![], by_level: vec![] };
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
    let lkv: &[(String, Json)] = match get_t(kv, "line") { Some(l) => keys(l, "line", &["max_tokens", "order", "prefix", "comment"], &[])?, None => &[] };
    let order = match get_t(lkv, "order") { None => vars.iter().filter(|v| !v.mood).map(|v| v.id.clone()).collect(), Some(o) => strs(o).ok_or_else(|| perr("line.order", "a list of trait ids"))? };
    for k in &order { need(vars.iter().any(|v| v.id == *k), "line.order", &format!("unknown trait or mood {k:?}"))?; }
    let max_tokens = num_or(lkv, "max_tokens", 40.0, "line", Some(8.0), Some(400.0), true)? as usize;
    let prefix = match get(lkv, "prefix") { None => "Stance: ".to_string(), Some(Json::Str(s)) => s.clone(), Some(_) => return Err(perr("line.prefix", "a string")) };
    let p = Persona { name, version, seed, digest, vars, steps, slots, say_order, prefer, couplings, inputs, history, habits, eng, max_tokens, order, prefix };
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
    agenda: Option<Vec<String>>, rest: Option<Vec<(String, String)>>, pub digest: String }
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
        Json::Obj(v)
    }
    pub fn to_json(&self, p: &Persona) -> Json { let mut b = self.body(p); if let Json::Obj(v) = &mut b { v.push(("digest".into(), Json::Str(self.digest.clone()))); } b }
    fn seal(&mut self, p: &Persona) { self.digest = sha(&self.body(p)); }
    /// A state document -> State, for this persona: the format, the persona digest, the state's own digest and the genes are
    /// checked (in that order), then every field's type (a well-formed state never fails those).
    pub fn read(p: &Persona, j: &Json) -> R<State> {
        let kv = j.as_obj().ok_or_else(|| perr("state", "must be a JSON object"))?;
        need(matches!(get(kv, "probbit_persona_state"), Some(Json::Num(x)) if *x == 1.0), "state", "not a probbit persona state (format 1)")?;
        let pd = get(kv, "persona").and_then(|x| x.get("digest")).and_then(Json::as_str).ok_or_else(|| perr("state.persona.digest", "missing"))?;
        if pd != p.digest { return Err(perr("state.persona.digest", format!("the persona file changed since this state was made ({} != {}); re-init", &pd[..pd.len().min(19)], &p.digest[..19]))); }
        let body = Json::Obj(kv.iter().filter(|(k, _)| k != "digest").cloned().collect());
        need(get(kv, "digest").and_then(Json::as_str) == Some(sha(&body).as_str()), "state.digest", "state was edited or corrupted")?;
        keys(j, "state", &["probbit_persona_state", "persona", "seed", "turn", "genes", "mood", "history", "prev", "rest", "digest"], &["persona", "seed", "turn", "genes", "mood", "history", "prev", "digest"])?;
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
        Ok(State { seed, turn, persona, genes: g.clone(), mood, history, prev, agenda, rest, digest: get(kv, "digest").and_then(Json::as_str).unwrap_or("").to_string() })
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
struct Meta { vals: Vec<(String, Json)>, hist: Vec<(String, f64)>, ignored: Vec<String>, active: Vec<String>, conditional: Vec<String>, active_inputs: Vec<String>,
    rules: Vec<Vec<Json>>, owners: Vec<String>, new_mood: Vec<(String, Vec<f64>)>, contrib: Vec<Vec<(String, Vec<f64>)>>, field: Vec<Vec<f64>>, values: Vec<String>, twin: bool }
#[derive(Default)]
struct Opts { no_inertia: bool, clamps: Vec<(String, String)>, twin: Option<bool>, exclude: Vec<String> }

/// Inputs -> (values per declared input, history features, elapsed hours, ignored ids); a bad value is a persona error
#[allow(clippy::type_complexity)]
fn resolve_inputs(p: &Persona, st: &State, raw: &[(String, Json)]) -> R<(Vec<(String, Json)>, Vec<(String, f64)>, f64, Vec<String>)> {
    let mut ignored: Vec<String> = raw.iter().map(|(k, _)| k.clone()).filter(|k| p.input(k).is_none() && !RESERVED_INPUTS.contains(&k.as_str())).collect(); ignored.sort();
    let mut vals = vec![];
    for x in &p.inputs { let path = format!("inputs.{}", x.id); let v = get(raw, &x.id).cloned().unwrap_or_else(|| x.default.clone());
        let v = match x.kind {
            Kind::Flag => { need(matches!(v, Json::Bool(_)), &path, "true | false")?; v }
            Kind::Level => { need(v.as_str().is_some_and(|s| x.levels.iter().any(|l| l == s)), &path, &format!("one of {}", x.levels.join(", ")))?; v }
            Kind::Number => Json::Num(num(Some(&v), &path, Some(0.0), Some(x.max), false)?) };
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
    let (vals, hist, elapsed, ignored) = resolve_inputs(p, st, raw)?;
    let (nv, ns) = (p.vars.len(), p.steps.len());
    let tix = |t: &str| -> usize { match t.strip_prefix("step.") { Some(s) => nv + p.steps.iter().position(|x| x == s).unwrap(), None => p.vars.iter().position(|v| v.id == t).unwrap() } };
    let mut contrib: Vec<Vec<(String, Vec<f64>)>> = vec![vec![]; nv + ns];
    let mut mood_ev: Vec<Option<Vec<f64>>> = p.vars.iter().map(|v| v.mood.then(|| vec![0.0; v.levels.len()])).collect();
    for (i, v) in p.vars.iter().enumerate() { contrib[i].push(("prior".into(), v.base.clone()));
        if let Some(g) = st.shift(&v.id) { contrib[i].push(("genes".into(), centered(v.levels.len()).iter().map(|c| g * c).collect())); } }
    for (j, s) in p.steps.iter().enumerate() { contrib[nv + j].push(("prior".into(), vec![0.0; ns]));
        if let Some((_, pf)) = p.prefer.iter().find(|(t, _)| *t == format!("step.{s}")) { contrib[nv + j].push(("prefer".into(), pf.clone())); } }
    let mut active_inputs = vec![];
    let mut push = |t: &str, src: String, w: Vec<f64>, contrib: &mut Vec<Vec<(String, Vec<f64>)>>| { let i = tix(t);
        match mood_ev.get_mut(i).and_then(Option::as_mut) { Some(ev) => for (a, b) in ev.iter_mut().zip(&w) { *a += b; }, None => contrib[i].push((src, w)) } };
    for x in &p.inputs { let v = get(&vals, &x.id).unwrap(); let g = st.react(&x.id).unwrap_or(1.0);
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
    let field: Vec<Vec<f64>> = (0..nv + ns).map(|i| { let n = if i < nv { p.vars[i].levels.len() } else { ns };
        let mut tot = vec![0.0; n]; for (_, vec) in &contrib[i] { for (a, b) in tot.iter_mut().zip(vec) { *a += b; } } tot.into_iter().map(r6).collect() }).collect();
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
    let meta = Meta { vals, hist, ignored, active: active.iter().map(|h| h.id.clone()).collect(), conditional, active_inputs, rules, owners, new_mood, contrib, field, values: sem, twin: use_twin };
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
    line: String, why: String, agenda: Option<Vec<String>>, state_digest: String, held: Vec<String>, timing: Option<Json> }
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
fn direction_word(p: &Persona, var: &str, vec: &[f64]) -> Option<(String, f64)> {
    let v = p.var(var)?; let d = vec.iter().zip(centered(vec.len())).fold(0.0, |a, (x, c)| a + x * c);
    (d.abs() >= 0.15).then(|| (format!("{var} {}", if d > 0.0 { v.levels.last().unwrap() } else { &v.levels[0] }), d.abs()))
}
fn why(p: &Persona, meta: &Meta, bound: &[String], status: &str) -> String {
    let mut parts: Vec<String> = p.habits.iter().filter(|h| bound.contains(&h.id)).map(|h| format!("{} (habit)", if h.say.is_empty() { h.id.replace('_', " ") } else { h.say.clone() })).collect();
    let mut srcs: Vec<(f64, String)> = vec![];
    let ids: Vec<&String> = p.inputs.iter().map(|x| &x.id).chain(p.history.iter().map(|h| &h.id)).collect();
    for id in ids { if !meta.active_inputs.contains(id) { continue; }
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
    let w = why(p, meta, &bound, &status);
    // the new state
    let mut ns = st.clone(); ns.turn = st.turn + 1; ns.mood = meta.new_mood.clone();
    let mut seen: Vec<&str> = vec![];
    for h in &p.history { if seen.contains(&h.of.as_str()) { continue; } seen.push(&h.of); // one entry per flag and turn, however many features read it
        let keep = p.history.iter().filter(|x| x.of == h.of).map(|x| x.window.max(x.cap)).max().unwrap_or(1).max(1);
        let mut seq = st.history_of(&h.of).to_vec(); seq.push(matches!(get(&meta.vals, &h.of), Some(Json::Bool(true))));
        let seq = seq[seq.len().saturating_sub(keep)..].to_vec();
        match ns.history.iter_mut().find(|(k, _)| *k == h.of) { Some(x) => x.1 = seq, None => ns.history.push((h.of.clone(), seq)) } }
    ns.prev = flat.iter().filter(|(k, _)| !k.starts_with("step.")).map(|(k, v)| (k.clone(), v.clone())).collect();
    ns.agenda = (!p.steps.is_empty()).then(|| order_steps.clone()); ns.seal(p);
    let out = Out { status, verdict: verdict.clone(), tier: doc.get("tier").cloned().unwrap_or(Json::Null), program: program.to_string(), stance, mood, active: meta.active.clone(), bound,
        violations: viol, conflict: conflict.to_vec(), yielded: yielded.to_vec(), unsure, escalate, inputs: meta.vals.iter().cloned().chain(meta.hist.iter().map(|(k, x)| (k.clone(), Json::Num(*x)))).collect(),
        ignored: meta.ignored.clone(), line: ln, why: w, agenda: (!p.steps.is_empty()).then_some(order_steps), state_digest: ns.digest.clone(), held: vec![], timing: None };
    (out, ns)
}

// ------------------------------------------------------------------------------------------------------------ turns
fn solve(p: &Persona, st: &State, prog: &Json, eng: Engine, calls: &mut usize) -> Json { *calls += 1; eng(prog, &flags(p, st)) }
fn ms(t: Instant) -> f64 { t.elapsed().as_secs_f64() * 1e3 }
/// A minimal set of active habits that cannot hold together (deletion filter: drop each habit in turn, keep it only if the rest
/// becomes feasible without it); [] when the rules without any habit are already infeasible
fn habit_conflict(p: &Persona, st: &State, raw: &[(String, Json)], active: &[String], o: &Opts, eng: Engine, calls: &mut usize) -> R<Vec<String>> {
    let mut feasible = |ex: &[String]| -> R<bool> { let mut oo = Opts { no_inertia: o.no_inertia, twin: Some(false), ..Default::default() }; oo.exclude = o.exclude.iter().chain(ex).cloned().collect();
        let (prog, _) = compile(p, st, raw, &oo)?; Ok(solve(p, st, &prog, eng, calls).get("verdict").and_then(Json::as_str) != Some("infeasible")) };
    if !feasible(active)? { return Ok(vec![]); }
    let mut core: Vec<String> = active.to_vec();
    for h in active { let trial: Vec<String> = core.iter().filter(|x| *x != h).cloned().collect();
        let ex: Vec<String> = active.iter().filter(|x| !trial.contains(x)).cloned().collect(); if !feasible(&ex)? { core = trial; } }
    Ok(core)
}
/// One persona turn: (stance document, new state). A pure function of (persona, state, inputs, engine version). `timing` adds a
/// non-canonical `timing` object (compile / engine / decode ms, engine calls, the 1-minute load average).
pub fn turn(p: &Persona, st: &State, raw: &Json, no_inertia: bool, eng: Engine, timing: bool) -> R<(Json, State)> {
    let raw: Vec<(String, Json)> = match raw { Json::Null => vec![], Json::Obj(v) => v.clone(), _ => return Err(perr("inputs", "must be a JSON object")) };
    for (n, (k, _)) in raw.iter().enumerate() { if raw[..n].iter().any(|(k2, _)| k2 == k) { return Err(perr(&format!("inputs.{k}"), "duplicate input")); } }
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
    let (mut out, mut ns) = decode(p, st, &ptext, &meta, &doc, conflict.as_deref(), yielded.as_deref());
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
    out.held = held;
    if timing { let r3 = |x: f64| Json::Num((x * 1e3).round() / 1e3);
        out.timing = Some(Json::Obj(vec![("compile_ms".into(), r3(compile_ms)), ("engine_ms".into(), r3(engine_ms)), ("decode_ms".into(), r3((ms(t0) - compile_ms - engine_ms).max(0.0))), ("turn_ms".into(), r3(ms(t0))),
            ("engine_calls".into(), Json::Num(calls as f64)), ("holds".into(), Json::Num(out.held.len() as f64)), ("load_avg_1m".into(), crate::sys::loadavg().map_or(Json::Null, |l| Json::Num((l * 100.0).round() / 100.0)))])); }
    Ok((out.to_json(p, st), ns))
}

/// A new individual: genes from the seed, an empty mood and history, the fallback levels as `prev`, and (with `rest`) the
/// resting stance: this individual's joint plan with no live evidence and a neutral mood (one engine call).
pub fn init(p: &Persona, seed: Option<u64>, rest: bool, eng: Engine) -> State {
    let seed = seed.unwrap_or(p.seed);
    let mut st = State { seed, turn: 0, persona: [p.name.clone(), p.version.clone(), p.digest.clone()], genes: genes(p, seed),
        mood: p.vars.iter().filter(|v| v.mood).map(|v| (v.id.clone(), vec![0.0; v.levels.len()])).collect(),
        history: { let mut h: Vec<(String, Vec<bool>)> = vec![]; for x in &p.history { if !h.iter().any(|(k, _)| *k == x.of) { h.push((x.of.clone(), vec![])); } } h },
        prev: p.vars.iter().map(|v| (v.id.clone(), v.fallback.clone())).collect(), agenda: (!p.steps.is_empty()).then(|| p.steps.clone()), rest: None, digest: String::new() };
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
/// `init` at `seed`, then every turn of the script -> (the stance documents, the final state)
pub fn replay(p: &Persona, seed: Option<u64>, turns: &[Json], no_inertia: bool, eng: Engine, timing: bool) -> R<(Vec<Json>, State)> {
    let mut st = init(p, seed, true, eng); let mut out = vec![];
    for (i, t) in turns.iter().enumerate() { let (o, ns) = turn(p, &st, t, no_inertia, eng, timing).map_err(|e| perr(&format!("script.turns[{i}].{}", e.path), e.msg))?; out.push(o); st = ns; }
    Ok((out, st))
}
/// `compile`: the turn's program (insertion order; its sha256 is the stance's `engine.program`)
pub fn program(p: &Persona, st: &State, raw: &Json) -> R<String> {
    let raw: Vec<(String, Json)> = match raw { Json::Null => vec![], Json::Obj(v) => v.clone(), _ => return Err(perr("inputs", "must be a JSON object")) };
    Ok(text(&compile(p, st, &raw, &Opts::default())?.0))
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

/// Total-variation distance between two odds objects over the same levels (None if their levels differ)
fn tv(oa: &[(String, Json)], ob: &[(String, Json)]) -> Option<f64> {
    let mut ka: Vec<&String> = oa.iter().map(|(k, _)| k).collect(); let mut kb: Vec<&String> = ob.iter().map(|(k, _)| k).collect(); ka.sort(); kb.sort();
    (ka == kb).then(|| 0.5 * oa.iter().fold(0.0, |s, (k, p)| s + (p.as_f64().unwrap_or(0.0) - ob.iter().find(|(l, _)| l == k).and_then(|(_, q)| q.as_f64()).unwrap_or(0.0)).abs()))
}
/// Mean total-variation distance between two individuals' per-trait odds over turns and shared traits, and the fraction of
/// (turn, trait) where their levels differ (0 / 0 for identical individuals)
pub fn distance(a: &[Json], b: &[Json]) -> Json {
    let (mut tvs, mut dis): (Vec<f64>, Vec<f64>) = (vec![], vec![]); let mut per: Vec<(String, Vec<f64>)> = vec![];
    for (x, y) in a.iter().zip(b) { let (Some(sa), Some(sb)) = (x.get("stance").and_then(Json::as_obj), y.get("stance")) else { continue };
        for (t, ea) in sa { let Some(eb) = sb.get(t) else { continue }; let (oa, ob) = (ea.get("odds").and_then(Json::as_obj).unwrap_or(&[]), eb.get("odds").and_then(Json::as_obj).unwrap_or(&[]));
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
    let tvsum = |i: usize| ta[i].get("stance").and_then(Json::as_obj).map_or(0.0, |s| s.iter().filter_map(|(t, ea)| {
        let eb = tb[i].get("stance")?.get(t)?; tv(ea.get("odds")?.as_obj()?, eb.get("odds")?.as_obj()?) }).fold(0.0, |a, b| a + b));
    let worst = argmax(ta.len().min(tb.len()), tvsum);
    let who = |p: &Persona, s: u64| Json::Obj(vec![("persona".into(), Json::Str(p.name.clone())), ("seed".into(), Json::Num(s as f64)), ("genes".into(), genes(p, s))]);
    let line_of = |t: &[Json]| t.get(worst).and_then(|x| x.get("line")).cloned().unwrap_or(Json::Null);
    Ok(Json::Obj(vec![("a".into(), who(p, sa)), ("b".into(), who(q, sb)), ("turns".into(), Json::Num(turns.len() as f64)), ("distance".into(), distance(&ta, &tb)),
        ("most_different_turn".into(), Json::Num(worst as f64)), ("line_a".into(), line_of(&ta)), ("line_b".into(), line_of(&tb))]))
}
/// `describe`: the persona's traits, moods, inputs, habits and agenda
pub fn describe(p: &Persona) -> Json {
    let s = |x: &str| Json::Str(x.to_string());
    Json::Obj(vec![("name".into(), s(&p.name)), ("version".into(), s(&p.version)), ("digest".into(), s(&p.digest)), ("seed".into(), Json::Num(p.seed as f64)),
        ("traits".into(), Json::Obj(p.vars.iter().filter(|v| !v.mood).map(|v| (v.id.clone(), Json::Obj(vec![("levels".into(), jstrs(&v.levels)), ("say".into(), jstrs(&v.say))]))).collect())),
        ("moods".into(), Json::Obj(p.vars.iter().filter(|v| v.mood).map(|v| (v.id.clone(), jstrs(&v.levels))).collect())),
        ("inputs".into(), Json::Obj(p.inputs.iter().map(|x| (x.id.clone(), Json::Obj(match x.kind { Kind::Level => vec![("kind".into(), s("level")), ("levels".into(), jstrs(&x.levels))],
            Kind::Flag => vec![("kind".into(), s("flag"))], Kind::Number => vec![("kind".into(), s("number"))] }))).collect())),
        ("habits".into(), Json::Arr(p.habits.iter().map(|h| s(&h.id)).collect())), ("agenda".into(), jstrs(&p.steps))])
}
/// `check`: valid, with its digest and sizes (one engine call: the resting stance)
pub fn check(p: &Persona, eng: Engine) -> R<Json> {
    let st = init(p, None, true, eng); let (prog, _) = compile(p, &st, &[], &Opts::default())?;
    let n = |x: usize| Json::Num(x as f64); let len = |k: &str| prog.get(k).and_then(Json::as_arr).map_or(0, <[Json]>::len);
    Ok(Json::Obj(vec![("ok".into(), Json::Bool(true)), ("name".into(), Json::Str(p.name.clone())), ("version".into(), Json::Str(p.version.clone())), ("digest".into(), Json::Str(p.digest.clone())),
        ("traits".into(), n(p.vars.iter().filter(|v| !v.mood).count())), ("moods".into(), n(p.vars.iter().filter(|v| v.mood).count())), ("inputs".into(), n(p.inputs.len())),
        ("history".into(), n(p.history.len())), ("habits".into(), n(p.habits.len())), ("steps".into(), n(p.steps.len())), ("program_vars".into(), n(len("vars"))), ("program_values".into(), n(len("values")))]))
}
/// `explain`: a turn in words: per trait and mood every contribution to its field, the joint odds, the habit-free twin's level
/// where it differs, the program's size and digest, the line and the why
pub fn explain(p: &Persona, st: &State, raw: &Json, eng: Engine) -> R<String> {
    let rawv: Vec<(String, Json)> = match raw { Json::Null => vec![], Json::Obj(v) => v.clone(), _ => return Err(perr("inputs", "must be a JSON object")) };
    let (prog, meta) = compile(p, st, &rawv, &Opts::default())?; let doc = eng(&prog, &flags(p, st)); let (out, _) = turn(p, st, raw, false, eng, false)?;
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
        let e = g(if v.mood { "mood" } else { "stance" }).get(&v.id).cloned().unwrap_or(Json::Null); let lvl = e.get("level").map_or(String::new(), s);
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
        let habit = habit_spec(&Json::Obj(full), &rp, &p.inputs, &p.history, &p.vars, &p.steps)?;
        need(!out.iter().any(|q| q.id == habit.id), &at(&rp, "id"), "property ids must be distinct")?;
        let rule = Json::Obj(kv.iter().filter(|(k, _)| k != "id" && k != "comment" && k != "say" && k != "priority").cloned().collect());
        out.push(Prop { id: habit.id.clone(), rule, habit });
    }
    Ok(out)
}
/// Does this turn's stance break the property? `before` is the state the turn ran on (its `prev` levels resolve `prev`
/// restrictions), `doc` the turn's stance document. None: it holds, it is not in force, or the turn has no vouched stance (status
/// refused or fallback: the host then uses the habits-only line; unreleased traits of a partial turn are not checked).
pub fn breaks(p: &Persona, pr: &Prop, before: &State, doc: &Json) -> Option<Vec<Broken>> {
    let status = doc.get("status").and_then(Json::as_str)?; if status != "ok" && status != "partial" { return None; }
    let inputs = doc.get("inputs").and_then(Json::as_obj)?;
    if !pr.habit.when.iter().all(|(k, c)| cond_true(p, k, c, inputs, &[])) { return None; }
    let entry = |id: &str| doc.get("stance").and_then(|s| s.get(id)).or_else(|| doc.get("mood").and_then(|m| m.get(id)));
    let mut out = vec![];
    for (k, r) in &pr.habit.then {
        let v = p.var(k)?; let e = entry(k)?; if e.get("released") != Some(&Json::Bool(true)) { continue; }
        let lvl = e.get("level").and_then(Json::as_str)?; let pl = before.prev.get(k).cloned().unwrap_or_else(|| v.fallback.clone());
        let ok = allowed(&v.levels, r, &pl);
        if !ok.iter().any(|l| l.as_str() == lvl) { out.push(Broken { var: k.clone(), level: lvl.to_string(), allowed: ok.into_iter().cloned().collect() }); } }
    if !pr.habit.rules.is_empty() && status == "ok" {
        let mut flat: HashMap<String, String> = p.vars.iter().filter_map(|v| entry(&v.id).and_then(|e| e.get("level")).and_then(Json::as_str).map(|l| (v.id.clone(), l.to_string()))).collect();
        if let Some(a) = doc.get("agenda").and_then(Json::as_arr) { for (i, s) in a.iter().enumerate() { if let (Some(s), Some(slot)) = (s.as_str(), p.slots.get(i)) { flat.insert(format!("step.{s}"), slot.clone()); } } }
        let (mut rules, mut owners) = (empty_rules(), vec![]);
        habit_rules(p, &Habit { then: vec![], ..pr.habit.clone() }, &before.prev, &mut rules, &mut owners);
        if !violations(&flat, &rules, &values_of(p)).is_empty() { out.push(Broken { var: "rules".into(), level: String::new(), allowed: vec![] }); } }
    (!out.is_empty()).then_some(out)
}
/// The odds mass the stance puts on levels the property's `then` entries forbid (the largest over its entries; 0 for raw rules):
/// what the fuzzer's beam climbs
pub fn pressure(p: &Persona, pr: &Prop, before: &State, doc: &Json) -> f64 {
    let entry = |id: &str| doc.get("stance").and_then(|s| s.get(id)).or_else(|| doc.get("mood").and_then(|m| m.get(id)));
    pr.habit.then.iter().filter_map(|(k, r)| { let v = p.var(k)?; let odds = entry(k)?.get("odds")?;
        let pl = before.prev.get(k).cloned().unwrap_or_else(|| v.fallback.clone()); let ok = allowed(&v.levels, r, &pl);
        Some(v.levels.iter().filter(|l| !ok.contains(l)).map(|l| odds.get(l).and_then(Json::as_f64).unwrap_or(0.0)).fold(0.0, |a, b| a + b)) })
        .fold(0.0, f64::max)
}
/// The event alphabet: per declared input its non-default values (a flag: true; a level input: its other levels; a number: `grid`
/// x its max, without its default), then `elapsed_hours` (`hours`) when a mood has a half-life. -> (id, kind, values)
pub fn alphabet(p: &Persona, grid: &[f64], hours: &[f64]) -> Vec<(String, &'static str, Vec<Json>)> {
    let mut out: Vec<(String, &'static str, Vec<Json>)> = p.inputs.iter().map(|x| match x.kind {
        Kind::Flag => (x.id.clone(), "flag", vec![Json::Bool(true)]),
        Kind::Level => (x.id.clone(), "level", x.levels.iter().filter(|l| Some(l.as_str()) != x.default.as_str()).map(|l| Json::Str(l.clone())).collect()),
        Kind::Number => { let mut v: Vec<f64> = vec![]; for g in grid { let n = r6(g * x.max); if Some(n) != x.default.as_f64() && !v.contains(&n) { v.push(n); } }
            (x.id.clone(), "number", v.into_iter().map(Json::Num).collect()) } }).filter(|e| !e.2.is_empty()).collect();
    if p.vars.iter().any(|v| v.mood && v.half_life.is_some()) && hours.iter().any(|h| *h > 0.0) {
        let mut h: Vec<Json> = vec![]; for x in hours.iter().filter(|h| **h > 0.0) { if !h.contains(&Json::Num(*x)) { h.push(Json::Num(*x)); } }
        out.push(("elapsed_hours".into(), "hours", h)); }
    out
}
/// The input assignments that put the property in force: one per combination of its input conditions (a level list gives one
/// per level; a number the smallest grid value in range). History conditions are left to the search. [] = no input can.
pub fn forcing(p: &Persona, pr: &Prop, grid: &[f64]) -> Vec<Vec<(String, Json)>> {
    let mut bases: Vec<Vec<(String, Json)>> = vec![vec![]];
    for (k, c) in &pr.habit.when {
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
