//! The Python guest spike's monitor side (Slice 1 of `docs/plans/hyalos-kernel-python`, ADR-0136):
//! a draft contract in input record 0 that the host holds every call to, on both backends, under
//! `hyalos.spike/1`, and a default build that knows none of it. Each test names the criterion it
//! proves. `HYALOS_REQUIRE_KVM` turns a host without KVM into a failure.

use super::*;
use crate::profile_tests::kvm_backend;
use std::path::Path;

/// SHA-256 and fuel of `task-kernel run`'s transcript file for the committed digest guest on
/// `evidence/fixtures/issues.tsv` under `hyalos.profile/2`, by seed, at this slice's base commit
/// (`6112dd0`). Both backends gave these bytes there (Slice 1, 1.6).
const PROFILE_2_BASE: &[(u64, &str, u64)] = &[
    (
        0,
        "0cb6dd274062e33925d46708d7648d6f9cd5f337440aeb333eb8ca10e189ca2a",
        29845,
    ),
    (
        7,
        "fa92e4576919e2ff968f3ce8849c3069b58c34c54c0627ec1783d4a0126587de",
        30886,
    ),
];

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

fn written(t: &Transcript) -> Vec<u8> {
    serde_json::to_vec_pretty(t).unwrap()
}

/// S-01.4.e2: with or without the feature, the digest guest under `hyalos.profile/2` gives the
/// base commit's transcripts and fuel on both backends.
#[test]
fn python_s_01_4_e2_profile_2_transcripts_hold() {
    let Some(kvm) = kvm_backend("python_s_01_4_e2") else {
        return;
    };
    let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
    let issues = root().join("evidence/fixtures/issues.tsv");
    let inputs = read_inputs(issues.to_str().unwrap()).unwrap();
    for &(seed, base, fuel) in PROFILE_2_BASE {
        for backend in [&Backend::Process, &kvm] {
            let t = execute(backend, &guest, inputs.clone(), Mode::Record { seed }).unwrap();
            assert_eq!(t.profile.as_deref(), Some("hyalos.profile/2"));
            assert_eq!(sha(&written(&t)), base, "seed {seed}");
            assert_eq!(t.fuel_used, fuel, "seed {seed}");
        }
    }
}

/// S-01.3.e1 (Slice 1 of `docs/plans/hyalos-kernel-capsules`): on the kernel image built with
/// `portable-dispatch`, the pinned profile 2 transcripts keep their hash and `fuel_used` on KVM.
#[test]
fn capsules_s_01_3_e1_profile_2_pins_hold_on_the_rebuilt_image() {
    let Some(kvm) = kvm_backend("capsules_s_01_3_e1") else {
        return;
    };
    let guest = fs::read(root().join("guest/digest.wasm")).unwrap();
    let issues = root().join("evidence/fixtures/issues.tsv");
    let inputs = read_inputs(issues.to_str().unwrap()).unwrap();
    for &(seed, base, fuel) in PROFILE_2_BASE {
        let t = execute(&kvm, &guest, inputs.clone(), Mode::Record { seed }).unwrap();
        assert_eq!(t.profile.as_deref(), Some("hyalos.profile/2"));
        assert_eq!(sha(&written(&t)), base, "seed {seed}");
        assert_eq!(t.fuel_used, fuel, "seed {seed}");
    }
}

