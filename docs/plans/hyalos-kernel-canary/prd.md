# PRD — hyalOS: notice when the provider stops refusing a forged body

## Problem

A request's 106 hidden bytes could hold a forged body. We treat that forgery as refused because the provider answered it with a 400 on 2026-11-21 (`docs/plans/hyalos-kernel-notary/decisions/ADR-011-the-provider-refuses-a-body-forged-in-the-hidden-bytes.md`).

1. That answer is the provider's behaviour, and it can change without notice.
2. The only re-check is a manual probe, so nothing stops a run when the answer changes.
3. A check run by the operator is not evidence, because the operator controls the hidden bytes and could make the forged request fail.
4. A record does not say when the assumption was last checked, or by whom.

## Users and jobs

- **Customer:** runs the notary, holds a provider credential, and relies on the audit.
- **Auditor:** anyone checking a record offline, with no credential.
- **Operator:** runs the work and is the party being checked.

**Jobs.**

- Customer: "When the provider's behaviour may have changed, I want my notary to stop serving runs until a check of my own passes, so I can trust what it signs."
- Auditor: "When I read a record, I want to see when the forged-body assumption was last checked and by whom, so I can judge the claim without trusting the operator."
- Operator: "When the canary holds, I want a clear refusal, so I know to ask the customer rather than retry."

## Goals

- G1. A changed provider answer stops new attested sessions.
- G2. The check runs on the customer's side, with the customer's credential, and the operator cannot fake it.
- G3. Each new record shows, under the notary's signature, when the check last passed.
- G4. Existing version 1 and version 2 records keep their audits, and cannot be used to avoid the check.

## Non-goals

- Testing other forged shapes.
- Closing the hidden API-key case. The fifth limit (notary pack ADR-012) stays in every report.
- Building the zero-knowledge proof that the hidden bytes hold no line break. It remains the lasting fix.
- Scheduling the check. The customer runs it, for example from cron.
- Letting the notary hold or read a provider credential.
- Making the 24-hour window a setting.

## Success measures

- M1. With a stand-in provider that accepts the forged shape, no further attestation is signed.
- M2. Every version 3 record that passes audit states the time of a fresh pass, signed by the trusted notary.
- M3. The committed version 2 run from #426 still passes its audit, with a line saying it has no canary.

## Non-functional requirements

| Quality | Target |
|---|---|
| Performance | One check makes at most 3 small provider calls. Reading the state adds no network call to a session |
| Reliability | Any unreadable, malformed, stale or future state refuses the session. A failed write records no new pass and leaves the file as it was |
| Security | The notary never opens the credential. The operator cannot write the state or choose the window |
| Privacy | No credential, reply body or provider error text is printed, stored or signed |
| Accessibility | none: command-line tools with plain text output |
| Observability | The check prints case names, status codes and one verdict. A refused session is logged as `refused canary` in the notary's session lines |
| Compatibility / portability | Version 1 and 2 records audit as before, plus one stated line for version 2. Linux only, as today |
| Maintainability | The forged request is built from the same body template the existing probe is pinned to |

## Assumptions

| Assumption | How to validate | If false |
|---|---|---|
| The customer can obtain a credential of the pinned 106-byte shape | The check refuses any other shape, with a fixed message | The check cannot pass, so the notary serves no run. A new decision is needed |
| The provider's 400 on the forged shape still holds | The first real check | The state latches and #409's fallback, the zero-knowledge proof, reopens |
| The notary's clock is honest | It is the customer's machine, and the notary's independence is already a stated limit | The window means less. The report keeps that limit |
| One check a day is frequent enough | Reviewed if a provider change is ever observed inside a window | A new decision shortens the window |

## Story map

| Check the provider | Hold the notary | Audit the record | Write it down |
|---|---|---|---|
| S-01 | S-02 | S-03 | S-04 |

## Stories and acceptance criteria

### S-01 The customer checks the provider

As the customer, I want one command that tests the forged shape with my credential, so that my notary knows whether the assumption still holds.

- **S-01.1** Given a working credential, when both controls get 200 and the forged shape gets 400, then the state records a pass at the verdict's time.
- **S-01.2** Given a check that gets anything else, when it ends, then no pass remains: a 200 on the forged shape latches, and any other result holds.
- **S-01.3** Given any credential file, when the check runs, then the credential appears in no output or state, and a file with the wrong shape or open permissions is refused before any provider call.
- **S-01.4** Given a latched state, when a later check passes, then it stays latched until the customer runs the reset command, which leaves it held.

### S-02 The notary holds runs without a fresh pass

As the customer, I want my notary to refuse sessions unless a fresh pass exists, so that a changed provider stops attested runs.

