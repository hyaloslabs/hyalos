//! The execution profiles: the fixed interpreter settings and limits a run and its replay depend on.
//!
//! Plain values with no I/O and no `wasmi` dependency, so the process backend, the kernel image and
//! the verifier all build from one definition. A profile's contents never change. A change is a new
//! name, and the old profile stays, so an old record keeps its meaning. A record replays only on the
//! `wasmi` its profile names, or on one checked equal to it (ADR-0134).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

/// One named set of interpreter settings and limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: &'static str,
    /// The exact `wasmi` version, because replay compares the fuel it counts.
    pub wasmi: &'static str,
    pub consume_fuel: bool,
    pub floats: bool,
    pub start_function: bool,
    pub ignore_custom_sections: bool,
    pub memory64: bool,
    pub multi_memory: bool,
    pub tail_call: bool,
    pub wide_arithmetic: bool,
    pub custom_page_sizes: bool,
    pub fuel: u64,
    pub max_memory: usize,
    pub memories: usize,
    pub tables: usize,
    pub table_elements: usize,
    pub instances: usize,
    pub trap_on_grow_failure: bool,
}

/// `hyalos.profile/1`. Records of versions 1 to 3 ran under it without naming it, and version 4
/// records made before `hyalos.profile/2` name it. It is kept unchanged, and its runs replay on
/// `wasmi` 2.0.3 because the two were checked equal (ADR-0134).
pub const PROFILE_1: Profile = Profile {
    name: "hyalos.profile/1",
    wasmi: "2.0.2",
    consume_fuel: true,
    floats: false,
    start_function: false,
    ignore_custom_sections: true,
    memory64: false,
    multi_memory: false,
    tail_call: false,
    wide_arithmetic: false,
    custom_page_sizes: false,
    fuel: 200_000_000,
    max_memory: 4 * 1024 * 1024,
    memories: 1,
    tables: 1,
    table_elements: 4096,
    instances: 1,
    trap_on_grow_failure: true,
};

/// `hyalos.profile/2`: profile 1's settings and limits on `wasmi` 2.0.3, which fixes
/// GHSA-qrr7-2g7f-r9rg (ADR-0134).
pub const PROFILE_2: Profile = Profile {
    name: "hyalos.profile/2",
    wasmi: "2.0.3",
    ..PROFILE_1
};

/// The profile new runs use. Its `wasmi` is the one this build links, which a test holds to both
/// lockfiles.
pub const CURRENT: &Profile = &PROFILE_2;

/// `hyalos.spike/1`: profile 2's settings and limits under the Python guest spike's draft contract,
/// which the audit enforces from a run's input record 0 (ADR-0136). Only a build with the
/// `spike-contract` feature knows it.
#[cfg(feature = "spike-contract")]
pub const SPIKE_1: Profile = Profile {
    name: "hyalos.spike/1",
    ..PROFILE_2
};

/// `hyalos.spike/2`: `hyalos.spike/1` with the limits the Monty guest needs (Slice 4 of
/// docs/plans/hyalos-kernel-python, ADR-0137). Floats are allowed, and every spike build turns on
/// `wasmi`'s deterministic float results, so a NaN has one bit pattern on every host (ADR-007 of
/// that pack); profiles 1 and 2 still refuse every float instruction at load. `max_memory` is the
/// smallest whole number of MiB at which the triage fixture's seed-0 run ends with a result, and
/// `fuel` is twice that run's `fuel_used` (413,707,242) rounded up to the next ten million
/// (spikes/438/README.md, `monty profile:`). Only a build with the `spike-contract` feature knows it.
#[cfg(feature = "spike-contract")]
pub const SPIKE_2: Profile = Profile {
    name: "hyalos.spike/2",
    floats: true,
    fuel: 830_000_000,
    max_memory: 5 * 1024 * 1024,
    ..SPIKE_1
};

/// The profiles this build knows.
const KNOWN_COUNT: usize = 2 + 2 * cfg!(feature = "spike-contract") as usize;

static KNOWN: [Profile; KNOWN_COUNT] = [
    PROFILE_1,
    PROFILE_2,
    #[cfg(feature = "spike-contract")]
    SPIKE_1,
    #[cfg(feature = "spike-contract")]
    SPIKE_2,
];

/// Pairs of an older `wasmi` and the linked one whose runs were checked equal: the same fuel and
/// the same transcripts for every committed guest and record, and no change to the code a profile's
/// guest can reach (ADR-0134). A profile on any other older `wasmi` does not replay here.
const CHECKED_EQUAL: [(&str, &str); 1] = [("2.0.2", "2.0.3")];

impl Profile {
    /// Whether this build's `wasmi` replays a run under this profile: it is the profile's own
    /// version, or one checked equal to it.
    pub fn replays_here(&self) -> bool {
        self.wasmi == CURRENT.wasmi || CHECKED_EQUAL.contains(&(self.wasmi, CURRENT.wasmi))
    }

