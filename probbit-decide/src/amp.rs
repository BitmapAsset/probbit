//! RFC 0011 (research, rounds 1-2): an amplitude register. A register of q qubits simulated exactly on a CPU as a state vector of
//! 2^q complex amplitudes (f64 pairs, on the heap: 16 * 2^q bytes): unitary gates with phase, interference between basis states,
//! exact marginals (no sampling), measurement by the Born rule with the engine's deterministic RNG, and a digest of the state at
//! 6 decimals (the rule for every emitted float). A simulation of a small quantum system and nothing else: no quantum hardware,
//! no speedup claim, no scaling past what 2^q amplitudes allow.
//!
//! Basis index x = sum of bit_i << i: qubit i is bit i. Round 2: a variable with d allowed values takes ceil(log2 d) qubits
//! (`Encoding`; d = 3: codes 00, 01, 10, code 11 unused), the start is the uniform superposition over the valid codes, the
//! mixer is block-diagonal per variable (`MIXER`, the identity on unused codes) and the cost is one general diagonal phase
//! layer exp(-i g E) with E = -log w per basis state, so pair tables between multi-level variables (3- and 4-body terms in
//! the qubits) need no decomposition. The unused codes keep amplitude 0 (checked after every layer).
#[allow(unused_imports)]
use std::prelude::rust_2021::*;
use probbit_core::Philox4x32;

/// Round 7: the register's one math path. Every function past + - * / that the register evaluates (cos, sin, sqrt, asin, acos,
/// atan2, floor) is the `libm` crate's on every build, std and no_std alike: pure Rust, the same code and the same evaluation
/// order on every target, no platform libm and no fused multiply-add, so a strand recorded on one build replays bit for bit on
/// the other whatever the schedule's length (round 5: the platform's cos/sin against libm's flipped a 6-decimal odd over the
/// long sweep's layers).
mod m {
    #[inline] pub fn cos(x: f64) -> f64 { libm::cos(x) }
    #[inline] pub fn sin(x: f64) -> f64 { libm::sin(x) }
    #[inline] pub fn sqrt(x: f64) -> f64 { libm::sqrt(x) }
    #[inline] pub fn asin(x: f64) -> f64 { libm::asin(x) }
    #[inline] pub fn acos(x: f64) -> f64 { libm::acos(x) }
    #[inline] pub fn atan2(y: f64, x: f64) -> f64 { libm::atan2(y, x) }
    #[inline] pub fn floor(x: f64) -> f64 { libm::floor(x) }
}

/// The largest register on the host (16 qubits = 1 MiB of amplitudes)
pub const Q_MAX_HOST: usize = 16;
/// Bytes per amplitude: two f64
pub const AMP_BYTES: usize = 16;

/// A complex number (f64 parts)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct C { pub re: f64, pub im: f64 }
impl C {
    pub const ZERO: C = C { re: 0.0, im: 0.0 };
    pub const ONE: C = C { re: 1.0, im: 0.0 };
    #[inline] pub fn new(re: f64, im: f64) -> C { C { re, im } }
    #[inline] pub fn mul(self, o: C) -> C { C { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re } }
    #[inline] pub fn add(self, o: C) -> C { C { re: self.re + o.re, im: self.im + o.im } }
    #[inline] pub fn conj(self) -> C { C { re: self.re, im: -self.im } }
    #[inline] pub fn norm2(self) -> f64 { self.re * self.re + self.im * self.im }
    /// e^{i t}
    #[inline] pub fn cis(t: f64) -> C { C { re: m::cos(t), im: m::sin(t) } }
}

/// The gate set: enough for Ising-type programs and QAOA layers. Angles in radians; RX(t) = exp(-i t X / 2),
/// RZ(t) = exp(-i t Z / 2), RZZ(t) = exp(-i t Z⊗Z / 2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gate { H(usize), X(usize), Z(usize), Rx(usize, f64), Rz(usize, f64), Cnot(usize, usize), Cz(usize, usize), Rzz(usize, usize, f64) }
impl Gate {
    /// The gate as one canonical text line (angles at 6 decimals): what `circuit_text` joins and the circuit digest hashes
    pub fn text(&self) -> String {
        match *self {
            Gate::H(a) => format!("h {a}"), Gate::X(a) => format!("x {a}"), Gate::Z(a) => format!("z {a}"),
            Gate::Rx(a, t) => format!("rx {a} {}", f6(t)), Gate::Rz(a, t) => format!("rz {a} {}", f6(t)),
            Gate::Cnot(a, b) => format!("cnot {a} {b}"), Gate::Cz(a, b) => format!("cz {a} {b}"), Gate::Rzz(a, b, t) => format!("rzz {a} {b} {}", f6(t)),
        }
    }
    /// The gate's matrix on its own qubits (2x2, or 4x4 on (a, b) with basis index bit_a + 2 bit_b), row-major
    pub fn matrix(&self) -> Vec<C> {
        let (r, z) = (core::f64::consts::FRAC_1_SQRT_2, C::ZERO);
        let half = |t: f64| (C::new(m::cos(t / 2.0), 0.0), m::sin(t / 2.0));
        match *self {
            Gate::H(_) => vec![C::new(r, 0.0), C::new(r, 0.0), C::new(r, 0.0), C::new(-r, 0.0)],
            Gate::X(_) => vec![z, C::ONE, C::ONE, z],
            Gate::Z(_) => vec![C::ONE, z, z, C::new(-1.0, 0.0)],
            Gate::Rx(_, t) => { let (c, s) = half(t); vec![c, C::new(0.0, -s), C::new(0.0, -s), c] }
            Gate::Rz(_, t) => vec![C::cis(-t / 2.0), z, z, C::cis(t / 2.0)],
            Gate::Cnot(..) => { let mut m = vec![z; 16]; for (i, j) in [(0, 0), (1, 3), (2, 2), (3, 1)] { m[i * 4 + j] = C::ONE; } m }
            Gate::Cz(..) => { let mut m = vec![z; 16]; for i in 0..4 { m[i * 5] = if i == 3 { C::new(-1.0, 0.0) } else { C::ONE }; } m }
            Gate::Rzz(_, _, t) => { let mut m = vec![z; 16]; for i in 0..4 { let same = (i & 1) == (i >> 1); m[i * 5] = C::cis(if same { -t / 2.0 } else { t / 2.0 }); } m }
        }
    }
}

