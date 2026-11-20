# Reviewing the trusted core

Hyalos has not had an independent security review. This page is for anyone willing to do one, in whole or in part. A finding goes through [`SECURITY.md`](../../SECURITY.md), never a public issue.

## What to review

The trusted core is `core/`: everything that runs in the engine or broker process, the CLI included. It is kept under 30,000 lines so that one person can read it end to end ([ADR-0017](../adr/0017-workspace-layout-and-trusted-core-boundary.md)). `cargo xtask lines` prints the count for each crate. Comments, blank lines and test modules are not counted. Everything in `crates/` is checks, simulators and test kits, and none of it is in the binary.

## Where to start

Start where the untrusted input arrives, and follow it inward:

1. **`core/broker`**, the only process the sandbox can reach. The model relay (`src/model/`), egress (`src/egress.rs`), the git proxy (`src/git.rs`), the forge calls (`src/forge.rs`, `src/pulls.rs`), the content gates (`src/content.rs`) and how a sandbox connection is admitted (`src/sandbox.rs`). The broker confines itself with Landlock before it reads anything (`core/hyalos/src/confine.rs`).
2. **`core/types`**, the shapes everything is checked against. The manifest a job runs under (`src/manifest.rs`), the tainted text types with no way out (`src/provenance.rs`), the closed event set and its versions (`src/event.rs`, `src/schema.rs`), and the engine–broker messages (`src/wire.rs`).
3. **`core/engine`**, which decides. `src/state.rs` holds the rules every event must pass (`check_rules`) and the invariants every state must keep. Only the engine's own methods write a decision, and a human's decision carries the operator's signature.
4. **`core/log`**, the signed, hash-chained log, and how it verifies across a compaction.
5. **`core/substrate`**, the container, gVisor and Firecracker drivers.
6. **`core/policy`** (Cedar) and **`core/plugin`** (the Wasmtime host and Sigstore verification).

## What would matter most

The list in [`SECURITY.md`](../../SECURITY.md) of what is especially in scope is also the list of what the design claims. Each claim names its code and its test in the [threat model](threat-model.md). The most useful findings are:

- a secret reaching the sandbox, a log line or an error message;
- a way out of the sandbox other than through the broker;
- forge or model text reaching an actor's prompt or the engine as an instruction;
- a gate that allows on an error, a timeout or something it cannot read (every gate is meant to fail closed);
- a decision written without the operator's signature;
- a change to the log that `hyalos log verify` does not catch.

A way to make one of the threat model's [known gaps](threat-model.md#known-gaps) worse is in scope too.

## Running the tests

`cargo test --workspace --locked` runs everything that needs no outside tool. Tests that need one skip without it unless its variable is set, and a skip is not a pass. Set `HYALOS_REQUIRE_CONTAINER`, `HYALOS_REQUIRE_GIT`, `HYALOS_REQUIRE_GVISOR`, `HYALOS_REQUIRE_LANDLOCK`, `HYALOS_REQUIRE_MICROVM`, `HYALOS_REQUIRE_SANDBOX_IMAGE`, `HYALOS_REQUIRE_CVC5` or `HYALOS_REQUIRE_MCP_CONFORMANCE` to make the matching tests fail instead of skip. `.github/workflows/ci.yml` shows how CI installs each tool.

## What has been checked so far

None of this is independent, and none of it replaces a reviewer:

- the [threat model](threat-model.md) and [data flows](data-flows.md), whose references to code and tests CI checks on every change (`cargo xtask docs`);
- a red-team corpus of prompt-injection attacks, whose reach is recorded for each release in `crates/redteam/reach.json` and may not grow;
- a TLA+ model of the job state machine (`spec/Job.tla`), checked with its mutants on every change;
- thousands of simulated jobs under seeded fault schedules on every change (`crates/sim`, `crates/world`);
- property-based contract suites for the broker, the substrates and the plugin host;
- symbolic proofs of the shipped Cedar baseline (`crates/policy-proof`);
- a read of `core/broker` and `core/types` by the maintainer, and line-by-line reviews of each change by AI agents. They share the maintainer's blind spots, which is why this page exists.
