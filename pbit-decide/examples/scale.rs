//! Scale on this Mac. (A) Dispatch T=200..1000 x 10..30 agents, ~91% full (no oracle): latency, R-hat, gate,
//! partial-certification coverage vs budget. (B) exact-oracle chain-of-blocks at T=1000 (299 agents): accuracy + gate.
use pbit_decide::*;
use pbit_decide::oracle::*;
fn main() {
    println!("(A) Dispatch, groups of 4, lam 0.8, 4 chains on 4 threads");
    println!("   T   A  cap  fill | budget  sweeps/ch  R-hat  2sigTVmax  whole  partial-coverage");
    for &(t, a) in &[(200usize, 10usize), (500, 20), (1000, 10), (1000, 30)] {
        let legal_agents = (0..a).filter(|k| k % 3 == 2).count(); let legal = (0..t).filter(|i| (i / 4) % 3 == 2).count();
        let cap = ((t as f64 / (0.91 * a as f64)).ceil() as usize).max((legal + legal_agents - 1) / legal_agents);
        let d = dispatch(t, a, cap, 4, 0.8, 7);
        for &bud in &[50.0, 200.0, 1000.0] {
            let Some((dec, g)) = decide_gated(&d.p, 0, 0, Some(bud), 3, &GATE) else { println!("{t} {a} infeasible"); break };
            let g = g.unwrap(); let cov = g.released_tasks(&GATE).iter().filter(|&&c| c).count() as f64 / t as f64;
            println!("{t:>4} {a:>3} {cap:>4} {:>4.2} | {bud:>5}ms {:>9} {:>6.3} {:>9.4}  {:>6}  {:.3}   (wall {:.0} ms, viol-free MAP: {})",
                t as f64 / (a * cap) as f64, dec.samples / 4, g.rhat, 2.0 * g.sig_tv_max, if g.diagnostics_passed(&GATE) { "CERT" } else { "refuse" }, cov, dec.ms, d.p.violations(&dec.map) == 0);
        }
    }
    println!("\n(B) exact-oracle chain-of-blocks T=1000 (100 blocks, 299 agents), 4 chains x budget");
    println!("lam capP/capB | budget sweeps/ch  meanTV  maxTV   R-hat  2sigTVmax whole  partial-cov  partial-FCR  good-tickets  DP ms");
    for &(lam, cp, cb) in &[(0.8, 5usize, 5usize), (0.8, 4, 3), (2.0, 4, 3)] { for pairs in [false, true] {
        if !pairs && lam < 2.0 { } else if !pairs { continue; }
        let ins = build(100, cp, cb, lam, 777, pairs); let t0 = std::time::Instant::now(); let ex = exact_dp(&ins); let dpms = t0.elapsed().as_secs_f64() * 1e3; let na = ins.p.a;
        for &bud in &[200.0, 1000.0, 3000.0] {
            let s = sample(&ins.p, 4, 0, Some(bud), 5, true, false).unwrap(); let g = gate_stats(&ins.p, &s); let tc = g.released_tasks(&GATE);
            let tvs: Vec<f64> = (0..ins.p.t).map(|i| 0.5 * (0..na).map(|a| (s.marg[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>()).collect();
            let cov = tc.iter().filter(|&&c| c).count(); let bad = tc.iter().zip(&tvs).filter(|(&c, &v)| c && v > 0.05).count();
            println!("{lam:>3} {cp}/{cb} pairs={} | {bud:>5}ms {:>7} {:>7.4} {:>6.4} {:>6.3} {:>8.4}  {:>6}  {:>9.3}  {:>9}  {:>9.3}  {:.0}", pairs as u8, s.sweeps / 4, mean_tv(&s.marg, &ex.marg, na), max_tv(&s.marg, &ex.marg, na),
                g.rhat, 2.0 * g.sig_tv_max, if g.diagnostics_passed(&GATE) { "CERT" } else { "refuse" }, cov as f64 / 1000.0, format!("{}/{}", bad, cov), tvs.iter().filter(|&&v| v <= 0.05).count() as f64 / 1000.0, dpms);
        }
    } }
}
