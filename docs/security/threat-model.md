# Threat model

What Hyalos defends, against whom, and how. Every control below names the code that implements it and a test that proves it. The test is the evidence. If a control has no test, it is listed under [Known gaps](#known-gaps), not claimed here. Paths are relative to the repository root, and `file::name` names a function, type or test in that file. A test marked *(unit)* is a `#[cfg(test)]` test inside the source file.

Where secrets live and what crosses each boundary: [data-flows.md](data-flows.md). How to report a vulnerability: [`SECURITY.md`](../../SECURITY.md).

## What we protect

- **Secrets:** the engine's log key (`engine.key`), the operator's key (`operator.key`), model provider keys (`provider.keys`), the GitHub App key (`github-app.pem`), per-job forge tokens and agent credentials.
- **Authority:** what a job may write to the forge (branches, pull requests, merges), and every decision a human must make.
- **The record:** the signed, hash-chained event log of every decision.
- **The operator's machine:** nothing a job runs may reach it except through the broker.

## Who attacks

| Adversary | Can | Cannot (by design) |
| --- | --- | --- |
| **A steered agent** in the sandbox, driven by prompt injection from an issue, a comment, a repository file, a page or a tool's output | Run any code in its sandbox and send anything to the broker's socket | Hold a secret, reach the network directly, or decide anything a human must decide |
| **A poisoned skill, plugin or MCP server** | Ship code and instructions that run inside a job | Get more than the operator granted, or change after install |
| **The driving agent** (Claude Code or similar, over MCP or the thin CLI) | Start and watch jobs | Approve, release, cancel, merge or install (ADR-0010, ADR-0070) |
| **Forge content**: issue text, comments, an outsider's issue | Be read by a reader step | Reach an actor's prompt or the engine as text |
| **A network attacker** | Intercept the broker's outbound traffic | Present a certificate the Mozilla roots don't vouch for |
| **Someone who tampers with the log on disk** | Edit, delete, reorder or truncate rows | Re-sign them without `engine.key` |
| **A compromised supply chain** | Publish a release or a trusted root | Pass the pinned Sigstore identity, roll back the root, or undo a revocation |

Out of scope: an attacker running as the operator's own OS user, or with root on the host (see [Known gaps](#known-gaps)), and a compromised model provider or GitHub.

## Trust boundaries

```
operator's terminal ── hyalos approve/release/merge (signed with operator.key)
        │
driving agent ── MCP / thin CLI (agent credential; no decisions)
        │
   ┌────▼──────── engine process ────────────┐   holds engine.key, github-app.pem;
   │ policy, holds, signed log, minting      │   mints the per-job forge token
   └────┬────────────────────────────────────┘
        │ private Unix channel, closed typed wire set (ADR-0035)
   ┌────▼──────── broker process ────────────┐   holds provider.keys and the live
   │ model relay, egress gate, git proxy,    │   job's forge token
   │ forge, content gates, plugin host       │
   └────┬────────────────────────────────────┘
        │ one socket per job (the sandbox's only way out)
   ┌────▼──────── sandbox ───────────────────┐   no network, no secrets,
   │ harness + agent + skills                │   placeholder keys only
   └─────────────────────────────────────────┘
```

## Controls

### Secrets stay out of the sandbox

| Threat | Control | Code | Test | ADR |
| --- | --- | --- | --- | --- |
| A steered agent reads or exfiltrates a provider key | Only the broker reads `provider.keys`. The sandbox's environment holds `placeholder-not-a-key`. The relay drops every header outside the pass-through list and adds the real key upstream. | `core/broker/src/sandbox.rs::Providers::parse`; `core/broker/src/model/relay.rs::Relay::handle`; `core/broker/src/model/mod.rs::Adapter::passthrough` | `core/hyalos/tests/model_call.rs::no_provider_key_reaches_the_sandbox`; `core/broker/tests/sandbox.rs::a_job_calls_a_model_through_its_own_socket_with_the_brokers_key` | 0016, 0038 |
| The agent bypasses the relay to call a provider with a key of its own | The egress gate refuses provider API hosts, and every name under them, even when the manifest lists them | `core/broker/src/egress.rs::Egress::is_provider_host` | `core/broker/tests/egress.rs::provider_hosts_are_refused_even_when_listed` | 0042, 0051 |
| A forge token reaches the sandbox | The engine mints a token scoped to the manifest. It reaches the broker only in `StartJob`, stays in memory while the job lives, and is revoked when the job ends. | `core/hyalos/src/github.rs::GitHubApp::mint`; `core/broker/src/serve.rs::Broker::forge_token` | `core/hyalos/tests/github_app.rs::the_engine_mints_a_token_for_the_manifest_hands_it_to_the_broker_and_revokes_it`; `core/broker/tests/serve.rs::the_forge_token_is_held_only_for_an_admitted_job_and_only_while_it_is_live` | 0044, 0045 |
| A token outlives an interrupted job | Interrupting a run stops the sandbox and revokes the token | `core/hyalos/src/run_cmd/drive.rs::Run::revoke` | `core/hyalos/tests/run_interrupt.rs::an_interrupted_run_stops_the_sandbox_revokes_the_token_and_records_the_end` | 0045 |
| The host's environment leaks into the sandbox | The spec's `env` is the sandbox's whole environment. Nothing is inherited. | `core/substrate/src/spec.rs::SandboxSpec` | `crates/substrate-contract/src/checks.rs::environment` | 0032 |
| A secret ends up in the log | A simulated full job, then a scan of the log for planted secrets | `crates/world/src/scan.rs::scan` | `crates/world/tests/world.rs::a_full_job_run_leaves_no_secret_or_raw_content_in_the_log` | 0036 |
| Key theft through a core dump or a readable key file | Core dumps are turned off before any key is read. Owner and mode are checked on every secret file. | `core/hyalos/src/hardening.rs::no_core_dumps`; `core/hyalos/src/keys.rs::read_private` | `core/hyalos/tests/hardening.rs::a_crash_leaves_no_core_file_where_an_unhardened_process_would_leave_one`; `core/hyalos/src/keys.rs::a_key_or_directory_others_can_reach_is_refused` *(unit)* | 0022, 0030 |

### The sandbox is contained

| Threat | Control | Code | Test | ADR |
| --- | --- | --- | --- | --- |
| A sandbox reaches the network | No network: `--network none`, or no NIC on a microVM. The only way out is the broker's socket. | `core/substrate/src/container.rs::ContainerSubstrate`; `core/substrate/src/microvm.rs::MicroVmSubstrate` | `crates/substrate-contract/src/checks.rs::no_network`, run by `crates/substrate-contract/tests/container.rs::the_container_driver_passes_the_whole_contract` and its gVisor and microVM twins | 0032, 0041, 0056 |
| Code in the sandbox escalates privilege | Read-only root, a non-root user, all capabilities dropped, `no-new-privileges`, memory and process limits | `core/substrate/src/spec.rs::SandboxSpec::validate` | `crates/substrate-contract/src/checks.rs::not_root`, `::read_only_root`, `::no_privileges`, `::limits` | 0032 |
| Weaker isolation is chosen silently | The strongest ready driver is chosen and recorded on `job.started` | `core/substrate/src/choose.rs::choose` | `core/substrate/src/choose.rs::the_strongest_ready_driver_is_chosen_and_every_driver_is_reported` *(unit)*; `crates/world/tests/world.rs::the_job_records_the_substrate_that_ran_it` | 0043 |
| Isolation is claimed beyond what was checked | Each driver states its attestation. Today every driver records `none`, never a claim it can't back. | `core/types/src/substrate.rs::Attestation` | `core/types/src/substrate.rs::attestation_is_a_reason_or_unrecorded_and_nothing_else` *(unit)* | 0057 |

### Every call out of the sandbox passes a gate

| Threat | Control | Code | Test | ADR |
| --- | --- | --- | --- | --- |
| Exfiltration over the web, server-side request forgery, or the cloud metadata address | The egress proxy allows only a host name the manifest lists, on port 443 or 80, that resolves to a public address. IP literals are refused. Every refusal is reported to the engine. | `core/broker/src/egress.rs::proxy`, `::is_public` | `core/broker/tests/egress.rs::an_allowed_name_that_resolves_to_a_private_address_is_refused`, `::anything_else_is_refused_with_403_and_reported_once`; `crates/broker-contract/tests/egress.rs::a_request_reaches_its_host_exactly_when_the_manifest_allows_it_and_is_else_refused_and_reported` | 0042 |
| Data goes to a sink the job did not declare | Each route maps to a flow (`job->model`, `job->forge`, `job->web`). An undeclared flow is refused. | `core/broker/src/sandbox.rs::sink` | `core/broker/tests/sandbox.rs::data_goes_only_where_the_manifest_declares_a_flow` | 0055 |
| A push or fetch goes to another repository or branch | The git proxy serves four routes, and checks the repository, the action, the token, and that each ref is under the job's branch prefix | `core/broker/src/git.rs::route`, `::ref_allowed` | `core/broker/tests/git.rs::a_job_clones_and_pushes_its_branch_and_nothing_else`, `::another_repository_is_refused_and_reported` | 0046 |
| A push carries a credential or hidden text | The content gate inflates the pack, resolves its deltas, and checks the result against every held secret | `core/broker/src/pack.rs::check`; `core/broker/src/content.rs::check` | `core/broker/tests/git.rs::what_a_push_carries_is_content_gated_after_its_deltas_are_resolved`; `core/broker/src/pack.rs::a_credential_a_delta_builds_from_its_base_is_refused` *(unit)* | 0054 |
| A pull request goes outside the manifest, or its text leaks a key, a link or hidden text | Only the manifest's repository, head and base. A tainted job's text gets the stricter published-text gate. | `core/broker/src/pulls.rs::answer`; `core/broker/src/content.rs::check_published` | `core/broker/tests/pulls.rs::a_pull_request_outside_the_manifest_is_refused_and_reported`, `::the_content_gate_refuses_a_credential_or_hidden_text_and_reports_it`; `core/broker/src/content.rs::a_credential_encoded_reversed_or_split_is_refused` *(unit)* | 0048, 0083 |
| A reader step uses tools | A reader's model call that carries tools is refused and reported | `core/broker/src/sandbox.rs::READER_TOOLS` | `core/broker/tests/sandbox.rs::a_readers_call_with_tools_is_refused_and_reported_by_the_role_gate` | 0061 |
| A network attacker intercepts the model call | Model calls use the Mozilla roots, and the certificate is checked before the key is sent. Plain HTTP goes only to loopback. | `core/broker/src/model/http.rs::HttpTransport::new` | `core/broker/tests/model_tls.rs::a_certificate_the_mozilla_roots_do_not_vouch_for_is_refused_before_the_key_is_sent`; `core/broker/tests/model_local.rs::plain_http_reaches_only_loopback_and_anything_else_needs_tls` | 0039 |
| A job spends without limit | The broker meters each job's model tokens against its budget | `core/broker/src/model/usage.rs::Meter::charge` | `crates/world/tests/world.rs::a_job_that_overspends_its_budget_does_not_succeed_and_the_sandbox_holds_only_its_placeholder` | 0038 |

### Untrusted text cannot steer the engine or an actor

| Threat | Control | Code | Test | ADR |
| --- | --- | --- | --- | --- |
| Prompt injection from forge or model text reaches an actor's prompt or the engine | `ForgeText` and `ModelText` have no way to read them as text: no `as_str`, `Display`, `Deref`, `AsRef` or `Into<String>`. `ActorPrompt` has no method that accepts them. | `core/types/src/provenance.rs::ForgeText`, `ModelText`; `core/types/src/prompt.rs::ActorPrompt` | the `compile_fail` doctests in `core/types/src/prompt.rs`; `core/types/src/prompt.rs::no_prompt_shows_its_text_in_debug` *(unit)* | 0047, 0059 |
| Forge content crosses to the engine as free text | Issues are typed, capped and parsed with `deny_unknown_fields`, then checked again by the engine. The wire is a closed set. | `core/broker/src/forge.rs::Forge::read_issue`; `core/types/src/wire.rs::ToEngine` | `core/broker/tests/forge.rs::content_over_a_cap_or_with_unknown_fields_fails_the_job_and_never_the_broker`, `::the_parser_never_panics_on_bytes_it_is_given`; `core/types/src/wire.rs::model_text_and_forge_content_cannot_cross_to_the_engine` *(unit)* | 0035, 0047 |
| A workflow lets tainted input reach an actor | Taint comes from the role and the inputs, and is checked at install. A job given forge content is minted only tainted. | `core/types/src/workflow.rs::Workflow::check`; `core/engine/src/mint.rs::Engine::mint_manifest` | `core/types/src/workflow.rs::an_actor_step_that_names_raw_text_does_not_install` *(unit)*; `core/engine/tests/policy_store.rs::an_actor_or_a_step_given_forge_content_is_minted_only_tainted` | 0060 |
| **A steered job: an outsider's issue drives a job** | The job holds until the operator releases the issue, or until policy releases it because only people with write permission wrote it | `core/hyalos/src/run_cmd/release.rs::Run::await_release`; `core/types/src/writers.rs::Writers::all_can_write` | `core/hyalos/tests/run_job.rs::policy_releases_an_issue_only_writers_wrote_and_an_outsiders_still_waits`, `::an_awaited_issue_reaches_the_actor_once_the_operator_releases_it` | 0078, 0103 |
| Raw issue text reaches an actor without release | The broker holds the bytes behind a handle, and the engine sees only its digest. A manifest may name the handle only after release. | `core/broker/src/handles.rs::hold`; `core/engine/src/engine.rs::Engine::release_handle` | `core/broker/tests/handles.rs::a_held_issue_is_served_only_to_a_job_whose_manifest_names_its_handle`; `core/engine/tests/handles.rs::a_human_releases_a_handle_and_only_then_may_a_manifest_name_it` | 0063 |
| A job's result carries injected text | A result is typed values only, checked against the signed manifest's result schema | `core/broker/src/submit.rs::answer` | `core/broker/tests/submit.rs::a_submitted_result_reaches_the_engine_as_typed_values_only` | 0064, 0079 |
| Tainted data reaches the driving agent unmarked | It is wrapped in an `<untrusted-data>` element that the data cannot close | `core/hyalos/src/mcp/mod.rs::untrusted_block` | `core/hyalos/src/mcp/mod.rs::untrusted_data_is_marked_in_the_text_and_cannot_close_its_element` *(unit)* | 0072 |
| Injection resistance regresses | A red-team corpus runs against the real broker gates. Its reach is recorded for each release and may not regress. | `crates/redteam/src/lib.rs::corpus`, `::regressions` | `crates/redteam/tests/corpus.rs::every_steered_write_in_the_corpus_is_refused_but_the_known_gaps`, `::the_reach_rate_does_not_regress_from_the_last_release` | 0065, 0110 |

### Gates fail closed

| Threat | Control | Code | Test | ADR |
| --- | --- | --- | --- | --- |
| An error, a timeout or a late answer is read as allow | `ask` returns an `Outcome`, not a `Result`. Only an on-time allow proceeds. | `core/policy/src/gate.rs::ask` | `core/policy/tests/gate.rs::only_an_on_time_allow_ever_proceeds`, `::a_gate_that_answers_after_the_deadline_has_timed_out_whatever_it_says` | 0009, 0020 |
| An unknown answer is recorded without a hold | A raw unknown `gate.answered` is refused, and replay refuses a log where an unknown did not hold | `core/engine/src/engine.rs::Engine::rule_gate` | `core/engine/tests/gates.rs::an_unknown_answer_cannot_be_written_on_its_own`, `::replay_refuses_a_log_where_unknown_did_not_hold` | 0020 |
| A pack the broker cannot read is pushed | The content gate answers unknown, and the engine holds the job | `core/hyalos/src/run_cmd/report.rs::Reported::take` | `core/hyalos/tests/run_job.rs::a_push_the_content_gate_cannot_read_holds_the_job` | 0054 |
| A lost broker, sandbox, model or forge counts as success | Losing any of them holds the task | `core/hyalos/src/run_cmd/actor.rs::Run::hold_lost` | `core/hyalos/tests/chaos.rs::a_broker_killed_mid_job_holds_the_task`, `::a_forge_lost_mid_job_holds_the_task` | 0106 |
| A Cedar forbid that errors lets a permit through | Any evaluation error is unknown, so the request holds | `core/policy/src/cedar.rs::Policies::judge` | `core/policy/tests/cedar.rs::a_baseline_forbid_that_errors_on_a_missing_attribute_holds_instead_of_allowing` | 0008, 0033 |
| Operator policy weakens the baseline | The baseline cannot be removed or replaced, and its properties are proved symbolically | `core/policy/policies/baseline.cedar`; `crates/policy-proof/src/lib.rs::Prover::prove_properties` | `core/policy/tests/shipped.rs::an_operator_policy_cannot_remove_or_take_the_place_of_a_baseline_policy`; `crates/policy-proof/tests/proofs.rs::every_property_holds_for_every_action` | 0033 |

### Decisions are the operator's, and the record holds

| Threat | Control | Code | Test | ADR |
| --- | --- | --- | --- | --- |
| The agent or the broker writes a decision event | `submit` refuses every decision kind. Only the engine's decision methods write them, each signed with `operator.key`. | `core/engine/src/engine.rs::Engine::submit` | `core/engine/tests/merges.rs::only_a_human_approves_once_before_any_job_and_never_through_submit`; `core/engine/tests/agents.rs::raw_agent_events_are_refused_from_the_engine_and_the_broker` | 0025, 0070 |
| A human decision is forged, moved or replayed | Each approval is signed and bound to the engine key, its place in the log and its event | `core/types/src/approval.rs::OperatorKey::approve` | `core/engine/tests/approvals.rs::a_signature_moved_to_another_event_or_another_log_does_not_hold`, `::an_approval_signed_with_any_other_key_such_as_the_agents_fails_verify` | 0070 |
| The broker forges an engine event | The broker may submit nothing but `gate.answered`. This is checked on write and on open. | `core/types/src/event.rs::Event::may_come_from` | `core/engine/tests/origin_and_time.rs::the_broker_may_submit_nothing_but_a_gate_answer`, `::a_signed_log_with_a_disallowed_origin_is_refused_on_open` | 0024 |
| An MCP tool makes a decision | No tool decides. Every tool has a thin-CLI twin, and the decision commands are a separate list. | `core/hyalos/src/tools_cmd.rs::COMMANDS`, `DECISIONS` | `core/hyalos/src/tools_cmd.rs::no_command_here_is_an_operator_decision` *(unit)*; `core/hyalos/tests/mcp.rs::approvals_are_shown_with_the_operators_commands_and_the_server_cannot_decide_them` | 0010, 0075 |
| A stolen or revoked agent credential is used | The log stores only the credential's SHA-256, and the credential is checked on every message | `core/hyalos/src/agent_cmd.rs::credential_digest`; `core/engine/src/state.rs::State::admitted_agent` | `core/hyalos/tests/mcp.rs::over_http_a_credential_is_admitted_until_the_operator_revokes_it_and_then_the_next_call_fails`, `::the_log_holds_the_credentials_digest_never_the_credential` | 0068 |
| A remote caller reaches the MCP server | HTTP binds loopback only and checks the Host and Origin headers | `core/hyalos/src/mcp/http.rs::bind` | `core/hyalos/src/mcp/mod.rs::http_serves_loopback_only_and_refuses_another_host_or_origin` *(unit)* | 0074 |
| An unreviewed head is merged | `merge.approved` binds the repository, the PR, the head and the method. Only untainted jobs may merge. | `core/engine/src/engine.rs::Engine::approve_merge` | `core/engine/tests/merges.rs::the_manifest_must_repeat_exactly_what_was_approved`; `core/hyalos/tests/merge.rs::a_head_that_moved_is_refused_by_github_and_recorded_as_such` | 0071 |
| A manifest is forged, relabelled or expired | Ed25519 signatures under a domain tag. The broker admits a manifest only for a live job, the credential's role, and before its deadline. | `core/types/src/manifest.rs::SignedManifest::open`; `core/broker/src/admission.rs::Admission::admit` | `core/broker/tests/admission.rs::a_signature_that_is_not_the_engines_is_refused`, `::a_known_version_cannot_be_relabelled_after_signing`, `::a_manifest_past_its_deadline_is_refused_and_one_at_it_is_not` | 0031 |
| A compromised broker reads the engine's keys from disk | Once its startup reads are done, the broker confines itself with Landlock to the system's files, the plugin store and its socket directory. The engine will not start a broker whose grants reach the config directory. | `core/hyalos/src/confine.rs::apply`; `core/hyalos/src/launch.rs::BrokerProcess::launch_confined` | `core/hyalos/tests/broker_confined.rs::a_broker_process_cannot_open_engine_key_or_github_app_pem`, `::the_engine_will_not_start_a_broker_that_could_read_the_config_directory` | 0118 |
| Another local process impersonates the broker or the engine | The socket sits in a 0700 directory. Both ends are checked with `SO_PEERCRED` and the parent's pid. The broker starts with a cleared environment. | `core/hyalos/src/channel.rs::Listener::accept`; `core/hyalos/src/launch.rs::BrokerProcess::start_with` | `core/hyalos/tests/broker_channel.rs::a_process_that_is_not_the_broker_cannot_use_the_channel`, `::the_broker_is_started_as_this_executable_with_nothing_of_the_engines` | 0035 |
| The log is edited, deleted, reordered or truncated | A SHA-256 chain from a key-bound genesis, an Ed25519 signature on every event, triggers that refuse UPDATE and DELETE, and an expected head | `core/log/src/verify.rs::verify`; `core/log/src/store.rs::Log::append` | `core/log/tests/log.rs::editing_any_column_of_an_event_names_that_event`, `::recomputing_every_hash_still_fails_because_the_attacker_cannot_sign`, `::swapping_two_events_is_noticed`, `::removing_events_from_the_end_is_only_caught_by_an_expected_head` | 0018, 0026 |

### Skills, plugins and the supply chain

| Threat | Control | Code | Test | ADR |
| --- | --- | --- | --- | --- |
| **A poisoned skill** with symlinks, hidden files, oversized content, content changed after install, or wider capabilities | The skill reader refuses links, hidden names and oversize content. Skills are stored by digest, and the store refuses changed content. An install that widens capabilities stops without the operator's grant. | `core/plugin/src/skill.rs::Skill::read`, `Store::load`; `core/hyalos/src/plugin_cmd.rs::manage_with` | `core/plugin/src/skill.rs::a_link_a_hidden_name_or_too_much_is_refused`, `::the_store_keeps_a_skill_by_digest_and_refuses_one_changed_after` *(unit)*; `core/hyalos/tests/plugin.rs::a_skill_asking_for_capabilities_installs_only_with_the_operators_grant`, `::an_install_is_refused_for_a_bad_skill_or_without_the_passphrase` | 0088, 0089, 0091 |
| A skill or plugin widens a job | A job gets the intersection of the grant and the manifest | `core/types/src/capability.rs::Capabilities::bound` | `core/engine/tests/policy_store.rs::a_skill_cannot_widen_a_job` | 0066 |
| A WASM plugin calls what it was not granted, or never stops | Only the granted interfaces are linked. Fuel, memory and call caps apply, with a fresh instance per job. | `core/plugin/src/lib.rs::Runtime::instantiate` | `core/plugin/tests/host.rs::a_component_importing_a_capability_it_was_not_granted_fails_to_instantiate`, `::a_plugin_that_never_stops_runs_out_of_fuel`; `crates/plugin-contract/tests/host.rs::a_call_reaches_the_broker_exactly_when_its_name_is_granted` | 0086, 0087 |
| An MCP server in the sandbox reaches beyond its grant | It runs as a skill in the sandbox, under the job's manifest bounded by its grant | `core/plugin/src/skill.rs::McpServer::launch` | `core/hyalos/tests/run_image.rs::an_mcp_server_in_the_sandbox_reaches_only_what_its_grant_allows` | 0101 |
| A release is unsigned or mis-signed, the trusted root is rolled back, or a revocation is undone | Offline Sigstore verification against the pinned pipeline identity. A trusted-root snapshot is accepted only if it is newer and vouched for by the current one. Revocations come in a signed list. | `core/plugin/src/sigstore.rs::Trust::verify`; `core/plugin/src/snapshot.rs::Snapshot::successor`; `core/plugin/src/revocation.rs::Revocations::verify` | `core/plugin/tests/sigstore.rs::a_changed_artifact_a_wrong_identity_and_a_flipped_signature_are_refused`; `core/plugin/tests/snapshot.rs::an_older_snapshot_is_refused`; `core/hyalos/tests/plugin.rs::a_revoked_release_is_disabled_and_never_installed_while_an_older_list_cannot_undo_it` | 0093, 0097, 0098 |
| A skill drafted from a steered run is trusted | A draft is tainted and read-only until it replays byte for byte from the signed log | `core/hyalos/src/draft.rs::replay` | `core/hyalos/src/run_cmd/drive.rs::a_draft_is_trusted_only_after_it_replays_and_is_always_held_to_read` *(unit)*; `core/engine/tests/plugins.rs::a_draft_comes_only_from_a_success_holds_only_read_and_survives_compaction` | 0113, 0114 |
| A release ships without its evidence, or an agent pushes a tag | Every release job waits for the evidence job, and only the operator's script creates a tag | `.github/workflows/release.yml`; `.github/scripts/tag-release.sh` | `crates/xtask/tests/release_gate.rs::every_release_job_waits_for_the_evidence`, `::nothing_but_the_operators_script_creates_a_tag` | 0107, 0111 |
| The broker links the engine's, the log's or the keys' code | A table of allowed crate dependencies, checked in CI | `crates/xtask/src/main.rs::violations` | `crates/xtask/src/main.rs::broker_may_not_depend_on_the_engine`, `::broker_may_not_link_the_log_or_substrate` *(unit)* | 0017 |

Several of these tests skip when the tool they need is missing: a container runtime, gVisor, a microVM, the sandbox image, or cvc5. CI sets the `HYALOS_REQUIRE_*` variables, so in CI they run or fail, and never skip.

## Known gaps

These are stated so that nobody relies on a control that does not exist.

1. **Without Landlock, the broker is not confined.** On Linux with Landlock, the broker cannot open the engine's key files (ADR-0118). On a kernel without Landlock, or on another system, it runs as the operator's OS user with no filesystem confinement, and says so on standard error. There, a broker compromised by code execution could read `engine.key` and `github-app.pem` from disk. `operator.key` is sealed under the passphrase either way.
2. **No hardware attestation.** Every driver records attestation as `none` (ADR-0057). The control records the truth honestly. It does not attest anything.
3. **What a job sends to its model is not content-gated.** The model provider is a declared sink (`job->model`), reached with the broker's key. Data a job has read may go to the model it is allowed to call.
4. **The red-team corpus has known gaps.** `crates/redteam/reach.json` lists the attacks that still reach for each release. The rule is that the list may not grow.
5. **The `compile_fail` doctests check error codes only on nightly.** On stable they prove only that the code fails to compile.
6. **A same-user attacker on the host is out of scope** (ADR-0035). Someone who can attach to the engine as the operator's user, or who has root, defeats these controls. The engine is marked not dumpable (ADR-0030), but that is a hardening step, not a boundary.
