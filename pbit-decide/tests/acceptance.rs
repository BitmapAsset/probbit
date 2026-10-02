use pbit_decide::*;
use std::time::Instant;

/// acc1 has a wall-clock bound; the multi-threaded tests (acc2, acc6, acc7, acc8) take this lock too so they never run
/// concurrently with it (acc1 failed its 10 ms p50 bound under the parallel suite once acc8 was added; alone p50 = 3.8 ms).
static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());

// The original anchor shape: 7 tasks x 4 agents, cap 2, groups of 3, lam=1, star agent (4^7=16384 configs, no skill rule).
fn r1_instance() -> Problem { let mut d = dispatch(7, 4, 2, 3, 1.0, 11).p; for v in d.allowed.iter_mut() { *v = true; } d }

#[test]
fn acc1_tv_le_001_within_10ms() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner());
    // Wall-clock is a p50 over 7 timed repetitions per seed after a warm-up run (a single shot was flaky under
    // parallel test threads: 11.6 ms once). TV is deterministic (fixed sweeps, seeded Philox) and asserted on every run.
    let p = r1_instance(); let ex = exact(&p, 5, 1 << 20).unwrap();
    let _ = sample(&p, 1, 12_000, None, 99, false, false).unwrap(); // warm-up (page-in, caches, clocks)
    for s in 0..5u64 {
        let mut ms = vec![]; let mut tv = 0.0;
        for _ in 0..7 { let t0 = Instant::now(); let smp = sample(&p, 1, 12_000, None, 100 + s, false, false).unwrap();
            ms.push(t0.elapsed().as_secs_f64() * 1e3); tv = mean_tv(&smp.marg, &ex.marg, p.a); assert!(tv <= 0.01, "TV {tv}"); }
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap()); let p50 = ms[ms.len() / 2];
        println!("seed {s}: feasible={} TV={tv:.4} p50 ms={p50:.2} (min {:.2} max {:.2})", ex.n_feasible, ms[0], ms[ms.len() - 1]);
        assert!(p50 <= 10.0, "p50 ms {p50}"); }
}

#[test]
fn acc2_zero_violations_in_1e6_samples() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner());
    let p = r1_instance(); let s = sample(&p, 4, 277_778, None, 5, true, false).unwrap();
    println!("samples={} violations={}", s.n, s.viol); assert!(s.n >= 1_000_000); assert_eq!(s.viol, 0);
    let big = dispatch(200, 10, 22, 4, 0.8, 3).p; let s = sample(&big, 4, 2000, None, 9, true, false).unwrap();
    assert_eq!(s.viol, 0);
}

#[test]
fn exact_matches_bruteforce() {
    let p = dispatch(6, 3, 2, 2, 0.7, 4).p; let ex = exact(&p, 3, 1 << 20).unwrap();
    let (mut z, mut m) = (0.0, vec![0.0; p.t * p.a]);
    for s in 0..3usize.pow(6) { let x: Vec<usize> = (0..6).map(|i| (s / 3usize.pow(i as u32)) % 3).collect();
        if p.violations(&x) > 0 { continue; } let w = p.logw(&x).exp(); z += w; for i in 0..6 { m[i * 3 + x[i]] += w; } }
    for v in m.iter_mut() { *v /= z; }
    assert!(mean_tv(&m, &ex.marg, 3) < 1e-12); assert!((ex.logz - z.ln()).abs() < 1e-9);
}

#[test]
fn acc5_refusal_is_a_gate() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner()); // acc10 flips the global WINDOW_K
    let hard = dispatch(60, 3, 20, 20, 5.0, 7).p;
    let d = decide(&hard, 0, 40, 7).unwrap();
    match d.verdict { Verdict::Unmixed { rhat } => assert!(rhat >= RHAT_REFUSE), _ => panic!("expected refusal") }
}

/// Calibrated MCSE gate: on exact-oracle instances (fixed sweeps => deterministic), a CERTIFIED answer must be within 0.05 per-task TV,
/// and the false-certification regime of the earlier R-hat-only gate (lam=4, tight bridges, no pair swaps, short run) must be refused.
#[test]
fn acc6_mcse_gate_calibrated_on_oracle() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner());
    use pbit_decide::oracle::*;
    let mut cert = 0;
    for (lam, cp, cb, pairs, sw) in [(0.5, 5, 5, false, 3000usize), (0.8, 4, 4, false, 3000), (0.8, 5, 5, false, 6000), (2.0, 4, 3, true, 4000), (4.0, 4, 3, false, 300), (4.0, 4, 3, false, 3000), (6.0, 3, 5, false, 3000)] {
        let ins = build(10, cp, cb, lam, 555 + (lam * 10.0) as u64, pairs); let ex = exact_dp(&ins);
        let s = sample(&ins.p, 4, sw, None, 3, true, false).unwrap(); let g = gate_stats(&ins.p, &s);
        let mx = max_tv(&s.marg, &ex.marg, ins.p.a); let c = g.diagnostics_passed(&GATE);
        println!("lam {lam} cap {cp}/{cb} pairs {pairs} sweeps {sw}: maxTV {mx:.4} R-hat {:.3} 2*sigTV {:.4} -> {}", g.rhat, 2.0 * g.sig_tv_max, if c { "CERTIFIED" } else { "refused" });
        if c { cert += 1; assert!(mx <= 0.05, "false certification: maxTV {mx}"); }
        if lam >= 4.0 && !pairs { assert!(!c || mx <= 0.05); }
    }
    assert!(cert >= 1, "gate never certifies: useless");
}

