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
/// At most `limit` of the listed (variable, value) pairs may be active at once. With `weights` (R19.7, P2.1 linear <=):
/// the weights of the active members sum to at most `limit` (one weight per member, each in 1..=MAX_CAP_WEIGHT; empty =
/// every weight 1, the unit cap). Knapsack / bin packing: one weighted cap per budget / bin, members never replicated.
#[derive(Clone, Debug, PartialEq)]
pub struct Cap { pub members: Vec<(usize, usize)>, pub limit: usize, pub weights: Vec<usize> }
impl Cap {
    /// At most `limit` of `members` (unit weights).
    pub fn new(members: Vec<(usize, usize)>, limit: usize) -> Cap { Cap { members, limit, weights: vec![] } }
    /// The weight of member `t` (1 on a unit cap).
    #[inline] pub fn w(&self, t: usize) -> usize { self.weights.get(t).copied().unwrap_or(1) }
}
/// R19.7: the largest weight of one weighted-cap member. Loads are sums of weights in 64-bit integers: exact, no overflow.
pub const MAX_CAP_WEIGHT: usize = 1_000_000;

#[derive(Clone, Debug)]
pub struct Model {
    /// R19 P1.1(a), WIP: add one global two-value flip per sweep (`Chain::global_flip`). Off by default: the router's own
    /// sampler has no such move yet, and `ir_lowering_bit_identical` pins the two samplers to each other.
    pub collective: bool,
    /// R19 P1.1(d), WIP: a Wolff cluster move over the non-negative Potts bonds (`Chain::cluster_move`), attempted with
    /// probability 1/2 per sweep. Every sweep, it and the global flip both flipped a complete-graph ferromagnet whole, so the
    /// two cancelled: chains stopped crossing modes and ferro12 gave 2/10 false whole answers (R19.2); a random mixture of
    /// pi-invariant moves is pi-invariant and breaks the lock-step. Off by default (`--cluster on`); the router mirrors it.
    pub cluster: bool,
    /// R19 P1.1(c), WIP: max(n/4, 1) Metropolis three-cycle rotations per sweep (`Chain::cycle3`). Off by default; IR only.
    pub cycles: bool,
    /// R19.6 (P2.1): an optional warm start, one value per variable, that the caller has checked is feasible (`pbit run`
    /// validates it). Chain 0 (stream 0) starts from it instead of its own start search; the other chains search as before,
    /// so the starts stay over-dispersed. None (default) = unchanged.
    pub start: Option<Vec<usize>>,
    pub n: usize, pub k: usize, pub h: Vec<f64>, pub allowed: Vec<bool>, pub clamp: Vec<Option<usize>>,
    pub pairs: Vec<Pair>, pub caps: Vec<Cap>,
    /// per variable: (other variable, pair index), sorted by other variable then pair index
    adj: Vec<Vec<(usize, usize)>>,
    /// per (i, v): the capacity constraints containing it
    cap_of: Vec<Vec<usize>>,
    /// R19.7: per (i, v) the weight of each membership in `cap_of` (aligned); empty when every weight is 1
    capw: Vec<Vec<usize>>,
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
        for (c, cp) in caps.iter().enumerate() {
            if !cp.weights.is_empty() && cp.weights.len() != cp.members.len() { return Err(format!("capacity {c}: {} weights for {} members", cp.weights.len(), cp.members.len())); }
            if let Some(&w) = cp.weights.iter().find(|&&w| w == 0 || w > MAX_CAP_WEIGHT) { return Err(format!("capacity {c}: weight {w} outside 1..={MAX_CAP_WEIGHT}")); }
            for &(i, v) in &cp.members {
            if i >= n || v >= k { return Err(format!("capacity {c}: member ({i}, {v}) out of range")); }
            if cap_of[i * k + v].last() == Some(&c) { return Err(format!("capacity {c}: duplicate member ({i}, {v})")); }
            cap_of[i * k + v].push(c); } }
        let capw: Vec<Vec<usize>> = if caps.iter().any(|c| c.weights.iter().any(|&w| w != 1)) {
            let mut cw = vec![vec![]; n * k]; for cp in &caps { for (t, &(i, v)) in cp.members.iter().enumerate() { cw[i * k + v].push(cp.w(t)); } } cw } else { vec![] };
        // a weighted program is never a partition: the matching start, bucket loads and frontier lanes count members
        let partition = capw.is_empty() && cap_of.iter().all(|c| c.len() <= 1);
        let cand = (0..n).map(|i| (0..k).filter(|&v| allowed[i * k + v]).collect()).collect();
        let bucket = cap_of.iter().map(|c| c.first().map_or(NONE, |&b| b as u32)).collect();
        let limit = caps.iter().map(|c| c.limit).collect();
        let padj = adj.iter().map(|a| a.iter().filter_map(|&(j, q)| match pairs[q].c { Coupling::Potts(w) => Some((j, w)), _ => None }).collect()).collect();
        let tadj = adj.iter().map(|a| a.iter().copied().filter(|&(_, q)| matches!(pairs[q].c, Coupling::Table(_))).collect()).collect();
        Ok(Model { n, k, h, allowed, clamp, pairs, caps, adj, cap_of, capw, cand, partition, bucket, limit, padj, tadj, collective: false, cluster: false, cycles: false, start: None })
    }
    /// The same program at inverse temperature `beta`: every log-weight scaled, hard rules unchanged.
    pub fn scaled(&self, beta: f64) -> Model {
        let h = self.h.iter().map(|v| v * beta).collect();
        let pairs = self.pairs.iter().map(|p| Pair { i: p.i, j: p.j, c: match &p.c { Coupling::Potts(w) => Coupling::Potts(w * beta), Coupling::Table(t) => Coupling::Table(t.iter().map(|v| v * beta).collect()) } }).collect();
        Model::new(self.n, self.k, h, self.allowed.clone(), self.clamp.clone(), pairs, self.caps.clone()).expect("scaling keeps a valid model")
    }
    /// What-if: the same program with variable `i` forced to value `v`.
    pub fn with_clamp(&self, i: usize, v: usize) -> Model { let mut m = self.clone(); m.clamp[i] = Some(v); if m.start.as_ref().is_some_and(|s| s.get(i) != Some(&v)) { m.start = None; } m }
    #[inline] pub fn ok(&self, i: usize, v: usize) -> bool { self.allowed[i * self.k + v] && self.clamp[i].map_or(true, |c| c == v) }
    /// Number of values variable `i` may take (allowed, and the clamp if set).
    pub fn cand_count(&self, i: usize) -> usize { match self.clamp[i] { Some(c) => self.allowed[i * self.k + c] as usize, None => self.cand[i].len() } }
    /// Every (variable, value) is in at most one capacity constraint.
    pub fn is_partition(&self) -> bool { self.partition }
    /// Some capacity constraint has a member weight other than 1 (R19.7 linear <=).
    pub fn weighted(&self) -> bool { !self.capw.is_empty() }
    /// R19.7: the weight of the `t`-th capacity membership of (variable, value) index `iv` (1 on unweighted programs).
    #[inline] fn cw(&self, iv: usize, t: usize) -> usize { if self.capw.is_empty() { 1 } else { self.capw[iv][t] } }
    /// Add / remove the capacity memberships of index `iv` (i * k + v) to / from `load` (weights included).
    #[inline] fn add_load(&self, load: &mut [usize], iv: usize) {
        if self.capw.is_empty() { for &c in &self.cap_of[iv] { load[c] += 1; } } else { for (&c, &w) in self.cap_of[iv].iter().zip(&self.capw[iv]) { load[c] += w; } } }
    #[inline] fn sub_load(&self, load: &mut [usize], iv: usize) {
        if self.capw.is_empty() { for &c in &self.cap_of[iv] { load[c] -= 1; } } else { for (&c, &w) in self.cap_of[iv].iter().zip(&self.capw[iv]) { load[c] -= w; } } }
    /// Index `iv` can be added at `load` without breaking a capacity constraint.
    #[inline] fn fits_load(&self, load: &[usize], iv: usize) -> bool {
        if self.capw.is_empty() { self.cap_of[iv].iter().all(|&c| load[c] < self.limit[c]) } else { self.cap_of[iv].iter().zip(&self.capw[iv]).all(|(&c, &w)| load[c] + w <= self.limit[c]) } }
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
        for i in 0..self.n { if !self.ok(i, x[i]) { v += 1; } self.add_load(&mut load, i * self.k + x[i]); }
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
        // R19.6: programs whose two-variable forbid caps prune a value under root arc consistency (precedence chains: job i
        // has no slot before i) first try a search that MAINTAINS arc consistency over those caps (MAC) with forward checking
        // on full caps: on a chain it never backtracks. Without it a 20-job x 30-slot chain thrashed past every start deadline.
        // Programs where nothing is pruned skip it: their starts and RNG streams are unchanged.
        if START_MAC.load(std::sync::atomic::Ordering::Relaxed) { match self.mac_start(&key, &rank, first_work, deadline) { Ok(x) => return Some(x), Err(true) => return None, Err(false) => {} } }
        #[allow(clippy::too_many_arguments)]
        fn dfs(m: &Model, left: usize, rank: &[usize], x: &mut [usize], load: &mut [usize], nodes: &mut u64, work: &mut u64, budget: u64, order_of: &dyn Fn(usize) -> Vec<usize>, mrv: bool, dl: (Option<std::time::Instant>, &mut bool)) -> bool {
            if left == 0 { return true; } *nodes += 1; if *nodes > 2_000_000 || *work > budget || *dl.1 { return false; }
            if *nodes & 63 == 0 { if let Some(d) = dl.0 { if std::time::Instant::now() >= d { *dl.1 = true; return false; } } }
            let fits = |i: usize, v: usize, load: &[usize]| m.fits_load(load, i * m.k + v);
            let mut pick = (usize::MAX, usize::MAX, 0usize); // (feasible count, rank, var)
            for i in 0..m.n { if x[i] != usize::MAX { continue; }
                let cnt = m.cand[i].iter().filter(|&&v| { *work += 1 + m.cap_of[i * m.k + v].len() as u64; m.ok(i, v) && fits(i, v, load) }).count();
                let key = if mrv { (cnt, rank[i]) } else { (0, i) }; if key < (pick.0, pick.1) { pick = (key.0, key.1, i); } if cnt == 0 { return false; } }
            let i = pick.2;
            for v in order_of(i) { if !fits(i, v, load) { continue; }
                m.add_load(load, i * m.k + v); x[i] = v;
                if dfs(m, left - 1, rank, x, load, nodes, work, budget, order_of, mrv, (dl.0, &mut *dl.1)) { return true; }
                m.sub_load(load, i * m.k + v); x[i] = usize::MAX; }
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
    /// The R19.6 MAC start of `feasible_init_until`: binary forbid caps (limit 1, members on two different variables) become
    /// per-(variable pair) bit tables; root arc consistency runs first and, if it prunes nothing, the start is not attempted.
    /// Otherwise a depth-first search (MRV on the propagated domains, the caller's random value order and tie-break) assigns a
    /// value, removes the members of every cap that becomes full from the open domains, and re-establishes arc consistency
    /// (AC-3); a wiped-out domain backtracks. Budgets as the plain search (START_WORK-style work, 2M nodes, clock every 64 nodes).
    /// Ok = a feasible start; Err(true) = `deadline` passed; Err(false) = not applicable or budgets spent (the plain search runs).
    fn mac_start(&self, key: &[f64], rank: &[usize], budget: u64, deadline: Option<std::time::Instant>) -> Result<Vec<usize>, bool> {
        let (n, k) = (self.n, self.k); let wd = k.div_ceil(64);
        let mut idx: HashMap<(usize, usize), usize> = HashMap::new(); let mut tab: Vec<Vec<u64>> = vec![]; let mut arcs: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
        for cp in &self.caps { if cp.limit != 1 || cp.members.len() != 2 || cp.members[0].0 == cp.members[1].0 { continue; }
            let ((a, s), (b, t)) = (cp.members[0], cp.members[1]);
            for (a, s, b, t) in [(a, s, b, t), (b, t, a, s)] { // row s of table (a -> b) = the values of b forbidden with a = s
                let q = *idx.entry((a, b)).or_insert_with(|| { tab.push(vec![0; k * wd]); arcs[a].push((b, tab.len() - 1)); tab.len() - 1 });
                tab[q][s * wd + t / 64] |= 1u64 << (t % 64); } }
        if tab.is_empty() { return Err(false); }
        let mut dom = vec![0u64; n * wd];
        for i in 0..n { for &v in &self.cand[i] { if self.ok(i, v) && self.cap_of[i * k + v].iter().enumerate().all(|(t, &c)| self.cw(i * k + v, t) <= self.limit[c]) { dom[i * wd + v / 64] |= 1u64 << (v % 64); } } }
        let count = |d: &[u64]| d.iter().map(|w| w.count_ones() as u64).sum::<u64>(); let before = count(&dom);
        let mut work = 0u64;
        if !ac3(&tab, &arcs, &mut dom, wd, (0..n).collect(), &mut work) || count(&dom) == before { return Err(false); }
        fn ac3(tab: &[Vec<u64>], arcs: &[Vec<(usize, usize)>], dom: &mut [u64], wd: usize, mut queue: Vec<usize>, work: &mut u64) -> bool {
            let mut inq = vec![false; arcs.len()]; for &a in &queue { inq[a] = true; } let mut all = vec![0u64; wd];
            while let Some(a) = queue.pop() { inq[a] = false;
                for &(b, q) in &arcs[a] { all.fill(!0); // values of b forbidden with EVERY value left to a
                    'rows: for w in 0..wd { let mut bits = dom[a * wd + w]; while bits != 0 { let s = w * 64 + bits.trailing_zeros() as usize; bits &= bits - 1;
                        *work += wd as u64; let mut z = true; for (u, al) in all.iter_mut().enumerate() { *al &= tab[q][s * wd + u]; z &= *al == 0; } if z { break 'rows; } } }
                    let (mut ch, mut any) = (false, false);
                    for (u, al) in all.iter().enumerate() { let d = &mut dom[b * wd + u]; let nd = *d & !al; ch |= nd != *d; *d = nd; any |= nd != 0; }
                    if !any { return false; } if ch && !inq[b] { inq[b] = true; queue.push(b); } } }
            true
        }
        #[allow(clippy::too_many_arguments)]
        fn go(m: &Model, tab: &[Vec<u64>], arcs: &[Vec<(usize, usize)>], wd: usize, left: usize, key: &[f64], rank: &[usize], dom: &mut Vec<u64>, x: &mut [usize], load: &mut [usize],
            nodes: &mut u64, work: &mut u64, budget: u64, dl: Option<std::time::Instant>, late: &mut bool) -> bool {
            if left == 0 { return true; } *nodes += 1; if *nodes > 2_000_000 || *work > budget || *late { return false; }
            if *nodes & 63 == 0 { if let Some(d) = dl { if std::time::Instant::now() >= d { *late = true; return false; } } }
            let (n, k) = (m.n, m.k); let mut pick = (u64::MAX, usize::MAX, 0usize);
            for i in 0..n { if x[i] != usize::MAX { continue; } let c: u64 = dom[i * wd..(i + 1) * wd].iter().map(|w| w.count_ones() as u64).sum(); if (c, rank[i]) < (pick.0, pick.1) { pick = (c, rank[i], i); } }
            *work += (n * wd) as u64; let i = pick.2;
            let mut vals: Vec<usize> = (0..k).filter(|&v| dom[i * wd + v / 64] >> (v % 64) & 1 == 1).collect();
            vals.sort_by(|&u, &v| key[i * k + u].partial_cmp(&key[i * k + v]).unwrap());
            for v in vals {
                let saved = dom.clone(); x[i] = v; dom[i * wd..(i + 1) * wd].fill(0); dom[i * wd + v / 64] = 1u64 << (v % 64); let mut q = vec![i];
                // forward checking: a full unit cap prunes its open members; a weighted cap (R19.7) every open member that no longer fits
                for (t, &c) in m.cap_of[i * k + v].iter().enumerate() { load[c] += m.cw(i * k + v, t); let cp = &m.caps[c];
                    if load[c] >= m.limit[c] || !cp.weights.is_empty() { *work += cp.members.len() as u64;
                    for (r, &(j, u)) in cp.members.iter().enumerate() { if x[j] == usize::MAX && load[c] + cp.w(r) > m.limit[c] && dom[j * wd + u / 64] >> (u % 64) & 1 == 1 { dom[j * wd + u / 64] &= !(1u64 << (u % 64)); q.push(j); } } } }
                let ok = q.iter().all(|&j| dom[j * wd..(j + 1) * wd].iter().any(|&w| w != 0)) && ac3(tab, arcs, dom, wd, q, work);
                if ok && go(m, tab, arcs, wd, left - 1, key, rank, dom, x, load, nodes, work, budget, dl, late) { return true; }
                m.sub_load(load, i * k + v); x[i] = usize::MAX; *dom = saved;
                if *late || *work > budget || *nodes > 2_000_000 { return false; }
            }
            false
        }
        let (mut x, mut load, mut nodes, mut late) = (vec![usize::MAX; n], vec![0usize; self.caps.len()], 0u64, false);
        if go(self, &tab, &arcs, wd, n, key, rank, &mut dom, &mut x, &mut load, &mut nodes, &mut work, budget, deadline, &mut late) {
            START_PROPAGATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed); return Ok(x); }
        Err(late)
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
        let fits = |ld: &[usize], i: usize, v: usize| m.ok(i, v) && m.fits_load(ld, i * k + v);
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
            m.add_load(&mut self.load, i * k + v); self.x[i] = v;
            self.dfs_mrv(left - 1, lw + d);
            m.sub_load(&mut self.load, i * k + v); self.x[i] = usize::MAX;
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
            else if !m.fits_load(&self.load, i * k + v) { continue; }
            let mut d = m.h[i * k + v];
            for &(j, w) in &m.padj[i] { if j >= i { break; } if self.x[j] == v { d += w; } }
            for &(j, q) in &m.tadj[i] { if j < i { d += m.coupling(q, i, v, self.x[j]); } }
            if m.partition { if b != NONE { self.load[b as usize] += 1; } } else { m.add_load(&mut self.load, i * k + v); }
            self.x[i] = v; self.dfs(i + 1, lw + d);
            if m.partition { if b != NONE { self.load[b as usize] -= 1; } } else { m.sub_load(&mut self.load, i * k + v); }
        }
    }
}

// ---------------- exact: frontier DP (the assignment router's frontier tier, generalised) ----------------
/// Per connected component of the coupling graph: members (ascending), the capacity constraints its allowed values touch
/// (ascending), and its assignments bucketed by their load vector on those caps: (loads, log-sum w, [(log w, values)]).
struct CTab { mem: Vec<usize>, caps: Vec<usize>, buckets: Vec<(Vec<u8>, f64, Vec<(f64, Vec<u16>)>)> }
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
        out.push(CTab { mem, caps, buckets });
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
    // The DP runs in LOG space. It used linear weights scaled by each layer's max: states more than ~745 nats below the max
    // underflowed to 0 (and can matter later: the continuation that dominates may start from one of them), and a layer whose
    // surviving states had all underflowed divided 0 by 0 (NaN marginals labelled exact; the CLI then panicked sorting them,
    // exit 134; found by the R19 fuzz test with weights of 1e9). Each state carries a log weight; same-key states merge by
    // log-sum-exp; backward messages are per-state log-sum-exps.
    let mut alpha: Vec<Vec<(u128, f64)>> = vec![vec![(0, 0.0)]]; let mut mxp: Vec<Vec<(u128, f64)>> = vec![vec![(0, 0.0)]];
    let mut tr: Vec<Vec<(u128, u32, u16, f64)>> = Vec::with_capacity(kk); let mut bp: Vec<Vec<(u32, u16)>> = Vec::with_capacity(kk); let mut mstates = 1;
    if gs.iter().any(|g| g.buckets.is_empty()) { return None; } // a component with no assignment: nothing feasible
    for j in 0..kk {
        if past(hard) { return None; }
        let g = &gs[sel[j]]; let mut t: Vec<(u128, u32, u16, f64)> = vec![];
        for (si, &(s, w)) in alpha[j].iter().enumerate() { for bi in 0..g.buckets.len() { if let Some(ns) = trans(j, s, bi) { t.push((ns, si as u32, bi as u16, w + g.buckets[bi].1)); } } }
        t.sort_by_key(|e| e.0);
        let mut v: Vec<(u128, f64)> = vec![]; let mut mm: Vec<(u128, f64)> = vec![]; let mut b: Vec<(u32, u16)> = vec![];
        let mut r0 = 0;
        while r0 < t.len() { let mut r1 = r0 + 1; while r1 < t.len() && t[r1].0 == t[r0].0 { r1 += 1; }
            let run = &t[r0..r1]; let mx = run.iter().map(|e| e.3).fold(f64::NEG_INFINITY, f64::max);
            v.push((t[r0].0, mx + run.iter().map(|e| (e.3 - mx).exp()).sum::<f64>().ln()));
            // max-product: the first best transition wins ties (as before)
            let (mut best, mut arg) = (f64::NEG_INFINITY, (t[r0].1, t[r0].2));
            for e in run { let lm = mxp[j][e.1 as usize].1 + bmax[sel[j]][e.2 as usize]; if lm > best { best = lm; arg = (e.1, e.2); } }
            mm.push((t[r0].0, best)); b.push(arg); r0 = r1; }
        if v.is_empty() || v.len() > max_states { return None; }
        mstates = mstates.max(v.len());
        alpha.push(v); mxp.push(mm); tr.push(t); bp.push(b);
    }
    let logz = alpha[kk][0].1; // the last frontier is empty: one final state
    // backward messages (log) and bucket posteriors -> marginals
    let k = m.k; let mut beta: Vec<f64> = vec![0.0]; let mut marg = vec![0.0; m.n * k];
    for j in (0..kk).rev() {
        if past(hard) { return None; }
        let g = &gs[sel[j]]; let t = &tr[j]; let nxt = &alpha[j + 1];
        let bn: Vec<f64> = t.iter().map(|e| beta[nxt.binary_search_by(|x| x.0.cmp(&e.0)).unwrap()]).collect();
        // bucket posteriors: joint (forward x bucket x backward) weights relative to the layer's largest (normalised by `tot`)
        let top = t.iter().zip(&bn).map(|(e, b)| e.3 + b).fold(f64::NEG_INFINITY, f64::max);
        let mut post = vec![0.0; g.buckets.len()]; let mut tot = 0.0;
        for (e, b) in t.iter().zip(&bn) { let w = (e.3 + b - top).exp(); post[e.2 as usize] += w; tot += w; }
        for (bi, &pw) in post.iter().enumerate() { if pw <= 0.0 { continue; } let pb = pw / tot; let asg = &g.buckets[bi].2; let lse = g.buckets[bi].1;
            for x in asg { let w = pb * (x.0 - lse).exp(); for (q, &i) in g.mem.iter().enumerate() { marg[i * k + x.1[q] as usize] += w; } } }
        // backward message of each previous state: a log-sum-exp over its own transitions (per state: none underflows)
        let mut nmx = vec![f64::NEG_INFINITY; alpha[j].len()];
        for (e, b) in t.iter().zip(&bn) { let x = g.buckets[e.2 as usize].1 + b; let s = &mut nmx[e.1 as usize]; if x > *s { *s = x; } }
        let mut acc = vec![0.0; alpha[j].len()];
        for (e, b) in t.iter().zip(&bn) { if b.is_finite() { acc[e.1 as usize] += (g.buckets[e.2 as usize].1 + b - nmx[e.1 as usize]).exp(); } }
        beta = nmx.iter().zip(&acc).map(|(&mx, &a)| if a > 0.0 { mx + a.ln() } else { f64::NEG_INFINITY }).collect();
    }
    // MAP: back-pointers from the single final key (the last frontier is empty), then the best assignment in each bucket
    let mut map = vec![0usize; m.n]; let mut idx = 0usize;
    for j in (0..kk).rev() { let (si, bi) = bp[j][idx]; let g = &gs[sel[j]];
        let x = g.buckets[bi as usize].2.iter().max_by(|a, b| a.0.partial_cmp(&b.0).unwrap()).unwrap();
        for (q, &i) in g.mem.iter().enumerate() { map[i] = x.1[q] as usize; } idx = si as usize; }
    let map_logw = m.logw(&map);
    Some(FrontierExact { marg, map, map_logw, logz, max_states: mstates })
}

