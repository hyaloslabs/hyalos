//! Tests of the review-package check (S-07 and 12.1 of `docs/plans/hyalos-kernel-core`).

use super::*;

const COMMIT: &str = "7a6e6481c0f07939c295f0baa87412d24f16af04";

/// A package that passes: every section, one commit, the required sentence, no claim.
fn package() -> String {
    let mut text = format!("# hyalOS review package\n\nhyalOS has {NOT_REVIEWED}.\n\n");
    for section in SECTIONS {
        let body = if *section == "Commit under review" {
            format!("`{COMMIT}`")
        } else {
            "Some text.".to_string()
        };
        text.push_str(&format!("## {section}\n\n{body}\n\n"));
    }
    text
}

fn check_with(text: &str, ancestor: Result<bool, String>) -> Vec<String> {
    findings(text, |_| ancestor.clone(), |_| None)
}

fn real() -> String {
    fs::read_to_string(crate::repo_root().join(PACKAGE)).unwrap()
}

fn read_real(file: &str) -> Option<String> {
    fs::read_to_string(crate::repo_root().join(file)).ok()
}

fn section<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text
        .find(&format!("\n## {name}\n"))
        .unwrap_or_else(|| panic!("no `## {name}`"));
    let rest = &text[start + name.len() + 5..];
    &rest[..rest.find("\n## ").unwrap_or(rest.len())]
}

#[test]
fn core_s_07_1_the_repository_package_passes() {
    // The ancestor check needs the commit's history, which CI's `lint` job fetches for `cargo xtask
    // review-package`; here it is taken as given, and `core_s_07_1_the_ancestor_check_asks_git`
    // tests the real one.
    assert_eq!(
        findings(&real(), |_| Ok(true), read_real),
        Vec::<String>::new()
    );
}

#[test]
fn core_s_07_1_e1_every_required_section_is_present() {
    let text = real();
    for want in SECTIONS {
        assert!(
            text.lines().any(|l| l == format!("## {want}")),
            "no `## {want}`"
        );
    }
    assert_eq!(check_with(&package(), Ok(true)), Vec::<String>::new());
}

#[test]
fn core_s_07_1_a_missing_empty_or_repeated_section_fails() {
    for want in SECTIONS {
        let text = package().replace(&format!("## {want}\n"), "");
        let found = check_with(&text, Ok(true));
        assert!(
            found
                .iter()
                .any(|f| f.contains(&format!("no `## {want}` section"))),
            "{want}: {found:?}"
        );
    }
    let empty = package().replace("## Limits\n\nSome text.\n", "## Limits\n\n");
    assert!(
        check_with(&empty, Ok(true))
            .iter()
            .any(|f| f.contains("`## Limits` section is empty"))
    );
    let twice = format!("{}## Scope\n\nMore.\n", package());
    assert!(
        check_with(&twice, Ok(true))
            .iter()
            .any(|f| f.contains("`## Scope` appears more than once"))
    );
    // A heading inside a code block is not a section.
    let fenced = package().replace("## Findings\n", "```text\n## Findings\n```\n");
    assert!(
        check_with(&fenced, Ok(true))
            .iter()
            .any(|f| f.contains("no `## Findings` section"))
    );
}

#[test]
fn core_s_07_1_e2_a_package_naming_no_commit_fails() {
    let short = package().replace(COMMIT, &COMMIT[..12]);
    assert!(
        check_with(&short, Ok(true))
            .iter()
            .any(|f| f.contains("names no commit by its full 40-character hash"))
    );
    let none = package().replace(&format!("`{COMMIT}`"), "The base of this pull request.");
    assert!(
        check_with(&none, Ok(true))
            .iter()
            .any(|f| f.contains("names no commit"))
    );
    // A hash in another section is not the commit under review.
    let elsewhere = none.replace("## Scope\n\nSome text.", &format!("## Scope\n\n{COMMIT}"));
    assert!(
        check_with(&elsewhere, Ok(true))
            .iter()
            .any(|f| f.contains("names no commit"))
    );
    // Forty hex digits inside a longer word are not a hash.
    let joined = package().replace(&format!("`{COMMIT}`"), &format!("x{COMMIT}"));
    assert!(
        check_with(&joined, Ok(true))
            .iter()
            .any(|f| f.contains("names no commit"))
    );
}

#[test]
fn core_s_07_1_the_commit_must_be_one_ancestor_of_head() {
    assert!(
        check_with(&package(), Ok(false))
            .iter()
            .any(|f| f.contains("is not an ancestor of HEAD"))
    );
    assert!(
        check_with(&package(), Err("no such commit".into()))
            .iter()
            .any(|f| f.contains("could not check"))
    );
    let other = "0123456789abcdef0123456789abcdef01234567";
    let two = package().replace(&format!("`{COMMIT}`"), &format!("`{COMMIT}` and `{other}`"));
    assert!(
        check_with(&two, Ok(true))
            .iter()
            .any(|f| f.contains("names 2 commits"))
    );
    // The same commit named twice, as the package's own diff command does, is one commit.
    let again = package().replace(&format!("`{COMMIT}`"), &format!("`{COMMIT}`, `{COMMIT}`"));
    assert_eq!(check_with(&again, Ok(true)), Vec::<String>::new());
}

