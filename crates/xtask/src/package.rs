//! `cargo xtask review-package`: hyalOS's review package stands alone and claims no review (S-07 of
//! `docs/plans/hyalos-kernel-core`, its ADR-006).
//!
//! `hyalos-os/review/README.md` is what an independent reviewer starts from. The check fails when a
//! required section is missing or empty, when the commit under review is not named once by its full
//! hash or is not an ancestor of `HEAD`, when the sentence `not yet been independently reviewed` is
//! missing, when any other line claims a review took place, or when a code reference written as
//! `` `path.rs::name` `` does not resolve. It fails closed: a commit it cannot check is a failure.

use std::fs;
use std::path::Path;
use std::process::Command;

/// The review package, relative to the repository root.
pub const PACKAGE: &str = "hyalos-os/review/README.md";

/// The `##` headings the package must hold, verbatim (Slice 12's contract).
pub const SECTIONS: &[&str] = &[
    "Scope",
    "Threat model",
    "Interface",
    "Limits",
    "Commit under review",
    "Commands",
    "Findings",
];

/// The words the package must contain (12.1).
pub const NOT_REVIEWED: &str = "not yet been independently reviewed";

/// Phrases that claim a review took place, as whole words. A line is read in lower case with its
/// punctuation read as spaces, and every occurrence of `NOT_REVIEWED` taken out first, so the
/// required sentence itself never matches. Each line is also read joined to the next, so a claim
/// wrapped across two lines is found. The list errs towards failing.
const CLAIMS: &[&str] = &[
    "has been reviewed",
    "have been reviewed",
    "has been independently reviewed",
    "have been independently reviewed",
    "was reviewed",
    "were reviewed",
    "was independently reviewed",
    "been audited",
    "has had an independent review",
    "has had a review",
    "review has happened",
    "review has taken place",
    "review took place",
    "review is complete",
    "review was completed",
    "review has been completed",
    "reviewed by",
    "passed review",
    "passed an independent review",
    "independently reviewed and",
    "review complete",
    "review completed",
    "review finished",
    "review passed",
    "review found",
    "reviewers found",
    "passed a review",
    "passed an audit",
    "passed audit",
    "audit found",
    "audit complete",
    "audit completed",
    "audit is complete",
    "audit passed",
    "audited by",
    "independently audited",
    "underwent",
    "undergone",
    "certified",
];

