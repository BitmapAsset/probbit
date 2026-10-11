//! `probbit persona` (docs/persona.md): the goldens byte for byte (0.5.0's equalled a reference implementation's), the YAML
//! subset's accepted and rejected inputs, 1,000-turn replays, habit properties over random personas, lint, the `evaluate` bridge,
//! refusals, malformed documents, the Python wrapper.
use std::io::Write;
use std::process::{Command, Stdio};
#[allow(dead_code)]
#[path = "../src/json.rs"]
mod json;
#[allow(dead_code)]
#[path = "../src/yaml.rs"]
mod yaml;
use json::Json;

fn probbit(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let _ = c.stdin.take().unwrap().write_all(stdin.as_bytes());
    let o = c.wait_with_output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
fn ex(p: &str) -> String { format!("{}/../examples/persona/{p}", env!("CARGO_MANIFEST_DIR")) }
/// A file as text with "\n" line ends (a Windows checkout may have made them "\r\n")
fn read(p: &str) -> String { std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p}: {e}")).replace("\r\n", "\n") }
fn tmp(name: &str) -> String { format!("{}/persona-{}-{name}", env!("CARGO_TARGET_TMPDIR"), std::process::id()) }
fn parse(s: &str) -> Json { json::parse(s).unwrap_or_else(|e| panic!("not JSON ({}): {s:.300}", e.msg)) }
fn jw(j: &Json) -> String { json::write(j, false) }
fn s<'a>(j: &'a Json, path: &[&str]) -> &'a Json { path.iter().fold(j, |x, k| x.get(k).unwrap_or_else(|| panic!("no {k} in {:.200}", jw(x)))) }
const NAMES: [&str; 3] = ["ops-engineer", "tutor", "trader-assistant"];

/// Check finite floor lifts on both sides of the raw exponential's range, including subnormal values.
#[test]
fn floor_lifts_hold_across_extreme_fields() {
    let file = tmp("floor-range.json");
    for effects in [r#"{"fun":-10,"safety":-20}"#, r#"{"fun":10}"#, r#"{"fun":10,"safety":20}"#] {
        let doc = format!(r#"{{"probbit_persona":1,"identity":{{"name":"Fixture","version":"1"}},
            "traits":[{{"id":"tone","levels":["low","mid","high"],"prior":[0.2,0.5,0.3]}}],
            "inputs":[{{"id":"pressure","kind":"number","max":100,"effects":{{"pursue":{effects}}}}}],
            "drives":{{"goals":[{{"id":"fun"}},{{"id":"safety","floor":0.1}}]}}}}"#);
        std::fs::write(&file, doc).unwrap();
        for json in [false, true] {
            let mut args = vec!["persona", "prove", &file, "--never", r#"{"then":{"tone":["mid"]}}"#, "--seeds", "0"];
            if json { args.push("--json"); }
            let (c, out, err) = probbit(&args, ""); assert_eq!(c, 0, "{err}");
            if json {
                let proof = parse(&out); let floor = &s(&proof, &["floors"]).as_arr().unwrap()[0];
                assert_eq!(s(floor, &["verdict"]).as_str(), Some("unknown"), "{out}");
            } else { assert!(out.contains("floor  safety") && out.contains("numeric floor guarantee is not certified"), "{out}"); }
        }
        for pressure in [50, 74, 75, 100] {
            let script = format!(r#"[{{"pressure":{pressure}}}]"#);
            let (c, out, err) = probbit(&["persona", "replay", &file, "--script", &script], "");
            assert_eq!(c, 0, "{err}");
            let d = parse(&out);
            assert_eq!(s(&d, &["status"]).as_str(), Some("ok"), "{out}");
            assert_eq!(s(&d, &["pursue", "released"]), &Json::Bool(true), "{out}");
            assert!(s(&d, &["pursue", "odds", "safety"]).as_f64().unwrap() >= 0.1, "{out}");
            assert!(s(&d, &["pursue", "lift"]).as_obj().unwrap().iter().all(|(_, v)| v.as_f64().is_some_and(f64::is_finite)), "{out}");
        }
    }
    let _ = std::fs::remove_file(file);
}

/// Check that six-decimal state output can be read with a cap that has seven decimals.
#[test]
fn rounded_learning_and_drive_caps_continue() {
    for (kind, block, inputs) in [
        ("learning", r#""inputs":[{"id":"praise","kind":"flag"}],"learning":{"from":["praise"],"traits":["tone"],"rate":0.5,"step_cap":0.1234567,"total_cap":0.1234567}"#, r#"{"praise":true}"#),
        ("drive", r#""drives":{"goals":[{"id":"fun"},{"id":"safety"}],"wanting":{"cap":0.1234567}}"#, r#"{"goals":{"fun":{"cue":true}}}"#),
    ] {
        let file = tmp(&format!("{kind}-cap.json")); let state = tmp(&format!("{kind}-cap-state.json"));
        let doc = format!(r#"{{"probbit_persona":1,"identity":{{"name":"Fixture","version":"1"}},
            "traits":[{{"id":"tone","levels":["low","mid","high"],"prior":[0.2,0.5,0.3]}}],{block}}}"#);
        std::fs::write(&file, doc).unwrap();
        let (c, out, err) = probbit(&["persona", "init", &file, "--out", &state], ""); assert_eq!(c, 0, "{out}{err}");
        for event in ["{}", inputs] {
            let (c, out, err) = probbit(&["persona", "turn", &file, "--state", &state, "--inputs", event], ""); assert_eq!(c, 0, "{out}{err}");
        }
        let saved = read(&state); assert!(saved.contains("0.123457"), "{saved}");
        let (c, out, err) = probbit(&["persona", "turn", &file, "--state", &state, "--inputs", "{}"], ""); assert_eq!(c, 0, "{out}{err}");
        for f in [file, state] { let _ = std::fs::remove_file(f); }
    }
}
fn workday() -> Vec<Json> { s(&parse(&read(&ex("workday.json"))), &["turns"]).as_arr().unwrap().to_vec() }

struct Mix(u64);
impl Mix {
    fn next(&mut self) -> u64 { self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15); let mut z = self.0; z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9); z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB); z ^ (z >> 31) }
    fn below(&mut self, k: usize) -> usize { (self.next() % k as u64) as usize }
    fn unit(&mut self) -> f64 { (self.next() >> 11) as f64 / (1u64 << 53) as f64 }
}

/// The acceptance test. For the three example personas, in both forms (YAML subset and JSON: one document, one digest), driven as
/// a host drives them: `init` = state0.json, `compile` of turn 0 = program-turn0.json, 20 `turn --state FILE` calls = workday.jsonl
/// line by line, the state file after them = final-state.json, `replay` = workday.jsonl, `lint` = lint.json. Every document is
/// canonical JSON. The 0.5.0 goldens equalled an independent reference implementation's byte for byte (docs/persona.md §5.3; each
/// `engine.program` digest aside, whose IR version key the reference's 0.4.0 engine spelled `pbit_ir`). 0.6.0 changed `why` (a
/// direction, "humour down", where it named a level) and the tutor (one habit more), so these files are now this implementation's
/// output, regenerated by driving it as above; the parity claim is for the 0.5.0 documents.
#[test]
fn stdout_matches_the_persona_goldens() {
    let turns = workday(); assert_eq!(turns.len(), 20);
    for n in NAMES { for form in ["yaml", "json"] {
        let p = ex(&format!("{n}.{form}")); let g = |f: &str| read(&ex(&format!("golden/{n}/{f}")));
        let (c, s0, e) = probbit(&["persona", "init", &p], ""); assert_eq!(c, 0, "{e}"); assert_eq!(s0, g("state0.json"), "{n}.{form} state0");
        let st = tmp(&format!("{n}-{form}-state.json")); std::fs::write(&st, &s0).unwrap();
        let (c, prog, e) = probbit(&["persona", "compile", &p, "--state", &st, "--inputs", &jw(&turns[0])], ""); assert_eq!(c, 0, "{e}");
        assert_eq!(prog, g("program-turn0.json"), "{n}.{form} program of turn 0");
        let gold = g("workday.jsonl"); let lines: Vec<&str> = gold.lines().collect(); assert_eq!(lines.len(), 20);
        for (i, t) in turns.iter().enumerate() {
            let (c, out, e) = probbit(&["persona", "turn", &p, "--state", &st, "--inputs", &jw(t)], ""); assert_eq!(c, 0, "{e}");
            assert_eq!(out.trim_end(), lines[i], "{n}.{form} turn {i}"); }
        assert_eq!(read(&st), g("final-state.json"), "{n}.{form} final state");
        let (c, rep, e) = probbit(&["persona", "replay", &p, "--script", &ex("workday.json")], ""); assert_eq!(c, 0, "{e}"); assert_eq!(rep, gold, "{n}.{form} replay");
        assert!(e.contains(&format!("final state {}", s(&parse(&g("final-state.json")), &["digest"]).as_str().unwrap())), "{e}");
        let (_, lint, _) = probbit(&["persona", "lint", &p], ""); assert_eq!(jw(s(&parse(&lint), &["conflicts"])), jw(s(&parse(&g("lint.json")), &["conflicts"])), "{n} lint");
        let _ = std::fs::remove_file(&st);
    } }
    // the three individuals are different people: same script, different persona files
    let first: Vec<String> = NAMES.iter().map(|n| read(&ex(&format!("golden/{n}/workday.jsonl"))).lines().next().unwrap().to_string()).collect();
    assert!(first[0] != first[1] && first[1] != first[2]);
}

