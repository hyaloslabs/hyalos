# hyalOS record format

This defines what a run leaves behind and how a verifier reads it. It covers record versions 1 and 2. New runs write version 2. The code that enforces it is `kernel/src/main.rs`, `kernel/src/ledger.rs` and `attest/src/`.

## What a run leaves

| Path | Holds |
|---|---|
| `<record>` | The transcript, as JSON |
| `<record>.salt` | The salt that hides the transcript's commitment |
| `<record>.attest/NNN.presentation` | The notary's presentation of answer `NNN`, numbered from 0 |
| The public log | The operator's signed chain of links |

## Record versions

A reader refuses any `version` other than 1 or 2, and names it. A record cannot change version: its links carry the same version, and a version 1 record relabelled as 2 has no valid `task`.

| Field | Version 1 | Version 2 |
|---|---|---|
| `version` | `1` | `2` |
| `task` | Absent | Required, and equal to the identity below |
| `guest_sha256` | The guest's SHA-256 | The same |
| `inputs_hex` | Each input, in hex | The same |
| `answers` | One entry per model call | The same, and each may hold `provider_proofs` |
| `effects`, `result_hex`, `fuel_used` | What the guest did | The same |

Unknown fields are refused at every level.

An answer holds `schema`, `question_sha256`, `label`, `model`, `response_id` and `attestation`, the SHA-256 of its presentation.

### Task identity

- **Version 1:** the SHA-256 of `<guest hash>:<SHA-256 of the inputs joined with no framing>`. Two different input lists can share it, so it is used only to read old records.
- **Version 2:** the SHA-256, in 64 lowercase hex characters, over the tag `hyalos.task/2`, the guest hash as 32 raw bytes, the input count, then each input's length and bytes. Counts and lengths are 8 big-endian bytes. `task-kernel task <guest> <inputs>` prints it.

## The chain

Each link is one signed JSON entry in the public log. A link's hash is the SHA-256 of its bytes.

| Link | Fields |
|---|---|
| Intent | `v`, `kind: "intent"`, `task`, `guest_sha256`. Version 2 adds `operator`, the SHA-256 of the operator public key's DER bytes in lowercase hex |
| Result | `v`, `kind: "result"`, `intent` (the intent link's hash), `outcome` (`ok` or `refused:<reason>`), `transcript` |

`transcript` is the SHA-256 of the salt followed by the transcript's compact JSON, and is empty for a refused run. A result's `v` equals its intent's, and both equal the record's version. Because a version 2 intent names the operator key, the same run under another key has another hash.

## The customer's authorisation

`notary authorize --dir <d> --task <hash> --attempts <n> [--again]` is the customer's decision: this exact task may have 1 to 100 attempts. It prints a random 64-character identifier and writes `<d>/authorisations/<identifier>`, mode 600:

```text
hyalos.authorisation/1
task <hash>
attempts <n>
round <r>
prior <p>
end
```

`round` counts the task's authorisations from 0. `prior` is the sum of the budgets authorised before this one. Only the newest round takes new runs. A run already on an older round may finish.

## The notary's state

`<d>/sequences` holds each run's session count and ticket, and is replaced whole in one durable write:

```text
hyalos.sequences/2
<run> <count> <authorisation> <ticket>
end <number of runs>
```

A run from before tickets is written with `- -` and is refused any further session. A `hyalos.sequences/1` file is rewritten as version 2 before serving, and the notary refuses to start on a version 1 file once any authorisation exists.

**Restoring a notary directory is the customer's responsibility.** Restoring a whole directory from a backup returns the tickets spent since that backup, and the notary cannot detect it (ADR-009).

## The link

The operator's first message is `<release> <run> <authorisation>`. The notary answers one word:

| Word | Meaning |
|---|---|
| `ok` | The session is served. A new run's ticket and the session's number are already saved |
| `release` | Another release. It is compared first, so an older tool hears this |
| `sequence` | The notary could not save, so nothing was spent |
| `authorisation` | The identifier is unknown, unreadable, not this run's, or not the task's newest round |
| `spent` | No ticket is left for a new run |

## The notary's signed extensions

Every attestation carries two, under the notary's one signature. A requester's own extension is refused.

| Extension | Bytes |
|---|---|
| `hyalos.run-sequence/1` | The run as 64 ASCII hex characters, then the session's sequence number as 8 big-endian bytes |
| `hyalos.ticket/1` | 144 bytes: the authorisation and the task as 64 ASCII hex characters each, then the budget, round, prior total and ticket as 4 big-endian bytes each |

## The audit

**Version 2.** The audit needs `--guest` and `--operator-public`. It refuses unless all of these hold:

- The record, its links and the chain agree on the version, and the intent names the audited key.
- Each answer's attestation verifies under the trusted notary, for this run and position, inside the run's window, with a sequence number equal to its position.
- Every answer carries one well-formed ticket, all the same, with a budget of 1 to 100 and a ticket below the budget.
- The ticket's task is the task recomputed from the record.
- The guest replays to the same record on the in-process backend.

A pass opens with `audit passed` and states `attempt {ticket + 1} of at most {budget} under this authorisation, {prior} authorised earlier, for this exact task on this notary`.

**Version 1.** Audited only with `--legacy-v1`. Any attestation carrying a ticket is refused. A pass opens with `legacy audit passed: no attempt budget`.

### What a pass does not prove

- The notary's independence is the customer's own.
- The model's name is the provider's claim.
- The answer's correctness is not judged.
- The log is trusted to have shown every entry under the operator's key.
- The request's hidden bytes are not shown, so the call may have used another credential or carried an added header.
- The operator may have asked the provider directly, away from the notary.
- The customer did not authorise this task again after this attestation.

Run counts are stated `under this log key only`.

## The provider-proof slot

A version 2 answer may hold `provider_proofs`, a list of entries:

| Field | Rule |
|---|---|
| `kind` | 1 to 64 bytes of lowercase letters, digits, `.`, `-` or `/` |
| `sha256` | 64 lowercase hex characters, over the proof's bytes as the provider supplied them |

Any other field, a malformed entry, or any entry on a version 1 record is refused. The list is left out of the bytes when empty, and replay copies it unchanged.

**No kind is known, so no proof is checked and none counts as evidence.** An entry cannot change a verdict or remove a limit. The report gives only their number, as `unchecked provider proofs: {n}, which prove nothing`, and never prints a kind.

## The assumption that no provider signs its responses

As of 2026-11-22, no model provider signs its responses in a way this format can check, which is why the notary exists. On 2026-11-02 Anthropic released FlashProve, a provable-inference research prototype. It is not an API feature, so the assumption stands.

**What would reopen it:** a provider offering a verifiable proof through its API. That proof would get a known `kind` and a checker, in a new decision, and only then could it count as evidence.