/// max |U†U - I| over the entries of a gate's matrix
pub fn unitarity_error(g: &Gate) -> f64 {
    let m = g.matrix(); let d = if m.len() == 4 { 2 } else { 4 }; let mut e: f64 = 0.0;
    for i in 0..d { for j in 0..d {
        let mut s = C::ZERO; for k in 0..d { s = s.add(m[k * d + i].conj().mul(m[k * d + j])); }
        let want = if i == j { 1.0 } else { 0.0 }; e = e.max((s.re - want).abs()).max(s.im.abs()); } }
    e
}

/// A number at 6 decimals as the canonical text writes it (`-0` written `0`)
fn f6(x: f64) -> String { let s = format!("{x:.6}"); if s.trim_start_matches('-').bytes().all(|b| b == b'0' || b == b'.') { "0.000000".into() } else { s } }

/// One step of a circuit: a gate, a diagonal phase layer exp(-i g E) (`Register::phase`), or a mixer layer on every variable
/// (`Register::mix`)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op { Gate(Gate), Phase(f64), Mix(f64), Oracle(f64), Diffuse(f64) }
impl Op {
    /// One canonical text line (angles at 6 decimals)
    pub fn text(&self) -> String { match self { Op::Gate(g) => g.text(), Op::Phase(g) => format!("phase {}", f6(*g)), Op::Mix(t) => format!("mix {}", f6(*t)), Op::Oracle(p) => format!("oracle {}", f6(*p)), Op::Diffuse(p) => format!("diffuse {}", f6(*p)) } }
}

/// The mixer every round-2 layer uses, by name (the receipt's `engine.mixer`)
pub const MIXER: &str = "complete-graph";

/// Round 2: how variables with d allowed values are packed into qubits. Variable i (d_i >= 1 values) takes b_i = ceil(log2 d_i)
/// qubits, packed in variable order (its code is bits o_i..o_i + b_i of the basis index); code c < d_i is its c-th value, codes
/// c >= d_i are unused (d = 3: code 11) and keep amplitude 0 for the whole evolution.
#[derive(Clone, Debug, PartialEq)]
pub struct Encoding { pub levels: Vec<usize> }
impl Encoding {
    /// Qubits for d values: ceil(log2 d) (0 for d = 1)
    pub fn bits(d: usize) -> usize { assert!(d >= 1, "a variable needs at least one value"); (usize::BITS - (d - 1).leading_zeros()) as usize }
    /// Every variable two-valued (round 1's programs)
    pub fn two_valued(q: usize) -> Encoding { Encoding { levels: vec![2; q] } }
    /// The encoded qubits, sum of ceil(log2 d_i)
    pub fn qubits(&self) -> usize { self.levels.iter().map(|&d| Encoding::bits(d)).sum() }
    /// Per variable its first qubit
    pub fn offsets(&self) -> Vec<usize> { let mut o = 0; self.levels.iter().map(|&d| { let r = o; o += Encoding::bits(d); r }).collect() }
    /// Variable i's code in basis index x
    pub fn code(&self, x: usize, i: usize) -> usize { let o = self.offsets()[i]; (x >> o) & ((1usize << Encoding::bits(self.levels[i])) - 1) }
    /// The codes of every variable in x, or None when one is unused
    pub fn decode(&self, x: usize) -> Option<Vec<usize>> {
        let mut o = 0; let mut v = Vec::with_capacity(self.levels.len());
        for &d in &self.levels { let b = Encoding::bits(d); let c = (x >> o) & ((1usize << b) - 1); if c >= d { return None; } v.push(c); o += b; }
        Some(v)
    }
    /// The canonical text the encoding digest hashes: one line per variable, `var <i> levels <d> qubits <o>..<o + b>`
    pub fn text(&self) -> String {
        let mut s = String::new(); for (i, (&d, o)) in self.levels.iter().zip(self.offsets()).enumerate() { s.push_str(&format!("var {i} levels {d} qubits {o}..{}\n", o + Encoding::bits(d))); } s
    }
}

