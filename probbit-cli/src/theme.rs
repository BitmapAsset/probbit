//! The visual theme shared by the hero screen, `--top`, `demo --live` and the `--summary` box: a neon palette in hand-written
//! ANSI (no crates). Visuals are for a person at a terminal. They are off with `NO_COLOR` (any non-empty value), `PROBBIT_THEME`
//! set to anything but `neon` (`plain` is the documented value), `--plain`, `TERM=dumb`, and whenever the stream they would
//! go to is not a terminal; every command's stdout is then exactly what it is without them. 256 colours (the xterm cube) when
//! the terminal advertises them, else the 16-colour set. Colours are foreground (and half-block background) only: the
//! terminal keeps its own background.
use std::io::IsTerminal;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth { Ansi16, Ansi256 }
/// A colour as (xterm-256 index, 16-colour foreground SGR code).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Col(pub u8, pub u8);
/// #00F0FF -> xterm 51 (#00FFFF); #FF2A6D -> 197 (#FF005F); #F9F002 -> 226 (#FFFF00). Fallbacks: bright cyan / magenta / yellow.
pub const CYAN: Col = Col(51, 96);
pub const MAGENTA: Col = Col(197, 95);
pub const YELLOW: Col = Col(226, 93);
pub const GREEN: Col = Col(48, 92);
pub const GREY: Col = Col(245, 37);
pub const DIM: Col = Col(240, 90);
pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";

#[derive(Clone, Copy, Debug)]
pub struct Theme { pub depth: Depth }
impl Theme {
    pub fn fg(&self, c: Col) -> String { match self.depth { Depth::Ansi256 => format!("\x1b[38;5;{}m", c.0), Depth::Ansi16 => format!("\x1b[{}m", c.1) } }
    pub fn paint(&self, c: Col, s: &str) -> String { format!("{}{s}{RESET}", self.fg(c)) }
    pub fn bold(&self, c: Col, s: &str) -> String { format!("{BOLD}{}{s}{RESET}", self.fg(c)) }
}

/// Visuals may be drawn at all (before the stream check): no `--plain`, `NO_COLOR` unset or empty, `PROBBIT_THEME` unset, empty or
/// `neon`, `TERM` not `dumb`.
pub fn wanted(args: &[String]) -> bool {
    !args.iter().any(|a| a == "--plain") && std::env::var_os("NO_COLOR").map_or(true, |v| v.is_empty())
        && std::env::var("PROBBIT_THEME").map_or(true, |v| v.is_empty() || v == "neon") && std::env::var("TERM").map_or(true, |t| t != "dumb")
}
/// The theme for stderr (`--top`, `demo --live`, the summary box), or None: plain, or stderr is not a terminal.
pub fn stderr(args: &[String]) -> Option<Theme> { (wanted(args) && std::io::stderr().is_terminal() && imp::vt(2)).then(|| Theme { depth: depth() }) }
/// The theme for stdout (the hero screen), or None.
pub fn stdout(args: &[String]) -> Option<Theme> { (wanted(args) && std::io::stdout().is_terminal() && imp::vt(1)).then(|| Theme { depth: depth() }) }
pub fn stdout_is_terminal() -> bool { std::io::stdout().is_terminal() }

/// 256 colours when the terminal says so (`COLORTERM`, a `TERM` with 256, Windows Terminal, or a known emulator), else 16.
fn depth() -> Depth {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let (ct, term, prog) = (env("COLORTERM"), env("TERM"), env("TERM_PROGRAM"));
    if ct.contains("truecolor") || ct.contains("24bit") || term.contains("256") || std::env::var_os("WT_SESSION").is_some()
        || ["iTerm.app", "Apple_Terminal", "vscode", "WezTerm", "ghostty", "Hyper"].contains(&prog.as_str()) { Depth::Ansi256 } else { Depth::Ansi16 }
}

/// (columns, rows) of the terminal on fd 1 or 2; `COLUMNS` / `LINES`, then 80 x 24, when unknown.
pub fn size(fd: i32) -> (usize, usize) {
    imp::size(fd).unwrap_or_else(|| { let e = |k: &str, d: usize| std::env::var(k).ok().and_then(|v| v.parse().ok()).filter(|&v: &usize| v > 0).unwrap_or(d); (e("COLUMNS", 80), e("LINES", 24)) })
}

