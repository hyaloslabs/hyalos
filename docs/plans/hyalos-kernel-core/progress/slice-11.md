# Slice 11: Third-party scope and advisories

| | |
|---|---|
| **Status** | blocked |
| **Issue** | #467 |
| **PR** | none (no PR is opened) |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-24 |

## Preflight

Against the branch's base, `812c403`.

| Citation | Holds? |
|---|---|
| `context.md` §2: `wasmi =2.0.2` in `hyalos-os/monitor/Cargo.toml` and `hyalos-os/kernel-image/Cargo.toml` (and `hyalos-os/audit/Cargo.toml`) | Holds. Locked at 2.0.2 in `hyalos-os/Cargo.lock` and `hyalos-os/kernel-image/Cargo.lock`. **But 2.0.2 is yanked, and GHSA-qrr7-2g7f-r9rg names it** (see Blocked on) |
| `context.md` §2: `kvm-ioctls =0.25.1`, `kvm-bindings =0.14.2` in `hyalos-os/monitor/Cargo.toml` | Holds |
| `context.md` §2: TLSNotary at tag `v0.1.0-beta.1`, `hyalos-os/attest/Cargo.toml:12` | Holds. Locked at commit `40a069565a2eaecbb5e691dbc634be20e8498daf` for `tlsn`, `tlsn-core` and `tlsn-attestation` |
| `context.md` §2: the upstream audit status of `wasmi` and `tlsn` (searched 2026-11-23) | Holds on a repeat search on 2026-11-24 |
| ADR-008: `hyalos-os/attest/Cargo.toml:23-24`, `rcgen` and `tokio-rustls` with `ring` | Holds |
| ADR-008: the certificate check at `hyalos-os/attest/src/lib.rs:476` | Differs: the line moved. The check is the `Pin` verifier (`impl ServerCertVerifier for Pin`, about line 690) in the same file. Found by symbol, not a contract change |
| `context.md` §3: dependency policy `deny.toml:50-54` and `hyalos-os/deny.toml` | Holds (`[sources]` at 50-54; `hyalos-os/deny.toml` has the three exceptions RUSTSEC-2026-0387, -2025-0141, -2025-0134) |
| Slice 11 pattern: the `hyalos-os` job at `.github/workflows/ci.yml:218` | Differs: the job now starts at line 221. Found by name |
| Slice 11 files: `crates/xtask/src/main.rs` | Holds. Slice 10 added `crates/xtask/src/hyalos.rs`, the pattern for a check of its own |
| `design.md` §2 "Review scope" and **Modifier: integration** | Hold as written |

## Acceptance

Not shown: the slice stopped before the check and the workflow were written.

