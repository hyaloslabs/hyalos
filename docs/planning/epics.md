# Epics and stories: filing seed

> **This file is not a status tracker.** It is the source text used to file GitHub epic and story issues ([ADR-0014](../adr/0014-tracker-holds-status.md)). Once filed, the tracker is authoritative; do not update this file to reflect progress. Delete or archive it after filing.

Ten epics, each with a goal, a definition of done and stories sized to one merge request. Story IDs (S1.1 and so on) become GitHub issue titles; acceptance criteria become the issue's task list. Each story is sized to one pull request, and its acceptance is something an agent can verify by running something.

## E1: Event log and engine loop (M1)

**Goal:** the engine is a deterministic state machine over a signed, hash-chained log. **Done when:** a task can be created, held and completed purely as events, and replayed from a snapshot to the same state. **ADRs:** 0003, 0009, 0013.

| Story | Acceptance |
| --- | --- |
| S1.1 As the engine, I append signed, hash-chained events to SQLite | Tampering with any stored event fails `hyalos log verify` naming the event |
| S1.2 As the engine, I rebuild tasks, jobs and holds as projections | Deleting the projection tables and replaying yields identical state |
| S1.3 As an operator, I back up and restore by snapshot plus replay | Restore onto a fresh machine reproduces state byte for byte |
| S1.4 As a maintainer, I version event schemas and upcast old events | A v1 event log loads under a v2 engine with no manual step |
| S1.5 As a developer, I run the engine under a fake clock and seeded randomness | The same seed produces the same event sequence on every run |
| S1.6 As the engine, every gate returns allow, deny or unknown, and unknown holds | A gate that times out leaves the task held with one open decision |

## E2: Substrates and the first contained job (M1)

**Goal:** a sandbox with no network and no secrets, reachable only through a socket to the broker. **Done when:** one job runs on the best substrate available and its event records which. **ADRs:** 0002, 0011.

| Story | Acceptance |
| --- | --- |
| S2.1 As the engine, I start a sandbox through a substrate interface | A contract suite passes against a fake substrate and the first real driver |
| S2.2 As a job, I run in a rootless container with gVisor and no network | Any outbound connection other than the broker socket fails |
| S2.3 As the engine, I detect the best substrate and record it per job | The job event names the substrate; doctor reports it |
| S2.4 As a Linux user with KVM, my jobs run in a microVM | The contract suite passes on Firecracker or Cloud Hypervisor |
| S2.5 As a macOS user, my jobs run in Apple's lightweight VMs | The contract suite passes on macOS |
| S2.6 As an auditor, I see attestation evidence when the substrate offers it | The job event carries the evidence, or records that none was available |

## E3: Broker and manifests (M1)

**Goal:** the broker is the sandbox's only route out and enforces a signed manifest. **Done when:** a job reaches an LLM and pushes to a branch with no credential ever inside the sandbox. **ADRs:** 0002, 0004, 0008.

| Story | Acceptance |
| --- | --- |
| S3.1 As the engine, I sign a versioned manifest per job | The broker refuses a job whose manifest signature or version is invalid |
| S3.2 As a job, I call an LLM through the broker, which adds the key | No provider key appears in the sandbox's environment, files or memory dump |
| S3.3 As a job, I clone and push through the broker's git proxy | A push outside the manifest's repo or branch prefix is refused and audited |
| S3.4 As the broker, I exchange for per-job forge tokens | No operator PAT is configured anywhere; tokens expire with the job |
| S3.5 As the broker, I enforce egress allowlists and declared data flows | A request to an undeclared host is refused and recorded as an event |
| S3.6 As a maintainer, I express manifest constraints in Cedar | Cedar analysis proves no manifest grants push outside its own repo |

## E4: Provenance, roles and the workflow language (M2)

**Goal:** attacker-written text cannot reach an acting step's prompt, by construction. **Done when:** a workflow that would let it fails to install. **ADRs:** 0005, 0008.

| Story | Acceptance |
| --- | --- |
| S4.1 As the engine, every value carries provenance as a Rust type | Code that passes tainted text to an actor prompt fails to compile |
| S4.2 As a workflow author, I declare step roles and typed reader schemas | The workflow schema rejects an actor step that names raw text as input |
| S4.3 As the broker, a reader's key refuses any tool definition | A reader request with tools gets 403 and an audit event |
| S4.4 As an actor, I read raw text only through a gated handle | Releasing a handle needs an approved decision bound to its digest |
| S4.5 As the broker, I content-gate outbound text from tainted jobs | The ported red-team items that target sinks are refused |

## E5: Agent interface and approvals (M2)

**Goal:** any MCP client can drive Hyalos, and only the operator can approve. **Done when:** Claude Code and Hermes each run a job end to end through MCP. **ADRs:** 0010, 0012.

