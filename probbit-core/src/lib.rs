//! probbit-core: RNGs and p-bit kernels.
//! - `Philox4x32`: counter-based, reproducible, stream-splittable RNG (Salmon et al. 2011, Philox4x32-10).
//! - `SplitMix64`: fast RNG identical to the one in the earlier C benchmarks (for apples-to-apples kernel ratios).
//! - `multispin`: T0 bit-sliced 2D ±J kernel, 64 replicas per u64, one shared uniform per site.
//!   `sweep_prefix` is the PT-in-a-word variant (per-lane betas, shared uniform -> contiguous lane prefix mask).
//! - `heatbath_f32`: T1 SoA f32 heat-bath on a 2D ±J lattice with LUT sigmoid.
//! - `rt`: the engine's clock and its no-thread switch (for targets without threads or a std clock: wasm32-unknown-unknown).

#[derive(Clone)]
pub struct Philox4x32 { key: [u32; 2], ctr: [u32; 4], buf: [u32; 4], idx: usize }
impl Philox4x32 {
    pub fn new(seed: u64, stream: u64) -> Self {
        Philox4x32 { key: [seed as u32, (seed >> 32) as u32], ctr: [0, 0, stream as u32, (stream >> 32) as u32], buf: [0; 4], idx: 4 }
    }
    #[inline(always)]
    fn block(&self) -> [u32; 4] {
        let (mut c, mut k) = (self.ctr, self.key);
        for _ in 0..10 {
            let p0 = (0xD2511F53u32 as u64) * (c[0] as u64);
            let p1 = (0xCD9E8D57u32 as u64) * (c[2] as u64);
            c = [((p1 >> 32) as u32) ^ c[1] ^ k[0], p1 as u32, ((p0 >> 32) as u32) ^ c[3] ^ k[1], p0 as u32];
            k = [k[0].wrapping_add(0x9E3779B9), k[1].wrapping_add(0xBB67AE85)];
        }
        c
    }
    #[inline(always)]
    pub fn next_u32(&mut self) -> u32 {
        if self.idx == 4 {
            self.buf = self.block();
            self.ctr[0] = self.ctr[0].wrapping_add(1);
            if self.ctr[0] == 0 { self.ctr[1] = self.ctr[1].wrapping_add(1); }
            self.idx = 0;
        }
        let v = self.buf[self.idx]; self.idx += 1; v
    }
    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 { ((self.next_u32() as u64) << 32) | self.next_u32() as u64 }
    /// uniform in [0,1)
    #[inline(always)]
    pub fn f64(&mut self) -> f64 { (self.next_u64() >> 11) as f64 * (1.0 / 9007199254740992.0) }
    #[inline(always)]
    pub fn below(&mut self, n: usize) -> usize { ((self.next_u32() as u64 * n as u64) >> 32) as usize }
}

#[derive(Clone)]
pub struct SplitMix64(pub u64);
impl SplitMix64 {
    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
}

