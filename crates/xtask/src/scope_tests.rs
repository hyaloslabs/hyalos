//! Tests of the review-scope and advisory checks (S-06 of `docs/plans/hyalos-kernel-core`).

use super::*;

const TLSN_SOURCE: &str = "git+https://github.com/tlsnotary/tlsn?tag=v0.1.0-beta.1#40a069565a2eaecbb5e691dbc634be20e8498daf";

/// A lockfile with one member depending on `wasmi` and `tlsn`, and `wasmi_core` under `wasmi`.
fn lock(wasmi: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"monitor\"\nversion = \"0.1.0\"\ndependencies = [\n \"tlsn\",\n \"wasmi\",\n]\n\n\
         [[package]]\nname = \"tlsn\"\nversion = \"0.1.0-beta.1\"\nsource = \"{TLSN_SOURCE}\"\n\n\
         [[package]]\nname = \"wasmi\"\nversion = \"{wasmi}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"00\"\ndependencies = [\"wasmi_core\"]\n\n\
         [[package]]\nname = \"wasmi_core\"\nversion = \"{wasmi}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
    )
}

fn row(name: &str, version: &str, part: &str, position: &str) -> String {
    format!(
        "[[component]]\nname = \"{name}\"\nversion = \"{version}\"\npart = \"{part}\"\nposition = \"{position}\"\n\
         upstream_audit = \"none found (searched 2026-11-24)\"\nreason = \"a reason\"\n\n"
    )
}

fn tlsn_row(part: &str, position: &str) -> String {
    format!(
        "[[component]]\nname = \"tlsn\"\nversion = \"v0.1.0-beta.1\"\ncommit = \"40a069565a2eaecbb5e691dbc634be20e8498daf\"\n\
         part = \"{part}\"\nposition = \"{position}\"\nupstream_audit = \"none found (searched 2026-11-24)\"\nreason = \"a reason\"\n\n"
    )
}

/// A scope that agrees with `lock("2.0.3")`.
fn scope() -> String {
    [
        row("wasmi", "2.0.3", "use", "covered"),
        row("wasmi", "2.0.3", "internals", "covered"),
        row("wasmi_core", "2.0.3", "internals", "covered"),
        tlsn_row("use", "covered"),
        tlsn_row("internals", "excluded"),
    ]
    .concat()
}

/// The findings on a test scope, leaving out the rows `SPLIT` wants that these small lockfiles do
/// not have.
fn check(scope: &str, lock: &str) -> Vec<String> {
    findings(scope, &[("Cargo.lock".into(), lock.into())])
        .into_iter()
        .filter(|f| !f.contains("(ADR-005 and ADR-008 split it)"))
        .collect()
}

fn real() -> (String, Vec<(String, String)>) {
    read_inputs(&crate::repo_root()).unwrap()
}

#[test]
fn core_s_06_1_the_scope_lists_every_component_the_claim_rests_on() {
    let (scope, _) = real();
    let (rows, errors) = parse_scope(&scope);
    assert_eq!(errors, Vec::<String>::new());
    for name in [
        "wasmi",
        "kvm-ioctls",
        "kvm-bindings",
        "seccompiler",
        "landlock",
        "tlsn",
        "tlsn-core",
        "tlsn-attestation",
        "sha2",
        "p256",
        "k256",
        "rustls",
        "tokio-rustls",
        "ring",
        "rcgen",
    ] {
        let mine: Vec<&Row> = rows.iter().filter(|r| r.get("name") == name).collect();
        assert!(!mine.is_empty(), "{name} has no row");
        for r in mine {
            for key in REQUIRED {
                assert!(!r.get(key).is_empty(), "{name} has no {key}");
            }
        }
    }
}

