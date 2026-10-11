//! Public processor responses: released assignments are never confused with the
//! legacy full diagnostic candidate. Fixed-work behavior is thread invariant.
#[allow(dead_code)]
#[path = "../src/json.rs"]
mod json;
use json::Json;
use std::io::Write;
use std::process::{Command, Stdio};

fn call(args: &[&str], input: &str) -> (i32, Json) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_probbit"))
        .args(args)
        .env_remove("PROBBIT_CONFIG")
        .env_remove("PROBBIT_THREADS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    let s = String::from_utf8(o.stdout).unwrap();
    (
        o.status.code().unwrap(),
        json::parse(&s).unwrap_or_else(|_| {
            panic!(
                "invalid output {s}; stderr {}",
                String::from_utf8_lossy(&o.stderr)
            )
        }),
    )
}
fn contract(j: &Json) {
    let verdict = j.get("verdict").and_then(Json::as_str).unwrap();
    let Some(plan) = j.get("plan") else {
        assert!(["infeasible", "refused", "declined"].contains(&verdict));
        assert!(j.get("released_plan").is_none());
        return;
    };
    let expected = match verdict {
        "exact" | "diagnostics_passed" => "released",
        "partial" => "partial",
        _ => "diagnostic",
    };
    assert_eq!(j.get("plan_status").and_then(Json::as_str), Some(expected));
    let released = j.get("released").and_then(Json::as_arr).unwrap();
    let rp = j.get("released_plan").and_then(Json::as_obj).unwrap();
    assert_eq!(rp.len(), released.len());
    for (id, value) in rp {
        assert!(released.contains(&json::str(id)));
        assert_eq!(plan.get(id), Some(value));
    }
    let count = plan.as_obj().unwrap().len();
    match expected {
        "released" => assert_eq!(rp.len(), count),
        "partial" => assert!(!rp.is_empty() && rp.len() < count),
        _ => assert!(rp.is_empty()),
    }
    if let Some(answers) = j.get("answers").and_then(Json::as_obj) {
        for (id, a) in answers {
            assert_eq!(
                a.get("probbit").unwrap().get("released"),
                Some(&Json::Bool(released.contains(&json::str(id))))
            );
        }
    }
}
fn stable(j: &Json) -> Json {
    match j {
        Json::Obj(v) => Json::Obj(
            v.iter()
                .filter(|(k, _)| {
                    ![
                        "ms",
                        "sample_ms",
                        "gate_ms",
                        "site_updates_per_s",
                        "process_cpu_ms",
                        "peak_rss_mb",
                        "nice",
                        "phases",
                        "threads",
                    ]
                    .contains(&k.as_str())
                })
                .map(|(k, v)| (k.clone(), stable(v)))
                .collect(),
        ),
        Json::Arr(v) => Json::Arr(v.iter().map(stable).collect()),
        x => x.clone(),
    }
}

#[test]
fn exact_refused_and_infeasible_plans_have_distinct_public_contracts() {
    let doc = r#"{"probbit_ir":1,"values":["a","b"],"vars":[{"id":"x"},{"id":"y"}]}"#;
    for args in [
        vec!["run"],
        vec!["run", "--op", "sample", "--sweeps", "1", "--polish-ms", "0"],
        vec!["run", "--op", "exact", "--exact-ms", "0"],
    ] {
        let (_, j) = call(&args, doc);
        contract(&j);
    }
    let (_, bad) = call(
        &["run"],
        r#"{"probbit_ir":1,"values":["a"],"vars":[{"id":"x"}],"caps":[{"value":"a","limit":0}]}"#,
    );
    assert_eq!(
        bad.get("verdict").and_then(Json::as_str),
        Some("infeasible")
    );
    contract(&bad);
    let (_, d) = call(&["demo", "--tasks", "12"], "");
    let d = json::write(&d, false);
    for args in [
        vec!["decide"],
        vec![
            "decide",
            "--mode",
            "sample",
            "--sweeps",
            "1",
            "--polish-ms",
            "0",
        ],
    ] {
        contract(&call(&args, &d).1);
    }
    let ev = include_str!("../../examples/evaluate/support-12.json");
    for args in [
        vec!["evaluate"],
        vec![
            "evaluate",
            "--op",
            "sample",
            "--sweeps",
            "1",
            "--polish-ms",
            "0",
        ],
    ] {
        contract(&call(&args, ev).1);
    }
}

#[test]
fn fixed_work_release_and_candidate_fields_match_at_one_two_and_four_threads() {
    let (_, d) = call(&["demo", "--tasks", "300"], "");
    let d = json::write(&d, false);
    let mut partial = 0;
    for (cmd, input) in [
        ("decide", d.as_str()),
        ("run", include_str!("../../examples/knapsack-20.json")),
        (
            "evaluate",
            include_str!("../../examples/evaluate/support-12.json"),
        ),
    ] {
        let mut first = None;
        for threads in ["1", "2", "4"] {
            let (code, j) = call(
                &[
                    cmd,
                    if cmd == "decide" { "--mode" } else { "--op" },
                    "sample",
                    "--sweeps",
                    "1000",
                    "--polish-sweeps",
                    "50",
                    "--threads",
                    threads,
                ],
                input,
            );
            contract(&j);
            if j.get("verdict").and_then(Json::as_str) == Some("partial") {
                partial += 1;
            }
            let got = (code, stable(&j));
            if let Some(expected) = &first {
                assert_eq!(&got, expected, "{cmd} threads {threads}");
            } else {
                first = Some(got);
            }
        }
    }
    assert!(
        partial >= 3,
        "the fixture must exercise a real partial release"
    );
}

#[test]
fn malformed_input_is_one_error_document_not_a_panic() {
    for input in [
        r#"{"probbit_ir":01}"#,
        "{\"probbit_ir\":1,\"comment\":\"a\nb\"}",
        r#"{"probbit_ir":1,"values":["a"],"vars":[{"h":{"a":1e309}}]}"#,
        r#"{"probbit_ir":1,"values":["a","b"],"vars":[{"id":"x"},{"id":"y"}],"pairs":[{"i":"x","j":"y","table":[[0,0]]}]}"#,
    ] {
        let (code, j) = call(&["run"], input);
        assert_eq!(code, 2);
        assert_eq!(j.as_obj().unwrap().len(), 1);
        assert!(j.get("error").is_some());
    }
}
