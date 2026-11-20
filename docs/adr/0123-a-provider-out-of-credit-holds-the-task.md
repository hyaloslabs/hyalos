# ADR-0123: A provider out of credit holds the task

**Status:** Proposed. Extends [ADR-0106](0106-a-dependency-lost-mid-job-holds-the-task.md)'s table of lost dependencies with a model provider that says the key's quota or credit is spent.

## Context

On bot1 on 2026-11-20, task 50's OpenAI account ran out of credits mid-job, after 11,712 tokens of a 200,000-token budget (#355). OpenAI started the answer as a 200 stream and then ended it with `response.failed`, code `insufficient_quota`. The stream had no final usage, so the meter charged the hold at the job's whole budget ([ADR-0038](0038-model-calls-from-the-sandbox-and-the-provider-key-file.md), #335). That is right: metering fails closed. But every retry was then refused `budget.spent`, the harness exited 1, and the job ended `sandbox.failed`. The operator was told the budget was spent, when the provider's billing was the cause.

The relay already knew a spent quota in one place: a 429 whose body says `insufficient_quota`, refused `provider.quota_spent` with no retry. It did not know it inside a stream. And in neither case was the loss reported, so the job failed instead of being held.

## Decision

**The relay reads a quota error in a stream.** Each adapter says which stream events mean the key's quota or credit is spent:
- OpenAI: an `error` or `response.failed` event with code `insufficient_quota`.
- Anthropic: an `error` event of type `billing_error`.

The meter still charges such a stream in full. It also marks the job, and from then on every call of that job is refused `provider.quota_spent` (402, "the model provider says this key's quota or credit is spent") before the budget is consulted. Nothing more is sent to the provider. Another job on the same key is not marked, so one that starts after the operator adds credit runs.

**A spent quota is a lost dependency.** The sandbox server reports a refusal `provider.quota_spent`, from the 429 or from the mark, as the `dependency` gate answering `unknown` with code `model.quota_spent`, alongside `provider.overloaded`'s `model.lost`. As for any loss, the job ends `held`, a hold opens that only a person releases, and the task is not completed. The operator adds credit, then retries with `hyalos approve <hold>`, or cancels.

No event, wire message or manifest field changes. The code is a new value of an existing field.

## Consequences

- The operator learns the provider's reason, and the task waits for them instead of failing.
- Metering still fails closed: the job's budget was charged in full and stays charged. A retry after the hold runs as a new job with its own budget, as ADR-0106 has it.
- A provider that reports a spent quota some other way, such as a plain 400, is still not recognised. Each adapter's list grows as such cases are seen.

Tests: `core/broker/tests/model.rs` (`a_provider_out_of_credit_mid_stream_is_named_not_the_budget`, for both protocols) and `core/hyalos/tests/chaos.rs` (`a_provider_out_of_credit_mid_stream_holds_the_task`).