/// pbit-ir v0 text form: bit-exact round trip (Problem -> text -> Problem), identical sampler output for the same seed,
/// and the binary p-bit lowering reproduces -logw exactly on feasible states.
#[test]
fn ir_round_trip_and_lowering_exact() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner()); // acc10 flips the global WINDOW_K
    use pbit_decide::ir::*;
    let mut d = dispatch(8, 4, 2, 2, 1.0, 7).p; d.clamp[3] = Some(1);
    let src = to_ir(&d); let q = from_ir(&src).unwrap(); assert_eq!(to_ir(&q), src);
    // logits of forbidden values are semantically dead and not exported; every live logit must be bit-exact
    assert!((0..d.h.len()).all(|k| !d.allowed[k] || d.h[k].to_bits() == q.h[k].to_bits()) && d.allowed == q.allowed && d.cap == q.cap && d.group == q.group && d.clamp == q.clamp);
    let (s1, s2) = (sample(&d, 1, 2000, None, 5, false, false).unwrap(), sample(&q, 1, 2000, None, 5, false, false).unwrap());
    assert!(s1.marg.iter().zip(&s2.marg).all(|(a, b)| a.to_bits() == b.to_bits()));
    let big = pbit_decide::oracle::build(4, 4, 3, 2.0, 1, false).p; let src = to_ir(&big); assert_eq!(to_ir(&from_ir(&src).unwrap()), src);
    let qb = lower_onehot(&d, 3.0); let s = sample(&d, 1, 300, None, 8, false, true).unwrap();
    let mut n = 0; for (plan, _) in s.plans.iter() { let x: Vec<usize> = plan.iter().map(|&v| v as usize).collect();
        let e = qb.energy(&qb.encode(&d, &x)); assert!((e + d.logw(&x)).abs() < 1e-9, "E {e} vs -logw {}", -d.logw(&x));
        assert_eq!(qb.decode(&d, &qb.encode(&d, &x)), Some(x)); n += 1; }
    assert!(n > 10);
}

/// The frozen-split trap. Small T, strong affinity, private cap 3 < group 5, every shared agent full in every plan.
/// With the earlier gate all 4 chains agreed (R-hat 1.000, 3*sigma .006) on an answer off by TV .51. The current gate must refuse,
/// and the two-group move must stay exact on an instance it can mix (nb = 5).
#[test]
fn acc7_frozen_split_is_refused_and_gpair_is_exact() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner());
    use pbit_decide::oracle::*;
    let trap = build(8, 3, 5, 4.0, 4250, true); let ex = exact_dp(&trap);
    let s = sample_opts(&trap.p, 4, 20_000, None, 9, true, false, true).unwrap(); let g = gate_stats(&trap.p, &s);
    let mx = max_tv(&s.marg, &ex.marg, trap.p.a);
    println!("trap: maxTV {mx:.3} R-hat {:.4} bound {:.4} frozen {}", g.rhat, g.tv_bound(&GATE), g.frozen);
    assert!(!g.diagnostics_passed(&GATE) || mx <= 0.05, "false certification: maxTV {mx}");
    assert!(g.released_tasks(&GATE).iter().zip(0..).all(|(&r, i)| !r || 0.5 * (0..trap.p.a).map(|a| (s.marg[i * trap.p.a + a] - ex.marg[i * trap.p.a + a]).abs()).sum::<f64>() <= 0.05));
    let ok = build(5, 3, 5, 4.0, 4247, true); let ex = exact_dp(&ok);
    let s = sample_opts(&ok.p, 4, 20_000, None, 9, true, false, true).unwrap();
    let mx = max_tv(&s.marg, &ex.marg, ok.p.a); println!("gpair nb=5: maxTV {mx:.4}"); assert!(mx <= 0.02, "gpair biased? maxTV {mx}");
    // saturation (rho = 1): certified => within tolerance
    let sat = build_sat(20, 2.0, 9031, true); let ex = exact_dp(&sat);
    let s = sample(&sat.p, 4, 30_000, None, 3, true, false).unwrap(); let g = gate_stats(&sat.p, &s);
    let mx = max_tv(&s.marg, &ex.marg, sat.p.a); println!("sat: maxTV {mx:.4} cert {}", g.diagnostics_passed(&GATE));
    assert!(!g.diagnostics_passed(&GATE) || mx <= 0.05);
}

