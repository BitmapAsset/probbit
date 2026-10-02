//! `exact_frontier` (general frontier DP, no knowledge of the chain structure) vs the structure-specific oracle
//! (`oracle::exact_dp` marginals, `oracle::exact_map_logw` optimum) on every calib_sat setting; plus the enumeration `exact()`
//! baseline on a tiny instance and the thick agent-router style case. env: BLOCKS (list), NINST, SEED0, MAXS.
use pbit_decide::*; use pbit_decide::oracle::*;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn main() {
    let blocks: Vec<usize> = std::env::var("BLOCKS").unwrap_or("20,100".into()).split(',').map(|v| v.parse().unwrap()).collect();
    let ninst: u64 = env("NINST", 16); let s0: u64 = env("SEED0", 7000); let maxs: usize = env("MAXS", 1 << 16);
    if std::env::var("ANYTIME").is_ok() {
        // the shipped anytime sampler (25 ms slices, deadline 1 s, whole-answer stop rule) on the same instances
        println!("blocks,inst,st,t,passed_at_ms,ms,max_tv,released");
        for &nbk in &blocks { for k in s0..s0 + ninst {
            let st = (k % 8) as usize; let seed = 9000 + 131 * k;
            let ins = match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
                3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), 5 => build(nbk, 4, 4, 0.5, seed, false),
                6 => build(8, 3, 5, 2.0, seed, true), _ => build(8, 3, 5, 4.0, seed, true) };
            let a = decide_anytime(&ins.p, 1000.0, 25.0, 1.0, 1 + k, &GATE, false, None).unwrap(); let ex = exact_dp(&ins);
            let rel = a.gate.released_tasks(&GATE).iter().filter(|&&r| r).count();
            println!("{nbk},{k},{st},{},{},{:.1},{:.4},{rel}", ins.p.t, a.passed_at_ms.map_or("NA".into(), |m| format!("{m:.1}")), a.decision.ms, max_tv(&a.decision.marg, &ex.marg, ins.p.a));
        } }
        return;
    }
    if std::env::var("THICKDECIDE").is_ok() {
        // thick queue: the exact tier must decline and leave the sampler's answer bit-identical (fixed sweeps => deterministic)
        println!("t,cap,lam,verdict_tier,verdict_forced,marg_bit_identical,ms_tier,ms_forced");
        for &(t, lam) in &[(120usize, 1.0f64), (300, 1.0), (300, 2.0)] { let cap = (t + 5) / 6 + 1; let p = dispatch(t, 6, cap, 3, lam, 5 + t as u64).p;
            let (a, _) = decide_gated(&p, 1, 3000, None, 9, &GATE).unwrap(); let (b, _) = decide_gated(&p, 0, 3000, None, 9, &GATE).unwrap();
            let v = |d: &Decision| match d.verdict { Verdict::Exact => "exact", Verdict::DiagnosticsPassed { .. } => "diagnostics_passed", Verdict::Unmixed { .. } => "refused" };
            let same = a.marg.iter().zip(&b.marg).all(|(x, y)| x.to_bits() == y.to_bits());
            println!("{t},{cap},{lam},{},{},{same},{:.1},{:.1}", v(&a), v(&b), a.ms, b.ms); }
        return;
    }
    if std::env::var("PODS").is_ok() {
        // mixed structure the chain oracle does not cover: P "pods" of thick sharing (3 private workers used by every task of the
        // pod, one random private forbidden per task) chained by bridge workers; groups of 3. Exactness vs enumeration where
        // enumeration runs, then time/states at scale.
        println!("pods,tasks_per_pod,t,a,lam,ms_frontier,max_states,ms_enum,max_abs_err_vs_enum,logz_diff");
        for &(np, n, lam) in &[(2usize, 6usize, 1.0f64), (2, 6, 3.0), (3, 4, 2.0), (20, 12, 1.0), (20, 12, 3.0), (100, 12, 2.0), (100, 15, 2.0)] {
            let a = 3 * np + (np - 1); let t = np * n; let mut r = pbit_core::Philox4x32::new(np as u64 * 31 + n as u64, 5);
            #[allow(clippy::approx_constant)] // 6.283185307, not TAU: the published benchmark instances depend on these exact bits
            let mut g = || { let u = r.f64() + 1e-12; let v = r.f64(); (-2.0 * u.ln()).sqrt() * (6.283185307 * v).cos() };
            let mut h = vec![0.0; t * a]; let mut allowed = vec![false; t * a]; let mut cap = vec![(n + 2) / 3; a];
            for b in 0..np - 1 { cap[3 * np + b] = 2; }
            for q in 0..np { for k in 0..n { let i = q * n + k; let forb = 3 * q + ((g().abs() * 7.0) as usize % 3);
                let mut el: Vec<usize> = (3 * q..3 * q + 3).filter(|&x| x != forb).collect(); if q > 0 { el.push(3 * np + q - 1); } if q < np - 1 { el.push(3 * np + q); }
                for &x in &el { allowed[i * a + x] = true; h[i * a + x] = 1.2 * g(); } } }
            let p = Problem { t, a, h, allowed, cap, group: (0..t).map(|i| i / 3).collect(), lam, clamp: vec![None; t], block_moves: false, pair_swaps: false, collective: false, cluster: false, cycles: false };
            let t0 = std::time::Instant::now(); let fe = exact_frontier(&p, maxs); let ms = t0.elapsed().as_secs_f64() * 1e3;
            let t1 = std::time::Instant::now(); let en = if t <= 12 { exact(&p, 1, 1 << 26) } else { None }; let me = t1.elapsed().as_secs_f64() * 1e3;
            let (err, dz) = match (&fe, &en) { (Some(f), Some(e)) => (format!("{:.2e}", f.marg.iter().zip(&e.marg).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)), format!("{:.2e}", f.logz - e.logz)), _ => ("NA".into(), "NA".into()) };
            println!("{np},{n},{t},{a},{lam},{ms:.2},{},{me:.1},{err},{dz}", fe.as_ref().map_or("declined".to_string(), |f| f.max_states.to_string()));
        }
        return;
    }
    if std::env::var("DECIDE").is_ok() {
        // end-to-end: decide_gated with the exact tier (exact_limit 1 -> enumeration declines at once -> frontier DP)
        println!("blocks,inst,st,lam,rho,t,verdict,ms,max_tv,map_gap_nats,violations");
        for &nbk in &blocks { for k in s0..s0 + ninst {
            let st = (k % 8) as usize; let seed = 9000 + 131 * k;
            let ins = match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
                3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), 5 => build(nbk, 4, 4, 0.5, seed, false),
                6 => build(8, 3, 5, 2.0, seed, true), _ => build(8, 3, 5, 4.0, seed, true) };
            let (d, _) = decide_gated(&ins.p, 1, 0, Some(200.0), 1 + k, &GATE).unwrap(); let ex = exact_dp(&ins); let mo = exact_map_logw(&ins);
            let v = match d.verdict { Verdict::Exact => "exact", Verdict::DiagnosticsPassed { .. } => "diagnostics_passed", Verdict::Unmixed { .. } => "refused" };
            println!("{nbk},{k},{st},{},{:.3},{},{v},{:.2},{:.2e},{:.2e},{}", ins.p.lam, rho(&ins), ins.p.t, d.ms, max_tv(&d.marg, &ex.marg, ins.p.a), mo - d.map_logw, ins.p.violations(&d.map));
        } }
        return;
    }
    if std::env::var("THICK").is_ok() {
        // thick sharing (every worker usable by most tasks): dispatch(t, 6 workers, cap, groups of 3). Where does the frontier DP
        // decline, how fast, and does it agree with enumeration where both run?
        println!("t,a,cap,lam,ms_frontier,result,max_states,ms_enum,enum_n,max_abs_err_vs_enum");
        for &t in &[6usize, 9, 12, 15, 18, 24, 30, 60, 120, 300] { let cap = (t + 5) / 6 + 1;
            let d = dispatch(t, 6, cap, 3, 1.0, 11 + t as u64); let p = &d.p;
            let t0 = std::time::Instant::now(); let fe = exact_frontier(p, maxs); let ms = t0.elapsed().as_secs_f64() * 1e3;
            let t1 = std::time::Instant::now(); let en = if t <= 12 { exact(p, 1, 50_000_000) } else { None }; let me = t1.elapsed().as_secs_f64() * 1e3;
            let err = match (&fe, &en) { (Some(f), Some(e)) => format!("{:.2e}", f.marg.iter().zip(&e.marg).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max)), _ => "NA".into() };
            println!("{t},6,{cap},1,{ms:.2},{},{},{me:.1},{},{err}", if fe.is_some() { "exact" } else { "declined" }, fe.as_ref().map_or(0, |f| f.max_states), en.as_ref().map_or(0, |e| e.n_feasible));
        }
        return;
    }
    println!("blocks,inst,st,lam,rho,t,ms_frontier,max_states,max_abs_marg_err,max_tv,map_gap_nats,map_viol,ms_oracle");
    for &nbk in &blocks { for k in s0..s0 + ninst {
        let st = (k % 8) as usize; let seed = 9000 + 131 * k;
        let ins = match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
            3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), 5 => build(nbk, 4, 4, 0.5, seed, false),
            6 => build(8, 3, 5, 2.0, seed, true), _ => build(8, 3, 5, 4.0, seed, true) };
        let t1 = std::time::Instant::now(); let ex = exact_dp(&ins); let mo = exact_map_logw(&ins); let ms_o = t1.elapsed().as_secs_f64() * 1e3;
        let t0 = std::time::Instant::now(); let fe = exact_frontier(&ins.p, maxs); let ms = t0.elapsed().as_secs_f64() * 1e3;
        match fe { None => println!("{nbk},{k},{st},{},{:.3},{},{ms:.2},NA,NA,NA,NA,NA,{ms_o:.2}", ins.p.lam, rho(&ins), ins.p.t),
            Some(f) => { let err = f.marg.iter().zip(&ex.marg).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
                println!("{nbk},{k},{st},{},{:.3},{},{ms:.2},{},{err:.2e},{:.2e},{:.2e},{},{ms_o:.2}", ins.p.lam, rho(&ins), ins.p.t, f.max_states, max_tv(&f.marg, &ex.marg, ins.p.a), mo - f.map_logw, ins.p.violations(&f.map)); } }
    } }
}
