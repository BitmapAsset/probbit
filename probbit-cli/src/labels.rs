//! A life's presentation labels (docs/persona.md §5.8): who the agent is and where it runs (`agent`, `host`, `lane`,
//! `session`, the keys the page reads by name, and free keys it shows as they are), for the monitor's identity header and switcher. They live in a sidecar next to the strand,
//! `<strand without its extension>.labels.json`, never in the strand: the header is the chain's first link and an engine
//! refuses a header key it does not know (0.12.1 says `unsupported by this engine: labels`), so a label in the strand would
//! move every `prev` and stop older engines from verifying it. A sidecar moves nothing: the strand's bytes, chain, persona
//! and state digests are those of the same life without labels.
use crate::json::{self, Json};

/// At most this many keys, each value at most this many characters
pub const MAX_KEYS: usize = 8;
pub const MAX_LEN: usize = 64;
/// Labels sorted by key
pub type Labels = Vec<(String, String)>;

/// A key: 1 to 64 of A-Z a-z 0-9 _ . -
fn check_key(k: &str) -> Result<(), String> {
    if k.is_empty() || k.len() > MAX_LEN || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) {
        return Err(format!("label key {k:?}: 1 to {MAX_LEN} of A-Z a-z 0-9 _ . -"));
    }
    Ok(())
}
/// A value: 1 to 64 printable characters (no newline, tab or other control character)
fn check_value(k: &str, v: &str) -> Result<(), String> {
    let n = v.chars().count();
    if n == 0 || n > MAX_LEN || v.chars().any(char::is_control) { return Err(format!("label {k}: 1 to {MAX_LEN} printable characters (no newline or other control character)")); }
    Ok(())
}
/// `k=v` -> (k, v), checked
pub fn parse(kv: &str) -> Result<(String, String), String> {
    let Some((k, v)) = kv.split_once('=') else { return Err(format!("--label {kv:?}: KEY=VALUE")) };
    check_key(k)?; check_value(k, v)?;
    Ok((k.to_string(), v.to_string()))
}
/// Set `k` to `v` (replacing it, else inserting it in key order); more than MAX_KEYS keys is an error
pub fn set(into: &mut Labels, k: String, v: String) -> Result<(), String> {
    match into.binary_search_by(|(x, _)| x.as_str().cmp(&k)) {
        Ok(i) => into[i].1 = v,
        Err(i) => { if into.len() >= MAX_KEYS { return Err(format!("at most {MAX_KEYS} labels")); } into.insert(i, (k, v)); } }
    Ok(())
}
/// The sidecar of the strand at `strand`: its last extension replaced by `.labels.json`
pub fn sidecar(strand: &str) -> String { std::path::Path::new(strand).with_extension("labels.json").to_string_lossy().into_owned() }
/// The labels as the sidecar holds them: one flat object, keys sorted
pub fn to_json(l: &Labels) -> Json { Json::Obj(l.iter().map(|(k, v)| (k.clone(), Json::Str(v.clone()))).collect()) }
/// A sidecar's object -> its labels, every key and value checked
pub fn from_json(j: &Json) -> Result<Labels, String> {
    let Json::Obj(kv) = j else { return Err("a labels file is one JSON object of strings".into()) };
    let mut l = Labels::new();
    for (k, v) in kv { let Json::Str(v) = v else { return Err(format!("label {k}: a string")) };
        check_key(k)?; check_value(k, v)?; if l.iter().any(|(x, _)| x == k) { return Err(format!("label {k} given twice")); } set(&mut l, k.clone(), v.clone())?; }
    Ok(l)
}
/// The labels in the sidecar of `strand` (none when it has none); Err for a sidecar that cannot be read or breaks the rules
pub fn read(strand: &str) -> Result<Labels, String> {
    let f = sidecar(strand);
    match std::fs::read_to_string(&f) {
        Ok(t) => json::parse(t.strip_prefix('\u{feff}').unwrap_or(&t)).map_err(|e| format!("{f}: not JSON: {}", e.msg)).and_then(|j| from_json(&j).map_err(|e| format!("{f}: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Labels::new()),
        Err(e) => Err(format!("cannot read {f}: {e}")) }
}
/// `probbit live --strand F --label k=v ...`: the keys merged into F's sidecar (keys not named stay), written whole (a temporary
/// file renamed over it) when it changes. The strand is not touched.
pub fn merge_into(strand: &str, add: &Labels) -> Result<Labels, String> {
    let mut l = read(strand)?;
    for (k, v) in add { set(&mut l, k.clone(), v.clone())?; }
    let f = sidecar(strand); let text = crate::persona::canon(&to_json(&l)) + "\n";
    if std::fs::read_to_string(&f).is_ok_and(|t| t == text) { return Ok(l); }
    let tmp = format!("{f}.{}.tmp", std::process::id());
    std::fs::write(&tmp, &text).and_then(|_| std::fs::rename(&tmp, &f)).map_err(|e| { let _ = std::fs::remove_file(&tmp); format!("cannot write {f}: {e}") })?;
    Ok(l)
}
/// Take every `--label k=v` out of `args` -> (the other arguments, the labels in order); a bad label fails the command
pub fn take(args: &[String], cmd: &str) -> (Vec<String>, Vec<(String, String)>) {
    let (mut rest, mut got) = (vec![], vec![]); let mut i = 0;
    while i < args.len() {
        if args[i] == "--label" {
            let Some(v) = args.get(i + 1) else { crate::fail(&format!("{cmd}: --label needs KEY=VALUE")) };
            got.push(parse(v).unwrap_or_else(|e| crate::fail(&format!("{cmd}: {e}")))); i += 2;
        } else { rest.push(args[i].clone()); i += 1; }
    }
    (rest, got)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_are_read_strictly() {
        assert_eq!(parse("agent=Nova"), Ok(("agent".into(), "Nova".into())));
        assert_eq!(parse("host=OpenClaw 2026.9.7 · β"), Ok(("host".into(), "OpenClaw 2026.9.7 · β".into())));
        assert_eq!(parse("a=b=c"), Ok(("a".into(), "b=c".into())));
        for bad in ["agent", "=x", "agent=", "a b=x", "agent=two\nlines", "agent=tab\there", &format!("agent={}", "x".repeat(65)), &format!("{}=x", "k".repeat(65))] {
            assert!(parse(bad).is_err(), "{bad:?}"); }
        assert!(parse(&format!("agent={}", "é".repeat(64))).is_ok(), "64 characters, not bytes");
        let mut l = Labels::new();
        for k in ["h", "g", "f", "e", "d", "c", "b", "a"] { set(&mut l, k.into(), "1".into()).unwrap(); }
        assert_eq!(l.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d", "e", "f", "g", "h"]);
        set(&mut l, "a".into(), "2".into()).unwrap(); assert!(set(&mut l, "i".into(), "1".into()).is_err(), "a ninth key");
        assert!(from_json(&json::parse(r#"{"agent":1}"#).unwrap()).is_err());
        assert!(from_json(&json::parse(r#"["agent"]"#).unwrap()).is_err());
        assert_eq!(sidecar("lanes/support.strand"), "lanes/support.labels.json");
        assert_eq!(sidecar("pip"), "pip.labels.json");
    }
    #[test]
    fn the_sidecar_merges_and_leaves_the_strand_alone() {
        let d = std::env::temp_dir().join(format!("probbit-labels-{}", std::process::id())); std::fs::create_dir_all(&d).unwrap();
        let s = d.join("nova.strand").to_string_lossy().into_owned(); std::fs::write(&s, "strand bytes\n").unwrap();
        merge_into(&s, &vec![("lane".into(), "support".into()), ("agent".into(), "Nova".into())]).unwrap();
        assert_eq!(std::fs::read_to_string(sidecar(&s)).unwrap(), "{\"agent\":\"Nova\",\"lane\":\"support\"}\n");
        let l = merge_into(&s, &vec![("agent".into(), "Nova 2".into())]).unwrap();
        assert_eq!(l, vec![("agent".to_string(), "Nova 2".to_string()), ("lane".to_string(), "support".to_string())], "keys not named stay");
        assert_eq!(read(&s).unwrap(), l); assert_eq!(std::fs::read_to_string(&s).unwrap(), "strand bytes\n");
        std::fs::write(sidecar(&s), "{\"agent\":\"a\\nb\"}").unwrap(); assert!(read(&s).is_err(), "a newline in a value is refused on read too");
        let _ = std::fs::remove_dir_all(&d);
    }
}
