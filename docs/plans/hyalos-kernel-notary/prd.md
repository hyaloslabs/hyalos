# PRD — hyalOS: an independent notary

## Problem

A run's record is meant to convince someone who does not trust the operator. Today it cannot, for four reasons.

1. **The operator holds the notary key.** The notary runs inside the operator's own tool under a key on the operator's machine. Whoever holds that key can sign an attestation for an answer the model never gave.
2. **An attestation is not tied to its run.** It proves the provider answered a question at some time. The operator can ask the same question several times and keep the answer they like, or reuse an old attestation in a later run that asks the same question.
3. **The audit trusts what the public log says.** It asks the log which entries it holds under the operator's key and believes the reply, without checking the log's own proofs. It also needs the operator's private key file just to learn the public key, so nobody but the operator can run it.
4. **Nobody knows what an attestation costs the notary.** The spike measured both sides together in one process.

Evidence: the spike in `spikes/task-kernel`. Its audit refused seven forgeries against the attestation check, and every one of them assumed the forger did not hold the notary key.

## Users and jobs

- **Operator:** runs tasks for someone else, holds the model credential and the ledger key.
- **Customer:** the party the work is for. Runs the notary on their own machine.
- **Auditor:** anyone who checks a record later, including the customer. Holds only public keys.

**Jobs.**

- Customer: "When a vendor runs automated work on my behalf, I want to witness each model call myself without seeing or handling the vendor's credential, so I can believe the record they hand me."
- Operator: "When I run a task for a customer, I want my record to carry proof they can check alone, so I can be believed without asking for trust."
- Auditor: "When I am handed a record, I want to check it with public keys and nothing else, so I can say what it proves without relying on the party who made it."

## Goals

- G1. An operator who holds every key on their own machine cannot produce a record that passes the audit, unless the customer's notary took part in each model call.
- G2. Each attested answer belongs to exactly one run and one position in it.
- G3. The audit runs with public keys only and verifies the public log's proofs itself.
- G4. When the notary cannot be reached, no task runs.
- G5. The notary's cost per attestation is measured and written down.

## Non-goals

- A public notary service for any caller. The parent pack records it as a separate idea.
- More than one notary, or a threshold of notaries.
- Defending against a notary that colludes with the operator. The customer runs the notary, so the customer would be deceiving themselves.
- Hiding the question from the auditor. A presentation still shows it.
- The record's long-term format and a packaged verifier (`hyalos-kernel-record`).
- Verifying the Bitcoin timestamp. The public log's own signed time is what this pack relies on.
- More than one model provider, or any request other than the one approved label question.
- Moving the spike into the workspace or the trusted core.

## Success measures

- M1. A forgery test in which the forger holds the operator's credential, ledger key and every file on the operator's machine fails the audit in every case tried.
- M2. An attested model call between two machines completes in 10 seconds or less.
- M3. The notary's CPU time, peak memory and bytes exchanged per attestation are recorded from at least 40 attestations.

## Non-functional requirements

| Quality | Target |
|---|---|
| Performance | One attested call takes 10 s or less with the notary on a different machine and a round trip of 20 ms or less between them. Proposed: the spike measured 1.9 s with both sides in one process, and nothing over a network |
| Reliability | The notary serves 40 sessions in a row without a restart. Any failure of the notary refuses the run, and never yields an unattested answer |
| Security | The notary's key exists only on the notary's machine. The notary takes part only in sessions with the one approved server name. The operator's credential never reaches the notary, a presentation, a log line or an error |
| Privacy | The notary learns that a session happened, when, with which server, and how many bytes moved. It learns no plaintext |
| Accessibility | none: command-line tools only |
| Observability | The notary writes one line per session: time, server name, bytes each way, outcome. Never plaintext, never a key |
| Compatibility / portability | Linux on x86-64. Both sides use the same pinned TLSNotary release, and a mismatch is refused, not negotiated |
| Maintainability | Stays in `spikes/task-kernel`. No dependency is added to `core/` or `crates/` |

## Assumptions

