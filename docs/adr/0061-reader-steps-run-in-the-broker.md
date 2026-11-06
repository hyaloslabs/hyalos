# ADR-0061: Reader steps run in the broker, and the engine gets typed values

**Status:** Proposed

## Context

S4.3 (#25) asks for three things:
- A reader request with tools gets 403 and an audit event.
- A reader's model call and the check of its answer against the schema run in the broker, and the engine receives only typed values ([ADR-0002](0002-one-binary-two-processes-sandboxes.md)).
- Input over the size or depth cap, or with unknown fields, fails the job, never the broker process.

What exists:
- The relay already refuses a reader's request that defines any tool or MCP server, with 403 `model.reader_tools` ([ADR-0037](0037-model-protocol-adapters-and-their-line-budget.md)). Nothing records the refusal.
- [ADR-0060](0060-the-workflow-document-and-the-check-at-install.md) gives a reader step an `output` of typed fields (`bool`, bounded integer, `choice`) and no string field.
- The broker reads an issue and hands it to the engine as `ForgeIssue` ([ADR-0047](0047-forge-content-as-typed-values-and-derived-taint.md)). For a reader, that would put the issue's text in the engine only for the engine to send it back.
- `ReaderPrompt` takes tainted text ([ADR-0059](0059-tainted-text-cannot-reach-an-actor-prompt-by-type.md)), and nothing uses it yet.

A reader has no harness and no sandbox: it is one model call, with no tools, whose answer is checked against a schema. Nothing about it needs a container.

## Decision

**The broker runs a reader step itself.** The engine sends `ToBroker::Read { job, call }`, and wire version 6 adds it. A `ReaderCall` (`hyalos_types::wire`, `deny_unknown_fields`) carries:
- `protocol` (`anthropic` or `openai`) and `model`;
- `instructions` and `context`, the operator's text, including earlier typed values the engine has rendered;
- `issue`, the number of the issue to read, if the step reads one;
- `output`, the step's typed fields.

The broker answers `ToEngine::Values { job, values }` or `Refused { job, reason }`. A refusal fails the job, and the broker forgets the job as `end_job` does (`Broker::end`). The broker carries on serving, as it does for a failed issue read.

**Typed values hold no text.** `hyalos_types::workflow::TypedValue` is `{"bool": b}`, `{"integer": n}` or `{"choice": i}`, where `i` is the index of the option in the step's output. The engine reads the option from its own copy of the workflow. So the one string a model writes on this path, its answer, ends in the broker, and a `Values` frame holds no string at all.

**What the broker does with a call** (`hyalos_broker::reader::run`), in order. Each failure is a short code:

| Check | Code |
| --- | --- |
| The job is admitted. | `admission.not_live` |
| Its manifest's role is `reader`. | `reader.not_a_reader` |
| The output is 1 to 16 well-named, well-formed fields (`workflow::output_is_valid`, ADR-0060's rules again, since the call comes over the channel); the model name is 1 to 128 printable ASCII bytes; there are at most 16 context pieces; the instructions and context are at most 32 KiB together. | `reader.bad_call` |
| The deadline has not passed. | `admission.expired` |
| The issue, if named, reads as ADR-0047 says, with the job's token and `read`. | the forge's codes (`forge.*`) |
| The broker has a model relay at all. | `model.no_provider` |

Then the broker builds the prompt with `ReaderPrompt`:
- the engine's fixed framing;
- the instructions and context, as operator text;
- the issue in an `<issue number="N">` block (`ReaderPrompt::issue`), preceded by a sentence saying who wrote it, with `<issue` and `</issue` inside the forge's text written with `&lt;` as in the direct run;
- the output, as JSON, as the shape of the answer.

It sends one request with no tools, no tool choice and no MCP servers. For Anthropic that is `/v1/messages` with `max_tokens`; for OpenAI it is `/v1/responses` with `max_output_tokens` and `store: false`. The output limit is 1,024 tokens (`READER_OUTPUT_TOKENS`). The request goes through `Models::answer`, the same relay a sandbox's call takes. So the budget hold, the meter, the retries, the provider key and the refusal of tools all apply unchanged. A relay refusal keeps the relay's code (`budget.spent`, `provider.overloaded` and so on). A provider status other than 200 is `reader.provider_refused`.

**Reading the answer.** The broker reads the response body to its end, so the meter charges what the provider reports.

| Check | Code |
| --- | --- |
| The response is at most 1 MiB. | `reader.too_large` |
| It nests at most 16 deep, checked by byte scan before parsing (`forge::depth`). | `reader.too_deep` |
| It is the protocol's JSON with a list of content blocks. | `reader.provider_unreadable` |
| Every block is text. Thinking, and OpenAI's reasoning items, are skipped. Anything else, such as a tool call, is refused, never ignored. | `reader.tool_call` |

The model's text is read with `workflow::read_answer`:

| Check | Code |
| --- | --- |
| Once a fence around the whole answer is removed, the text is at most 16 KiB. | `reader.too_large` |
| It is one JSON object. | `reader.not_json` |
| No key is named twice. | `reader.duplicate_field` |
| It has no field the output does not have. | `reader.unknown_field` |
| It has every field the output has. | `reader.missing_field` |
| Each value is of its field's kind and in range: a JSON boolean; a whole number within `min` and `max` (no fraction, no string); an option spelled exactly. | `reader.bad_value` |

**The engine checks again.** `BrokerProcess::read(job, call, wait)` sends the call and waits up to `wait` for the answer, then checks the values with `workflow::values_fit`: one per field, in the output's name order, each of its field's kind and range. The broker is the less trusted process, so its answer is checked as every message is. Values that do not fit, or any other message in their place, mean a broker that broke the protocol, and the engine closes the channel and stops it (`reader.bad_answer`). Silence for `wait` is `reader.no_answer`, which also stops it.

**A reader's call with tools is recorded.** On a job's socket, when the relay refuses a call with `model.reader_tools`, the broker reports the role gate's `deny` (`Gate::Role`, with the SHA-256 of the request path as the inputs) through the job's reports, once per path, capped as the other gates are (`role.too_many_refusals`). `hyalos run` already records every gate report the broker sends. The role gate existed in the event type and had no reporter until now.

## Consequences

S4.3's acceptance holds:
- A reader's request with tools gets 403 and a `Gate::Role` `deny`.
- A reader's model call and the check of its answer happen in the broker. The engine's frame holds only typed values, and the engine checks them again.
- An answer over a cap, nested too deeply, with an unknown field, or otherwise not the output fails the job, and the broker serves on.

The tests:
- `core/types/src/workflow.rs`: `read_answer` and `values_fit`.
- `core/broker/tests/reader.rs`: both protocols, against a stand-in GitHub holding an injection, every refusal, the job ended after each, and the broker serving on.
- `core/broker/tests/sandbox.rs`: the role gate's reports.
- `core/hyalos/tests/reader_call.rs`: the real broker binary and a local model server.

**The issue's text stays in the broker on this path.** The engine names an issue by number, and the broker reads it and gives it to the model. Only typed values come back. The direct run still reads the issue into the engine (ADR-0052) until S4.6 deletes it.

**A reader's prompt can still be steered.** The issue can tell the model what to answer. The schema bounds what that buys: a bool, a number in range or one of the operator's options, which then taint every step that reads them (ADR-0060). That is the design of ADR-0005, not something this ADR fixes.

**The call blocks the broker's channel**, as an issue read does. A reader's call takes seconds, and up to the relay's retries when the provider is busy. The broker's sockets serve sandboxes on their own threads, so a sandbox of another job is not held up. But other messages from the engine wait behind it, and so do the reports it would send. The runner in S4.6 can choose a `wait`, and can run readers when no other job needs the channel. Moving reader calls to their own thread is a change to the broker if that proves too slow.

**A reader job is still admitted with a manifest**, so its budget, its deadline and its `read` action come from the policy (`mint_manifest`) like any other job's. It gets a socket like any job. Nothing runs in it, since no sandbox is started for a reader.

What this does not decide:
- **The runner that sends `Read` for each reader step** and gives its values to later steps (S4.6, #78). That includes how the engine chooses `protocol` and `model`.
- **Planners.** A planner reads typed values and returns typed values, so the same call could serve it. Whether it does is S4.6's choice.
- **Structured output from the provider.** The request asks for JSON in words. A provider's own JSON-schema mode would make fewer answers fail, but the broker checks every answer in full either way. It can be added to the request without changing what is checked.

**Costs.** About 250 lines of trusted core in the broker, 120 in `hyalos-types` and 40 in the engine. The wire version goes to 6. Engine and broker are one binary, so they always agree on it.
