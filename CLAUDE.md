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
- The event log is `hyalos-log` (ADR-0018): SQLite, hash-chained, Ed25519-signed. Check one with `hyalos log verify`: it reads `events.db` from the data directory and the public key from `engine.pub` in the config directory (`$HYALOS_DATA_DIR` / `$HYALOS_CONFIG_DIR`, else XDG, else `~/.local/share/hyalos` and `~/.config/hyalos`); `--db`, `--public-key`, `--public-key-file` and `--expect-head <seq>:<hash>` override or add to that. The public key must never come from the log itself. Only the engine depends on `hyalos-log`; the broker must not.
- Events are the closed `hyalos_types::Event` set (ADR-0019), stored as tagged JSON. Tasks, jobs and holds are an in-memory `State` that `hyalos-engine` rebuilds by replaying the log; `Engine::submit` checks an event against the state before it appends, so the log never holds an event replay would refuse. An id is the sequence number of the event that created it.
- Schema versions (ADR-0023): the log is never rewritten, so an event written at an older version is upcast in memory when read (`Event::decode_with` and a `hyalos_types::Schema` of pure per-kind steps), and new events are written at `Schema::current`. A newer version than the build knows is refused, never skipped. Bumping `SCHEMA_VERSION` means adding the new version's pinned payloads (`core/types/tests/schema.rs`) and the steps; `Engine::open_with` runs another schema against a past log.
- Gates (ADR-0020): `hyalos_policy::ask` returns an `Outcome`, never a `Result`, and only an on-time allow proceeds; `Engine::rule_gate` writes an unknown answer, the job ending held and the hold as one transaction, and replay refuses a log where unknown did not hold. Reasons are `ReasonCode`s (short codes), never free text.
- Backup and restore (ADR-0021, Proposed): `hyalos log backup <new-file>` and `hyalos log restore <backup>` copy the log with SQLite (safe while the engine runs, never a plain file copy), verify the copy against the operator's public key, and only then create the destination, which must not exist. A restore never overwrites a log.
- The engine's keys (ADR-0022): `hyalos init` creates `engine.key` (mode 0600) and `engine.pub` from OS entropy, only when there is neither a key nor a log; a log with no key is a lost key and is refused, never replaced. The deterministic crates never see entropy. Operator procedures are in `docs/operations/`.
- Simulation lives in `crates/sim`: `Sim::new(seed)` gives a fake clock and one named random stream per boundary.
- The broker never depends on the engine, log or substrate crates, and the engine never depends on the broker. `cargo xtask layout` enforces this; a new `core/` crate means editing its table and the ADR.
