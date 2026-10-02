//! R19.6: cost of the arc-consistent (MAC) start vs the plain start search on programs where MAC applies.
//! `cargo run --release -p probbit-ir --example mac_start_cost` -> per program, median ms of `feasible_init_until` (N = 7 seeds,
//! 5 s deadline) with MAC on / off, and how many starts each found.
use probbit_core::Philox4x32;
use probbit_ir::*;
use std::sync::atomic::Ordering::Relaxed;

fn colouring(n: usize, d: f64, seed: u64) -> Model {
    // random G(n, m), m = n d / 2, 3 colours as pair caps (limit 1), one edge clamped (r, g): root AC prunes its neighbours
    let mut r = Philox4x32::new(seed, 9); let m = (n as f64 * d / 2.0).round() as usize; let mut e = std::collections::BTreeSet::new();
    while e.len() < m { let a = (r.f64() * n as f64) as usize % n; let b = (r.f64() * n as f64) as usize % n; if a != b { e.insert((a.min(b), a.max(b))); } }
    let caps: Vec<Cap> = e.iter().flat_map(|&(a, b)| (0..3).map(move |c| Cap { weights: vec![], members: vec![(a, c), (b, c)], limit: 1 })).collect();
    let mut clamp = vec![None; n]; let &(a, b) = e.iter().next().unwrap(); clamp[a] = Some(0); clamp[b] = Some(1);
    Model::new(n, 3, (0..n * 3).map(|_| r.f64() * 0.3).collect(), vec![true; n * 3], clamp, vec![], caps).unwrap()
}
fn chain(n: usize, k: usize) -> Model {
    let mut caps: Vec<Cap> = (0..k).map(|s| Cap { weights: vec![], members: (0..n).map(|i| (i, s)).collect(), limit: 1 }).collect();
    for i in 0..n - 1 { for a in 0..k { for b in 0..=a { caps.push(Cap { weights: vec![], members: vec![(i, a), (i + 1, b)], limit: 1 }); } } }
    Model::new(n, k, vec![0.0; n * k], vec![true; n * k], vec![None; n], vec![], caps).unwrap()
}
fn main() {
    let progs = [("colouring n=300 d=2", colouring(300, 2.0, 1)), ("colouring n=3000 d=2", colouring(3000, 2.0, 1)), ("colouring n=300 d=4", colouring(300, 4.0, 2)), ("chain 20 jobs x 30 slots", chain(20, 30))];
    for (name, m) in &progs {
        for mac in [true, false] {
            START_MAC.store(mac, Relaxed); let mut t: Vec<f64> = vec![]; let mut found = 0;
            for seed in 0..7u64 { let t0 = std::time::Instant::now();
                let x = m.feasible_init_until(&mut Philox4x32::new(seed, 1), START_WORK, Some(t0 + std::time::Duration::from_secs(5)));
                t.push(t0.elapsed().as_secs_f64() * 1e3); if let Some(x) = x { assert_eq!(m.violations(&x), 0); found += 1; } }
            t.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("{name:26} MAC {:3}: median {:9.3} ms (min {:9.3}, max {:9.3}), starts {found}/7", if mac { "on" } else { "off" }, t[3], t[0], t[6]);
        }
    }
    START_MAC.store(true, Relaxed);
}
