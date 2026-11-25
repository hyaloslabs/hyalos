//! In a build without `test-trust`, no stand-in provider setting changes whom the check trusts.

#![cfg(not(feature = "test-trust"))]
// One of the files allowed `unsafe` (`UNSAFE_FILES` in crates/xtask/src/hyalos.rs, ADR-0133).
#![allow(unsafe_code)]

use attest_check::Trust;
use hyalos_audit::Verifier;
use std::env;

const SETTINGS: [&str; 3] = [
    "HYALOS_TRUST_ROOT",
    "HYALOS_TRUST_SERVER_NAME",
    "HYALOS_TRUST_PROVIDER_ADDRESS",
];

#[test]
fn s_03_7_e2_provider_settings_change_nothing() {
    for name in SETTINGS {
        // SAFETY: this is the only test in this binary, so no other thread reads the environment.
        unsafe { env::set_var(name, "a-stand-in") };
    }
    assert_eq!(Verifier::from_env().unwrap().trust, Trust::real());
}
