# Hyalos

Clean-room Rust rewrite of hivrr. Currently moving from planning into the first code. The Cargo workspace skeleton and CI exist but the crates are empty. Throwaway spike rigs under `spikes/` are not part of the workspace and not trusted core.

## Documentation rules (see docs/README.md)

- Intent and scope: `docs/prd/`. Never put status there.
- Decisions: `docs/adr/NNNN-title.md`, one per file, from `0000-template.md`. Add new rows to `docs/adr/README.md`.
- Never edit Context, Decision or Consequences of an **Accepted** ADR. Write a new ADR that supersedes it and change only the old one's Status line. **Proposed** ADRs may be edited.
- Status lives in GitHub issues (epics as parent issues, milestones M1 to M4), not in this repo. `docs/planning/` is working material only.

## Planned stack (ADR-0001)

Single Cargo workspace producing one `hyalos` binary (engine + broker). Trusted-core paths require human code-owner review.

## Code layout (ADR-0017)

- `core/` is the trusted core: code that runs in the engine or broker process. It has a 30,000-line budget.
- `crates/` holds everything else (`xtask` checks, later the simulator and test kits). It is not counted.
- Before pushing, run what CI runs: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `cargo xtask layout`, `cargo xtask lines`. The toolchain is pinned in `rust-toolchain.toml`.
- `hyalos-types`, `hyalos-log`, `hyalos-policy` and `hyalos-engine` are deterministic (ADR-0013). They get time from `hyalos_types::Clock` and randomness from `hyalos_types::Entropy`, never from the system. Their identical `clippy.toml` bans `SystemTime`, `Instant`, `HashMap`, `HashSet`, `RandomState` and `fs::read_dir`, and they may take no external dependency without an entry in `EXTERNAL_ALLOWED` in `crates/xtask`.
- Simulation lives in `crates/sim`: `Sim::new(seed)` gives a fake clock and one named random stream per boundary.
- The broker never depends on the engine, log or substrate crates, and the engine never depends on the broker. `cargo xtask layout` enforces this; a new `core/` crate means editing its table and the ADR.
