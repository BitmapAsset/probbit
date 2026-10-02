//! Strong affinity with private capacity < group size (cap 3/5, groups of 5, lam >= 4).
//! Pair swaps vs pair swaps + exact two-group joint heat-bath (`sample_opts(.., group_pairs=true)`), TV vs the DP oracle.
use pbit_decide::*;
use pbit_decide::oracle::*;
fn main() {
    // correctness first: small exact-enumerable instance, move enabled
    let d = dispatch(9, 4, 3, 3, 3.0, 5).p; let ex = exact(&d, 1, 1 << 24).unwrap();
    let s = sample_opts(&d, 4, 20000, None, 1, true, false, true).unwrap();
    println!("small check (9 tasks, groups of 3, lam 3): TV vs exact enumeration mean {:.4} max {:.4}, violations {}", mean_tv(&s.marg, &ex.marg, d.a), max_tv(&s.marg, &ex.marg, d.a), s.viol);
    println!("nb,lam,capp,capb,seed,budget,move,sweeps,meanTV,maxTV,rhat,bound3,cert,rel,relbad");
    let nb: usize = std::env::var("BLOCKS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    // EASY=1: the regimes where the move is NOT needed (does it cost accuracy per ms there?)
    let sets: Vec<(f64, usize, usize)> = if std::env::var("EASY").is_ok() { vec![(0.5, 4, 4), (0.8, 4, 3), (2.0, 4, 3)] } else { vec![(4.0, 3, 5), (6.0, 3, 5), (4.0, 4, 3)] };
    for &(lam, cp, cb) in &sets { for sd in 0..4u64 {
        let ins = build(nb, cp, cb, lam, 4242 + sd * 17 + (lam as u64) * 1000 + cp as u64, true); let ex = exact_dp(&ins); let na = ins.p.a;
        for &bud in &[50.0, 200.0, 1000.0] { for (mv, gp) in [("pairs", false), ("pairs+gpair", true)] {
            let s = sample_opts(&ins.p, 4, 0, Some(bud), 9 + sd, true, false, gp).unwrap(); let g = gate_stats(&ins.p, &s);
            let tvs: Vec<f64> = (0..ins.p.t).map(|i| 0.5 * (0..na).map(|a| (s.marg[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>()).collect();
            let rel = g.released_tasks(&GATE);
            println!("{nb},{lam},{cp},{cb},{sd},{bud},{mv},{},{:.4},{:.4},{:.4},{:.4},{},{},{}", s.sweeps / 4, tvs.iter().sum::<f64>() / tvs.len() as f64, tvs.iter().cloned().fold(0.0, f64::max), g.rhat, g.tv_bound(&GATE), g.diagnostics_passed(&GATE) as u8,
                rel.iter().filter(|&&r| r).count(), rel.iter().zip(&tvs).filter(|(&r, &v)| r && v > 0.05).count());
        } }
    } }
}