/// The complete-graph mixer on d levels as a d x d matrix (row-major): exp(i t A) with A = J - I the adjacency of the complete
/// graph K_d. Closed form (J^2 = d J): e^{-it} (I + (e^{itd} - 1)/d J). d = 2 is exp(i t X) = RX(-2t); d = 1 is the identity.
pub fn mixer_matrix(d: usize, t: f64) -> Vec<C> {
    let (ph, c) = mixer_coef(d, t); (0..d * d).map(|k| { let off = c.mul(ph); if k / d == k % d { ph.add(off) } else { off } }).collect()
}
/// (e^{-it}, (e^{itd} - 1)/d)
fn mixer_coef(d: usize, t: f64) -> (C, C) { let e = C::cis(t * d as f64); (C::cis(-t), C::new((e.re - 1.0) / d as f64, e.im / d as f64)) }
/// max |U^dagger U - I| of the mixer on the whole 2^b block of a d-level variable (the mixer on the valid codes, the identity on
/// the unused ones)
pub fn mixer_unitarity_error(d: usize, t: f64) -> f64 {
    let n = 1usize << Encoding::bits(d); let m = mixer_matrix(d, t);
    let u = |i: usize, j: usize| if i < d && j < d { m[i * d + j] } else if i == j { C::ONE } else { C::ZERO };
    let mut e: f64 = 0.0;
    for i in 0..n { for j in 0..n { let mut s = C::ZERO; for k in 0..n { s = s.add(u(k, i).conj().mul(u(k, j))); }
        e = e.max((s.re - if i == j { 1.0 } else { 0.0 }).abs()).max(s.im.abs()); } }
    e
}

/// q qubits as 2^q complex amplitudes, and the ops applied (the circuit)
#[derive(Clone, Debug)]
pub struct Register { q: usize, amp: Vec<C>, ops: Vec<Op> }
impl Register {
    /// |0...0> on q qubits (q <= Q_MAX_HOST); the amplitudes take exactly 16 * 2^q bytes of heap
    pub fn new(q: usize) -> Register {
        assert!(q <= Q_MAX_HOST, "an amplitude register has at most {Q_MAX_HOST} qubits on the host");
        let mut amp = vec![C::ZERO; 1usize << q]; amp[0] = C::ONE;
        let r = Register { q, amp, ops: vec![] };
        assert_eq!(r.bytes(), AMP_BYTES << q); r
    }
    /// Round 2: the uniform superposition over the valid codes of every variable, 1/sqrt(prod d_i) on each valid basis state and
    /// 0 on the rest (for an all-two-valued encoding this is H on every qubit)
    pub fn uniform(enc: &Encoding) -> Register {
        let mut r = Register::new(enc.qubits()); let a = 1.0 / m::sqrt(enc.levels.iter().map(|&d| d as f64).product::<f64>());
        for (x, v) in r.amp.iter_mut().enumerate() { *v = if enc.decode(x).is_some() { C::new(a, 0.0) } else { C::ZERO }; }
        r
    }
    pub fn qubits(&self) -> usize { self.q }
    /// The amplitudes' bytes: 16 * 2^q
    pub fn bytes(&self) -> usize { self.amp.len() * core::mem::size_of::<C>() }
    pub fn amplitudes(&self) -> &[C] { &self.amp }
    pub fn ops(&self) -> &[Op] { &self.ops }
    /// The gates among the ops
    pub fn gates(&self) -> Vec<Gate> { self.ops.iter().filter_map(|o| if let Op::Gate(g) = o { Some(*g) } else { None }).collect() }
    /// The circuit as text, one op per line (angles at 6 decimals)
    pub fn circuit_text(&self) -> String { let mut s = String::new(); for o in &self.ops { s.push_str(&o.text()); s.push('\n'); } s }

