//! Process resource usage without crates: getrusage(2) via FFI on 64-bit Unix (macOS and Linux struct layouts); None elsewhere.
// 64-bit only: the layouts below assume 64-bit time_t / long (32-bit Linux or BSD would read garbage telemetry)
#[cfg(all(unix, target_pointer_width = "64"))]
mod imp {
    #[cfg(target_os = "macos")] #[repr(C)] struct Timeval { sec: i64, usec: i32, _pad: i32 }
    #[cfg(not(target_os = "macos"))] #[repr(C)] struct Timeval { sec: i64, usec: i64 }
    #[repr(C)] struct RUsage { utime: Timeval, stime: Timeval, maxrss: i64, _rest: [i64; 13] }
    extern "C" { fn getrusage(who: i32, usage: *mut RUsage) -> i32; }
    /// (user + system CPU ms, peak resident set in MB) of this process so far.
    pub fn usage() -> Option<(f64, f64)> {
        let mut r = std::mem::MaybeUninit::<RUsage>::zeroed();
        if unsafe { getrusage(0, r.as_mut_ptr()) } != 0 { return None; }
        let r = unsafe { r.assume_init() };
        let ms = |t: &Timeval| t.sec as f64 * 1e3 + t.usec as f64 / 1e3;
        // ru_maxrss: bytes on macOS, kilobytes on Linux
        let rss_mb = if cfg!(target_os = "macos") { r.maxrss as f64 / 1048576.0 } else { r.maxrss as f64 / 1024.0 };
        Some((ms(&r.utime) + ms(&r.stime), rss_mb))
    }
}
#[cfg(all(unix, target_pointer_width = "64"))] pub use imp::usage;
#[cfg(not(all(unix, target_pointer_width = "64")))] pub fn usage() -> Option<(f64, f64)> { None }

/// CPU time of the calling thread in ms (clock_gettime, CLOCK_THREAD_CPUTIME_ID: 16 on macOS, 3 on Linux; 64-bit timespec);
/// None elsewhere. `demo --live` charges its renderer with it.
#[cfg(all(any(target_os = "macos", target_os = "linux"), target_pointer_width = "64"))]
pub fn thread_cpu_ms() -> Option<f64> {
    #[repr(C)] struct Timespec { sec: i64, nsec: i64 }
    extern "C" { fn clock_gettime(clk: i32, tp: *mut Timespec) -> i32; }
    let clk = if cfg!(target_os = "macos") { 16 } else { 3 };
    let mut t = Timespec { sec: 0, nsec: 0 };
    (unsafe { clock_gettime(clk, &mut t) } == 0).then(|| t.sec as f64 * 1e3 + t.nsec as f64 / 1e6)
}
#[cfg(not(all(any(target_os = "macos", target_os = "linux"), target_pointer_width = "64")))]
pub fn thread_cpu_ms() -> Option<f64> { None }

/// `--priority low` = nice 10 for this process (plus the macOS background band, see `set_low`) (setpriority(2) via FFI, PRIO_PROCESS, who = 0 = self), called before any
/// worker thread starts so the chains inherit it (Linux nice is per-thread; inheritance covers it — Linux unmeasured). Lowering
/// needs no privilege; raising would, so there is no "high". Returns the nice value read back; a process already at >= 10 is
/// left alone. Off Unix both functions return None, so `--priority low` exits 2 there (not implemented; untested).
#[cfg(unix)]
mod prio {
    extern "C" { fn setpriority(which: i32, who: u32, prio: i32) -> i32; fn getpriority(which: i32, who: u32) -> i32; }
    pub fn nice() -> Option<i32> { Some(unsafe { getpriority(0, 0) }) }
    pub fn set_low() -> Option<i32> { let cur = unsafe { getpriority(0, 0) };
        if cur < 10 && unsafe { setpriority(0, 0, 10) } != 0 { return None; }
        // macOS: nice 10 alone did not yield under contention (a 10-thread foreground was slowed x1.78 by a normal and
        // x1.73 by a nice-10 hog); PRIO_DARWIN_PROCESS (4) + PRIO_DARWIN_BG (0x1000) puts this process in the background band
        // (throttled CPU and I/O; the scheduler may keep it on efficiency cores). Best effort: nice 10 stays if it fails.
        #[cfg(target_os = "macos")] unsafe { setpriority(4, 0, 0x1000); }
        nice() }
}
#[cfg(unix)] pub use prio::{nice, set_low};
#[cfg(not(unix))] pub fn nice() -> Option<i32> { None }
#[cfg(not(unix))] pub fn set_low() -> Option<i32> { None }
