//! BENCHMARKS §2: the router, pbit vs greedy+repair vs exact, on three families.
//! A. exact-oracle chain-of-blocks queues (`oracle::build`, T = 80/200; exact odds + exact optimum by transfer-matrix DP).
//! B. `pbit demo --tasks N` queues at N = 12/18/24/30 (same generator as the CLI; exact odds + optimum from the IR frontier tier).
//! C. `pbit demo --tasks 300` queues (no oracle: plan log w, and agreement with a 3 s sampler reference).
//! Methods: greedy = `feasible_init` (best-logit-first matching + augmenting-path repair); greedy+polish = the same anneal
//! the sampler's plan gets, for the sampler's wall time (EQUAL time); pbit = what `pbit decide --mode sample` runs (4 chains,
//! BUD ms, the shipped gate, then POL ms polish of the best plan); exact = the IR frontier tier (what `pbit decide` runs first).
//! Released = tasks the gate certifies; wrong = released tasks whose odds are > 0.05 TV from the exact odds.
//! env: NINST (A instances, default 16), SEEDS (B/C seeds, default 4), BUD (200), POL (50), REF_MS (3000), ONLY=A|B|C.
use pbit_core::Philox4x32;
use pbit_decide::oracle::*;
use pbit_decide::*;
use std::time::Instant;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn argmax_rows(m: &[f64], t: usize, a: usize) -> Vec<usize> {
    (0..t).map(|i| (0..a).max_by(|&u, &v| m[i * a + u].partial_cmp(&m[i * a + v]).unwrap()).unwrap()).collect()
}
fn agree(x: &[usize], y: &[usize]) -> f64 { x.iter().zip(y).filter(|(a, b)| a == b).count() as f64 / x.len() as f64 }
fn tvs(m: &[f64], e: &[f64], t: usize, a: usize) -> Vec<f64> { (0..t).map(|i| 0.5 * (0..a).map(|k| (m[i * a + k] - e[i * a + k]).abs()).sum::<f64>()).collect() }
fn ms(t0: Instant) -> f64 { t0.elapsed().as_secs_f64() * 1e3 }
/// copy of the CLI `pbit demo` generator (pbit-cli/src/main.rs demo_cmd): scores rounded to 0.01, quotas scaled with N
fn demo(n: usize, seed: u64, hard: bool) -> Problem {
    const W: [usize; 6] = [2, 2, 3, 2, 2, 3];
    const TPL: [(bool, bool, [f64; 6]); 8] = [(true, false, [2.0, 1.6, 1.0, 0.6, -1.0, 1.2]), (true, false, [2.2, 1.8, 0.8, 0.9, -1.0, 0.5]),
        (false, false, [1.6, 1.2, 0.0, -0.5, 2.0, -1.0]), (false, true, [1.8, 1.0, 1.2, 0.0, 1.5, 1.0]), (false, false, [0.5, 1.0, 1.6, 0.5, -1.0, -1.5]),
        (false, false, [0.3, 0.8, 1.5, 1.0, -1.0, 0.0]), (false, false, [1.0, 0.8, 0.2, -0.5, 1.8, -2.0]), (false, false, [1.8, 1.4, 0.6, 0.2, -1.0, 0.8])];
    let mut r = Philox4x32::new(seed, 4242);
    #[allow(clippy::approx_constant)] // 6.283185307, not TAU: the published benchmark instances depend on these exact bits
    let mut g = || { let u = r.f64() + 1e-12; let v = r.f64(); (-2.0 * u.ln()).sqrt() * (6.283185307 * v).cos() };
    let scale = (n as f64 / 12.0).max(1.0) * if hard { 0.89 } else { 1.0 };
    let a = 6; let (mut h, mut allowed) = (vec![0.0; n * a], vec![true; n * a]);
    for i in 0..n { let (pii, prod, fit) = TPL[i % 8];
        for w in 0..a { h[i * a + w] = ((fit[w] + 0.7 * g()) * 100.0).round() / 100.0;
            if pii && w != 3 && w != 5 { allowed[i * a + w] = false; } if prod && (w == 2 || w == 3) { allowed[i * a + w] = false; } } }
    let cap = W.iter().map(|&c| (c as f64 * scale).ceil().max(1.0) as usize).collect();
    let lam = if hard { 2.5 } else if n <= 12 { 0.8 } else { 1.2 };
    Problem { t: n, a, h, allowed, cap, group: (0..n).map(|i| i / 3).collect(), lam, clamp: vec![None; n], block_moves: false, pair_swaps: false, collective: false, cluster: false, cycles: false }
}
#[derive(Default)]
struct Agg { ms: Vec<f64>, gap: Vec<f64>, top1: Vec<f64>, maxtv: Vec<f64>, cert: usize, rel: usize, bad: usize, runs: usize, viol: usize, t: usize }
fn q(v: &[f64], p: f64) -> f64 { if v.is_empty() { return f64::NAN; } let mut s = v.to_vec(); s.sort_by(|a, b| a.partial_cmp(b).unwrap()); s[((s.len() - 1) as f64 * p).round() as usize] }
fn mean(v: &[f64]) -> f64 { v.iter().sum::<f64>() / v.len().max(1) as f64 }
/// one instance with an exact oracle (marginals `em`, optimum `opt`): every method, one CSV row each
fn with_oracle(fam: &str, id: u64, p: &Problem, em: &[f64], opt: f64, bud: f64, pol: f64, agg: &mut [Agg; 4]) {
    let etop = argmax_rows(em, p.t, p.a);
    let t0 = Instant::now(); let f = exact_frontier(p, FRONTIER_MAX_STATES); let fms = ms(t0);
    if let Some(f) = &f { let e = tvs(&f.marg, em, p.t, p.a).into_iter().fold(0.0, f64::max);
        println!("{fam},{id},{},exact_frontier,{fms:.3},{},{:.4},{:.4},{e:.2e},,,", p.t, p.violations(&f.map), opt - f.map_logw, agree(&f.map, &etop));
        let a = &mut agg[0]; a.ms.push(fms); a.gap.push(opt - f.map_logw); a.top1.push(agree(&argmax_rows(&f.marg, p.t, p.a), &etop)); a.maxtv.push(e); a.runs += 1; a.t += p.t; }
    else { println!("{fam},{id},{},exact_frontier,{fms:.3},declined,,,,,,", p.t); }
    let t0 = Instant::now(); let gx = p.feasible_init().unwrap(); let gms = ms(t0);
    println!("{fam},{id},{},greedy,{gms:.3},{},{:.4},{:.4},,,,", p.t, p.violations(&gx), opt - p.logw(&gx), agree(&gx, &etop));
    let a = &mut agg[1]; a.ms.push(gms); a.gap.push(opt - p.logw(&gx)); a.top1.push(agree(&gx, &etop)); a.viol += p.violations(&gx); a.runs += 1; a.t += p.t;
    // pbit: forced sampler (exact_limit 0) = `pbit decide --mode sample`, then the CLI's polish of the best plan
    let t0 = Instant::now(); let (d, g) = decide_gated(p, 0, 0, Some(bud), 1 + id, &GATE).unwrap(); let g = g.unwrap();
    let pp = polish_plan(p, Some(&d.map), pol, 1 + id).unwrap(); let pms = ms(t0);
    let tv = tvs(&d.marg, em, p.t, p.a); let rel = g.released_tasks(&GATE); let nrel = rel.iter().filter(|&&r| r).count();
    let nbad = rel.iter().zip(&tv).filter(|(&r, &v)| r && v > 0.05).count(); let mtv = tv.iter().cloned().fold(0.0, f64::max);
    let mtop = agree(&argmax_rows(&d.marg, p.t, p.a), &etop);
    println!("{fam},{id},{},pbit,{pms:.1},{},{:.4},{mtop:.4},{mtv:.4},{},{nrel},{nbad}", p.t, p.violations(&pp.1), opt - pp.0, g.diagnostics_passed(&GATE) as u8);
    let a = &mut agg[3]; a.ms.push(pms); a.gap.push(opt - pp.0); a.top1.push(mtop); a.maxtv.push(mtv); a.cert += g.diagnostics_passed(&GATE) as usize; a.rel += nrel; a.bad += nbad; a.viol += p.violations(&pp.1); a.runs += 1; a.t += p.t;
    // greedy + polish for the SAME wall time the pbit path used
    let t0 = Instant::now(); let gx = p.feasible_init().unwrap(); let gp = polish_plan(p, Some(&gx), (pms - ms(t0)).max(1.0), 1 + id).unwrap(); let gpms = ms(t0);
    println!("{fam},{id},{},greedy+polish,{gpms:.1},{},{:.4},{:.4},,,,", p.t, p.violations(&gp.1), opt - gp.0, agree(&gp.1, &etop));
    let a = &mut agg[2]; a.ms.push(gpms); a.gap.push(opt - gp.0); a.top1.push(agree(&gp.1, &etop)); a.viol += p.violations(&gp.1); a.runs += 1; a.t += p.t;
}
fn summary(label: &str, agg: &[Agg; 4]) {
    for (k, name) in ["exact (frontier tier)", "greedy", "greedy+polish, equal time", "pbit (sampler+gate+polish)"].iter().enumerate() { let a = &agg[k]; if a.runs == 0 { continue; }
        println!("# {label} | {name} | runs {} | ms p50 {:.3} p95 {:.3} | gap nats p50 {:.3} max {:.3} | top1 vs exact odds {:.3} | violations {} | maxTV p50 {} | passed {} | released {} of {} | wrong {}",
            a.runs, q(&a.ms, 0.5), q(&a.ms, 0.95), q(&a.gap, 0.5), q(&a.gap, 1.0), mean(&a.top1), a.viol,
            if a.maxtv.is_empty() { "-".into() } else { format!("{:.2e}", q(&a.maxtv, 0.5)) }, if k == 3 { a.cert.to_string() } else { "-".into() },
            if k == 3 { a.rel.to_string() } else { "-".into() }, a.t, if k == 3 { a.bad.to_string() } else { "-".into() }); }
}
fn main() {
    let bud: f64 = env("BUD", 200.0); let pol: f64 = env("POL", 50.0); let only: String = env("ONLY", String::new());
    let ninst: u64 = env("NINST", 16); let seeds: u64 = env("SEEDS", 4); let ref_ms: f64 = env("REF_MS", 3000.0);
    if only == "S" { // generator check vs the CLI JSON (bench/router_ilp.py prints the same signature from `pbit demo`)
        for &(n, s, hd) in &[(30usize, 7u64, false), (300, 7, false), (300, 8, false), (300, 9, false), (300, 10, false), (300, 7, true)] { let p = demo(n, s, hd);
            let sh: f64 = (0..n * 6).filter(|&k| p.allowed[k]).map(|k| p.h[k]).sum(); println!("sig n={n} seed={s} hard={hd}: sum_h={sh:.2} allowed={} cap={:?} lam={}", p.allowed.iter().filter(|&&v| v).count(), p.cap, p.lam); }
        return;
    }
    println!("fam,id,t,method,ms,viol,gap_nats,top1_vs_exact,maxTV,diagnostics_passed,released,wrong");
    if only.is_empty() || only == "A" {
        let mut agg: [Agg; 4] = Default::default();
        for k in 7000..7000 + ninst {
            let st = (k % 8) as usize; let seed = 9000 + 131 * k; let nbk = 20;
            let ins = match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
                3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), 5 => build(nbk, 4, 4, 0.5, seed, false),
                6 => build(8, 3, 5, 2.0, seed, true), _ => build(8, 3, 5, 4.0, seed, true) };
            let ex = exact_dp(&ins); let opt = exact_map_logw(&ins);
            with_oracle(&format!("A{st}"), k, &ins.p, &ex.marg, opt, bud, pol, &mut agg);
        }
        summary("A oracle blocks T=80/200", &agg);
    }
    if only.is_empty() || only == "B" {
        for &n in &[12usize, 18, 24, 30] { let mut agg: [Agg; 4] = Default::default();
            for s in 0..seeds { let p = demo(n, 7 + s, false);
                let Some(f) = exact_frontier(&p, 1 << 20) else { println!("B,{s},{n},oracle,declined"); continue };
                with_oracle("B", 7 + s, &p, &f.marg, f.map_logw, bud, pol, &mut agg); }
            summary(&format!("B demo T={n}"), &agg); }
    }
    if only.is_empty() || only == "C" {
        let mut rows: Vec<[f64; 8]> = vec![];
        for s in 0..seeds.min(5) { let p = demo(300, 7 + s, false);
            let t0 = Instant::now(); let dec = exact_frontier(&p, FRONTIER_MAX_STATES).is_some(); let dms = ms(t0);
            let rf = sample_on(&p, 4, 4, 0, Some(ref_ms), 777 + s, false, auto_group_pairs(&p), 100, 0).unwrap(); let rtop = argmax_rows(&rf.marg, p.t, p.a);
            let t0 = Instant::now(); let gx = p.feasible_init().unwrap(); let gms = ms(t0);
            let t0 = Instant::now(); let (d, g) = decide_gated(&p, 0, 0, Some(bud), 1 + s, &GATE).unwrap(); let g = g.unwrap();
            let pp = polish_plan(&p, Some(&d.map), pol, 1 + s).unwrap(); let pms = ms(t0);
            let nrel = g.released_tasks(&GATE).iter().filter(|&&r| r).count();
            let t0 = Instant::now(); let gx2 = p.feasible_init().unwrap(); let gp = polish_plan(&p, Some(&gx2), (pms - ms(t0)).max(1.0), 1 + s).unwrap(); let gpms = ms(t0);
            println!("C,{},300,frontier_tier,{dms:.2},{},,,,,,", 7 + s, if dec { "solved" } else { "declined" });
            println!("C,{},300,greedy,{gms:.3},{},logw={:.3},{:.4},,,,", 7 + s, p.violations(&gx), p.logw(&gx), agree(&gx, &rtop));
            println!("C,{},300,greedy+polish,{gpms:.1},{},logw={:.3},{:.4},,,,", 7 + s, p.violations(&gp.1), gp.0, agree(&gp.1, &rtop));
            println!("C,{},300,pbit,{pms:.1},{},logw={:.3},{:.4},,{},{nrel},", 7 + s, p.violations(&pp.1), pp.0, agree(&argmax_rows(&d.marg, p.t, p.a), &rtop), g.diagnostics_passed(&GATE) as u8);
            rows.push([gms, p.logw(&gx), gpms, gp.0, pms, pp.0, nrel as f64, g.diagnostics_passed(&GATE) as u8 as f64]);
        }
        let col = |j: usize| rows.iter().map(|r| r[j]).collect::<Vec<_>>();
        println!("# C demo T=300 ({} seeds, ref {ref_ms} ms) | greedy ms p50 {:.3} logw p50 {:.2} | greedy+polish ms p50 {:.1} logw p50 {:.2} | pbit ms p50 {:.1} logw p50 {:.2} | pbit passed {} released p50 {}",
            rows.len(), q(&col(0), 0.5), q(&col(1), 0.5), q(&col(2), 0.5), q(&col(3), 0.5), q(&col(4), 0.5), q(&col(5), 0.5), col(7).iter().sum::<f64>(), q(&col(6), 0.5));
    }
}
