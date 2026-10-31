//! probbit-nostd: the subset of `std` that probbit-core, probbit-ir, probbit-decide and probbit-persona use, built from `core`,
//! `alloc`, `hashbrown`, `libm` and `portable-atomic` for a target without an operating system (xtensa-esp32s3-none-elf).
//! A crate that is `no_std` imports it as `std` (`#[macro_use] extern crate probbit_nostd as std;`), so its `std::` paths compile
//! unchanged. Threads run in order on the calling thread (the engine's `rt::sequential()` mode); `thread_local!` is a static
//! (one core runs the engine); files and sleeping are absent (`fs::read` fails, `sleep` returns at once).
#![no_std]
extern crate alloc;

#[allow(deprecated)] // the integer modules (`std::u64::MAX` style paths) are deprecated in std too, and still exist there
pub use core::{any, array, ascii, cell, char, clone, cmp, convert, default, f32, f64, hash, hint, i16, i32, i64, i8,
    isize, iter, marker, mem, num, ops, option, ptr, result, slice, str, u16, u32, u64, u8, usize};
pub use alloc::{borrow, boxed, fmt, rc, string, vec};

pub mod time { pub use core::time::Duration; }

pub mod collections {
    pub use alloc::collections::*;
    pub use hashbrown::{HashMap, HashSet};
    pub mod hash_map { pub use hashbrown::hash_map::*; }
    pub mod hash_set { pub use hashbrown::hash_set::*; }
}

pub mod sync {
    pub use alloc::sync::*;
    pub mod atomic {
        pub use core::sync::atomic::{compiler_fence, fence, AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicUsize, Ordering};
        pub use portable_atomic::{AtomicI64, AtomicU64};
    }
}

pub mod io {
    /// The only I/O error here: there is no operating system.
    #[derive(Debug, Clone, Copy)]
    pub struct Error;
    impl core::fmt::Display for Error { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("no operating system on this target") } }
    pub type Result<T> = core::result::Result<T, Error>;
    static PRINT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    /// Install the console writer used by `print!`/`println!`/`eprintln!` (the firmware passes esp-println's).
    pub fn set_print(f: fn(&str)) { PRINT.store(f as usize, core::sync::atomic::Ordering::Relaxed) }
    #[doc(hidden)]
    pub fn _print(args: core::fmt::Arguments<'_>) {
        let p = PRINT.load(core::sync::atomic::Ordering::Relaxed);
        if p != 0 { let f: fn(&str) = unsafe { core::mem::transmute(p) }; f(&alloc::fmt::format(args)); }
    }
}

pub mod fs {
    /// No filesystem on bare metal: the firmware passes documents in as strings.
    pub fn read<P>(_path: P) -> crate::io::Result<alloc::vec::Vec<u8>> { Err(crate::io::Error) }
}

pub mod panic {
    pub fn resume_unwind(_payload: alloc::boxed::Box<dyn core::any::Any + Send>) -> ! { panic!("a sequential section panicked") }
}

