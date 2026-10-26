//! `probbit check` (0.8.3): `persona lint`'s pipeline on a persona's rules plus `live verify` on its strands, one verdict line and
//! exit 0 (every rule held, every strand replays), 1 (a rule broken or a strand failing; 1 wins), 3 (only unproven rules).
use std::io::Write;
use std::process::{Command, Stdio};

#[path = "../../probbit-persona/src/json.rs"]
#[allow(dead_code)]
mod json;
use json::Json;

/// `probbit` run from the repository root (the shipped examples are addressed from there): (exit code, stdout, stderr)
fn probbit(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).current_dir(format!("{}/..", env!("CARGO_MANIFEST_DIR")))
        .env_remove("PROBBIT_CONFIG").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let _ = c.stdin.take().unwrap().write_all(stdin.as_bytes()); let o = c.wait_with_output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
fn tmp(name: &str) -> String { format!("{}/check-{}-{name}", env!("CARGO_TARGET_TMPDIR"), std::process::id()) }
fn parse(s: &str) -> Json { json::parse(s).unwrap_or_else(|e| panic!("not JSON ({}): {s:.300}", e.msg)) }
fn jw(j: &Json) -> String { json::write(j, false) }
fn s<'a>(j: &'a Json, path: &[&str]) -> &'a Json { path.iter().fold(j, |x, k| x.get(k).unwrap_or_else(|| panic!("no {k} in {:.200}", jw(x)))) }
const DISPATCH: &str = "examples/persona/dispatch-policy.yaml";
const DISPATCH_PROPS: &str = "examples/persona/dispatch-props.json";
const TUTOR: &str = "examples/persona/tutor.yaml";

/// The shipped rule-lock example: every rule held by construction on seeds 0-99, exit 0
#[test]
fn dispatch_example_holds_every_rule() {
    let (c, out, e) = probbit(&["check", DISPATCH, "--props", DISPATCH_PROPS], ""); assert_eq!(c, 0, "{out}\n{e}");
    assert!(out.starts_with(&format!("probbit check  {DISPATCH}  (Dispatch 1.0.0, seeds 0-99, depth 8)\n")), "{out}");
    for id in ["risky_work_asks", "repeated_failures_ask", "routine_work_acts"] {
        assert!(out.lines().any(|l| l.starts_with("rule    ") && l.contains(id) && l.ends_with("  held_by_construction")), "{id}: {out}"); }
    assert!(out.ends_with("check: held 3 · broken 0 · unproven 0 · strands 0/0\n"), "{out}");
}

/// Without the habit that states it, `routine_work_acts` is broken on some seeds: exit 1, the seeds named; the habits that stay
/// keep their rules held
#[test]
fn dispatch_without_its_routine_habit_is_broken() {
    let src = std::fs::read_to_string(format!("{}/../{DISPATCH}", env!("CARGO_MANIFEST_DIR"))).unwrap().replace("\r\n", "\n");
    let (a, b) = (src.find("  - id: routine_acts\n").unwrap(), src.find("  - id: risky_asks\n").unwrap());
    let f = tmp("dispatch-no-habit.yaml"); std::fs::write(&f, format!("{}{}", &src[..a], &src[b..])).unwrap();
    let props = format!("{}/../{DISPATCH_PROPS}", env!("CARGO_MANIFEST_DIR"));
    let (c, out, e) = probbit(&["check", &f, "--props", &props, "--seeds", "0-19"], ""); assert_eq!(c, 1, "{out}\n{e}");
    let line = out.lines().find(|l| l.contains("routine_work_acts")).unwrap(); assert!(line.contains("  broken seed"), "{out}");
    assert!(out.lines().any(|l| l.contains("risky_work_asks") && l.ends_with("held_by_construction")), "{out}");
    assert!(out.ends_with("check: held 2 · broken 1 · unproven 0 · strands 0/0\n"), "{out}");
    let _ = std::fs::remove_file(&f);
}

