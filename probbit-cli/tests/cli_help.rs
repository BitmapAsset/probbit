//! R94 item 2: `probbit help [COMMAND [SUBCOMMAND]]` prints exactly what `COMMAND [SUBCOMMAND] --help` prints (exit 0), and
//! `probbit help` alone exactly what `probbit --help` prints.
use std::process::Command;

fn probbit(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
const COMMANDS: [&str; 13] = ["persona", "live", "monitor", "decide", "run", "evaluate", "demo", "ir", "mcp", "stats", "help", "version", "--version"];

#[test]
fn help_command_is_dash_dash_help() {
    let (c, top, err) = probbit(&["help"]); assert_eq!(c, 0, "{err}"); assert!(err.is_empty());
    for h in [vec!["--help"], vec!["-h"], vec!["help", "--plain"]] { assert_eq!(probbit(&h), (0, top.clone(), String::new()), "{h:?}"); }
    for cmd in COMMANDS {
        let want = probbit(&[cmd, "--help"]); assert_eq!(want.0, 0, "{cmd}");
        assert_eq!(probbit(&["help", cmd]), want, "help {cmd}"); assert_eq!(probbit(&[cmd, "-h"]), want, "{cmd} -h");
    }
    for (cmd, subs) in [("persona", &["init", "turn", "replay", "explain", "diff", "lint", "fuzz", "prove", "check", "compile", "describe"][..]), ("live", &["verify", "control"][..])] {
        for s in subs {
            let want = probbit(&[cmd, s, "--help"]); assert_eq!(want.0, 0, "{cmd} {s}"); assert!(want.1.starts_with(&format!("usage: probbit {cmd} {s} ")), "{}", want.1);
            assert_eq!(probbit(&["help", cmd, s]), want, "help {cmd} {s}");
            assert_ne!(want.1, probbit(&[cmd, "--help"]).1, "{cmd} {s}: the subcommand has its own help");
        }
    }
    // flags after the subcommand do not change it: `persona turn x.yaml --state s --help` is turn's help
    assert_eq!(probbit(&["persona", "turn", "x.yaml", "--state", "s", "--help"]).1, probbit(&["help", "persona", "turn"]).1);
    // `live PERSONA --help`: the word after `live` is a persona, not a subcommand: live's help
    assert_eq!(probbit(&["live", "tutor.yaml", "--help"]).1, probbit(&["help", "live"]).1);
}

#[test]
fn help_errors_exit_2() {
    for args in [vec!["help", "frobnicate"], vec!["help", "persona", "frobnicate"], vec!["help", "persona", "init", "extra"]] {
        let (c, out, err) = probbit(&args); assert_eq!(c, 2, "{args:?}"); assert!(out.is_empty(), "{args:?}: {out}"); assert!(err.starts_with("probbit: "), "{args:?}: {err}");
    }
    let (_, _, err) = probbit(&["help", "persona", "frobnicate"]); assert!(err.contains("subcommand \"frobnicate\"") && err.contains("probbit persona --help"), "{err}");
}
