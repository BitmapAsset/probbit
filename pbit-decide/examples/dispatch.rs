//! Dispatch demo: tickets x agents, capacities, skills (legal tickets -> legal agents only),
//! same-customer affinity, per-question logits from a stub judge.  `cargo run --release --example dispatch`
use pbit_decide::*;
use std::time::Instant;

fn plan_str(d: &Dispatch, x: &[usize]) -> String {
    (0..d.p.a).map(|a| { let ts: Vec<String> = (0..d.p.t).filter(|&i| x[i] == a).map(|i| format!("T{:02}", i)).collect(); format!("A{}:{{{}}}", a, ts.join(",")) }).collect::<Vec<_>>().join(" ")
}
fn pct(v: &mut Vec<f64>, q: f64) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f64 * q).round() as usize] }

fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    println!("=== pbit-decide :: Dispatch demo (seed {seed}) ===");
    // ---------- 1. small instance: 8 tickets, 4 agents, cap 2, same-customer pairs ----------
    let d = dispatch(8, 4, 2, 2, 1.0, seed);
    let p = &d.p;
    println!("\n[1] INSTANCE  {} tickets x {} agents, capacity {} each, same-customer affinity lam={}", p.t, p.a, p.cap[0], p.lam);
    println!("    agents : {}", d.agents.join("  "));
    println!("    tickets: {}", d.tickets.join(" "));
    let am = argmax_plan(p);
    println!("\n[2] PER-QUESTION JUDGE (argmax of each ticket's logits, rules ignored)");
    println!("    plan   : {}", plan_str(&d, &am));
    println!("    rule violations: {}  -> {}", p.violations(&am), if p.violations(&am) > 0 { "INFEASIBLE plan, and its 'odds' are meaningless" } else { "feasible (lucky)" });

    let t0 = Instant::now(); let ex = exact(p, 5, 1 << 22).unwrap(); let ems = t0.elapsed().as_secs_f64() * 1e3;
    println!("\n[3] JOINT DECISION (router -> EXACT: {} feasible plans enumerated in {:.2} ms)", ex.n_feasible, ems);
    println!("    MAP plan: {}   violations={}", plan_str(&d, &ex.top[0].1), p.violations(&ex.top[0].1));
    println!("    top-5 plans with probabilities:");
    for (k, (pr, x)) in ex.top.iter().enumerate() { println!("      #{} p={:.4}  {}", k + 1, pr, plan_str(&d, x)); }

    let t0 = Instant::now(); let s = sample(p, 1, 10_000, None, seed, false, false).unwrap(); let gms = t0.elapsed().as_secs_f64() * 1e3;
    let ind = independent(p);
    println!("\n[4] SAMPLER CHECK (constraint-preserving Gibbs, 1 chain, 10k sweeps, forced even though exact is available)");
    println!("    mean TV vs exact = {:.4}  (max {:.4})   time = {:.2} ms   samples with a violation = {}/{}", mean_tv(&s.marg, &ex.marg, p.a), max_tv(&s.marg, &ex.marg, p.a), gms, s.viol, s.n);
    println!("    per-question judge mean TV vs exact joint = {:.4}", mean_tv(&ind, &ex.marg, p.a));

    println!("\n[5] MARGINALS  ticket: P(agent) under the rules  | judge's independent P for the same agent");
    for i in 0..p.t { let best = (0..p.a).max_by(|&u, &v| ex.marg[i * p.a + u].partial_cmp(&ex.marg[i * p.a + v]).unwrap()).unwrap();
        let row: Vec<String> = (0..p.a).map(|a| format!("A{}={:.2}", a, ex.marg[i * p.a + a])).collect();
        println!("    {}  {}  | best A{} judge-says {:.2}", d.tickets[i], row.join(" "), best, ind[i * p.a + best]); }

    // ---------- what-if ----------
    let i = (0..p.t).max_by(|&u, &v| { let e = |q: usize| { let mut m: Vec<f64> = (0..p.a).map(|a| ex.marg[q * p.a + a]).collect(); m.sort_by(|a, b| b.partial_cmp(a).unwrap()); m[1] }; e(u).partial_cmp(&e(v)).unwrap() }).unwrap();
    let mut order: Vec<usize> = (0..p.a).collect(); order.sort_by(|&u, &v| ex.marg[i * p.a + v].partial_cmp(&ex.marg[i * p.a + u]).unwrap());
    let alt = order[1];
    let pc = p.with_clamp(i, alt); let ec = exact(&pc, 5, 1 << 22).unwrap();
    println!("\n[6] WHAT-IF  clamp {} -> A{} (its 2nd choice; prior probability of this scenario = {:.3})", d.tickets[i], alt, ex.marg[i * p.a + alt]);
    println!("    new MAP : {}", plan_str(&d, &ec.top[0].1));
    let moved: Vec<String> = (0..p.t).filter(|&j| j != i && ec.top[0].1[j] != ex.top[0].1[j]).map(|j| format!("{}:A{}->A{}", d.tickets[j], ex.top[0].1[j], ec.top[0].1[j])).collect();
    println!("    knock-on reassignments: {}", if moved.is_empty() { "none".into() } else { moved.join(", ") });
    let shift = (0..p.t).filter(|&j| j != i).map(|j| 0.5 * (0..p.a).map(|a| (ec.marg[j * p.a + a] - ex.marg[j * p.a + a]).abs()).sum::<f64>()).fold(0.0, f64::max);
    println!("    largest marginal shift on another ticket (TV) = {:.3}", shift);

    // ---------- 2. scale: T=200 ----------
    let big = dispatch(200, 10, 22, 4, 0.8, seed);
    println!("\n[7] SCALE  T=200 tickets x 10 agents, cap 22 (91% full), groups of 4, 2000 sweeps/chain");
    let mut st = vec![]; let mut mt = vec![]; let mut last = None;
    for r in 0..11 { let t0 = Instant::now(); let _ = sample(&big.p, 1, 2000, None, seed + r, false, false).unwrap(); st.push(t0.elapsed().as_secs_f64() * 1e3); }
    for r in 0..11 { let dd = decide(&big.p, 0, 2000, seed + 100 + r).unwrap(); mt.push(dd.ms); last = Some(dd); }
    let dd = last.unwrap();
    println!("    latency p50: 1 chain single-thread = {:.1} ms ; 4 chains on 4 threads (+R-hat, plan table) = {:.1} ms", pct(&mut st, 0.5), pct(&mut mt, 0.5));
    let ab = argmax_plan(&big.p);
    println!("    judge argmax plan violations = {} ; pbit-decide best-seen plan violations = {} ; best-seen logw {:.2} (a sample, not the optimum: see polish_plan) vs judge-argmax logw(unconstrained) {:.2}", big.p.violations(&ab), big.p.violations(&dd.map), dd.map_logw, big.p.logw(&ab));
    match dd.verdict { Verdict::DiagnosticsPassed { rhat } => println!("    R-hat-only verdict: DIAGNOSTICS PASSED (split R-hat {:.3} < {})", rhat, RHAT_REFUSE),
        Verdict::Unmixed { rhat } => println!("    R-hat-only verdict: UNMIXED -> escalate (R-hat {:.3})", rhat), Verdict::Exact => {} }
    for sw in [2000usize, 8000] {
        let (gd, g) = decide_gated(&big.p, 0, sw, None, seed + 7, &GATE).unwrap(); let g = g.unwrap();
        let cov = g.released_tasks(&GATE).iter().filter(|&&c| c).count();
        println!("    calibrated gate, {:>4} sweeps/chain ({:.0} ms): error bound {:.3} -> {} ; per-ticket release: {}/{} tickets released, rest escalated", sw, gd.ms, g.tv_bound(&GATE),
            if g.diagnostics_passed(&GATE) { "DIAGNOSTICS PASSED" } else { "whole answer refused" }, cov, big.p.t);
    }
    println!("    top plan empirical frequency = {:.5} over {} samples (at T=200 no single plan has meaningful mass; use marginals/conditionals)", dd.top[0].0, dd.samples);

    // ---------- 3. refusal (calibrated per-marginal error gate) ----------
    let hard = dispatch(60, 3, 20, 20, 5.0, seed);
    println!("\n[8] REFUSAL PATH  60 tickets x 3 agents, cap 20 (100% full), groups of 20 with strong affinity lam=5 (multimodal)");
    println!("    gate = split R-hat < {} AND {} x max_task sigTV <= {} (sigTV = batch-means MCSE of every marginal, pooled-mean, 4 chains)", GATE.rhat_max, GATE.z, GATE.tv_tol);
    for sw in [40usize, 400, 4000] {
        let (dh, g) = decide_gated(&hard.p, 0, sw, None, seed, &GATE).unwrap(); let g = g.unwrap();
        println!("    {:>5} sweeps: R-hat {:.3}, per-ticket error bound {:.3} (worst {}) -> {}", sw, g.rhat, g.tv_bound(&GATE), hard.tickets[g.worst_task],
            match dh.verdict { Verdict::DiagnosticsPassed { .. } => "DIAGNOSTICS PASSED", _ => "UNMIXED: refusing to give odds, escalate to a human / bigger budget" });
    }
    // ---------- 4. the case the old R-hat-only gate got wrong ----------
    use pbit_decide::oracle::{build, exact_dp};
    let ins = build(20, 4, 3, 2.0, 3677, false); let ex = exact_dp(&ins);
    let t0 = Instant::now(); let s = sample(&ins.p, 4, 7300, None, 43, true, false).unwrap(); let ms = t0.elapsed().as_secs_f64() * 1e3; let g = gate_stats(&ins.p, &s);
    println!("\n[9] WHY THE GATE CHANGED  T=200 chain-of-blocks (exact answer known by DP), lam=2, tight shared agents, 4 x 7300 sweeps ({:.0} ms)", ms);
    println!("    old gate (R-hat only): R-hat {:.3} -> {}", g.rhat, if g.rhat < 1.05 { "would PASS" } else { "refuse" });
    println!("    calibrated gate: per-ticket error bound {:.3} > {} -> {}", g.tv_bound(&GATE), GATE.tv_tol, if g.diagnostics_passed(&GATE) { "DIAGNOSTICS PASSED" } else { "REFUSE" });
    println!("    truth (exact DP): mean TV {:.3}, worst ticket off by TV {:.3}  -> refusing was right", mean_tv(&s.marg, &ex.marg, ins.p.a), max_tv(&s.marg, &ex.marg, ins.p.a));
}