pub mod thread {
    use core::cell::UnsafeCell;
    use core::marker::PhantomData;
    use core::time::Duration;
    /// `std::thread::scope` without threads: every spawn runs at once on the calling thread.
    pub struct Scope<'scope, 'env: 'scope> { _p: PhantomData<(&'scope mut &'scope (), &'env mut &'env ())> }
    pub fn scope<'env, F, T>(f: F) -> T where F: for<'scope> FnOnce(&'scope Scope<'scope, 'env>) -> T {
        let s = Scope { _p: PhantomData }; f(&s)
    }
    impl<'scope, 'env> Scope<'scope, 'env> {
        pub fn spawn<F, T>(&'scope self, f: F) -> ScopedJoinHandle<'scope, T> where F: FnOnce() -> T + Send + 'scope, T: Send + 'scope {
            ScopedJoinHandle { v: f(), _p: PhantomData }
        }
    }
    pub struct ScopedJoinHandle<'scope, T> { v: T, _p: PhantomData<&'scope ()> }
    impl<'scope, T> ScopedJoinHandle<'scope, T> {
        pub fn join(self) -> core::result::Result<T, alloc::boxed::Box<dyn core::any::Any + Send + 'static>> { Ok(self.v) }
    }
    pub struct Builder;
    impl Builder {
        pub fn new() -> Builder { Builder }
        pub fn stack_size(self, _size: usize) -> Builder { self }
        pub fn name(self, _name: alloc::string::String) -> Builder { self }
        pub fn spawn_scoped<'scope, 'env, F, T>(self, s: &'scope Scope<'scope, 'env>, f: F) -> crate::io::Result<ScopedJoinHandle<'scope, T>>
        where F: FnOnce() -> T + Send + 'scope, T: Send + 'scope { Ok(s.spawn(f)) }
    }
    pub fn available_parallelism() -> crate::io::Result<core::num::NonZeroUsize> { Ok(core::num::NonZeroUsize::MIN) }
    pub fn sleep(_d: Duration) {}
    /// `thread_local!` storage: one static, initialised on first use (the engine runs on one core).
    pub struct LocalKey<T: 'static> { init: fn() -> T, slot: UnsafeCell<Option<T>> }
    unsafe impl<T> Sync for LocalKey<T> {}
    impl<T: 'static> LocalKey<T> {
        #[doc(hidden)]
        pub const fn new(init: fn() -> T) -> Self { LocalKey { init, slot: UnsafeCell::new(None) } }
        pub fn with<F: FnOnce(&T) -> R, R>(&'static self, f: F) -> R {
            let slot = unsafe { &mut *self.slot.get() };
            if slot.is_none() { *slot = Some((self.init)()); }
            f(slot.as_ref().unwrap())
        }
    }
    impl<T: Copy + 'static> LocalKey<core::cell::Cell<T>> {
        pub fn get(&'static self) -> T { self.with(|c| c.get()) }
        pub fn set(&'static self, v: T) { self.with(|c| c.set(v)) }
        pub fn replace(&'static self, v: T) -> T { self.with(|c| c.replace(v)) }
    }
}

#[macro_export]
macro_rules! thread_local {
    () => {};
    ($(#[$a:meta])* $vis:vis static $name:ident : $t:ty = const { $init:expr } $(; $($rest:tt)*)?) => {
        $(#[$a])* $vis static $name: $crate::thread::LocalKey<$t> = $crate::thread::LocalKey::new({ fn __init() -> $t { $init } __init });
        $($crate::thread_local!($($rest)*);)?
    };
    ($(#[$a:meta])* $vis:vis static $name:ident : $t:ty = $init:expr $(; $($rest:tt)*)?) => {
        $(#[$a])* $vis static $name: $crate::thread::LocalKey<$t> = $crate::thread::LocalKey::new({ fn __init() -> $t { $init } __init });
        $($crate::thread_local!($($rest)*);)?
    };
}
#[macro_export]
macro_rules! print { ($($t:tt)*) => { $crate::io::_print(format_args!($($t)*)) }; }
#[macro_export]
macro_rules! println { () => { $crate::io::_print(format_args!("\n")) }; ($($t:tt)*) => { $crate::io::_print(format_args!("{}\n", format_args!($($t)*))) }; }
#[macro_export]
macro_rules! eprint { ($($t:tt)*) => { $crate::io::_print(format_args!($($t)*)) }; }
#[macro_export]
macro_rules! eprintln { () => { $crate::io::_print(format_args!("\n")) }; ($($t:tt)*) => { $crate::io::_print(format_args!("{}\n", format_args!($($t)*))) }; }

/// `f64`/`f32` methods that live in `std` (they call the platform libm) and not in `core`: here they call the `libm` crate.
/// Inherent methods win where `core` has them, so this trait only fills the gaps.
pub trait FloatExt: Sized {
    fn exp(self) -> Self; fn exp_m1(self) -> Self; fn ln(self) -> Self; fn ln_1p(self) -> Self; fn log2(self) -> Self; fn log10(self) -> Self;
    fn sqrt(self) -> Self; fn powf(self, e: Self) -> Self; fn powi(self, n: i32) -> Self; fn tanh(self) -> Self; fn cos(self) -> Self; fn sin(self) -> Self;
    fn floor(self) -> Self; fn ceil(self) -> Self; fn round(self) -> Self; fn trunc(self) -> Self; fn fract(self) -> Self;
    fn asin(self) -> Self; fn acos(self) -> Self; fn atan2(self, x: Self) -> Self;
}
/// `powi` as compiler-rt's `__powidf2` computes it (what `std`'s `f64::powi` lowers to), not `pow`: same bits as the host.
fn powi64(mut a: f64, mut b: i32) -> f64 {
    let recip = b < 0; let mut r = 1.0;
    loop { if b & 1 != 0 { r *= a; } b /= 2; if b == 0 { break; } a *= a; }
    if recip { 1.0 / r } else { r }
}
impl FloatExt for f64 {
    fn exp(self) -> f64 { libm::exp(self) } fn exp_m1(self) -> f64 { libm::expm1(self) } fn ln(self) -> f64 { libm::log(self) }
    fn ln_1p(self) -> f64 { libm::log1p(self) } fn log2(self) -> f64 { libm::log2(self) } fn log10(self) -> f64 { libm::log10(self) }
    fn sqrt(self) -> f64 { libm::sqrt(self) } fn powf(self, e: f64) -> f64 { libm::pow(self, e) } fn powi(self, n: i32) -> f64 { powi64(self, n) }
    fn tanh(self) -> f64 { libm::tanh(self) } fn cos(self) -> f64 { libm::cos(self) } fn sin(self) -> f64 { libm::sin(self) }
    fn floor(self) -> f64 { libm::floor(self) } fn ceil(self) -> f64 { libm::ceil(self) } fn round(self) -> f64 { libm::round(self) }
    fn trunc(self) -> f64 { libm::trunc(self) } fn fract(self) -> f64 { self - libm::trunc(self) }
    fn asin(self) -> f64 { libm::asin(self) } fn acos(self) -> f64 { libm::acos(self) } fn atan2(self, x: f64) -> f64 { libm::atan2(self, x) }
}
impl FloatExt for f32 {
    fn exp(self) -> f32 { libm::expf(self) } fn exp_m1(self) -> f32 { libm::expm1f(self) } fn ln(self) -> f32 { libm::logf(self) }
    fn ln_1p(self) -> f32 { libm::log1pf(self) } fn log2(self) -> f32 { libm::log2f(self) } fn log10(self) -> f32 { libm::log10f(self) }
    fn sqrt(self) -> f32 { libm::sqrtf(self) } fn powf(self, e: f32) -> f32 { libm::powf(self, e) } fn powi(self, n: i32) -> f32 { powi64(self as f64, n) as f32 }
    fn tanh(self) -> f32 { libm::tanhf(self) } fn cos(self) -> f32 { libm::cosf(self) } fn sin(self) -> f32 { libm::sinf(self) }
    fn floor(self) -> f32 { libm::floorf(self) } fn ceil(self) -> f32 { libm::ceilf(self) } fn round(self) -> f32 { libm::roundf(self) }
    fn trunc(self) -> f32 { libm::truncf(self) } fn fract(self) -> f32 { self - libm::truncf(self) }
    fn asin(self) -> f32 { libm::asinf(self) } fn acos(self) -> f32 { libm::acosf(self) } fn atan2(self, x: f32) -> f32 { libm::atan2f(self, x) }
}

/// What a `no_std` module adds with one line (`use std::prelude::rust_2021::*;`) to see the names `std`'s prelude gives it.
pub mod prelude {
    pub mod rust_2021 {
        pub use core::prelude::rust_2021::*;
        pub use alloc::borrow::ToOwned;
        pub use alloc::boxed::Box;
        pub use alloc::string::{String, ToString};
        pub use alloc::vec::Vec;
        pub use crate::FloatExt;
    }
}