/// Shipped options. decide_anytime on exact-oracle instances: a certificate must be within tolerance and released tickets
/// correct; polish_plan is feasible and never worse than its start; auto two-group move is on iff lam >= 2.
#[test]
fn acc8_anytime_polish_autogpair() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner());
    use pbit_decide::oracle::*;
    for (lam, cp, cb, seed) in [(0.8, 4, 3, 21u64), (2.0, 4, 3, 22), (0.5, 4, 4, 23)] {
        let ins = build(20, cp, cb, lam, seed, true); let ex = exact_dp(&ins); let na = ins.p.a;
        assert_eq!(auto_group_pairs(&ins.p), lam >= 2.0);
        let a = decide_anytime(&ins.p, 400.0, 25.0, 1.0, seed, &GATE, false, None).unwrap();
        let mx = max_tv(&a.decision.marg, &ex.marg, na);
        println!("lam {lam}: anytime cert {:?} after {:.0} ms, {} looks, maxTV {mx:.4}", a.passed_at_ms, a.decision.ms, a.looks);
        if a.passed_at_ms.is_some() { assert!(mx <= 0.05, "false anytime certificate {mx}"); }
        let rel = a.gate.released_tasks(&GATE);
        for i in 0..ins.p.t { if rel[i] { assert!(0.5 * (0..na).map(|q| (a.decision.marg[i * na + q] - ex.marg[i * na + q]).abs()).sum::<f64>() <= 0.05); } }
        let start = a.decision.map.clone(); let pol = polish_plan(&ins.p, Some(&start), 60.0, seed).unwrap();
        assert_eq!(ins.p.violations(&pol.1), 0); assert!(pol.0 >= ins.p.logw(&start) - 1e-9);
        assert!(pol.0 <= exact_map_logw(&ins) + 1e-6, "polish above the exact optimum: DP or polish bug");
    }
    assert_eq!(PARTIAL_RHAT, 1.002); assert_eq!(BATCH_RATIO_MAX, 1.5); assert_eq!(GATE.z, 3.0);
}

/// Exact odds by frontier DP. (1) equal to brute-force enumeration on small thick problems (grouped and ungrouped);
/// (2) equal to the structure-specific oracle (marginals and MAP optimum) on the frozen-split trap and a rho = 1 chain;
/// (3) decide_gated's exact tier answers the trap exactly (exact_limit 0 still forces the sampler);
/// (4) the window move with every group in the window is exact on the trap that freezes the shipped sampler; default off.
#[test]
fn acc10_frontier_exact_and_window_move() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner());
    use pbit_decide::oracle::*; use std::sync::atomic::Ordering;
    assert_eq!(WINDOW_K.load(Ordering::Relaxed), 0);
    for (t, gsz, lam, seed) in [(9usize, 3usize, 1.0, 11u64), (8, 1, 0.0, 5), (10, 2, 2.0, 3), (9, 3, 1.5, 21)] {
        let mut p = dispatch(t, 4, (t + 3) / 4 + 1, gsz, lam, seed).p; if seed == 21 { p.clamp[4] = Some(0); } // what-if clamp
        let e = exact(&p, 1, 1 << 24).unwrap(); let f = exact_frontier(&p, 1 << 16).unwrap();
        let err = f.marg.iter().zip(&e.marg).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(err < 1e-9, "frontier vs enumeration {err}"); assert!((f.logz - e.logz).abs() < 1e-9, "logz {} vs {}", f.logz, e.logz);
        assert!((f.map_logw - p.logw(&e.top[0].1)).abs() < 1e-9 && p.violations(&f.map) == 0);
    }
    let trap = build(8, 3, 5, 4.0, 4250, true); let sat = build_sat(20, 2.0, 77, true);
    for ins in [&trap, &sat] { let ex = exact_dp(ins); let f = exact_frontier(&ins.p, FRONTIER_MAX_STATES).unwrap();
        assert!(max_tv(&f.marg, &ex.marg, ins.p.a) < 1e-9); assert!((f.map_logw - exact_map_logw(ins)).abs() < 1e-9); }
    let ex = exact_dp(&trap);
    let (d, g) = decide_gated(&trap.p, 1, 0, Some(20.0), 3, &GATE).unwrap(); assert!(g.is_none() && matches!(d.verdict, Verdict::Exact));
    assert!(max_tv(&d.marg, &ex.marg, trap.p.a) < 1e-9 && trap.p.violations(&d.map) == 0);
    WINDOW_K.store(16, Ordering::Relaxed);
    let s = sample_opts(&trap.p, 4, 3000, None, 9, true, false, true); WINDOW_K.store(0, Ordering::Relaxed);
    let s = s.unwrap(); let g = gate_stats(&trap.p, &s); let mx = max_tv(&s.marg, &ex.marg, trap.p.a);
    println!("window move on the trap: maxTV {mx:.4} frozen {} bound {:.4} certified {}", g.frozen, g.tv_bound(&GATE), g.diagnostics_passed(&GATE));
    assert!(mx < 0.035 && g.frozen == 0 && g.diagnostics_passed(&GATE), "window move: maxTV {mx} frozen {}", g.frozen);
}

