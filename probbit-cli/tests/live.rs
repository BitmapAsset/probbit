//! `probbit live` with senses (docs/persona.md §2.11, §5.7): readings logged in the strand as used, `live verify` recomputing the
//! normals (summary `normals` with `max_move_50`), a life split across runs equal to one run, the one stderr line for readings
//! sent to a persona without the block, the refusal property P3s, and `probbit check` on a sensed persona. With action-addressed
//! feedback (§2.8): the refusal property P3c with random payments, every refusal row byte for byte, control lines closing open
//! decisions, and `probbit check` on a credit persona. Unsupported is not diverges (§5.7): a newer engine's strand (a block or a
//! line kind this engine does not know) is unsupported (exit 5), a tampered one still diverges (exit 1), and `probbit check`
//! says which.
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

const ADDRESSED: &str = "examples/agent-harness/addressed.json";
fn lines_of(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

/// P3c's generator for the addressed policy (a credit block, max_open 8, half_life_events 32, `reward_from: [env]`): a model of
/// the bookkeeping (RFC 0001 B.2: the next turn, the open decisions after expiry, eviction beyond max_open, which turns were
/// awaited) picks mostly valid payments (late, reordered, the previous stance by name, unaddressed when unambiguous, cancels)
/// and some that a row refuses (B-R1 ambiguous, B-R2 paid/cancelled/evicted/never opened/to come, B-R3, B-R4, B-R5, B-R6),
/// and predicts whether the engine accepts each event
struct Book { t: u64, open: Vec<u64>, awaited: Vec<u64> }
impl Book {
    fn event(&mut self, r: &mut Mix) -> (String, bool) {
        let t = self.t; let prev = t.checked_sub(1); let live: Vec<u64> = self.open.iter().copied().filter(|n| t - n - 1 < 128).collect();
        let gone: Vec<u64> = self.awaited.iter().copied().filter(|n| !live.contains(n) && Some(*n) != prev).collect();
        let never: Vec<u64> = (0..t).filter(|n| !self.awaited.contains(n) && Some(*n) != prev).collect();
        let pick = |r: &mut Mix, v: &[u64]| v[r.below(v.len())];
        let ambiguous = live.iter().any(|n| Some(*n) != prev);
        let (mut kv, mut ok, mut pays, mut cancels): (Vec<String>, bool, Option<u64>, Option<u64>) = (vec![], true, None, None);
        match r.below(100) {
            0..=39 => {} // a decision (when awaited) or an idle event
            40..=74 => { kv.push(r#""praise":true,"src":"env:probe""#.into());
                match r.below(20) {
                    0..=8 if !live.is_empty() => { let n = pick(r, &live); kv.push(format!(r#""for":{n}"#)); pays = Some(n); } // late or reordered
                    9 | 10 if prev.is_some() => { kv.push(format!(r#""for":{}"#, t - 1)); pays = prev; }                     // the previous stance by name
                    13 | 14 if !gone.is_empty() => { kv.push(format!(r#""for":{}"#, pick(r, &gone))); ok = false; }       // paid, cancelled, evicted, expired
                    15 if !never.is_empty() => { kv.push(format!(r#""for":{}"#, pick(r, &never))); ok = false; }           // never opened
                    16 => { kv.push(format!(r#""for":{}"#, t + 2)); ok = false; }                                                // still to come
                    17 if !live.is_empty() => { kv.push(format!(r#""for":{{"turn":{},"stance":"sha256:{}"}}"#, pick(r, &live), "0".repeat(64))); ok = false; } // B-R6
                    18 => { kv.push(r#""for":1.5"#.into()); ok = false; }                                                         // B-R5
                    _ => { if ambiguous { ok = false; } else { pays = prev; } } } }                                               // unaddressed: B-R1 when ambiguous
            75..=86 => { kv.push(r#""src":"env:executor""#.into());
                match r.below(10) { 0..=6 if !live.is_empty() => { let n = pick(r, &live); kv.push(format!(r#""cancel":[{n}]"#)); cancels = Some(n); }
                    7 if !live.is_empty() => { let n = pick(r, &live); kv.push(format!(r#""cancel":[{n},{n}]"#)); ok = false; }  // B-R4 twice
                    _ => { kv.push(format!(r#""cancel":[{}]"#, if gone.is_empty() { t + 5 } else { pick(r, &gone) })); ok = false; } } } // B-R4 not open
            _ => { ok = false; kv.push(match r.below(5) { 0 => r#""await":"yes""#.to_string(), 1 => r#""praise":true,"src":"self""#.into(), 2 => r#""praise":true"#.into(),
                3 => format!(r#""cancel":[{}],"src":"self""#, t.saturating_sub(1)), _ => format!(r#""for":{}"#, t.saturating_sub(1)) }); } }      // B-R5, provenance, B-R3
        let wait = r.below(10) < 4 && !kv.iter().any(|k| k.contains("await")); if wait { kv.push(r#""await":true"#.into()); }
        if r.below(5) == 0 { kv.push(r#""error":true"#.into()); }
        if r.below(4) == 0 { kv.push(format!(r#""elapsed_hours":{}"#, r.below(4) as f64 * 0.5)); }
        if ok { self.open = live.into_iter().filter(|n| Some(*n) != pays && Some(*n) != cancels).collect();
            if wait { self.open.push(t); self.awaited.push(t); while self.open.len() > 8 { self.open.remove(0); } } self.t += 1; }
        (format!("{{{}}}", kv.join(",")), ok)
    }
}

/// P3c: a refused event changes nothing, with payments. 40 individuals x 300 random events with random `await`, `for` and
/// `cancel` (late, reordered, duplicated, cancelled, unknown, mismatched, malformed): the run with the refused events equals the
/// run without them, byte for byte (every stance, the strand with a checkpoint after each event, the final state)
#[test]
fn p3c_a_refused_payment_changes_nothing() {
    let (mut refused, mut accepted, mut paid, mut late) = (0usize, 0usize, 0usize, 0usize);
    for seed in 0..40u64 {
        let (mut r, mut book) = (Mix(0xC4ED_17C0 + seed), Book { t: 0, open: vec![], awaited: vec![] });
        let (evs, model): (Vec<String>, Vec<bool>) = (0..300).map(|_| book.event(&mut r)).unzip();
        let all = events_file("p3c-all.jsonl", &evs);
        let st = init(ADDRESSED, seed, "p3c-state.json"); let strand = tmp("p3c.strand"); clean(&[&strand]);
        let (c, out, _) = live(ADDRESSED, &st, &all, &strand, "1");
        let docs: Vec<&str> = out.lines().collect(); assert_eq!(docs.len(), 300, "seed {seed}: one line per event");
        let ok: Vec<usize> = (0..300).filter(|&i| !docs[i].starts_with(r#"{"error""#)).collect();
        for i in 0..300 { assert_eq!(ok.contains(&i), model[i], "seed {seed} event {i}: the model of B.2-B.3 and the engine disagree on {}: {}", evs[i], docs[i]); }
        assert_eq!(c, if ok.len() < 300 { 2 } else { 0 }, "seed {seed}");
        refused += 300 - ok.len(); accepted += ok.len();
        for &i in &ok { let p = s(&parse(docs[i]), &["credit", "paid"]).clone(); if p != Json::Null { paid += 1; if s(&p, &["e"]).as_f64() != Some(1.0) { late += 1; } } }
        let kept = events_file("p3c-kept.jsonl", &ok.iter().map(|&i| evs[i].clone()).collect::<Vec<_>>());
        let st2 = init(ADDRESSED, seed, "p3c-state2.json"); let strand2 = tmp("p3c2.strand"); clean(&[&strand2]);
        let (c, out2, e) = live(ADDRESSED, &st2, &kept, &strand2, "1"); assert_eq!(c, 0, "seed {seed}: {e}");
        assert_eq!(out2, ok.iter().map(|&i| format!("{}\n", docs[i])).collect::<String>(), "seed {seed}: the stances");
        assert_eq!(read(&strand2), read(&strand), "seed {seed}: the strand"); assert_eq!(read(&st2), read(&st), "seed {seed}: the state");
        let (c, v, e) = probbit(&["live", "verify", &strand]); assert_eq!((c, s(&parse(&v), &["ok"])), (0, &Json::Bool(true)), "seed {seed}: {e}");
        clean(&[&st, &strand, &st2, &strand2, &all, &kept]);
    }
    eprintln!("p3c: {refused} refused, {accepted} accepted, {paid} paid ({late} late)");
    assert!(refused >= 1000 && accepted >= 4000 && late >= 200, "a mix: {refused} refused, {accepted} accepted, {paid} paid, {late} late");
}

/// Each refusal row (B-R1 to B-R6) refuses its event whole: one {"error"} at its path, exit 2, and the run equals the run without
/// the event, byte for byte (stances, strand, state)
#[test]
fn every_refusal_row_changes_nothing() {
    // turn 0 and 1 await, 2 pays 0, 3 awaits and 4 cancels it, 5 awaits: then turn 6 has 1 and 5 open, 5 the previous stance
    let base = lines_of(&[r#"{"await":true}"#, r#"{"await":true,"error":true,"src":"env:probe"}"#, r#"{"praise":true,"for":0,"src":"env:probe"}"#, r#"{"await":true}"#,
        r#"{"cancel":[3],"src":"env:executor"}"#, r#"{"await":true}"#]);
    let tail = lines_of(&[r#"{"praise":true,"for":5,"src":"env:probe"}"#, r#"{"cancel":[1],"src":"env:executor"}"#, r#"{"praise":true,"src":"env:probe"}"#]);
    let z = "0".repeat(64);
    let rows: Vec<(&str, String, &str)> = vec![("B-R1", r#"{"praise":true,"src":"env:probe"}"#.into(), "inputs.for"), ("B-R2 paid", r#"{"praise":true,"for":0,"src":"env:probe"}"#.into(), "inputs.for"),
        ("B-R2 cancelled", r#"{"praise":true,"for":3,"src":"env:probe"}"#.into(), "inputs.for"), ("B-R2 never opened", r#"{"praise":true,"for":2,"src":"env:probe"}"#.into(), "inputs.for"),
        ("B-R2 to come", r#"{"praise":true,"for":9,"src":"env:probe"}"#.into(), "inputs.for"), ("B-R3", r#"{"for":1}"#.into(), "inputs.for"),
        ("B-R4 not open", r#"{"cancel":[3],"src":"env:executor"}"#.into(), "inputs.cancel[0]"), ("B-R4 twice", r#"{"cancel":[1,1],"src":"env:executor"}"#.into(), "inputs.cancel[1]"),
        ("B-R4 paid now", r#"{"praise":true,"for":1,"cancel":[1],"src":"env:probe"}"#.into(), "inputs.cancel[0]"), ("B-R5 await", r#"{"await":1}"#.into(), "inputs.await"),
        ("B-R5 for", r#"{"praise":true,"for":-1,"src":"env:probe"}"#.into(), "inputs.for"), ("B-R5 cancel", r#"{"cancel":3,"src":"env:executor"}"#.into(), "inputs.cancel"),
        ("B-R5 cancel src", r#"{"cancel":[1],"src":"self"}"#.into(), "inputs.src"),
        ("B-R6", format!(r#"{{"praise":true,"for":{{"turn":1,"stance":"sha256:{z}"}},"src":"env:probe"}}"#), "inputs.for.stance")];
    let without: Vec<String> = base.iter().chain(&tail).cloned().collect(); let ev0 = events_file("rows-without.jsonl", &without);
    let st0 = init(ADDRESSED, 3, "rows-state0.json"); let strand0 = tmp("rows0.strand"); clean(&[&strand0]);
    let (c, out0, e) = live(ADDRESSED, &st0, &ev0, &strand0, "1"); assert_eq!(c, 0, "{e}");
    for (row, ev, path) in &rows {
        let with: Vec<String> = base.iter().cloned().chain([ev.clone()]).chain(tail.iter().cloned()).collect(); let ev1 = events_file("rows-with.jsonl", &with);
        let st = init(ADDRESSED, 3, "rows-state.json"); let strand = tmp("rows.strand"); clean(&[&strand]);
        let (c, out, e) = live(ADDRESSED, &st, &ev1, &strand, "1"); assert_eq!(c, 2, "{row}: {e}");
        let lines: Vec<&str> = out.lines().collect(); let err = parse(lines[6]);
        assert!(err.get("error").is_some() && jw(&err).contains(&format!("\"events[6].{path}\"")), "{row}: {}", lines[6]);
        assert_eq!(lines.iter().enumerate().filter(|(i, _)| *i != 6).map(|(_, l)| format!("{l}\n")).collect::<String>(), out0, "{row}: the stances");
        assert_eq!((read(&strand), read(&st)), (read(&strand0), read(&st0)), "{row}: the strand and the state");
        clean(&[&st, &strand, &ev1]);
    }
    clean(&[&st0, &strand0, &ev0]);
}

/// A control line closes every open decision (closed as control): `live verify` lists them in `open` before it and none after,
/// the next stance lists them as closed, a payment naming one is refused (B-R2), and the strand replays
#[test]
fn control_lines_close_open_decisions() {
    let st = init(ADDRESSED, 0, "ctl-state.json"); let strand = tmp("ctl.strand"); clean(&[&strand]);
    let ev = events_file("ctl1.jsonl", &lines_of(&[r#"{"await":true}"#, r#"{"await":true}"#, "{}"]));
    let (c, _, e) = live(ADDRESSED, &st, &ev, &strand, "1"); assert_eq!(c, 0, "{e}");
    let open = |f: &str| { let (c, v, e) = probbit(&["live", "verify", f]); assert_eq!(c, 0, "{e}"); let v = parse(&v); assert_eq!(s(&v, &["ok"]), &Json::Bool(true)); jw(s(&v, &["open"])) };
    assert_eq!(open(&strand), "[0,1]");
    for what in ["pause", "resume"] { let (c, _, e) = probbit(&["live", "control", &strand, what, "--by", "human:ops", "--reason", "maintenance", "--at", "2026-10-14T09:00:00Z"]); assert_eq!(c, 0, "{e}"); }
    assert_eq!(open(&strand), "[]");
    let ev2 = events_file("ctl2.jsonl", &lines_of(&[r#"{"praise":true,"for":1,"src":"env:probe"}"#, r#"{"await":true}"#, r#"{"praise":true,"src":"env:probe"}"#]));
    let (c, out, e) = live(ADDRESSED, &st, &ev2, &strand, "1"); assert_eq!(c, 2, "{e}");
    let docs: Vec<Json> = out.lines().map(parse).collect(); assert_eq!(docs.len(), 3);
    assert!(docs[0].get("error").is_some() && jw(&docs[0]).contains("inputs.for"), "{}", jw(&docs[0]));
    assert_eq!((jw(s(&docs[1], &["credit", "closed"])), jw(s(&docs[1], &["credit", "open"]))), (r#"[{"as":"control","turn":0},{"as":"control","turn":1}]"#.into(), "[3]".into()));
    assert_eq!((s(&docs[2], &["credit", "paid", "turn"]).as_f64(), s(&docs[2], &["credit", "paid", "e"]).as_f64()), (Some(3.0), Some(1.0)), "the previous stance, paid unaddressed");
    assert_eq!(open(&strand), "[]");
    clean(&[&st, &strand, &ev, &ev2]);
}

/// `probbit check` on a persona with a credit block: the retry rule held by its habit, and a strand with late, cancelled and
/// digest-bound payments replays
#[test]
fn probbit_check_holds_a_credit_persona() {
    let props = tmp("retry-props.json"); std::fs::write(&props, r#"{"props":[{"id":"ask_after_two_errors","when":{"error_streak":2},"then":{"action":["ask"]}}]}"#).unwrap();
    let golden = parse(&read(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/agent-harness/golden/events.json")));
    let ev = events_file("check-credit.jsonl", &golden.as_arr().unwrap().iter().map(jw).collect::<Vec<_>>());
    let st = init(ADDRESSED, 0, "check-credit-state.json"); let strand = tmp("check-credit.strand"); clean(&[&strand]);
    let (c, _, e) = live(ADDRESSED, &st, &ev, &strand, "4"); assert_eq!(c, 0, "{e}");
    let (c, out, e) = probbit(&["check", ADDRESSED, "--props", &props, "--seeds", "0-9", "--strand", &strand]); assert_eq!(c, 0, "{out}\n{e}");
    assert!(out.lines().any(|l| l.starts_with("rule    ") && l.contains("ask_after_two_errors") && l.contains("held")), "{out}");
    assert!(out.ends_with("check: held 1 · broken 0 · unproven 0 · strands 1/1\n"), "{out}");
    clean(&[&st, &strand, &ev, &props]);
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

/// The fixtures of "unsupported is not diverges" (docs/persona.md §5.7), in probbit-cli/tests/fixtures/strands/:
/// `sensed-credit-0.9.0-dev.strand`, recorded by this build (`probbit live sensed-credit.json --seed 0 --clock fixed --events
/// sensed-credit.events.jsonl --strand ... --checkpoint-every 4`: a senses block and a learning.credit block, 5 events, a
/// checkpoint at line 6); `sensed-credit-0.9.0-dev.verify-0.8.2.txt`, what the released 0.8.2 binary printed on it (it cannot be
/// changed); and one version up, derived from the 0.9.0 strand (not recordings: no engine with these exists yet), the header's
/// engine set to "probbit 0.10.0-dev" and every later line re-chained: `next-load.strand` (the header's persona document gains
/// RFC 0002 part 2's `load` block, which no engine has yet, its digest recomputed for it) and `next-rest.strand` (a rest line appended as line 8).
const STRANDS: &str = "probbit-cli/tests/fixtures/strands";
fn fixture(name: &str) -> String { format!("{STRANDS}/{name}") }
fn fixture_text(name: &str) -> String { read(&format!("{}/../{STRANDS}/{name}", env!("CARGO_MANIFEST_DIR"))) }
const THIS: &str = concat!("probbit ", env!("CARGO_PKG_VERSION"));

/// (i) the 0.9.0 strand verifies here (exit 0); the released 0.8.2 binary called it a divergence at line 1 (exit 1). (ii) one
/// version up, a block or a line kind this engine does not know is unsupported (exit 5): the document {engine, line, ok: false,
/// strand_engine, unsupported} on stdout and `line K: unsupported by this engine: FIELD` on stderr, never `diverges`, from the
/// header, from the last checkpoint and with --actions alike
#[test]
fn a_newer_engines_strand_is_unsupported_not_a_divergence() {
    let (c, out, e) = probbit(&["live", "verify", &fixture("sensed-credit-0.9.0-dev.strand")]); assert_eq!(c, 0, "{e}");
    let v = parse(&out); assert_eq!((s(&v, &["ok"]), s(&v, &["events"]), s(&v, &["engine"])), (&Json::Bool(true), &Json::Num(5.0), &Json::Str("probbit 0.9.0-dev".into())));
    assert!(v.get("normals").is_some() && v.get("open").is_some(), "the senses and the credit replayed: {out}");
    // the released 0.8.2 binary's recorded answer: the tamper verdict on a correct strand
    let rec = fixture_text("sensed-credit-0.9.0-dev.verify-0.8.2.txt");
    assert!(rec.starts_with("# probbit 0.8.2, the released binary") && rec.lines().any(|l| l == "# exit 1"), "{rec}");
    let old = parse(rec.lines().find(|l| !l.starts_with('#')).unwrap());
    assert_eq!((s(&old, &["ok"]), s(&old, &["line"])), (&Json::Bool(false), &Json::Num(1.0)));
    assert!(s(&old, &["diverges"]).as_str().unwrap().starts_with("the persona document: senses: unknown field"), "{rec}");
    // one version up: unsupported, at line 1 for the header's persona, at line 8 for the rest line
    let actions = tmp("unsupported-actions.jsonl"); std::fs::write(&actions, "{\"turn\":0,\"action\":{\"action\":\"retry\"}}\n").unwrap();
    for (name, line, field) in [("next-load.strand", 1, "load"), ("next-rest.strand", 8, "rest")] {
        let want = format!(r#"{{"engine":"{THIS}","line":{line},"ok":false,"strand_engine":"probbit 0.10.0-dev","unsupported":"{field}"}}"#);
        let say = format!("probbit: live verify: line {line}: unsupported by this engine: {field} (written by probbit 0.10.0-dev; this engine is {THIS}): not verified, and not a divergence");
        for extra in [vec![], vec!["--from-checkpoint"], vec!["--actions", actions.as_str()]] {
            let f = fixture(name); let mut a = vec!["live", "verify", f.as_str()]; a.extend(extra.iter().copied());
            let (c, out, e) = probbit(&a);
            assert_eq!((c, out.trim_end(), e.trim_end()), (5, want.as_str(), say.as_str()), "{name} {extra:?}");
            assert!(!out.contains("diverges") && !e.contains("diverges"), "{name} {extra:?}");
        }
    }
    clean(&[&actions]);
}

/// A tampered strand still diverges at its line (exit 1), next to an unsupported one too: an input changed, a known header value
/// changed, a line removed after the unknown block, the rest line's prev changed. And `probbit check` reports an unsupported strand
/// as unsupported (never FAIL), fails the gate (exit 1) and names the field and the strand's engine in --json.
#[test]
fn a_tampered_strand_still_diverges_and_check_says_unsupported() {
    let t = fixture_text("sensed-credit-0.9.0-dev.strand"); let load = fixture_text("next-load.strand"); let rest = fixture_text("next-rest.strand");
    let lines = |x: &str| x.lines().map(str::to_string).collect::<Vec<_>>();
    let mut cut = lines(&load); cut.remove(2);
    let mut moved = lines(&rest); let last = moved.len() - 1;
    let k = moved[last].find("\"prev\":\"sha256:").unwrap() + "\"prev\":\"sha256:".len();
    let flip = if &moved[last][k..k + 1] == "0" { "1" } else { "0" }; moved[last].replace_range(k..k + 1, flip);
    for (name, text, line, why) in [
        ("an input changed", t.replacen(r#""error":true,"senses":{"tool_fail":true}"#, r#""error":false,"senses":{"tool_fail":false}"#, 1), 3, "the stance differs"),
        ("a known header value changed", t.replacen(r#""seed":0,"state""#, r#""seed":1,"state""#, 1), 1, "the header is not as written"),
        ("a line removed after the unknown block", cut.join("\n") + "\n", 3, "prev is not the sha256 of the line before (a line before it was changed, removed or reordered)"),
        ("the rest line's prev changed", moved.join("\n") + "\n", 8, "prev is not the sha256 of the line before (a line before it was changed, removed or reordered)")] {
        let f = tmp(&format!("tampered-{}.strand", name.replace(' ', "-"))); std::fs::write(&f, &text).unwrap();
        let (c, out, e) = probbit(&["live", "verify", &f]);
        assert_eq!((c, out.trim_end()), (1, format!(r#"{{"diverges":"{why}","line":{line},"ok":false}}"#).as_str()), "{name}: {e}");
        clean(&[&f]);
    }
    // probbit check: the 0.9.0 strand replays, the newer one is unsupported (its line, its field, the engine that wrote it)
    let persona = fixture("sensed-credit.json"); let (ok, nx) = (fixture("sensed-credit-0.9.0-dev.strand"), fixture("next-load.strand"));
    let (c, out, e) = probbit(&["check", &persona, "--seeds", "0-1", "--strand", &ok, "--strand", &nx]); assert_eq!(c, 1, "{out}\n{e}");
    assert!(out.lines().any(|l| l.starts_with("strand  ") && l.contains(&ok) && l.ends_with("  ok 5 events")), "{out}");
    assert!(out.lines().any(|l| l.starts_with("strand  ") && l.contains(&nx) && l.ends_with("  line 1: unsupported by this engine: load (written by probbit 0.10.0-dev)")), "{out}");
    assert!(!out.contains("FAIL") && !out.contains("diverges"), "{out}");
    let (c, out, e) = probbit(&["check", &persona, "--seeds", "0-1", "--strand", &nx, "--json"]); assert_eq!(c, 1, "{e}");
    let d = parse(&out); let st = &s(&d, &["strands"]).as_arr().unwrap()[0];
    assert_eq!(jw(st), format!(r#"{{"error":"line 1: unsupported by this engine: load","events":null,"final_state":null,"ok":false,"path":"{nx}","strand_engine":"probbit 0.10.0-dev","unsupported":"load"}}"#));
    assert_eq!((s(&d, &["verdict", "strands_ok"]), s(&d, &["verdict", "exit"])), (&Json::Num(0.0), &Json::Num(1.0)));
}

// ------------------------------------------------------------------- RFC 0002 wave 1a: needs, the omission let-down, valence
const BODY: &str = "examples/persona/body.yaml";
const BODY_PROPS: &str = "examples/persona/body-props.json";
fn body_day() -> Vec<String> { parse(&read(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/persona/golden/body/day.json"))).as_arr().unwrap().iter().map(jw).collect() }
/// The first line of `text` that starts with `prefix`, with the number after its first `key` replaced by `v`
fn plant(text: &str, prefix: &str, key: &str, v: &str) -> String {
    let mut out: Vec<String> = vec![]; let mut done = false;
    for l in text.lines() { match (done, l.starts_with(prefix), l.find(key)) {
        (false, true, Some(i)) => { let j = i + key.len(); let end = l[j..].find(|c: char| c != '.' && c != '-' && !c.is_ascii_digit()).map_or(l.len(), |k| j + k);
            out.push(format!("{}{v}{}", &l[..j], &l[end..])); done = true; }
        _ => out.push(l.to_string()) } }
    assert!(done, "nothing planted"); out.iter().map(|l| format!("{l}\n")).collect()
}

/// A body strand (needs, windows, valence, liking) verifies: `live verify` recomputes the deficits, the success rates and the
/// windows from the logged amounts, readings and stamped hours (summary `needs`: per need its deficit now and the hours it sat at
/// its cap), and a changed amount, a checkpoint's changed deficit or success rate, or a changed window fails the replay
#[test]
fn a_body_strand_verifies_and_recomputes_deficits_and_windows() {
    let ev = events_file("body.jsonl", &body_day()); let st = init(BODY, 7, "body-state.json"); let strand = tmp("body.strand"); clean(&[&strand]);
    let (c, out, e) = live(BODY, &st, &ev, &strand, "4"); assert_eq!(c, 0, "{e}"); assert_eq!(out.lines().count(), 24);
    let (c, v, e) = probbit(&["live", "verify", &strand]); assert_eq!(c, 0, "{e}"); let v = parse(&v); assert_eq!(s(&v, &["ok"]), &Json::Bool(true));
    let fin = parse(&read(&st));
    for id in ["context", "contact"] { assert_eq!(s(&v, &["needs", id, "deficit"]), s(&fin, &["needs", "deficit", id]), "{id}"); }
    // context sat at its cap for the half hour after event 13, contact for the hour after the 24 h gap
    assert_eq!((s(&v, &["needs", "context", "at_cap_hours"]).as_f64(), s(&v, &["needs", "contact", "at_cap_hours"]).as_f64()), (Some(0.5), Some(1.0)), "{}", jw(s(&v, &["needs"])));
    let text = read(&strand);
    assert!(text.contains(r#""needs":{"contact":{"amount":0.8,"src":"human:principal"}},"src":"env:host"}"#), "an amount is logged as given, with its own source");
    for (what, bad) in [("an amount", text.replacen(r#""amount":0.6"#, r#""amount":0.5"#, 1)),
        ("a checkpoint deficit", plant(&text, r#"{"checkpoint""#, r#""needs":{"deficit":{"contact":"#, "0.5")),
        ("a checkpoint success rate", plant(&text, r#"{"checkpoint""#, r#""success":{"compact":"#, "0.5")),
        ("a checkpoint window", plant(&text, r#"{"checkpoint""#, r#""window":{"compact":"#, "0.5"))] {
        let f = tmp("body-bad.strand"); std::fs::write(&f, &bad).unwrap(); let (c, v, _) = probbit(&["live", "verify", &f]);
        assert_ne!(c, 0, "{what}: {v}"); assert_eq!(s(&parse(&v), &["ok"]), &Json::Bool(false), "{what}"); clean(&[&f]); }
    // windows: open after a pursuit, count down by the stamped hours, fire once at 0 (the why says so), close on a win
    let docs: Vec<Json> = out.lines().map(parse).collect(); let w = |i: usize, g: &str| s(&docs[i], &["drives", "window", g]).as_f64();
    let pursued = |i: usize| s(&docs[i], &["pursue", "goal"]).as_str().unwrap().to_string();
    let why = |i: usize| s(&docs[i], &["why"]).as_str().unwrap().to_string();
    assert_eq!((pursued(2), w(3, "compact")), ("compact".into(), None), "event 3, an hour later: the window fired");
    assert!(why(3).contains("an expected win did not come (compact the context)") && !why(4).contains("did not come"), "{}", why(3));
    assert_eq!((pursued(20), w(21, "report")), ("report".into(), None)); assert!(why(21).contains("an expected win did not come (report to the principal)"), "{}", why(21));
    let tail = events_file("body-tail.jsonl", &[r#"{"elapsed_hours":1,"src":"env:host"}"#.to_string(), r#"{"elapsed_hours":1,"src":"env:host"}"#.to_string(), r#"{"elapsed_hours":1,"src":"env:host"}"#.to_string()]);
    let st2 = init(BODY, 7, "body-state2.json"); let strand2 = tmp("body2.strand"); clean(&[&strand2]);
    let head = events_file("body-head.jsonl", &body_day()[..21]); let (c, o1, e) = live(BODY, &st2, &head, &strand2, "0"); assert_eq!(c, 0, "{e}");
    assert_eq!(s(&parse(o1.lines().last().unwrap()), &["pursue", "goal"]), &Json::Str("report".into()));
    let (c, o2, e) = live(BODY, &st2, &tail, &strand2, "0"); assert_eq!(c, 0, "{e}"); let t: Vec<Json> = o2.lines().map(parse).collect();
    assert_eq!(s(&t[0], &["drives", "window", "report"]).as_f64(), Some(1.0), "counted down by 1 h");
    assert!(s(&t[1], &["drives", "window", "report"]).is_null() && s(&t[1], &["why"]).as_str().unwrap().contains("an expected win did not come (report to the principal)"));
    assert!(!s(&t[2], &["why"]).as_str().unwrap().contains("did not come"), "fires once");
    clean(&[&st, &strand, &ev, &st2, &strand2, &head, &tail]);
}

/// Replay: a body life split across two runs equals the life in one run, byte for byte (strand, state, stances)
#[test]
fn a_body_life_split_across_runs_equals_one_run() {
    let lines = body_day(); let all = events_file("body-all.jsonl", &lines);
    let st1 = init(BODY, 2, "body-one-state.json"); let one = tmp("body-one.strand"); clean(&[&one]);
    let (c, out1, e) = live(BODY, &st1, &all, &one, "3"); assert_eq!(c, 0, "{e}");
    for k in [1, 10, 21] {
        let (a, b) = (events_file("body-a.jsonl", &lines[..k]), events_file("body-b.jsonl", &lines[k..]));
        let st2 = init(BODY, 2, "body-two-state.json"); let two = tmp("body-two.strand"); clean(&[&two]);
        let (c, o1, e) = live(BODY, &st2, &a, &two, "3"); assert_eq!(c, 0, "{e}");
        let (c, o2, e) = live(BODY, &st2, &b, &two, "3"); assert_eq!(c, 0, "{e}");
        assert_eq!(read(&two), read(&one), "split at {k}: the strand"); assert_eq!(read(&st2), read(&st1), "split at {k}: the state");
        assert_eq!(format!("{o1}{o2}"), out1, "split at {k}: the stances");
        clean(&[&st2, &two, &a, &b]);
    }
    let (c, v, e) = probbit(&["live", "verify", &one]); assert_eq!(c, 0, "{e}"); assert_eq!(s(&parse(&v), &["ok"]), &Json::Bool(true));
    clean(&[&st1, &one, &all]);
}

/// `needs` sent to a persona without the block is an undeclared input (listed in `ignored`): `live` says so once on stderr,
/// naming the first event that carried it; a needs win and its bound goal's explicit win in one event are refused (N-E3) at the
/// amount's path, and the run goes on as without that event
#[test]
fn needs_without_the_block_are_named_once_and_one_outcome_pays_once() {
    let line = |i: usize| format!("probbit: live: events[{i}]: this persona has no needs block: `needs` is an undeclared input (listed in ignored), no amount is paid");
    let ev = events_file("tutor-needs.jsonl", &[r#"{"elapsed_hours":1}"#.into(), r#"{"needs":{"context":0.6}}"#.into(), r#"{"needs":{"contact":0.8},"error":true}"#.into()]);
    let st = init(TUTOR, 2, "tutor-needs-state.json"); let strand = tmp("tutor-needs.strand"); clean(&[&strand]);
    let (c, out, e) = live(TUTOR, &st, &ev, &strand, "0"); assert_eq!(c, 0, "{e}");
    assert_eq!(e.lines().filter(|l| l.contains("needs block")).collect::<Vec<_>>(), [line(1).as_str()], "{e}");
    let docs: Vec<Json> = out.lines().map(parse).collect(); assert_eq!(jw(s(&docs[2], &["ignored"])), r#"["needs"]"#);
    assert!(docs.iter().all(|d| d.get("needs").is_none()), "no new stance keys without the block");
    let base = vec![body_day()[0].clone(), body_day()[1].clone(), body_day()[2].clone()];
    let with: Vec<String> = base.iter().take(2).cloned().chain([r#"{"elapsed_hours":1,"needs":{"context":0.6},"goals":{"compact":{"win":1}},"src":"env:host"}"#.to_string()]).chain(base.iter().skip(2).cloned()).collect();
    let (st1, st2) = (init(BODY, 5, "nE3-a.json"), init(BODY, 5, "nE3-b.json")); let (s1, s2) = (tmp("nE3-a.strand"), tmp("nE3-b.strand")); clean(&[&s1, &s2]);
    let (e1, e2) = (events_file("nE3-with.jsonl", &with), events_file("nE3-without.jsonl", &base));
    let (c, o1, _) = live(BODY, &st1, &e1, &s1, "1"); assert_eq!(c, 2);
    let l: Vec<&str> = o1.lines().collect(); assert!(l[2].starts_with(r#"{"error""#) && l[2].contains(r#""events[2].inputs.needs.context""#), "{}", l[2]);
    let (c, o2, e) = live(BODY, &st2, &e2, &s2, "1"); assert_eq!(c, 0, "{e}");
    assert_eq!(l.iter().enumerate().filter(|(i, _)| *i != 2).map(|(_, x)| format!("{x}\n")).collect::<String>(), o2);
    assert_eq!((read(&s1), read(&st1)), (read(&s2), read(&st2)), "the strand and the state");
    clean(&[&st, &strand, &ev, &st1, &st2, &s1, &s2, &e1, &e2]);
}

/// One random event for the body persona: sources (env, the compactor, the principal, self, clock, missing, malformed), amounts
/// (declared and not, in and out of range, strings, with their own source, with the bound goal's win), readings, quiet hours,
/// goal wins, engine-owned ids, stamped hours that open and expire windows (some negative)
fn needs_event(r: &mut Mix) -> String {
    const SRC: [&str; 9] = [r#""env:host""#, r#""env:host""#, r#""env:host""#, r#""env:compactor""#, r#""human:principal""#, r#""self""#, r#""clock""#, r#""env:""#, "7"];
    let mut kv: Vec<String> = vec![];
    if r.below(12) != 0 { kv.push(format!(r#""src":{}"#, SRC[r.below(9)])); }
    let amt = |r: &mut Mix, cap: f64| -> String { match r.below(25) { 0 => "0".into(), 1 => format!("{}", cap + 0.5), 2 => r#""0.5""#.into(), _ => format!("{}", ((r.unit() * cap) * 100.0).ceil().max(1.0) / 100.0) } };
    let mut nd: Vec<String> = vec![];
    if r.below(4) == 0 { let a = amt(r, 1.0); nd.push(match r.below(4) { 0 => format!(r#""context":{{"amount":{a},"src":"env:compactor"}}"#), 1 => format!(r#""context":{{"amount":{a},"src":"self"}}"#), _ => format!(r#""context":{a}"#) }); }
    if r.below(4) == 0 { let a = amt(r, 1.0); nd.push(match r.below(4) { 0 | 1 => format!(r#""contact":{{"amount":{a},"src":"human:principal"}}"#), 2 => format!(r#""contact":{{"amount":{a},"src":"env:host"}}"#), _ => format!(r#""contact":{a}"#) }); }
    if r.below(30) == 0 { nd.push(r#""sleep":0.2"#.into()); }
    if !nd.is_empty() { kv.push(format!(r#""needs":{{{}}}"#, nd.join(","))); }
    if r.below(3) == 0 { kv.push(format!(r#""senses":{{"tokens":{}}}"#, match r.below(20) { 0 => "1.5".to_string(), _ => format!("{}", (r.unit() * 40.0).round() / 100.0) })); }
    if r.below(8) == 0 { kv.push(r#""quiet":true"#.into()); }
    if r.below(10) == 0 { kv.push(format!(r#""goals":{{"{}":{{"win":{}}}}}"#, ["work", "compact", "report"][r.below(3)], [0.5, 1.0][r.below(2)])); }
    if r.below(40) == 0 { kv.push(r#""need_context":1"#.into()); } else if r.below(40) == 0 { kv.push(r#""drv_omit":0.3"#.into()); }
    kv.push(format!(r#""elapsed_hours":{}"#, match r.below(40) { 0 => "-1".to_string(), 1 => "24".to_string(), _ => format!("{}", [0.0, 0.5, 1.0, 1.0, 2.0, 3.0][r.below(6)]) }));
    format!("{{{}}}", kv.join(","))
}

/// P3n: a refused event changes nothing. 40 individuals x 300 random events mixing sources, amounts, windows and payments on the
/// body persona: the run with the refused events equals the run without them, byte for byte (every stance, the strand with a
/// checkpoint after each event, the final state), and the strand verifies
#[test]
fn p3n_a_refused_needs_event_changes_nothing() {
    let (mut refused, mut accepted, mut paid, mut fired) = (0usize, 0usize, 0usize, 0usize);
    for seed in 0..40u64 {
        let mut r = Mix(0x4EED_5000 + seed); let evs: Vec<String> = (0..300).map(|_| needs_event(&mut r)).collect();
        let all = events_file("p3n-all.jsonl", &evs);
        let st = init(BODY, seed, "p3n-state.json"); let strand = tmp("p3n.strand"); clean(&[&strand]);
        let (c, out, _) = live(BODY, &st, &all, &strand, "1");
        let docs: Vec<&str> = out.lines().collect(); assert_eq!(docs.len(), 300, "seed {seed}: one line per event");
        let ok: Vec<usize> = (0..300).filter(|&i| !docs[i].starts_with(r#"{"error""#)).collect();
        assert_eq!(c, if ok.len() < 300 { 2 } else { 0 }, "seed {seed}");
        refused += 300 - ok.len(); accepted += ok.len();
        for &i in &ok { if evs[i].contains(r#""needs""#) { paid += 1; } if docs[i].contains("an expected win did not come") { fired += 1; } }
        let kept = events_file("p3n-kept.jsonl", &ok.iter().map(|&i| evs[i].clone()).collect::<Vec<_>>());
        let st2 = init(BODY, seed, "p3n-state2.json"); let strand2 = tmp("p3n2.strand"); clean(&[&strand2]);
        let (c, out2, e) = live(BODY, &st2, &kept, &strand2, "1"); assert_eq!(c, 0, "seed {seed}: {e}");
        assert_eq!(out2, ok.iter().map(|&i| format!("{}\n", docs[i])).collect::<String>(), "seed {seed}: the stances");
        assert_eq!(read(&strand2), read(&strand), "seed {seed}: the strand"); assert_eq!(read(&st2), read(&st), "seed {seed}: the state");
        if seed % 8 == 0 { let (c, v, e) = probbit(&["live", "verify", &strand]); assert_eq!((c, s(&parse(&v), &["ok"])), (0, &Json::Bool(true)), "seed {seed}: {e}"); }
        clean(&[&st, &strand, &st2, &strand2, &all, &kept]);
    }
    eprintln!("p3n: {refused} refused, {accepted} accepted ({paid} with amounts, {fired} windows fired)");
    assert!(refused >= 1000 && accepted >= 4000 && paid >= 1000 && fired >= 200, "a mix: {refused} refused, {accepted} accepted, {paid} paid, {fired} fired");
}

/// `probbit check` on the body persona: both rules held by construction (quiet hours never report; a nearly full context
/// compacts), and a body strand replays
#[test]
fn probbit_check_holds_the_body_persona() {
    let ev = events_file("check-body.jsonl", &body_day()); let st = init(BODY, 1, "check-body-state.json"); let strand = tmp("check-body.strand"); clean(&[&strand]);
    let (c, _, e) = live(BODY, &st, &ev, &strand, "6"); assert_eq!(c, 0, "{e}");
    let (c, out, e) = probbit(&["check", BODY, "--props", BODY_PROPS, "--seeds", "0-9", "--strand", &strand]); assert_eq!(c, 0, "{out}\n{e}");
    for id in ["no_reports_in_quiet_hours", "compacts_a_full_context"] {
        assert!(out.lines().any(|l| l.starts_with("rule    ") && l.contains(id) && l.ends_with("  held_by_construction")), "{id}: {out}"); }
    assert!(out.ends_with("check: held 2 · broken 0 · unproven 0 · strands 1/1\n"), "{out}");
    clean(&[&st, &strand, &ev]);
}
