//! Admission, on generated manifests, credentials, live sets and clocks (S8.3, ADR-0004, ADR-0031,
//! ADR-0109).
//!
//! Each case draws a manifest and then, each with its own chance, the ways it can be wrong: signed by
//! another key, its bytes altered, a credential for another job or role, a job that is not live, a
//! clock past its deadline. The broker must admit the manifest exactly when none was drawn, and
//! otherwise refuse with the code of the first check, in the order `Admission::admit` documents.

use std::collections::BTreeSet;

use hyalos_broker::{Admission, Credential, LiveJobs};
use hyalos_props::{ALPHANUMERIC, SeededRng, forall, one_in, pick, seeds, text, within};
use hyalos_types::{
    Budget, Clock, Entropy, JobId, MANIFEST_VERSION, Manifest, ManifestKey, Role, SignedManifest,
    Timestamp,
};

struct At(Timestamp);

impl Clock for At {
    fn now(&self) -> Timestamp {
        self.0
    }
}

struct Live(BTreeSet<JobId>);

impl LiveJobs for Live {
    fn is_live(&self, job: JobId) -> bool {
        self.0.contains(&job)
    }
}

fn key(rng: &mut SeededRng) -> ManifestKey {
    let mut seed = [0u8; 32];
    for chunk in seed.chunks_mut(8) {
        chunk.copy_from_slice(&rng.next_u64().to_le_bytes());
    }
    ManifestKey::from_seed(seed)
}

fn manifest(rng: &mut SeededRng, job: JobId, role: Role, deadline: Timestamp) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        job,
        role,
        tainted: one_in(rng, 2),
        // A marker no refusal may repeat.
        repo: format!("secret-{}/repo", text(rng, ALPHANUMERIC, 12)),
        branch_prefix: "hyalos/".into(),
        pr_base: None,
        actions: ["read".to_string()].into(),
        egress_hosts: BTreeSet::new(),
        tools: BTreeSet::new(),
        data_flows: BTreeSet::new(),
        budget: Budget {
            tokens: within(rng, 0..1_000_000),
        },
        deadline,
        approval: None,
        handles: Default::default(),
        result: None,
        closes: None,
    }
}

fn other_role(rng: &mut SeededRng, not: Role) -> Role {
    let others: Vec<Role> = Role::ALL.into_iter().filter(|r| *r != not).collect();
    *pick(rng, &others)
}

#[test]
fn a_manifest_is_admitted_exactly_when_nothing_is_wrong_with_it_and_else_the_first_check_refuses()
-> Result<(), String> {
    let mut admitted = 0;
    let mut refused: BTreeSet<String> = BTreeSet::new();
    let ran = forall("broker.admission", seeds(1_000)?, |rng| {
        let engine = key(rng);
        let job = JobId::from_seq(within(rng, 1..1_000));
        let role = *pick(rng, &Role::ALL);
        let deadline = Timestamp::from_millis(within(rng, 1_000..1_000_000_000));
        let m = manifest(rng, job, role, deadline);

        // What is wrong, each drawn on its own.
        let other_key = one_in(rng, 6);
        let altered = one_in(rng, 6);
        let wrong_job = one_in(rng, 6);
        let not_live = one_in(rng, 6);
        let wrong_role = one_in(rng, 6);
        let late = one_in(rng, 6);

        let mut signed = if other_key {
            key(rng).sign(&m)
        } else {
            engine.sign(&m)
        }
        .map_err(|e| e.to_string())?;
        if altered {
            let mut payload = signed.payload().to_vec();
            let at = rng.below(payload.len() as u64) as usize;
            payload[at] ^= 1 << rng.below(8);
            signed = SignedManifest::from_parts(signed.version(), payload, *signed.signature());
        }
        let credential = Credential {
            job: if wrong_job {
                JobId::from_seq(job.seq() + within(rng, 1..100))
            } else {
                job
            },
            role: if wrong_role {
                other_role(rng, role)
            } else {
                role
            },
        };
        let live = Live(if not_live {
            BTreeSet::new()
        } else {
            BTreeSet::from([job])
        });
        // On time up to and including the deadline itself.
        let now = if late {
            deadline.as_millis() + within(rng, 1..100_000)
        } else {
            deadline.as_millis() - within(rng, 0..1_000)
        };

        let expected = if other_key || altered {
            Some("manifest.bad_signature")
        } else if wrong_job {
            Some("admission.wrong_job")
        } else if not_live {
            Some("admission.job_not_live")
        } else if wrong_role {
            Some("admission.role_mismatch")
        } else if late {
            Some("admission.expired")
        } else {
            None
        };
        let got = Admission::new(engine.public()).admit(
            &signed,
            &credential,
            &live,
            &At(Timestamp::from_millis(now)),
        );
        match (got, expected) {
            (Ok(opened), None) if opened == m => {
                admitted += 1;
                Ok(())
            }
            (Ok(_), None) => Err("admitted a manifest other than the one signed".into()),
            (Ok(_), Some(want)) => Err(format!("admitted where {want} was due")),
            (Err(refusal), want) => {
                let code = refusal.reason().as_str().to_string();
                if Some(code.as_str()) != want {
                    return Err(format!("refused {code}, expected {want:?}"));
                }
                // A refusal says which check failed, and nothing from inside the document.
                let said = format!("{refusal} {refusal:?}");
                if said.contains(&m.repo) {
                    return Err(format!("the refusal {code} repeats the manifest's repo"));
                }
                refused.insert(code);
                Ok(())
            }
        }
    })?;
    // The cases reached every outcome, so none of them passed by checking nothing.
    if ran >= 1_000 {
        assert!(admitted > 0, "no case was admitted");
        assert_eq!(
            refused,
            [
                "admission.expired",
                "admission.job_not_live",
                "admission.role_mismatch",
                "admission.wrong_job",
                "manifest.bad_signature",
            ]
            .map(String::from)
            .into(),
        );
    }
    Ok(())
}