/// `probbit persona --help` documents every subcommand and the exit codes; bad flags and subcommands exit 2.
#[test]
fn persona_help_and_flags() {
    let (c, h, e) = probbit(&["persona", "--help"], ""); assert_eq!(c, 0, "{e}"); assert!(h.starts_with("usage: probbit persona"), "{h}");
    for w in ["init", "turn", "replay", "explain", "diff", "lint", "check", "compile", "describe", "--no-inertia", "--timing", "exit: 0"] { assert!(h.contains(w), "help lacks {w}"); }
    let p = ex("tutor.yaml");
    for args in [vec!["persona"], vec!["persona", "nope", &p], vec!["persona", "init"], vec!["persona", "init", &p, "--bogus"], vec!["persona", "turn", &p], vec!["persona", "init", &p, "--seed", "x"], vec!["persona", "init", &p, "--seed", "-1"]] {
        let (c, out, e) = probbit(&args, ""); assert_eq!(c, 2, "{args:?}: {out} {e}"); assert!(out.is_empty(), "{args:?}: a flag error goes to stderr"); }
    let (c, d, _) = probbit(&["persona", "describe", &p], ""); assert_eq!(c, 0);
    assert_eq!(s(&parse(&d), &["agenda"]).as_arr().unwrap().len(), 4); assert!(s(&parse(&d), &["traits", "emoji", "levels"]).as_arr().is_some());
    let (c, k, _) = probbit(&["persona", "check", &p], ""); assert_eq!(c, 0); let k = parse(&k);
    assert_eq!((s(&k, &["traits"]).as_f64(), s(&k, &["moods"]).as_f64(), s(&k, &["habits"]).as_f64()), (Some(9.0), Some(2.0), Some(7.0)));
    let (c, x, e) = probbit(&["persona", "explain", &p, "--script", &ex("workday.json"), "--turn", "4"], ""); assert_eq!(c, 0, "{e}");
    assert!(x.contains("turn 4") && x.contains("odds (exact, joint)") && x.contains("line (") && x.contains("why: "), "{x}");
    let (c, d, e) = probbit(&["persona", "diff", &p, "--seed", "1", "--seed2", "2", "--script", &ex("workday.json")], ""); assert_eq!(c, 0, "{e}");
    let tv = s(&parse(&d), &["distance", "tv"]).as_f64().unwrap(); assert!(tv > 0.0 && tv < 1.0, "{d}");
    let (_, same, _) = probbit(&["persona", "diff", &p, "--seed", "3", "--seed2", "3", "--script", &ex("workday.json")], "");
    assert_eq!(s(&parse(&same), &["distance", "tv"]).as_f64(), Some(0.0)); // the same individual: distance 0
}