// ---------------- compile pass (R19 P1.3 a): factors that change no odds are removed before tier selection ----------------
/// What `compile_parts` removed or rewrote.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Compiled { pub pairs_dropped: usize, pub caps_dropped: usize, pub tables_folded: usize }
/// The inference compiler's elimination pass, on a program's parts (before `Model::new`): (1) a pair whose every entry is
/// exactly 0 (`Potts(0)`, an all-zero table) adds nothing to any state's log-weight and is dropped; kept, it links its ends and
/// hides independence from the exact tiers (review case E17: 496 `potts: 0` pairs on 32 independent variables turned a 0.05 ms
/// frontier answer into 116 ms of sampling). (2) A table that is a sum of one term per end, t[a][b] = u[a] + w[b] EXACTLY in
/// f64 with u[a] = t[a][0] and w[b] = t[0][b] - t[0][0] (constant tables included), is folded into the two variables'
/// unaries: every state's log-weight, hence log Z, odds and plans, is unchanged up to float summation order, and the pair no
/// longer links its ends. (3) A cap whose limit reaches its number of distinct member variables can never bind (each variable
/// holds one value) and is dropped. Potts(w != 0) is never separable (k >= 2), so plain Potts programs are untouched.
pub fn compile_parts(k: usize, h: &mut [f64], pairs: Vec<Pair>, caps: Vec<Cap>) -> (Vec<Pair>, Vec<Cap>, Compiled) {
    let mut c = Compiled::default(); let mut kept = Vec::with_capacity(pairs.len());
    for p in pairs { match &p.c {
        Coupling::Potts(w) if *w == 0.0 => c.pairs_dropped += 1,
        Coupling::Table(t) if t.iter().all(|&x| x == 0.0) => c.pairs_dropped += 1,
        Coupling::Table(t) if t.len() == k * k && (0..k).all(|a| (0..k).all(|b| t[a * k + b] == t[a * k] + (t[b] - t[0]))) => {
            for a in 0..k { h[p.i * k + a] += t[a * k]; h[p.j * k + a] += t[a] - t[0]; } c.tables_folded += 1; }
        _ => kept.push(p) } }
    let nc = caps.len();
    // never binding: the most one plan can load the cap (per variable its heaviest member; unit = its distinct member variables) fits
    let caps: Vec<Cap> = caps.into_iter().filter(|cp| { let mut v: Vec<(usize, usize)> = cp.members.iter().enumerate().map(|(t, &(i, _))| (i, cp.w(t))).collect(); v.sort_unstable();
        let most: usize = v.iter().enumerate().filter(|&(q, &(i, _))| v.get(q + 1).map_or(true, |&(j, _)| j != i)).map(|(_, &(_, w))| w).sum(); cp.limit < most }).collect();
    c.caps_dropped = nc - caps.len();
    (kept, caps, c)
}
impl Model {
    /// This program after `compile_parts` (same variables, values, hard rules, clamps and sampler flags; identical odds and
    /// log Z up to float summation order), with what the pass removed.
    pub fn compiled(&self) -> (Model, Compiled) {
        let mut h = self.h.clone(); let (pairs, caps, c) = compile_parts(self.k, &mut h, self.pairs.clone(), self.caps.clone());
        let mut m = Model::new(self.n, self.k, h, self.allowed.clone(), self.clamp.clone(), pairs, caps).expect("the compile pass keeps a valid model");
        m.collective = self.collective; m.cluster = self.cluster; m.cycles = self.cycles; (m, c)
    }
}

// ---------------- exact: connected components + forest sum-/max-product (R19 P1.3 b, c) ----------------
/// Result of `exact_components`: exact marginals, an exact MAP plan and its log w, log Z (all as `FrontierExact`), and how
/// the components were solved. `infeasible` = some component has no feasible assignment (then the rest is not filled in).
pub struct CompExact { pub marg: Vec<f64>, pub map: Vec<usize>, pub map_logw: f64, pub logz: f64,
    /// components, and how many were solved by forest sum-/max-product, enumeration, and the frontier DP
    pub components: usize, pub forest: usize, pub enumerated: usize, pub frontier: usize,
    /// components solved by the occupancy-count DP (`count_dp`)
    pub occupancy: usize, pub infeasible: bool }
/// Largest component the occupancy-count DP takes (its forward table is O(n^2) floats: 2048 -> 16.8 MB).
pub const COUNT_MAX_VARS: usize = 2048;
/// Work bound of the forest tier: sum over edges of |values(i)| x |values(j)| (and n x k for the message tables).
pub const FOREST_MAX_WORK: f64 = 5e7;
fn lse(xs: impl Iterator<Item = f64>) -> f64 { let v: Vec<f64> = xs.collect(); let mx = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if mx == f64::NEG_INFINITY { return mx; } mx + v.iter().map(|x| (x - mx).exp()).sum::<f64>().ln() }
/// EXACT marginals, log Z and an exact MAP plan by decomposition (R19 P1.3, the inference compiler's structural tiers). The
/// variables are split into connected components of pairs AND capacity constraints (two variables are linked if a pair or a
/// cap mentions both); log Z is the sum of the components' log Z, each variable's odds and MAP value come from its own
/// component. A cap-free component whose pairs form a tree (pairs = variables - 1) is solved by sum-product (odds, log Z) and
/// max-product (MAP) in log space: O(edges x values^2), e.g. a 1,000-variable chain. A `CountSpec` component by the
/// occupancy-count DP. Any other component by enumeration if its raw space fits what is left of ONE shared budget of `limit`
/// (summed over the enumerated components), else by the frontier DP (partition components, `max_states` > 0). None (decline) if one component cannot be
/// solved, the program is a single non-tree component (the whole-program tiers already tried it), the forest work exceeds
/// `FOREST_MAX_WORK`, or `hard` passes (clock read per component).
pub fn exact_components_until(m: &Model, limit: u64, max_states: usize, hard: Option<std::time::Instant>) -> Option<CompExact> {
    if past(hard) { return None; }
    deep(m.n, || components(m, limit, max_states, hard))
}
fn components(m: &Model, limit: u64, max_states: usize, hard: Option<std::time::Instant>) -> Option<CompExact> {
    let (n, k) = (m.n, m.k); let mut par: Vec<usize> = (0..n).collect();
    fn find(par: &mut [usize], mut x: usize) -> usize { while par[x] != x { par[x] = par[par[x]]; x = par[x]; } x }
    fn join(par: &mut [usize], a: usize, b: usize) { let (a, b) = (find(par, a), find(par, b)); if a != b { par[a.max(b)] = a.min(b); } }
    for p in &m.pairs { join(&mut par, p.i, p.j); }
    for c in &m.caps { if let Some(&(i0, _)) = c.members.first() { for &(i, _) in &c.members { join(&mut par, i0, i); } } }
    let mut slot = vec![usize::MAX; n]; let mut mems: Vec<Vec<usize>> = vec![];
    for i in 0..n { let r = find(&mut par, i); if slot[r] == usize::MAX { slot[r] = mems.len(); mems.push(vec![]); } mems[slot[r]].push(i); }
    let comp_of: Vec<usize> = (0..n).map(|i| slot[find(&mut par, i)]).collect();
    let mut npairs = vec![0usize; mems.len()]; for p in &m.pairs { npairs[comp_of[p.i]] += 1; }
    let mut ccaps: Vec<Vec<usize>> = vec![vec![]; mems.len()]; for (c, cp) in m.caps.iter().enumerate() { if let Some(&(i0, _)) = cp.members.first() { ccaps[comp_of[i0]].push(c); } }
    let tree = |c: usize| ccaps[c].is_empty() && npairs[c] + 1 == mems[c].len();
    let mut cpairs: Vec<Vec<usize>> = vec![vec![]; mems.len()]; for (q, p) in m.pairs.iter().enumerate() { cpairs[comp_of[p.i]].push(q); }
    let counted: Vec<Option<CountSpec>> = (0..mems.len()).map(|c| if tree(c) { None } else { count_spec(m, &mems[c], &ccaps[c], &cpairs[c]) }).collect();
    if mems.len() == 1 && !tree(0) && counted[0].is_none() { return None; }
    let vals = |i: usize| -> Vec<usize> { m.cand[i].iter().copied().filter(|&v| m.ok(i, v)).collect() };
    let fwork: f64 = m.pairs.iter().filter(|p| tree(comp_of[p.i])).map(|p| (vals(p.i).len() * vals(p.j).len()) as f64).sum::<f64>()
        + (0..n).filter(|&i| tree(comp_of[i])).count() as f64 * k as f64;
    if fwork > FOREST_MAX_WORK { return None; }
    let mut out = CompExact { marg: vec![0.0; n * k], map: vec![0; n], map_logw: 0.0, logz: 0.0, components: mems.len(), forest: 0, enumerated: 0, frontier: 0, occupancy: 0, infeasible: false };
    let mut enum_left = limit as f64;
    for (c, mem) in mems.iter().enumerate() {
        if past(hard) { return None; }
        if tree(c) { out.forest += 1;
            let z = tree_bp(m, mem, &vals, &mut out.marg, &mut out.map); if z == f64::NEG_INFINITY { out.infeasible = true; return Some(out); }
            out.logz += z; continue; }
        if let Some(sp) = &counted[c] { out.occupancy += 1;
            let z = count_dp(m, mem, sp, &mut out.marg, &mut out.map); if z == f64::NEG_INFINITY { out.infeasible = true; return Some(out); }
            out.logz += z; continue; }
        // a component with caps or cycles: a sub-model of its own variables, pairs and caps, solved by the whole-program tiers
        let mut loc = vec![usize::MAX; n]; for (q, &i) in mem.iter().enumerate() { loc[i] = q; }
        let nn = mem.len(); let mut h = Vec::with_capacity(nn * k); let mut al = Vec::with_capacity(nn * k);
        for &i in mem { h.extend_from_slice(&m.h[i * k..i * k + k]); al.extend_from_slice(&m.allowed[i * k..i * k + k]); }
        let pairs: Vec<Pair> = m.pairs.iter().filter(|p| comp_of[p.i] == c).map(|p| Pair { i: loc[p.i], j: loc[p.j], c: p.c.clone() }).collect();
        let caps: Vec<Cap> = ccaps[c].iter().map(|&q| Cap { members: m.caps[q].members.iter().map(|&(i, v)| (loc[i], v)).collect(), limit: m.caps[q].limit, weights: m.caps[q].weights.clone() }).collect();
        let sub = Model::new(nn, k, h, al, mem.iter().map(|&i| m.clamp[i]).collect(), pairs, caps).ok()?;
        // Enumeration only where the component's RAW space fits what is left of one shared budget of `limit` (so the tier
        // never spends more than one whole-program enumeration: per-component node budgets on many hard components would
        // add up); larger components go to the frontier DP (bounded by `max_states`) or the tier declines.
        let space: f64 = (0..nn).map(|i| sub.cand_count(i) as f64).product();
        let mut got: Option<(Vec<f64>, Vec<usize>, f64)> = None;
        if space <= enum_left { enum_left -= space;
            if let Some(e) = exact_within(&sub, 1, limit, None, hard).0 {
                if e.n_feasible == 0 { out.infeasible = true; return Some(out); } out.enumerated += 1; got = Some((e.marg, e.top[0].1.clone(), e.logz)); } }
        if got.is_none() && max_states > 0 { got = exact_frontier_until(&sub, max_states, hard).map(|f| (f.marg, f.map, f.logz)); if got.is_some() { out.frontier += 1; } }
        let (mg, mp, z) = got?;
        for (q, &i) in mem.iter().enumerate() { out.map[i] = mp[q]; out.marg[i * k..i * k + k].copy_from_slice(&mg[q * k..q * k + k]); }
        out.logz += z;
    }
    out.map_logw = m.logw(&out.map);
    Some(out)
}
/// An occupancy-count component (R19 P1.3(d)): its members can take only the two values `a` < `b`, every pair of members is
/// coupled once by the same symmetric term (`same_a` when both take a, `same_b` both b, `diff` otherwise; no pairs = all 0),
/// and every cap counts ALL members that can take its value: log w then depends on the members' unaries and the count of b
/// only. `cap_a` / `cap_b`: the tightest limit on the count of a / b (usize::MAX = none).
struct CountSpec { a: usize, b: usize, same_a: f64, same_b: f64, diff: f64, cap_a: usize, cap_b: usize }
fn count_spec(m: &Model, mem: &[usize], caps: &[usize], pairs: &[usize]) -> Option<CountSpec> {
    let (k, nn) = (m.k, mem.len()); if nn < 2 || nn > COUNT_MAX_VARS { return None; }
    let mut vs: Vec<usize> = mem.iter().flat_map(|&i| m.cand[i].iter().copied().filter(move |&v| m.ok(i, v))).collect(); vs.sort_unstable(); vs.dedup();
    if vs.len() != 2 { return None; } let (a, b) = (vs[0], vs[1]);
    let (mut same_a, mut same_b, mut diff) = (0.0, 0.0, 0.0);
    if !pairs.is_empty() {
        if pairs.len() != nn * (nn - 1) / 2 { return None; }
        let mut seen: Vec<(usize, usize)> = pairs.iter().map(|&q| (m.pairs[q].i.min(m.pairs[q].j), m.pairs[q].i.max(m.pairs[q].j))).collect(); seen.sort_unstable(); seen.dedup();
        if seen.len() != pairs.len() { return None; } // a repeated pair: not one term per pair
        let term = |q: usize| -> Option<(f64, f64, f64)> { match &m.pairs[q].c { Coupling::Potts(w) => Some((*w, *w, 0.0)),
            Coupling::Table(t) => if t[a * k + b] == t[b * k + a] { Some((t[a * k + a], t[b * k + b], t[a * k + b])) } else { None } } };
        let t0 = term(pairs[0])?; if pairs.iter().any(|&q| term(q) != Some(t0)) { return None; } (same_a, same_b, diff) = t0;
    }
    let (mut cap_a, mut cap_b) = (usize::MAX, usize::MAX);
    if caps.iter().any(|&c| !m.caps[c].weights.is_empty()) { return None; } // R19.7: a weighted load is not a count
    for &c in caps { let act: Vec<(usize, usize)> = m.caps[c].members.iter().copied().filter(|&(i, v)| m.ok(i, v)).collect();
        let Some(&(_, v)) = act.first() else { continue }; if act.iter().any(|&(_, w)| w != v) { return None; }
        let mut on: Vec<usize> = act.iter().map(|&(i, _)| i).collect(); on.sort_unstable(); on.dedup();
        if on.len() != mem.iter().filter(|&&i| m.ok(i, v)).count() { return None; } // the cap must count every member that can take v
        if v == a { cap_a = cap_a.min(m.caps[c].limit); } else { cap_b = cap_b.min(m.caps[c].limit); } }
    Some(CountSpec { a, b, same_a, same_b, diff, cap_a, cap_b })
}
/// Occupancy-count DP on one `CountSpec` component, in log space: f[q][c] = log-sum over the first q members' assignments with
/// c of them on b (O(n^2)); g(c) = the pair terms at count c (-inf past a cap); log Z = lse_c f[n][c] + g(c). Odds of b by the
/// backward recursion (as in the external review's count DP), clamped to [0, 1] (a sum of rounded exps can exceed 1 by ~5e-11 at
/// n = 512), exactly 0 / 1 for members that can take only one value; MAP by max-product over the same table with choice bits.
/// Writes the members' odds and MAP values, returns log Z (-inf: no count satisfies the caps).
fn count_dp(m: &Model, mem: &[usize], sp: &CountSpec, marg: &mut [f64], map: &mut [usize]) -> f64 {
    let (k, nn, ni) = (m.k, mem.len(), f64::NEG_INFINITY);
    let ph = |i: usize, v: usize| if m.ok(i, v) { m.h[i * k + v] } else { ni };
    let la = |x: f64, y: f64| if x == ni { y } else if y == ni { x } else { let mx = x.max(y); mx + ((x - mx).exp() + (y - mx).exp()).ln() };
    let c2 = |x: usize| (x * x.saturating_sub(1) / 2) as f64;
    let g: Vec<f64> = (0..=nn).map(|c| if c > sp.cap_b || nn - c > sp.cap_a { ni } else { c2(nn - c) * sp.same_a + c2(c) * sp.same_b + ((nn - c) * c) as f64 * sp.diff }).collect();
    let mut f: Vec<Vec<f64>> = Vec::with_capacity(nn + 1); f.push(vec![0.0]);
    let mut mf: Vec<f64> = vec![0.0]; let mut pick: Vec<Vec<bool>> = Vec::with_capacity(nn);
    for (q, &i) in mem.iter().enumerate() { let (ha, hb) = (ph(i, sp.a), ph(i, sp.b));
        let mut nx = vec![ni; q + 2]; let mut nm = vec![ni; q + 2]; let mut pk = vec![false; q + 2];
        for c in 0..=q { nx[c] = la(nx[c], f[q][c] + ha); nx[c + 1] = la(nx[c + 1], f[q][c] + hb);
            let (sa, sb) = (mf[c] + ha, mf[c] + hb); if sa > nm[c] || (nm[c] == ni && sa != ni) { nm[c] = sa; pk[c] = false; } if sb > nm[c + 1] { nm[c + 1] = sb; pk[c + 1] = true; } }
        f.push(nx); mf = nm; pick.push(pk); }
    let logz = (0..=nn).map(|c| f[nn][c] + g[c]).fold(ni, la); if logz == ni { return ni; }
    let mut bk = g.clone();
    for q in (0..nn).rev() { let i = mem[q]; let (ha, hb) = (ph(i, sp.a), ph(i, sp.b));
        let pb = if ha == ni { 1.0 } else if hb == ni { 0.0 } else { ((0..=q).map(|c| f[q][c] + hb + bk[c + 1]).fold(ni, la) - logz).exp().clamp(0.0, 1.0) };
        marg[i * k + sp.a] = 1.0 - pb; marg[i * k + sp.b] = pb;
        bk = (0..=q).map(|c| la(ha + bk[c], hb + bk[c + 1])).collect(); }
    // MAP: best count (lowest on ties), then the choice bits back to the first member
    let (mut best, mut cb) = (ni, 0usize); for c in 0..=nn { let s = mf[c] + g[c]; if s > best { best = s; cb = c; } }
    for q in (0..nn).rev() { let on_b = pick[q][cb]; map[mem[q]] = if on_b { sp.b } else { sp.a }; if on_b { cb -= 1; } }
    logz
}
/// Sum-product (upward + downward: log Z and odds) and max-product with back-pointers (MAP) on one cap-free tree, rooted at
/// its smallest member, in log space; writes its members' odds and MAP values, returns its log Z (-inf if a member has no
/// allowed value). Messages are dense over `0..k` (-inf on values a variable cannot take); a parent's message to one child
/// sums its other children's messages with prefix / suffix sums (no subtraction of logs, so no cancellation).
fn tree_bp(m: &Model, mem: &[usize], vals: &dyn Fn(usize) -> Vec<usize>, marg: &mut [f64], map: &mut [usize]) -> f64 {
    let k = m.k; let ni = f64::NEG_INFINITY; let nn = mem.len();
    let mut loc: HashMap<usize, usize> = HashMap::with_capacity(nn); for (q, &i) in mem.iter().enumerate() { loc.insert(i, q); }
    // BFS from the root: order, parent (local index) and the pair joining each node to its parent
    let (mut ord, mut par, mut pq) = (vec![0usize], vec![usize::MAX; nn], vec![usize::MAX; nn]); let mut seen = vec![false; nn]; seen[0] = true;
    let mut h = 0; while h < ord.len() { let q = ord[h]; h += 1; let i = mem[q];
        for &(j, pi) in &m.adj[i] { let r = loc[&j]; if !seen[r] { seen[r] = true; par[r] = q; pq[r] = pi; ord.push(r); } } }
    let vl: Vec<Vec<usize>> = mem.iter().map(|&i| vals(i)).collect();
    let phi = |q: usize, v: usize| -> f64 { let i = mem[q]; if m.ok(i, v) { m.h[i * k + v] } else { ni } };
    // upward: up[q][x_parent] = log sum_{x_q} exp(phi_q + acc_q + psi), acc_q = sum of q's children's up messages
    let mut acc = vec![0.0; nn * k]; let mut up = vec![ni; nn * k];
    let (mut accm, mut upm, mut arg) = (vec![0.0; nn * k], vec![ni; nn * k], vec![0u32; nn * k]);
    for &q in ord.iter().skip(1).rev() { let (p, i) = (par[q], mem[q]);
        for &xp in &vl[p] {
            up[q * k + xp] = lse(vl[q].iter().map(|&x| phi(q, x) + acc[q * k + x] + m.coupling(pq[q], i, x, xp)));
            let (mut best, mut bx) = (ni, vl[q].first().copied().unwrap_or(0));
            for &x in &vl[q] { let s = phi(q, x) + accm[q * k + x] + m.coupling(pq[q], i, x, xp); if s > best { best = s; bx = x; } }
            upm[q * k + xp] = best; arg[q * k + xp] = bx as u32; }
        for v in 0..k { acc[p * k + v] += up[q * k + v]; accm[p * k + v] += upm[q * k + v]; }
    }
    let logz = lse(vl[0].iter().map(|&x| phi(0, x) + acc[x]));
    if logz == ni { return logz; }
    // downward: dn[q][x_q] = log sum_{x_p} exp(phi_p + dn_p + (sum of p's OTHER children's up) + psi)
    let mut ch: Vec<Vec<usize>> = vec![vec![]; nn]; for &q in ord.iter().skip(1) { ch[par[q]].push(q); }
    let mut dn = vec![0.0; nn * k];
    for &p in &ord { let cs = &ch[p]; if cs.is_empty() { continue; } let ip = mem[p];
        let mut pre = vec![0.0; (cs.len() + 1) * k]; for (j, &c) in cs.iter().enumerate() { for v in 0..k { pre[(j + 1) * k + v] = pre[j * k + v] + up[c * k + v]; } }
        let mut suf = vec![0.0; (cs.len() + 1) * k]; for (j, &c) in cs.iter().enumerate().rev() { for v in 0..k { suf[j * k + v] = suf[(j + 1) * k + v] + up[c * k + v]; } }
        for (j, &c) in cs.iter().enumerate() {
            for &x in &vl[c] { dn[c * k + x] = lse(vl[p].iter().map(|&xp| phi(p, xp) + dn[p * k + xp] + pre[j * k + xp] + suf[(j + 1) * k + xp] + m.coupling(pq[c], ip, xp, x))); } }
    }
    for q in 0..nn { let i = mem[q]; let b: Vec<f64> = (0..k).map(|v| phi(q, v) + dn[q * k + v] + acc[q * k + v]).collect();
        let z = lse(b.iter().cloned()); for v in 0..k { marg[i * k + v] = if b[v] == ni { 0.0 } else { (b[v] - z).exp() }; } }
    // max-product MAP: the root's best value, then each child's back-pointer from its parent's value (lowest index on ties)
    let (mut best, mut bx) = (ni, vl[0][0]); for &x in &vl[0] { let s = phi(0, x) + accm[x]; if s > best { best = s; bx = x; } }
    let mut xs = vec![0usize; nn]; xs[0] = bx; for &q in ord.iter().skip(1) { xs[q] = arg[q * k + xs[par[q]]] as usize; }
    for q in 0..nn { map[mem[q]] = xs[q]; }
    logz
}

