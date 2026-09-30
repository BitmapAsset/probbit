//! Certification-aware scheduling. Anytime loop (25 ms slices, deadline 300 ms); after each look, not-yet-released
//! tickets get FOCUS extra site updates per sweep. Tickets released at the deadline (and bad ones vs the exact DP), focus off vs on.
use pbit_decide::*;
use pbit_decide::oracle::*;
use std::sync::atomic::Ordering;
fn main() {
    println!("inst,setting,focus,rel,relbad,samples,maxTV");
    for k in 0..30u64 { let st = [2usize, 3, 5][(k % 3) as usize]; let seed = 11000 + 31 * k;
        let ins = match st { 2 => build(20, 4, 3, 0.8, seed, true), 3 => build(20, 4, 3, 2.0, seed, true), _ => build(20, 4, 4, 0.5, seed, false) };
        let ex = exact_dp(&ins); let na = ins.p.a;
        for reps in [0usize, 2, 8] { ANYTIME_FOCUS_REPS.store(reps, Ordering::Relaxed);
            let a = decide_anytime(&ins.p, 300.0, 25.0, 1.0, 5 + k, &GATE, false, Some(1.01)).unwrap(); // target > 1: never stops early
            let rel = a.gate.certified_tasks(&GATE);
            let tv: Vec<f64> = (0..ins.p.t).map(|i| 0.5 * (0..na).map(|q| (a.decision.marg[i * na + q] - ex.marg[i * na + q]).abs()).sum::<f64>()).collect();
            println!("{k},{st},{reps},{},{},{},{:.4}", rel.iter().filter(|&&r| r).count(), rel.iter().zip(&tv).filter(|(&r, &v)| r && v > 0.05).count(), a.decision.samples, tv.iter().cloned().fold(0.0, f64::max));
        } }
    ANYTIME_FOCUS_REPS.store(0, Ordering::Relaxed);
}
