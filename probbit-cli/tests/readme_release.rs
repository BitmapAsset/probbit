//! The landing page names the release (RELEASING.md): README.md's line "The current release is vX.Y.Z." agrees with the
//! version Cargo.toml gives this build (`CARGO_PKG_VERSION`, the workspace version): equal on a release, lower on a `-dev`
//! build. No `vX.Y.Z` in the README is newer than that release (an example would point at a tag that does not exist).
const README: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../README.md");

fn version(s: &str) -> Option<(u64, u64, u64)> {
    let mut p = s.split('.').map(|n| n.parse::<u64>().ok());
    let v = (p.next()??, p.next()??, p.next()??);
    if p.next().is_some() { None } else { Some(v) }
}

/// Every `vX.Y.Z` in `text` whose `v` does not follow a letter or a digit, with its three numbers.
fn tags(text: &str) -> Vec<(String, (u64, u64, u64))> {
    let (b, mut out, mut i) = (text.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'v' && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let mut j = i + 1; while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') { j += 1; }
            let s = text[i + 1..j].trim_end_matches('.');
            if let Some(v) = version(s) { out.push((format!("v{s}"), v)); }
            i = j;
        } else { i += 1; }
    }
    out
}

#[test]
fn the_readme_names_the_release_cargo_toml_builds() {
    let readme = std::fs::read_to_string(README).unwrap();
    let cargo = env!("CARGO_PKG_VERSION");
    let (core, pre) = match cargo.split_once('-') { Some((c, p)) => (c, Some(p)), None => (cargo, None) };
    let build = version(core).unwrap_or_else(|| panic!("Cargo.toml's version {cargo:?} is not X.Y.Z or X.Y.Z-<pre>"));
    let said: Vec<&str> = readme.lines().filter_map(|l| l.split("The current release is ").nth(1)).collect();
    assert_eq!(said.len(), 1, "README.md needs exactly one line \"The current release is vX.Y.Z.\", found {}", said.len());
    let (tag, current) = tags(said[0]).into_iter().next().unwrap_or_else(|| panic!("no vX.Y.Z after \"The current release is\": {:?}", said[0]));
    match pre {
        None => assert!(current == build, "Cargo.toml says {cargo}, README.md says the current release is {tag}: update the landing page (RELEASING.md)"),
        Some(_) => assert!(current < build, "Cargo.toml says {cargo} (unreleased), README.md says the current release is {tag}: it must name a release below it"),
    }
    for (t, v) in tags(&readme) { assert!(v <= current, "README.md shows {t}, newer than its current release {tag}: a tag that does not exist yet"); }
}