/// The YAML subset reads what the reference reader (miniyaml.py) reads, to the same values, and refuses what it refuses, at the same
/// line. Two deliberate differences, both on inputs the reference got wrong, are pinned at the end.
#[test]
fn yaml_subset_reads_and_refuses_like_the_reference() {
    let ok: [(&str, &str); 15] = [
        ("a: 1\nb: [x, y]\n", r#"{"a":1,"b":["x","y"]}"#),
        ("- a\n- b: 2\n  c: 3\n- [1, 2]\n", r#"["a",{"b":2,"c":3},[1,2]]"#),
        ("key:\n  - x\n  - y\n", r#"{"key":["x","y"]}"#),
        ("key:\n- x\n- y\nnext: 1\n", r#"{"key":["x","y"],"next":1}"#),
        ("a: 'it''s'\nb: \"q\\\"x\\u00e9\"\n", r#"{"a":"it's","b":"q\"xé"}"#),
        ("a: ~\nb: null\nc: true\nd: FALSE\ne: -0.5e2\nf: +3\n", r#"{"a":null,"b":null,"c":true,"d":false,"e":-50,"f":3}"#),
        ("# comment\na: x # trailing\nb: 'a # not a comment'\nc: x#y\n", r##"{"a":"x","b":"a # not a comment","c":"x#y"}"##),
        ("---\na: {x: 1, w: [a, b], 'z': c, 'y': d}\n", r#"{"a":{"x":1,"w":["a","b"],"z":"c","y":"d"}}"#),
        ("\u{feff}a: 1\r\nb: 2\r\n", r#"{"a":1,"b":2}"#),
        ("a: [x, ]\n", r#"{"a":["x",null]}"#),
        ("a:\nb: 2\n", r#"{"a":null,"b":2}"#),
        ("say: [plain and businesslike, \"\", \"blunt: state it\"]\n", r#"{"say":["plain and businesslike","","blunt: state it"]}"#),
        ("a: 01x\nb: yes\n", r#"{"a":"01x","b":"yes"}"#),
        ("seq:\n  -\n    a: 1\n  - b\n", r#"{"seq":[{"a":1},"b"]}"#),
        ("[1, 2.5, x]\n", r#"[1,2.5,"x"]"#),
    ];
    for (src, want) in ok { match yaml::load(src) { Ok(v) => assert_eq!(jw(&v), jw(&parse(want)), "{src:?}"), Err(e) => panic!("{src:?} refused at line {}: {}", e.line, e.msg) } }
    let bad: [(&str, usize, &str); 21] = [
        ("a:\n\tb: 1\n", 2, "tab in indentation"), ("a: &x 1\n", 1, "anchor"), ("a: *x\n", 1, "alias"), ("a: !!str 1\n", 1, "tag"), ("a: |\n  text\n", 1, "block scalars"),
        ("a: [1,\n  2]\n", 1, "ends early"), ("a: 1\na: 2\n", 2, "duplicate key"), ("yes: 1\n", 1, "would not be a string"), ("1: x\n", 1, "would not be a string"),
        ("a: 1\n---\nb: 2\n", 2, "only one document"), ("a: 'x\n", 1, "unterminated"), ("a:\n  b: 1\n c: 2\n", 3, "bad indentation"), ("? a\n: b\n", 1, "expected `key: value`"),
        ("- a\nb: 1\n", 2, "unexpected text"), ("a: \"x\ty\"\n", 1, "bad double-quoted string"), ("a: [x, y\n", 1, "unterminated ["), ("a: {b: 1, b: 2}\n", 1, "duplicate key"),
        ("a: {on: 1}\n", 1, "would not be a string"), ("a: ]\n", 1, "unexpected ']'"), ("a: `x`\n", 1, "unexpected '`'"), ("- &a x\n", 1, "anchor"),
    ];
    for (src, line, msg) in bad { match yaml::load(src) { Ok(v) => panic!("{src:?} read as {}", jw(&v)), Err(e) => { assert_eq!(e.line, line, "{src:?}: {}", e.msg); assert!(e.msg.contains(msg), "{src:?}: {}", e.msg); } } }
    // differences from the reference reader: a quoted scalar is a sequence item (it refused it), and no number that is not exact
    // in a double is read (it read 1e999 as infinity and kept big integers exact, so digests would differ between implementations)
    assert_eq!(jw(&yaml::load("- \"x\"\n- 'y'\n").ok().unwrap()), r#"["x","y"]"#);
    for src in ["a: 1e999\n", "a: 12345678901234567890\n"] { assert!(yaml::load(src).is_err(), "{src:?}"); }
    // every example persona reads to the same document as its JSON form
    for n in NAMES { assert_eq!(jw(&yaml::load(&read(&ex(&format!("{n}.yaml")))).ok().unwrap()), jw(&parse(&read(&ex(&format!("{n}.json"))))), "{n}"); }
}

/// The standard inputs at random (fixed seed): sentiment, error, loss, praise, criticism, claim_done, stakes, time_pressure, and
/// the example personas' own (confused, task); every persona ignores what it does not declare.
fn random_script(r: &mut Mix, n: usize) -> Vec<Json> {
    (0..n).map(|_| { let mut t: Vec<(String, Json)> = vec![];
        let u = r.unit(); if u < 0.2 { t.push(("sentiment".into(), Json::Str("negative".into()))); } else if u > 0.8 { t.push(("sentiment".into(), Json::Str("positive".into()))); }
        for (k, p) in [("error", 0.15), ("loss", 0.03), ("praise", 0.1), ("criticism", 0.07), ("claim_done", 0.1), ("confused", 0.1)] { if r.unit() < p { t.push((k.into(), Json::Bool(true))); } }
        let x = (r.unit().powi(2) * 100.0).round() / 100.0; if x >= 0.05 { t.push(("stakes".into(), Json::Num(x))); }
        let x = (r.unit().powi(3) * 100.0).round() / 100.0; if x >= 0.05 { t.push(("time_pressure".into(), Json::Num(x))); }
        if r.unit() < 0.3 { t.push(("task".into(), Json::Str(["chat", "question", "code", "ops", "explain"][r.below(5)].into()))); }
        if r.unit() < 0.03 { t.push(("elapsed_hours".into(), Json::Num((r.unit() * 1000.0).round() / 100.0))); }
        Json::Obj(t) }).collect()
}

/// A 1,000-turn random script replays byte for byte: twice in two processes, and through `turn --state FILE` one process per turn
/// (the first 60 turns), for every example persona and two seeds.
#[test]
fn a_1000_turn_replay_is_byte_identical() {
    let mut r = Mix(20_261_002); let script = Json::Arr(random_script(&mut r, 1000)); let sp = tmp("long1000.json"); std::fs::write(&sp, jw(&script)).unwrap();
    let turns = script.as_arr().unwrap();
    for n in NAMES { for seed in ["1", "7"] {
        let p = ex(&format!("{n}.yaml"));
        let (c, a, e) = probbit(&["persona", "replay", &p, "--seed", seed, "--script", &sp], ""); assert_eq!(c, 0, "{e}");
        let (_, b, e2) = probbit(&["persona", "replay", &p, "--seed", seed, "--script", &sp], "");
        assert_eq!(a.lines().count(), 1000); assert!(a == b && e == e2, "{n} seed {seed}: two replays differ");
        let st = tmp(&format!("{n}-{seed}-long.json")); let (_, s0, _) = probbit(&["persona", "init", &p, "--seed", seed], ""); std::fs::write(&st, s0).unwrap();
        for (i, (t, line)) in turns.iter().zip(a.lines()).take(60).enumerate() {
            let (c, out, e) = probbit(&["persona", "turn", &p, "--state", &st, "--inputs", &jw(t)], ""); assert_eq!(c, 0, "{e}"); assert_eq!(out.trim_end(), line, "{n} seed {seed} turn {i}"); }
        let _ = std::fs::remove_file(&st);
    } }
    let _ = std::fs::remove_file(&sp);
}

/// A restriction of a `then` habit -> the allowed levels (docs/persona.md §2.5), independently of the implementation
fn allowed(levels: &[String], r: &Json, prev: &str) -> Vec<String> {
    let idx = |l: &str| -> usize { if let Some(i) = levels.iter().position(|x| x == l) { return i; }
        let p = levels.iter().position(|x| x == prev).unwrap() as i64; (p + match l { "prev-1" => -1, "prev+1" => 1, _ => 0 }).clamp(0, levels.len() as i64 - 1) as usize };
    match r { Json::Arr(a) => levels.iter().filter(|l| a.iter().any(|x| x.as_str() == Some(l.as_str()))).cloned().collect(),
        _ => { let (k, x) = &r.as_obj().unwrap()[0];
            match k.as_str() { "not" => levels.iter().filter(|l| !x.as_arr().unwrap().iter().any(|y| y.as_str() == Some(l.as_str()))).cloned().collect(),
                "at_most" => levels[..=idx(x.as_str().unwrap())].to_vec(), _ => levels[idx(x.as_str().unwrap())..].to_vec() } } }
}
/// A random persona (JSON): 3-5 traits, maybe a mood, couplings, a flag, a level and a number input, maybe a streak, 2-4 conditional
/// habits restricting traits (level lists, `not`, `at_most` / `at_least` with levels and prev, prev-1, prev+1), conflicts resolved by
/// fallback or yield. Every trait's fallback is its first level.
/// (trait id, its levels) and (habit id, its `then` restrictions)
type Levels = Vec<(String, Vec<String>)>;
type Thens = Vec<(String, Vec<(String, Json)>)>;
fn random_persona(r: &mut Mix, k: usize) -> (Json, Levels, Thens) {
    let names = ["low", "mid", "high", "max"]; let num = |x: f64| Json::Num((x * 100.0).round() / 100.0); let st = |x: &str| Json::Str(x.into());
    let nt = 3 + r.below(3); let mut traits = vec![]; let mut levels = vec![];
    for t in 0..nt { let n = 2 + r.below(3); let lv: Vec<String> = names[..n].iter().map(|s| s.to_string()).collect();
        traits.push(Json::Obj(vec![("id".into(), st(&format!("t{t}"))), ("levels".into(), Json::Arr(lv.iter().map(|l| st(l)).collect())),
            ("prior".into(), Json::Arr((0..n).map(|_| Json::Num((1 + r.below(9)) as f64)).collect())), ("spread".into(), num(r.below(6) as f64 * 0.1)),
            ("say".into(), Json::Arr(lv.iter().map(|l| st(&format!("t{t} {l}"))).collect())), ("fallback".into(), st(&lv[0]))]));
        levels.push((format!("t{t}"), lv)); }
    let mood = r.below(2) == 0; let mut ids: Vec<String> = levels.iter().map(|x| x.0.clone()).collect(); if mood { ids.push("m0".into()); }
    let target = |r: &mut Mix| ids[r.below(ids.len())].clone();
    let eff = |r: &mut Mix| Json::Obj((0..1 + r.below(3)).map(|_| (target(r), num(r.unit() * 3.0 - 1.5))).fold(vec![], |mut v: Vec<(String, Json)>, (k, x)| { if !v.iter().any(|y| y.0 == k) { v.push((k, x)); } v }));
    let mut couplings = vec![]; for _ in 0..1 + r.below(3) { let (a, b) = (target(r), target(r)); if a != b { couplings.push(Json::Obj(vec![("vars".into(), Json::Arr(vec![st(&a), st(&b)])), ("align".into(), num(r.unit() * 2.0 - 1.0))])); } }
    let inputs = vec![Json::Obj(vec![("id".into(), st("f0")), ("kind".into(), st("flag")), ("effects".into(), eff(r))]),
        Json::Obj(vec![("id".into(), st("lv")), ("kind".into(), st("level")), ("levels".into(), Json::Arr(vec![st("neg"), st("neu"), st("pos")])), ("default".into(), st("neu")), ("effects".into(), Json::Obj(vec![("neg".into(), eff(r)), ("pos".into(), eff(r))]))]),
        Json::Obj(vec![("id".into(), st("n0")), ("kind".into(), st("number")), ("reactivity_spread".into(), num(0.3)), ("effects".into(), eff(r))])];
    let streak = r.below(2) == 0;
    let whens = [("f0", Json::Bool(true)), ("lv", st("neg")), ("n0", num(0.5)), ("streak", Json::Num(2.0))];
    let mut habits = vec![]; let mut hdefs = vec![];
    for h in 0..2 + r.below(3) { let (wk, wv) = whens[r.below(if streak { 4 } else { 3 })].clone();
        let mut then: Vec<(String, Json)> = vec![];
        for _ in 0..1 + r.below(2) { let (t, lv) = levels[r.below(nt)].clone(); if then.iter().any(|x| x.0 == t) { continue; }
            let restr = match r.below(5) { 0 => Json::Arr(vec![st(&lv[r.below(lv.len())])]), 1 => Json::Obj(vec![("not".into(), Json::Arr(vec![st(&lv[r.below(lv.len())])]))]),
                2 => Json::Obj(vec![("at_most".into(), st(["prev", "prev-1", &lv[r.below(lv.len())]][r.below(3)]))]),
                3 => Json::Obj(vec![("at_least".into(), st(["prev", "prev+1", &lv[r.below(lv.len())]][r.below(3)]))]),
                _ => Json::Arr(vec![st(&lv[0]), st(&lv[lv.len() - 1])]) };
            then.push((t, restr)); }
        hdefs.push((format!("h{h}"), then.clone()));
        habits.push(Json::Obj(vec![("id".into(), st(&format!("h{h}"))), ("when".into(), Json::Obj(vec![(wk.into(), wv)])), ("then".into(), Json::Obj(then)),
            ("say".into(), st(&format!("habit {h}"))), ("priority".into(), Json::Num(r.below(3) as f64))])); }
    let mut doc = vec![("probbit_persona".to_string(), Json::Num(1.0)), ("identity".into(), Json::Obj(vec![("name".into(), st(&format!("Random {k}"))), ("version".into(), st("1")), ("seed".into(), Json::Num(1.0))])),
        ("traits".into(), Json::Arr(traits))];
    if mood { doc.push(("moods".into(), Json::Arr(vec![Json::Obj(vec![("id".into(), st("m0")), ("levels".into(), Json::Arr(vec![st("down"), st("even"), st("up")])), ("inertia".into(), num(0.5))])]))); }
    doc.push(("couplings".into(), Json::Arr(couplings))); doc.push(("inputs".into(), Json::Arr(inputs)));
    if streak { doc.push(("history".into(), Json::Arr(vec![Json::Obj(vec![("id".into(), st("streak")), ("of".into(), st("f0")), ("kind".into(), st("streak")), ("cap".into(), Json::Num(3.0)), ("effects".into(), eff(r))])]))); }
    doc.push(("habits".into(), Json::Arr(habits)));
    doc.push(("engine".into(), Json::Obj(vec![("on_conflict".into(), st(if r.below(2) == 0 { "yield" } else { "fallback" }))])));
    (Json::Obj(doc), levels, hdefs)
}
fn with(doc: &Json, k: &str, v: Json) -> Json { let Json::Obj(o) = doc else { panic!() }; let mut o = o.clone(); o.retain(|(x, _)| x != k); o.push((k.into(), v)); Json::Obj(o) }

/// Habit properties over random personas and random inputs (12 personas x 2 seeds x 90 turns = 2,160 turns): whenever there is a
/// stance, every habit in force holds (`violations` 0, and re-checked here from the persona's own rules and the previous turn's
/// levels); every habit reported `bound` is broken by the habit-free twin (the same persona without habits: its unaries do not
/// depend on earlier stances, so its plan is the twin's); `values: positional` and `values: semantic` give the same stances.
#[test]
fn habits_hold_bound_habits_bind_and_value_alphabets_agree() {
    let (mut turns, mut bound_seen, mut checked) = (0, 0, 0); let mut r = Mix(0xBEEF);
    for k in 0..12 {
        let (doc, levels, hdefs) = random_persona(&mut r, k);
        let lvl = |t: &str| &levels.iter().find(|x| x.0 == t).unwrap().1;
        let sem = with(&doc, "engine", Json::Obj(vec![("on_conflict".into(), s(&doc, &["engine", "on_conflict"]).clone()), ("values".into(), Json::Str("semantic".into()))]));
        let free = with(&with(&doc, "habits", Json::Arr(vec![])), "engine", Json::Obj(vec![("twin".into(), Json::Bool(false))]));
        let script = Json::Arr((0..90).map(|_| { let mut t = vec![]; if r.unit() < 0.35 { t.push(("f0".to_string(), Json::Bool(true))); }
            if r.unit() < 0.5 { t.push(("lv".into(), Json::Str(["neg", "neu", "pos"][r.below(3)].into()))); }
            if r.unit() < 0.6 { t.push(("n0".into(), Json::Num((r.unit() * 100.0).round() / 100.0))); }
            Json::Obj(t) }).collect());
        let (pp, ps, pf, sp) = (tmp(&format!("rand{k}.json")), tmp(&format!("rand{k}-sem.json")), tmp(&format!("rand{k}-free.json")), tmp(&format!("rand{k}-script.json")));
        for (f, d) in [(&pp, &doc), (&ps, &sem), (&pf, &free), (&sp, &script)] { std::fs::write(f, jw(d)).unwrap(); }
        for seed in ["1", "2"] {
            let run = |p: &str| -> Vec<Json> { let (c, out, e) = probbit(&["persona", "replay", p, "--seed", seed, "--script", &sp], ""); assert_eq!(c, 0, "{e} {}", jw(&doc)); out.lines().map(parse).collect() };
            let (main, semantic, twin) = (run(&pp), run(&ps), run(&pf));
            let mut prev: Vec<(String, String)> = levels.iter().map(|(t, lv)| (t.clone(), lv[0].clone())).collect();
            for (i, ((a, b), f)) in main.iter().zip(&semantic).zip(&twin).enumerate() {
                turns += 1; let status = s(a, &["status"]).as_str().unwrap();
                // positional == semantic (the persona digest, the program and the state digest name the variant)
                let strip = |x: &Json| { let Json::Obj(o) = x else { panic!() }; Json::Obj(o.iter().filter(|(k, _)| !["persona", "engine", "state_digest"].contains(&k.as_str())).cloned().collect()) };
                assert_eq!(jw(&strip(a)), jw(&strip(b)), "persona {k} seed {seed} turn {i}: positional != semantic");
                let level = |x: &Json, t: &str| s(x, &["stance", t, "level"]).as_str().unwrap().to_string();
                let pv = |t: &str| prev.iter().find(|x| x.0 == t).unwrap().1.clone();
                if status != "fallback" {
                    assert_eq!(s(a, &["habits", "violations"]).as_f64(), Some(0.0), "persona {k} turn {i}");
                    for h in s(a, &["habits", "active"]).as_arr().unwrap() { let then = &hdefs.iter().find(|x| x.0 == h.as_str().unwrap()).unwrap().1;
                        for (t, restr) in then { checked += 1; assert!(allowed(lvl(t), restr, &pv(t)).contains(&level(a, t)), "persona {k} turn {i}: habit {} broken on {t}", h.as_str().unwrap()); } }
                    for h in s(a, &["habits", "bound"]).as_arr().unwrap() { bound_seen += 1; let then = &hdefs.iter().find(|x| x.0 == h.as_str().unwrap()).unwrap().1;
                        assert_eq!(s(f, &["status"]).as_str(), Some("ok"));
                        assert!(then.iter().any(|(t, restr)| !allowed(lvl(t), restr, &pv(t)).contains(&level(f, t))), "persona {k} turn {i}: bound habit {} holds on the twin", h.as_str().unwrap()); }
                }
                prev = levels.iter().map(|(t, _)| (t.clone(), level(a, t))).collect();
            }
        }
        for f in [&pp, &ps, &pf, &sp] { let _ = std::fs::remove_file(f); }
    }
    eprintln!("habit properties: {turns} turns, {checked} restrictions re-checked, {bound_seen} bound habits broken by the twin");
    assert!(turns >= 2000 && bound_seen > 50 && checked > 1000, "{turns} {bound_seen} {checked}");
}

/// `lint` passes clean personas and finds planted contradictions: statically (`lint`), at load (an unconditional habit the fallback
/// stance breaks) and at run time (`fallback` with the conflicting habits named, or the lower-priority habit yielding).
#[test]
fn lint_finds_planted_contradictions_and_passes_clean_personas() {
    for (n, conflicts) in [("ops-engineer", 0), ("trader-assistant", 0), ("tutor", 1)] {
        let (c, out, e) = probbit(&["persona", "lint", &ex(&format!("{n}.yaml"))], ""); assert_eq!(c, 0, "{e}"); let l = parse(&out);
        assert_eq!(s(&l, &["conflicts"]).as_arr().unwrap().len(), conflicts, "{n}"); assert_eq!(s(&l, &["ok"]), &Json::Bool(true)); }
    let base = parse(&read(&ex("ops-engineer.json")));
    let habits = |extra: &str| { let mut h = s(&base, &["habits"]).as_arr().unwrap().to_vec(); h.push(parse(extra)); with(&base, "habits", Json::Arr(h)) };
    let planted = habits(r#"{"id": "planted_long", "when": {"stakes": 0.5}, "then": {"verbosity": ["full"]}, "say": "at stakes, full detail"}"#);
    let pf = tmp("planted.json"); std::fs::write(&pf, jw(&planted)).unwrap();
    let (c, out, _) = probbit(&["persona", "lint", &pf], ""); assert_eq!(c, 1, "{out}"); let l = parse(&out);
    let f = &s(&l, &["conflicts"]).as_arr().unwrap()[0];
    assert_eq!(jw(s(f, &["habits"])), r#"["after_error_shorter","planted_long"]"#); assert_eq!(s(f, &["when_prev"]).as_arr().unwrap().len(), 9); // every previous caution x verbosity
    assert!(s(f, &["resolution"]).as_str().unwrap().starts_with("fallback"));
    // at run time: both in force -> no stance, the conflict named, the line without them
    let st = tmp("planted-state.json"); let (_, s0, _) = probbit(&["persona", "init", &pf], ""); std::fs::write(&st, s0).unwrap();
    let (c, out, e) = probbit(&["persona", "turn", &pf, "--state", &st, "--inputs", r#"{"error": true, "stakes": 0.8}"#], ""); assert_eq!(c, 0, "{e}"); let t = parse(&out);
    assert_eq!(s(&t, &["status"]).as_str(), Some("fallback")); assert_eq!(jw(s(&t, &["habits", "conflict"])), r#"["after_error_shorter","planted_long"]"#);
    assert!(s(&t, &["escalate"]).as_str().unwrap().contains("habits conflict here")); let line = s(&t, &["line"]).as_str().unwrap();
    assert!(!line.contains("at stakes, full detail") && !line.contains("shorter than last time"), "{line}");
    // on_conflict yield: resolved by priority (a tie: the later-declared habit yields), and the turn has a stance again
    let yielding = with(&planted, "engine", parse(r#"{"on_conflict": "yield"}"#)); std::fs::write(&pf, jw(&yielding)).unwrap();
    let (c, out, _) = probbit(&["persona", "lint", &pf], ""); assert_eq!(c, 0, "{out}");
    assert_eq!(s(&s(&parse(&out), &["conflicts"]).as_arr().unwrap()[0], &["resolution"]).as_str(), Some("yield: planted_long yields (priority tie: the later-declared habit yields)"));
    let (_, s0, _) = probbit(&["persona", "init", &pf], ""); std::fs::write(&st, s0).unwrap();
    let (_, out, _) = probbit(&["persona", "turn", &pf, "--state", &st, "--inputs", r#"{"error": true, "stakes": 0.8}"#], ""); let t = parse(&out);
    assert_eq!(s(&t, &["status"]).as_str(), Some("ok")); assert_eq!(jw(s(&t, &["habits", "yielded"])), r#"["planted_long"]"#); assert_eq!(s(&t, &["habits", "violations"]).as_f64(), Some(0.0));
    // an unconditional habit the fallback stance breaks: refused when the persona is read
    std::fs::write(&pf, jw(&habits(r#"{"id": "always_full", "then": {"verbosity": ["full"]}}"#))).unwrap();
    let (c, out, _) = probbit(&["persona", "check", &pf], ""); assert_eq!(c, 2);
    assert_eq!(jw(s(&parse(&out), &["error"])), r#"{"code":"persona","path":"traits","message":"the fallback stance breaks unconditional habit(s): always_full"}"#);
    for f in [&pf, &st] { let _ = std::fs::remove_file(f); }
}

/// Refusals map to the stance: forced onto the sampler at 8 sweeps (`engine.op: sample`), the gate vouches for nothing; every turn
/// is then `refused` (or `fallback` when no chain started) with `escalate` naming what is unvouched, and the line carries only the
/// habits in force (always safe to state): no unvouched trait's phrase. At the default (exact) the same turns are `ok`.
#[test]
fn refusals_leave_unvouched_traits_out_of_the_line() {
    let doc = parse(&read(&ex("tutor.json"))); let says: Vec<String> = s(&doc, &["traits"]).as_arr().unwrap().iter().flat_map(|t| s(t, &["say"]).as_arr().unwrap().iter().filter_map(|x| x.as_str().map(str::to_string)).filter(|x| !x.is_empty()).collect::<Vec<_>>()).collect();
    let habit_says: Vec<String> = s(&doc, &["habits"]).as_arr().unwrap().iter().filter_map(|h| h.get("say").and_then(Json::as_str).map(str::to_string)).collect();
    let f = tmp("sample8.json"); std::fs::write(&f, jw(&with(&doc, "engine", parse(r#"{"op": "sample", "sweeps": 8, "on_conflict": "yield"}"#)))).unwrap();
    let (c, out, e) = probbit(&["persona", "replay", &f, "--script", &ex("workday.json")], ""); assert_eq!(c, 0, "{e}");
    let mut refused = 0;
    for (i, l) in out.lines().enumerate() { let t = parse(l); let st = s(&t, &["status"]).as_str().unwrap(); assert!(["refused", "partial", "fallback"].contains(&st), "turn {i}: {st}");
        let line = s(&t, &["line"]).as_str().unwrap(); let esc = s(&t, &["escalate"]).as_str().unwrap();
        if st == "refused" { refused += 1; assert!(esc.starts_with("engine refused: unvouched: "), "{esc}");
            let items: Vec<&str> = line.trim_start_matches("Stance: ").trim_end_matches('.').split("; ").collect();
            assert!(line == "Stance: neutral." || items.iter().all(|x| habit_says.iter().any(|h| h == x)), "turn {i}: {line}");
            assert!(!says.iter().any(|x| items.contains(&x.as_str())), "turn {i}: a trait phrase in a refused line: {line}"); }
        for (k, e) in s(&t, &["stance"]).as_obj().unwrap() { if e.get("released") == Some(&Json::Bool(false)) && st == "partial" {
            let v = s(&doc, &["traits"]).as_arr().unwrap().iter().find(|x| s(x, &["id"]).as_str() == Some(k)).unwrap();
            let lv = s(e, &["level"]).as_str().unwrap(); let i = s(v, &["levels"]).as_arr().unwrap().iter().position(|x| x.as_str() == Some(lv)).unwrap();
            let phrase = s(v, &["say"]).as_arr().unwrap()[i].as_str().unwrap(); assert!(phrase.is_empty() || !line.contains(phrase), "unvouched {k} in {line}"); } } }
    assert!(refused >= 10, "{refused}");
    let (_, exact, _) = probbit(&["persona", "replay", &ex("tutor.yaml"), "--script", &ex("workday.json")], "");
    assert!(exact.lines().all(|l| s(&parse(l), &["status"]).as_str() == Some("ok")));
    let _ = std::fs::remove_file(&f);
}

/// The `evaluate` bridge (docs/persona.md §8): a turn's program as a System One request, one `choice` question per variable, the
/// persona's own field as `probbit.logw` (judge weight 0) and its pairs and habits as `probbit.rules`. `probbit evaluate` then gives
/// the stance the `run` path gave, level and odds (within 2e-6), on every workday turn of the three example personas.
#[test]
fn the_evaluate_bridge_at_judge_weight_0_equals_the_run_path() {
    let mut same = 0;
    for n in NAMES {
        let p = ex(&format!("{n}.yaml")); let st = tmp(&format!("{n}-bridge.json")); let (_, s0, _) = probbit(&["persona", "init", &p], ""); std::fs::write(&st, s0).unwrap();
        let levels: Vec<(String, Vec<String>)> = { let d = parse(&probbit(&["persona", "describe", &p], "").1);
            s(&d, &["traits"]).as_obj().unwrap().iter().map(|(k, v)| (k.clone(), s(v, &["levels"]).as_arr().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect())).collect() };
        for (i, t) in workday().iter().enumerate() {
            let (_, prog, _) = probbit(&["persona", "compile", &p, "--state", &st, "--inputs", &jw(t)], ""); let prog = parse(&prog);
            let (c, out, e) = probbit(&["persona", "turn", &p, "--state", &st, "--inputs", &jw(t)], ""); assert_eq!(c, 0, "{e}"); let stance = parse(&out);
            let main = |j: &Json| !jw(j).contains("\"free.");
            let vars: Vec<&Json> = s(&prog, &["vars"]).as_arr().unwrap().iter().filter(|v| main(v)).collect();
            let questions: Vec<(String, Json)> = vars.iter().map(|v| (s(v, &["id"]).as_str().unwrap().to_string(), Json::Obj(vec![("type".into(), Json::Str("choice".into())),
                ("criteria".into(), Json::Obj(s(v, &["allowed"]).as_arr().unwrap().iter().map(|a| (a.as_str().unwrap().to_string(), a.clone())).collect()))]))).collect();
            let logw: Vec<(String, Json)> = vars.iter().map(|v| (s(v, &["id"]).as_str().unwrap().to_string(), s(v, &["h"]).clone())).collect();
            let rules: Vec<(String, Json)> = ["pairs", "tables", "precedes", "implies", "linear", "caps", "all_different"].iter().filter_map(|k| prog.get(k).and_then(Json::as_arr)
                .map(|l| (k.to_string(), Json::Arr(l.iter().filter(|r| main(r)).cloned().collect())))).filter(|(_, l)| !l.as_arr().unwrap().is_empty()).collect();
            let req = Json::Obj(vec![("model".into(), Json::Str("judge+persona".into())), ("questions".into(), Json::Obj(questions)),
                ("probbit".into(), Json::Obj(vec![("logw".into(), Json::Obj(logw)), ("rules".into(), Json::Obj(rules))]))]);
            let (c, out, e) = probbit(&["evaluate"], &jw(&req)); assert_eq!(c, 0, "{n} turn {i}: {e} {out:.300}"); let ev = parse(&out);
            for (tr, lv) in &levels {
                let a = s(&ev, &["answers", tr]); let idx = |v: &str| v[1..].parse::<usize>().unwrap();
                assert_eq!(lv[idx(s(a, &["choice"]).as_str().unwrap())], s(&stance, &["stance", tr, "level"]).as_str().unwrap(), "{n} turn {i} {tr}");
                for (o, x) in s(a, &["probabilities"]).as_obj().unwrap() { let want = s(&stance, &["stance", tr, "odds", &lv[idx(o)]]).as_f64().unwrap();
                    assert!((x.as_f64().unwrap() - want).abs() < 2e-6, "{n} turn {i} {tr} {o}: {} vs {want}", jw(x)); }
                same += 1; }
        }
        let _ = std::fs::remove_file(&st);
    }
    assert_eq!(same, 20 * (8 + 9 + 8));
}

/// Malformed persona documents (byte edits, inserted tabs / anchors / quotes / brackets, truncation, swapped or duplicated lines of
/// the example files, in both forms), states and inputs: never a crash; exit 0 (still a valid persona) or exit 2 with exactly one
/// `{"error": {"code": "persona", "path", "message"}}` object on stdout.
#[test]
fn malformed_documents_are_one_error_object_never_a_crash() {
    let mut r = Mix(0xF022); let (mut ok, mut err) = (0, 0); let f = tmp("fuzz-persona");
    for case in 0..600 {
        let n = NAMES[r.below(3)]; let json_form = r.below(3) == 0; let mut b = read(&ex(&format!("{n}.{}", if json_form { "json" } else { "yaml" }))).into_bytes();
        for _ in 0..1 + r.below(3) { let p = r.below(b.len());
            match r.below(7) {
                0 => { b.remove(p); }
                1 => { const INS: [&str; 16] = ["\t", "&a ", "*a", "!t ", "| ", "[", "{", ":", "- ", "#", "\"", "'", " ", "\n", "1e999", "yes: 1\n"]; let x = INS[r.below(INS.len())]; b.splice(p..p, x.bytes()); }
                2 => b.truncate(p),
                3 => { let q = (p + 1 + r.below(40)).min(b.len()); let s = b[p..q].to_vec(); b.splice(q..q, s); }
                4 => { let mut lines: Vec<String> = String::from_utf8_lossy(&b).lines().map(str::to_string).collect(); let (i, j) = (r.below(lines.len()), r.below(lines.len())); lines.swap(i, j); b = lines.join("\n").into_bytes(); }
                5 => { const W: [&str; 6] = ["-1", "99", "true", "null", "\"x\"", "[]"]; let s = String::from_utf8_lossy(&b).to_string(); let digits: Vec<usize> = s.char_indices().filter(|c| c.1.is_ascii_digit()).map(|c| c.0).collect();
                    if let Some(&d) = digits.get(r.below(digits.len().max(1))) { let mut t = s.clone(); t.replace_range(d..d + 1, W[r.below(W.len())]); b = t.into_bytes(); } }
                _ => { let s = String::from_utf8_lossy(&b).to_string(); b = s.replacen(["levels", "prior", "when", "then", "kind", "effects"][r.below(6)], ["level", "priors", "If", "than", "type", "effect"][r.below(6)], 1).into_bytes(); }
            } }
        let path = format!("{f}-{case}.{}", if json_form { "json" } else { "yaml" }); std::fs::write(&path, &b).unwrap();
        let (c, out, e) = probbit(&["persona", "check", &path], "");
        match c { 0 => { ok += 1; assert_eq!(s(&parse(&out), &["ok"]), &Json::Bool(true)); }
            2 => { err += 1; assert_eq!(out.lines().count(), 1, "case {case}: {out}"); let d = parse(&out); let Json::Obj(o) = &d else { panic!() }; assert_eq!(o.len(), 1, "case {case}");
                assert_eq!(s(&d, &["error", "code"]).as_str(), Some("persona"), "case {case}: {out}"); assert!(s(&d, &["error", "path"]).as_str().is_some() && s(&d, &["error", "message"]).as_str().is_some()); }
            _ => panic!("case {case}: exit {c}\n{e}\n{}", String::from_utf8_lossy(&b)) }
        let _ = std::fs::remove_file(&path);
    }
    eprintln!("malformed personas: {ok} still valid, {err} one error object each");
    assert!(ok > 30 && err > 300, "{ok} {err}");
    // states and inputs
    let p = ex("tutor.yaml"); let st = tmp("fuzz-state.json"); let (_, s0, _) = probbit(&["persona", "init", &p], ""); let s0j = parse(&s0);
    for (state, inputs, path) in [(s0.clone(), r#"{"stakes": "high"}"#, "inputs.stakes"), (s0.clone(), r#"{"sentiment": "angry"}"#, "inputs.sentiment"), (s0.clone(), "[1]", "inputs"),
        (s0.clone(), "{\"loss\": 1}", "inputs.loss"), (s0.clone(), "{not json", "inputs"), (s0.replace("\"turn\":0", "\"turn\":5"), "{}", "state.digest"),
        ("[]".into(), "{}", "state"), ("{\"probbit_persona_state\": 2}".into(), "{}", "state"), (jw(&with(&s0j, "seed", Json::Num(9.0))), "{}", "state.digest")] {
        std::fs::write(&st, &state).unwrap(); let (c, out, e) = probbit(&["persona", "turn", &p, "--state", &st, "--inputs", inputs], "");
        assert_eq!(c, 2, "{state:.80} {inputs}: {out} {e}"); assert_eq!(s(&parse(&out), &["error", "path"]).as_str(), Some(path), "{inputs}: {out}");
        assert_eq!(read(&st), state, "a refused turn must not write the state"); }
    let _ = std::fs::remove_file(&st);
}

/// The Python wrapper's persona functions (python/test_persona.py), on Python >= 3.9, stdlib only
#[test]
fn python_persona_tests_pass() {
    let ok = Command::new("python3").args(["-c", "import sys; sys.exit(0 if sys.version_info >= (3, 9) else 1)"]).output().is_ok_and(|o| o.status.success());
    if !ok { eprintln!("python3 >= 3.9 not found: python/test_persona.py skipped"); return; }
    let o = Command::new("python3").arg("test_persona.py").current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../python")).env("PROBBIT_BIN", env!("CARGO_BIN_EXE_probbit")).output().unwrap();
    assert!(o.status.success(), "python/test_persona.py failed:\n{}", String::from_utf8_lossy(&o.stderr));
}

// ------------------------------------------------------------------------------------- persona fuzz (docs/persona.md §5.6)
/// The command run from the repository root, so every path it prints is the same on every OS
fn probbit_at_root(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).current_dir(format!("{}/..", env!("CARGO_MANIFEST_DIR"))).stdin(Stdio::null()).output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
/// The tutor as it shipped in 0.5.0, byte for byte: the counterexample stays reproducible whatever happens to the example
const TUTOR_050: &str = "probbit-cli/tests/fixtures/persona/tutor-0.5.0.yaml";
const TUTOR_050_DIGEST: &str = "sha256:1e9c6697e3d07ba8ed24110f8203d17f41d7a56d2ebd93b73c36f58105441740";
const UPSET_NOT_PLAYFUL: &str = "{when: {sentiment: negative}, then: {humour: {at_most: light}}}";

/// The golden: on the 0.5.0 tutor, "never playful with an upset learner" breaks; the shortest counterexample is seed 1's single
/// event {"sentiment":"negative"} (humour playful with odds 0.411, although light has 0.425); the human report is pinned byte for
/// byte, exit 1; the JSON document says the same; the printed replay command reproduces the breaking stance.
#[test]
fn fuzz_finds_the_0_5_0_tutor_counterexample() {
    let (_, k, _) = probbit_at_root(&["persona", "check", TUTOR_050]); assert_eq!(s(&parse(&k), &["digest"]).as_str(), Some(TUTOR_050_DIGEST), "the fixture is the 0.5.0 tutor");
    let (c, out, e) = probbit_at_root(&["persona", "fuzz", TUTOR_050, "--seeds", "0-99", "--never", UPSET_NOT_PLAYFUL]);
    assert_eq!(c, 1, "{e}"); assert!(e.contains("turns/s"), "{e}");
    assert_eq!(out, read(&format!("{}/tests/fixtures/persona/fuzz-tutor-0.5.0.txt", env!("CARGO_MANIFEST_DIR"))));
    let (c, j, e) = probbit_at_root(&["persona", "fuzz", TUTOR_050, "--seeds", "0-99", "--never", UPSET_NOT_PLAYFUL, "--json"]); assert_eq!(c, 1, "{e}");
    let j = parse(&j); assert_eq!(s(&j, &["probbit_persona_fuzz"]).as_f64(), Some(1.0)); assert_eq!(s(&j, &["found"]), &Json::Bool(true));
    let pr = &s(&j, &["properties"]).as_arr().unwrap()[0];
    assert_eq!((s(pr, &["verdict"]).as_str(), s(pr, &["shortest", "seed"]).as_f64(), jw(s(pr, &["shortest", "script"]))), (Some("counterexample"), Some(1.0), r#"[{"sentiment":"negative"}]"#.to_string()));
    assert_eq!(jw(s(pr, &["shortest", "broken"])), r#"[{"allowed":["none","light"],"level":"playful","var":"humour"}]"#);
    let replay = s(pr, &["shortest", "replay"]).as_str().unwrap().to_string();
    assert_eq!(replay, format!("probbit persona replay {TUTOR_050} --seed 1 --script '[{{\"sentiment\":\"negative\"}}]'"));
    let (c, rep, e) = probbit_at_root(&["persona", "replay", TUTOR_050, "--seed", "1", "--script", r#"[{"sentiment":"negative"}]"#]); assert_eq!(c, 0, "{e}");
    assert_eq!(&parse(rep.trim_end()), s(pr, &["shortest", "stance"]), "the replayed stance is the reported one");
    assert_eq!(s(&parse(rep.trim_end()), &["stance", "humour", "level"]).as_str(), Some("playful"));
}

/// Determinism: the same inputs give byte-identical output whatever the number of search threads (and again on a second run);
/// another --fuzz-seed searches other scripts. Hard properties never break: the tutor's habits as rules, and on 8 random personas
/// (conflicts resolved by fallback, so no habit ever yields) every habit as a rule, over 10 individuals each.
#[test]
fn fuzz_is_deterministic_and_never_breaks_a_habit() {
    let base = ["persona", "fuzz", TUTOR_050, "--seeds", "0-24", "--scripts", "15", "--never", UPSET_NOT_PLAYFUL, "--json"];
    let run = |extra: &[&str]| { let a: Vec<&str> = base.iter().chain(extra).copied().collect(); probbit_at_root(&a) };
    let (c1, a, _) = run(&["--threads", "1"]); let (c2, b, _) = run(&["--threads", "5"]); let (_, a2, _) = run(&["--threads", "1"]);
    assert_eq!((c1, c2), (1, 1)); assert!(a == b && a == a2, "fuzz output depends on the threads or the run");
    let (_, other, _) = run(&["--fuzz-seed", "9"]); assert_ne!(a, other, "--fuzz-seed changes the random scripts");
    for rule in ["{when: {loss: true}, then: {humour: [none], emoji: {at_most: sparse}}}", "{when: {confused: true}, then: {humour: {at_most: light}, verbosity: {at_least: short}}}"] {
        let (c, out, e) = probbit_at_root(&["persona", "fuzz", TUTOR_050, "--seeds", "0-19", "--scripts", "20", "--never", rule]); assert_eq!(c, 0, "{rule}: {out} {e}");
        assert!(out.contains("none found  0 of 20 individuals broke it"), "{out}"); }
    let mut r = Mix(0xF022);
    for k in 0..8 {
        let (doc, _, hdefs) = random_persona(&mut r, k);
        let doc = with(&doc, "engine", Json::Obj(vec![("on_conflict".into(), Json::Str("fallback".into()))]));
        let habits = s(&doc, &["habits"]).as_arr().unwrap().iter().map(|h| Json::Obj(vec![("id".into(), s(h, &["id"]).clone()), ("when".into(), s(h, &["when"]).clone()), ("then".into(), s(h, &["then"]).clone())])).collect();
        let (pp, rp) = (tmp(&format!("fuzz-rand{k}.json")), tmp(&format!("fuzz-rand{k}-props.json")));
        std::fs::write(&pp, jw(&doc)).unwrap(); std::fs::write(&rp, jw(&Json::Obj(vec![("props".into(), Json::Arr(habits))]))).unwrap();
        let (c, out, e) = probbit(&["persona", "fuzz", &pp, "--seeds", "0-9", "--scripts", "20", "--depth", "6", "--props", &rp, "--json"], "");
        assert_eq!(c, 0, "random persona {k}: a habit broke: {out:.600} {e}");
        assert_eq!(s(&parse(&out), &["properties"]).as_arr().unwrap().len(), hdefs.len());
        for f in [&pp, &rp] { let _ = std::fs::remove_file(f); }
    }
}

/// A bad rule is one error object at its path (exit 2), like a bad habit; a missing rule or a bad flag is a stderr line (exit 2)
#[test]
fn fuzz_refuses_bad_rules_and_flags() {
    for (rule, path) in [("{when: {sentiment: grumpy}, then: {humour: [none]}}", "--never.when.sentiment"), ("{when: {loss: true}, then: {humourz: [none]}}", "--never.then.humourz"),
        ("{when: {loss: true}}", "--never"), (r#"{"when": {"loss": true}, "then": {"humour": {"at_most": "loud"}}}"#, "--never.then.humour.at_most"), ("[1, 2]", "--never[0]")] {
        let (c, out, e) = probbit_at_root(&["persona", "fuzz", TUTOR_050, "--never", rule]); assert_eq!(c, 2, "{rule}: {e}");
        assert_eq!(s(&parse(&out), &["error", "path"]).as_str(), Some(path), "{rule}: {out}"); }
    for args in [vec!["--seeds", "5-1"], vec!["--seeds", "x"], vec!["--grid", "0,2"], vec!["--depth", "0"], vec![]] {
        let mut a = vec!["persona", "fuzz", TUTOR_050]; if !args.is_empty() { a.extend(["--never", UPSET_NOT_PLAYFUL]); } a.extend(args.iter().copied());
        let (c, out, e) = probbit_at_root(&a); assert_eq!(c, 2, "{a:?}: {e}"); assert!(out.is_empty(), "{a:?}: {out}"); }
}

/// `persona prove` on the 0.5.0 tutor (5 individuals): its own habit's rule is held by construction, "never blunt when upset" is
/// proved for every event sequence, "never playful when upset" is unknown (the fuzzer breaks it); exit 1 while a rule is unknown, 0
/// when every rule is held or proved; the JSON document says the same; a bad rule is one error object, exit 2.
#[test]
fn prove_gives_the_three_verdicts_on_the_0_5_0_tutor() {
    let props = tmp("prove-props.json");
    std::fs::write(&props, r#"{"props": [{"id": "loss_no_jokes", "when": {"loss": true}, "then": {"humour": ["none"]}},
        {"id": "not_blunt_upset", "when": {"sentiment": "negative"}, "then": {"directness": {"at_most": "balanced"}}},
        {"id": "not_playful_upset", "when": {"sentiment": "negative"}, "then": {"humour": {"at_most": "light"}}}]}"#).unwrap();
    let (c, out, e) = probbit_at_root(&["persona", "prove", TUTOR_050, "--seeds", "0-4", "--props", &props]); assert_eq!(c, 1, "{e}"); assert!(e.contains("prove: "), "{e}");
    for want in ["loss_no_jokes  {when: {loss: true}, then: {humour: [none]}}\n  held by construction  (habit no_jokes_on_loss)",
        "not_blunt_upset  {when: {sentiment: negative}, then: {directness: {at_most: balanced}}}\n  proved for every event sequence  (5 individuals; ",
        "not_playful_upset  {when: {sentiment: negative}, then: {humour: {at_most: light}}}\n  unknown  the bound decides 0 of 5 individuals; not seeds 0-4\n  seed 0: humour: the score gap ",
        &format!("  search it: probbit persona fuzz {TUTOR_050} --seeds 0-4 --never '{{when: {{sentiment: negative}}, then: {{humour: {{at_most: light}}}}}}'")] {
        assert!(out.contains(want), "missing {want:?} in\n{out}"); }
    let (c, j, e) = probbit_at_root(&["persona", "prove", TUTOR_050, "--seeds", "0-1", "--props", &props, "--json", "--threads", "2"]); assert_eq!(c, 1, "{e}");
    let j = parse(&j); assert_eq!(s(&j, &["probbit_persona_prove"]).as_f64(), Some(1.0));
    let v: Vec<&str> = s(&j, &["properties"]).as_arr().unwrap().iter().map(|p| s(p, &["verdict"]).as_str().unwrap()).collect();
    assert_eq!(v, ["held_by_construction", "proved", "unknown"]);
    let (c, _, e) = probbit_at_root(&["persona", "prove", TUTOR_050, "--seeds", "0-2", "--never", "{when: {sentiment: negative}, then: {directness: {at_most: balanced}}}"]); assert_eq!(c, 0, "{e}");
    let (c, out, _) = probbit_at_root(&["persona", "prove", TUTOR_050, "--never", "{when: {sentimentx: negative}, then: {humour: [none]}}"]); assert_eq!(c, 2);
    assert_eq!(s(&parse(&out), &["error", "code"]).as_str(), Some("persona"));
    let _ = std::fs::remove_file(&props);
}

/// `persona lint --props`: the contradictions as before, then every rule proved or held, and the unknown ones fuzzed; exit 1 and
/// `ok` false when the fuzzer breaks one (the 0.5.0 tutor: never playful when upset), exit 0 when every rule is held or proved.
#[test]
fn lint_with_rules_proves_then_fuzzes_the_unknown_ones() {
    let props = tmp("lint-props.json");
    std::fs::write(&props, r#"{"props": [{"id": "loss_no_jokes", "when": {"loss": true}, "then": {"humour": ["none"]}},
        {"id": "not_playful_upset", "when": {"sentiment": "negative"}, "then": {"humour": {"at_most": "light"}}}]}"#).unwrap();
    let (c, out, e) = probbit_at_root(&["persona", "lint", TUTOR_050, "--props", &props, "--seeds", "0-4"]); assert_eq!(c, 1, "{e}");
    let l = parse(&out); assert_eq!((s(&l, &["ok"]), s(&l, &["broken"]).as_f64(), s(&l, &["unresolved"]).as_f64()), (&Json::Bool(false), Some(1.0), Some(0.0)));
    let p = s(&l, &["props"]).as_arr().unwrap();
    assert_eq!((s(&p[0], &["verdict"]).as_str(), p[0].get("fuzz").is_none()), (Some("held_by_construction"), true));
    assert_eq!((s(&p[1], &["verdict"]).as_str(), s(&p[1], &["fuzz", "verdict"]).as_str()), (Some("unknown"), Some("counterexample")));
    assert_eq!(jw(s(&p[1], &["fuzz", "shortest", "script"])), r#"[{"sentiment":"negative"}]"#);
    let (c, out, e) = probbit_at_root(&["persona", "lint", TUTOR_050, "--never", "{when: {sentiment: negative}, then: {directness: {at_most: balanced}}}", "--seeds", "0-2"]); assert_eq!(c, 0, "{e}");
    assert_eq!(s(&parse(&out), &["ok"]), &Json::Bool(true));
    let _ = std::fs::remove_file(&props);
}

/// The tutor as it ships now (one habit more, no_play_when_upset): "never playful when upset" is held by construction and the
/// fuzzer finds nothing. On the 0.5.0 tutor's counterexample, `why` names the push's direction ("humour down", not "humour none"
/// next to a playful stance), and `lint` warns that seed 1's planned humour (playful) is not its most likely level (light); a
/// warning does not change the exit code.
#[test]
fn the_fixed_tutor_holds_and_lint_warns_when_the_plan_is_not_the_mode() {
    let tutor = "examples/persona/tutor.yaml";
    let (c, out, e) = probbit_at_root(&["persona", "prove", tutor, "--seeds", "0-99", "--never", UPSET_NOT_PLAYFUL]); assert_eq!(c, 0, "{e}");
    assert!(out.contains("  held by construction  (habit no_play_when_upset)"), "{out}");
    let (c, out, e) = probbit_at_root(&["persona", "fuzz", tutor, "--seeds", "0-19", "--scripts", "20", "--never", UPSET_NOT_PLAYFUL]); assert_eq!(c, 0, "{e}");
    assert!(out.contains("none found  0 of 20 individuals broke it"), "{out}");
    let (c, rep, e) = probbit_at_root(&["persona", "replay", TUTOR_050, "--seed", "1", "--script", r#"[{"sentiment":"negative"}]"#]); assert_eq!(c, 0, "{e}");
    let t = parse(rep.trim_end()); assert_eq!(s(&t, &["stance", "humour", "level"]).as_str(), Some("playful"));
    assert_eq!(s(&t, &["why"]).as_str(), Some("learner upset -> valence down, humour down"));
    let (c, l, e) = probbit_at_root(&["persona", "lint", TUTOR_050, "--seeds", "1"]); assert_eq!(c, 0, "{e}");
    let w = s(&parse(&l), &["warnings"]).as_arr().unwrap().to_vec(); assert_eq!(w.len(), 1, "{l}");
    assert_eq!((s(&w[0], &["trait"]).as_str(), jw(s(&w[0], &["example", "inputs"])), s(&w[0], &["example", "plan"]).as_str(), s(&w[0], &["example", "mode"]).as_str()),
        (Some("humour"), r#"{"sentiment":"negative"}"#.to_string(), Some("playful"), Some("light")));
    assert_eq!((s(&w[0], &["example", "p_plan"]).as_f64(), s(&w[0], &["example", "p_mode"]).as_f64()), (Some(0.410818), Some(0.424967)));
}

// ---------------- 0.7.0: probbit live (docs/persona.md §5.7)
/// `probbit live PERSONA --demo week --plain` (the shipped command, seed 2): two runs print the same lines and write the same strand,
/// byte for byte, and `probbit live verify` replays the strand to the last-line sha256 the demo printed
#[test]
fn live_demo_week_is_deterministic_and_verifies() {
    let (a, b) = (tmp("week-a.strand"), tmp("week-b.strand")); let _ = std::fs::remove_file(&a); let _ = std::fs::remove_file(&b);
    let (c1, o1, _) = probbit(&["live", &ex("tutor.yaml"), "--seed", "2", "--demo", "week", "--plain", "--strand", &a], "");
    let (c2, o2, _) = probbit(&["live", &ex("tutor.yaml"), "--seed", "2", "--demo", "week", "--plain", "--strand", &b], "");
    assert_eq!((c1, c2), (0, 0), "{o1}"); assert_eq!(o1.replace(&a, "S"), o2.replace(&b, "S")); assert_eq!(read(&a), read(&b));
    assert!(o1.lines().count() > 40 && !o1.contains('\x1b'), "one plain line per event");
    let (c, v, _) = probbit(&["live", "verify", &a], ""); assert_eq!(c, 0, "{v}");
    let head = s(&parse(&v), &["last_line"]).as_str().unwrap().to_string(); assert!(o1.contains(&format!("last line {head};")), "the demo prints the head verify finds");
    let (c, _, e) = probbit(&["live", &ex("tutor.yaml"), "--demo", "week", "--plain", "--strand", &a], ""); assert_eq!(c, 2, "a strand is never overwritten: {e}");
    let (c, _, e) = probbit(&["live", &ex("tutor.yaml"), "--demo", "week", "--clock", "fixed"], ""); assert_eq!(c, 2); assert!(e.contains("does not apply"), "{e}");
    let _ = std::fs::remove_file(&a); let _ = std::fs::remove_file(&b);
}

/// `--strand FILE` on an existing strand continues it from `--state` (the state after its last line): two runs of 3 and 4 events
/// write the strand one run of 7 writes; a new individual (`--seed`) cannot continue it
#[test]
fn live_continues_a_strand_across_runs() {
    let evs: Vec<String> = (0..7).map(|t| format!(r#"{{"praise":{},"loss":{},"elapsed_hours":{}}}"#, t % 2 == 1, t % 3 == 2, 0.5 + t as f64)).collect();
    let (one, two, st1, st) = (tmp("cont-one.strand"), tmp("cont-two.strand"), tmp("cont-state1.json"), tmp("cont-state.json")); for f in [&one, &two, &st1, &st] { let _ = std::fs::remove_file(f); }
    let (_, s0, _) = probbit(&["persona", "init", &ex("tutor.yaml"), "--seed", "3"], ""); std::fs::write(&st, &s0).unwrap(); std::fs::write(&st1, &s0).unwrap();
    let (c, _, e) = probbit(&["live", &ex("tutor.yaml"), "--state", &st1, "--clock", "fixed", "--strand", &one], &(evs.join("\n") + "\n")); assert_eq!(c, 0, "{e}");
    let (c, _, e) = probbit(&["live", &ex("tutor.yaml"), "--state", &st, "--clock", "fixed", "--strand", &two], &(evs[..3].join("\n") + "\n")); assert_eq!(c, 0, "{e}");
    let (c, _, e) = probbit(&["live", &ex("tutor.yaml"), "--state", &st, "--clock", "fixed", "--strand", &two], &(evs[3..].join("\n") + "\n")); assert_eq!(c, 0, "{e}");
    assert_eq!(read(&one), read(&two));
    let (c, _, e) = probbit(&["live", &ex("tutor.yaml"), "--seed", "3", "--clock", "fixed", "--strand", &two], "{}\n"); assert_eq!(c, 2); assert!(e.contains("continue it with --state"), "{e}");
    for f in [&one, &two, &st1, &st] { let _ = std::fs::remove_file(f); }
}

/// `--watch` follows the events file as lines are appended (a half-written line waits for its newline) and stops when the file is
/// removed; the stances are the ones the same events give read at once
#[cfg(unix)]
#[test]
fn live_watch_follows_an_events_file() {
    let (f, s1, s2) = (tmp("watch-events.jsonl"), tmp("watch-1.strand"), tmp("watch-2.strand")); for x in [&f, &s1, &s2] { let _ = std::fs::remove_file(x); }
    let evs = [r#"{"praise":true,"elapsed_hours":1}"#, r#"{"loss":true,"elapsed_hours":2}"#, r#"{"sentiment":"negative","elapsed_hours":0.25}"#];
    std::fs::write(&f, format!("{}\n", evs[0])).unwrap();
    let c = Command::new(env!("CARGO_BIN_EXE_probbit")).args(["live", &ex("tutor.yaml"), "--seed", "4", "--clock", "fixed", "--events", &f, "--watch", "--strand", &s1])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let pause = || std::thread::sleep(std::time::Duration::from_millis(300));
    pause(); { let w = &mut std::fs::OpenOptions::new().append(true).open(&f).unwrap(); w.write_all(&evs[1].as_bytes()[..10]).unwrap(); w.flush().unwrap(); pause();
        w.write_all(format!("{}\n{}\n", &evs[1][10..], evs[2]).as_bytes()).unwrap(); w.flush().unwrap(); }
    pause(); std::fs::remove_file(&f).unwrap();
    let o = c.wait_with_output().unwrap(); assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let (_, at_once, _) = probbit(&["live", &ex("tutor.yaml"), "--seed", "4", "--clock", "fixed", "--strand", &s2], &(evs.join("\n") + "\n"));
    assert_eq!(String::from_utf8_lossy(&o.stdout), at_once); assert_eq!(read(&s1), read(&s2)); assert_eq!(at_once.lines().count(), 3);
    for x in [&s1, &s2] { let _ = std::fs::remove_file(x); }
}

// ---------------- the safety kit (docs/persona.md §2.10, §5.7)
/// 300 events of the drives fixture with `src` on each (self, a person, a sensor, the clock), rewards on many
fn src_events() -> Vec<String> {
    (0..300).map(|t| { let mut e = vec![r#""elapsed_hours":0.5"#.to_string()];
        if t % 2 == 0 { e.push(r#""praise":true"#.into()); } if t % 5 == 1 { e.push(r#""criticism":true"#.into()); } if t % 3 == 0 { e.push(r#""goals":{"fun":{"win":1.0}}"#.into()); }
        e.push(format!(r#""src":"{}""#, ["self", "human:owner", "env:tests", "clock"][t % 4])); format!("{{{}}}", e.join(",")) }).collect()
}
/// A persona without `reward_from` gives 0.8.0's bytes with `src` in its events (an undeclared input, listed in `ignored`): the
/// trace, the continued 0.8.0 strand and the final state of 300 events of the drives fixture (seed 4) match the historical
/// digests. Fresh headers identify the current binary; the run leaves no lock file behind. The same persona with
/// `reward_from: [human, env]` refuses the rewards from `self` and the
/// clock (exit 2, one error at `inputs.src` per refused event) and accepts the others.
#[test]
fn src_without_reward_from_gives_0_8_0_bytes() {
    let fx = format!("{}/tests/fixtures/persona/drives-adversary.json", env!("CARGO_MANIFEST_DIR"));
    let (script, st, sd) = (tmp("src-script.json"), tmp("src-state.json"), tmp("src.strand")); for f in [&st, &sd] { let _ = std::fs::remove_file(f); }
    let evs = src_events(); std::fs::write(&script, format!("[{}]", evs.join(","))).unwrap();
    let (c, _, e) = probbit(&["persona", "replay", &fx, "--seed", "4", "--script", &script], ""); assert_eq!(c, 0, "{e}");
    assert_eq!(e.trim(), "replay: 300 turns, trace sha256 ce8f247d1fe761ea651998f48360491030f15ba3c8530b0ed59d824868f71b71, final state sha256:52312b3cb8744360e7613e4db2fc5e3d326cb3c2204af7312e2a4196f9781551");
    let (_, s0, _) = probbit(&["persona", "init", &fx, "--seed", "4"], ""); std::fs::write(&st, &s0).unwrap();
    let (c, _, e) = probbit(&["live", &fx, "--state", &st, "--clock", "fixed", "--strand", &sd], ""); assert_eq!(c, 0, "{e}");
    let header = read(&sd); assert_eq!(header.lines().count(), 1);
    let current_engine = concat!("\"engine\":\"probbit ", env!("CARGO_PKG_VERSION"), "\"");
    assert!(header.contains(current_engine), "{header}");
    // Continue a historical header: its version participates in every chained hash. Keep the 0.8.0 golden, not a new hash per release.
    std::fs::write(&sd, header.replacen(current_engine, "\"engine\":\"probbit 0.8.0\"", 1)).unwrap();
    let (c, out, e) = probbit(&["live", &fx, "--state", &st, "--clock", "fixed", "--strand", &sd], &(evs.join("\n") + "\n")); assert_eq!(c, 0, "{e}");
    assert!(e.contains("strand head sha256:c37fea55abbec95d0c0f1f1703741e20c26cd5d66587132917772435a5e94655, final state sha256:52312b3cb8744360e7613e4db2fc5e3d326cb3c2204af7312e2a4196f9781551"), "{e}");
    let (c, verified, e) = probbit(&["live", "verify", &sd], ""); assert_eq!(c, 0, "{verified}{e}");
    assert_eq!(out.lines().filter(|l| l.contains(r#""ignored":["src"]"#)).count(), 300);
    assert!(!std::path::Path::new(&format!("{sd}.lock")).exists(), "the lock is released");
    // with reward_from: the rewards from self and the clock are refused, the others accepted
    let mut d = parse(&read(&fx)); if let Json::Obj(kv) = &mut d { kv.push(("reward_from".into(), parse(r#"["human","env"]"#))); }
    let guarded = tmp("src-guarded.json"); std::fs::write(&guarded, jw(&d)).unwrap(); let gs = tmp("src-guarded-state.json");
    let (_, s0, _) = probbit(&["persona", "init", &guarded, "--seed", "4"], ""); std::fs::write(&gs, &s0).unwrap();
    let (c, out, _) = probbit(&["live", &guarded, "--state", &gs, "--clock", "fixed"], &(evs.join("\n") + "\n")); assert_eq!(c, 2);
    let refused = out.lines().filter(|l| l.contains(r#""path":"events["#) && l.contains(r#"].inputs.src""#)).count();
    let rewarded = |t: usize| t % 2 == 0 || t % 5 == 1 || t % 3 == 0;
    assert_eq!(refused, (0..300).filter(|t| rewarded(*t) && (t % 4 == 0 || t % 4 == 3)).count());
    let (c, out, _) = probbit(&["persona", "turn", &guarded, "--state", &gs, "--inputs", r#"{"praise":true,"src":"self"}"#], ""); assert_eq!(c, 2); assert!(out.contains(r#""path":"inputs.src""#), "{out}");
    for f in [&script, &st, &sd, &guarded, &gs] { let _ = std::fs::remove_file(f); }
}

/// The safety kit through the CLI: `--checkpoint-every` writes checkpoint lines that `verify` and `verify --from-checkpoint`
/// check; `live control` pauses (every event refused, exit 4, nothing written), resumes and retires (final); the individual cannot
/// control itself; a held lock refuses a second writer and a control line (exit 4) and changes nothing; no lock is left behind
#[test]
fn live_safety_kit_exit_codes() {
    let (st, sd) = (tmp("kit-state.json"), tmp("kit.strand")); let lock = format!("{sd}.lock"); for f in [&st, &sd, &lock] { let _ = std::fs::remove_file(f); }
    let tutor = ex("tutor.yaml");
    let (_, s0, _) = probbit(&["persona", "init", &tutor, "--seed", "3"], ""); std::fs::write(&st, &s0).unwrap();
    let evs = |n: usize| (0..n).map(|t| format!(r#"{{"praise":{},"elapsed_hours":1}}"#, t % 2 == 0)).collect::<Vec<_>>().join("\n") + "\n";
    let (c, _, e) = probbit(&["live", &tutor, "--state", &st, "--clock", "fixed", "--strand", &sd, "--checkpoint-every", "2"], &evs(5)); assert_eq!(c, 0, "{e}");
    assert_eq!(read(&sd).lines().filter(|l| l.starts_with(r#"{"checkpoint":"#)).count(), 2);
    let (c, v, _) = probbit(&["live", "verify", &sd], ""); assert_eq!(c, 0, "{v}"); assert!(v.contains(r#""checkpoints":2"#), "{v}");
    let (c, v, _) = probbit(&["live", "verify", &sd, "--from-checkpoint"], ""); assert_eq!(c, 0, "{v}"); assert!(v.contains(r#""from_checkpoint":4"#), "{v}");
    let (c, o, e) = probbit(&["live", "control", &sd, "pause", "--by", "human:owner", "--reason", "a check", "--at", "2026-10-08T10:00:00Z"], ""); assert_eq!(c, 0, "{e}");
    assert!(o.contains(r#""status":"paused""#), "{o}");
    let before = read(&sd);
    let (c, o, _) = probbit(&["live", &tutor, "--state", &st, "--clock", "fixed", "--strand", &sd], &evs(3)); assert_eq!(c, 4);
    assert_eq!(o.lines().filter(|l| l.contains(r#""code":"paused""#)).count(), 3); assert_eq!(read(&sd), before, "nothing written while paused");
    let (c, _, _) = probbit(&["live", "control", &sd, "resume", "--by", "self", "--reason", "x"], ""); assert_eq!(c, 2, "the individual cannot resume itself");
    let (c, _, e) = probbit(&["live", "control", &sd, "resume", "--by", "human:owner", "--reason", "checked"], ""); assert_eq!(c, 0, "{e}");
    let (c, _, e) = probbit(&["live", &tutor, "--state", &st, "--clock", "fixed", "--strand", &sd], &evs(2)); assert_eq!(c, 0, "{e}");
    // a lock held by a running process (this test's): a second writer and a control line exit 4 and change nothing
    std::fs::write(&lock, format!(r#"{{"pid":{},"since":"2026-10-08T10:00:00Z","t":1}}"#, std::process::id())).unwrap(); let before = read(&sd);
    let (c, o, _) = probbit(&["live", &tutor, "--state", &st, "--clock", "fixed", "--strand", &sd], &evs(1)); assert_eq!(c, 4); assert!(o.contains(r#""code":"locked""#), "{o}");
    let (c, _, _) = probbit(&["live", "control", &sd, "pause", "--by", "human:owner", "--reason", "x"], ""); assert_eq!(c, 4, "control takes the lock too");
    assert_eq!(read(&sd), before); std::fs::remove_file(&lock).unwrap();
    let (c, _, _) = probbit(&["live", "control", &sd, "retire", "--by", "human:owner", "--reason", "the end"], ""); assert_eq!(c, 0);
    let (c, _, _) = probbit(&["live", "control", &sd, "resume", "--by", "human:owner", "--reason", "again"], ""); assert_eq!(c, 4, "retire is final");
    let (c, v, _) = probbit(&["live", "verify", &sd], ""); assert_eq!(c, 0, "{v}"); assert!(v.contains(r#""status":"retired""#) && v.contains(r#""controls":3"#), "{v}");
    assert!(!std::path::Path::new(&lock).exists(), "no lock left behind");
    for f in [&st, &sd] { let _ = std::fs::remove_file(f); }
}

/// The `drives` block (0.8.0) is additive: a persona without it gives 0.7.0's documents byte for byte, with `goals` in the inputs
/// too (an ignored input there, as it was in 0.7.0). The digests below are 0.7.0's own `persona replay` output on the same scripts
/// (tests/fixtures/persona/drives-noblock.json: 40 random turns per example persona, with goal signals and idle hours).
#[test]
fn personas_without_drives_are_byte_identical_to_0_7_0() {
    let f = parse(&read(&format!("{}/tests/fixtures/persona/drives-noblock.json", env!("CARGO_MANIFEST_DIR"))));
    for (n, trace, fin) in [
        ("ops-engineer", "095d2a2fe9101d65158176e1a9dd3c72e8cff0c92d1e77da95e7fbd226e353e3", "314cd4885b0097c59021a38a3470d6ab91896bad3a8f3802a7c6c29ceffa8835"),
        ("tutor", "781acf63e0cf143a7d0ae5f29630b5106ce264e39d2e4c7d91a3c6fdbf71f25d", "2374f248c5fcc701082acff586cacd74cf8f82240d7cd2f6b67a9a8a13ae8521"),
        ("trader-assistant", "ef8abbd4f939ab9b49d08072db52d4cc8fbbd406c5a139d0dc1e26c963cc82c6", "a9cf897d2006994688ee3014e7ff5cf00582ae25c83b8146510aa5e6a08212f6")] {
        let v = s(&f, &[n]); let script = tmp(&format!("noblock-{n}.json")); std::fs::write(&script, jw(s(v, &["turns"]))).unwrap();
        let seed = format!("{}", s(v, &["seed"]).as_f64().unwrap() as u64);
        let (c, out, e) = probbit(&["persona", "replay", &ex(&format!("{n}.yaml")), "--seed", &seed, "--script", &script], ""); assert_eq!(c, 0, "{e}");
        assert_eq!(e.trim(), format!("replay: 40 turns, trace sha256 {trace}, final state sha256:{fin}"), "{n}");
        assert!(out.lines().filter(|l| l.contains(r#""ignored":["goals"]"#)).count() > 10, "{n}: goals given and ignored");
        assert!(!out.contains(r#""pursue""#) && !out.contains(r#""drives""#), "{n}: no drives fields without the block");
    }
}

/// drives (§2.9): the adversary fixture's replay (16 turns of random host inputs and goal signals, seed 4) equals its golden,
/// which is the Python reference's (a prototype of the design on the 0.7.0 engine) trace of the same script, byte for byte
#[test]
fn a_drives_replay_equals_the_reference_trace() {
    let f = |n: &str| format!("{}/tests/fixtures/persona/{n}", env!("CARGO_MANIFEST_DIR"));
    let (c, out, e) = probbit(&["persona", "replay", &f("drives-adversary.json"), "--seed", "4", "--script", &f("drives-adversary-script.json")], ""); assert_eq!(c, 0, "{e}");
    assert_eq!(out, read(&f("drives-adversary-replay.jsonl")));
    assert!(e.contains("final state sha256:27e92b4f21a52888b03c857a8132a25b58270aae613a77ce1edd2d74adcb3422"), "{e}");
    assert_eq!(out.lines().filter(|l| l.contains(r#""goals":{"#)).count(), 9, "goal signals on 9 of the 16 turns");
}

/// drives (§2.9), 200 turns: one script of random host inputs and goal signals (tests/fixtures/persona/drives-adversary-200.json,
/// signals on 163 turns, idle hours 0-12) for three individuals. Each trace is the Python reference's (the prototype on the
/// 0.7.0 engine) byte for byte: the constants are the sha256 of its text and its final state
#[test]
fn two_hundred_drives_turns_equal_the_reference_for_three_individuals() {
    let f = |n: &str| format!("{}/tests/fixtures/persona/{n}", env!("CARGO_MANIFEST_DIR"));
    for (seed, trace, fin) in [
        ("2", "fa9fc8019b33d7a12c09af11d3a7b3adbf7988e9600479141dbd4ba2157e5d02", "57bc4e515330f3ec7a9e3ae3024751bbfa3826fd1008829a962bcdc5335ceec6"),
        ("5", "6f535a6256a24bad911a499e4bf12501581d4b9934d68e5466bce330f9e6c2ae", "08d10ad478746ec2d16b6b8d6a37b5540c791d803917afd194f7e96c9fbb7853"),
        ("9", "16c8d13dc3f6e6f2478be92dbc68ed1f503c06fc02109c4d64ab1ba4dabdf36d", "9d91e77fe18b390d3925a643294ac240b40107042c9bf63624e939aecc5f8882")] {
        let (c, out, e) = probbit(&["persona", "replay", &f("drives-adversary.json"), "--seed", seed, "--script", &f("drives-adversary-200.json")], ""); assert_eq!(c, 0, "{e}");
        assert_eq!(e.trim(), format!("replay: 200 turns, trace sha256 {trace}, final state sha256:{fin}"), "seed {seed}");
        let goals: std::collections::BTreeSet<String> = out.lines().map(|l| s(&parse(l), &["pursue", "goal"]).as_str().unwrap().to_string()).collect();
        assert_eq!(goals.len(), 4, "seed {seed}: every goal pursued on some turn");
    }
}
