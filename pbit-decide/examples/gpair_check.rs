//! Correctness of the two-group move on small chain-of-blocks instances (exact DP), cap 3/5, lam 4.
//! Also compares per-bridge total-load distributions (exact vs sampled) to tell a biased move from a hidden mode.
use pbit_decide::*;
use pbit_decide::oracle::*;
fn main() {
    let nbs: Vec<usize> = std::env::var("NBS").unwrap_or("5,6,8,10".into()).split(',').map(|v| v.parse().unwrap()).collect();
    let lam: f64 = std::env::var("LAM").ok().and_then(|v| v.parse().ok()).unwrap_or(4.0);
    for &nb in &nbs { for gp in [false, true] {
        let ins = build(nb, 3, 5, lam, 4242 + nb as u64, true); let ex = exact_dp(&ins);
        let s = sample_opts(&ins.p, 4, 0, Some(1500.0), 9, true, false, gp).unwrap(); let g = gate_stats(&ins.p, &s);
        println!("nb {nb:2} gpair {gp:5}: sweeps/ch {:>7} meanTV {:.4} maxTV {:.4} rhat {:.3} bound {:.3} frozen {} cert {}", s.sweeps / 4, mean_tv(&s.marg, &ex.marg, ins.p.a), max_tv(&s.marg, &ex.marg, ins.p.a), g.rhat, g.tv_bound(&GATE), g.frozen, g.diagnostics_passed(&GATE));
        if std::env::var("BRIDGE").is_ok() { let t = ins.p.t;
            for b in 0..nb - 1 { let a = 2 * nb + b; let mut h = vec![0.0f64; 11]; let mut n = 0.0f64;
                for tr in &s.traj { for row in tr.chunks(t) { h[row.iter().filter(|&&v| v as usize == a).count()] += 1.0; n += 1.0; } }
                println!("   bridge {b}: exact {:?}  sampled {:?}", ex.bridge[b].iter().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>(), h.iter().take(6).map(|v| (v / n * 100.0).round() / 100.0).collect::<Vec<_>>()); } }
    } }
}
