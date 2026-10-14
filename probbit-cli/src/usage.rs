//! What the command table (`COMMANDS`, main.rs) renders: every `--help` / `probbit help` screen and the did-you-mean of an
//! unknown command or subcommand. The parser reads the same table (`flags_of`,
//! `sub_flags`), so the help, the completions and the man page list exactly what is accepted and cannot drift from it.

/// A flag. `arg`: "" = a switch; "[X]" = an optional value (parsed as a switch: `--progress [MS]`, `--demo [week|drives]`);
/// anything else = the value it takes (`N`, `on|off`, `FILE`).
pub(crate) struct Flag { pub name: &'static str, pub arg: &'static str, pub help: &'static str }
impl Flag { pub fn takes_value(&self) -> bool { !self.arg.is_empty() && !self.arg.starts_with('[') } }
/// A subcommand: `args` is its synopsis after `probbit CMD SUB` (positionals and every flag of `flags`, test `synopses_match_flags`).
pub(crate) struct Sub { pub name: &'static str, pub args: &'static str, pub help: &'static str, pub flags: &'static [Flag] }
pub(crate) struct Command {
    pub name: &'static str,
    /// other spellings the dispatcher accepts (`--version`, `-V`)
    pub aliases: &'static [&'static str],
    /// the top-level help's group: persona, live, monitor, decide/run, mcp, other
    pub group: &'static str,
    /// one line for the top-level help, the completions and the man page's COMMANDS
    pub summary: &'static str,
    /// synopsis lines after `probbit ` (the first one names the command in the top-level help)
    pub usage: &'static [&'static str],
    /// what it does; "\n" starts a paragraph; wrapped to 100 columns when rendered
    pub about: &'static str,
    /// sections before the flags (title, text)
    pub before: &'static [(&'static str, &'static str)],
    pub flags: &'static [&'static [Flag]],
    pub subs: &'static [Sub],
    /// the subcommands get their own lines in the top-level help (live verify, live control)
    pub list_subs: bool,
    /// the command's flags apply to every subcommand too (persona's --pretty); else only to the command's own form (live PERSONA)
    pub shared: bool,
    /// sections after the flags and subcommands (controls, the safety kit, exit)
    pub after: &'static [(&'static str, &'static str)],
    pub examples: &'static [&'static str],
    pub run: fn(&[String]),
}
impl Command {
    pub fn flags(&self) -> impl Iterator<Item = &'static Flag> { self.flags.iter().flat_map(|g| g.iter()) }
    pub fn sub(&self, name: &str) -> Option<&'static Sub> { self.subs.iter().find(|s| s.name == name) }
    /// The flags a subcommand accepts: its own, then the command's when they are shared
    pub fn sub_flags(&self, s: &'static Sub) -> impl Iterator<Item = &'static Flag> {
        let own: &'static [&'static [Flag]] = if self.shared { self.flags } else { &[] }; s.flags.iter().chain(own.iter().flat_map(|g| g.iter()))
    }
}

pub(crate) const WIDTH: usize = 100;
/// Where a flag's or a subcommand's description starts
const COL: usize = 28;
/// The same in the top-level help
const TOP_COL: usize = 24;
pub(crate) const GROUPS: [&str; 6] = ["persona", "live", "monitor", "decide/run", "mcp", "other"];

