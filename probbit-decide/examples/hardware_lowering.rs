//! What happens if the joint decision is compiled to a plain p-bit fabric (one-hot QUBO + penalty slack bits,
//! single-bit heat-bath = native p-bit dynamics) instead of carrying constraints natively (feasible-set + swap moves)?
//! Exact oracle: 8 tickets x 4 agents Dispatch (enumeration). Metrics at equal wall-clock.
use probbit_decide::*;
use probbit_decide::ir::*;
use std::time::Instant;
fn main() {
    let bud: f64 = std::env::var("BUD").ok().and_then(|v| v.parse().ok()).unwrap_or(20.0);
    for seed in [7u64, 11, 13] {
        let d = dispatch(8, 4, 2, 2, 1.0, seed).p; let ex = exact(&d, 1, 1 << 22).unwrap();
        let t0 = Instant::now(); let mut s = 0usize; let mut nat = vec![0.0; d.t * d.a]; let mut ch = Chain::new(&d, seed, 0).unwrap();
        while t0.elapsed().as_secs_f64() * 1e3 < bud { for _ in 0..64 { ch.sweep(); s += 1; for i in 0..d.t { nat[i * d.a + ch.x[i]] += 1.0; } } }
        for v in nat.iter_mut() { *v /= s as f64; }
        println!("seed {seed}: native constraint-preserving sampler: {s} samples in {bud} ms, 0 infeasible by construction, TV {:.4}", mean_tv(&nat, &ex.marg, d.a));
        for pen in [0.5, 1.0, 2.0, 4.0, 8.0, 16.0] {
            let q = lower_onehot(&d, pen); let mut st = q.encode(&d, &ex.top[0].1); let mut rng = probbit_core::Philox4x32::new(seed, 5);
            let (mut n, mut feas) = (0usize, 0usize); let mut m = vec![0.0; d.t * d.a]; let t0 = Instant::now();
            while t0.elapsed().as_secs_f64() * 1e3 < bud { for _ in 0..64 { q.sweep(&mut st, &mut rng); n += 1;
                if let Some(x) = q.decode(&d, &st) { feas += 1; for i in 0..d.t { m[i * d.a + x[i]] += 1.0; } } } }
            let tv = if feas > 0 { for v in m.iter_mut() { *v /= feas as f64; } mean_tv(&m, &ex.marg, d.a) } else { f64::NAN };
            println!("   p-bit QUBO lowering P={pen:>4}: {} bits, {} couplings | {n} sweeps, infeasible {:.1}%, feasible samples {feas}, TV(feasible-only) {tv:.4}",
                q.n, q.n_edges, 100.0 * (n - feas) as f64 / n as f64);
        }
    }
}
