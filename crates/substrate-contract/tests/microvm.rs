//! The whole contract, behaviour and isolation, against the Firecracker microVM driver (ADR-0058).
//!
//! It needs `/dev/kvm`, Firecracker, a guest kernel, the static init and a container runtime with
//! `ubuntu:24.04`, named by `HYALOS_FIRECRACKER`, `HYALOS_VM_KERNEL` and `HYALOS_VM_INIT`. It skips
//! without them unless `HYALOS_REQUIRE_MICROVM` is set (CI sets it).

use std::path::PathBuf;

use hyalos_substrate::{Isolation, MicroVmConfig, MicroVmSubstrate, SandboxSpec, Substrate};
use hyalos_substrate_contract::{Fixture, run};

type Res = Result<(), Box<dyn std::error::Error>>;

const IMAGE: &str = "ubuntu:24.04";

/// The driver for this machine, with its own cache and run directories under `name`, or `None` to
/// skip.
fn driver(name: &str) -> Result<Option<(MicroVmSubstrate, PathBuf)>, String> {
    let required = std::env::var_os("HYALOS_REQUIRE_MICROVM").is_some();
    let var = |v: &str| std::env::var_os(v).map(PathBuf::from);
    let (Some(firecracker), Some(kernel), Some(init)) = (
        var("HYALOS_FIRECRACKER"),
        var("HYALOS_VM_KERNEL"),
        var("HYALOS_VM_INIT"),
    ) else {
        if required {
            return Err("HYALOS_REQUIRE_MICROVM is set, and HYALOS_FIRECRACKER, HYALOS_VM_KERNEL or HYALOS_VM_INIT is not".into());
        }
        eprintln!("skipped: no Firecracker, kernel or init named");
        return Ok(None);
    };
    // Short, since the VM's Unix sockets live under it; the sandbox's user must reach the broker's.
    let base = std::env::temp_dir().join(format!("hyalos-vm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let config = MicroVmConfig {
        firecracker,
        kernel,
        init,
        cache_dir: std::env::var_os("HYALOS_VM_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.join("cache")),
        run_dir: base.join("run"),
    };
    for runtime in ["docker", "podman"] {
        let d = MicroVmSubstrate::new(config.clone(), runtime);
        match d.ready(IMAGE) {
            Ok(()) => return Ok(Some((d, base))),
            Err(e) => eprintln!("{runtime}: {e}"),
        }
    }
    if required {
        return Err("no runtime with the image could boot a microVM".into());
    }
    eprintln!("skipped: no microVM could boot here");
    Ok(None)
}

#[test]
fn the_microvm_driver_passes_the_whole_contract() -> Res {
    let Some((driver, base)) = driver("contract")? else {
        return Ok(());
    };
    assert_eq!(driver.name(), "microvm");
    assert_eq!(driver.isolation(), Isolation::MicroVm);
    assert_eq!(driver.recorded_as().as_str(), "microvm.firecracker");
    // The probe leaves nothing behind.
    assert!(driver.existing()?.is_empty());
    let work_dir = base.join("work");
    std::fs::create_dir_all(&work_dir)?;
    let report = run(
        &driver,
        &Fixture {
            image: IMAGE.into(),
            missing_image: "hyalos-no-such-image:0".into(),
            work_dir,
            isolation: true,
        },
    );
    let _ = std::fs::remove_dir_all(&base);
    println!(
        "runtime: {} in a microVM\n{}",
        driver.runtime(),
        report.summary()
    );
    assert!(report.is_clean(), "{}", report.summary());
    assert!(report.skipped.is_empty(), "{}", report.summary());
    assert_eq!(report.passed.len(), 22);
    Ok(())
}

/// The program runs on the guest's own kernel, not the host's: what makes this the strongest
/// boundary (ADR-0011).
#[test]
fn a_microvm_runs_its_own_kernel() -> Res {
    let Some((driver, base)) = driver("kernel")? else {
        return Ok(());
    };
    let host = std::fs::read_to_string("/proc/sys/kernel/osrelease")?;
    let dir = base.join("sock");
    std::fs::create_dir_all(&dir)?;
    let socket = dir.join("broker.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket)?;
    let spec = SandboxSpec::new(
        hyalos_types::JobId::from_seq(1),
        IMAGE,
        vec![
            "sh".into(),
            "-c".into(),
            "uname -r; grep -c . /proc/cpuinfo >/dev/null && echo cpus".into(),
        ],
        socket,
    );
    let out = driver.start(&spec)?.wait();
    let _ = std::fs::remove_dir_all(&base);
    let text = out.stdout_text();
    assert!(text.ends_with("cpus\n"), "{text} {:?}", out.stderr);
    assert!(!text.contains(host.trim()), "the host's kernel: {text}");
    Ok(())
}

#[test]
fn a_driver_with_nothing_installed_is_not_available() {
    let d = MicroVmSubstrate::new(
        MicroVmConfig {
            firecracker: "/nonexistent/firecracker".into(),
            kernel: "/nonexistent/vmlinux".into(),
            init: "/nonexistent/init".into(),
            cache_dir: std::env::temp_dir(),
            run_dir: std::env::temp_dir(),
        },
        "docker",
    );
    assert!(d.available().is_err());
    assert!(d.ready(IMAGE).is_err());
}

/// The sandbox image's harnesses start in a microVM, and the forwarder's port reaches the broker's
/// socket on the host over vsock (ADR-0058). It skips without `hyalos-sandbox:test` unless
/// `HYALOS_REQUIRE_SANDBOX_IMAGE` is set.
#[test]
fn the_sandbox_image_reaches_the_broker_from_a_microvm() -> Res {
    const SANDBOX: &str = "hyalos-sandbox:test";
    let Some((driver, base)) = driver("image")? else {
        return Ok(());
    };
    let has_image = std::process::Command::new(driver.runtime())
        .args(["image", "inspect", SANDBOX])
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_image {
        let _ = std::fs::remove_dir_all(&base);
        if std::env::var_os("HYALOS_REQUIRE_SANDBOX_IMAGE").is_some() {
            return Err(format!("no {SANDBOX} image").into());
        }
        eprintln!("skipped: no {SANDBOX} image");
        return Ok(());
    }
    let dir = base.join("sock");
    std::fs::create_dir_all(&dir)?;
    let socket = dir.join("broker.sock");
    // A stand-in broker: one HTTP answer per connection.
    let listener = std::os::unix::net::UnixListener::bind(&socket)?;
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for mut conn in listener.incoming().flatten() {
            let mut request = [0u8; 4096];
            let _ = conn.read(&mut request);
            let _ = conn.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-length: 6\r\nconnection: close\r\n\r\nbroker",
            );
        }
    });
    let mut spec = SandboxSpec::new(
        hyalos_types::JobId::from_seq(1),
        SANDBOX,
        vec![
            "hyalos-forward".into(),
            "sh".into(),
            "-c".into(),
            "codex --version >/dev/null && claude --version >/dev/null && echo harnesses && curl -sS --noproxy '*' http://127.0.0.1:8089/v1/ping".into(),
        ],
        socket,
    );
    spec.memory_bytes = 1024 * 1024 * 1024;
    let out = driver.start(&spec)?.wait();
    let _ = std::fs::remove_dir_all(&base);
    assert_eq!(
        out.ending,
        hyalos_substrate::Ending::Exited(0),
        "{} {}",
        out.stdout_text(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout_text(), "harnesses\nbroker");
    Ok(())
}