pub trait Rng32 { fn u32(&mut self) -> u32; }
impl Rng32 for SplitMix64 { #[inline(always)] fn u32(&mut self) -> u32 { self.next_u64() as u32 } }
impl Rng32 for Philox4x32 { #[inline(always)] fn u32(&mut self) -> u32 { self.next_u32() } }


/// 2D ±J periodic lattice, bit-sliced: w[i] holds spin i of 64 replicas (bit=1 means -1).
pub mod multispin {
    use super::Rng32;
    /// The fields are private: `sweep_fast` reads through raw pointers and relies on `w`, `jr` and `jd` holding exactly `l * l`
    /// words. With `pub` fields safe code could break that (`l = 2^32` on empty vectors: `l * l` wrapped to 0 in release, the
    /// length check passed, the sweep read through a dangling pointer). Build one with `random` or `new`.
    pub struct Lattice { l: usize, w: Vec<u64>, jr: Vec<u64>, jd: Vec<u64> }
    impl Lattice {
        pub fn random(l: usize, rng: &mut super::SplitMix64) -> Self {
            let n = l.checked_mul(l).expect("l * l overflows usize");
            let jr = (0..n).map(|_| if rng.next_u64() & 1 == 1 { !0 } else { 0 }).collect();
            let jd = (0..n).map(|_| if rng.next_u64() & 1 == 1 { !0 } else { 0 }).collect();
            let w = (0..n).map(|_| rng.next_u64()).collect();
            Lattice { l, w, jr, jd }
        }
        /// A lattice from its spins `w` and couplings `jr` (right), `jd` (down), row-major; None unless each holds `l * l` words.
        pub fn new(l: usize, w: Vec<u64>, jr: Vec<u64>, jd: Vec<u64>) -> Option<Self> {
            let n = l.checked_mul(l)?; (w.len() == n && jr.len() == n && jd.len() == n).then_some(Lattice { l, w, jr, jd })
        }
        pub fn l(&self) -> usize { self.l }
        pub fn w(&self) -> &[u64] { &self.w }
        pub fn jr(&self) -> &[u64] { &self.jr }
        pub fn jd(&self) -> &[u64] { &self.jd }
        /// One checkerboard Metropolis sweep, all 64 lanes at one beta (thresholds th4=exp(-4b), th8=exp(-8b) in u32 scale).
        #[inline(never)]
        pub fn sweep<R: Rng32>(&mut self, th4: u32, th8: u32, rng: &mut R) {
            self.sweep_masks(rng, |r| (if r < th4 { !0 } else { 0 }, if r < th8 { !0 } else { 0 }));
        }
        /// PT-in-a-word: lane k has beta_k (sorted ascending => thresholds descending); shared uniform r
        /// selects a contiguous prefix of lanes that accept (binary search) — the prefix-mask trick.
        #[inline(never)]
        pub fn sweep_prefix<R: Rng32>(&mut self, th4: &[u32; 64], th8: &[u32; 64], rng: &mut R) {
            let pm = |th: &[u32; 64], r: u32| -> u64 {
                let (mut lo, mut hi) = (0usize, 64usize);
                while lo < hi { let m = (lo + hi) >> 1; if th[m] > r { lo = m + 1 } else { hi = m } }
                if lo == 64 { !0 } else { (1u64 << lo) - 1 }
            };
            self.sweep_masks(rng, |r| (pm(th4, r), pm(th8, r)));
        }
        #[inline(always)]
        fn sweep_masks<R: Rng32, F: Fn(u32) -> (u64, u64)>(&mut self, rng: &mut R, masks: F) {
            let l = self.l; let (w, jr, jd) = (&mut self.w, &self.jr, &self.jd);
            for c in 0..2 { for y in 0..l {
                let yu = (y + l - 1) % l; let yd = (y + 1) % l;
                let mut x = (y + c) & 1;
                while x < l {
                    let xl = if x == 0 { l - 1 } else { x - 1 }; let xr = if x + 1 == l { 0 } else { x + 1 }; let i = y * l + x;
                    let si = w[i];
                    let b0 = si ^ w[y * l + xr] ^ jr[i];
                    let b1 = si ^ w[y * l + xl] ^ jr[y * l + xl];
                    let b2 = si ^ w[yd * l + x] ^ jd[i];
                    let b3 = si ^ w[yu * l + x] ^ jd[yu * l + x];
                    let any = b0 | b1 | b2 | b3;
                    let ge2 = (b0 & b1) | (b0 & b2) | (b0 & b3) | (b1 & b2) | (b1 & b3) | (b2 & b3);
                    let eq1 = any & !ge2;
                    let (m4, m8) = masks(rng.u32());
                    w[i] = si ^ (ge2 | (eq1 & m4) | (!any & m8));
                    x += 2;
                }
            }}
        }

