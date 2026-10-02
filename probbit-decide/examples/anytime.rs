//! Anytime certification (25 ms slices, re-gate, stop at pass or deadline) vs one fixed-budget look,
//! on exact-oracle instances. Does repeated looking inflate the false-certification rate?  env: BLOCKS, NINST, DEADLINE, SLICE
use probbit_decide::*;
use probbit_decide::oracle::*;
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
fn main() {
    let nbk: usize = env("BLOCKS", 20); let ninst: u64 = env("NINST", 36); let dl: f64 = env("DEADLINE", 600.0); let sl: f64 = env("SLICE", 25.0);
    let s0: u64 = env("SEED0", 0);
    println!("inst,setting,mode,cert,t_ms,looks,maxTV,rel,relbad");
    for k in s0..s0 + ninst {
        let st = (k % 6) as usize; let seed = 7000 + 97 * k;
        let ins = match st { 0 => build_sat(nbk, 0.8, seed, true), 1 => build_sat(nbk, 2.0, seed, true), 2 => build(nbk, 4, 3, 0.8, seed, true),
            3 => build(nbk, 4, 3, 2.0, seed, true), 4 => build(nbk, 5, 5, 4.0, seed, true), _ => build(nbk, 4, 4, 0.5, seed, false) };
        let ex = exact_dp(&ins); let na = ins.p.a;
        let tvs = |m: &[f64]| -> Vec<f64> { (0..ins.p.t).map(|i| 0.5 * (0..na).map(|a| (m[i * na + a] - ex.marg[i * na + a]).abs()).sum::<f64>()).collect() };
        let row = |mode: &str, cert: bool, t: f64, looks: usize, m: &[f64], rel: &[bool]| { let tv = tvs(m);
            println!("{k},{st},{mode},{},{t:.1},{looks},{:.5},{},{}", cert as u8, tv.iter().cloned().fold(0.0, f64::max), rel.iter().filter(|&&r| r).count(), rel.iter().zip(&tv).filter(|(&r, &v)| r && v > 0.05).count()); };
        let s = sample(&ins.p, 4, 0, Some(dl), 3 + k, true, false).unwrap(); let g = gate_stats(&ins.p, &s);
        row("fixed", g.diagnostics_passed(&GATE), dl, 1, &s.marg, &g.released_tasks(&GATE));
        for (mode, asp, gr) in [("any", false, 1.0), ("any_geo", false, 1.5)] {
            let a = decide_anytime(&ins.p, dl, sl, gr, 3 + k, &GATE, asp, None).unwrap();
            let c = GateCfg { z: look_z(&GATE, a.looks, asp), ..GATE };
            row(mode, a.passed_at_ms.is_some(), a.decision.ms, a.looks, &a.decision.marg, &a.gate.released_tasks(&c));
        }
    }
}
