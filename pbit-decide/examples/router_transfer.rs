//! Transfer test of the shipped gate on the agent_router family (never used for tuning): 13-15 tasks x 6 workers,
//! several quota settings and affinities, exact enumeration vs decide_gated (sampler forced), released tickets checked.
use pbit_core::Philox4x32;
use pbit_decide::*;
const W: [&str; 6] = ["opus", "sonnet", "luna-pro", "local-gemma", "codex", "human"];
const CUST: [&str; 8] = ["acme", "globex", "initech", "umbrella", "hooli", "stark", "wayne", "wonka"];
// (template, PII, production DB, judge fit per worker: opus sonnet luna gemma codex human)
const TPL: [(&str, bool, bool, [f64; 6]); 8] = [
    ("draft reply: refund dispute (PII)", true, false, [2.0, 1.6, 1.0, 0.6, -1.0, 1.2]),
    ("summarize 40-page contract (PII)", true, false, [2.2, 1.8, 0.8, 0.9, -1.0, 0.5]),
    ("refactor auth middleware", false, false, [1.6, 1.2, 0.0, -0.5, 2.0, -1.0]),
    ("prod DB migration review", false, true, [1.8, 1.0, 1.2, 0.0, 1.5, 1.0]),
    ("weekly SEO report", false, false, [0.5, 1.0, 1.6, 0.5, -1.0, -1.5]),
    ("triage support inbox", false, false, [0.3, 0.8, 1.5, 1.0, -1.0, 0.0]),
    ("fix flaky CI test", false, false, [1.0, 0.8, 0.2, -0.5, 1.8, -2.0]),
    ("investor update draft", false, false, [1.8, 1.4, 0.6, 0.2, -1.0, 0.8]),
];

#[allow(dead_code)] // name/tpl are used by agent_router, not by this test
struct Queue { p: Problem, name: Vec<String>, tpl: Vec<usize> }

/// task i = template i%8 for customer (i/3)%8; every 3 consecutive tasks are one customer workflow (affinity group)
fn queue(n: usize, cap: [usize; 6], lam: f64, seed: u64) -> Queue {
    let mut r = Philox4x32::new(seed, 4242);
    let mut g = || { let u = r.f64() + 1e-12; let v = r.f64(); (-2.0 * u.ln()).sqrt() * (6.283185307 * v).cos() };
    let a = W.len(); let (mut h, mut allowed, mut name, mut tpl) = (vec![0.0; n * a], vec![true; n * a], vec![], vec![]);
    for i in 0..n {
        let k = i % TPL.len(); let (tn, pii, prod, fit) = TPL[k];
        name.push(format!("T{:03} {:<8} {}", i, CUST[(i / 3) % CUST.len()], tn)); tpl.push(k);
        for w in 0..a {
            h[i * a + w] = fit[w] + 0.7 * g();
            if pii && w != 3 && w != 5 { allowed[i * a + w] = false; } // PII: local-gemma or human only
            if prod && (w == 2 || w == 3) { allowed[i * a + w] = false; } // prod DB: no cheap models
        }
    }
    let group = (0..n).map(|i| i / 3).collect();
    Queue { p: Problem { t: n, a, h, allowed, cap: cap.to_vec(), group, lam, clamp: vec![None; n], block_moves: false, pair_swaps: false, collective: false, cluster: false, cycles: false }, name, tpl }
}

fn main() {
    println!("n,lam,capset,seed,budget,feasible,rel,relbad,cert,certbad,maxTV");
    let caps = [[2usize, 2, 3, 2, 2, 3], [2, 3, 3, 3, 2, 2], [3, 2, 2, 2, 3, 3]];
    for n in [13usize, 14, 15] { for &lam in &[0.8, 2.0, 4.0] { for (ci, c) in caps.iter().enumerate() { for sd in 0..2u64 {
        let q = queue(n, *c, lam, 31 + sd * 7 + n as u64); let p = &q.p;
        let Some(ex) = exact(p, 1, 20_000_000) else { continue }; if ex.n_feasible == 0 { continue; }
        for b in [25.0, 100.0] {
            let (d, g) = decide_gated(p, 0, 0, Some(b), 3 + sd, &GATE).unwrap(); let g = g.unwrap(); let rel = g.released_tasks(&GATE);
            let tv: Vec<f64> = (0..p.t).map(|i| 0.5 * (0..p.a).map(|k| (d.marg[i * p.a + k] - ex.marg[i * p.a + k]).abs()).sum::<f64>()).collect();
            let mx = tv.iter().cloned().fold(0.0, f64::max); let cert = g.diagnostics_passed(&GATE);
            println!("{n},{lam},{ci},{sd},{b},{},{},{},{},{},{mx:.4}", ex.n_feasible, rel.iter().filter(|&&r| r).count(), rel.iter().zip(&tv).filter(|(&r, &v)| r && v > 0.05).count(), cert as u8, (cert && mx > 0.05) as u8);
        } } } } }
}