        /// Tuned kernel (same math as `sweep`, bit-identical output for the same RNG stream): raw-pointer row
        /// access (no bounds checks), wrap-around only at the two row ends, no per-site modulo.
        #[inline(never)]
        pub fn sweep_fast<R: Rng32>(&mut self, th4: u32, th8: u32, rng: &mut R) {
            let l = self.l; let n = l.checked_mul(l); // checked: a wrapped l * l passed this assert
            assert!(l >= 4 && l % 2 == 0 && n.is_some_and(|n| self.w.len() == n && self.jr.len() == n && self.jd.len() == n));
            let w = self.w.as_mut_ptr(); let (jr, jd) = (self.jr.as_ptr(), self.jd.as_ptr());
            for c in 0..2 { for y in 0..l {
                let yu = if y == 0 { l - 1 } else { y - 1 }; let yd = if y + 1 == l { 0 } else { y + 1 };
                // SAFETY: all indices are < l*l by construction (x < l, rows < l), lengths asserted above.
                unsafe {
                    let row = w.add(y * l); let up = w.add(yu * l); let dn = w.add(yd * l);
                    let jrr = jr.add(y * l); let jdr = jd.add(y * l); let jdu = jd.add(yu * l);
                    let site = |x: usize, xl: usize, xr: usize, r: u32| {
                        let si = *row.add(x);
                        let b0 = si ^ *row.add(xr) ^ *jrr.add(x);
                        let b1 = si ^ *row.add(xl) ^ *jrr.add(xl);
                        let b2 = si ^ *dn.add(x) ^ *jdr.add(x);
                        let b3 = si ^ *up.add(x) ^ *jdu.add(x);
                        let any = b0 | b1 | b2 | b3;
                        let ge2 = (b0 & b1) | (b0 & b2) | (b0 & b3) | (b1 & b2) | (b1 & b3) | (b2 & b3);
                        let eq1 = any & !ge2;
                        let m4 = ((r < th4) as u64).wrapping_neg(); let m8 = ((r < th8) as u64).wrapping_neg();
                        *row.add(x) = si ^ (ge2 | (eq1 & m4) | (!any & m8));
                    };
                    let x0 = (y + c) & 1;
                    // first site of the row (x0 = 0 wraps left) , interior, last site (x = l-1 wraps right)
                    let mut x = x0;
                    if x == 0 { site(0, l - 1, 1, rng.u32()); x = 2; }
                    while x + 1 < l { site(x, x - 1, x + 1, rng.u32()); x += 2; }
                    if x == l - 1 { site(x, x - 1, 0, rng.u32()); }
                }
            }}
        }
        /// Energy per lane (E = -sum J s s).
        pub fn energies(&self) -> [i64; 64] {
            let l = self.l; let mut un = [0i64; 64];
            for y in 0..l { for x in 0..l { let i = y * l + x;
                let mut a = self.w[i] ^ self.w[y * l + (x + 1) % l] ^ self.jr[i];
                let mut b = self.w[i] ^ self.w[((y + 1) % l) * l + x] ^ self.jd[i];
                while a != 0 { un[a.trailing_zeros() as usize] += 1; a &= a - 1; }
                while b != 0 { un[b.trailing_zeros() as usize] += 1; b &= b - 1; }
            }}
            let n = (l * l) as i64; let mut e = [0i64; 64];
            for k in 0..64 { e[k] = 2 * un[k] - 2 * n; } e
        }
    }
    pub fn thresholds(beta: f64) -> (u32, u32) {
        ((((-4.0 * beta).exp()) * 4294967295.0) as u32, (((-8.0 * beta).exp()) * 4294967295.0) as u32)
    }
}

/// T1: SoA f32 heat-bath on 2D ±J lattice, LUT for tanh(beta*h), h in {-4,-2,0,2,4}.
pub mod heatbath_f32 {
    use super::Rng32;
    /// The fields are private: besides the `multispin` lengths, `sweep_fast` reads its 9-entry table at h + 4 unchecked, which
    /// is in range only for ±1 spins and couplings (`pub` couplings of 1000.0 read past the table). `random` and `new` build
    /// only such lattices, and the sweeps write only ±1.
    pub struct Lattice { l: usize, s: Vec<f32>, jr: Vec<f32>, jd: Vec<f32> }
    impl Lattice {
        pub fn random(l: usize, rng: &mut super::SplitMix64) -> Self {
            let n = l.checked_mul(l).expect("l * l overflows usize");
            let pm = |r: &mut super::SplitMix64| if r.next_u64() & 1 == 1 { -1.0 } else { 1.0 };
            let jr = (0..n).map(|_| pm(rng)).collect(); let jd = (0..n).map(|_| pm(rng)).collect();
            let s = (0..n).map(|_| pm(rng)).collect();
            Lattice { l, s, jr, jd }
        }
        /// A lattice from its spins `s` and couplings `jr` (right), `jd` (down), row-major; None unless each holds `l * l`
        /// values and every value is exactly 1.0 or -1.0.
        pub fn new(l: usize, s: Vec<f32>, jr: Vec<f32>, jd: Vec<f32>) -> Option<Self> {
            let n = l.checked_mul(l)?; let pm = |v: &f32| *v == 1.0 || *v == -1.0;
            (s.len() == n && jr.len() == n && jd.len() == n && s.iter().chain(&jr).chain(&jd).all(pm)).then_some(Lattice { l, s, jr, jd })
        }
        pub fn l(&self) -> usize { self.l }
        pub fn s(&self) -> &[f32] { &self.s }
        pub fn jr(&self) -> &[f32] { &self.jr }
        pub fn jd(&self) -> &[f32] { &self.jd }
        #[inline(never)]
        pub fn sweep<R: Rng32>(&mut self, lut: &[f32; 9], rng: &mut R) {
            let l = self.l; let (s, jr, jd) = (&mut self.s, &self.jr, &self.jd);
            for c in 0..2 { for y in 0..l {
                let yu = (y + l - 1) % l; let yd = (y + 1) % l;
                let mut x = (y + c) & 1;
                while x < l {
                    let xl = if x == 0 { l - 1 } else { x - 1 }; let xr = if x + 1 == l { 0 } else { x + 1 }; let i = y * l + x;
                    let h = jr[i] * s[y * l + xr] + jr[y * l + xl] * s[y * l + xl] + jd[i] * s[yd * l + x] + jd[yu * l + x] * s[yu * l + x];
                    let u = (rng.u32() >> 8) as f32 * (1.0 / 16777216.0) * 2.0 - 1.0;
                    s[i] = if lut[(h as i32 + 4) as usize] > u { 1.0 } else { -1.0 };
                    x += 2;
                }
            }}
        }

