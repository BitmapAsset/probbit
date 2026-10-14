//! R94 item 5: help layout. Every help screen (top-level, every command, every subcommand, `probbit help ...`) fits 100 columns;
//! the top-level help starts with the version line, groups the commands (persona · live · monitor · decide/run · mcp · other)
//! and ends with 4-6 examples; `monitor --help` is split into usage / modes / flags / demo sections.
use std::process::Command;

fn probbit(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
fn screens() -> Vec<(String, String)> {
    let mut v: Vec<Vec<&str>> = vec![vec!["--help"], vec!["help"]];
    for c in ["persona", "live", "monitor", "decide", "run", "evaluate", "demo", "ir", "mcp", "stats", "help", "completions", "version"] { v.push(vec![c, "--help"]); }
    for s in ["init", "turn", "replay", "explain", "diff", "lint", "fuzz", "prove", "check", "compile", "describe"] { v.push(vec!["persona", s, "--help"]); }
    for s in ["verify", "control"] { v.push(vec!["live", s, "--help"]); }
    v.into_iter().map(|a| { let (c, out, err) = probbit(&a); assert_eq!(c, 0, "{a:?}: {err}"); (a.join(" "), out) }).collect()
}

#[test]
fn no_help_line_is_wider_than_100_columns() {
    for (name, out) in screens() {
        for l in out.lines() { assert!(l.chars().count() <= 100, "`probbit {name}`: {} columns: {l}", l.chars().count()); }
        assert!(!out.contains('\t') && !out.lines().any(|l| l.ends_with(' ')), "`probbit {name}`: tabs or trailing spaces");
    }
}

#[test]
fn top_level_help_is_versioned_grouped_and_ends_with_examples() {
    let (_, h, _) = probbit(&["--help"]);
    assert!(h.starts_with(&format!("probbit {} · ", env!("CARGO_PKG_VERSION"))), "{h}");
    let at = |s: &str| h.find(s).unwrap_or_else(|| panic!("no {s:?} in {h}"));
    let groups = ["\npersona\n", "\nlive\n", "\nmonitor\n", "\ndecide/run\n", "\nmcp\n", "\nother\n"];
    assert!(groups.windows(2).all(|w| at(w[0]) < at(w[1])), "groups out of order");
    let ex: Vec<&str> = h[at("\nexamples:\n")..].lines().skip(2).collect();
    assert!((4..=6).contains(&ex.len()) && ex.iter().all(|l| l.starts_with("  ")), "{ex:?}");
    assert!(h.lines().count() <= 60, "{} lines", h.lines().count());
}

#[test]
fn monitor_help_has_sections() {
    let (_, h, _) = probbit(&["monitor", "--help"]);
    let at = |s: &str| h.find(s).unwrap_or_else(|| panic!("no {s:?} in {h}"));
    assert!(h.starts_with("usage: probbit monitor STRAND"));
    assert!(at("\nmodes: ") < at("\nflags:\n") && at("\nflags:\n") < at("\ndemo: ") && at("\ndemo: ") < at("\nexit: "), "{h}");
    // every flag and fact of the 0.8.1 paragraph is still there
    for w in ["--follow", "--once", "--plain", "--fps N", "--serve", "--open", "--port N", "--demo [week|drives]", "127.0.0.1", "no --host", "/events", "/doc/N",
        "heartbeat every 15 s", "10000", "xdg-open", "NO_COLOR=1", "TERM=dumb", "1-60, default 10", "default 0: a free one", "--demo drives", "under a minute", "changes no file"] {
        assert!(h.contains(w), "monitor help lost {w:?}");
    }
}
