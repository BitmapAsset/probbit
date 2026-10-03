//! The YAML subset a persona file may use (docs/persona.md §1), read into the crate's `Json`. Supported: block mappings
//! (`key: value`, `key:` + an indented block), block sequences (`- item`, including `- key: value` items), one-line flow
//! collections (`[a, b]`, `{a: 1, b: [x, y]}`), plain / single- / double-quoted scalars, numbers, true / false / null / ~ and
//! comments (`#` at the line start or after whitespace, outside quotes). Rejected with a line number, never guessed: tabs in
//! indentation, anchors / aliases / tags, block scalars (`|`, `>`), flow collections over several lines, duplicate keys, a second
//! document, and plain keys another YAML reader would not read as strings (yes, no, on, off, true, false, null, numbers: quote
//! them). Whitespace, line breaks and the quoting rules are Python's (the reference reader is the persona prototype's
//! `miniyaml.py`; this is a port of it). Differences, both on inputs it got wrong: a quoted scalar as a block-sequence item
//! (`- "x"`) is read (miniyaml refused it), and a number that is not exact in a double (an integer beyond 2^53, `1e999`) is an
//! error (miniyaml kept the integer exact, or read infinity).
use crate::json::Json;

pub struct YamlError { pub line: usize, pub msg: String }
fn err<T>(line: usize, msg: impl Into<String>) -> Result<T, YamlError> { Err(YamlError { line, msg: msg.into() }) }

/// Python's `str.isspace`: Unicode White_Space plus U+001C..U+001F (what `strip()`, `split()` and the regex `\s` use there)
pub fn space(c: char) -> bool { c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c) }
/// Python's `str.splitlines()`
fn lines(t: &str) -> Vec<String> {
    let (mut out, mut cur, mut it) = (vec![], String::new(), t.chars().peekable());
    while let Some(c) = it.next() { match c {
        '\r' => { if it.peek() == Some(&'\n') { it.next(); } out.push(std::mem::take(&mut cur)); }
        '\n' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}' => out.push(std::mem::take(&mut cur)),
        c => cur.push(c) } }
    if !cur.is_empty() { out.push(cur); } out
}

/// The line without its comment (a `#` at the start or after a space / tab, outside quotes), right-stripped
fn strip_comment(s: &[char], ln: usize) -> Result<String, YamlError> {
    let (mut out, mut q, mut i) = (String::new(), None::<char>, 0);
    while i < s.len() { let c = s[i];
        if let Some(qc) = q { out.push(c);
            if qc == '"' && c == '\\' && i + 1 < s.len() { out.push(s[i + 1]); i += 2; continue; }
            if c == qc { if qc == '\'' && i + 1 < s.len() && s[i + 1] == '\'' { out.push('\''); i += 2; continue; } q = None; }
        } else {
            if (c == '"' || c == '\'') && (i == 0 || " \t[{,:-".contains(s[i - 1])) { q = Some(c); }
            else if c == '#' && (i == 0 || s[i - 1] == ' ' || s[i - 1] == '\t') { break; }
            out.push(c);
        }
        i += 1; }
    if q.is_some() { return err(ln, "unterminated quoted string"); }
    Ok(out.trim_end_matches(space).to_string())
}

/// `^\s*-\s+X` (from the start) or `:\s+X` (anywhere) for the indicator X
fn indicator(s: &[char], x: char) -> bool {
    let after_spaces = |mut j: usize| { let s0 = j; while j < s.len() && space(s[j]) { j += 1; } (j > s0 && j < s.len() && s[j] == x).then_some(()) };
    let mut j = 0; while j < s.len() && space(s[j]) { j += 1; }
    if j < s.len() && s[j] == '-' && after_spaces(j + 1).is_some() { return true; }
    (0..s.len()).any(|p| s[p] == ':' && after_spaces(p + 1).is_some())
}
/// `(^|\s)[|>][-+0-9]*\s*$`
fn block_scalar(s: &[char]) -> bool {
    (0..s.len()).any(|i| (s[i] == '|' || s[i] == '>') && (i == 0 || space(s[i - 1])) && {
        let mut j = i + 1; while j < s.len() && (s[j] == '-' || s[j] == '+' || s[j].is_ascii_digit()) { j += 1; } s[j..].iter().all(|&c| space(c)) })
}

