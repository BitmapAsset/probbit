//! Generic IR engine vs the specialized assignment engine on the same problems (bit-identical outputs, so this is
//! pure speed). Enumeration: plans/s. Sampler: sweeps/s single chain, no threads. 7 alternating repeats, median + IQR.
use probbit_decide::*;
use std::time::Instant;
fn med_iqr(mut v: Vec<f64>) -> (f64, f64, f64) { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); let q = |f: f64| v[((v.len() - 1) as f64 * f) as usize]; (q(0.5), q(0.25), q(0.75)) }
fn main() {
    let reps = 7;
    for (name, p) in [("dispatch 11x4 cap3 g3", dispatch(11, 4, 3, 3, 1.0, 5).p), ("dispatch 12x4 cap4 g3", dispatch(12, 4, 4, 3, 1.0, 6).p)] {
        let m = p.lower(); let (mut a, mut b) = (vec![], vec![]); let mut nf = 0;
        for _ in 0..reps { let t0 = Instant::now(); let e = exact(&p, 5, 1 << 26).unwrap(); a.push(t0.elapsed().as_secs_f64() * 1e3); nf = e.n_feasible;
            let t0 = Instant::now(); let f = probbit_ir::exact(&m, 5, 1 << 26).unwrap(); b.push(t0.elapsed().as_secs_f64() * 1e3); assert_eq!(f.logz.to_bits(), e.logz.to_bits()); }
        let (ma, la, ha) = med_iqr(a); let (mb, lb, hb) = med_iqr(b);
        println!("exact {name}: {nf} plans | specialized {ma:.1} ms [{la:.1},{ha:.1}] | IR {mb:.1} ms [{lb:.1},{hb:.1}] | IR/spec {:.2}", mb / ma);
    }
    for (name, p, sw) in [("r1 7x4 (acc1)", { let mut d = dispatch(7, 4, 2, 3, 1.0, 11).p; for v in d.allowed.iter_mut() { *v = true; } d }, 12_000usize),
                          ("dispatch 200x10 g4", dispatch(200, 10, 22, 4, 0.8, 3).p, 600), ("dispatch 60x3 g20 lam5", dispatch(60, 3, 20, 20, 5.0, 7).p, 1500)] {
        let m = p.lower(); let (mut a, mut b) = (vec![], vec![]);
        for r in 0..reps { let t0 = Instant::now(); let s = sample(&p, 1, sw, None, 9 + r as u64, false, false).unwrap(); a.push(s.sweeps as f64 / t0.elapsed().as_secs_f64());
            let t0 = Instant::now(); let s2 = probbit_ir::sample(&m, 1, sw, None, 9 + r as u64, false, false).unwrap(); b.push(s2.sweeps as f64 / t0.elapsed().as_secs_f64()); assert_eq!(s.traj, s2.traj); }
        let (ma, la, ha) = med_iqr(a); let (mb, lb, hb) = med_iqr(b);
        println!("sample {name}: specialized {ma:.0} sweeps/s [{la:.0},{ha:.0}] ({:.2} M site-updates/s) | IR {mb:.0} sweeps/s [{lb:.0},{hb:.0}] | IR/spec {:.2}", ma * p.t as f64 / 1e6, mb / ma);
    }
}
