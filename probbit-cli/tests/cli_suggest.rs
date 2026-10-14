//! R94 item 3: an unknown command or subcommand exits 2 (as before) and names the closest one (Damerau-Levenshtein distance
//! <= 2, or the one command it is a prefix of); a miss says where the list is. stdout stays empty.
use std::process::Command;

fn probbit(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_probbit")).args(args).output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}
fn err2(args: &[&str]) -> String { let (c, out, err) = probbit(args); assert_eq!(c, 2, "{args:?}: {err}"); assert!(out.is_empty(), "{args:?}: {out}"); err }

#[test]
fn unknown_command_hit() {
    assert_eq!(err2(&["persna", "init", "x.yaml"]), "probbit: unknown command \"persna\"; did you mean \"persona\"? see `probbit --help`\n");
    for (typo, want) in [("dcide", "decide"), ("rnu", "run"), ("monitr", "monitor"), ("pers", "persona"), ("verison", "version")] {
        assert!(err2(&[typo]).contains(&format!("did you mean \"{want}\"?")), "{typo}");
    }
    assert!(err2(&["help", "persna"]).contains("did you mean \"persona\"?"));
}

#[test]
fn unknown_command_miss() {
    assert_eq!(err2(&["frobnicate"]), "probbit: unknown command \"frobnicate\"; see `probbit --help`\n");
    assert!(!err2(&["x"]).contains("did you mean"), "a one-letter word is not anything's typo");
}

#[test]
fn unknown_subcommand_typo() {
    assert_eq!(err2(&["persona", "lnt", "x.yaml"]), "probbit: unknown persona subcommand \"lnt\"; did you mean \"lint\"? see `probbit persona --help`\n");
    assert!(err2(&["persona", "fuz", "x.yaml"]).contains("did you mean \"fuzz\"?"));
    assert_eq!(err2(&["persona", "frobnicate", "x.yaml"]), "probbit: unknown persona subcommand \"frobnicate\"; see `probbit persona --help`\n");
    assert!(err2(&["help", "persona", "descibe"]).contains("did you mean \"describe\"?"));
    // live's first word is a persona file; a missing file that is a subcommand's typo is named as one
    assert!(err2(&["live", "verfy", "x.strand"]).contains("unknown live subcommand \"verfy\"; did you mean \"verify\"?"));
    // no subcommand at all keeps its own message
    assert!(err2(&["persona"]).contains("persona: a subcommand: init, turn"));
}
