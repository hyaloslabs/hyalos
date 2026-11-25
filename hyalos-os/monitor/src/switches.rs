//! The test-only inputs from the environment (S-05.4 of `docs/plans/hyalos-kernel-core`). This
//! module is compiled only with the `test-switches` feature, and it is the only place in the
//! monitor that names them, so a release build reads none of them: its `attacks` command is
//! refused, its runs print no timings and its memory takes no advice.

/// `HYALOS_TRACE`: a KVM run prints its timings to standard error.
pub(crate) fn trace() -> bool {
    std::env::var_os("HYALOS_TRACE").is_some()
}

/// `HYALOS_HUGE`: the machine's memory is advised to use huge pages.
pub(crate) fn huge() -> bool {
    std::env::var_os("HYALOS_HUGE").is_some()
}

/// `HYALOS_SELFTEST_KERNEL`: the self-test kernel the `attacks` command probes with.
pub(crate) fn selftest_kernel() -> Option<String> {
    std::env::var("HYALOS_SELFTEST_KERNEL").ok()
}
