//! pbit-ir: the instruction set of the pbit virtual p-bit processor. Zero external crates.
//!
//! A program is a constrained categorical field over `n` variables, each taking a value in `0..k`:
//!   log w(x) = sum_i h[i*k + x_i] + sum_{pairs (i,j)} J_ij(x_i, x_j)
//!   hard rules: `allowed[i*k + v]` (forbidden values), `clamp[i]` (forced value; the what-if instruction),
//!               capacity: for every `Cap`, #{(i, v) in members : x_i == v} <= limit.
//! Instructions: `exact` (enumeration: log Z, marginals, top plans), `sample` (constraint-preserving Gibbs: heat-bath site
//! updates restricted to feasible values + Metropolis swap moves, multi-chain), `gate_stats` (the certification gate
//! over generic marginals), `Model::with_clamp` (what-if). Front-ends lower to this form: the assignment router in
//! pbit-decide is one (`Problem::lower`), and its results through the lowering are bit-identical to its own engine.
use pbit_core::Philox4x32;
use std::collections::HashMap;

/// A pairwise log-weight term between variables `i` and `j`.
#[derive(Clone, Debug, PartialEq)]
pub enum Coupling {
    /// `+w` when `x_i == x_j` (value indices compared): affinity, colouring penalties, ferromagnetic Ising.
    Potts(f64),
    /// `+table[x_i * k + x_j]`: any pairwise table (max-cut, anti-ferromagnets, preferences).
    Table(Vec<f64>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Pair { pub i: usize, pub j: usize, pub c: Coupling }
/// At most `limit` of the listed (variable, value) pairs may be active at once.
#[derive(Clone, Debug, PartialEq)]
pub struct Cap { pub members: Vec<(usize, usize)>, pub limit: usize }

#[derive(Clone, Debug)]
pub struct Model {
    pub n: usize, pub k: usize, pub h: Vec<f64>, pub allowed: Vec<bool>, pub clamp: Vec<Option<usize>>,
    pub pairs: Vec<Pair>, pub caps: Vec<Cap>,
    /// per variable: (other variable, pair index), sorted by other variable then pair index
    adj: Vec<Vec<(usize, usize)>>,
    /// per (i, v): the capacity constraints containing it
    cap_of: Vec<Vec<usize>>,
    /// per variable: allowed values, ascending
    cand: Vec<Vec<usize>>,
    /// every (i, v) is in at most one capacity constraint: feasibility is a capacitated bipartite matching
    partition: bool,
    /// hot-loop layout: per (i, v) its single capacity constraint (NONE = unconstrained; valid iff `partition`),
    /// per-constraint limits, and per-variable Potts neighbours (j, w) / table neighbours (j, pair), sorted by j
    bucket: Vec<u32>, limit: Vec<usize>, padj: Vec<Vec<(usize, f64)>>, tadj: Vec<Vec<(usize, usize)>>,
}
const NONE: u32 = u32::MAX;

impl Model {
    pub fn new(n: usize, k: usize, h: Vec<f64>, allowed: Vec<bool>, clamp: Vec<Option<usize>>, pairs: Vec<Pair>, caps: Vec<Cap>) -> Result<Model, String> {
        if h.len() != n * k || allowed.len() != n * k || clamp.len() != n { return Err("h / allowed must have n*k entries and clamp n".into()); }
        if k == 0 || k > 65535 { return Err("k must be in 1..=65535".into()); }
        if let Some(i) = clamp.iter().position(|c| c.map_or(false, |v| v >= k)) { return Err(format!("clamp of variable {i} out of range")); }
        let mut adj = vec![vec![]; n];
        for (q, p) in pairs.iter().enumerate() {
            if p.i >= n || p.j >= n || p.i == p.j { return Err(format!("pair {q}: bad variables ({}, {})", p.i, p.j)); }
            if let Coupling::Table(t) = &p.c { if t.len() != k * k { return Err(format!("pair {q}: table must have k*k entries")); } }
            adj[p.i].push((p.j, q)); adj[p.j].push((p.i, q));
        }
        for a in adj.iter_mut() { a.sort(); }
        let mut cap_of = vec![vec![]; n * k];
        for (c, cp) in caps.iter().enumerate() { for &(i, v) in &cp.members {
            if i >= n || v >= k { return Err(format!("capacity {c}: member ({i}, {v}) out of range")); }
            if cap_of[i * k + v].last() == Some(&c) { return Err(format!("capacity {c}: duplicate member ({i}, {v})")); }
            cap_of[i * k + v].push(c); } }
        let partition = cap_of.iter().all(|c| c.len() <= 1);
        let cand = (0..n).map(|i| (0..k).filter(|&v| allowed[i * k + v]).collect()).collect();
        let bucket = cap_of.iter().map(|c| c.first().map_or(NONE, |&b| b as u32)).collect();
        let limit = caps.iter().map(|c| c.limit).collect();
        let padj = adj.iter().map(|a| a.iter().filter_map(|&(j, q)| match pairs[q].c { Coupling::Potts(w) => Some((j, w)), _ => None }).collect()).collect();
        let tadj = adj.iter().map(|a| a.iter().copied().filter(|&(_, q)| matches!(pairs[q].c, Coupling::Table(_))).collect()).collect();
        Ok(Model { n, k, h, allowed, clamp, pairs, caps, adj, cap_of, cand, partition, bucket, limit, padj, tadj })
    }
    /// The same program at inverse temperature `beta`: every log-weight scaled, hard rules unchanged.
    pub fn scaled(&self, beta: f64) -> Model {
        let h = self.h.iter().map(|v| v * beta).collect();
        let pairs = self.pairs.iter().map(|p| Pair { i: p.i, j: p.j, c: match &p.c { Coupling::Potts(w) => Coupling::Potts(w * beta), Coupling::Table(t) => Coupling::Table(t.iter().map(|v| v * beta).collect()) } }).collect();
        Model::new(self.n, self.k, h, self.allowed.clone(), self.clamp.clone(), pairs, self.caps.clone()).expect("scaling keeps a valid model")
    }
    /// What-if: the same program with variable `i` forced to value `v`.
    pub fn with_clamp(&self, i: usize, v: usize) -> Model { let mut m = self.clone(); m.clamp[i] = Some(v); m }
    #[inline] pub fn ok(&self, i: usize, v: usize) -> bool { self.allowed[i * self.k + v] && self.clamp[i].map_or(true, |c| c == v) }
    /// Number of values variable `i` may take (allowed, and the clamp if set).
    pub fn cand_count(&self, i: usize) -> usize { match self.clamp[i] { Some(c) => self.allowed[i * self.k + c] as usize, None => self.cand[i].len() } }
    /// Every (variable, value) is in at most one capacity constraint.
    pub fn is_partition(&self) -> bool { self.partition }
    #[inline] fn coupling(&self, q: usize, me: usize, vme: usize, vo: usize) -> f64 {
        let p = &self.pairs[q];
        match &p.c { Coupling::Potts(w) => if vme == vo { *w } else { 0.0 }, Coupling::Table(t) => if p.i == me { t[vme * self.k + vo] } else { t[vo * self.k + vme] } }
    }
    pub fn logw(&self, x: &[usize]) -> f64 {
        let mut e = 0.0;
        for i in 0..self.n { e += self.h[i * self.k + x[i]];
            for &(j, q) in &self.adj[i] { if j > i { match &self.pairs[q].c { Coupling::Potts(w) => if x[i] == x[j] { e += *w; }, Coupling::Table(_) => e += self.coupling(q, i, x[i], x[j]) } } } }
        e
    }
    /// Number of broken rules: variables on a forbidden / non-clamped value plus total capacity overflow.
    pub fn violations(&self, x: &[usize]) -> usize {
        let mut load = vec![0usize; self.caps.len()]; let mut v = 0;
        for i in 0..self.n { if !self.ok(i, x[i]) { v += 1; } for &c in &self.cap_of[i * self.k + x[i]] { load[c] += 1; } }
        for (c, cp) in self.caps.iter().enumerate() { if load[c] > cp.limit { v += load[c] - cp.limit; } }
        v
    }
    /// Over-dispersed random feasible start. Partition models: capacitated bipartite matching by augmenting paths with a
    /// random insertion order and random value order (the assignment router's over-dispersed start, bit-identical through the lowering).
    /// Other models: randomised depth-first search that always branches on the unassigned variable with the fewest
    /// capacity-feasible values (dynamic MRV, random tie-break), with a 2M-node budget and a work budget of START_WORK
    /// capacity checks, after either of which (the node budget too) it retries in ascending value order (see below). None if the budgets run out, which
    /// is NOT a proof of infeasibility (`is_partition() == false`); for partition models None is a proof.
    pub fn feasible_init_rand(&self, rng: &mut Philox4x32) -> Option<Vec<usize>> { self.feasible_init_with(rng, START_WORK) }
    /// `feasible_init_rand` with the random attempt's work budget given (the ascending retries always get START_WORK).
    pub fn feasible_init_with(&self, rng: &mut Philox4x32, first_work: u64) -> Option<Vec<usize>> { self.feasible_init_until(rng, first_work, None) }
    /// `feasible_init_with` that also gives up (None, no further retries) once `deadline` passes (non-partition search
    /// only, clock read every 64 nodes (4096 was ~170 ms apart on a 200-job program); a start found before the deadline is identical).
    pub fn feasible_init_until(&self, rng: &mut Philox4x32, first_work: u64, deadline: Option<std::time::Instant>) -> Option<Vec<usize>> {
        let (n, k) = (self.n, self.k);
        let key: Vec<f64> = (0..n * k + n).map(|_| rng.f64()).collect();
        let order_of = |i: usize| { let mut o: Vec<usize> = self.cand[i].iter().copied().filter(|&v| self.ok(i, v)).collect();
            o.sort_by(|&u, &v| key[i * k + u].partial_cmp(&key[i * k + v]).unwrap()); o };
        let mut vars: Vec<usize> = (0..n).collect(); vars.sort_by(|&u, &v| key[n * k + u].partial_cmp(&key[n * k + v]).unwrap());
        let mut x = vec![usize::MAX; n];
        if self.partition {
            let mut on: Vec<Vec<usize>> = vec![vec![]; self.caps.len()];
            fn aug(m: &Model, i: usize, seen: &mut [bool], x: &mut [usize], on: &mut [Vec<usize>], order_of: &dyn Fn(usize) -> Vec<usize>) -> bool {
                let order = order_of(i);
                for &v in &order { match m.cap_of[i * m.k + v].first() { None => { x[i] = v; return true; }
                    Some(&b) => if on[b].len() < m.caps[b].limit { x[i] = v; on[b].push(i); return true; } } }
                for &v in &order { let b = match m.cap_of[i * m.k + v].first() { Some(&b) => b, None => continue }; if seen[b] { continue; } seen[b] = true;
                    for q in 0..on[b].len() { let j = on[b][q];
                        if aug(m, j, seen, x, on, order_of) { let pos = on[b].iter().position(|&z| z == j).unwrap(); on[b][pos] = i; x[i] = v; return true; } } }
                false
            }
            for &i in &vars { let mut seen = vec![false; self.caps.len()]; if !aug(self, i, &mut seen, &mut x, &mut on, &order_of) { return None; } }
            return Some(x);
        }
        let mut load = vec![0usize; self.caps.len()]; let (mut nodes, mut work) = (0u64, 0u64); let mut late = false;
        let rank: Vec<usize> = { let mut r = vec![0; n]; for (q, &i) in vars.iter().enumerate() { r[i] = q; } r }; // random tie-break
        #[allow(clippy::too_many_arguments)]
        fn dfs(m: &Model, left: usize, rank: &[usize], x: &mut [usize], load: &mut [usize], nodes: &mut u64, work: &mut u64, budget: u64, order_of: &dyn Fn(usize) -> Vec<usize>, mrv: bool, dl: (Option<std::time::Instant>, &mut bool)) -> bool {
            if left == 0 { return true; } *nodes += 1; if *nodes > 2_000_000 || *work > budget || *dl.1 { return false; }
            if *nodes & 63 == 0 { if let Some(d) = dl.0 { if std::time::Instant::now() >= d { *dl.1 = true; return false; } } }
            let fits = |i: usize, v: usize, load: &[usize]| m.cap_of[i * m.k + v].iter().all(|&c| load[c] < m.limit[c]);
            let mut pick = (usize::MAX, usize::MAX, 0usize); // (feasible count, rank, var)
            for i in 0..m.n { if x[i] != usize::MAX { continue; }
                let cnt = m.cand[i].iter().filter(|&&v| { *work += 1 + m.cap_of[i * m.k + v].len() as u64; m.ok(i, v) && fits(i, v, load) }).count();
                let key = if mrv { (cnt, rank[i]) } else { (0, i) }; if key < (pick.0, pick.1) { pick = (key.0, key.1, i); } if cnt == 0 { return false; } }
            let i = pick.2;
            for v in order_of(i) { if !fits(i, v, load) { continue; }
                for &c in &m.cap_of[i * m.k + v] { load[c] += 1; } x[i] = v;
                if dfs(m, left - 1, rank, x, load, nodes, work, budget, order_of, mrv, (dl.0, &mut *dl.1)) { return true; }
                for &c in &m.cap_of[i * m.k + v] { load[c] -= 1; } x[i] = usize::MAX; }
            false
        }
        if dfs(self, n, &rank, &mut x, &mut load, &mut nodes, &mut work, first_work, &order_of, true, (deadline, &mut late)) { return Some(x); }
        if late { return None; }
        // search exhausted within both budgets = no feasible plan. The 2M-node budget running out is not a proof either
        // (it used to return None here, skipping the retry: 3 of 8 chain streams on a 100-job schedule never started)
        if work <= first_work && nodes <= 2_000_000 { return None; }
        // The random value order thrashed on a 200-job precedence schedule (82,678 caps; a 2M-node search at O(n k caps)
        // per node). Once START_WORK cap checks are spent, retry in ascending value order (earliest slot first), first with the
        // same MRV, then in input variable order (ASAP when jobs are numbered in precedence order), each within START_WORK; the
        // caller's uniform sweeps then disperse the start. Programs whose random search finishes within START_WORK are unchanged
        // (START_FALLBACKS counts starts that needed a retry).
        START_FALLBACKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let asc = |i: usize| self.cand[i].iter().copied().filter(|&v| self.ok(i, v)).collect::<Vec<usize>>();
        for mrv in [true, false] { x.fill(usize::MAX); load.fill(0); nodes = 0; work = 0;
            if dfs(self, n, &rank, &mut x, &mut load, &mut nodes, &mut work, START_WORK, &asc, mrv, (deadline, &mut late)) { return Some(x); } if late { return None; } }
        // MRV's tie-break decides whether the ascending search thrashes (100-job schedule: 2 of 8 chain streams failed
        // both retries above, 5 of the other 6 tie-breaks succeed); START_RERANKS more MRV attempts with fresh random tie-breaks.
        // Runs only after every earlier attempt failed, so the starts (and RNG streams) of programs that start today are unchanged.
        for _ in 0..START_RERANKS { let mut r: Vec<(f64, usize)> = (0..n).map(|i| (rng.f64(), i)).collect(); r.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let mut rank = vec![0; n]; for (q, &(_, i)) in r.iter().enumerate() { rank[i] = q; }
            x.fill(usize::MAX); load.fill(0); nodes = 0; work = 0;
            if dfs(self, n, &rank, &mut x, &mut load, &mut nodes, &mut work, START_WORK, &asc, true, (deadline, &mut late)) { return Some(x); } if late { return None; } }
        None
    }
}

// ---------------- exact: enumeration ----------------
pub struct Exact { pub logz: f64, pub marg: Vec<f64>, pub top: Vec<(f64, Vec<usize>)>, pub n_feasible: u64 }
/// Enumerate every feasible plan if there are at most `limit` (None otherwise; internal DFS nodes are bounded too, so a
/// large tight instance declines instead of backtracking for minutes). `top` = the `k` most likely plans with probabilities.
/// The dynamic-MRV search (non-partition programs) also declines once it spends more than max(64 x limit,
/// EXACT_GAP_WORK) capacity checks without finding a new plan: on a 200-job schedule its FIRST plan took 1.66 G checks
/// (1.15 s, 19,564 dead ends), while every answering sudoku / colouring / scheduling oracle run needs <= 5.1 M between plans.
pub fn exact(m: &Model, topk: usize, limit: u64) -> Option<Exact> { exact_until(m, topk, limit, None).0 }
/// `exact` whose dynamic-MRV search, once past its gap budget, keeps searching until `deadline` (clock read every 64
/// nodes) instead of declining: the gap budget stopped the search for the FIRST plan, which on an infeasible CSP is also the
/// proof (hard 3-colourings an earlier binary proved infeasible in 0.13-4.3 s came back `refused`). None = `exact`.
/// The bool is true if the search ran past its gap budget on the deadline's credit (the caller's budget was spent).
pub fn exact_until(m: &Model, topk: usize, limit: u64, deadline: Option<std::time::Instant>) -> (Option<Exact>, bool) { exact_within(m, topk, limit, deadline, None) }
/// `exact_until` with a HARD stop: past `hard` (clock read every 64 nodes, in both passes and both search orders) the
/// search declines (None), whatever its plan count or gap budget. This is the opt-in `--exact-ms` cap of the CLI; None = `exact_until`.
pub fn exact_within(m: &Model, topk: usize, limit: u64, deadline: Option<std::time::Instant>, hard: Option<std::time::Instant>) -> (Option<Exact>, bool) {
    deep(m.n, || exact_search(m, topk, limit, deadline, hard))
}
fn exact_search(m: &Model, topk: usize, limit: u64, deadline: Option<std::time::Instant>, hard: Option<std::time::Instant>) -> (Option<Exact>, bool) {
    if past(hard) { return (None, false); }
    // Non-partition programs (CSPs: sudoku, colouring) branch on the most constrained variable (dynamic MRV), like a
    // classical backtracking solver; partition programs keep the static order (bit-identical to the assignment front-end)
    let mrv = !m.partition;
    let mut st = Ex { m, x: vec![if mrv { usize::MAX } else { 0 }; m.n], load: vec![0; m.caps.len()], mx: f64::NEG_INFINITY, sum: 0.0, marg: vec![0.0; m.n * m.k], top: vec![], topk, n: 0, limit, pass: 0, nodes: 0,
        node_limit: limit.saturating_mul(64).max(1_000_000), work: 0, last: 0, gap_limit: limit.saturating_mul(64).max(EXACT_GAP_WORK), cut: false, deadline, extended: false, hard, stop: 0,
        wsum: (0..m.n).map(|i| m.cand[i].iter().map(|&v| 1 + m.cap_of[i * m.k + v].len() as u64).sum()).collect() };
    st.stop = st.arm();
    let go = |st: &mut Ex| if mrv { st.dfs_mrv(m.n, 0.0) } else { st.dfs(0, 0.0) };
    go(&mut st); let ext = st.extended; if st.cut || st.n > limit || st.nodes > st.node_limit { return (None, ext); }
    if st.n == 0 { return (Some(Exact { logz: f64::NEG_INFINITY, marg: vec![0.0; m.n * m.k], top: vec![], n_feasible: 0 }), ext); }
    // pass 1 walks the same tree as pass 0 did within its bounds; the counters restart (pass 1 used to inherit pass 0's
    // node count, so a pass 0 with more than node_limit / 2 nodes was cut short in pass 1 and returned a partial "exact" sum).
    // No gap cut in pass 1 (pass 0 finished the tree, possibly past the gap budget on the deadline's credit, and a
    // pass 1 cut would return a partial sum; without a deadline pass 0's gaps all fit the budget, so this changes nothing)
    st.pass = 1; st.n = 0; st.nodes = 0; st.work = 0; st.last = 0; st.gap_limit = u64::MAX; st.deadline = None; st.stop = st.arm(); go(&mut st);
    if st.cut { return (None, ext); } // only the hard stop cuts pass 1; its partial sum is not an answer
    let z = st.sum; for v in st.marg.iter_mut() { *v /= z; }
    let logz = st.mx + z.ln();
    let top = st.top.into_iter().map(|(lw, x)| ((lw - logz).exp(), x)).collect();
    (Some(Exact { logz, marg: st.marg, top, n_feasible: st.n }), ext)
}
struct Ex<'a> { m: &'a Model, x: Vec<usize>, load: Vec<usize>, mx: f64, sum: f64, marg: Vec<f64>, top: Vec<(f64, Vec<usize>)>, topk: usize, n: u64, limit: u64, pass: u8, nodes: u64, node_limit: u64,
    /// dynamic-MRV only: capacity checks so far, at the last plan found, the allowed gap, and whether the gap ran out
    /// (`cut` is also set by the hard stop, in both search orders)
    work: u64, last: u64, gap_limit: u64, cut: bool,
    /// Past the gap budget, keep searching until this instant; `extended` = that happened
    deadline: Option<std::time::Instant>, extended: bool,
    /// Hard stop (`exact_within`) and the node count that triggers the next check (`halt`): the node limit without a
    /// hard stop, so the default path keeps a single comparison
    hard: Option<std::time::Instant>, stop: u64,
    /// Per variable, the checks one MRV scan of it counts (sum over its candidates of 1 + |cap_of|): the same count earlier code
    /// summed inside the scan's filter at every node, which cost 18-21% on 3-colouring proofs (1.4-4.2% on sudoku)
    wsum: Vec<u64> }
