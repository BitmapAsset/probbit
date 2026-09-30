//! Unit-time job scheduling as a pbit-ir program. Jobs are variables, time slots are values. Hard rules:
//! a release/deadline window per job (allowed slots), at most CAP jobs per slot (one capacity per slot), precedence i -> j (i in
//! an earlier slot than j) as pair caps "at most one of (i, a), (j, b)" for every a >= b. Soft log-weight h = -BETA * w_i * slot / T
//! (weighted completion time) + noise. Part A (small n): the IR's exact enumeration is the oracle; the sampler + gate is scored
//! on every job it RELEASES (false release = released job whose true TV error > 0.05); plans vs the exact MAP. Part B (large n,
//! no oracle): plan log-weight at EQUAL wall time, greedy + repair restarts vs pbit (sample, then anneal).
//! Baseline: greedy list scheduling (topological order, heaviest ready job first, best feasible slot) + repair (move / swap hill
//! climbing to a local optimum); at equal time it restarts with a random ready-job order and keeps the best.
//! env: NINST (part A instances per setting), MS (budget per method), SEEDS (part B).
use pbit_core::Philox4x32;
use pbit_ir::*;
use std::time::Instant;

const BETA: f64 = 2.0;
struct Inst { n: usize, t: usize, cap: usize, win: Vec<(usize, usize)>, pred: Vec<Vec<usize>>, succ: Vec<Vec<usize>>, w: Vec<f64>, h: Vec<f64> }