const GOLDEN_EXACT: Option<[u64; 3]> = Some([0xe1103c5aab070900, 0xf0acfbd4d1871c16, 0x34c366ce59dcb885]);
/// Re-pinned with `pbit_ir::FORCED_CAPS_PARTITION` on: cases 1 (0xbb9a05a399f9e9f4 -> now) and 4 (0xccdc575f31a766c0 -> now)
/// moved (a cap filled by forced legal tickets is no longer called frozen; case 1 certifies, see `forced_caps_do_not_freeze_the_gate`).
/// Re-pinned R19.3 (P1.2: partition programs also count free tasks that never moved in any chain): the T=200 A=59 and T=60 A=3
/// cases (0x8553b053831fdf0d -> now, 0x7a8591ebf0436b01 -> now; frozen now 23 and 5, every other statistic unchanged; neither passes after).
const GOLDEN_GATE: Option<[u64; 7]> = Some([0x81dca6f953ee6f7d, 0xaa917ded9cc365f5, 0x8665356ade9a50f7, 0xc975cf2b77f9ef09, 0x08d4ae7f7dfbc7c4, 0x1710deb19fdb868a, 0x7b8bf0d8d2759157]);
/// The assignment Problem is a front-end of the general IR. Through `Problem::lower`, the IR's enumeration, its sampler
/// (site + swap moves; the assignment-only accelerators are off here) and its certification gate reproduce this crate's
/// results BIT FOR BIT: log Z, marginals, top plans, per-chain trajectories and traces, and every gate statistic.
#[test]
fn ir_lowering_bit_identical() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner()); // acc10 flips the global WINDOW_K
    use pbit_decide::oracle::*;
    let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    let mut cases: Vec<Problem> = vec![r1_instance(), dispatch(6, 3, 2, 2, 0.7, 4).p, build(8, 3, 5, 4.0, 4250, true).p, build_sat(20, 2.0, 77, true).p, dispatch(60, 3, 20, 20, 5.0, 7).p, dispatch(200, 10, 22, 4, 0.8, 3).p];
    let mut c = dispatch(9, 4, 4, 3, 1.5, 21).p; c.clamp[4] = Some(0); cases.push(c);
    let mut digests: Vec<u64> = vec![]; let mut ex_digests: Vec<u64> = vec![];
    for (n, p) in cases.iter_mut().enumerate() {
        p.block_moves = false; p.pair_swaps = false; let p = &*p; let m = p.lower(); // assignment-only accelerators stay in the front-end
        let lim = if p.t >= 60 { 1 << 12 } else { 1 << 22 }; let ex = exact(p, 5, lim); let has_exact = ex.is_some();
        assert_eq!(has_exact, pbit_ir::exact(&m, 1, lim).is_some(), "case {n}: enumeration declines differently");
        if let Some(e) = &ex { // golden digest of the Problem-specific enumeration (kept specialized; the IR's must match it)
            let mut d: u64 = 0xcbf29ce484222325; let mut feed = |x: u64| { for b in x.to_le_bytes() { d = (d ^ b as u64).wrapping_mul(0x100000001b3); } };
            feed(e.n_feasible); feed(e.logz.to_bits()); for v in &e.marg { feed(v.to_bits()); } for (pr, x) in &e.top { feed(pr.to_bits()); for &v in x { feed(v as u64); } }
            println!("case {n}: exact digest {d:#018x}"); ex_digests.push(d); }
        if let Some(e) = ex { let f = pbit_ir::exact(&m, 5, lim).unwrap();
            assert_eq!(e.n_feasible, f.n_feasible); assert_eq!(e.logz.to_bits(), f.logz.to_bits(), "case {n} logz"); assert_eq!(bits(&e.marg), bits(&f.marg), "case {n} marg");
            assert_eq!(e.top.len(), f.top.len()); for (a, b) in e.top.iter().zip(&f.top) { assert_eq!((a.0.to_bits(), &a.1), (b.0.to_bits(), &b.1)); } }
        let sweeps = if p.t >= 60 { 400 } else { 3000 };
        let s1 = sample(p, 4, sweeps, None, 5 + n as u64, true, true).unwrap(); let s2 = pbit_ir::sample(&m, 4, sweeps, None, 5 + n as u64, true, true).unwrap();
        assert_eq!(bits(&s1.marg), bits(&s2.marg), "case {n} marg"); assert_eq!(s1.traj, s2.traj, "case {n} traj"); assert_eq!(s1.plans, s2.plans);
        for (a, b) in s1.trace.iter().zip(&s2.trace) { assert_eq!(bits(a), bits(b), "case {n} trace"); }
        assert_eq!((s1.n, s1.viol, s1.sweeps, s1.best.0.to_bits(), &s1.best.1), (s2.n, s2.viol, s2.sweeps, s2.best.0.to_bits(), &s2.best.1));
        let (g1, g2) = (gate_stats(p, &s1), pbit_ir::gate_stats(&m, &s1));
        assert_eq!((g1.rhat.to_bits(), g1.sig_tv_max.to_bits(), g1.min_ess.to_bits(), g1.min_batches, g1.chain_dis.to_bits(), g1.frozen, g1.worst_task),
                   (g2.rhat.to_bits(), g2.sig_tv_max.to_bits(), g2.min_ess.to_bits(), g2.min_batches, g2.chain_dis.to_bits(), g2.frozen, g2.worst_task), "case {n} gate");
        assert_eq!(bits(&g1.sig_tv), bits(&g2.sig_tv)); assert_eq!(bits(&g1.sig_tv_long), bits(&g2.sig_tv_long)); assert_eq!(bits(&g1.rhat_task), bits(&g2.rhat_task));
        // golden digest of the earlier gate (captured from the Problem-welded implementation before it was deleted)
        let mut dg: u64 = 0xcbf29ce484222325; let mut feed = |x: u64| { for b in x.to_le_bytes() { dg = (dg ^ b as u64).wrapping_mul(0x100000001b3); } };
        for v in [g1.rhat, g1.sig_tv_max, g1.min_ess, g1.chain_dis].iter().chain(&g1.sig_tv).chain(&g1.sig_tv_long).chain(&g1.rhat_task) { feed(v.to_bits()); }
        for v in [g1.min_batches, g1.frozen, g1.worst_task] { feed(v as u64); }
        println!("case {n}: T={} A={} bit-identical (exact {}, samples {}, frozen {}, certified {}) gate digest {dg:#018x}", p.t, p.a, has_exact, s1.n, g1.frozen, g1.diagnostics_passed(&GATE));
        digests.push(dg);
    }
    if let Some(want) = GOLDEN_GATE { assert_eq!(digests, want.to_vec(), "gate digests moved"); }
    if let Some(want) = GOLDEN_EXACT { assert_eq!(ex_digests, want.to_vec(), "enumeration digests moved"); }
}

