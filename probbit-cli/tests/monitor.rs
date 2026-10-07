//! `probbit monitor` (docs/persona.md §5.8) end to end: help, one frame pinned byte for byte (--once --plain), the exit codes,
//! --follow picking up appended lines within a second and replaying a truncated strand from the start.
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The tutor's demo week (seed 2), as `probbit live examples/persona/tutor.yaml --seed 2 --demo week --strand` 0.7.0 wrote it
const WEEK: &str = "tests/fixtures/monitor/tutor-week.strand";

fn probbit(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).env_remove("NO_COLOR").env_remove("PROBBIT_THEME").stdin(Stdio::null()).output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
fn tmp(tag: &str) -> String { std::env::temp_dir().join(format!("probbit-monitor-{tag}-{}.strand", std::process::id())).to_str().unwrap().to_string() }
/// A pinned frame: the file's text with `STRAND` standing for the strand's path (line endings as \n, whatever the checkout did)
fn pinned(name: &str, path: &str) -> String { std::fs::read_to_string(format!("tests/fixtures/monitor/{name}")).unwrap().replace("\r\n", "\n").replace("STRAND", path) }
/// The fixture's leading `n` lines (the header and n - 1 events)
fn lead(n: usize) -> String { std::fs::read_to_string(WEEK).unwrap().split_inclusive('\n').take(n).collect() }

#[test]
fn monitor_has_help() {
    for h in ["--help", "-h"] {
        let (c, out, err) = probbit(&["monitor", h]); assert_eq!(c, 0, "{err}"); assert!(err.is_empty(), "{err}");
        assert!(out.starts_with("usage: probbit monitor STRAND"), "{out}");
        for f in ["--follow", "--once", "--plain", "--fps N", "--demo", "exit: 0", "live verify"] { assert!(out.contains(f), "monitor help lacks {f}: {out}"); }
    }
    let (c, out, _) = probbit(&["--help"]); assert_eq!(c, 0); assert!(out.contains("probbit monitor STRAND"), "the usage lists monitor");
}

/// --once --plain: one frame in plain ASCII, byte for byte; the last event of the week and a failure turn (event 18: loss and
/// praise lit, the bound habit in brackets, humour held at none)
#[test]
fn one_plain_frame_is_pinned() {
    let (c, out, err) = probbit(&["monitor", WEEK, "--once", "--plain"]); assert_eq!(c, 0, "{err}");
    assert!(out.is_ascii(), "--plain draws in plain ASCII"); assert_eq!(out, pinned("tutor-week.once.txt", WEEK));
    let f = tmp("e18"); std::fs::write(&f, lead(19)).unwrap();
    let (c, out, err) = probbit(&["monitor", &f, "--once", "--plain"]); assert_eq!(c, 0, "{err}");
    assert_eq!(out, pinned("tutor-week-18.once.txt", &f));
    // without --plain and without a terminal: no colour, the same frame drawn with box characters
    let (c, uni, _) = probbit(&["monitor", &f]); assert_eq!(c, 0);
    assert!(!uni.contains('\x1b') && uni.contains("replay verified ✓") && uni.contains('█'), "{uni}");
    assert_eq!(uni.lines().count(), out.lines().count());
    let _ = std::fs::remove_file(&f);
}

/// --demo --plain --once: the last frame of the tutor's week, the same as `live --demo week` writes it (the pinned frame of the
/// fixture, all but the footer, which says the week is in memory), the same on every run
#[test]
fn the_demo_is_the_week_live_writes() {
    let (c, a, err) = probbit(&["monitor", "--demo", "--plain", "--once"]); assert_eq!(c, 0, "{err}");
    let (_, b, _) = probbit(&["monitor", "--demo", "--plain"]); assert_eq!(a, b, "deterministic; piped, the demo prints its last frame");
    let week = pinned("tutor-week.once.txt", WEEK); let (wl, al): (Vec<&str>, Vec<&str>) = (week.lines().collect(), a.lines().collect());
    assert_eq!(wl.len(), al.len()); assert_eq!(wl[..wl.len() - 1], al[..al.len() - 1]);
    assert!(al[al.len() - 1].starts_with("strand: the demo week, in memory | written by probbit ") && al[al.len() - 1].ends_with("--seed 2 --demo week"), "{a}");
    for (args, msg) in [(vec!["monitor", WEEK, "--demo"], "give no STRAND"), (vec!["monitor", "--demo", "--follow"], "--follow does not apply")] {
        let (c, _, err) = probbit(&args); assert_eq!(c, 2, "{args:?}"); assert!(err.contains(msg), "{args:?}: {err}");
    }
}

