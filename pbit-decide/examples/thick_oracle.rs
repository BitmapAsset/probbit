//! NEW exact-oracle family with THICK sharing (every worker usable by most tasks; before this, the router had no exact oracle).
//! dispatch(T, 6 workers, cap, groups of 3, lam, seed); exact odds from `exact_frontier` (checked vs `exact()` enumeration
//! up to T=12). Shipped sampler + gate (AUTO two-group move) vs window moves (KS), one CSV row per (inst, K, budget).
//! env: T (48), NINST, SEED0, LAMS (list), CAPX (extra cap per worker over ceil(T/6)), KS, BUDGETS.
use pbit_decide::*; use std::sync::atomic::Ordering;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn list<T: std::str::FromStr>(k: &str, d: &str) -> Vec<T> { std::env::var(k).unwrap_or(d.into()).split(',').filter_map(|v| v.parse().ok()).collect() }
fn main() {
    let t: usize = env("T", 48); let ninst: u64 = env("NINST", 6); let s0: u64 = env("SEED0", 100); let capx: usize = env("CAPX", 1);
    let lams: Vec<f64> = list("LAMS", "0.5,1,2"); let ks: Vec<usize> = list("KS", "0,3"); let buds: Vec<f64> = list("BUDGETS", "50,200");
    println!("inst,st,lam,rho,k,wreps,budget,sweeps,maxTV,meanTV,rhat,sigmax,frozen,cert,rel,bad,ms,t,ex_states,ex_ms");
    let (mut zr, mut z3, mut z4, mut nr) = (0usize, 0usize, 0usize, 0usize); let _ = &mut zr;
    for k in s0..s0 + ninst { for &lam in &lams {
        let cap = (t + 5) / 6 + capx; let d = dispatch(t, 6, cap, 3, lam, k); let p = &d.p; let na = p.a;
        let te = std::time::Instant::now(); let Some(ex) = exact_frontier(p, 1 << 18) else { eprintln!("no exact {k} {lam}"); continue }; let ex_ms = te.elapsed().as_secs_f64() * 1e3;
        let rho = t as f64 / (6 * cap) as f64;
        for &wk in &ks { WINDOW_K.store(wk, Ordering::Relaxed);
            for &bud in &buds {
                let t0 = std::time::Instant::now();
                let s = sample_opts(p, 4, 0, Some(bud), 1 + k, true, false, auto_group_pairs(p)).unwrap(); let g = gate_stats(p, &s); let ms = t0.elapsed().as_secs_f64() * 1e3;
                let tvs: Vec<f64> = (0..p.t).map(|i| 0.5 * (0..na).map(|a| (s.marg[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>()).collect();
                let mx = tvs.iter().cloned().fold(0.0, f64::max); let rel = g.certified_tasks(&GATE);
                let nrel = rel.iter().filter(|&&r| r).count(); let bad = rel.iter().zip(&tvs).filter(|(&r, &v)| r && v > GATE.tv_tol).count();
                for i in 0..p.t { if rel[i] { let zt = tvs[i] / g.sig_tv[i].max(g.sig_tv_long[i]).max(1e-12); nr += 1; if zt > 3.0 { z3 += 1; } if zt > 4.0 { z4 += 1; } } }
                println!("{k},thick,{lam},{rho:.3},{wk},1,{bud},{},{mx:.5},{:.5},{:.5},{:.5},{},{},{nrel},{bad},{ms:.1},{},{},{ex_ms:.1}", s.sweeps / 4, tvs.iter().sum::<f64>() / tvs.len() as f64,
                    g.rhat, g.sig_tv_max, g.frozen, g.certified(&GATE) as u8, p.t, ex.max_states);
            }
        }
    } }
    WINDOW_K.store(0, Ordering::Relaxed);
    eprintln!("ZSUM released {nr} zt>3 {z3} ({:.2e}) zt>4 {z4} ({:.2e}) [calibrated normal: 2.7e-3 / 6.3e-5]", z3 as f64 / nr.max(1) as f64, z4 as f64 / nr.max(1) as f64);
}