| Assumption | How to validate | If false |
|---|---|---|
| The model provider accepts a request that carries the binding in a field it ignores, and still answers | One attested call with the binding present, before any other work | Stop. The fallback is time alone: the session must fall between the run's two links. That is weaker, and the developer decides whether it is enough |
| The protocol stays usable between two machines on one network | Measure one attested call across two machines | M2 and the Performance target are renegotiated with the developer |
| The protocol library lets the notary add a signed field of its own to an attestation | Add a run and a sequence number to one attestation and read them back from the presentation | Stop. S-03.5 cannot be met as written, and re-asking inside a run stays open |
| The public log returns, for each entry, an inclusion proof and a signed time that can be verified with its published key | Verify one existing entry from `spikes/task-kernel/ledger` | Stop. G3 cannot be met as written |
| The provider keeps accepting the TLS version and key exchange the protocol needs | The first attested call of every run | Runs are refused until the protocol or the provider changes |
| One customer notary serves one operator, one session at a time | Stated by the developer's choice of a customer-run notary | Concurrency becomes its own slice |

## Story map

| Stand up the notary | Run with it | Tie answers to the run | Audit alone | Know the cost |
|---|---|---|---|---|
| S-01 | S-02 | S-03 | S-04 | S-05 |

## Stories and acceptance criteria

### S-01 The customer runs their own notary

As a customer, I want to run the notary as my own service under my own key, so that no attestation exists unless I took part.

- **S-01.1** Given no notary key on the machine, when the customer starts the notary, then it makes a key readable only by its owner, prints the public key, and listens on the address it was given.
- **S-01.2** Given a running notary, when an operator's tool opens a session for the approved server, then the notary takes part and returns an attestation signed with its key.
- **S-01.3** Given a running notary, when a session turns out to be with any other server, then the notary signs nothing and records a refusal.
- **S-01.4** Given the operator's tool, when it runs any command, then it never opens a notary key. No code path in it reads one.
- **S-01.5** Given a session, when it ends for any reason, then the notary writes one line with the time, server name, bytes each way and outcome, and no plaintext or key.

### S-02 The operator's run uses the customer's notary

As an operator, I want every model call in a run to go through the customer's notary, so that my record carries their attestations.

- **S-02.1** Given the notary's address and public key, when a run makes a model call, then the call is attested by that notary and the record names the presentation by hash.
- **S-02.2** Given the notary cannot be reached, when a run needs a model call, then the run is refused and its result link says so. No record is written.
- **S-02.3** Given the notary answers with an attestation signed by a different key than the one configured, when the tool checks it, then the call is refused.
- **S-02.4** Given the two sides run different protocol releases, when a session starts, then it is refused.

### S-03 Each attestation belongs to one run

As an auditor, I want each attestation tied to the run and position it was made for, so that an answer cannot be re-rolled or reused.

- **S-03.1** Given a run whose intent link is logged, when a model call is attested, then the attested request carries that intent hash and the question's position in the run.
- **S-03.2** Given a record, when the audit checks an answer, then it refuses unless the presentation's binding names this run's intent hash and this answer's position.
- **S-03.3** Given a record, when the audit checks an answer, then it refuses unless the session time falls between the times the public log signed for the run's intent link and result link.
- **S-03.4** Given the approved request template, when a presentation's request differs from it in anything but the title and the binding, then verification refuses it.
- **S-03.5** Given a notary that signs, into every attestation, the run it was told and how many attestations it had already signed for that run, when the audit checks a record, then it refuses unless every answer's attestation names the record's own run and carries a sequence number equal to the answer's position (ADR-006).

### S-04 An auditor checks a record with public keys only

As an auditor, I want to audit a record holding nothing but public keys, so that I do not depend on the operator to check the operator.

- **S-04.1** Given the operator's public key and the notary's public key, when the audit runs, then it opens no private key and needs no file from the operator's home directory.
- **S-04.2** Given the ledger's links, when the audit runs, then it verifies each link's signature under the operator's public key, and each entry's inclusion proof and signed time under the public log's published key.
- **S-04.3** Given a record with answers, when the audit runs, then every answer must carry an attestation. A record with any unattested answer is refused.
- **S-04.4** Given a forger who holds the operator's credential, ledger key and files but not the notary's key, when they build a record with an answer the notary did not witness for that run, then the audit refuses it.
- **S-04.5** Given an audit that passes, when it reports, then it states what was proven and what was not: the notary's independence is the customer's own, the model's name is the provider's claim, the answer's correctness is not judged, the public log is trusted to have shown every entry it holds under the operator's key, and the request's hidden bytes are not shown, so the call may have used another credential or carried an added header (ADR-012). The report also states how many runs the public log holds under that key for the same task, as the intent link names the task, so a reader sees when a task was run more than once.

