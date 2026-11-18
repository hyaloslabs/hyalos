//! The plugin host's property-based contract suite (S8.3, ADR-0013, ADR-0087, ADR-0109).
//!
//! The suite is in `tests/host.rs`: properties of the Wasmtime host that must hold for every grant
//! and every call a seeded generator draws.
//!
//! - A component instantiates exactly when the grant covers every capability interface it imports.
//!   Otherwise it is refused `plugin.undeclared_import` and nothing runs.
//! - A call reaches the broker exactly when its name is in the grant's field for that interface,
//!   compared exactly. Any other name is answered `capability.not_granted`, and the broker never
//!   hears of it.
//! - One run makes at most `CALLS` calls to the broker. Past that, each call is refused
//!   `plugin.too_many_calls` without asking it.
//!
//! A failing case names its seed and replays with `HYALOS_PROP_SEED=<seed>`.