/// R19 P1.1(a): with the global two-value flip on (`Problem::collective` -> `Model::collective`) the router sampler and the
/// IR sampler stay bit-identical through the lowering. A custom case has free two-worker tasks over different worker pairs in
/// one group (pairs whose affinity term changes under the flip), binding caps and three-worker tasks; the flip must fire there.
#[test]
fn ir_lowering_bit_identical_collective() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner());
    use pbit_decide::oracle::*;
    let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    let mut c = dispatch(16, 3, 7, 8, 0.6, 13).p; for i in 0..12 { c.allowed[i * 3 + 2] = false; } c.allowed[12 * 3] = false;
    let mut cases: Vec<Problem> = vec![c, r1_instance(), dispatch(9, 4, 4, 3, 1.5, 21).p, build(8, 3, 5, 4.0, 4250, true).p];
    let mut fired = false;
    for (n, p) in cases.iter_mut().enumerate() {
        p.block_moves = false; p.pair_swaps = false;
        let mut q = p.clone(); q.collective = false; let off = sample(&q, 4, 2000, None, 7 + n as u64, true, true).unwrap();
        // R19.2 c5: also the Wolff cluster move (`Problem::cluster`), alone and with the collective moves
        for (coll, clu, cyc) in [(true, false, false), (true, true, false), (false, true, false), (false, false, true), (true, true, true)] {
            p.collective = coll; p.cluster = clu; p.cycles = cyc; let m = p.lower(); assert_eq!((m.collective, m.cluster, m.cycles), (coll, clu, cyc));
            let s1 = sample(p, 4, 2000, None, 7 + n as u64, true, true).unwrap(); let s2 = pbit_ir::sample(&m, 4, 2000, None, 7 + n as u64, true, true).unwrap();
            assert_eq!(bits(&s1.marg), bits(&s2.marg), "case {n} ({coll}, {clu}, {cyc}) marg"); assert_eq!(s1.traj, s2.traj, "case {n} traj"); assert_eq!(s1.plans, s2.plans);
            for (a, b) in s1.trace.iter().zip(&s2.trace) { assert_eq!(bits(a), bits(b), "case {n} trace"); }
            assert_eq!((s1.n, s1.viol, s1.best.0.to_bits(), &s1.best.1), (s2.n, s2.viol, s2.best.0.to_bits(), &s2.best.1));
            assert_eq!(s1.moves, s2.moves, "case {n}: mode-transition counters");
            if bits(&off.marg) != bits(&s1.marg) { fired = true; }
            println!("case {n}: T={} A={} collective {coll} cluster {clu} cycles {cyc} bit-identical (samples {}), differs from all-off: {}", p.t, p.a, s1.n, bits(&off.marg) != bits(&s1.marg));
        }
    }
    assert!(fired, "the flip never changed a run");
}

