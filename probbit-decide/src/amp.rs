//! RFC 0011 (research, round 1): an amplitude register. A register of q qubits simulated exactly on a CPU as a state vector of
//! 2^q complex amplitudes (f64 pairs, on the heap: 16 * 2^q bytes): unitary gates with phase, interference between basis states,
//! exact marginals (no sampling), measurement by the Born rule with the engine's deterministic RNG, and a digest of the state at
//! 6 decimals (the rule for every emitted float). A simulation of a small quantum system and nothing else: no quantum hardware,
//! no speedup claim, no scaling past what 2^q amplitudes allow.
//!
//! Basis index x = sum of bit_i << i: qubit i is bit i. For a program over two-valued variables, bit 0 is the variable's first
//! value and bit 1 its second; z_i = +1 for bit 0, -1 for bit 1.
#[allow(unused_imports)]
use std::prelude::rust_2021::*;
use probbit_core::Philox4x32;

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
    #[inline] pub fn cis(t: f64) -> C { C { re: t.cos(), im: t.sin() } }
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
        let half = |t: f64| (C::new((t / 2.0).cos(), 0.0), (t / 2.0).sin());
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

/// q qubits as 2^q complex amplitudes, and the list of gates applied (the circuit)
#[derive(Clone, Debug)]
pub struct Register { q: usize, amp: Vec<C>, gates: Vec<Gate> }
impl Register {
    /// |0...0> on q qubits (q <= Q_MAX_HOST); the amplitudes take exactly 16 * 2^q bytes of heap
    pub fn new(q: usize) -> Register {
        assert!(q <= Q_MAX_HOST, "an amplitude register has at most {Q_MAX_HOST} qubits on the host");
        let mut amp = vec![C::ZERO; 1usize << q]; amp[0] = C::ONE;
        let r = Register { q, amp, gates: vec![] };
        assert_eq!(r.bytes(), AMP_BYTES << q); r
    }
    pub fn qubits(&self) -> usize { self.q }
    /// The amplitudes' bytes: 16 * 2^q
    pub fn bytes(&self) -> usize { self.amp.len() * core::mem::size_of::<C>() }
    pub fn amplitudes(&self) -> &[C] { &self.amp }
    pub fn gates(&self) -> &[Gate] { &self.gates }
    /// The circuit as text, one gate per line (angles at 6 decimals)
    pub fn circuit_text(&self) -> String { let mut s = String::new(); for g in &self.gates { s.push_str(&g.text()); s.push('\n'); } s }

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
        self.gates.push(g);
    }
    /// The Born probabilities |a_x|^2
    pub fn probabilities(&self) -> Vec<f64> { self.amp.iter().map(|a| a.norm2()).collect() }
    /// Exact marginals P(qubit i = 1), one per qubit (no sampling)
    pub fn marginals(&self) -> Vec<f64> {
        let mut m = vec![0.0; self.q];
        for (x, a) in self.amp.iter().enumerate() { let p = a.norm2(); for (i, mi) in m.iter_mut().enumerate() { if x >> i & 1 == 1 { *mi += p; } } }
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
}

/// A transverse-field sweep: `steps` Trotter steps over total time `time`, s = (k + 1/2) / steps at step k,
/// H(s) = -(1 - s) sum X_i - s sum (a_i Z_i + b_ij Z_i Z_j), started from |+...+>
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Schedule { pub steps: usize, pub time: f64 }
impl Schedule {
    /// The canonical text the schedule digest hashes
    pub fn text(&self) -> String { format!("transverse-field linear steps {} time {}\n", self.steps, f6(self.time)) }
    pub fn name(&self) -> String { format!("tf-linear-{}x{}", self.steps, f6(self.time)) }
}
/// The tier's fixed default schedule (round 1; schedules are round 2's study)
pub const DEFAULT_SCHEDULE: Schedule = Schedule { steps: 16, time: 4.0 };

/// The cost layer exp(-i g H_P) with H_P = -(sum a_i Z_i + sum b_ij Z_i Z_j): RZ(-2 g a_i), RZZ(-2 g b_ij)
fn cost_layer(r: &mut Register, p: &Ising, g: f64) {
    for (i, &a) in p.a.iter().enumerate() { if a != 0.0 { r.apply(Gate::Rz(i, -2.0 * g * a)); } }
    for &(i, j, b) in &p.b { if b != 0.0 { r.apply(Gate::Rzz(i, j, -2.0 * g * b)); } }
}
/// The mixer layer exp(-i t H_D) with H_D = -sum X_i: RX(-2 t) on every qubit
fn mixer_layer(r: &mut Register, t: f64) { for i in 0..r.q { r.apply(Gate::Rx(i, -2.0 * t)); } }

/// The register (q = p.q, fresh) after H on every qubit and the Trotterised sweep of `s`
pub fn evolve_ising(p: &Ising, s: &Schedule) -> Register {
    let mut r = Register::new(p.q); for i in 0..p.q { r.apply(Gate::H(i)); }
    let dt = s.time / s.steps as f64;
    for k in 0..s.steps { let x = (k as f64 + 0.5) / s.steps as f64; cost_layer(&mut r, p, dt * x); mixer_layer(&mut r, dt * (1.0 - x)); }
    r
}
/// QAOA with p layers: H on every qubit, then per layer (gamma, beta) the cost layer and the mixer layer
pub fn qaoa(p: &Ising, layers: usize, params: &[(f64, f64)]) -> Register {
    assert_eq!(params.len(), layers, "one (gamma, beta) per layer");
    let mut r = Register::new(p.q); for i in 0..p.q { r.apply(Gate::H(i)); }
    for &(g, b) in params { cost_layer(&mut r, p, g); mixer_layer(&mut r, b); }
    r
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
    fn measure_and_digest_are_deterministic() {
        let p = Ising { q: 4, a: vec![0.5, 0.1, -0.3, 0.2], b: vec![(0, 3, 0.4)] };
        let (r1, r2) = (evolve_ising(&p, &DEFAULT_SCHEDULE), evolve_ising(&p, &DEFAULT_SCHEDULE));
        assert_eq!(r1.digest(), r2.digest()); assert_eq!(r1.measure(7), r2.measure(7)); assert_eq!(r1.circuit_text(), r2.circuit_text());
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(f6(-0.0000001), "0.000000");
    }
}