#[test]
fn core_s_06_1_e1_the_wasmi_row_names_the_audited_versions_and_not_this_one() {
    let (scope, _) = real();
    let (rows, _) = parse_scope(&scope);
    let wasmi: Vec<&Row> = rows.iter().filter(|r| r.get("name") == "wasmi").collect();
    assert_eq!(wasmi.len(), 2);
    for r in wasmi {
        let audit = r.get("upstream_audit");
        let version = r.get("version");
        assert!(
            audit.contains("0.31.0") && audit.contains("0.36.0") && audit.contains("0.38.0"),
            "{audit}"
        );
        assert!(
            audit.contains(&format!("Neither covers {version}")),
            "{audit}"
        );
    }
}

#[test]
fn core_s_06_1_e2_a_row_with_no_reason_fails() {
    let good = scope();
    assert_eq!(check(&good, &lock("2.0.3")), Vec::<String>::new());
    for bad in [
        good.replacen("reason = \"a reason\"\n", "", 1),
        good.replacen("reason = \"a reason\"", "reason = \"  \"", 1),
    ] {
        let found = check(&bad, &lock("2.0.3"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with("wasmi has no `reason`"), "{found:?}");
    }
}

#[test]
fn core_s_06_1_unknown_values_and_unreadable_lines_fail() {
    let good = scope();
    for (bad, want) in [
        (
            good.replacen("part = \"use\"", "part = \"api\"", 1),
            "has part `api`",
        ),
        (
            good.replacen("position = \"covered\"", "position = \"partly\"", 1),
            "has position `partly`",
        ),
        (
            good.replacen("reason = \"a reason\"", "reasons = \"a reason\"", 1),
            "unknown field `reasons`",
        ),
        (
            good.replacen("reason = \"a reason\"", "reason = 'a reason'", 1),
            "not a `[[component]]` table",
        ),
        (
            good.replacen("reason = \"a reason\"", "reason = \"a\"\nreason = \"b\"", 1),
            "`reason` given twice",
        ),
        (
            format!("name = \"x\"\n{good}"),
            "not a `[[component]]` table",
        ),
        (
            good.replacen("none found (searched 2026-11-24)", "none found", 1),
            "no date searched",
        ),
        (
            good.replacen(
                "none found (searched 2026-11-24)",
                "not searched (utility, 2026-11-24)",
                1,
            ),
            "is `not searched` but not `excluded`",
        ),
        (
            format!("{good}{}", row("wasmi", "2.0.3", "use", "covered")),
            "wasmi 2.0.3 has two `use` rows",
        ),
        (String::new(), "no `[[component]]` rows"),
    ] {
        let found = check(&bad, &lock("2.0.3"));
        assert!(found.iter().any(|f| f.contains(want)), "{want}: {found:?}");
    }
    // An excluded utility row may say it was not searched, with the date.
    let utility = good.replace(
        &row("wasmi_core", "2.0.3", "internals", "covered"),
        &row("wasmi_core", "2.0.3", "internals", "excluded").replace(
            "none found (searched 2026-11-24)",
            "not searched (utility, 2026-11-24)",
        ),
    );
    assert_ne!(utility, good);
    assert_eq!(check(&utility, &lock("2.0.3")), Vec::<String>::new());
}

#[test]
fn core_s_06_1_split_components_need_both_rows_and_their_positions() {
    let one = [
        row("wasmi", "2.0.3", "use", "covered"),
        row("wasmi_core", "2.0.3", "internals", "covered"),
        tlsn_row("use", "covered"),
        tlsn_row("internals", "covered"),
    ]
    .concat();
    let found = findings(&one, &[("Cargo.lock".into(), lock("2.0.3"))]);
    assert!(
        found.contains(&"wasmi needs a `internals` row (ADR-005 and ADR-008 split it)".into()),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|f| f.ends_with("tlsn's internals must be `excluded` (ADR-005, ADR-008)")),
        "{found:?}"
    );
}

#[test]
fn core_s_06_2_e1_versions_equal_to_the_scope_pass() {
    assert_eq!(check(&scope(), &lock("2.0.3")), Vec::<String>::new());
}

