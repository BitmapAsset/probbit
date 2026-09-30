//! Lambda-ladder replica exchange vs plain / pair-swap sampler at EQUAL wall-clock, exact DP oracle, T=200.
use pbit_decide::*;
use pbit_decide::oracle::*;
fn main() {
    let bud: f64 = std::env::var("BUD").ok().and_then(|v| v.parse().ok()).unwrap_or(200.0);
    println!("lam cap seed | method            sweeps/ch  meanTV  maxTV   R-hat  2sigTV  gate");
    for &(lam, cp, cb) in &[(2.0, 4usize, 3usize), (4.0, 4, 3), (6.0, 3, 5), (6.0, 4, 3)] { for sd in 0..3u64 {
        let base = build(20, cp, cb, lam, 9000 + sd * 31 + (lam * 10.0) as u64, false); let ex = exact_dp(&base);
        let mut pp = base.p.clone(); pp.pair_swaps = true;
        let runs: Vec<(&str, Samples)> = vec![
            ("plain", sample(&base.p, 4, 0, Some(bud), 1 + sd, true, false).unwrap()),
            ("pair-swaps", sample(&pp, 4, 0, Some(bud), 1 + sd, true, false).unwrap()),
            ("ladder6", sample_tempered(&base.p, &[0.0, 0.2, 0.4, 0.6, 0.8, 1.0], bud, 1 + sd).unwrap()),
            ("ladder6+pairs", sample_tempered(&pp, &[0.0, 0.2, 0.4, 0.6, 0.8, 1.0], bud, 1 + sd).unwrap()),
            ("ladder12+pairs", sample_tempered(&pp, &(0..12).map(|k| k as f64 / 11.0).collect::<Vec<_>>(), bud, 1 + sd).unwrap()),
        ];
        for (name, s) in runs { let g = gate_stats(&base.p, &s);
            println!("{lam:>3} {cp}/{cb} {sd}    | {name:<16} {:>9} {:>7.4} {:>7.4} {:>6.3} {:>6.3}  {}", s.sweeps / 4, mean_tv(&s.marg, &ex.marg, base.p.a), max_tv(&s.marg, &ex.marg, base.p.a), g.rhat, 2.0 * g.sig_tv_max, if g.certified(&GATE) { "CERT" } else { "refuse" }); }
    } }
}
