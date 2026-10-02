//! Max-cut on Gset-LIKE graphs (generated with the rudy recipes' shapes, NOT the Gset files: literature best cuts
//! do not apply): G1-like = Erdos-Renyi n = 800, p = 0.06, unit weights; G11-like = 2D toroidal grid 20 x 40, weights +-1.
//! probbit (IR sampler at fixed beta; sample + anneal; anneal only) vs weighted simulated annealing (single-spin Metropolis,
//! geometric beta 0.2 -> 5, 4 restarts on 4 threads, best of 4) at EQUAL wall time (MS, default 200). 5 seeds, median + IQR.
use probbit_core::Philox4x32;
use probbit_ir::*;
use std::time::Instant;
fn cut(e: &[(usize, usize, i64)], x: &[usize]) -> i64 { e.iter().filter(|&&(a, b, _)| x[a] != x[b]).map(|e| e.2).sum() }
fn sa(adj: &[Vec<(usize, i64)>], ms: f64, seed: u64, stream: u64) -> (i64, u64) {
    let n = adj.len(); let mut r = Philox4x32::new(seed, stream); let mut x: Vec<u8> = (0..n).map(|_| (r.f64() < 0.5) as u8).collect();
    let mut c: i64 = adj.iter().enumerate().map(|(i, a)| a.iter().filter(|&&(j, _)| j > i && x[j] != x[i]).map(|&(_, w)| w).sum::<i64>()).sum(); let mut best = c;
    let t0 = Instant::now(); let mut flips = 0u64; let (b0, b1) = (0.2f64, 5.0f64); let mut beta = b0;
    loop {
        if flips % 1024 == 0 { let f = t0.elapsed().as_secs_f64() * 1e3 / ms; if f >= 1.0 { break; } beta = b0 * (b1 / b0).powf(f); }
        let i = r.below(n); let d: i64 = adj[i].iter().map(|&(j, w)| if x[j] == x[i] { w } else { -w }).sum(); // cut change if i flips
        if d >= 0 || r.f64() < (beta * d as f64).exp() { x[i] ^= 1; c += d; if c > best { best = c; } }
        flips += 1;
    }
    (best, flips)
}
fn q(mut v: Vec<f64>) -> (f64, f64, f64) { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); let p = |f: f64| v[((v.len() - 1) as f64 * f).round() as usize]; (p(0.5), p(0.25), p(0.75)) }
fn main() {
    let ms: f64 = std::env::var("MS").ok().and_then(|v| v.parse().ok()).unwrap_or(200.0);
    println!("machine: Apple M4 (4P+6E), 4 threads per method, budget {ms} ms, 5 seeds; cut = sum of weights of cut edges");
    let mut r = Philox4x32::new(8080, 1); let n = 800;
    let mut g1 = vec![]; for i in 0..n { for j in i + 1..n { if r.f64() < 0.06 { g1.push((i, j, 1i64)); } } }
    let mut g11 = vec![]; let (h, w) = (20usize, 40usize);
    for a in 0..h { for b in 0..w { let i = a * w + b; for j in [a * w + (b + 1) % w, ((a + 1) % h) * w + b] { g11.push((i.min(j), i.max(j), if r.f64() < 0.5 { 1 } else { -1 })); } } }
    for (name, e) in [("G1-like (ER n=800 p=0.06, w=1)", &g1), ("G11-like (torus 20x40, w=+-1)", &g11)] {
        let mut adj = vec![vec![]; n]; for &(a, b, wt) in e.iter() { adj[a].push((b, wt)); adj[b].push((a, wt)); }
        let prog = |beta: f64| { let pairs: Vec<Pair> = e.iter().map(|&(i, j, wt)| Pair { i, j, c: Coupling::Table(vec![0.0, beta * wt as f64, beta * wt as f64, 0.0]) }).collect();
            Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap() };
        println!("{name}: |E| = {}, sum w = {}", e.len(), e.iter().map(|x| x.2).sum::<i64>());
        for beta in [1.0, 2.0] { let m = prog(beta); let (mut pc, mut pf) = (vec![], vec![]);
            for seed in 0..5u64 { let t0 = Instant::now(); let s = sample(&m, 4, 0, Some(ms), 100 + seed, true, false).unwrap(); let dt = t0.elapsed().as_secs_f64();
                pc.push(cut(e, &s.best.1) as f64); pf.push(s.sweeps as f64 * n as f64 / dt); }
            let (c, cl, ch) = q(pc); let (f, _, _) = q(pf);
            println!("  probbit sample beta={beta}: best cut median {c:.0} [{cl:.0},{ch:.0}], {:.1} M site updates/s (4 chains)", f / 1e6); }
        let (m1, m2) = (prog(1.0), prog(2.0)); let (mut ac, mut bc) = (vec![], vec![]);
        for seed in 0..5u64 { let s = sample(&m2, 4, 0, Some(ms / 2.0), 100 + seed, true, false).unwrap();
            let (lw, x) = anneal(&m1, Some(&s.best.1), &[0.5, 1.0, 2.0, 4.0, 8.0], ms / 2.0, 4, 300 + seed).unwrap(); assert_eq!(lw.round() as i64, cut(e, &x)); ac.push(lw);
            let (lw, _) = anneal(&m1, None, &[0.5, 1.0, 2.0, 4.0, 8.0], ms, 4, 500 + seed).unwrap(); bc.push(lw); }
        let (c, cl, ch) = q(ac); println!("  probbit sample {:.0} ms + anneal {:.0} ms: best cut median {c:.0} [{cl:.0},{ch:.0}]", ms / 2.0, ms / 2.0);
        let (c, cl, ch) = q(bc); println!("  probbit anneal only {ms:.0} ms: best cut median {c:.0} [{cl:.0},{ch:.0}]");
        let (mut sc, mut sf) = (vec![], vec![]);
        for seed in 0..5u64 { let t0 = Instant::now();
            let outs: Vec<(i64, u64)> = std::thread::scope(|scp| { let hs: Vec<_> = (0..4u64).map(|k| { let adj = &adj; scp.spawn(move || sa(adj, ms, 900 + seed, k)) }).collect(); hs.into_iter().map(|h| h.join().unwrap()).collect() });
            let dt = t0.elapsed().as_secs_f64(); sc.push(outs.iter().map(|o| o.0).max().unwrap() as f64); sf.push(outs.iter().map(|o| o.1).sum::<u64>() as f64 / dt); }
        let (c, cl, ch) = q(sc); let (f, _, _) = q(sf);
        println!("  SA (4 restarts): best cut median {c:.0} [{cl:.0},{ch:.0}], {:.1} M flip attempts/s", f / 1e6);
    }
}
