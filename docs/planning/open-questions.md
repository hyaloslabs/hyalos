# Open questions for the planning session

These decide whether some ADRs stand as written. Settle them before accepting. When a question is settled, record the answer in the affected ADR (while still Proposed) or in a new ADR, then delete the entry here.

1. **Provider keys in the broker ([ADR-0002](../adr/0002-one-binary-two-processes-sandboxes.md)):** LLM provider keys can't be scoped per job. Is holding them in the broker acceptable, or should LLM calls route broker to engine?
2. **Verifier agent for escalations ([ADR-0012](../adr/0012-humans-approve-only-irreversible-actions.md)):** which escalations, if any, should always reach a human even when the verifier agrees?
3. **Keyless signing and air gaps ([ADR-0007](../adr/0007-capability-based-marketplace-trust.md)):** is the offline bundle enough for air-gapped users, or do they also need a key-based path?
4. **macOS substrate (S2.5, [ADR-0011](../adr/0011-pluggable-substrates.md)):** is Apple's lightweight VM support mature enough for M1, or does macOS start on containers?
