//! The execution profiles: the fixed interpreter settings and limits a run and its replay depend on.
//!
//! Plain values with no I/O and no `wasmi` dependency, so the process backend, the kernel image and
//! the verifier all build from one definition. A profile's contents never change. A change is a new
//! name, and the old profile stays, so an old record keeps its meaning.

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

/// `hyalos.profile/1`. Records of versions 1 to 3 ran under it without naming it.
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

/// The profile new runs use.
pub const CURRENT: &Profile = &PROFILE_1;

static KNOWN: [Profile; 1] = [PROFILE_1];

/// The profile of this exact name, or nothing.
pub fn lookup(name: &str) -> Option<&'static Profile> {
    KNOWN.iter().find(|profile| profile.name == name)
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
    fn s_02_4_a_name_gives_its_profile_or_nothing() {
        assert_eq!(lookup("hyalos.profile/1"), Some(&PROFILE_1));
        for unknown in [
            "hyalos.profile/9",
            "hyalos.profile/1 ",
            "HYALOS.PROFILE/1",
            "",
        ] {
            assert_eq!(lookup(unknown), None, "{unknown}");
        }
    }
}