/// (indentation in spaces, the stripped line, line number) per content line
fn rows(text: &str) -> Result<Vec<(usize, String, usize)>, YamlError> {
    let mut rows = vec![];
    for (n, raw) in lines(text).into_iter().enumerate() { let ln = n + 1;
        let raw = if ln == 1 { raw.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(raw) } else { raw };
        let body = raw.trim_end_matches('\r');
        let lead = &body[..body.len() - body.trim_start_matches([' ', '\t']).len()];
        if lead.contains('\t') { return err(ln, "tab in indentation"); }
        let s = strip_comment(&body.chars().collect::<Vec<_>>(), ln)?;
        let st = s.trim_matches(space); if st.is_empty() { continue; }
        if st == "---" || st == "..." { if !rows.is_empty() { return err(ln, "only one document per file"); } continue; }
        let cs: Vec<char> = s.chars().collect();
        for (bad, what) in [('&', "anchor"), ('*', "alias"), ('!', "tag")] {
            if s.trim_start_matches(space).starts_with(bad) || indicator(&cs, bad) { return err(ln, format!("{what} not supported")); } }
        if block_scalar(&cs) { return err(ln, "block scalars (| or >) not supported"); }
        rows.push((s.len() - s.trim_start_matches(' ').len(), st.to_string(), ln));
    }
    Ok(rows)
}

/// A JSON string literal as Python's `json.loads` reads one (strict: no raw control characters); the whole token
fn json_string(t: &[char]) -> Option<String> {
    let (mut out, mut i) = (String::new(), 1);
    if t.first() != Some(&'"') { return None; }
    let hex4 = |i: usize| -> Option<u32> { (i + 4 <= t.len()).then_some(())?; u32::from_str_radix(&t[i..i + 4].iter().collect::<String>(), 16).ok().filter(|_| t[i..i + 4].iter().all(|c| c.is_ascii_hexdigit())) };
    loop { let c = *t.get(i)?;
        match c {
            '"' => { i += 1; break; }
            '\\' => { let e = *t.get(i + 1)?; i += 2;
                match e { '"' => out.push('"'), '\\' => out.push('\\'), '/' => out.push('/'), 'b' => out.push('\u{8}'), 'f' => out.push('\u{c}'), 'n' => out.push('\n'), 'r' => out.push('\r'), 't' => out.push('\t'),
                    'u' => { let mut cp = hex4(i)?; i += 4;
                        if (0xD800..0xDC00).contains(&cp) && t.get(i) == Some(&'\\') && t.get(i + 1) == Some(&'u') { if let Some(lo) = hex4(i + 2).filter(|lo| (0xDC00..0xE000).contains(lo)) { cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00); i += 6; } }
                        out.push(char::from_u32(cp)?); } // a lone surrogate is no character
                    _ => return None } }
            c if (c as u32) < 0x20 => return None,
            c => { out.push(c); i += 1; } } }
    t[i..].iter().all(|&c| matches!(c, ' ' | '\t' | '\n' | '\r')).then_some(out)
}
fn unquote(t: &[char], ln: usize) -> Result<String, YamlError> {
    let tok: String = t.iter().collect();
    if t[0] == '"' { return json_string(t).map_or_else(|| err(ln, format!("bad double-quoted string {tok}")), Ok); }
    if t.len() < 2 || t[t.len() - 1] != '\'' { return err(ln, format!("bad single-quoted string {tok}")); }
    Ok(t[1..t.len() - 1].iter().collect::<String>().replace("''", "'"))
}

/// `^[-+]?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][-+]?[0-9]+)?$` -> Some(is an integer)
fn number_form(t: &str) -> Option<bool> {
    let b = t.as_bytes(); let mut i = 0; let digits = |i: &mut usize| { let s = *i; while *i < b.len() && b[*i].is_ascii_digit() { *i += 1; } *i - s };
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') { i += 1; }
    if i < b.len() && b[i] == b'0' { i += 1; } else if i < b.len() && (b'1'..=b'9').contains(&b[i]) { digits(&mut i); } else { return None; }
    let mut int = true;
    if i < b.len() && b[i] == b'.' { i += 1; if digits(&mut i) == 0 { return None; } int = false; }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += if b.get(i + 1).is_some_and(|c| *c == b'-' || *c == b'+') { 2 } else { 1 };
        if digits(&mut i) == 0 { return None; }
        int = false;
    }
    (i == b.len()).then_some(int)
}
fn scalar(tok: &str, ln: usize) -> Result<Json, YamlError> {
    let t = tok.trim_matches(space); let Some(c0) = t.chars().next() else { return Ok(Json::Null) };
    if c0 == '"' || c0 == '\'' { return unquote(&t.chars().collect::<Vec<_>>(), ln).map(Json::Str); }
    if ["null", "Null", "NULL", "~"].contains(&t) { return Ok(Json::Null); }
    if ["true", "True", "TRUE"].contains(&t) { return Ok(Json::Bool(true)); }
    if ["false", "False", "FALSE"].contains(&t) { return Ok(Json::Bool(false)); }
    if let Some(int) = number_form(t) {
        let x: f64 = t.parse().map_err(|_| YamlError { line: ln, msg: format!("bad number {t}") })?;
        if !x.is_finite() || (int && x.abs() > 9_007_199_254_740_992.0) { return err(ln, format!("number {t} is not exact in a double (at most 2^53 for an integer): quote it or write it smaller")); }
        return Ok(Json::Num(x)); }
    if c0 == '[' || c0 == '{' { return flow(&t.chars().collect::<Vec<_>>(), ln); }
    if "]},".contains(c0) || t.starts_with("? ") || c0 == '@' || c0 == '`' { return err(ln, format!("unexpected '{c0}'")); }
    Ok(Json::Str(t.to_string()))
}

