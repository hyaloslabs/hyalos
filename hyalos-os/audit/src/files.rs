//! How the audit reads the evidence, so a caller can bound each read before it happens.

use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Transcript,
    Presentation,
    Link,
    Guest,
    Key,
    Salt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError {
    Missing,
    TooLarge,
    Unreadable,
}

pub trait Files: Sync {
    fn read(&self, path: &Path, kind: FileKind) -> Result<Vec<u8>, FileError>;
}

/// The monitor's reads: no limit, and any file it cannot read counts as absent, as it always has.
pub struct PlainFiles;

impl Files for PlainFiles {
    fn read(&self, path: &Path, _: FileKind) -> Result<Vec<u8>, FileError> {
        fs::read(path).map_err(|_| FileError::Missing)
    }
}