/// 0 every line replays (an incomplete last line is said, not replayed); 1 a line differs: the header names it and the frame shows
/// the event before it; 2 a bad flag, an unreadable file, a file that is not a strand
#[test]
fn exit_codes() {
    let week = std::fs::read_to_string(WEEK).unwrap().replace("\r\n", "\n");
    let mut lines: Vec<String> = week.lines().map(String::from).collect();
    lines[12] = lines[12].replacen(r#""elapsed_hours":1"#, r#""elapsed_hours":2"#, 1);
    let bent = tmp("bent"); std::fs::write(&bent, lines.join("\n") + "\n").unwrap();
    let (c, out, _) = probbit(&["monitor", &bent, "--once", "--plain"]); assert_eq!(c, 1, "{out}");
    let head = out.lines().next().unwrap(); assert!(head.contains("event 11 ") && head.contains("line 13 diverges: the stance differs"), "{head}");
    let (c, v, _) = probbit(&["live", "verify", &bent]); assert_eq!(c, 1); assert!(v.contains(r#""line":13"#), "the line verify names: {v}");
    let part = tmp("part"); std::fs::write(&part, lead(4) + &week.lines().nth(4).unwrap()[..40]).unwrap();
    let (c, out, err) = probbit(&["monitor", &part, "--once", "--plain"]); assert_eq!(c, 0, "{err}");
    assert!(out.contains("event 3 ") && out.contains("line 5 is incomplete (no newline at its end): not replayed"), "{out}");
    let bare = tmp("head"); std::fs::write(&bare, lead(1)).unwrap();
    let (c, out, _) = probbit(&["monitor", &bare, "--once", "--plain"]); assert_eq!(c, 0); assert!(out.contains("event 0 ") && out.contains("no events yet"), "{out}");
    let not = tmp("not"); std::fs::write(&not, "{\"hello\":1}\n").unwrap();
    for (args, msg) in [(vec!["monitor", &not], "not a probbit strand"), (vec!["monitor", "no/such.strand"], "cannot read"), (vec!["monitor"], "the strand file comes before the flags"),
        (vec!["monitor", WEEK, "--once", "--follow"], "give one of them"), (vec!["monitor", WEEK, "--fps", "0"], "--fps N, from 1 to 60"), (vec!["monitor", WEEK, "--nope"], "unknown argument")] {
        let (c, out, err) = probbit(&args); assert_eq!((c, out.as_str()), (2, ""), "{args:?}: {err}"); assert!(err.contains(msg), "{args:?}: {err}");
    }
    for f in [bent, part, bare, not] { let _ = std::fs::remove_file(f); }
}

/// stdout of a running child, collected as it comes
fn collect(child: &mut std::process::Child) -> Arc<Mutex<String>> {
    let buf = Arc::new(Mutex::new(String::new())); let b = buf.clone(); let mut out = child.stdout.take().unwrap();
    std::thread::spawn(move || { let mut chunk = [0u8; 4096]; while let Ok(n) = out.read(&mut chunk) { if n == 0 { break; } b.lock().unwrap().push_str(&String::from_utf8_lossy(&chunk[..n])); } });
    buf
}
/// Wait for `what` to appear in the output after byte `from` -> the time it took
fn wait(buf: &Arc<Mutex<String>>, from: usize, what: &str, limit: Duration) -> Duration {
    let t = Instant::now();
    loop { if buf.lock().unwrap()[from..].contains(what) { return t.elapsed(); }
        assert!(t.elapsed() < limit, "no {what:?} within {limit:?}: {}", &buf.lock().unwrap()[from..]); std::thread::sleep(Duration::from_millis(10)); }
}

/// --follow: a line appended to the strand is replayed and drawn within a second; a truncated strand is replayed from the start,
/// with a warning
#[test]
fn follow_picks_up_appended_lines() {
    let f = tmp("follow"); std::fs::write(&f, lead(4)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_probbit")).args(["monitor", &f, "--follow", "--plain"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let buf = collect(&mut child);
    wait(&buf, 0, "| event 3 |", Duration::from_secs(20));
    for k in 5..=7 {
        let at = buf.lock().unwrap().len();
        let line = lead(k)[lead(k - 1).len()..].to_string();
        std::io::Write::write_all(&mut std::fs::OpenOptions::new().append(true).open(&f).unwrap(), line.as_bytes()).unwrap();
        let took = wait(&buf, at, &format!("| event {} |", k - 1), Duration::from_secs(5));
        assert!(took < Duration::from_secs(1), "event {} drawn after {took:?}", k - 1);
    }
    let at = buf.lock().unwrap().len();
    std::fs::write(&f, lead(3)).unwrap();
    wait(&buf, at, "the strand shrank (truncated or rotated): replayed from the start", Duration::from_secs(5));
    wait(&buf, at, "| event 2 |", Duration::from_secs(5));
    // removed, then another individual's strand in its place (a rotation): said, and replayed from its header
    let other = tmp("other"); let _ = std::fs::remove_file(&other);
    let (c, _, e) = probbit(&["live", "../examples/persona/tutor.yaml", "--seed", "3", "--demo", "week", "--plain", "--strand", &other]); assert_eq!(c, 0, "{e}");
    let at = buf.lock().unwrap().len();
    std::fs::remove_file(&f).unwrap();
    wait(&buf, at, "the strand is gone (removed or rotated): waiting for it", Duration::from_secs(5));
    std::fs::rename(&other, &f).unwrap();
    wait(&buf, at, "| seed 3 |", Duration::from_secs(5));
    wait(&buf, at, "| event 50 |", Duration::from_secs(5));
    // renamed over it without a gap: a strand of another length, or another header at the top, starts the replay over
    let (c, _, e) = probbit(&["live", "../examples/persona/tutor.yaml", "--seed", "4", "--demo", "week", "--plain", "--strand", &other]); assert_eq!(c, 0, "{e}");
    let at = buf.lock().unwrap().len();
    std::fs::rename(&other, &f).unwrap();
    wait(&buf, at, "| seed 4 |", Duration::from_secs(5));
    let _ = child.kill(); let _ = child.wait(); let _ = std::fs::remove_file(&f);
}
