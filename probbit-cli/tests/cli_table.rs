//! R94 item 1: the command table is the one source. Every command the top-level help lists runs (`COMMAND --help` exits 0 with
//! its usage), every subcommand a help lists has its own help, and every flag a help lists is accepted by the parser.
use std::process::Command;

fn probbit(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).env_remove("PROBBIT_CONFIG").output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
/// The flags a help's `flags:` section lists
fn help_flags(h: &str) -> Vec<String> {
    h.lines().skip_while(|l| *l != "flags:").skip(1).take_while(|l| l.starts_with(' ')).filter_map(|l| l.strip_prefix("  --"))
        .map(|l| format!("--{}", l.split_whitespace().next().unwrap())).collect()
}
/// The commands of the top-level help (piped): the first word of each two-space indented line of the groups
fn listed() -> Vec<String> {
    let (c, out, err) = probbit(&["--help"]); assert_eq!(c, 0, "{err}");
    let groups = &out[..out.find("\nevery command\n").expect("the flags every command takes")];
    let mut v: Vec<String> = groups.lines().filter(|l| l.starts_with("  ") && !l.starts_with("   ") && !l.trim_start().starts_with('-'))
        .filter_map(|l| l.split_whitespace().next()).filter(|w| w.chars().all(|c| c.is_ascii_lowercase())).map(String::from).collect();
    v.dedup(); v.retain(|w| w != "probbit" && w != "claude"); v
}

#[test]
fn every_listed_command_runs_and_every_old_command_is_listed() {
    let cmds = listed();
    for old in ["persona", "live", "check", "monitor", "decide", "run", "evaluate", "demo", "ir", "mcp", "stats", "version"] { assert!(cmds.iter().any(|c| c == old), "{old} missing from {cmds:?}"); }
    for c in &cmds {
        let (code, out, err) = probbit(&[c, "--help"]);
        assert_eq!(code, 0, "{c}: {err}"); assert!(out.starts_with(&format!("usage: probbit {c}")), "{c}: {out}"); assert!(err.is_empty(), "{c}: {err}");
    }
    for alias in ["--version", "-V"] { let (code, out, _) = probbit(&[alias]); assert_eq!((code, out.trim()), (0, concat!("probbit ", env!("CARGO_PKG_VERSION")))); }
    let (code, _, err) = probbit(&["frobnicate"]); assert_eq!(code, 2); assert!(err.contains("unknown command \"frobnicate\""), "{err}");
}

#[test]
fn every_listed_subcommand_has_help_and_every_listed_flag_parses() {
    for (cmd, subs) in [("persona", &["init", "turn", "replay", "explain", "diff", "lint", "fuzz", "prove", "check", "compile", "describe"][..]), ("live", &["verify", "control"][..])] {
        let (_, h, _) = probbit(&[cmd, "--help"]);
        for s in subs {
            assert!(h.contains(&format!("\n  {s} ")), "{cmd} help lists no {s}");
            let (code, out, err) = probbit(&[cmd, s, "--help"]); assert_eq!(code, 0, "{cmd} {s}: {err}");
            assert!(out.starts_with(&format!("usage: probbit {cmd} {s}")), "{cmd} {s}: {out}");
            // every flag the subcommand's help lists is accepted: the error, if any, is never "unknown argument"
            for f in help_flags(&out) {
                let (_, _, err) = probbit(&[cmd, s, "no-such-file.yaml", &f]);
                assert!(!err.contains("unknown argument"), "{cmd} {s} {f}: {err}");
            }
        }
    }
    for cmd in ["decide", "run", "evaluate", "demo", "ir", "stats", "monitor", "check"] {
        let (_, h, _) = probbit(&[cmd, "--help"]);
        let fl = help_flags(&h); assert!(!fl.is_empty(), "{cmd}: no flags listed");
        for f in fl {
            // a value flag given no value fails on the value, never as an unknown argument
            let (_, _, err) = probbit(&[cmd, &f, "--help-no-such"]);
            assert!(!err.contains(&format!("unknown argument \"{f}\"")), "{cmd} {f}: {err}");
        }
    }
}
