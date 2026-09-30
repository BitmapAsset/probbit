//! Cost of one gate evaluation (the anytime per-look overhead) on a 1 s, T=1000 and a 300 ms, T=200 run.
use pbit_decide::*;
use pbit_decide::oracle::*;
fn main() {
    for &(nb, bud) in &[(100usize, 1000.0), (20, 300.0)] {
        let ins = build(nb, 4, 3, 0.8, 99, true); let s = sample(&ins.p, 4, 0, Some(bud), 1, true, false).unwrap();
        let mut ms = vec![]; let mut g = None;
        for _ in 0..5 { let t0 = std::time::Instant::now(); g = Some(gate_stats(&ins.p, &s)); ms.push(t0.elapsed().as_secs_f64() * 1e3); }
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap()); let g = g.unwrap();
        println!("T={} budget {bud} ms, {} samples: gate_stats p50 {:.1} ms (min {:.1}) | bound {:.5} rhat {:.5} frozen {}", ins.p.t, s.n, ms[2], ms[0], g.tv_bound(&GATE), g.rhat, g.frozen);
    }
}