/// One-line flow collection `[...]` / `{...}`
fn flow(t: &[char], ln: usize) -> Result<Json, YamlError> {
    struct F<'a> { t: &'a [char], pos: usize, ln: usize }
    impl F<'_> {
        fn ws(&mut self) { while self.pos < self.t.len() && self.t[self.pos] == ' ' { self.pos += 1; } }
        /// (token, quoted)
        fn token(&mut self, stops: &str) -> Result<(String, bool), YamlError> {
            self.ws(); let (t, i) = (self.t, self.pos);
            if i < t.len() && (t[i] == '"' || t[i] == '\'') { let q = t[i]; let mut j = i + 1;
                while j < t.len() { if q == '"' && t[j] == '\\' { j += 2; continue; }
                    if t[j] == q { if q == '\'' && j + 1 < t.len() && t[j + 1] == '\'' { j += 2; continue; } break; } j += 1; }
                if j >= t.len() { return err(self.ln, "unterminated quoted string in flow collection"); }
                self.pos = j + 1; return Ok((unquote(&t[i..=j], self.ln)?, true)); }
            let mut j = i; while j < t.len() && !stops.contains(t[j]) { j += 1; }
            self.pos = j; Ok((t[i..j].iter().collect::<String>().trim_matches(space).to_string(), false))
        }
        fn value(&mut self) -> Result<Json, YamlError> {
            self.ws(); let ln = self.ln; if self.pos >= self.t.len() { return err(ln, "flow collection ends early"); }
            match self.t[self.pos] {
                '[' => { self.pos += 1; let mut out = vec![]; self.ws();
                    if self.pos < self.t.len() && self.t[self.pos] == ']' { self.pos += 1; return Ok(Json::Arr(out)); }
                    loop { out.push(self.value()?); self.ws();
                        match self.t.get(self.pos) { None => return err(ln, "unterminated ["), Some(',') => self.pos += 1, Some(']') => { self.pos += 1; return Ok(Json::Arr(out)); }
                            _ => return err(ln, "expected , or ] in flow sequence") } } }
                '{' => { self.pos += 1; let mut out: Vec<(String, Json)> = vec![]; self.ws();
                    if self.pos < self.t.len() && self.t[self.pos] == '}' { self.pos += 1; return Ok(Json::Obj(out)); }
                    loop { let (k, quoted) = self.token(":,}")?;
                        if !quoted { if k.is_empty() { return err(ln, "empty key in flow mapping"); } check_plain_key(&k, ln)?; }
                        self.ws(); if self.t.get(self.pos) != Some(&':') { return err(ln, format!("expected : after key {k:?}")); } self.pos += 1;
                        if out.iter().any(|(x, _)| *x == k) { return err(ln, format!("duplicate key {k:?}")); }
                        let v = self.value()?; out.push((k, v)); self.ws();
                        match self.t.get(self.pos) { None => return err(ln, "unterminated {"), Some(',') => self.pos += 1, Some('}') => { self.pos += 1; return Ok(Json::Obj(out)); }
                            _ => return err(ln, "expected , or } in flow mapping") } } }
                _ => { let (tok, quoted) = self.token(",]}")?; if quoted { Ok(Json::Str(tok)) } else { scalar(&tok, ln) } } }
        }
    }
    let mut f = F { t, pos: 0, ln }; let v = f.value()?; f.ws();
    if f.pos != t.len() { return err(ln, format!("trailing text after flow collection: {:?}", t[f.pos..].iter().collect::<String>())); }
    Ok(v)
}

