//! Human formatting must not change machine values, persisted state or replay records.
use std::process::{Command, Output};
#[allow(dead_code)]
#[path = "../src/json.rs"]
mod json;

const PERSONA: &str = "../examples/persona/tutor.yaml";
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_probbit"))
        .args(args)
        .env("PROBBIT_THREADS", "2")
        .env("PROBBIT_PRIORITY", "normal")
        .output()
        .unwrap()
}
fn stdout(o: &Output) -> &str {
    std::str::from_utf8(&o.stdout).unwrap()
}
fn stderr(o: &Output) -> &str {
    std::str::from_utf8(&o.stderr).unwrap()
}
fn parsed(o: &Output) -> json::Json {
    json::parse(stdout(o)).unwrap_or_else(|e| panic!("{}: {}", e.msg, stderr(o)))
}
fn compare(args: &[&str]) {
    let compact = run(args);
    let mut pretty_args = args.to_vec();
    pretty_args.push("--pretty");
    let pretty = run(&pretty_args);
    assert_eq!(compact.status.code(), pretty.status.code(), "{args:?}");
    assert_eq!(parsed(&compact), parsed(&pretty), "{args:?}");
    assert_eq!(stdout(&compact).lines().count(), 1, "{args:?}");
    assert!(stdout(&pretty).contains("\n  \""), "{args:?}");
}

#[test]
fn persona_pretty_preserves_single_document_values() {
    for sub in ["init", "check", "describe"] {
        compare(&["persona", sub, PERSONA]);
    }
    compare(&["persona", "diff", PERSONA, "--script", "[{\"loss\":true}]"]);
    compare(&["persona", "lint", PERSONA, "--seeds", "2", "--threads", "1"]);
    let rule = r#"{"when":{"loss":true},"then":{"humour":["none"]}}"#;
    for sub in ["fuzz", "prove"] {
        compare(&[
            "persona",
            sub,
            PERSONA,
            "--seeds",
            "2",
            "--threads",
            "1",
            "--never",
            rule,
            "--json",
        ]);
        let pretty = run(&[
            "persona",
            sub,
            PERSONA,
            "--seeds",
            "2",
            "--threads",
            "1",
            "--never",
            rule,
            "--pretty",
        ]);
        assert!(pretty.status.success(), "{}", stderr(&pretty));
        assert!(stdout(&pretty).contains("\n  \""));
        parsed(&pretty);
    }
}