/// The MRV enumeration's minimum patience between two plans found (capacity checks; ~70 ms on an Apple M4).
pub const EXACT_GAP_WORK: u64 = 100_000_000;
impl<'a> Ex<'a> {
    fn arm(&self) -> u64 { if self.hard.is_some() { self.node_limit.min(self.nodes + 63) } else { self.node_limit } }
    /// The real stop conditions, reached only past `stop`: plan / node limit, a cut, the hard stop (clock every 64 nodes).
    #[cold]
    fn halt(&mut self) -> bool {
        if self.n > self.limit || self.nodes > self.node_limit || self.cut { return true; }
        if past(self.hard) { self.cut = true; return true; }
        self.stop = self.arm(); false
    }
    fn leaf(&mut self, lw: f64) {
        let (m, k) = (self.m, self.m.k); self.n += 1; self.last = self.work;
        if self.pass == 0 { if lw > self.mx { self.mx = lw; } return; }
        let w = (lw - self.mx).exp(); self.sum += w;
        for j in 0..m.n { self.marg[j * k + self.x[j]] += w; }
        if self.top.len() < self.topk || lw > self.top.last().unwrap().0 {
            self.top.push((lw, self.x.clone())); self.top.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap()); self.top.truncate(self.topk); }
    }
    /// Dynamic-MRV enumeration (unassigned = usize::MAX): branch on the unassigned variable with the fewest capacity-feasible
    /// values (lowest index on ties, so both passes visit the same tree); a variable with none prunes the branch.
    fn dfs_mrv(&mut self, left: usize, lw: f64) {
        if (self.n > self.limit || self.nodes > self.stop || self.cut) && self.halt() { return; }
        if self.work - self.last > self.gap_limit {
            match self.deadline { Some(d) if self.nodes & 63 != 0 || std::time::Instant::now() < d => self.extended = true, _ => { self.cut = true; return; } } }
        self.nodes += 1; let m = self.m; let k = m.k;
        if left == 0 { self.leaf(lw); return; }
        let fits = |ld: &[usize], i: usize, v: usize| m.ok(i, v) && m.cap_of[i * k + v].iter().all(|&c| ld[c] < m.limit[c]);
        let (mut pick, mut w) = ((usize::MAX, 0usize), 0u64); // w: capacity checks (upper bound, as START_WORK counts them)
        for i in 0..m.n { if self.x[i] != usize::MAX { continue; }
            w += self.wsum[i];
            let cnt = m.cand[i].iter().filter(|&&v| fits(&self.load, i, v)).count();
            if cnt < pick.0 { if cnt == 0 { self.work += w; return; } pick = (cnt, i); } }
        self.work += w;
        let i = pick.1;
        for &v in &m.cand[i] { if !fits(&self.load, i, v) { continue; }
            let mut d = m.h[i * k + v];
            for &(j, w) in &m.padj[i] { if self.x[j] == v { d += w; } } // unassigned (usize::MAX) never matches
            for &(j, q) in &m.tadj[i] { if self.x[j] != usize::MAX { d += m.coupling(q, i, v, self.x[j]); } }
            for &c in &m.cap_of[i * k + v] { self.load[c] += 1; } self.x[i] = v;
            self.dfs_mrv(left - 1, lw + d);
            for &c in &m.cap_of[i * k + v] { self.load[c] -= 1; } self.x[i] = usize::MAX;
        }
    }
    fn dfs(&mut self, i: usize, lw: f64) {
        if (self.n > self.limit || self.nodes > self.stop) && self.halt() { return; }
        self.nodes += 1; let m = self.m; let k = m.k;
        if i == m.n { self.leaf(lw); return; }
        for &v in &m.cand[i] {
            if !m.ok(i, v) { continue; }
            let b = m.bucket[i * k + v];
            if m.partition { if b != NONE && self.load[b as usize] >= m.limit[b as usize] { continue; } }
            else if m.cap_of[i * k + v].iter().any(|&c| self.load[c] >= m.limit[c]) { continue; }
            let mut d = m.h[i * k + v];
            for &(j, w) in &m.padj[i] { if j >= i { break; } if self.x[j] == v { d += w; } }
            for &(j, q) in &m.tadj[i] { if j < i { d += m.coupling(q, i, v, self.x[j]); } }
            if m.partition { if b != NONE { self.load[b as usize] += 1; } } else { for &c in &m.cap_of[i * k + v] { self.load[c] += 1; } }
            self.x[i] = v; self.dfs(i + 1, lw + d);
            if m.partition { if b != NONE { self.load[b as usize] -= 1; } } else { for &c in &m.cap_of[i * k + v] { self.load[c] -= 1; } }
        }
    }
}

// ---------------- exact: frontier DP (the assignment router's frontier tier, generalised) ----------------
/// Per connected component of the coupling graph: members (ascending), the capacity constraints its allowed values touch
/// (ascending), and its assignments bucketed by their load vector on those caps: (loads, log-sum w, [(log w, values)]).
struct CTab { mem: Vec<usize>, caps: Vec<usize>, buckets: Vec<(Vec<u8>, f64, Vec<(f64, Vec<u16>)>)>, /// exp(bucket log-sum - max)
    bw: Vec<f64> }
