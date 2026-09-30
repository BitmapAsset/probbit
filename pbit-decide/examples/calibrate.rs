//! Refusal-gate calibration harness: exact DP oracle (chain-of-blocks, T = 10*BLOCKS) vs 4-chain sampler.
//! Prints one CSV row per run; FCR/FRR tables are computed from the CSV.
//! env: SEED0 (first seed, default 0), NSEED (default 3), BLOCKS (default 20), BUDGETS (comma list ms, default 10,25,50,200)
use pbit_decide::*;
use pbit_decide::oracle::*;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn main() {
    let s0: u64 = env("SEED0", 0); let ns: u64 = env("NSEED", 3); let nbk: usize = env("BLOCKS", 20);
    let buds: Vec<f64> = std::env::var("BUDGETS").unwrap_or("10,25,50,200".into()).split(',').map(|v| v.parse().unwrap()).collect();
    println!("lam,capp,capb,tight,budget,pairs,seed,sweeps,meanTV,maxTV,rhat,sig_tv_max,min_ess,chain_dis,min_batches,worst_true_task,worst_gate_task,sig67,minb67,tasks_cert,tasks_cert_bad,tasks_good");
    for sd in s0..s0 + ns { for &lam in &[0.5, 0.8, 2.0, 4.0, 6.0] { for &(cp, cb) in &[(5usize, 5usize), (4, 4), (4, 3), (3, 5)] { for pairs in [false, true] {
        let ins = build(nbk, cp, cb, lam, 1234 + cp as u64 * 10 + cb as u64 + 1000 * sd + (lam * 100.0) as u64 * 7, pairs);
        let ex = exact_dp(&ins); if ex.marg.iter().any(|v| v.is_nan()) { continue; }
        let tight = (nbk * M) as f64 / (2.0 * nbk as f64 * cp as f64 + (nbk - 1) as f64 * cb as f64);
        for &bud in &buds {
            let s = sample(&ins.p, 4, 0, Some(bud), 42 + sd, true, false).unwrap();
            let g = gate_stats(&ins.p, &s); let g2 = gate_stats_with(&ins.p, &s, 2.0 / 3.0); let na = ins.p.a;
            let tvs: Vec<f64> = (0..ins.p.t).map(|i| 0.5 * (0..na).map(|a| (s.marg[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>()).collect();
            let tc = g.certified_tasks(&GATE);
            let (wt, mx) = tvs.iter().enumerate().fold((0, 0.0), |b, (i, &v)| if v > b.1 { (i, v) } else { b });
            println!("{lam},{cp},{cb},{tight:.3},{bud},{},{sd},{},{:.5},{:.5},{:.4},{:.5},{:.1},{:.4},{},{wt},{},{:.5},{},{},{},{}", pairs as u8, s.sweeps / 4,
                tvs.iter().sum::<f64>() / tvs.len() as f64, mx, g.rhat, g.sig_tv_max, g.min_ess, g.chain_dis, g.min_batches, g.worst_task, g2.sig_tv_max, g2.min_batches, tc.iter().filter(|&&c| c).count(), tc.iter().zip(&tvs).filter(|(&c, &v)| c && v > 0.05).count(), tvs.iter().filter(|&&v| v <= 0.05).count());
        }
    } } } }
}
