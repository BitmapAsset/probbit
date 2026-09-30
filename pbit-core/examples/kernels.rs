//! Kernel bench, same shapes as the earlier C benchmark (2D ±J, L=512, 200 sweeps, beta=0.6).
use pbit_core::*;
use std::time::Instant;
fn main() {
    let (l, sw, beta) = (512usize, 200usize, 0.6f64); let n = (l * l) as f64;
    let mut g = SplitMix64(12345);
    let mut hb = heatbath_f32::Lattice::random(l, &mut g); let lut = heatbath_f32::lut(beta as f32);
    let t0 = Instant::now(); for _ in 0..sw { hb.sweep(&lut, &mut g); } let r1 = n * sw as f64 / t0.elapsed().as_secs_f64();
    let mut ph = Philox4x32::new(1, 0);
    let t0 = Instant::now(); for _ in 0..sw { hb.sweep(&lut, &mut ph); } let r1p = n * sw as f64 / t0.elapsed().as_secs_f64();
    let mut ms = multispin::Lattice::random(l, &mut g); let (t4, t8) = multispin::thresholds(beta);
    let t0 = Instant::now(); for _ in 0..sw { ms.sweep(t4, t8, &mut g); } let r2 = n * 64.0 * sw as f64 / t0.elapsed().as_secs_f64();
    let t0 = Instant::now(); for _ in 0..sw { ms.sweep(t4, t8, &mut ph); } let r2p = n * 64.0 * sw as f64 / t0.elapsed().as_secs_f64();
    let mut th4 = [0u32; 64]; let mut th8 = [0u32; 64];
    for k in 0..64 { let b = 0.2 * (3.0f64 / 0.2).powf(k as f64 / 63.0); let (a, c) = multispin::thresholds(b); th4[k] = a; th8[k] = c; }
    let t0 = Instant::now(); for _ in 0..sw { ms.sweep_prefix(&th4, &th8, &mut g); } let r3 = n * 64.0 * sw as f64 / t0.elapsed().as_secs_f64();
    println!("T1 SoA f32 heat-bath LUT, splitmix : {:.3e} updates/s/thread", r1);
    println!("T1 SoA f32 heat-bath LUT, Philox   : {:.3e} updates/s/thread", r1p);
    println!("T0 multispin 64-lane, splitmix     : {:.3e} updates/s/thread", r2);
    println!("T0 multispin 64-lane, Philox       : {:.3e} updates/s/thread", r2p);
    println!("T0 PT-in-a-word prefix-mask, splitm: {:.3e} updates/s/thread", r3);
    for th in [4usize, 10] {
        let t0 = Instant::now();
        std::thread::scope(|sc| { for k in 0..th { sc.spawn(move || { let mut g = SplitMix64(99 + k as u64); let mut m = multispin::Lattice::random(l, &mut g);
            for _ in 0..sw { m.sweep(t4, t8, &mut g); } std::hint::black_box(m.w[0]); }); } });
        println!("T0 multispin splitmix x{:>2} threads : {:.3e} updates/s total", th, n * 64.0 * sw as f64 * th as f64 / t0.elapsed().as_secs_f64());
    }
    // Tuned kernels (bit-identical output, see test fast_kernels_bit_identical_to_reference)
    let t0 = Instant::now(); for _ in 0..sw { hb.sweep_fast(&lut, &mut g); } let f1 = n * sw as f64 / t0.elapsed().as_secs_f64();
    let t0 = Instant::now(); for _ in 0..sw { hb.sweep_fast(&lut, &mut ph); } let f1p = n * sw as f64 / t0.elapsed().as_secs_f64();
    let t0 = Instant::now(); for _ in 0..sw { ms.sweep_fast(t4, t8, &mut g); } let f2 = n * 64.0 * sw as f64 / t0.elapsed().as_secs_f64();
    let t0 = Instant::now(); for _ in 0..sw { ms.sweep_fast(t4, t8, &mut ph); } let f2p = n * 64.0 * sw as f64 / t0.elapsed().as_secs_f64();
    for th in [4usize, 10] {
        let t0 = Instant::now();
        std::thread::scope(|sc| { for k in 0..th { sc.spawn(move || { let mut g = SplitMix64(99 + k as u64); let mut m = multispin::Lattice::random(l, &mut g);
            for _ in 0..sw { m.sweep_fast(t4, t8, &mut g); } std::hint::black_box(m.w[0]); }); } });
        println!("fast multispin splitmix x{:>2} thr    : {:.3e} updates/s total", th, n * 64.0 * sw as f64 * th as f64 / t0.elapsed().as_secs_f64());
    }
    println!("fast T1 f32 LUT, splitmix          : {:.3e} updates/s/thread", f1);
    println!("fast T1 f32 LUT, Philox            : {:.3e} updates/s/thread", f1p);
    println!("fast T0 multispin, splitmix        : {:.3e} updates/s/thread", f2);
    println!("fast T0 multispin, Philox          : {:.3e} updates/s/thread", f2p);
    println!("chk {}", ms.w.iter().map(|v| v.count_ones() as u64).sum::<u64>() + hb.s.iter().filter(|&&v| v > 0.0).count() as u64);
}
