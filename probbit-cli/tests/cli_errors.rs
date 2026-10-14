//! R94 item 6: one error channel. A human line on stderr, always. A bad input document's `{"error"}` object goes to stdout when
//! stdout is not a terminal (every wrapper reads it there) or with --json; at a terminal without --json only the line: never
//! both. A bad command or flag stays off stdout unless --json is given (then code "command" + "suggestion", or "flag").
use std::process::{Command, Stdio};
use std::io::Write;

fn probbit(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let _ = c.stdin.take().unwrap().write_all(stdin.as_bytes());
    let o = c.wait_with_output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
/// The command under a pseudo-terminal (`script`): stdout on the terminal, stderr to a file. -> (exit, the terminal, stderr);
/// None where there is no `script` (Windows)
fn at_terminal(args: &[&str], input: &str) -> Option<(i32, String, String)> {
    if !cfg!(unix) || Command::new("sh").args(["-c", "command -v script"]).output().map_or(true, |o| !o.status.success()) { return None; }
    let tag = format!("{}-{}", std::process::id(), args.join("_").replace(['/', ' ', '"', '\''], ""));
    let (inp, errp) = (std::env::temp_dir().join(format!("probbit-err-in-{tag}")), std::env::temp_dir().join(format!("probbit-err-2-{tag}")));
    std::fs::write(&inp, input).unwrap();
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let cmd = format!("{} {} < {} 2> {}", q(env!("CARGO_BIN_EXE_probbit")), args.iter().map(|a| q(a)).collect::<Vec<_>>().join(" "), q(&inp.to_string_lossy()), q(&errp.to_string_lossy()));
    let mut c = Command::new("script");
    if cfg!(target_os = "macos") { c.args(["-q", "/dev/null", "sh", "-c", &cmd]); } else { c.args(["-q", "-e", "-c", &cmd, "/dev/null"]); }
    let o = c.stdin(Stdio::null()).env("TERM", "xterm-256color").output().unwrap();
    let err = std::fs::read_to_string(&errp).unwrap_or_default(); let _ = std::fs::remove_file(&inp); let _ = std::fs::remove_file(&errp);
    // macOS script(1) echoes a ^D for the closed stdin
    let screen = String::from_utf8_lossy(&o.stdout).replace("^D", "").replace(['\u{8}', '\r'], "");
    Some((o.status.code().unwrap_or(-1), screen, err))
}
const BAD: &str = "{";
const WANT: &str = "{\"error\":{\"code\":\"schema\",\"path\":\"\",\"message\":\"bad JSON: expected string at byte 1\"}}\n";

#[test]
fn piped_input_error_is_one_object_on_stdout_and_a_line_on_stderr() {
    for cmd in ["decide", "run", "evaluate", "ir"] {
        let (c, out, err) = probbit(&[cmd], BAD); assert_eq!(c, 2, "{cmd}");
        assert_eq!(out, WANT, "{cmd}"); assert!(err.starts_with("probbit: ") && err.lines().count() == 1 && !err.contains('{'), "{cmd}: {err}");
    }
    let (c, out, _) = probbit(&["decide", "--json"], BAD); assert_eq!((c, out.as_str()), (2, WANT), "--json changes nothing piped");
    let (c, out, err) = probbit(&["persona", "check", "no-such-persona.yaml"], ""); assert_eq!(c, 2);
    assert!(out.starts_with("{\"error\":{\"code\":\"persona\"") && out.lines().count() == 1 && err.starts_with("probbit: "), "{out} {err}");
}

#[test]
fn at_a_terminal_the_line_only_and_with_json_the_object_too() {
    let Some((c, screen, err)) = at_terminal(&["decide"], BAD) else { eprintln!("no script(1): terminal test skipped"); return };
    assert_eq!(c, 2); assert!(!screen.contains("{\"error\""), "the object on a terminal: {screen:?}"); assert!(screen.trim().is_empty(), "{screen:?}");
    assert_eq!(err, "probbit: bad JSON: expected string at byte 1\n");
    let (c, screen, err) = at_terminal(&["decide", "--json"], BAD).unwrap();
    assert_eq!(c, 2); assert_eq!(screen.trim(), WANT.trim(), "--json: the object at a terminal"); assert_eq!(err, "probbit: bad JSON: expected string at byte 1\n");
    let (c, screen, err) = at_terminal(&["persona", "check", "no-such-persona.yaml"], "").unwrap();
    assert_eq!(c, 2); assert!(screen.trim().is_empty() && err.starts_with("probbit: persona: "), "{screen:?} {err}");
    // a good answer at a terminal is unchanged
    let (c, screen, _) = at_terminal(&["version"], "").unwrap(); assert_eq!((c, screen.trim()), (0, concat!("probbit ", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn usage_errors_stay_off_stdout_unless_json() {
    for args in [vec!["persna"], vec!["decide", "--nope"], vec!["persona", "lnt", "x.yaml"], vec!["completions", "tcsh"]] {
        let (c, out, err) = probbit(&args, ""); assert_eq!(c, 2, "{args:?}"); assert!(out.is_empty(), "{args:?}: {out}"); assert!(err.starts_with("probbit: "), "{args:?}");
    }
    let (c, out, err) = probbit(&["persna", "init", "x.yaml", "--json"], ""); assert_eq!(c, 2);
    assert_eq!(out, "{\"error\":{\"code\":\"command\",\"path\":\"\",\"message\":\"unknown command \\\"persna\\\"; did you mean \\\"persona\\\"? see `probbit --help`\",\"suggestion\":\"persona\"}}\n");
    assert_eq!(err, "probbit: unknown command \"persna\"; did you mean \"persona\"? see `probbit --help`\n");
    let (_, out, _) = probbit(&["frobnicate", "--json"], ""); assert!(out.ends_with(",\"suggestion\":null}}\n"), "{out}");
    let (_, out, _) = probbit(&["persona", "lnt", "x.yaml", "--json"], ""); assert!(out.contains("\"code\":\"command\"") && out.contains("\"suggestion\":\"lint\""), "{out}");
    let (c, out, _) = probbit(&["decide", "--budget-ms", "nan", "--json"], ""); assert_eq!(c, 2);
    assert!(out.starts_with("{\"error\":{\"code\":\"flag\",\"path\":\"\",\"message\":\"--budget-ms must be") && !out.contains("suggestion"), "{out}");
}

#[test]
fn every_command_accepts_json() {
    for args in [vec!["version", "--json"], vec!["stats", "--sweeps", "10", "--json"], vec!["help", "persona"], vec!["completions", "bash", "--json"]] {
        let (c, _, err) = probbit(&args, ""); assert_eq!(c, 0, "{args:?}: {err}");
    }
    let (c, out, _) = probbit(&["demo", "--json"], ""); assert_eq!(c, 0); assert!(out.starts_with('{'));
}