// ---------------- sample: constraint-preserving Gibbs ----------------
pub struct Chain<'a> { pub m: &'a Model, pub x: Vec<usize>, load: Vec<usize>, rng: Philox4x32, w: Vec<f64>, dl: Vec<(usize, i64)>, flip: Option<Box<FlipPlan>>,
    /// relabel scratch: per variable a changed flag, the changed variables (ascending), per capacity constraint the load
    /// change and a touched flag, the constraints touched
    rl: (Vec<bool>, Vec<usize>, Vec<i64>, Vec<bool>, Vec<usize>),
    /// R19 P1.2: accepted collective moves on this chain, [global two-value flips, label swaps]: jumps between mirror /
    /// label-permuted modes (`Samples::moves`, gate output `mode_transitions`). Counting consumes no draws.
    pub moves: [u64; 2],
    /// R19.7: inverse temperature applied to every log-weight difference (heat-bath exponent, MH ratios, cluster bond
    /// probabilities). 1.0 (every sampling chain) multiplies by exactly 1: unchanged results. `anneal_on` sets it per stage
    /// instead of building a scaled Model clone per beta (5 full clones outside the polish clock: 526 ms at a 50 ms budget and
    /// +1 GB at n = 200, k = 65,535, R19.6 c10).
    pub beta: f64,
    /// R19.7: sweeps run site and swap moves only (the polish: its per-beta clones came from `Model::new`, which leaves
    /// `collective` / `cluster` / `cycles` off, so it has always annealed with site + swap moves; kept).
    pub plain: bool }
