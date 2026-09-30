//! End-to-end: `pbit demo | pbit decide` on the exact path and the sampler path; error exit codes.
use std::io::Write;
use std::process::{Command, Stdio};

fn pbit(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pbit")).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // A child that rejects its arguments exits before reading stdin; the broken pipe that follows is expected there,
    // so a failed write is not an error (a truncated input would fail the test's own assertions anyway).
    let _ = c.stdin.take().unwrap().write_all(stdin.as_bytes());
    let o = c.wait_with_output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
fn field<'a>(out: &'a str, key: &str) -> &'a str { let k = format!("\"{key}\":"); let s = out.find(&k).expect(key) + k.len(); let rest = &out[s..]; let e = rest.find(|c| c == ',' || c == '}').unwrap(); &rest[..e] }

#[test]
fn demo_then_decide_exact_and_sampler() {
    let (c, demo, _) = pbit(&["demo", "--tasks", "12"], ""); assert_eq!(c, 0);
    let (c, out, err) = pbit(&["decide"], &demo); assert_eq!(c, 0, "{err}");
    assert_eq!(field(&out, "verdict"), "\"exact\""); assert_eq!(field(&out, "violations"), "0"); assert!(out.contains("\"n_feasible\":"));
    let (c, out, err) = pbit(&["decide", "--mode", "sample", "--budget-ms", "150", "--seed", "3"], &demo); assert!(c == 0 || c == 3, "{err}");
    assert!(out.contains("\"gate\":")); assert_eq!(field(&out, "violations"), "0");
    let (c, demo, _) = pbit(&["demo", "--tasks", "300"], ""); assert_eq!(c, 0);
    let (c, out, err) = pbit(&["decide", "--budget-ms", "300"], &demo); assert!(c == 0 || c == 3, "{err}");
    let v = field(&out, "verdict"); assert!(["\"certified\"", "\"partial\"", "\"refused\""].contains(&v), "{v}");
    assert_eq!(field(&out, "violations"), "0");
}

