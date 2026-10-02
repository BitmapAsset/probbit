//! Exact-oracle instance family for calibration (moved from examples/accuracy.rs).
//! Chain-of-blocks Dispatch: B blocks x M=10 tasks. Block b may use 2 private agents plus the "bridge"
//! agents shared with blocks b-1 and b+1 (capacity couples neighbours). Groups of 5 within a block, affinity lam.
//! Exact marginals + bridge-load distributions by transfer-matrix DP over bridge loads.
use crate::Problem;
use pbit_core::Philox4x32;

pub const M: usize = 10;
pub struct Inst { pub p: Problem, pub capb: usize, pub nb: usize }
pub fn build(nblocks: usize, capp: usize, capb: usize, lam: f64, seed: u64, pair_swaps: bool) -> Inst {
    let b_ = nblocks; let na = 2 * b_ + (b_ - 1); let t = b_ * M;
    let mut r = Philox4x32::new(seed, 77);
    let mut g = || { let u = r.f64() + 1e-12; let v = r.f64(); (-2.0 * u.ln()).sqrt() * (6.283185307 * v).cos() };
    let mut h = vec![0.0; t * na]; let mut allowed = vec![false; t * na]; let mut cap = vec![capp; na];
    for b in 0..b_ - 1 { cap[2 * b_ + b] = capb; }
    for b in 0..b_ { for k in 0..M { let i = b * M + k;
        let mut el = vec![2 * b, 2 * b + 1]; if b > 0 { el.push(2 * b_ + b - 1); } if b < b_ - 1 { el.push(2 * b_ + b); }
        for &a in &el { allowed[i * na + a] = true; h[i * na + a] = 1.2 * g(); } } }
    let group = (0..t).map(|i| i / 5).collect();
    Inst { p: Problem { t, a: na, h, allowed, cap, group, lam, clamp: vec![None; t], block_moves: false, pair_swaps, collective: false, cluster: false, cycles: false }, capb, nb: b_ }
}
struct Blk { w: Vec<f64>, wm: Vec<f64>, agents: Vec<Option<usize>> }
fn enum_block(ins: &Inst, b: usize) -> Blk {
    let p = &ins.p; let cb = ins.capb + 1; let nb = ins.nb;
    let agents = vec![Some(2 * b), Some(2 * b + 1), if b > 0 { Some(2 * nb + b - 1) } else { None }, if b < nb - 1 { Some(2 * nb + b) } else { None }];
    let mut w = vec![0.0; cb * cb]; let mut wm = vec![0.0; M * 4 * cb * cb];
    let mut x = [0usize; M]; let mut ld = [0usize; 4];
    let off: f64 = (0..M).map(|k| agents.iter().flatten().map(|&a| p.h[(b * M + k) * p.a + a]).fold(f64::NEG_INFINITY, f64::max)).sum::<f64>() + p.lam.max(0.0) * 20.0;
    #[allow(clippy::too_many_arguments)]
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
pub struct ExactOut { pub marg: Vec<f64>, pub bridge: Vec<Vec<f64>> }
pub fn exact_dp(ins: &Inst) -> ExactOut {
    let cb = ins.capb + 1; let c = ins.capb; let nb = ins.nb;
    let blks: Vec<Blk> = (0..nb).map(|b| enum_block(ins, b)).collect();
    let norm = |v: &mut Vec<f64>| { let s: f64 = v.iter().sum(); if s > 0.0 { for x in v.iter_mut() { *x /= s; } } };
    let mut g = vec![vec![0.0; cb]; nb]; let mut f = vec![vec![0.0; cb]; nb];
    g[0][0] = 1.0;
    for b in 0..nb { if b > 0 { for l in 0..cb { g[b][l] = (0..=c - l).map(|r| f[b - 1][r]).sum(); } norm(&mut g[b]); }
        for r in 0..cb { f[b][r] = (0..cb).map(|l| g[b][l] * blks[b].w[l * cb + r]).sum(); } norm(&mut f[b]); }
    let mut hh = vec![vec![0.0; cb]; nb]; hh[nb - 1][0] = 1.0;
    let mut kk = vec![vec![0.0; cb]; nb];
    for b in (0..nb).rev() { if b < nb - 1 { for r in 0..cb { hh[b][r] = (0..=c - r).map(|l| kk[b + 1][l]).sum(); } norm(&mut hh[b]); }
        for l in 0..cb { kk[b][l] = (0..cb).map(|r| blks[b].w[l * cb + r] * hh[b][r]).sum(); } norm(&mut kk[b]); }
    let na = ins.p.a; let mut marg = vec![0.0; ins.p.t * na];
    for b in 0..nb { let blk = &blks[b]; let mut zb = 0.0;
        for l in 0..cb { for r in 0..cb { zb += blk.w[l * cb + r] * g[b][l] * hh[b][r]; } }
        for k in 0..M { for la in 0..4 { let Some(a) = blk.agents[la] else { continue };
            let mut s = 0.0; for l in 0..cb { for r in 0..cb { s += blk.wm[(k * 4 + la) * cb * cb + l * cb + r] * g[b][l] * hh[b][r]; } }
            marg[(b * M + k) * na + a] = s / zb; } } }
    let mut bridge = vec![]; for b in 0..nb - 1 { let mut d = vec![0.0; cb];
        for r in 0..cb { for l in 0..=c - r { d[r + l] += f[b][r] * kk[b + 1][l]; } } norm(&mut d); bridge.push(d); }
    ExactOut { marg, bridge }
}
/// SATURATED (rho = 1 exactly) member of the family. Private agents cap 4, bridges cap 2, and block 0's
/// two private agents cap 5, so total capacity = 8B + 2 + 2(B-1) = 10B = T. Every feasible plan fills every agent, so a
/// plain site update can never move a task (no free slot anywhere): only swap moves mix. Exact by the same DP.
pub fn build_sat(nblocks: usize, lam: f64, seed: u64, pair_swaps: bool) -> Inst {
    let mut ins = build(nblocks, 4, 2, lam, seed, pair_swaps);
    for a in [0, 1] { ins.p.cap[a] = 5; }
    ins
}
/// load factor rho = T / total capacity
pub fn rho(ins: &Inst) -> f64 { ins.p.t as f64 / ins.p.cap.iter().sum::<usize>() as f64 }
/// Exact MAP log-weight (max-product version of `exact_dp`): max over feasible plans of log w.
/// Used to measure the gap between the sampler's best-seen plan and the true optimum (what an ILP/CP solver returns).
pub fn exact_map_logw(ins: &Inst) -> f64 {
    let p = &ins.p; let cb = ins.capb + 1; let c = ins.capb; let nb = ins.nb;
    let tabs: Vec<Vec<f64>> = (0..nb).map(|b| {
        let agents = [Some(2 * b), Some(2 * b + 1), if b > 0 { Some(2 * nb + b - 1) } else { None }, if b < nb - 1 { Some(2 * nb + b) } else { None }];
        let mut w = vec![f64::NEG_INFINITY; cb * cb]; let mut x = [0usize; M]; let mut ld = [0usize; 4];
        #[allow(clippy::too_many_arguments)]
        fn rec(p: &Problem, b: usize, k: usize, lw: f64, x: &mut [usize; M], ld: &mut [usize; 4], ag: &[Option<usize>; 4], w: &mut [f64], cb: usize) {
            if k == M { let i = ld[2] * cb + ld[3]; if lw > w[i] { w[i] = lw; } return; }
            for la in 0..4 { let Some(a) = ag[la] else { continue }; if ld[la] >= p.cap[a] { continue; }
                let mut d = p.h[(b * M + k) * p.a + a]; let g0 = (k / 5) * 5; for j in g0..k { if x[j] == la { d += p.lam; } }
                x[k] = la; ld[la] += 1; rec(p, b, k + 1, lw + d, x, ld, ag, w, cb); ld[la] -= 1; }
        }
        rec(p, b, 0, 0.0, &mut x, &mut ld, &agents, &mut w, cb); w }).collect();
    // f[r] = best log w of blocks 0..=b with block b using r slots of bridge b
    let mut f: Vec<f64> = (0..cb).map(|r| tabs[0][r]).collect();
    for b in 1..nb { let mut g = vec![f64::NEG_INFINITY; cb];
        for r in 0..cb { for l in 0..cb { let prev = (0..=c - l).map(|q| f[q]).fold(f64::NEG_INFINITY, f64::max); let v = prev + tabs[b][l * cb + r]; if v > g[r] { g[r] = v; } } }
        f = g; }
    f[0]
}
