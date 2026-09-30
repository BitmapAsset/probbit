//! Is the sampler's best-seen plan the optimum? Exact MAP by max-product DP (what an ILP would return).
use pbit_decide::*;
use pbit_decide::oracle::*;
fn main() {
    println!("setting            T    budget  best-seen logw   exact MAP logw   gap (nats) | polish 200ms gap");
    for &(nb, cp, cb, lam, sat) in &[(20usize, 4usize, 3usize, 0.8, false), (20, 4, 3, 2.0, false), (20, 3, 5, 4.0, false), (20, 4, 2, 2.0, true), (100, 4, 3, 0.8, false), (100, 4, 2, 2.0, true)] {
        for sd in 0..2u64 {
            let ins = if sat { build_sat(nb, lam, 800 + sd, true) } else { build(nb, cp, cb, lam, 800 + sd, true) };
            let opt = exact_map_logw(&ins);
            for bud in [50.0, 200.0] { let s = sample(&ins.p, 4, 0, Some(bud), 5 + sd, true, false).unwrap();
                let pol = polish_plan(&ins.p, Some(&s.best.1), 200.0, 77 + sd).unwrap(); assert_eq!(ins.p.violations(&pol.1), 0);
                println!("{:<18} {:>4} {:>6.0}ms {:>15.3} {:>16.3} {:>11.3} | {:>8.3}", format!("cap{cp}/{cb} lam{lam}{}", if sat { " rho=1" } else { "" }), ins.p.t, bud, s.best.0, opt, opt - s.best.0, opt - pol.0); }
        }
    }
}