/// `text` wrapped to `WIDTH`: the first line starts with `first` (already `first.len()` columns), the others with `indent`
/// spaces. "\n" in `text` starts a new line (indented). A word longer than a line is never split.
pub(crate) fn wrap(first: &str, text: &str, indent: usize) -> String {
    let mut out = String::new();
    for (i, para) in text.split('\n').enumerate() {
        let mut line = if i == 0 { first.to_string() } else { " ".repeat(indent) };
        let mut empty = i != 0 || first.trim().is_empty() || first.ends_with(' ');
        for w in para.split(' ').filter(|w| !w.is_empty()) {
            let len = line.chars().count();
            if !empty && len + 1 + w.chars().count() > WIDTH { out.push_str(line.trim_end()); out.push('\n'); line = " ".repeat(indent); empty = true; }
            if !empty && !line.ends_with(' ') { line.push(' '); }
            line.push_str(w); empty = false;
        }
        out.push_str(line.trim_end()); out.push('\n');
    }
    out
}
/// A synopsis wrapped to `WIDTH` without splitting a [bracketed] or (grouped) part: continuation lines start at `indent`.
fn wrap_synopsis(first: &str, syn: &str, indent: usize) -> String {
    let mut parts: Vec<String> = vec![]; let mut depth = 0i32;
    for w in syn.split(' ').filter(|w| !w.is_empty()) {
        if depth > 0 { let l = parts.last_mut().unwrap(); l.push(' '); l.push_str(w); } else { parts.push(w.to_string()); }
        depth += w.matches(['[', '(']).count() as i32 - w.matches([']', ')']).count() as i32;
    }
    let (mut out, mut line) = (String::new(), first.to_string());
    for p in parts {
        if line.chars().count() + 1 + p.chars().count() > WIDTH && line.trim().len() > first.trim().len() { out.push_str(&line); out.push('\n'); line = " ".repeat(indent); }
        if !line.ends_with(' ') { line.push(' '); } line.push_str(&p);
    }
    out.push_str(&line); out.push('\n'); out
}
/// `  left   description`, the description from column `col` (on the next line when `left` is wider)
fn item_at(left: &str, help: &str, col: usize) -> String {
    let n = left.chars().count();
    if n + 2 <= col { wrap(&format!("{left}{}", " ".repeat(col - n)), help, col) } else { format!("{left}\n{}", wrap(&" ".repeat(col), help, col)) }
}
fn item(left: &str, help: &str) -> String { item_at(left, help, COL) }
fn section(title: &str, text: &str) -> String { wrap(&format!("{title}: "), text, 2) }
fn flag_left(f: &Flag) -> String { if f.arg.is_empty() { format!("  {}", f.name) } else { format!("  {} {}", f.name, f.arg) } }