fn gen(n: usize, t: usize, cap: usize, deg: f64, seed: u64, id: u64) -> Inst {
    let mut r = Philox4x32::new(seed, id);
    // layered DAG (a random i < j DAG has chains of length ~n, longer than any horizon): L = t/3 layers, edges only from layer l
    // to l + 1, each job ~deg predecessors; windows: release in [0, t/4], deadline in [3t/4, t-1]
    let (mut pred, mut succ) = (vec![vec![]; n], vec![vec![]; n]); let nl = (t / 3).max(2); let layer = |i: usize| i * nl / n;
    for i in 0..n { for j in i + 1..n { if layer(j) == layer(i) + 1 && r.f64() < deg * nl as f64 / n as f64 { pred[j].push(i); succ[i].push(j); } } }
    let win: Vec<(usize, usize)> = (0..n).map(|_| (r.below(t / 4 + 1), t - 1 - r.below(t / 4 + 1))).collect();
    let w: Vec<f64> = (0..n).map(|_| 0.5 + r.f64()).collect();
    let h = (0..n * t).map(|q| -BETA * w[q / t] * (q % t) as f64 / t as f64 + 0.3 * (r.f64() - 0.5)).collect();
    Inst { n, t, cap, win, pred, succ, w, h }
}
fn model(p: &Inst) -> Model {
    let (n, t) = (p.n, p.t);
    let allowed: Vec<bool> = (0..n * t).map(|q| { let (lo, hi) = p.win[q / t]; (lo..=hi).contains(&(q % t)) }).collect();
    let mut caps: Vec<Cap> = (0..t).map(|s| Cap { members: (0..n).filter(|&i| allowed[i * t + s]).map(|i| (i, s)).collect(), limit: p.cap }).collect();
    for j in 0..n { for &i in &p.pred[j] { for a in 0..t { for b in 0..=a { if allowed[i * t + a] && allowed[j * t + b] { caps.push(Cap { members: vec![(i, a), (j, b)], limit: 1 }); } } } } }
    Model::new(n, t, p.h.clone(), allowed, vec![None; n], vec![], caps).unwrap()
}
fn logw(p: &Inst, x: &[usize]) -> f64 { x.iter().enumerate().map(|(i, &s)| p.h[i * p.t + s]).sum() }
/// can job i sit in slot s given the other jobs' slots (window, precedence both ways, capacity with i removed from its slot)?
fn fits(p: &Inst, x: &[usize], load: &[usize], i: usize, s: usize) -> bool {
    let (lo, hi) = p.win[i];
    s >= lo && s <= hi && (load[s] < p.cap || x[i] == s) && p.pred[i].iter().all(|&q| x[q] == usize::MAX || x[q] < s) && p.succ[i].iter().all(|&q| x[q] == usize::MAX || x[q] > s)
}
fn greedy(p: &Inst, r: Option<&mut Philox4x32>) -> Option<Vec<usize>> {
    // latest start per job (deadline and successors' latest starts; jobs are numbered in topological order): the greedy never
    // places a job so late that a successor chain cannot fit (capacity is ignored here, so it can still fail)
    let mut ls: Vec<usize> = p.win.iter().map(|w| w.1).collect();
    for i in (0..p.n).rev() { for &j in &p.succ[i] { ls[i] = ls[i].min(ls[j].checked_sub(1)?); } }
    let (mut x, mut load, mut indeg) = (vec![usize::MAX; p.n], vec![0; p.t], p.pred.iter().map(|v| v.len()).collect::<Vec<_>>());
    let mut ready: Vec<usize> = (0..p.n).filter(|&i| indeg[i] == 0).collect(); let mut r = r;
    while !ready.is_empty() {
        let k = match r.as_deref_mut() { Some(r) => r.below(ready.len()), None => (0..ready.len()).max_by(|&a, &b| p.w[ready[a]].partial_cmp(&p.w[ready[b]]).unwrap()).unwrap() };
        let i = ready.swap_remove(k);
        let s = (0..=ls[i]).filter(|&s| fits(p, &x, &load, i, s)).max_by(|&a, &b| p.h[i * p.t + a].partial_cmp(&p.h[i * p.t + b]).unwrap())?;
        x[i] = s; load[s] += 1;
        for &j in &p.succ[i] { indeg[j] -= 1; if indeg[j] == 0 { ready.push(j); } }
    }
    Some(x)
}
/// repair: first-improvement hill climbing over single-job moves and two-job slot swaps until no move gains
fn repair(p: &Inst, x: &mut [usize]) {
    let mut load = vec![0; p.t]; for &s in x.iter() { load[s] += 1; }
    loop { let mut moved = false;
        for i in 0..p.n { for s in 0..p.t { if s != x[i] && p.h[i * p.t + s] > p.h[i * p.t + x[i]] + 1e-12 && fits(p, x, &load, i, s) { load[x[i]] -= 1; load[s] += 1; x[i] = s; moved = true; } } }
        for i in 0..p.n { for j in i + 1..p.n { let (a, b) = (x[i], x[j]); if a == b { continue; }
            if p.h[i * p.t + b] + p.h[j * p.t + a] > p.h[i * p.t + a] + p.h[j * p.t + b] + 1e-12 {
                x[i] = b; x[j] = a; // a swap leaves every slot's load unchanged: check windows and precedence only
                let ok = |k: usize| { let s = x[k]; s >= p.win[k].0 && s <= p.win[k].1 && p.pred[k].iter().all(|&q| x[q] < s) && p.succ[k].iter().all(|&q| x[q] > s) };
                if ok(i) && ok(j) { moved = true; } else { x[i] = a; x[j] = b; } } } }
        if !moved { return; } }
}
fn restarts(p: &Inst, ms: f64, seed: u64) -> (Option<f64>, usize) {
    let t0 = Instant::now(); let mut r = Philox4x32::new(seed, 7); let (mut best, mut runs) = (None::<f64>, 0);
    while runs == 0 || t0.elapsed().as_secs_f64() * 1e3 < ms { runs += 1;
        if let Some(mut x) = greedy(p, if runs == 1 { None } else { Some(&mut r) }) { repair(p, &mut x); let lw = logw(p, &x); if best.map_or(true, |b| lw > b) { best = Some(lw); } } }
    (best, runs)
}
fn pbit_plan(m: &Model, ms: f64, seed: u64) -> Option<(f64, Vec<usize>)> {
    let s = sample(m, 4, 0, Some(ms / 2.0), seed, true, false)?;
    anneal(m, Some(&s.best.1), &[1.0, 2.0, 4.0, 8.0, 16.0], ms / 2.0, 4, seed + 1)
}
fn q(mut v: Vec<f64>) -> (f64, f64, f64) { if v.is_empty() { return (f64::NAN, f64::NAN, f64::NAN); } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); let p = |f: f64| v[((v.len() - 1) as f64 * f).round() as usize]; (p(0.5), p(0.25), p(0.75)) }
fn main() {
    let env = |k: &str, d: f64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (ninst, ms, seeds) = (env("NINST", 20.0) as u64, env("MS", 100.0), env("SEEDS", 5.0) as u64);
    if let Ok(n) = std::env::var("PROBE_START") { // time one chain's random feasible-start search (the sampler's start)
        let n: usize = n.parse().unwrap(); let p = gen(n, n / 5, env("PCAP", (n / 20) as f64) as usize, 1.0, 4242, n as u64 * 1000 + env("PSEED", 0.0) as u64); let m = model(&p);
        if let Ok(path) = std::env::var("JSON_OUT") { // the same program as pbit-ir JSON v1 (for `pbit run`)
            let vals: Vec<String> = (0..p.t).map(|s| format!("\"s{s}\"")).collect();
            let vars: Vec<String> = (0..p.n).map(|i| format!("{{\"id\": \"j{i}\", \"h\": {{{}}}, \"allowed\": [{}]}}",
                (0..p.t).map(|s| format!("\"s{s}\": {}", p.h[i * p.t + s])).collect::<Vec<_>>().join(", "), (p.win[i].0..=p.win[i].1).map(|s| format!("\"s{s}\"")).collect::<Vec<_>>().join(", "))).collect();
            let caps: Vec<String> = m.caps.iter().map(|c| format!("{{\"limit\": {}, \"members\": [{}]}}", c.limit, c.members.iter().map(|&(i, v)| format!("[\"j{i}\", \"s{v}\"]")).collect::<Vec<_>>().join(", "))).collect();
            std::fs::write(&path, format!("{{\"pbit_ir\": 1, \"values\": [{}], \"vars\": [{}], \"caps\": [{}]}}", vals.join(", "), vars.join(", "), caps.join(", "))).unwrap(); println!("wrote {path}"); return;
        }
        if let Ok(k) = std::env::var("PROBE_STREAMS") { for c in 0..k.parse::<u64>().unwrap() { let f0 = START_FALLBACKS.load(std::sync::atomic::Ordering::Relaxed); // each sampler chain's start
            let t0 = Instant::now(); let ch = Chain::new(&m, 60, c); println!("n={n} stream {c}: {} after {:.1} ms (retries {})", if ch.is_some() { "found" } else { "gave up" }, t0.elapsed().as_secs_f64() * 1e3, START_FALLBACKS.load(std::sync::atomic::Ordering::Relaxed) - f0); } return; }
        if std::env::var("PROBE_EXACT").is_ok() { for lim in [1u64, 2_000_000] { let t0 = Instant::now(); let e = exact(&m, 1, lim); // the exact tier's decline cost
            println!("n={n} exact(limit {lim}): {} after {:.1} ms", if e.is_some() { "answered" } else { "declined" }, t0.elapsed().as_secs_f64() * 1e3); } return; }
        let t0 = Instant::now(); let c = Chain::new(&m, 60, 0); let dt = t0.elapsed().as_secs_f64() * 1e3;
        println!("n={n} caps={} greedy plan exists: {}; start search: {} after {dt:.1} ms; ascending-order retries: {}", m.caps.len(), greedy(&p, None).is_some(),
            c.as_ref().map_or("gave up".to_string(), |c| format!("found (violations {})", m.violations(&c.x))), START_FALLBACKS.load(std::sync::atomic::Ordering::Relaxed));
        if c.is_some() { let t1 = Instant::now(); if let Some(s) = sample(&m, 4, 0, Some(ms), 60, true, false) { let g = gate_stats(&m, &s); let rel = if g.certified(&GATE) { n } else { g.certified_tasks(&GATE).iter().filter(|&&r| r).count() };
            println!("  sampler {ms} ms budget: {:.0} ms total (4 chain starts incl.), violations {}, gate: certified {} released {rel}/{n} rhat {:.4} frozen {}", t1.elapsed().as_secs_f64() * 1e3, s.viol, g.certified(&GATE), g.rhat, g.frozen); } else { println!("  sampler: no start"); } }
        return;
    }
    println!("machine: Apple M4 (4P+6E); sampler 4 chains / 4 threads; gate z=3 tol 0.05; budget {ms} ms per method; beta {BETA}");
    println!("A: n,t,cap,inst,prec_edges,schedules,exact_ms,verdict,released,false_releases,max_tv_released,map_logw,gap_greedy_repair,gap_restarts_eq_time,gap_pbit_eq_time");
    for &(n, t, cap) in &[(10usize, 6usize, 2usize), (12, 6, 3)] {
        let (mut rel, mut fr, mut certs, mut refused, mut used) = (0, 0, 0, 0, 0); let (mut g1, mut g2, mut g3) = (vec![], vec![], vec![]); let mut gfail = 0;
        for inst in 0..ninst {
            let p = gen(n, t, cap, 1.0, 2026, (n * 100 + cap) as u64 * 1000 + inst); let m = model(&p);
            let t1 = Instant::now(); let ex = match exact(&m, 1, 1 << 23) { Some(x) if x.n_feasible > 0 => x, _ => continue }; let exm = t1.elapsed().as_secs_f64() * 1e3; used += 1;
            let map = m.logw(&ex.top[0].1); assert_eq!(m.violations(&ex.top[0].1), 0);
            let s = sample(&m, 4, 0, Some(ms), 40 + inst, true, false).unwrap(); let g = gate_stats(&m, &s); let whole = g.certified(&GATE);
            let mask = if whole { vec![true; n] } else { g.certified_tasks(&GATE) };
            let tv: Vec<f64> = (0..n).map(|i| 0.5 * (0..t).map(|c| (s.marg[i * t + c] - ex.marg[i * t + c]).abs()).sum::<f64>()).collect();
            let (mut nr, mut nf, mut mx) = (0, 0, 0.0f64); for i in 0..n { if mask[i] { nr += 1; mx = mx.max(tv[i]); if tv[i] > 0.05 { nf += 1; } } }
            let verdict = if whole { "certified" } else if nr > 0 { "partial" } else { "refused" }; if whole { certs += 1; } if nr == 0 { refused += 1; } rel += nr; fr += nf;
            let gr = greedy(&p, None).map(|mut x| { repair(&p, &mut x); assert_eq!(m.violations(&x), 0); map - logw(&p, &x) });
            let rs = restarts(&p, ms, 90 + inst).0.map(|b| map - b); let pb = pbit_plan(&m, ms, 60 + inst).map(|(lw, x)| { assert_eq!(m.violations(&x), 0); map - lw });
            match gr { Some(v) => g1.push(v), None => gfail += 1 } if let Some(v) = rs { g2.push(v); } if let Some(v) = pb { g3.push(v); }
            let f = |v: Option<f64>| v.map_or("fail".to_string(), |v| format!("{v:.4}"));
            println!("{n},{t},{cap},{inst},{},{},{exm:.1},{verdict},{nr},{nf},{mx:.4},{map:.4},{},{},{}", p.pred.iter().map(|v| v.len()).sum::<usize>(), ex.n_feasible, f(gr), f(rs), f(pb));
        }
        let opt = |v: &[f64]| v.iter().filter(|&&g| g < 1e-9).count();
        println!("SUMMARY A n={n} t={t} cap={cap}: {used} feasible instances; certified {certs}, refused {refused}, released {rel} jobs, FALSE releases {fr}; optimal plans (gap < 1e-9): greedy+repair {}/{} (greedy failed {gfail}), restarts@{ms}ms {}/{}, pbit@{ms}ms {}/{}; mean gap {:.4} / {:.4} / {:.4}",
            opt(&g1), g1.len(), opt(&g2), g2.len(), opt(&g3), g3.len(), g1.iter().sum::<f64>() / g1.len().max(1) as f64, g2.iter().sum::<f64>() / g2.len().max(1) as f64, g3.iter().sum::<f64>() / g3.len().max(1) as f64);
    }
    println!("B: n,t,cap,seed,prec_edges,greedy_repair_logw,greedy_repair_ms,restarts_logw,restarts_runs,pbit_anneal_from_greedy_logw (equal time {ms} ms)");
    for &(n, t, cap) in &[(200usize, 40usize, 10usize), (500, 50, 20)] {
        let (mut d1, mut d2) = (vec![], vec![]);
        for sd in 0..seeds {
            let p = gen(n, t, cap, 1.0, 4242, n as u64 * 1000 + sd); let m = model(&p);
            let t1 = Instant::now(); let g0 = greedy(&p, None); let gr = g0.clone().map(|mut x| { repair(&p, &mut x); logw(&p, &x) }); let gms = t1.elapsed().as_secs_f64() * 1e3;
            let (rs, runs) = restarts(&p, ms, 90 + sd);
            // the sampler's generic feasible start (MRV DFS, 2M-node budget at O(n k caps) per node) did not return within 30 s here
            // (in an earlier build, 30 s alarm): pbit starts from the plain greedy plan and anneals for the same wall time as the restarts
            let pb = g0.and_then(|x0| anneal(&m, Some(&x0), &[1.0, 2.0, 4.0, 8.0, 16.0], ms, 4, 60 + sd)).map(|(lw, x)| { assert_eq!(m.violations(&x), 0); lw });
            let f = |v: Option<f64>| v.map_or("fail".to_string(), |v| format!("{v:.3}"));
            println!("{n},{t},{cap},{sd},{},{},{gms:.2},{},{runs},{}", p.pred.iter().map(|v| v.len()).sum::<usize>(), f(gr), f(rs), f(pb));
            if let (Some(a), Some(b)) = (rs, pb) { d1.push(b - a); } if let (Some(a), Some(b)) = (gr, pb) { d2.push(b - a); }
        }
        let (a, al, ah) = q(d1.clone()); let (b, bl, bh) = q(d2.clone());
        println!("SUMMARY B n={n} t={t} cap={cap}: pbit anneal-from-greedy minus restarts (log w, + = pbit better) median {a:.3} [{al:.3},{ah:.3}] over {}; pbit minus single greedy+repair median {b:.3} [{bl:.3},{bh:.3}]", d1.len());
    }
}
