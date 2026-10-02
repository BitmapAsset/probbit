//! Sudoku as a probbit-ir program vs a classical DFS-backtracking solver (bitmasks + minimum-remaining-
//! values). Puzzles are GENERATED (random full grid, then givens removed while the solution stays unique); "open" puzzles
//! then lose EXTRA givens, so they have several solutions. Per puzzle: DFS time to solve + prove uniqueness (count to 2),
//! DFS exhaustive count + exact per-cell odds (open puzzles), and the probbit sampler (4 chains, MS ms, gate): its verdict and,
//! for every variable it RELEASES, the true TV error vs the DFS odds (a false release = TV > 0.05); plus the IR exact
//! tier (dynamic-MRV enumeration): time, solution count and max odds error vs the DFS. env: N (default 10), MS.
use probbit_core::Philox4x32;
use probbit_ir::*;
use std::time::Instant;
struct S { g: [u8; 81], row: [u16; 9], col: [u16; 9], bx: [u16; 9] }
impl S {
    fn new(g: &[u8; 81]) -> S { let mut s = S { g: [0; 81], row: [0; 9], col: [0; 9], bx: [0; 9] }; for c in 0..81 { if g[c] > 0 { s.set(c, g[c]); } } s }
    fn set(&mut self, c: usize, d: u8) { let b = 1u16 << d; self.g[c] = d; self.row[c / 9] |= b; self.col[c % 9] |= b; self.bx[c / 27 * 3 + c % 9 / 3] |= b; }
    fn clr(&mut self, c: usize, d: u8) { let b = !(1u16 << d); self.g[c] = 0; self.row[c / 9] &= b; self.col[c % 9] &= b; self.bx[c / 27 * 3 + c % 9 / 3] &= b; }
    fn cand(&self, c: usize) -> u16 { !(self.row[c / 9] | self.col[c % 9] | self.bx[c / 27 * 3 + c % 9 / 3]) & 0x3fe }
    /// Count solutions up to `limit`; `odds` accumulates per (cell, digit) solution counts; `rng` randomises the digit order.
    fn count(&mut self, limit: u64, odds: &mut Option<Vec<u64>>, rng: &mut Option<Philox4x32>, stop_first: bool) -> u64 {
        let mut best = (10u32, 81usize);
        for c in 0..81 { if self.g[c] == 0 { let k = self.cand(c).count_ones(); if k < best.0 { best = (k, c); if k <= 1 { break; } } } }
        if best.1 == 81 { if let Some(o) = odds { for c in 0..81 { o[c * 9 + self.g[c] as usize - 1] += 1; } } return 1; }
        let c = best.1; let m = self.cand(c); let mut ds: Vec<u8> = (1..=9).filter(|&d| m >> d & 1 == 1).collect();
        if let Some(r) = rng { for q in (1..ds.len()).rev() { let o = r.below(q + 1); ds.swap(q, o); } }
        let mut n = 0;
        for d in ds { self.set(c, d); n += self.count(limit - n, odds, rng, stop_first); if n >= limit || (stop_first && n > 0) { return n; } self.clr(c, d); }
        n
    }
}
fn model(g: &[u8; 81]) -> Model {
    let mut units: Vec<Vec<usize>> = (0..9).map(|r| (0..9).map(|c| r * 9 + c).collect()).collect();
    units.extend((0..9).map(|c| (0..9).map(|r| r * 9 + c).collect::<Vec<_>>())); units.extend((0..9).map(|b| (0..9).map(|q| (b / 3 * 3 + q / 3) * 9 + b % 3 * 3 + q % 3).collect::<Vec<_>>()));
    let caps = units.iter().flat_map(|u| (0..9).map(move |d| Cap { weights: vec![], members: u.iter().map(|&c| (c, d)).collect(), limit: 1 })).collect();
    let clamp = g.iter().map(|&d| if d > 0 { Some(d as usize - 1) } else { None }).collect();
    Model::new(81, 9, vec![0.0; 729], vec![true; 729], clamp, vec![], caps).unwrap()
}
fn q(mut v: Vec<f64>) -> (f64, f64, f64) { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); let p = |f: f64| v[((v.len() - 1) as f64 * f).round() as usize]; (p(0.5), p(0.25), p(0.75)) }
fn main() {
    let n: u64 = std::env::var("N").ok().and_then(|v| v.parse().ok()).unwrap_or(10); let ms: f64 = std::env::var("MS").ok().and_then(|v| v.parse().ok()).unwrap_or(100.0);
    println!("machine: Apple M4 (4P+6E); sampler 4 chains on 4 threads, {ms} ms, gate z=3 tol 0.05; {n} generated puzzles per class");
    println!("class,puzzle,givens,solutions,dfs_unique_ms,dfs_count_ms,probbit_exact_ms,probbit_exact_n,probbit_exact_odds_err,verdict,released,released_blank,escalated,false_releases,max_tv_released,probbit_ms");
    let (mut fr_total, mut rel_total, mut t_dfs, mut t_pb, mut t_ex) = (0usize, 0usize, vec![], vec![], vec![]);
    for k in 0..n {
        let mut r = Philox4x32::new(31337, k);
        let mut s = S::new(&[0; 81]); s.count(1, &mut None, &mut Some(Philox4x32::new(7, k)), true); let full = s.g;
        let mut g = full; let mut order: Vec<usize> = (0..81).collect(); for q in (1..81).rev() { let o = r.below(q + 1); order.swap(q, o); }
        for &c in &order { let d = g[c]; g[c] = 0; if S::new(&g).count(2, &mut None, &mut None, false) != 1 { g[c] = d; } }
        let mut open = g; let mut removed = 0; for &c in &order { if open[c] > 0 && removed < 4 { open[c] = 0; removed += 1; } }
        for (class, pz) in [("unique", g), ("open", open)] {
            let givens = pz.iter().filter(|&&d| d > 0).count();
            let t = Instant::now(); let two = S::new(&pz).count(2, &mut None, &mut None, false); let dfs_u = t.elapsed().as_secs_f64() * 1e3;
            let mut odds = Some(vec![0u64; 729]); let t = Instant::now(); let sols = S::new(&pz).count(u64::MAX, &mut odds, &mut None, false); let dfs_c = t.elapsed().as_secs_f64() * 1e3;
            let ex: Vec<f64> = odds.unwrap().iter().map(|&c| c as f64 / sols as f64).collect(); assert!(two == sols.min(2));
            let m = model(&pz); let t = Instant::now(); let e = exact(&m, 1, 2_000_000); let pe = t.elapsed().as_secs_f64() * 1e3; // MRV enumeration
            let (en, eerr) = e.map_or((0, f64::NAN), |e| (e.n_feasible, e.marg.iter().zip(&ex).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max))); t_ex.push(pe);
            let t = Instant::now(); let smp = sample(&m, 4, 0, Some(ms), 11 + k, true, false).unwrap(); let gs = gate_stats(&m, &smp);
            let whole = gs.diagnostics_passed(&GATE); let rel = if whole { vec![true; 81] } else { gs.released_tasks(&GATE) }; let pb = t.elapsed().as_secs_f64() * 1e3;
            let (mut fr, mut mx, mut nr, mut nb) = (0, 0.0f64, 0, 0);
            for c in 0..81 { if !rel[c] { continue; } nr += 1; if pz[c] == 0 { nb += 1; } let tv = 0.5 * (0..9).map(|d| (smp.marg[c * 9 + d] - ex[c * 9 + d]).abs()).sum::<f64>(); mx = mx.max(tv); if tv > 0.05 { fr += 1; } }
            let verdict = if whole { "diagnostics_passed" } else if nr > 0 { "partial" } else { "refused" };
            println!("{class},{k},{givens},{sols},{dfs_u:.3},{dfs_c:.3},{pe:.3},{en},{eerr:.1e},{verdict},{nr},{nb},{},{fr},{mx:.4},{pb:.1}", 81 - nr);
            fr_total += fr; rel_total += nr; t_dfs.push(dfs_u); t_pb.push(pb);
        }
    }
    let (a, a1, a3) = q(t_dfs); let (b, b1, b3) = q(t_pb); let (c, c1, c3) = q(t_ex);
    println!("probbit exact (MRV enumeration, full count + odds) ms median {c:.3} [{c1:.3},{c3:.3}] over both classes");
    println!("summary: released {rel_total} variables, false releases {fr_total}; DFS solve+prove ms median {a:.3} [{a1:.3},{a3:.3}]; probbit sampler+gate ms median {b:.1} [{b1:.1},{b3:.1}]");
    println!("START_FALLBACKS (feasible starts that needed the ascending retry): {}", START_FALLBACKS.load(std::sync::atomic::Ordering::Relaxed));
}
