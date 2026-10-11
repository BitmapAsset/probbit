//! Independent exhaustive checks: the oracle does not call Model::logw, violations,
//! candidate/start search, compilation, or any exact solver.
use probbit_ir::{Cap, Coupling, Model, Pair};

struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 32) % n as u64) as usize
    }
    fn weight(&mut self) -> f64 {
        self.next(17) as f64 / 4.0 - 2.0
    }
}

fn feasible(m: &Model, x: &[usize]) -> bool {
    (0..m.n).all(|i| m.allowed[i * m.k + x[i]] && m.clamp[i].map_or(true, |v| v == x[i]))
        && m.caps.iter().all(|c| {
            c.members
                .iter()
                .enumerate()
                .filter(|(_, (i, v))| x[*i] == *v)
                .map(|(q, _)| c.weights.get(q).copied().unwrap_or(1) as u64)
                .sum::<u64>()
                <= c.limit as u64
        })
}
fn score(m: &Model, x: &[usize]) -> f64 {
    let mut w = (0..m.n).map(|i| m.h[i * m.k + x[i]]).sum::<f64>();
    for p in &m.pairs {
        w += match &p.c {
            Coupling::Potts(v) => {
                if x[p.i] == x[p.j] {
                    *v
                } else {
                    0.0
                }
            }
            Coupling::Table(t) => t[x[p.i] * m.k + x[p.j]],
        };
    }
    w
}
fn oracle(m: &Model) -> (usize, f64, f64, Vec<f64>) {
    let mut states = vec![];
    for mut code in 0..m.k.pow(m.n as u32) {
        let x: Vec<_> = (0..m.n)
            .map(|_| {
                let v = code % m.k;
                code /= m.k;
                v
            })
            .collect();
        if feasible(m, &x) {
            states.push((score(m, &x), x));
        }
    }
    let best = states
        .iter()
        .map(|(w, _)| *w)
        .fold(f64::NEG_INFINITY, f64::max);
    let z: f64 = states.iter().map(|(w, _)| (w - best).exp()).sum();
    let mut marg = vec![0.0; m.n * m.k];
    for (w, x) in &states {
        for (i, &v) in x.iter().enumerate() {
            marg[i * m.k + v] += (w - best).exp() / z;
        }
    }
    (states.len(), best + z.ln(), best, marg)
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 2e-10, "{a} != {b}");
}

#[test]
fn exact_tiers_and_compilation_match_320_independent_exhaustive_oracles() {
    let mut r = Rng(0x534d414c4c);
    let mut answered_components = 0;
    let mut answered_frontier = 0;
    for case in 0..320 {
        let (n, k) = (1 + r.next(6), 2 + r.next(3));
        let h = (0..n * k).map(|_| r.weight()).collect();
        let allowed = (0..n * k).map(|_| r.next(7) != 0).collect();
        let clamp = (0..n)
            .map(|_| {
                if r.next(5) == 0 {
                    Some(r.next(k))
                } else {
                    None
                }
            })
            .collect();
        let mut pairs = vec![];
        for i in 0..n {
            for j in i + 1..n {
                if r.next(4) == 0 {
                    // Include reverse-oriented, asymmetric tables; non-negative and negative Potts terms.
                    pairs.push(if r.next(2) == 0 {
                        Pair {
                            i,
                            j,
                            c: Coupling::Potts(r.weight()),
                        }
                    } else {
                        Pair {
                            i: j,
                            j: i,
                            c: Coupling::Table((0..k * k).map(|_| r.weight()).collect()),
                        }
                    });
                }
            }
        }
        let mut caps = vec![];
        for _ in 0..r.next(4) {
            let members: Vec<_> = (0..n)
                .flat_map(|i| (0..k).map(move |v| (i, v)))
                .filter(|_| r.next(4) == 0)
                .collect();
            let weights = if case % 3 == 0 {
                members.iter().map(|_| 1 + r.next(4)).collect()
            } else {
                vec![]
            };
            caps.push(Cap {
                members,
                weights,
                limit: r.next(n + 3),
            });
        }
        let m = Model::new(n, k, h, allowed, clamp, pairs, caps).unwrap();
        let (count, logz, best, marg) = oracle(&m);
        for actual in [&m, &m.compiled().0] {
            let e = probbit_ir::exact(actual, 5, 100_000).unwrap();
            assert_eq!(e.n_feasible as usize, count, "case {case}");
            if count == 0 {
                assert!(e.top.is_empty());
                assert_eq!(e.logz, f64::NEG_INFINITY);
            } else {
                close(e.logz, logz);
                close(score(&m, &e.top[0].1), best);
                for (a, b) in e.marg.iter().zip(&marg) {
                    close(*a, *b);
                }
                for (p, x) in &e.top {
                    assert!(feasible(&m, x));
                    close(*p, (score(&m, x) - logz).exp());
                }
            }
            if let Some(c) = probbit_ir::exact_components_until(actual, 100_000, 100_000, None) {
                answered_components += 1;
                assert_eq!(c.infeasible, count == 0, "case {case}");
                if count > 0 {
                    close(c.logz, logz);
                    close(c.map_logw, best);
                    assert!(feasible(&m, &c.map));
                    for (a, b) in c.marg.iter().zip(&marg) {
                        close(*a, *b);
                    }
                }
            }
            if let Some(f) = probbit_ir::exact_frontier(actual, 100_000) {
                answered_frontier += 1;
                assert!(count > 0);
                close(f.logz, logz);
                close(f.map_logw, best);
                assert!(feasible(&m, &f.map));
                for (a, b) in f.marg.iter().zip(&marg) {
                    close(*a, *b);
                }
            }
        }
    }
    assert!(answered_components > 100 && answered_frontier > 100);
    eprintln!("320 independent oracles; 640 enumerations; {answered_components} component answers; {answered_frontier} frontier answers");
}

#[test]
fn constructors_reject_overflow_and_non_finite_weights() {
    assert!(Model::new(usize::MAX, 2, vec![], vec![], vec![], vec![], vec![]).is_err());
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Model::new(1, 1, vec![bad], vec![true], vec![None], vec![], vec![]).is_err());
        for c in [Coupling::Potts(bad), Coupling::Table(vec![bad])] {
            assert!(Model::new(
                2,
                1,
                vec![0.0; 2],
                vec![true; 2],
                vec![None; 2],
                vec![Pair { i: 0, j: 1, c }],
                vec![]
            )
            .is_err());
        }
    }
    assert!(Model::new(1, 2, vec![0.0], vec![true; 2], vec![None], vec![], vec![]).is_err());
    assert!(Model::new(
        2,
        2,
        vec![0.0; 4],
        vec![true; 4],
        vec![None; 2],
        vec![Pair {
            i: 0,
            j: 1,
            c: Coupling::Table(vec![0.0; 3])
        }],
        vec![]
    )
    .is_err());
}

#[test]
fn polish_never_returns_an_invalid_supplied_start() {
    let m = Model::new(
        2,
        2,
        vec![0.0; 4],
        vec![true; 4],
        vec![None; 2],
        vec![],
        vec![Cap::new(vec![(0, 0), (1, 0)], 1)],
    )
    .unwrap();
    for start in [vec![0, 0], vec![], vec![0], vec![0, 2], vec![0, 1, 0]] {
        assert!(
            probbit_ir::anneal_on(&m, Some(&start), &[1.0, 2.0], 0.0, 0, 1, 1, 7).is_none(),
            "accepted {start:?}"
        );
    }
}
