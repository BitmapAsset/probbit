//! Simultaneous coverage on 1,000 independent fair bits (review case E6, ported to the gate/2 names).
//! Heat-bath sweeps refresh the whole target, so there is no metastability: per-item misses here are Monte-Carlo tails of the
//! per-item 3-sigma bounds, not a mixing failure. Per sweep budget over seeds 1..=SEEDS (default 50): whole answers passed,
//! released items, released items off by more than 0.05 (`bad`). `COLLECTIVE=1` turns the collective moves on (the CLI default).
use probbit_ir::*;
fn main() {
    let n = 1000; let seeds: u64 = std::env::var("SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
    let mut m = Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], vec![], vec![]).unwrap();
    m.collective = std::env::var("COLLECTIVE").as_deref() == Ok("1");
    println!("iid_gate: n={n} fair bits, 4 chains, seeds 1..={seeds}, collective {}", m.collective);
    for sweeps in [280, 400, 800] {
        let (mut whole, mut released, mut bad, mut calls_bad) = (0usize, 0usize, 0usize, 0usize);
        for seed in 1..=seeds {
            let s = sample(&m, 4, sweeps, None, seed, true, false).unwrap(); let g = gate_stats(&m, &s);
            let w = g.diagnostics_passed(&GATE); let rel = if w { vec![true; n] } else { g.released_tasks(&GATE) };
            let b = (0..n).filter(|&i| rel[i] && (s.marg[2 * i] - 0.5).abs() > 0.05).count();
            whole += w as usize; released += rel.iter().filter(|&&r| r).count(); bad += b; calls_bad += (b > 0) as usize;
        }
        println!("sweeps {sweeps}: whole {whole}/{seeds}, released {released}, bad {bad} (in {calls_bad} calls)");
    }
}