        /// Tuned: raw-pointer rows, no modulo, branch-free select, unchecked LUT. Same output as `sweep` for the same RNG.
        #[inline(never)]
        pub fn sweep_fast<R: Rng32>(&mut self, lut: &[f32; 9], rng: &mut R) {
            let l = self.l; let n = l.checked_mul(l); // checked: a wrapped l * l passed this assert
            assert!(l >= 4 && l % 2 == 0 && n.is_some_and(|n| self.s.len() == n && self.jr.len() == n && self.jd.len() == n));
            let s = self.s.as_mut_ptr(); let (jr, jd) = (self.jr.as_ptr(), self.jd.as_ptr()); let lp = lut.as_ptr();
            for c in 0..2 { for y in 0..l {
                let yu = if y == 0 { l - 1 } else { y - 1 }; let yd = if y + 1 == l { 0 } else { y + 1 };
                // SAFETY: indices < l*l (asserted); h is an integer in [-4,4] for ±1 spins and ±1 couplings -> LUT index in 0..9.
                // The fields are private and every lattice holds only ±1 values (`random`, `new`; the sweeps write ±1).
                unsafe {
                    let row = s.add(y * l); let up = s.add(yu * l); let dn = s.add(yd * l);
                    let jrr = jr.add(y * l); let jdr = jd.add(y * l); let jdu = jd.add(yu * l);
                    let site = |x: usize, xl: usize, xr: usize, r: u32| {
                        let h = *jrr.add(x) * *row.add(xr) + *jrr.add(xl) * *row.add(xl) + *jdr.add(x) * *dn.add(x) + *jdu.add(x) * *up.add(x);
                        let u = (r >> 8) as f32 * (1.0 / 16777216.0) * 2.0 - 1.0;
                        *row.add(x) = if *lp.add((h as i32 + 4) as usize) > u { 1.0 } else { -1.0 };
                    };
                    let mut x = (y + c) & 1;
                    if x == 0 { site(0, l - 1, 1, rng.u32()); x = 2; }
                    while x + 1 < l { site(x, x - 1, x + 1, rng.u32()); x += 2; }
                    if x == l - 1 { site(x, x - 1, 0, rng.u32()); }
                }
            }}
        }
    }
    pub fn lut(beta: f32) -> [f32; 9] { let mut t = [0f32; 9]; for k in 0..9 { t[k] = (beta * (k as f32 - 4.0)).tanh(); } t }
}

/// The runtime probbit-ir and probbit-decide run on: their clock and whether they may spawn threads.
///
/// `Instant` is `std::time::Instant` on every target that has one (the CLI: unchanged behaviour, the same type). On
/// wasm32-unknown-unknown (a browser), where `std::time::Instant::now()` panics, it is a millisecond instant read from a clock
/// function the embedder installs with `set_clock` (e.g. `performance.now()`); reading it before `set_clock` panics.
///
/// `sequential()` (per thread; on by default only on wasm32-unknown-unknown, where a thread spawn fails): every parallel section
/// of probbit-ir and probbit-decide (chains, gate passes, polish, the exact tiers' deep-stack thread) runs its work in order on the
/// calling thread instead of on scoped threads, as one worker would (`--threads 1`). A chain's random stream depends on its index
/// only, so at fixed work the answer is the same as on any number of threads. Stack depth is then the caller's: the deep-stack
/// threads sized for > 2,048-variable programs are not used.
pub mod rt {
    use std::cell::Cell;
    thread_local! { static SEQUENTIAL: Cell<bool> = const { Cell::new(cfg!(all(target_family = "wasm", target_os = "unknown"))) }; }
    /// No threads on this thread's calls into the engine (see the module doc). Returns the previous setting.
    pub fn set_sequential(on: bool) -> bool { SEQUENTIAL.with(|s| s.replace(on)) }
    /// Whether this thread's engine calls run without threads.
    pub fn sequential() -> bool { SEQUENTIAL.with(|s| s.get()) }

    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    pub use std::time::Instant;
    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    pub use clock::{set_clock, Instant};
    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    mod clock {
        use std::cell::Cell;
        use std::time::Duration;
        thread_local! { static NOW_MS: Cell<Option<fn() -> f64>> = const { Cell::new(None) }; }
        /// Install the clock: a function returning monotonic milliseconds (e.g. JavaScript's `performance.now()`).
        pub fn set_clock(now_ms: fn() -> f64) { NOW_MS.with(|c| c.set(Some(now_ms))) }
        /// A point in time in milliseconds of the installed clock (the subset of `std::time::Instant` the engine uses).
        #[derive(Clone, Copy, Debug)]
        pub struct Instant(f64);
        impl Instant {
            pub fn now() -> Instant { Instant(NOW_MS.with(|c| c.get()).expect("probbit_core::rt::set_clock was not called")()) }
            pub fn elapsed(&self) -> Duration { Instant::now().saturating_duration_since(*self) }
            pub fn duration_since(&self, earlier: Instant) -> Duration { self.saturating_duration_since(earlier) }
            pub fn saturating_duration_since(&self, earlier: Instant) -> Duration { Duration::from_secs_f64((self.0 - earlier.0).max(0.0) / 1e3) }
        }
        impl PartialEq for Instant { fn eq(&self, o: &Instant) -> bool { self.0 == o.0 } }
        impl Eq for Instant {}
        impl PartialOrd for Instant { fn partial_cmp(&self, o: &Instant) -> Option<std::cmp::Ordering> { Some(self.cmp(o)) } }
        impl Ord for Instant { fn cmp(&self, o: &Instant) -> std::cmp::Ordering { self.0.total_cmp(&o.0) } }
        impl std::ops::Add<Duration> for Instant { type Output = Instant; fn add(self, d: Duration) -> Instant { Instant(self.0 + d.as_secs_f64() * 1e3) } }
        impl std::ops::Sub<Instant> for Instant { type Output = Duration; fn sub(self, o: Instant) -> Duration { self.saturating_duration_since(o) } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn philox_reproducible_and_streams_differ() {
        let mut a = Philox4x32::new(42, 0); let mut b = Philox4x32::new(42, 0); let mut c = Philox4x32::new(42, 1);
        let va: Vec<u32> = (0..16).map(|_| a.next_u32()).collect();
        let vb: Vec<u32> = (0..16).map(|_| b.next_u32()).collect();
        let vc: Vec<u32> = (0..16).map(|_| c.next_u32()).collect();
        assert_eq!(va, vb); assert_ne!(va, vc);
        let mut r = Philox4x32::new(7, 3); let m: f64 = (0..200000).map(|_| r.f64()).sum::<f64>() / 200000.0;
        assert!((m - 0.5).abs() < 0.005);
    }
    #[test]
    fn multispin_ferromagnet_orders_at_low_t() {
        // all-ferro couplings (J=+1 -> jr bits 0); at beta=1.0 (> beta_c=0.4407) lanes should order: |m| large
        let mut g = SplitMix64(1); let r = multispin::Lattice::random(32, &mut g);
        let mut lat = multispin::Lattice::new(32, r.w().to_vec(), vec![0; 32 * 32], vec![0; 32 * 32]).unwrap();
        let (t4, t8) = multispin::thresholds(1.0);
        for _ in 0..400 { lat.sweep(t4, t8, &mut g); }
        let e = lat.energies(); let n = 32 * 32;
        let mean_e: f64 = e.iter().map(|&x| x as f64 / n as f64).sum::<f64>() / 64.0;
        assert!(mean_e < -1.8, "mean energy per spin {mean_e}"); // exact 2D Ising at beta=1: ~ -1.997
    }
    #[test]
    fn fast_kernels_bit_identical_to_reference() {
        for l in [4usize, 6, 32] {
            let mut g = SplitMix64(5); let mut a = multispin::Lattice::random(l, &mut g); let mut b = multispin::Lattice::new(l, a.w().to_vec(), a.jr().to_vec(), a.jd().to_vec()).unwrap();
            let (t4, t8) = multispin::thresholds(0.6); let (mut r1, mut r2) = (Philox4x32::new(9, 1), Philox4x32::new(9, 1));
            for _ in 0..20 { a.sweep(t4, t8, &mut r1); b.sweep_fast(t4, t8, &mut r2); } assert_eq!(a.w(), b.w());
            let mut g = SplitMix64(6); let mut a = heatbath_f32::Lattice::random(l, &mut g); let mut b = heatbath_f32::Lattice::new(l, a.s().to_vec(), a.jr().to_vec(), a.jd().to_vec()).unwrap();
            let lut = heatbath_f32::lut(0.6); let (mut r1, mut r2) = (SplitMix64(3), SplitMix64(3));
            for _ in 0..20 { a.sweep(&lut, &mut r1); b.sweep_fast(&lut, &mut r2); } assert_eq!(a.s(), b.s());
        }
    }
    /// The 2026-10-01 review reached undefined behaviour in `sweep_fast` from safe code through the `pub` fields (exit 139):
    /// `l = 2^32` on empty vectors (`l * l` wrapped to 0) and heat-bath couplings of 1000.0 (a read past the 9-entry table).
    /// With private fields both lattices come only from these constructors, which refuse them.
    #[test]
    fn lattice_constructors_refuse_what_the_fast_kernels_cannot_take() {
        let big = 1usize << (usize::BITS / 2); // big * big overflows usize
        assert!(multispin::Lattice::new(big, vec![], vec![], vec![]).is_none());
        assert!(heatbath_f32::Lattice::new(big, vec![], vec![], vec![]).is_none());
        assert!(multispin::Lattice::new(4, vec![0; 15], vec![0; 16], vec![0; 16]).is_none());
        assert!(heatbath_f32::Lattice::new(4, vec![1.0; 16], vec![1000.0; 16], vec![1000.0; 16]).is_none());
        assert!(heatbath_f32::Lattice::new(4, vec![1.0; 16], vec![1.0; 16], vec![f32::NAN; 16]).is_none());
        let (mut a, mut b) = (heatbath_f32::Lattice::new(4, vec![1.0; 16], vec![1.0; 16], vec![-1.0; 16]).unwrap(), heatbath_f32::Lattice::new(4, vec![1.0; 16], vec![1.0; 16], vec![-1.0; 16]).unwrap());
        let (lut, mut r1, mut r2) = (heatbath_f32::lut(0.6), SplitMix64(3), SplitMix64(3));
        for _ in 0..20 { a.sweep(&lut, &mut r1); b.sweep_fast(&lut, &mut r2); } assert_eq!(a.s(), b.s());
        let mut m = multispin::Lattice::new(4, vec![!0; 16], vec![0; 16], vec![!0; 16]).unwrap(); let (t4, t8) = multispin::thresholds(0.6);
        m.sweep_fast(t4, t8, &mut SplitMix64(1)); assert_eq!((m.l(), m.w().len(), m.jr().len(), m.jd().len()), (4, 16, 16, 16));
    }
}
