//! R94 item 4: `probbit completions bash|zsh|fish` prints a static script generated from the command table. Each script names
//! every command, subcommand and flag the helps list; bash and zsh scripts pass `bash -n` / `zsh -n` (fish: `fish -n` where
//! installed); the bash one completes real words; an unknown shell exits 2 with the list.
use std::process::{Command, Stdio};

fn probbit(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
fn script(shell: &str) -> String { let (c, out, err) = probbit(&["completions", shell]); assert_eq!(c, 0, "{shell}: {err}"); assert!(err.is_empty()); out }
fn has(cmd: &str) -> bool { Command::new(cmd).arg("-c").arg("exit 0").stdout(Stdio::null()).stderr(Stdio::null()).status().map_or(false, |s| s.success()) }
fn tmp(name: &str, text: &str) -> std::path::PathBuf { let p = std::env::temp_dir().join(format!("probbit-compl-{}-{name}", std::process::id())); std::fs::write(&p, text).unwrap(); p }
/// (command, subcommand) -> the flags its help lists
fn helped() -> Vec<(String, Vec<String>)> {
    let flags = |h: &str| -> Vec<String> { h.lines().skip_while(|l| *l != "flags:").skip(1).take_while(|l| l.starts_with(' ')).filter_map(|l| l.strip_prefix("  --"))
        .map(|l| format!("--{}", l.split_whitespace().next().unwrap())).collect() };
    let mut v = vec![];
    for c in ["persona", "live", "monitor", "decide", "run", "evaluate", "demo", "ir", "mcp", "stats", "help", "completions", "version"] {
        v.push((c.to_string(), flags(&probbit(&[c, "--help"]).1)));
    }
    for s in ["init", "turn", "replay", "explain", "diff", "lint", "fuzz", "prove", "check", "compile", "describe"] { v.push((format!("persona {s}"), flags(&probbit(&["persona", s, "--help"]).1))); }
    for s in ["verify", "control"] { v.push((format!("live {s}"), flags(&probbit(&["live", s, "--help"]).1))); }
    v
}

#[test]
fn every_script_names_every_command_subcommand_and_flag() {
    let all = helped();
    for sh in ["bash", "zsh", "fish"] {
        let s = script(sh);
        for (name, flags) in &all {
            for w in name.split(' ') { assert!(s.contains(w), "{sh}: no {w}"); }
            for f in flags { let needle = if sh == "fish" { format!("-l {}", &f[2..]) } else { f.clone() }; assert!(s.contains(&needle), "{sh}: {name}: no {f}"); }
        }
    }
}

#[test]
fn scripts_parse() {
    if !cfg!(unix) { eprintln!("syntax checks run on Unix (a Windows bash takes no Windows paths)"); return; }
    let mut checked = vec![];
    for (sh, ext) in [("bash", "bash"), ("zsh", "zsh"), ("fish", "fish")] {
        if !has(sh) { eprintln!("no {sh} here: its syntax check skipped"); continue; }
        let p = tmp(&format!("syntax.{ext}"), &script(sh));
        let o = Command::new(sh).arg("-n").arg(&p).output().unwrap(); let _ = std::fs::remove_file(&p);
        assert!(o.status.success(), "{sh} -n: {}", String::from_utf8_lossy(&o.stderr)); checked.push(sh);
    }
    assert!(checked.contains(&"bash"), "bash is on every Unix runner");
}

/// The bash script completes: commands, subcommands, a subcommand's flags, a command's flags, the shells, help's commands
#[test]
fn bash_completes_real_words() {
    if !cfg!(unix) || !has("bash") { eprintln!("no bash: skipped"); return; }
    let p = tmp("functional.bash", &script("bash"));
    let probe = "t() { COMP_WORDS=(\"$@\"); COMP_CWORD=$(($# - 1)); COMPREPLY=(); _probbit; echo \"${COMPREPLY[*]}\"; }\n\
        t probbit pers; t probbit persona tu; t probbit persona turn x.yaml --st; t probbit live ver; t probbit live verify s --f;\n\
        t probbit decide --bud; t probbit completions z; t probbit help mon; t probbit monitor --de; t probbit persona fuzz x.yaml --fix";
    let o = Command::new("bash").arg("-c").arg(format!("source '{}'\n{probe}", p.display())).output().unwrap(); let _ = std::fs::remove_file(&p);
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(out.lines().collect::<Vec<_>>(), ["persona", "turn", "--state", "verify", "--from-checkpoint", "--budget-ms", "zsh", "monitor", "--demo", "--fixture-src"],
        "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn unknown_or_missing_shell_exits_2() {
    let (c, out, err) = probbit(&["completions", "tcsh"]); assert_eq!(c, 2); assert!(out.is_empty());
    assert_eq!(err, "probbit: completions: unknown shell \"tcsh\"; one of bash, zsh, fish\n");
    let (c, _, err) = probbit(&["completions"]); assert_eq!(c, 2); assert!(err.contains("bash, zsh, fish"), "{err}");
    let (c, _, err) = probbit(&["completions", "zsh", "--extra"]); assert_eq!(c, 2); assert!(err.contains("unknown argument \"--extra\" for `probbit completions`"), "{err}");
}
