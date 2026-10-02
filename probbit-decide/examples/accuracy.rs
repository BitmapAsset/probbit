//! Risk #1: joint ACCURACY at T=200 against an exact answer.
//! Chain-of-blocks Dispatch: 20 blocks x 10 tasks = 200 tasks. Block b may use 2 private agents plus the
//! "bridge" agents shared with block b-1 and b+1 (capacity couples neighbouring blocks). Groups of 5
//! within a block, lam. Exact marginals + bridge-load distributions by transfer-matrix DP over bridge
//! loads (tree-width bounded by block size); the sampler sees only the flat 200-task problem.
use probbit_decide::*;
use probbit_core::Philox4x32;

const B: usize = 20; const M: usize = 10;
struct Inst { p: Problem, capb: usize }
fn build(capp: usize, capb: usize, lam: f64, seed: u64) -> Inst {
    let na = 2 * B + (B - 1); let t = B * M;
    let mut r = Philox4x32::new(seed, 77);
    #[allow(clippy::approx_constant)] // 6.283185307, not TAU: the published benchmark instances depend on these exact bits
    let mut g = || { let u = r.f64() + 1e-12; let v = r.f64(); (-2.0 * u.ln()).sqrt() * (6.283185307 * v).cos() };
    let mut h = vec![0.0; t * na]; let mut allowed = vec![false; t * na]; let mut cap = vec![capp; na];
    for b in 0..B - 1 { cap[2 * B + b] = capb; }
    for b in 0..B { for k in 0..M { let i = b * M + k;
        let mut el = vec![2 * b, 2 * b + 1]; if b > 0 { el.push(2 * B + b - 1); } if b < B - 1 { el.push(2 * B + b); }
        for &a in &el { allowed[i * na + a] = true; h[i * na + a] = 1.2 * g(); } } }
    let group = (0..t).map(|i| i / 5).collect();
    Inst { p: Problem { t, a: na, h, allowed, cap, group, lam, clamp: vec![None; t], block_moves: std::env::var("BLOCK").is_ok(), pair_swaps: std::env::var("PAIRS").is_ok(), collective: false, cluster: false, cycles: false }, capb }
}
// per-block enumeration: W[l][r], Wm[k][a][l][r] with agents local idx 0,1 private, 2 left bridge, 3 right bridge
struct Blk { w: Vec<f64>, wm: Vec<f64>, agents: Vec<Option<usize>> }
fn enum_block(ins: &Inst, b: usize) -> Blk {
    let p = &ins.p; let cb = ins.capb + 1;
    let agents = vec![Some(2 * b), Some(2 * b + 1), if b > 0 { Some(2 * B + b - 1) } else { None }, if b < B - 1 { Some(2 * B + b) } else { None }];
    let mut w = vec![0.0; cb * cb]; let mut wm = vec![0.0; M * 4 * cb * cb];
    let mut x = [0usize; M]; let mut ld = [0usize; 4];
    // offset: max possible logw to keep exp in range
    let off: f64 = (0..M).map(|k| agents.iter().flatten().map(|&a| p.h[(b * M + k) * p.a + a]).fold(f64::NEG_INFINITY, f64::max)).sum::<f64>() + p.lam.max(0.0) * 20.0;
    fn rec(ins: &Inst, b: usize, k: usize, lw: f64, x: &mut [usize; M], ld: &mut [usize; 4], ag: &[Option<usize>], w: &mut [f64], wm: &mut [f64], off: f64) {
        let p = &ins.p; let cb = ins.capb + 1;
        if k == M { let v = (lw - off).exp(); let idx = ld[2] * cb + ld[3]; w[idx] += v;
            for j in 0..M { wm[(j * 4 + x[j]) * cb * cb + idx] += v; } return; }
        for la in 0..4 { let Some(a) = ag[la] else { continue };
            if ld[la] >= p.cap[a] { continue; }
            let mut d = p.h[(b * M + k) * p.a + a];
            let g0 = (k / 5) * 5; for j in g0..k { if x[j] == la { d += p.lam; } }
            x[k] = la; ld[la] += 1; rec(ins, b, k + 1, lw + d, x, ld, ag, w, wm, off); ld[la] -= 1; }
    }
    rec(ins, b, 0, 0.0, &mut x, &mut ld, &agents, &mut w, &mut wm, off);
    Blk { w, wm, agents }
}
struct ExactOut { marg: Vec<f64>, bridge: Vec<Vec<f64>> }
fn exact_dp(ins: &Inst) -> ExactOut {
    let cb = ins.capb + 1; let c = ins.capb;
    let blks: Vec<Blk> = (0..B).map(|b| enum_block(ins, b)).collect();
    let norm = |v: &mut Vec<f64>| { let s: f64 = v.iter().sum(); if s > 0.0 { for x in v.iter_mut() { *x /= s; } } };
    // f_b(r), g_b(l)
    let mut g = vec![vec![0.0; cb]; B]; let mut f = vec![vec![0.0; cb]; B];
    g[0][0] = 1.0;
    for b in 0..B { if b > 0 { for l in 0..cb { g[b][l] = (0..=c - l).map(|r| f[b - 1][r]).sum(); } norm(&mut g[b]); }
        for r in 0..cb { f[b][r] = (0..cb).map(|l| g[b][l] * blks[b].w[l * cb + r]).sum(); } norm(&mut f[b]); }
    let mut hh = vec![vec![0.0; cb]; B]; hh[B - 1][0] = 1.0;
    let mut kk = vec![vec![0.0; cb]; B]; // k_b(l) = sum_r W_b[l][r] h_b(r)
    for b in (0..B).rev() { if b < B - 1 { for r in 0..cb { hh[b][r] = (0..=c - r).map(|l| kk[b + 1][l]).sum(); } norm(&mut hh[b]); }
        for l in 0..cb { kk[b][l] = (0..cb).map(|r| blks[b].w[l * cb + r] * hh[b][r]).sum(); } norm(&mut kk[b]); }
    let na = ins.p.a; let mut marg = vec![0.0; ins.p.t * na];
    for b in 0..B { let blk = &blks[b]; let mut zb = 0.0;
        for l in 0..cb { for r in 0..cb { zb += blk.w[l * cb + r] * g[b][l] * hh[b][r]; } }
        for k in 0..M { for la in 0..4 { let Some(a) = blk.agents[la] else { continue };
            let mut s = 0.0; for l in 0..cb { for r in 0..cb { s += blk.wm[(k * 4 + la) * cb * cb + l * cb + r] * g[b][l] * hh[b][r]; } }
            marg[(b * M + k) * na + a] = s / zb; } } }
    let mut bridge = vec![]; for b in 0..B - 1 { let mut d = vec![0.0; cb];
        for r in 0..cb { for l in 0..=c - r { d[r + l] += f[b][r] * kk[b + 1][l]; } } norm(&mut d); bridge.push(d); }
    ExactOut { marg, bridge }
}