    fn one(&mut self, a: usize, m: [C; 4]) {
        let bit = 1usize << a;
        for i in 0..self.amp.len() { if i & bit != 0 { continue; } let (x, y) = (self.amp[i], self.amp[i | bit]);
            self.amp[i] = m[0].mul(x).add(m[1].mul(y)); self.amp[i | bit] = m[2].mul(x).add(m[3].mul(y)); }
    }
    /// Apply one gate (logged in the circuit)
    pub fn apply(&mut self, g: Gate) {
        let q = self.q; let ok = |a: usize| assert!(a < q, "qubit {a} out of range ({q} qubits)");
        match g {
            Gate::H(a) | Gate::X(a) | Gate::Z(a) | Gate::Rx(a, _) | Gate::Rz(a, _) => { ok(a); let m = g.matrix(); self.one(a, [m[0], m[1], m[2], m[3]]); }
            Gate::Cnot(c, t) => { ok(c); ok(t); assert_ne!(c, t); let (bc, bt) = (1usize << c, 1usize << t);
                for i in 0..self.amp.len() { if i & bc != 0 && i & bt == 0 { self.amp.swap(i, i | bt); } } }
            Gate::Cz(a, b) => { ok(a); ok(b); assert_ne!(a, b); let m = (1usize << a) | (1usize << b);
                for i in 0..self.amp.len() { if i & m == m { self.amp[i] = C::new(-self.amp[i].re, -self.amp[i].im); } } }
            Gate::Rzz(a, b, t) => { ok(a); ok(b); assert_ne!(a, b); let (same, diff) = (C::cis(-t / 2.0), C::cis(t / 2.0));
                for i in 0..self.amp.len() { let p = if ((i >> a) & 1) == ((i >> b) & 1) { same } else { diff }; self.amp[i] = p.mul(self.amp[i]); } }
        }
        self.ops.push(Op::Gate(g));
    }
    /// Round 2: the general diagonal phase layer, a_x <- e^{-i g E(x)} a_x for every basis state with a finite E(x) (E = -log w
    /// of its plan); E(x) = NaN marks an unused code, left untouched (its amplitude is 0). O(2^q), any order of interaction.
    pub fn phase(&mut self, e: &[f64], g: f64) {
        assert_eq!(e.len(), self.amp.len(), "one energy per basis state");
        for (a, &ex) in self.amp.iter_mut().zip(e) { if !ex.is_nan() { *a = C::cis(-g * ex).mul(*a); } }
        self.ops.push(Op::Phase(g));
    }
    /// Round 2: the mixer layer, the complete-graph mixer exp(i t A_d) on the valid codes of every variable (`mixer_matrix`),
    /// the identity on its unused codes: block-diagonal, O(d) per block by the closed form.
    pub fn mix(&mut self, enc: &Encoding, t: f64) {
        assert_eq!(enc.qubits(), self.q, "the encoding's qubits");
        let mut buf: Vec<C> = vec![];
        for (&d, o) in enc.levels.iter().zip(enc.offsets()) {
            if d < 2 { continue; } let b = Encoding::bits(d); let mask = ((1usize << b) - 1) << o; let (ph, c) = mixer_coef(d, t);
            for x in 0..self.amp.len() { if x & mask != 0 { continue; }
                buf.clear(); let mut s = C::ZERO; for k in 0..d { let a = self.amp[x | (k << o)]; buf.push(a); s = s.add(a); } let cs = c.mul(s);
                for (k, &a) in buf.iter().enumerate() { self.amp[x | (k << o)] = ph.mul(a.add(cs)); } }
        }
        self.ops.push(Op::Mix(t));
    }
    /// Round 4: the oracle, a_x <- e^{i phi} a_x on the basis states marked good (phi = pi: the sign flip)
    pub fn oracle(&mut self, good: &[bool], phi: f64) {
        assert_eq!(good.len(), self.amp.len(), "one mark per basis state"); let w = C::cis(phi);
        for (a, &g) in self.amp.iter_mut().zip(good) { if g { *a = w.mul(*a); } }
        self.ops.push(Op::Oracle(phi));
    }
    /// Round 4: the diffusion about a stored state psi, I - (1 - e^{i phi}) |psi><psi| (phi = pi: the reflection I - 2 |psi><psi|)
    pub fn diffuse(&mut self, psi: &[C], phi: f64) {
        assert_eq!(psi.len(), self.amp.len(), "the stored state's amplitudes");
        let ip = psi.iter().zip(&self.amp).fold(C::ZERO, |s, (p, a)| s.add(p.conj().mul(*a)));
        let w = C::cis(phi); let f = C::new(1.0 - w.re, -w.im).mul(ip);
        for (a, p) in self.amp.iter_mut().zip(psi) { let d = f.mul(*p); *a = C::new(a.re - d.re, a.im - d.im); }
        self.ops.push(Op::Diffuse(phi));
    }
    /// The Born mass on unused codes (the basis states whose E is NaN)
    pub fn invalid_mass(&self, e: &[f64]) -> f64 { self.amp.iter().zip(e).filter(|(_, x)| x.is_nan()).map(|(a, _)| a.norm2()).sum() }
    /// The Born probabilities |a_x|^2
    pub fn probabilities(&self) -> Vec<f64> { self.amp.iter().map(|a| a.norm2()).collect() }
    /// Exact marginals P(qubit i = 1), one per qubit (no sampling)
    pub fn marginals(&self) -> Vec<f64> {
        let mut m = vec![0.0; self.q];
        for (x, a) in self.amp.iter().enumerate() { let p = a.norm2(); for (i, mi) in m.iter_mut().enumerate() { if x >> i & 1 == 1 { *mi += p; } } }
        m
    }
    /// Round 2: exact marginals per variable and level, P(variable i has code c), concatenated in variable order (sum d_i entries)
    pub fn level_marginals(&self, enc: &Encoding) -> Vec<f64> {
        let base: Vec<usize> = enc.levels.iter().scan(0, |s, &d| { let r = *s; *s += d; Some(r) }).collect();
        let mut m = vec![0.0; enc.levels.iter().sum()];
        for (x, a) in self.amp.iter().enumerate() { if let Some(v) = enc.decode(x) { let p = a.norm2(); for (i, c) in v.into_iter().enumerate() { m[base[i] + c] += p; } } }
        m
    }
    /// One measurement of every qubit by the Born rule: the basis index, drawn with Philox4x32(seed, stream 0)
    pub fn measure(&self, seed: u64) -> usize {
        let u = Philox4x32::new(seed, 0).f64(); let mut c = 0.0;
        for (x, a) in self.amp.iter().enumerate() { c += a.norm2(); if u < c { return x; } }
        self.amp.iter().rposition(|a| a.norm2() > 0.0).unwrap_or(0)
    }
    /// The canonical state: one line per amplitude, `re im` at 6 decimals
    pub fn canonical(&self) -> String { let mut s = String::new(); for a in &self.amp { s.push_str(&f6(a.re)); s.push(' '); s.push_str(&f6(a.im)); s.push('\n'); } s }
    /// sha256 of the canonical state
    pub fn digest(&self) -> [u8; 32] { sha256(self.canonical().as_bytes()) }
}

/// An Ising program over q two-valued variables: log w(x) = c + sum_i a_i z_i + sum_(i<j) b_ij z_i z_j (z = +1 for value 0)
#[derive(Clone, Debug, PartialEq)]
pub struct Ising { pub q: usize, pub a: Vec<f64>, pub b: Vec<(usize, usize, f64)> }
impl Ising {
    /// The program's log weight at basis index x, without the constant
    pub fn logw(&self, x: usize) -> f64 {
        let z = |i: usize| if x >> i & 1 == 0 { 1.0 } else { -1.0 };
        self.a.iter().enumerate().map(|(i, a)| a * z(i)).sum::<f64>() + self.b.iter().map(|&(i, j, b)| b * z(i) * z(j)).sum::<f64>()
    }
    /// E(x) = -log w(x) over the 2^q basis states (the phase layer's input)
    pub fn energies(&self) -> Vec<f64> { (0..1usize << self.q).map(|x| -self.logw(x)).collect() }
}