/// Components (union-find over `pairs`, ordered by smallest member) with their bucket tables. None if a component has
/// more than 4096 assignments. Assignments whose load on one cap already exceeds min(limit, 255) are dropped here.
/// Also None once `hard` passes (clock read per component).
fn comp_tabs(m: &Model, hard: Option<std::time::Instant>) -> Option<Vec<CTab>> {
    let (n, k) = (m.n, m.k); let mut par: Vec<usize> = (0..n).collect();
    fn find(par: &mut [usize], mut x: usize) -> usize { while par[x] != x { par[x] = par[par[x]]; x = par[x]; } x }
    for p in &m.pairs { let (a, b) = (find(&mut par, p.i), find(&mut par, p.j)); if a != b { par[a.max(b)] = a.min(b); } } // root = smallest member
    let mut slot = vec![usize::MAX; n]; let mut mems: Vec<Vec<usize>> = vec![];
    for i in 0..n { let r = find(&mut par, i); if slot[r] == usize::MAX { slot[r] = mems.len(); mems.push(vec![]); } mems[slot[r]].push(i); }
    let vals = |i: usize| m.cand[i].iter().copied().filter(move |&v| m.ok(i, v));
    let mut out = vec![];
    for mem in mems {
        if past(hard) || mem.iter().map(|&i| vals(i).count() as f64).product::<f64>() > 4096.0 { return None; }
        let mut caps: Vec<usize> = mem.iter().flat_map(|&i| vals(i).map(move |v| m.bucket[i * k + v])).filter(|&b| b != NONE).map(|b| b as usize).collect();
        caps.sort(); caps.dedup();
        struct Rec<'a> { m: &'a Model, mem: &'a [usize], caps: &'a [usize], x: Vec<usize>, load: Vec<usize>, map: HashMap<Vec<u8>, Vec<(f64, Vec<u16>)>> }
        fn rec(r: &mut Rec, q: usize, lw: f64) {
            let m = r.m; let k = m.k;
            if q == r.mem.len() { r.map.entry(r.load.iter().map(|&l| l as u8).collect()).or_default().push((lw, r.mem.iter().map(|&i| r.x[i] as u16).collect())); return; }
            let i = r.mem[q];
            for &v in &m.cand[i] { if !m.ok(i, v) { continue; }
                let b = m.bucket[i * k + v]; let lane = if b == NONE { None } else { r.caps.binary_search(&(b as usize)).ok() };
                if let Some(l) = lane { if r.load[l] + 1 > m.limit[b as usize].min(255) { continue; } }
                let mut d = m.h[i * k + v]; // couplings to earlier members (every neighbour is in this component)
                for &(j, w) in &m.padj[i] { if j >= i { break; } if r.x[j] == v { d += w; } }
                for &(j, qq) in &m.tadj[i] { if j < i { d += m.coupling(qq, i, v, r.x[j]); } }
                if let Some(l) = lane { r.load[l] += 1; } r.x[i] = v; rec(r, q + 1, lw + d); if let Some(l) = lane { r.load[l] -= 1; }
            }
        }
        let mut r = Rec { m, mem: &mem, caps: &caps, x: vec![0; n], load: vec![0; caps.len()], map: HashMap::new() };
        rec(&mut r, 0, 0.0);
        let mut buckets: Vec<(Vec<u8>, f64, Vec<(f64, Vec<u16>)>)> = r.map.into_iter().map(|(lv, v)| {
            let mx = v.iter().map(|x| x.0).fold(f64::NEG_INFINITY, f64::max); let lse = mx + v.iter().map(|x| (x.0 - mx).exp()).sum::<f64>().ln(); (lv, lse, v) }).collect();
        buckets.sort_by(|a, b| a.0.cmp(&b.0)); // HashMap order is random: sort for determinism
        let bm = buckets.iter().map(|b| b.1).fold(f64::NEG_INFINITY, f64::max); let bw = buckets.iter().map(|b| (b.1 - bm).exp()).collect();
        out.push(CTab { mem, caps, buckets, bw });
    }
    Some(out)
}
/// Exact result of `exact_frontier`: marginals, one exact MAP plan and its log w, log Z, and the largest DP layer.
pub struct FrontierExact { pub marg: Vec<f64>, pub map: Vec<usize>, pub map_logw: f64, pub logz: f64, pub max_states: usize }
/// Default DP layer cap (the assignment router's `FRONTIER_MAX_STATES`).
pub const FRONTIER_MAX_STATES: usize = 1 << 12;
/// EXACT marginals, log Z and an exact MAP plan without enumerating the joint space, for partition programs (every
/// (var, value) in at most one capacity constraint) whose components share caps thinly. Components of the coupling graph
/// are eliminated in a greedy min-frontier order; the DP state is the load vector on the frontier caps (used by a
/// component already processed AND one still to come). Sum-product forward/backward -> marginals; max-product with
/// back-pointers -> MAP. None (decline, use the sampler) if the program is not a partition, a component has > 4096
/// assignments, the frontier exceeds 16 caps, a layer exceeds `max_states`, or nothing is feasible.
/// On an assignment `Problem::lower()` the components are its groups (ungrouped tasks = singletons) and the caps its
/// agents, so this is the router's earlier frontier DP; results agree with it to float rounding (component order can differ from its group order).
pub fn exact_frontier(m: &Model, max_states: usize) -> Option<FrontierExact> { exact_frontier_until(m, max_states, None) }
/// Whether an optional hard stop has passed.
fn past(hard: Option<std::time::Instant>) -> bool { hard.is_some_and(|h| std::time::Instant::now() >= h) }
/// `exact_frontier` that declines once `hard` passes (clock read per component while tabulating and per DP step,
/// forward and backward). This is the opt-in `--exact-ms` cap of the CLI; None = `exact_frontier`.
pub fn exact_frontier_until(m: &Model, max_states: usize, hard: Option<std::time::Instant>) -> Option<FrontierExact> {
    if !m.partition || past(hard) { return None; }
    deep(m.n, || frontier(m, max_states, hard))
}
fn frontier(m: &Model, max_states: usize, hard: Option<std::time::Instant>) -> Option<FrontierExact> {
    // Carried cap loads live in 8-bit lanes, and a transition past 255 was dropped as if it broke the cap, so a cap that
    // could hold more than 255 returned a truncated distribution labelled exact (600 free spins, at-most-600 cap: log Z 406.99 vs
    // 415.89). Decline whenever a cap's limit and its member count both exceed 255 (a load can then pass 255)
    let mut msz = vec![0usize; m.caps.len()]; for q in 0..m.n * m.k { for &c in &m.cap_of[q] { msz[c] += 1; } }
    if (0..m.caps.len()).any(|a| m.limit[a] > 255 && msz[a] > 255) { return None; }
    let gs = comp_tabs(m, hard)?; let (na, kk) = (m.caps.len(), gs.len()); if kk == 0 { return None; }
    // greedy min-frontier elimination order (static: depends only on the program)
    let sel = { let mut rem = vec![0usize; na]; for g in &gs { for &a in &g.caps { rem[a] += 1; } }
        let mut open = vec![false; na]; let mut n_open = 0usize; let mut left: Vec<usize> = (0..kk).collect(); let mut ord = Vec::with_capacity(kk);
        while !left.is_empty() { let mut best = (usize::MAX, 0usize);
            for (q, &g) in left.iter().enumerate() { let ag = &gs[g].caps;
                let after = n_open - ag.iter().filter(|&&a| open[a] && rem[a] == 1).count() + ag.iter().filter(|&&a| !open[a] && rem[a] > 1).count();
                if after < best.0 { best = (after, q); } }
            let g = left.remove(best.1); for &a in &gs[g].caps { rem[a] -= 1; let o = rem[a] > 0; if o != open[a] { if o { n_open += 1; } else { n_open -= 1; } } open[a] = o; } ord.push(g); }
        ord };
    let mut last = vec![0usize; na]; for (j, &g) in sel.iter().enumerate() { for &a in &gs[g].caps { last[a] = j; } }
    // fr[j] = caps used at some step <= j and at some later step (sorted)
    let fr: Vec<Vec<usize>> = { let mut cur: Vec<usize> = vec![]; let mut out = Vec::with_capacity(kk);
        for (j, &g) in sel.iter().enumerate() { cur.retain(|&a| last[a] > j); for &a in &gs[g].caps { if last[a] > j && !cur.contains(&a) { cur.push(a); } }
            let mut f = cur.clone(); f.sort(); out.push(f); }
        out };
    if fr.iter().any(|f| f.len() > 16) { return None; }
    let lane = |f: &[usize], a: usize| f.iter().position(|&b| b == a);
    type Step = (Vec<(Option<usize>, Option<usize>)>, Vec<(usize, usize)>);
    let steps: Vec<Step> = (0..kk).map(|j| { let g = &gs[sel[j]]; let prev: &[usize] = if j == 0 { &[] } else { &fr[j - 1] };
        (g.caps.iter().map(|&a| (lane(prev, a), lane(&fr[j], a))).collect(),
         prev.iter().enumerate().filter(|(_, a)| !g.caps.contains(a)).filter_map(|(lp, &a)| lane(&fr[j], a).map(|ln| (lp, ln))).collect()) }).collect();
    let trans = |j: usize, s: u128, bi: usize| -> Option<u128> {
        let g = &gs[sel[j]]; let (roles, carried) = &steps[j]; let lv = &g.buckets[bi].0; let mut ns: u128 = 0;
        for &(lp, ln) in carried { ns |= ((s >> (8 * lp)) & 0xff) << (8 * ln); }
        for (q, &a) in g.caps.iter().enumerate() { let u = lv[q] as u128 + roles[q].0.map_or(0, |lp| (s >> (8 * lp)) & 0xff);
            if u > 255 || u as usize > m.limit[a] { return None; } if let Some(ln) = roles[q].1 { ns |= u << (8 * ln); } }
        Some(ns)
    };
    let bmax: Vec<Vec<f64>> = gs.iter().map(|g| g.buckets.iter().map(|b| b.2.iter().map(|x| x.0).fold(f64::NEG_INFINITY, f64::max)).collect()).collect();
    let mut alpha: Vec<Vec<(u128, f64)>> = vec![vec![(0, 1.0)]]; let mut mxp: Vec<Vec<(u128, f64)>> = vec![vec![(0, 0.0)]];
    let mut tr: Vec<Vec<(u128, u32, u16, f64)>> = Vec::with_capacity(kk); let mut bp: Vec<Vec<(u32, u16)>> = Vec::with_capacity(kk); let mut mstates = 1;
    // log Z = per-layer normalisers + each component's bucket-weight offset (bw = exp(lse - max lse))
    let mut logz: f64 = gs.iter().map(|g| g.buckets.iter().map(|b| b.1).fold(f64::NEG_INFINITY, f64::max)).sum();
    if !logz.is_finite() { return None; } // a component with no assignment: nothing feasible
    for j in 0..kk {
        if past(hard) { return None; }
        let g = &gs[sel[j]]; let mut t: Vec<(u128, u32, u16, f64)> = vec![];
        for (si, &(s, w)) in alpha[j].iter().enumerate() { for bi in 0..g.buckets.len() { if let Some(ns) = trans(j, s, bi) { t.push((ns, si as u32, bi as u16, w * g.bw[bi])); } } }
        t.sort_by_key(|e| e.0);
        let mut v: Vec<(u128, f64)> = vec![]; let mut mm: Vec<(u128, f64)> = vec![]; let mut b: Vec<(u32, u16)> = vec![];
        for e in &t { let lm = mxp[j][e.1 as usize].1 + bmax[sel[j]][e.2 as usize];
            match v.last_mut() { Some(l) if l.0 == e.0 => { l.1 += e.3; let ml = mm.last_mut().unwrap(); if lm > ml.1 { ml.1 = lm; *b.last_mut().unwrap() = (e.1, e.2); } },
                _ => { v.push((e.0, e.3)); mm.push((e.0, lm)); b.push((e.1, e.2)); } } }
        if v.is_empty() || v.len() > max_states { return None; }
        mstates = mstates.max(v.len());
        let mx = v.iter().map(|e| e.1).fold(0.0, f64::max); for e in v.iter_mut() { e.1 /= mx; } for e in t.iter_mut() { e.3 /= mx; }
        logz += mx.ln(); alpha.push(v); mxp.push(mm); tr.push(t); bp.push(b);
    }
    // backward messages (per-layer normalised) and bucket posteriors -> marginals
    let k = m.k; let mut beta: Vec<f64> = vec![1.0]; let mut marg = vec![0.0; m.n * k];
    for j in (0..kk).rev() {
        if past(hard) { return None; }
        let g = &gs[sel[j]]; let t = &tr[j]; let nxt = &alpha[j + 1];
        let bidx = |key: u128| nxt.binary_search_by(|e| e.0.cmp(&key)).unwrap();
        let mut post = vec![0.0; g.buckets.len()]; let mut nb = vec![0.0; alpha[j].len()]; let mut tot = 0.0;
        for e in t { let bn = beta[bidx(e.0)]; let w = e.3 * bn; post[e.2 as usize] += w; tot += w; nb[e.1 as usize] += g.bw[e.2 as usize] * bn; }
        for (bi, &pw) in post.iter().enumerate() { if pw <= 0.0 { continue; } let pb = pw / tot; let asg = &g.buckets[bi].2; let lse = g.buckets[bi].1;
            for x in asg { let w = pb * (x.0 - lse).exp(); for (q, &i) in g.mem.iter().enumerate() { marg[i * k + x.1[q] as usize] += w; } } }
        let mb = nb.iter().cloned().fold(0.0, f64::max); if mb > 0.0 { for v in nb.iter_mut() { *v /= mb; } } beta = nb;
    }
    // MAP: back-pointers from the single final key (the last frontier is empty), then the best assignment in each bucket
    let mut map = vec![0usize; m.n]; let mut idx = 0usize;
    for j in (0..kk).rev() { let (si, bi) = bp[j][idx]; let g = &gs[sel[j]];
        let x = g.buckets[bi as usize].2.iter().max_by(|a, b| a.0.partial_cmp(&b.0).unwrap()).unwrap();
        for (q, &i) in g.mem.iter().enumerate() { map[i] = x.1[q] as usize; } idx = si as usize; }
    let map_logw = m.logw(&map); logz += alpha[kk][0].1.ln();
    Some(FrontierExact { marg, map, map_logw, logz, max_states: mstates })
}

// ---------------- sample: constraint-preserving Gibbs ----------------
pub struct Chain<'a> { pub m: &'a Model, pub x: Vec<usize>, load: Vec<usize>, rng: Philox4x32, w: Vec<f64>, dl: Vec<(usize, i64)> }
impl<'a> Chain<'a> {
    pub fn new(m: &'a Model, seed: u64, stream: u64) -> Option<Self> { Self::new_until(m, seed, stream, None) }
    /// `new` whose feasible-start search gives up at `deadline` (a wall-clock sampling budget's end).
    pub fn new_until(m: &'a Model, seed: u64, stream: u64, deadline: Option<std::time::Instant>) -> Option<Self> {
        let mut rng = Philox4x32::new(seed, stream);
        let x = m.feasible_init_until(&mut rng, START_WORK, deadline)?;
        let mut load = vec![0; m.caps.len()]; for (i, &v) in x.iter().enumerate() { for &c in &m.cap_of[i * m.k + v] { load[c] += 1; } }
        let mut c = Chain { m, x, load, rng, w: vec![f64::NEG_INFINITY; m.k], dl: vec![] };
        for _ in 0..5 { c.sweep_uniform(); } // over-dispersion: uniform over feasible values (h ignored)
        Some(c)
    }
    /// moving variable i from a0 to v keeps every capacity constraint
    #[inline] fn cap_ok(&self, i: usize, a0: usize, v: usize) -> bool {
        let m = self.m; let k = m.k;
        if m.partition { let b = m.bucket[i * k + v]; return b == NONE || b == m.bucket[i * k + a0] || self.load[b as usize] < m.limit[b as usize]; }
        let old = &m.cap_of[i * k + a0];
        m.cap_of[i * k + v].iter().all(|c| old.contains(c) || self.load[*c] < m.limit[*c])
    }
    #[inline] fn set(&mut self, i: usize, v: usize) {
        let m = self.m; let k = m.k;
        if m.partition { let (b0, b1) = (m.bucket[i * k + self.x[i]], m.bucket[i * k + v]);
            if b0 != NONE { self.load[b0 as usize] -= 1; } if b1 != NONE { self.load[b1 as usize] += 1; } self.x[i] = v; return; }
        for &c in &m.cap_of[i * k + self.x[i]] { self.load[c] -= 1; } for &c in &m.cap_of[i * k + v] { self.load[c] += 1; } self.x[i] = v;
    }
    fn sweep_uniform(&mut self) {
        let m = self.m;
        for i in 0..m.n { let a0 = self.x[i]; let mut n = 0;
            for &v in &m.cand[i] { if m.ok(i, v) && (v == a0 || self.cap_ok(i, a0, v)) { n += 1; } }
            let mut r = self.rng.below(n);
            for &v in &m.cand[i] { if m.ok(i, v) && (v == a0 || self.cap_ok(i, a0, v)) { if r == 0 { self.set(i, v); break; } r -= 1; } } }
    }
    /// Heat-bath over the feasible values of variable i given all others.
    #[inline] pub fn site(&mut self, i: usize) {
        let m = self.m; let k = m.k; let a0 = self.x[i]; let cand = &m.cand[i];
        if m.partition { let b0 = m.bucket[i * k + a0];
            for &v in cand { let b = m.bucket[i * k + v];
                self.w[v] = if m.ok(i, v) && (v == a0 || b == NONE || b == b0 || self.load[b as usize] < m.limit[b as usize]) { m.h[i * k + v] } else { f64::NEG_INFINITY }; }
        } else { for &v in cand { self.w[v] = if m.ok(i, v) && (v == a0 || self.cap_ok(i, a0, v)) { m.h[i * k + v] } else { f64::NEG_INFINITY }; } }
        // w[b] of a non-candidate b may be stale; it is never read (loops use cand; Potts checks allowed first)
        for &(j, wq) in &m.padj[i] { let b = self.x[j]; if m.allowed[i * k + b] && self.w[b] > f64::NEG_INFINITY { self.w[b] += wq; } }
        for &(j, q) in &m.tadj[i] { let b = self.x[j]; for &v in cand { if self.w[v] > f64::NEG_INFINITY { self.w[v] += m.coupling(q, i, v, b); } } }
        let mut mx = f64::NEG_INFINITY; for &v in cand { if self.w[v] > mx { mx = self.w[v]; } }
        let mut tot = 0.0; for &v in cand { let e = if self.w[v] == f64::NEG_INFINITY { 0.0 } else { (self.w[v] - mx).exp() }; self.w[v] = e; tot += e; }
        let mut r = self.rng.f64() * tot; let mut pick = a0;
        for &v in cand { if self.w[v] > 0.0 { pick = v; if r < self.w[v] { break; } r -= self.w[v]; } }
        self.set(i, pick);
    }
    /// log-weight of i at value v from its unary term and every coupling except the one(s) to `skip`
    fn pair_e(&self, i: usize, v: usize, skip: usize) -> f64 {
        let m = self.m; let mut e = m.h[i * m.k + v];
        for &(j, w) in &m.padj[i] { if j != skip && self.x[j] == v { e += w; } }
        for &(j, q) in &m.tadj[i] { if j != skip { e += m.coupling(q, i, v, self.x[j]); } } e
    }
    /// Metropolis exchange of the values of two random variables (keeps per-value capacity loads; checked in general).
    #[inline] pub fn swap(&mut self) {
        let m = self.m; let k = m.k; let i = self.rng.below(m.n); let j = self.rng.below(m.n);
        let (ai, aj) = (self.x[i], self.x[j]);
        if i == j || ai == aj || !m.ok(i, aj) || !m.ok(j, ai) { return; }
        let balanced = m.partition && m.bucket[i * k + ai] == m.bucket[j * k + ai] && m.bucket[i * k + aj] == m.bucket[j * k + aj];
        if !balanced && (!m.cap_of[i * k + ai].is_empty() || !m.cap_of[i * k + aj].is_empty() || !m.cap_of[j * k + ai].is_empty() || !m.cap_of[j * k + aj].is_empty()) {
            self.dl.clear();
            for (idx, s) in [(i * k + ai, -1i64), (i * k + aj, 1), (j * k + aj, -1), (j * k + ai, 1)] { for &c in &m.cap_of[idx] {
                match self.dl.iter_mut().find(|e| e.0 == c) { Some(e) => e.1 += s, None => self.dl.push((c, s)) } } }
            if self.dl.iter().any(|&(c, d)| d > 0 && self.load[c] as i64 + d > m.limit[c] as i64) { self.dl.clear(); return; }
        }
        let mut e0 = self.pair_e(i, ai, j) + self.pair_e(j, aj, i);
        let mut e1 = self.pair_e(i, aj, j) + self.pair_e(j, ai, i);
        // Potts terms between i and j vanish before and after (ai != aj); tables do not
        for &(o, q) in &m.tadj[i] { if o == j { e0 += m.coupling(q, i, ai, aj); e1 += m.coupling(q, i, aj, ai); } }
        if e1 >= e0 || self.rng.f64() < (e1 - e0).exp() {
            if !self.dl.is_empty() { for &(c, d) in &self.dl { self.load[c] = (self.load[c] as i64 + d) as usize; } self.dl.clear(); }
            self.x[i] = aj; self.x[j] = ai;
        } else { self.dl.clear(); }
    }
    /// one sweep = n site updates + max(n/2, 2) swap attempts
    pub fn sweep(&mut self) { for i in 0..self.m.n { self.site(i); } for _ in 0..(self.m.n / 2).max(2) { self.swap(); } }
    pub fn logw(&self) -> f64 { self.m.logw(&self.x) }
    /// A chain AT a feasible state `x`: no random feasible-start search, no over-dispersion sweeps (the anneal's warm
    /// start). `anneal` used `new` + `set_state` once per beta stage and discarded the search's result; on a 200-job precedence
    /// schedule (82,678 pair caps) that search (2M-node budget, O(n k caps) per node) did not return within a 30 s alarm.
    pub fn from_state(m: &'a Model, seed: u64, stream: u64, x: &[usize]) -> Self {
        let mut c = Chain { m, x: vec![], load: vec![], rng: Philox4x32::new(seed, stream), w: vec![f64::NEG_INFINITY; m.k], dl: vec![] }; c.set_state(x); c
    }
    /// Replace the state (must be feasible for this model); capacity loads are recomputed.
    pub fn set_state(&mut self, x: &[usize]) {
        let k = self.m.k; self.x = x.to_vec(); self.load = vec![0; self.m.caps.len()];
        for (i, &v) in x.iter().enumerate() { for &c in &self.m.cap_of[i * k + v] { self.load[c] += 1; } }
    }
}

