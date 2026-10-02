//! AI-agent task router (gated sampled odds): a queue of agent tasks x workers (models + a human queue), hard policy rules
//! (PII stays on-prem or with humans, prod-DB migrations never go to cheap models), per-hour quotas, same-customer
//! context affinity, stub-judge logits.  `cargo run --release --example agent_router`
use pbit_core::Philox4x32;
use pbit_decide::*;
use std::time::Instant;

const W: [&str; 6] = ["opus", "sonnet", "luna-pro", "local-gemma", "codex", "human"];
const CUST: [&str; 8] = ["acme", "globex", "initech", "umbrella", "hooli", "stark", "wayne", "wonka"];
// (template, PII, production DB, judge fit per worker: opus sonnet luna gemma codex human)
const TPL: [(&str, bool, bool, [f64; 6]); 8] = [
    ("draft reply: refund dispute (PII)", true, false, [2.0, 1.6, 1.0, 0.6, -1.0, 1.2]),
    ("summarize 40-page contract (PII)", true, false, [2.2, 1.8, 0.8, 0.9, -1.0, 0.5]),
    ("refactor auth middleware", false, false, [1.6, 1.2, 0.0, -0.5, 2.0, -1.0]),
    ("prod DB migration review", false, true, [1.8, 1.0, 1.2, 0.0, 1.5, 1.0]),
    ("weekly SEO report", false, false, [0.5, 1.0, 1.6, 0.5, -1.0, -1.5]),
    ("triage support inbox", false, false, [0.3, 0.8, 1.5, 1.0, -1.0, 0.0]),
    ("fix flaky CI test", false, false, [1.0, 0.8, 0.2, -0.5, 1.8, -2.0]),
    ("investor update draft", false, false, [1.8, 1.4, 0.6, 0.2, -1.0, 0.8]),
];

struct Queue { p: Problem, name: Vec<String>, tpl: Vec<usize> }

/// task i = template i%8 for customer (i/3)%8; every 3 consecutive tasks are one customer workflow (affinity group)
fn queue(n: usize, cap: [usize; 6], lam: f64, seed: u64) -> Queue {
    let mut r = Philox4x32::new(seed, 4242);
    #[allow(clippy::approx_constant)] // 6.283185307, not TAU: the published benchmark instances depend on these exact bits
    let mut g = || { let u = r.f64() + 1e-12; let v = r.f64(); (-2.0 * u.ln()).sqrt() * (6.283185307 * v).cos() };
    let a = W.len(); let (mut h, mut allowed, mut name, mut tpl) = (vec![0.0; n * a], vec![true; n * a], vec![], vec![]);
    for i in 0..n {
        let k = i % TPL.len(); let (tn, pii, prod, fit) = TPL[k];
        name.push(format!("T{:03} {:<8} {}", i, CUST[(i / 3) % CUST.len()], tn)); tpl.push(k);
        for w in 0..a {
            h[i * a + w] = fit[w] + 0.7 * g();
            if pii && w != 3 && w != 5 { allowed[i * a + w] = false; } // PII: local-gemma or human only
            if prod && (w == 2 || w == 3) { allowed[i * a + w] = false; } // prod DB: no cheap models
        }
    }
    let group = (0..n).map(|i| i / 3).collect();
    Queue { p: Problem { t: n, a, h, allowed, cap: cap.to_vec(), group, lam, clamp: vec![None; n], block_moves: false, pair_swaps: false, collective: false, cluster: false, cycles: false }, name, tpl }
}

fn top2(m: &[f64], i: usize, a: usize) -> (usize, usize) {
    let mut o: Vec<usize> = (0..a).collect(); o.sort_by(|&u, &v| m[i * a + v].partial_cmp(&m[i * a + u]).unwrap()); (o[0], o[1])
}
fn tv(x: &[f64], y: &[f64], i: usize, a: usize) -> f64 { 0.5 * (0..a).map(|k| (x[i * a + k] - y[i * a + k]).abs()).sum::<f64>() }