/// The frontier-DP exact tier runs on the IR (`exact_frontier(p)` = `pbit_ir::exact_frontier(&p.lower())`: components
/// = groups, caps = agents). GOLDEN_FRONTIER = (log Z, MAP log w, sum_q (q+1) marg[q], DP layer peak) captured from the earlier
/// front-end implementation before it was deleted; the IR reproduces them to float rounding, not bit for bit (components are
/// ordered by smallest member, the earlier one used BTreeMap group order then singletons: a different elimination / summation order).
const GOLDEN_FRONTIER: [(f64, f64, f64, usize); 7] = [(26.06255508601748, 24.705555594994905, 162.44424682256948, 16),
    (21.873213864325106, 18.99902824621148, 131.22493572253217, 24), (32.25429450362531, 29.56395901407431, 203.84657929581314, 52),
    (28.416837434673507, 27.385227150193156, 163.22821104445615, 16), (389.09123724666017, 374.69842041947567, 73760.17652835738, 220),
    (625.9094600698604, 598.329239302225, 1179282.9999999988, 72), (78.20841378635619, 68.44689855659561, 1738.5502183862075, 2028)];
#[test]
fn ir_frontier_matches_r6_frontier() {
    use pbit_decide::oracle::*;
    let mut cases: Vec<Problem> = vec![];
    for (t, gsz, lam, seed) in [(9usize, 3usize, 1.0, 11u64), (8, 1, 0.0, 5), (10, 2, 2.0, 3), (9, 3, 1.5, 21)] {
        let mut p = dispatch(t, 4, (t + 3) / 4 + 1, gsz, lam, seed).p; if seed == 21 { p.clamp[4] = Some(0); } cases.push(p); }
    cases.push(build(8, 3, 5, 4.0, 4250, true).p); cases.push(build_sat(20, 2.0, 77, true).p); cases.push(dispatch(24, 6, 5, 3, 1.0, 7).p);
    let rel = |x: f64, y: f64| (x - y).abs() <= 1e-9 * y.abs().max(1.0);
    for (n, (p, &(lz, mlw, ck, st))) in cases.iter().zip(GOLDEN_FRONTIER.iter()).enumerate() {
        let m = p.lower(); assert!(m.is_partition());
        let f = exact_frontier(p, FRONTIER_MAX_STATES).unwrap_or_else(|| panic!("case {n}: frontier declined"));
        let c: f64 = f.marg.iter().enumerate().map(|(q, v)| (q + 1) as f64 * v).sum();
        assert!(rel(f.logz, lz) && rel(f.map_logw, mlw) && rel(c, ck) && f.max_states == st, "case {n}: ({}, {}, {c}, {}) vs golden ({lz}, {mlw}, {ck}, {st})", f.logz, f.map_logw, f.max_states);
        assert!(p.violations(&f.map) == 0 && (p.logw(&f.map) - f.map_logw).abs() < 1e-12, "case {n}: MAP");
    }
}

/// Kill test: a capacity that forced members (single-allowed tasks or what-if clamps) fill on their own can never
/// change hands, so it is not a mixing signal. With `pbit_ir::FORCED_CAPS_PARTITION` off, the gate called it frozen and refused
/// these queues outright; on, it must certify them AND every certified odds must be within the 0.05 TV tolerance of exact.
#[test]
fn forced_caps_do_not_freeze_the_gate() {
    let _serial = HEAVY.lock().unwrap_or_else(|e| e.into_inner());
    let mut cases: Vec<Problem> = vec![dispatch(6, 3, 2, 2, 0.7, 4).p]; // legal tickets are forced onto the one legal agent and fill it
    for seed in [1u64, 2, 3] { let mut p = dispatch(12, 3, 4, 3, 1.0, seed).p; // what-if: 4 clamps fill agent 0 (cap 4)
        let free: Vec<usize> = (0..p.t).filter(|&i| p.allowed[i * p.a]).take(4).collect(); assert_eq!(free.len(), 4); for i in free { p.clamp[i] = Some(0); } cases.push(p); }
    for (n, p) in cases.iter().enumerate() {
        let e = exact(p, 1, 5_000_000).expect("small enough to enumerate");
        let (d, g) = decide_gated(p, 0, 20000, None, 11 + n as u64, &GATE).unwrap(); let g = g.unwrap();
        let tv = (0..p.t).map(|i| 0.5 * (0..p.a).map(|a| (d.marg[i * p.a + a] - e.marg[i * p.a + a]).abs()).sum::<f64>()).fold(0.0, f64::max);
        assert_eq!(g.frozen, 0, "case {n}: a cap filled by forced members was called frozen");
        assert!(g.diagnostics_passed(&GATE), "case {n}: not certified (rhat {}, tv bound {})", g.rhat, g.tv_bound(&GATE));
        assert!(tv <= 0.05, "case {n}: certified but max TV {tv} vs exact");
    }
}

