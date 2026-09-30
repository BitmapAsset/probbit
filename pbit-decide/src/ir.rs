//! pbit-ir v0: a portable, line-based text form of a constrained Potts decision problem, plus a lowering to a
//! binary one-hot QUBO/Ising "p-bit topology" (what a hardware p-bit fabric natively samples).
//! Floats are written as IEEE-754 bit patterns (hex) so the round trip is bit-exact.
use crate::Problem;

pub fn to_ir(p: &Problem) -> String {
    let mut s = format!("pbit-ir v0\nvars {} {}\nlam {:016x}\nflags {} {}\n", p.t, p.a, p.lam.to_bits(), p.block_moves as u8, p.pair_swaps as u8);
    s += "cap"; for c in &p.cap { s += &format!(" {c}"); } s += "\n";
    for i in 0..p.t {
        s += &format!("var {} group {} clamp {}", i, if p.group[i] == usize::MAX { -1i64 } else { p.group[i] as i64 }, p.clamp[i].map_or(-1i64, |c| c as i64));
        for a in 0..p.a { if p.allowed[i * p.a + a] { s += &format!(" {}:{:016x}", a, p.h[i * p.a + a].to_bits()); } }
        s += "\n";
    }
    s + "end\n"
}
pub fn from_ir(src: &str) -> Result<Problem, String> {
    let mut ln = src.lines(); let e = |m: &str| m.to_string();
    if ln.next() != Some("pbit-ir v0") { return Err(e("bad header")); }
    let v: Vec<usize> = ln.next().ok_or(e("eof"))?.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect(); let (t, a) = (v[0], v[1]);
    let lam = f64::from_bits(u64::from_str_radix(ln.next().ok_or(e("eof"))?.split_whitespace().nth(1).ok_or(e("lam"))?, 16).map_err(|x| x.to_string())?);
    let fl: Vec<u8> = ln.next().ok_or(e("eof"))?.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
    let cap: Vec<usize> = ln.next().ok_or(e("eof"))?.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
    let (mut h, mut allowed, mut group, mut clamp) = (vec![0.0; t * a], vec![false; t * a], vec![usize::MAX; t], vec![None; t]);
    for i in 0..t { let w: Vec<&str> = ln.next().ok_or(e("eof"))?.split_whitespace().collect();
        if w[0] != "var" || w[1].parse::<usize>().ok() != Some(i) { return Err(format!("var line {i}")); }
        let g: i64 = w[3].parse().unwrap(); if g >= 0 { group[i] = g as usize; }
        let c: i64 = w[5].parse().unwrap(); if c >= 0 { clamp[i] = Some(c as usize); }
        for kv in &w[6..] { let (k, x) = kv.split_once(':').ok_or(e("kv"))?; let k: usize = k.parse().unwrap();
            allowed[i * a + k] = true; h[i * a + k] = f64::from_bits(u64::from_str_radix(x, 16).map_err(|x| x.to_string())?); } }
    if ln.next() != Some("end") { return Err(e("missing end")); }
    Ok(Problem { t, a, h, allowed, cap, group, lam, clamp, block_moves: fl[0] == 1, pair_swaps: fl[1] == 1 })
}

