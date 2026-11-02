# ADR-0001: Rust for the trusted core

**Status:** Accepted on November 1, 2026. The Go runtime check is no longer a precondition.

## Context

The core must be small, reviewable and enforce security properties by construction. The plugin runtime of choice, Wasmtime, is Rust and is the reference implementation of the Component Model and WASI 0.3. The microVM tooling (Firecracker, Cloud Hypervisor) and Cedar are Rust.

## Decision

Write the engine and broker in Rust as one Cargo workspace producing one binary. Agent workloads inside sandboxes may use any language.

## Consequences

Provenance can be a compile-time type, and Wasmtime embeds natively. Compiles are slower and fewer contributors know Rust.