#[test]
fn core_s_06_2_e2_wasmi_moved_without_a_scope_change_fails() {
    let found = check(&scope(), &lock("2.0.4"));
    assert!(
        found.iter().any(|f| f.contains(
            "wasmi row says 2.0.3, the lockfiles pin 2.0.4; read the row again before changing it"
        )),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("wasmi 2.0.4 is locked but has no row")),
        "{found:?}"
    );
    // The same move in the real lockfiles fails the real scope.
    let (scope, locks) = real();
    let locks: Vec<(String, String)> = locks
        .into_iter()
        .map(|(f, t)| {
            let t = t.replace(
                "name = \"wasmi\"\nversion = \"2.0.3\"",
                "name = \"wasmi\"\nversion = \"2.0.4\"",
            );
            (f, t)
        })
        .collect();
    let found = findings(&scope, &locks);
    assert!(
        found
            .iter()
            .any(|f| f.contains("wasmi row says 2.0.3, the lockfiles pin 2.0.4")),
        "{found:?}"
    );
}

#[test]
fn core_s_06_2_a_moved_tlsn_tag_or_commit_fails() {
    for moved in [
        TLSN_SOURCE.replace("v0.1.0-beta.1", "v0.1.0-beta.2"),
        TLSN_SOURCE.replace("40a0695", "50a0695"),
        TLSN_SOURCE.replace("?tag=v0.1.0-beta.1", "?rev=40a0695"),
    ] {
        let found = check(&scope(), &lock("2.0.3").replace(TLSN_SOURCE, &moved));
        assert!(
            found
                .iter()
                .any(|f| f.contains("tlsn row says tag v0.1.0-beta.1 commit 40a0695")),
            "{moved}: {found:?}"
        );
    }
    // A row with no commit does not match a git pin.
    let found = check(
        &scope().replace(
            "commit = \"40a069565a2eaecbb5e691dbc634be20e8498daf\"\n",
            "",
        ),
        &lock("2.0.3"),
    );
    assert!(
        found
            .iter()
            .any(|f| f.contains("tlsn row says v0.1.0-beta.1, the lockfiles pin tag")),
        "{found:?}"
    );
}

#[test]
fn core_s_06_2_an_unscoped_direct_dependency_fails() {
    let lock = lock("2.0.3").replace(
        " \"wasmi\",\n]",
        " \"wasmi\",\n \"smallvec\",\n]\n\n[[package]]\nname = \"smallvec\"\nversion = \"1.6.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"",
    );
    let found = check(&scope(), &lock);
    assert_eq!(
        found,
        [
            "smallvec 1.6.0 is a direct dependency of a hyalOS crate but has no row; a utility crate gets an `excluded` row with its reason"
        ]
    );
    // An indirect crate needs no row, unless the scope lists it.
    let mut indirect = lock_without_direct_smallvec();
    assert_eq!(check(&scope(), &indirect), Vec::<String>::new());
    indirect.push_str("\n[[package]]\nname = \"wasmi_core\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n");
    let found = check(&scope(), &indirect);
    assert_eq!(
        found,
        [
            "wasmi_core 1.0.0 is locked but has no row; its rows name another version, so read them again"
        ]
    );
}

fn lock_without_direct_smallvec() -> String {
    format!(
        "{}\n[[package]]\nname = \"smallvec\"\nversion = \"1.6.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        lock("2.0.3")
    )
}

#[test]
fn core_s_06_2_a_listed_component_in_no_lockfile_fails() {
    let found = check(
        &format!(
            "{}{}",
            scope(),
            row("kvm-ioctls", "0.25.1", "use", "covered")
        ),
        &lock("2.0.3"),
    );
    assert!(
        found
            .iter()
            .any(|f| f.ends_with("kvm-ioctls is in no hyalOS lockfile")),
        "{found:?}"
    );
}