/// `probbit CMD --help` = `probbit help CMD`
pub(crate) fn command_help(c: &Command) -> String {
    let mut h = String::new();
    for (i, u) in c.usage.iter().enumerate() { h += &wrap_synopsis(if i == 0 { "usage: probbit" } else { "       probbit" }, u, 15); }
    h += &wrap("  ", c.about, 2);
    for (t, x) in c.before { h += &section(t, x); }
    if c.flags().next().is_some() { h += "flags:\n"; for f in c.flags() { h += &item(&flag_left(f), f.help); } }
    if !c.subs.is_empty() {
        h += &format!("subcommands (probbit help {} SUBCOMMAND: its flags):\n", c.name);
        for s in c.subs { h += &wrap_synopsis(&format!("  {}", s.name), s.args, 6); h += &wrap(&" ".repeat(6), s.help, 6); }
    }
    for (t, x) in c.after { h += &section(t, x); }
    if !c.examples.is_empty() { h += "examples:\n"; for e in c.examples { h += &format!("  {e}\n"); } }
    h
}
/// `probbit CMD SUB --help` = `probbit help CMD SUB`: the subcommand's synopsis, what it does, its flags (and the command's own,
/// such as persona's --pretty), the command's exit codes
pub(crate) fn sub_help(c: &Command, s: &'static Sub) -> String {
    let mut h = wrap_synopsis(&format!("usage: probbit {} {}", c.name, s.name), s.args, 15);
    h += &wrap("  ", s.help, 2);
    if c.sub_flags(s).next().is_some() { h += "flags:\n"; for f in c.sub_flags(s) { h += &item(&flag_left(f), f.help); } }
    for (t, x) in c.after.iter().filter(|(t, _)| *t == "exit") { h += &section(t, x); }
    h
}
/// The first words of a synopsis: up to the first flag, option group, choice or redirection (`live verify STRAND
/// [--from-checkpoint]` -> `live verify STRAND`)
fn head(syn: &str) -> String {
    syn.split(' ').take_while(|w| !w.is_empty() && !w.starts_with(['[', '-', '<', '>', '(']) && !w.contains('|')).collect::<Vec<_>>().join(" ")
}
/// `probbit --help` piped, `probbit help`, the usage a bare `probbit` prints on stderr
pub(crate) fn top_help(cmds: &[Command], version: &str) -> String {
    let mut h = format!("probbit {version} · a virtual p-bit processor: joint decisions under hard rules, with gated odds\n");
    h += "usage: probbit COMMAND [flags]   ·   probbit help COMMAND [SUBCOMMAND]   ·   probbit COMMAND --help\n";
    for g in GROUPS {
        h += &format!("\n{g}\n");
        for c in cmds.iter().filter(|c| c.group == g) {
            h += &item_at(&format!("  {}", head(c.usage[0])), c.summary, TOP_COL);
            if c.list_subs { for s in c.subs { h += &item_at(&format!("  {} {} {}", c.name, s.name, head(s.args)), first_sentence(s.help), TOP_COL); } }
        }
    }
    h += "\nevery command\n";
    h += &item_at("  --help, -h", "usage, every flag with its default, the exit codes (= probbit help COMMAND)", TOP_COL);
    h += &item_at("  --plain", "no colour, no animation (as NO_COLOR=1 or PROBBIT_THEME=plain)", TOP_COL);
    h += "\n"; h += &section("exit", EXIT);
    h += "\nexamples:\n";
    for e in TOP_EXAMPLES { h += &format!("  {e}\n"); }
    h
}
/// The exit codes of every command (the man page's EXIT STATUS, the top-level help)
pub(crate) const EXIT: &str = "0 done or an answer; 1 an answer that says no (infeasible, a line that differs, a counterexample, a rule left unknown); 2 a bad command, flag or input; 3 refused / declined / non-finite (decide, run, evaluate); 4 refused by a lock, a pause or a retirement (live). Each command's --help has its own list.";
pub(crate) const TOP_EXAMPLES: [&str; 6] = [
    "probbit demo | probbit decide --summary --pretty",
    "probbit run --pretty < examples/knapsack-20.json",
    "probbit persona init examples/persona/tutor.yaml --out pip.json",
    "probbit live examples/persona/tutor.yaml --seed 2 --demo week",
    "probbit monitor --demo --open",
    "claude mcp add probbit -- probbit mcp",
];
fn first_sentence(s: &str) -> &str { s.split(['\n', ';']).next().unwrap_or(s).trim() }

/// Optimal-string-alignment (restricted Damerau-Levenshtein) distance: insertions, deletions, substitutions and swaps of two
/// neighbouring characters, 1 each
pub(crate) fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, r) in d.iter_mut().enumerate() { r[0] = i; } for j in 0..=b.len() { d[0][j] = j; }
    for i in 1..=a.len() { for j in 1..=b.len() {
        let c = usize::from(a[i - 1] != b[j - 1]);
        d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + c);
        if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] { d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1); }
    } }
    d[a.len()][b.len()]
}
/// The candidate an unknown `word` most likely meant: the only one it is a prefix of (2 characters at least), else the closest
/// within distance 2 (1 for a word of 3 characters or fewer, so `x` does not become `ir`); ties go to the earlier candidate.
pub(crate) fn suggest<'a>(word: &str, cands: &[&'a str]) -> Option<&'a str> {
    let pre: Vec<&&str> = cands.iter().filter(|c| word.chars().count() >= 2 && c.starts_with(word)).collect();
    if pre.len() == 1 { return Some(pre[0]); }
    let max = if word.chars().count() <= 3 { 1 } else { 2 };
    cands.iter().map(|c| (distance(word, c), *c)).filter(|(d, _)| *d <= max).min_by_key(|(d, _)| *d).map(|(_, c)| c)
}