fn main() {
    println!("block_moves={} pair_swaps={}", std::env::var("BLOCK").is_ok(), std::env::var("PAIRS").is_ok()); println!("chain-of-blocks Dispatch T=200, 59 agents; exact by DP; sampler = 4 chains x budget on 4 threads");
    println!("capP capB tight  lam | budget  sweeps/chain  meanTV   maxTV  bridgeLoadTV  R-hat  chainDis  verdict(R-hat<1.05)  verdict(+chainDis<0.10)");
    for &(cp, cbr, lam) in &[(5usize, 5usize, 0.8f64), (4, 4, 0.8), (4, 3, 0.8), (3, 5, 0.8), (4, 3, 2.0), (4, 3, 4.0), (3, 5, 6.0)] {
        let sd: u64 = std::env::var("SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0); let ins = build(cp, cbr, lam, 1234 + cp as u64 * 10 + cbr as u64 + 1000 * sd);
        let tight = 200.0 / (40.0 * cp as f64 + 19.0 * cbr as f64);
        let t0 = std::time::Instant::now(); let ex = exact_dp(&ins); let dpms = t0.elapsed().as_secs_f64() * 1e3;
        if ex.marg.iter().any(|v| v.is_nan()) { println!("{cp} {cbr} infeasible"); continue; }
        for &bud in &[10.0, 50.0, 200.0] {
            let s = sample(&ins.p, 4, 0, Some(bud), 42, true, false).unwrap();
            // bridge load distribution from the sampler requires per-sample loads: approximate via re-run with plans kept is too heavy;
            // instead compute bridge-load TV from a dedicated single run below.
            let rh = split_rhat(&s.trace);
            let bl = bridge_tv(&ins, &ex, bud);
            let cd = chain_disagreement(&s, ins.p.a);
            println!("{:>4} {:>4} {:>5.2} {:>4.1} | {:>4}ms {:>12} {:>7.4} {:>7.4} {:>12.4} {:>6.3} {:>8.3}  {:>10}  {:>10}", cp, cbr, tight, lam, bud, s.sweeps / 4,
                mean_tv(&s.marg, &ex.marg, ins.p.a), max_tv(&s.marg, &ex.marg, ins.p.a), bl, rh, cd, if rh < RHAT_REFUSE { "diagnostics_passed" } else { "UNMIXED" },
                if rh < RHAT_REFUSE && cd < 0.10 { "diagnostics_passed" } else { "UNMIXED" });
        }
        println!("      (exact DP {:.0} ms)", dpms);
    }
}
// bridge-load (a 20-task joint statistic) TV: 4 chains, budget each, loads histogrammed per sweep
fn bridge_tv(ins: &Inst, ex: &ExactOut, bud: f64) -> f64 {
    let p = &ins.p; let cb = ins.capb + 1;
    let hs: Vec<Vec<Vec<f64>>> = std::thread::scope(|sc| { (0..4).map(|c| sc.spawn(move || {
        let mut ch = probbit_decide::Chain::new(p, 4242, c).unwrap(); let mut hist = vec![vec![0.0; cb]; B - 1];
        let t0 = std::time::Instant::now(); let mut k = 0;
        while t0.elapsed().as_secs_f64() * 1e3 < bud { ch.sweep(); k += 1; if k <= 20 { continue; }
            let mut ld = vec![0usize; B - 1]; for i in 0..p.t { let a = ch.x[i]; if a >= 2 * B { ld[a - 2 * B] += 1; } }
            for b in 0..B - 1 { hist[b][ld[b]] += 1.0; } }
        hist })).collect::<Vec<_>>().into_iter().map(|h| h.join().unwrap()).collect() });
    let mut tot = 0.0; for b in 0..B - 1 { let mut d = vec![0.0; cb]; for h in &hs { for l in 0..cb { d[l] += h[b][l]; } }
        let s: f64 = d.iter().sum(); tot += 0.5 * (0..cb).map(|l| (d[l] / s - ex.bridge[b][l]).abs()).sum::<f64>(); }
    tot / (B - 1) as f64
}
