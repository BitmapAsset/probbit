//! Frontier-DP k-group exact block move (`WINDOW_K`) vs the shipped path, against the exact DP oracle.
//! Settings follow calib_sat (ST = k % 8 unless ST is set): 7 = oracle::build(8,3,5,4.0,..) (T=80, lam 4, private cap 3 < group 5),
//! the family that freezes today. One CSV row per (instance, K, budget). env: ST, NINST, SEED0, KS (list), WREPS, BUDGETS (ms),
//! SWEEPS (fixed sweeps instead of a budget), BLOCKS, AUTO (default 1 = two-group move iff lam >= 2).
use pbit_decide::*; use pbit_decide::oracle::*; use std::sync::atomic::Ordering;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn list<T: std::str::FromStr>(k: &str, d: &str) -> Vec<T> { std::env::var(k).unwrap_or(d.into()).split(',').filter_map(|v| v.parse().ok()).collect() }
pub fn inst(st: usize, nbk: usize, seed: u64) -> Inst {
    match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
        3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), 5 => build(nbk, 4, 4, 0.5, seed, false),
        6 => build(8, 3, 5, 2.0, seed, true), _ => build(8, 3, 5, 4.0, seed, true) }
}
fn main() {
    let nbk: usize = env("BLOCKS", 20); let ninst: u64 = env("NINST", 8); let s0: u64 = env("SEED0", 0); let stf: i64 = env("ST", -1);
    let ks: Vec<usize> = list("KS", "0,4,16"); let wr: usize = env("WREPS", 1); let buds: Vec<f64> = list("BUDGETS", "50,200,1000");
    let sweeps: usize = env("SWEEPS", 0); let auto: u8 = env("AUTO", 1);
    println!("inst,st,lam,rho,k,wreps,budget,sweeps,maxTV,meanTV,rhat,sigmax,frozen,cert,rel,bad,ms");
    for k in s0..s0 + ninst {
        let st = if stf >= 0 { stf as usize } else { (k % 8) as usize }; let ins = inst(st, nbk, 9000 + 131 * k);
        let ex = exact_dp(&ins); if ex.marg.iter().any(|v| v.is_nan()) { eprintln!("nan oracle {k}"); continue; }
        let (p, na) = (&ins.p, ins.p.a); let gp = auto == 1 && auto_group_pairs(p);
        for &wk in &ks { WINDOW_K.store(wk, Ordering::Relaxed); WINDOW_REPS.store(wr, Ordering::Relaxed);
            let runs: Vec<Option<f64>> = if sweeps > 0 { vec![None] } else { buds.iter().map(|&b| Some(b)).collect() };
            for bud in runs {
                let t0 = std::time::Instant::now();
                let s = sample_opts(p, 4, sweeps, bud, 1 + k, true, false, gp).unwrap(); let g = gate_stats(p, &s); let ms = t0.elapsed().as_secs_f64() * 1e3;
                let tvs: Vec<f64> = (0..p.t).map(|i| 0.5 * (0..na).map(|a| (s.marg[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>()).collect();
                let mx = tvs.iter().cloned().fold(0.0, f64::max); let rel = g.certified_tasks(&GATE);
                let nrel = rel.iter().filter(|&&r| r).count(); let bad = rel.iter().zip(&tvs).filter(|(&r, &v)| r && v > GATE.tv_tol).count();
                println!("{k},{st},{},{:.3},{wk},{wr},{},{},{mx:.5},{:.5},{:.5},{:.5},{},{},{nrel},{bad},{ms:.1}", p.lam, rho(&ins), bud.unwrap_or(0.0), s.sweeps / 4,
                    tvs.iter().sum::<f64>() / tvs.len() as f64, g.rhat, g.sig_tv_max, g.frozen, g.certified(&GATE) as u8);
            }
        }
    }
    WINDOW_K.store(0, Ordering::Relaxed);
}