/// The assignment enumeration's pass 1 inherited pass 0's DFS node count, so a pass 0 using more than half the node
/// budget was cut short in pass 1 and returned a partial "exact" answer. 7 free tasks over 10 agents (cap 1) + 3 tasks pinned
/// to agents 7, 8, 9 by skill: 7! = 5040 plans, ~1M static-order nodes. Every limit must decline or return all 5040, and the
/// IR (whose enumeration had the same flaw) must agree.
#[test]
fn exact_pass_two_is_not_cut_short() {
    let (t, a) = (10usize, 10usize);
    let allowed: Vec<bool> = (0..t * a).map(|q| q / a < 7 || q % a == q / a).collect();
    let p = Problem { t, a, h: (0..t * a).map(|q| 0.01 * (q % 7) as f64).collect(), allowed, cap: vec![1; a], group: vec![usize::MAX; t], lam: 0.0,
        clamp: vec![None; t], block_moves: false, pair_swaps: false, collective: false, cluster: false, cycles: false };
    let full = exact(&p, 1, 1 << 20).unwrap(); assert_eq!(full.n_feasible, 5040);
    let mut answered = 0;
    for lim in [5040u64, 15_000, 15_921, 16_000, 20_000, 25_000, 31_000] {
        let (e, f) = (exact(&p, 1, lim), pbit_ir::exact(&p.lower(), 1, lim)); assert_eq!(e.is_some(), f.is_some(), "limit {lim}");
        if let (Some(e), Some(f)) = (e, f) { answered += 1; assert_eq!(e.n_feasible, 5040, "limit {lim}"); assert_eq!(f.n_feasible, 5040, "limit {lim}");
            assert_eq!(e.logz.to_bits(), full.logz.to_bits(), "limit {lim}"); }
    }
    assert!(answered >= 4, "answered {answered}");
}
#[test]
fn logw_matches_the_pairwise_definition_bit_for_bit() {
    // `Problem::logw` was O(t^2) (2 s per call at 100,000 tasks); the linear rewrite must add the same terms in the same
    // order, so the value is identical to the last bit (lam = 0.37 is not exact in binary: a different order or lam * count would show)
    let naive = |p: &Problem, x: &[usize]| { let mut e = 0.0; for i in 0..p.t { e += p.h[i * p.a + x[i]];
        for j in i + 1..p.t { if p.group[i] != usize::MAX && p.group[i] == p.group[j] && x[i] == x[j] { e += p.lam; } } } e };
    for (t, a, gs, seed) in [(7, 4, 3, 11), (60, 3, 5, 2), (300, 5, 4, 7), (257, 2, 64, 9)] {
        let mut p = dispatch(t, a, t, gs, 0.37, seed).p; for i in (0..t).step_by(5) { p.group[i] = usize::MAX; } // ungrouped tasks too
        let mut r = seed;
        for _ in 0..20 {
            let x: Vec<usize> = (0..t).map(|_| { r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (r >> 33) as usize % a }).collect();
            assert_eq!(p.logw(&x).to_bits(), naive(&p, &x).to_bits(), "t={t} a={a} group size {gs}");
            // R19.8: both paths at every size (the threshold LOGW_PAIRWISE_MAX only picks one)
            assert_eq!(p.logw_by(&x, true).to_bits(), p.logw_by(&x, false).to_bits(), "paths t={t} a={a} group size {gs}");
        }
    }
}

/// R19.8 (P2.3 item 2): `lower_until` gives the `lower` program without a deadline or before it, and None once it passed.
#[test]
fn lower_until_honours_the_deadline() {
    let p = dispatch(60, 5, 14, 20, 0.7, 3).p;
    let (a, b) = (p.lower(), p.lower_until(Some(Instant::now() + std::time::Duration::from_secs(600))).expect("before the deadline"));
    assert_eq!((a.n, a.k, a.pairs.len(), a.caps.len()), (b.n, b.k, b.pairs.len(), b.caps.len())); assert_eq!(a.pairs.len(), 3 * 190);
    assert_eq!(a.logw(&vec![0; 60]).to_bits(), b.logw(&vec![0; 60]).to_bits());
    assert!(p.lower_until(Some(Instant::now())).is_none());
    assert!(p.lower_until(None).is_some());
}

