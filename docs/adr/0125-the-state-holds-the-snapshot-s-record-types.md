# ADR-0125: The state holds the snapshot's record types

**Status:** Proposed. Supersedes [ADR-0026](0026-compacting-the-log-by-signed-snapshot.md)'s rule that the engine's conversion names every field, for the plain records only.

## Context

ADR-0026 has the snapshot's `StateImage` mirror the engine's state field for field, and the engine's conversion name every field, "so adding one to the state stops compiling until the image carries it". That rule made a change to the state prompt a change to the image, and so a bump of `STATE_VERSION`.

Eleven of the state's records are plain copies of an image type: the policy in force, the handles released, the merges approved, the plugins installed, the install policy, the updates waiting, the trusted-root snapshot, the revocation list, the plugins revoked, the drafts and the archives pruned. Each was a struct in `core/engine/src/state.rs` with the same fields as its `*Image` in `core/types/src/snapshot.rs`, and `State::to_image` and `State::from_image` copied them one field at a time. That was about 340 lines of the trusted core that said nothing the types did not.

The only difference was the time fields: the image's `at` and `signed_at` were `u64`, the state's `Timestamp`. `Timestamp` is `#[serde(transparent)]` over `u64`, so either writes the same JSON. The records kept by plugin name (plugins, updates, revoked plugins, drafts) also carry the name in the image, as the state's map key does.

## Decision

**The state holds the image's types for the eleven plain records.** Their `at` and `signed_at` fields are `Timestamp` in the image. The engine keeps its names as aliases (`pub type ReleasedHandle = HandleImage;` and the rest), so callers are unchanged. The records kept by name carry their name, as the image does, and are keyed by it. `to_image` and `from_image` copy these records whole. `from_image` still refuses a name that appears twice, and still checks the rebuilt state against its invariants.

**What ADR-0026 says otherwise still holds.** The image is versioned. A change to it bumps `STATE_VERSION` and adds a step. A version this build does not know is refused. Tasks, jobs, holds and agents, and the cancels, resolutions and revocations within them, differ in shape from their images and are still converted field by field, so a field added to one stops compiling until the image carries it.

**A pinned image of every shared record guards the rest.** A field added to a shared record now changes the image's bytes with nothing in the engine to stop it. `core/engine/tests/image_records.rs` replays events that leave at least one of each of the eleven records in a state, writes its image, and compares the bytes with ones pinned from the build before this change. It also checks that none of the eleven is empty, and that the image reads back to the same state. `core/types/tests/snapshot.rs` still pins every version's image, and its version 17 row also holds one of each. A change that moves these bytes bumps the version and adds a row; the pinned bytes are never edited to match.

## Consequences

- The conversion of the plain records is a copy, and the trusted core is smaller by about 340 lines.
- A change to a shared record is caught by a failing pinned test, not by the compiler. A test is weaker than a type error: it relies on the pinned state holding one of every record, which the test asserts.
- A new plain record should be an image type held by the state, with an instance added to the pinned test under a new version.
- The image types now name `Timestamp`, so code that builds an image by hand writes `Timestamp::from_millis`.

Tests: `core/engine/tests/image_records.rs` (`every_shared_record_is_written_exactly_as_pinned`) and `core/types/tests/snapshot.rs` (`the_current_version_writes_exactly_the_pinned_bytes`, `every_released_version_still_reads`).
