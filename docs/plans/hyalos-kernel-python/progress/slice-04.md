# Slice 4 — The triage task runs under Monty

| | |
|---|---|
| **Status** | done |
| **Issue** | #497 |
| **PR** | this PR |
| **Executor model** | Claude Opus 5.5 (`claude-opus-5-5`) |
| **Date** | 2026-11-25 |

## Preflight

Checked on the branch's base, `da9b9c8` (Slice 3 merged).

| Citation | Holds |
|---|---|
| `hyalos-os/audit/src/replay.rs:284-297` the process backend's mapping from a profile to `wasmi` | differs, not a contract: those lines are the linker's `infer` and `effect` bindings; the mapping is `engine` at `:326-340`, and maps every setting the plan says |
| `hyalos-os/kernel-image/src/main.rs:233-246` the kernel's mapping | holds (`config`) |
| `hyalos-os/monitor/src/kvm.rs:29-41` the machine layout and guards; `hyalos-os/kernel-image/src/main.rs:34-35` the heap | holds |
| `hyalos-os/profile/src/lib.rs:134-146` a profile checked against its predecessor | differs, not a contract: those lines pin profile 1's fields; the check against a predecessor is `profile_2_is_profile_1_on_wasmi_2_0_3` at `:159-171` |
| `hyalos-os/monitor/src/main.rs:165` `same_settings` in `on_backend` | holds, at `:167` |
| `hyalos-os/monitor/tests/release_switches.rs:82` `built_kernel` | holds |
| `.github/workflows/ci.yml:284-292` the workspace test with `HYALOS_REQUIRE_KVM` | holds |
| `design.md` §2 Spike profiles, Kernel image spike build, KVM under a spike profile, Python surface (`_hyalos`, Runtime guest `run`) | holds. Slice 3 added no KVM mechanism, since `hyalos.spike/1` runs on the default image, so this slice adds it |
| ADR-007 (`wasmi/deterministic` build-wide under `spike-contract`) | holds: `wasmi` 2.0.3's `deterministic` feature forwards to `wasmi_core/deterministic` |
| Slice 3's `spikes/438/micropython/run.sh`, `evidence/expected.tsv` and `spike_evidence_tests.rs` | holds |

## Acceptance

