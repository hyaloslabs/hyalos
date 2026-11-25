//! S-05.3 and S-05.6: what a customer can check of a built copy.

#[path = "../src/commit.rs"]
mod commit;

use hyalos_audit::sha;
use std::{fs, process::Command};

fn version() -> Vec<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_hyalos-verify"))
        .arg("--version")
        .env_clear()
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn s_05_3_e1_a_binary_with_one_changed_byte_has_another_hash() {
    let bytes = fs::read(env!("CARGO_BIN_EXE_hyalos-verify")).unwrap();
    let mut altered = bytes.clone();
    let middle = altered.len() / 2;
    altered[middle] ^= 1;
    assert_ne!(sha(&bytes), sha(&altered));
}

#[test]
fn s_05_6_e1_a_release_build_names_its_commit_and_what_it_reads() {
    let full = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(commit::named(Some(full)), full);
    let lines = version();
    assert_eq!(
        lines[0],
        concat!("hyalos-verify ", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        &lines[2..],
        ["records 1-4", "profiles hyalos.profile/1 hyalos.profile/2"]
    );
}

#[test]
fn s_05_6_e2_a_build_outside_a_checkout_says_the_commit_is_unknown() {
    assert_eq!(commit::named(None), "unknown");
    if option_env!("HYALOS_BUILD_COMMIT").is_none() {
        assert_eq!(version()[1], "commit unknown");
    }
}

#[test]
fn local_6_a_commit_that_is_not_one_is_never_named() {
    for bad in [
        "",
        "main",
        "0123456789abcdef",
        "g123456789abcdef0123456789abcdef01234567",
        "0123456789abcdef0123456789abcdef012345678",
    ] {
        assert_eq!(commit::named(Some(bad)), "unknown", "{bad}");
    }
}
