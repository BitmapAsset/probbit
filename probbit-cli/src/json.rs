//! Minimal JSON reader/writer (no external crates). Enough for the probbit problem/decision documents.
#[derive(Clone, Debug, PartialEq)]
pub enum Json { Null, Bool(bool), Num(f64), Str(String), Arr(Vec<Json>), Obj(Vec<(String, Json)>) }

impl Json {
    pub fn get(&self, k: &str) -> Option<&Json> { match self { Json::Obj(v) => v.iter().find(|(key, _)| key == k).map(|(_, x)| x), _ => None } }
    pub fn as_f64(&self) -> Option<f64> { if let Json::Num(x) = self { Some(*x) } else { None } }
    pub fn as_str(&self) -> Option<&str> { if let Json::Str(s) = self { Some(s) } else { None } }
    pub fn as_arr(&self) -> Option<&[Json]> { if let Json::Arr(v) = self { Some(v) } else { None } }
    pub fn as_obj(&self) -> Option<&[(String, Json)]> { if let Json::Obj(v) = self { Some(v) } else { None } }
    pub fn is_null(&self) -> bool { matches!(self, Json::Null) }
}

/// Parse one JSON document. A syntax error is a `schema` error; a number that is not a finite double (`1e309`: it used to parse
/// to inf and abort the process, exit 134) or nesting deeper than `MAX_DEPTH` (the recursive parser's stack) a `limit` error.
/// So every parsed number is finite.
pub fn parse(src: &str) -> Result<Json, InErr> {
    let b = src.as_bytes(); let mut i = 0;
    let v = node(b, &mut i, 0).map_err(|e| match e.strip_prefix("limit: ") { Some(m) => limit("", m), None => schema("", format!("bad JSON: {e}")) })?; ws(b, &mut i);
    if i != b.len() { return Err(schema("", format!("bad JSON: trailing characters at byte {i}"))); } Ok(v)
}