/// naive-router audit: PII leaks, prod-DB on cheap models, quota overflow per worker
fn audit(q: &Queue, x: &[usize]) -> String {
    let p = &q.p; let mut load = [0usize; 6]; let (mut pii, mut prod) = (0, 0);
    for i in 0..p.t { load[x[i]] += 1; if !p.allowed[i * p.a + x[i]] { if TPL[q.tpl[i]].1 { pii += 1 } else { prod += 1 } } }
    let over: Vec<String> = (0..p.a).filter(|&w| load[w] > p.cap[w]).map(|w| format!("{} over quota by {}", W[w], load[w] - p.cap[w])).collect();
    format!("{} PII tasks sent to cloud models, {} prod-DB migrations sent to cheap models, {}  (total violations {})",
        pii, prod, if over.is_empty() { "no quota overflow".into() } else { over.join(", ") }, p.violations(x))
}

fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    let t_all = Instant::now();
    println!("=== pbit-decide :: AI-agent task router (agent-router demo, seed {seed}) ===");
    let sq = queue(12, [2, 2, 3, 2, 2, 3], 0.8, seed); let p = &sq.p; let a = p.a;
    println!("\n[1] QUEUE  {} tasks -> {} workers, quotas (tasks/hour): {}", p.t, a,
        (0..a).map(|w| format!("{}={}", W[w], p.cap[w])).collect::<Vec<_>>().join(" "));
    println!("    rules: PII tasks only on local-gemma or human; prod-DB migrations never on luna-pro/local-gemma;");
    println!("           quotas are hard; tasks of one customer workflow prefer one worker (context reuse, bonus lam={})", p.lam);
    for i in 0..p.t { println!("    {}", sq.name[i]); }

    let am = argmax_plan(p);
    println!("\n[2] BASELINE  naive LLM router = each task's judge argmax, rules ignored");
    println!("    {}", audit(&sq, &am));

    let t0 = Instant::now(); let ex = exact(p, 5, 5_000_000).expect("feasible set too large for exact"); let ems = t0.elapsed().as_secs_f64() * 1e3;
    let best = &ex.top[0].1;
    println!("\n[3] EXACT JOINT DECISION  all {} rule-abiding plans enumerated in {:.0} ms", ex.n_feasible, ems);
    println!("    per task: worker in the single most likely plan | that task's two most likely workers across ALL plans");
    println!("    best plan probability {:.4} (next: {:.4}, {:.4}), violations {}", ex.top[0].0, ex.top[1].0, ex.top[2].0, p.violations(best));
    for i in 0..p.t { let (b, s) = top2(&ex.marg, i, a);
        println!("    {:<48} plan: {:<11} | odds {} {:.2}, {} {:.2} | naive said {}", sq.name[i], W[best[i]], W[b], ex.marg[i * a + b], W[s], ex.marg[i * a + s], W[am[i]]); }
    let ci = (0..p.t).find(|&i| best[i] == 3).unwrap_or(0);
    let ec = exact(&p.with_clamp(ci, 5), 5, 5_000_000).unwrap();
    println!("    WHAT-IF: customer demands a human on {} (prior P = {:.3})", sq.name[ci], ex.marg[ci * a + 5]);
    let moved: Vec<String> = (0..p.t).filter(|&j| j != ci && ec.top[0].1[j] != best[j]).map(|j| format!("T{:03} {}->{}", j, W[best[j]], W[ec.top[0].1[j]])).collect();
    println!("      knock-on reassignments: {}  (violations {})", if moved.is_empty() { "none".into() } else { moved.join(", ") }, p.with_clamp(ci, 5).violations(&ec.top[0].1));

    let (d4, g4) = decide_gated(p, 0, 0, Some(50.0), seed, &GATE).unwrap(); let g4 = g4.unwrap();
    let rel = g4.released_tasks(&GATE); let nrel = rel.iter().filter(|&&c| c).count();
    let tvr = (0..p.t).filter(|&i| rel[i]).map(|i| tv(&d4.marg, &ex.marg, i, a)).fold(0.0, f64::max);
    let agree = (0..p.t).filter(|&i| rel[i] && top2(&d4.marg, i, a).0 == top2(&ex.marg, i, a).0).count();
    println!("\n[4] HONESTY CHECK  same 12 tasks, exact switched off, 4-chain sampler + gate, 50 ms budget ({} samples, {:.0} ms)", d4.samples, d4.ms);
    println!("    released {}/{} tasks individually (R-hat {:.4}); max TV vs exact over released tasks = {:.4} (gate tolerance {})",
        nrel, p.t, g4.rhat, tvr, GATE.tv_tol);
    println!("    released tasks whose top worker matches exact: {}/{} ; worst TV over ALL tasks = {:.4}", agree, nrel, max_tv(&d4.marg, &ex.marg, a));

    let bq = queue(300, [30, 55, 95, 50, 55, 30], 1.2, seed); let bp = &bq.p;
    let tot: usize = bp.cap.iter().sum();
    println!("\n[5] REAL-SIZE QUEUE  {} tasks, {} workers, {} quota slots ({:.0}% full), {} customer workflows of 3", bp.t, bp.a, tot, 100.0 * bp.t as f64 / tot as f64, bp.t / 3);
    println!("    BASELINE naive router: {}", audit(&bq, &argmax_plan(bp)));
    let mut runs = vec![];
    for b in [25.0, 200.0] {
        let (d, g) = decide_gated(bp, 0, 0, Some(b), seed, &GATE).unwrap(); let g = g.unwrap();
        let rel = g.released_tasks(&GATE); let n = rel.iter().filter(|&&c| c).count();
        println!("    budget {:>3.0} ms ({} samples): whole-answer error bound {:.3} -> {} ; R-hat {:.4} ; released {}/{} tasks, escalated {} ; best plan violations {}",
            b, d.samples, g.tv_bound(&GATE), if g.diagnostics_passed(&GATE) { "DIAGNOSTICS PASSED" } else { "whole answer: diagnostics NOT passed" }, g.rhat, n, bp.t, bp.t - n, bp.violations(&d.map));
        runs.push((b, d, g, rel));
    }
    let a = decide_anytime(bp, 500.0, 25.0, 1.0, seed, &GATE, false, None).unwrap();
    println!("    ANYTIME (sample in 25 ms slices, stop when the whole answer passes the diagnostics, deadline 500 ms): {} after {:.0} ms ({} looks)",
        if a.passed_at_ms.is_some() { "DIAGNOSTICS PASSED" } else { "diagnostics not passed by the deadline" }, a.decision.ms, a.looks);
    // the 5 hardest tasks = largest error bar at 200 ms; show both budgets so the escalation reason is visible
    let (d, g, rel) = (&runs[1].1, &runs[1].2, &runs[1].3); let (g0, rel0) = (&runs[0].2, &runs[0].3);
    let mut hard: Vec<usize> = (0..bp.t).collect(); hard.sort_by(|&u, &v| g.sig_tv[v].partial_cmp(&g.sig_tv[u]).unwrap());
    println!("    5 hardest tasks (error bar = 3 x sigma_TV, gate needs <= {}; odds from the 200 ms run):", GATE.tv_tol);
    for &i in hard.iter().take(5) { let (b1, b2) = top2(&d.marg, i, bp.a); let st = |r: bool| if r { "released" } else { "ESCALATED" };
        let bar = |g: &Gate| GATE.z * g.sig_tv[i].max(g.sig_tv_long[i]);
        println!("      {:<48} 25ms +/-{:.3} {:<9} | 200ms +/-{:.3} {:<9} | {} {:.2} vs {} {:.2}", bq.name[i], bar(g0), st(rel0[i]),
            bar(g), st(rel[i]), W[b1], d.marg[i * bp.a + b1], W[b2], d.marg[i * bp.a + b2]); }

    // ---------- real-size PROOF: a 300-task queue whose exact answer is computable (pod structure -> exact DP) ----------
    use pbit_decide::oracle::{build, exact_dp, exact_map_logw};
    let pods = build(30, 4, 3, 2.0, 1000 + seed, true); let pe = exact_dp(&pods); let pp = &pods.p;
    println!("\n[6] REAL-SIZE PROOF  {} tasks in 30 customer pods: each pod has 2 dedicated agents (quota 4) and shares one specialist", pp.t);
    println!("    (quota 3) with the next pod; workflows of 5 prefer one worker (lam {}). This shape has an EXACT answer (transfer-matrix DP),", pp.lam);
    println!("    so every released task can be checked. {} workers, {:.0}% full.", pp.a, 100.0 * pp.t as f64 / pp.cap.iter().sum::<usize>() as f64);
    for b in [200.0, 1000.0] {
        let (d, g) = decide_gated(pp, 0, 0, Some(b), seed, &GATE).unwrap(); let g = g.unwrap(); let rel = g.released_tasks(&GATE);
        let n = rel.iter().filter(|&&r| r).count(); let bad = (0..pp.t).filter(|&i| rel[i] && tv(&d.marg, &pe.marg, i, pp.a) > GATE.tv_tol).count();
        let worst = (0..pp.t).filter(|&i| rel[i]).map(|i| tv(&d.marg, &pe.marg, i, pp.a)).fold(0.0, f64::max);
        let ratio = g.sig_tv_long.iter().cloned().fold(0.0, f64::max) / g.sig_tv_max;
        println!("    {:>4.0} ms: released {}/{} tasks, escalated {} ; released tasks off by more than {} vs exact: {} (worst released error {:.3}) ; plan violations {}",
            b, n, pp.t, pp.t - n, GATE.tv_tol, bad, worst, pp.violations(&d.map));
        println!("             run-wide checks: R-hat {:.4} (release needs < {}), batch-stability ratio {:.2} (needs <= {}), frozen specialists {}",
            g.rhat, PARTIAL_RHAT, ratio, BATCH_RATIO_MAX, g.frozen);
        if b == 1000.0 { let opt = exact_map_logw(&pods); let t0 = Instant::now(); let pol = polish_plan(pp, Some(&d.map), 100.0, seed).unwrap();
            println!("    plan quality: best sampled plan is {:.1} nats below the proven optimum (exact max-product DP); after a {:.0} ms polish: {:.2} nats (violations {})",
                opt - d.map_logw, t0.elapsed().as_secs_f64() * 1e3, (opt - pol.0).max(0.0), pp.violations(&pol.1)); }
    }
    // ---------- the trap: where a sampler looks converged but is wrong ----------
    let trap = build(8, 3, 5, 4.0, 4250, true); let te = exact_dp(&trap); let tp = &trap.p;
    let s = sample_opts(tp, 4, 0, Some(1000.0), 9, true, false, true).unwrap(); let g = gate_stats(tp, &s);
    let r3 = g.rhat < 1.05 && 3.0 * g.sig_tv_max <= 0.05 && g.min_batches >= 8;
    println!("\n[7] THE TRAP  {} tasks, every shared specialist is full in every plan, workflows of 5 but dedicated quota 3 (lam {})", tp.t, tp.lam);
    println!("    4 chains agree: R-hat {:.3}, naive error bar {:.3} -> the earlier gate would say: {}", g.rhat, 3.0 * g.sig_tv_max, if r3 { "DIAGNOSTICS PASSED" } else { "refuse" });
    println!("    truth (exact DP): worst task off by {:.2}. The current gate: frozen full specialists = {}, error bar {:.3} -> {}",
        max_tv(&s.marg, &te.marg, tp.a), g.frozen, g.tv_bound(&GATE), if g.diagnostics_passed(&GATE) { "DIAGNOSTICS PASSED" } else { "REFUSED: escalate the whole queue (the frozen-capacity check catches this trap)" });

    println!("\n[8] WHAT THIS PROVES / WHAT IT DOES NOT");
    println!("    Proves: given a judge's scores, the router returns only rule-abiding plans (0 PII leaks, 0 quota overflow),");
    println!("    its per-task odds match exact enumeration where exact is possible, and at real size it releases only the tasks");
    println!("    whose own error bar passes the gate, escalating tasks whose odds are not yet pinned down (the two-way splits); the gate is a heuristic with published counterexamples (README: Known failure modes).");
    println!("    Does NOT prove: that the judge scores are good (here they are a seeded stub: template fit + Gaussian noise),");
    println!("    anything about real production traffic, or real-world latency/cost savings. Sampler budgets are wall-clock, so");
    println!("    counts in [4]-[7] can differ slightly run to run.   total runtime {:.1} s", t_all.elapsed().as_secs_f64());
}
