//! The behavioural half of the contract, against the fake that runs programs on the host.

use hyalos_substrate_contract::{Fixture, LocalProcessSubstrate, run};

type Res = Result<(), Box<dyn std::error::Error>>;

#[test]
fn the_fake_passes_the_behavioural_contract_and_says_what_it_skipped() -> Res {
    let work_dir =
        std::env::temp_dir().join(format!("hyalos-contract-fake-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir)?;
    // Under /tmp, not target/: the sandbox's user must be able to walk to the socket, and a home
    // directory on a runner is not world-searchable.
    let fake = LocalProcessSubstrate::new("local");
    let cleanup = work_dir.clone();
    let report = run(
        &fake,
        &Fixture {
            image: "local".into(),
            missing_image: "absent".into(),
            work_dir,
            isolation: false,
        },
    );
    let _ = std::fs::remove_dir_all(&cleanup);
    println!("{}", report.summary());
    assert!(report.is_clean(), "{}", report.summary());
    assert_eq!(report.passed.len(), 12);
    assert_eq!(report.skipped.len(), 7);
    Ok(())
}