// ---------------- anneal: plan polish for any program (heuristic, no optimality proof) ----------------
/// Anneal the constraint-preserving moves through inverse temperatures `betas` (one continuous schedule per chain, `chains`
/// chains on threads, `ms` wall-clock split evenly across the betas), each chain warm-started from `start` if given (else a
/// dispersed random feasible start). Returns the best feasible plan by the ORIGINAL log w, never worse than `start`.
/// The IR form of the router's `polish_plan`. Heuristic: an exact solver gives a proof, this does not.
pub fn anneal(m: &Model, start: Option<&[usize]>, betas: &[f64], ms: f64, chains: usize, seed: u64) -> Option<(f64, Vec<usize>)> { anneal_on(m, start, betas, ms, 0, chains, chains, seed) }
/// Fixed-work `anneal`: `sweeps` sweeps per chain in total (split evenly over the betas) instead of `ms` of wall clock,
/// so the result is a pure function of (model, start, betas, sweeps, chains, seed). Backs `--polish-sweeps`.
pub fn anneal_sweeps(m: &Model, start: Option<&[usize]>, betas: &[f64], sweeps: usize, chains: usize, seed: u64) -> Option<(f64, Vec<usize>)> { anneal_on(m, start, betas, 0.0, sweeps.max(1), chains, chains, seed) }
/// `anneal` / `anneal_sweeps` on `threads` workers (worker w runs chains w, w+threads, ...; `--threads` for the polish).
/// `sweeps > 0` = fixed work (result independent of `threads`); else `ms` is the wall-clock deadline of the whole phase, so each
/// chain gets ms / ceil(chains / threads), as in the sampler. threads >= chains = the old one-thread-per-chain behaviour.
#[allow(clippy::too_many_arguments)]
pub fn anneal_on(m: &Model, start: Option<&[usize]>, betas: &[f64], ms: f64, sweeps: usize, chains: usize, threads: usize, seed: u64) -> Option<(f64, Vec<usize>)> {
    let t = threads.clamp(1, chains.max(1)); let ms = ms / chains.div_ceil(t) as f64;
    let qs: Vec<Model> = betas.iter().map(|&b| m.scaled(b)).collect();
    let run = |c: usize| -> Option<(f64, Vec<usize>)> {
        let mut x = match start { Some(s) => s.to_vec(), None => Chain::new(m, seed, c as u64)?.x };
        let mut best = (m.logw(&x), x.clone()); let t0 = std::time::Instant::now();
        for (q, mq) in qs.iter().enumerate() {
            let mut ch = Chain::from_state(mq, seed ^ 0x5eed, (c * 64 + q) as u64, &x);
            let stop = ms * (q + 1) as f64 / qs.len() as f64; let mut j = 0usize;
            let n = sweeps * (q + 1) / qs.len() - sweeps * q / qs.len();
            while if sweeps > 0 { j < n } else { j % 4 != 0 || t0.elapsed().as_secs_f64() * 1e3 < stop } { ch.sweep(); j += 1; let lw = m.logw(&ch.x); if lw > best.0 { best = (lw, ch.x.clone()); } }
            x = ch.x.clone();
        }
        Some(best)
    };
    let mut slots: Vec<Option<(f64, Vec<usize>)>> = (0..chains).map(|_| None).collect();
    std::thread::scope(|sc| { let hs: Vec<_> = (0..t).map(|w| { let run = &run; chain_thread(m).spawn_scoped(sc, move || (w..chains).step_by(t).map(|c| (c, run(c))).collect::<Vec<_>>()).unwrap() }).collect();
        for h in hs { for (c, r) in h.join().unwrap() { slots[c] = r; } } });
    slots.into_iter().collect::<Option<Vec<_>>>()?.into_iter().max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
}