- **S-02.1** Given no fresh pass, when a session opens, then the notary answers `canary`, spends no number or ticket, and the run ends with a canary reason.
- **S-02.2** Given a fresh pass, when the notary signs, then the attestation carries exactly one canary extension holding that pass's time.
- **S-02.3** Given a running notary, when it checks the canary, then it reads only the state file and never a credential.

### S-03 The audit checks and states the canary

As an auditor, I want the audit to refuse a record without a fresh signed pass, so that I need not trust the operator.

- **S-03.1** Given a version 3 record, when any answer's canary is missing, doubled, malformed, later than its session or over 24 hours older, then the audit refuses.
- **S-03.2** Given a version 3 record that passes, when the report is printed, then it states the oldest pass's time and that the customer's notary signed it, and it keeps the hidden-bytes limit and adds the canary's own limit.
- **S-03.3** Given a version 1 or 2 record, when it is audited, then a record with no canary passes as before, a version 2 report states it has no canary, and any attestation carrying a canary is refused.

### S-04 The format and the real run are written down

As a customer or auditor, I want the check documented, so that I can run and verify it without reading the code.

- **S-04.1** Given `RECORD.md`, when it is read, then it defines version 3, the state file, the canary extension, the `canary` link word, the 24-hour window, the latch and the limits.
- **S-04.2** Given the operator's two-machine probe, when no customer pass exists on the notary machine, then it stops before sending anything to the public log or the issue.

## Rules and examples

### S-01 The customer checks the provider

Rule S-01.1: only 200, 400, 200 is a pass.

| ID | Given provider statuses | When | Then |
|---|---|---|---|
| S-01.1.e1 | 200, 400, 200 | the check runs | prints `verdict: the provider refused the forged body`, exits 0, state `passed` at the verdict's time |
| S-01.1.e2 | 200, 400, 200, and the state file cannot be written | the check runs | exits non-zero, records no new pass, and leaves the state file as it was |

Rule S-01.2: every other result leaves no pass.

| ID | Given provider statuses or state | When | Then |
|---|---|---|---|
| S-01.2.e1 | 200, 200, 200 | the check runs | prints `verdict: THE PROVIDER ACCEPTS THE FORGED SHAPE`, exits 1, state `latched` |
| S-01.2.e2 | 500 on the first control, with an earlier pass in the state | the check runs | exits 2, state `held`, the earlier pass is gone |
| S-01.2.e3 | 200, 500, 200 | the check runs | exits 2, state `held` |
| S-01.2.e4 | 200, 400, 500 | the check runs | exits 2, state `held` |
| S-01.2.e5 | the connection drops | the check runs | exits 2, state `held` |
| S-01.2.e6 | an earlier pass, and the non-pass results cannot be staged | the check starts | refused with exit 2, no provider connection, state unchanged |
| S-01.2.e7 | an earlier pass, staging succeeded, and the rename of a non-pass result fails | the verdict is recorded | the state file is removed, so no pass remains, and a later passing check is refused until the customer's reset |

Rule S-01.3: the credential is checked before any provider call and never echoed.

| ID | Given credential file | When | Then |
|---|---|---|---|
| S-01.3.e1 | valid, holding a marker value | a check runs to any verdict | the marker is in neither stdout, stderr nor the state file |
| S-01.3.e2 | length 105 | the check starts | refused with a fixed message, no provider connection, state unchanged |
| S-01.3.e3 | mode 644 | the check starts | refused with a fixed message, no provider connection, state unchanged |

Rule S-01.4: a latch is the customer's to clear.

| ID | Given state | When | Then |
|---|---|---|---|
| S-01.4.e1 | `latched`, provider now answers 200, 400, 200 | the check runs | prints that the state stays latched, exits 1, state `latched` |
| S-01.4.e2 | `latched` | the customer runs the reset | state `held`, and a later passing check records a pass |
| S-01.4.e3 | a malformed state file | a check runs | refused with a fixed message, exits 2, no provider connection, file unchanged |
| S-01.4.e4 | a malformed state file | the customer runs the reset | state `held` |
| S-01.4.e5 | the state file was removed after an earlier check, so the marker remains | a check runs | refused with exit 2, no provider connection, until the customer's reset |
| S-01.4.e6 | a `passed` state with no marker, the forged shape gets 200, and the rename of the staged result fails | the check ends, then a passing check runs | the state is removed, the marker remains, and the later check is refused with exit 2 and no provider connection |
| S-01.4.e7 | a state file with no marker, and the marker cannot be made | a check starts | refused with exit 2, no provider connection, state unchanged |

### S-02 The notary holds runs without a fresh pass

Rule S-02.1: a session opens only on a fresh pass, checked before anything is spent.