/// A transverse-field sweep: `steps` Trotter steps over total time `time`, s = (k + 1/2) / steps at step k,
/// H(s) = -(1 - s) sum_i A_i + s E (A_i the complete-graph mixer of variable i; E = -log w), started from the uniform
/// superposition over the valid codes
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Schedule { pub steps: usize, pub time: f64 }
impl Schedule {
    /// The canonical text the schedule digest hashes
    pub fn text(&self) -> String { format!("transverse-field linear steps {} time {}\n", self.steps, f6(self.time)) }
    pub fn name(&self) -> String { format!("tf-linear-{}x{}", self.steps, f6(self.time)) }
}
/// The tier's fixed default schedule (rounds 1 and 2; schedules are a later round's study)
pub const DEFAULT_SCHEDULE: Schedule = Schedule { steps: 16, time: 4.0 };
/// Round 5: the longer sweep (`engine.schedule: tf-linear-64x16`): 4x the default's steps and time, so about 4x its layers
pub const LONG_SCHEDULE: Schedule = Schedule { steps: 64, time: 16.0 };
/// Round 5: the two layers (gamma, beta) of `engine.schedule: qaoa-p2`, fixed by a grid search on a held-out third of the P0
/// corpus programs (the largest median separation on the decisions the thermal tier is indifferent on, subject to agreement
/// >= 90 % on the others; docs/rfc/0011-amplitude-register.md, "Schedules and the indifferent set"), then frozen: gamma 0.500000,
/// beta 0.500000, then gamma 1.500000, beta 0.250000
pub const QAOA_P2: [(f64, f64); 2] = [(0.5, 0.5), (1.5, 0.25)];
/// Round 5: the names `engine.schedule` accepts (the first is the default)
pub const SCHEDULE_NAMES: [&str; 3] = ["tf-linear-16x4", "tf-linear-64x16", "qaoa-p2"];
/// Round 5: the evolution the tier runs on each component: a transverse-field sweep or QAOA layers
#[derive(Clone, Debug, PartialEq)]
pub enum Evolution { Sweep(Schedule), Qaoa(Vec<(f64, f64)>) }
impl Evolution {
    /// The evolution of a name in `SCHEDULE_NAMES` (None for any other name)
    pub fn named(name: &str) -> Option<Evolution> {
        match name { "tf-linear-16x4" => Some(Evolution::Sweep(DEFAULT_SCHEDULE)), "tf-linear-64x16" => Some(Evolution::Sweep(LONG_SCHEDULE)),
            "qaoa-p2" => Some(Evolution::Qaoa(QAOA_P2.to_vec())), _ => None }
    }
    /// The register after the evolution from the uniform start
    pub fn run(&self, enc: &Encoding, e: &[f64]) -> Register { match self { Evolution::Sweep(s) => sweep(enc, e, s), Evolution::Qaoa(p) => qaoa_on(enc, e, p) } }
    /// The canonical text the schedule digest hashes (a sweep's is `Schedule::text`, unchanged)
    pub fn text(&self) -> String {
        match self { Evolution::Sweep(s) => s.text(),
            Evolution::Qaoa(p) => { let mut t = format!("qaoa layers {}\n", p.len()); for &(g, b) in p { t.push_str(&format!("gamma {} beta {}\n", f6(g), f6(b))); } t } }
    }
    /// The receipt's `schedule_name` (a sweep's is `Schedule::name`, unchanged)
    pub fn name(&self) -> String {
        match self { Evolution::Sweep(s) => s.name(), Evolution::Qaoa(p) if p[..] == QAOA_P2[..] => "qaoa-p2".into(), Evolution::Qaoa(p) => format!("qaoa-p{}-lab", p.len()) }
    }
}

/// The bar on the Born mass of the unused codes, checked after every layer (it is 0 by construction; never renormalised)
pub const INVALID_MASS_MAX: f64 = 1e-15;
fn checked(r: &Register, e: &[f64]) { let m = r.invalid_mass(e); assert!(m <= INVALID_MASS_MAX, "unused-code mass {m:e} after a layer"); }

/// Round 2: the sweep `s` on an encoding and its energies (one per basis state, NaN on unused codes): the uniform start over
/// the valid codes, then per step the phase layer (g = dt s) and the mixer layer (t = dt (1 - s))
pub fn sweep(enc: &Encoding, e: &[f64], s: &Schedule) -> Register {
    let mut r = Register::uniform(enc); let dt = s.time / s.steps as f64;
    for k in 0..s.steps { let x = (k as f64 + 0.5) / s.steps as f64; r.phase(e, dt * x); checked(&r, e); r.mix(enc, dt * (1.0 - x)); checked(&r, e); }
    r
}
/// Round 2: QAOA on an encoding: the uniform start, then per layer (gamma, beta) the phase layer and the mixer layer
pub fn qaoa_on(enc: &Encoding, e: &[f64], params: &[(f64, f64)]) -> Register {
    let mut r = Register::uniform(enc);
    for &(g, b) in params { r.phase(e, g); checked(&r, e); r.mix(enc, b); checked(&r, e); }
    r
}
/// The sweep on an Ising program (every variable two-valued): the same phase and mixer layers as `sweep`
pub fn evolve_ising(p: &Ising, s: &Schedule) -> Register { sweep(&Encoding::two_valued(p.q), &p.energies(), s) }
/// QAOA with p layers on an Ising program: the same layers as `qaoa_on`
pub fn qaoa(p: &Ising, layers: usize, params: &[(f64, f64)]) -> Register {
    assert_eq!(params.len(), layers, "one (gamma, beta) per layer"); qaoa_on(&Encoding::two_valued(p.q), &p.energies(), params)
}

