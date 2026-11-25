# hyalOS: coverage of deferred work

**Status:** reviewed with the developer (2026-11-22). Rows still marked **Decide** remain open.

Every item a pack deferred, excluded or left open has one row. When a pack defers something, it updates this table in the same pull request, so nothing depends on a later pack remembering it.

| Outcome | Meaning |
|---|---|
| **Decided** | Settled by a named decision |
| **Owned** | A named pack must deliver or decide it |
| **Limit** | Accepted, and the audit report or the docs state it |
| **Parked** | Not planned. Names what would reopen it |
| **Decide** | Needs the developer's call |

## Proof and verification

| Item | Came from | Outcome | Owner or trigger |
|---|---|---|---|
| Packaged standalone verifier | Parent, record non-goal | Owned | `verifier`, #434 |
| Bitcoin timestamp check | Notary ADR-005, record non-goal | Parked | An auditor who needs a second time source |
| Task data private during verification | Parent, record non-goal | Parked | A buyer whose data requires it |
| Other forged request shapes | Canary non-goal | Owned | #441, planned apart from the verifier |
| Hidden API-key case | Notary ADR-012 | Limit | Stays in every report until the proof below exists |
| Zero-knowledge proof of the hidden bytes | Notary ADR-011, #409 | Owned | #442, which decides it |
| Model's identity | Parent | Limit | Reopens if a provider ships a checkable proof (the record's provider-proof slot) |
| Operator asking the provider off the notary | Record non-goal | Limit | Stated in every report. It can inform whether to start an attempt, but a started attempt spends a ticket (record ADR-004) |
| Colluding notary | Notary ADR-001 | Limit | Stated in every report |
| Proof that the customer is independent | Record non-goal | Limit | Stated in every report |
| A threshold of notaries | Notary non-goal | Parked | A customer who will not trust one notary |
| More than one operator per notary | Record non-goal | Parked | A customer with several operators |
| Scheduling the canary, and its 24-hour window | Canary non-goal | Limit | The customer runs it, for example from cron |

## Kernel and runtime

| Item | Came from | Outcome | Owner or trigger |
|---|---|---|---|
| Where the code lands | Parent | Decided | ADR-001 |
| Which interpreter | Parent | Decided | ADR-002 |
| Record does not name its interpreter | This pass | Decided | ADR-002, built by `core` and `verifier` |
| seL4 under the kernel | Parent | Parked | ADR-002. Reopens if the review finds KVM isolation insufficient |
| Clock in a taken-over kernel | Parent | Owned | `core`, ADR-001, #458 |
| Hardware randomness in a taken-over kernel | This pass, measured by `core` | Owned | `core`, ADR-002, #459, #460 |
| Monitor confinement and hardening | Parent | Owned | `core`, ADR-003, #461 to #465 |
| Review scope for `tlsn` and `wasmi` | #412 | Owned | `core`, ADR-005, ADR-008, #467 |
| Independent security review | #63 | Owned | `core` prepared the package (ADR-006, #468): `hyalos-os/review/README.md`. Engaging a reviewer and closing findings: #491. #63 still covers v1's trusted core |
| Leaving TLSNotary's beta pin | Parent, #412 | Owned | `core`, ADR-005, #467 |

## Tasks and product

| Item | Came from | Outcome | Owner or trigger |
|---|---|---|---|
| Inference limit and real attestation cost | #411 | Owned | `capsules` |
| Authorising a contract on new inputs | Record ADR-002 | Owned | `capsules` |
| Python as a guest runtime | Python review, 2026-11-22 | Owned | Python guest spike, then `capsules` |
| A runtime bug letting hostile input steer a task | This pass | Owned | Python guest spike tests it, and the report states it |
| A small SDK generated from the contract | Developer, 2026-11-22 | Owned | Python guest spike, then `capsules` |
| Monitor enforcing a per-task contract | This pass | Owned | Minimal check in the Python guest spike, then `capsules` |
| Floats and a higher memory limit | Python review | Owned | `capsules`, each as a new execution profile |
| Multi-line script as a task input | This pass | Owned | `capsules` |
| More than one model provider | Notary non-goal | Owned | `capsules` declares it, and each provider needs its own canary |
| A second, non-GitHub job | Parent | Owned | `connectors` |
| How v1 and hyalOS join | Parent, developer 2026-11-22 | Decided | ADR-003 |
| One credential store across v1 and hyalOS | This pass | Owned | ADR-003, built by `engine` |
| Linking the event log to a hyalOS record | This pass | Owned | ADR-003, built by `engine` |
| The v1 workflow alongside hyalOS | Parent | Owned | ADR-003, built by `engine`. Coding jobs stay on the microVM substrate |
| Public notary service | Parent | Parked | Measured costs exist (#411). Still needs a trust and liability answer |
| Whether anyone pays | Parent, #407 | Parked | Closed as not planned. Reopens when there is a buyer to ask |
