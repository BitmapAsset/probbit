//! Front-end `exact_frontier(p)` vs the IR `pbit_ir::exact_frontier(&p.lower())` (lowering included): wall time
//! (the original measurements were taken BEFORE the front-end's own copy of the DP was deleted; now both columns run the IR),
//! (median + IQR of REPS runs) on thin queues solved exactly and thick queues that must decline. env: REPS (default 9).
use pbit_decide::*; use pbit_decide::oracle::*;
fn q(v: &mut Vec<f64>) -> (f64, f64, f64) { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); let n = v.len(); (v[n / 2], v[n / 4], v[3 * n / 4]) }
fn main() {
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(9);
    let cases: Vec<(&str, Problem)> = vec![("thin T=24 A=6", dispatch(24, 6, 5, 3, 1.0, 7).p), ("trap T=80", build(8, 3, 5, 4.0, 4250, true).p),
        ("sat T=200", build_sat(20, 2.0, 77, true).p), ("thick T=120 (decline)", dispatch(120, 6, 21, 3, 1.0, 125).p), ("thick T=300 (decline)", dispatch(300, 6, 51, 3, 1.0, 305).p)];
    println!("case,frontend_ms_median,frontend_iqr,ir_ms_median,ir_iqr,lower_ms_median,both_solved,states_frontend,states_ir");
    for (name, p) in &cases {
        let (mut a, mut b, mut l) = (vec![], vec![], vec![]); let (mut sa, mut sb) = (None, None);
        for _ in 0..reps {
            let t = std::time::Instant::now(); let x = exact_frontier(p, FRONTIER_MAX_STATES); a.push(t.elapsed().as_secs_f64() * 1e3);
            let t = std::time::Instant::now(); let m = p.lower(); l.push(t.elapsed().as_secs_f64() * 1e3); let y = pbit_ir::exact_frontier(&m, pbit_ir::FRONTIER_MAX_STATES); b.push(t.elapsed().as_secs_f64() * 1e3);
            sa = x.map(|f| f.max_states); sb = y.map(|f| f.max_states);
        }
        let ((am, a1, a3), (bm, b1, b3), (lm, _, _)) = (q(&mut a), q(&mut b), q(&mut l));
        println!("{name},{am:.3},{a1:.3}-{a3:.3},{bm:.3},{b1:.3}-{b3:.3},{lm:.3},{},{:?},{:?}", sa.is_some() && sb.is_some(), sa, sb);
    }
}
