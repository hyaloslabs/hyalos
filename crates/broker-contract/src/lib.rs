//! The broker's property-based contract suite (S8.3, ADR-0013, ADR-0109).
//!
//! The suite is in `tests/`: properties of the broker's enforcement that must hold for every
//! manifest, credential and request a seeded generator draws.
//!
//! - **Admission** (`tests/admission.rs`): a manifest is admitted exactly when it is the engine's,
//!   for the credential's job and role, for a live job, and in date. Otherwise the refusal names the
//!   first check that failed, and nothing from the document.
//! - **Egress** (`tests/egress.rs`): through a running broker and a job's socket, a proxy request
//!   reaches its host exactly when the manifest lists the host, on its one port, and the host is not
//!   a provider's. Every other request is refused with 403 and reported once to the engine as the
//!   egress gate's `deny`, with the code that says why. A refused request connects to nothing.
//!
//! A failing case names its seed and replays with `HYALOS_PROP_SEED=<seed>`.
