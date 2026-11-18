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

#[test]
fn every_release_job_waits_for_the_evidence() -> Res {
    let release = jobs(&read(".github/workflows/release.yml")?);
    let evidence = release
        .get("evidence")
        .ok_or("release.yml has no evidence job")?;
    assert!(evidence.on_tag && evidence.needs.is_empty());
    let mut tagged = 0;
    for (key, job) in &release {
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
    assert!(tagged >= 3, "{tagged} release jobs");
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
