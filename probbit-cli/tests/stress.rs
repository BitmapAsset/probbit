//! R19 P0.3: the frozen stress corpus (tests/stress/, provenance in its README) against INDEPENDENT oracles. The oracles never
//! call probbit's inference: an occupancy count DP for one-group two-worker routers, brute-force enumeration for small programs,
//! and a transfer matrix for paths. `stress_oracles_agree` checks the oracles against each other and the external review's published number;
//! the per-family gate tests run the CLI at defaults for seeds 1..20 and require that no released item is outside the gate's
//! tolerance (0.05 TV) whenever the verdict releases anything. Families that fail today are #[ignore]d with the measured counts.
use std::process::{Command, Stdio};
use std::io::Write;
#[allow(dead_code)]
#[path = "../src/json.rs"]
mod json;
use json::Json;

fn load(name: &str) -> Json { json::parse(&std::fs::read_to_string(format!("{}/tests/stress/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap() }
fn f(j: &Json) -> f64 { j.as_f64().unwrap() }
fn s(j: &Json) -> &str { j.as_str().unwrap() }
fn choose2(k: usize) -> f64 { (k * k.saturating_sub(1) / 2) as f64 }

/// Router oracle: two workers, one group, caps that bind nothing. log w = sum_i score_i(x_i) + lam [C(k,2) + C(n-k,2)], k = #B.
/// P(task i on B) = r_i sum_k e^{(-i)}_{k-1} aff(k) / sum_k e_k aff(k), with r_i = exp(score_B - score_A) and e the elementary
/// symmetric polynomials of r (count DP). Returns P(B) per task, in task order.
fn count_dp(doc: &Json) -> Vec<f64> {
    let ws = doc.get("workers").unwrap().as_arr().unwrap(); assert_eq!(ws.len(), 2);
    let (wa, wb) = (s(ws[0].get("id").unwrap()), s(ws[1].get("id").unwrap()));
    let ts = doc.get("tasks").unwrap().as_arr().unwrap(); let n = ts.len();
    assert!(ws.iter().all(|w| f(w.get("cap").unwrap()) as usize >= n), "caps must not bind");
    assert!(ts.iter().all(|t| t.get("group").and_then(Json::as_str) == ts[0].get("group").and_then(Json::as_str)), "one group");
    let lam = doc.get("affinity").map_or(0.0, f);
    let r: Vec<f64> = ts.iter().map(|t| { let sc = t.get("scores").unwrap(); (f(sc.get(wb).unwrap()) - f(sc.get(wa).unwrap())).exp() }).collect();
    let aff: Vec<f64> = (0..=n).map(|k| (lam * (choose2(k) + choose2(n - k))).exp()).collect();
    let esp = |skip: Option<usize>| { let mut e = vec![0.0; n + 1]; e[0] = 1.0;
        for (j, &rj) in r.iter().enumerate() { if Some(j) == skip { continue; } for k in (1..=n).rev() { e[k] += rj * e[k - 1]; } } e };
    let all = esp(None); let z: f64 = (0..=n).map(|k| all[k] * aff[k]).sum();
    (0..n).map(|i| { let e = esp(Some(i)); r[i] * (1..=n).map(|k| e[k - 1] * aff[k]).sum::<f64>() / z }).collect()
}
/// Closed form for identical tasks (the external review, §2): sum_k C(n,k) exp(lam [C(k,2)+C(n-k,2)] + h k).
fn occupancy_closed_form(n: usize, lam: f64, h: f64) -> f64 {
    let c = |k: usize| (0..k).fold(1.0, |a, i| a * (n - i) as f64 / (i + 1) as f64);
    let w = |k: usize| c(k) * (lam * (choose2(k) + choose2(n - k)) + h * k as f64).exp();
    (0..=n).map(|k| w(k) * k as f64 / n as f64).sum::<f64>() / (0..=n).map(w).sum::<f64>()
}
/// A probbit-ir program read independently: (ids, values, h[n*k], allowed[n*k], pairs (i, j, table k*k), caps (members, limit)).
struct Ir { ids: Vec<String>, vals: Vec<String>, k: usize, h: Vec<f64>, allowed: Vec<bool>, pairs: Vec<(usize, usize, Vec<f64>)>, caps: Vec<(Vec<(usize, usize)>, usize)> }
fn ir(doc: &Json) -> Ir {
    let vals: Vec<String> = doc.get("values").unwrap().as_arr().unwrap().iter().map(|v| s(v).to_string()).collect(); let k = vals.len();
    let vi = |x: &str| vals.iter().position(|v| v == x).unwrap();
    let vs = doc.get("vars").unwrap().as_arr().unwrap(); let n = vs.len();
    let ids: Vec<String> = vs.iter().map(|v| s(v.get("id").unwrap()).to_string()).collect(); let xi = |x: &str| ids.iter().position(|v| v == x).unwrap();
    let (mut h, mut allowed) = (vec![0.0; n * k], vec![true; n * k]);
    for (i, v) in vs.iter().enumerate() {
        if let Some(hh) = v.get("h") { for (val, x) in hh.as_obj().unwrap() { h[i * k + vi(val)] = f(x); } }
        assert!(v.get("allowed").is_none() && v.get("forbid").is_none() && v.get("clamp").is_none(), "oracle reads h / pairs / caps only");
    }
    let pairs = doc.get("pairs").map_or(vec![], |ps| ps.as_arr().unwrap().iter().map(|p| { let (i, j) = (xi(s(p.get("i").unwrap())), xi(s(p.get("j").unwrap())));
        let t = if let Some(w) = p.get("potts") { (0..k * k).map(|q| if q / k == q % k { f(w) } else { 0.0 }).collect() } else { p.get("table").unwrap().as_arr().unwrap().iter().flat_map(|r| r.as_arr().unwrap().iter().map(f)).collect() };
        (i, j, t) }).collect());
    let caps = doc.get("caps").map_or(vec![], |cs| cs.as_arr().unwrap().iter().map(|c| { let q = vi(s(c.get("value").unwrap())); assert!(c.get("vars").is_none());
        ((0..n).filter(|&i| allowed[i * k + q]).map(|i| (i, q)).collect(), f(c.get("limit").unwrap()) as usize) }).collect());
    allowed.iter_mut().for_each(|a| *a = true);
    Ir { ids, vals, k, h, allowed, pairs, caps }
}
/// Brute force over k^n assignments (log space): marginals [n*k].
fn brute(p: &Ir) -> Vec<f64> {
    let (n, k) = (p.ids.len(), p.k); assert!((k as f64).powi(n as i32) <= 1e7);
    let mut lws = vec![]; let mut x = vec![0usize; n];
    loop {
        if (0..n).all(|i| p.allowed[i * k + x[i]]) && p.caps.iter().all(|(m, lim)| m.iter().filter(|&&(i, q)| x[i] == q).count() <= *lim) {
            let lw = (0..n).map(|i| p.h[i * k + x[i]]).sum::<f64>() + p.pairs.iter().map(|(i, j, t)| t[x[*i] * k + x[*j]]).sum::<f64>(); lws.push((lw, x.clone())); }
        let mut i = 0; while i < n { x[i] += 1; if x[i] < k { break; } x[i] = 0; i += 1; } if i == n { break; }
    }
    let mx = lws.iter().map(|e| e.0).fold(f64::NEG_INFINITY, f64::max); let z: f64 = lws.iter().map(|e| (e.0 - mx).exp()).sum();
    let mut m = vec![0.0; n * k]; for (lw, x) in &lws { let w = (lw - mx).exp() / z; for i in 0..n { m[i * k + x[i]] += w; } } m
}
/// Transfer matrix on a path x0 - x1 - ... (pairs (i, i+1) only, no caps): forward-backward in log space, marginals [n*k].
fn chain_tm(p: &Ir) -> Vec<f64> {
    let (n, k) = (p.ids.len(), p.k); assert!(p.caps.is_empty() && p.pairs.len() == n - 1 && p.pairs.iter().enumerate().all(|(q, e)| e.0 == q && e.1 == q + 1));
    let lse = |v: &[f64]| { let m = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max); m + v.iter().map(|x| (x - m).exp()).sum::<f64>().ln() };
    let mut fw = vec![vec![0.0; k]; n]; let mut bw = vec![vec![0.0; k]; n];
    for a in 0..k { fw[0][a] = p.h[a]; }
    for i in 1..n { for b in 0..k { fw[i][b] = p.h[i * k + b] + lse(&(0..k).map(|a| fw[i - 1][a] + p.pairs[i - 1].2[a * k + b]).collect::<Vec<_>>()); } }
    for i in (0..n - 1).rev() { for a in 0..k { bw[i][a] = lse(&(0..k).map(|b| p.pairs[i].2[a * k + b] + p.h[(i + 1) * k + b] + bw[i + 1][b]).collect::<Vec<_>>()); } }
    let mut m = vec![0.0; n * k];
    for i in 0..n { let l: Vec<f64> = (0..k).map(|a| fw[i][a] + bw[i][a]).collect(); let z = lse(&l); for a in 0..k { m[i * k + a] = (l[a] - z).exp(); } } m
}

#[test]
fn stress_oracles_agree() {
    for (file, h) in [("asymmetric-router32-w0.2-h0.03.json", 0.03), ("asymmetric-router32-w0.2-h0.06.json", 0.06)] {
        let cf = occupancy_closed_form(32, 0.2, h); for pb in count_dp(&load(file)) { assert!((pb - cf).abs() < 1e-12, "{file}: {pb} vs {cf}"); }
    }
    assert!((occupancy_closed_form(32, 0.2, 0.06) - 0.8698133938587349).abs() < 1e-12, "the external review's published P(B)");
    // count DP vs brute force on the first 12 heterogeneous tasks (2^12 assignments)
    let het = load("heterogeneous-router32.json"); let ts = het.get("tasks").unwrap().as_arr().unwrap();
    let sub = json::obj(vec![("workers", json::parse(r#"[{"id":"A","cap":12},{"id":"B","cap":12}]"#).unwrap()), ("tasks", Json::Arr(ts[..12].to_vec())), ("affinity", json::num(0.2))]);
    let dp = count_dp(&sub);
    let p = Ir { ids: (0..12).map(|i| format!("t{i}")).collect(), vals: vec!["A".into(), "B".into()], k: 2,
        h: ts[..12].iter().flat_map(|t| { let sc = t.get("scores").unwrap(); [f(sc.get("A").unwrap()), f(sc.get("B").unwrap())] }).collect(), allowed: vec![true; 24],
        pairs: (0..12).flat_map(|i| (i + 1..12).map(move |j| (i, j, vec![0.2, 0.0, 0.0, 0.2]))).collect(), caps: vec![] };
    let bf = brute(&p); for i in 0..12 { assert!((dp[i] - bf[i * 2 + 1]).abs() < 1e-12, "task {i}: {} vs {}", dp[i], bf[i * 2 + 1]); }
    // transfer matrix vs brute force on the first 12 variables of chain100
    let mut c = ir(&load("chain100.json")); c.ids.truncate(12); c.h.truncate(24); c.allowed.truncate(24); c.pairs.truncate(11);
    let (tm, bf) = (chain_tm(&c), brute(&c)); for q in 0..24 { assert!((tm[q] - bf[q]).abs() < 1e-12, "chain {q}: {} vs {}", tm[q], bf[q]); }
    // ferro12 (with and without redundant caps): symmetric under 0 <-> 1, so every marginal is 1/2
    for file in ["ferro12.json", "ferro12-w0.5-redundantcaps.json", "ferro12-w1.0-redundantcaps.json"] {
        for x in brute(&ir(&load(file))) { assert!((x - 0.5).abs() < 1e-12, "{file}: {x}"); } }
}

/// Per family: run each seed, compare every released item with the oracle. Returns (whole answers released, false whole
/// answers, released items, wrong released items, verdict tally).
fn family(file: &str, args: &[&str], truth: &dyn Fn(&str) -> Vec<(String, f64)>, seeds: std::ops::RangeInclusive<u64>) -> (usize, usize, usize, usize, std::collections::BTreeMap<String, usize>) {
    let input = std::fs::read_to_string(format!("{}/tests/stress/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let router = args[0] == "decide"; let seeds: Vec<u64> = seeds.collect();
    let outs: Vec<String> = std::thread::scope(|sc| { let hs: Vec<_> = seeds.chunks(5).map(|ch| { let input = &input; sc.spawn(move || ch.iter().map(|sd| {
        let sd = sd.to_string(); let mut a: Vec<&str> = args.to_vec(); a.extend(["--seed", &sd]);
        let mut c = Command::new(env!("CARGO_BIN_EXE_probbit")).args(&a).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap(); String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap() }).collect::<Vec<_>>()) }).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect() });
    let (mut whole, mut false_whole, mut rel, mut wrong) = (0, 0, 0, 0); let mut tally = std::collections::BTreeMap::new();
    for out in &outs {
        let d = json::parse(out).unwrap_or_else(|e| panic!("{file}: {e:?}: {out:.300}")); let v = s(d.get("verdict").unwrap()).to_string();
        *tally.entry(v.clone()).or_insert(0) += 1;
        // R19.4: exact answers (the inference compiler's tiers now answer the router families at defaults) are checked too
        if !["diagnostics_passed", "exact", "partial"].contains(&v.as_str()) { continue; }
        let est = d.get(if router { "odds" } else { "marginals" }).unwrap();
        let mut bad = 0; let released = d.get("released").unwrap().as_arr().unwrap();
        for id in released { let id = s(id); let row = est.get(id).unwrap();
            let tv = 0.5 * truth(id).iter().map(|(val, p)| (row.get(val).map_or(0.0, f) - p).abs()).sum::<f64>(); if tv > 0.05 { bad += 1; } }
        rel += released.len(); wrong += bad; if v != "partial" { whole += 1; if bad > 0 { false_whole += 1; } }
    }
    eprintln!("{file} {args:?} seeds {}..{}: whole {whole} (false {false_whole}), released {rel} (wrong {wrong}), verdicts {tally:?}", seeds[0], seeds[seeds.len() - 1]);
    (whole, false_whole, rel, wrong, tally)
}
fn router_truth(file: &str) -> impl Fn(&str) -> Vec<(String, f64)> {
    let d = load(file); let pb = count_dp(&d); let ids: Vec<String> = d.get("tasks").unwrap().as_arr().unwrap().iter().map(|t| s(t.get("id").unwrap()).to_string()).collect();
    move |id: &str| { let i = ids.iter().position(|x| x == id).unwrap(); vec![("A".into(), 1.0 - pb[i]), ("B".into(), pb[i])] }
}
fn ir_truth(file: &str, chain: bool) -> impl Fn(&str) -> Vec<(String, f64)> {
    let p = ir(&load(file)); let m = if chain { chain_tm(&p) } else { brute(&p) };
    move |id: &str| { let i = p.ids.iter().position(|x| x == id).unwrap(); (0..p.k).map(|q| (p.vals[q].clone(), m[i * p.k + q])).collect() }
}
fn assert_clean(r: (usize, usize, usize, usize, std::collections::BTreeMap<String, usize>)) { assert!(r.1 == 0 && r.3 == 0, "false whole answers {}, wrong released items {} (of {})", r.1, r.3, r.2); }
/// Defaults (wall clock: 200 ms budget, 50 ms polish) and the external review's fixed work (`--sweeps 4000 --polish-ms 0`, deterministic).
/// Router families (`decide`) also run with `--mode sample`: since R19.4 their defaults are answered exactly (occupancy-count
/// DP), so the sampler + gate path is exercised explicitly; every exact answer must also match the oracle (and does: 20/20).
fn check(files: &[&str], base: &[&str], truth: &dyn Fn(&str, &str) -> Vec<(String, f64)>) {
    let fixed = [base, &["--sweeps", "4000", "--polish-ms", "0"][..]].concat();
    let mut runs: Vec<Vec<&str>> = vec![base.to_vec(), fixed.clone()];
    if base[0] == "decide" { runs.push([base, &["--mode", "sample"][..]].concat()); runs.push([&fixed[..], &["--mode", "sample"][..]].concat()); }
    let rs: Vec<_> = files.iter().flat_map(|f| runs.iter().map(|a| family(f, a, &|id| truth(f, id), 1..=20)).collect::<Vec<_>>()).collect();
    for r in rs { assert_clean(r); }
}

// Measured on the R19.1 build (Apple M4, load 7-9), seeds 1..20, at defaults (200 ms budget) and at the external review's fixed work
// (`--sweeps 4000 --polish-ms 0`): every whole answer released was false; every released item was wrong (RED counts below).
// R19.2 (P1.1(a), the global two-value flip, `--collective on` = the default): every family 20/20 diagnostics_passed, 0 false,
// 0 wrong released items, at defaults and at fixed work (load ~4). `--collective off` reproduces the RED counts.
/// RED (R19.1): defaults 2/20 whole answers, both false, 64/64 released wrong (18 refused); fixed work: the same 2/20, 64/64.
/// GREEN (R19.2, flip on): 20/20 whole answers, 640 released, 0 wrong, defaults and fixed work.
#[test]
fn stress_asymmetric_router_h003() { check(&["asymmetric-router32-w0.2-h0.03.json"], &["decide"], &|f, id| router_truth(f)(id)); }
/// RED (R19.1): defaults 3/20 whole answers, all false, 96/96 released wrong (17 refused); fixed work: the same 3/20, 96/96.
/// External review (seeds 1..100, defaults): 14/100, all false. GREEN (R19.2, flip on): 20/20, 640 released, 0 wrong.
#[test]
fn stress_asymmetric_router_h006() { check(&["asymmetric-router32-w0.2-h0.06.json"], &["decide"], &|f, id| router_truth(f)(id)); }
/// RED (R19.1): defaults 2/20 whole answers, both false, 64/64 released wrong (18 refused); fixed work: the same. External review: 2/16.
/// GREEN (R19.2, flip on): 20/20, 640 released, 0 wrong.
#[test]
fn stress_heterogeneous_router() { check(&["heterogeneous-router32.json"], &["decide"], &|f, id| router_truth(f)(id)); }
/// RED (R19.1): `--op sample` defaults 3/20 whole answers, all false, 36/36 released wrong (17 refused); fixed work: the same.
/// GREEN (R19.2, flip on): 20/20, 240 released, 0 wrong.
#[test]
fn stress_ferro12() { check(&["ferro12.json"], &["run", "--op", "sample"], &|f, id| ir_truth(f, false)(id)); }
/// RED (R19.1): `--op sample`, w0.5: defaults 20/20 refused (clean), fixed work 3/20 false whole answers (36 wrong);
/// w1.0: defaults 2/20 false whole answers (24 wrong), fixed work 20/20 refused (clean).
/// GREEN (R19.2, flip on): both weights 20/20, 240 released, 0 wrong, defaults and fixed work.
#[test]
fn stress_ferro12_redundant_caps() { check(&["ferro12-w0.5-redundantcaps.json", "ferro12-w1.0-redundantcaps.json"], &["run", "--op", "sample"], &|f, id| ir_truth(f, false)(id)); }
/// GREEN (R19.1): `--op sample` 20/20 whole answers, 2,000 released, 0 wrong, at defaults and at fixed work: a regression guard.
#[test]
fn stress_chain100() { check(&["chain100.json"], &["run", "--op", "sample"], &|f, id| ir_truth(f, true)(id)); }
/// R19.2: every move on (`--cluster on --cycles on` with the default `--collective on`). Before the cluster move became a
/// probability-1/2 mixture, it and the global flip each flipped ferro12 whole every sweep and cancelled: fixed work, seeds
/// 1..10, 8 refused and 2 FALSE whole answers (max error 0.5). After: 20/20 diagnostics_passed, 0 false, on both inputs.
#[test]
fn stress_ferro12_all_moves() { check(&["ferro12.json", "ferro12-w1.0-redundantcaps.json"], &["run", "--op", "sample", "--cluster", "on", "--cycles", "on"], &|f, id| ir_truth(f, false)(id)); }
/// R19 P1.2 measurement (not a pass/fail test): the gate ALONE, collective moves off, the external review's fixed work, seeds 1..20, on the
/// families the moves fixed. Prints whole / false / released / wrong per family. Run: `cargo test --release -p probbit-cli --test
/// stress -- --ignored --nocapture gate_alone`. Counts are in BENCHMARKS "Known failure modes" (R19.3: gate/2 vs gate/3).
#[test]
#[ignore = "measurement: prints the gate-alone counts with the moves off; RED by design (assumption 1)"]
fn stress_gate_alone_moves_off() {
    // R19.9: every collective move off (`--cluster` / `--cycles` became default-on in R19.4 / R19.5, and the router inputs are
    // answered exactly at defaults since R19.4, hence `--mode sample`): with `--collective off` alone this measured the moves
    let off = ["--collective", "off", "--cluster", "off", "--cycles", "off", "--sweeps", "4000", "--polish-ms", "0"];
    for f in ["asymmetric-router32-w0.2-h0.03.json", "asymmetric-router32-w0.2-h0.06.json", "heterogeneous-router32.json"] {
        family(f, &[&["decide", "--mode", "sample"][..], &off[..]].concat(), &|id| router_truth(f)(id), 1..=20); }
    for f in ["ferro12.json", "ferro12-w0.5-redundantcaps.json", "ferro12-w1.0-redundantcaps.json"] {
        family(f, &[&["run", "--op", "sample"][..], &off[..]].concat(), &|id| ir_truth(f, false)(id), 1..=20); }
}

/// R19 P1.3(c): at defaults (`probbit run`, op decide) chain100 is answered EXACTLY by the forest tier (sum-product), not
/// sampled: every odds value agrees with the independent transfer matrix within 1e-6 (the CLI prints 6 decimals; the IR unit
/// test `components_and_forest_match_enumeration` checks 1e-9), and `--op exact` gives the same log Z.
/// (R19.4 measurement, Apple M4, load 3.6-8.5, N = 5: chain100 sampled 281.9 ms -> forest 0.055 ms; chain1000 281.3 -> 0.390 ms.)
#[test]
fn stress_chain100_forest_tier() {
    let input = std::fs::read_to_string(format!("{}/tests/stress/chain100.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let run = |args: &[&str]| { let mut c = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap(); let o = c.wait_with_output().unwrap(); (o.status.code(), json::parse(&String::from_utf8(o.stdout).unwrap()).unwrap()) };
    let p = ir(&load("chain100.json")); let tm = chain_tm(&p);
    let (code, d) = run(&["run"]); assert_eq!(code, Some(0));
    assert_eq!(s(d.get("verdict").unwrap()), "exact"); assert_eq!(s(d.get("tier").unwrap()), "forest");
    assert_eq!(f(d.get("components").unwrap().get("forest").unwrap()), 1.0);
    let mg = d.get("marginals").unwrap();
    for (i, id) in p.ids.iter().enumerate() { for q in 0..p.k { let got = mg.get(id).unwrap().get(&p.vals[q]).map_or(0.0, f);
        assert!((got - tm[i * p.k + q]).abs() <= 5.1e-7, "{id}={}: forest {got} vs transfer matrix {}", p.vals[q], tm[i * p.k + q]); } }
    let (code, e) = run(&["run", "--op", "exact"]); assert_eq!(code, Some(0)); assert_eq!(s(e.get("tier").unwrap()), "forest");
    assert_eq!(f(e.get("logz").unwrap()), f(d.get("logz").unwrap()));
}

/// R19 P1.3(d): at defaults `probbit decide` answers the external review's router families EXACTLY by the occupancy-count DP (one two-worker
/// group, uniform affinity, whole-worker caps): every P(B) agrees with the independent elementary-symmetric-polynomial oracle
/// within print rounding. (R19.4, Apple M4, load 8-18, N = 5: 426-429 ms sampled -> 0.086-0.092 ms exact.)
#[test]
fn stress_router_occupancy_tier() {
    for file in ["asymmetric-router32-w0.2-h0.03.json", "asymmetric-router32-w0.2-h0.06.json", "heterogeneous-router32.json"] {
        let input = std::fs::read_to_string(format!("{}/tests/stress/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_probbit")).args(["decide"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap(); let o = c.wait_with_output().unwrap(); assert_eq!(o.status.code(), Some(0));
        let d = json::parse(&String::from_utf8(o.stdout).unwrap()).unwrap();
        assert_eq!(s(d.get("verdict").unwrap()), "exact"); assert_eq!(s(d.get("tier").unwrap()), "occupancy", "{file}");
        let doc = load(file); let pb = count_dp(&doc); let odds = d.get("odds").unwrap();
        for (i, t) in doc.get("tasks").unwrap().as_arr().unwrap().iter().enumerate() { let id = s(t.get("id").unwrap());
            let got = odds.get(id).unwrap().get("B").map_or(0.0, f); assert!((got - pb[i]).abs() <= 5.1e-7, "{file} {id}: {got} vs oracle {}", pb[i]); }
    }
}

/// R19.6 c12 FINDING (false refusal, never wrong): calibrate.py holdout seed 20261009, saturated-k3 program #3. Four variables
/// allow {a, c}, eight allow {b, c}, every value capped at 4 with n = 12, so the four {a, c} variables are FORCED to a by counting
/// (70 plans). The chains mix (rhat 1.000005, tv_bound 0.0024, 170K label swaps) but `release_reason` is `frozen` for all 12
/// items: the frozen rule reads a cap saturated by members forced by counting as a mixing failure (FORCED_CAPS_PARTITION covers
/// clamps / single-allowed values only). Refused at 4/4 seeds at load 10.5 and 3.7. FIXED in R19.7 (`probbit_ir::forced_values`:
/// a variable every chain held at one value is tested with that value forbidden; the capacitated matching proves no plan is
/// left, so it is a constant, not a frozen member). Un-ignored.
#[test]
fn stress_saturated_forced_by_counting_is_not_frozen() {
    let prog = r#"{"probbit_ir":1,"values":["a","b","c"],"vars":[{"id":"x0","h":{"b":-0.4725,"c":-0.3631},"allowed":["b","c"]},{"id":"x1","h":{"b":-0.1105,"c":-0.2823},"allowed":["b","c"]},{"id":"x2","h":{"b":0.3948,"c":-0.2015},"allowed":["b","c"]},{"id":"x3","h":{"a":0.0444,"c":-0.2414},"allowed":["a","c"]},{"id":"x4","h":{"b":0.0723,"c":-0.0482},"allowed":["b","c"]},{"id":"x5","h":{"b":-0.2338,"c":0.3248},"allowed":["b","c"]},{"id":"x6","h":{"a":0.1801,"c":0.3416},"allowed":["a","c"]},{"id":"x7","h":{"b":-0.3542,"c":0.4688},"allowed":["b","c"]},{"id":"x8","h":{"a":0.2333,"c":0.0227},"allowed":["a","c"]},{"id":"x9","h":{"b":-0.2588,"c":0.0981},"allowed":["b","c"]},{"id":"x10","h":{"b":0.3718,"c":-0.3839},"allowed":["b","c"]},{"id":"x11","h":{"a":0.2714,"c":0.1566},"allowed":["a","c"]}],"pairs":[{"i":"x0","j":"x11","table":[[0.244,-0.023,0.057],[0.06,-0.271,0.291],[-0.163,0.185,-0.03]]},{"i":"x1","j":"x4","table":[[0.225,0.178,-0.109],[-0.362,-0.031,0.007],[0.069,0.384,0.372]]},{"i":"x2","j":"x3","table":[[-0.221,-0.01,-0.244],[0.262,0.063,-0.051],[-0.005,0.072,0.283]]},{"i":"x3","j":"x7","table":[[0.297,0.202,0.201],[0.382,0.27,-0.09],[0.378,-0.088,0.366]]},{"i":"x3","j":"x9","table":[[-0.282,-0.191,-0.113],[0.09,-0.132,-0.013],[0.104,-0.211,0.231]]},{"i":"x5","j":"x7","table":[[-0.101,0.01,0.348],[-0.335,-0.374,-0.196],[-0.281,0.328,-0.165]]},{"i":"x5","j":"x10","table":[[0.225,-0.363,0.182],[-0.202,-0.242,0.241],[0.354,0.037,0.012]]},{"i":"x7","j":"x11","table":[[0.188,-0.06,-0.141],[0.02,-0.251,-0.048],[-0.18,-0.34,0.048]]}],"caps":[{"value":"a","limit":4},{"value":"b","limit":4},{"value":"c","limit":4}]}"#;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_probbit")).args(["run", "--op", "sample", "--sweeps", "4000", "--seed", "1"])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().and_then(|mut c| { use std::io::Write; c.stdin.take().unwrap().write_all(prog.as_bytes())?; c.wait_with_output() }).unwrap();
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(!s.contains("\"verdict\":\"refused\""), "{s}");
}