/// Round 4: what `amplify_exact` did: the iterations (standard ones + the final phase-matched one), the final step's two
/// phases (oracle, diffusion; radians), the Born mass on the good states before and after
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Amplified { pub iterations: usize, pub phases: (f64, f64), pub mass_before: f64, pub mass_after: f64 }
/// The bar on the good states' mass after `amplify_exact` (asserted)
pub const AMPLIFIED_MASS_MIN: f64 = 1.0 - 1e-12;
/// Round 4: exact amplitude amplification of the good states (`good(x)` per basis index; an unused code is never good) about
/// the register's current state psi, in place of a projection. With a0 = the good mass of psi and theta = asin(sqrt(a0)),
/// k = floor(pi / (4 theta) - 1/2) standard iterations (oracle phase pi, diffusion phase pi about psi) bring the state to angle
/// (2k + 1) theta <= pi/2 in the plane of |good> (psi's normalised good part) and |bad>; one final iteration with phases
/// (phi, varphi) chosen so that its bad component is 0 lands on |good> exactly (Long's exact search, one matched step). Every
/// step stays in that plane, so the result is e^{i chi} |good>: the good states keep psi's relative amplitudes, the Born
/// probabilities are psi's restricted to the good states and divided by a0 -- the projection's numbers, by a unitary
/// evolution. a0 = 1: nothing to do (0 iterations); a0 = 0: None (nothing to amplify). Round 5: a landing whose mass after is
/// below 1 - 1e-12 (rounding drift over many iterations at a tiny a0, measured with the long sweep) is also None, so the tier
/// declines to the default tiers instead of panicking; the register is then left mid-rotation and must not be read.
/// Round 7: psi is renormalised once before the standard iterations, when there are any (its squared norm summed with
/// compensation; a NaN mass declines like a miss). The sweep's layers
/// leave it 1 + eps, a larger eps the more layers; the diffusion about such a psi is not unitary (|D v|^2 = |v|^2 + 4 eps |<psi|v>|^2),
/// so the error grows with every iteration, linearly in k: that, not the rotation, was the ~1e-12 shortfall of the long sweep's
/// 165 corpus programs (k in the hundreds at a tiny a0), and the same eps of the other sign is round 4's drift above 1.
pub fn amplify_exact(r: &mut Register, good: impl Fn(usize) -> bool) -> Option<Amplified> {
    let g: Vec<bool> = (0..r.amp.len()).map(&good).collect(); let mass = |r: &Register| r.amp.iter().zip(&g).filter(|(_, &x)| x).map(|(a, _)| a.norm2()).sum::<f64>();
    let a0 = mass(r); if !(a0 > 0.0) { return None; }
    if a0 >= 1.0 - 1e-15 { return Some(Amplified { iterations: 0, phases: (0.0, 0.0), mass_before: a0, mass_after: a0 }); }
    let pi = core::f64::consts::PI; let k_of = |a0: f64| m::floor(pi / (4.0 * m::asin(m::sqrt(a0))) - 0.5).max(0.0) as usize;
    // renormalise only when there are standard iterations to drift over (k >= 1, so a0 <= 1/4): with none, the one matched step
    // is round 4's, unchanged, and a mass near 1 cannot round above 1. The receipt's mass before stays the swept state's (a0);
    // the rotation uses the renormalised state's.
    let mass_before = a0;
    let a0 = if k_of(a0) == 0 { a0 } else { let u = 1.0 / m::sqrt(compensated_norm2(&r.amp)); for a in r.amp.iter_mut() { *a = C::new(a.re * u, a.im * u); } mass(r) };
    let psi = r.amp.clone();
    let k = k_of(a0);
    for _ in 0..k { r.oracle(&g, pi); r.diffuse(&psi, pi); }
    // the final step: the state is +-(al |good> + be |bad>), al, be >= 0; with s = sin theta, c = cos theta, S_psi(varphi)
    // S_f(phi) leaves be - (1 - e^{i varphi}) c (al s e^{i phi} + be c) on |bad>: it vanishes for
    // cos phi = -(be / al) cos(2 theta) / sin(2 theta) (|cos phi| < 1 for this k) and 1 - e^{i varphi} = be / (c (al s e^{i phi} + be c))
    let a = mass(r).min(1.0); let (al, be) = (m::sqrt(a), m::sqrt((1.0 - a).max(0.0))); let (s, c) = (m::sqrt(a0), m::sqrt(1.0 - a0));
    let cphi = (-(be / al) * (1.0 - 2.0 * a0) / (2.0 * s * c)).clamp(-1.0, 1.0); let phi = m::acos(cphi);
    let w = C::new(al * s * m::cos(phi) + be * c, al * s * m::sin(phi)); let den = w.norm2() * c; // z = be / (c w) = be conj(w) / (c |w|^2)
    let z = C::new(be * w.re / den, -be * w.im / den); let vphi = m::atan2(-z.im, 1.0 - z.re);
    r.oracle(&g, phi); r.diffuse(&psi, vphi);
    let m = mass(r); if !(m >= AMPLIFIED_MASS_MIN) { return None; } // a NaN declines too
    Some(Amplified { iterations: k + 1, phases: (phi, vphi), mass_before, mass_after: m })
}

