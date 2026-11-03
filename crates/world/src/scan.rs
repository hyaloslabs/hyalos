//! Looking for planted strings in the files an engine wrote (S1.1, ADR-0003).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Fixtures;

/// A planted string found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
    pub file: PathBuf,
    pub fixture: &'static str,
}

/// Searches every file in `dir` (the database, its write-ahead log and anything else beside them) for
/// each planted string, as raw bytes. The search is on bytes, not events, so a string stored anywhere
/// in the file counts, in a payload, a column or a page that was freed.
pub fn scan(dir: &Path, fixtures: &Fixtures) -> io::Result<Vec<Leak>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    let mut leaks = Vec::new();
    for file in files {
        let bytes = fs::read(&file)?;
        for (name, needle) in fixtures.all() {
            if contains(&bytes, needle.as_bytes()) {
                leaks.push(Leak {
                    file: file.clone(),
                    fixture: name,
                });
            }
        }
    }
    Ok(leaks)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_finds_a_string_anywhere_and_only_whole_matches() {
        assert!(contains(b"abc secret xyz", b"secret"));
        assert!(contains(b"secret", b"secret"));
        assert!(contains(b"xsecret", b"secret"));
        assert!(!contains(b"secre", b"secret"));
        assert!(!contains(b"", b"secret"));
        assert!(!contains(b"anything", b""));
    }
}
