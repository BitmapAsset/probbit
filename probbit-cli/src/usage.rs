//! What the command table (`COMMANDS`, main.rs) renders: every `--help` / `probbit help` screen, the did-you-mean of an unknown
//! command or subcommand, `probbit completions bash|zsh|fish` and `probbit man`. The parser reads the same table (`flags_of`,
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
fn wrap_synopsis(first: &str, syn: &str, indent: usize) -> String { wrap_synopsis_to(first, syn, indent, WIDTH) }
fn wrap_synopsis_to(first: &str, syn: &str, indent: usize, width: usize) -> String {
    let mut parts: Vec<String> = vec![]; let mut depth = 0i32;
    for w in syn.split(' ').filter(|w| !w.is_empty()) {
        if depth > 0 { let l = parts.last_mut().unwrap(); l.push(' '); l.push_str(w); } else { parts.push(w.to_string()); }
        depth += w.matches(['[', '(']).count() as i32 - w.matches([']', ')']).count() as i32;
    }
    let (mut out, mut line) = (String::new(), first.to_string());
    for p in parts {
        if line.chars().count() + 1 + p.chars().count() > width && line.trim().len() > first.trim().len() { out.push_str(&line); out.push('\n'); line = " ".repeat(indent); }
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
    h += &item_at("  --json", "errors as one {\"error\"} object on stdout, at a terminal and for bad flags too", TOP_COL);
    h += &item_at("  --plain", "no colour, no animation where it draws (as NO_COLOR=1, PROBBIT_THEME=plain)", TOP_COL);
    h += "\n"; h += &section("exit", EXIT);
    h += "\nexamples:\n";
    for e in TOP_EXAMPLES { h += &format!("  {e}\n"); }
    h
}
/// The exit codes of every command (the man page's EXIT STATUS, the top-level help)
pub(crate) const EXIT: &str = "0 done or an answer; 1 an answer that says no (infeasible, a line that differs, a counterexample, a rule left unknown); 2 a bad command, flag or input (a line on stderr; for bad input also one {\"error\"} object on stdout when stdout is not a terminal); 3 refused / declined / non-finite (decide, run, evaluate); 4 refused by a lock, a pause or a retirement (live). Each command's --help has its own list.";
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

/// The flag names a completion offers for a list of flags, plus --help and --json (every command takes them)
fn words(fs: &[&Flag]) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = fs.iter().map(|f| f.name).collect();
    for g in ["--help", "--json"] { if !v.contains(&g) { v.push(g); } } v
}
fn names(cmds: &[Command]) -> Vec<&'static str> { cmds.iter().map(|c| c.name).collect() }
fn quote_sh(s: &str) -> String { format!("'{}'", s.replace('\'', "'\\''")) }
pub(crate) const SHELLS: [&str; 3] = ["bash", "zsh", "fish"];
/// `probbit completions bash|zsh|fish`: a static script from the table (commands, subcommands, flags); None = an unknown shell
pub(crate) fn completions(shell: &str, cmds: &[Command]) -> Option<String> {
    match shell { "bash" => Some(bash(cmds)), "zsh" => Some(zsh(cmds)), "fish" => Some(fish(cmds)), _ => None }
}
/// bash: the words for the position (commands; a command's subcommands and flags; a subcommand's flags), files otherwise
/// (`complete -o default`)
fn bash(cmds: &[Command]) -> String {
    let mut s = String::from("# bash completion for probbit (generated by `probbit completions bash`)\n");
    s += "# install: probbit completions bash > ~/.local/share/bash-completion/completions/probbit\n";
    s += "_probbit() {\n    local cur=${COMP_WORDS[COMP_CWORD]} words=''\n";
    s += &format!("    if [ \"$COMP_CWORD\" -eq 1 ]; then\n        words={}\n    else\n        case \"${{COMP_WORDS[1]}}\" in\n", quote_sh(&[names(cmds), vec!["--help", "--version"]].concat().join(" ")));
    for c in cmds {
        let own = words(&c.flags().collect::<Vec<_>>());
        let first = match c.name { "help" => names(cmds), "completions" => SHELLS.to_vec(), _ => [c.subs.iter().map(|s| s.name).collect(), own.clone()].concat() };
        if c.subs.is_empty() {
            s += &format!("            {}) if [ \"$COMP_CWORD\" -eq 2 ]; then words={}; else words={}; fi ;;\n", c.name, quote_sh(&first.join(" ")), quote_sh(&own.join(" ")));
            continue;
        }
        s += &format!("            {})\n                if [ \"$COMP_CWORD\" -eq 2 ]; then words={}\n                else\n                    case \"${{COMP_WORDS[2]}}\" in\n", c.name, quote_sh(&first.join(" ")));
        for sb in c.subs { s += &format!("                        {}) words={} ;;\n", sb.name, quote_sh(&words(&c.sub_flags(sb).collect::<Vec<_>>()).join(" "))); }
        s += &format!("                        *) words={} ;;\n                    esac\n                fi ;;\n", quote_sh(&own.join(" ")));
    }
    s += "        esac\n    fi\n    COMPREPLY=($(compgen -W \"$words\" -- \"$cur\"))\n}\ncomplete -o default -F _probbit probbit\n";
    s
}
/// zsh: `_describe` for commands and subcommands, `_arguments` for the flags (with their help and what their value is)
fn zsh(cmds: &[Command]) -> String {
    let esc = |t: &str| t.replace('\'', "'\\''").replace('[', "\\[").replace(']', "\\]");
    let name = |t: &str| esc(t).replace(':', "\\:");
    let spec = |f: &Flag| {
        let help = esc(first_sentence(f.help)); let a = f.arg.trim_start_matches('[').trim_end_matches(']');
        let action = if ["FILE", "STATE", "STRAND", "TRACE", "PERSONA"].iter().any(|k| a.contains(k)) { "_files".to_string() }
            else if !a.is_empty() && a.chars().all(|c| c.is_ascii_lowercase() || c == '|' || c == ':') { format!("({})", a.split('|').map(|x| x.replace(':', "\\:")).collect::<Vec<_>>().join(" ")) }
            else { " ".to_string() };
        if f.arg.is_empty() { format!("'{}[{help}]'", f.name) }
        else { format!("'{}[{help}]{}{}:{action}'", f.name, if f.takes_value() { ":" } else { "::" }, a.replace(':', "\\:").replace('|', "\\|")) }
    };
    let args = |fs: Vec<&Flag>| -> String {
        let mut a: Vec<String> = fs.iter().map(|f| spec(f)).collect();
        if !fs.iter().any(|f| f.name == "--json") { a.push("'--json[errors as JSON on stdout]'".into()); }
        a.push("'(- *)'{-h,--help}'[usage and every flag]'".into()); a.push("'*:file:_files'".into()); a.join(" \\\n                ")
    };
    let mut s = String::from("#compdef probbit\n# zsh completion for probbit (generated by `probbit completions zsh`)\n");
    s += "# install: probbit completions zsh > ~/.zfunc/_probbit, with fpath=(~/.zfunc $fpath) before compinit in ~/.zshrc\n";
    s += "_probbit() {\n    local -a cmds subs\n    if (( CURRENT == 2 )); then\n        cmds=(\n";
    for c in cmds { s += &format!("            '{}:{}'\n", name(c.name), esc(c.summary)); }
    s += "        )\n        _describe -t commands 'probbit command' cmds\n        return\n    fi\n    case $words[2] in\n";
    for c in cmds {
        match c.name {
            "help" => { s += &format!("        help) _values 'command' {} ;;\n", names(cmds).join(" ")); continue; }
            "completions" => { s += &format!("        completions) _values 'shell' {} ;;\n", SHELLS.join(" ")); continue; }
            _ => {}
        }
        if c.subs.is_empty() { s += &format!("        {})\n            shift words; (( CURRENT-- ))\n            _arguments {} ;;\n", c.name, args(c.flags().collect())); continue; }
        s += &format!("        {})\n            if (( CURRENT == 3 )); then\n                subs=(\n", c.name);
        for sb in c.subs { s += &format!("                    '{}:{}'\n", name(sb.name), esc(first_sentence(sb.help))); }
        s += "                )\n                _describe -t subcommands 'subcommand' subs\n";
        if c.name == "live" { s += "                _files\n"; }
        s += "                return\n            fi\n            case $words[3] in\n";
        for sb in c.subs { s += &format!("                {})\n                    shift 2 words; (( CURRENT -= 2 ))\n                    _arguments {} ;;\n", sb.name, args(c.sub_flags(sb).collect())); }
        s += &format!("                *)\n                    shift words; (( CURRENT-- ))\n                    _arguments {} ;;\n            esac ;;\n", args(c.flags().collect()));
    }
    s += "        *) _files ;;\n    esac\n}\nif [ \"$funcstack[1]\" = \"_probbit\" ]; then _probbit \"$@\"; else compdef _probbit probbit; fi\n";
    s
}
/// fish: one `complete` line per command, subcommand and flag, each with its condition and its help
fn fish(cmds: &[Command]) -> String {
    let q = |t: &str| format!("'{}'", t.replace('\\', "\\\\").replace('\'', "\\'"));
    let flag = |cond: &str, f: &Flag| format!("complete -c probbit -n {} -l {}{} -d {}\n", q(cond), &f.name[2..], if f.takes_value() { " -r" } else { "" }, q(first_sentence(f.help)));
    let mut s = String::from("# fish completion for probbit (generated by `probbit completions fish`)\n");
    s += "# install: probbit completions fish > ~/.config/fish/completions/probbit.fish\n";
    for c in cmds { s += &format!("complete -c probbit -f -n __fish_use_subcommand -a {} -d {}\n", c.name, q(c.summary)); }
    for c in cmds {
        let on = format!("__fish_seen_subcommand_from {}", c.name);
        match c.name {
            "help" => s += &format!("complete -c probbit -f -n {} -a {}\n", q(&on), q(&names(cmds).join(" "))),
            "completions" => s += &format!("complete -c probbit -f -n {} -a {}\n", q(&on), q(&SHELLS.join(" "))),
            _ => {}
        }
        let subs: Vec<&str> = c.subs.iter().map(|s| s.name).collect();
        for sb in c.subs {
            s += &format!("complete -c probbit -n {} -a {} -d {}\n", q(&format!("{on}; and not __fish_seen_subcommand_from {}", subs.join(" "))), sb.name, q(first_sentence(sb.help)));
            for f in sb.flags { s += &flag(&format!("{on}; and __fish_seen_subcommand_from {}", sb.name), f); }
        }
        let own = if c.shared || subs.is_empty() { on.clone() } else { format!("{on}; and not __fish_seen_subcommand_from {}", subs.join(" ")) };
        for f in c.flags() { s += &flag(&own, f); }
        s += &format!("complete -c probbit -n {} -l json -d 'errors as JSON on stdout'\n", q(&on));
    }
    s += "complete -c probbit -s h -l help -d 'usage and every flag'\n";
    s
}