#[test]
fn persona_pretty_leaves_state_and_program_digests_unchanged() {
    let path = std::env::temp_dir().join(format!("probbit-pretty-{}.state", std::process::id()));
    let path = path.to_str().unwrap();
    let init = run(&["persona", "init", PERSONA, "--seed", "2"]);
    let initial = stdout(&init);
    let saved = run(&[
        "persona", "init", PERSONA, "--seed", "2", "--out", path, "--pretty",
    ]);
    assert!(saved.status.success(), "{}", stderr(&saved));
    assert!(saved.stdout.is_empty(), "--out does not contaminate stdout");
    assert!(stderr(&saved).contains("wrote ") && stderr(&saved).contains("Pip 1.1.0, seed 2"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), initial);
    compare(&[
        "persona",
        "compile",
        PERSONA,
        "--state",
        path,
        "--inputs",
        "{\"loss\":true}",
    ]);
    let compact = run(&[
        "persona",
        "turn",
        PERSONA,
        "--state",
        path,
        "--inputs",
        "{\"loss\":true}",
    ]);
    let state = std::fs::read_to_string(path).unwrap();
    std::fs::write(path, initial).unwrap();
    let pretty = run(&[
        "persona",
        "turn",
        PERSONA,
        "--state",
        path,
        "--inputs",
        "{\"loss\":true}",
        "--pretty",
    ]);
    assert!(pretty.status.success(), "{}", stderr(&pretty));
    assert_eq!(parsed(&compact), parsed(&pretty));
    assert_eq!(std::fs::read_to_string(path).unwrap(), state);
    assert!(stdout(&pretty).contains("\n  \""));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn non_json_persona_outputs_explain_why_pretty_does_not_apply() {
    for (sub, hint) in [("replay", "JSONL"), ("explain", "readable text")] {
        let out = run(&["persona", sub, PERSONA, "--script", "[{}]", "--pretty"]);
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        assert!(stderr(&out).contains(hint), "{}", stderr(&out));
    }
    let replay = run(&["persona", "replay", PERSONA, "--script", "[{},{}]"]);
    assert!(replay.status.success());
    assert_eq!(stdout(&replay).lines().count(), 2);
    for line in stdout(&replay).lines() {
        json::parse(line).unwrap();
    }
}

#[test]
fn command_and_flag_errors_are_actionable_and_stay_off_stdout() {
    for (args, hint) in [
        (vec!["monitr"], "unknown command"),
        (
            vec!["persona", "init", PERSONA, "--out", "--pretty"],
            "--out needs a value",
        ),
        (vec!["decide", "--nope"], "probbit decide --help"),
        (vec!["--help", "--nope"], "unknown argument"),
    ] {
        let o = run(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
        assert!(stderr(&o).contains(hint), "{args:?}: {}", stderr(&o));
    }
    for args in [vec!["version"], vec!["--version"], vec!["-V"]] {
        let o = run(&args);
        assert!(o.status.success());
        assert_eq!(
            stdout(&o),
            concat!("probbit ", env!("CARGO_PKG_VERSION"), "\n")
        );
        assert!(o.stderr.is_empty());
    }
}

#[test]
fn demo_rejects_out_of_range_sizes_before_allocating() {
    for n in ["0", "3333334", "18446744073709551615"] {
        let o = run(&["demo", "--tasks", n]);
        assert_eq!(o.status.code(), Some(2));
        assert!(o.stdout.is_empty());
        assert!(stderr(&o).contains("--tasks"), "{}", stderr(&o));
    }
}

#[test]
fn fuzz_cli_exports_only_explicitly_authorized_synthetic_fixtures() {
    let path = std::env::temp_dir().join(format!(
        "probbit-fixture-source-{}.json",
        std::process::id()
    ));
    let path = path.to_str().unwrap();
    std::fs::write(path, r#"{"probbit_persona":1,"identity":{"name":"Fixture","version":"1"},
      "traits":[{"id":"action","levels":["retry","ask"],"logw":[1,0]}],
      "inputs":[{"id":"praise","kind":"flag"},{"id":"criticism","kind":"flag"}],
      "learning":{"from":["praise","criticism"],"traits":["action"],"rate":3,"step_cap":2,"total_cap":2},
      "reward_from":["env"]}"#).unwrap();
    let mut args = vec![
        "persona",
        "fuzz",
        path,
        "--never",
        r#"{"then":{"action":["retry"]}}"#,
        "--seeds",
        "0",
        "--scripts",
        "0",
        "--depth",
        "3",
        "--beam",
        "4",
        "--threads",
        "1",
        "--json",
    ];
    let abstract_out = run(&args);
    assert_eq!(
        abstract_out.status.code(),
        Some(1),
        "{}",
        stderr(&abstract_out)
    );
    let doc = parsed(&abstract_out);
    let shortest = doc.get("properties").unwrap().as_arr().unwrap()[0]
        .get("shortest")
        .unwrap();
    assert_eq!(shortest.get("replay"), Some(&json::Json::Null));
    args.extend(["--fixture-src", "env:synthetic", "--pretty"]);
    let authorized = run(&args);
    assert_eq!(authorized.status.code(), Some(1), "{}", stderr(&authorized));
    let doc = parsed(&authorized);
    let shortest = doc.get("properties").unwrap().as_arr().unwrap()[0]
        .get("shortest")
        .unwrap();
    assert!(shortest
        .get("replay")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("env:synthetic"));
    let script = json::write(shortest.get("script").unwrap(), false);
    let replay = run(&[
        "persona", "replay", path, "--seed", "0", "--script", &script,
    ]);
    assert!(replay.status.success(), "{}", stderr(&replay));
    assert_eq!(
        json::parse(stdout(&replay).lines().last().unwrap()).unwrap(),
        *shortest.get("stance").unwrap()
    );
    let source_index = args.iter().position(|x| *x == "env:synthetic").unwrap();
    for forbidden in ["env:production", "human:synthetic", "self"] {
        args[source_index] = forbidden;
        let denied = run(&args);
        assert_eq!(denied.status.code(), Some(2), "{}", stderr(&denied));
        assert!(parsed(&denied).get("error").is_some());
    }
    std::fs::remove_file(path).unwrap();
}
