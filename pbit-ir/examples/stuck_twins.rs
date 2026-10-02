//! Kill test for the per-component stuck rule. Each program = part A, a random colouring of G(14, p)
//! with colours 0-2 only (often rigid: its chains cannot move) + part B, a random colouring of G(12, p) with colours 0-3 (mixes);
//! mean degree 3, two clamped vertices per part, soft preferences h ~ U[-0.5, 0.5], k = 4. Odd instances add ONE random edge
//! joining A and B, so B's odds depend on A's colours. Exact odds from the IR's enumeration; sampler 4 chains, MS ms, gate. A false
//! release = a released vertex whose true TV error exceeds the gate's 0.05. env: NINST (default 40), MS (100), SEED (2026).
use pbit_core::Philox4x32;
use pbit_ir::*;
fn main() {
    let env = |k: &str, d: f64| std::env::var(k).ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(d);
    let (ninst, ms, sb) = (env("NINST", 40.0) as u64, env("MS", 100.0), env("SEED", 2026.0) as u64);
    println!("machine: Apple M4 (4P+6E); sampler 4 chains / 4 threads, {ms} ms, gate z=3 tol 0.05; {ninst} programs, seed {sb}");
    println!("inst,link,plans,frozen,verdict,released_a,released_b,false_releases,max_tv_released,max_tv_b_all");
    let (na, nb, k) = (14usize, 12usize, 4usize); let n = na + nb;
    let (mut rel_a, mut rel_b_free, mut rel_b_link, mut fr, mut runs, mut stuck_runs) = (0, 0, 0, 0, 0, 0);
    for inst in 0..ninst {
        let mut r = Philox4x32::new(sb, inst);
        let mut e = vec![];
        for i in 0..na { for j in i + 1..na { if r.f64() < 3.0 / (na - 1) as f64 { e.push((i, j)); } } }
        for i in 0..nb { for j in i + 1..nb { if r.f64() < 3.0 / (nb - 1) as f64 { e.push((na + i, na + j)); } } }
        let link = inst % 2 == 1; if link { let (a, b) = (r.below(na), na + r.below(nb)); e.push((a, b)); }
        let caps: Vec<Cap> = e.iter().flat_map(|&(a, b)| (0..k).map(move |c| Cap { weights: vec![], members: vec![(a, c), (b, c)], limit: 1 })).collect();
        let allowed: Vec<bool> = (0..n * k).map(|q| q / k >= na || q % k < 3).collect();
        let h: Vec<f64> = (0..n * k).map(|_| r.f64() - 0.5).collect();
        let mut clamp = vec![None; n]; clamp[0] = Some(0); clamp[na - 1] = Some(1); clamp[na] = Some(0); clamp[n - 1] = Some(1);
        let mut m = Model::new(n, k, h, allowed, clamp, vec![], caps).unwrap(); m.collective = std::env::var("COLLECTIVE").as_deref() == Ok("1");
        let ex = match exact(&m, 1, 1 << 23) { Some(x) if x.n_feasible > 0 => x, _ => { println!("{inst},{link},skip (infeasible or > 2^23 plans)"); continue } };
        let s = match sample(&m, 4, 0, Some(ms), 40 + inst, true, false) { Some(s) => s, None => { println!("{inst},{link},{},no start", ex.n_feasible); continue } };
        let g = gate_stats(&m, &s); let whole = g.diagnostics_passed(&GATE); let mask = if whole { vec![true; n] } else { g.released_tasks(&GATE) };
        let tv: Vec<f64> = (0..n).map(|i| 0.5 * (0..k).map(|c| (s.marg[i * k + c] - ex.marg[i * k + c]).abs()).sum::<f64>()).collect();
        let (ra, rb) = ((0..na).filter(|&i| mask[i]).count(), (na..n).filter(|&i| mask[i]).count());
        let nf = (0..n).filter(|&i| mask[i] && tv[i] > 0.05).count(); let mx = (0..n).filter(|&i| mask[i]).map(|i| tv[i]).fold(0.0, f64::max);
        let verdict = if whole { "diagnostics_passed" } else if ra + rb > 0 { "partial" } else { "refused" };
        runs += 1; if g.frozen > 0 { stuck_runs += 1; } rel_a += ra; fr += nf; if link { rel_b_link += rb; } else { rel_b_free += rb; }
        println!("{inst},{link},{},{},{verdict},{ra},{rb},{nf},{mx:.4},{:.4}", ex.n_feasible, g.frozen, (na..n).map(|i| tv[i]).fold(0.0, f64::max));
    }
    println!("SUMMARY {runs} programs ({stuck_runs} with frozen > 0): released A {rel_a}, released B {rel_b_free} (unlinked) + {rel_b_link} (linked), FALSE releases {fr}");
}