/// Ctrl-C while a live view hides the cursor: show it again and exit 130 (Unix; elsewhere the default handler runs).
pub fn restore_cursor_on_interrupt() { imp::on_interrupt(); }

#[cfg(unix)]
mod imp {
    use std::os::raw::{c_int, c_ulong};
    #[repr(C)] struct Winsize { row: u16, col: u16, xp: u16, yp: u16 }
    extern "C" { fn ioctl(fd: c_int, req: c_ulong, ...) -> c_int; fn signal(sig: c_int, h: usize) -> usize; fn write(fd: c_int, b: *const u8, n: usize) -> isize; fn _exit(code: c_int) -> !; }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
    const TIOCGWINSZ: Option<c_ulong> = Some(0x4008_7468);
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64", target_arch = "arm", target_arch = "riscv64", target_arch = "loongarch64")))]
    const TIOCGWINSZ: Option<c_ulong> = Some(0x5413);
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly",
        all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64", target_arch = "arm", target_arch = "riscv64", target_arch = "loongarch64")))))]
    const TIOCGWINSZ: Option<c_ulong> = None;
    pub fn size(fd: i32) -> Option<(usize, usize)> {
        let mut w = Winsize { row: 0, col: 0, xp: 0, yp: 0 };
        (unsafe { ioctl(fd, TIOCGWINSZ?, &mut w as *mut Winsize) } == 0 && w.col > 0 && w.row > 0).then_some((w.col as usize, w.row as usize))
    }
    pub fn vt(_fd: i32) -> bool { true }
    extern "C" fn on_int(_: c_int) { let s = b"\x1b[0m\x1b[?25h\n"; unsafe { write(2, s.as_ptr(), s.len()); _exit(130) } }
    pub fn on_interrupt() { unsafe { signal(2, on_int as extern "C" fn(c_int) as usize); } }
}
#[cfg(windows)]
mod imp {
    type H = *mut core::ffi::c_void;
    #[repr(C)] struct Coord { x: i16, y: i16 }
    #[repr(C)] struct Rect { l: i16, t: i16, r: i16, b: i16 }
    #[repr(C)] struct Info { size: Coord, cursor: Coord, attr: u16, win: Rect, max: Coord }
    #[link(name = "kernel32")]
    extern "system" { fn GetStdHandle(n: u32) -> H; fn GetConsoleMode(h: H, m: *mut u32) -> i32; fn SetConsoleMode(h: H, m: u32) -> i32; fn GetConsoleScreenBufferInfo(h: H, i: *mut Info) -> i32; }
    fn handle(fd: i32) -> H { unsafe { GetStdHandle(if fd == 1 { -11i32 as u32 } else { -12i32 as u32 }) } }
    /// ANSI sequences need ENABLE_VIRTUAL_TERMINAL_PROCESSING (0x4); false (= plain) if the console refuses it.
    pub fn vt(fd: i32) -> bool { let h = handle(fd); let mut m = 0u32; unsafe { GetConsoleMode(h, &mut m) != 0 && (m & 4 != 0 || SetConsoleMode(h, m | 4) != 0) } }
    pub fn size(fd: i32) -> Option<(usize, usize)> {
        let mut i = Info { size: Coord { x: 0, y: 0 }, cursor: Coord { x: 0, y: 0 }, attr: 0, win: Rect { l: 0, t: 0, r: 0, b: 0 }, max: Coord { x: 0, y: 0 } };
        if unsafe { GetConsoleScreenBufferInfo(handle(fd), &mut i) } == 0 { return None; }
        let (c, r) = (i.win.r as i32 - i.win.l as i32 + 1, i.win.b as i32 - i.win.t as i32 + 1); (c > 0 && r > 0).then_some((c as usize, r as usize))
    }
    pub fn on_interrupt() {}
}
#[cfg(not(any(unix, windows)))]
mod imp { pub fn size(_: i32) -> Option<(usize, usize)> { None } pub fn vt(_: i32) -> bool { false } pub fn on_interrupt() {} }