/// A chain thread's stack is sized to the program. The feasible-start search recurses once per variable, and on the
/// default 2 MiB thread stack a 2-colouring path of 8,000 variables aborted the process (stack overflow) inside `sample`.
/// The reservation is virtual; pages are touched only as deep as the search goes.
fn chain_thread(m: &Model) -> std::thread::Builder { std::thread::Builder::new().stack_size(stack_for(m.n)) }
fn stack_for(n: usize) -> usize { (8usize << 20).max(n.saturating_mul(1024).saturating_add(1 << 20)) }
/// Variables above which the exact tiers run on their own thread (`deep`).
pub const DEEP_VARS: usize = 2048;
/// The exact tiers recurse once per variable (enumeration / MRV DFS; the frontier's tabulation once per component member)
/// on the CALLER's stack: a 100,000-variable clamped program aborted `pbit run --op decide|exact` (exit 134, main-thread stack
/// overflow, 8 MiB). Above DEEP_VARS variables `f` runs on a scoped thread sized like a chain thread; below, in place (unchanged).
pub fn deep<T: Send>(n: usize, f: impl FnOnce() -> T + Send) -> T {
    if n <= DEEP_VARS { return f(); }
    std::thread::scope(|s| std::thread::Builder::new().stack_size(stack_for(n)).spawn_scoped(s, f).expect("exact-tier thread").join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
}

pub struct Samples { pub chain_marg: Vec<Vec<f64>>, /// per-chain state trajectory (n entries per kept sweep) for batch-means MCSE
    pub traj: Vec<Vec<u16>>, pub marg: Vec<f64>, pub n: usize, pub plans: HashMap<Vec<u8>, u32>, pub trace: Vec<Vec<f64>>, pub viol: usize, pub best: (f64, Vec<usize>), pub sweeps: usize }

/// Run `chains` chains, each for `sweeps` sweeps (or until `budget_ms` wall-clock; burn-in 10% or 20 sweeps), optionally on threads.
pub fn sample(m: &Model, chains: usize, sweeps: usize, budget_ms: Option<f64>, seed: u64, threads: bool, keep_plans: bool) -> Option<Samples> {
    let run = |c: usize| run_chain(m, c, sweeps, budget_ms, seed, keep_plans, 1.0, 0, 1.0);
    let parts: Vec<Samples> = if threads {
        std::thread::scope(|sc| { let hs: Vec<_> = (0..chains).map(|c| { let run = &run; chain_thread(m).spawn_scoped(sc, move || run(c)).unwrap() }).collect(); hs.into_iter().map(|h| h.join().unwrap()).collect::<Option<Vec<_>>>() })?
    } else { (0..chains).map(run).collect::<Option<Vec<_>>>()? };
    Some(merge(m.n * m.k, parts))
}
/// One chain of `sample` (stream `c`): a pure function of (program, seed, c, sweeps) when `budget_ms` is None. `duty` < 1
/// (CPU limit): after every ~2 ms of sweeping the thread sleeps busy * (1/duty - 1); samples are unchanged.
#[allow(clippy::too_many_arguments)]
fn run_chain(m: &Model, c: usize, sweeps: usize, budget_ms: Option<f64>, seed: u64, keep_plans: bool, duty: f64, max_rows: usize, start_frac: f64) -> Option<Samples> {
    // A wall-clock budget includes the chain's feasible-start search (docs: "the deadline of the whole sampling phase";
    // a 200-job schedule's 1 s budget used to sample for 1.76 s: 0.7 s of start outside the clock)
    let t0 = std::time::Instant::now(); let k = m.k;
    let mut ch = Chain::new_until(m, seed, c as u64, budget_ms.map(|b| t0 + std::time::Duration::from_secs_f64(b.max(0.0) * start_frac / 1e3)))?;
    let burn = sweeps / 10;
    let mut s = Samples { chain_marg: vec![], traj: vec![vec![]], marg: vec![0.0; m.n * k], n: 0, plans: HashMap::new(), trace: vec![vec![]], viol: 0, best: (f64::NEG_INFINITY, vec![]), sweeps: 0 };
    let mut it = 0usize; let mut busy0 = std::time::Instant::now(); let mut stride = 1usize;
    loop {
        if let Some(b) = budget_ms { if it % 8 == 0 && t0.elapsed().as_secs_f64() * 1e3 >= b { break; } } else if it >= sweeps { break; }
        ch.sweep(); it += 1; progress_tick(it);
        if duty < 1.0 { let b = busy0.elapsed(); if b.as_secs_f64() >= 0.002 { std::thread::sleep(b.mul_f64(1.0 / duty - 1.0)); busy0 = std::time::Instant::now(); } }
        let burn_now = if budget_ms.is_some() { it <= 20 } else { it <= burn };
        if burn_now { continue; }
        for i in 0..m.n { s.marg[i * k + ch.x[i]] += 1.0; }
        s.viol += (m.violations(&ch.x) > 0) as usize;
        let lw = ch.logw(); record_row(&mut s, &ch.x, lw, &mut stride, max_rows); s.n += 1;
        if lw > s.best.0 { s.best = (lw, ch.x.clone()); }
        if keep_plans { *s.plans.entry(ch.x.iter().map(|&v| v as u8).collect()).or_insert(0) += 1; }
    }
    // A budget spent before the first recorded row (burn-in, or a slow start) left `best` empty, and the CLI's polish
    // then indexed an empty plan (`pbit run --budget-ms 0.01` on a 200-job schedule aborted). The chain's state is feasible.
    if s.n == 0 { s.best = (ch.logw(), ch.x.clone()); }
    s.sweeps = it; Some(s)
}
/// Resource control: `threads` worker threads (>= 1) run `chains` chains; worker w runs chains w, w + threads, ...
/// and results are pooled in chain order, so with fixed `sweeps` the answer is bit-identical for every thread count (and to
/// `sample`). A wall-clock budget is the deadline of the whole call: each chain gets budget / ceil(chains / threads).
/// `cpu_pct` in 1..=100: per-thread duty cycle; with fixed `sweeps` the answer is unchanged, only slower.
/// `max_rows` > 0 (memory limit): at most that many trajectory rows per chain, see `record_row`; 0 = unbounded.
#[allow(clippy::too_many_arguments)]
pub fn sample_on(m: &Model, chains: usize, threads: usize, sweeps: usize, budget_ms: Option<f64>, seed: u64, keep_plans: bool, cpu_pct: u32, max_rows: usize) -> Option<Samples> {
    sample_on_starting(m, chains, threads, sweeps, budget_ms, seed, keep_plans, cpu_pct, max_rows, 1.0)
}
/// `sample_on` whose chains give up their feasible-start search after `start_frac` of their wall-clock budget
/// (sampling still runs to the budget's end), leaving the rest for an exact fallback (`pbit run --op decide`). 1.0 = `sample_on`.
#[allow(clippy::too_many_arguments)]
pub fn sample_on_starting(m: &Model, chains: usize, threads: usize, sweeps: usize, budget_ms: Option<f64>, seed: u64, keep_plans: bool, cpu_pct: u32, max_rows: usize, start_frac: f64) -> Option<Samples> {
    let t = threads.clamp(1, chains.max(1)); let rounds = chains.div_ceil(t).max(1); let b = budget_ms.map(|b| b / rounds as f64);
    let mut slots: Vec<Option<Samples>> = (0..chains).map(|_| None).collect();
    std::thread::scope(|sc| {
        let hs: Vec<_> = (0..t).map(|w| chain_thread(m).spawn_scoped(sc, move || (w..chains).step_by(t).map(|c| (c, run_chain(m, c, sweeps, b, seed, keep_plans, cpu_pct.clamp(1, 100) as f64 / 100.0, max_rows, start_frac))).collect::<Vec<_>>()).unwrap()).collect();
        for h in hs { for (c, s) in h.join().unwrap() { slots[c] = s; } }
    });
    Some(merge(m.n * m.k, slots.into_iter().collect::<Option<Vec<_>>>()?))
}
/// Monitoring: while `PROGRESS_ON` is set (the CLI's `--progress`), every sampler chain (this crate's and the router's)
/// adds its sweeps to `PROGRESS_SWEEPS` in steps of 64, so a monitor thread can report throughput while the chains run.
/// Off (the default) it costs one relaxed load per 64 sweeps; samples are never affected.
/// Work budget (capacity checks) of one attempt of the non-partition random feasible start, and how many starts fell
/// back to the ascending-value retry (process-wide, for tests and benchmarks).
pub const START_WORK: u64 = 500_000_000;
/// Extra ascending-value MRV attempts (fresh random tie-breaks) after the ascending-order retries fail.
pub const START_RERANKS: usize = 4;
pub static START_FALLBACKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static PROGRESS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static PROGRESS_SWEEPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[inline]
pub fn progress_tick(it: usize) {
    use std::sync::atomic::Ordering::Relaxed;
    if it & 63 == 0 && PROGRESS_ON.load(Relaxed) { PROGRESS_SWEEPS.fetch_add(64, Relaxed); }
}
/// Memory limit. Records a kept sweep (state `x`, log-weight `lw`; call BEFORE `s.n += 1`) into the chain's trajectory and
/// trace if it falls on the current `stride`. With `max_rows` > 0, a chain holding `max_rows` rows keeps every other row and
/// doubles its stride: equally spaced thinning, so trajectory memory stays <= max_rows * n * 2 bytes per chain for any budget.
/// Marginals still count every kept sweep. The gate's batch means then describe a thinned chain, whose mean has variance >= the
/// full chain's mean (any stationary chain: the full mean is the average of `stride` interleaved thinned means; MacEachern &
/// Berliner 1994, lit.), so its error bar for the reported marginals is conservative. `max_rows` = 0: every row (unchanged).
pub fn record_row(s: &mut Samples, x: &[usize], lw: f64, stride: &mut usize, max_rows: usize) {
    if *stride > 1 && s.n % *stride != 0 { return; }
    // With a cap, reserve the whole buffer once: growing by doubling toward the cap made peak RSS ~1.6x the cap (10 s run,
    // --mem-limit-mb 1024: 1,632 MB). The buffer never holds more than max_rows rows, so it never reallocates.
    // try_: an absurd cap (e.g. 10 TB) must not abort the process; if the reservation fails the buffer just grows as before.
    if max_rows > 0 && s.trace[0].is_empty() { let _ = s.traj[0].try_reserve_exact(max_rows.max(2).saturating_mul(x.len())); let _ = s.trace[0].try_reserve_exact(max_rows.max(2)); }
    s.traj[0].extend(x.iter().map(|&v| v as u16)); s.trace[0].push(lw);
    if max_rows > 0 && s.trace[0].len() >= max_rows.max(2) {
        let (n, rows) = (x.len(), s.trace[0].len()); let keep = rows.div_ceil(2); let (tr, tc) = (&mut s.traj[0], &mut s.trace[0]);
        for r in 1..keep { tr.copy_within(2 * r * n..(2 * r + 1) * n, r * n); tc[r] = tc[2 * r]; }
        tr.truncate(keep * n); tc.truncate(keep); *stride *= 2;
    }
}
/// Pool per-chain samples (chain order) into one `Samples`.
pub fn merge(nk: usize, parts: Vec<Samples>) -> Samples {
    let mut out = Samples { chain_marg: vec![], traj: vec![], marg: vec![0.0; nk], n: 0, plans: HashMap::new(), trace: vec![], viol: 0, best: (f64::NEG_INFINITY, vec![]), sweeps: 0 };
    for s in parts { for (m, v) in out.marg.iter_mut().zip(&s.marg) { *m += v; } out.chain_marg.push(s.marg.iter().map(|v| v / (s.n as f64).max(1.0)).collect()); out.n += s.n; out.viol += s.viol; out.sweeps += s.sweeps;
        for (key, v) in s.plans { *out.plans.entry(key).or_insert(0) += v; }
        if s.best.0 > out.best.0 { out.best = s.best; } out.trace.push(s.trace.into_iter().next().unwrap()); out.traj.push(s.traj.into_iter().next().unwrap()); }
    let n = out.n as f64; for v in out.marg.iter_mut() { *v /= n.max(1.0); }
    out
}

// ---------------- the certification gate over generic marginals ----------------
/// Split-R-hat (Gelman et al.) on a scalar trace per chain.
pub fn split_rhat(traces: &[Vec<f64>]) -> f64 {
    let n = traces.iter().map(|t| t.len()).min().unwrap_or(0) / 2;
    if n < 4 || traces.len() < 2 { return f64::INFINITY; }
    let halves: Vec<&[f64]> = traces.iter().flat_map(|t| { let t = &t[t.len() - 2 * n..]; vec![&t[..n], &t[n..]] }).collect();
    let m = halves.len() as f64; let nf = n as f64;
    let means: Vec<f64> = halves.iter().map(|h| h.iter().sum::<f64>() / nf).collect();
    let gm = means.iter().sum::<f64>() / m;
    let b = nf / (m - 1.0) * means.iter().map(|x| (x - gm).powi(2)).sum::<f64>();
    let w = halves.iter().zip(&means).map(|(h, mu)| h.iter().map(|x| (x - mu).powi(2)).sum::<f64>() / (nf - 1.0)).sum::<f64>() / m;
    if w <= 0.0 { return if b <= 0.0 { 1.0 } else { f64::INFINITY }; }
    (((nf - 1.0) / nf * w + b / nf) / w).sqrt()
}
pub fn mean_tv(a: &[f64], b: &[f64], na: usize) -> f64 {
    let t = a.len() / na; (0..t).map(|i| 0.5 * (0..na).map(|k| (a[i * na + k] - b[i * na + k]).abs()).sum::<f64>()).sum::<f64>() / t as f64
}
pub fn max_tv(a: &[f64], b: &[f64], na: usize) -> f64 {
    let t = a.len() / na; (0..t).map(|i| 0.5 * (0..na).map(|k| (a[i * na + k] - b[i * na + k]).abs()).sum::<f64>()).fold(0.0, f64::max)
}
/// Between-chain marginal disagreement: max over variables of the max pairwise TV between chain marginals.
pub fn chain_disagreement(s: &Samples, na: usize) -> f64 {
    let c = &s.chain_marg; let mut worst: f64 = 0.0;
    for u in 0..c.len() { for v in u + 1..c.len() { worst = worst.max(max_tv(&c[u], &c[v], na)); } } worst
}
/// Gate statistics. `sig_tv[i]` = 0.5 * sum_v sigma_iv, where sigma_iv is the multi-chain batch-means MCSE of the
/// pooled marginal P(x_i = v). Batch variances are taken around the POOLED mean, so between-chain disagreement
/// inflates sigma (a stuck chain cannot hide behind its own small within-chain variance).
#[derive(Clone, Debug)]
pub struct Gate { pub rhat: f64, /// per-variable 0.5*sum_v sigma_iv
    pub sig_tv: Vec<f64>, pub sig_tv_max: f64, pub worst_task: usize, pub min_ess: f64, pub min_batches: usize, pub chain_dis: f64,
    /// Dual-batch bound: per-variable sigma with batch size n^(2/3) (longer batches see slow swap-only mixing)
    pub sig_tv_long: Vec<f64>,
    /// Number of FROZEN SATURATED shared capacity constraints: full in every kept sample, usable by >= 2 variable classes
    /// (distinct feasible-value sets), and whose class composition never changed in at least one chain. Site + swap moves
    /// conserve that composition while the constraint is full, so the chain is reducible there and no within-run statistic can see it.
    /// Non-partition programs: forced members (clamps) do not count as a class, and every free variable that never
    /// changed value in any chain is added (see `frozen_saturated`).
    pub frozen: usize,
    /// Per-variable split-R-hat of the marginal indicators (max over values with pooled P > 0.02), from half-chain
    /// counts with Bernoulli within-variance. Experimental: used only by `certified_tasks_local`.
    pub rhat_task: Vec<f64>,
    /// Per variable, escalated by the frozen rule (never released by `certified_tasks`). Partition programs: every variable
    /// iff `frozen > 0` (unchanged). Non-partition programs: the variables whose connected component (free variables linked by
    /// couplings and by caps that forced members do not fill) holds a stuck variable or a frozen cap's free member; the target
    /// factorizes over these components, so the others keep valid odds; forced variables too while `frozen > 0`. Empty (from
    /// `gate_stats_with`) = every variable iff `frozen > 0`.
    pub escalate: Vec<bool> }
#[derive(Clone, Copy, Debug)]
pub struct GateCfg { pub z: f64, pub tv_tol: f64, pub rhat_max: f64, pub min_batches: usize }
/// Default: certify iff logw split-R-hat < 1.05 AND z * max_i sig_tv_i <= tv_tol AND every chain has >= 8 batches.
/// z calibrated by examples/calibrate.rs against the exact DP oracle: z=2 was enough for the earlier dense sampler but gave
/// FCR 9.1% once the sparse site update made lam>=2 runs fast enough to certify; z=3: 0 false certs in 2400 runs.
pub const GATE_BS_POW: f64 = 0.5;
pub const PARTIAL_RHAT: f64 = 1.002;
/// Per-variable release needs max sigma(n^(2/3)) <= BATCH_RATIO_MAX * max sigma(sqrt n) (batch-size stability).
/// First set to 2.0 from 2 offending runs (ratios 2.13/2.33); then a third fresh set leaked 70 bad tickets from T=80 lam-2 runs at ratios
/// 1.90-1.95 -> 1.5 (0 bad over all 13 calibration sets, 145,335 released, -1.3% coverage vs 2.0).
pub const BATCH_RATIO_MAX: f64 = 1.5;
pub const GATE: GateCfg = GateCfg { z: 3.0, tv_tol: 0.05, rhat_max: 1.05, min_batches: 8 };
impl Gate {
    pub fn tv_bound(&self, cfg: &GateCfg) -> f64 { cfg.z * self.sig_tv_max.max(self.sig_tv_long.iter().cloned().fold(0.0, f64::max)) }
    /// Partial certification: if the run is globally sane (R-hat, batches), certify each variable whose OWN bound passes;
    /// the rest are escalated individually instead of refusing the whole joint answer.
    pub fn certified_tasks(&self, cfg: &GateCfg) -> Vec<bool> {
        // per-ticket release needs a STRICTER global mixing check than the whole-answer gate (calibrated:
        // R-hat < 1.05 let stuck-region tickets through at lam >= 2 with ticket FCR 8.6%; < 1.005 -> 2.5%)
        // Recalibrated with calib_sat (3 fresh instance sets incl. rho = 1 and T = 1000): guard 1.002 + dual-batch sigma.
        // + batch-size stability: if the run-wide MCSE still grows with batch length (sigma_n^(2/3) > 2 x sigma_sqrt(n)),
        //   the chains have not reached the asymptotic regime; per-ticket bounds are then biased low (18 bad tickets
        //   in the two runs with ratio 2.13 / 2.33; 70 more at 1.90-1.95 on a fresh set -> threshold 1.5, see BATCH_RATIO_MAX).
        // The frozen rule escalates per variable (`escalate`; whole answers still need frozen == 0, see `certified`), and the
        // batch-stability check reads only the variables it leaves in (chains trapped in different modes of a stuck component
        // give between-chain offsets that grow with batch length by construction). Nothing escalated: the run-wide check as before.
        let esc = |i: usize| self.escalate.get(i).copied().unwrap_or(self.frozen > 0); let n = self.sig_tv.len();
        let long = (0..n).filter(|&i| !esc(i)).map(|i| self.sig_tv_long[i]).fold(0.0, f64::max);
        let short = if (0..n).any(esc) { (0..n).filter(|&i| !esc(i)).map(|i| if self.sig_tv[i].is_nan() { f64::INFINITY } else { self.sig_tv[i] }).fold(0.0, f64::max) } else { self.sig_tv_max };
        let ok = self.rhat < PARTIAL_RHAT.min(cfg.rhat_max) && self.min_batches >= cfg.min_batches && long <= BATCH_RATIO_MAX * short;
        self.sig_tv.iter().zip(&self.sig_tv_long).enumerate().map(|(i, (&s, &l))| ok && !esc(i) && cfg.z * s.max(l) <= cfg.tv_tol).collect()
    }
    /// EXPERIMENTAL — refuted as a replacement (16 bad / 5,226 on the 2nd fresh set, one T=80 lam-2 run with logw
    /// R-hat 1.008). Kept for a hybrid candidate (add run-wide R-hat < 1.005). Per-ticket release with a LOCAL mixing check instead of the run-wide R-hat cliff:
    /// logw R-hat < cfg.rhat_max, frozen == 0, batch stability, >= min batches, and per ticket rhat_task < 1.01 and bound <= tol.
    pub fn certified_tasks_local(&self, cfg: &GateCfg) -> Vec<bool> {
        let long = self.sig_tv_long.iter().cloned().fold(0.0, f64::max);
        let ok = self.rhat < cfg.rhat_max && self.min_batches >= cfg.min_batches && self.frozen == 0 && long <= BATCH_RATIO_MAX * self.sig_tv_max;
        (0..self.sig_tv.len()).map(|i| ok && self.rhat_task.get(i).map_or(false, |&r| r < 1.01) && cfg.z * self.sig_tv[i].max(self.sig_tv_long[i]) <= cfg.tv_tol).collect()
    }
    /// The batch-stability guard applies to whole answers too (T=80 stress set: one false certificate, maxTV .111, at
    /// ratio 1.82; over all 16 calibration sets ratio <= 1.5 removes it at a cost of 15 of 318 certificates).
    pub fn certified(&self, cfg: &GateCfg) -> bool { self.rhat < cfg.rhat_max && self.tv_bound(cfg) <= cfg.tv_tol && self.min_batches >= cfg.min_batches && self.frozen == 0
        && self.sig_tv_long.iter().cloned().fold(0.0, f64::max) <= BATCH_RATIO_MAX * self.sig_tv_max }
}
pub fn gate_stats(m: &Model, s: &Samples) -> Gate {
    let (mut g, l, (fz, esc)) = std::thread::scope(|sc| { let hl = sc.spawn(|| gate_stats_with(m, s, 2.0 / 3.0)); let hf = sc.spawn(|| frozen_detail(m, s));
        (gate_stats_with(m, s, GATE_BS_POW), hl.join().unwrap(), hf.join().unwrap()) });
    g.sig_tv_long = l.sig_tv; g.frozen = fz; g.escalate = esc; g.rhat_task = rhat_tasks(m, s); g
}
/// see `Gate::rhat_task`
pub fn rhat_tasks(m: &Model, s: &Samples) -> Vec<f64> {
    let (t, na) = (m.n, m.k); let n = s.traj.iter().map(|tr| tr.len() / t).min().unwrap_or(0) / 2; if n < 4 || s.traj.len() < 2 { return vec![f64::INFINITY; t]; }
    let mut cnt: Vec<Vec<u32>> = vec![]; // per half: counts per (variable, value)
    for tr in &s.traj { let nc = tr.len() / t; let st = nc - 2 * n;
        for h in 0..2 { let mut c = vec![0u32; t * na]; for q in st + h * n..st + (h + 1) * n { let row = &tr[q * t..(q + 1) * t]; for i in 0..t { c[i * na + row[i] as usize] += 1; } } cnt.push(c); } }
    let mm = cnt.len() as f64; let nf = n as f64;
    (0..t).map(|i| { let mut worst: f64 = 1.0;
        for a in 0..na { if !m.allowed[i * na + a] || s.marg[i * na + a] <= 0.02 { continue; }
            let means: Vec<f64> = cnt.iter().map(|c| c[i * na + a] as f64 / nf).collect(); let gm = means.iter().sum::<f64>() / mm;
            let b = nf / (mm - 1.0) * means.iter().map(|x| (x - gm).powi(2)).sum::<f64>();
            let w = means.iter().map(|&q| q * (1.0 - q) * nf / (nf - 1.0)).sum::<f64>() / mm;
            let r = if w <= 0.0 { if b <= 0.0 { 1.0 } else { f64::INFINITY } } else { (((nf - 1.0) / nf * w + b / nf) / w).sqrt() };
            if r > worst { worst = r; } }
        worst }).collect()
}
/// Apply the forced-cap rule to partition programs (the router) too (now ON): a capacity its forced members
/// (clamped / single-allowed tasks) fill on their own is not a mixing signal, and forced members are left out of the class count.
/// Kill test with it OFF: 24-task demos whose exact MAP fills a worker, clamped to it -> forced sampler refused 10/10. ON:
/// clamped 10/10 certified, 240 released, 0 false vs exact; 20 random-clamp demos 480 released, 0 false; GOLDEN_GATE cases 1 and 4
/// re-pinned (forced legal tickets; case 1 certifies within 0.05 of exact). The oracle calibration families have no forced members.
pub const FORCED_CAPS_PARTITION: bool = true;
/// see `Gate::frozen`. One pass per chain (chains in parallel): per kept row, each capacity constraint's load and a multiset
/// hash of its occupants' classes (sum of a random 64-bit key per class, wrapping); O(n * (vars + caps)).
pub fn frozen_saturated(m: &Model, s: &Samples) -> usize { frozen_detail(m, s).0 }
/// `frozen_saturated` and, per variable, `Gate::escalate`.
pub fn frozen_detail(m: &Model, s: &Samples) -> (usize, Vec<bool>) {
    let (t, k, nc) = (m.n, m.k, m.caps.len());
    let mut sig: HashMap<Vec<bool>, usize> = HashMap::new();
    let class: Vec<usize> = (0..t).map(|i| { let q = sig.len(); *sig.entry((0..k).map(|v| m.ok(i, v)).collect()).or_insert(q) }).collect();
    let mut kr = Philox4x32::new(0xF0F0, 3); let key: Vec<u64> = (0..sig.len()).map(|_| ((kr.f64() * 4294967296.0) as u64) << 32 | (kr.f64() * 4294967296.0) as u64).collect();
    // Non-partition programs only (the assignment front-end's calibrated gate is unchanged): a member whose variable
    // is forced (one allowed value, e.g. a clamp) holds its slot for good, so it is left out of the class count, and a cap
    // such members fill on their own can never change hands: not a mixing signal. Before this, every pre-coloured vertex of
    // a colouring program froze its edge caps and the gate refused 54/54 instances.
    let multi: Vec<bool> = m.caps.iter().map(|c| {
        // With FORCED_CAPS_PARTITION, partition programs also leave forced members out of the class count (their classes
        // sit on the cap for good; if only one class of free members can take the other slots, the multiset can never change)
        let free = |i: usize| (m.partition && !FORCED_CAPS_PARTITION) || m.cand_count(i) > 1;
        let forced = if m.partition && !FORCED_CAPS_PARTITION { 0 } else { c.members.iter().filter(|&&(i, v)| m.ok(i, v) && m.cand_count(i) == 1).count() };
        let mut cl: Vec<usize> = c.members.iter().filter(|&&(i, v)| m.ok(i, v) && free(i)).map(|&(i, _)| class[i]).collect(); cl.sort(); cl.dedup(); cl.len() >= 2 && forced < c.limit }).collect();
    // per chain: (always full per constraint, changed per constraint)
    let per: Vec<(Vec<bool>, Vec<bool>)> = std::thread::scope(|sc| { let hs: Vec<_> = s.traj.iter().map(|tr| { let (class, key) = (&class, &key); sc.spawn(move || {
        let mut full = vec![true; nc]; let mut changed = vec![false; nc]; let mut prev: Vec<u64> = vec![]; let mut h = vec![0u64; nc]; let mut ld = vec![0usize; nc];
        for row in tr.chunks(t) { for v in h.iter_mut() { *v = 0; } for v in ld.iter_mut() { *v = 0; }
            if m.partition { for i in 0..t { let b = m.bucket[i * k + row[i] as usize]; if b != NONE { h[b as usize] = h[b as usize].wrapping_add(key[class[i]]); ld[b as usize] += 1; } } }
            else { for i in 0..t { for &c in &m.cap_of[i * k + row[i] as usize] { h[c] = h[c].wrapping_add(key[class[i]]); ld[c] += 1; } } }
            for c in 0..nc { if ld[c] < m.caps[c].limit { full[c] = false; } }
            if !prev.is_empty() { for c in 0..nc { if prev[c] != h[c] { changed[c] = true; } } }
            std::mem::swap(&mut prev, &mut h); if h.len() != nc { h = vec![0u64; nc]; } }
        (full, changed) }) }).collect(); hs.into_iter().map(|h| h.join().unwrap()).collect() });
    let fcaps: Vec<usize> = (0..nc).filter(|&c| multi[c] && per.iter().all(|q| q.0[c]) && per.iter().any(|q| !q.1[c])).collect();
    if m.partition { return (fcaps.len(), vec![!fcaps.is_empty(); t]); }
    // Non-partition programs also count every FREE variable (> 1 allowed value) that never changed value in any chain:
    // a chain that cannot move carries no information about the odds (sudoku: stuck chains look certain), so no release.
    let mut stuck: Vec<usize> = (0..t).filter(|&i| m.cand_count(i) > 1 && s.traj.iter().all(|tr| tr.chunks(t).all(|row| row[i] == tr[i]))).collect();
    if fcaps.is_empty() && stuck.is_empty() { return (0, vec![false; t]); }
    // Unit propagation. A cap that forced variables fill forbids its other members' values; a variable left with one value
    // is forced too (to a fixpoint). Sound: such a variable is a constant under the target, so the chains are right not to move it
    // (colouring: a vertex whose other colours clamped neighbours hold). It is not stuck and, like a clamp, links nothing.
    // Worklist, linear in the caps' sizes: per cap a count of forced members; a cap fires once, when the count reaches its limit.
    // (An earlier pass-until-no-change loop was O(n x caps) on a forcing chain listed against cap order: 234 ms at 8,000 variables.)
    let mut dom: Vec<Vec<usize>> = (0..t).map(|i| (0..k).filter(|&v| m.ok(i, v)).collect()).collect();
    let (mut cnt, mut fired) = (vec![0usize; nc], vec![false; nc]); let mut queue: Vec<usize> = (0..t).filter(|&i| dom[i].len() == 1).collect();
    let mut ready: Vec<usize> = (0..nc).filter(|&c| m.caps[c].limit == 0).collect();
    loop {
        while let Some(i) = queue.pop() { for &c in &m.cap_of[i * k + dom[i][0]] { cnt[c] += 1; if cnt[c] >= m.caps[c].limit { ready.push(c); } } }
        let Some(c) = ready.pop() else { break };
        if fired[c] { continue; } fired[c] = true;
        for &(i, v) in &m.caps[c].members { if dom[i].len() > 1 { if let Some(q) = dom[i].iter().position(|&x| x == v) { dom[i].remove(q); if dom[i].len() == 1 { queue.push(i); } } } }
    }
    let free = |i: usize| dom[i].len() > 1; stuck.retain(|&i| free(i));
    let fz = fcaps.len() + stuck.len(); if fz == 0 { return (0, vec![false; t]); }
    // Escalate per connected component of the FREE variables (was: every variable). A coupling links its two ends; a cap
    // links its free members unless forced members fill it (then it only forbids values); forced variables are constants and
    // link nothing. The target is a product over these components, and a site or swap move that changes two components is
    // feasible only if both single-variable changes are: a stuck component leaves the others' odds valid.
    fn root(uf: &mut [usize], mut a: usize) -> usize { while uf[a] != a { uf[a] = uf[uf[a]]; a = uf[a]; } a }
    let mut uf: Vec<usize> = (0..t).collect();
    for p in &m.pairs { if free(p.i) && free(p.j) { let (a, b) = (root(&mut uf, p.i), root(&mut uf, p.j)); uf[a] = b; } }
    for (ci, c) in m.caps.iter().enumerate() {
        if fired[ci] { continue; }
        let mut first: Option<usize> = None;
        for &(i, v) in &c.members { if free(i) && dom[i].contains(&v) { let a = root(&mut uf, i);
            match first { None => first = Some(a), Some(b) => { let b = root(&mut uf, b); uf[a] = b; } } } }
    }
    let mut bad = vec![false; t];
    for &i in &stuck { let r = root(&mut uf, i); bad[r] = true; }
    for &c in &fcaps { for &(i, v) in &m.caps[c].members { if free(i) && dom[i].contains(&v) { let r = root(&mut uf, i); bad[r] = true; } } }
    // forced variables (constants, trivially right) are released only when nothing is stuck, as before: a stuck sudoku releases
    // no givens and stays `refused`
    (fz, (0..t).map(|i| { let r = root(&mut uf, i); !free(i) || bad[r] }).collect())
}
/// batch size = n_c^bs_pow (0.5 = classic sqrt(n); larger = more conservative under long autocorrelation)
pub fn gate_stats_with(m: &Model, s: &Samples, bs_pow: f64) -> Gate {
    // Each chain's share of the pooled mean by its trajectory rows (== s.n when nothing was thinned; with --mem-limit-mb
    // every chain on fixed sweeps thins alike, so the shares are unchanged; using s.n here would shrink the variance when thinned)
    let (t, na) = (m.n, m.k); let tn = t * na; let nn = s.traj.iter().map(|tr| tr.len() / t).sum::<usize>() as f64;
    let mut var = vec![0.0f64; tn]; let mut min_b = usize::MAX;
    // chains in parallel; contributions are added in chain order afterwards, so the result is bit-identical to serial.
    let parts: Vec<(usize, Option<(f64, Vec<f64>)>)> = std::thread::scope(|sc| { let hs: Vec<_> = s.traj.iter().map(|tr| sc.spawn(move || {
        let nc = tr.len() / t; if nc == 0 { return (0, None); }
        let bs = ((nc as f64).powf(bs_pow) as usize).max(1); let nb = nc / bs;
        let start = nc - nb * bs; let mut acc = vec![0.0f64; tn]; let mut cnt = vec![0u32; tn];
        for b in 0..nb {
            for v in cnt.iter_mut() { *v = 0; }
            for q in start + b * bs..start + (b + 1) * bs { let row = &tr[q * t..(q + 1) * t]; for i in 0..t { cnt[i * na + row[i] as usize] += 1; } }
            for v in 0..tn { if m.allowed[v] { let d = cnt[v] as f64 / bs as f64 - s.marg[v]; acc[v] += d * d; } }
        }
        (nb, if nb >= 2 { Some(((nc as f64 / nn).powi(2) / (nb as f64 * (nb as f64 - 1.0)), acc)) } else { None }) })).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect() });
    for (nb, c) in parts { min_b = min_b.min(nb);
        match c { Some((wgt, acc)) => { for v in 0..tn { var[v] += wgt * acc[v]; } }, None => { for v in 0..tn { var[v] = f64::INFINITY; } } } }
    let mut sig_tv_max = 0.0; let mut worst = 0; let mut min_ess = f64::INFINITY; let mut sig_tv = vec![0.0; t];
    for i in 0..t { let st: f64 = 0.5 * (0..na).map(|a| var[i * na + a].sqrt()).sum::<f64>(); sig_tv[i] = st;
        if st > sig_tv_max || st.is_nan() { sig_tv_max = if st.is_nan() { f64::INFINITY } else { st }; worst = i; }
        for a in 0..na { let mg = s.marg[i * na + a]; if mg > 0.02 && mg < 0.98 { let e = mg * (1.0 - mg) / var[i * na + a]; if e < min_ess { min_ess = e; } } } }
    Gate { rhat: split_rhat(&s.trace), sig_tv_long: vec![0.0; t], sig_tv, sig_tv_max, worst_task: worst, min_ess, min_batches: if min_b == usize::MAX { 0 } else { min_b }, chain_dis: chain_disagreement(s, na), frozen: 0, rhat_task: vec![], escalate: vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 2 x 3 toy: exact marginals by brute force over all 3^n states, with a table coupling and a non-partition capacity.
    fn toy() -> Model {
        let (n, k) = (4, 3); let h = vec![0.1, -0.3, 0.5, 0.0, 0.2, -0.1, 0.4, 0.4, -0.2, -0.5, 0.3, 0.1];
        let pairs = vec![Pair { i: 0, j: 1, c: Coupling::Potts(0.7) }, Pair { i: 1, j: 3, c: Coupling::Table(vec![0.0, 0.5, -0.2, 0.3, 0.0, 0.1, -0.4, 0.2, 0.0]) }, Pair { i: 2, j: 0, c: Coupling::Potts(-0.6) }];
        let caps = vec![Cap { members: vec![(0, 0), (1, 0), (2, 0), (3, 1)], limit: 1 }, Cap { members: vec![(0, 0), (2, 2), (3, 2)], limit: 1 }];
        let mut allowed = vec![true; n * k]; allowed[2 * k + 1] = false;
        Model::new(n, k, h, allowed, vec![None; n], pairs, caps).unwrap()
    }
    fn brute(m: &Model) -> (f64, Vec<f64>) {
        let mut z = 0.0; let mut mg = vec![0.0; m.n * m.k];
        for s in 0..m.k.pow(m.n as u32) { let x: Vec<usize> = (0..m.n).map(|i| (s / m.k.pow(i as u32)) % m.k).collect();
            if m.violations(&x) > 0 { continue; } let w = m.logw(&x).exp(); z += w; for i in 0..m.n { mg[i * m.k + x[i]] += w; } }
        for v in mg.iter_mut() { *v /= z; } (z.ln(), mg)
    }
    #[test]
    fn exact_matches_brute_force_on_general_model() {
        let m = toy(); assert!(!m.is_partition()); let (lz, mg) = brute(&m); let e = exact(&m, 3, 1 << 20).unwrap();
        assert!((e.logz - lz).abs() < 1e-12); assert!(max_tv(&e.marg, &mg, m.k) < 1e-12);
        let c = m.with_clamp(1, 2); let (lz, mg) = brute(&c); let e = exact(&c, 1, 1 << 20).unwrap();
        assert!((e.logz - lz).abs() < 1e-12 && max_tv(&e.marg, &mg, c.k) < 1e-12 && e.top[0].1[1] == 2);
    }
    /// `record_row` thinning keeps the trajectory equally spaced from kept sweep 0 (so batch means see a thinned chain),
    /// never holds more than `max_rows` rows, and max_rows = 0 records every row.
    #[test]
    fn record_row_thins_evenly_within_the_cap() {
        for (total, cap) in [(100usize, 8usize), (1000, 64), (37, 5), (64, 64), (10, 0)] {
            let mut s = Samples { chain_marg: vec![], traj: vec![vec![]], marg: vec![], n: 0, plans: HashMap::new(), trace: vec![vec![]], viol: 0, best: (0.0, vec![]), sweeps: 0 };
            let mut stride = 1usize;
            for kept in 0..total { record_row(&mut s, &[kept, kept + 1], kept as f64, &mut stride, cap); s.n += 1;
                assert!(cap == 0 || s.trace[0].len() < cap.max(2), "cap {cap}"); }
            let xs: Vec<usize> = s.traj[0].chunks(2).map(|r| r[0] as usize).collect();
            assert!(xs.iter().enumerate().all(|(r, &x)| x == r * stride && s.traj[0][2 * r + 1] as usize == x + 1 && s.trace[0][r] == x as f64), "{total} {cap}: {xs:?} stride {stride}");
            assert!(*xs.last().unwrap() + stride >= total, "the last row is the last on-stride kept sweep");
            if cap == 0 { assert_eq!(xs.len(), total); }
        }
    }
    /// Frustrated max-cut on a 3-regular graph (anti-ferromagnetic table couplings, zero field: every true marginal
    /// is 0.5). A certificate must be within tolerance; a short run at strong coupling must not certify a stuck answer.
    #[test]
    fn maxcut_gate_is_honest_against_brute_force() {
        let edges = [(0, 1), (0, 5), (0, 9), (1, 2), (1, 7), (2, 3), (2, 10), (3, 4), (3, 8), (4, 5), (4, 11), (5, 6), (6, 7), (6, 10), (7, 8), (8, 9), (9, 11), (10, 11)];
        let mut certs = 0;
        for (beta, sweeps) in [(0.7, 5000usize), (0.7, 500), (1.5, 500), (1.5, 5000)] {
            let pairs = edges.iter().map(|&(i, j)| Pair { i, j, c: Coupling::Table(vec![0.0, beta, beta, 0.0]) }).collect();
            let m = Model::new(12, 2, vec![0.0; 24], vec![true; 24], vec![None; 12], pairs, vec![]).unwrap();
            let ex = exact(&m, 1, 1 << 20).unwrap(); assert!(ex.marg.iter().all(|&p| (p - 0.5).abs() < 1e-12));
            let s = sample(&m, 4, sweeps, None, 11, true, false).unwrap(); let g = gate_stats(&m, &s); let mx = max_tv(&s.marg, &ex.marg, 2);
            if g.certified(&GATE) { certs += 1; assert!(mx <= 0.05, "false certificate: beta {beta} sweeps {sweeps} maxTV {mx}"); }
            for (i, ok) in g.certified_tasks(&GATE).into_iter().enumerate() { assert!(!ok || (s.marg[2 * i] - 0.5).abs() <= 0.05); }
        }
        assert!(certs >= 1, "gate never certifies: useless");
    }
    /// Determinism contract: same program + seed + chain count => bit-identical samples whether chains run on threads or not.
    #[test]
    fn threads_do_not_change_results() {
        let m = toy(); let a = sample(&m, 4, 3000, None, 21, true, true).unwrap(); let b = sample(&m, 4, 3000, None, 21, false, true).unwrap();
        assert!(a.marg.iter().zip(&b.marg).all(|(x, y)| x.to_bits() == y.to_bits())); assert_eq!(a.traj, b.traj); assert_eq!(a.plans, b.plans);
        let (ga, gb) = (gate_stats(&m, &a), gate_stats(&m, &b)); assert_eq!((ga.rhat.to_bits(), ga.sig_tv_max.to_bits(), ga.frozen), (gb.rhat.to_bits(), gb.sig_tv_max.to_bits(), gb.frozen));
    }
    #[test]
    fn anneal_is_feasible_and_bounded_by_exact() {
        let m = toy(); let e = exact(&m, 1, 1 << 20).unwrap(); let map = e.top[0].1.clone(); let opt = m.logw(&map);
        let start = Chain::new(&m, 5, 0).unwrap().x; let (lw, x) = anneal(&m, Some(&start), &[0.5, 2.0, 8.0], 20.0, 2, 9).unwrap();
        assert_eq!(m.violations(&x), 0); assert!(lw >= m.logw(&start) - 1e-12 && lw <= opt + 1e-12); assert!((lw - m.logw(&x)).abs() < 1e-12);
        assert!((lw - opt).abs() < 1e-12, "anneal missed the optimum of a 4-variable toy: {lw} vs {opt}");
        // Fixed-work anneal is a pure function of its inputs and obeys the same bounds
        let a = anneal_sweeps(&m, Some(&start), &[0.5, 2.0, 8.0], 300, 2, 9).unwrap(); let b = anneal_sweeps(&m, Some(&start), &[0.5, 2.0, 8.0], 300, 2, 9).unwrap();
        assert_eq!(a, b); assert_eq!(m.violations(&a.1), 0); assert!(a.0 >= m.logw(&start) - 1e-12 && a.0 <= opt + 1e-12);
    }
    #[test]
    fn sampler_is_feasible_and_matches_exact_on_general_model() {
        let m = toy(); let e = exact(&m, 1, 1 << 20).unwrap();
        let s = sample(&m, 4, 40_000, None, 3, true, false).unwrap();
        assert_eq!(s.viol, 0); let tv = max_tv(&s.marg, &e.marg, m.k); assert!(tv < 0.02, "maxTV {tv}");
        let g = gate_stats(&m, &s); assert!(g.certified(&GATE), "bound {} rhat {}", g.tv_bound(&GATE), g.rhat);
    }
    /// The frontier DP is exact on general partition programs (random components with Potts + table couplings,
    /// caps shared across components, clamps), declines non-partition programs and oversized components.
    #[test]
    fn frontier_matches_enumeration_on_partition_programs() {
        let close = |f: &FrontierExact, e: &Exact, m: &Model| { assert!((f.logz - e.logz).abs() < 1e-9, "logz {} vs {}", f.logz, e.logz);
            assert!(max_tv(&f.marg, &e.marg, m.k) < 1e-9); assert!((f.map_logw - e.top[0].0.ln() - e.logz).abs() < 1e-9 && m.violations(&f.map) == 0); };
        assert!(exact_frontier(&toy(), FRONTIER_MAX_STATES).is_none(), "toy is not a partition: must decline");
        let mut tested = 0;
        for seed in 0..40u64 {
            let mut r = Philox4x32::new(seed, 99); let (n, k) = (7 + r.below(4), 2 + r.below(2));
            let h: Vec<f64> = (0..n * k).map(|_| r.f64() * 2.0 - 1.0).collect();
            let mut allowed: Vec<bool> = (0..n * k).map(|_| r.f64() < 0.85).collect(); for i in 0..n { allowed[i * k + r.below(k)] = true; }
            let mut pairs = vec![]; for _ in 0..r.below(n) { let (i, j) = (r.below(n), r.below(n)); if i == j { continue; }
                pairs.push(Pair { i, j, c: if r.f64() < 0.5 { Coupling::Potts(r.f64() * 2.0 - 1.0) } else { Coupling::Table((0..k * k).map(|_| r.f64() - 0.5).collect()) } }); }
            // partition caps: each (i, v) lands in at most one of 3 caps (or none)
            let mut caps: Vec<Cap> = (0..3).map(|_| Cap { members: vec![], limit: 1 + r.below(3) }).collect();
            for i in 0..n { for v in 0..k { let c = r.below(5); if c < 3 { caps[c].members.push((i, v)); } } }
            let mut clamp = vec![None; n]; if seed % 4 == 0 { let i = r.below(n); clamp[i] = (0..k).find(|&v| allowed[i * k + v]); }
            let m = Model::new(n, k, h, allowed, clamp, pairs, caps).unwrap(); assert!(m.is_partition());
            let e = exact(&m, 1, 1 << 22).unwrap(); let f = exact_frontier(&m, FRONTIER_MAX_STATES);
            if e.n_feasible == 0 { assert!(f.is_none()); continue; }
            if let Some(f) = f { close(&f, &e, &m); tested += 1; }
        }
        assert!(tested >= 30, "frontier declined too often: {tested}/40");
        // max-cut without caps: one component of 2^12 assignments is solved, 2^13 declines
        let ring = |n: usize| { let pairs = (0..n).map(|i| Pair { i, j: (i + 1) % n, c: Coupling::Table(vec![0.0, 0.8, 0.8, 0.0]) }).collect();
            Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap() };
        let m = ring(12); let (e, f) = (exact(&m, 1, 1 << 20).unwrap(), exact_frontier(&m, FRONTIER_MAX_STATES).unwrap()); close(&f, &e, &m);
        assert!(exact_frontier(&ring(13), FRONTIER_MAX_STATES).is_none());
    }
    /// Non-partition programs enumerate with dynamic MRV. Empty 4x4 sudoku (shidoku): 288 solutions, every odds
    /// 1/4; one given (cell 0 = value 0) leaves 72, and the other cells of its row/column/box can no longer take value 0.
    #[test]
    fn mrv_enumeration_counts_shidoku() {
        let mut units: Vec<Vec<usize>> = (0..4).map(|r| (0..4).map(|c| r * 4 + c).collect()).collect();
        units.extend((0..4).map(|c| (0..4).map(|r| r * 4 + c).collect::<Vec<_>>())); units.extend((0..4).map(|b| (0..4).map(|q| (b / 2 * 2 + q / 2) * 4 + b % 2 * 2 + q % 2).collect::<Vec<_>>()));
        let caps: Vec<Cap> = units.iter().flat_map(|u| (0..4).map(move |d| Cap { members: u.iter().map(|&c| (c, d)).collect(), limit: 1 })).collect();
        let m = Model::new(16, 4, vec![0.0; 64], vec![true; 64], vec![None; 16], vec![], caps).unwrap(); assert!(!m.is_partition());
        let e = exact(&m, 3, 1 << 20).unwrap(); assert_eq!(e.n_feasible, 288); assert!(e.marg.iter().all(|&p| (p - 0.25).abs() < 1e-12));
        let c = m.with_clamp(0, 0); let e = exact(&c, 1, 1 << 20).unwrap(); assert_eq!(e.n_feasible, 72);
        for q in [1, 2, 3, 4, 8, 12, 5] { assert_eq!(e.marg[q * 4], 0.0, "cell {q}"); }
        assert!(c.violations(&e.top[0].1) == 0 && e.top[0].1[0] == 0);
    }
    /// Non-partition gate. A pre-coloured vertex no longer freezes its edge caps (proper 4-colouring of a wheel-like
    /// graph with a clamp certifies, within tolerance of the exact odds); stuck chains (a shidoku with 2 solutions that
    /// differ on a 4-cell rectangle: no site or swap move connects them) are refused even if every chain agrees.
    #[test]
    fn gate_colouring_clamp_certifies_and_stuck_chain_refuses() {
        let (n, k) = (9, 4); let mut e: Vec<(usize, usize)> = (0..8).map(|i| (i, (i + 1) % 8)).collect(); e.extend([(0, 8), (2, 8), (4, 8), (6, 8)]);
        let caps: Vec<Cap> = e.iter().flat_map(|&(a, b)| (0..k).map(move |c| Cap { members: vec![(a, c), (b, c)], limit: 1 })).collect();
        let h: Vec<f64> = (0..n * k).map(|q| 0.3 * ((q * 7 % 5) as f64 - 2.0)).collect(); let mut clamp = vec![None; n]; clamp[8] = Some(0);
        let m = Model::new(n, k, h, vec![true; n * k], clamp, vec![], caps).unwrap(); assert!(!m.is_partition());
        let ex = exact(&m, 1, 1 << 20).unwrap(); let s = sample(&m, 4, 4000, None, 5, true, false).unwrap(); let g = gate_stats(&m, &s);
        assert!(g.frozen == 0 && g.certified(&GATE), "frozen {} bound {}", g.frozen, g.tv_bound(&GATE)); assert!(max_tv(&s.marg, &ex.marg, k) <= 0.05);
        let mut units: Vec<Vec<usize>> = (0..4).map(|r| (0..4).map(|c| r * 4 + c).collect()).collect();
        units.extend((0..4).map(|c| (0..4).map(|r| r * 4 + c).collect::<Vec<_>>())); units.extend((0..4).map(|b| (0..4).map(|q| (b / 2 * 2 + q / 2) * 4 + b % 2 * 2 + q % 2).collect::<Vec<_>>()));
        let caps: Vec<Cap> = units.iter().flat_map(|u| (0..4).map(move |d| Cap { members: u.iter().map(|&c| (c, d)).collect(), limit: 1 })).collect();
        let sol = [0, 1, 2, 3, 2, 3, 0, 1, 1, 0, 3, 2, 3, 2, 1, 0]; let clamp: Vec<Option<usize>> = (0..16).map(|q| if q % 3 == 0 || q == 5 || q == 10 { Some(sol[q]) } else { None }).collect();
        let m = Model::new(16, 4, vec![0.0; 64], vec![true; 64], clamp, vec![], caps).unwrap(); assert_eq!(exact(&m, 1, 1 << 20).unwrap().n_feasible, 2); // 2 solutions no local move connects
        let s = sample(&m, 4, 2000, None, 5, true, false).unwrap(); let g = gate_stats(&m, &s);
        assert!(g.frozen > 0 && !g.certified(&GATE) && g.certified_tasks(&GATE).iter().all(|&r| !r), "a chain that cannot move must not certify");
    }
    /// The frozen rule escalates per connected component of the free variables. Next to the stuck 2-solution shidoku
    /// above: variable 16 (no links) is released within 0.05 of the exact odds; 17 (Potts pair to a cell where the two
    /// solutions differ) and 18 (shares a cap with that cell) are escalated, and so are the givens while anything is stuck.
    #[test]
    fn stuck_component_escalates_only_its_component() {
        let mut units: Vec<Vec<usize>> = (0..4).map(|r| (0..4).map(|c| r * 4 + c).collect()).collect();
        units.extend((0..4).map(|c| (0..4).map(|r| r * 4 + c).collect::<Vec<_>>())); units.extend((0..4).map(|b| (0..4).map(|q| (b / 2 * 2 + q / 2) * 4 + b % 2 * 2 + q % 2).collect::<Vec<_>>()));
        let mut caps: Vec<Cap> = units.iter().flat_map(|u| (0..4).map(move |d| Cap { members: u.iter().map(|&c| (c, d)).collect(), limit: 1 })).collect();
        let sol = [0, 1, 2, 3, 2, 3, 0, 1, 1, 0, 3, 2, 3, 2, 1, 0]; let mut clamp: Vec<Option<usize>> = (0..16).map(|q| if q % 3 == 0 || q == 5 || q == 10 { Some(sol[q]) } else { None }).collect();
        let two = exact(&Model::new(16, 4, vec![0.0; 64], vec![true; 64], clamp.clone(), vec![], caps.clone()).unwrap(), 2, 1 << 20).unwrap(); assert_eq!(two.n_feasible, 2);
        let c = (0..16).find(|&q| two.top[0].1[q] != two.top[1].1[q]).unwrap(); let v0 = two.top[0].1[c];
        caps.push(Cap { members: vec![(c, v0), (18, v0)], limit: 1 }); clamp.extend([None, None, None]);
        let mut h = vec![0.0; 19 * 4]; for v in 0..4 { h[16 * 4 + v] = 0.4 * v as f64 - 0.5; }
        let m = Model::new(19, 4, h, vec![true; 76], clamp, vec![Pair { i: 17, j: c, c: Coupling::Potts(2.0) }], caps).unwrap(); assert!(!m.is_partition());
        let ex = exact(&m, 1, 1 << 20).unwrap(); let s = sample(&m, 4, 4000, None, 5, true, false).unwrap(); let g = gate_stats(&m, &s); let rel = g.certified_tasks(&GATE);
        let tv = |i: usize| 0.5 * (0..4).map(|v| (s.marg[i * 4 + v] - ex.marg[i * 4 + v]).abs()).sum::<f64>();
        println!("frozen {} rhat {:.5} tv 16/17/18 {:.4} {:.4} {:.4}", g.frozen, g.rhat, tv(16), tv(17), tv(18));
        assert!(g.frozen > 0 && !g.certified(&GATE));
        assert!(rel[16] && tv(16) <= 0.05, "unlinked variable: released {} tv {}", rel[16], tv(16));
        assert!((0..16).chain([17, 18]).all(|i| !rel[i]), "stuck, linked and given variables must be escalated: {rel:?}");
    }
    /// A variable the clamps force through unit propagation is a constant, not a stuck chain. 3-colouring: 0 and 1
    /// clamped to colours 0 / 1, vertex 2 (neighbours 0, 1) is forced to 2, then vertex 3 (neighbours 0, 2) to 1; 4 and 5 mix.
    /// Before unit propagation, vertices 2 and 3 counted as stuck and the whole answer was refused.
    #[test]
    fn propagation_forced_variables_are_not_stuck() {
        let e = [(0, 2), (1, 2), (0, 3), (2, 3), (3, 4), (4, 5), (2, 5)]; let k = 3;
        let caps: Vec<Cap> = e.iter().flat_map(|&(a, b)| (0..k).map(move |c| Cap { members: vec![(a, c), (b, c)], limit: 1 })).collect();
        let h: Vec<f64> = (0..6 * k).map(|q| 0.25 * ((q * 5 % 7) as f64 - 3.0)).collect(); let mut clamp = vec![None; 6]; clamp[0] = Some(0); clamp[1] = Some(1);
        let m = Model::new(6, k, h, vec![true; 6 * k], clamp, vec![], caps).unwrap();
        let ex = exact(&m, 1, 1 << 20).unwrap(); assert!(ex.marg[2 * k + 2] == 1.0 && ex.marg[3 * k + 1] == 1.0);
        let s = sample(&m, 4, 4000, None, 3, true, false).unwrap(); let g = gate_stats(&m, &s);
        assert!(g.frozen == 0 && g.certified(&GATE), "frozen {} rhat {} bound {}", g.frozen, g.rhat, g.tv_bound(&GATE)); assert!(max_tv(&s.marg, &ex.marg, k) <= 0.05);
    }
    /// Chain threads get a stack sized to the program. On the default 2 MiB stack the feasible-start search (one frame
    /// per variable) aborted the process on this 8,000-variable 2-colouring path. Unit propagation forces every vertex from the
    /// clamp through a worklist (the pass loop it replaced was O(n x caps) here: 234 ms, now ~5 ms).
    #[test]
    fn long_forced_path_samples_without_stack_overflow() {
        let (n, k) = (8000, 2); let caps: Vec<Cap> = (0..n - 1).flat_map(|i| (0..k).map(move |c| Cap { members: vec![(i, c), (i + 1, c)], limit: 1 })).collect();
        let mut clamp = vec![None; n]; clamp[n - 1] = Some(0);
        let m = Model::new(n, k, vec![0.0; n * k], vec![true; n * k], clamp, vec![], caps).unwrap();
        let s = sample(&m, 2, 20, None, 1, true, false).unwrap(); assert_eq!(s.viol, 0);
        let (fz, esc) = frozen_detail(&m, &s); assert_eq!(fz, 0); assert!(esc.iter().all(|&e| !e));
    }
    /// Unit-time scheduling as an IR program: jobs = variables, slots = values, windows = allowed,
    /// one capacity per slot, precedence i -> j as pair caps "at most one of (i, a), (j, b)" for a >= b. The exact tier must
    /// match brute force over all 4^6 slot vectors checked by hand; a warm-started chain (`Chain::from_state`) and `anneal` from
    /// a given plan stay feasible and never lose the start's log w.
    #[test]
    fn schedule_program_matches_brute_force_and_warm_starts() {
        let (n, t, cap) = (6usize, 4usize, 2usize); let win = [(0, 3), (0, 2), (1, 3), (0, 3), (1, 3), (0, 3)]; let prec = [(0, 3), (1, 4), (3, 5)];
        let h: Vec<f64> = (0..n * t).map(|q| -0.4 * (q % t) as f64 * (1.0 + (q / t) as f64 / 5.0) + 0.1 * ((q * 7) % 5) as f64).collect();
        let allowed: Vec<bool> = (0..n * t).map(|q| (win[q / t].0..=win[q / t].1).contains(&(q % t))).collect();
        let mut caps: Vec<Cap> = (0..t).map(|s| Cap { members: (0..n).filter(|&i| allowed[i * t + s]).map(|i| (i, s)).collect(), limit: cap }).collect();
        for &(i, j) in &prec { for a in 0..t { for b in 0..=a { if allowed[i * t + a] && allowed[j * t + b] { caps.push(Cap { members: vec![(i, a), (j, b)], limit: 1 }); } } } }
        let m = Model::new(n, t, h.clone(), allowed.clone(), vec![None; n], vec![], caps).unwrap(); assert!(!m.is_partition());
        let (mut z, mut marg, mut cnt, mut best) = (0.0f64, vec![0.0; n * t], 0u64, (f64::NEG_INFINITY, vec![]));
        for code in 0..t.pow(n as u32) { let x: Vec<usize> = (0..n).map(|i| code / t.pow(i as u32) % t).collect();
            let ok = (0..n).all(|i| allowed[i * t + x[i]]) && (0..t).all(|s| x.iter().filter(|&&v| v == s).count() <= cap) && prec.iter().all(|&(i, j)| x[i] < x[j]);
            if !ok { assert!(m.violations(&x) > 0 || (0..n).any(|i| !allowed[i * t + x[i]])); continue; }
            assert_eq!(m.violations(&x), 0); let lw: f64 = (0..n).map(|i| h[i * t + x[i]]).sum(); let w = lw.exp(); z += w; cnt += 1;
            for i in 0..n { marg[i * t + x[i]] += w; } if lw > best.0 { best = (lw, x); } }
        let e = exact(&m, 1, 1 << 20).unwrap(); assert_eq!(e.n_feasible, cnt); assert!((e.logz - z.ln()).abs() < 1e-9, "{} vs {}", e.logz, z.ln());
        assert!(e.marg.iter().zip(&marg).all(|(a, b)| (a - b / z).abs() < 1e-12)); assert_eq!(e.top[0].1, best.1);
        let worst = (0..t.pow(n as u32)).map(|c| (0..n).map(|i| c / t.pow(i as u32) % t).collect::<Vec<usize>>()).filter(|x| m.violations(x) == 0 && (0..n).all(|i| allowed[i * t + x[i]]))
            .min_by(|a, b| m.logw(a).partial_cmp(&m.logw(b)).unwrap()).unwrap();
        let mut ch = Chain::from_state(&m, 3, 0, &worst); assert_eq!(ch.x, worst);
        for _ in 0..50 { ch.sweep(); assert_eq!(m.violations(&ch.x), 0); }
        let (lw, x) = anneal_sweeps(&m, Some(&worst), &[1.0, 4.0, 16.0], 300, 2, 7).unwrap();
        assert_eq!(m.violations(&x), 0); assert!(lw >= m.logw(&worst) && lw <= best.0 + 1e-12);
        let s = sample(&m, 4, 20_000, None, 11, true, false).unwrap(); assert_eq!(s.viol, 0); let tv = max_tv(&s.marg, &e.marg, t); assert!(tv < 0.03, "maxTV {tv}");
    }
    /// The random feasible start of non-partition programs has a work budget; past it the search retries in ascending
    /// value order (MRV, then input order). On programs the random search finishes within budget the start is unchanged
    /// (checked against an unlimited budget); a zero budget forces the retry, which must still return a feasible start.
    #[test]
    fn feasible_start_work_budget_and_retry() {
        let (n, t) = (6usize, 4usize); let allowed = vec![true; n * t];
        let mut caps: Vec<Cap> = (0..t).map(|s| Cap { members: (0..n).map(|i| (i, s)).collect(), limit: 2 }).collect();
        for &(i, j) in &[(0usize, 3usize), (1, 4), (3, 5)] { for a in 0..t { for b in 0..=a { caps.push(Cap { members: vec![(i, a), (j, b)], limit: 1 }); } } }
        let m = Model::new(n, t, vec![0.0; n * t], allowed, vec![None; n], vec![], caps).unwrap(); assert!(!m.is_partition());
        for seed in 0..20u64 {
            let a = m.feasible_init_rand(&mut Philox4x32::new(seed, 1)); let b = m.feasible_init_with(&mut Philox4x32::new(seed, 1), u64::MAX); assert_eq!(a, b);
            let r = m.feasible_init_with(&mut Philox4x32::new(seed, 1), 0).expect("the ascending retry finds a start"); assert_eq!(m.violations(&r), 0);
        }
        let g = { let mut c = vec![]; for i in 0..6 { for j in i + 1..6 { if (i + j) % 3 != 0 { for v in 0..3 { c.push(Cap { members: vec![(i, v), (j, v)], limit: 1 }); } } } } c };
        let col = Model::new(6, 3, vec![0.0; 18], vec![true; 18], vec![None; 6], vec![], g).unwrap();
        for seed in 0..20u64 { assert_eq!(col.feasible_init_rand(&mut Philox4x32::new(seed, 2)), col.feasible_init_with(&mut Philox4x32::new(seed, 2), u64::MAX)); }
    }
    /// Thread count is a resource control, not a semantic one: fixed sweeps => bit-identical for 1..=5 threads
    /// (and under a 40% CPU duty cycle).
    #[test]
    fn frontier_declines_when_a_cap_load_can_pass_255() {
        // n free spins under an at-most-n cap on "on" (every spin its own component, the cap carried across all of them):
        // log Z = n ln 2. At n = 200 the frontier answers exactly; at n = 600 it used to answer 406.99 (truth 415.89) and now declines
        for (n, answers) in [(200usize, true), (600, false)] {
            let caps = vec![Cap { members: (0..n).map(|i| (i, 1)).collect(), limit: n }];
            let m = Model::new(n, 2, vec![0.0; n * 2], vec![true; n * 2], vec![None; n], vec![], caps).unwrap();
            match exact_frontier(&m, 4096) { Some(f) => { assert!(answers, "n {n}"); assert!((f.logz - n as f64 * 2f64.ln()).abs() < 1e-9, "n {n}: {}", f.logz); }
                None => assert!(!answers, "n {n}") } }
    }
    #[test]
    fn exact_pass_two_is_not_cut_short() {
        // 7 free variables over 10 values + 3 needles (value 7, 8, 9), one cap per value: 7! = 5040 plans but ~1M DFS
        // nodes in the static order. Pass 1 used to inherit pass 0's node count: limits 15,921-30,000 returned 2 / 80 / 1,960 /
        // 4,188 plans (a wrong log Z presented as exact). Now every limit either declines or returns all 5040.
        let (n, k) = (10usize, 10usize);
        let allowed: Vec<bool> = (0..n * k).map(|q| q / k < 7 || q % k == q / k).collect();
        let caps: Vec<Cap> = (0..k).map(|v| Cap { members: (0..n).filter(|&i| allowed[i * k + v]).map(|i| (i, v)).collect(), limit: 1 }).collect();
        let m = Model::new(n, k, (0..n * k).map(|q| 0.01 * (q % 7) as f64).collect(), allowed, vec![None; n], vec![], caps).unwrap(); assert!(m.is_partition());
        let full = exact(&m, 1, 1 << 20).unwrap(); assert_eq!(full.n_feasible, 5040);
        let mut answered = 0;
        for lim in [5040u64, 15_000, 15_921, 16_000, 20_000, 25_000, 31_000] { if let Some(e) = exact(&m, 1, lim) { answered += 1;
            assert_eq!(e.n_feasible, 5040, "limit {lim}"); assert_eq!(e.logz.to_bits(), full.logz.to_bits(), "limit {lim}"); } }
        assert_eq!(answered, 5);
    }
    #[test]
    fn exact_mrv_declines_a_thrashing_search() {
        // Pigeonhole (9 pigeons, 8 holes) with each hole's cap repeated 100x (non-partition, ~110K MRV nodes, a few
        // hundred M capacity checks and no plan): the default tier gives up after EXACT_GAP_WORK checks without a new plan;
        // an unbounded limit (`--mode exact`) still proves the program infeasible.
        let (p, h, dup) = (9usize, 8usize, 100usize);
        let caps: Vec<Cap> = (0..h * dup).map(|c| Cap { members: (0..p).map(|i| (i, c % h)).collect(), limit: 1 }).collect();
        let m = Model::new(p, h, vec![0.0; p * h], vec![true; p * h], vec![None; p], vec![], caps).unwrap(); assert!(!m.is_partition());
        assert!(exact(&m, 1, 1).is_none());
        assert_eq!(exact(&m, 1, u64::MAX / 128).expect("unbounded limit").n_feasible, 0);
    }
    #[test]
    fn exact_until_searches_past_the_gap_budget_to_the_deadline() {
        // Var A (values a0, a1) is branched first; A = a0 takes hole h8, leaving 9 pigeons for 8 holes whose caps are
        // listed 100x (the thrashing pigeonhole above: > EXACT_GAP_WORK checks, no plan); A = a1 leaves h8 to pigeon 8: 8! plans.
        // Without a deadline the tier declines in the thrash; with one it runs on, and pass 1 must not cut (it would return a
        // partial sum): the answer equals the unbounded enumeration bit for bit. A passed deadline declines.
        let (p, h, dup) = (9usize, 8usize, 100usize); let (k, a) = (h + 3, p); let (a0, a1) = (h + 1, h + 2);
        let mut allowed = vec![false; (p + 1) * k]; for i in 0..p { for v in 0..h { allowed[i * k + v] = true; } } allowed[(p - 1) * k + h] = true;
        allowed[a * k + a0] = true; allowed[a * k + a1] = true;
        let mut caps: Vec<Cap> = (0..h * dup).map(|c| Cap { members: (0..p).map(|i| (i, c % h)).collect(), limit: 1 }).collect();
        caps.push(Cap { members: vec![(p - 1, h), (a, a0)], limit: 1 });
        let hh: Vec<f64> = (0..(p + 1) * k).map(|q| ((q * 7) % 5) as f64 * 0.1).collect();
        let m = Model::new(p + 1, k, hh, allowed, vec![None; p + 1], vec![], caps).unwrap(); assert!(!m.is_partition());
        assert!(exact(&m, 1, 1 << 20).is_none(), "the gap budget declines in the thrash");
        let full = exact(&m, 1, u64::MAX / 128).unwrap(); assert_eq!(full.n_feasible, 40_320);
        let now = std::time::Instant::now();
        let (e, ext) = exact_until(&m, 1, 1 << 20, Some(now + std::time::Duration::from_secs(3600))); let e = e.expect("runs on to the deadline");
        assert!(ext); assert_eq!(e.n_feasible, 40_320); assert_eq!(e.logz.to_bits(), full.logz.to_bits());
        assert!(e.marg.iter().zip(&full.marg).all(|(x, y)| x.to_bits() == y.to_bits())); assert_eq!(e.top[0].1, full.top[0].1);
        assert!(exact_until(&m, 1, 1 << 20, Some(now)).0.is_none(), "a passed deadline declines");
    }
    #[test]
    fn exact_tiers_on_100k_variables_do_not_overflow_the_stack() {
        // 100,000 clamped variables: the enumeration recursed 100k deep on the caller's stack (the CLI main thread
        // aborted, exit 134; a test thread has 2 MiB). With a coupling path the frontier's tabulation recursed as deep.
        let n = 100_000; let h: Vec<f64> = (0..2 * n).map(|q| (q % 3) as f64 * 0.1).collect(); let mut clamp = vec![Some(0usize); n]; clamp[7] = Some(1);
        let m = Model::new(n, 2, h.clone(), vec![true; 2 * n], clamp.clone(), vec![], vec![]).unwrap();
        let e = exact(&m, 1, 1 << 20).expect("one plan"); assert_eq!(e.n_feasible, 1); assert_eq!(e.top[0].1[7], 1);
        let pairs = (0..n - 1).map(|i| Pair { i, j: i + 1, c: Coupling::Potts(0.2) }).collect();
        let m = Model::new(n, 2, h, vec![true; 2 * n], clamp, pairs, vec![]).unwrap();
        let (e, f) = (exact(&m, 1, 1 << 20).expect("one plan"), exact_frontier(&m, FRONTIER_MAX_STATES).expect("one component, one assignment"));
        assert_eq!(e.n_feasible, 1); assert_eq!(f.map, e.top[0].1); assert!((f.logz - e.logz).abs() < 1e-6 * e.logz.abs().max(1.0), "{} vs {}", f.logz, e.logz);
    }
    #[test]
    fn exact_within_hard_stop_declines_and_a_far_one_changes_nothing() {
        // --exact-ms: a passed hard stop declines every exact tier (MRV on a non-partition program; static order and
        // frontier DP on a partition one); a far one returns the unbounded answer bit for bit; a near one stops a long search.
        let now = std::time::Instant::now(); let far = Some(now + std::time::Duration::from_secs(3600));
        let same = |a: &Exact, b: &Exact| a.n_feasible == b.n_feasible && a.logz.to_bits() == b.logz.to_bits() && a.top[0].1 == b.top[0].1
            && a.marg.iter().zip(&b.marg).all(|(x, y)| x.to_bits() == y.to_bits());
        let m = toy(); assert!(!m.is_partition()); let e = exact(&m, 3, 1 << 20).unwrap();
        assert!(same(&exact_within(&m, 3, 1 << 20, None, far).0.unwrap(), &e));
        assert!(exact_within(&m, 3, 1 << 20, None, Some(now)).0.is_none(), "MRV: a passed hard stop declines");
        let (n, k) = (10usize, 2usize); let h: Vec<f64> = (0..n * k).map(|q| ((q * 7) % 5) as f64 * 0.1 - 0.2).collect();
        let pairs = (0..n - 1).map(|i| Pair { i, j: i + 1, c: Coupling::Potts(0.3) }).collect();
        let m = Model::new(n, k, h, vec![true; n * k], vec![None; n], pairs, vec![Cap { members: (0..n).map(|i| (i, 1)).collect(), limit: 4 }]).unwrap();
        assert!(m.is_partition()); let e = exact(&m, 3, 1 << 20).unwrap(); assert_eq!(e.n_feasible, 386);
        assert!(same(&exact_within(&m, 3, 1 << 20, None, far).0.unwrap(), &e));
        assert!(exact_within(&m, 3, 1 << 20, None, Some(now)).0.is_none(), "static order: a passed hard stop declines");
        let (f, g) = (exact_frontier(&m, FRONTIER_MAX_STATES).unwrap(), exact_frontier_until(&m, FRONTIER_MAX_STATES, far).unwrap());
        assert_eq!(f.logz.to_bits(), g.logz.to_bits()); assert_eq!(f.map, g.map); assert!(f.marg.iter().zip(&g.marg).all(|(x, y)| x.to_bits() == y.to_bits()));
        assert!(exact_frontier_until(&m, FRONTIER_MAX_STATES, Some(now)).is_none(), "frontier: a passed hard stop declines");
        // mid-search: the thrashing pigeonhole of the test above, unbounded limit (no gap cut): a 20 ms stop ends it
        let (p, h, dup) = (9usize, 8usize, 100usize); let (k, a) = (h + 3, p); let (a0, a1) = (h + 1, h + 2);
        let mut allowed = vec![false; (p + 1) * k]; for i in 0..p { for v in 0..h { allowed[i * k + v] = true; } } allowed[(p - 1) * k + h] = true;
        allowed[a * k + a0] = true; allowed[a * k + a1] = true;
        let mut caps: Vec<Cap> = (0..h * dup).map(|c| Cap { members: (0..p).map(|i| (i, c % h)).collect(), limit: 1 }).collect();
        caps.push(Cap { members: vec![(p - 1, h), (a, a0)], limit: 1 });
        let m = Model::new(p + 1, k, vec![0.0; (p + 1) * k], allowed, vec![None; p + 1], vec![], caps).unwrap();
        let t = std::time::Instant::now(); let r = exact_within(&m, 1, u64::MAX / 128, None, Some(t + std::time::Duration::from_millis(20)));
        let el = t.elapsed().as_secs_f64() * 1e3; assert!(r.0.is_none() && el < 500.0, "stopped after {el} ms");
    }
    #[test]
    fn start_retries_after_node_budget_and_rerank() {
        // schedule_oracle's PROBE_START=100 instance (layered DAG, 100 jobs x 20 slots, cap 5, precedence pair caps).
        // Before: chain streams 3, 4, 5 (seed 60) gave up with 0 retries (the random attempt ran out of NODES, which skipped the
        // retry); after the node-budget fix streams 3 and 4 still failed both retries; the fresh-tie-break MRV attempts start them.
        let (n, t, cap) = (100usize, 20usize, 5usize); let mut r = Philox4x32::new(4242, 100_000); let nl = t / 3; let layer = |i: usize| i * nl / n;
        let mut pred = vec![vec![]; n]; for i in 0..n { for j in i + 1..n { if layer(j) == layer(i) + 1 && r.f64() < nl as f64 / n as f64 { pred[j].push(i); } } }
        let win: Vec<(usize, usize)> = (0..n).map(|_| (r.below(t / 4 + 1), t - 1 - r.below(t / 4 + 1))).collect();
        let allowed: Vec<bool> = (0..n * t).map(|q| (win[q / t].0..=win[q / t].1).contains(&(q % t))).collect();
        let mut caps: Vec<Cap> = (0..t).map(|s| Cap { members: (0..n).filter(|&i| allowed[i * t + s]).map(|i| (i, s)).collect(), limit: cap }).collect();
        for j in 0..n { for &i in &pred[j] { for a in 0..t { for b in 0..=a { if allowed[i * t + a] && allowed[j * t + b] { caps.push(Cap { members: vec![(i, a), (j, b)], limit: 1 }); } } } } }
        let m = Model::new(n, t, vec![0.0; n * t], allowed, vec![None; n], vec![], caps).unwrap(); assert_eq!(m.caps.len(), 10_710);
        let s = sample_on(&m, 5, 5, 1, None, 60, false, 100, 0).expect("all 5 chains start"); assert_eq!(s.viol, 0);
        // The start search gives up at a passed deadline (stream 3 needs > 4096 nodes); a far deadline changes nothing
        let now = std::time::Instant::now(); assert!(m.feasible_init_until(&mut Philox4x32::new(60, 3), START_WORK, Some(now)).is_none());
        assert_eq!(m.feasible_init_until(&mut Philox4x32::new(60, 7), START_WORK, Some(now + std::time::Duration::from_secs(3600))), m.feasible_init_rand(&mut Philox4x32::new(60, 7)));
    }
    #[test]
    fn sample_on_is_thread_count_invariant() {
        let m = toy(); let a = sample(&m, 5, 1500, None, 9, true, true).unwrap();
        for t in 1..=5 { let b = sample_on(&m, 5, t, 1500, None, 9, true, if t == 3 { 40 } else { 100 }, 0).unwrap();
            assert!(a.marg.iter().zip(&b.marg).all(|(x, y)| x.to_bits() == y.to_bits()), "threads {t}"); assert_eq!(a.traj, b.traj); assert_eq!(a.plans, b.plans); }
    }
}