/// S-01.4.e1: a default build refuses a record naming `hyalos.spike/1` as an unknown profile, and
/// its reason table is the 29 entries it was.
#[cfg(not(feature = "spike-contract"))]
#[test]
fn python_s_01_4_e1_a_default_build_refuses_a_spike_record() {
    let guest = wat::parse_str(with_run("(call $ret (i32.const 0) (i32.const 1))")).unwrap();
    let inputs = vec![br#"{"asks":[0],"may_touch":[0]}"#.to_vec()];
    let t = execute(
        &Backend::Process,
        &guest,
        inputs.clone(),
        Mode::Record { seed: 0 },
    )
    .unwrap();
    let spike = Transcript {
        profile: Some("hyalos.spike/1".into()),
        task: task_v4("hyalos.spike/1", &sha(&guest), &inputs).unwrap(),
        ..t
    };
    assert_eq!(
        read_transcript(&written(&spike)).unwrap_err(),
        "unknown profile hyalos.spike/1"
    );
    assert_eq!(hyalos_profile::lookup("hyalos.spike/1"), None);
    assert_eq!(runner::REASONS.len(), 29);
}

#[cfg(feature = "spike-contract")]
mod with_the_feature {
    use super::*;
    use crate::spike;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    const DRAFT: &[u8] = br#"{"asks":[0],"may_touch":[0]}"#;
    const NO_EFFECT: &[u8] = br#"{"asks":[0],"may_touch":[]}"#;
    const NO_SCHEMA: &[u8] = br#"{"asks":[],"may_touch":[0]}"#;
    const SDK: &[u8] = b"def _nothing():\n    pass\n";
    const SCRIPT: &[u8] = b"from hyalos import task\n";
    const SPIKE: &str = "hyalos.spike/1";

    fn records(contract: &[u8]) -> Vec<Vec<u8>> {
        vec![
            contract.to_vec(),
            SDK.to_vec(),
            SCRIPT.to_vec(),
            b"457\tfix a crash in read".to_vec(),
        ]
    }

    /// The stand-in model, counting every question and effect that reached it.
    #[derive(Clone, Default)]
    struct Seen {
        asked: Arc<AtomicUsize>,
        sent: Arc<AtomicUsize>,
    }

    impl Source for Seen {
        fn infer(&mut self, at: usize, schema: i32, q: &[u8], d: &str) -> Result<Asked, Reason> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            Mode::Record { seed: 0 }.infer(at, schema, q, d)
        }
        fn effect(&mut self, _: usize, _: i32, _: &[u8], _: &str) -> Result<String, Reason> {
            self.sent.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        }
    }

    /// One run under `profile` on one backend, with what reached the source.
    fn under(
        backend: &Backend,
        profile: &str,
        guest: &[u8],
        inputs: Vec<Vec<u8>>,
    ) -> (Result<Vec<u8>, Reason>, usize, usize) {
        let seen = Seen::default();
        let outcome = hyalos_audit::execute_with(
            CURRENT_VERSION,
            Some(profile),
            guest,
            inputs,
            seen.clone(),
            |guest, host, profile| match backend {
                Backend::Process => hyalos_audit::in_process(guest, host, profile),
                Backend::Kvm { kernel, limit } => runner::run(kernel, guest, host, *limit),
            },
        )
        .map(|t| written(&t));
        (
            outcome,
            seen.asked.load(Ordering::SeqCst),
            seen.sent.load(Ordering::SeqCst),
        )
    }

    /// One guest under `hyalos.spike/1` on both backends, which must agree on the outcome and on
    /// what reached the source.
    fn on_both(
        test: &str,
        guest: &[u8],
        inputs: Vec<Vec<u8>>,
    ) -> Option<(Result<Vec<u8>, Reason>, usize, usize)> {
        let kvm = kvm_backend(test)?;
        let process = under(&Backend::Process, SPIKE, guest, inputs.clone());
        let kernel = under(&kvm, SPIKE, guest, inputs);
        assert_eq!(process, kernel, "{test}: the backends differ");
        Some(process)
    }

    fn wasm(body: &str) -> Vec<u8> {
        wat::parse_str(with_run(body)).unwrap()
    }

    const RET: &str = "(call $ret (i32.const 0) (i32.const 1))";
    const ASK: &str = "(drop (call $infer (i32.const 0) (i32.const 0) (i32.const 1)))";
    const SEND: &str = "(drop (call $effect (i32.const 0) (i32.const 0) (i32.const 1)))";

    #[test]
    fn python_s_01_1_e1_an_ungranted_effect_is_refused_and_never_sent() {
        let guest = wasm(&format!("{SEND} {RET}"));
        let Some(outcome) = on_both("python_s_01_1_e1", &guest, records(NO_EFFECT)) else {
            return;
        };
        assert_eq!(outcome, (Err(Reason::UnknownEffect), 0, 0));
    }

    #[test]
    fn python_s_01_1_e2_a_granted_effect_is_recorded() {
        let guest = wasm(&format!("{SEND} {RET}"));
        let Some((outcome, asked, sent)) = on_both("python_s_01_1_e2", &guest, records(DRAFT))
        else {
            return;
        };
        let t = read_transcript(&outcome.unwrap()).unwrap();
        assert_eq!((t.effects.len(), asked, sent), (1, 0, 1));
        assert_eq!(t.profile.as_deref(), Some(SPIKE));
    }

    #[test]
    fn python_s_01_2_e1_an_ungranted_schema_is_refused_and_never_asked() {
        let guest = wasm(&format!("{ASK} {RET}"));
        let Some(outcome) = on_both("python_s_01_2_e1", &guest, records(NO_SCHEMA)) else {
            return;
        };
        assert_eq!(outcome, (Err(Reason::UnknownSchema), 0, 0));
    }

    #[test]
    fn python_s_01_2_e2_an_unknown_schema_is_refused_as_without_a_contract() {
        let guest = wasm(&format!(
            "(drop (call $infer (i32.const 9) (i32.const 0) (i32.const 1))) {RET}"
        ));
        let Some(outcome) = on_both("python_s_01_2_e2", &guest, records(DRAFT)) else {
            return;
        };
        assert_eq!(outcome, (Err(Reason::UnknownSchema), 0, 0));
        let without = under(
            &Backend::Process,
            "hyalos.profile/2",
            &guest,
            records(DRAFT),
        );
        assert_eq!(without, (Err(Reason::UnknownSchema), 0, 0));
    }

    /// A guest that is not a module, so a refusal other than `InvalidModule` came before it loaded.
    const NOT_A_MODULE: &[u8] = b"not a module";

    #[test]
    fn python_s_01_3_e1_a_record_0_that_is_not_a_contract_is_refused_before_the_guest_loads() {
        let inputs = [vec![b"457\tfix a crash in read".to_vec()], records(DRAFT)].concat();
        let Some(outcome) = on_both("python_s_01_3_e1", NOT_A_MODULE, inputs) else {
            return;
        };
        assert_eq!(outcome, (Err(Reason::BadContract), 0, 0));
    }

    #[test]
    fn python_s_01_3_e2_no_input_records_is_refused() {
        let Some(outcome) = on_both("python_s_01_3_e2", NOT_A_MODULE, vec![]) else {
            return;
        };
        assert_eq!(outcome, (Err(Reason::BadContract), 0, 0));
        let guest = wasm(RET);
        assert_eq!(
            under(&Backend::Process, SPIKE, &guest, vec![]),
            (Err(Reason::BadContract), 0, 0)
        );
    }

    #[test]
    fn python_s_01_3_e3_a_contract_with_an_unknown_field_is_refused() {
        let contract = br#"{"asks":[0],"may_touch":[0],"tools":[]}"#;
        let Some(outcome) = on_both("python_s_01_3_e3", NOT_A_MODULE, records(contract)) else {
            return;
        };
        assert_eq!(outcome, (Err(Reason::BadContract), 0, 0));
    }

    /// A run that asks once and sends once under the draft contract, on the process backend.
    fn asks_and_sends() -> (Vec<u8>, Transcript) {
        let guest = wasm(&format!("{ASK} {SEND} {RET}"));
        let t = hyalos_audit::execute_with(
            CURRENT_VERSION,
            Some(SPIKE),
            &guest,
            records(DRAFT),
            Mode::Record { seed: 0 },
            |guest, host, profile| on_backend(&Backend::Process, guest, host, profile),
        )
        .unwrap();
        (guest, t)
    }

    #[test]
    fn python_s_01_5_e1_a_spike_record_replays_on_both_backends() {
        let Some(kvm) = kvm_backend("python_s_01_5_e1") else {
            return;
        };
        let (guest, t) = asks_and_sends();
        let t = read_transcript(&written(&t)).unwrap();
        assert_eq!((t.answers.len(), t.effects.len()), (1, 1));
        for backend in [&Backend::Process, &kvm] {
            assert_eq!(replay(backend, &guest, &t), Ok(()));
        }
    }

    #[test]
    fn python_s_01_5_e2_a_record_whose_contract_drops_its_effect_fails_replay() {
        let Some(kvm) = kvm_backend("python_s_01_5_e2") else {
            return;
        };
        let (guest, mut t) = asks_and_sends();
        t.inputs_hex[0] = hex(NO_EFFECT);
        let inputs: Vec<Vec<u8>> = t.inputs_hex.iter().map(|i| unhex(i).unwrap()).collect();
        t.task = task_v4(SPIKE, &t.guest_sha256, &inputs).unwrap();
        let t = read_transcript(&written(&t)).unwrap();
        for backend in [&Backend::Process, &kvm] {
            assert_eq!(replay(backend, &guest, &t), Err(Reason::UnknownEffect));
        }
    }

    /// A directory of this test's own, removed when it is dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let dir = env::temp_dir().join(format!("hyalos-spike-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Writes a file here and gives its path.
        fn file(&self, name: &str, bytes: &[u8]) -> String {
            let path = self.0.join(name);
            fs::write(&path, bytes).unwrap();
            path.to_str().unwrap().to_string()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A 40-line script, with the newlines a line-by-line loader would split it on.
    fn script_40() -> Vec<u8> {
        (1..=40)
            .map(|n| format!("x{n} = {n}\n"))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn python_s_03_1_e1_each_file_reaches_the_guest_as_one_whole_record() {
        let scratch = Scratch::new("s_03_1_e1");
        let script = script_40();
        let inputs = spike::inputs(
            &scratch.file("contract", DRAFT),
            &scratch.file("sdk.py", SDK),
            &scratch.file("script.py", &script),
            &scratch.file("data.tsv", b"1\tone\n2\ttwo\n3\tthree\n"),
        )
        .unwrap();
        let want: Vec<Vec<u8>> = vec![
            DRAFT.to_vec(),
            SDK.to_vec(),
            script.clone(),
            b"1\tone".to_vec(),
            b"2\ttwo".to_vec(),
            b"3\tthree".to_vec(),
        ];
        assert_eq!(inputs, want);
        // The guest reads record 2 and returns it: every byte of the script, newlines included.
        let echo = wasm(
            "(local $n i32)
             (local.set $n (call $read (i32.const 2) (i32.const 0) (i32.const 4096)))
             (call $ret (i32.const 0) (local.get $n))",
        );
        let Some((outcome, ..)) = on_both("python_s_03_1_e1", &echo, inputs) else {
            return;
        };
        let t = read_transcript(&outcome.unwrap()).unwrap();
        assert_eq!(unhex(&t.result_hex).unwrap(), script);
        assert_eq!(t.inputs_hex.len(), 6);
    }

    #[test]
    fn python_s_03_1_e2_an_empty_script_is_refused_before_the_run() {
        let scratch = Scratch::new("s_03_1_e2");
        let refused = spike::inputs(
            &scratch.file("contract", DRAFT),
            &scratch.file("sdk.py", SDK),
            &scratch.file("script.py", b""),
            &scratch.file("data.tsv", b"1\tone\n"),
        );
        assert_eq!(
            refused.unwrap_err(),
            "the script is empty: a spike run needs one"
        );
    }

    #[test]
    fn python_s_03_2_e1_the_task_is_version_4_framing_over_every_record() {
        let Some((outcome, ..)) = on_both("python_s_03_2_e1", &wasm(RET), records(DRAFT)) else {
            return;
        };
        let t = read_transcript(&outcome.unwrap()).unwrap();
        assert_eq!(t.version, 4);
        assert_eq!(t.profile.as_deref(), Some(SPIKE));
        let guest = wasm(RET);
        assert_eq!(
            t.task,
            task_v4(SPIKE, &sha(&guest), &records(DRAFT)).unwrap()
        );
        assert_ne!(t.task, current_task(&sha(&guest), &records(DRAFT)).unwrap());
    }

    #[test]
    fn python_s_03_3_e1_one_changed_script_byte_is_refused() {
        let (_, t) = asks_and_sends();
        assert!(read_transcript(&written(&t)).is_ok());
        let mut changed = t.clone();
        let mut script = unhex(&changed.inputs_hex[2]).unwrap();
        script[0] ^= 1;
        changed.inputs_hex[2] = hex(&script);
        assert_eq!(
            read_transcript(&written(&changed)).unwrap_err(),
            "this record's task is not the one its guest and inputs give"
        );
    }

    /// 1.1: with the feature, every hostile guest still ends with its listed reason under
    /// `hyalos.profile/2`, on both backends.
    #[test]
    fn python_1_1_the_hostile_guests_end_as_listed_with_the_feature() {
        let Some(kvm) = kvm_backend("python_1_1") else {
            return;
        };
        for (name, source, want) in attacks() {
            let wasm = wat::parse_str(&source).unwrap();
            for backend in [&Backend::Process, &kvm] {
                let got = execute(backend, &wasm, vec![], Mode::Record { seed: 0 }).map(|_| ());
                assert_eq!(got, Err(want), "{name}");
            }
        }
    }

    /// The default spike profile has the default kernel image's settings, so that image runs it, and
    /// `BadContract` is a holder's reason only. A name that is not a spike profile is refused in
    /// `python_1_5_spike_task_equals_record_task` (`tests/release_switches.rs`).
    #[test]
    fn python_the_default_spike_profile_runs_on_the_default_image() {
        assert_eq!(spike::profile().unwrap().name, SPIKE);
        assert!(hyalos_profile::SPIKE_1.same_settings(hyalos_profile::CURRENT));
        assert_eq!(runner::REASONS.last(), Some(&Reason::BadContract));
        assert!(!runner::RUNNER_REASONS.contains(&Reason::BadContract));
    }

    const SPIKE_2: &str = "hyalos.spike/2";

    /// 4.3: an `f32` zero divided by zero, from memory so no translator folds it, gives its NaN's
    /// bits as the result. Under `hyalos.spike/2` both backends give the same transcript, and the
    /// bits are the canonical NaN, `0x7fc00000`: every spike build turns on `wasmi`'s deterministic
    /// float results (ADR-007 of docs/plans/hyalos-kernel-python), without which x86 gives
    /// `0xffc00000`.
    #[test]
    fn python_4_3_a_float_nan_is_canonical_on_both_backends() {
        let guest = wasm(
            "(i32.store (i32.const 0) (i32.reinterpret_f32 \
             (f32.div (f32.load (i32.const 4)) (f32.load (i32.const 4))))) \
             (call $ret (i32.const 0) (i32.const 4))",
        );
        let Some(kvm) = crate::profile_tests::spike_kvm_backend("python_4_3", "spike-profile-2")
        else {
            return;
        };
        let process = under(&Backend::Process, SPIKE_2, &guest, records(DRAFT));
        let kernel = under(&kvm, SPIKE_2, &guest, records(DRAFT));
        assert_eq!(process, kernel, "python_4_3: the backends differ");
        let t = read_transcript(&process.0.unwrap()).unwrap();
        assert_eq!(t.profile.as_deref(), Some(SPIKE_2));
        assert_eq!(unhex(&t.result_hex).unwrap(), 0x7fc0_0000_u32.to_le_bytes());
        // The same guest is refused at load under the profiles that allow no floats.
        for profile in [SPIKE, "hyalos.profile/2"] {
            let refused = under(&Backend::Process, profile, &guest, records(DRAFT)).0;
            assert_eq!(refused, Err(Reason::InvalidModule), "{profile}");
        }
    }

    /// The Monty triage case of `spikes/438/evidence/expected.tsv`, seed 0: its inputs.
    fn monty_triage() -> (Vec<u8>, Vec<Vec<u8>>) {
        let repo = root().join("..");
        let path = |p: &str| repo.join(p).to_str().unwrap().to_string();
        let guest = fs::read(repo.join("spikes/438/guests/monty.wasm")).unwrap();
        let inputs = spike::inputs(
            &path("spikes/438/contracts/draft"),
            &path("spikes/438/sdk/draft.py"),
            &path("spikes/438/scripts/triage.py"),
            &path("hyalos-os/evidence/fixtures/issues.tsv"),
        )
        .unwrap();
        (guest, inputs)
    }

    /// 4.2: the Monty triage guest on the fixture, seed 0, under `hyalos.spike/2` on the process
    /// backend, ends with a result, so `max_memory` holds its peak; one MiB less does not, so it is
    /// the smallest whole number of MiB that does; and the profile's `fuel` is twice the run's
    /// `fuel_used` rounded up to the next ten million.
    #[test]
    fn python_4_2_profile_limits_cover_the_fixture() {
        let (guest, inputs) = monty_triage();
        let profile = hyalos_profile::lookup(SPIKE_2).unwrap();
        let t = hyalos_audit::execute_with(
            CURRENT_VERSION,
            Some(SPIKE_2),
            &guest,
            inputs.clone(),
            Mode::Record { seed: 0 },
            hyalos_audit::in_process,
        )
        .unwrap();
        assert_eq!(unhex(&t.result_hex).unwrap(), b"ok 40");
        assert_eq!(
            profile.fuel,
            (2 * t.fuel_used).div_ceil(10_000_000) * 10_000_000
        );
        assert_eq!(profile.max_memory % (1024 * 1024), 0);
        let smaller: &'static hyalos_profile::Profile =
            Box::leak(Box::new(hyalos_profile::Profile {
                max_memory: profile.max_memory - 1024 * 1024,
                ..*profile
            }));
        let less = hyalos_audit::execute_with(
            CURRENT_VERSION,
            Some(SPIKE_2),
            &guest,
            inputs,
            Mode::Record { seed: 0 },
            |guest, host, _| hyalos_audit::in_process(guest, host, smaller),
        );
        assert_eq!(less.err(), Some(Reason::GuestTrap));
    }

    /// A spike kernel image carries the marker and its profile's name exactly as the monitor reads
    /// them, and the read refuses everything but no marker or one marker naming a known profile.
    #[test]
    fn python_4_2_the_image_reports_its_own_profile() {
        let kernel = include_str!("../../kernel-image/src/main.rs");
        let marker = String::from_utf8(spike::KERNEL_MARKER.to_vec()).unwrap();
        let spike_2 = format!("{marker}hyalos.spike/2\0").replace('\0', "\\0");
        assert!(kernel.contains(&format!("*b\"{spike_2}\"")), "{spike_2}");
        let image = |names: &[&str]| -> Vec<u8> {
            let mut bytes = b"code".to_vec();
            for name in names {
                bytes.extend_from_slice(spike::KERNEL_MARKER);
                bytes.extend_from_slice(name.as_bytes());
                bytes.extend_from_slice(b"\0more");
            }
            bytes
        };
        assert_eq!(spike::image_profile(b"code"), Ok(hyalos_profile::CURRENT));
        assert_eq!(
            spike::image_profile(&image(&["hyalos.spike/2"])),
            Ok(&hyalos_profile::SPIKE_2)
        );
        for refused in [
            image(&["hyalos.spike/9"]),
            image(&["hyalos.spike/2", "hyalos.spike/2"]),
            image(&[""]),
            [spike::KERNEL_MARKER, b"hyalos.spike/2"].concat(),
        ] {
            assert!(spike::image_profile(&refused).is_err());
        }
    }
}
