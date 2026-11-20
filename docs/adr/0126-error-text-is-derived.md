# ADR-0126: Error text is derived

**Status:** Accepted

## Context

Each error enum in the trusted core wrote its text by hand: an `impl fmt::Display` with one match arm per variant, and an empty `impl std::error::Error`. `RuleViolation` alone had 54 arms. The arms were mostly `write!` calls split over several lines by rustfmt, so a one-line message took four or five lines.

The trusted core is at its 30,000-line budget (ADR-0017). Those lines buy nothing a reviewer needs. They also put the message far from the variant it describes.

## Decision

The core derives error text with `thiserror`. Each variant carries its message in an `#[error("…")]` attribute next to its doc comment.

- The text is the same, byte for byte. A message that formats a field (`.path.display()`, `.0.code()`, a join) does it in the attribute. A message that chooses between texts calls a small function that returns the text.
- No new behaviour. No `#[from]`, `#[source]` or `#[error(transparent)]` is added, and no field is named `source`, so `Error::source()` stays `None` and no `From` impl appears. Where a message was a wrapped error's text alone it is `"{0}"`.
- An enum stays hand-written when deriving would change it: one with no `Error` impl today (`CapabilityError`, the log's `Reason`), a generic one (`CompactError`), or one whose text branches on a field in a way a helper would not make shorter (`PolicyUnavailable`, `MintError`, `ChannelError`).
- `thiserror` is in `EXTERNAL_ALLOWED` for `hyalos-types`, `hyalos-log`, `hyalos-policy` and `hyalos-engine` (ADR-0013). Its proc-macro runs at build time; the generated code formats the value and nothing else.

## Consequences

- The trusted core loses about 670 counted lines, and each message sits on its variant.
- The build takes a proc-macro dependency. `thiserror`, `thiserror-impl`, `syn`, `quote` and `proc-macro2` were already in the lockfile through other crates, so the lockfile gains no package.
- Messages stay pinned by the tests that already check them. "Echo nothing" still holds: an attribute can name only the variant's own fields, and no field was added to any message.
- A new error enum in the core uses the derive, unless one of the reasons above to stay hand-written applies.