/// Binary lowering: one p-bit s_{i,a} per allowed (task, agent) + bounded-binary slack bits per agent for capacity.
/// E(s,z) = -sum h s - lam sum_{same group, i<j} sum_a s_ia s_ja + P sum_i (sum_a s_ia - 1)^2 + P sum_a (sum_i s_ia + sum_k c_k z_ak - cap_a)^2
/// For every feasible x with its exact slack, E = -logw(x). Hardware p-bits sample exp(-E) with plain single-bit updates.
pub struct Qubo { pub n: usize, pub lin: Vec<f64>, pub adj: Vec<Vec<(usize, f64)>>, pub offset: f64, pub var_of: Vec<(usize, usize)>, pub slack: Vec<Vec<(usize, usize)>>, pub n_edges: usize }
pub fn lower_onehot(p: &Problem, pen: f64) -> Qubo {
    let mut var_of = vec![]; let mut id = vec![usize::MAX; p.t * p.a];
    for i in 0..p.t { for a in 0..p.a { if p.ok(i, a) { id[i * p.a + a] = var_of.len(); var_of.push((i, a)); } } }
    let ns = var_of.len(); let mut n = ns; let mut slack = vec![vec![]; p.a];
    for a in 0..p.a { let (mut rem, mut k) = (p.cap[a], 1usize); while rem > 0 { let c = k.min(rem); slack[a].push((n, c)); n += 1; rem -= c; k *= 2; } }
    let mut lin = vec![0.0; n]; let mut q: std::collections::HashMap<(usize, usize), f64> = Default::default(); let mut offset = 0.0;
    let mut addq = |u: usize, v: usize, w: f64| { let k = if u < v { (u, v) } else { (v, u) }; *q.entry(k).or_insert(0.0) += w; };
    for (v, &(i, a)) in var_of.iter().enumerate() { lin[v] -= p.h[i * p.a + a]; }
    for i in 0..p.t { for j in i + 1..p.t { if p.group[i] != usize::MAX && p.group[i] == p.group[j] { for a in 0..p.a {
        let (u, v) = (id[i * p.a + a], id[j * p.a + a]); if u != usize::MAX && v != usize::MAX { addq(u, v, -p.lam); } } } } }
    // P (sum c_v x_v - b)^2 = P[ sum c_v^2 x_v + 2 sum_{u<v} c_u c_v x_u x_v - 2b sum c_v x_v + b^2 ]
    let mut square = |terms: &[(usize, f64)], b: f64, lin: &mut Vec<f64>, offset: &mut f64| {
        for (k, &(u, cu)) in terms.iter().enumerate() { lin[u] += pen * (cu * cu - 2.0 * b * cu);
            for &(v, cv) in &terms[k + 1..] { addq(u, v, 2.0 * pen * cu * cv); } } *offset += pen * b * b; };
    for i in 0..p.t { let tm: Vec<(usize, f64)> = (0..p.a).filter_map(|a| { let u = id[i * p.a + a]; (u != usize::MAX).then_some((u, 1.0)) }).collect(); square(&tm, 1.0, &mut lin, &mut offset); }
    for a in 0..p.a { let mut tm: Vec<(usize, f64)> = (0..p.t).filter_map(|i| { let u = id[i * p.a + a]; (u != usize::MAX).then_some((u, 1.0)) }).collect();
        tm.extend(slack[a].iter().map(|&(z, c)| (z, c as f64))); square(&tm, p.cap[a] as f64, &mut lin, &mut offset); }
    let mut adj = vec![vec![]; n]; let n_edges = q.len();
    for ((u, v), w) in q { if w != 0.0 { adj[u].push((v, w)); adj[v].push((u, w)); } }
    Qubo { n, lin, adj, offset, var_of, slack, n_edges }
}
impl Qubo {
    pub fn energy(&self, s: &[u8]) -> f64 { let mut e = self.offset;
        for u in 0..self.n { if s[u] == 1 { e += self.lin[u]; for &(v, w) in &self.adj[u] { if v > u && s[v] == 1 { e += w; } } } } e }
    /// Encode a task assignment + exact slack.
    pub fn encode(&self, p: &Problem, x: &[usize]) -> Vec<u8> {
        let mut s = vec![0u8; self.n]; for (v, &(i, a)) in self.var_of.iter().enumerate() { if x[i] == a { s[v] = 1; } }
        let mut load = vec![0usize; p.a]; for &a in x { load[a] += 1; }
        for a in 0..p.a { let mut rem = p.cap[a] - load[a]; for &(z, c) in self.slack[a].iter().rev() { if rem >= c { s[z] = 1; rem -= c; } } }
        s
    }
    /// Decode: Some(x) iff every task is exactly one-hot and every capacity holds.
    pub fn decode(&self, p: &Problem, s: &[u8]) -> Option<Vec<usize>> {
        let mut x = vec![usize::MAX; p.t];
        for (v, &(i, a)) in self.var_of.iter().enumerate() { if s[v] == 1 { if x[i] != usize::MAX { return None; } x[i] = a; } }
        if x.iter().any(|&v| v == usize::MAX) { return None; } if p.violations(&x) > 0 { return None; } Some(x)
    }
    /// Plain p-bit dynamics: sequential single-bit heat-bath on exp(-E) (what an unconstrained p-bit fabric does).
    pub fn sweep(&self, s: &mut [u8], rng: &mut pbit_core::Philox4x32) {
        for u in 0..self.n { let mut d = self.lin[u]; for &(v, w) in &self.adj[u] { if s[v] == 1 { d += w; } }
            s[u] = (rng.f64() * (1.0 + d.exp()) < 1.0) as u8; }
    }
}
