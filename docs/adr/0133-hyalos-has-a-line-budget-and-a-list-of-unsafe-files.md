# ADR-0133: hyalOS has a line budget and a list of files that may use `unsafe`

**Status:** Proposed

## Context

ADR-0017 gives the trusted core, `core/`, a budget of 30,000 lines, and `cargo xtask lines` fails above it. hyalOS is its own workspace ([ADR-0130](0130-hyalos-os-is-its-own-workspace.md)), so its code is not in that count, and nothing measured it. Its kernel image, monitor, machine runner and verifier are what an independent reviewer will read, so their size is the review's cost. hyalOS also uses `unsafe` for the KVM interface, the kernel image, the guest's imports and a few system calls, and its lint table denies `unsafe_code` everywhere else; but no check said which files may allow it, or that each use is explained.

The plan pack [`hyalos-kernel-core`](../plans/hyalos-kernel-core/README.md) chose a budget of 12,000 lines and a named list of files in its [ADR-007](../plans/hyalos-kernel-core/decisions/ADR-007-a-line-budget-for-hyalos-own-code.md). This is the repository's record of it.

## Decision

**hyalOS's own non-test code may not pass 12,000 lines, and `unsafe` appears only in listed files, each use with a `SAFETY` comment.**

- **Counting.** `cargo xtask lines` also counts the Rust under `hyalos-os/` with the trusted core's rules (blank and comment lines are not counted, nor anything after a line starting `#[cfg(test)]`), leaving out `tests/` directories, `*_tests.rs` files and build output (`target/`, or any directory Cargo tagged with `CACHEDIR.TAG`). It reports the count per crate against the budget and fails above it. Since the count stops at the first `#[cfg(test)]`, it also fails when anything but test modules follows that line, so no code can be hidden after a file's tests. The trusted core's count does not change, and its budget table in `crates/xtask` is untouched.
- **`unsafe`.** `cargo xtask hyalos-unsafe` reads every Rust file under `hyalos-os/`. The keyword `unsafe` as a whole word, whatever follows it, on a line that is not a comment, fails the check in a file not on `UNSAFE_FILES` in `crates/xtask/src/hyalos.rs`, or with no `// SAFETY` comment on its line or within the three lines above. A listed file that no longer exists fails it too. The test-only uses, behind the monitor's `test-switches` feature or the kernel image's `selftest`, are checked the same.
- **The list** holds eleven files: the guest, the kernel image, the monitor's `kvm.rs`, `live.rs` and `runner.rs`, the notary and the canary and its test, and three verifier tests that need `env::set_var` or `pre_exec`, which the standard library offers only as `unsafe`. A file joins the list in review, with a reason in the list's comment.
- **CI's `lint` job** runs both on every pull request.
- **Not counted:** vendored and third-party code. The review scope covers it instead.

## Consequences

- At this ADR, hyalOS is 9,021 lines of 12,000, so a capsule or a further hardening has about 3,000 lines of room before a new ADR must raise the budget.
- A reviewer reads the list to find every `unsafe` in hyalOS, and each use says why it holds.
- The keyword check is textual: it does not parse Rust, so `unsafe` inside a string literal or a block comment would be counted. It errs towards failing, never towards passing.
- Raising the budget, or a budget per crate, takes a new ADR.
