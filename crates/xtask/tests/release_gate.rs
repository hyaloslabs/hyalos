//! A tag requires the evidence, and green evidence never tags (S8.6, ADR-0013, ADR-0111).
//!
//! These read the workflows and scripts as text, so a change that would break the gate fails here, on
//! the pull request, rather than at the next tag:
//! - every job the evidence script asks for is a job of the ci workflow, under that name;
//! - every job of a release waits for the evidence job, directly or through another;
//! - nothing in a workflow or a script creates a tag, or a release whose tag does not exist already,
//!   except the operator's own tagging script.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

type Res<T = ()> = Result<T, String>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> Res<String> {
    fs::read_to_string(root().join(path)).map_err(|e| format!("{path}: {e}"))
}

/// The job names `release-evidence.sh` requires, from its `EVIDENCE=( ... )` array.
fn evidence_names() -> Res<Vec<String>> {
    let script = read(".github/scripts/release-evidence.sh")?;
    let start = script.find("EVIDENCE=(").ok_or("no EVIDENCE array")?;
    let body = &script[start..];
    let end = body.find("\n)").ok_or("EVIDENCE is not closed")?;
    let names: Vec<String> = body[..end]
        .lines()
        .skip(1)
        .map(|l| l.trim().trim_matches('"').to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(names)
}

/// Each job of a workflow: its key, and the `name:` and `needs:` lines directly under it.
#[derive(Default, Debug)]
struct Job {
    name: Option<String>,
    needs: BTreeSet<String>,
    on_tag: bool,
    /// Every `name:` value anywhere in the job, such as a matrix part's.
    names: BTreeSet<String>,
}

fn jobs(workflow: &str) -> BTreeMap<String, Job> {
    let mut jobs: BTreeMap<String, Job> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut in_jobs = false;
    for line in workflow.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs || line.trim_start().starts_with('#') {
            continue;
        }
        if let Some(key) = line.strip_prefix("  ").and_then(|l| l.strip_suffix(':'))
            && !key.starts_with(' ')
        {
            current = Some(key.to_string());
            jobs.entry(key.to_string()).or_default();
            continue;
        }
        let Some(job) = current.as_ref().and_then(|k| jobs.get_mut(k)) else {
            continue;
        };
        if let Some(name) = line.strip_prefix("    name: ") {
            job.name = Some(name.trim().to_string());
        } else if let Some(needs) = line.strip_prefix("    needs: ") {
            job.needs = needs
                .trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect();
        } else if line.trim() == "if: github.event_name == 'push'" {
            job.on_tag = true;
        }
        if let Some((_, value)) = line
            .trim_start()
            .trim_start_matches("- ")
            .split_once("name: ")
        {
            job.names.insert(value.trim().to_string());
        }
    }
    jobs
}

#[test]
fn every_job_the_evidence_asks_for_is_a_ci_job_by_that_name() -> Res {
    let ci = jobs(&read(".github/workflows/ci.yml")?);
    let names = evidence_names()?;
    assert!(names.len() >= 5, "{names:?}");
    for wanted in &names {
        let found = ci.values().any(|job| {
            let Some(name) = &job.name else { return false };
            if name == wanted {
                return true;
            }
            // A matrix job, `test (${{ matrix.part.name }})`, is named by its part.
            match (name.split_once("${{ matrix."), wanted.split_once(" (")) {
                (Some((prefix, _)), Some((_, part))) => {
                    wanted.starts_with(prefix)
                        && part
                            .strip_suffix(')')
                            .is_some_and(|p| job.names.contains(p))
                }
                _ => false,
            }
        });
        assert!(
            found,
            "the evidence asks for {wanted:?}, which no ci job is named"
        );
    }
    // The red-team corpus, the contract suites and the chaos scenarios run in the two test parts, the
    // simulation and the spec in their own jobs: all are asked for.
    for part in [
        "simulation",
        "job spec",
        "test (the hyalos crate)",
        "test (the rest of the workspace)",
    ] {
        assert!(names.iter().any(|n| n == part), "{part} is not evidence");
    }
    Ok(())
}