/// The global two-value flip's precomputed structure (built on a chain's first flip; the model cannot change while borrowed).
/// The log-weight change of the flip touches only these terms, so a flip costs O(flipped + listed pairs + cap memberships)
/// instead of two full `logw` evaluations and a `violations` scan (R19.1: +20.6% / +77.7% sampling time at fixed work).
struct FlipPlan {
    /// (variable, first candidate, second candidate, h[second] - h[first]) per free two-candidate variable, ascending
    vars: Vec<(usize, usize, usize, f64)>,
    /// per variable: its index in `vars` (usize::MAX = not flipped)
    pos: Vec<usize>,
    /// pairs with exactly one flipped endpoint: (pair, flipped endpoint, other endpoint), by flipped endpoint then other
    one: Vec<(usize, usize, usize)>,
    /// pairs between two flipped variables whose term can change: (vars index of i, of j, change by [s_i * 2 + s_j]), s = 0 at
    /// the first candidate. Potts pairs over the same two candidates (ferromagnets, one-group routers) never change: not listed.
    both: Vec<(usize, usize, [f64; 4])>,
    /// some flipped (variable, value) is in a capacity constraint
    capped: bool,
    /// scratch: per capacity constraint the proposal's load change and a touched flag, and the constraints touched
    dcap: Vec<i64>, seen: Vec<bool>, touched: Vec<usize>,
}
impl<'a> Chain<'a> {
    pub fn new(m: &'a Model, seed: u64, stream: u64) -> Option<Self> { Self::new_until(m, seed, stream, None) }
    /// `new` whose feasible-start search gives up at `deadline` (a wall-clock sampling budget's end).
    pub fn new_until(m: &'a Model, seed: u64, stream: u64, deadline: Option<std::time::Instant>) -> Option<Self> {
        let mut rng = Philox4x32::new(seed, stream);
        let x = match &m.start { Some(s) if stream == 0 && s.len() == m.n && m.violations(s) == 0 => s.clone(), _ => m.feasible_init_until(&mut rng, START_WORK, deadline)? }; // an infeasible start is ignored
        let mut load = vec![0; m.caps.len()]; for (i, &v) in x.iter().enumerate() { m.add_load(&mut load, i * m.k + v); }
        let mut c = Chain { m, x, load, rng, w: vec![f64::NEG_INFINITY; m.k], dl: vec![], flip: None, rl: (vec![false; m.n], vec![], vec![0; m.caps.len()], vec![false; m.caps.len()], vec![]), moves: [0; 2], beta: 1.0, plain: false };
        for _ in 0..5 { c.sweep_uniform(); } // over-dispersion: uniform over feasible values (h ignored)
        Some(c)
    }
    /// moving variable i from a0 to v keeps every capacity constraint
    #[inline] fn cap_ok(&self, i: usize, a0: usize, v: usize) -> bool {
        let m = self.m; let k = m.k;
        if m.partition { let b = m.bucket[i * k + v]; return b == NONE || b == m.bucket[i * k + a0] || self.load[b as usize] < m.limit[b as usize]; }
        let old = &m.cap_of[i * k + a0];
        if m.capw.is_empty() { return m.cap_of[i * k + v].iter().all(|c| old.contains(c) || self.load[*c] < m.limit[*c]); }
        // R19.7 weighted: the load after the move (the old membership's weight leaves, the new one's arrives)
        m.cap_of[i * k + v].iter().zip(&m.capw[i * k + v]).all(|(c, &w)| { let out = old.iter().position(|o| o == c).map_or(0, |t| m.capw[i * k + a0][t]); self.load[*c] - out + w <= m.limit[*c] })
    }
    #[inline] fn set(&mut self, i: usize, v: usize) {
        let m = self.m; let k = m.k;
        if m.partition { let (b0, b1) = (m.bucket[i * k + self.x[i]], m.bucket[i * k + v]);
            if b0 != NONE { self.load[b0 as usize] -= 1; } if b1 != NONE { self.load[b1 as usize] += 1; } self.x[i] = v; return; }
        m.sub_load(&mut self.load, i * k + self.x[i]); m.add_load(&mut self.load, i * k + v); self.x[i] = v;
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
        let (mut tot, beta) = (0.0, self.beta); for &v in cand { let e = if self.w[v] == f64::NEG_INFINITY { 0.0 } else { ((self.w[v] - mx) * beta).exp() }; self.w[v] = e; tot += e; }
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
            for (idx, s) in [(i * k + ai, -1i64), (i * k + aj, 1), (j * k + aj, -1), (j * k + ai, 1)] { for (t, &c) in m.cap_of[idx].iter().enumerate() { let s = s * m.cw(idx, t) as i64;
                match self.dl.iter_mut().find(|e| e.0 == c) { Some(e) => e.1 += s, None => self.dl.push((c, s)) } } }
            if self.dl.iter().any(|&(c, d)| d > 0 && self.load[c] as i64 + d > m.limit[c] as i64) { self.dl.clear(); return; }
        }
        let mut e0 = self.pair_e(i, ai, j) + self.pair_e(j, aj, i);
        let mut e1 = self.pair_e(i, aj, j) + self.pair_e(j, ai, i);
        // Potts terms between i and j vanish before and after (ai != aj); tables do not
        for &(o, q) in &m.tadj[i] { if o == j { e0 += m.coupling(q, i, ai, aj); e1 += m.coupling(q, i, aj, ai); } }
        if e1 >= e0 || self.rng.f64() < ((e1 - e0) * self.beta).exp() {
            if !self.dl.is_empty() { for &(c, d) in &self.dl { self.load[c] = (self.load[c] as i64 + d) as usize; } self.dl.clear(); }
            self.x[i] = aj; self.x[j] = ai;
        } else { self.dl.clear(); }
    }
    /// one sweep = n site updates + max(n/2, 2) swap attempts
    pub fn sweep(&mut self) { for i in 0..self.m.n { self.site(i); } for _ in 0..(self.m.n / 2).max(2) { self.swap(); } if self.plain { return; } if self.m.collective { self.global_flip(); if self.m.k > 2 { self.relabel(); } } if self.m.cluster && self.rng.f64() < 0.5 { self.cluster_move(); } if self.m.cycles && self.m.k > 2 && !self.m.caps.is_empty() { for _ in 0..(self.m.n / 4).max(1) { self.cycle3(); } } }
    /// R19 P1.1(a) (`Model::collective`): the global two-value flip. Every free variable with exactly two candidate values
    /// switches to the other one at once: an involution, so the proposal is symmetric, accepted with min(1, exp(dlogw)) when
    /// every cap still holds (detailed balance). It crosses the barrier between mirror-image modes (complete-graph
    /// ferromagnets, one-group two-worker routers) that site and swap moves do not cross in practice. dlogw is incremental
    /// (`FlipPlan`): unary changes, then pairs with one flipped endpoint, then pairs of flipped endpoints whose term changes,
    /// in that order; pbit-decide's router chain sums in the same order, so the two samplers stay bit-identical.
    pub fn global_flip(&mut self) {
        if self.flip.is_none() { self.flip = Some(Box::new(self.flip_plan())); }
        let mut f = self.flip.take().unwrap();
        if !f.vars.is_empty() {
            if let Some(d) = self.flip_delta(&mut f) { if d >= 0.0 || self.rng.f64() < (d * self.beta).exp() {
                for &(i, c0, c1, _) in &f.vars { self.x[i] ^= c0 ^ c1; } // x_i is c0 or c1
                self.moves[0] += 1;
                for &c in &f.touched { self.load[c] = (self.load[c] as i64 + f.dcap[c]) as usize; } } }
            for &c in &f.touched { f.dcap[c] = 0; f.seen[c] = false; } f.touched.clear();
        }
        self.flip = Some(f);
    }
    /// The flip's log-weight change, or None when it would overflow a capacity constraint. Leaves the per-constraint load
    /// changes in `f.dcap` / `f.touched` (the caller applies and clears them).
    fn flip_delta(&self, f: &mut FlipPlan) -> Option<f64> {
        let m = self.m; let k = m.k;
        // capacity: the load change of the whole proposal on every constraint it touches (the current state is feasible)
        if f.capped { for &(i, c0, c1, _) in &f.vars { let (a, b) = if self.x[i] == c0 { (c0, c1) } else { (c1, c0) };
            for (v, s) in [(a, -1i64), (b, 1)] { for (t, &c) in m.cap_of[i * k + v].iter().enumerate() { if !f.seen[c] { f.seen[c] = true; f.touched.push(c); } f.dcap[c] += s * m.cw(i * k + v, t) as i64; } } } }
        if !f.touched.iter().all(|&c| self.load[c] as i64 + f.dcap[c] <= m.limit[c] as i64) { return None; }
        let mut d = 0.0;
        for &(i, c0, _, dh) in &f.vars { d += if self.x[i] == c0 { dh } else { -dh }; }
        for &(q, i, j) in &f.one { let (xi, xj) = (self.x[i], self.x[j]); let v = &f.vars[f.pos[i]]; let yi = xi ^ v.1 ^ v.2;
            match &m.pairs[q].c { Coupling::Potts(w) => { if xj == yi { d += w; } else if xj == xi { d -= w; } }
                Coupling::Table(_) => d += m.coupling(q, i, yi, xj) - m.coupling(q, i, xi, xj) } }
        for &(a, b, t) in &f.both { let (va, vb) = (&f.vars[a], &f.vars[b]); d += t[(self.x[va.0] != va.1) as usize * 2 + (self.x[vb.0] != vb.1) as usize]; }
        Some(d)
    }
    /// R19 P1.1(b) (`Model::collective`, k > 2): swap two value labels everywhere at once. Values a != b are drawn uniformly;
    /// every free variable that allows both and holds one of them takes the other (a state-independent set, so the map is
    /// an involution and the proposal symmetric); MH with min(1, exp(dlogw)) when every cap holds. It crosses between the
    /// label-permuted modes of colourings / Potts models and routers with interchangeable workers, which site, swap and
    /// two-value flip moves do not. For k = 2 it is the global flip (skipped). The router chain mirrors it bit for bit.
    pub fn relabel(&mut self) {
        let m = self.m; let k = m.k; let a = self.rng.below(k); let mut b = self.rng.below(k - 1); if b >= a { b += 1; }
        let (mut on, mut ch, mut dc, mut seen, mut tc) = std::mem::take(&mut self.rl);
        for i in 0..m.n { let v = self.x[i]; if (v == a || v == b) && m.clamp[i].is_none() && m.allowed[i * k + a] && m.allowed[i * k + b] { on[i] = true; ch.push(i); } }
        if !ch.is_empty() {
            for &i in &ch { let (x, y) = (self.x[i], self.x[i] ^ a ^ b);
                for (v, s) in [(x, -1i64), (y, 1)] { for (t, &c) in m.cap_of[i * k + v].iter().enumerate() { if !seen[c] { seen[c] = true; tc.push(c); } dc[c] += s * m.cw(i * k + v, t) as i64; } } }
            if tc.iter().all(|&c| self.load[c] as i64 + dc[c] <= m.limit[c] as i64) {
                let mut d = 0.0;
                for &i in &ch { let (x, y) = (self.x[i], self.x[i] ^ a ^ b); d += m.h[i * k + y] - m.h[i * k + x]; }
                // Potts neighbours (sorted by j, as the router's mates), then table neighbours; a Potts pair that both change keeps its equality
                for &i in &ch { let (xi, yi) = (self.x[i], self.x[i] ^ a ^ b);
                    for &(j, w) in &m.padj[i] { if !on[j] { let xj = self.x[j]; if xj == yi { d += w; } else if xj == xi { d -= w; } } }
                    for &(j, q) in &m.tadj[i] { let xj = self.x[j];
                        if !on[j] { d += m.coupling(q, i, yi, xj) - m.coupling(q, i, xi, xj); } else if j > i { d += m.coupling(q, i, yi, xj ^ a ^ b) - m.coupling(q, i, xi, xj); } } }
                if d >= 0.0 || self.rng.f64() < (d * self.beta).exp() {
                    for &i in &ch { self.x[i] ^= a ^ b; } self.moves[1] += 1;
                    for &c in &tc { self.load[c] = (self.load[c] as i64 + dc[c]) as usize; } }
            }
            for &c in &tc { dc[c] = 0; seen[c] = false; } tc.clear(); for &i in &ch { on[i] = false; } ch.clear();
        }
        self.rl = (on, ch, dc, seen, tc);
    }
    /// R19 P1.1(d), WIP (`Model::cluster`): a Wolff cluster move. A seed variable s (uniform) and a new value v != x_s (uniform)
    /// are drawn; the cluster grows from s over Potts bonds with w > 0 to neighbours holding x_s that are free and allow v, each
    /// bond activated with probability 1 - exp(-w); the whole cluster takes v. The bond terms on the cluster boundary cancel
    /// against the proposal ratio (Fortuin-Kasteleyn), so the Metropolis accept uses only the unary change, table and
    /// negative-Potts terms, and positive bonds to neighbours that could not join (clamped / value not allowed) in either
    /// direction; every cap is checked. Mixed-sign tables get no cluster acceleration (they enter the accept as plain terms).
    pub fn cluster_move(&mut self) {
        let m = self.m; let k = m.k; if k < 2 { return; } let beta = self.beta;
        let s = self.rng.below(m.n); let old = self.x[s]; let mut v = self.rng.below(k - 1); if v >= old { v += 1; }
        let free = |i: usize, val: usize| m.clamp[i].is_none() && m.allowed[i * k + val];
        if !free(s, v) { return; }
        let (mut inc, mut cl, mut dc, mut seen, mut tc) = std::mem::take(&mut self.rl);
        inc[s] = true; cl.push(s); let mut head = 0;
        while head < cl.len() { let i = cl[head]; head += 1;
            for &(j, w) in &m.padj[i] { if w > 0.0 && !inc[j] && self.x[j] == old && free(j, v) && self.rng.f64() < -(-w * beta).exp_m1() { inc[j] = true; cl.push(j); } } }
        for &i in &cl { for (val, sg) in [(old, -1i64), (v, 1)] { for (t, &c) in m.cap_of[i * k + val].iter().enumerate() { if !seen[c] { seen[c] = true; tc.push(c); } dc[c] += sg * m.cw(i * k + val, t) as i64; } } }
        if tc.iter().all(|&c| self.load[c] as i64 + dc[c] <= m.limit[c] as i64) {
            let mut d = 0.0;
            for &i in &cl { d += m.h[i * k + v] - m.h[i * k + old];
                for &(j, w) in &m.padj[i] { if inc[j] { continue; } let xj = self.x[j]; // internal Potts pairs keep their equality
                    let cancelled = w > 0.0 && ((xj == old && free(j, v)) || (xj == v && free(j, old)));
                    if !cancelled { if xj == v { d += w; } else if xj == old { d -= w; } } }
                for &(j, q) in &m.tadj[i] { let xj = self.x[j];
                    if !inc[j] { d += m.coupling(q, i, v, xj) - m.coupling(q, i, old, xj); } else if j > i { d += m.coupling(q, i, v, v) - m.coupling(q, i, old, old); } } }
            if d >= 0.0 || self.rng.f64() < (d * beta).exp() {
                for &i in &cl { self.x[i] = v; } for &c in &tc { self.load[c] = (self.load[c] as i64 + dc[c]) as usize; } }
        }
        for &c in &tc { dc[c] = 0; seen[c] = false; } tc.clear(); for &i in &cl { inc[i] = false; } cl.clear();
        self.rl = (inc, cl, dc, seen, tc);
    }
    /// R19 P1.1(c), WIP (`Model::cycles`): a three-cycle rotation. An ordered triple (i, j, l) of distinct variables holding
    /// three distinct values is drawn uniformly; i takes x_j, j takes x_l, l takes x_i. Every value keeps its count, so
    /// per-value quotas hold (member-specific caps are checked); the reverse rotation is the triple (i, l, j), equally likely,
    /// so the proposal is symmetric; MH with min(1, exp(dlogw)). It connects capacity-saturated states that no pairwise swap
    /// connects (e.g. allowed sets {a,c}, {b,a}, {c,b} under one-per-value quotas). `sweep` attempts it only on programs
    /// with k > 2 values AND at least one cap (R19.5): with two values no triple holds three distinct values, and without caps
    /// site moves already connect every state, so there it only spent random draws (ferro12 at defaults: -18.2% sweeps).
    pub fn cycle3(&mut self) {
        let m = self.m; let k = m.k; if m.n < 3 { return; }
        let (i, j, l) = (self.rng.below(m.n), self.rng.below(m.n), self.rng.below(m.n));
        if i == j || j == l || i == l { return; }
        let (a, b, c) = (self.x[i], self.x[j], self.x[l]);
        if a == b || b == c || a == c || !m.ok(i, b) || !m.ok(j, c) || !m.ok(l, a) { return; }
        self.dl.clear();
        for (idx, s) in [(i * k + a, -1i64), (i * k + b, 1), (j * k + b, -1), (j * k + c, 1), (l * k + c, -1), (l * k + a, 1)] { for (t, &cc) in m.cap_of[idx].iter().enumerate() { let s = s * m.cw(idx, t) as i64;
            match self.dl.iter_mut().find(|e| e.0 == cc) { Some(e) => e.1 += s, None => self.dl.push((cc, s)) } } }
        if self.dl.iter().any(|&(cc, d)| d > 0 && self.load[cc] as i64 + d > m.limit[cc] as i64) { self.dl.clear(); return; }
        // local log-weight of the three variables (pairs inside the triple counted once)
        let local = |x: &[usize]| { let mut e = 0.0; for &v in &[i, j, l] { e += m.h[v * k + x[v]];
            for &(o, q) in &m.adj[v] { if (o != i && o != j && o != l) || o > v { e += m.coupling(q, v, x[v], x[o]); } } } e };
        let e0 = local(&self.x); self.x[i] = b; self.x[j] = c; self.x[l] = a; let e1 = local(&self.x);
        if e1 >= e0 || self.rng.f64() < ((e1 - e0) * self.beta).exp() { for &(cc, d) in &self.dl { self.load[cc] = (self.load[cc] as i64 + d) as usize; } }
        else { self.x[i] = a; self.x[j] = b; self.x[l] = c; }
        self.dl.clear();
    }
    fn flip_plan(&self) -> FlipPlan {
        let m = self.m; let k = m.k;
        let vars: Vec<(usize, usize, usize, f64)> = (0..m.n).filter(|&i| m.cand_count(i) == 2).map(|i| { let (c0, c1) = (m.cand[i][0], m.cand[i][1]); (i, c0, c1, m.h[i * k + c1] - m.h[i * k + c0]) }).collect();
        let mut pos = vec![usize::MAX; m.n]; for (q, v) in vars.iter().enumerate() { pos[v.0] = q; }
        let (mut one, mut both) = (vec![], vec![]);
        for (a, &(i, c0, c1, _)) in vars.iter().enumerate() { for &(j, q) in &m.adj[i] {
            if pos[j] == usize::MAX { one.push((q, i, j)); continue; }
            if j < i { continue; }
            let (d0, d1) = (vars[pos[j]].1, vars[pos[j]].2); let mut t = [0.0; 4];
            for (si, &(xi, yi)) in [(c0, c1), (c1, c0)].iter().enumerate() { for (sj, &(xj, yj)) in [(d0, d1), (d1, d0)].iter().enumerate() {
                t[si * 2 + sj] = match &m.pairs[q].c { Coupling::Potts(w) => (if yi == yj { *w } else { 0.0 }) - (if xi == xj { *w } else { 0.0 }),
                    Coupling::Table(_) => m.coupling(q, i, yi, yj) - m.coupling(q, i, xi, xj) }; } }
            if t.iter().any(|&v| v != 0.0) { both.push((a, pos[j], t)); } } }
        let capped = vars.iter().any(|&(i, c0, c1, _)| !m.cap_of[i * k + c0].is_empty() || !m.cap_of[i * k + c1].is_empty());
        FlipPlan { vars, pos, one, both, capped, dcap: vec![0; m.caps.len()], seen: vec![false; m.caps.len()], touched: vec![] }
    }
    pub fn logw(&self) -> f64 { self.m.logw(&self.x) }
    /// A chain AT a feasible state `x`: no random feasible-start search, no over-dispersion sweeps (the anneal's warm
    /// start). `anneal` used `new` + `set_state` once per beta stage and discarded the search's result; on a 200-job precedence
    /// schedule (82,678 pair caps) that search (2M-node budget, O(n k caps) per node) did not return within a 30 s alarm.
    pub fn from_state(m: &'a Model, seed: u64, stream: u64, x: &[usize]) -> Self {
        let mut c = Chain { m, x: vec![], load: vec![], rng: Philox4x32::new(seed, stream), w: vec![f64::NEG_INFINITY; m.k], dl: vec![], flip: None, rl: (vec![false; m.n], vec![], vec![0; m.caps.len()], vec![false; m.caps.len()], vec![]), moves: [0; 2], beta: 1.0, plain: false }; c.set_state(x); c
    }
    /// Replace the state (must be feasible for this model); capacity loads are recomputed.
    pub fn set_state(&mut self, x: &[usize]) {
        let k = self.m.k; self.x = x.to_vec(); self.load = vec![0; self.m.caps.len()];
        let m = self.m; for (i, &v) in x.iter().enumerate() { m.add_load(&mut self.load, i * k + v); }
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
    // R19.7: one model, the stage's beta on the chain (was a scaled Model clone per beta, built before the clock started)
    let run = |c: usize| -> Option<(f64, Vec<usize>)> {
        let mut x = match start { Some(s) => s.to_vec(), None => Chain::new(m, seed, c as u64)?.x };
        let mut best = (m.logw(&x), x.clone()); let t0 = std::time::Instant::now(); let nq = betas.len();
        for (q, &b) in betas.iter().enumerate() {
            let mut ch = Chain::from_state(m, seed ^ 0x5eed, (c * 64 + q) as u64, &x); ch.beta = b; ch.plain = true;
            let stop = ms * (q + 1) as f64 / nq as f64; let mut j = 0usize;
            let n = sweeps * (q + 1) / nq - sweeps * q / nq;
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
    pub traj: Vec<Vec<u16>>, pub marg: Vec<f64>, pub n: usize, pub plans: HashMap<Vec<u8>, u32>, pub trace: Vec<Vec<f64>>, pub viol: usize, pub best: (f64, Vec<usize>), pub sweeps: usize,
    /// R19 P1.2: per chain (chain order), accepted collective moves [global flips, label swaps] (`Chain::moves`); empty when a
    /// sampler does not track them (tempering, the anytime router loop)
    pub moves: Vec<[u64; 2]> }

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
    let mut s = Samples { chain_marg: vec![], traj: vec![vec![]], marg: vec![0.0; m.n * k], n: 0, plans: HashMap::new(), trace: vec![vec![]], viol: 0, best: (f64::NEG_INFINITY, vec![]), sweeps: 0, moves: vec![] };
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
    s.sweeps = it; s.moves = vec![ch.moves]; Some(s)
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
/// Whether the R19.6 MAC start is tried (default true; process-wide, for the cost benchmark `examples/mac_start_cost.rs`).
pub static START_MAC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Starts found by the R19.6 arc-consistent (MAC) start search (process-wide, for tests and benchmarks).
pub static START_PROPAGATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
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
    let mut out = Samples { chain_marg: vec![], traj: vec![], marg: vec![0.0; nk], n: 0, plans: HashMap::new(), trace: vec![], viol: 0, best: (f64::NEG_INFINITY, vec![]), sweeps: 0, moves: vec![] };
    for s in parts { for (m, v) in out.marg.iter_mut().zip(&s.marg) { *m += v; } out.chain_marg.push(s.marg.iter().map(|v| v / (s.n as f64).max(1.0)).collect()); out.n += s.n; out.viol += s.viol; out.sweeps += s.sweeps; out.moves.extend(s.moves);
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
    let c = &s.chain_marg; let nc = c.len();
    // R19.7 (P2.3): per variable the largest pairwise TV is half the L1 diameter of the chains' marginal vectors, and the L1
    // diameter of points in R^k is the max over sign vectors s (s_0 = +1, 2^(k-1) of them) of max_u s.p_u - min_u s.p_u:
    // O(chains x 2^(k-1) x k) per variable instead of O(chains^2 x k) (10,000 chains: 7.5 s of gate, R18). Only for many
    // chains (the rounding differs from the pairwise sum, so default runs keep the pairwise scan and their bytes).
    if nc >= CHAIN_DIS_PROJ_MIN && (1..=12).contains(&na) && nc > 4 << (na - 1) { return chain_disagreement_proj(c, na); }
    let mut worst: f64 = 0.0;
    for u in 0..nc { for v in u + 1..nc { worst = worst.max(max_tv(&c[u], &c[v], na)); } } worst
}
/// R19.7: chains from which `chain_disagreement` uses sign projections.
pub const CHAIN_DIS_PROJ_MIN: usize = 128;
/// `chain_disagreement` by sign projections (see there); equal to the pairwise scan up to rounding.
pub fn chain_disagreement_proj(c: &[Vec<f64>], na: usize) -> f64 {
    // R19.8: variables on the gate pool, and every sign of a chain's vector in one visit (was one pass over all chains per
    // sign: 2^(k-1) cache misses per chain). Same sums in the same order, min / max / max are exact: bit-identical.
    let t = c.first().map_or(0, |m| m.len() / na); let ns = 1usize << (na - 1);
    pool_map(t, |i| { let (mut lo, mut hi) = (vec![f64::INFINITY; ns], vec![f64::NEG_INFINITY; ns]);
        for m in c { let p = &m[i * na..(i + 1) * na];
            for sg in 0..ns { let mut d = p[0]; for (v, &x) in p.iter().enumerate().skip(1) { if sg >> (v - 1) & 1 == 1 { d -= x; } else { d += x; } }
                if d < lo[sg] { lo[sg] = d; } if d > hi[sg] { hi[sg] = d; } } }
        (0..ns).map(|sg| 0.5 * (hi[sg] - lo[sg])).fold(0.0, f64::max) }).into_iter().fold(0.0, f64::max)
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
    /// Non-partition programs: forced members (clamps) do not count as a class. Every program (partition programs since R19.3)
    /// adds each free variable that never changed value in any chain, after unit propagation (see `frozen_detail`).
    pub frozen: usize,
    /// Per-variable split-R-hat of the marginal indicators (max over values with pooled P > 0.02), from half-chain
    /// counts with Bernoulli within-variance. gate/3: every release needs it < `ITEM_RHAT_MAX` (whole answers: every variable).
    /// It sees chains that sit in different modes of one variable even when their log-weight traces agree (symmetric modes:
    /// the log-weight R-hat is blind there), and when chains disagree beyond what the batch-means MCSE expects (with C chains
    /// split between two values, the MCSE treats the chains' batches as independent and reads ~sqrt(batches) too small).
    pub rhat_task: Vec<f64>,
    /// R19 P1.2: per capacity constraint, split-R-hat of its load (occupancy count) trace (`rhat_occupancy`), and per variable
    /// the largest over the constraints it is a member of (`occ_task`; empty = no constraints). gate/3 releases a variable only if
    /// its `occ_task` < ITEM_RHAT_MAX too: chains that hold different counts on a worker while each member's indicator still mixes.
    pub rhat_occ: Vec<f64>, pub occ_task: Vec<f64>,
    /// Per variable, escalated by the frozen rule (never released by `released_tasks`). Partition programs: every variable
    /// iff `frozen > 0` (unchanged). Non-partition programs: the variables whose connected component (free variables linked by
    /// couplings and by caps that forced members do not fill) holds a stuck variable or a frozen cap's free member; the target
    /// factorizes over these components, so the others keep valid odds; forced variables too while `frozen > 0`. Empty (from
    /// `gate_stats_with`) = every variable iff `frozen > 0`.
    pub escalate: Vec<bool>,
    /// Batches per chain (min over chains) in the LONG pass (batch size n^(2/3)); `min_batches` is the short pass (sqrt n). gate/2
    /// requires BOTH to reach `GateCfg::min_batches` (before, only the short pass was checked, so the advertised ">= 8 batches per
    /// chain" did not hold for the long-batch MCSE: 300 sweeps per chain had 6 long batches and released items).
    pub min_batches_long: usize }
/// The gate's version, reported with every sampled answer; bump it whenever a release rule or threshold changes.
pub const GATE_VERSION: &str = "gate/3";
/// What a sampled release assumes (reported with every sampled answer). The gate checks diagnostics; it proves none of these.
pub const GATE_ASSUMPTIONS: [&str; 4] = [
    "every mode with non-negligible mass is visited by some chain (not checked; counterexamples: BENCHMARKS Known failure modes)",
    "the chains are near stationarity after burn-in (diagnosed by split R-hat of the log-weight trace and of every variable's value indicators, not proved)",
    "batch means are approximately normal: bound = 3 x the larger of two batch-means MCSEs (batch sizes sqrt n and n^(2/3))",
    "the odds are Monte-Carlo estimates of the marginals of the supplied score model, not predictive calibration"];
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
/// gate/3 (R19 P1.2): a variable is released only if its indicator split-R-hat (`Gate::rhat_task`) is below this, and a whole
/// answer passes only if every variable's is. 1.05 = the whole-answer log-weight threshold: a mode detector, deliberately loose
/// (well-mixed chains that pass the 3-sigma MCSE bound sit near 1.00-1.01 at 4 chains; between-chain disagreement on a value
/// gives R-hat >> 1, infinite when each chain holds one value throughout). Not tuned on the stress corpus (P1.4 freezes thresholds).
pub const ITEM_RHAT_MAX: f64 = 1.05;
pub const GATE: GateCfg = GateCfg { z: 3.0, tv_tol: 0.05, rhat_max: 1.05, min_batches: 8 };
impl Gate {
    pub fn tv_bound(&self, cfg: &GateCfg) -> f64 { cfg.z * self.sig_tv_max.max(self.sig_tv_long.iter().cloned().fold(0.0, f64::max)) }
    /// Partial certification: if the run is globally sane (R-hat, batches), certify each variable whose OWN bound passes;
    /// the rest are escalated individually instead of refusing the whole joint answer.
    pub fn released_tasks(&self, cfg: &GateCfg) -> Vec<bool> {
        // per-ticket release needs a STRICTER global mixing check than the whole-answer gate (calibrated:
        // R-hat < 1.05 let stuck-region tickets through at lam >= 2 with ticket FCR 8.6%; < 1.005 -> 2.5%)
        // Recalibrated with calib_sat (3 fresh instance sets incl. rho = 1 and T = 1000): guard 1.002 + dual-batch sigma.
        // + batch-size stability: if the run-wide MCSE still grows with batch length (sigma_n^(2/3) > 2 x sigma_sqrt(n)),
        //   the chains have not reached the asymptotic regime; per-ticket bounds are then biased low (18 bad tickets
        //   in the two runs with ratio 2.13 / 2.33; 70 more at 1.90-1.95 on a fresh set -> threshold 1.5, see BATCH_RATIO_MAX).
        // The frozen rule escalates per variable (`escalate`; whole answers still need frozen == 0, see `diagnostics_passed`), and the
        // batch-stability check reads only the variables it leaves in (chains trapped in different modes of a stuck component
        // give between-chain offsets that grow with batch length by construction). Nothing escalated: the run-wide check as before.
        let esc = |i: usize| self.escalate.get(i).copied().unwrap_or(self.frozen > 0); let n = self.sig_tv.len();
        let long = (0..n).filter(|&i| !esc(i)).map(|i| self.sig_tv_long[i]).fold(0.0, f64::max);
        let short = if (0..n).any(esc) { (0..n).filter(|&i| !esc(i)).map(|i| if self.sig_tv[i].is_nan() { f64::INFINITY } else { self.sig_tv[i] }).fold(0.0, f64::max) } else { self.sig_tv_max };
        let ok = self.rhat < PARTIAL_RHAT.min(cfg.rhat_max) && self.min_batches.min(self.min_batches_long) >= cfg.min_batches && long <= BATCH_RATIO_MAX * short;
        self.sig_tv.iter().zip(&self.sig_tv_long).enumerate().map(|(i, (&s, &l))| ok && !esc(i) && self.item_rhat_ok(i) && cfg.z * s.max(l) <= cfg.tv_tol).collect()
    }
    /// gate/3: variable `i`'s indicator split-R-hat is below `ITEM_RHAT_MAX` (false when it was not computed: `gate_stats_with`)
    pub fn item_rhat_ok(&self, i: usize) -> bool { self.rhat_task.get(i).map_or(false, |&r| r < ITEM_RHAT_MAX) && self.occ_task.get(i).map_or(true, |&r| r < ITEM_RHAT_MAX) }
    /// EXPERIMENTAL — refuted as a replacement (16 bad / 5,226 on the 2nd fresh set, one T=80 lam-2 run with logw
    /// R-hat 1.008). Kept for a hybrid candidate (add run-wide R-hat < 1.005). Per-ticket release with a LOCAL mixing check instead of the run-wide R-hat cliff:
    /// logw R-hat < cfg.rhat_max, frozen == 0, batch stability, >= min batches, and per ticket rhat_task < 1.01 and bound <= tol.
    pub fn released_tasks_local(&self, cfg: &GateCfg) -> Vec<bool> {
        let long = self.sig_tv_long.iter().cloned().fold(0.0, f64::max);
        let ok = self.rhat < cfg.rhat_max && self.min_batches >= cfg.min_batches && self.frozen == 0 && long <= BATCH_RATIO_MAX * self.sig_tv_max;
        (0..self.sig_tv.len()).map(|i| ok && self.rhat_task.get(i).map_or(false, |&r| r < 1.01) && cfg.z * self.sig_tv[i].max(self.sig_tv_long[i]) <= cfg.tv_tol).collect()
    }
    /// The batch-stability guard applies to whole answers too (T=80 stress set: one false certificate, maxTV .111, at
    /// ratio 1.82; over all 16 calibration sets ratio <= 1.5 removes it at a cost of 15 of 318 certificates).
    /// Per variable, why it was released or escalated (gate/2 `release_reason`): `whole_answer_gate` (the whole-answer test
    /// passed: every variable is released by it, without the stricter per-item R-hat), `item_gate` (its own bound and the
    /// per-item global checks passed), or the first failed check: `frozen`, `rhat`, `batches`, `batch_stability`, `item_rhat` (gate/3:
    /// its own indicator R-hat), `item_bound`.
    pub fn release_reasons(&self, cfg: &GateCfg) -> Vec<&'static str> {
        if self.diagnostics_passed(cfg) { return vec!["whole_answer_gate"; self.sig_tv.len()]; }
        let esc = |i: usize| self.escalate.get(i).copied().unwrap_or(self.frozen > 0); let n = self.sig_tv.len();
        let long = (0..n).filter(|&i| !esc(i)).map(|i| self.sig_tv_long[i]).fold(0.0, f64::max);
        let short = if (0..n).any(esc) { (0..n).filter(|&i| !esc(i)).map(|i| if self.sig_tv[i].is_nan() { f64::INFINITY } else { self.sig_tv[i] }).fold(0.0, f64::max) } else { self.sig_tv_max };
        let rel = self.released_tasks(cfg);
        (0..n).map(|i| if rel[i] { "item_gate" } else if esc(i) { "frozen" } else if !(self.rhat < PARTIAL_RHAT.min(cfg.rhat_max)) { "rhat" }
            else if self.min_batches.min(self.min_batches_long) < cfg.min_batches { "batches" } else if !(long <= BATCH_RATIO_MAX * short) { "batch_stability" }
            else if !self.item_rhat_ok(i) { "item_rhat" } else { "item_bound" }).collect()
    }
    pub fn diagnostics_passed(&self, cfg: &GateCfg) -> bool { self.rhat < cfg.rhat_max && self.tv_bound(cfg) <= cfg.tv_tol && self.min_batches.min(self.min_batches_long) >= cfg.min_batches && self.frozen == 0
        && self.sig_tv_long.iter().cloned().fold(0.0, f64::max) <= BATCH_RATIO_MAX * self.sig_tv_max && (0..self.sig_tv.len()).all(|i| self.item_rhat_ok(i)) }
}
/// R19.8 (P2.3): worker threads of the gate's per-chain passes (`pool_map`); 0 = `std::thread::available_parallelism`.
/// The CLI sets it to `--threads`. Statistics never depend on it.
pub static GATE_THREADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// `f(0..n)` on min(n, GATE_THREADS) scoped workers (worker w takes w, w + W, ...), results in index order. The gate's
/// per-chain passes spawned ONE THREAD PER CHAIN before (R19.8 profile at 10,000 chains: nearly all of the gate's 2.2-6.5 s was
/// thread create / map / unmap / teardown, ~1% statistics); each result is computed alone and combined in chain order by the
/// caller, so every statistic is bit-identical to the per-chain-thread version.
pub fn pool_map<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let g = GATE_THREADS.load(std::sync::atomic::Ordering::Relaxed);
    let w = if g == 0 { std::thread::available_parallelism().map_or(1, |p| p.get()) } else { g }.clamp(1, n.max(1));
    if w == 1 { return (0..n).map(f).collect(); }
    let mut slots: Vec<Option<T>> = (0..n).map(|_| None).collect();
    std::thread::scope(|sc| { let f = &f; let hs: Vec<_> = (0..w).map(|wk| sc.spawn(move || (wk..n).step_by(w).map(|c| (c, f(c))).collect::<Vec<_>>())).collect();
        for h in hs { for (c, r) in h.join().unwrap() { slots[c] = Some(r); } } });
    slots.into_iter().map(|r| r.expect("pool_map slot")).collect()
}
pub fn gate_stats(m: &Model, s: &Samples) -> Gate {
    let (mut g, l, (fz, esc)) = std::thread::scope(|sc| { let hl = sc.spawn(|| gate_stats_bs(m, s, 2.0 / 3.0, false)); let hf = sc.spawn(|| frozen_detail(m, s));
        (gate_stats_with(m, s, GATE_BS_POW), hl.join().unwrap(), hf.join().unwrap()) });
    g.sig_tv_long = l.sig_tv; g.min_batches_long = l.min_batches; g.frozen = fz; g.escalate = esc; g.rhat_task = rhat_tasks(m, s);
    g.rhat_occ = rhat_occupancy(m, s);
    if !g.rhat_occ.is_empty() { let mut o = vec![1.0f64; m.n]; for (c, cap) in m.caps.iter().enumerate() { for &(i, _) in &cap.members { if !(o[i] >= g.rhat_occ[c]) { o[i] = g.rhat_occ[c]; } } } g.occ_task = o; }
    g
}
/// see `Gate::rhat_occ`: per capacity constraint, split-R-hat over chains of its load per kept row (half-chains, sample
/// within-variance). 1.0 when the load never varies (a saturated cap); infinite when it is constant within each half-chain but
/// differs between them. Streaming: per half-chain only a sum and a sum of squares per constraint.
pub fn rhat_occupancy(m: &Model, s: &Samples) -> Vec<f64> {
    let (t, k, nc) = (m.n, m.k, m.caps.len()); if nc == 0 { return vec![]; }
    let n = s.traj.iter().map(|tr| tr.len() / t).min().unwrap_or(0) / 2; if n < 4 || s.traj.len() < 2 { return vec![f64::INFINITY; nc]; }
    let mut halves: Vec<Vec<(f64, f64)>> = vec![]; let mut ld = vec![0usize; nc];
    for tr in &s.traj { let st = tr.len() / t - 2 * n;
        for h in 0..2 { let mut acc = vec![(0.0f64, 0.0f64); nc];
            for q in st + h * n..st + (h + 1) * n { let row = &tr[q * t..(q + 1) * t]; for v in ld.iter_mut() { *v = 0; }
                if m.partition { for i in 0..t { let b = m.bucket[i * k + row[i] as usize]; if b != NONE { ld[b as usize] += 1; } } }
                else { for i in 0..t { m.add_load(&mut ld, i * k + row[i] as usize); } }
                for c in 0..nc { let x = ld[c] as f64; acc[c].0 += x; acc[c].1 += x * x; } }
            halves.push(acc); } }
    let (mm, nf) = (halves.len() as f64, n as f64);
    (0..nc).map(|c| { let means: Vec<f64> = halves.iter().map(|h| h[c].0 / nf).collect(); let gm = means.iter().sum::<f64>() / mm;
        let b = nf / (mm - 1.0) * means.iter().map(|x| (x - gm).powi(2)).sum::<f64>();
        let w = halves.iter().zip(&means).map(|(h, mu)| ((h[c].1 - nf * mu * mu) / (nf - 1.0)).max(0.0)).sum::<f64>() / mm;
        if w <= 0.0 { if b <= 0.0 { 1.0 } else { f64::INFINITY } } else { (((nf - 1.0) / nf * w + b / nf) / w).sqrt() } }).collect()
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
/// R19.7: at most this many forced-by-counting tests per gate call (`forced_values`).
pub const FORCED_TESTS: usize = 64;
/// R19.7 (the R19.6 c12 finding, a false refusal): per variable the value it is FORCED to, or None. Forced = one allowed /
/// clamped value, or (partition programs) forced BY COUNTING: every chain held it at one value a for its whole run, and with
/// a forbidden the capacitated matching finds no plan (on partition programs a proof: Kuhn augmenting paths are complete).
/// Quota programs whose caps sum to n force members that way (saturated-k3: four {a, c} variables, eight {b, c}, every value
/// capped at 4, n = 12: the {a, c} ones are always a). Such a variable is a constant under the target: the chains are right
/// not to move it, so it is not stuck, it is left out of the class counts and it counts as a forced member of its caps.
/// At most FORCED_TESTS matchings; non-partition programs (no proof from their budgeted start search) are not tested.
pub fn forced_values(m: &Model, s: &Samples) -> Vec<Option<usize>> {
    let (t, k) = (m.n, m.k);
    let mut fv: Vec<Option<usize>> = (0..t).map(|i| if m.cand_count(i) == 1 { (0..k).find(|&v| m.ok(i, v)) } else { None }).collect();
    if !m.partition || s.traj.is_empty() { return fv; }
    let mut tests = 0;
    for i in 0..t { if fv[i].is_some() || s.traj[0].len() < t { continue; }
        let a = s.traj[0][i] as usize;
        if !s.traj.iter().all(|tr| tr.chunks(t).all(|row| row[i] as usize == a)) { continue; }
        tests += 1; if tests > FORCED_TESTS { break; }
        let mut al = m.allowed.clone(); al[i * k + a] = false;
        let Ok(sub) = Model::new(t, k, vec![0.0; t * k], al, m.clamp.clone(), vec![], m.caps.clone()) else { continue };
        if sub.feasible_init_rand(&mut Philox4x32::new(0xF0CE, i as u64)).is_none() { fv[i] = Some(a); } }
    fv
}
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
    // R19.7: forced = single-valued or forced by counting (`forced_values`)
    let fv = forced_values(m, s);
    let multi: Vec<bool> = m.caps.iter().map(|c| {
        // With FORCED_CAPS_PARTITION, partition programs also leave forced members out of the class count (their classes
        // sit on the cap for good; if only one class of free members can take the other slots, the multiset can never change)
        let free = |i: usize| (m.partition && !FORCED_CAPS_PARTITION) || fv[i].is_none();
        // R19.7: forced members' weight plus the lightest free member's must fit (unit: fewer forced members than the limit)
        let forced: usize = if m.partition && !FORCED_CAPS_PARTITION { 0 } else { c.members.iter().enumerate().filter(|&(_, &(i, v))| fv[i] == Some(v)).map(|(t, _)| c.w(t)).sum() };
        let light = c.members.iter().enumerate().filter(|&(_, &(i, v))| m.ok(i, v) && free(i)).map(|(t, _)| c.w(t)).min().unwrap_or(usize::MAX);
        let mut cl: Vec<usize> = c.members.iter().filter(|&&(i, v)| m.ok(i, v) && free(i)).map(|&(i, _)| class[i]).collect(); cl.sort(); cl.dedup(); cl.len() >= 2 && forced.saturating_add(light) <= c.limit }).collect();
    // full = no member fits on top of the load (unit: load >= limit)
    let wmin: Vec<usize> = m.caps.iter().map(|c| (0..c.members.len()).map(|t| c.w(t)).min().unwrap_or(1)).collect();
    // per chain: (always full per constraint, changed per constraint)
    let per: Vec<(Vec<bool>, Vec<bool>)> = pool_map(s.traj.len(), |ci| { let tr = &s.traj[ci];
        let mut full = vec![true; nc]; let mut changed = vec![false; nc]; let mut prev: Vec<u64> = vec![]; let mut h = vec![0u64; nc]; let mut ld = vec![0usize; nc];
        for row in tr.chunks(t) { for v in h.iter_mut() { *v = 0; } for v in ld.iter_mut() { *v = 0; }
            if m.partition { for i in 0..t { let b = m.bucket[i * k + row[i] as usize]; if b != NONE { h[b as usize] = h[b as usize].wrapping_add(key[class[i]]); ld[b as usize] += 1; } } }
            else { for i in 0..t { let iv = i * k + row[i] as usize; for &c in &m.cap_of[iv] { h[c] = h[c].wrapping_add(key[class[i]]); } m.add_load(&mut ld, iv); } }
            for c in 0..nc { if ld[c] + wmin[c] <= m.caps[c].limit { full[c] = false; } }
            if !prev.is_empty() { for c in 0..nc { if prev[c] != h[c] { changed[c] = true; } } }
            std::mem::swap(&mut prev, &mut h); if h.len() != nc { h = vec![0u64; nc]; } }
        (full, changed) });
    let fcaps: Vec<usize> = (0..nc).filter(|&c| multi[c] && per.iter().all(|q| q.0[c]) && per.iter().any(|q| !q.1[c])).collect();
    // Every program also counts each FREE variable (> 1 allowed value) that never changed value in any chain: a chain that
    // cannot move carries no information about the odds (sudoku: stuck chains look certain), so no release. Partition programs
    // (the router) too since R19.3 (P1.2; before, they returned here with the saturated-cap count only); per component since R19.5.
    let mut stuck: Vec<usize> = (0..t).filter(|&i| fv[i].is_none() && s.traj.iter().all(|tr| tr.chunks(t).all(|row| row[i] == tr[i]))).collect();
    if fcaps.is_empty() && stuck.is_empty() { return (0, vec![false; t]); }
    // Unit propagation. A cap that forced variables fill forbids its other members' values; a variable left with one value
    // is forced too (to a fixpoint). Sound: such a variable is a constant under the target, so the chains are right not to move it
    // (colouring: a vertex whose other colours clamped neighbours hold). It is not stuck and, like a clamp, links nothing.
    // Worklist, linear in the caps' sizes: per cap a count of forced members; a cap fires once, when the count reaches its limit.
    // (An earlier pass-until-no-change loop was O(n x caps) on a forcing chain listed against cap order: 234 ms at 8,000 variables.)
    let mut dom: Vec<Vec<usize>> = (0..t).map(|i| match fv[i] { Some(v) => vec![v], None => (0..k).filter(|&v| m.ok(i, v)).collect() }).collect();
    let (mut cnt, mut fired) = (vec![0usize; nc], vec![false; nc]); let mut queue: Vec<usize> = (0..t).filter(|&i| dom[i].len() == 1).collect();
    // R19.7: a weighted cap is re-checked whenever its forced load grows (and once at the start), removing every member that
    // no longer fits; it never counts as fired (its other free members stay linked)
    let mut ready: Vec<usize> = (0..nc).filter(|&c| m.caps[c].limit == 0 || !m.caps[c].weights.is_empty()).collect();
    loop {
        while let Some(i) = queue.pop() { let iv = i * k + dom[i][0]; for (t, &c) in m.cap_of[iv].iter().enumerate() { cnt[c] += m.cw(iv, t); if cnt[c] >= m.caps[c].limit || !m.caps[c].weights.is_empty() { ready.push(c); } } }
        let Some(c) = ready.pop() else { break };
        let cp = &m.caps[c]; if cp.weights.is_empty() { if fired[c] { continue; } fired[c] = true; }
        for (r, &(i, v)) in cp.members.iter().enumerate() { if dom[i].len() > 1 && cnt[c] + cp.w(r) > cp.limit { if let Some(q) = dom[i].iter().position(|&x| x == v) { dom[i].remove(q); if dom[i].len() == 1 { queue.push(i); } } } }
    }
    let free = |i: usize| dom[i].len() > 1; stuck.retain(|&i| free(i));
    let fz = fcaps.len() + stuck.len(); if fz == 0 { return (0, vec![false; t]); }
    // R19.5: partition programs (the router) escalate per component too (they escalated EVERY variable before; the product
    // argument below does not depend on the cap layout). Escalate per connected component of the FREE variables (was: every variable). A coupling links its two ends; a cap
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
pub fn gate_stats_with(m: &Model, s: &Samples, bs_pow: f64) -> Gate { gate_stats_bs(m, s, bs_pow, true) }
/// `gate_stats_with`; `dis` = false skips `chain_disagreement` (chain_dis = 0): `gate_stats`' long-batch pass, whose
/// chain_dis was computed and dropped (R19.8: the second-largest gate cost at 100,000 chains).
fn gate_stats_bs(m: &Model, s: &Samples, bs_pow: f64, dis: bool) -> Gate {
    // Each chain's share of the pooled mean by its trajectory rows (== s.n when nothing was thinned; with --mem-limit-mb
    // every chain on fixed sweeps thins alike, so the shares are unchanged; using s.n here would shrink the variance when thinned)
    let (t, na) = (m.n, m.k); let tn = t * na; let nn = s.traj.iter().map(|tr| tr.len() / t).sum::<usize>() as f64;
    let mut var = vec![0.0f64; tn]; let mut min_b = usize::MAX;
    // chains in parallel (R19.8: on the bounded `pool_map`); contributions are added in chain order afterwards, so the result
    // is bit-identical to serial.
    let parts: Vec<(usize, Option<(f64, Vec<f64>)>)> = pool_map(s.traj.len(), |ci| { let tr = &s.traj[ci];
        let nc = tr.len() / t; if nc == 0 { return (0, None); }
        let bs = ((nc as f64).powf(bs_pow) as usize).max(1); let nb = nc / bs;
        let start = nc - nb * bs; let mut acc = vec![0.0f64; tn]; let mut cnt = vec![0u32; tn];
        for b in 0..nb {
            for v in cnt.iter_mut() { *v = 0; }
            for q in start + b * bs..start + (b + 1) * bs { let row = &tr[q * t..(q + 1) * t]; for i in 0..t { cnt[i * na + row[i] as usize] += 1; } }
            for v in 0..tn { if m.allowed[v] { let d = cnt[v] as f64 / bs as f64 - s.marg[v]; acc[v] += d * d; } }
        }
        (nb, if nb >= 2 { Some(((nc as f64 / nn).powi(2) / (nb as f64 * (nb as f64 - 1.0)), acc)) } else { None }) });
    for (nb, c) in parts { min_b = min_b.min(nb);
        match c { Some((wgt, acc)) => { for v in 0..tn { var[v] += wgt * acc[v]; } }, None => { for v in 0..tn { var[v] = f64::INFINITY; } } } }
    let mut sig_tv_max = 0.0; let mut worst = 0; let mut min_ess = f64::INFINITY; let mut sig_tv = vec![0.0; t];
    for i in 0..t { let st: f64 = 0.5 * (0..na).map(|a| var[i * na + a].sqrt()).sum::<f64>(); sig_tv[i] = st;
        if st > sig_tv_max || st.is_nan() { sig_tv_max = if st.is_nan() { f64::INFINITY } else { st }; worst = i; }
        for a in 0..na { let mg = s.marg[i * na + a]; if mg > 0.02 && mg < 0.98 { let e = mg * (1.0 - mg) / var[i * na + a]; if e < min_ess { min_ess = e; } } } }
    Gate { rhat: split_rhat(&s.trace), sig_tv_long: vec![0.0; t], sig_tv, sig_tv_max, worst_task: worst, min_ess, min_batches: if min_b == usize::MAX { 0 } else { min_b }, chain_dis: if dis { chain_disagreement(s, na) } else { 0.0 }, frozen: 0, rhat_task: vec![], rhat_occ: vec![], occ_task: vec![], escalate: vec![], min_batches_long: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 2 x 3 toy: exact marginals by brute force over all 3^n states, with a table coupling and a non-partition capacity.
    fn toy() -> Model {
        let (n, k) = (4, 3); let h = vec![0.1, -0.3, 0.5, 0.0, 0.2, -0.1, 0.4, 0.4, -0.2, -0.5, 0.3, 0.1];
        let pairs = vec![Pair { i: 0, j: 1, c: Coupling::Potts(0.7) }, Pair { i: 1, j: 3, c: Coupling::Table(vec![0.0, 0.5, -0.2, 0.3, 0.0, 0.1, -0.4, 0.2, 0.0]) }, Pair { i: 2, j: 0, c: Coupling::Potts(-0.6) }];
        let caps = vec![Cap { weights: vec![], members: vec![(0, 0), (1, 0), (2, 0), (3, 1)], limit: 1 }, Cap { weights: vec![], members: vec![(0, 0), (2, 2), (3, 2)], limit: 1 }];
        let mut allowed = vec![true; n * k]; allowed[2 * k + 1] = false;
        Model::new(n, k, h, allowed, vec![None; n], pairs, caps).unwrap()
    }
    fn brute(m: &Model) -> (f64, Vec<f64>) {
        let mut z = 0.0; let mut mg = vec![0.0; m.n * m.k];
        for s in 0..m.k.pow(m.n as u32) { let x: Vec<usize> = (0..m.n).map(|i| (s / m.k.pow(i as u32)) % m.k).collect();
            if m.violations(&x) > 0 { continue; } let w = m.logw(&x).exp(); z += w; for i in 0..m.n { mg[i * m.k + x[i]] += w; } }
        for v in mg.iter_mut() { *v /= z; } (z.ln(), mg)
    }
    /// R19.7 (P2.1 linear <=): 300 random programs with weighted caps (plus unit caps, binary forbid caps that prune under
    /// root arc consistency so the MAC start runs, pairs and forbidden values): `violations`, enumeration (n_feasible, log Z,
    /// odds), the components tier, the compile pass and the start search vs a direct reading of the rule (sum of the active
    /// members' weights <= limit), 1e-9.
    #[test]
    fn weighted_caps_match_brute_force() {
        let mut r = Philox4x32::new(1907, 1); let (mut infeasible, mut weighted) = (0, 0);
        for case in 0..300u64 {
            let (n, k) = (2 + r.below(5), 2 + r.below(2));
            let h: Vec<f64> = (0..n * k).map(|_| r.f64() * 2.0 - 1.0).collect();
            let mut allowed = vec![true; n * k]; if r.f64() < 0.3 { allowed[r.below(n) * k + 1] = false; }
            let pairs = if r.f64() < 0.5 { vec![Pair { i: 0, j: 1, c: Coupling::Potts(r.f64() - 0.5) }] } else { vec![] };
            let mut caps = vec![];
            for _ in 0..1 + r.below(2) { let (mut mem, mut w) = (vec![], vec![]);
                for i in 0..n { for v in 0..k { if r.f64() < 0.4 { mem.push((i, v)); w.push(1 + r.below(5)); } } }
                if mem.is_empty() { continue; } let tot: usize = w.iter().sum(); caps.push(Cap { members: mem, limit: r.below(tot + 1), weights: w }); }
            if r.f64() < 0.3 { caps.push(Cap::new((0..n).map(|i| (i, 0)).collect(), 1 + r.below(n))); }
            if r.f64() < 0.3 { caps.push(Cap::new(vec![(0, 1), (1, 0)], 1)); caps.push(Cap::new(vec![(0, 1), (1, 1)], 1)); } // x0 = 1 has no support
            let direct = |x: &[usize]| caps.iter().all(|c| c.members.iter().enumerate().filter(|&(_, &(i, v))| x[i] == v).map(|(t, _)| c.w(t)).sum::<usize>() <= c.limit);
            let m = Model::new(n, k, h, allowed.clone(), vec![None; n], pairs, caps.clone()).unwrap();
            let (mut z, mut mg, mut nf) = (0.0, vec![0.0; n * k], 0u64);
            for s in 0..k.pow(n as u32) { let x: Vec<usize> = (0..n).map(|i| (s / k.pow(i as u32)) % k).collect();
                let ok = (0..n).all(|i| allowed[i * k + x[i]]) && direct(&x);
                assert_eq!(ok, m.violations(&x) == 0, "case {case}: violations at {x:?}");
                if !ok { continue; } nf += 1; let w = m.logw(&x).exp(); z += w; for i in 0..n { mg[i * k + x[i]] += w; } }
            let e = exact(&m, 1, 1 << 20).unwrap(); assert_eq!(e.n_feasible, nf, "case {case}");
            let mut rng = Philox4x32::new(case, 0); let st = m.feasible_init_rand(&mut rng);
            assert_eq!(st.is_some(), nf > 0, "case {case}: start search"); if let Some(x) = &st { assert_eq!(m.violations(x), 0, "case {case}"); }
            if nf == 0 { infeasible += 1; } else if m.weighted() { weighted += 1; }
            if nf == 0 { continue; }
            assert!((e.logz - z.ln()).abs() < 1e-9, "case {case}: log Z {} vs {}", e.logz, z.ln());
            for q in 0..n * k { assert!((e.marg[q] - mg[q] / z).abs() < 1e-9, "case {case}: odds {q}"); }
            if let Some(c) = exact_components_until(&m, 1 << 20, 0, None) { assert!((c.logz - z.ln()).abs() < 1e-9, "case {case}: components");
                for q in 0..n * k { assert!((c.marg[q] - mg[q] / z).abs() < 1e-9, "case {case}: components odds {q}"); } }
            let (cm, _) = m.compiled(); let ce = exact(&cm, 1, 1 << 20).unwrap(); assert!((ce.logz - e.logz).abs() < 1e-9, "case {case}: compiled");
        }
        assert!(infeasible > 10 && weighted > 100, "coverage: {infeasible} infeasible, {weighted} feasible weighted");
    }
    /// R19.7 (the R19.6 c12 finding): four {a, c} variables and eight {b, c}, every value capped at 4 (n = 12): the {a, c} ones
    /// are forced to a BY COUNTING. `forced_values` proves it with the matching, the frozen rule then reports nothing and every
    /// item is releasable; with c capped at 5 nothing is forced, and a moves-off run that never moves still counts as frozen.
    #[test]
    fn forced_by_counting_is_not_frozen() {
        let build = |cap_c: usize| { let (n, k) = (12, 3); let mut allowed = vec![false; n * k];
            for i in 0..n { allowed[i * k + if i < 4 { 0 } else { 1 }] = true; allowed[i * k + 2] = true; }
            let h: Vec<f64> = (0..n * k).map(|q| ((q * 7) % 5) as f64 * 0.1).collect();
            let caps = (0..k).map(|v| Cap::new((0..n).filter(|&i| allowed[i * k + v]).map(|i| (i, v)).collect(), if v == 2 { cap_c } else { 4 })).collect();
            let mut m = Model::new(n, k, h, allowed, vec![None; n], vec![], caps).unwrap(); m.collective = true; m.cycles = true; m.cluster = true; m };
        let m = build(4); assert!(m.is_partition());
        let s = sample(&m, 4, 3000, None, 1, false, false).unwrap();
        let fv = forced_values(&m, &s); for i in 0..12 { assert_eq!(fv[i], if i < 4 { Some(0) } else { None }, "var {i}"); }
        let (fz, esc) = frozen_detail(&m, &s); assert_eq!(fz, 0); assert!(esc.iter().all(|&e| !e));
        let e = exact(&m, 1, 1 << 20).unwrap(); assert_eq!(e.n_feasible, 70);
        let worst = (0..36).map(|q| (s.marg[q] - e.marg[q]).abs()).fold(0.0, f64::max); assert!(worst < 0.03, "{worst}");
        let free = build(5); let s5 = sample(&free, 4, 3000, None, 1, false, false).unwrap(); assert!(forced_values(&free, &s5).iter().all(|f| f.is_none()));
        // a genuinely stuck program stays frozen: allowed {a, c}, {b, a}, {c, b}, one per value, moves off: its two plans differ by
        // a 3-cycle, no site or swap move connects them, and no variable is forced (each takes two values over the plans)
        let al = vec![true, false, true, true, true, false, false, true, true];
        let cyc = Model::new(3, 3, vec![0.0; 9], al.clone(), vec![None; 3], vec![], (0..3).map(|v| Cap::new((0..3).filter(|&i| al[i * 3 + v]).map(|i| (i, v)).collect(), 1)).collect()).unwrap();
        let sc = sample(&cyc, 4, 500, None, 1, false, false).unwrap(); assert!(forced_values(&cyc, &sc).iter().all(|f| f.is_none())); assert!(frozen_detail(&cyc, &sc).0 > 0);
    }
    /// R19.7: the polish's in-chain beta (+ site/swap-only sweeps) reproduces the per-beta scaled Model clones it replaced: same
    /// seeds, streams and fixed sweeps -> the same best log w and plan, on a capped colouring-like program with all moves enabled
    /// on the model, a weighted knapsack and a table-coupled program.
    #[test]
    fn anneal_beta_matches_scaled_clones() {
        let reference = |m: &Model, betas: &[f64], sweeps: usize, chains: usize, seed: u64| -> (f64, Vec<usize>) {
            let qs: Vec<Model> = betas.iter().map(|&b| m.scaled(b)).collect(); let mut out: Option<(f64, Vec<usize>)> = None;
            for c in 0..chains { let mut x = Chain::new(m, seed, c as u64).unwrap().x; let mut best = (m.logw(&x), x.clone());
                for (q, mq) in qs.iter().enumerate() { let mut ch = Chain::from_state(mq, seed ^ 0x5eed, (c * 64 + q) as u64, &x);
                    let n = sweeps * (q + 1) / qs.len() - sweeps * q / qs.len();
                    for _ in 0..n { ch.sweep(); let lw = m.logw(&ch.x); if lw > best.0 { best = (lw, ch.x.clone()); } } x = ch.x.clone(); }
                if out.as_ref().map_or(true, |o| best.0 >= o.0) { out = Some(best); } }
            out.unwrap() };
        let mut r = Philox4x32::new(1907, 3); let betas = [0.5, 1.0, 2.0, 4.0, 8.0];
        let (n, k) = (12, 3); let h: Vec<f64> = (0..n * k).map(|_| r.f64() * 2.0 - 1.0).collect();
        let pairs: Vec<Pair> = (0..n - 1).map(|i| Pair { i, j: i + 1, c: if i % 3 == 0 { Coupling::Table((0..9).map(|_| r.f64() - 0.5).collect()) } else { Coupling::Potts(-0.8) } }).collect();
        let mut col = Model::new(n, k, h, vec![true; n * k], vec![None; n], pairs, (0..k).map(|v| Cap::new((0..n).map(|i| (i, v)).collect(), 5)).collect()).unwrap();
        col.collective = true; col.cluster = true; col.cycles = true;
        let w: Vec<usize> = (0..10).map(|_| 1 + r.below(6)).collect(); let tot: usize = w.iter().sum();
        let knap = Model::new(10, 2, (0..20).map(|q| if q % 2 == 1 { r.f64() } else { 0.0 }).collect(), vec![true; 20], vec![None; 10], vec![], vec![Cap { members: (0..10).map(|i| (i, 1)).collect(), limit: tot / 2, weights: w }]).unwrap();
        for (name, m) in [("colouring", &col), ("knapsack", &knap)] { for seed in 1..4 {
            let got = anneal_sweeps(m, None, &betas, 400, 2, seed).unwrap(); let want = reference(m, &betas, 400, 2, seed);
            assert_eq!(got.0.to_bits(), want.0.to_bits(), "{name} seed {seed}: {} vs {}", got.0, want.0); assert_eq!(got.1, want.1, "{name} seed {seed}"); } }
    }
    /// R19.7 (P2.3): the sign-projection L1 diameter equals the pairwise max TV scan (random marginals, k = 1..6, 2..300 chains).
    #[test]
    fn chain_disagreement_projection_matches_pairs() {
        let mut r = Philox4x32::new(1907, 4);
        for case in 0..60 { let (k, nc, t) = (1 + case % 6, 2 + r.below(300), 1 + r.below(5));
            let c: Vec<Vec<f64>> = (0..nc).map(|_| (0..t).flat_map(|_| { let w: Vec<f64> = (0..k).map(|_| r.f64()).collect(); let z: f64 = w.iter().sum(); w.into_iter().map(move |x| x / z) }).collect()).collect();
            let mut pw: f64 = 0.0; for u in 0..nc { for v in u + 1..nc { pw = pw.max(max_tv(&c[u], &c[v], k)); } }
            let pj = chain_disagreement_proj(&c, k); assert!((pw - pj).abs() < 1e-12, "case {case}: k {k} chains {nc}: {pw} vs {pj}"); }
    }
    /// R19.8 (P2.3): the gate's per-chain passes on the bounded `pool_map` (one thread per chain before) give the same Gate,
    /// bit for bit, for every worker count.
    #[test]
    fn gate_pool_is_bit_identical() {
        assert_eq!(pool_map(7, |i| i * i), vec![0, 1, 4, 9, 16, 25, 36]); assert!(pool_map(0, |i| i).is_empty());
        let m = toy(); let s = sample_on(&m, 37, 2, 300, None, 5, false, 100, 0).unwrap();
        let run = |w: usize| { GATE_THREADS.store(w, std::sync::atomic::Ordering::Relaxed); format!("{:?}", gate_stats(&m, &s)) };
        let one = run(1); for w in [2, 3, 37, 64, 0] { assert_eq!(run(w), one, "workers {w}"); }
    }
    /// R19.7: weighted-cap rules in `Model::new` and the compile pass.
    #[test]
    fn weighted_cap_rules() {
        let mk = |caps: Vec<Cap>| Model::new(2, 2, vec![0.0; 4], vec![true; 4], vec![None; 2], vec![], caps);
        assert!(mk(vec![Cap { members: vec![(0, 1), (1, 1)], limit: 3, weights: vec![2] }]).is_err());
        assert!(mk(vec![Cap { members: vec![(0, 1), (1, 1)], limit: 3, weights: vec![2, 0] }]).is_err());
        assert!(mk(vec![Cap { members: vec![(0, 1), (1, 1)], limit: 3, weights: vec![2, MAX_CAP_WEIGHT + 1] }]).is_err());
        let m = mk(vec![Cap { members: vec![(0, 1), (1, 1)], limit: 3, weights: vec![2, 2] }]).unwrap(); assert!(m.weighted() && !m.is_partition());
        let u = mk(vec![Cap { members: vec![(0, 1), (1, 1)], limit: 1, weights: vec![1, 1] }]).unwrap(); assert!(!u.weighted() && u.is_partition()); // all 1 = unit
        // dropped iff the heaviest member per variable sums to <= limit (here 3 + 2 = 5)
        let c3 = || vec![Cap { members: vec![(0, 1), (0, 0), (1, 1)], limit: 5, weights: vec![2, 3, 2] }];
        assert_eq!(compile_parts(2, &mut [0.0; 4], vec![], c3()).2.caps_dropped, 1);
        let mut c4 = c3(); c4[0].limit = 4; assert_eq!(compile_parts(2, &mut [0.0; 4], vec![], c4).2.caps_dropped, 0);
    }
    /// R19.7: every sampler move (site, swap, global flip, label swap, three-cycle, cluster) keeps weighted caps and the target:
    /// a 10-item knapsack with a pair term and a 6-item x 3-bin packing with Potts / table pairs, all moves on, vs brute force.
    #[test]
    fn weighted_caps_sampler_matches_enumeration() {
        let mut r = Philox4x32::new(1907, 2);
        let n = 10; let w: Vec<usize> = (0..n).map(|_| 1 + r.below(6)).collect(); let tot: usize = w.iter().sum();
        let h: Vec<f64> = (0..2 * n).map(|q| if q % 2 == 1 { r.f64() * 1.5 } else { 0.0 }).collect();
        let knap = Model::new(n, 2, h, vec![true; 2 * n], vec![None; n], vec![Pair { i: 0, j: 1, c: Coupling::Potts(0.8) }], vec![Cap { members: (0..n).map(|i| (i, 1)).collect(), limit: tot * 2 / 5, weights: w }]).unwrap();
        let (n2, k2) = (6, 3); let w2: Vec<usize> = (0..n2).map(|_| 1 + r.below(4)).collect(); let cap = w2.iter().sum::<usize>().div_ceil(3) + 1;
        let caps2: Vec<Cap> = (0..k2).map(|b| Cap { members: (0..n2).map(|i| (i, b)).collect(), limit: cap, weights: w2.clone() }).collect();
        let h2: Vec<f64> = (0..n2 * k2).map(|_| r.f64() - 0.5).collect();
        let pairs2 = vec![Pair { i: 0, j: 1, c: Coupling::Potts(1.0) }, Pair { i: 2, j: 3, c: Coupling::Potts(0.7) }, Pair { i: 4, j: 5, c: Coupling::Table((0..9).map(|q| (q % 4) as f64 * 0.2).collect()) }];
        let bins = Model::new(n2, k2, h2, vec![true; n2 * k2], vec![None; n2], pairs2, caps2).unwrap();
        for (name, mut m) in [("knapsack", knap), ("bins", bins)] { m.collective = true; m.cluster = true; m.cycles = true;
            let (lz, mg) = brute(&m); assert!(lz.is_finite(), "{name}: infeasible");
            let s = sample(&m, 4, 20_000, None, 7, false, false).unwrap();
            let worst = (0..m.n * m.k).map(|q| (s.marg[q] - mg[q]).abs()).fold(0.0, f64::max);
            assert!(worst < 0.02, "{name}: max |odds - exact| = {worst}");
        }
    }
    /// R19 P1.2 (gate/3): the per-variable indicator R-hat is part of the release decision. A run that passes every other check
    /// keeps an item with R-hat < ITEM_RHAT_MAX, escalates one at or above it (reason `item_rhat`), and no whole answer passes
    /// while any item fails it; a gate without per-item R-hats (`gate_stats_with` alone) releases nothing.
    #[test]
    fn item_rhat_is_part_of_the_release_decision() {
        let g = |rt: Vec<f64>| Gate { rhat: 1.0, sig_tv: vec![0.001; 2], sig_tv_max: 0.001, worst_task: 0, min_ess: 1e4, min_batches: 31, chain_dis: 0.0,
            sig_tv_long: vec![0.001; 2], frozen: 0, rhat_task: rt, rhat_occ: vec![], occ_task: vec![], escalate: vec![false; 2], min_batches_long: 10 };
        let bad = g(vec![1.0, f64::INFINITY]); assert!(!bad.diagnostics_passed(&GATE)); assert_eq!(bad.released_tasks(&GATE), vec![true, false]);
        assert_eq!(bad.release_reasons(&GATE), vec!["item_gate", "item_rhat"]);
        let edge = g(vec![1.0, ITEM_RHAT_MAX]); assert!(!edge.diagnostics_passed(&GATE)); assert_eq!(edge.released_tasks(&GATE), vec![true, false]);
        let good = g(vec![1.0, 1.01]); assert!(good.diagnostics_passed(&GATE)); assert_eq!(good.release_reasons(&GATE), vec!["whole_answer_gate"; 2]);
        let none = g(vec![]); assert!(!none.diagnostics_passed(&GATE)); assert_eq!(none.released_tasks(&GATE), vec![false, false]);
    }
    /// R19 P1.2: on real samples, chains trapped at different values of a symmetric ferromagnet (8 spins, all pairs +2.5 nats,
    /// no field, collective moves off: site moves cannot cross the barrier) give those spins an indicator R-hat far above
    /// ITEM_RHAT_MAX, and none of them is released; when every chain sits in one mode the R-hat cannot see it (assumption 1).
    #[test]
    fn item_rhat_sees_chains_split_between_symmetric_modes() {
        let n = 8; let pairs: Vec<Pair> = (0..n).flat_map(|i| (i + 1..n).map(move |j| Pair { i, j, c: Coupling::Potts(2.5) })).collect();
        let m = Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap();
        let mut split = 0;
        for seed in 1..=8 {
            let s = sample(&m, 4, 2000, None, seed, true, false).unwrap(); let g = gate_stats(&m, &s);
            let means: Vec<f64> = s.chain_marg.iter().map(|cm| cm[0]).collect();
            if means.iter().cloned().fold(0.0, f64::max) - means.iter().cloned().fold(1.0, f64::min) > 0.5 { split += 1;
                assert!(g.rhat_task.iter().all(|&r| r > 10.0 * ITEM_RHAT_MAX), "seed {seed}: {:?}", g.rhat_task);
                assert!(!g.diagnostics_passed(&GATE) && g.released_tasks(&GATE).iter().all(|&r| !r), "seed {seed}"); }
        }
        assert!(split >= 3, "only {split} of 8 seeds split the chains");
    }
    /// R19 P1.2: the generic "never moved in any chain" test also runs on partition programs (it returned early there before):
    /// a free variable whose other value carries e^-60 odds never moves, so the run is refused (`frozen` >= 1, nothing released).
    /// Conservative by design: from inside the run the gate cannot tell "certain" from "stuck". A free variable that moves keeps
    /// the run's other checks in charge (frozen 0).
    #[test]
    fn partition_programs_count_never_moved_variables() {
        let mk = |gap: f64| { let mut h = vec![0.0; 3 * 2]; h[0] = gap; h[2] = 0.3; h[5] = -0.2;
            Model::new(3, 2, h, vec![true; 6], vec![None; 3], vec![], vec![Cap { weights: vec![], members: vec![(0, 0), (1, 0), (2, 0)], limit: 2 }, Cap { weights: vec![], members: vec![(0, 1), (1, 1), (2, 1)], limit: 2 }]).unwrap() };
        let m = mk(60.0); assert!(m.is_partition());
        let s = sample(&m, 4, 2000, None, 3, true, false).unwrap(); let g = gate_stats(&m, &s);
        assert!(g.frozen >= 1 && !g.diagnostics_passed(&GATE) && g.released_tasks(&GATE).iter().all(|&r| !r), "frozen {}", g.frozen);
        let m = mk(0.5); let s = sample(&m, 4, 2000, None, 3, true, false).unwrap(); let g = gate_stats(&m, &s);
        assert_eq!(g.frozen, 0, "a moving variable is not stuck");
    }
    /// R19.5: partition programs escalate per connected component (they escalated every variable before). Two independent
    /// quota pools: {0, 1} under one-per-value quotas with an e^-60 value on variable 0 (both never move), {2, 3} free. Only
    /// the stuck pool is escalated; the run still is not a whole answer.
    #[test]
    fn partition_programs_escalate_only_the_stuck_component() {
        let mut h = vec![0.0; 4 * 2]; h[0] = 60.0; h[4] = 0.3; h[7] = -0.2;
        let cap = |a: usize, b: usize, v: usize, limit: usize| Cap { weights: vec![], members: vec![(a, v), (b, v)], limit };
        let m = Model::new(4, 2, h, vec![true; 8], vec![None; 4], vec![], vec![cap(0, 1, 0, 1), cap(0, 1, 1, 1), cap(2, 3, 0, 2), cap(2, 3, 1, 2)]).unwrap();
        assert!(m.is_partition());
        let s = sample(&m, 4, 2000, None, 3, true, false).unwrap(); let (fz, esc) = frozen_detail(&m, &s);
        assert!(fz >= 1); assert_eq!(esc, vec![true, true, false, false]);
        assert!(!gate_stats(&m, &s).diagnostics_passed(&GATE));
    }
    /// R19 P1.2: per-chain mode-transition counters. With the collective moves on, every chain of a symmetric ferromagnet (no
    /// field, where the flip is always accepted) records accepted global flips, the pooled `Samples::moves` has one entry per
    /// chain in chain order, and counting consumes no draws (the samples equal a run with the counters' values ignored: same
    /// seed, same marginals); off, every count is zero.
    #[test]
    fn mode_transition_counters() {
        let n = 6; let pairs: Vec<Pair> = (0..n).flat_map(|i| (i + 1..n).map(move |j| Pair { i, j, c: Coupling::Potts(1.0) })).collect();
        let mut m = Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap();
        let off = sample(&m, 4, 500, None, 7, true, false).unwrap(); assert_eq!(off.moves, vec![[0, 0]; 4]);
        m.collective = true; let on = sample(&m, 4, 500, None, 7, true, false).unwrap(); assert_eq!(on.moves.len(), 4);
        // h = 0: the flip's dlogw is 0, so it is accepted on every sweep (burn-in included)
        assert!(on.moves.iter().all(|c| c[0] as usize * 4 == on.sweeps && c[1] == 0), "{:?} sweeps {}", on.moves, on.sweeps);
        let again = sample_on(&m, 4, 1, 500, None, 7, false, 100, 0).unwrap(); assert_eq!(again.moves, on.moves);
        assert!(again.marg.iter().zip(&on.marg).all(|(a, b)| a.to_bits() == b.to_bits()));
    }
    /// R19 P1.2: the occupancy-count R-hat sees a macroscopic split the indicator R-hat cannot. 16 two-value variables under one
    /// non-binding cap on value 1; chain A holds exactly 7 ones per row, chain B exactly 9 (rotating, so every indicator mixes:
    /// half-chain means 7/16 vs 9/16, indicator R-hat < 1). The cap's load is constant within each chain and differs between them:
    /// occupancy R-hat infinite, so every member is escalated (`item_rhat`) and no whole answer passes.
    #[test]
    fn occupancy_rhat_sees_count_modes_the_indicators_miss() {
        let n = 16; let m = Model::new(n, 2, vec![0.0; 2 * n], vec![true; 2 * n], vec![None; n], vec![], vec![Cap { weights: vec![], members: (0..n).map(|i| (i, 1)).collect(), limit: n }]).unwrap();
        let chain = |ones: usize| -> Vec<u16> { (0..64).flat_map(|r| (0..n).map(move |i| ((i + n - r % n) % n < ones) as u16)).collect() };
        let traj = vec![chain(7), chain(9)]; let mut marg = vec![0.0; 2 * n];
        for tr in &traj { for row in tr.chunks(n) { for i in 0..n { marg[i * 2 + row[i] as usize] += 1.0 / 128.0; } } }
        let s = Samples { chain_marg: vec![], traj, marg, n: 128, plans: HashMap::new(), trace: vec![vec![0.0; 64]; 2], viol: 0, best: (0.0, vec![]), sweeps: 128, moves: vec![] };
        let rt = rhat_tasks(&m, &s); assert!(rt.iter().all(|&r| r < ITEM_RHAT_MAX), "indicators should be blind here: {rt:?}");
        let ro = rhat_occupancy(&m, &s); assert_eq!(ro.len(), 1); assert!(ro[0].is_infinite(), "{ro:?}");
        let g = gate_stats(&m, &s); assert!((0..n).all(|i| !g.item_rhat_ok(i)) && !g.diagnostics_passed(&GATE) && g.released_tasks(&GATE).iter().all(|&r| !r));
        // both chains at 8 ones: the load never varies, occupancy R-hat 1, the members are not escalated by it
        let traj = vec![chain(8), chain(8)]; let s2 = Samples { traj, ..s }; assert_eq!(rhat_occupancy(&m, &s2), vec![1.0]);
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
            let mut s = Samples { chain_marg: vec![], traj: vec![vec![]], marg: vec![], n: 0, plans: HashMap::new(), trace: vec![vec![]], viol: 0, best: (0.0, vec![]), sweeps: 0, moves: vec![] };
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
            if g.diagnostics_passed(&GATE) { certs += 1; assert!(mx <= 0.05, "false certificate: beta {beta} sweeps {sweeps} maxTV {mx}"); }
            for (i, ok) in g.released_tasks(&GATE).into_iter().enumerate() { assert!(!ok || (s.marg[2 * i] - 0.5).abs() <= 0.05); }
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
        let g = gate_stats(&m, &s); assert!(g.diagnostics_passed(&GATE), "bound {} rhat {}", g.tv_bound(&GATE), g.rhat);
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
            let mut caps: Vec<Cap> = (0..3).map(|_| Cap { weights: vec![], members: vec![], limit: 1 + r.below(3) }).collect();
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
    /// R19 P1.1(a) (WIP, off by default): one chain on a 6-spin complete-graph ferromagnet (Potts 2: an 18-nat barrier between
    /// the two mirror modes; small fields; one redundant cap) stays in its mode with site + swap moves, and samples the exact
    /// marginals once the global two-value flip is on.
    #[test]
    fn global_flip_samples_exact_marginals_on_mirror_modes() {
        let n = 6; let pairs = (0..n).flat_map(|i| (i + 1..n).map(move |j| Pair { i, j, c: Coupling::Potts(2.0) })).collect();
        let h: Vec<f64> = (0..n).flat_map(|i| [0.0, 0.05 * i as f64]).collect();
        let caps = vec![Cap { weights: vec![], members: (0..n).map(|i| (i, 0)).collect(), limit: n }];
        let mut m = Model::new(n, 2, h, vec![true; 2 * n], vec![None; n], pairs, caps).unwrap();
        let e = exact(&m, 1, 1 << 20).unwrap();
        let off = sample(&m, 1, 20000, None, 3, false, false).unwrap(); m.collective = true;
        let on = sample(&m, 1, 20000, None, 3, false, false).unwrap();
        let (tv_off, tv_on) = (max_tv(&off.marg, &e.marg, 2), max_tv(&on.marg, &e.marg, 2));
        assert!(tv_off > 0.2, "site + swap moves alone should stay in one mode: max TV {tv_off}");
        assert!(tv_on < 0.03, "with the global flip: max TV {tv_on}");
        // The external review's ferro12 shape (12 spins, complete graph, Potts 10, truth 1/2 per spin by symmetry), 4 chains x 4000 sweeps,
        // seeds 1..20: runs whose pooled max TV exceeds the gate's 0.05 tolerance, flip off vs on
        let pairs: Vec<Pair> = (0..12).flat_map(|i| (i + 1..12).map(move |j| Pair { i, j, c: Coupling::Potts(10.0) })).collect();
        let mut f = Model::new(12, 2, vec![0.0; 24], vec![true; 24], vec![None; 12], pairs, vec![]).unwrap();
        let bad = |f: &Model| (1..=20u64).filter(|&sd| { let s = sample(f, 4, 4000, None, sd, false, false).unwrap(); s.marg.iter().any(|p| (p - 0.5).abs() > 0.05) }).count();
        let off12 = bad(&f); f.collective = true; let on12 = bad(&f);
        eprintln!("global flip: 6-spin max TV off {tv_off:.4} on {tv_on:.4}; ferro12 runs off by > 0.05: {off12}/20 off, {on12}/20 on");
        assert_eq!(on12, 0, "ferro12 with the global flip: {on12}/20 runs off by > 0.05");
    }
    /// R19 P1.1(a): the global flip's incremental log-weight change and capacity check equal the full recomputation
    /// (logw(y) - logw(x), violations(y) == 0) on random programs mixing Potts / table pairs, two- and three-candidate
    /// variables, clamps and binding caps; and with the flip on, a capped mixed program still samples its exact marginals.
    #[test]
    fn global_flip_delta_matches_full_recomputation() {
        let (mut checked, mut infeasible) = (0, 0);
        for seed in 0..300u64 {
            let mut r = Philox4x32::new(seed, 11); let (n, k) = (4 + r.below(5), 2 + r.below(2));
            let h: Vec<f64> = (0..n * k).map(|_| r.f64() * 2.0 - 1.0).collect();
            let mut allowed: Vec<bool> = (0..n * k).map(|_| r.f64() < 0.7).collect(); for i in 0..n { allowed[i * k + r.below(k)] = true; }
            let clamp: Vec<Option<usize>> = (0..n).map(|i| if r.f64() < 0.1 { (0..k).find(|&v| allowed[i * k + v]) } else { None }).collect();
            let mut pairs = vec![]; for _ in 0..r.below(2 * n) { let (i, j) = (r.below(n), r.below(n)); if i == j { continue; }
                pairs.push(Pair { i, j, c: if r.f64() < 0.5 { Coupling::Potts(r.f64() * 2.0 - 1.0) } else { Coupling::Table((0..k * k).map(|_| r.f64() - 0.5).collect()) } }); }
            let caps = (0..r.below(3)).map(|_| { let mut mem: Vec<(usize, usize)> = (0..n * k).filter(|_| r.f64() < 0.3).map(|q| (q / k, q % k)).collect(); mem.dedup();
                Cap { weights: vec![], limit: (mem.len() / 2).max(1), members: mem } }).collect();
            let Ok(mut m) = Model::new(n, k, h, allowed, clamp, pairs, caps) else { continue }; m.collective = true;
            let Some(mut c) = Chain::new(&m, seed, 0) else { continue };
            for _ in 0..20 { c.sweep();
                let mut f = c.flip_plan(); let x = c.x.clone();
                let y: Vec<usize> = (0..n).map(|i| if m.cand_count(i) == 2 { let v = &m.cand[i]; if x[i] == v[0] { v[1] } else { v[0] } } else { x[i] }).collect();
                match c.flip_delta(&mut f) {
                    None => { assert!(m.violations(&y) > 0, "seed {seed}: flip refused but feasible"); infeasible += 1; }
                    Some(d) => { assert_eq!(m.violations(&y), 0, "seed {seed}: flip accepted but infeasible");
                        let full = m.logw(&y) - m.logw(&x); assert!((d - full).abs() < 1e-9, "seed {seed}: incremental {d} vs full {full}"); checked += 1; } }
            }
        }
        assert!(checked > 2000 && infeasible > 50, "coverage: {checked} feasible, {infeasible} capacity-refused");
        // detailed balance end to end: three values, two-candidate variables over different value pairs, a binding cap
        let (n, k) = (6, 3); let mut allowed = vec![true; n * k]; allowed[2] = false; allowed[1 * k + 2] = false; allowed[2 * k] = false; allowed[3 * k + 1] = false;
        let pairs = vec![Pair { i: 0, j: 1, c: Coupling::Potts(1.5) }, Pair { i: 1, j: 2, c: Coupling::Table(vec![0.3, -0.2, 0.1, 0.0, 0.5, -0.4, 0.2, 0.0, -0.1]) },
            Pair { i: 2, j: 3, c: Coupling::Potts(-0.7) }, Pair { i: 4, j: 5, c: Coupling::Potts(0.9) }, Pair { i: 0, j: 5, c: Coupling::Potts(0.6) }];
        let h: Vec<f64> = (0..n * k).map(|q| 0.2 * ((q * 7 % 5) as f64 - 2.0)).collect();
        let caps = vec![Cap { weights: vec![], members: (0..n).map(|i| (i, 1)).collect(), limit: 3 }];
        let mut m = Model::new(n, k, h, allowed, vec![None; n], pairs, caps).unwrap(); m.collective = true;
        let e = exact(&m, 1, 1 << 20).unwrap(); let s = sample(&m, 4, 40000, None, 9, false, false).unwrap();
        let tv = max_tv(&s.marg, &e.marg, k); eprintln!("global flip delta: {checked} checked, {infeasible} refused; capped 3-value program max TV {tv:.4}");
        assert!(tv < 0.02, "capped mixed program with the flip on: max TV {tv}");
    }
    /// R19 P1.1(b): the label swap. A 3-value Potts ferromagnet on a complete graph has three label-permuted modes: site,
    /// swap and two-value flip moves stay in one, the relabel move samples the exact marginals. Random capped programs (k = 3, 4;
    /// Potts + table pairs, partial domains, clamps) keep their exact marginals with every collective move on (detailed balance).
    #[test]
    fn relabel_samples_exact_marginals() {
        let n = 6; let pairs = (0..n).flat_map(|i| (i + 1..n).map(move |j| Pair { i, j, c: Coupling::Potts(2.0) })).collect();
        let h: Vec<f64> = (0..n * 3).map(|q| 0.03 * (q % 3) as f64 * (q / 3) as f64).collect();
        let mut m = Model::new(n, 3, h, vec![true; 3 * n], vec![None; n], pairs, vec![]).unwrap();
        let e = exact(&m, 1, 1 << 20).unwrap();
        let off = sample(&m, 1, 20000, None, 3, false, false).unwrap(); m.collective = true;
        let on = sample(&m, 1, 20000, None, 3, false, false).unwrap();
        let (tv_off, tv_on) = (max_tv(&off.marg, &e.marg, 3), max_tv(&on.marg, &e.marg, 3));
        assert!(tv_off > 0.3, "site + swap moves alone should stay in one mode: max TV {tv_off}");
        assert!(tv_on < 0.03, "with the relabel move: max TV {tv_on}");
        let mut worst: f64 = 0.0; let mut tested = 0;
        for seed in 0..12u64 {
            let mut r = Philox4x32::new(seed, 13); let (n, k) = (4 + r.below(3), 3 + r.below(2));
            let h: Vec<f64> = (0..n * k).map(|_| (r.f64() - 0.5) * 0.6).collect();
            let mut allowed: Vec<bool> = (0..n * k).map(|_| r.f64() < 0.8).collect(); for i in 0..n { allowed[i * k + r.below(k)] = true; }
            let clamp: Vec<Option<usize>> = (0..n).map(|i| if r.f64() < 0.1 { (0..k).find(|&v| allowed[i * k + v]) } else { None }).collect();
            let mut pairs = vec![]; for _ in 0..n + r.below(n) { let (i, j) = (r.below(n), r.below(n)); if i == j { continue; }
                pairs.push(Pair { i, j, c: if r.f64() < 0.5 { Coupling::Potts(r.f64() * 3.0 - 1.0) } else { Coupling::Table((0..k * k).map(|_| r.f64() - 0.5).collect()) } }); }
            let caps = (0..1 + r.below(2)).map(|_| { let v = r.below(k); Cap { weights: vec![], members: (0..n).filter(|&i| allowed[i * k + v]).map(|i| (i, v)).collect(), limit: (n / 2).max(1) } }).collect();
            let Ok(mut m) = Model::new(n, k, h, allowed, clamp, pairs, caps) else { continue }; m.collective = true;
            let Some(e) = exact(&m, 1, 1 << 20) else { continue }; if e.n_feasible == 0 { continue; }
            let s = sample(&m, 4, 20000, None, seed, false, false).unwrap(); let tv = max_tv(&s.marg, &e.marg, k);
            worst = worst.max(tv); tested += 1; assert!(tv < 0.03, "seed {seed} (n {n}, k {k}): max TV {tv}");
        }
        eprintln!("relabel: 3-value ferro max TV off {tv_off:.4} on {tv_on:.4}; {tested} random capped programs, worst max TV {worst:.4}");
        assert!(tested >= 8);
    }
    /// R19 P1.1(d): the Wolff cluster move keeps the exact marginals on random programs mixing positive and negative Potts
    /// bonds, tables, fields, partial domains, clamps and caps (k = 2..4), alone (collective off) and with every move on; and it
    /// fires (a 6x6 Potts-2.5 grid's run differs from the run without it).
    #[test]
    fn cluster_move_samples_exact_marginals() {
        let (mut worst, mut tested): (f64, usize) = (0.0, 0);
        for seed in 0..16u64 {
            let mut r = Philox4x32::new(seed, 17); let (n, k) = (4 + r.below(4), 2 + r.below(3));
            let h: Vec<f64> = (0..n * k).map(|_| (r.f64() - 0.5) * 0.8).collect();
            let mut allowed: Vec<bool> = (0..n * k).map(|_| r.f64() < 0.85).collect(); for i in 0..n { allowed[i * k + r.below(k)] = true; }
            let clamp: Vec<Option<usize>> = (0..n).map(|i| if r.f64() < 0.1 { (0..k).find(|&v| allowed[i * k + v]) } else { None }).collect();
            let mut pairs = vec![]; for _ in 0..n + r.below(2 * n) { let (i, j) = (r.below(n), r.below(n)); if i == j { continue; }
                pairs.push(Pair { i, j, c: if r.f64() < 0.75 { Coupling::Potts(r.f64() * 3.0 - 0.5) } else { Coupling::Table((0..k * k).map(|_| r.f64() - 0.5).collect()) } }); }
            let caps = (0..r.below(3)).map(|_| { let v = r.below(k); Cap { weights: vec![], members: (0..n).filter(|&i| allowed[i * k + v]).map(|i| (i, v)).collect(), limit: (n / 2).max(1) } }).collect();
            let Ok(mut m) = Model::new(n, k, h, allowed, clamp, pairs, caps) else { continue }; m.cluster = true;
            let Some(e) = exact(&m, 1, 1 << 20) else { continue }; if e.n_feasible == 0 { continue; }
            for coll in [false, true] { m.collective = coll;
                let s = sample(&m, 4, 20000, None, seed, false, false).unwrap(); let tv = max_tv(&s.marg, &e.marg, k);
                worst = worst.max(tv); tested += 1; assert!(tv < 0.03, "seed {seed} (n {n}, k {k}, collective {coll}): max TV {tv}"); }
        }
        let g = 6; let pairs: Vec<Pair> = (0..g * g).flat_map(|q| { let (r, c) = (q / g, q % g); let mut v = vec![];
            if c + 1 < g { v.push(Pair { i: q, j: q + 1, c: Coupling::Potts(2.5) }); } if r + 1 < g { v.push(Pair { i: q, j: q + g, c: Coupling::Potts(2.5) }); } v }).collect();
        let mut m = Model::new(g * g, 3, (0..3 * g * g).map(|q| 0.02 * (q % 3) as f64).collect(), vec![true; 3 * g * g], vec![None; g * g], pairs, vec![]).unwrap();
        let a = sample(&m, 1, 200, None, 1, false, false).unwrap(); m.cluster = true; let b = sample(&m, 1, 200, None, 1, false, false).unwrap();
        assert_ne!(a.marg, b.marg, "the cluster move never fired");
        eprintln!("cluster move: {tested} random program runs, worst max TV {worst:.4}");
        assert!(tested >= 20);
    }
    /// R19 P1.1(c): the three-cycle rotation. Three variables with allowed sets {0,2}, {1,0}, {2,1} under one-per-value quotas
    /// have two feasible states that no site or swap move connects; with the rotation on, the sampled marginals are exact. Random
    /// saturated programs (quotas sum to n) keep their exact marginals with the rotation on.
    #[test]
    fn cycle3_connects_swap_disconnected_states() {
        let mut allowed = vec![false; 9]; for (i, vs) in [[0, 2], [1, 0], [2, 1]].iter().enumerate() { for &v in vs { allowed[i * 3 + v] = true; } }
        let caps = (0..3).map(|v| Cap { weights: vec![], members: (0..3).filter(|&i| allowed[i * 3 + v]).map(|i| (i, v)).collect(), limit: 1 }).collect();
        let mut m = Model::new(3, 3, vec![0.3, 0.0, -0.2, 0.0, 0.1, 0.0, 0.0, 0.4, 0.2], allowed, vec![None; 3], vec![], caps).unwrap();
        let e = exact(&m, 1, 1 << 10).unwrap(); assert_eq!(e.n_feasible, 2);
        let tv_off = (1..=8u64).map(|sd| max_tv(&sample(&m, 1, 4000, None, sd, false, false).unwrap().marg, &e.marg, 3)).fold(0.0, f64::max);
        m.cycles = true; let tv_on = max_tv(&sample(&m, 4, 20000, None, 3, false, false).unwrap().marg, &e.marg, 3);
        assert!(tv_off > 0.2, "without the rotation every chain stays in its start: max TV {tv_off}");
        assert!(tv_on < 0.02, "with the rotation: max TV {tv_on}");
        let mut worst: f64 = 0.0; let mut tested = 0;
        for seed in 0..12u64 {
            let mut r = Philox4x32::new(seed, 19); let (n, k) = (4 + r.below(3), 3);
            let h: Vec<f64> = (0..n * k).map(|_| (r.f64() - 0.5) * 0.8).collect();
            let mut allowed: Vec<bool> = (0..n * k).map(|_| r.f64() < 0.75).collect(); for i in 0..n { allowed[i * k + r.below(k)] = true; }
            let mut pairs = vec![]; for _ in 0..r.below(n) { let (i, j) = (r.below(n), r.below(n)); if i != j { pairs.push(Pair { i, j, c: Coupling::Table((0..k * k).map(|_| r.f64() - 0.5).collect()) }); } }
            let lim = [n / 3 + 1, n / 3 + 1, n - 2 * (n / 3 + 1)];
            let caps = (0..k).map(|v| Cap { weights: vec![], members: (0..n).filter(|&i| allowed[i * k + v]).map(|i| (i, v)).collect(), limit: lim[v].max(1) }).collect();
            let Ok(mut m) = Model::new(n, k, h, allowed, vec![None; n], pairs, caps) else { continue }; m.cycles = true;
            let Some(e) = exact(&m, 1, 1 << 20) else { continue }; if e.n_feasible < 2 { continue; }
            let s = sample(&m, 4, 20000, None, seed, false, false).unwrap(); let tv = max_tv(&s.marg, &e.marg, k);
            worst = worst.max(tv); tested += 1; assert!(tv < 0.03, "seed {seed}: max TV {tv}");
        }
        eprintln!("cycle3: swap-disconnected pair max TV off {tv_off:.4} on {tv_on:.4}; {tested} saturated programs worst {worst:.4}");
        assert!(tested >= 6);
    }
    /// R19.5: the rotation is attempted only with k > 2 values AND a cap. A two-value capped program and a three-value cap-free
    /// program sample the SAME chain (identical marginals and best plan, fixed sweeps) with `cycles` on and off; a three-value
    /// capped program does not (the move runs and draws).
    #[test]
    fn cycle3_is_skipped_without_three_values_and_a_cap() {
        let run = |k: usize, capped: bool, cycles: bool| {
            let n = 9; let mut r = Philox4x32::new(5, 23);
            let h: Vec<f64> = (0..n * k).map(|_| r.f64() - 0.5).collect();
            let pairs = (0..n - 1).map(|i| Pair { i, j: i + 1, c: Coupling::Potts(0.4) }).collect();
            let caps = if capped { vec![Cap { weights: vec![], members: (0..n).map(|i| (i, 0)).collect(), limit: 4 }] } else { vec![] };
            let mut m = Model::new(n, k, h, vec![true; n * k], vec![None; n], pairs, caps).unwrap(); m.cycles = cycles;
            let s = sample(&m, 2, 500, None, 9, false, false).unwrap(); (s.marg, s.best.1)
        };
        assert_eq!(run(2, true, true), run(2, true, false), "two values: no rotation");
        assert_eq!(run(3, false, true), run(3, false, false), "no cap: no rotation");
        assert_ne!(run(3, true, true), run(3, true, false), "three values + a cap: the rotation runs");
    }
    /// R19 P0.2: the frontier DP is exact in log space for weight spreads far beyond exp's range. With linear weights scaled
    /// per layer, states > ~745 nats below a layer's max underflowed, and a layer of all-underflowed states gave NaN marginals
    /// labelled exact (the fuzz test's router cases with scores of 1e9 then aborted the CLI). Same generator as above with
    /// every weight scaled by 1e3 / 1e6 / 1e9 (the CLI's weight limit): finite, and equal to enumeration.
    #[test]
    fn frontier_is_exact_in_log_space_for_huge_weight_spreads() {
        let mut tested = 0;
        for &scale in &[1e3, 1e6, 1e9] { for seed in 0..20u64 {
            let mut r = Philox4x32::new(seed, 7); let (n, k) = (6 + r.below(4), 2 + r.below(2));
            let h: Vec<f64> = (0..n * k).map(|_| (r.f64() * 2.0 - 1.0) * scale).collect();
            let mut allowed: Vec<bool> = (0..n * k).map(|_| r.f64() < 0.85).collect(); for i in 0..n { allowed[i * k + r.below(k)] = true; }
            let mut pairs = vec![]; for _ in 0..r.below(n) { let (i, j) = (r.below(n), r.below(n)); if i == j { continue; }
                pairs.push(Pair { i, j, c: if r.f64() < 0.5 { Coupling::Potts((r.f64() * 2.0 - 1.0) * scale) } else { Coupling::Table((0..k * k).map(|_| (r.f64() - 0.5) * scale).collect()) } }); }
            let mut caps: Vec<Cap> = (0..3).map(|_| Cap { weights: vec![], members: vec![], limit: 1 + r.below(3) }).collect();
            for i in 0..n { for v in 0..k { let c = r.below(5); if c < 3 { caps[c].members.push((i, v)); } } }
            let m = Model::new(n, k, h, allowed, vec![None; n], pairs, caps).unwrap();
            let e = exact(&m, 1, 1 << 22).unwrap(); let f = exact_frontier(&m, FRONTIER_MAX_STATES);
            if e.n_feasible == 0 { assert!(f.is_none()); continue; }
            let Some(f) = f else { continue }; tested += 1;
            assert!(f.marg.iter().all(|x| x.is_finite()) && f.logz.is_finite(), "scale {scale} seed {seed}: non-finite frontier answer");
            let tol = 1e-12 * (1.0 + e.logz.abs());
            assert!((f.logz - e.logz).abs() < tol, "scale {scale} seed {seed}: logz {} vs {}", f.logz, e.logz);
            assert!(max_tv(&f.marg, &e.marg, m.k) < 1e-9, "scale {scale} seed {seed}: marginals differ by {}", max_tv(&f.marg, &e.marg, m.k));
            assert!(m.violations(&f.map) == 0 && (f.map_logw - e.top[0].0.ln() - e.logz).abs() < tol, "scale {scale} seed {seed}: MAP");
        } }
        assert!(tested >= 40, "frontier declined too often: {tested}/60");
    }
    /// Non-partition programs enumerate with dynamic MRV. Empty 4x4 sudoku (shidoku): 288 solutions, every odds
    /// 1/4; one given (cell 0 = value 0) leaves 72, and the other cells of its row/column/box can no longer take value 0.
    #[test]
    fn mrv_enumeration_counts_shidoku() {
        let mut units: Vec<Vec<usize>> = (0..4).map(|r| (0..4).map(|c| r * 4 + c).collect()).collect();
        units.extend((0..4).map(|c| (0..4).map(|r| r * 4 + c).collect::<Vec<_>>())); units.extend((0..4).map(|b| (0..4).map(|q| (b / 2 * 2 + q / 2) * 4 + b % 2 * 2 + q % 2).collect::<Vec<_>>()));
        let caps: Vec<Cap> = units.iter().flat_map(|u| (0..4).map(move |d| Cap { weights: vec![], members: u.iter().map(|&c| (c, d)).collect(), limit: 1 })).collect();
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
        let caps: Vec<Cap> = e.iter().flat_map(|&(a, b)| (0..k).map(move |c| Cap { weights: vec![], members: vec![(a, c), (b, c)], limit: 1 })).collect();
        let h: Vec<f64> = (0..n * k).map(|q| 0.3 * ((q * 7 % 5) as f64 - 2.0)).collect(); let mut clamp = vec![None; n]; clamp[8] = Some(0);
        let m = Model::new(n, k, h, vec![true; n * k], clamp, vec![], caps).unwrap(); assert!(!m.is_partition());
        let ex = exact(&m, 1, 1 << 20).unwrap(); let s = sample(&m, 4, 4000, None, 5, true, false).unwrap(); let g = gate_stats(&m, &s);
        assert!(g.frozen == 0 && g.diagnostics_passed(&GATE), "frozen {} bound {}", g.frozen, g.tv_bound(&GATE)); assert!(max_tv(&s.marg, &ex.marg, k) <= 0.05);
        let mut units: Vec<Vec<usize>> = (0..4).map(|r| (0..4).map(|c| r * 4 + c).collect()).collect();
        units.extend((0..4).map(|c| (0..4).map(|r| r * 4 + c).collect::<Vec<_>>())); units.extend((0..4).map(|b| (0..4).map(|q| (b / 2 * 2 + q / 2) * 4 + b % 2 * 2 + q % 2).collect::<Vec<_>>()));
        let caps: Vec<Cap> = units.iter().flat_map(|u| (0..4).map(move |d| Cap { weights: vec![], members: u.iter().map(|&c| (c, d)).collect(), limit: 1 })).collect();
        let sol = [0, 1, 2, 3, 2, 3, 0, 1, 1, 0, 3, 2, 3, 2, 1, 0]; let clamp: Vec<Option<usize>> = (0..16).map(|q| if q % 3 == 0 || q == 5 || q == 10 { Some(sol[q]) } else { None }).collect();
        let m = Model::new(16, 4, vec![0.0; 64], vec![true; 64], clamp, vec![], caps).unwrap(); assert_eq!(exact(&m, 1, 1 << 20).unwrap().n_feasible, 2); // 2 solutions no local move connects
        let s = sample(&m, 4, 2000, None, 5, true, false).unwrap(); let g = gate_stats(&m, &s);
        assert!(g.frozen > 0 && !g.diagnostics_passed(&GATE) && g.released_tasks(&GATE).iter().all(|&r| !r), "a chain that cannot move must not certify");
    }
    /// The frozen rule escalates per connected component of the free variables. Next to the stuck 2-solution shidoku
    /// above: variable 16 (no links) is released within 0.05 of the exact odds; 17 (Potts pair to a cell where the two
    /// solutions differ) and 18 (shares a cap with that cell) are escalated, and so are the givens while anything is stuck.
    #[test]
    fn stuck_component_escalates_only_its_component() {
        let mut units: Vec<Vec<usize>> = (0..4).map(|r| (0..4).map(|c| r * 4 + c).collect()).collect();
        units.extend((0..4).map(|c| (0..4).map(|r| r * 4 + c).collect::<Vec<_>>())); units.extend((0..4).map(|b| (0..4).map(|q| (b / 2 * 2 + q / 2) * 4 + b % 2 * 2 + q % 2).collect::<Vec<_>>()));
        let mut caps: Vec<Cap> = units.iter().flat_map(|u| (0..4).map(move |d| Cap { weights: vec![], members: u.iter().map(|&c| (c, d)).collect(), limit: 1 })).collect();
        let sol = [0, 1, 2, 3, 2, 3, 0, 1, 1, 0, 3, 2, 3, 2, 1, 0]; let mut clamp: Vec<Option<usize>> = (0..16).map(|q| if q % 3 == 0 || q == 5 || q == 10 { Some(sol[q]) } else { None }).collect();
        let two = exact(&Model::new(16, 4, vec![0.0; 64], vec![true; 64], clamp.clone(), vec![], caps.clone()).unwrap(), 2, 1 << 20).unwrap(); assert_eq!(two.n_feasible, 2);
        let c = (0..16).find(|&q| two.top[0].1[q] != two.top[1].1[q]).unwrap(); let v0 = two.top[0].1[c];
        caps.push(Cap { weights: vec![], members: vec![(c, v0), (18, v0)], limit: 1 }); clamp.extend([None, None, None]);
        let mut h = vec![0.0; 19 * 4]; for v in 0..4 { h[16 * 4 + v] = 0.4 * v as f64 - 0.5; }
        let m = Model::new(19, 4, h, vec![true; 76], clamp, vec![Pair { i: 17, j: c, c: Coupling::Potts(2.0) }], caps).unwrap(); assert!(!m.is_partition());
        let ex = exact(&m, 1, 1 << 20).unwrap(); let s = sample(&m, 4, 4000, None, 5, true, false).unwrap(); let g = gate_stats(&m, &s); let rel = g.released_tasks(&GATE);
        let tv = |i: usize| 0.5 * (0..4).map(|v| (s.marg[i * 4 + v] - ex.marg[i * 4 + v]).abs()).sum::<f64>();
        println!("frozen {} rhat {:.5} tv 16/17/18 {:.4} {:.4} {:.4}", g.frozen, g.rhat, tv(16), tv(17), tv(18));
        assert!(g.frozen > 0 && !g.diagnostics_passed(&GATE));
        assert!(rel[16] && tv(16) <= 0.05, "unlinked variable: released {} tv {}", rel[16], tv(16));
        assert!((0..16).chain([17, 18]).all(|i| !rel[i]), "stuck, linked and given variables must be escalated: {rel:?}");
    }
    /// A variable the clamps force through unit propagation is a constant, not a stuck chain. 3-colouring: 0 and 1
    /// clamped to colours 0 / 1, vertex 2 (neighbours 0, 1) is forced to 2, then vertex 3 (neighbours 0, 2) to 1; 4 and 5 mix.
    /// Before unit propagation, vertices 2 and 3 counted as stuck and the whole answer was refused.
    #[test]
    fn propagation_forced_variables_are_not_stuck() {
        let e = [(0, 2), (1, 2), (0, 3), (2, 3), (3, 4), (4, 5), (2, 5)]; let k = 3;
        let caps: Vec<Cap> = e.iter().flat_map(|&(a, b)| (0..k).map(move |c| Cap { weights: vec![], members: vec![(a, c), (b, c)], limit: 1 })).collect();
        let h: Vec<f64> = (0..6 * k).map(|q| 0.25 * ((q * 5 % 7) as f64 - 3.0)).collect(); let mut clamp = vec![None; 6]; clamp[0] = Some(0); clamp[1] = Some(1);
        let m = Model::new(6, k, h, vec![true; 6 * k], clamp, vec![], caps).unwrap();
        let ex = exact(&m, 1, 1 << 20).unwrap(); assert!(ex.marg[2 * k + 2] == 1.0 && ex.marg[3 * k + 1] == 1.0);
        let s = sample(&m, 4, 4000, None, 3, true, false).unwrap(); let g = gate_stats(&m, &s);
        assert!(g.frozen == 0 && g.diagnostics_passed(&GATE), "frozen {} rhat {} bound {}", g.frozen, g.rhat, g.tv_bound(&GATE)); assert!(max_tv(&s.marg, &ex.marg, k) <= 0.05);
    }
    /// Chain threads get a stack sized to the program. On the default 2 MiB stack the feasible-start search (one frame
    /// per variable) aborted the process on this 8,000-variable 2-colouring path. Unit propagation forces every vertex from the
    /// clamp through a worklist (the pass loop it replaced was O(n x caps) here: 234 ms, now ~5 ms).
    #[test]
    fn long_forced_path_samples_without_stack_overflow() {
        let (n, k) = (8000, 2); let caps: Vec<Cap> = (0..n - 1).flat_map(|i| (0..k).map(move |c| Cap { weights: vec![], members: vec![(i, c), (i + 1, c)], limit: 1 })).collect();
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
        let mut caps: Vec<Cap> = (0..t).map(|s| Cap { weights: vec![], members: (0..n).filter(|&i| allowed[i * t + s]).map(|i| (i, s)).collect(), limit: cap }).collect();
        for &(i, j) in &prec { for a in 0..t { for b in 0..=a { if allowed[i * t + a] && allowed[j * t + b] { caps.push(Cap { weights: vec![], members: vec![(i, a), (j, b)], limit: 1 }); } } } }
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
        let mut caps: Vec<Cap> = (0..t).map(|s| Cap { weights: vec![], members: (0..n).map(|i| (i, s)).collect(), limit: 2 }).collect();
        for &(i, j) in &[(0usize, 3usize), (1, 4), (3, 5)] { for a in 0..t { for b in 0..=a { caps.push(Cap { weights: vec![], members: vec![(i, a), (j, b)], limit: 1 }); } } }
        let m = Model::new(n, t, vec![0.0; n * t], allowed, vec![None; n], vec![], caps).unwrap(); assert!(!m.is_partition());
        for seed in 0..20u64 {
            let a = m.feasible_init_rand(&mut Philox4x32::new(seed, 1)); let b = m.feasible_init_with(&mut Philox4x32::new(seed, 1), u64::MAX); assert_eq!(a, b);
            let r = m.feasible_init_with(&mut Philox4x32::new(seed, 1), 0).expect("the ascending retry finds a start"); assert_eq!(m.violations(&r), 0);
        }
        let g = { let mut c = vec![]; for i in 0..6 { for j in i + 1..6 { if (i + j) % 3 != 0 { for v in 0..3 { c.push(Cap { weights: vec![], members: vec![(i, v), (j, v)], limit: 1 }); } } } } c };
        let col = Model::new(6, 3, vec![0.0; 18], vec![true; 18], vec![None; 6], vec![], g).unwrap();
        for seed in 0..20u64 { assert_eq!(col.feasible_init_rand(&mut Philox4x32::new(seed, 2)), col.feasible_init_with(&mut Philox4x32::new(seed, 2), u64::MAX)); }
    }
    /// R19.6 (P2.1): `Model::start` is used only when feasible (else the stream searches as before), and a what-if clamp
    /// that contradicts it drops it.
    #[test]
    fn warm_start_is_used_only_when_feasible() {
        let caps: Vec<Cap> = (0..3).map(|s| Cap { weights: vec![], members: (0..3).map(|i| (i, s)).collect(), limit: 1 }).collect();
        let mut m = Model::new(3, 3, vec![0.0; 9], vec![true; 9], vec![None; 3], vec![], caps).unwrap();
        m.start = Some(vec![0, 0, 1]); // infeasible (value 0 twice): ignored
        for st in 0..3u64 { let c = Chain::new_until(&m, 5, st, None).unwrap(); assert_eq!(m.violations(&c.x), 0); }
        m.start = Some(vec![2, 0, 1]);
        for st in 0..3u64 { let c = Chain::new_until(&m, 5, st, None).unwrap(); assert_eq!(m.violations(&c.x), 0); }
        assert!(m.with_clamp(0, 1).start.is_none()); assert_eq!(m.with_clamp(0, 2).start, m.start);
    }
    /// R19.6: the arc-consistent (MAC) start. A 20-job x 30-slot chain (j0 < j1 < ... < j19 as pairwise forbid caps + an
    /// all-different cap per slot) thrashed the plain search past every start deadline; MAC places it at once. On 300 random
    /// small programs (binary forbid caps + a wider cap + forbidden values) every MAC start is feasible, and MAC never claims a
    /// start for a program that enumeration proves infeasible.
    #[test]
    fn mac_start_places_precedence_chains_and_is_sound() {
        let (n, k) = (20usize, 30usize);
        let mut caps: Vec<Cap> = (0..k).map(|s| Cap { weights: vec![], members: (0..n).map(|i| (i, s)).collect(), limit: 1 }).collect();
        for i in 0..n - 1 { for a in 0..k { for b in 0..=a { caps.push(Cap { weights: vec![], members: vec![(i, a), (i + 1, b)], limit: 1 }); } } }
        let m = Model::new(n, k, vec![0.0; n * k], vec![true; n * k], vec![None; n], vec![], caps).unwrap(); assert!(!m.is_partition());
        let p0 = START_PROPAGATED.load(std::sync::atomic::Ordering::Relaxed);
        for seed in 0..8u64 { let t = std::time::Instant::now();
            let x = m.feasible_init_until(&mut Philox4x32::new(seed, 3), START_WORK, Some(t + std::time::Duration::from_secs(5))).expect("MAC start");
            assert_eq!(m.violations(&x), 0); assert!((1..n).all(|i| x[i] > x[i - 1])); }
        assert!(START_PROPAGATED.load(std::sync::atomic::Ordering::Relaxed) >= p0 + 8);
        let mut rng = Philox4x32::new(77, 0); let (mut used, mut infeasible) = (0, 0);
        for _ in 0..300 {
            let (n, k) = (5usize, 4usize); let mut caps = vec![];
            for i in 0..n { for j in i + 1..n { for a in 0..k { for b in 0..k { if rng.f64() < 0.3 { caps.push(Cap { weights: vec![], members: vec![(i, a), (j, b)], limit: 1 }); } } } } }
            caps.push(Cap { weights: vec![], members: (0..n).map(|i| (i, (rng.f64() * k as f64) as usize % k)).collect(), limit: 1 });
            let allowed: Vec<bool> = (0..n * k).map(|_| rng.f64() < 0.85).collect();
            let Ok(m) = Model::new(n, k, (0..n * k).map(|_| rng.f64() - 0.5).collect(), allowed, vec![None; n], vec![], caps) else { continue };
            let key: Vec<f64> = (0..n * k).map(|_| rng.f64()).collect(); let rank: Vec<usize> = (0..n).collect();
            let feasible = exact(&m, 1, 1_000_000).unwrap().n_feasible > 0; infeasible += !feasible as usize;
            match m.mac_start(&key, &rank, START_WORK, None) { Ok(x) => { used += 1; assert!(feasible); assert_eq!(m.violations(&x), 0); } Err(late) => assert!(!late) }
        }
        eprintln!("MAC starts {used}, infeasible programs {infeasible} (of 300)"); assert!(used > 50 && infeasible > 10, "MAC starts {used}, infeasible programs {infeasible}");
    }
    /// Thread count is a resource control, not a semantic one: fixed sweeps => bit-identical for 1..=5 threads
    /// (and under a 40% CPU duty cycle).
    #[test]
    fn frontier_declines_when_a_cap_load_can_pass_255() {
        // n free spins under an at-most-n cap on "on" (every spin its own component, the cap carried across all of them):
        // log Z = n ln 2. At n = 200 the frontier answers exactly; at n = 600 it used to answer 406.99 (truth 415.89) and now declines
        for (n, answers) in [(200usize, true), (600, false)] {
            let caps = vec![Cap { weights: vec![], members: (0..n).map(|i| (i, 1)).collect(), limit: n }];
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
        let caps: Vec<Cap> = (0..k).map(|v| Cap { weights: vec![], members: (0..n).filter(|&i| allowed[i * k + v]).map(|i| (i, v)).collect(), limit: 1 }).collect();
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
        let caps: Vec<Cap> = (0..h * dup).map(|c| Cap { weights: vec![], members: (0..p).map(|i| (i, c % h)).collect(), limit: 1 }).collect();
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
        let mut caps: Vec<Cap> = (0..h * dup).map(|c| Cap { weights: vec![], members: (0..p).map(|i| (i, c % h)).collect(), limit: 1 }).collect();
        caps.push(Cap { weights: vec![], members: vec![(p - 1, h), (a, a0)], limit: 1 });
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
        let m = Model::new(n, k, h, vec![true; n * k], vec![None; n], pairs, vec![Cap { weights: vec![], members: (0..n).map(|i| (i, 1)).collect(), limit: 4 }]).unwrap();
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
        let mut caps: Vec<Cap> = (0..h * dup).map(|c| Cap { weights: vec![], members: (0..p).map(|i| (i, c % h)).collect(), limit: 1 }).collect();
        caps.push(Cap { weights: vec![], members: vec![(p - 1, h), (a, a0)], limit: 1 });
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
        let mut caps: Vec<Cap> = (0..t).map(|s| Cap { weights: vec![], members: (0..n).filter(|&i| allowed[i * t + s]).map(|i| (i, s)).collect(), limit: cap }).collect();
        for j in 0..n { for &i in &pred[j] { for a in 0..t { for b in 0..=a { if allowed[i * t + a] && allowed[j * t + b] { caps.push(Cap { weights: vec![], members: vec![(i, a), (j, b)], limit: 1 }); } } } } }
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

    /// R19 P1.3(b, c): the components + forest tier agrees with whole-program enumeration (log Z, every odds value, the MAP's
    /// log w, infeasibility) on random programs: forests with forbidden values and clamps, plus cyclic and capped components
    /// (solved by enumeration / the frontier inside the tier); and a 1,000-variable chain's log Z matches a transfer matrix.
    #[test]
    fn components_and_forest_match_enumeration() {
        let (mut forest_only, mut mixed, mut declined, mut infeasible) = (0, 0, 0, 0);
        for seed in 0..400u64 {
            let mut r = Philox4x32::new(seed, 23); let (n, k) = (5 + r.below(6), 2 + r.below(2));
            let h: Vec<f64> = (0..n * k).map(|_| r.f64() * 3.0 - 1.5).collect();
            let mut allowed: Vec<bool> = (0..n * k).map(|_| r.f64() < 0.8).collect(); for i in 0..n { allowed[i * k + r.below(k)] = true; }
            let clamp: Vec<Option<usize>> = (0..n).map(|i| if r.f64() < 0.1 { (0..k).find(|&v| allowed[i * k + v]) } else { None }).collect();
            let coup = |r: &mut Philox4x32| if r.f64() < 0.5 { Coupling::Potts(r.f64() * 3.0 - 1.5) } else { Coupling::Table((0..k * k).map(|_| r.f64() * 2.0 - 1.0).collect()) };
            let mut pairs = vec![]; for i in 1..n { if r.f64() < 0.75 { let j = r.below(i); pairs.push(Pair { i, j, c: coup(&mut r) }); } }
            if r.f64() < 0.3 { let (i, j) = (r.below(n), r.below(n)); if i != j { pairs.push(Pair { i, j, c: coup(&mut r) }); } }
            let caps: Vec<Cap> = if r.f64() < 0.4 { let mut mem: Vec<(usize, usize)> = (0..n * k).filter(|_| r.f64() < 0.25).map(|q| (q / k, q % k)).collect(); mem.dedup();
                if mem.is_empty() { vec![] } else { vec![Cap { weights: vec![], limit: r.below(mem.len()), members: mem }] } } else { vec![] };
            let m = Model::new(n, k, h, allowed, clamp, pairs, caps).unwrap();
            let e = exact(&m, 1, 1 << 22).unwrap();
            let Some(c) = exact_components_until(&m, 1 << 22, FRONTIER_MAX_STATES, None) else { declined += 1; continue };
            assert_eq!(c.infeasible, e.n_feasible == 0, "seed {seed}: infeasible {} vs n_feasible {}", c.infeasible, e.n_feasible);
            if c.infeasible { infeasible += 1; continue; }
            assert!((c.logz - e.logz).abs() < 1e-9 * e.logz.abs().max(1.0), "seed {seed}: log Z {} vs {}", c.logz, e.logz);
            let d = c.marg.iter().zip(&e.marg).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max); assert!(d < 1e-9, "seed {seed}: odds differ by {d}");
            assert_eq!(m.violations(&c.map), 0, "seed {seed}: MAP infeasible"); let best = m.logw(&e.top[0].1);
            assert!((c.map_logw - best).abs() < 1e-9 * best.abs().max(1.0), "seed {seed}: MAP log w {} vs {best}", c.map_logw);
            assert_eq!(c.forest + c.enumerated + c.frontier, c.components);
            if c.forest == c.components { forest_only += 1; } else { mixed += 1; }
        }
        eprintln!("components tier: {forest_only} forest-only, {mixed} mixed, {infeasible} infeasible, {declined} declined");
        assert!(forest_only > 100 && mixed > 40 && infeasible > 5, "coverage: {forest_only} / {mixed} / {infeasible} / {declined}");
        // a 1,000-variable two-value chain (tables): log Z by an independent forward transfer matrix
        let (n, k) = (1000, 2); let mut r = Philox4x32::new(5, 5);
        let h: Vec<f64> = (0..n * k).map(|_| r.f64() * 2.0 - 1.0).collect();
        let pairs: Vec<Pair> = (1..n).map(|i| Pair { i: i - 1, j: i, c: Coupling::Table((0..4).map(|_| r.f64() * 2.0 - 1.0).collect()) }).collect();
        let tabs: Vec<Vec<f64>> = pairs.iter().map(|p| match &p.c { Coupling::Table(t) => t.clone(), _ => unreachable!() }).collect();
        let m = Model::new(n, k, h.clone(), vec![true; n * k], vec![None; n], pairs, vec![]).unwrap();
        let t0 = std::time::Instant::now(); let c = exact_components_until(&m, 2_000_000, FRONTIER_MAX_STATES, None).unwrap(); let ms = t0.elapsed().as_secs_f64() * 1e3;
        let mut a = [h[0], h[1]];
        for i in 1..n { let t = &tabs[i - 1]; let nx = |v: usize| { let (x, y) = (a[0] + t[v], a[1] + t[2 + v]); let mx = x.max(y); mx + ((x - mx).exp() + (y - mx).exp()).ln() + h[i * k + v] }; a = [nx(0), nx(1)]; }
        let tm = a[0].max(a[1]) + ((a[0] - a[0].max(a[1])).exp() + (a[1] - a[0].max(a[1])).exp()).ln();
        assert_eq!((c.forest, c.components), (1, 1)); assert!((c.logz - tm).abs() < 1e-9 * tm.abs(), "chain1000 log Z {} vs transfer matrix {tm}", c.logz);
        eprintln!("chain1000: forest tier {ms:.3} ms, log Z {} (transfer matrix {tm})", c.logz);
    }

    /// R19 P1.3(d): the occupancy-count DP agrees with enumeration on complete-graph two-value components (uniform Potts or
    /// symmetric tables, a third value forbidden, forced members, binding / infeasible whole-value caps, a second independent
    /// component), a near miss (one pair different) goes to enumeration instead, and at n = 512 identical tasks log Z and every
    /// odds value match the closed form (odds clamped into [0, 1]).
    #[test]
    fn occupancy_count_dp_matches_enumeration() {
        let (mut counted, mut near, mut infeasible) = (0, 0, 0);
        for seed in 0..300u64 {
            let mut r = Philox4x32::new(seed, 29); let (n, k) = (3 + r.below(9), 2 + r.below(2)); let miss = r.f64() < 0.15;
            let mut allowed = vec![true; n * k]; if k == 3 { for i in 0..n { allowed[i * k + 2] = false; } }
            let h: Vec<f64> = (0..n * k).map(|_| r.f64() * 2.0 - 1.0).collect();
            for i in 0..n { if r.f64() < 0.1 { allowed[i * k + r.below(2)] = false; } }
            let term = if r.f64() < 0.5 { Coupling::Potts(r.f64() * 2.0 - 0.7) } else { let (x, y, z) = (r.f64() - 0.5, r.f64() - 0.5, r.f64() - 0.5);
                let mut t = vec![0.3; k * k]; t[0] = x; t[k + 1] = y; t[1] = z; t[k] = z; Coupling::Table(t) };
            let mut pairs = vec![]; for i in 0..n { for j in i + 1..n { pairs.push(Pair { i, j, c: term.clone() }); } }
            if miss { pairs[0].c = Coupling::Potts(1.234); }
            let mut caps = vec![]; for v in 0..2 { if r.f64() < 0.5 { caps.push(Cap { weights: vec![], members: (0..n).filter(|&i| allowed[i * k + v]).map(|i| (i, v)).collect(), limit: r.below(n + 1) }); } }
            // a second, independent forest component (two variables) so the tier combines a count component with a tree
            let (n2, mut h2, mut al2) = (n + 2, h.clone(), allowed.clone()); h2.extend((0..2 * k).map(|_| r.f64() - 0.5)); al2.extend(vec![true; 2 * k]);
            pairs.push(Pair { i: n, j: n + 1, c: Coupling::Potts(0.4) }); caps.retain(|c| !c.members.is_empty());
            let m = Model::new(n2, k, h2, al2, vec![None; n2], pairs, caps).unwrap();
            let e = exact(&m, 1, 1 << 22).unwrap(); let c = exact_components_until(&m, 1 << 22, FRONTIER_MAX_STATES, None).unwrap();
            assert_eq!(c.infeasible, e.n_feasible == 0, "seed {seed}"); if c.infeasible { infeasible += 1; continue; }
            if miss { assert_eq!(c.occupancy, 0, "seed {seed}: a near miss must not be counted"); near += 1; } else { assert_eq!(c.occupancy, 1, "seed {seed}"); counted += 1; }
            assert!((c.logz - e.logz).abs() < 1e-9 * e.logz.abs().max(1.0), "seed {seed}: log Z {} vs {}", c.logz, e.logz);
            let d = c.marg.iter().zip(&e.marg).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max); assert!(d < 1e-9, "seed {seed}: odds differ by {d}");
            assert_eq!(m.violations(&c.map), 0, "seed {seed}"); let best = m.logw(&e.top[0].1);
            assert!((c.map_logw - best).abs() < 1e-9 * best.abs().max(1.0), "seed {seed}: MAP log w {} vs {best}", c.map_logw);
        }
        eprintln!("count DP: {counted} counted, {near} near misses (enumerated), {infeasible} infeasible");
        assert!(counted > 150 && near > 20 && infeasible > 5, "coverage {counted} / {near} / {infeasible}");
        // n = 512 identical tasks (h_b = 0.06, Potts 0.2): closed form sum_c C(n,c) exp(0.2 (C(c,2) + C(n-c,2)) + 0.06 c)
        let n = 512; let mut h = vec![0.0; 2 * n]; for i in 0..n { h[2 * i + 1] = 0.06; }
        let pairs: Vec<Pair> = (0..n).flat_map(|i| (i + 1..n).map(move |j| Pair { i, j, c: Coupling::Potts(0.2) })).collect();
        let m = Model::new(n, 2, h, vec![true; 2 * n], vec![None; n], pairs, vec![]).unwrap();
        let t0 = std::time::Instant::now(); let c = exact_components_until(&m, 2_000_000, FRONTIER_MAX_STATES, None).unwrap(); let ms = t0.elapsed().as_secs_f64() * 1e3;
        let lc: Vec<f64> = (0..=n).map(|c| { let ln_choose = (1..=c).map(|q| (((n - c + q) as f64) / q as f64).ln()).sum::<f64>(); let c2 = |x: usize| (x * x.saturating_sub(1) / 2) as f64;
            ln_choose + 0.2 * (c2(c) + c2(n - c)) + 0.06 * c as f64 }).collect();
        let mx = lc.iter().cloned().fold(f64::NEG_INFINITY, f64::max); let z: f64 = lc.iter().map(|x| (x - mx).exp()).sum(); let logz = mx + z.ln();
        let pb = lc.iter().enumerate().map(|(c, x)| (x - mx).exp() * c as f64 / n as f64).sum::<f64>() / z;
        assert_eq!(c.occupancy, 1); assert!((c.logz - logz).abs() < 1e-9 * logz.abs(), "n=512 log Z {} vs closed form {logz}", c.logz);
        for i in 0..n { let (x, y) = (c.marg[2 * i], c.marg[2 * i + 1]); assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) && (x + y - 1.0).abs() < 1e-15);
            assert!((y - pb).abs() < 1e-9, "n=512 task {i}: {y} vs closed form {pb}"); }
        eprintln!("count DP n=512: {ms:.3} ms, log Z {} (closed form {logz}), P(b) {} (closed form {pb})", c.logz, c.marg[1]);
    }

    /// R19 P1.3(a): the compile pass leaves log Z and every odds value unchanged (enumeration of the compiled vs the original
    /// program, 1e-12) while it folds constant / separable tables into unaries, drops zero pairs and never-binding caps; a
    /// non-separable table and Potts(w != 0) are kept.
    #[test]
    fn compile_pass_preserves_the_distribution() {
        let (mut folded, mut dropped, mut kept) = (0, 0, 0);
        for seed in 0..200u64 {
            let mut r = Philox4x32::new(seed, 31); let (n, k) = (4 + r.below(5), 2 + r.below(2));
            let q4 = |r: &mut Philox4x32| (r.below(9) as f64 - 4.0) * 0.25; // exactly representable, so separable sums are exact
            let h: Vec<f64> = (0..n * k).map(|_| r.f64() - 0.5).collect();
            let mut pairs = vec![];
            for _ in 0..r.below(2 * n) + 1 { let (i, j) = (r.below(n), r.below(n)); if i == j { continue; }
                let c = match r.below(5) { 0 => Coupling::Potts(0.0), 1 => { let c0 = q4(&mut r); Coupling::Table(vec![c0; k * k]) },
                    2 => { let u: Vec<f64> = (0..k).map(|_| q4(&mut r)).collect(); let w: Vec<f64> = (0..k).map(|_| q4(&mut r)).collect(); Coupling::Table((0..k * k).map(|x| u[x / k] + w[x % k]).collect()) },
                    3 => Coupling::Potts(q4(&mut r) + 0.125), _ => Coupling::Table((0..k * k).map(|_| r.f64() - 0.5).collect()) };
                pairs.push(Pair { i, j, c }); }
            let caps = vec![Cap { weights: vec![], members: (0..n).map(|i| (i, 0)).collect(), limit: if r.f64() < 0.5 { n } else { n / 2 } }];
            let m = Model::new(n, k, h, vec![true; n * k], vec![None; n], pairs, caps).unwrap(); let (mc, c) = m.compiled();
            folded += c.tables_folded; dropped += c.pairs_dropped + c.caps_dropped; kept += mc.pairs.len();
            assert_eq!(mc.pairs.len() + c.pairs_dropped + c.tables_folded, m.pairs.len());
            let (a, b) = (exact(&m, 1, 1 << 20).unwrap(), exact(&mc, 1, 1 << 20).unwrap());
            assert!((a.logz - b.logz).abs() < 1e-12 * a.logz.abs().max(1.0), "seed {seed}: log Z {} vs compiled {}", a.logz, b.logz);
            let d = a.marg.iter().zip(&b.marg).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max); assert!(d < 1e-12, "seed {seed}: odds differ by {d}");
        }
        eprintln!("compile pass: {folded} tables folded, {dropped} pairs/caps dropped, {kept} pairs kept");
        assert!(folded > 150 && dropped > 100 && kept > 150, "coverage {folded} / {dropped} / {kept}");
    }
}
