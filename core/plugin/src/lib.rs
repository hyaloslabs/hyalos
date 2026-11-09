//! The plugin runtime (ADR-0006, ADR-0007, ADR-0086): hosting a plugin's WASM component in Wasmtime
//! with its capabilities as host imports (S6.1), installing skills and plugins (S6.2, S6.7),
//! verifying a release's Sigstore bundle (S7.12) and checking for updates (S7.10). It runs in the
//! engine process and holds no engine secret.
//!
//! Layout rule (ADR-0086): this crate depends only on `hyalos-types` among the workspace crates, and
//! only the `hyalos` binary depends on it. Its source has a part budget of 3,000 lines in
//! `cargo xtask lines`, set before any of it is written.
