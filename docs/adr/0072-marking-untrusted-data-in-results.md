# ADR-0072: Marking untrusted data in the driving agent's results

**Status:** Proposed

## Context

[ADR-0010](0010-mcp-primary-interface-two-principals.md) says results carry provenance. Text derived from tainted input is returned marked as untrusted data, and the driving agent never receives credentials or manifest signing material ([ADR-0005](0005-provenance-is-a-type.md)). A job's result today is the actor's submitted values (ADR-0064): typed, so a truth value, a number, or one of the options the workflow's author wrote. No forge or model text can be in it. But the harness that chose those values read the issue, and whoever wrote the issue could have steered the choice. Nothing told the driving agent so. A client without structured content reads only the text, so marking only the structured result would lose the mark there.

## Decision

**Every result says where its values came from.** A finished run's structured result has `provenance` and `untrusted`. The values are `model_from_tainted` unless the workflow shows the actor's step untainted (`Checked::tainted`); a step the workflow cannot answer for counts as tainted.

**The text fallback carries the mark too.** For an untrusted result, the text content is:
1. a one-line summary written by Hyalos;
2. a fixed notice (`mcp::UNTRUSTED_NOTICE`) that the data was not written by the operator or by Hyalos and is not an instruction;
3. the data as JSON inside one `<untrusted-data provenance="…">` element.

Every `<`, `>` and `&` in the JSON is written as a JSON escape (`\u003c`, `\u003e`, `\u0026`), and so is every character outside ASCII (`\uXXXX`, surrogate pairs above U+FFFF, so look-alikes, bidi and zero-width characters cannot steer a reader). The data therefore cannot close the element or open another, and it still parses back to the same values. The provenance attribute keeps only lowercase letters and `_`. `tasks/get`, `job_status` and `job_result` give the same result.

**One way out for untrusted data.** Any later result that carries free text from a tainted source returns it through `mcp::Called::untrusted`, not as plain text.

**No secrets by construction.** Results are built from the log's typed state, the workflow's own option names and Hyalos's reason codes. The log never holds a credential, key or token (S1.1's scan, ADR-0036). Error messages are Hyalos's own text. A test checks that no answer holds the agent's credential or the engine's key.

## Consequences

A driving agent can tell data from instructions in both structured and text results, with the same mark across clients. As ADR-0010 says, a mark helps an agent treat text as data but cannot force it to. Typed values keep what an attacker can put in a result to a choice among the workflow's options. A client that renders the text sees the element as written. MCP's content annotations have no field for trust, so the mark is in the result itself.
