//! Saturation + scale calibration of the whole-answer gate and the per-ticket release.
//! Instance k cycles through 6 settings (incl. two at rho = 1 exactly); exact DP oracle for every instance.
//! Prints one CSV row per (instance, budget) with the gate outcome for every (z, guard) cell.
//! env: BLOCKS (20|100), NINST (50), BUDGETS (ms list), SEED0.
use probbit_decide::*;
use probbit_decide::oracle::*;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn main() {
    let nbk: usize = env("BLOCKS", 20); let ninst: u64 = env("NINST", 50); let s0: u64 = env("SEED0", 0);
    let buds: Vec<f64> = std::env::var("BUDGETS").unwrap_or("50,200".into()).split(',').map(|v| v.parse().unwrap()).collect();
    let t = (nbk * M) as f64; let zt = (2.0 * t.ln()).sqrt();
    let zs = [2.5, 3.0, zt]; let guards = [1.002, 1.005, 1.01];
    print!("inst,setting,lam,rho,budget,sweeps,maxTV,meanTV,rhat,sigmax,minb,good_tickets");
    for z in &zs { print!(",w{z:.2}"); } for z in &zs { for g in &guards { print!(",rel{z:.2}_{g},bad{z:.2}_{g}"); } }
    for z in &zs { print!(",r{z:.2},b{z:.2},pb{z:.2},pbbad{z:.2}"); } println!(",sig67,t,frozen,rel_loc,bad_loc,rel_hyb,bad_hyb");
    for k in s0..s0 + ninst {
        let st = (k % 8) as usize; if std::env::var("SAT_ONLY").is_ok() && st > 1 { continue; }
        if let Ok(o) = std::env::var("ONLY") { if !o.split(',').any(|v| v == st.to_string()) { continue; } } let seed = 9000 + 131 * k;
        let ins = match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
            3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), 5 => build(nbk, 4, 4, 0.5, seed, false),
            // Small-T strong coupling, cap 3 < group 5 (the regime where every chain froze in one bridge split)
            6 => build(8, 3, 5, 2.0, seed, true), _ => build(8, 3, 5, 4.0, seed, true) };
        let lam = ins.p.lam; let ex = exact_dp(&ins); if ex.marg.iter().any(|v| v.is_nan()) { eprintln!("nan oracle {k}"); continue; }
        let na = ins.p.a;
        for &bud in &buds {
            // AUTO=1 uses the shipped decide path policy (two-group move iff lam >= 2)
            let gp = std::env::var("AUTO").is_ok() && auto_group_pairs(&ins.p);
            let s = sample_opts(&ins.p, 4, 0, Some(bud), 1 + k, true, false, gp).unwrap(); let g = gate_stats(&ins.p, &s);
            let tvs: Vec<f64> = (0..ins.p.t).map(|i| 0.5 * (0..na).map(|a| (s.marg[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>()).collect();
            let mx = tvs.iter().cloned().fold(0.0, f64::max); let good = tvs.iter().filter(|&&v| v <= 0.05).count();
            print!("{k},{st},{lam},{:.3},{bud},{},{mx:.5},{:.5},{:.5},{:.5},{},{good}", rho(&ins), s.sweeps / 4, tvs.iter().sum::<f64>() / tvs.len() as f64, g.rhat, g.sig_tv_max, g.min_batches);
            for &z in &zs { let c = GateCfg { z, ..GATE }; print!(",{}", g.diagnostics_passed(&c) as u8); }
            for &z in &zs { for &gd in &guards {
                let ok = g.rhat < gd && g.min_batches >= GATE.min_batches && g.frozen == 0 && g.sig_tv_long.iter().cloned().fold(0.0, f64::max) <= BATCH_RATIO_MAX * g.sig_tv_max;
                let rel: Vec<bool> = g.sig_tv.iter().zip(&g.sig_tv_long).map(|(&sg, &l)| ok && z * sg.max(l) <= GATE.tv_tol).collect();
                print!(",{},{}", rel.iter().filter(|&&r| r).count(), rel.iter().zip(&tvs).filter(|(&r, &v)| r && v > 0.05).count()); } }
            // variants: r = whole answer + strict R-hat 1.005; b = batch size n^(2/3); pb = per-ticket (guard 1.002) with n^(2/3)
            let g67 = gate_stats_with(&ins.p, &s, 2.0 / 3.0);
            for &z in &zs { let c = GateCfg { z, ..GATE }; let r = g.diagnostics_passed(&c) && g.rhat < 1.005; let b = g.diagnostics_passed(&c) && g67.diagnostics_passed(&c);
                let ok = g.rhat < 1.002 && g.min_batches >= GATE.min_batches;
                let rel: Vec<bool> = g.sig_tv.iter().zip(&g67.sig_tv).map(|(&a, &b)| ok && z * a.max(b) <= GATE.tv_tol).collect();
                print!(",{},{},{},{}", r as u8, b as u8, rel.iter().filter(|&&r| r).count(), rel.iter().zip(&tvs).filter(|(&r, &v)| r && v > 0.05).count()); }
            let rl = g.released_tasks_local(&GATE);
            let hy = g.rhat < 1.005; // hybrid candidate: local rule + run-wide logw R-hat < 1.005
            println!(",{:.5},{},{},{},{},{},{}", g67.sig_tv_max, ins.p.t, g.frozen, rl.iter().filter(|&&r| r).count(), rl.iter().zip(&tvs).filter(|(&r, &v)| r && v > 0.05).count(),
                if hy { rl.iter().filter(|&&r| r).count() } else { 0 }, if hy { rl.iter().zip(&tvs).filter(|(&r, &v)| r && v > 0.05).count() } else { 0 });
        }
    }
}