### S-05 The notary's cost is known

As the developer, I want the notary's own cost per attestation measured, so that a notary service can be priced on evidence.

- **S-05.1** Given a running notary, when 40 or more sessions have completed, then a report gives the notary's CPU seconds, peak memory and bytes exchanged per attestation.
- **S-05.2** Given the same sessions, when the report is written, then it gives the operator's wall time per attested call, so M2 can be read from it.

## Rules and examples

### S-01 The customer runs their own notary

Rule: the notary's key is made once, on the notary's machine, readable by its owner only.

| ID | Given | When the notary starts | Then |
|---|---|---|---|
| S-01.1.e1 | No key file | | A key file is made with owner-only access, the public key is printed, the service listens |
| S-01.1.e2 | A key file from an earlier start | | The same public key is printed and no new key is made |
| S-01.1.e3 | A key file that others can read | | The notary refuses to start and says the key file's access is too open |

Rule: the notary signs only for the approved server.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.2.e1 | A running notary | A session with the approved server completes | An attestation signed with the notary's key is returned |
| S-01.3.e1 | A running notary | A session's server certificate is for another name | No attestation is returned and the session line says refused |
| S-01.3.e2 | A running notary | The session is dropped halfway | No attestation is returned and the notary serves the next session |

Rule: the operator's tool cannot sign as a notary.

| ID | Given | When | Then |
|---|---|---|---|
| S-01.4.e1 | The operator's tool, built | Its commands are listed | None makes, reads or prints a notary key |
| S-01.4.e2 | A notary key file placed in the operator's home directory | The operator's tool runs a model call with no notary reachable | The call is refused. The file is not opened |

Rule: a session line holds facts about the session and nothing from inside it.

| ID | Given | When a session ends | Then |
|---|---|---|---|
| S-01.5.e1 | A completed session | | One line: time, server name, bytes each way, `attested` |
| S-01.5.e2 | A refused session | | One line with `refused` and a reason code. No part of the exchange and no key appears |

### S-02 The operator's run uses the customer's notary

Rule: a model call happens through the configured notary or not at all.

| ID | Given | When a run makes a model call | Then |
|---|---|---|---|
| S-02.1.e1 | Notary reachable, its public key configured | | The answer is used, and the record names its presentation by hash |
| S-02.2.e1 | Nothing listening at the notary's address | | The run is refused, the result link says refused, no record is written |
| S-02.2.e2 | The notary accepts the connection and then stops answering | | The call is refused when its deadline passes |
| S-02.3.e1 | The attestation is signed by a key other than the configured one | | The call is refused |
| S-02.4.e1 | The notary runs a different protocol release | | The session is refused at its start |

### S-03 Each attestation belongs to one run

Rule: the binding inside the attested request must name this run and this position.

| ID | Given | When the audit checks an answer | Then |
|---|---|---|---|
| S-03.2.e1 | The presentation's binding names this run's intent hash and position 3, for answer 3 | | Accepted |
| S-03.2.e2 | The presentation was made in an earlier run that asked the same question | | Refused: the binding names another run |
| S-03.2.e3 | Answers 2 and 5 ask the same question, and answer 5 reuses answer 2's presentation | | Refused: the binding names position 2 |
| S-03.2.e4 | The presentation's request carries no binding | | Refused |

Rule: the session must happen inside the run's window in the public log.

| ID | Given | When the audit checks an answer | Then |
|---|---|---|---|
| S-03.3.e1 | Session time between the log's times for the intent and the result | | Accepted |
| S-03.3.e2 | Session time earlier than the log's time for the intent | | Refused: the answer was obtained before the run was declared |
| S-03.3.e3 | Session time later than the log's time for the result | | Refused |

Rule: only the approved request can be attested.

| ID | Given | When a presentation is verified | Then |
|---|---|---|---|
| S-03.4.e1 | The request is the template with a title and a binding | | Accepted |
| S-03.4.e2 | The request names a different model, or different instructions | | Refused |

Rule: every kept answer is the attestation the notary signed for its run at exactly that point in the sequence, so no kept answer replaced an earlier one. An ask after the last kept answer changes nothing in the record.