| ID | How it was shown |
|---|---|
| S-05.1 | Shown on the process backend only: the transcripts, and replay of every record there. The KVM half is a recorded result (S-05.1.e2): the guest is larger than the KVM machine's guest space. The criterion is met by that record |
| S-05.1.e1 | `expected.tsv` case `monty-triage-0`, listed `process`: `python_committed_evidence_replays_on_both_backends` re-runs it under the record's profile, `hyalos.spike/2`, compares the transcript with `evidence/monty-triage-0.json` byte for byte, and replays the record. Case `monty-triage-7` the same way. `run.sh` checks the KVM leg still ends `TooLarge` |
| S-05.1.e2 | `process-only: monty — does not fit KVM: …` in `spikes/438/README.md`, with the sizes (6,944,896 bytes against `GUEST_MAX` 1 MiB) and the command that shows it (`run.sh`'s KVM leg, `refused: TooLarge`) |
| S-05.2 | The two rows below |
| S-05.2.e1 | Case `monty-no-notify` (the `no-notify` contract and SDK, the triage script): `result error AttributeError effects 0`, with the 40 `ask.classify` answers recorded before it (`ok: 43 inputs, 40 inferences, 0 effect`) |
| S-05.2.e2 | Case `monty-raw-effect` (`scripts/raw-effect.py`, which calls `_hyalos.effect` directly, under `no-notify`): `refused UnknownEffect`, no record written. Monty's direct call is MicroPython's, so no `raw-effect-monty.py` was needed. On the process backend only, as S-05.1 |
| S-05.3 | S-05.3.e1 |
| S-05.3.e1 | `spikes/438/monty/typecheck.sh`: Monty's own checker, given the hand-typed `monty/no-notify.pyi` as module `hyalos`, rejects `triage.py` with one error, `unresolved-attribute` on `task.notify` at line 50. `README.md`: `typecheck: rejected — …` |
| S-05.3.e2 | Does not arise: type checking ran. `typecheck.sh` prints `typecheck: unusable: <why>` if the tool cannot build or run |
| 4.1 | `spikes/438/monty/build.sh`: fetches tag `v1.6.0` from `https://github.com/pydantic/monty` (the tag exists), stops unless it is `3f89dcbf82071896efb5e3ded507db4191e8d829`, builds twice from a clean tree at one path with `SOURCE_DATE_EPOCH=0` and `--locked`, compares, and writes `guests/monty.wasm`: `monty.wasm: 6944896 bytes, sha256 31db15c5e91c455f775c550b7f2aa8eee962e58eaa8039c6986a102c98c823f3`, built by rustc 1.99.1 for `wasm32-unknown-unknown`, which `README.md` records. Randomness is the guest's constant `getrandom` backend. The guest's only imports are the four `hyalos` functions (no WASI, no JavaScript) |
| 4.2 | `hyalos.spike/2` in `hyalos-os/profile/src/lib.rs`: floats, `max_memory` 5 MiB, `fuel` 830,000,000, measured as the plan says (README's `monty profile:` line has the figures and the command). `python_4_2_profile_limits_cover_the_fixture` reruns the fixture at seed 0: it ends `ok 40`, one MiB less ends `GuestTrap`, and `fuel` equals twice `fuel_used` rounded up to the next ten million. The KVM mechanism is new here: the kernel image feature `spike-profile-2`, its marker, the monitor's read and `on_backend`'s comparison, `kernel.sh hyalos.spike/2`, and the image helper next to `built_kernel`. `python_4_2_a_spike_profile_runs_only_on_its_own_image` (`release_switches.rs`) shows the default image runs profile 2 and `hyalos.spike/1` and refuses `hyalos.spike/2` with `ReplayDiverged`; the `spike-profile-2` image runs `hyalos.spike/2` and refuses the other two with `ReplayDiverged`; and an image whose marker names `hyalos.spike/9` is refused before boot with a message and no record. `python_4_2_the_image_reports_its_own_profile` holds the kernel's marker to the monitor's and tries every refused marker form. No spike-only layout: see Deviations |
| 4.3 | `python_4_3_a_float_nan_is_canonical_on_both_backends`: an `f32` 0/0 from memory under `hyalos.spike/2` gives the same transcript on both backends, with result bits `0x7fc00000`, and the same guest is refused `InvalidModule` under `hyalos.spike/1` and `hyalos.profile/2`. With `wasmi/deterministic` taken out of the two `spike-contract` features (tried once, then restored), the test fails: the process backend gives `0xffc00000` while the `spike-profile-2` image still gives `0x7fc00000` |
| 4.4 | The workspace tests with `spike-contract` on pass, including `python_s_01_4_e2_profile_2_transcripts_hold` (profile 2's pinned transcripts and fuel, both backends), the profile 1 pinned-transcript tests and `python_1_1_the_hostile_guests_end_as_listed_with_the_feature` (the float guest is still `InvalidModule` under profile 2) |

### Verify, as run

From the repository root, on a host with KVM and Landlock. Every line exited as expected, except
the three `cargo tree` lines, which ran in the inverted form (Deviations):

- `build.sh`: `monty.wasm: 6944896 bytes, sha256 31db15c5…23f3`, `built by: rustc 1.99.1
  (58cf8f9c3 2026-10-13), target wasm32-unknown-unknown`, the same bytes as the first run.
- The `grep` of the hash and of `monty profile: hyalos.spike/2` in `README.md`: found.
- `run.sh`:
  ```
  monty-triage-0 process: result ok 40 effects 1
  monty-triage-7 process: result ok 40 effects 1
  monty-no-notify process: result error AttributeError effects 0
  monty-raw-effect process: refused UnknownEffect
  monty-triage-0 kvm: refused TooLarge (process-only)
  monty: ok
  ```
- The evidence and the guest: unchanged by the re-runs (checked by checksum, since they are new
  files, which `git diff --exit-code` does not see before they are committed).
- `typecheck.sh`: `typecheck: rejected`, with the one diagnostic; exit 0. The `grep` of the
  `typecheck:` line: found.
- Both `cargo fmt --check` lines, the two workspace `clippy` lines, the kernel image's `clippy`
  (default, `selftest`, and also `spike-profile-2`) and the two `attest` builds: clean.
- The workspace tests with the spike features and `HYALOS_REQUIRE_KVM=1 HYALOS_REQUIRE_LANDLOCK=1`:
  `test result: ok` in every binary, `task-kernel`'s unit tests 266 passed. `release_switches`
  without the feature, with KVM and Landlock required: `test result: ok`, 8 passed.
- `grep '^skipped:'` over the log: nothing. The `--list` count: 4.
- `cargo tree … -e features -i wasmi`: `wasmi feature "deterministic"` 0 times for the default
  monitor and the default kernel image, once for the spike monitor build and once for the
  `spike-profile-2` image.
- `cargo xtask lines`, `hyalos-unsafe`, `docs`: ok.

## Deviations

| Plan said | Reality | Why | Contract affected |
|---|---|---|---|
| S-05.1 and 4.2: run on KVM, with a spike-only layout when the default one cannot hold Monty | Monty is listed `process`, recorded as `process-only: monty — does not fit KVM: …`. No spike-only layout was built, and `HEAP`, `STACK` and `GUEST_MAX` are unchanged. The `spike-profile-2` image has the default layout; it runs 4.3's float guest on KVM | The guest is 6,944,896 bytes against `GUEST_MAX` 1 MiB. A layout that held it would still leave the kernel stack overflow of issue #503, which ends any guest loop of about 10,000 iterations in `KernelFault`, and Monty's triage run is about 414 million fuel. The stop condition for a machine that cannot hold the guest applies, a recorded result. That #503 would stop Monty is inferred from Slice 3's finding, not shown on Monty, and the README says so | n |
| 4.1: "Monty's own `Cargo.lock`" | The guest crate is a workspace of its own, so its `Cargo.lock` (`monty/guest/Cargo.lock`) is Monty's, pruned by Cargo to what this target and these features reach. `build.sh` checks that every registry package in it is at the version Monty's own lockfile pins, and builds with `--locked`. The same holds for the type-check tool's lockfile | Monty's lockfile has no entry for the guest crate or the `web-time` stand-in, so `--locked` cannot use it unchanged. Pruning changes no version | n |
| Verify: `cargo tree … -e features \| grep 'wasmi feature "deterministic"'` | Run with `-i wasmi` (the inverted tree), the same filter | The forward feature tree prints each of `wasmi`'s feature nodes once, where `wasmi` first appears, and never prints `deterministic` there: the spike line counted 0 although the feature is on (4.3 fails without it). So the two `!` lines would pass vacuously. The inverted tree lists every enabled `wasmi` feature: 0 for the default monitor and image, 1 for the spike builds | n |
| R2: no edits to Monty beyond build configuration and the host binding | One line of Monty's workspace `Cargo.toml`, changed in the build's copy: `chrono` without its default features. `web-time` is replaced through `[patch.crates-io]` by a clock that never moves (`monty/guest/web-time`). No Rust source of Monty's is edited | `chrono`'s `wasmbind` and `web-time` import JavaScript's clock on this target, which no profile allows. Both changes are build configuration, so the stop condition does not trigger | n |

## Decisions

- **Monty `v1.6.0`**, the tag the plan names (it exists; `v1.7.0` is newer), commit
  `3f89dcbf82071896efb5e3ded507db4191e8d829`, confirmed published at
  `https://github.com/pydantic/monty`. Recorded in `monty/fetch.sh` and `spikes/438/README.md`.
- **Toolchain:** rustc 1.99.1, the repository's pinned toolchain (`rust-toolchain.toml`), through
  rustup, with `wasm32-unknown-unknown`. Monty needs 1.98 or later. Paths are remapped
  (`--remap-path-prefix`), so the bytes do not depend on where the cache or the Cargo home is.
- **The embedding** (`monty/guest/src/lib.rs`): `MontyRun` driven one suspension at a time.
  `import hyalos` is answered with a module file whose source defines `bytearray` and then
  `exec`s record 1's bytes, read through `_hyalos.read(1)`, unchanged. A Monty file module's
  unknown name is a `NameError` the host is never asked about, and Monty has no `bytearray`, so
  the SDK's `isinstance(value, (bytes, bytearray))` needs the name defined. The class has no
  instances and cannot be built, so `isinstance` gives what CPython gives for every value Monty can
  hold. `import _hyalos` is answered with a host module whose four functions come back as
  function calls by name. A `bytes` argument is read from its repr, which Monty writes losslessly
  (`\\`, `\t`, `\n`, `\r`, quotes and `\xNN`): Monty has no other public way to read bytes. Records
  are read through a 16 KiB buffer, the digest guest's. Time is fixed at the epoch, `getrandom`
  returns a constant, and print is disabled.
- **`hyalos.spike/2`'s figures:** peak 5 MiB (4 MiB ends `GuestTrap`), fixture fuel 413,701,352,
  so `fuel` 830,000,000. Seed 7 uses 420,736,426, also under the limit.
- **The marker** is `\0hyalos-kernel-profile=` and the name, then a zero byte. The kernel names it
  in `run` with `black_box`, so the linker keeps it. The default image does not contain it, which
  was checked on the built images.
- **On KVM, `run.sh`** checks that the triage case ends `TooLarge` on the `spike-profile-2` image,
  so a machine that can hold the guest makes it fail.
- **The type-check stub** (`monty/no-notify.pyi`) types `task` as `no-notify.py` builds it:
  `read_inputs` returning issues with `number: int | None` and `title: bytes | None`,
  `ask.classify`, `finish`, and no `notify`.
- **ADR-0137** records the profile, the build-wide deterministic floats, the image feature, the
  marker and its read, and the absent layout. ADR-0136 is accepted and is not edited.

## Discoveries

- **Monty's records equal MicroPython's.** For the triage case at seeds 0 and 7 and the no-notify
  case, the effects, the answers and the results are byte-identical. Monty uses about 12.5 times
  the fuel: 413,701,352 against 33,127,745 at seed 0.
- **Monty's guest is 28 times MicroPython's** (6,944,896 against 249,874 bytes), at
  `opt-level = "s"` with fat LTO; what takes the space was not measured. It needs 5 MiB of memory,
  where MicroPython runs within `hyalos.spike/1`'s 4 MiB.
- **Monty has no `bytearray` and no `globals()`.** Both are named by code a CPython author would
  write: the SDK's `isinstance` check needed the stand-in class, and `exec(source)` with no
  namespace runs in the module's own globals, as in CPython.
- **Monty's type checker works on a spike task.** Given the `no-notify` stub as module `hyalos`,
  it reports exactly the ungranted `notify` and nothing else; with a stub that also types
  `notify.team`, it accepts the same script. A checker that knows the contract could refuse an
  ungranted call before the task is signed, which the generated SDK alone cannot.
- **A default build does not read the marker.** Pointed at a `spike-profile-2` image, a default
  monitor runs profile 2 on an interpreter that allows floats. ADR-0137 records it; the image is
  chosen by path.
