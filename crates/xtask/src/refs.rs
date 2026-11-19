//! `cargo xtask docs`: the security pages' references to code and tests still resolve (#333).
//!
//! `docs/security/*.md` cite code as `` `path/to/file.rs::Name::item` ``, and a later reference in
//! the same cell may name only `` `::item` `` of the file before it. Each reference's file must exist
//! and each of its names must appear in that file as a whole word. Every row of the threat model's
//! controls names a test, since a control without one belongs under Known gaps.

use std::fs;
use std::path::Path;

/// A reference to code: the file, relative to the repository root, and the names in it.
#[derive(Debug, PartialEq, Eq)]
pub struct Ref {
    pub file: String,
    pub names: Vec<String>,
}

/// The references in `text`, in order. `::name` alone refers to the file of the reference before it.
pub fn refs(text: &str) -> Vec<Ref> {
    let mut out: Vec<Ref> = Vec::new();
    for span in text.split('`').skip(1).step_by(2) {
        let (file, names) = match span.split_once("::") {
            Some(("", names)) => match out.last() {
                Some(last) => (last.file.clone(), names),
                None => continue,
            },
            Some((file, names)) if file.ends_with(".rs") && is_path(file) => {
                (file.to_string(), names)
            }
            _ => continue,
        };
        let names: Vec<String> = names.split("::").map(str::to_string).collect();
        if names.iter().all(|n| !n.is_empty() && n.chars().all(ident)) {
            out.push(Ref { file, names });
        }
    }
    out
}

fn is_path(file: &str) -> bool {
    file.contains('/')
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_.-".contains(c))
}

fn ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `name` appears in `source` as a whole word.
pub fn has_word(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(at, _)| {
        let before = source[..at].chars().next_back();
        let after = source[at + name.len()..].chars().next();
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

/// The threat model's control rows that name no test: the rows of every table between
/// `## Controls` and `## Known gaps` whose Test column holds no reference.
pub fn untested_rows(threat_model: &str) -> Vec<String> {
    let mut in_controls = false;
    let mut out = Vec::new();
    for line in threat_model.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_controls = heading.trim() == "Controls";
            continue;
        }
        if !in_controls || !line.starts_with('|') || line.starts_with("| ---") {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split(" | ").map(str::trim).collect();
        if cells.first() == Some(&"Threat") {
            continue;
        }
        let tested = cells.get(3).is_some_and(|test| !refs(test).is_empty());
        if !tested {
            out.push(cells.first().copied().unwrap_or_default().to_string());
        }
    }
    out
}

pub fn check(root: &Path) -> Result<(), String> {
    let dir = root.join("docs/security");
    let mut pages: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    pages.sort();
    let mut problems = Vec::new();
    let mut count = 0;
    for page in &pages {
        let text = fs::read_to_string(page).map_err(|e| format!("{}: {e}", page.display()))?;
        let shown = page.strip_prefix(root).unwrap_or(page).display().to_string();
        for r in refs(&text) {
            count += 1;
            let Ok(source) = fs::read_to_string(root.join(&r.file)) else {
                problems.push(format!("{shown}: {} does not exist", r.file));
                continue;
            };
            for name in &r.names {
                if !has_word(&source, name) {
                    problems.push(format!("{shown}: {}::{name} is not in that file", r.file));
                }
            }
        }
        if page.file_name().is_some_and(|n| n == "threat-model.md") {
            for row in untested_rows(&text) {
                problems.push(format!("{shown}: the control \"{row}\" names no test"));
            }
        }
    }
    if problems.is_empty() {
        println!("docs ok: {count} references in {} pages", pages.len());
        Ok(())
    } else {
        Err(format!(
            "{} problem(s) in docs/security:\n  {}",
            problems.len(),
            problems.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_read_with_their_file_and_continuations_take_the_last_file() {
        let found = refs(
            "see `core/a.rs::Thing::run`, `::other`, `Store::load` and `cargo test`, then `crates/b/src/c.rs::d`",
        );
        assert_eq!(
            found,
            [
                Ref {
                    file: "core/a.rs".into(),
                    names: vec!["Thing".into(), "run".into()]
                },
                Ref {
                    file: "core/a.rs".into(),
                    names: vec!["other".into()]
                },
                Ref {
                    file: "crates/b/src/c.rs".into(),
                    names: vec!["d".into()]
                },
            ]
        );
    }

    #[test]
    fn a_name_matches_only_as_a_whole_word() {
        let source = "pub fn check_published() {}\nfn check() {}";
        assert!(has_word(source, "check"));
        assert!(has_word(source, "check_published"));
        assert!(!has_word("fn check_published() {}", "check"));
        assert!(!has_word(source, "published"));
    }

    #[test]
    fn a_control_without_a_test_is_found_and_known_gaps_are_not_controls() {
        let page = "## Controls\n\n| Threat | Control | Code | Test | ADR |\n| --- | --- | --- | --- | --- |\n\
            | Tested | c | `core/a.rs::f` | `core/tests/a.rs::t` | 1 |\n\
            | Untested | c | `core/a.rs::f` | none yet | 2 |\n\n\
            ## Known gaps\n\n| Gap | Why |\n| --- | --- |\n| A gap | later |\n";
        assert_eq!(untested_rows(page), ["Untested"]);
    }
}