| Story | Acceptance |
| --- | --- |
| S5.1 As a driving agent, I use Hyalos through a stateless MCP 2026-07-28 server | The official conformance suite passes |
| S5.2 As a driving agent, I start a job as an MCP Task and poll its progress | A Task survives an engine restart and resumes from the log |
| S5.3 As an operator, I see approvals and results as MCP Apps in my client | An approval renders and resolves inside Claude Code without a browser |
| S5.4 As an operator, I approve through a separate channel the agent cannot use | An approval signed with the agent's credentials is refused |
| S5.5 As a new user, I tell my agent "set up Hyalos" | The Hyalos Agent Skill installs and runs a first job in under 10 minutes |
| S5.6 As an operator, I use a thin CLI for the same actions | Every CLI command maps to one MCP tool; no CLI-only behaviour |

## E6: Plugin runtime (M3)

**Goal:** any Agent Skill, WASM component or MCP server runs with only the capabilities it declares. **Done when:** a public skill with scripts and a third-party MCP server each run contained. **ADRs:** 0006, 0011.

| Story | Acceptance |
| --- | --- |
| S6.1 As the engine, I host WASM components in Wasmtime with capabilities as host imports | A component calling an undeclared import fails to instantiate |
| S6.2 As a user, I install an Agent Skill and its scripts run in a sandbox | A skill script cannot read files or hosts outside its manifest |
| S6.3 As a user, I run a third-party MCP server contained behind the broker | The server has no network except what its manifest allows |
| S6.4 As a user, I see a capability diff before any install or update | The diff lists added, removed and unchanged capabilities |
| S6.5 As a workflow author, I compose skills, components and deterministic steps | A workflow mixing all three installs and passes the role checks of E4 |

## E7: Marketplace and supply chain (M3)

**Goal:** publishing and installing need no per-release human, and trust rests on enforced capabilities and automated evidence. **Done when:** a plugin goes from merge to installed on another machine with no human step, and an update adding a capability waits for consent. **ADRs:** 0007, 0012.

| Story | Acceptance |
| --- | --- |
| S7.1 As an author, my plugin builds reproducibly in CI | Two independent builds produce the same digest |
| S7.2 As the pipeline, I sign keylessly against the CI workflow identity | A signature from any other workflow identity fails verification |
| S7.3 As the pipeline, I record every release in a public transparency log | Install refuses a release with no log entry |
| S7.4 As the pipeline, I run automated evidence on every release | Static analysis, review agents, red-team items and the capability diff all report; a failure blocks publication |
| S7.5 As an operator, I set install policy once | Updates within policy install automatically; an added capability waits for consent |
| S7.6 As an air-gapped operator, I install from an offline signed bundle | Install works with no network and verifies the same evidence |
| S7.7 As a maintainer, I revoke a proven-malicious release | A revoked digest refuses to install and installed copies are disabled with a notice |

## E8: Verification and release gates (M4)

**Goal:** every release is proven by evidence, not by hand. **Done when:** a release cannot be tagged without green simulation, contract, red-team and chaos results for its commit. **ADRs:** 0013.

| Story | Acceptance |
| --- | --- |
| S8.1 As a maintainer, I replay thousands of fault schedules in CI | A failing seed reproduces locally with one command |
| S8.2 As a maintainer, I specify the job state machine in TLA+ | The model checker passes for the invariants in the spec |
| S8.3 As a maintainer, every boundary has a property-based contract suite | Broker, substrates and plugin host each run theirs in CI |
| S8.4 As a maintainer, I port and grow the red-team corpus | The reach-a-sink rate is tracked per release and may not regress |
| S8.5 As a maintainer, chaos scenarios kill each dependency mid-job | Every scenario ends with tasks held, never proceeding unguarded |
| S8.6 As a maintainer, a tag requires the evidence above | Tagging without it is refused |

## E9: Learnings and self-improvement (M4)

**Goal:** Hyalos gets better with use without creating a new way in. **Done when:** a skill generated from a successful run becomes trusted only after verified replay. **ADRs:** 0003, 0005.

| Story | Acceptance |
| --- | --- |
| S9.1 As a driving agent, I query learnings over the event log | Learnings return with their provenance and source events |
| S9.2 As the engine, I draft a skill from a successful run | The draft is tainted and cannot run with more than read capabilities |
| S9.3 As the engine, a drafted skill becomes trusted only after replay in simulation | A draft that fails replay or the capability checks stays tainted |

## E10: Launch (M4)

**Goal:** a release you can introduce with pride. **Done when:** the repo is public, reviewed and installable in minutes. **ADRs:** 0014.

| Story | Acceptance |
| --- | --- |
| S10.1 As a new user, I go from install to first contained job in under 10 minutes | Timed on a clean macOS and Linux machine |
| S10.2 As a reader, I find a threat model, SECURITY.md and a data-flows page | Each control in the threat model names its code and its test |
| S10.3 As a maintainer, the trusted core has an independent security review | Findings are fixed or recorded as ADRs before release |
| S10.4 As a visitor, I watch a two-minute demo of an attack being contained | The demo shows a poisoned skill refused and a steered job held |
| S10.5 As a maintainer, the repo is MIT licensed with clean history and contributor terms | A secrets scan of full history is clean |
