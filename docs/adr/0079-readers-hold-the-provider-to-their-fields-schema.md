# ADR-0079: Readers hold the provider to their fields' schema

**Status:** Accepted

## Context

A reader or planner step runs in the broker as one model call with no tools ([ADR-0061](0061-reader-steps-run-in-the-broker.md), [ADR-0062](0062-the-fix-issue-workflow-and-its-runner.md)). The prompt lists the step's fields as the workflow writes them, for example `{"approach":{"choice":["minimal_fix",...]},"add_test":"bool"}`. `workflow::read_answer` then accepts only an object with exactly those fields, each value exactly of its kind. That check is the boundary: no text crosses to the engine, and a choice crosses as an index.

On bot1 (#253, #273), the default step model `gpt-5-mini` failed the planner step with `reader.bad_value` on every run, so every default `hyalos start` failed before the actor. `gpt-5` passed. Hyalos never shows the model's text, so the run could not say which value was wrong. The prompt showed the fields' shape, not an answer, and nothing held the model to it. A model can mirror the shape (`{"approach":{"choice":"docs_only"}}`), quote a bool (`"false"`) or respell an option (`"docs-only"`).

Both providers can constrain a response to a JSON Schema: OpenAI's Responses API with `text.format` (`json_schema`, `strict: true`), and Anthropic's Messages API with `output_config.format` (`json_schema`, no beta header, including the default step model `claude-haiku-5-5`). Anthropic's form refuses numeric constraints such as `minimum` and `maximum`.

## Decision

**Every reader and planner call sends its fields as a JSON Schema the provider must follow.** `workflow::answer_schema(output)` builds it from the step's output alone:
- an object whose fields are all required, with `additionalProperties: false`;
- a choice is a `string` with an `enum` of its options;
- a bool is a `boolean`;
- an integer is an `integer`, its range given in a `description`, since one provider refuses `minimum` and `maximum`.

The broker sends it as `output_config.format` to Anthropic and as `text.format` with `strict: true` to OpenAI. The schema is built from the workflow, which the operator installed, and holds nothing from the issue.

**`read_answer` stays the check.** The schema makes a good answer likely, but the broker still reads every value as before, ranges included. A provider that ignores the schema, or a model name with no structured output, still fails closed with a `reader.*` code.

**The prompt shows the answer's form.** After the fields, `workflow::answer_form(output)` gives the form with a placeholder for each value, such as `{"add_test": <true or false>, "approach": "<one of its options, exactly as written>"}`. It shows no option, so the form steers no choice.

**A refused answer names its field.** When `read_answer` refuses an answer that is a JSON object, the broker prints the first field that is missing or not of its kind on its standard error (`workflow::refused_field`). It prints only the field's name, which the workflow's author wrote, never a value from the answer. The job's ending reason stays the short code.

## Consequences

The default step models no longer fail by misspelling a value. A failure that remains says which field, so the next one can be told apart without seeing the model's text.

A step model must support structured outputs. Current OpenAI and Anthropic models do. An older or third-party model behind a compatible base may refuse the request (`reader.provider_refused`) and needs `--step-model`.

The output budget is unchanged (`READER_OUTPUT_TOKENS`, 1,024). If a reasoning model's reasoning uses it up, the answer is cut off and is refused as `reader.not_json`, not `bad_value`. Raising the budget is left until that is seen.