#[test]
fn core_s_07_1_the_ancestor_check_asks_git() {
    let root = crate::repo_root();
    let head = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success());
    let Some(head) = head else {
        assert!(
            std::env::var_os("HYALOS_REQUIRE_GIT").is_none(),
            "HYALOS_REQUIRE_GIT is set and git cannot read this repository"
        );
        eprintln!("skipped: core_s_07_1_the_ancestor_check_asks_git needs git");
        return;
    };
    let head = String::from_utf8(head.stdout).unwrap();
    assert_eq!(is_ancestor(&root, head.trim()), Ok(true));
    // A commit git does not hold fails closed, never as an ancestor.
    assert!(is_ancestor(&root, "0000000000000000000000000000000000000000").is_err());
}

#[test]
fn core_12_1_the_package_says_it_has_not_been_reviewed() {
    assert!(real().contains(NOT_REVIEWED));
    let missing = package().replace(NOT_REVIEWED, "been looked at");
    assert!(
        check_with(&missing, Ok(true))
            .iter()
            .any(|f| f.contains("are missing (ADR-006)"))
    );
}

#[test]
fn core_12_1_a_line_claiming_a_review_fails() {
    for claim in [
        "hyalOS has been independently reviewed by a firm.",
        "The kernel has been reviewed.",
        "The review is complete.",
        "It passed an independent review in 2026.",
        "Both crates have been reviewed and audited.",
        "THE CODE WAS REVIEWED.",
        "An independent audit found nothing.",
        "Security review: complete.",
        "The monitor was audited by a firm.",
        "The kernel has\nbeen reviewed.",
        "It underwent a review.",
    ] {
        let text = package().replace("## Limits\n\nSome text.", &format!("## Limits\n\n{claim}"));
        assert!(
            check_with(&text, Ok(true))
                .iter()
                .any(|f| f.contains("claims a review took place")),
            "{claim}"
        );
    }
    // The required sentence is not itself a claim, even beside other text.
    let fine = package().replace(
        "## Limits\n\nSome text.",
        &format!(
            "## Limits\n\nIt has {NOT_REVIEWED}, and the scope says what a review would cover."
        ),
    );
    assert_eq!(check_with(&fine, Ok(true)), Vec::<String>::new());
}

#[test]
fn core_12_1_the_package_states_every_limit_in_the_design() {
    let design = read_real("docs/plans/hyalos-kernel-core/design.md").unwrap();
    let start = design
        .find("**Limits to state in the package.**")
        .expect("design.md's Modifier: security names the limits");
    let named = &design[start
        ..design[start..]
            .find("\n\n")
            .map_or(design.len(), |e| start + e)];
    let text = real();
    let limits = section(&text, "Limits");
    // Each limit the design names, and the words that state it here.
    for (in_design, in_package) in [
        (
            "KVM and the host kernel are trusted",
            "KVM and the host's Linux kernel are trusted",
        ),
        (
            "Side channels are out of scope",
            "Side channels are out of scope",
        ),
        ("`RDSEED` may be reachable", "`RDSEED`"),
        (
            "TLSNotary's protocol and cryptography are excluded",
            "TLSNotary's protocol and cryptography are excluded",
        ),
        ("`rustls`, `ring` and `rcgen`", "`rustls`"),
        ("`rustls`, `ring` and `rcgen`", "`ring`"),
        ("`rustls`, `ring` and `rcgen`", "`rcgen`"),
        (
            "`forge-ticket` and `canary-test-hooks` features are test-only",
            "`forge-ticket` and `canary-test-hooks` features are test-only",
        ),
        (
            "No `auditor` findings are linked",
            "No `auditor` findings are linked",
        ),
    ] {
        assert!(
            named.contains(in_design),
            "design.md no longer says {in_design:?}"
        );
        assert!(
            limits.contains(in_package),
            "the package's Limits do not say {in_package:?}"
        );
    }
    // ADR-009 of the plan widens the design's `RDSEED` limit to `RDRAND` and `RDPID`.
    for instruction in ["`RDRAND`", "`RDSEED`", "`RDPID`"] {
        assert!(limits.contains(instruction), "{instruction}");
    }
}

#[test]
fn core_12_1_no_line_claims_a_taken_over_kernel_lacks_randomness_everywhere() {
    // Slice 12's stop condition: the words may appear only in a sentence that says hyalOS does not
    // claim them.
    let text = real();
    let mentions: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("no hardware randomness"))
        .collect();
    assert!(!mentions.is_empty());
    for line in mentions {
        assert!(line.contains("does not claim"), "{line}");
    }
}

#[test]
fn core_s_07_3_findings_say_how_to_report_and_how_each_is_closed() {
    let text = real();
    let findings = section(&text, "Findings");
    assert!(findings.contains("SECURITY.md"));
    assert!(findings.contains("Never in a public issue"));
    assert!(findings.contains("cites the fixing commit"), "S-07.3.e1");
    assert!(findings.contains("cites the ADR"), "S-07.3.e2");
}

#[test]
fn core_s_07_1_a_code_reference_that_does_not_resolve_fails() {
    let text = package().replace(
        "## Interface\n\nSome text.",
        "## Interface\n\n`hyalos-os/monitor/src/kvm.rs::allowed_cpuid`, `::no_such_name`, `hyalos-os/gone.rs::x`",
    );
    let found = findings(&text, |_| Ok(true), read_real);
    assert_eq!(
        found,
        [
            format!("{PACKAGE}: hyalos-os/monitor/src/kvm.rs::no_such_name is not in that file"),
            format!("{PACKAGE}: hyalos-os/gone.rs does not exist"),
        ]
    );
}
