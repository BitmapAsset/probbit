use probbit_decide::{exact, polish_plan_on, Problem};

fn fixture(case: usize) -> Problem {
    let (t, a) = (1 + case % 6, 2 + case % 3);
    Problem {
        t,
        a,
        h: (0..t * a)
            .map(|i| ((i * 17 + case * 7) % 29) as f64 / 7.0 - 2.0)
            .collect(),
        allowed: (0..t * a).map(|i| (i * 11 + case) % 7 != 0).collect(),
        cap: (0..a).map(|i| (case + i * 3) % (t + 1)).collect(),
        group: (0..t)
            .map(|i| if i % 3 == 0 { usize::MAX } else { i % 2 })
            .collect(),
        lam: (case % 5) as f64 / 3.0,
        clamp: (0..t)
            .map(|i| {
                if (case + i) % 11 == 0 {
                    Some(case % a)
                } else {
                    None
                }
            })
            .collect(),
        block_moves: false,
        pair_swaps: false,
        collective: true,
        cluster: true,
        cycles: true,
    }
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-10, "{a} != {b}");
}

#[test]
fn router_and_lowering_match_180_independent_exhaustive_oracles() {
    for case in 0..180 {
        let p = fixture(case);
        let mut rows = vec![];
        for mut code in 0..p.a.pow(p.t as u32) {
            let x: Vec<_> = (0..p.t)
                .map(|_| {
                    let a = code % p.a;
                    code /= p.a;
                    a
                })
                .collect();
            if (0..p.t).any(|i| !p.allowed[i * p.a + x[i]] || p.clamp[i].is_some_and(|a| a != x[i]))
            {
                continue;
            }
            if (0..p.a).any(|a| x.iter().filter(|&&v| v == a).count() > p.cap[a]) {
                continue;
            }
            let unary: f64 = x.iter().enumerate().map(|(i, &a)| p.h[i * p.a + a]).sum();
            let mut same = 0;
            for i in 0..p.t {
                for j in i + 1..p.t {
                    if p.group[i] != usize::MAX && p.group[i] == p.group[j] && x[i] == x[j] {
                        same += 1;
                    }
                }
            }
            rows.push((unary + p.lam * same as f64, x));
        }
        let e = exact(&p, 5, 100_000).unwrap();
        let lowered = probbit_ir::exact(&p.lower(), 5, 100_000).unwrap();
        assert_eq!(e.n_feasible as usize, rows.len());
        assert_eq!(lowered.n_feasible, e.n_feasible);
        if rows.is_empty() {
            assert!(e.top.is_empty());
            assert_eq!(e.logz, f64::NEG_INFINITY);
            continue;
        }
        let best = rows
            .iter()
            .map(|(w, _)| *w)
            .fold(f64::NEG_INFINITY, f64::max);
        let z: f64 = rows.iter().map(|(w, _)| (w - best).exp()).sum();
        let logz = best + z.ln();
        close(e.logz, logz);
        close(lowered.logz, logz);
        close(p.logw(&e.top[0].1), best);
        let mut marg = vec![0.0; p.t * p.a];
        for (w, x) in rows {
            for (i, a) in x.into_iter().enumerate() {
                marg[i * p.a + a] += (w - best).exp() / z;
            }
        }
        for (i, &want) in marg.iter().enumerate() {
            close(e.marg[i], want);
            close(lowered.marg[i], want);
        }
    }
}

#[test]
fn router_polish_refuses_infeasible_or_malformed_starts() {
    let mut p = fixture(1);
    p.allowed.fill(true);
    p.clamp.fill(None);
    p.cap = vec![1; p.a];
    for start in [vec![0, 0], vec![], vec![0], vec![0, p.a], vec![0, 1, 0]] {
        assert!(
            polish_plan_on(&p, Some(&start), 0.0, 0, 1, 7).is_none(),
            "accepted {start:?}"
        );
    }
}