const KEY_WORDS: [&str; 10] = ["true", "false", "null", "yes", "no", "on", "off", "y", "n", "~"];
/// Every key is a string in this subset: a plain key another YAML reader would read as a bool / null / number is refused
fn check_plain_key(k: &str, ln: usize) -> Result<(), YamlError> {
    if KEY_WORDS.contains(&k.to_lowercase().as_str()) || number_form(k).is_some() { return err(ln, format!("key {k:?} would not be a string in YAML: quote it")); } Ok(())
}
/// `key: rest` -> Some((key, rest after the colon)); None = not a key. `item`: a block-sequence item, where a quoted scalar
/// without a colon after it is a scalar, not a malformed key.
fn split_key(s: &str, ln: usize, item: bool) -> Result<Option<(String, String)>, YamlError> {
    let c: Vec<char> = s.chars().collect();
    if c[0] == '"' || c[0] == '\'' { let q = c[0]; let mut j = 1;
        while j < c.len() { if q == '"' && c[j] == '\\' { j += 2; continue; }
            if c[j] == q { if q == '\'' && j + 1 < c.len() && c[j + 1] == '\'' { j += 2; continue; } break; } j += 1; }
        let rest: String = c.get(j + 1..).map_or(String::new(), |r| r.iter().collect());
        if item && !rest.starts_with(':') { return Ok(None); }
        let key = unquote(&c[..(j + 1).min(c.len())], ln)?;
        if !rest.starts_with(':') { return err(ln, "expected : after quoted key"); }
        return Ok(Some((key, rest[1..].to_string()))); }
    // ^([^:\[\]{},#]+?):(\s|$)
    let Some(p) = c.iter().position(|ch| ":[]{},#".contains(*ch)) else { return Ok(None) };
    if p == 0 || c[p] != ':' || c.get(p + 1).is_some_and(|&n| !space(n)) { return Ok(None); }
    let key = c[..p].iter().collect::<String>().trim_matches(space).to_string(); check_plain_key(&key, ln)?;
    Ok(Some((key, c[p + 1..].iter().collect())))
}
fn is_item(s: &str) -> bool { s == "-" || s.starts_with("- ") }

/// The block whose lines start at rows[i] with exactly `indent` spaces -> (value, next index)
fn block(rows: &mut Vec<(usize, String, usize)>, mut i: usize, indent: usize) -> Result<(Json, usize), YamlError> {
    if is_item(&rows[i].1) {
        let mut out = vec![];
        while i < rows.len() && rows[i].0 == indent && is_item(&rows[i].1) {
            let ln = rows[i].2; let rest = rows[i].1[1..].to_string(); let inner = rest.trim_start_matches(' ').to_string();
            if inner.is_empty() {
                let v = if i + 1 < rows.len() && rows[i + 1].0 > indent { let (v, n) = block(rows, i + 1, rows[i + 1].0)?; i = n; v } else { i += 1; Json::Null };
                out.push(v); continue; }
            let col = indent + 1 + (rest.len() - inner.len());
            let key = if inner.starts_with(['[', '{']) { None } else { split_key(&inner, ln, true)? };
            if key.is_some() { rows[i] = (col, inner, ln); let (v, n) = block(rows, i, col)?; i = n; out.push(v); }
            else { out.push(scalar(&inner, ln)?); i += 1; }
        }
        if i < rows.len() && rows[i].0 > indent { return err(rows[i].2, "bad indentation"); }
        return Ok((Json::Arr(out), i));
    }
    let mut out: Vec<(String, Json)> = vec![];
    while i < rows.len() && rows[i].0 == indent {
        let (ln, s) = (rows[i].2, rows[i].1.clone());
        if is_item(&s) { return err(ln, "sequence item where a mapping key was expected"); }
        let Some((k, rest)) = split_key(&s, ln, false)? else { return err(ln, "expected `key: value`") };
        if out.iter().any(|(x, _)| *x == k) { return err(ln, format!("duplicate key {k:?}")); }
        let rest = rest.trim_matches(space);
        let v = if !rest.is_empty() { i += 1; scalar(rest, ln)? }
            else if i + 1 < rows.len() && rows[i + 1].0 > indent { let (v, n) = block(rows, i + 1, rows[i + 1].0)?; i = n; v }
            // a sequence at the key's own indentation (common YAML style)
            else if i + 1 < rows.len() && rows[i + 1].0 == indent && is_item(&rows[i + 1].1) { let (v, n) = block(rows, i + 1, indent)?; i = n; v }
            else { i += 1; Json::Null };
        out.push((k, v));
    }
    if i < rows.len() && rows[i].0 > indent { return err(rows[i].2, "bad indentation"); }
    Ok((Json::Obj(out), i))
}

/// Read a document of the subset.
pub fn load(text: &str) -> Result<Json, YamlError> {
    let mut rows = rows(text)?;
    if rows.is_empty() { return Ok(Json::Null); }
    if rows.len() == 1 && !rows[0].1.contains(':') && !rows[0].1.starts_with('-') { return scalar(&rows[0].1.clone(), rows[0].2); }
    let base = rows[0].0; let (v, i) = block(&mut rows, 0, base)?;
    if i != rows.len() { return err(rows[i].2, "unexpected text (indentation?)"); }
    Ok(v)
}
