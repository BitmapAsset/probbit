//! `probbit live` with senses (docs/persona.md §2.11, §5.7): readings logged in the strand as used, `live verify` recomputing the
//! normals (summary `normals` with `max_move_50`), a life split across runs equal to one run, the one stderr line for readings
//! sent to a persona without the block, the refusal property P3s, and `probbit check` on a sensed persona.
use std::io::Write;
use std::process::{Command, Stdio};
#[allow(dead_code)]
#[path = "../src/json.rs"]
mod json;
use json::Json;

const SENSED: &str = "examples/persona/sensed.yaml";
const SENSED_PROPS: &str = "examples/persona/sensed-props.json";
const TUTOR: &str = "examples/persona/tutor.yaml";

/// `probbit` run from the repository root: (exit code, stdout, stderr)
fn probbit(args: &[&str]) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let _ = c.stdin.take().unwrap().write_all(b"");
    let o = c.wait_with_output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
fn tmp(name: &str) -> String { format!("{}/live-{}-{name}", env!("CARGO_TARGET_TMPDIR"), std::process::id()) }
fn read(p: &str) -> String { std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p}: {e}")).replace("\r\n", "\n") }
fn parse(s: &str) -> Json { json::parse(s).unwrap_or_else(|e| panic!("not JSON ({}): {s:.300}", e.msg)) }
fn jw(j: &Json) -> String { json::write(j, false) }
fn s<'a>(j: &'a Json, path: &[&str]) -> &'a Json { path.iter().fold(j, |x, k| x.get(k).unwrap_or_else(|| panic!("no {k} in {:.200}", jw(x)))) }
fn clean(files: &[&str]) { for f in files { let _ = std::fs::remove_file(f); let _ = std::fs::remove_file(format!("{f}.lock")); } }
/// The sensed golden's 24 events, one canonical line each
fn day() -> Vec<String> { parse(&read(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/persona/golden/sensed/day.json"))).as_arr().unwrap().iter().map(jw).collect() }
fn events_file(name: &str, lines: &[String]) -> String { let f = tmp(name); std::fs::write(&f, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap(); f }
/// `persona init` of `persona` at `seed` into a fresh state file
fn init(persona: &str, seed: u64, name: &str) -> String {
    let st = tmp(name); clean(&[&st]); let (c, _, e) = probbit(&["persona", "init", persona, "--seed", &seed.to_string(), "--out", &st]); assert_eq!(c, 0, "{e}"); st
}
/// `live --clock fixed` from the state file over the events file, logging to `strand`
fn live(persona: &str, st: &str, events: &str, strand: &str, every: &str) -> (i32, String, String) {
    probbit(&["live", persona, "--state", st, "--clock", "fixed", "--events", events, "--strand", strand, "--checkpoint-every", every])
}

/// The strand logs each reading as used (with its source), `live verify` recomputes the normals from them: the summary's
/// `normals` per sense (mean, n, max_move_50) equal the final state's, and a changed reading in the strand fails the replay
#[test]
fn readings_are_logged_and_verify_recomputes_the_normals() {
    let ev = events_file("day.jsonl", &day()); let st = init(SENSED, 1, "day-state.json"); let strand = tmp("day.strand"); clean(&[&strand]);
    let (c, out, e) = live(SENSED, &st, &ev, &strand, "6"); assert_eq!(c, 0, "{e}"); assert_eq!(out.lines().count(), 24);
    let text = read(&strand);
    assert!(text.contains(r#"{"inputs":{"elapsed_hours":0.1,"senses":{"tempo":25,"tool_fail":false},"src":"env:chat"},"n":6,"#), "{text:.600}");
    assert!(text.contains(r#""senses":{"tempo":{"src":"env:chat","v":4}},"src":"human:ana""#), "a reading keeps its own source");
    let (c, v, e) = probbit(&["live", "verify", &strand]); assert_eq!(c, 0, "{e}"); let v = parse(&v);
    assert_eq!(s(&v, &["ok"]), &Json::Bool(true));
    let fin = parse(&read(&st)); let norms = s(&v, &["normals"]).as_obj().unwrap();
    assert_eq!(norms.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["silence", "tempo", "tool_fail", "tool_latency"]);
    let reads = |id: &str| day().iter().filter(|l| parse(l).get("senses").is_some_and(|x| x.get(id).is_some())).count() as f64;
    for (id, n) in norms {
        assert_eq!(s(n, &["mean"]), s(&fin, &["senses", "norm", id, "mean"]), "{id}"); assert_eq!(s(n, &["n"]), s(&fin, &["senses", "norm", id, "n"]), "{id}");
        assert_eq!(s(n, &["n"]).as_f64(), Some(reads(id)), "{id}: one count per reading");
        let m = s(n, &["max_move_50"]).as_f64().unwrap(); assert!(m > 0.0 && m <= 4.0, "{id}: {m}"); }
    let bad = tmp("day-bad.strand"); std::fs::write(&bad, text.replacen(r#""tempo":25"#, r#""tempo":26"#, 1)).unwrap();
    let (c, v, _) = probbit(&["live", "verify", &bad]); assert_ne!(c, 0, "{v}"); assert_eq!(s(&parse(&v), &["ok"]), &Json::Bool(false));
    clean(&[&st, &strand, &bad, &ev]);
}

/// Replay: a life split across two runs (the strand continued with the state after its last line) equals the life in one run,
/// byte for byte, strand and state
#[test]
fn a_sensed_life_split_across_runs_equals_one_run() {
    let lines = day(); let all = events_file("all.jsonl", &lines);
    let st1 = init(SENSED, 4, "one-state.json"); let one = tmp("one.strand"); clean(&[&one]);
    let (c, out1, e) = live(SENSED, &st1, &all, &one, "5"); assert_eq!(c, 0, "{e}");
    for k in [1, 9, 17] {
        let (a, b) = (events_file("a.jsonl", &lines[..k]), events_file("b.jsonl", &lines[k..]));
        let st2 = init(SENSED, 4, "two-state.json"); let two = tmp("two.strand"); clean(&[&two]);
        let (c, o1, e) = live(SENSED, &st2, &a, &two, "5"); assert_eq!(c, 0, "{e}");
        let (c, o2, e) = live(SENSED, &st2, &b, &two, "5"); assert_eq!(c, 0, "{e}");
        assert_eq!(read(&two), read(&one), "split at {k}: the strand"); assert_eq!(read(&st2), read(&st1), "split at {k}: the state");
        assert_eq!(format!("{o1}{o2}"), out1, "split at {k}: the stances");
        clean(&[&st2, &two, &a, &b]);
    }
    clean(&[&st1, &one, &all]);
}

/// A persona without a senses block lists `senses` in `ignored`, as before, and `live` says so once on stderr, naming the first
/// event that carried it; no line without a `senses` key, and none for a persona with the block
#[test]
fn readings_sent_to_a_persona_without_the_block_are_named_once() {
    let line = |i: usize| format!("probbit: live: events[{i}]: this persona has no senses block: `senses` is an undeclared input (listed in ignored), no reading is judged");
    let ev = events_file("tutor.jsonl", &[r#"{"elapsed_hours":1}"#.into(), r#"{"senses":{"tool_fail":true}}"#.into(), r#"{"senses":{"tempo":3},"error":true}"#.into()]);
    let st = init(TUTOR, 2, "tutor-state.json"); let strand = tmp("tutor.strand"); clean(&[&strand]);
    let (c, out, e) = live(TUTOR, &st, &ev, &strand, "0"); assert_eq!(c, 0, "{e}");
    assert_eq!(e.lines().filter(|l| l.contains("senses block")).collect::<Vec<_>>(), [line(1).as_str()], "{e}");
    let docs: Vec<Json> = out.lines().map(parse).collect();
    assert_eq!((jw(s(&docs[0], &["ignored"])), jw(s(&docs[1], &["ignored"])), jw(s(&docs[2], &["ignored"]))), ("[]".into(), r#"["senses"]"#.into(), r#"["senses"]"#.into()));
    assert!(docs.iter().all(|d| d.get("noticed").is_none() && d.get("senses").is_none()), "no new stance keys without the block");
    let quiet = events_file("tutor-quiet.jsonl", &[r#"{"elapsed_hours":1}"#.into(), r#"{"error":true}"#.into()]);
    let st2 = init(TUTOR, 2, "tutor-state2.json"); let strand2 = tmp("tutor2.strand"); clean(&[&strand2]);
    let (c, _, e) = live(TUTOR, &st2, &quiet, &strand2, "0"); assert_eq!(c, 0, "{e}"); assert!(!e.contains("senses block"), "{e}");
    let sev = events_file("sensed.jsonl", &day()[..3]); let st3 = init(SENSED, 2, "sensed-state3.json"); let strand3 = tmp("sensed3.strand"); clean(&[&strand3]);
    let (c, _, e) = live(SENSED, &st3, &sev, &strand3, "0"); assert_eq!(c, 0, "{e}"); assert!(!e.contains("senses block"), "{e}");
    clean(&[&st, &strand, &ev, &quiet, &st2, &strand2, &sev, &st3, &strand3]);
}

/// SplitMix64: the property tests' own seeded generator
struct Mix(u64);
impl Mix {
    fn next(&mut self) -> u64 { self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15); let mut z = self.0; z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9); z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB); z ^ (z >> 31) }
    fn below(&mut self, k: usize) -> usize { (self.next() % k as u64) as usize }
    fn unit(&mut self) -> f64 { (self.next() >> 11) as f64 / (1u64 << 53) as f64 }
}

/// One random event for the sensed persona: sources (env, human, self, clock, missing, malformed), readings (declared and not,
/// in and out of range, flags that are not true/false, readings with their own source), feedback, elapsed hours (some negative)
fn random_event(r: &mut Mix) -> String {
    // most events come from the instruments; the rest from a person, the individual itself, the clock, nowhere or a malformed one
    const ENV: [&str; 3] = [r#""env""#, r#""env:runner""#, r#""env:host""#];
    const OTHER: [&str; 5] = [r#""human:ana""#, r#""self""#, r#""clock""#, r#""env:""#, "7"];
    let mut kv: Vec<String> = vec![];
    match r.below(20) { k if k < 14 => kv.push(format!(r#""src":{}"#, ENV[r.below(3)])), k if k < 19 => kv.push(format!(r#""src":{}"#, OTHER[k as usize - 14])), _ => {} }
    let mut rd: Vec<String> = vec![];
    let num = |r: &mut Mix, lo: f64, hi: f64| -> String { match r.below(30) { 0 => format!("{}", lo - 1.0), 1 => format!("{}", hi + 1.0), 2 => r#""9""#.into(), _ => format!("{}", ((lo + r.unit() * (hi - lo)) * 1000.0).round() / 1000.0) } };
    if r.below(2) == 0 { rd.push(format!(r#""tool_fail":{}"#, match r.below(20) { 0 => "1", 1 => r#"{"v":true,"src":"self"}"#, 2 => r#"{"v":false,"src":"env:ci"}"#, k if k < 11 => "true", _ => "false" })); }
    if r.below(2) == 0 { let v = num(r, 0.0, 30.0); rd.push(format!(r#""tool_latency":{v}"#)); }
    if r.below(2) == 0 { let v = num(r, 0.0, 200.0); rd.push(if r.below(5) == 0 { format!(r#""tempo":{{"v":{v},"src":"env:chat"}}"#) } else { format!(r#""tempo":{v}"#) }); }
    if r.below(3) == 0 { let v = num(r, 0.0, 40.0); rd.push(format!(r#""silence":{v}"#)); }
    if r.below(25) == 0 { rd.push(r#""latency":3"#.into()); }
    if !rd.is_empty() || r.below(20) == 0 { kv.push(format!(r#""senses":{{{}}}"#, rd.join(","))); }
    if r.below(6) == 0 { kv.push(r#""praise":true"#.into()); } else if r.below(6) == 0 { kv.push(r#""criticism":true"#.into()); }
    kv.push(format!(r#""elapsed_hours":{}"#, match r.below(40) { 0 => "-1".to_string(), 1 => "720".to_string(), _ => format!("{}", r.below(8) as f64 * 0.25) }));
    format!("{{{}}}", kv.join(","))
}

/// P3s: a refused event changes nothing. 40 individuals x 300 random events mixing sources, readings, `self`, `clock` and
/// out-of-range values: the run with the refused events equals the run without them, byte for byte (every stance, the strand
/// with a checkpoint after each event, the final state)
#[test]
fn p3s_a_refused_event_changes_nothing() {
    let (mut refused, mut accepted) = (0usize, 0usize);
    for seed in 0..40u64 {
        let mut r = Mix(0x5E45_E500 + seed); let evs: Vec<String> = (0..300).map(|_| random_event(&mut r)).collect();
        let all = events_file("p3s-all.jsonl", &evs);
        let st = init(SENSED, seed, "p3s-state.json"); let strand = tmp("p3s.strand"); clean(&[&strand]);
        let (c, out, _) = live(SENSED, &st, &all, &strand, "1");
        let docs: Vec<&str> = out.lines().collect(); assert_eq!(docs.len(), 300, "seed {seed}: one line per event");
        let ok: Vec<usize> = (0..300).filter(|&i| !docs[i].starts_with(r#"{"error""#)).collect();
        assert_eq!(c, if ok.len() < 300 { 2 } else { 0 }, "seed {seed}");
        refused += 300 - ok.len(); accepted += ok.len();
        let kept = events_file("p3s-kept.jsonl", &ok.iter().map(|&i| evs[i].clone()).collect::<Vec<_>>());
        let st2 = init(SENSED, seed, "p3s-state2.json"); let strand2 = tmp("p3s2.strand"); clean(&[&strand2]);
        let (c, out2, e) = live(SENSED, &st2, &kept, &strand2, "1"); assert_eq!(c, 0, "seed {seed}: {e}");
        assert_eq!(out2, ok.iter().map(|&i| format!("{}\n", docs[i])).collect::<String>(), "seed {seed}: the stances");
        assert_eq!(read(&strand2), read(&strand), "seed {seed}: the strand"); assert_eq!(read(&st2), read(&st), "seed {seed}: the state");
        clean(&[&st, &strand, &st2, &strand2, &all, &kept]);
    }
    eprintln!("p3s: {refused} refused, {accepted} accepted"); assert!(refused >= 1000 && accepted >= 4000, "a mix of both: {refused} refused, {accepted} accepted");
}

/// `probbit check` reuses the lint pipeline: on the sensed persona both habit rules are held by construction, and a sensed
/// strand replays
#[test]
fn probbit_check_holds_a_sensed_persona() {
    let (c, out, e) = probbit(&["check", SENSED, "--props", SENSED_PROPS]); assert_eq!(c, 0, "{out}\n{e}");
    for id in ["no_bold_after_failure", "short_when_busy"] {
        assert!(out.lines().any(|l| l.starts_with("rule    ") && l.contains(id) && l.ends_with("  held_by_construction")), "{id}: {out}"); }
    assert!(out.ends_with("check: held 2 · broken 0 · unproven 0 · strands 0/0\n"), "{out}");
    let ev = events_file("check.jsonl", &day()); let st = init(SENSED, 1, "check-state.json"); let strand = tmp("check.strand"); clean(&[&strand]);
    let (c, _, e) = live(SENSED, &st, &ev, &strand, "8"); assert_eq!(c, 0, "{e}");
    let (c, out, e) = probbit(&["check", SENSED, "--props", SENSED_PROPS, "--seeds", "0-9", "--strand", &strand]); assert_eq!(c, 0, "{out}\n{e}");
    assert!(out.ends_with("check: held 2 · broken 0 · unproven 0 · strands 1/1\n"), "{out}");
    clean(&[&st, &strand, &ev]);
}