/// How many tag-triggered jobs a workflow has, each checked to wait for its evidence job.
fn tagged_jobs_wait_for_the_evidence(release: &BTreeMap<String, Job>) -> Res<usize> {
    let evidence = release.get("evidence").ok_or("no evidence job")?;
    assert!(evidence.needs.is_empty());
    let mut tagged = 0;
    for (key, job) in release {
        if !job.on_tag || key == "evidence" {
            continue;
        }
        tagged += 1;
        // Walk `needs` back to the evidence job.
        let mut seen = BTreeSet::new();
        let mut stack: Vec<&String> = job.needs.iter().collect();
        let mut reaches = false;
        while let Some(n) = stack.pop() {
            if n == "evidence" {
                reaches = true;
                break;
            }
            if seen.insert(n.clone())
                && let Some(j) = release.get(n)
            {
                stack.extend(j.needs.iter());
            }
        }
        assert!(reaches, "release job {key} does not wait for the evidence");
    }
    Ok(tagged)
}

#[test]
fn every_release_job_waits_for_the_evidence() -> Res {
    let release = jobs(&read(".github/workflows/release.yml")?);
    assert!(release.get("evidence").is_some_and(|job| job.on_tag));
    let tagged = tagged_jobs_wait_for_the_evidence(&release)?;
    assert!(tagged >= 3, "{tagged} release jobs");
    Ok(())
}

const VERIFIER: &str = ".github/workflows/release-verifier.yml";

/// The text of one job, from its key to the next job.
fn job_text<'a>(workflow: &'a str, key: &str) -> Res<&'a str> {
    let head = format!("\n  {key}:\n");
    let start = workflow.find(&head).ok_or(format!("no job {key}"))? + 1;
    let rest = &workflow[start + head.len() - 1..];
    let end = rest
        .lines()
        .scan(0, |at, line| {
            let here = *at;
            *at += line.len() + 1;
            Some((here, line))
        })
        .find(|(_, line)| line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        .map_or(rest.len(), |(here, _)| here);
    Ok(&rest[..end])
}

#[test]
fn s_05_1_e1_a_verifier_release_holds_the_binary_its_hash_and_provenance() -> Res {
    let text = read(VERIFIER)?;
    assert!(text.contains("tags: [\"verifier-v*\"]"));
    let build = job_text(&text, "build")?;
    assert!(build.contains("actions/attest-build-provenance@"));
    assert!(build.contains("subject-path: dist/hyalos-verify-x86_64-linux"));
    let attest = build
        .find("Attest the binary")
        .ok_or("no provenance step")?;
    let step = &build[attest..build[attest..].find("subject-path").unwrap() + attest];
    assert!(!step.contains("if:"), "provenance must not be optional");
    assert!(build.contains("grep -qxF \"commit $GITHUB_SHA\""));
    let release = job_text(&text, "release")?;
    assert!(release.contains("dist/hyalos-verify-x86_64-linux dist/SHA256SUMS"));
    assert!(release.contains("sha256sum -c SHA256SUMS"));
    Ok(())
}

#[test]
fn s_05_1_e2_nothing_is_published_without_the_hyalos_os_evidence() -> Res {
    let text = read(VERIFIER)?;
    let workflow = jobs(&text);
    assert_eq!(tagged_jobs_wait_for_the_evidence(&workflow)?, 1);
    assert_eq!(
        workflow
            .iter()
            .filter(|(_, job)| job.on_tag)
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        ["release"]
    );
    assert!(job_text(&text, "evidence")?.contains("release-evidence.sh \"$GITHUB_SHA\" hyalos-os"));
    let ci = jobs(&read(".github/workflows/ci.yml")?);
    assert!(
        ci.values()
            .any(|job| job.name.as_deref() == Some("hyalos-os"))
    );
    Ok(())
}

#[test]
fn s_05_1_the_manual_mode_takes_no_input_and_publishes_nothing() -> Res {
    let text = read(VERIFIER)?;
    assert!(text.contains("\n  workflow_dispatch:\n\npermissions:"));
    assert_eq!(text.matches("gh release").count(), 1);
    let release = job_text(&text, "release")?;
    assert!(release.contains("gh release create"));
    assert!(
        release
            .lines()
            .any(|l| l == "    if: github.event_name == 'push'")
    );
    assert!(text.contains("refs/heads/main"));
    Ok(())
}

#[test]
fn s_05_2_the_adr_documents_both_provenance_checks() -> Res {
    let adr = read("docs/adr/0131-a-verifier-release-is-confirmed-two-ways.md")?;
    let check = "gh attestation verify hyalos-verify-x86_64-linux --repo hyaloslabs/hyalos --signer-workflow hyaloslabs/hyalos/.github/workflows/release-verifier.yml --source-ref";
    assert!(adr.contains(&format!("{check} refs/tags/<tag>")));
    assert!(adr.contains(&format!("{check} refs/heads/main")));
    Ok(())
}