| ID | Given state at session open | When | Then |
|---|---|---|---|
| S-02.1.e1 | `passed` 1 hour ago | a session opens | served |
| S-02.1.e2 | no state file | a new run's first session opens | replies `canary`, no ticket spent, the monitor reports the canary reason |
| S-02.1.e3 | `passed` 24 hours and 1 second ago | a session opens | replies `canary`, no number spent |
| S-02.1.e4 | `held` | a session opens | replies `canary` |
| S-02.1.e5 | `latched` | a session opens | replies `canary` |
| S-02.1.e6 | malformed, or a pass in the future | a session opens | replies `canary` |
| S-02.1.e7 | a run with sessions served, and the pass then goes stale | its next session opens | replies `canary`, and the run ends refused |
| S-02.1.e8 | a pass fresh at open but over 24 hours old at the signed session time | the notary comes to sign | no attestation, logged `refused canary`, and the session's number stays spent |

Rule S-02.2: every attestation signs its pass.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.2.e1 | a pass at time T | an attestation is signed | verify reports canary time T |
| S-02.2.e2 | a requester adds its own canary extension | the notary signs | the request is refused, as for other requester extensions |

Rule S-02.3: the notary holds no credential.

| ID | Given | When | Then |
|---|---|---|---|
| S-02.3.e1 | a marker credential file in the notary's directory | sessions are served | the notary does not open it, shown by its access time or by removing read permission |
| S-02.3.e2 | the notary's source | searched | it references no credential path or `pinned_credential` |

### S-03 The audit checks and states the canary

Rule S-03.1: a version 3 audit requires one fresh signed pass per answer.

| ID | Given a version 3 record whose attestation | When audited | Then |
|---|---|---|---|
| S-03.1.e1 | carries a pass 1 hour before its session | audited | passes |
| S-03.1.e2 | carries no canary | audited | refused, naming the answer |
| S-03.1.e3 | carries two canaries | audited | refused |
| S-03.1.e4 | carries a canary of the wrong length | audited | refused |
| S-03.1.e5 | carries a pass after its session | audited | refused |
| S-03.1.e6 | carries a pass 24 hours and 1 second before its session | audited | refused |
| S-03.1.e7 | carries a pass exactly 24 hours before its session | audited | passes |

Rule S-03.2: a pass states when, by whom and what it does not prove.

| ID | Given | When | Then |
|---|---|---|---|
| S-03.2.e1 | answers with passes at T1 and a later T2 | the report prints | names T1, the oldest, in UTC, as checked by the customer on this notary |
| S-03.2.e2 | any passing version 3 record | the report prints | still has the hidden-bytes limit, and adds that the canary tested one forged shape, with the customer's credential, at the time stated |

Rule S-03.3: older records keep their audits and cannot carry a canary.

| ID | Given | When audited | Then |
|---|---|---|---|
| S-03.3.e1 | the committed version 2 record from #426 | audited | `audit passed`, with `not proven: this record has no provider canary` |
| S-03.3.e2 | a version 2 record whose attestation carries a canary | audited | refused |
| S-03.3.e3 | a version 1 record, with `--legacy-v1` | audited | `legacy audit passed: no attempt budget`, as before |

### S-04 The format and the real run are written down

Rule S-04.1: the format document covers the canary.

| ID | Given | When | Then |
|---|---|---|---|
| S-04.1.e1 | `RECORD.md` | searched | names `hyalos.canary/1`, version 3, the `canary` word and the 24-hour window |
| S-04.1.e2 | `RECORD.md` | searched | states that a latch needs the customer's reset, and that the canary does not close the hidden API-key case |

Rule S-04.2: the real probe will not publish without a pass.

| ID | Given on the notary machine | When the probe runs | Then |
|---|---|---|---|
| S-04.2.e1 | a fresh pass | it reaches the run | it proceeds |
| S-04.2.e2 | no fresh pass | it starts | exits non-zero before any public log entry or issue comment |

## Failure modes

| Failure | Trigger | Expected behaviour | What the user sees | Criterion |
|---|---|---|---|---|
| Provider accepts the forged shape | Provider change | Latch, and refuse all sessions | The check's verdict, then `canary` on each run | S-01.2, S-02.1 |
| Provider or network outage | Control fails | Hold, and refuse sessions until a later pass | `inconclusive` verdict | S-01.2 |
| Customer stops running the check | No pass for 24 hours | Refuse sessions | Run ends with the canary reason | S-02.1 |
| Pass goes stale during a run | A long run crosses the window | The next session is refused and the run ends refused | Canary reason in the result link | S-02.1.e7 |
| State file damaged | Disk or edit | Refuse sessions | `canary` | S-02.1.e6 |
| Bad credential | Wrong length or open mode | Refused before any call | Fixed message | S-01.3 |
| Operator submits an older-format record | Downgrade attempt | A canary in a version 2 attestation is refused | Audit refusal | S-03.3.e2 |