/// `text` in lower case, with `NOT_REVIEWED` taken out and every run of characters that are not
/// letters or digits read as one space, padded so a phrase matches only whole words.
fn words(text: &str) -> String {
    let flat: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let flat = format!(
        " {} ",
        flat.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    flat.replace(&format!(" {NOT_REVIEWED} "), " ")
}

/// Whether `text` holds a phrase of `CLAIMS`.
fn claims_review(text: &str) -> bool {
    let text = words(text);
    CLAIMS
        .iter()
        .any(|claim| text.contains(&format!(" {claim} ")))
}

/// The body of each `## ` section, by heading, in order.
fn sections(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && let Some(heading) = line.strip_prefix("## ") {
            out.push((heading.trim().to_string(), String::new()));
            continue;
        }
        if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

/// Every full commit hash in `text`: a run of exactly 40 lower-case hex digits.
fn full_hashes(text: &str) -> Vec<String> {
    let hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
    let mut out = Vec::new();
    let mut run = String::new();
    // Whether the current run follows a letter or digit, so is part of a longer word.
    let mut joined = false;
    let mut before = ' ';
    for c in text.chars().chain(std::iter::once(' ')) {
        if hex(c) {
            if run.is_empty() {
                joined = before.is_ascii_alphanumeric();
            }
            run.push(c);
        } else {
            if run.len() == 40 && !joined && !c.is_ascii_alphanumeric() && !out.contains(&run) {
                out.push(run.clone());
            }
            run.clear();
        }
        before = c;
    }
    out
}

/// The lines that claim a review took place, as `line number: text`.
fn claims(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let next = lines.get(i + 1).copied().unwrap_or("");
        // A claim on the next line alone is reported there, not twice.
        let wrapped = !claims_review(next) && claims_review(&format!("{line} {next}"));
        if claims_review(line) || wrapped {
            out.push(format!("{}: {}", i + 1, line.trim()));
        }
    }
    out
}

/// What is wrong with the package `text`. `ancestor` says whether a commit is an ancestor of
/// `HEAD`, or why it could not tell; `read` gives a repository file's text, or `None`.
pub fn findings(
    text: &str,
    ancestor: impl Fn(&str) -> Result<bool, String>,
    read: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let found = sections(text);
    for want in SECTIONS {
        let bodies: Vec<&String> = found
            .iter()
            .filter(|(h, _)| h == want)
            .map(|(_, b)| b)
            .collect();
        match bodies.as_slice() {
            [] => out.push(format!("{PACKAGE}: no `## {want}` section")),
            [body] if body.trim().is_empty() => {
                out.push(format!("{PACKAGE}: the `## {want}` section is empty"))
            }
            [_] => {}
            _ => out.push(format!("{PACKAGE}: `## {want}` appears more than once")),
        }
    }
    let commit_section = found
        .iter()
        .find(|(h, _)| h == "Commit under review")
        .map(|(_, b)| b.as_str())
        .unwrap_or_default();
    match full_hashes(commit_section).as_slice() {
        [] => out.push(format!(
            "{PACKAGE}: `## Commit under review` names no commit by its full 40-character hash"
        )),
        [commit] => match ancestor(commit) {
            Ok(true) => {}
            Ok(false) => out.push(format!(
                "{PACKAGE}: the commit under review, {commit}, is not an ancestor of HEAD"
            )),
            Err(why) => out.push(format!(
                "{PACKAGE}: could not check that {commit} is an ancestor of HEAD: {why}"
            )),
        },
        many => out.push(format!(
            "{PACKAGE}: `## Commit under review` names {} commits; it must name one",
            many.len()
        )),
    }
    if !text.contains(NOT_REVIEWED) {
        out.push(format!(
            "{PACKAGE}: the words `{NOT_REVIEWED}` are missing (ADR-006)"
        ));
    }
    for line in claims(text) {
        out.push(format!(
            "{PACKAGE}:{line}\n    claims a review took place; nothing may until one has (ADR-006)"
        ));
    }
    for r in crate::refs::refs(text) {
        let Some(source) = read(&r.file) else {
            out.push(format!("{PACKAGE}: {} does not exist", r.file));
            continue;
        };
        for name in &r.names {
            if !crate::refs::has_word(&source, name) {
                out.push(format!("{PACKAGE}: {}::{name} is not in that file", r.file));
            }
        }
    }
    out
}

/// Whether `commit` is an ancestor of `HEAD` in the repository at `root`. Git's exit 1 is "no"; any
/// other failure, such as a commit a shallow clone does not hold, is an error.
fn is_ancestor(root: &Path, commit: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", commit, "HEAD"])
        .output()
        .map_err(|_| "git could not be run".to_string())?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err("git does not hold that commit here; a shallow clone needs its history".into()),
    }
}

pub fn check(root: &Path) -> Result<(), String> {
    let text = fs::read_to_string(root.join(PACKAGE)).map_err(|e| format!("{PACKAGE}: {e}"))?;
    let problems = findings(
        &text,
        |commit| is_ancestor(root, commit),
        |file| fs::read_to_string(root.join(file)).ok(),
    );
    if problems.is_empty() {
        println!(
            "review package ok: {} sections, commit under review is an ancestor of HEAD, {} code references",
            SECTIONS.len(),
            crate::refs::refs(&text).len()
        );
        Ok(())
    } else {
        Err(format!(
            "{} problem(s) in the review package:\n  {}",
            problems.len(),
            problems.join("\n  ")
        ))
    }
}

#[cfg(test)]
#[path = "package_tests.rs"]
mod tests;