| ID | State |
|---|---|
| S-06.1 | Partial: `hyalos-os/review/scope.toml` drafted with a row per part for every direct dependency of a hyalOS crate (derived from the three lockfiles' workspace members) and the crates ADR-005 and ADR-008 name. Not checked yet |
| S-06.1.e1 | Partial: the `wasmi` rows cite SRLabs (0.31.0) and Runtime Verification (0.36.0-0.38.0) and say neither covers 2.0.2. No check reads them yet |
| S-06.1.e2 | Not shown |
| S-06.2, S-06.2.e1, S-06.2.e2 | Not shown (`cargo xtask review-scope` not written) |
| S-06.3, S-06.3.e1, S-06.3.e2 | Not shown. `cargo deny --all-features check advisories` from `hyalos-os/` exits 0 today (RustSec has no entry yet), but the GitHub feed the slice adds would fail on GHSA-qrr7-2g7f-r9rg |
| S-06.4, S-06.4.e1, S-06.4.e2 | Partial: `hyalos-os/review/pin-policy.md` drafted from ADR-005. Dry runs not recorded |
| 11.1 | Partial: the draft follows ADR-005's "Leaving the beta pin" paragraph item by item |

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| `design.md` §2: `upstream_audit` is a citation or `none found` with the date searched | The draft gives the excluded utility crates (`anyhow`, `base64`, `futures`, `hex`, `libc`, `serde`, `serde_json`, `tokio`, `tokio-util`, `wat`, and `bincode`) `not searched (utility, 2026-11-24)` | Writing `none found` for a search that was not made would be false. The check, when written, should accept this value only on an `excluded` row, or every utility crate needs a real search | n (to confirm when the slice resumes) |
| ADR-005 names no position for `ureq` | The draft gives it a `use` row (covered) and an `internals` row (excluded) | It is the HTTPS client the audit and the monitor reach the public log with, over `rustls`: ADR-008's "our use of TLS" covers its configuration. The log's answers are checked by P-256 signature (`audit/src/chain.rs`), not by TLS | n |
| ADR-005: "Signature and hash crates" | Rows for `sha2` and `p256` (direct) and `k256` (through `tlsn-attestation`: the notary's and verifier's secp256k1) | `k256` is not a direct dependency, but the attestation's signature rests on it | n |

## Decisions

- **Direct dependencies come from the lockfiles,** as the dependencies of each package with no `source` (the workspace members), dev and build dependencies included. That gives 24 crates; none is in `guest/Cargo.lock`, and `kernel-image/Cargo.lock` adds only `wasmi`.
- **`wasmi` has two rows,** `use` and `internals`, both covered, since ADR-005's position names both "the configuration and host boundary we use" and "the changes since the last audited version". `wasmi_core` and `wasmi_ir` have `internals` rows so a move of the interpreter's parts is caught too.
- **TLSNotary is three crates,** each with a `use` (covered) and an `internals` (excluded) row and the tag and commit.
- **The existing advisory process stays:** a RustSec exception is a `hyalos-os/deny.toml` `ignore` entry with a reason, an assessment under `hyalos-os/advisories/`, and a test that fails when an assessed version changes (`attest/tests/advisories.rs`). The slice's GitHub-feed check should follow it, not invent another (see Blocked on).

## Discoveries

- **Yanked crates.** `cargo deny check advisories` warns (not fails) that `wasmi 2.0.2`, `wasmi_core 2.0.2`, `aes` and `der` are yanked. The `wasmi` yank matches GHSA-qrr7-2g7f-r9rg; `aes` and `der` were not looked into. The deny policy's `yanked` level is the default warning.
- **RustSec will likely list GHSA-qrr7-2g7f-r9rg.** When it does, `cargo deny` in CI's `hyalos-os` job will fail on every pull request touching `hyalos-os/` and on every push to `main`, independent of this slice.
- **`wasmi` features in use.** Every build has `memory64` and none has `simd`: the monitor and the audit take `wasmi`'s default features (`stable`, `std`, `wat`, `validate`, `memory64`, `auto-dispatch`), and the kernel image `stable`, `validate`, `memory64`, `auto-dispatch` and `libm` (`cargo tree -e features -i wasmi`). The advisory names builds with `simd` and without `memory64`.
- **The profile pins `wasmi`.** `hyalos-os/profile/src/lib.rs` holds `wasmi: "2.0.2"` because replay compares the fuel it counts, and `hyalos.profile/1` belongs to the verifier pack (its ADR-007), which this pack consumes and does not change.
- **The "presentation differential"** ADR-005 requires before a pin move has no test in the repository today; the draft pin policy says the first move adds it.
- `webpki-roots` (0.26.11 under `ureq`, 1.0.12 under TLSNotary) and `rustls-webpki` 0.103.17 hold the trust anchors and the chain check; the draft leaves them to the TLSNotary and `rustls` `use` rows. The resumed slice may give them rows of their own.

## Blocked on

**Stop condition triggered** (Slice 11, Stop conditions): "Stop if the check finds a live advisory against a pinned crate. Report it rather than ignoring it."

**The advisory.** [GHSA-qrr7-2g7f-r9rg](https://github.com/wasmi-labs/wasmi/security/advisories/GHSA-qrr7-2g7f-r9rg), published by `wasmi-labs/wasmi` on 2026-11-24 (high): "store{8,16}_lane decodes incorrectly without the memory64 feature". Affected: `wasmi` and `wasmi_ir` `>= 2.0.0-beta.4, <= 2.0.2`; patched in `2.0.3`. hyalOS pins `wasmi =2.0.2` in the monitor, the audit and the kernel image, and crates.io has yanked 2.0.2. The design's advisory matching (`design.md`, **Modifier: integration**) fails the job when the affected range includes the pinned version, so the slice's daily GitHub-feed check would fail on its first run. RustSec does not list it yet, so `cargo deny` passes today.

**Assessment so far, for the developer's decision.** The advisory names builds with `simd` and without `memory64`. No hyalOS build enables `simd`, and every build enables `memory64` (Discoveries). This reading suggests hyalOS is not affected, but it is not an assessment of the kind `advisories/RUSTSEC-2026-0387.md` records.

**The question.** Which of these does the developer choose?

1. **Move `wasmi` to `2.0.3`** in its own pull request before Slice 11 resumes. That changes `hyalos.profile/1`'s `wasmi` value, which the verifier pack owns (its ADR-007) and this pack does not change, and replay compares fuel by that version, so it needs a decision on the profile first.
2. **Record an assessed exception** as `advisories/GHSA-qrr7-2g7f-r9rg.md`, with a test like `attest/tests/advisories.rs` that holds the features it assessed. The design gives the GitHub-feed check no exception list, so this also needs the design to say how a GitHub advisory is excepted (for example, a list in `hyalos-os/deny.toml`'s style that the daily job reads, each entry naming its assessment).
3. **Both:** the exception now, and the move with the profile change later.
