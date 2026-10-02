//! Does the gate (calibrated on chain-of-blocks) transfer to a DIFFERENT family? Dispatch generator
//! (star agent, skill rule, same-customer groups), small enough for exact enumeration.
use pbit_decide::*;
fn main() {
    let (mut n, mut cert, mut bad, mut good, mut fr) = (0, 0, 0, 0, 0); let mut worst: f64 = 0.0;
    let (mut tk, mut tkbad) = (0usize, 0usize);
    for seed in 0..12u64 { for &(t, a, cap, gs, lam) in &[(9usize, 4usize, 3usize, 3usize, 1.0f64), (10, 4, 3, 2, 3.0), (10, 4, 3, 5, 5.0), (9, 3, 3, 3, 4.0)] {
        let p = dispatch(t, a, cap, gs, lam, 100 + seed).p; let Some(ex) = exact(&p, 1, 1 << 22) else { continue }; if ex.n_feasible == 0 { continue; }
        for sw in [100usize, 1000, 10000] {
            let s = sample(&p, 4, sw, None, seed, true, false).unwrap(); let g = gate_stats(&p, &s); let mx = max_tv(&s.marg, &ex.marg, p.a);
            n += 1; let c = g.diagnostics_passed(&GATE); if mx <= 0.05 { good += 1; if !c { fr += 1; } }
            if c { cert += 1; if mx > 0.05 { bad += 1; worst = worst.max(mx); } }
            for (i, &ci) in g.released_tasks(&GATE).iter().enumerate() { if ci { tk += 1; let tv = 0.5 * (0..p.a).map(|k| (s.marg[i * p.a + k] - ex.marg[i * p.a + k]).abs()).sum::<f64>(); if tv > 0.05 { tkbad += 1; } } }
        } } }
    println!("transfer family (Dispatch, exact enumeration): runs {n}, good {good}, passed {cert}, false-pass {bad} (worst {worst:.3}), FCR {:.3}, FRR {:.3}; per-ticket released {tk}, bad {tkbad}",
        bad as f64 / (cert.max(1)) as f64, fr as f64 / good.max(1) as f64);
}