#[test]
fn core_s_06_2_an_unreadable_lockfile_fails() {
    for bad in [
        lock("2.0.3").replace("version = \"2.0.3\"", "version = 2"),
        lock("2.0.3").replace(" \"tlsn\",", " tlsn,"),
        lock("2.0.3").replace(" \"tlsn\",", " \"nothing\","),
    ] {
        assert!(!check(&scope(), &bad).is_empty(), "{bad}");
    }
}

#[test]
fn core_s_06_2_the_repository_scope_agrees_with_the_lockfiles() {
    let (scope, locks) = real();
    assert_eq!(findings(&scope, &locks), Vec::<String>::new());
}

// ---- advisories ---------------------------------------------------------------------------------

const WASMI_FEED: &str = include_str!("../fixtures/advisories/wasmi-labs-wasmi.json");
const TLSN_FEED: &str = include_str!("../fixtures/advisories/tlsnotary-tlsn.json");

fn feeds(wasmi: &str, tlsn: &str) -> Vec<(String, String)> {
    vec![
        ("wasmi-labs/wasmi".into(), wasmi.into()),
        ("tlsnotary/tlsn".into(), tlsn.into()),
    ]
}

fn locked(wasmi: &str) -> Vec<Locked> {
    let mut l = parse_lock("Cargo.lock", &lock(wasmi)).unwrap();
    l.push(Locked {
        name: "wasmi_ir".into(),
        version: wasmi.into(),
        source: Some("registry+https://github.com/rust-lang/crates.io-index".into()),
        deps: Vec::new(),
    });
    l
}

#[test]
fn core_s_06_3_e1_no_advisory_affects_the_pinned_versions() {
    // The saved feeds of 2026-11-24 against the versions pinned now.
    assert_eq!(
        advisory_findings(&feeds(WASMI_FEED, TLSN_FEED), &locked("2.0.3")),
        Ok(Vec::new())
    );
    let (_, locks) = real();
    let mut all = Vec::new();
    for (f, t) in &locks {
        all.extend(parse_lock(f, t).unwrap());
    }
    assert_eq!(
        advisory_findings(&feeds(WASMI_FEED, TLSN_FEED), &all),
        Ok(Vec::new())
    );
    // The same feed against the real lockfiles with 2.0.2 pinned in two of them names each crate once.
    let old: Vec<Locked> = all
        .into_iter()
        .map(|mut p| {
            if p.name.starts_with("wasmi") {
                p.version = "2.0.2".into();
            }
            p
        })
        .collect();
    let found = advisory_findings(&feeds(WASMI_FEED, TLSN_FEED), &old).unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn core_s_06_3_e2_an_advisory_against_a_pinned_crate_fails_naming_it() {
    let found = advisory_findings(&feeds(WASMI_FEED, TLSN_FEED), &locked("2.0.2")).unwrap();
    assert_eq!(
        found,
        [
            "GHSA-qrr7-2g7f-r9rg (wasmi-labs/wasmi): wasmi 2.0.2 is in its affected range `>= 2.0.0-beta.4, <= 2.0.2`",
            "GHSA-qrr7-2g7f-r9rg (wasmi-labs/wasmi): wasmi_ir 2.0.2 is in its affected range `>= 2.0.0-beta.4, <= 2.0.2`",
        ]
    );
    // A `tlsn` advisory is matched against the pinned tag, its `v` dropped.
    let tlsn = r#"[[{"ghsa_id":"GHSA-test-tlsn-0001","state":"published","withdrawn_at":null,
        "vulnerabilities":[{"package":{"ecosystem":"rust","name":"tlsn"},"vulnerable_version_range":"< 0.1.0-beta.2"}]}]]"#;
    let found = advisory_findings(&feeds(WASMI_FEED, tlsn), &locked("2.0.3")).unwrap();
    assert_eq!(
        found,
        [
            "GHSA-test-tlsn-0001 (tlsnotary/tlsn): tlsn 0.1.0-beta.1 is in its affected range `< 0.1.0-beta.2`"
        ]
    );
    // An advisory naming no package or range is about every version of the repository's crate.
    let bare = r#"[{"ghsa_id":"GHSA-test-tlsn-0002","state":"published","vulnerabilities":[]}]"#;
    let found = advisory_findings(&feeds(WASMI_FEED, bare), &locked("2.0.3")).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("GHSA-test-tlsn-0002"));
}

