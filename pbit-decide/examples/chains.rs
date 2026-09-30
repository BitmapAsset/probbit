//! Does spending the same wall-clock on MORE, shorter chains (8 threads) expose stuck regions that 4 chains hide?
use pbit_decide::*;
use pbit_decide::oracle::*;
fn main() {
    println!("inst                         | chains budget sweeps/ch  maxTV   R-hat  2sigTV  whole   partial(bad/cov)");
    let mut cases: Vec<(usize, f64, usize, usize, bool, u64, f64)> = vec![(100, 2.0, 4, 3, true, 777, 1000.0), (100, 2.0, 4, 3, true, 778, 1000.0), (100, 2.0, 4, 3, true, 779, 1000.0)];
    for sd in 0..6u64 { cases.push((20, 2.0, 4, 3, true, 5000 + sd, 200.0)); }
    for &(nb, lam, cp, cb, pairs, seed, bud) in &cases {
        let ins = build(nb, cp, cb, lam, seed, pairs); let ex = exact_dp(&ins); let na = ins.p.a;
        for ch in [4usize, 8] {
            let s = sample(&ins.p, ch, 0, Some(bud), 11, true, false).unwrap(); let g = gate_stats(&ins.p, &s); let tc = g.certified_tasks(&GATE);
            let tvs: Vec<f64> = (0..ins.p.t).map(|i| 0.5 * (0..na).map(|a| (s.marg[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>()).collect();
            let cov = tc.iter().filter(|&&c| c).count(); let bad = tc.iter().zip(&tvs).filter(|(&c, &v)| c && v > 0.05).count();
            println!("T={:<4} lam {lam} {cp}/{cb} seed {seed:<5} | {ch:>6} {bud:>5}ms {:>8} {:>7.4} {:>6.3} {:>6.4}  {:>6}  {bad}/{cov}", nb * 10, s.sweeps / ch, max_tv(&s.marg, &ex.marg, na), g.rhat, 2.0 * g.sig_tv_max, if g.certified(&GATE) { "CERT" } else { "refuse" });
        }
    }
}
