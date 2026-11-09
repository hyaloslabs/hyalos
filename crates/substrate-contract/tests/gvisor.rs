//! The whole contract, behaviour and isolation, against the container driver under gVisor
//! (ADR-0056). Skips with a message where no runtime has gVisor registered as `hyalos-runsc` and the
//! `ubuntu:24.04` image. Set `HYALOS_REQUIRE_GVISOR=1` (CI does) to make that a failure.

use hyalos_substrate::{ContainerSubstrate, Isolation, SandboxSpec, Substrate};
use hyalos_substrate_contract::{Fixture, run};

type Res = Result<(), Box<dyn std::error::Error>>;

const IMAGE: &str = "ubuntu:24.04";

#[test]
fn the_gvisor_driver_passes_the_whole_contract() -> Res {
    let required = std::env::var_os("HYALOS_REQUIRE_GVISOR").is_some();
    let ready: Vec<_> = ContainerSubstrate::gvisor_candidates()
        .into_iter()
        .map(|d| {
            let r = d.ready(IMAGE);
            (d, r)
        })
        .collect();
    let Some(driver) = ready
        .iter()
        .find(|(_, r)| r.is_ok())
        .map(|(d, _)| d.clone())
    else {
        let why: Vec<String> = ready
            .iter()
            .map(|(d, r)| format!("{}: {}", d.runtime(), r.clone().unwrap_err()))
            .collect();
        assert!(!required, "no runtime runs gVisor here: {why:?}");
        eprintln!("skipped: no runtime runs gVisor here: {why:?}");
        return Ok(());
    };
    assert!(driver.is_gvisor());
    assert_eq!(driver.name(), "gvisor");
    assert_eq!(driver.isolation(), Isolation::UserspaceKernel);
    assert_eq!(
        driver.recorded_as().as_str(),
        format!("gvisor.{}", driver.runtime())
    );
    let work_dir =
        std::env::temp_dir().join(format!("hyalos-contract-gvisor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work_dir);
    std::fs::create_dir_all(&work_dir)?;
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
    println!(
        "runtime: {} under gVisor\n{}",
        driver.runtime(),
        report.summary()
    );
    assert!(report.is_clean(), "{}", report.summary());
    assert!(report.skipped.is_empty(), "{}", report.summary());
    assert_eq!(report.passed.len(), 20);
    Ok(())
}

/// The program sees gVisor's kernel, not the host's: what makes this a stronger boundary than a
/// container on the same runtime.
#[test]
fn a_gvisor_sandbox_runs_on_gvisor_s_kernel_not_the_host_s() -> Res {
    let Some(driver) = ContainerSubstrate::gvisor_candidates()
        .into_iter()
        .find(|d| d.ready(IMAGE).is_ok())
    else {
        assert!(std::env::var_os("HYALOS_REQUIRE_GVISOR").is_none());
        eprintln!("skipped: no runtime runs gVisor here");
        return Ok(());
    };
    let host = std::fs::read_to_string("/proc/sys/kernel/osrelease")?;
    // A socket for the mount; nothing connects to it.
    let dir = std::env::temp_dir().join(format!("hyalos-gvisor-kernel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let socket = dir.join("broker.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket)?;
    let spec = SandboxSpec::new(
        hyalos_types::JobId::from_seq(1),
        IMAGE,
        vec![
            "sh".into(),
            "-c".into(),
            "uname -r; dmesg | head -n 1".into(),
        ],
        socket,
    );
    let mut sandbox = driver.start(&spec)?;
    let out = sandbox.wait();
    let _ = std::fs::remove_dir_all(&dir);
    let text = out.stdout_text();
    assert!(text.contains("-gvisor\n"), "{text}");
    assert!(text.contains("Starting gVisor"), "{text}");
    assert!(!text.contains(host.trim()), "{text}");
    Ok(())
}

#[test]
fn a_runtime_without_gvisor_is_not_ready() {
    let driver = ContainerSubstrate::gvisor("/nonexistent/runtime");
    assert!(driver.ready(IMAGE).is_err());
}
