//! The no-thread path (`pbit_core::rt::set_sequential`, what the browser build runs) gives the same documents as 4 threads at
//! fixed work: the 300-task router demo at 3,200 sweeps (as `pbit demo --tasks 300 | pbit decide --sweeps 3200 --polish-ms 0`), a
//! sampled pbit-ir program and a sampled `evaluate` request. Only timings and `telemetry.threads` (1 vs 4) may differ. The
//! CLI's own threads-1-vs-N equality is pbit-cli's `run_threads_and_chains_are_resource_controls`; this extends it to no threads.
#[allow(dead_code)]
#[path = "../../pbit-cli/src/json.rs"]
mod json;
use json::Json;

/// The document without wall-clock fields and with `telemetry.threads` named apart (the golden tests' normalization + threads).
fn stable(j: &Json) -> Json {
    match j { Json::Obj(v) => Json::Obj(v.iter().filter(|(k, _)| !["ms", "sample_ms", "gate_ms", "site_updates_per_s", "process_cpu_ms", "peak_rss_mb", "nice", "phases", "threads"].contains(&k.as_str()))
            .map(|(k, x)| (k.clone(), stable(x))).collect()),
        Json::Arr(v) => Json::Arr(v.iter().map(stable).collect()), x => x.clone() }
}
fn with_flags(doc: &str, flags: &str) -> String {
    let mut d = json::parse(doc).unwrap(); if let Json::Obj(v) = &mut d { v.push(("flags".into(), json::parse(flags).unwrap())); } json::write(&d, false)
}
fn threads(doc: &str) -> f64 { json::parse(doc).unwrap().get("telemetry").and_then(|t| t.get("threads")).and_then(Json::as_f64).unwrap() }
/// (no threads, 4 threads) for one call
fn both(f: fn(&str) -> (String, i32), doc: &str, flags: &str) -> ((String, i32), (String, i32)) {
    (f(&with_flags(doc, flags)), f(&with_flags(doc, &flags.replacen('{', r#"{"threads": 4, "#, 1))))
}

#[test]
fn the_300_task_demo_without_threads_equals_4_threads() {
    let (demo, c) = pbit_wasm::demo(r#"{"tasks": 300, "seed": 7}"#); assert_eq!(c, 0);
    let ((a, ca), (b, cb)) = both(pbit_wasm::decide, &demo, r#"{"sweeps": 3200, "polish_ms": 0}"#);
    let (ja, jb) = (json::parse(&a).unwrap(), json::parse(&b).unwrap());
    assert_eq!((ca, cb), (0, 0)); assert_eq!(stable(&ja), stable(&jb), "no threads vs 4 threads: the documents differ");
    assert_eq!((threads(&a), threads(&b)), (1.0, 4.0));
    assert_eq!(ja.get("verdict").and_then(Json::as_str), Some("diagnostics_passed")); assert_eq!(ja.get("violations").and_then(Json::as_f64), Some(0.0));
    assert_eq!(ja.get("gate").and_then(|g| g.get("sweeps")).and_then(Json::as_f64), Some(12800.0));
}

#[test]
fn run_and_evaluate_without_threads_equal_4_threads() {
    let knap = include_str!("../../examples/knapsack-20.json"); let ev = include_str!("../../examples/evaluate/support-12.json");
    for (f, doc, flags) in [(pbit_wasm::run as fn(&str) -> (String, i32), knap, r#"{"op": "sample", "sweeps": 3000, "polish_sweeps": 400}"#),
        (pbit_wasm::evaluate, ev, r#"{"op": "sample", "sweeps": 2000, "polish_ms": 0}"#), (pbit_wasm::evaluate, ev, r#"{"op": "sample", "sweeps": 400, "polish_ms": 0}"#)] {
        let ((a, ca), (b, cb)) = both(f, doc, flags);
        assert_eq!(ca, cb, "{flags}"); assert_eq!(stable(&json::parse(&a).unwrap()), stable(&json::parse(&b).unwrap()), "{flags}: no threads vs 4 threads");
        assert_eq!((threads(&a), threads(&b)), (1.0, 4.0)); }
}

/// Flags as in `pbit mcp`: bad ones are one `flag` error (exit 2); bad documents the CLI's error object; `program` the compiled program.
#[test]
fn flags_errors_and_the_program() {
    let ev = include_str!("../../examples/evaluate/support-12.json");
    for (flags, path) in [(r#"{"sweeps": -1}"#, "flags.sweeps"), (r#"{"summary": true}"#, "flags.summary"), (r#"{"mode": "exact"}"#, "flags.mode"), (r#"{"deadline_ms": 50, "sweeps": 10}"#, "flags.deadline_ms")] {
        let (out, c) = pbit_wasm::evaluate(&with_flags(ev, flags)); assert_eq!(c, 2, "{flags}");
        let e = json::parse(&out).unwrap(); assert_eq!(e.get("error").and_then(|e| e.get("code")).and_then(Json::as_str), Some("flag"));
        assert_eq!(e.get("error").and_then(|e| e.get("path")).and_then(Json::as_str), Some(path)); }
    let (out, c) = pbit_wasm::run(r#"{"pbit_ir": 1, "values": ["a"], "vars": [{"id": "x", "zz": 1}]}"#); assert_eq!(c, 2);
    assert_eq!(json::parse(&out).unwrap().get("error").and_then(|e| e.get("path")).and_then(Json::as_str), Some("vars[0].zz"));
    let (p, c) = pbit_wasm::evaluate(&with_flags(ev, r#"{"program": true}"#)); assert_eq!(c, 0);
    let (r, c) = pbit_wasm::run(&p); assert_eq!(c, 0); assert_eq!(json::parse(&r).unwrap().get("verdict").and_then(Json::as_str), Some("exact"));
}
