//! Proves the determinism lint (ADR-0013, ADR-0017) fires, using the real configuration.
//!
//! It builds a throwaway crate that uses every banned item, copies in `core/types/clippy.toml` (which
//! `cargo xtask layout` requires to match the other deterministic crates), and runs clippy over it.
//! Each banned item must be reported by name, and a crate that avoids them must pass, so a config
//! that silently stops working fails here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BANNED: &str = r#"
use std::collections::{HashMap, HashSet};
use std::hash::RandomState;
use std::time::{Instant, SystemTime};

pub fn clock() -> u128 {
    let wall = SystemTime::now();
    let mono = Instant::now();
    wall.elapsed().map(|d| d.as_millis()).unwrap_or(0) + mono.elapsed().as_millis()
}

pub fn unordered() -> usize {
    let mut map: HashMap<u8, u8> = HashMap::new();
    map.insert(1, 2);
    let set: HashSet<u8> = HashSet::new();
    map.len() + set.len()
}

pub fn random_hasher() -> RandomState {
    RandomState::new()
}

pub fn os_order() -> usize {
    std::fs::read_dir(".").map(|d| d.count()).unwrap_or(0)
}
"#;

const CLEAN: &str = r#"
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub fn ordered() -> usize {
    let mut map: BTreeMap<u8, u8> = BTreeMap::new();
    map.insert(1, 2);
    let set: BTreeSet<u8> = BTreeSet::new();
    map.len() + set.len() + Duration::from_secs(1).as_secs() as usize
}
"#;

/// What clippy must say for each banned item. Matching on the path keeps this independent of wording.
const EXPECTED: &[&str] = &[
    "std::time::SystemTime",
    "std::time::Instant",
    "std::collections::HashMap",
    "std::collections::HashSet",
    "std::hash::RandomState",
    "std::fs::read_dir",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .expect("xtask lives two levels below the repository root")
}

/// Writes a one-file crate next to the real config and returns clippy's exit status and output.
fn clippy(name: &str, source: &str) -> (bool, String) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).expect("create the throwaway crate");
    // The empty [workspace] table keeps cargo from treating this as a member of the real workspace.
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n"
    );
    fs::write(dir.join("Cargo.toml"), manifest).expect("write the manifest");
    fs::write(dir.join("src/lib.rs"), source).expect("write the source");
    fs::copy(
        repo_root().join("core/types/clippy.toml"),
        dir.join("clippy.toml"),
    )
    .expect("copy the real clippy.toml");

    let out = Command::new("cargo")
        .args(["clippy", "--offline", "--quiet", "--", "-D", "warnings"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .output()
        .expect("run cargo clippy");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

#[test]
fn every_banned_item_is_reported_by_name() {
    let (ok, text) = clippy("determinism_banned", BANNED);
    assert!(!ok, "clippy accepted code that uses banned items:\n{text}");
    for item in EXPECTED {
        assert!(
            text.contains(item),
            "clippy did not report `{item}`:\n{text}"
        );
    }
}

#[test]
fn code_that_avoids_them_passes() {
    let (ok, text) = clippy("determinism_clean", CLEAN);
    assert!(ok, "clippy rejected clean code:\n{text}");
}