#[test]
fn core_s_06_3_a_withdrawn_advisory_or_another_ecosystem_is_not_a_finding() {
    let withdrawn = r#"[{"ghsa_id":"GHSA-x","state":"withdrawn","vulnerabilities":[]},
        {"ghsa_id":"GHSA-y","state":"published","withdrawn_at":"2026-01-01T00:00:00Z","vulnerabilities":[]},
        {"ghsa_id":"GHSA-z","state":"published","vulnerabilities":[{"package":{"ecosystem":"npm","name":"tlsn"},"vulnerable_version_range":""}]}]"#;
    assert_eq!(
        advisory_findings(&feeds(WASMI_FEED, withdrawn), &locked("2.0.3")),
        Ok(Vec::new())
    );
}

#[test]
fn core_s_06_3_a_feed_that_cannot_be_read_fails() {
    for (wasmi, want) in [
        ("<html>rate limited</html>", "is not JSON"),
        (
            r#"{"message":"Bad credentials"}"#,
            "not a list of advisories",
        ),
        (r#"[{"state":"published"}]"#, "no `ghsa_id`"),
        (
            r#"[{"ghsa_id":"GHSA-r","vulnerabilities":[{"package":{"name":"wasmi"},"vulnerable_version_range":"~> 2"}]}]"#,
            "cannot read range",
        ),
    ] {
        let err = advisory_findings(&feeds(wasmi, TLSN_FEED), &locked("2.0.3")).unwrap_err();
        assert!(err.contains(want), "{want}: {err}");
    }
    let one = vec![("wasmi-labs/wasmi".to_string(), WASMI_FEED.to_string())];
    assert_eq!(
        advisory_findings(&one, &locked("2.0.3")),
        Err("no feed was read for tlsnotary/tlsn".into())
    );
    let mut other = feeds(WASMI_FEED, TLSN_FEED);
    other.push(("someone/else".into(), "[]".into()));
    assert!(advisory_findings(&other, &locked("2.0.3")).is_err());
}

#[test]
fn core_s_06_3_ranges_follow_semver() {
    for (v, range, want) in [
        ("2.0.2", ">= 2.0.0-beta.4, <= 2.0.2", true),
        ("2.0.3", ">= 2.0.0-beta.4, <= 2.0.2", false),
        ("2.0.0-beta.9", ">= 2.0.0-beta.4, <= 2.0.2", true),
        ("2.0.0-beta.10", ">= 2.0.0-beta.9", true),
        ("2.0.0-beta.3", ">= 2.0.0-beta.4", false),
        ("2.0.0-beta.4", ">= 2.0.0-alpha", true),
        ("1.0.0", ">= 0.41.0 and <= 1.0.0", true),
        ("0.31.0", ">= 0.15.0 and <= 0.31.0", true),
        ("0.31.1", "< 0.32", true),
        ("0.32.0", "< 0.32", false),
        ("v0.1.0-beta.1", "= 0.1.0-beta.1", true),
        ("0.1.0-beta.1", "0.1.0-beta.1", true),
        ("0.1.0", "", true),
        ("2.0.3", "> 2.0.3", false),
    ] {
        assert_eq!(in_range(v, range), Ok(want), "{v} in {range}");
    }
    assert!(in_range("2.0.3", ">= 2.x").is_err());
    assert!(in_range("2.0.3", "~2.0").is_err());
}
