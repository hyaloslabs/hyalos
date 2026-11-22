# PRD — hyalOS: a versioned record with a customer-set attempt budget

## Problem

The notary pack proved each answer belongs to its run and position. It left four gaps:

1. **The operator chooses how many attempts exist.** They can run a task several times and hand over the record they prefer. The audit's count of runs is per task and per operator key, so a varied input or a second key avoids it (#408).
2. **The task identity is ambiguous.** Inputs are joined with no framing, so `["ab", "c"]` and `["a", "bc"]` share an identity (`task_of`, `spikes/task-kernel/kernel/src/main.rs:761`).
3. **The record has no enforced version.** A transcript and its links carry a version, and the audit reads neither (`spikes/task-kernel/kernel/src/main.rs:714`, `spikes/task-kernel/kernel/src/ledger.rs:361`).
4. **A provider-side proof has nowhere to go.** The "providers do not sign" assumption is undated, and the model's identity stays the provider's claim (#410).

## Users and jobs

- **Customer:** runs the notary and receives the work.
- **Operator:** runs the task. Honest operators need a clear refusal. A dishonest one is the attacker.
- **Auditor:** any third party checking a record, trusting neither the operator nor us.

**Jobs.**

- Customer: "When I commission a task, I want to set how many attempts it gets, so I can rely on the record not being the best of many tries."
- Operator: "When a run is refused for its budget, I want to know why, so I can ask the customer for another authorisation."
- Auditor: "When I check a record, I want to see which attempt it was and the ceiling on attempts, so I can judge what the answer is worth."

## Goals

- G1. The customer, not the operator, sets the ceiling on attested attempts for a task.
- G2. An auditor can read that ceiling and the attempt number from what the notary signed, with no extra key and no call to the notary.
- G3. A record has one enforced format version, and a verifier refuses what it cannot read.
- G4. Two different input lists never share a task identity.
- G5. A future provider-side proof has a typed place in the record, and an unknown one never counts as proof.

## Non-goals

- A packaged standalone verifier, Bitcoin timestamp checking, or keeping task data private during verification. These stay with a later record pack.
- The provider-behaviour canary (#409).
- Producing or verifying a real provider proof.
- Stopping an operator asking the provider directly, away from the notary. They hold the credential. The report names this as a limit.
- Proving the customer is independent of the operator. That limit stands from the notary pack.
- More than one enrolled operator per notary.

## Success measures

- M1. With a budget of N, the N+1th new run is refused by the notary, whichever task label or operator log key it uses.
- M2. Every forgery in the design's threat model has a named test that the audit refuses.
- M3. `out/live3.json` still replays identically, and a version 1 record is audited only when the auditor asks for it by name.
- M4. A record carrying a provider proof of an unknown kind reports that proof as unchecked and claims nothing from it.

## Non-functional requirements

| Quality | Target |
|---|---|
| Performance | Adds no network round trip to a session. At session open, one read of the task's authorisations and the existing durable save. A version 2 audit adds one in-process replay |
| Reliability | A crash at any point never returns a spent ticket, and never issues one ticket to two runs |
| Security | Fail closed. Every unreadable, unknown or mismatched value is a refusal. Threat model in `design.md` |
| Privacy | The notary learns only hashes: the task, the run and the authorisation. Nothing new goes to the public log |
| Accessibility | none: command-line tools with plain-text output |
| Observability | The notary's existing session line records a budget refusal by its reason word. No secret or task data is logged |
| Compatibility / portability | Version 1 records stay readable through an explicit option. Old notary state is migrated once before serving, and its runs can take no more sessions |
| Maintainability | The format is documented in `spikes/task-kernel/RECORD.md`, and each format change bumps its version |

## Assumptions

| Assumption | How to validate | If false |
|---|---|---|
| The pinned TLSNotary lets an attestation carry two extensions | Read at the pinned commit: its builder appends to a list. Prove it with a real MPC-TLS session against the stand-in provider in the ticket slice | Stop. Merge both values into one new extension by a new ADR |
| The customer's notary machine is out of the operator's reach | Stated limit from the notary pack | Nothing in this pack holds |
| An auditor can obtain the guest's bytes | The customer or operator supplies them with the record, and their hash is checked | A version 2 record cannot be audited |
| No provider signs its responses, as of 2026-11-22. FlashProve (released 2026-11-02) is a research prototype and no API feature | Recheck when the format next changes | Define a known proof kind in a new record version |

## Story map

| Define the record | Authorise | Run | Audit |
|---|---|---|---|
| S-01, S-02 | S-03 | S-04 | S-05, S-06 |

## Stories and acceptance criteria

### S-01 The record has one enforced version

As an auditor, I want a record's version enforced, so that a tool never reads a format it does not understand.

- **S-01.1** Given a record of a version the tool does not know, when it is audited or replayed, then it is refused and its version is named.
- **S-01.2** Given a version 1 record, when it is audited without the option `--legacy-v1`, then it is refused and the refusal names the option.
- **S-01.3** Given a version 1 record and `--legacy-v1`, when it is audited, then the old checks apply and the report opens with `legacy audit passed: no attempt budget`.
- **S-01.4** Given a version 2 record, when any link of its run does not carry version 2, then the audit refuses it.
- **S-01.5** Given a version 1 record, when it is replayed, then it replays byte-identically.
- **S-01.6** Given a version 1 record any of whose attestations carries a ticket extension, when it is audited with `--legacy-v1`, then it is refused.
- **S-01.7** Given a passing legacy audit, when it reports, then its first line is never `audit passed`.
- **S-01.8** Given a chain with a link whose `v` is not 1 or 2, or a result whose `v` differs from its intent's, when any record in it is audited, then the audit refuses it.
- **S-01.9** Given a version 2 transcript, when its required `task` is missing or is not the version 2 identity recomputed from it, then reading, replay or the audit refuses it.

### S-02 A task's identity is unambiguous

As a customer, I want each distinct task to have its own identity, so that an authorisation names exactly one task.

- **S-02.1** Given two different input lists, when their version 2 task identities are computed, then they differ, including when their joined bytes are equal.
- **S-02.2** Given a version 1 and a version 2 identity of the same guest and inputs, when compared, then they differ.
- **S-02.3** Given a guest and an inputs file, when the customer runs the `task` command, then it prints the version 2 identity the run will use, reading no key and using no network.

### S-03 The customer authorises a task

As a customer, I want to authorise a task with a number of attempts, so that I set the ceiling.

- **S-03.1** Given a task identity and 1 to 100 attempts, when the customer runs `notary authorize`, then a new authorisation is durably kept and its identifier is printed.
- **S-03.2** Given a malformed task identity, or attempts outside 1 to 100, when `notary authorize` runs, then it is refused and nothing is kept.
- **S-03.3** Given a task already authorised on this notary, when `notary authorize` runs without `--again`, then it is refused.
- **S-03.4** Given a task already authorised and `--again`, when `notary authorize` runs, then the new authorisation's round is one more than the highest before it.
- **S-03.5** Given a notary that is serving, when `notary authorize` runs, then it succeeds, and the next session can use the new authorisation without a restart.
- **S-03.6** Given earlier authorisations of a task, when a new one is made, then it records the sum of their budgets as its prior total.
- **S-03.7** Given a notary directory, when the customer runs `notary tickets`, then it prints each authorisation's task, round, budget, prior total and spent tickets, reading no key, including while the notary is serving.

### S-04 The notary spends one ticket per run

As a customer, I want each new run to spend one ticket, so that an operator cannot exceed the budget.

- **S-04.1** Given an authorisation with a ticket left, when a session opens for a new run, then the next ticket is durably spent for that run before the notary answers `ok`.
- **S-04.2** Given a run that already holds a ticket under that authorisation, when another session opens for it, then no ticket is spent and its sequence number advances as before.
- **S-04.3** Given an authorisation with no ticket left, when a session opens for a new run, then the notary answers `spent`, no session starts and no sequence number is used.
- **S-04.4** Given an unknown or unreadable authorisation, when a session opens, then the notary answers `authorisation` and no session starts.
- **S-04.5** Given a run that holds a ticket under one authorisation, when a session opens for it naming another, then the notary answers `authorisation`.
- **S-04.6** Given a session that fails after opening, when the operator tries again, then the spent ticket is not returned.
- **S-04.7** Given notary state from before this pack, when the notary starts, then it rewrites that state as version 2 before serving, keeps every run's number, and refuses any further session for those runs with `authorisation`.
- **S-04.8** Given a refusal for the budget or the authorisation, when the monitor receives it, then the run ends with a distinct reason and is logged as refused.
- **S-04.9** Given a task with a newer authorisation, when a session for a new run names an older one, then the notary answers `authorisation` and spends no ticket.
- **S-04.10** Given a version 1 sequences file and any authorisation, when the notary starts, then it refuses to start, and `notary authorize` refuses while the file is version 1.

### S-05 The audit reads the attempt from the attestation

As an auditor, I want the attempt and ceiling from what the notary signed, so that I do not take the operator's word.

- **S-05.1** Given a version 2 record, when any answer's attestation has no single, well-formed ticket extension, then the audit refuses it.
- **S-05.2** Given a version 2 record, when its answers do not all carry the same authorisation, task, budget, round and ticket, then the audit refuses it.
- **S-05.3** Given a version 2 record, when the task in its ticket extension is not the task recomputed from the record, then the audit refuses it.
- **S-05.4** Given a ticket extension whose ticket is not below its budget, or whose budget is outside 1 to 100, then the audit refuses it.
- **S-05.5** Given a version 2 record that passes, when the audit reports, then it states the attempt number, the ceiling under this authorisation, the budget authorised earlier, and that the identity is this exact task on this notary.
- **S-05.6** Given a version 2 record that passes, when the audit reports, then it adds two limits to the notary pack's five: the operator may have asked the provider directly, away from the notary, and the customer may have authorised this task again after this attestation.
- **S-05.7** Given an operator holding every operator key, when they try each forgery in the design's threat model, then the audit refuses each by its named check.
- **S-05.8** Given a version 2 record, when `--guest` is missing, its hash is not the record's, or replay diverges, then the audit refuses it.
- **S-05.9** Given a version 2 intent, when its `operator` is not the SHA-256 of the operator key the audit was given, then the audit refuses it.
- **S-05.10** Given a passing version 2 record, when the audit reports, then both run-count lines say `under this log key only`.

### S-06 The record has a slot for a provider proof

As an auditor, I want a place for a provider's proof that can never overclaim, so that the format can grow when one exists.

- **S-06.1** Given a version 2 answer with no provider proof, when it is written, then the slot is absent from its bytes.
- **S-06.2** Given a version 2 answer naming a well-formed proof of a kind the tool does not know, when audited, then the audit's other checks still decide, and the report gives the number of proofs not checked and says they prove nothing, without printing their kind.
- **S-06.3** Given a provider-proof entry malformed by any rule in `design.md` §2, when the record is read, then it is refused.
- **S-06.4** Given any record, when the audit reports, then the limit that the model's name is the provider's claim is still stated.
- **S-06.5** Given `spikes/task-kernel/RECORD.md`, when read, then it defines both record versions, the slot and the dated assumption that no provider signs its responses, with what would reopen it.

## Rules and examples

### S-01 The record has one enforced version

Rule: a tool reads only versions it knows, and version 1 only when asked by name.

| ID | Given a record of | Option | Then |
|---|---|---|---|
| S-01.1.e1 | version 3 | none | Refused, naming version 3 |
| S-01.1.e2 | version 0 | `--legacy-v1` | Refused, naming version 0 |
| S-01.2.e1 | version 1 | none | Refused, naming `--legacy-v1` |
| S-01.2.e2 | version 2 | none | Not refused for its version |
| S-01.3.e1 | version 1, valid | `--legacy-v1` | First line is `legacy audit passed: no attempt budget` |
| S-01.3.e2 | version 2, valid | `--legacy-v1` | Audited as version 2, with no legacy statement |

Rule: a run's links carry its record's version, and a version 1 record replays unchanged.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.4.e1 | A version 2 record whose intent link says `v` 1 | Audited | Refused |
| S-01.4.e2 | A version 2 record whose result link says `v` 1 | Audited | Refused |
| S-01.4.e3 | A chain holding a version 1 run and a version 2 run | The version 2 record is audited | Passes |
| S-01.5.e1 | `out/live3.json` | Replayed | `replay identical` |
| S-01.5.e2 | That record with its version changed to 2 | Replayed | Refused or diverged, never identical |

Rule: a record cannot change version, and every link's version is known.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.6.e1 | Version 1 links and record around a ticketed attestation | Audited with `--legacy-v1` | Refused |
| S-01.6.e2 | A real version 1 record with no ticket | Audited with `--legacy-v1` | Passes as legacy |
| S-01.7.e1 | A valid version 1 record | Audited with `--legacy-v1` | First line is not `audit passed` |
| S-01.7.e2 | A valid version 2 record | Audited | First line is `audit passed` |
| S-01.8.e1 | Another link in the chain has `v` 99 | Any record is audited | Refused |
| S-01.8.e2 | A result with `v` 1 for an intent with `v` 2 | Any record is audited | Refused |
| S-01.8.e3 | The committed chain, all `v` 1 | A record is audited with `--legacy-v1` | Not refused for this check |
| S-01.9.e1 | A version 2 transcript with no `task` | Read | Refused |
| S-01.9.e2 | A version 2 transcript whose `task` is another task's | Replayed | Refused or diverged |

### S-02 A task's identity is unambiguous

Rule: the version 2 identity frames each input and differs from the version 1 form.

| ID | Given input lists | Then |
|---|---|---|
| S-02.1.e1 | `["ab", "c"]` and `["a", "bc"]` | Different identities |
| S-02.1.e2 | `[]` and `[""]` | Different identities |
| S-02.1.e3 | The same list twice | The same identity |
| S-02.2.e1 | One guest and inputs, in version 1 and version 2 | Different identities |
| S-02.2.e2 | No inputs, in version 1 and version 2 | Different identities |

Rule: the `task` command needs only the guest and inputs.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.3.e1 | A guest and inputs | `task` runs | Prints the identity the live run then logs |
| S-02.3.e2 | A missing guest file | `task` runs | Refused, printing no identity |

### S-03 The customer authorises a task

Rule: an authorisation names one well-formed task and 1 to 100 attempts.

| ID | Task | Attempts | Then |
|---|---|---|---|
| S-03.1.e1 | Valid | 1 | Kept, identifier printed, round 0 |
| S-03.1.e2 | Valid | 100 | Kept |
| S-03.2.e1 | Valid | 0 | Refused, nothing kept |
| S-03.2.e2 | Valid | 101 | Refused, nothing kept |
| S-03.2.e3 | Uppercase hex | 1 | Refused, nothing kept |

Rule: authorising a task again is explicit and visible.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.3.e1 | The task has round 0 | `authorize` with no `--again` | Refused |
| S-03.3.e2 | Another task is authorised | `authorize` with no `--again` | Kept, round 0 |
| S-03.4.e1 | The task has round 0 | `authorize --again` | Kept, round 1 |
| S-03.4.e2 | The task has rounds 0 and 1 | `authorize --again` | Kept, round 2 |

Rule: a serving notary sees a new authorisation at once.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.5.e1 | A serving notary | `authorize`, then a session naming it | The session is served |
| S-03.5.e2 | A half-written authorisation file | A session names it | `authorisation`, no session |

Rule: each round carries what came before it, and the customer can see what was spent.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.6.e1 | No earlier round | `authorize --attempts 3` | Prior total 0 |
| S-03.6.e2 | Rounds of budget 3 and 5 | `authorize --again` | Prior total 8 |
| S-03.7.e1 | A budget of 3 with one ticket spent | `notary tickets` | Shows 1 of 3 spent |
| S-03.7.e2 | An unreadable authorisation file | `notary tickets` | Refused, printing no partial total |

### S-04 The notary spends one ticket per run

Rule: a new run spends the next ticket, and a known run spends none.

| ID | Budget | Spent | Session for | Then |
|---|---|---|---|---|
| S-04.1.e1 | 2 | 0 | A new run | `ok`, ticket 0 |
| S-04.1.e2 | 2 | 1 | Another new run | `ok`, ticket 1 |
| S-04.2.e1 | 2 | 1 | The run holding ticket 0 | `ok`, ticket 0, next sequence number |
| S-04.2.e2 | 1 | 1 | The run holding ticket 0 | `ok`: a known run needs no new ticket |
| S-04.3.e1 | 1 | 1 | A new run | `spent` |
| S-04.3.e2 | 2 | 2 | A new run under a different operator log key | `spent` |

Rule: an authorisation must be readable and must be the run's own.

| ID | Given | Then |
|---|---|---|
| S-04.4.e1 | An identifier the notary never issued | `authorisation` |
| S-04.4.e2 | An identifier that is not 64 lowercase hex characters | `authorisation` |
| S-04.5.e1 | A run on authorisation A, a session naming B | `authorisation`, and A's and B's tickets unchanged |
| S-04.5.e2 | A run on authorisation A, a session naming A | Served |

Rule: a spent ticket is never returned, by failure or restart.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.6.e1 | Budget 1, a session dropped after the reply | A new run opens | `spent` |
| S-04.6.e2 | Budget 2, one ticket spent, the notary restarted | A new run opens | `ok`, ticket 1 |
| S-04.6.e3 | The save fails while spending | The session opens | `sequence`, the ticket not spent |

Rule: older notary state loads and cannot be used to avoid a budget.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.7.e1 | A `hyalos.sequences/1` file and no authorisation | The notary starts | Rewrites it as version 2 before serving, keeping each run's number |
| S-04.7.e2 | A run from that file | A session opens for it | `authorisation` |
| S-04.7.e3 | A `hyalos.sequences/1` file cut short | The notary starts | Refuses to start |

Rule: the monitor names each refusal.

| ID | The tool prints | Then the run ends with |
|---|---|---|
| S-04.8.e1 | `spent` | The attempts-spent reason, logged as refused |
| S-04.8.e2 | `authorisation` | The no-authorisation reason, logged as refused |
| S-04.8.e3 | An unknown word | The existing `ModelUnavailable` |

Rule: only the newest authorisation takes new runs, and old state cannot return a ticket.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.9.e1 | Rounds 0 and 1, a new run naming round 0 | A session opens | `authorisation`, no ticket spent |
| S-04.9.e2 | Rounds 0 and 1, a new run naming round 1 | A session opens | `ok` |
| S-04.9.e3 | Rounds 0 and 1, a run already on round 0 | A session opens | Served, with no new ticket |
| S-04.10.e1 | A version 1 file and one authorisation | The notary starts | Refuses to start |
| S-04.10.e2 | A version 1 file | `notary authorize` runs | Refused, nothing kept |
| S-04.10.e3 | A version 2 file and authorisations | The notary starts | Starts |

### S-05 The audit reads the attempt from the attestation

Rule: the ticket extension is present once, in its one form, and agrees across the record.

| ID | Given a version 2 record whose attestation | Then |
|---|---|---|
| S-05.1.e1 | Has no ticket extension | Refused |
| S-05.1.e2 | Has two ticket extensions | Refused |
| S-05.1.e3 | Has a ticket extension one byte short | Refused |
| S-05.2.e1 | Answer 1 carries another ticket than answer 0 | Refused |
| S-05.2.e2 | Answer 1 carries another authorisation than answer 0 | Refused |
| S-05.2.e3 | All answers carry the same values | Not refused for this check |

Rule: the ticket belongs to this task and is inside its budget.

| ID | Given | Then |
|---|---|---|
| S-05.3.e1 | A ticket issued for another task | Refused |
| S-05.3.e2 | A ticket issued for the record's task | Not refused for this check |
| S-05.4.e1 | Ticket 2 of budget 2 | Refused |
| S-05.4.e2 | Budget 0 | Refused |
| S-05.4.e3 | Budget 101 | Refused |

Rule: a passing report states the attempt, the ceiling and what the budget cannot show.

| ID | Given | Then the report says |
|---|---|---|
| S-05.5.e1 | Ticket 0 of budget 1, round 0, prior 0 | Attempt 1 of at most 1 under this authorisation, 0 authorised earlier, for this exact task on this notary |
| S-05.5.e2 | Ticket 2 of budget 5, round 1, prior 3 | Attempt 3 of at most 5 under this authorisation, 3 authorised earlier |
| S-05.6.e1 | Any passing version 2 record | The seven limits, by name |
| S-05.6.e2 | A failing record | No statement of proof |

Rule: each forgery is refused by its own check.

| ID | Forgery by an operator holding every operator key | Then |
|---|---|---|
| S-05.7.e1 | Take a ticket's attestation from one run and place it in another run's record | Refused: the notary names another run |
| S-05.7.e2 | Run a varied input under an authorisation for the original task | Refused: the ticket's task is not the record's |
| S-05.7.e3 | Present the record as version 1 to avoid the budget | Refused without `--legacy-v1`, and reported as having no budget with it |
| S-05.7.e4 | Spend the budget under a second operator log key, then ask for one more | The notary answers `spent` |
| S-05.7.e5 | Edit the budget or ticket in a stored attestation | Refused: the attestation does not verify |
| S-05.7.e6 | Open extra sessions in one run and write the answers into the record | Refused: replay diverges |
| S-05.7.e7 | Ask a reworded question at a position | Refused: replay diverges |
| S-05.7.e8 | Back a chain under a second log key with one run's attestations | Refused: the run hash names another key |

Rule: a version 2 audit replays the guest, and holds the intent to the operator's key.

| ID | Given | Then |
|---|---|---|
| S-05.8.e1 | No `--guest` | Refused |
| S-05.8.e2 | A guest with one byte changed | Refused |
| S-05.8.e3 | The record's guest, and a record that replays | Not refused for this check |
| S-05.9.e1 | An intent naming another key's fingerprint | Refused |
| S-05.9.e2 | An intent naming this key's fingerprint | Not refused for this check |
| S-05.10.e1 | A chain with one run under this key | The line says 1 run, under this log key only |
| S-05.10.e2 | A chain with three runs of the task | The line says 3 runs, under this log key only |

### S-06 The record has a slot for a provider proof

Rule: the slot is absent when empty, typed when present, and proves nothing unless its kind is known.

| ID | Given an answer with | When | Then |
|---|---|---|---|
| S-06.1.e1 | No proof | Written | Its bytes hold no slot |
| S-06.1.e2 | No proof, in a version 1 record | Replayed | Byte-identical |
| S-06.2.e1 | A well-formed proof of an unknown kind, in a valid record | Audited | Passes, and reports one proof not checked |
| S-06.2.e2 | The same proof, in a record that fails another check | Audited | Refused by that check |
| S-06.3.e1 | A proof with an empty kind | Read | Refused |
| S-06.3.e2 | A proof whose digest is not 64 lowercase hex characters | Read | Refused |
| S-06.4.e1 | A well-formed proof of an unknown kind | Audited | The model-name limit is still stated |
| S-06.4.e2 | No proof | Audited | The model-name limit is stated |

Rule: the format document carries the assumption and its date.

| ID | Given `RECORD.md` | Then |
|---|---|---|
| S-06.5.e1 | Searched for the assumption | It reads as of 2026-11-22 and names FlashProve as a research prototype |
| S-06.5.e2 | Searched for what reopens it | It names a provider offering a verifiable proof through its API |

## Failure modes

| Failure | Trigger | Expected behaviour | What the user sees | Criterion |
|---|---|---|---|---|
| Budget spent | A new run past the ceiling | No session, the run logged as refused | The attempts-spent reason | S-04.3, S-04.8 |
| Unknown authorisation | A wrong, missing or half-written identifier | No session | The no-authorisation reason | S-04.4, S-04.8 |
| Crash while spending | The notary dies during the save | The ticket is spent or the session never opened, and never both for two runs | A failed call, and a later clean start | S-04.6 |
| Older notary state | An upgrade | Loads, with old runs closed to further sessions | `authorisation` for those runs | S-04.7 |
| Unknown record version | A record from a newer tool | Refused | The version is named | S-01.1 |
| Version 1 record | An older record | Refused unless asked for by name | The option is named | S-01.2 |
| Unknown provider proof | A newer record or a junk entry | Never counted as proof | A count of proofs not checked | S-06.2 |
| Superseded authorisation | A new run naming an older round | No session | The no-authorisation reason | S-04.9 |
| Old notary state with authorisations | A restored file | The notary refuses to start | The refusal, naming the state | S-04.10 |
| Guest missing or changed at audit | An auditor without the exact guest | Refused | The check that failed | S-05.8 |
