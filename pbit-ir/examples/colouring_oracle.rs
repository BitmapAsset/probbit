//! Proper graph colouring as a pbit-ir program (hard rule per edge and colour: at most one endpoint
//! takes it; random soft preferences h in [-0.5, 0.5]; two vertices pre-coloured by clamps). Exact odds from the IR's
//! MRV enumeration (env H scales h); the sampler (4 chains, MS ms) + gate is scored on every variable it RELEASES: a false release is a
//! released vertex whose true TV error exceeds the gate's 0.05 tolerance. Random G(n, p), mean degree 3. env: NINST, MS.
use pbit_core::Philox4x32;
use pbit_ir::*;
fn main() {
    let ninst: u64 = std::env::var("NINST").ok().and_then(|v| v.parse().ok()).unwrap_or(20); let ms: f64 = std::env::var("MS").ok().and_then(|v| v.parse().ok()).unwrap_or(100.0);
    let hs: f64 = std::env::var("H").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0); // field strength scale
    let sb: u64 = std::env::var("SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(777); // graph seed base (fresh sets)
    println!("machine: Apple M4 (4P+6E); sampler 4 chains / 4 threads, {ms} ms, gate z=3 tol 0.05; {ninst} graphs per setting; h scale {hs}");
    println!("n,k,inst,edges,max_deg,colourings,exact_ms,verdict,released,false_releases,max_tv_released,max_tv_all,tv_bound,frozen");
    for &(n, k) in &[(12usize, 4usize), (12, 5), (14, 3)] {
        let (mut rel, mut fr, mut certs, mut refused, mut rel14, mut fr14, mut stuck_runs) = (0, 0, 0, 0, 0, 0, 0);
        for inst in 0..ninst {
            let mut r = Philox4x32::new(sb, (n * 100 + k) as u64 * 1000 + inst);
            let mut e = vec![]; for i in 0..n { for j in i + 1..n { if r.f64() < 3.0 / (n - 1) as f64 { e.push((i, j)); } } }
            let mut deg = vec![0; n]; for &(a, b) in &e { deg[a] += 1; deg[b] += 1; }
            let caps: Vec<Cap> = e.iter().flat_map(|&(a, b)| (0..k).map(move |c| Cap { members: vec![(a, c), (b, c)], limit: 1 })).collect();
            let h: Vec<f64> = (0..n * k).map(|_| (r.f64() - 0.5) * hs).collect(); let mut clamp = vec![None; n]; clamp[0] = Some(0); clamp[n - 1] = Some(1);
            let m = Model::new(n, k, h, vec![true; n * k], clamp, vec![], caps).unwrap();
            let t = std::time::Instant::now(); let ex = match exact(&m, 1, 1 << 23) { Some(x) if x.n_feasible > 0 => x, _ => continue }; let exm = t.elapsed().as_secs_f64() * 1e3;
            let s = sample(&m, 4, 0, Some(ms), 40 + inst, true, false).unwrap(); let g = gate_stats(&m, &s); let whole = g.certified(&GATE);
            let mask = if whole { vec![true; n] } else { g.certified_tasks(&GATE) };
            let tv: Vec<f64> = (0..n).map(|i| 0.5 * (0..k).map(|c| (s.marg[i * k + c] - ex.marg[i * k + c]).abs()).sum::<f64>()).collect();
            let (mut nr, mut nf, mut mx) = (0, 0, 0.0f64); for i in 0..n { if mask[i] { nr += 1; mx = mx.max(tv[i]); if tv[i] > 0.05 { nf += 1; } } }
            // The same run under the earlier all-or-nothing rule (frozen > 0 refused every variable)
            if g.frozen == 0 { rel14 += nr; fr14 += nf; } else { stuck_runs += 1; }
            let verdict = if whole { "certified" } else if nr > 0 { "partial" } else { "refused" }; if whole { certs += 1; } if nr == 0 { refused += 1; }
            rel += nr; fr += nf;
            println!("{n},{k},{inst},{},{},{},{exm:.1},{verdict},{nr},{nf},{mx:.4},{:.4},{:.4},{}", e.len(), deg.iter().max().unwrap(), ex.n_feasible, tv.iter().cloned().fold(0.0, f64::max), g.tv_bound(&GATE), g.frozen);
        }
        println!("SUMMARY n={n} k={k}: certified {certs}, refused {refused}, released {rel} vertices, FALSE releases {fr} | runs with frozen > 0: {stuck_runs}; earlier all-or-nothing rule on the same runs: released {rel14}, FALSE {fr14}");
    }
    println!("START_FALLBACKS (feasible starts that needed the ascending retry): {}", START_FALLBACKS.load(std::sync::atomic::Ordering::Relaxed));
}
