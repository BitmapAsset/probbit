//! R19 P1.1(a)/(b): cost of the collective moves (`Model::collective`: global two-value flip, and for k > 2 the label swap) at
//! fixed work, off vs on, interleaved, median of REPS (default 5). Programs: a 400-spin ring (table couplings), a 12-spin
//! complete-graph ferromagnet (Potts 10), and a 400-variable 3-colouring ring (Potts -1, small fields; label swap only).
use pbit_ir::*;
fn main() {
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let ring = { let n = 400; let pairs = (0..n).map(|i| Pair { i, j: (i + 1) % n, c: Coupling::Table(vec![0.4, -0.4, -0.4, 0.4]) }).collect();
        Model::new(n, 2, (0..2 * n).map(|q| if q % 2 == 1 { 0.1 * ((q / 2 % 7) as f64 - 3.0) } else { 0.0 }).collect(), vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap() };
    let ferro = { let pairs = (0..12).flat_map(|i| (i + 1..12).map(move |j| Pair { i, j, c: Coupling::Potts(10.0) })).collect();
        Model::new(12, 2, vec![0.0; 24], vec![true; 24], vec![None; 12], pairs, vec![]).unwrap() };
    let col3 = { let n = 400; let pairs = (0..n).map(|i| Pair { i, j: (i + 1) % n, c: Coupling::Potts(-1.0) }).collect();
        Model::new(n, 3, (0..3 * n).map(|q| 0.05 * ((q % 5) as f64 - 2.0)).collect(), vec![true; 3 * n], vec![None; n], pairs, vec![]).unwrap() };
    println!("program,sweeps_per_chain,off_ms_median,on_ms_median,ratio");
    for (name, m0, sweeps) in [("ring400", ring, 2000usize), ("ferro12", ferro, 20000), ("colour3-ring400", col3, 2000)] {
        let (mut off, mut on) = (vec![], vec![]);
        for r in 0..reps { for flip in [false, true] { let mut m = m0.clone(); m.collective = flip;
            let t = std::time::Instant::now(); let s = sample(&m, 4, sweeps, None, 1 + r as u64, false, false).unwrap(); let ms = t.elapsed().as_secs_f64() * 1e3;
            assert!(s.n > 0); if flip { on.push(ms) } else { off.push(ms) } } }
        let med = |v: &mut Vec<f64>| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2] };
        let (a, b) = (med(&mut off), med(&mut on)); println!("{name},{sweeps},{a:.2},{b:.2},{:.3}", b / a);
    }
}
