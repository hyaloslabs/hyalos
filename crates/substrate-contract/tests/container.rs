//! The whole contract, behaviour and isolation, against the container driver on a real runtime.
//! Skips with a message where no runtime, or no `ubuntu:24.04` image, is present.
//! Set `HYALOS_REQUIRE_CONTAINER=1` (CI does) to make a missing runtime a failure rather than a skip.

use hyalos_substrate::{ContainerSubstrate, Isolation, Substrate};
use hyalos_substrate_contract::{Fixture, run};

type Res = Result<(), Box<dyn std::error::Error>>;

const IMAGE: &str = "ubuntu:24.04";

fn has_image(driver: &ContainerSubstrate) -> bool {
    std::process::Command::new(driver.runtime())
        .args(["image", "inspect", IMAGE])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn the_container_driver_passes_the_whole_contract() -> Res {
    let required = std::env::var_os("HYALOS_REQUIRE_CONTAINER").is_some();
    // The first runtime that works *and* has the image: a machine may have both, with the image in one.
    let Some(driver) = ["podman", "docker"]
        .into_iter()
        .map(ContainerSubstrate::new)
        .find(|d| d.available().is_ok() && has_image(d))
    else {
        assert!(
            !required,
            "no container runtime with {IMAGE} on this machine"
        );
        eprintln!("skipped: no container runtime with {IMAGE} on this machine");
        return Ok(());
    };
    assert_eq!(driver.name(), "container");
    assert_eq!(driver.isolation(), Isolation::Container);
    let work_dir =
        std::env::temp_dir().join(format!("hyalos-contract-container-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir)?;
    // Under /tmp, not target/: the sandbox's user must be able to walk to the socket, and a home
    // directory on a runner is not world-searchable.
    let cleanup = work_dir.clone();
    let report = run(
        &driver,
        &Fixture {
            image: IMAGE.into(),
            missing_image: "hyalos-no-such-image:0".into(),
            work_dir,
            isolation: true,
        },
    );
    let _ = std::fs::remove_dir_all(&cleanup);
    println!("runtime: {}\n{}", driver.runtime(), report.summary());
    assert!(report.is_clean(), "{}", report.summary());
    assert!(report.skipped.is_empty(), "{}", report.summary());
    assert_eq!(report.passed.len(), 18);
    Ok(())
}

#[test]
fn a_runtime_that_is_not_there_is_unavailable_and_starts_nothing() -> Res {
    let driver = ContainerSubstrate::new("/nonexistent/runtime");
    assert!(driver.available().is_err());
    assert!(driver.existing().is_err());
    let spec = hyalos_substrate::SandboxSpec::new(
        hyalos_types::JobId::from_seq(1),
        IMAGE,
        vec!["true".into()],
        "/tmp/none.sock".into(),
    );
    assert!(matches!(
        driver.start(&spec),
        Err(hyalos_substrate::StartError::Unavailable(_))
    ));
    Ok(())
}