/// Round 7: sum |a_x|^2 with Neumaier's compensation (the plain running sum of 2^16 terms can be off by ~1e-14, the size of the
/// drift the renormalisation removes)
fn compensated_norm2(amp: &[C]) -> f64 {
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for a in amp { let x = a.norm2(); let t = s + x; c += if s >= x { (s - t) + x } else { (x - t) + s }; s = t; } // s, x >= 0
    s + c
}

/// sha256 (FIPS 180-4), one shot
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d,
        0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec(); msg.push(0x80); while msg.len() % 64 != 56 { msg.push(0); } msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for blk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([blk[4 * i], blk[4 * i + 1], blk[4 * i + 2], blk[4 * i + 3]]); }
        for i in 16..64 { let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3); let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1); }
        let mut v = h;
        for i in 0..64 { let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25); let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22); let mj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]); let t2 = s0.wrapping_add(mj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]]; }
        for i in 0..8 { h[i] = h[i].wrapping_add(v[i]); }
    }
    let mut out = [0u8; 32]; for i in 0..8 { out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes()); } out
}
/// Lower-case hex
pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_gate_is_unitary_to_1e_12() {
        for t in [0.0, 0.3, -1.7, 2.5, 3.14159, 10.0] {
            for g in [Gate::H(0), Gate::X(0), Gate::Z(0), Gate::Rx(0, t), Gate::Rz(0, t), Gate::Cnot(0, 1), Gate::Cz(0, 1), Gate::Rzz(0, 1, t)] { assert!(unitarity_error(&g) < 1e-12, "{g:?}"); }
        }
    }
    #[test]
    fn memory_is_16_times_2_to_the_q() { for q in [0, 1, 8, 12] { assert_eq!(Register::new(q).bytes(), 16 << q); } }
    #[test]
    fn bell_state_interference_and_norm() {
        let mut r = Register::new(2); r.apply(Gate::H(0)); r.apply(Gate::Cnot(0, 1));
        let p = r.probabilities(); assert!((p[0] - 0.5).abs() < 1e-15 && (p[3] - 0.5).abs() < 1e-15 && p[1] < 1e-30 && p[2] < 1e-30);
        // H Z H = X; two H in a row cancel (destructive interference on |1>)
        let mut r = Register::new(1); r.apply(Gate::H(0)); r.apply(Gate::H(0)); assert!(r.probabilities()[1] < 1e-30);
        let mut r = Register::new(1); r.apply(Gate::H(0)); r.apply(Gate::Z(0)); r.apply(Gate::H(0)); assert!((r.probabilities()[1] - 1.0).abs() < 1e-15);
        let p = Ising { q: 3, a: vec![0.4, -0.2, 0.1], b: vec![(0, 1, 0.6), (1, 2, -0.3)] };
        let r = evolve_ising(&p, &DEFAULT_SCHEDULE); assert!((r.probabilities().iter().sum::<f64>() - 1.0).abs() < 1e-12);
        let r = qaoa(&p, 2, &[(0.4, 0.7), (0.2, 0.3)]); assert!((r.probabilities().iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }
    #[test]
    fn three_level_codes_stay_empty_and_the_mixer_is_unitary() {
        for d in 1..=8 { for t in [0.0, 0.3, -1.7, 2.5, 10.0] { assert!(mixer_unitarity_error(d, t) < 1e-12, "d {d} t {t}"); } }
        assert_eq!((Encoding::bits(1), Encoding::bits(2), Encoding::bits(3), Encoding::bits(4), Encoding::bits(5)), (0, 1, 2, 2, 3));
        // d = 2: the complete-graph mixer is RX(-2t) exactly
        let m = mixer_matrix(2, 0.7); let g = Gate::Rx(0, -1.4).matrix(); for k in 0..4 { assert!((m[k].re - g[k].re).abs() < 1e-15 && (m[k].im - g[k].im).abs() < 1e-15); }
        // a three-level variable, a two-valued flag and a three-level variable with a pair table favouring the diagonal
        let enc = Encoding { levels: vec![3, 2, 3] }; assert_eq!(enc.qubits(), 5);
        let lw = |v: &[usize]| [0.4, -0.2, 0.9][v[0]] + if v[1] == 1 { 0.3 } else { 0.0 } + if v[0] == v[2] { 1.1 } else { 0.0 } + if v[1] == 1 && v[2] == 2 { -0.5 } else { 0.0 };
        let e: Vec<f64> = (0..32).map(|x| enc.decode(x).map_or(f64::NAN, |v| -lw(&v))).collect();
        let r = sweep(&enc, &e, &DEFAULT_SCHEDULE); assert!(r.invalid_mass(&e) == 0.0);
        assert!((r.probabilities().iter().sum::<f64>() - 1.0).abs() < 1e-12);
        let lm = r.level_marginals(&enc); assert_eq!(lm.len(), 8); for (o, d) in [(0, 3), (3, 2), (5, 3)] { assert!((lm[o..o + d].iter().sum::<f64>() - 1.0).abs() < 1e-12); }
        let r = qaoa_on(&enc, &e, &[(0.4, 0.7), (0.9, 0.2)]); assert!(r.invalid_mass(&e) == 0.0);
        // the uniform start over the valid codes: 18 of 32 basis states
        assert_eq!(Register::uniform(&enc).probabilities().iter().filter(|&&p| p > 0.0).count(), 18);
    }
    #[test]
    fn exact_amplification_lands_on_the_projection() {
        // a three-level, a two-valued and a three-level variable; good = a rule over the plans; masses from 0.5 down to ~1e-3
        let enc = Encoding { levels: vec![3, 2, 3] };
        let lw = |v: &[usize]| [0.4, -0.2, 0.9][v[0]] + if v[1] == 1 { 0.3 } else { 0.0 } + if v[0] == v[2] { 1.1 } else { 0.0 } + if v[1] == 1 && v[2] == 2 { -0.5 } else { 0.0 };
        let e: Vec<f64> = (0..32).map(|x| enc.decode(x).map_or(f64::NAN, |v| -lw(&v))).collect();
        for rule in 0..18usize {
            let good = |x: usize| enc.decode(x).is_some_and(|v| (v[0] * 6 + v[1] * 3 + v[2] + rule) % 18 < 1 + rule % 9);
            let sw = sweep(&enc, &e, &DEFAULT_SCHEDULE); let p = sw.probabilities(); let a0: f64 = (0..32).filter(|&x| good(x)).map(|x| p[x]).sum();
            let mut r = sw.clone(); let am = amplify_exact(&mut r, good).unwrap(); assert!(am.mass_after >= AMPLIFIED_MASS_MIN && (am.mass_before - a0).abs() < 1e-15);
            let q = r.probabilities(); for x in 0..32 { let want = if good(x) { p[x] / a0 } else { 0.0 }; assert!((q[x] - want).abs() < 1e-12, "rule {rule} x {x}: {} vs {want}", q[x]); }
            assert_eq!(r.invalid_mass(&e), 0.0);
        }
        let mut r = Register::uniform(&enc); assert!(amplify_exact(&mut r, |_| false).is_none());
        let mut r = Register::uniform(&enc); assert_eq!(amplify_exact(&mut r, |x| enc.decode(x).is_some()).unwrap().iterations, 0);
    }
    /// Round 5: the named evolutions: the default name is `sweep` with the default schedule (the same register, text and name),
    /// the long sweep has 4x the layers, qaoa-p2 two layers with the frozen angles; an unknown name is None
    #[test]
    fn named_schedules_run_what_they_name() {
        let enc = Encoding { levels: vec![3, 2] }; let e: Vec<f64> = (0..8).map(|x| enc.decode(x).map_or(f64::NAN, |v| -[0.4, -0.2, 0.9][v[0]] - 0.3 * v[1] as f64)).collect();
        let d = Evolution::named(SCHEDULE_NAMES[0]).unwrap(); assert_eq!(d, Evolution::Sweep(DEFAULT_SCHEDULE));
        assert_eq!(d.run(&enc, &e).amplitudes(), sweep(&enc, &e, &DEFAULT_SCHEDULE).amplitudes()); assert_eq!((d.text(), d.name()), (DEFAULT_SCHEDULE.text(), DEFAULT_SCHEDULE.name()));
        let l = Evolution::named(SCHEDULE_NAMES[1]).unwrap(); assert_eq!(l.run(&enc, &e).circuit_text().lines().count(), 4 * d.run(&enc, &e).circuit_text().lines().count());
        let q = Evolution::named(SCHEDULE_NAMES[2]).unwrap(); assert_eq!(q.name(), "qaoa-p2"); assert_eq!(q.run(&enc, &e).amplitudes(), qaoa_on(&enc, &e, &QAOA_P2).amplitudes());
        assert_eq!(q.text(), "qaoa layers 2\ngamma 0.500000 beta 0.500000\ngamma 1.500000 beta 0.250000\n"); assert!(Evolution::named("tf-linear-8x2").is_none());
    }
    /// Round 7: a psi whose squared norm is off by 1e-12 (a long sweep leaves ~1e-14) and one good state of 2^14, so about a
    /// hundred iterations: without the renormalisation the landing drifts by ~2 k eps (~2e-10); with it the mass stays within
    /// 1e-12 of 1 and the good state's probability is 1
    #[test]
    fn a_landing_after_a_hundred_iterations_keeps_its_mass() {
        let enc = Encoding::two_valued(14); let mut r = Register::uniform(&enc); let f = (1.0 + 1e-12f64).sqrt();
        for a in r.amp.iter_mut() { *a = C::new(a.re * f, a.im * f); }
        let am = amplify_exact(&mut r, |x| x == 5).unwrap(); assert!(am.iterations > 90, "{}", am.iterations);
        assert!((am.mass_after - 1.0).abs() <= 1e-12, "mass after {:e}", am.mass_after - 1.0); assert!((r.probabilities()[5] - 1.0).abs() <= 1e-12);
        assert!((compensated_norm2(r.amplitudes()) - 1.0).abs() <= 1e-12);
    }
    #[test]
    fn measure_and_digest_are_deterministic() {
        let p = Ising { q: 4, a: vec![0.5, 0.1, -0.3, 0.2], b: vec![(0, 3, 0.4)] };
        let (r1, r2) = (evolve_ising(&p, &DEFAULT_SCHEDULE), evolve_ising(&p, &DEFAULT_SCHEDULE));
        assert_eq!(r1.digest(), r2.digest()); assert_eq!(r1.measure(7), r2.measure(7)); assert_eq!(r1.circuit_text(), r2.circuit_text());
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(f6(-0.0000001), "0.000000");
    }
}
