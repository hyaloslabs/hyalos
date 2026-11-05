# ADR-0055: Data flows name where the job's data may go

**Status:** Proposed

## Context

[ADR-0004](0004-one-signed-manifest-per-job.md) puts data flows in the signed manifest beside its actions, hosts and tools. [ADR-0031](0031-the-signed-manifest-and-its-admission.md) made them a set of plain names, leaving their vocabulary to the story that defines it. [ADR-0034](0034-the-operator-policy-store-and-minting-within-policy.md) has minting ask the policy about each one (`data_flow`), and tests and examples used names such as `repo->model`. Nothing gave those names a meaning, and nothing checked traffic against them. [ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md) left them out because a TLS tunnel hides a request's content. That is S3.5b (#172).

A name like `repo->model` reads as "the repository's contents may go to the model". The broker cannot check that. Everything a job's data can leave by passes through its socket:
- model calls;
- the git proxy, where a push carries data out and a fetch brings the forge's in ([ADR-0046](0046-the-git-proxy.md));
- the forge API, which opens pull requests ([ADR-0048](0048-opening-a-pull-request-and-the-content-gate-on-its-text.md));
- the egress gate's tunnels to web hosts.

The broker sees which of these a call uses. It never sees where, among the repository, the issue, model output and the harness's own work, the bytes inside the sandbox came from. Once a sandbox has read something, any of it may be in anything the sandbox sends.

## Decision

**A flow names where the job's data may go, and its source is always the job.** There are three, defined in `hyalos_types::flow`:

| Flow | The sink | Calls that carry it |
|---|---|---|
| `job->model` | a model provider | `/v1/messages*`, `/v1/responses*` |
| `job->forge` | the forge, written to | a push (both the `git-receive-pack` advertisement and the call itself), and anything under `/forge/` |
| `job->web` | a web host | a `CONNECT` or an absolute-URL request to the egress gate |

A flow that names its source would promise a check the broker cannot make, so the name keeps the arrow and fixes the source. A fetch through the git proxy carries the forge's data in and the job's ref negotiation out, so it needs `read` and no flow. A path the broker does not serve carries nothing anywhere, and keeps its 404.

**Minting refuses a flow it does not know.** `mint_manifest` refuses any other name with `mint.unknown_flow`, whatever the policy says, as it refuses an unknown action. Each known flow is still one question to the policy (`data_flow`, with the flow as the target's name). So an operator can keep a tainted job off the web:

```
forbid (principal, action == Action::"data_flow", resource)
  when { principal.tainted && resource.name == "job->web" };
```

**The broker enforces flows on the job's socket**, before the call reaches the relay, the git proxy, the forge API or the egress gate. The check comes after the ended and deadline checks, so those keep their own codes.
- A call to a sink whose flow the manifest does not declare is refused with a 403 (`flow.not_declared`). A model call gets the error in its provider's shape, with the code in the message. Any other call gets a plain refusal.
- Each refusal is reported as the manifest gate's `deny` with the SHA-256 of the flow's name. It is reported once per flow and job, under the manifest gate's cap ([ADR-0042](0042-the-egress-gate-and-the-allowlisting-proxy.md)'s reporting).
- Nothing of a refused call is sent on.

The flows are checked beside the rest of the manifest, not instead of it. A job with `job->web` still reaches only its `egress_hosts`, and a job with `job->forge` still pushes only inside its branch prefix. The flows add one thing: a job may have no path to a sink at all, whatever else its manifest says.

**`hyalos run` declares what its job needs.** It always declares `job->model` and `job->forge`, and `job->web` only when `--allow-host` names a host. A run without one therefore has its web requests refused as an undeclared flow, before the egress gate ever sees them. The operator's policy must now permit `data_flow` for the run to mint ([running-a-job.md](../operations/running-a-job.md)).

## Consequences

An operator can say, in policy, which sinks a kind of job may send to, and the broker holds every job to its signed answer at the only place its data can leave. A tainted job can be kept off the web entirely, or a reader off the forge.

The flows say where data may go, not what. A job allowed `job->web` can send any of what it holds to the hosts it may reach. Narrowing that needs the content of a request, and the tunnel hides it. The content gate ([ADR-0048](0048-opening-a-pull-request-and-the-content-gate-on-its-text.md), [ADR-0054](0054-the-content-gate-on-what-a-push-carries.md)) is what checks content, where the broker sees it.

The names in existing manifests change meaning: `repo->model` is no longer a flow, and a manifest that declares it is not minted. No manifest outlives its job, so nothing stored is affected. The pinned manifest bytes in `core/types/tests/manifest.rs` keep `repo->model`, since the type holds any name and only minting checks them.

Tests:
- `core/broker/tests/sandbox.rs`: a job without `job->model` has its model calls refused in both providers' shapes, reported once and never sent. A job with only `job->model` has its push advertisement, push, pull request, `CONNECT` and absolute-URL request refused and reported per flow. Its fetch goes on to the git proxy, and its model call passes.
- `core/engine/tests/policy_store.rs`: unknown flow names are refused at minting, and a known flow the policy does not permit is denied naming it.
- `core/hyalos/tests/run_image.rs` passes `--allow-host` so the unlisted host its harness asks for is still the egress gate's refusal.

Not here: flows for comments or other forge writes, which have no route yet, and flows between the steps of a task, which have no engine loop yet.
