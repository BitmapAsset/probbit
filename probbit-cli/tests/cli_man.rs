//! R94 item 7: `probbit man` prints the manual page (roff) from the command table; docs/probbit.1 is that output, committed (the
//! release attaches it and install.sh installs it); the page renders under `man -l` (man-db) or mandoc.
use std::process::{Command, Stdio};

fn man() -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_probbit")).arg("man").output().unwrap();
    assert!(o.status.success() && o.stderr.is_empty(), "{}", String::from_utf8_lossy(&o.stderr)); String::from_utf8(o.stdout).unwrap()
}
const PAGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/probbit.1");

#[test]
fn committed_man_page_is_generated() {
    let committed = std::fs::read_to_string(PAGE).unwrap().replace("\r\n", "\n");
    assert!(committed == man(), "docs/probbit.1 is stale: regenerate it with `cargo run -p probbit-cli -- man > docs/probbit.1` (after any help or version change)");
}

#[test]
fn man_page_has_every_section_command_and_flag() {
    let m = man();
    assert!(m.starts_with(&format!(".TH PROBBIT 1 \"\" \"probbit {}\"", env!("CARGO_PKG_VERSION"))), "{}", &m[..80]);
    let at = |s: &str| m.find(s).unwrap_or_else(|| panic!("no {s:?}"));
    let sections = [".SH NAME\n", ".SH SYNOPSIS\n", ".SH DESCRIPTION\n", ".SH COMMANDS\n", ".SH EXIT STATUS\n", ".SH EXAMPLES\n", ".SH SEE ALSO\n"];
    assert!(sections.windows(2).all(|w| at(w[0]) < at(w[1])), "sections out of order");
    assert!(m.is_ascii(), "the page is ASCII (non-ASCII as \\[uXXXX])");
    for c in ["persona", "live", "monitor", "decide", "run", "evaluate", "demo", "ir", "mcp", "stats", "help", "completions", "man", "version"] { at(&format!(".SS {c}\n")); }
    // every flag of every help screen, as roff writes it
    let mut screens: Vec<Vec<&str>> = ["persona", "live", "monitor", "decide", "run", "evaluate", "demo", "ir", "stats"].iter().map(|c| vec![*c, "--help"]).collect();
    for s in ["init", "turn", "replay", "explain", "diff", "lint", "fuzz", "prove", "compile"] { screens.push(vec!["persona", s, "--help"]); }
    for s in ["verify", "control"] { screens.push(vec!["live", s, "--help"]); }
    for a in screens {
        let h = String::from_utf8(Command::new(env!("CARGO_BIN_EXE_probbit")).args(&a).output().unwrap().stdout).unwrap();
        for f in h.lines().skip_while(|l| *l != "flags:").skip(1).take_while(|l| l.starts_with(' ')).filter_map(|l| l.strip_prefix("  --")) {
            let f = f.split_whitespace().next().unwrap(); assert!(m.contains(&format!("\\fB\\-\\-{}\\fR", f.replace('-', "\\-"))), "{a:?}: --{f} not in the man page");
        }
    }
}

#[test]
fn man_page_renders() {
    if !cfg!(unix) { eprintln!("no man on Windows: render check skipped"); return; }
    // man-db (Linux): `man -l FILE`; elsewhere mandoc (macOS, BSD)
    let try_run = |cmd: &str, args: &[&str]| Command::new(cmd).args(args).env("MANPAGER", "cat").env("PAGER", "cat").env("MANWIDTH", "100").stdin(Stdio::null()).output().ok().filter(|o| o.status.success());
    let out = try_run("man", &["-l", PAGE]).or_else(|| try_run("mandoc", &["-T", "utf8", PAGE]));
    let Some(o) = out else { eprintln!("neither `man -l` nor mandoc here: render check skipped"); return };
    // overstrike bold and underline (`c BS c`, `_ BS c`): keep the character after each backspace
    let raw = String::from_utf8_lossy(&o.stdout); let mut text = String::new();
    for c in raw.chars() { if c == '\u{8}' { text.pop(); } else { text.push(c); } }
    for w in ["NAME", "SYNOPSIS", "COMMANDS", "EXIT STATUS", "SEE ALSO"] { assert!(text.contains(w), "rendered page lacks {w}"); }
    assert!(text.contains("persona") && text.contains("budget") && text.contains("5.7"), "{}", &text[..text.len().min(400)]);
}