| ID | Given | When the audit checks a record | Then |
|---|---|---|---|
| S-03.5.e1 | 8 answers whose attestations name the record's run and carry sequence numbers 0 to 7 in order | | Accepted |
| S-03.5.e2 | The operator asked the question at position 3 twice and kept the second answer, so the sequence numbers run 0, 1, 2, 4 | | Refused: an attestation is unaccounted for |
| S-03.5.e3 | The notary restarted in the middle of a run | | Its next sequence number for that run is one more than its last. It never repeats or goes back |
| S-03.5.e4 | The operator asked the first question three times and kept the third answer, so position 0 carries sequence number 2 | | Refused: an attestation is unaccounted for |
| S-03.5.e5 | The operator told the notary another run's intent hash while asking this run's question | | Refused: the notary's run is not the record's |

### S-04 An auditor checks a record with public keys only

Rule: the audit needs public keys and nothing private.

| ID | Given | When the audit runs | Then |
|---|---|---|---|
| S-04.1.e1 | An account with the record, the ledger's links, the presentations and two public keys | | The audit completes |
| S-04.1.e2 | The same, and the operator's private key file is absent | | The audit still completes |

Rule: the audit believes the public log only as far as its proofs go.

| ID | Given | When the audit runs | Then |
|---|---|---|---|
| S-04.2.e1 | Every link's signature and every entry's proof verify | | Accepted |
| S-04.2.e2 | One stored entry's inclusion proof is altered | | Refused, naming the link |
| S-04.2.e3 | One link is signed by a key other than the operator's | | Refused, naming the link |

Rule: every answer in a record is attested, and holding the operator's keys is not enough to fake one.

| ID | Given | When the audit runs | Then |
|---|---|---|---|
| S-04.3.e1 | A record with 8 answers and 8 valid presentations | | Accepted |
| S-04.3.e2 | A record with one answer that names no presentation | | Refused |
| S-04.4.e1 | The forger runs a notary of their own and attests an invented answer with it | | Refused: not the trusted notary |
| S-04.4.e2 | The forger takes a real presentation from another run and logs a consistent chain around it | | Refused: the binding names another run |
| S-04.4.e3 | The forger obtains a real attestation first and logs the intent afterwards | | Refused: the session is outside the run's window |

Rule: a passing audit says what it does not prove.

| ID | Given | When the audit passes | Then |
|---|---|---|---|
| S-04.5.e1 | A record that passes | | The report lists what was proven, then the five limits by name |
| S-04.5.e2 | A record that fails | | The report names the first check that failed and prints no statement of proof |
| S-04.5.e3 | A record that passes, whose task the log holds three runs of under the operator's key | | The report says the task was run 3 times |

### S-05 The notary's cost is known

Rule: cost is reported per attestation, from enough sessions to average.

| ID | Given | When the report is asked for | Then |
|---|---|---|---|
| S-05.1.e1 | 40 completed sessions | | CPU seconds, peak memory and bytes per attestation, and the operator's wall time per call |
| S-05.1.e2 | Fewer than 40 completed sessions | | The report says how many there were and that the figures are not yet a measurement |

## Failure modes

| Failure | Trigger | Expected behaviour | What the user sees | Criterion |
|---|---|---|---|---|
| Notary down | Nothing listening at its address | The run is refused before any model call is used | Operator: the run was refused, the notary could not be reached | S-02.2 |
| Notary stalls | It accepts a connection and goes quiet | The call is refused at its deadline | Operator: the run was refused, the notary did not answer in time | S-02.2 |
| Wrong notary | The attestation's key is not the configured one | The call is refused | Operator: the attestation is not from the configured notary | S-02.3 |
| Release mismatch | The two sides run different protocol releases | The session is refused at its start | Both: the releases differ | S-02.4 |
| Provider rejects the binding | The provider refuses or alters a request that carries it | The call is refused | Operator: the provider did not accept the request | S-03.1 |
| Provider stops supporting the protocol's TLS | The provider drops the TLS version or key exchange | Every call is refused | Operator: the session could not be set up | S-02.2 |
| Public log unreachable at audit | The auditor is offline and holds no stored proofs | The audit is refused, never passed on trust | Auditor: the log's proofs could not be checked | S-04.2 |
| Key file too open | The notary's key can be read by others | The notary does not start | Customer: the key file's access is too open | S-01.1 |
| A model call fails after the notary signed | The provider answers with an error, or the operator's tool fails after the attestation | The run is refused, and no later answer for that position can pass | Operator: the run was refused | S-03.5 |
| Session dropped | The connection between the two sides breaks | No attestation, the notary stays up | Operator: the run was refused. Customer: a refused session line | S-01.3 |
