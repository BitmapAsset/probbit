//! Kill test: max-cut / Ising programs on the IR vs brute-force enumeration. Random 3-regular graphs (pairing model),
//! n = 12 / 16, weight `beta` per cut edge (table [[0, b], [b, 0]]: anti-ferromagnetic, frustrated on odd cycles),
//! fields 0 (exact spin-flip symmetry: every true marginal is 0.5) or uniform(-0.2, 0.2). 4 chains x S sweeps, the certification
//! gate, compared with exact marginals. Counts certified runs, false certificates (maxTV > 0.05) and wrong released spins.
use pbit_core::Philox4x32;
use pbit_ir::*;
fn regular3(n: usize, r: &mut Philox4x32) -> Vec<(usize, usize)> {
    loop { let mut st: Vec<usize> = (0..3 * n).map(|q| q / 3).collect();
        for q in (1..st.len()).rev() { let o = r.below(q + 1); st.swap(q, o); }
        let mut e: Vec<(usize, usize)> = st.chunks(2).map(|c| (c[0].min(c[1]), c[0].max(c[1]))).collect(); e.sort();
        if e.iter().all(|&(a, b)| a != b) && e.windows(2).all(|w| w[0] != w[1]) { return e; } }
}
fn main() {
    let base: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1000); // instance seed base (measured with 1000)
    println!("instance seed base {base}\nn beta field sweeps | runs passed FALSE_pass | released wrong_released | median_maxTV");
    for n in [12usize, 16] { for beta in [0.3, 0.7, 1.5] { for field in [0.0, 0.2] { for sweeps in [500usize, 5000] {
        let (mut runs, mut cert, mut fcert, mut rel, mut bad, mut tvs) = (0, 0, 0, 0, 0, vec![]);
        for inst in 0..6u64 {
            let mut r = Philox4x32::new(base + inst, n as u64);
            let edges = regular3(n, &mut r);
            let h: Vec<f64> = (0..n).flat_map(|_| { let f = field * (2.0 * r.f64() - 1.0); [0.0, f] }).collect();
            let pairs = edges.iter().map(|&(i, j)| Pair { i, j, c: Coupling::Table(vec![0.0, beta, beta, 0.0]) }).collect();
            let mut m = Model::new(n, 2, h, vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap(); m.collective = std::env::var("COLLECTIVE").as_deref() == Ok("1");
            let ex = exact(&m, 1, 1 << 20).unwrap();
            let s = sample(&m, 4, sweeps, None, 77 + inst + (base - 1000) * 7, true, false).unwrap(); let g = gate_stats(&m, &s);
            let mx = max_tv(&s.marg, &ex.marg, 2); tvs.push(mx); runs += 1;
            if g.diagnostics_passed(&GATE) { cert += 1; if mx > 0.05 { fcert += 1; } }
            for (i, ok) in g.released_tasks(&GATE).into_iter().enumerate() { if ok { rel += 1; if (s.marg[2 * i] - ex.marg[2 * i]).abs() > 0.05 { bad += 1; } } }
        }
        tvs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("{n} {beta} {field} {sweeps} | {runs} {cert} {fcert} | {rel} {bad} | {:.4}", tvs[tvs.len() / 2]);
    } } } }
}