#[test]
fn errors_have_exit_codes() {
    let (c, out, _) = pbit(&["decide"], r#"{"workers":[{"id":"a","cap":1}],"tasks":[{"id":"t1","scores":{"a":1}},{"id":"t2","scores":{"a":1}}]}"#);
    assert_eq!(c, 1); assert_eq!(field(&out, "verdict"), "\"infeasible\"");
    let (c, _, err) = pbit(&["decide"], "not json"); assert_eq!(c, 2); assert!(err.contains("bad JSON"));
    let (c, _, err) = pbit(&["decide"], r#"{"workers":[{"id":"a","cap":1}],"tasks":[{"id":"t1","scores":{"zz":1}}]}"#); assert_eq!(c, 2); assert!(err.contains("unknown worker"));
    let (c, out, _) = pbit(&["ir"], r#"{"workers":[{"id":"a","cap":1},{"id":"b","cap":1}],"tasks":[{"id":"t1","group":"g","scores":{"a":1,"b":0.5}},{"id":"t2","group":"g","scores":{"a":0.2,"b":1}}],"affinity":1.0}"#);
    assert_eq!(c, 0); assert!(out.starts_with("pbit-ir v0\nvars 2 2\n")); assert!(out.trim_end().ends_with("end"));
}

#[test]
fn clamp_is_honoured() {
    let (_, demo, _) = pbit(&["demo", "--tasks", "12"], "");
    let clamped = demo.replacen("\"id\": \"T000\",", "\"id\": \"T000\", \"clamp\": \"human\",", 1); assert_ne!(clamped, demo);
    let (c, out, err) = pbit(&["decide"], &clamped); assert_eq!(c, 0, "{err}"); assert!(out.contains("\"T000\":\"human\""));
}

#[test]
fn frontier_tier_is_exact_on_thin_queues() {
    // 13 tasks are past the 2M-plan enumeration limit (17.8M feasible plans), so auto mode used to sample; the
    // frontier DP now answers exactly. It must agree with brute-force enumeration (--mode exact) on log Z, the plan and the odds.
    let (_, demo, _) = pbit(&["demo", "--tasks", "13"], "");
    let (c, fr, err) = pbit(&["decide"], &demo); assert_eq!(c, 0, "{err}");
    assert_eq!(field(&fr, "verdict"), "\"exact\""); assert_eq!(field(&fr, "tier"), "\"frontier\""); assert_eq!(field(&fr, "violations"), "0");
    let (c, en, err) = pbit(&["decide", "--mode", "exact"], &demo); assert_eq!(c, 0, "{err}");
    assert_eq!(field(&en, "tier"), "\"enumerate\""); assert_eq!(field(&en, "n_feasible"), "17846010");
    assert_eq!(field(&fr, "logz"), field(&en, "logz"));
    let sub = |s: &str, k: &str| { let a = s.find(&format!("\"{k}\":")).unwrap(); let b = a + s[a..].find('}').unwrap(); s[a..=b].to_string() };
    assert_eq!(sub(&fr, "plan"), sub(&en, "plan"));
    let odds = |s: &str| { let a = s.find("\"odds\":").unwrap(); let b = a + s[a..].find("\"released\"").unwrap(); s[a..b].to_string() };
    assert_eq!(odds(&fr), odds(&en));
    // --frontier-states 0 switches the tier off (the earlier path: straight to the gated sampler)
    let (c, out, err) = pbit(&["decide", "--frontier-states", "0", "--budget-ms", "100"], &demo); assert!(c == 0 || c == 3, "{err}");
    assert!(out.contains("\"gate\":")); assert_eq!(field(&out, "violations"), "0");
}

#[test]
fn run_general_ir_programs() {
    // `pbit run` executes pbit-ir JSON v1 (docs/pbit-ir-json.md). Soft 3-colouring of a triangle with a forbidden value and
    // "at most one red": 12 feasible plans, exact odds.
    let tri = r#"{"pbit_ir": 1, "values": ["r", "g", "b"], "vars": [{"id": "a", "h": {"r": 0.5}}, {"id": "b"}, {"id": "c", "forbid": ["b"]}],
        "pairs": [{"i": "a", "j": "b", "potts": -2}, {"i": "b", "j": "c", "potts": -2}, {"i": "a", "j": "c", "potts": -2}], "caps": [{"limit": 1, "value": "r"}]}"#;
    let (c, out, err) = pbit(&["run"], tri); assert_eq!(c, 0, "{err}");
    assert_eq!(field(&out, "verdict"), "\"exact\""); assert_eq!(field(&out, "n_feasible"), "12"); assert_eq!(field(&out, "violations"), "0");
    assert!(out.contains("\"plan\":{\"a\":\"r\",\"b\":\"b\",\"c\":\"g\"}"), "{out}");
    let (c, out, _) = pbit(&["run"], &tri.replace("{\"id\": \"b\"}", "{\"id\": \"b\", \"clamp\": \"r\"}")); assert_eq!(c, 0); // what-if: b is red, so a is not
    assert!(out.contains("\"b\":{\"r\":1,") && out.contains("\"a\":{\"b\":0.880797,\"g\":0.119203,\"r\":0}") && field(&out, "n_feasible") == "2", "{out}");
    // 60-spin Ising ring (table couplings, no capacities): sampled and gated, never enumerated
    let vars: Vec<String> = (0..60).map(|i| format!("{{\"id\": \"s{i}\", \"h\": {{\"+\": {}}}}}", 0.1 * ((i % 7) as f64 - 3.0))).collect();
    let pairs: Vec<String> = (0..60).map(|i| format!("{{\"i\": \"s{i}\", \"j\": \"s{}\", \"table\": [[0.4, -0.4], [-0.4, 0.4]]}}", (i + 1) % 60)).collect();
    let ring = format!("{{\"pbit_ir\": 1, \"values\": [\"-\", \"+\"], \"vars\": [{}], \"pairs\": [{}]}}", vars.join(","), pairs.join(","));
    let (c, out, err) = pbit(&["run", "--budget-ms", "100"], &ring); assert!(c == 0 || c == 3, "{err}");
    assert_eq!(field(&out, "tier"), "\"sample\""); assert!(out.contains("\"gate\":")); assert_eq!(field(&out, "violations"), "0");
    let lw = |k: &str| field(&out, k).parse::<f64>().unwrap(); assert!(lw("plan_logw") >= lw("sampled_best_logw"), "polish made the plan worse");
    let th = std::thread::available_parallelism().map_or(1, |n| n.get()).min(4); // default: min(4, cores)
    assert!(out.contains(&format!("\"telemetry\":{{\"chains\":4,\"threads\":{th},")) && lw("site_updates_per_s") > 1e5, "{out}");
    if cfg!(unix) { assert!(lw("process_cpu_ms") > 0.0 && lw("peak_rss_mb") > 1.0, "{out}"); } // getrusage FFI
    // fixed work (--sweeps) + no polish => the decision is a pure function of (program, seed): byte-identical runs
    let strip = |o: &str| { let mut t = o.to_string(); for k in ["\"ms\":", "\"sample_ms\":", "\"gate_ms\":", "\"site_updates_per_s\":", "\"process_cpu_ms\":", "\"peak_rss_mb\":"] { while let Some(a) = t.find(k) { let b = a + t[a..].find(|ch| ch == ',' || ch == '}').unwrap(); t.replace_range(a..b, "_"); } } t };
    let run1 = pbit(&["run", "--op", "sample", "--sweeps", "400", "--polish-ms", "0", "--seed", "5"], &ring); let run2 = pbit(&["run", "--op", "sample", "--sweeps", "400", "--polish-ms", "0", "--seed", "5"], &ring);
    assert_eq!(strip(&run1.1), strip(&run2.1)); assert!(run1.1.contains("\"sweeps\":1600"), "{}", run1.1);
    let (c, _, err) = pbit(&["run"], r#"{"pbit_ir": 1, "values": ["x"], "vars": [{"id": "a", "h": {"y": 1}}]}"#); assert_eq!(c, 2); assert!(err.contains("unknown value y"));
}

#[test]
fn sudoku_exact_tier_solves_and_sampler_refuses_honestly() {
    // One-hot sudoku as a pbit-ir program (243 at-most-one capacities over (cell, digit), givens as clamps).
    // Enumeration proves the solution unique; the sampler must find a start (dynamic-MRV search) and must NOT claim
    // "infeasible", and its gate refuses a chain that cannot move (frozen saturated capacities).
    let p = "530070000600195000098000060800060003400803001700020006060000280000419005000080079";
    let mk = |p: &str| -> String {
    let vars: Vec<String> = (0..81).map(|q| { let d = &p[q..q + 1]; if d == "0" { format!("{{\"id\": \"c{q}\"}}") } else { format!("{{\"id\": \"c{q}\", \"clamp\": \"{d}\"}}") } }).collect();
    let mut units: Vec<Vec<usize>> = (0..9).map(|r| (0..9).map(|c| r * 9 + c).collect()).collect();
    units.extend((0..9).map(|c| (0..9).map(|r| r * 9 + c).collect::<Vec<_>>())); units.extend((0..9).map(|b| (0..9).map(|q| (b / 3 * 3 + q / 3) * 9 + b % 3 * 3 + q % 3).collect::<Vec<_>>()));
    let caps: Vec<String> = units.iter().flat_map(|u| (1..=9).map(move |d| format!("{{\"limit\": 1, \"members\": [{}]}}", u.iter().map(|q| format!("[\"c{q}\", \"{d}\"]")).collect::<Vec<_>>().join(",")))).collect();
    format!("{{\"pbit_ir\": 1, \"values\": [\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\"], \"vars\": [{}], \"caps\": [{}]}}", vars.join(","), caps.join(",")) };
    let prog = mk(p);
    let (c, out, err) = pbit(&["run"], &prog); assert_eq!(c, 0, "{err}");
    assert_eq!(field(&out, "verdict"), "\"exact\""); assert_eq!(field(&out, "n_feasible"), "1"); assert_eq!(field(&out, "violations"), "0");
    assert!(out.contains("\"c0\":\"5\",\"c1\":\"3\",\"c2\":\"4\",\"c3\":\"6\",\"c4\":\"7\",\"c5\":\"8\",\"c6\":\"9\",\"c7\":\"1\",\"c8\":\"2\""), "{out}");
    // This puzzle falls to unit propagation (naked singles), so every cell is a constant under the target: no chain is
    // stuck and the gate certifies the sampler's answer, the unique solution. With rows 7-9 cleared (several solutions: those rows
    // can be permuted; a full grid is rigid under site and swap moves) the chains cannot move and the gate refuses.
    let (c, out, err) = pbit(&["run", "--op", "sample", "--budget-ms", "60", "--polish-ms", "0"], &prog);
    assert_eq!(c, 0, "{err}"); assert_eq!(field(&out, "verdict"), "\"certified\"", "{out}"); assert_eq!(field(&out, "violations"), "0");
    assert!(out.contains("\"c0\":\"5\",\"c1\":\"3\",\"c2\":\"4\",\"c3\":\"6\",\"c4\":\"7\",\"c5\":\"8\",\"c6\":\"9\",\"c7\":\"1\",\"c8\":\"2\""), "{out}");
    let (c, out, _) = pbit(&["run", "--op", "sample", "--budget-ms", "60", "--polish-ms", "0"], &mk(&format!("{}{}", &p[..54], "0".repeat(27))));
    assert_eq!(c, 3); assert_eq!(field(&out, "verdict"), "\"refused\"", "{out}"); assert_eq!(field(&out, "violations"), "0");
}

#[test]
fn run_partial_release_next_to_a_stuck_component() {
    // A stuck part escalates only its own connected component. A 4x4 sudoku with 2 solutions no local move connects
    // (every chain is stuck) next to a free variable x that shares nothing with it: `--op sample` releases x alone (`partial`,
    // exit 0), the cells and the givens are escalated. Before per-component escalation the whole answer was refused.
    let sol = [0, 1, 2, 3, 2, 3, 0, 1, 1, 0, 3, 2, 3, 2, 1, 0];
    let mut vars: Vec<String> = (0..16).map(|q| if q % 3 == 0 || q == 5 || q == 10 { format!("{{\"id\": \"c{q}\", \"clamp\": \"{}\"}}", sol[q] + 1) } else { format!("{{\"id\": \"c{q}\"}}") }).collect();
    vars.push(r#"{"id": "x", "h": {"1": -0.5, "2": -0.1, "3": 0.3, "4": 0.7}}"#.into());
    let mut units: Vec<Vec<usize>> = (0..4).map(|r| (0..4).map(|c| r * 4 + c).collect()).collect();
    units.extend((0..4).map(|c| (0..4).map(|r| r * 4 + c).collect::<Vec<_>>())); units.extend((0..4).map(|b| (0..4).map(|q| (b / 2 * 2 + q / 2) * 4 + b % 2 * 2 + q % 2).collect::<Vec<_>>()));
    let caps: Vec<String> = units.iter().flat_map(|u| (1..=4).map(move |d| format!("{{\"limit\": 1, \"members\": [{}]}}", u.iter().map(|q| format!("[\"c{q}\", \"{d}\"]")).collect::<Vec<_>>().join(",")))).collect();
    let prog = format!("{{\"pbit_ir\": 1, \"values\": [\"1\",\"2\",\"3\",\"4\"], \"vars\": [{}], \"caps\": [{}]}}", vars.join(","), caps.join(","));
    let (c, out, err) = pbit(&["run", "--op", "sample", "--sweeps", "4000", "--polish-ms", "0", "--seed", "5"], &prog);
    assert_eq!(c, 0, "{err}"); assert_eq!(field(&out, "verdict"), "\"partial\"", "{out}"); assert_eq!(field(&out, "released"), "[\"x\"]", "{out}");
    assert_eq!(field(&out, "violations"), "0"); assert!(out.contains("\"escalated\":[\"c0\",\"c1\","), "{out}");
    assert_eq!(field(&out, "frozen_escalated"), "16", "{out}"); // the 16 cells (8 stuck, 8 givens); x is not escalated by the frozen rule
}

#[test]
fn run_frontier_tier_is_exact_beyond_enumeration() {
    // `pbit run` decide = enumerate -> frontier DP -> sampler. On the triangle, forcing enumeration off must give the
    // same odds and plan from the frontier tier; --frontier-states 0 falls through to the sampler.
    let tri = r#"{"pbit_ir": 1, "values": ["r", "g", "b"], "vars": [{"id": "a", "h": {"r": 0.5}}, {"id": "b"}, {"id": "c", "forbid": ["b"]}],
        "pairs": [{"i": "a", "j": "b", "potts": -2}, {"i": "b", "j": "c", "potts": -2}, {"i": "a", "j": "c", "potts": -2}], "caps": [{"limit": 1, "value": "r"}]}"#;
    let sec = |o: &str| { let a = o.find("\"marginals\":").unwrap(); o[a..a + o[a..].find(",\"released\"").unwrap()].to_string() };
    let (c, en, _) = pbit(&["run"], tri); assert_eq!(c, 0); assert_eq!(field(&en, "tier"), "\"enumerate\"");
    let (c, fr, err) = pbit(&["run", "--exact-limit", "5"], tri); assert_eq!(c, 0, "{err}");
    assert_eq!(field(&fr, "tier"), "\"frontier\""); assert_eq!(sec(&en), sec(&fr)); assert_eq!(field(&en, "plan_logw"), field(&fr, "plan_logw"));
    let (_, sa, _) = pbit(&["run", "--exact-limit", "5", "--frontier-states", "0", "--budget-ms", "50"], tri); assert_eq!(field(&sa, "tier"), "\"sample\"");
    // 40 two-person teams over 3 shifts (Potts inside a team), global quotas on shifts a and b: 3^80 plans, far beyond
    // enumeration; the frontier carries the two quota loads (<= 11 x 16 states) and answers exactly
    let vars: Vec<String> = (0..80).map(|i| format!("{{\"id\": \"p{i}\", \"h\": {{\"a\": {}, \"b\": {}}}}}", 0.1 * ((i % 5) as f64), -0.05 * ((i % 3) as f64))).collect();
    let pairs: Vec<String> = (0..40).map(|t| format!("{{\"i\": \"p{}\", \"j\": \"p{}\", \"potts\": 0.7}}", 2 * t, 2 * t + 1)).collect();
    let prog = format!("{{\"pbit_ir\": 1, \"values\": [\"a\", \"b\", \"c\"], \"vars\": [{}], \"pairs\": [{}], \"caps\": [{{\"limit\": 10, \"value\": \"a\"}}, {{\"limit\": 15, \"value\": \"b\"}}]}}", vars.join(","), pairs.join(","));
    let (c, out, err) = pbit(&["run"], &prog); assert_eq!(c, 0, "{err}");
    assert_eq!(field(&out, "verdict"), "\"exact\""); assert_eq!(field(&out, "tier"), "\"frontier\""); assert_eq!(field(&out, "violations"), "0");
    assert!(field(&out, "frontier_states").parse::<usize>().unwrap() <= 11 * 16, "{out}");
}

#[test]
fn run_threads_and_chains_are_resource_controls() {
    // --threads / PBIT_THREADS change how chains are scheduled, never the answer (fixed --sweeps, no polish);
    // --chains is reported in gate + telemetry; bad values exit 2.
    let vars: Vec<String> = (0..40).map(|i| format!("{{\"id\": \"s{i}\", \"h\": {{\"+\": {}}}}}", 0.1 * ((i % 7) as f64 - 3.0))).collect();
    let pairs: Vec<String> = (0..40).map(|i| format!("{{\"i\": \"s{i}\", \"j\": \"s{}\", \"table\": [[0.4, -0.4], [-0.4, 0.4]]}}", (i + 1) % 40)).collect();
    let ring = format!("{{\"pbit_ir\": 1, \"values\": [\"-\", \"+\"], \"vars\": [{}], \"pairs\": [{}]}}", vars.join(","), pairs.join(","));
    let strip = |o: &str| { let mut t = o.to_string(); for k in ["\"ms\":", "\"sample_ms\":", "\"gate_ms\":", "\"site_updates_per_s\":", "\"process_cpu_ms\":", "\"peak_rss_mb\":", "\"threads\":"] { while let Some(a) = t.find(k) { let b = a + t[a..].find(|ch| ch == ',' || ch == '}').unwrap(); t.replace_range(a..b, "_"); } } t };
    let base = ["run", "--op", "sample", "--sweeps", "300", "--polish-ms", "0", "--seed", "3", "--chains", "6"];
    let outs: Vec<String> = ["1", "2", "4", "6"].iter().map(|t| { let mut a = base.to_vec(); a.extend(["--threads", t]); let (c, o, e) = pbit(&a, &ring); assert!(c == 0 || c == 3, "{e}"); o }).collect();
    for o in &outs[1..] { assert_eq!(strip(&outs[0]), strip(o)); }
    assert!(outs[0].contains("\"telemetry\":{\"chains\":6,\"threads\":1,") && outs[2].contains("\"chains\":6,\"threads\":4,") && outs[0].contains("\"sweeps\":1800"), "{}", outs[0]);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pbit")).args(base).env("PBIT_THREADS", "2").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn()
        .and_then(|mut ch| { use std::io::Write; ch.stdin.take().unwrap().write_all(ring.as_bytes())?; ch.wait_with_output() }).unwrap();
    let o = String::from_utf8_lossy(&out.stdout).to_string(); assert!(o.contains("\"threads\":2,"), "{o}"); assert_eq!(strip(&outs[0]), strip(&o));
    let (c, _, err) = pbit(&["run", "--threads", "0"], &ring); assert_eq!(c, 2); assert!(err.contains("--threads"), "{err}");
    // --cpu-limit is a duty cycle: same answer at fixed sweeps (the field itself differs, so mask it); > 100 exits 2
    let mut a = base.to_vec(); a.extend(["--threads", "2", "--cpu-limit", "50"]); let (_, o, _) = pbit(&a, &ring);
    assert!(o.contains("\"cpu_limit_pct\":50"), "{o}"); assert_eq!(strip(&outs[0]).replace("\"cpu_limit_pct\":100", ""), strip(&o).replace("\"cpu_limit_pct\":50", ""));
    let (c, _, err) = pbit(&["run", "--cpu-limit", "150"], &ring); assert_eq!(c, 2); assert!(err.contains("--cpu-limit"), "{err}");
}

#[test]
fn run_priority_is_a_resource_control() {
    // --priority low / PBIT_PRIORITY=low -> nice >= 10 (setpriority FFI, reported in telemetry); bad values exit 2.
    let prog = r#"{"pbit_ir": 1, "values": ["-", "+"], "vars": [{"id": "a"}, {"id": "b"}, {"id": "c"}], "pairs": [{"i": "a", "j": "b", "table": [[0.3, 0], [0, 0.3]]}]}"#;
    let args = ["run", "--op", "sample", "--sweeps", "200", "--polish-ms", "0"];
    let nice = |o: &str| field(o, "nice").parse::<i64>().unwrap();
    let (c, base, err) = pbit(&args, prog); assert!(c == 0 || c == 3, "{err}");
    let mut a = args.to_vec(); a.extend(["--priority", "low"]); let (c, low, err) = pbit(&a, prog);
    // Off Unix `--priority low` is not implemented and exits 2 (documented), so the Windows CI job checks exactly that
    if cfg!(unix) { assert!(c == 0 || c == 3, "{err}"); assert!(nice(&low) >= 10 && nice(&low) >= nice(&base), "{low}"); } else { assert_eq!(c, 2, "{err}"); }
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pbit")).args(args).env("PBIT_PRIORITY", "low").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn()
        .and_then(|mut ch| { use std::io::Write; ch.stdin.take().unwrap().write_all(prog.as_bytes())?; ch.wait_with_output() }).unwrap();
    if cfg!(unix) { assert!(nice(&String::from_utf8_lossy(&out.stdout)) >= 10); }
    let mut a = args.to_vec(); a.extend(["--priority", "high"]); let (c, _, err) = pbit(&a, prog); assert_eq!(c, 2); assert!(err.contains("--priority"), "{err}");
}

#[test]
fn run_config_file_layer() {
    // Precedence flag > PBIT_* env > pbit.json (cwd, or $PBIT_CONFIG) > default, for chains / threads / cpu_limit / priority.
    let dir = std::env::temp_dir().join(format!("pbit-r8-cfg-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
    // "priority": "low" only where it is implemented (Unix); off Unix it would make every run exit 2
    std::fs::write(dir.join("pbit.json"), format!(r#"{{"chains": 3, "threads": 2, "cpu_limit": 90{}}}"#, if cfg!(unix) { r#", "priority": "low""# } else { "" })).unwrap();
    let prog = r#"{"pbit_ir": 1, "values": ["-", "+"], "vars": [{"id": "a"}, {"id": "b"}], "pairs": [{"i": "a", "j": "b", "table": [[0.3, 0], [0, 0.3]]}]}"#;
    let go = |extra: &[&str], env: &[(&str, &str)], cwd: &std::path::Path| { let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_pbit"));
        c.args(["run", "--op", "sample", "--sweeps", "100", "--polish-ms", "0"]).args(extra).current_dir(cwd).env_remove("PBIT_CHAINS").env_remove("PBIT_THREADS").env_remove("PBIT_CONFIG");
        for (k, v) in env { c.env(k, v); }
        let out = c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn()
            .and_then(|mut ch| { use std::io::Write; ch.stdin.take().unwrap().write_all(prog.as_bytes())?; ch.wait_with_output() }).unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string()) };
    let (c, o, e) = go(&[], &[], &dir); assert!(c == 0 || c == 3, "{e}");
    assert!(o.contains("\"chains\":3,\"threads\":2,") && o.contains("\"cpu_limit_pct\":90"), "{o}"); if cfg!(unix) { assert!(field(&o, "nice").parse::<i64>().unwrap() >= 10); }
    let (_, o, _) = go(&["--chains", "5"], &[("PBIT_CHAINS", "4")], &dir); assert!(o.contains("\"chains\":5,"), "flag wins: {o}");
    let (_, o, _) = go(&[], &[("PBIT_CHAINS", "4")], &dir); assert!(o.contains("\"chains\":4,"), "env beats file: {o}");
    let other = std::env::temp_dir(); let cfg = dir.join("pbit.json"); let cfg = cfg.to_str().unwrap();
    let (_, o, _) = go(&[], &[("PBIT_CONFIG", cfg)], &other); assert!(o.contains("\"chains\":3,"), "PBIT_CONFIG: {o}");
    std::fs::write(dir.join("pbit.json"), r#"{"threads": 0}"#).unwrap(); let (c, _, e) = go(&[], &[], &dir); assert_eq!(c, 2, "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn decide_sampler_path_reports_telemetry() {
    // `pbit decide` answers from the sampler on a 60-task demo (the exact tiers decline) and reports telemetry.
    let (c, demo, _) = pbit(&["demo", "--tasks", "60"], ""); assert_eq!(c, 0);
    let (c, out, err) = pbit(&["decide", "--budget-ms", "60", "--polish-ms", "0"], &demo); assert!(c == 0 || c == 3, "{err}");
    let th = std::thread::available_parallelism().map_or(1, |n| n.get()).min(4); // default: min(4, cores)
    assert!(out.contains(&format!("\"telemetry\":{{\"chains\":4,\"threads\":{th},")), "{out}");
    if cfg!(unix) { assert!(field(&out, "process_cpu_ms").parse::<f64>().unwrap() > 0.0 && field(&out, "peak_rss_mb").parse::<f64>().unwrap() > 1.0, "{out}"); }
}

#[test]
fn decide_takes_the_processor_controls() {
    // `pbit decide` (router sampler path) takes the controls of `pbit run`: --threads / PBIT_THREADS never change the
    // answer at fixed --sweeps with no polish; --chains, --cpu-limit, --priority are reported in gate + telemetry; bad values exit 2.
    let (c, demo, _) = pbit(&["demo", "--tasks", "60"], ""); assert_eq!(c, 0);
    let strip = |o: &str| { let mut t = o.to_string(); for k in ["\"ms\":", "\"sample_ms\":", "\"gate_ms\":", "\"site_updates_per_s\":", "\"process_cpu_ms\":", "\"peak_rss_mb\":", "\"threads\":", "\"nice\":", "\"cpu_limit_pct\":"] { while let Some(a) = t.find(k) { let b = a + t[a..].find(|ch| ch == ',' || ch == '}').unwrap(); t.replace_range(a..b, "_"); } } t };
    let base = ["decide", "--sweeps", "300", "--polish-ms", "0", "--seed", "3", "--chains", "6"];
    let outs: Vec<String> = ["1", "2", "4", "6"].iter().map(|t| { let mut a = base.to_vec(); a.extend(["--threads", t]); let (c, o, e) = pbit(&a, &demo); assert!(c == 0 || c == 3, "{e}"); o }).collect();
    for o in &outs[1..] { assert_eq!(strip(&outs[0]), strip(o)); }
    assert!(outs[0].contains("\"telemetry\":{\"chains\":6,\"threads\":1,") && outs[2].contains("\"chains\":6,\"threads\":4,") && outs[0].contains("\"sweeps\":1800"), "{}", outs[0]);
    let mut a = base.to_vec(); a.extend(["--threads", "2", "--cpu-limit", "50"]); if cfg!(unix) { a.extend(["--priority", "low"]); } // exit 2 off Unix (documented)
    let (c, o, e) = pbit(&a, &demo); assert!(c == 0 || c == 3, "{e}");
    assert!(o.contains("\"cpu_limit_pct\":50"), "{o}"); assert_eq!(strip(&outs[0]), strip(&o));
    if cfg!(unix) { assert!(field(&o, "nice").parse::<i64>().unwrap() >= 10, "{o}"); }
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pbit")).args(base).env("PBIT_THREADS", "3").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn()
        .and_then(|mut ch| { ch.stdin.take().unwrap().write_all(demo.as_bytes())?; ch.wait_with_output() }).unwrap();
    let o = String::from_utf8_lossy(&out.stdout).to_string(); assert!(o.contains("\"threads\":3,"), "{o}"); assert_eq!(strip(&outs[0]), strip(&o));
    for bad in [["--chains", "0"], ["--threads", "0"], ["--cpu-limit", "101"], ["--priority", "high"]] { let (c, _, e) = pbit(&["decide", bad[0], bad[1]], &demo); assert_eq!(c, 2, "{bad:?}: {e}"); }
    // decide reads the config file layer too ($PBIT_CONFIG; env beats file)
    let cfg = std::env::temp_dir().join(format!("pbit-r9-decide-cfg-{}.json", std::process::id())); std::fs::write(&cfg, r#"{"chains": 5, "mem_limit_mb": 2}"#).unwrap();
    let go = |env: &[(&str, &str)]| { let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_pbit")); c.args(["decide", "--sweeps", "200", "--polish-ms", "0"]).env("PBIT_CONFIG", &cfg);
        for (k, v) in env { c.env(k, v); } let o = c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn()
            .and_then(|mut ch| { ch.stdin.take().unwrap().write_all(demo.as_bytes())?; ch.wait_with_output() }).unwrap(); String::from_utf8_lossy(&o.stdout).to_string() };
    let o = go(&[]); assert!(o.contains("\"telemetry\":{\"chains\":5,") && o.contains("\"mem_limit_mb\":2,"), "{o}");
    let o = go(&[("PBIT_CHAINS", "3")]); assert!(o.contains("\"telemetry\":{\"chains\":3,"), "env beats file: {o}"); let _ = std::fs::remove_file(&cfg);
}

#[test]
fn progress_streams_jsonl_telemetry_on_stderr() {
    // --progress MS = JSONL telemetry lines on stderr while the decision runs (decide and run); the answer is unchanged.
    let strip = |o: &str| { let mut t = o.to_string(); for k in ["\"ms\":", "\"sample_ms\":", "\"gate_ms\":", "\"site_updates_per_s\":", "\"process_cpu_ms\":", "\"peak_rss_mb\":"] { while let Some(a) = t.find(k) { let b = a + t[a..].find(|ch| ch == ',' || ch == '}').unwrap(); t.replace_range(a..b, "_"); } } t };
    let check = |err: &str, total: f64| { let lines: Vec<&str> = err.lines().collect(); assert!(lines.len() >= 2, "{err}"); let mut last = 0.0;
        for l in &lines { assert!(l.starts_with("{\"event\":\"progress\",\"ms\":") && l.ends_with('}'), "{l}"); let s: f64 = field(l, "sweeps").parse().unwrap(); assert!(s >= last && s <= total, "{l}"); last = s; }
        assert!(last > 0.0, "no chain progress reported: {err}"); };
    let (c, demo, _) = pbit(&["demo", "--tasks", "60"], ""); assert_eq!(c, 0);
    let args = ["decide", "--sweeps", "20000", "--polish-ms", "0", "--seed", "4"];
    let (c0, plain, e0) = pbit(&args, &demo); assert!(e0.is_empty(), "stderr must stay empty without --progress: {e0}");
    let mut a = args.to_vec(); a.extend(["--progress", "10"]); let (c1, prog, err) = pbit(&a, &demo); assert_eq!(c0, c1);
    assert_eq!(strip(&plain), strip(&prog)); check(&err, field(&prog, "sweeps").parse().unwrap());
    let vars: Vec<String> = (0..400).map(|i| format!("{{\"id\": \"s{i}\", \"h\": {{\"+\": {}}}}}", 0.1 * ((i % 7) as f64 - 3.0))).collect();
    let pairs: Vec<String> = (0..400).map(|i| format!("{{\"i\": \"s{i}\", \"j\": \"s{}\", \"table\": [[0.4, -0.4], [-0.4, 0.4]]}}", (i + 1) % 400)).collect();
    let ring = format!("{{\"pbit_ir\": 1, \"values\": [\"-\", \"+\"], \"vars\": [{}], \"pairs\": [{}]}}", vars.join(","), pairs.join(","));
    let args = ["run", "--op", "sample", "--sweeps", "3000", "--polish-ms", "0", "--progress", "10"];
    let (c, out, err) = pbit(&args, &ring); assert!(c == 0 || c == 3, "{err}"); check(&err, field(&out, "sweeps").parse().unwrap());
}

#[test]
fn stats_is_the_processor_spec_sheet() {
    // `pbit stats` reports machine, effective controls with their source (flag > env > file > default) and a measured self-test.
    let (c, out, err) = pbit(&["stats", "--sweeps", "200"], ""); assert_eq!(c, 0, "{err}");
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    assert!(out.contains(&format!("\"logical_cpus\":{cores}")) && out.contains(&format!("\"threads\":{{\"value\":{},\"source\":\"default\"}}", cores.min(4))), "{out}");
    assert!(field(&out, "site_updates_per_s").parse::<f64>().unwrap() > 1e5 && field(&out, "site_updates_per_s_1_thread").parse::<f64>().unwrap() > 1e5, "{out}");
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_pbit")).args(["stats", "--sweeps", "100", "--chains", "3", "--priority", "low"]).env("PBIT_THREADS", "2").output().unwrap();
    let o = String::from_utf8_lossy(&o.stdout).to_string(); assert!(o.contains("\"mem_limit_mb\":{\"value\":1024,\"source\":\"default\"}"), "{o}");
    assert!(o.contains("\"chains\":{\"value\":3,\"source\":\"flag\"}") && o.contains("\"threads\":{\"value\":2,\"source\":\"env\"}") && o.contains("\"priority\":{\"value\":\"low\",\"source\":\"flag\"}"), "{o}");
    let (c, _, e) = pbit(&["stats", "--threads", "0"], ""); assert_eq!(c, 2, "{e}");
}

#[test]
fn mem_limit_bounds_the_sample_buffers() {
    // --mem-limit-mb / PBIT_MEM_LIMIT_MB caps the trajectory rows per chain (thinning), reported in telemetry; the answer
    // stays valid and deterministic at fixed sweeps; junk exits 2. Default 1024 MB; 0 = unbounded (same answer when the cap never binds).
    let (c, demo, _) = pbit(&["demo", "--tasks", "60"], ""); assert_eq!(c, 0);
    let base = ["decide", "--sweeps", "20000", "--polish-ms", "0", "--seed", "2"];
    let (_, dflt, _) = pbit(&base, &demo); assert_eq!(field(&dflt, "traj_rows"), "72000", "{dflt}"); assert_eq!(field(&dflt, "mem_limit_mb"), "1024");
    let mut u = base.to_vec(); u.extend(["--mem-limit-mb", "0"]); let (c, free, e) = pbit(&u, &demo); assert!(c == 0 || c == 3, "{e}");
    assert_eq!(field(&free, "traj_rows"), "72000", "{free}"); assert_eq!(field(&free, "mem_limit_mb"), "0");
    let mut a = base.to_vec(); a.extend(["--mem-limit-mb", "1"]); let (c, capped, e) = pbit(&a, &demo); assert!(c == 0 || c == 3, "{e}");
    let rows: usize = field(&capped, "traj_rows").parse().unwrap(); assert!(rows <= 4 * (1 << 20) / (4 * (2 * 60 + 8)) && rows >= 4 * 64, "{capped}");
    assert_eq!(field(&capped, "violations"), "0"); assert_eq!(field(&capped, "mem_limit_mb"), "1");
    let strip = |o: &str| { let mut t = o.to_string(); for k in ["\"ms\":", "\"sample_ms\":", "\"gate_ms\":", "\"site_updates_per_s\":", "\"process_cpu_ms\":", "\"peak_rss_mb\":"] { while let Some(a) = t.find(k) { let b = a + t[a..].find(|ch| ch == ',' || ch == '}').unwrap(); t.replace_range(a..b, "_"); } } t };
    let (_, again, _) = pbit(&a, &demo); assert_eq!(strip(&capped), strip(&again));
    assert_eq!(strip(&free).replace("\"mem_limit_mb\":0,", ""), strip(&dflt).replace("\"mem_limit_mb\":1024,", ""));
    let ring = r#"{"pbit_ir": 1, "values": ["-", "+"], "vars": [{"id": "a"}, {"id": "b"}, {"id": "c"}], "pairs": [{"i": "a", "j": "b", "table": [[0.3, 0], [0, 0.3]]}]}"#;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pbit")).args(["run", "--op", "sample", "--sweeps", "100000", "--polish-ms", "0"]).env("PBIT_MEM_LIMIT_MB", "1").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn()
        .and_then(|mut ch| { ch.stdin.take().unwrap().write_all(ring.as_bytes())?; ch.wait_with_output() }).unwrap();
    let o = String::from_utf8_lossy(&out.stdout).to_string(); let rows: usize = field(&o, "traj_rows").parse().unwrap();
    assert!(rows <= (1 << 20) / (2 * 3 + 8) && rows < 4 * 90000, "{o}"); assert_eq!(field(&o, "mem_limit_mb"), "1");
    for bad in ["-1", "x"] { let (c, _, e) = pbit(&["decide", "--mem-limit-mb", bad], &demo); assert_eq!(c, 2, "{bad}: {e}"); }
}

#[test]
fn polish_sweeps_makes_the_whole_answer_deterministic() {
    // The default --polish-ms polish is wall-clock, so at fixed --sweeps the odds were identical for any --threads but the
    // polished plan could differ run to run. --polish-sweeps N polishes for fixed work: plan + plan_logw identical too.
    let strip = |o: &str| { let mut t = o.to_string(); for k in ["\"ms\":", "\"sample_ms\":", "\"gate_ms\":", "\"site_updates_per_s\":", "\"process_cpu_ms\":", "\"peak_rss_mb\":", "\"threads\":"] { while let Some(a) = t.find(k) { let b = a + t[a..].find(|ch| ch == ',' || ch == '}').unwrap(); t.replace_range(a..b, "_"); } } t };
    let (c, demo, _) = pbit(&["demo", "--tasks", "60", "--seed", "3"], ""); assert_eq!(c, 0);
    let base = ["decide", "--mode", "sample", "--sweeps", "120", "--polish-sweeps", "200", "--seed", "5"];
    let outs: Vec<String> = ["1", "4", "2", "4"].iter().map(|t| { let mut a = base.to_vec(); a.extend(["--threads", t]); let (c, o, e) = pbit(&a, &demo); assert!(c == 0 || c == 3, "{e}"); o }).collect();
    for o in &outs[1..] { assert_eq!(strip(&outs[0]), strip(o)); }
    assert!(outs[0].contains("\"polish_sweeps\":200") && outs[0].contains("\"polish_ms\":0"), "{}", outs[0]);
    let (_, raw, _) = pbit(&["decide", "--mode", "sample", "--sweeps", "120", "--polish-ms", "0", "--seed", "5"], &demo);
    let lw = |o: &str| field(o, "plan_logw").parse::<f64>().unwrap(); assert!(lw(&outs[0]) >= lw(&raw), "polish must never be worse than the sampled plan");
    // same contract on `pbit run` (general IR program: a 40-spin frustrated ring)
    let vars: Vec<String> = (0..40).map(|i| format!("{{\"id\": \"s{i}\", \"h\": {{\"+\": {}}}}}", 0.1 * ((i % 7) as f64 - 3.0))).collect();
    let pairs: Vec<String> = (0..40).map(|i| format!("{{\"i\": \"s{i}\", \"j\": \"s{}\", \"table\": [[0.4, -0.4], [-0.4, 0.4]]}}", (i + 1) % 40)).collect();
    let ring = format!("{{\"pbit_ir\": 1, \"values\": [\"-\", \"+\"], \"vars\": [{}], \"pairs\": [{}]}}", vars.join(","), pairs.join(","));
    let base = ["run", "--op", "sample", "--sweeps", "300", "--polish-sweeps", "100", "--seed", "3", "--chains", "4"];
    let outs: Vec<String> = ["1", "4", "4"].iter().map(|t| { let mut a = base.to_vec(); a.extend(["--threads", t]); let (c, o, e) = pbit(&a, &ring); assert!(c == 0 || c == 3, "{e}"); o }).collect();
    for o in &outs[1..] { assert_eq!(strip(&outs[0]), strip(o)); }
    assert!(outs[0].contains("\"polish_sweeps\":100"), "{}", outs[0]);
    // The polish honours --threads (it ran 4 threads whatever --threads said): with 1 thread and a sampler-free budget
    // the process CPU stays near the wall time (4 polish threads for 200 ms would add ~600 ms of CPU)
    let (c, o, e) = pbit(&["run", "--op", "sample", "--sweeps", "50", "--polish-ms", "200", "--threads", "1", "--chains", "4"], &ring); assert!(c == 0 || c == 3, "{e}");
    if let Ok(cpu) = field(&o, "process_cpu_ms").parse::<f64>() { // null off Unix (no getrusage)
        assert!(cpu < 450.0, "--threads 1 polish used {cpu} ms CPU (wall {} ms): more than one thread", field(&o, "ms")); }
}

#[test]
fn tiny_budget_refuses_instead_of_aborting() {
    // A budget spent before any row was recorded (the first 20 sweeps are burn-in) left the sampler's best plan empty;
    // the polish then indexed it and both `pbit decide` (300-task demo) and `pbit run` aborted with exit 134 at --budget-ms 0.01.
    let (c, demo, err) = pbit(&["demo", "--tasks", "300"], ""); assert_eq!(c, 0, "{err}");
    for b in ["0.000001", "0.01"] {
        let (c, out, err) = pbit(&["decide", "--budget-ms", b], &demo); assert_eq!(c, 3, "decide {b}: {err}");
        assert_eq!(field(&out, "verdict"), "\"refused\""); assert_eq!(field(&out, "violations"), "0");
    }
    let vars: Vec<String> = (0..40).map(|i| format!("{{\"id\": \"s{i}\"}}")).collect();
    let pairs: Vec<String> = (0..40).map(|i| format!("{{\"i\": \"s{i}\", \"j\": \"s{}\", \"table\": [[0.4, -0.4], [-0.4, 0.4]]}}", (i + 1) % 40)).collect();
    let ring = format!("{{\"pbit_ir\": 1, \"values\": [\"-\", \"+\"], \"vars\": [{}], \"pairs\": [{}]}}", vars.join(","), pairs.join(","));
    for op in ["sample", "decide"] {
        let (c, out, err) = pbit(&["run", "--op", op, "--budget-ms", "0.000001"], &ring); assert_eq!(c, 3, "run {op}: {err}");
        assert_eq!(field(&out, "verdict"), "\"refused\""); assert_eq!(field(&out, "violations"), "0");
    }
}
#[test]
fn decide_refusal_points_to_the_exhaustive_exact_op() {
    // An infeasible program whose search outlasts the exact tier's gap budget (pigeonhole: 9 pigeons, 8 holes, each
    // hole's cap listed 100x, as in pbit-ir's exact_mrv_declines_a_thrashing_search) is `refused` by `--op decide` (not proven),
    // and the reason names `--op exact`, which proves it infeasible (exit 1)
    let vars: Vec<String> = (0..9).map(|i| format!("{{\"id\": \"p{i}\"}}")).collect();
    let caps: Vec<String> = (0..800).map(|c| format!("{{\"limit\": 1, \"value\": \"h{}\"}}", c % 8)).collect();
    let vals: Vec<String> = (0..8).map(|h| format!("\"h{h}\"")).collect();
    let prog = format!("{{\"pbit_ir\": 1, \"values\": [{}], \"vars\": [{}], \"caps\": [{}]}}", vals.join(","), vars.join(","), caps.join(","));
    // Fixed work (--sweeps) keeps the work-only bound, so this refusal is deterministic (a 50 ms wall-clock budget now
    // gives the search 25 ms, and the proof takes ~35 ms on an M4: a race on a faster machine)
    let (c, out, err) = pbit(&["run", "--op", "decide", "--sweeps", "50"], &prog); assert_eq!(c, 3, "{err}");
    assert_eq!(field(&out, "verdict"), "\"refused\""); assert!(out.contains("--op exact"), "{out}");
    let (c, out, err) = pbit(&["run", "--op", "exact"], &prog); assert_eq!(c, 1, "{err}");
    assert_eq!(field(&out, "verdict"), "\"infeasible\"");
    // Under a wall-clock budget, when no chain can start, `decide` runs the exact search again past its gap budget to
    // the end of --budget-ms: the proof comes back. `--op sample` never enumerates: still refused
    let (c, out, err) = pbit(&["run", "--op", "decide", "--budget-ms", "3000"], &prog); assert_eq!(c, 1, "{err}");
    assert_eq!(field(&out, "verdict"), "\"infeasible\"");
    let (c, out, err) = pbit(&["run", "--op", "sample", "--budget-ms", "3000"], &prog); assert_eq!(c, 3, "{err}");
    assert_eq!(field(&out, "verdict"), "\"refused\"");
}
#[test]
fn a_closed_stdout_pipe_is_not_a_crash() {
    // `pbit demo --tasks 3000 | head -c 20` panicked ("failed printing to stdout: Broken pipe") and aborted with exit
    // 134; the output (> 64 KB) now stops quietly when the reader goes away, with the command's normal exit code
    use std::io::Read;
    let mut c = Command::new(env!("CARGO_BIN_EXE_pbit")).args(["demo", "--tasks", "3000"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    { let mut so = c.stdout.take().unwrap(); let mut buf = [0u8; 16]; so.read_exact(&mut buf).unwrap(); }
    let out = c.wait_with_output().unwrap(); let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{err}"); assert!(!err.contains("panicked"), "{err}");
}
#[test]
fn non_finite_budgets_are_bad_input() {
    // `--budget-ms inf` / `1e300` aborted (exit 134) and `NaN` hung `pbit run` and `pbit decide`; now exit 2 (bad input)
    let prog = r#"{"pbit_ir": 1, "values": ["-", "+"], "vars": [{"id": "a"}, {"id": "b"}]}"#;
    let (_, demo, _) = pbit(&["demo", "--tasks", "12"], "");
    for b in ["inf", "NaN", "1e300", "-5"] {
        let (c, _, err) = pbit(&["run", "--op", "sample", "--budget-ms", b], prog); assert_eq!(c, 2, "run {b}: {err}");
        let (c, _, err) = pbit(&["decide", "--budget-ms", b], &demo); assert_eq!(c, 2, "decide {b}: {err}");
    }
}
#[test]
fn exact_ms_reached_only_when_an_exact_tier_ran() {
    // `--op sample --exact-ms 0` / `--mode sample --exact-ms 0` said exact_ms_reached: true
    let prog = r#"{"pbit_ir": 1, "values": ["-", "+"], "vars": [{"id": "a"}, {"id": "b"}]}"#;
    let (_, demo, _) = pbit(&["demo", "--tasks", "12"], "");
    let (_, out, err) = pbit(&["run", "--op", "sample", "--sweeps", "200", "--polish-ms", "0", "--exact-ms", "0"], prog);
    assert!(out.contains("\"exact_ms_reached\":false"), "{out} {err}");
    let (_, out, err) = pbit(&["decide", "--mode", "sample", "--sweeps", "200", "--polish-ms", "0", "--exact-ms", "0"], &demo);
    assert!(out.contains("\"exact_ms_reached\":false"), "{out} {err}");
    let (_, out, err) = pbit(&["decide", "--sweeps", "200", "--polish-ms", "0", "--exact-ms", "0"], &demo);
    assert!(out.contains("\"exact_ms_reached\":true"), "auto mode, cap 0: {out} {err}");
}
#[test]
fn polish_ms_and_mode_are_bad_input() {
    // `--polish-ms inf` hung both commands forever, `-7` ran; `pbit decide --mode foo` (or `--mode --pretty`)
    // ran the sampler with exit 0; a bad `--polish-sweeps` failed only after the whole --budget-ms of sampling
    let prog = r#"{"pbit_ir": 1, "values": ["-", "+"], "vars": [{"id": "a"}, {"id": "b"}]}"#;
    let (_, demo, _) = pbit(&["demo", "--tasks", "12"], "");
    for b in ["inf", "NaN", "-7", "1e300"] {
        let (c, out, err) = pbit(&["run", "--op", "sample", "--budget-ms", "20", "--polish-ms", b], prog); assert_eq!(c, 2, "run {b}: {err}"); assert!(out.is_empty());
        let (c, out, err) = pbit(&["decide", "--mode", "sample", "--budget-ms", "20", "--polish-ms", b], &demo); assert_eq!(c, 2, "decide {b}: {err}"); assert!(out.is_empty());
    }
    for m in [&["--mode", "foo"][..], &["--mode", "EXACT"], &["--mode", "--pretty"]] {
        let (c, out, err) = pbit(&[&["decide"][..], m].concat(), &demo); assert_eq!(c, 2, "{m:?}: {err}"); assert!(out.is_empty());
    }
    let t = std::time::Instant::now();
    let (c, _, err) = pbit(&["decide", "--mode", "sample", "--budget-ms", "3000", "--polish-sweeps", "-1"], &demo);
    assert_eq!(c, 2, "{err}"); assert!(t.elapsed().as_secs_f64() < 2.0, "bad --polish-sweeps failed only after sampling");
    for good in [&["decide", "--mode", "sample", "--budget-ms", "20", "--polish-ms", "0"][..], &["decide", "--budget-ms", "20", "--polish-ms", "7.5"]] {
        let (c, _, err) = pbit(good, &demo); assert!(c == 0 || c == 3, "{good:?}: {err}");
    }
}
#[test]
fn unknown_flags_and_unparsable_values_are_bad_input() {
    // Both used to run on defaults silently (exit 0): a typo, a value that does not parse (`--sweeps 1e4` = wall-clock
    // mode, not the deterministic fixed-work mode asked for), and `"--budget-ms 50"` as ONE argument (an unsplit shell variable)
    let prog = r#"{"pbit_ir": 1, "values": ["a", "b"], "vars": [{"id": "x"}, {"id": "y"}], "caps": [{"limit": 1, "value": "a"}]}"#;
    for bad in [&["run", "--sweeps", "1e4"][..], &["run", "--budgetms", "5"], &["run", "--budget-ms 50"], &["run", "--seed", "x"], &["run", "--seed"],
                &["run", "--op"], &["decide", "--exact-limit", "-1"], &["decide", "--mode", "exact", "--oops"], &["stats", "--sweeps", "ten"], &["demo", "--tasks", "3", "--pretty"], &["ir", "--hard"], &["run", "--progress", "0"], &["run", "--progress", "1.5"]] {
        let (c, out, err) = pbit(bad, prog); assert_eq!(c, 2, "{bad:?}: {out} {err}"); assert!(out.is_empty(), "{bad:?}"); assert!(err.starts_with("pbit: "), "{bad:?}: {err}");
    }
    // A repeated value flag, a bad `pbit stats --priority`, and arguments after `pbit version` exited 0
    for bad in [&["decide", "--budget-ms", "50", "--budget-ms", "nan"][..], &["run", "--seed", "1", "--seed", "x"], &["run", "--chains", "2", "--chains", "2"],
                &["stats", "--priority", "bogus"], &["version", "--bogus"],
                // `--progress` (optional value) was still first-wins when repeated
                &["run", "--progress", "1000", "--progress", "10"], &["decide", "--progress", "--progress"]] {
        let (c, out, err) = pbit(bad, prog); assert_eq!(c, 2, "{bad:?}: {out} {err}"); assert!(out.is_empty(), "{bad:?}"); assert!(err.starts_with("pbit: "), "{bad:?}: {err}");
    }
    for good in [&["run", "--sweeps", "400", "--pretty"][..], &["run", "--progress", "--sweeps", "400"], &["run", "--sweeps", "400", "--progress", "50"], &["run", "--budget-ms", "1e2"]] {
        let (c, _, err) = pbit(good, prog); assert_eq!(c, 0, "{good:?}: {err}");
    }
}
#[test]
fn decide_fallback_enumerates_a_program_no_chain_can_start() {
    // Wall-clock-bound: the 4 s budget below is sized for a fast desktop (see the note that follows). Shared CI runners
    // overran it (macOS by 0.15 s, Windows by 5.6 s) and answered `refused`, so the test skips itself there; run it locally.
    if std::env::var_os("CI").is_some() { eprintln!("skipped: wall-clock-bound test; run it without the CI environment variable"); return; }
    // The fallback's `exact` document. A branches first (2 values); A = a0 leaves 9 pigeons for 8 holes whose caps are listed
    // 100x (a thrash past the gap budget, no plan), A = a1 frees hole h8 for pigeon 8: 8! plans. With 1024 chains (256 rounds of
    // 15.6 ms slices) each chain's start search gets ~7.8 ms (a0-first starts need > 18 ms on an M4), so some chain fails to start and
    // the rerun exact search enumerates the program inside the budget: `exact` at ~3.3 s on an M4, 0.68 s before the deadline (the
    // slices are wall-clock; only the ~0.26 s of exact search scales with the machine: ~3.6x slower still passes, inferred; 2000 ms
    // left ~1.9x). The answer must equal `--op exact` field for field. A sampler answer (a much faster machine starting every chain)
    // is still a valid decision, so that case only checks it is not refused / infeasible.
    let mut vars: Vec<String> = (0..9).map(|i| format!("{{\"id\": \"p{i}\", \"allowed\": [{}], \"h\": {{{}}}}}",
        (0..if i == 8 { 9 } else { 8 }).map(|v| format!("\"h{v}\"")).collect::<Vec<_>>().join(","),
        (0..9).map(|v| format!("\"h{v}\": {}", ((i * 11 + v) * 7 % 5) as f64 * 0.1)).collect::<Vec<_>>().join(","))).collect();
    vars.push("{\"id\": \"A\", \"allowed\": [\"a0\", \"a1\"]}".into());
    let mut caps: Vec<String> = (0..800).map(|c| format!("{{\"limit\": 1, \"value\": \"h{}\"}}", c % 8)).collect();
    caps.push("{\"limit\": 1, \"members\": [[\"p8\", \"h8\"], [\"A\", \"a0\"]]}".into());
    let vals: Vec<String> = (0..9).map(|h| format!("\"h{h}\"")).chain(["\"a0\"".to_string(), "\"a1\"".to_string()]).collect();
    let prog = format!("{{\"pbit_ir\": 1, \"values\": [{}], \"vars\": [{}], \"caps\": [{}]}}", vals.join(","), vars.join(","), caps.join(","));
    let (c, ex, err) = pbit(&["run", "--op", "exact"], &prog); assert_eq!(c, 0, "{err}"); assert_eq!(field(&ex, "n_feasible"), "40320");
    let (c, out, err) = pbit(&["run", "--op", "decide", "--budget-ms", "4000", "--chains", "1024"], &prog); assert_eq!(c, 0, "{err} {out}");
    if field(&out, "tier") == "\"enumerate\"" {
        for k in ["verdict", "n_feasible", "logz", "plan_logw"] { assert_eq!(field(&out, k), field(&ex, k), "{k}"); }
        let tail = |d: &str| d[d.find("\"plan\":").unwrap()..d.find("\"ms\":").unwrap()].to_string(); assert_eq!(tail(&out), tail(&ex));
    } else { assert_eq!(field(&out, "tier"), "\"sample\"", "{out}"); }
}

#[test]
fn exact_ms_caps_the_exact_tiers() {
    // --exact-ms N (opt-in) caps the exact tiers that run before the sampler. A far cap gives the same exact document;
    // 0 declines them at once: decide / auto sample (== --op/--mode sample at fixed work, plus two telemetry fields), exact modes decline.
    let strip = |o: &str| { let mut t = o.to_string(); for k in ["\"ms\":", "\"sample_ms\":", "\"gate_ms\":", "\"site_updates_per_s\":", "\"process_cpu_ms\":", "\"peak_rss_mb\":"] { while let Some(a) = t.find(k) { let b = a + t[a..].find(|ch| ch == ',' || ch == '}').unwrap(); t.replace_range(a..b, "_"); } } t };
    let cap0 = |o: &str| o.replace(",\"exact_ms\":0,\"exact_ms_reached\":true", "");
    let (c, demo, _) = pbit(&["demo", "--tasks", "12"], ""); assert_eq!(c, 0);
    let (_, a, _) = pbit(&["decide"], &demo); let (_, b, _) = pbit(&["decide", "--exact-ms", "100000"], &demo);
    assert_eq!(field(&a, "verdict"), "\"exact\""); assert_eq!(strip(&a), strip(&b));
    let fixed = ["--sweeps", "200", "--polish-sweeps", "100", "--threads", "2"];
    let (c, o, e) = pbit(&[&["decide", "--exact-ms", "0"][..], &fixed[..]].concat(), &demo); assert!(c == 0 || c == 3, "{e}");
    assert!(o.contains(",\"exact_ms\":0,\"exact_ms_reached\":true"), "{o}");
    let (_, s, _) = pbit(&[&["decide", "--mode", "sample"][..], &fixed[..]].concat(), &demo); assert_eq!(strip(&cap0(&o)), strip(&s));
    let (c, _, e) = pbit(&["decide", "--mode", "exact", "--exact-ms", "0"], &demo); assert_eq!(c, 2); assert!(e.contains("--exact-ms"), "{e}");
    // pbit run: a 12-spin frustrated ring (4,096 plans: enumerated)
    let vars: Vec<String> = (0..12).map(|i| format!("{{\"id\": \"s{i}\", \"h\": {{\"+\": {}}}}}", 0.1 * ((i % 7) as f64 - 3.0))).collect();
    let pairs: Vec<String> = (0..12).map(|i| format!("{{\"i\": \"s{i}\", \"j\": \"s{}\", \"table\": [[0.4, -0.4], [-0.4, 0.4]]}}", (i + 1) % 12)).collect();
    let ring = format!("{{\"pbit_ir\": 1, \"values\": [\"-\", \"+\"], \"vars\": [{}], \"pairs\": [{}]}}", vars.join(","), pairs.join(","));
    let (_, a, _) = pbit(&["run"], &ring); let (_, b, _) = pbit(&["run", "--exact-ms", "100000"], &ring);
    assert_eq!(field(&a, "verdict"), "\"exact\""); assert_eq!(strip(&a), strip(&b));
    let (c, o, _) = pbit(&["run", "--op", "exact", "--exact-ms", "0"], &ring); assert_eq!(c, 3); assert!(o.contains("stopped at --exact-ms"), "{o}");
    let (c, o, e) = pbit(&[&["run", "--op", "decide", "--exact-ms", "0"][..], &fixed[..]].concat(), &ring); assert!(c == 0 || c == 3, "{e}");
    let (_, s, _) = pbit(&[&["run", "--op", "sample"][..], &fixed[..]].concat(), &ring);
    assert_eq!(strip(&cap0(&o)).replace("\"op\":\"decide\"", "\"op\":\"sample\""), strip(&s));
    for bad in ["-1", "x", "inf", "NaN", "2e9"] { let (c, _, e) = pbit(&["run", "--exact-ms", bad], &ring); assert_eq!(c, 2, "{bad}: {e}"); }
}