/// roff text: backslashes as `\e`, `-` as `\-` (a real hyphen-minus: flags can be copied), any other non-ASCII character as
/// `\[uXXXX]` (the page is ASCII: § · → render under groff and mandoc alike), a line-leading `.` or `'` guarded
fn roff(t: &str) -> String {
    let t: String = t.replace('\\', "\\e").replace('-', "\\-").chars().map(|c| if c.is_ascii() { c.to_string() } else { format!("\\[u{:04X}]", c as u32) }).collect();
    t.split('\n').map(|l| if l.starts_with(['.', '\'']) { format!("\\&{l}") } else { l.to_string() }).collect::<Vec<_>>().join("\n")
}
fn roff_flag(f: &Flag) -> String { format!(".TP\n\\fB{}\\fR{}\n{}\n", roff(f.name), if f.arg.is_empty() { String::new() } else { format!(" \\fI{}\\fR", roff(f.arg)) }, roff(&f.help.replace('\n', " "))) }
/// A synopsis line in a no-fill block, wrapped for an 80-column terminal (7 columns of man's indent) with a hanging indent
fn man_synopsis(u: &str) -> String {
    let w = wrap_synopsis_to("probbit", u, 8, 72);
    roff(w.trim_end()).replacen("probbit", "\\fBprobbit\\fR", 1) + "\n"
}
/// `probbit man`: the manual page (roff, man(7) macros) from the table: NAME, SYNOPSIS, DESCRIPTION, COMMANDS, EXIT STATUS,
/// ENVIRONMENT, EXAMPLES, SEE ALSO. docs/probbit.1 is this output, committed (test `committed_man_page_is_generated`).
pub(crate) fn man(cmds: &[Command], version: &str) -> String {
    let mut m = format!(".TH PROBBIT 1 \"\" \"probbit {version}\" \"probbit manual\"\n");
    m += ".SH NAME\nprobbit \\- a virtual probabilistic (p\\-bit) processor: joint decisions under hard rules, with gated odds\n";
    m += ".SH SYNOPSIS\n.nf\n";
    for c in cmds { for u in c.usage { m += &man_synopsis(u); } }
    m += ".fi\n.SH DESCRIPTION\n";
    m += &roff("probbit reads a JSON problem on stdin and writes a JSON answer on stdout: exact where the structure allows, sampled otherwise, and a sampled answer passes a published diagnostics gate or is refused. The persona, live and monitor commands are the individuality layer: a persona file, an individual's state and a turn's inputs give one stance with exact odds, and a strand logs a life that replays byte for byte.");
    m += "\n.PP\n";
    m += &roff("Every command takes --help (-h): its usage, every flag with its default and its exit codes; probbit help COMMAND [SUBCOMMAND] prints the same. An unknown command or subcommand exits 2 and names the closest one.");
    m += "\n.PP\n";
    m += &roff("Errors: one human line on stderr, always. A bad input document also gives one {\"error\": {\"code\", \"path\", \"message\"}} object on stdout when stdout is not a terminal, or with --json. A bad command or flag prints nothing on stdout unless --json is given (code \"flag\", or \"command\" with a \"suggestion\"). --json is accepted by every command and changes no answer.");
    m += "\n.SH COMMANDS\n";
    for c in cmds {
        m += &format!(".SS {}\n{}\n", c.name, roff(c.summary));
        m += ".PP\n.nf\n"; for u in c.usage { m += &man_synopsis(u); } m += ".fi\n";
        for p in c.about.split('\n') { m += &format!(".PP\n{}\n", roff(p)); }
        for (t, x) in c.before { m += &format!(".PP\n\\fI{}\\fR: {}\n", roff(t), roff(&x.replace('\n', " "))); }
        if c.flags().next().is_some() { m += ".PP\nFlags:\n.RS 4\n"; for f in c.flags() { m += &roff_flag(f); } m += ".RE\n"; }
        if !c.subs.is_empty() {
            m += ".PP\nSubcommands:\n.RS 4\n";
            for s in c.subs {
                m += &format!(".TP\n\\fB{}\\fR {}\n{}\n", roff(s.name), roff(s.args), roff(&s.help.replace('\n', " ")));
                if !s.flags.is_empty() { m += ".RS 4\n"; for f in s.flags { m += &roff_flag(f); } m += ".RE\n"; }
            }
            m += ".RE\n";
        }
        for (t, x) in c.after { m += &format!(".PP\n\\fI{}\\fR: {}\n", roff(t), roff(&x.replace('\n', " "))); }
        if !c.examples.is_empty() { m += ".PP\nExamples:\n.RS 4\n.nf\n"; for e in c.examples { m += &format!("{}\n", roff(e)); } m += ".fi\n.RE\n"; }
    }
    m += &format!(".SH EXIT STATUS\n{}\n", roff(EXIT));
    m += ".SH ENVIRONMENT\n";
    m += &roff("PROBBIT_CHAINS, PROBBIT_THREADS, PROBBIT_CPU_LIMIT, PROBBIT_MEM_LIMIT_MB, PROBBIT_PRIORITY: the processor controls (flag > environment > probbit.json, or the file PROBBIT_CONFIG names > default). NO_COLOR=1 or PROBBIT_THEME=plain: no colour on a terminal.");
    m += "\n.SH EXAMPLES\n.nf\n";
    for e in TOP_EXAMPLES { m += &format!("{}\n", roff(e)); }
    m += ".fi\n.SH SEE ALSO\n";
    m += &roff("README.md, docs/CHEATSHEET.md, docs/persona.md, docs/probbit-ir-json.md and docs/agents.md in the release archive or at https://github.com/BitmapAsset/probbit");
    m += "\n";
    m
}
