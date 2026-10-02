//! Max-cut on random 3-regular graphs, n = 200 / 1000 / 2000. probbit (the IR sampler at fixed inverse temperature beta,
//! 4 chains on 4 threads, best cut among kept samples) vs simulated annealing (single-spin Metropolis, geometric beta
//! 0.2 -> 5 over the budget, 4 independent restarts on 4 threads, best of 4) at EQUAL wall time. 5 seeds, median + IQR.
use probbit_core::Philox4x32;
use probbit_ir::*;
use std::time::Instant;
fn regular3(n: usize, r: &mut Philox4x32) -> Vec<(usize, usize)> {
    loop { let mut st: Vec<usize> = (0..3 * n).map(|q| q / 3).collect();
        for q in (1..st.len()).rev() { let o = r.below(q + 1); st.swap(q, o); }
        let mut e: Vec<(usize, usize)> = st.chunks(2).map(|c| (c[0].min(c[1]), c[0].max(c[1]))).collect(); e.sort();
        if e.iter().all(|&(a, b)| a != b) && e.windows(2).all(|w| w[0] != w[1]) { return e; } }
}
fn cut(e: &[(usize, usize)], x: &[usize]) -> usize { e.iter().filter(|&&(a, b)| x[a] != x[b]).count() }
fn sa(adj: &[Vec<usize>], ms: f64, seed: u64, stream: u64) -> (usize, u64) {
    let n = adj.len(); let mut r = Philox4x32::new(seed, stream); let mut x: Vec<u8> = (0..n).map(|_| (r.f64() < 0.5) as u8).collect();
    let mut c: i64 = adj.iter().enumerate().map(|(i, a)| a.iter().filter(|&&j| j > i && x[j] != x[i]).count() as i64).sum(); let mut best = c;
    let t0 = Instant::now(); let mut flips = 0u64; let (b0, b1) = (0.2f64, 5.0f64); let mut beta = b0;
    loop {
        if flips % 1024 == 0 { let f = t0.elapsed().as_secs_f64() * 1e3 / ms; if f >= 1.0 { break; } beta = b0 * (b1 / b0).powf(f); }
        let i = r.below(n); let same = adj[i].iter().filter(|&&j| x[j] == x[i]).count() as i64; let d = 2 * same - adj[i].len() as i64; // cut change if i flips
        if d >= 0 || r.f64() < (beta * d as f64).exp() { x[i] ^= 1; c += d; if c > best { best = c; } }
        flips += 1;
    }
    (best as usize, flips)
}
fn q(mut v: Vec<f64>) -> (f64, f64, f64) { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); let p = |f: f64| v[((v.len() - 1) as f64 * f).round() as usize]; (p(0.5), p(0.25), p(0.75)) }
fn main() {
    let ms = 200.0;
    println!("machine: Apple M4 (4P+6E), 4 threads per method, budget {ms} ms, 5 seeds; cut = edges cut (|E| = 1.5 n)");
    for n in [200usize, 1000, 2000] {
        let mut r = Philox4x32::new(424242, n as u64); let e = regular3(n, &mut r);
        let mut adj = vec![vec![]; n]; for &(a, b) in &e { adj[a].push(b); adj[b].push(a); }
        for beta in [1.0, 2.0] {
            let pairs: Vec<Pair> = e.iter().map(|&(i, j)| Pair { i, j, c: Coupling::Table(vec![0.0, beta, beta, 0.0]) }).collect();
            let m = Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap();
            let (mut pc, mut pf) = (vec![], vec![]);
            for seed in 0..5u64 { let t0 = Instant::now(); let s = sample(&m, 4, 0, Some(ms), 100 + seed, true, false).unwrap(); let dt = t0.elapsed().as_secs_f64();
                pc.push(cut(&e, &s.best.1) as f64); pf.push(s.sweeps as f64 * n as f64 / dt); }
            let (c, cl, ch) = q(pc); let (f, _, _) = q(pf);
            println!("n={n} probbit beta={beta}: best cut median {c:.0} [{cl:.0},{ch:.0}] ({:.3} of |E|), {:.1} M site updates/s (4 chains)", c / e.len() as f64, f / 1e6);
        }
        { // sample 100 ms at beta 2, then anneal the best plan for 100 ms (betas 0.5 -> 8 on the unit-weight program)
            let pairs: Vec<Pair> = e.iter().map(|&(i, j)| Pair { i, j, c: Coupling::Table(vec![0.0, 1.0, 1.0, 0.0]) }).collect();
            let m1 = Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap(); let m2 = m1.scaled(2.0);
            let (mut ac, mut bc) = (vec![], vec![]);
            for seed in 0..5u64 { let s = sample(&m2, 4, 0, Some(ms / 2.0), 100 + seed, true, false).unwrap();
                let (lw, x) = anneal(&m1, Some(&s.best.1), &[0.5, 1.0, 2.0, 4.0, 8.0], ms / 2.0, 4, 300 + seed).unwrap(); assert_eq!(lw as usize, cut(&e, &x)); ac.push(lw);
                let (lw, _) = anneal(&m1, None, &[0.5, 1.0, 2.0, 4.0, 8.0], ms, 4, 500 + seed).unwrap(); bc.push(lw); }
            let (c, cl, ch) = q(ac); println!("n={n} probbit sample {:.0} ms + anneal {:.0} ms: best cut median {c:.0} [{cl:.0},{ch:.0}] ({:.3} of |E|)", ms / 2.0, ms / 2.0, c / e.len() as f64);
            let (c, cl, ch) = q(bc); println!("n={n} probbit anneal only {ms:.0} ms: best cut median {c:.0} [{cl:.0},{ch:.0}] ({:.3} of |E|)", c / e.len() as f64);
        }
        let (mut sc, mut sf) = (vec![], vec![]);
        for seed in 0..5u64 { let t0 = Instant::now();
            let outs: Vec<(usize, u64)> = std::thread::scope(|scp| { let hs: Vec<_> = (0..4u64).map(|k| { let adj = &adj; scp.spawn(move || sa(adj, ms, 900 + seed, k)) }).collect(); hs.into_iter().map(|h| h.join().unwrap()).collect() });
            let dt = t0.elapsed().as_secs_f64(); sc.push(outs.iter().map(|o| o.0).max().unwrap() as f64); sf.push(outs.iter().map(|o| o.1).sum::<u64>() as f64 / dt); }
        let (c, cl, ch) = q(sc); let (f, _, _) = q(sf);
        println!("n={n} SA (4 restarts): best cut median {c:.0} [{cl:.0},{ch:.0}] ({:.3} of |E|), {:.1} M flip attempts/s", c / e.len() as f64, f / 1e6);
    }
}
