//! Is the false-refusal wall genuine ambiguity (ties) or estimation error? For every escalated ticket of the shipped
//! per-ticket gate, record WHY (run-wide check failed = "unmixed"; own bar too wide = "wide-bar") and whether the EXACT odds
//! are a tie (top-2 gap < TIE). Tie-aware release can recover at most the tie share. env: BLOCKS, NINST, SEED0, BUD, TIE.
use pbit_decide::*; use pbit_decide::oracle::*;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn main() {
    let nbk: usize = env("BLOCKS", 20); let ninst: u64 = env("NINST", 24); let s0: u64 = env("SEED0", 6000); let bud: f64 = env("BUD", 200.0); let tie: f64 = env("TIE", 0.10);
    println!("inst,st,lam,rho,tickets,released,esc_unmixed,esc_widebar,esc_tie,esc_widebar_tie,exact_ties_all");
    for k in s0..s0 + ninst {
        let st = (k % 8) as usize; let seed = 9000 + 131 * k;
        let ins = match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
            3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), 5 => build(nbk, 4, 4, 0.5, seed, false),
            6 => build(8, 3, 5, 2.0, seed, true), _ => build(8, 3, 5, 4.0, seed, true) };
        let (p, na) = (&ins.p, ins.p.a); let ex = exact_frontier(p, 1 << 16).unwrap();
        let gap = |i: usize| { let mut v: Vec<f64> = (0..na).map(|a| ex.marg[i * na + a]).collect(); v.sort_by(|a, b| b.partial_cmp(a).unwrap()); v[0] - v.get(1).copied().unwrap_or(0.0) };
        let s = sample_opts(p, 4, 0, Some(bud), 1 + k, true, false, auto_group_pairs(p)).unwrap(); let g = gate_stats(p, &s); let rel = g.released_tasks(&GATE);
        let long = g.sig_tv_long.iter().cloned().fold(0.0, f64::max);
        let run_ok = g.rhat < PARTIAL_RHAT && g.min_batches >= GATE.min_batches && g.frozen == 0 && long <= BATCH_RATIO_MAX * g.sig_tv_max;
        let (mut nr, mut un, mut wb, mut et, mut wbt, mut at) = (0, 0, 0, 0, 0, 0);
        for i in 0..p.t { let t = gap(i) < tie; if t { at += 1; }
            if rel[i] { nr += 1; continue; } if run_ok { wb += 1; if t { wbt += 1; } } else { un += 1; } if t { et += 1; } }
        println!("{k},{st},{},{:.3},{},{nr},{un},{wb},{et},{wbt},{at}", p.lam, rho(&ins), p.t);
    }
}