    /// Whether this profile has the same settings and limits as `other`, whatever its name and
    /// `wasmi`, so a backend built for `other` runs it the same way.
    pub fn same_settings(&self, other: &Profile) -> bool {
        Profile {
            name: other.name,
            wasmi: other.wasmi,
            ..*self
        } == *other
    }

    /// Whether a run under this profile is held to a draft contract read from its input record 0.
    /// Every `hyalos.spike/` profile is (ADR-0136).
    #[cfg(feature = "spike-contract")]
    pub fn enforces_contract(&self) -> bool {
        self.name.starts_with("hyalos.spike/")
    }
}

/// The profile of this exact name, or nothing. A profile this build cannot replay is nothing.
pub fn lookup(name: &str) -> Option<&'static Profile> {
    KNOWN
        .iter()
        .find(|profile| profile.name == name && profile.replays_here())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s_02_1_profile_1_holds_the_settings_both_backends_used() {
        assert_eq!(
            PROFILE_1,
            Profile {
                name: "hyalos.profile/1",
                wasmi: "2.0.2",
                consume_fuel: true,
                floats: false,
                start_function: false,
                ignore_custom_sections: true,
                memory64: false,
                multi_memory: false,
                tail_call: false,
                wide_arithmetic: false,
                custom_page_sizes: false,
                fuel: 200_000_000,
                max_memory: 4_194_304,
                memories: 1,
                tables: 1,
                table_elements: 4096,
                instances: 1,
                trap_on_grow_failure: true,
            }
        );
    }

    #[test]
    fn profile_2_is_profile_1_on_wasmi_2_0_3() {
        assert_eq!(
            PROFILE_2,
            Profile {
                name: "hyalos.profile/2",
                wasmi: "2.0.3",
                ..PROFILE_1
            }
        );
        assert!(PROFILE_2.same_settings(&PROFILE_1));
        assert_eq!(CURRENT, &PROFILE_2);
    }

    #[test]
    fn every_known_profile_replays_here_and_no_other_wasmi_does() {
        assert!(KNOWN.iter().all(Profile::replays_here));
        let other = Profile {
            name: "hyalos.profile/1",
            wasmi: "2.0.1",
            ..PROFILE_1
        };
        assert!(!other.replays_here());
        assert!(
            !Profile {
                fuel: 1,
                ..PROFILE_1
            }
            .same_settings(&PROFILE_2)
        );
    }

    #[test]
    fn s_02_4_a_name_gives_its_profile_or_nothing() {
        assert_eq!(lookup("hyalos.profile/1"), Some(&PROFILE_1));
        assert_eq!(lookup("hyalos.profile/2"), Some(&PROFILE_2));
        for unknown in [
            "hyalos.profile/9",
            "hyalos.profile/1 ",
            "HYALOS.PROFILE/1",
            "",
        ] {
            assert_eq!(lookup(unknown), None, "{unknown}");
        }
    }

    /// S-01.4.e1: a default build knows no spike profile.
    #[cfg(not(feature = "spike-contract"))]
    #[test]
    fn python_s_01_4_e1_a_default_build_knows_no_spike_profile() {
        assert_eq!(lookup("hyalos.spike/1"), None);
        assert!(KNOWN.iter().all(|p| !p.name.starts_with("hyalos.spike/")));
    }

    /// `hyalos.spike/1` is profile 2's settings and `wasmi` under another name, enforcing a contract.
    #[cfg(feature = "spike-contract")]
    #[test]
    fn python_spike_1_is_profile_2_with_a_contract() {
        assert_eq!(
            SPIKE_1,
            Profile {
                name: "hyalos.spike/1",
                ..PROFILE_2
            }
        );
        assert_eq!(lookup("hyalos.spike/1"), Some(&SPIKE_1));
        assert!(SPIKE_1.same_settings(&PROFILE_2) && SPIKE_1.replays_here());
        assert!(SPIKE_1.enforces_contract());
        assert!(!PROFILE_1.enforces_contract() && !PROFILE_2.enforces_contract());
        assert_eq!(lookup("hyalos.spike/3"), None);
    }

    /// `hyalos.spike/2` is `hyalos.spike/1` with floats and Monty's measured memory and fuel, and
    /// nothing else changed.
    #[cfg(feature = "spike-contract")]
    #[test]
    fn python_spike_2_is_spike_1_with_floats_and_monty_s_limits() {
        assert_eq!(
            SPIKE_2,
            Profile {
                name: "hyalos.spike/2",
                floats: true,
                fuel: 830_000_000,
                max_memory: 5_242_880,
                ..SPIKE_1
            }
        );
        assert_eq!(lookup("hyalos.spike/2"), Some(&SPIKE_2));
        assert!(SPIKE_2.replays_here() && SPIKE_2.enforces_contract());
        assert!(!SPIKE_2.same_settings(&PROFILE_2));
        const { assert!(!PROFILE_1.floats && !PROFILE_2.floats && !SPIKE_1.floats) };
    }
}
