//! The proofs about the baseline (S3.6, ADR-0008). They need a `cvc5` executable; without one they
//! skip with a message, unless `HYALOS_REQUIRE_CVC5` is set (CI sets it), which makes that a failure.

use hyalos_policy::BASELINE;
use hyalos_policy_proof::{PROPERTIES, Prover, markdown};

type Res = Result<(), Box<dyn std::error::Error>>;

fn prover() -> Result<Option<Prover>, String> {
    match Prover::new() {
        Ok(p) => Ok(Some(p)),
        Err(why) if std::env::var_os("HYALOS_REQUIRE_CVC5").is_none() => {
            eprintln!("skipped: {why}");
            Ok(None)
        }
        Err(why) => Err(format!("CI requires the solver: {why}")),
    }
}

#[test]
fn every_property_holds_for_every_action() -> Res {
    let Some(mut prover) = prover()? else {
        return Ok(());
    };
    let findings = prover.prove_properties()?;
    // Ten actions, each against every property: none was skipped.
    assert_eq!(findings.len(), 10 * PROPERTIES.len());
    let broken: Vec<String> = findings
        .iter()
        .filter_map(|f| {
            f.counterexample
                .as_ref()
                .map(|c| format!("{} / {}:\n{c}", f.property, f.action))
        })
        .collect();
    assert!(broken.is_empty(), "{}", broken.join("\n---\n"));
    Ok(())
}

#[test]
fn the_baseline_is_not_vacuous_it_allows_something_in_every_action() -> Res {
    let Some(mut prover) = prover()? else {
        return Ok(());
    };
    assert_eq!(prover.actions_that_allow_nothing()?, Vec::<String>::new());
    Ok(())
}

#[test]
fn no_baseline_policy_can_error_on_a_well_formed_request() -> Res {
    let Some(mut prover) = prover()? else {
        return Ok(());
    };
    assert_eq!(
        prover.policies_that_can_error()?,
        Vec::<(String, String)>::new()
    );
    Ok(())
}

#[test]
fn the_properties_fail_for_a_baseline_that_lacks_the_rule() -> Res {
    // The same proof against a baseline with each rule removed in turn must find a counterexample:
    // otherwise the proof would pass whatever the baseline said.
    let Some(mut prover) = prover()? else {
        return Ok(());
    };
    let rules: Vec<&str> = BASELINE
        .split("\n\n")
        .filter(|b| b.contains("@id"))
        .collect();
    assert_eq!(rules.len(), 5);
    for removed in &rules {
        let weaker = BASELINE.replace(removed, "");
        let comparison = prover.compare(BASELINE, &weaker)?;
        assert!(
            !comparison.allows_more.is_empty() && comparison.allows_less.is_empty(),
            "removing\n{removed}\nshould allow more, not less"
        );
    }
    Ok(())
}

#[test]
fn a_change_is_reported_as_allowing_more_less_or_the_same() -> Res {
    let Some(mut prover) = prover()? else {
        return Ok(());
    };
    // Same.
    let same = prover.compare(BASELINE, BASELINE)?;
    assert!(same.unchanged());
    assert!(markdown(&same).contains("exactly what it did before"));
    // Whitespace and comments are not a change.
    let reformatted = format!("// a note\n{}", BASELINE.replace("forbid (", "forbid  ("));
    assert!(prover.compare(BASELINE, &reformatted)?.unchanged());
    // More: a rule removed.
    let rule = BASELINE
        .split("\n\n")
        .find(|b| b.contains("baseline.tainted_no_merge"))
        .ok_or("no such rule")?;
    let more = prover.compare(BASELINE, &BASELINE.replace(rule, ""))?;
    assert!(more.allows_more.iter().any(|d| d.action.contains("merge")));
    assert!(markdown(&more).contains("allows more than before"));
    // Less: a rule added.
    let stricter = format!(
        "{BASELINE}\n@id(\"baseline.no_comments\")\nforbid (principal, action == Action::\"comment\", resource);\n"
    );
    let less = prover.compare(BASELINE, &stricter)?;
    assert!(less.allows_more.is_empty());
    assert!(
        less.allows_less
            .iter()
            .any(|d| d.action.contains("comment"))
    );
    Ok(())
}