#[test]
fn s_05_5_e1_the_verifier_workflow_creates_no_tag() -> Res {
    let text = read(VERIFIER)?.replace("\\\n", " ");
    let publish: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("gh release create"))
        .collect();
    assert_eq!(publish.len(), 1);
    assert!(publish[0].contains("--verify-tag"));
    for line in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
        assert!(
            !line.contains("git tag") && !line.contains("git push"),
            "{line}"
        );
    }
    Ok(())
}

#[test]
fn s_05_5_e2_a_tag_off_main_or_of_another_version_stops_first() -> Res {
    let text = read(VERIFIER)?;
    let evidence = job_text(&text, "evidence")?;
    assert!(evidence.contains("fetch-depth: 0"));
    assert!(evidence.contains("git merge-base --is-ancestor \"$GITHUB_SHA\" origin/main"));
    assert!(evidence.contains("test \"$TAG\" = \"verifier-v$version\""));
    assert!(evidence.contains("hyalos-os/verify/Cargo.toml"));
    Ok(())
}

#[test]
fn local_6_3_a_verifier_tag_never_starts_the_product_release() -> Res {
    let release = read(".github/workflows/release.yml")?;
    assert!(release.contains("tags: [\"v[0-9]*\"]"));
    assert!(!release.contains("tags: [\"v*\"]"));
    Ok(())
}

#[test]
fn local_6_4_the_evidence_list_defaults_to_the_products() -> Res {
    let script = read(".github/scripts/release-evidence.sh")?;
    assert!(script.contains("if [[ $# -gt 0 ]]; then\n  EVIDENCE=(\"$@\")\nfi"));
    assert!(evidence_names()?.len() >= 5);
    Ok(())
}

#[test]
fn local_6_6_the_verifier_build_is_pinned_and_checked_in_ci() -> Res {
    let build = read(".github/scripts/build-verifier.sh")?;
    let image = |text: &str| -> Option<String> {
        let line = text.lines().find(|l| l.starts_with("builder="))?;
        Some(line.to_string())
    };
    let product = read(".github/scripts/build-release-binary.sh")?;
    assert!(image(&build).is_some_and(|l| l.contains("@sha256:")));
    assert_eq!(image(&build), image(&product));
    assert!(build.contains("musl=\"1.2.3-1\""));
    assert!(build.contains("\"musl-tools=$MUSL\""));
    assert!(build.contains("$PWD:/src:ro"));
    assert!(build.contains("x86_64-unknown-linux-musl"));
    assert!(build.contains("CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS"));
    assert!(build.contains("--remap-path-prefix=/src="));
    // A source tree inside another checkout must not take that checkout's commit.
    assert!(build.contains("git rev-parse --show-toplevel"));
    let ci = read(".github/workflows/ci.yml")?;
    assert!(job_text(&ci, "hyalos-os")?.contains(".github/scripts/check-verifier-rebuild.sh"));
    let check = read(".github/scripts/check-verifier-rebuild.sh")?;
    assert!(check.contains("two builds of $commit differ"));
    assert!(check.contains("a changed source gave the same binary"));
    Ok(())
}

#[test]
fn nothing_but_the_operators_script_creates_a_tag() -> Res {
    let mut files = Vec::new();
    for dir in [".github/workflows", ".github/scripts"] {
        for entry in fs::read_dir(root().join(dir)).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.file_name().and_then(|n| n.to_str()) != Some("tag-release.sh") {
                files.push(path);
            }
        }
    }
    assert!(files.len() > 5);
    for path in files {
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        // Commands span lines with a trailing backslash: read each as one.
        let joined = text.replace("\\\n", " ");
        for line in joined.lines() {
            let l = line.trim();
            if l.starts_with('#') {
                continue;
            }
            let bad = l.contains("git tag ")
                || (l.contains("git push") && (l.contains("tag") || l.contains("refs/")))
                || l.contains("/git/refs")
                || l.contains("/git/tags")
                || (l.contains("gh release create") && !l.contains("--verify-tag"))
                || l.contains("/releases\" -X POST")
                || l.contains("create-release");
            assert!(!bad, "{} creates a tag: {l}", path.display());
        }
    }
    // The operator's script creates one, and pushes nothing.
    let script = read(".github/scripts/tag-release.sh")?;
    assert!(script.contains("git tag -a"));
    assert!(script.contains("release-evidence.sh"));
    for line in script
        .lines()
        .filter(|l| !l.trim_start().starts_with("echo"))
    {
        assert!(!line.contains("git push"), "tag-release.sh pushes: {line}");
    }
    Ok(())
}