/// The shipped tutor: a proved rule exits 0 ("held"), a broken one 1, an unproven one 3, broken next to unproven 1
#[test]
fn exit_codes_follow_the_rules() {
    let props = |name: &str, body: &str| { let f = tmp(name); std::fs::write(&f, body).unwrap(); f };
    let proved = props("proved.json", r#"{"props": [{"id": "error_care", "when": {"error": true}, "then": {"caution": {"at_least": "measured"}}}]}"#);
    let broken = props("broken.json", r#"{"props": [{"id": "praise_lifts", "when": {"praise": true}, "then": {"valence": {"at_least": "even"}}}]}"#);
    let unproven = props("unproven.json", r#"{"props": [{"id": "loss_warm", "when": {"loss": true}, "then": {"warmth": ["warm"]}}]}"#);
    let both = props("both.json", r#"{"props": [{"id": "praise_lifts", "when": {"praise": true}, "then": {"valence": {"at_least": "even"}}},
        {"id": "loss_warm", "when": {"loss": true}, "then": {"warmth": ["warm"]}}]}"#);
    let (c, out, e) = probbit(&["check", TUTOR, "--props", &proved, "--seeds", "0-2"], ""); assert_eq!(c, 0, "{out}\n{e}");
    assert!(out.lines().any(|l| l.contains("error_care") && l.ends_with("  held")), "{out}");
    let (c, out, e) = probbit(&["check", TUTOR, "--props", &broken, "--seeds", "0-2"], ""); assert_eq!(c, 1, "{out}\n{e}");
    assert!(out.lines().any(|l| l.contains("praise_lifts  broken seed")), "{out}"); assert!(out.contains("check: held 0 · broken 1 · unproven 0"), "{out}");
    let (c, out, e) = probbit(&["check", TUTOR, "--props", &unproven, "--seeds", "0-2"], ""); assert_eq!(c, 3, "{out}\n{e}");
    assert!(out.lines().any(|l| l.contains("loss_warm  unproven  seeds 0-2 not proved")), "{out}"); assert!(out.contains("check: held 0 · broken 0 · unproven 1"), "{out}");
    let (c, out, e) = probbit(&["check", TUTOR, "--props", &both, "--seeds", "0-2"], ""); assert_eq!(c, 1, "1 wins over 3: {out}\n{e}");
    assert!(out.contains("check: held 0 · broken 1 · unproven 1"), "{out}");
    // the --json verdict says the same, and its rules are the persona lint document
    let (c, j, e) = probbit(&["check", TUTOR, "--props", &both, "--seeds", "0-2", "--json"], ""); assert_eq!(c, 1, "{e}");
    let j = parse(&j); assert_eq!(jw(s(&j, &["verdict"])), r#"{"broken":1,"conflicts":0,"exit":1,"held":0,"strands_ok":0,"strands_total":0,"unproven":1}"#);
    let (_, lint, _) = probbit(&["persona", "lint", TUTOR, "--props", &both, "--seeds", "0-2"], ""); assert_eq!(jw(s(&j, &["rules"])), jw(&parse(&lint)));
    for f in [proved, broken, unproven, both] { let _ = std::fs::remove_file(f); }
}

/// Strands: one that replays (ok N events) and a tampered copy (FAIL with its line): exit 1 although every rule holds; the --json
/// shape, every field named
#[test]
fn strands_replay_or_fail() {
    let good = tmp("good.strand"); let _ = std::fs::remove_file(&good);
    let events = "{\"blast\":0.2}\n{\"error\":true}\n{\"error\":true,\"criticism\":true,\"src\":\"human\"}\n{\"praise\":true,\"src\":\"env\"}\n{\"blast\":0.9}\n";
    let (c, _, e) = probbit(&["live", DISPATCH, "--seed", "2", "--clock", "fixed", "--strand", &good], events); assert_eq!(c, 0, "{e}");
    let (c, out, e) = probbit(&["check", DISPATCH, "--props", DISPATCH_PROPS, "--seeds", "0-9", "--strand", &good], ""); assert_eq!(c, 0, "{out}\n{e}");
    assert!(out.lines().any(|l| l.starts_with("strand  ") && l.contains(&good) && l.ends_with("  ok 5 events")), "{out}");
    assert!(out.ends_with("strands 1/1\n"), "{out}");
    let bad = tmp("tampered.strand");
    let text = std::fs::read_to_string(&good).unwrap(); assert!(text.contains("\"blast\":0.9"), "{text:.300}");
    std::fs::write(&bad, text.replace("\"blast\":0.9", "\"blast\":0.1")).unwrap();
    let (c, out, e) = probbit(&["check", DISPATCH, "--props", DISPATCH_PROPS, "--seeds", "0-9", "--strand", &good, "--strand", &bad], ""); assert_eq!(c, 1, "{out}\n{e}");
    assert!(out.lines().any(|l| l.contains(&bad) && l.contains("  FAIL line ")), "{out}");
    assert!(out.ends_with("check: held 3 · broken 0 · unproven 0 · strands 1/2\n"), "{out}");
    let (c, j, e) = probbit(&["check", DISPATCH, "--props", DISPATCH_PROPS, "--seeds", "0-9", "--depth", "4", "--hours", "1,24", "--strand", &good, "--strand", &bad, "--json"], ""); assert_eq!(c, 1, "{e}");
    let j = parse(&j);
    let keys: Vec<&str> = j.as_obj().unwrap().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["depth", "hours", "persona", "props", "rules", "seeds", "strands", "verdict"]);
    assert_eq!((s(&j, &["persona"]).as_str(), s(&j, &["props"]).as_str(), s(&j, &["seeds"]).as_str(), s(&j, &["depth"]).as_f64(), jw(s(&j, &["hours"]))),
        (Some(DISPATCH), Some(DISPATCH_PROPS), Some("0-9"), Some(4.0), "[1,24]".to_string()));
    assert_eq!(s(&j, &["rules", "ok"]), &Json::Bool(true));
    let st = s(&j, &["strands"]).as_arr().unwrap(); assert_eq!(st.len(), 2);
    assert_eq!((s(&st[0], &["ok"]), s(&st[0], &["events"]).as_f64(), st[0].get("error").is_none()), (&Json::Bool(true), Some(5.0), true));
    assert!(s(&st[0], &["final_state"]).as_str().is_some_and(|d| d.starts_with("sha256:")));
    assert_eq!((s(&st[1], &["path"]).as_str(), s(&st[1], &["ok"]), s(&st[1], &["events"]), s(&st[1], &["final_state"])), (Some(bad.as_str()), &Json::Bool(false), &Json::Null, &Json::Null));
    assert!(s(&st[1], &["error"]).as_str().is_some_and(|e| e.starts_with("line ")), "{}", jw(&st[1]));
    assert_eq!(jw(s(&j, &["verdict"])), r#"{"broken":0,"conflicts":0,"exit":1,"held":3,"strands_ok":1,"strands_total":2,"unproven":0}"#);
    // a strand that cannot be read fails too
    let (c, out, _) = probbit(&["check", DISPATCH, "--seeds", "0", "--strand", "no-such.strand"], ""); assert_eq!(c, 1, "{out}");
    assert!(out.contains("no-such.strand  FAIL cannot read") && out.contains("rules   none given"), "{out}");
    for f in [good, bad] { let _ = std::fs::remove_file(f); }
}

/// Bad usage exits 2: no persona, a missing --strand value, a flag check does not take, a bad depth, a missing persona file
#[test]
fn bad_usage_exits_2() {
    for a in [vec!["check"], vec!["check", DISPATCH, "--strand"], vec!["check", DISPATCH, "--never", "{when: {blast: 0.7}, then: {action: [ask]}}"],
        vec!["check", DISPATCH, "--depth", "0"], vec!["check", DISPATCH, "--seeds", "9-1"]] {
        let (c, out, e) = probbit(&a, ""); assert_eq!(c, 2, "{a:?}: {e}"); assert!(out.is_empty(), "{a:?}: {out}"); }
    let (c, out, _) = probbit(&["check", "no-such-persona.yaml"], ""); assert_eq!(c, 2); assert_eq!(s(&parse(&out), &["error", "code"]).as_str(), Some("persona"));
}

/// The command table picks it up: the top-level help lists it, `check --help` and `probbit help check` print its usage and exit
/// codes, the completions offer it with its flags, the man page has it
#[test]
fn the_table_lists_check() {
    let (c, top, _) = probbit(&["--help"], ""); assert_eq!(c, 0); assert!(top.contains("\n  check PERSONA "), "{top}");
    let (c, h, _) = probbit(&["check", "--help"], ""); assert_eq!(c, 0);
    assert!(h.starts_with("usage: probbit check PERSONA [--props FILE] [--strand FILE]... [--seeds LIST] [--depth N]\n               [--hours LIST] [--json]\n"), "{h}");
    for f in ["--props FILE", "--strand FILE", "--seeds LIST", "--depth N", "--hours LIST", "--json"] { assert!(h.contains(&format!("\n  {f} ")), "{f}: {h}"); }
    assert!(h.contains("exit: 0 every rule held"), "{h}");
    let (_, h2, _) = probbit(&["help", "check"], ""); assert_eq!(h, h2);
    let (_, bash, _) = probbit(&["completions", "bash"], ""); assert!(bash.contains("            check) if [ \"$COMP_CWORD\" -eq 2 ]; then words='--props --strand --seeds --depth --hours --json --help';"), "{bash}");
    let (_, man, _) = probbit(&["man"], ""); assert!(man.contains(".SS check\n"), "man page");
}