/// A structured input error (docs/probbit-ir-json.md "Input contract"): `probbit decide`, `run` and `ir` print it as ONE JSON object
/// `{"error":{"code","path","message"}}` on stdout (plus a human line on stderr) and exit 2. `code`: `schema` (bad JSON, a wrong
/// type, an unknown, duplicate or missing field), `value` (well-typed but unusable: an unknown or duplicate name, an empty domain,
/// a number out of its range), `limit` (beyond a documented numeric or size limit). `path`: `tasks[3].allowed`, "" = the document.
#[derive(Debug)]
pub struct InErr { pub code: &'static str, pub path: String, pub msg: String }
impl InErr { pub fn to_json(&self) -> Json { obj(vec![("error", obj(vec![("code", str(self.code)), ("path", str(&self.path)), ("message", str(&self.msg))]))]) } }
pub fn schema(path: &str, msg: impl Into<String>) -> InErr { InErr { code: "schema", path: path.to_string(), msg: msg.into() } }
pub fn value(path: &str, msg: impl Into<String>) -> InErr { InErr { code: "value", path: path.to_string(), msg: msg.into() } }
pub fn limit(path: &str, msg: impl Into<String>) -> InErr { InErr { code: "limit", path: path.to_string(), msg: msg.into() } }
/// `path.key` / `path[i]` ("" = the document)
pub fn at(path: &str, key: &str) -> String { if path.is_empty() { key.to_string() } else { format!("{path}.{key}") } }
pub fn ix(path: &str, i: usize) -> String { format!("{path}[{i}]") }
/// `j` must be an object whose keys are all in `known`, each at most once (unknown fields used to be ignored silently: a cap
/// object where the caps array belongs was dropped and the program ran without it).
pub fn fields<'a>(j: &'a Json, path: &str, known: &[&str]) -> Result<&'a [(String, Json)], InErr> {
    let o = j.as_obj().ok_or_else(|| schema(path, "must be an object"))?;
    for (n, (k, _)) in o.iter().enumerate() {
        if !known.contains(&k.as_str()) { return Err(schema(&at(path, k), format!("unknown field \"{k}\" (known: {})", known.join(", ")))); }
        if o[..n].iter().any(|(k2, _)| k2 == k) { return Err(schema(&at(path, k), format!("duplicate field \"{k}\""))); } }
    Ok(o)
}
/// A data-keyed object (`scores`, `h`): any keys, each at most once (a duplicate key used to emit duplicate output keys).
pub fn keyed<'a>(j: &'a Json, path: &str) -> Result<&'a [(String, Json)], InErr> {
    let o = j.as_obj().ok_or_else(|| schema(path, "must be an object"))?; let mut seen = std::collections::HashSet::with_capacity(o.len());
    for (k, _) in o { if !seen.insert(k.as_str()) { return Err(schema(&at(path, k), format!("duplicate key \"{k}\""))); } } Ok(o)
}
/// An optional field: absent or `null` = None.
pub fn opt<'a>(j: &'a Json, k: &str) -> Option<&'a Json> { j.get(k).filter(|x| !x.is_null()) }
pub fn req<'a>(j: &'a Json, k: &str, path: &str) -> Result<&'a Json, InErr> { opt(j, k).ok_or_else(|| schema(&at(path, k), format!("missing \"{k}\""))) }
pub fn arr<'a>(j: &'a Json, path: &str) -> Result<&'a [Json], InErr> { j.as_arr().ok_or_else(|| schema(path, "must be an array")) }
pub fn text<'a>(j: &'a Json, path: &str) -> Result<&'a str, InErr> { j.as_str().ok_or_else(|| schema(path, "must be a string")) }
pub fn number(j: &Json, path: &str) -> Result<f64, InErr> { j.as_f64().ok_or_else(|| schema(path, "must be a number")) }
/// The magnitude limit for every score / weight (scores, affinity, h, potts, table): |x| <= 1e9. Weights are natural-log odds,
/// so beyond |x| ~ 745 one already decides (e^745 is past the double range of probability ratios): the limit removes no
/// expressible distribution, and it keeps every log-space quantity (sums over variables, pairs and same-group pairs, log Z)
/// below ~1e9 x (input size), far inside the double range (1.8e308). The arithmetic stays in log space (max-subtracted exp).
pub const MAX_WEIGHT: f64 = 1e9;
/// The bound on dense (variable, value) tables: `probbit run`'s vars x values and the router's tasks x workers. h, allowed and the
/// sampler's and gate's per-pair arrays are dense, so a 0.6 MB program (65,535 values, 2,000 variables) peaked at 6.3 GB RSS.
/// Above it: a `limit` error before anything is allocated. The largest documented program is 200 x 65,535 = 13.1 million.
pub const MAX_DENSE: usize = 20_000_000;
pub fn weight(j: &Json, path: &str) -> Result<f64, InErr> {
    let x = number(j, path)?; if x.abs() > MAX_WEIGHT { return Err(limit(path, format!("{x:e} is beyond the weight limit |x| <= 1e9 (natural-log odds; rescale)"))); } Ok(x)
}
/// Paths of the non-finite numbers in `j` (a decision may carry none; main.rs `finish`).
pub fn non_finite(j: &Json, path: &str, out: &mut Vec<String>) {
    match j { Json::Num(x) if !x.is_finite() => out.push(path.to_string()), Json::Arr(v) => for (i, x) in v.iter().enumerate() { non_finite(x, &ix(path, i), out) },
        Json::Obj(v) => for (k, x) in v { non_finite(x, &at(path, k), out) }, _ => {} }
}
/// A whole number in 0..=2^53 (exact in a double): caps and limits.
pub fn count(j: &Json, path: &str) -> Result<usize, InErr> {
    let x = number(j, path)?; if x < 0.0 || x.fract() != 0.0 { return Err(value(path, "must be a non-negative integer")); }
    if x > 9_007_199_254_740_992.0 { return Err(limit(path, "must be at most 2^53")); } Ok(x as usize)
}
/// An array of distinct strings (`values`, `allowed`, `forbid`, cap `vars`).
pub fn names<'a>(j: &'a Json, path: &str) -> Result<Vec<&'a str>, InErr> {
    let a = j.as_arr().ok_or_else(|| schema(path, "must be an array of strings"))?;
    let mut seen = std::collections::HashSet::with_capacity(a.len()); let mut out = Vec::with_capacity(a.len());
    for (i, x) in a.iter().enumerate() { let s = x.as_str().ok_or_else(|| schema(&ix(path, i), "must be a string"))?;
        if !seen.insert(s) { return Err(value(&ix(path, i), format!("duplicate name \"{s}\""))); } out.push(s); }
    Ok(out)
}
fn ws(b: &[u8], i: &mut usize) { while *i < b.len() && matches!(b[*i], b' ' | b'\n' | b'\r' | b'\t') { *i += 1; } }
pub const MAX_DEPTH: usize = 256;
fn node(b: &[u8], i: &mut usize, d: usize) -> Result<Json, String> {
    ws(b, i); if *i >= b.len() { return Err("unexpected end of input".into()); }
    if d >= MAX_DEPTH && matches!(b[*i], b'{' | b'[') { return Err(format!("limit: arrays / objects nested deeper than {MAX_DEPTH} at byte {i}")); }
    match b[*i] {
        b'{' => { *i += 1; let mut out = vec![]; ws(b, i);
            if *i < b.len() && b[*i] == b'}' { *i += 1; return Ok(Json::Obj(out)); }
            loop { ws(b, i); let k = string(b, i)?; ws(b, i);
                if *i >= b.len() || b[*i] != b':' { return Err(format!("expected ':' at byte {i}")); } *i += 1;
                let v = node(b, i, d + 1)?; out.push((k, v)); ws(b, i);
                match b.get(*i) { Some(b',') => { *i += 1; } Some(b'}') => { *i += 1; return Ok(Json::Obj(out)); } _ => return Err(format!("expected ',' or '}}' at byte {i}")) } } }
        b'[' => { *i += 1; let mut out = vec![]; ws(b, i);
            if *i < b.len() && b[*i] == b']' { *i += 1; return Ok(Json::Arr(out)); }
            loop { let v = node(b, i, d + 1)?; out.push(v); ws(b, i);
                match b.get(*i) { Some(b',') => { *i += 1; } Some(b']') => { *i += 1; return Ok(Json::Arr(out)); } _ => return Err(format!("expected ',' or ']' at byte {i}")) } } }
        b'"' => Ok(Json::Str(string(b, i)?)),
        b't' if b[*i..].starts_with(b"true") => { *i += 4; Ok(Json::Bool(true)) }
        b'f' if b[*i..].starts_with(b"false") => { *i += 5; Ok(Json::Bool(false)) }
        b'n' if b[*i..].starts_with(b"null") => { *i += 4; Ok(Json::Null) }
        b'-' | b'0'..=b'9' => { let s = *i; *i += 1;
            while *i < b.len() && matches!(b[*i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') { *i += 1; }
            let x = std::str::from_utf8(&b[s..*i]).unwrap().parse::<f64>().map_err(|e| format!("bad number at byte {s}: {e}"))?;
            if !x.is_finite() { return Err(format!("limit: number {} at byte {s} is not a finite double", String::from_utf8_lossy(&b[s..*i]))); } Ok(Json::Num(x)) }
        c => Err(format!("unexpected character '{}' at byte {i}", c as char)),
    }
}
fn string(b: &[u8], i: &mut usize) -> Result<String, String> {
    if *i >= b.len() || b[*i] != b'"' { return Err(format!("expected string at byte {i}")); } *i += 1;
    let mut out = String::new();
    loop { if *i >= b.len() { return Err("unterminated string".into()); }
        match b[*i] { b'"' => { *i += 1; return Ok(out); }
            b'\\' => { *i += 1; let c = *b.get(*i).ok_or("bad escape")?; *i += 1;
                match c { b'"' => out.push('"'), b'\\' => out.push('\\'), b'/' => out.push('/'), b'n' => out.push('\n'), b't' => out.push('\t'), b'r' => out.push('\r'), b'b' => out.push('\u{8}'), b'f' => out.push('\u{c}'),
                    b'u' => { let mut cp = hex4(b, i)?;
                        // A UTF-16 surrogate pair (`"\ud83d\ude00"`: Python's json.dumps writes every emoji so) is ONE character; each
                        // half decoded to U+FFFD, so two emoji ids collided. A lone surrogate is no character: an error, not U+FFFD.
                        if (0xD800..0xDC00).contains(&cp) && b.get(*i..*i + 2) == Some(b"\\u") { let s = *i; *i += 2; let lo = hex4(b, i)?;
                            if (0xDC00..0xE000).contains(&lo) { cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00); } else { *i = s; } }
                        out.push(char::from_u32(cp).ok_or_else(|| format!("lone surrogate \\u{cp:04x} at byte {}", *i - 6))?); }
                    _ => return Err(format!("bad escape at byte {i}")) } }
            _ => { let s = *i; while *i < b.len() && b[*i] != b'"' && b[*i] != b'\\' { *i += 1; } out.push_str(std::str::from_utf8(&b[s..*i]).map_err(|e| e.to_string())?); } } }
}

/// The 4 hex digits of a `\u` escape at `*i` (advanced past them).
fn hex4(b: &[u8], i: &mut usize) -> Result<u32, String> {
    let h = std::str::from_utf8(b.get(*i..*i + 4).ok_or("bad \\u escape")?).map_err(|e| e.to_string())?; *i += 4;
    u32::from_str_radix(h, 16).map_err(|e| e.to_string())
}

/// Serialize. `pretty` = 2-space indentation. Numbers: integers print without a fraction, others with up to 6 significant decimals.
pub fn write(j: &Json, pretty: bool) -> String { let mut s = String::new(); w(j, pretty, 0, &mut s); s }
fn w(j: &Json, pretty: bool, d: usize, s: &mut String) {
    let nl = |s: &mut String, d: usize| if pretty { s.push('\n'); for _ in 0..d { s.push_str("  "); } };
    match j {
        Json::Null => s.push_str("null"), Json::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        // |x| >= 1e15 prints in exponent form: `(x * 1e6)` overflowed to a literal `inf` (invalid JSON) above ~1.8e302 (review case E15).
        // Non-finite -> null is a last resort only: main.rs `finish` never lets one through with an answer.
        Json::Num(x) => { if !x.is_finite() { s.push_str("null"); } else if x.abs() >= 1e15 { s.push_str(&format!("{x:e}")); } else if x.fract() == 0.0 { s.push_str(&format!("{}", *x as i64)); } else { s.push_str(&format!("{}", (x * 1e6).round() / 1e6)); } }
        Json::Str(t) => esc(t, s),
        Json::Arr(v) => { s.push('['); for (k, x) in v.iter().enumerate() { if k > 0 { s.push(','); } nl(s, d + 1); w(x, pretty, d + 1, s); } if !v.is_empty() { nl(s, d); } s.push(']'); }
        Json::Obj(v) => { s.push('{'); for (k, (key, x)) in v.iter().enumerate() { if k > 0 { s.push(','); } nl(s, d + 1); esc(key, s); s.push(':'); if pretty { s.push(' '); } w(x, pretty, d + 1, s); } if !v.is_empty() { nl(s, d); } s.push('}'); }
    }
}
fn esc(t: &str, s: &mut String) { s.push('"'); for c in t.chars() { match c { '"' => s.push_str("\\\""), '\\' => s.push_str("\\\\"), '\n' => s.push_str("\\n"), '\t' => s.push_str("\\t"), '\r' => s.push_str("\\r"), c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)), c => s.push(c) } } s.push('"'); }
pub fn obj(v: Vec<(&str, Json)>) -> Json { Json::Obj(v.into_iter().map(|(k, x)| (k.to_string(), x)).collect()) }
pub fn num(x: f64) -> Json { Json::Num(x) }
pub fn str(x: &str) -> Json { Json::Str(x.to_string()) }
