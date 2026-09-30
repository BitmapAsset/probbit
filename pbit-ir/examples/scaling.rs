//! IR sampler throughput vs chains (one thread per chain): binary max-cut, random 3-regular n = 2000 at beta 1, and a
//! 9-value one-hot-free Potts program (n = 2000, k = 9, ring + chords). 300 ms per run, 5 reps, median [IQR] site updates/s.
use pbit_core::Philox4x32;
use pbit_ir::*;
use std::time::Instant;
fn regular3(n: usize, r: &mut Philox4x32) -> Vec<(usize, usize)> {
    loop { let mut st: Vec<usize> = (0..3 * n).map(|q| q / 3).collect();
        for q in (1..st.len()).rev() { let o = r.below(q + 1); st.swap(q, o); }
        let mut e: Vec<(usize, usize)> = st.chunks(2).map(|c| (c[0].min(c[1]), c[0].max(c[1]))).collect(); e.sort();
        if e.iter().all(|&(a, b)| a != b) && e.windows(2).all(|w| w[0] != w[1]) { return e; } }
}
fn main() {
    let n = 2000; let mut r = Philox4x32::new(7, 7); let e = regular3(n, &mut r);
    let mc = Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], e.iter().map(|&(i, j)| Pair { i, j, c: Coupling::Table(vec![0.0, 1.0, 1.0, 0.0]) }).collect(), vec![]).unwrap();
    let k = 9; let h: Vec<f64> = (0..n * k).map(|_| r.f64() - 0.5).collect();
    let pp = Model::new(n, k, h, vec![true; n * k], vec![None; n], e.iter().map(|&(i, j)| Pair { i, j, c: Coupling::Potts(-0.8) }).collect(), vec![]).unwrap();
    println!("Apple M4 (4P+6E), pbit-ir sample, 300 ms per run, 5 reps; one thread per chain");
    for (name, m) in [("max-cut n=2000 k=2 (table)", &mc), ("colouring-like Potts n=2000 k=9", &pp)] {
        for chains in [1usize, 2, 4, 8] {
            let mut v = vec![]; for rep in 0..5u64 { let t0 = Instant::now(); let s = sample(m, chains, 0, Some(300.0), 50 + rep, true, false).unwrap(); v.push(s.sweeps as f64 * n as f64 / t0.elapsed().as_secs_f64()); }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("{name} chains={chains}: {:.1} M site updates/s [{:.1},{:.1}] = {:.1} M per chain", v[2] / 1e6, v[1] / 1e6, v[3] / 1e6, v[2] / 1e6 / chains as f64);
        }
    }
}
