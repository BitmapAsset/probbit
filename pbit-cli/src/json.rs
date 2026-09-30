//! Minimal JSON reader/writer (no external crates). Enough for the pbit problem/decision documents.
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

pub fn parse(src: &str) -> Result<Json, String> {
    let b = src.as_bytes(); let mut i = 0; let v = value(b, &mut i)?; ws(b, &mut i);
    if i != b.len() { return Err(format!("trailing characters at byte {i}")); } Ok(v)
}
fn ws(b: &[u8], i: &mut usize) { while *i < b.len() && matches!(b[*i], b' ' | b'\n' | b'\r' | b'\t') { *i += 1; } }
fn value(b: &[u8], i: &mut usize) -> Result<Json, String> {
    ws(b, i); if *i >= b.len() { return Err("unexpected end of input".into()); }
    match b[*i] {
        b'{' => { *i += 1; let mut out = vec![]; ws(b, i);
            if *i < b.len() && b[*i] == b'}' { *i += 1; return Ok(Json::Obj(out)); }
            loop { ws(b, i); let k = string(b, i)?; ws(b, i);
                if *i >= b.len() || b[*i] != b':' { return Err(format!("expected ':' at byte {i}")); } *i += 1;
                let v = value(b, i)?; out.push((k, v)); ws(b, i);
                match b.get(*i) { Some(b',') => { *i += 1; } Some(b'}') => { *i += 1; return Ok(Json::Obj(out)); } _ => return Err(format!("expected ',' or '}}' at byte {i}")) } } }
        b'[' => { *i += 1; let mut out = vec![]; ws(b, i);
            if *i < b.len() && b[*i] == b']' { *i += 1; return Ok(Json::Arr(out)); }
            loop { let v = value(b, i)?; out.push(v); ws(b, i);
                match b.get(*i) { Some(b',') => { *i += 1; } Some(b']') => { *i += 1; return Ok(Json::Arr(out)); } _ => return Err(format!("expected ',' or ']' at byte {i}")) } } }
        b'"' => Ok(Json::Str(string(b, i)?)),
        b't' if b[*i..].starts_with(b"true") => { *i += 4; Ok(Json::Bool(true)) }
        b'f' if b[*i..].starts_with(b"false") => { *i += 5; Ok(Json::Bool(false)) }
        b'n' if b[*i..].starts_with(b"null") => { *i += 4; Ok(Json::Null) }
        b'-' | b'0'..=b'9' => { let s = *i; *i += 1;
            while *i < b.len() && matches!(b[*i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') { *i += 1; }
            std::str::from_utf8(&b[s..*i]).unwrap().parse::<f64>().map(Json::Num).map_err(|e| format!("bad number at byte {s}: {e}")) }
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
                    b'u' => { let h = std::str::from_utf8(b.get(*i..*i + 4).ok_or("bad \\u escape")?).map_err(|e| e.to_string())?; *i += 4;
                        let cp = u32::from_str_radix(h, 16).map_err(|e| e.to_string())?; out.push(char::from_u32(cp).unwrap_or('\u{fffd}')); }
                    _ => return Err(format!("bad escape at byte {i}")) } }
            _ => { let s = *i; while *i < b.len() && b[*i] != b'"' && b[*i] != b'\\' { *i += 1; } out.push_str(std::str::from_utf8(&b[s..*i]).map_err(|e| e.to_string())?); } } }
}

/// Serialize. `pretty` = 2-space indentation. Numbers: integers print without a fraction, others with up to 6 significant decimals.
pub fn write(j: &Json, pretty: bool) -> String { let mut s = String::new(); w(j, pretty, 0, &mut s); s }
fn w(j: &Json, pretty: bool, d: usize, s: &mut String) {
    let nl = |s: &mut String, d: usize| if pretty { s.push('\n'); for _ in 0..d { s.push_str("  "); } };
    match j {
        Json::Null => s.push_str("null"), Json::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        Json::Num(x) => { if x.fract() == 0.0 && x.abs() < 1e15 { s.push_str(&format!("{}", *x as i64)); } else if x.is_finite() { s.push_str(&format!("{}", (x * 1e6).round() / 1e6)); } else { s.push_str("null"); } }
        Json::Str(t) => esc(t, s),
        Json::Arr(v) => { s.push('['); for (k, x) in v.iter().enumerate() { if k > 0 { s.push(','); } nl(s, d + 1); w(x, pretty, d + 1, s); } if !v.is_empty() { nl(s, d); } s.push(']'); }
        Json::Obj(v) => { s.push('{'); for (k, (key, x)) in v.iter().enumerate() { if k > 0 { s.push(','); } nl(s, d + 1); esc(key, s); s.push(':'); if pretty { s.push(' '); } w(x, pretty, d + 1, s); } if !v.is_empty() { nl(s, d); } s.push('}'); }
    }
}
fn esc(t: &str, s: &mut String) { s.push('"'); for c in t.chars() { match c { '"' => s.push_str("\\\""), '\\' => s.push_str("\\\\"), '\n' => s.push_str("\\n"), '\t' => s.push_str("\\t"), '\r' => s.push_str("\\r"), c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)), c => s.push(c) } } s.push('"'); }
pub fn obj(v: Vec<(&str, Json)>) -> Json { Json::Obj(v.into_iter().map(|(k, x)| (k.to_string(), x)).collect()) }
pub fn num(x: f64) -> Json { Json::Num(x) }
pub fn str(x: &str) -> Json { Json::Str(x.to_string()) }
