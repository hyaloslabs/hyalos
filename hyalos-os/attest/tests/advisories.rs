//! Holds `advisories/RUSTSEC-2026-0387.md` to what it assessed. A change to any version below means
//! the notary's compiled signing must be read again before the `deny.toml` exception still stands.

use std::{collections::BTreeSet, fs, path::Path};

fn workspace(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path)).unwrap()
}

/// Each locked package as (name, version, its dependencies).
fn packages() -> Vec<(String, String, Vec<String>)> {
    let lock = workspace("Cargo.lock");
    let mut out = Vec::new();
    for block in lock.split("[[package]]").skip(1) {
        let field = |key: &str| {
            block
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key} = \"")))
                .map(|rest| rest.trim_end_matches('"').to_string())
                .unwrap()
        };
        let deps = block
            .split_once("dependencies = [")
            .map(|(_, rest)| {
                let list = rest.split(']').next().unwrap();
                list.lines()
                    .map(|line| line.trim().trim_end_matches(',').trim_matches('"'))
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        out.push((field("name"), field("version"), deps));
    }
    out
}

#[test]
fn rustsec_2026_0387_the_assessed_versions_are_the_locked_ones() {
    let packages = packages();
    let version = |name: &str| -> BTreeSet<String> {
        packages
            .iter()
            .filter(|(n, _, _)| n == name)
            .map(|(_, v, _)| v.clone())
            .collect()
    };
    for (name, assessed) in [
        ("elliptic-curve", "0.13.8"),
        ("k256", "0.13.4"),
        ("p256", "0.13.2"),
        ("primeorder", "0.13.6"),
        ("ecdsa", "0.16.9"),
    ] {
        assert_eq!(
            version(name),
            BTreeSet::from([assessed.to_string()]),
            "{name}"
        );
    }
    let bigint = version("crypto-bigint");
    assert!(bigint.contains("0.5.5"), "{bigint:?}");
    for other in bigint.iter().filter(|v| *v != "0.5.5") {
        let parts: Vec<u64> = other.split('.').map(|p| p.parse().unwrap()).collect();
        assert!(
            parts >= vec![0, 7, 6],
            "crypto-bigint {other} is below the patched 0.7.6"
        );
    }
    let holders: BTreeSet<String> = packages
        .iter()
        .filter(|(_, _, deps)| deps.iter().any(|d| d == "crypto-bigint 0.5.5"))
        .map(|(n, v, _)| format!("{n} {v}"))
        .collect();
    assert_eq!(
        holders,
        BTreeSet::from(["elliptic-curve 0.13.8".to_string()])
    );
}

#[test]
fn rustsec_2026_0387_the_assessed_toolchain_is_the_pinned_one() {
    let toolchain = workspace("../rust-toolchain.toml");
    assert!(toolchain.contains("channel = \"1.99.1\""), "{toolchain}");
}

#[test]
fn rustsec_2026_0387_the_exception_names_its_assessment() {
    let deny = workspace("deny.toml");
    let line = deny
        .lines()
        .find(|line| line.contains("RUSTSEC-2026-0387"))
        .unwrap();
    assert!(line.contains("advisories/RUSTSEC-2026-0387.md"), "{line}");
    let assessment = workspace("advisories/RUSTSEC-2026-0387.md");
    assert!(
        assessment.contains("**Assessed:** 2026-11-24, at commit `c3480ec`, with rustc 1.99.1")
    );
}
