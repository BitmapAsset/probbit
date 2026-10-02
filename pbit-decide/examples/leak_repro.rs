//! Reproduce a per-ticket leak seen on a fresh calibration set (instance 5043: T=200, lam 2, cap 4/3, 50 ms; one released ticket at
//! TV .0533 > .05) and decide hole vs boundary noise. Replicates the instance REPS times (rep 0 = the original run's seed 1 + k) and scores
//! every ticket's error against the gate's own bar: zt = TV / max(sig, sig_long) (released iff 3 * max(..) <= .05).
//! Calibrated bars give P(zt > 3) <= ~.0027 per ticket (1-d normal, two-sided). env: INST, REPS, BUD, SWEEPS, BLOCKS.
use pbit_decide::*; use pbit_decide::oracle::*;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn main() {
    let insts: Vec<u64> = std::env::var("INST").unwrap_or("5043".into()).split(',').map(|v| v.parse().unwrap()).collect();
    let reps: u64 = env("REPS", 100); let bud: f64 = env("BUD", 50.0); let sweeps: usize = env("SWEEPS", 0); let nbk: usize = env("BLOCKS", 20);
    println!("inst,rep,sweeps,rhat,ratio,rel,bad,max_rel_tv,max_rel_zt,n_zt3_all,n_all,worst_task,worst_tv,worst_bar");
    let (mut rel_t, mut bad_t, mut z3_rel, mut z3_all, mut n_all, mut z4_rel) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut zr: Vec<f64> = vec![];
    for &k in &insts {
        let st = (k % 8) as usize; let seed = 9000 + 131 * k;
        let ins = match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
            3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), 5 => build(nbk, 4, 4, 0.5, seed, false),
            6 => build(8, 3, 5, 2.0, seed, true), _ => build(8, 3, 5, 4.0, seed, true) };
        let ex = exact_dp(&ins); let (p, na) = (&ins.p, ins.p.a); let gp = auto_group_pairs(p);
        for r in 0..reps {
            let sd = 1 + k + r * 7919;
            let s = if sweeps > 0 { sample_opts(p, 4, sweeps, None, sd, true, false, gp) } else { sample_opts(p, 4, 0, Some(bud), sd, true, false, gp) }.unwrap();
            let g = gate_stats(p, &s); let zc: f64 = env("Z", GATE.z); let rel = g.released_tasks(&GateCfg { z: zc, ..GATE });
            let long = g.sig_tv_long.iter().cloned().fold(0.0, f64::max);
            let (mut nr, mut nb, mut mrt, mut mrz, mut nz3, mut wt, mut wtv, mut wb) = (0, 0, 0.0f64, 0.0f64, 0, 0, 0.0, 0.0);
            for i in 0..p.t { let tv = 0.5 * (0..na).map(|a| (s.marg[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>();
                let bar = g.sig_tv[i].max(g.sig_tv_long[i]); let zt = tv / bar.max(1e-12);
                if zt > 3.0 { nz3 += 1; } if tv > wtv { wtv = tv; wt = i; wb = bar; }
                if rel[i] { nr += 1; if tv > GATE.tv_tol { nb += 1; } mrt = mrt.max(tv); mrz = mrz.max(zt); zr.push(zt); if zt > 3.0 { z3_rel += 1; } if zt > 4.0 { z4_rel += 1; } } }
            rel_t += nr; bad_t += nb; z3_all += nz3; n_all += p.t;
            println!("{k},{r},{},{:.5},{:.3},{nr},{nb},{mrt:.5},{mrz:.3},{nz3},{},{wt},{wtv:.5},{wb:.5}", s.sweeps / 4, g.rhat, long / g.sig_tv_max, p.t);
        }
    }
    zr.sort_by(|a, b| a.partial_cmp(b).unwrap()); let q = |f: f64| if zr.is_empty() { f64::NAN } else { zr[((zr.len() - 1) as f64 * f) as usize] };
    eprintln!("SUMMARY released {rel_t} bad {bad_t} (rate {:.2e}); released zt>3: {z3_rel} ({:.2e}), zt>4: {z4_rel}; all tickets zt>3: {z3_all}/{n_all} ({:.2e}); released zt q50 {:.2} q99 {:.2} q999 {:.2} max {:.2}",
        bad_t as f64 / rel_t.max(1) as f64, z3_rel as f64 / rel_t.max(1) as f64, z3_all as f64 / n_all.max(1) as f64, q(0.5), q(0.99), q(0.999), q(1.0));
}
