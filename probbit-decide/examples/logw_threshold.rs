//! R19.8 (P2.3 item 3): cost of `Problem::logw`'s two bit-identical paths (pairwise loop vs linear map) at the same task
//! counts, to place `LOGW_PAIRWISE_MAX`. Per (t, group size): 7 rounds, paths alternating, each round = calls on 64 random
//! plans; prints the median ns per call per path. Run: cargo run --release -p probbit-decide --example logw_threshold
use probbit_decide::*;
fn main() {
    println!("   t  gs | pairwise ns  linear ns  linear/pairwise");
    for &gs in &[4usize, 8] { for &t in &[64usize, 96, 128, 160, 192, 256, 320] {
        let p = dispatch(t, 6, t, gs, 0.37, 7).p; let mut r = 7u64;
        let xs: Vec<Vec<usize>> = (0..64).map(|_| (0..t).map(|_| { r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (r >> 33) as usize % 6 }).collect()).collect();
        let reps = (2_000_000 / (t * t)).max(20);
        let (mut a, mut b) = (vec![], vec![]); let mut sink = 0.0;
        for round in 0..14 { let pw = round % 2 == 0; let t0 = std::time::Instant::now();
            for _ in 0..reps { for x in &xs { sink += p.logw_by(x, pw); } }
            let ns = t0.elapsed().as_secs_f64() * 1e9 / (reps * xs.len()) as f64; if pw { a.push(ns) } else { b.push(ns) } }
        let med = |v: &mut Vec<f64>| { v.sort_by(|x, y| x.partial_cmp(y).unwrap()); v[v.len() / 2] };
        let (ma, mb) = (med(&mut a), med(&mut b));
        println!("{t:>4} {gs:>3} | {ma:>11.1} {mb:>10.1}  {:>15.2}   (sink {:.0})", mb / ma, sink % 10.0);
    } }
}