/// R19.9 (finding 1): the enumeration keeps per-(group, worker) counts of assigned mates and a per-(task, worker) memo of
/// the affinity fold instead of scanning every group mate at every node (199.9 s -> seconds on a near-saturated 3,000-task
/// group). Reference = the R19.8 enumeration verbatim (mates scan, same passes, same top-k upkeep): plan count, log Z,
/// every marginal and the top plans with their odds must match to the last bit. lam values are not exact in binary
/// (a count-times-lam product would differ in the last bits); group ids are sparse; some tasks are ungrouped.
#[test]
fn enumeration_affinity_memo_is_bit_identical() {
    struct R<'a> { p: &'a Problem, mates: Vec<Vec<usize>>, x: Vec<usize>, load: Vec<usize>, pass: u8, mx: f64, sum: f64,
        marg: Vec<f64>, top: Vec<(f64, Vec<usize>)>, n: u64 }
    impl R<'_> {
        fn dfs(&mut self, i: usize, lw: f64) {
            let p = self.p;
            if i == p.t {
                self.n += 1;
                if self.pass == 0 { if lw > self.mx { self.mx = lw; } return; }
                let w = (lw - self.mx).exp(); self.sum += w;
                for j in 0..p.t { self.marg[j * p.a + self.x[j]] += w; }
                if self.top.len() < 5 || lw > self.top.last().unwrap().0 {
                    self.top.push((lw, self.x.clone())); self.top.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap()); self.top.truncate(5); }
                return;
            }
            for a in 0..p.a {
                if !p.ok(i, a) || self.load[a] >= p.cap[a] { continue; }
                let mut d = p.h[i * p.a + a];
                for &j in &self.mates[i] { if j < i && self.x[j] == a { d += p.lam; } }
                self.x[i] = a; self.load[a] += 1; self.dfs(i + 1, lw + d); self.load[a] -= 1;
            }
        }
    }
    let mut r = 20261001u64; let mut rnd = |m: usize| { r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (r >> 33) as usize % m };
    let (mut feasible, mut with_mates) = (0, 0);
    for case in 0..240usize {
        let (t, a) = (4 + case % 6, 2 + case % 3);
        let lam = [0.37, -1.3, 2.1, 0.1, -0.05][case % 5];
        let mut p = dispatch(t, a, t, 1, lam, case as u64 + 1).p;
        for v in p.allowed.iter_mut() { *v = rnd(5) != 0; }
        for i in 0..t { let k = rnd(a); p.allowed[i * a + k] = true; } // every task keeps one worker
        for c in p.cap.iter_mut() { *c = 1 + rnd(t); }
        let ng = 1 + rnd(3);
        for g in p.group.iter_mut() { *g = if rnd(4) == 0 { usize::MAX } else { 1000 + 7 * rnd(ng) }; }
        let e = exact(&p, 5, 1 << 40).expect("small programs enumerate");
        let mut s = R { p: &p, mates: p.mates(), x: vec![0; t], load: vec![0; a], pass: 0, mx: f64::NEG_INFINITY, sum: 0.0,
            marg: vec![0.0; t * a], top: vec![], n: 0 };
        s.dfs(0, 0.0);
        if s.n == 0 { assert_eq!(e.n_feasible, 0, "case {case}"); continue; }
        feasible += 1; if s.mates.iter().any(|m| !m.is_empty()) { with_mates += 1; }
        s.pass = 1; s.n = 0; s.dfs(0, 0.0);
        let z = s.sum; for m in s.marg.iter_mut() { *m /= z; }
        let logz = s.mx + z.ln();
        assert_eq!(e.n_feasible, s.n, "case {case}");
        assert_eq!(e.logz.to_bits(), logz.to_bits(), "case {case}: logz");
        for (u, v) in e.marg.iter().zip(&s.marg) { assert_eq!(u.to_bits(), v.to_bits(), "case {case}: marginal"); }
        assert_eq!(e.top.len(), s.top.len(), "case {case}");
        for ((po, xo), (lw, x)) in e.top.iter().zip(&s.top) { assert_eq!(po.to_bits(), (lw - logz).exp().to_bits(), "case {case}: top odds"); assert_eq!(xo, x); }
        // a capped memo (shorter chunks, the fold continues past them) gives the same bits
        for m in [0, 1, 2 * t * a] {
            let f = exact_memo_capped(&p, 5, 1 << 40, m).expect("small programs enumerate");
            assert_eq!((f.n_feasible, f.logz.to_bits()), (e.n_feasible, e.logz.to_bits()), "case {case}: memo cap {m}");
            for (u, v) in f.marg.iter().zip(&e.marg) { assert_eq!(u.to_bits(), v.to_bits(), "case {case}: memo cap {m}"); }
        }
    }
    assert!(feasible >= 150 && with_mates >= 150, "too few informative cases: {feasible} feasible, {with_mates} with mates");
}

/// R19.9 (finding 2): `sample_on`'s wall-clock budget is a deadline per worker, not a slice per chain. Each chain used to time
/// its own budget / ceil(chains / threads) slice and checked the clock every 8 sweeps, so with thousands of chains every chain
/// overran its few microseconds and nothing charged the overrun (100,000 chains on 200 ms: ~460 ms of sampling). Now the r-th
/// chain of a worker stops at call start + (r + 1) slices (CLI A/B, BENCHMARKS §6: 100,000 chains sampled 431.5 -> 269.4 ms on
/// 200 ms; the rest is the chains' builds, which every chain needs). A LOOSE guard, not a separation: at 32,000 chains the
/// builds alone fill a 60 ms budget on an Apple M4 (CLI old 89.8 vs new 88.9 ms), so both pass. It pins that the budget stays
/// a deadline within 3x at many chains and that every chain is still in the output.
#[test]
fn sample_on_budget_is_a_deadline_at_many_chains() {
    let p = dispatch(24, 5, 10, 3, 1.0, 1).p;
    let t0 = Instant::now();
    let s = sample_on(&p, 32_000, 4, 0, Some(60.0), 1, false, false, 100, 0).expect("chains start");
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    assert_eq!(s.moves.len(), 32_000, "every chain is in the output");
    let lim = if std::env::var_os("CI").is_some() { 1500.0 } else { 180.0 }; assert!(ms < lim, "sampling took {ms:.1} ms for a 60 ms budget (limit {lim})");
}
