//! `hyalos doctor` against the real binary, with stand-in `podman` and `docker` commands on `PATH`
//! (S2.3, ADR-0043). A runtime that answers but cannot start an init is reported and not chosen.

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type Res<T = ()> = Result<T, Box<dyn Error>>;

/// A runtime that answers `version` and has every image. `run` succeeds, or fails as a Podman
/// without catatonit does. Each `run` appends its arguments to `<name>.runs`.
fn runtime(dir: &Path, name: &str, has_init: bool, has_image: bool) -> Res {
    let run = if has_init {
        "exit 0".to_string()
    } else {
        r#"echo 'Error: container-init binary not found on the host: could not find "catatonit" in one of [/usr/libexec/podman]' >&2; exit 125"#
            .to_string()
    };
    let image = if has_image { "exit 0" } else { "exit 1" };
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  version) exit 0 ;;\n  image) {image} ;;\n  run) echo \"$*\" >> \"{runs}\"; {run} ;;\n  *) exit 3 ;;\nesac\n",
        runs = dir.join(format!("{name}.runs")).display()
    );
    let path = dir.join(name);
    std::fs::write(&path, script)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn dir(name: &str) -> Res<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-doctor-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn doctor(path: &Path, args: &[&str]) -> Res<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_hyalos"))
        .arg("doctor")
        .args(args)
        .env_clear()
        .env("PATH", path)
        // Its own directories, where `microvm.conf` would be (ADR-0058).
        .env("HYALOS_CONFIG_DIR", path.join("config"))
        .env("HYALOS_DATA_DIR", path.join("data"))
        .env("HYALOS_RUNTIME_DIR", path.join("run"))
        .output()?)
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn a_podman_without_an_init_is_not_chosen_and_doctor_says_what_to_install() -> Res {
    let d = dir("no-init")?;
    runtime(&d, "podman", false, true)?;
    runtime(&d, "docker", true, true)?;
    let out = doctor(&d, &["--image", "sandbox:1"])?;
    let t = text(&out);
    assert_eq!(out.status.code(), Some(0), "{t}");
    assert!(t.contains("host: linux: ok"), "{t}");
    assert!(
        t.contains("substrate container.podman: not available: podman cannot start a container with an init"),
        "{t}"
    );
    assert!(t.contains("install catatonit"), "{t}");
    assert!(t.contains("substrate container.docker: ready"), "{t}");
    assert!(t.contains("chosen: container.docker"), "{t}");
    assert!(t.contains("spend cap: set a spend limit"), "{t}");
    // Without `microvm.conf` no microVM is asked, and doctor says how to set one up.
    assert!(!t.contains("microvm.firecracker"), "{t}");
    assert!(t.contains("microvm: not set up"), "{t}");
    // The probe is started the way a job's sandbox is: under an init, with no network, never pulling.
    let runs = std::fs::read_to_string(d.join("docker.runs"))?;
    for flag in [
        "--rm",
        "--init",
        "--network none",
        "--pull never",
        "sandbox:1",
    ] {
        assert!(runs.contains(flag), "{flag} missing from {runs}");
    }
    Ok(())
}

#[test]
fn with_no_ready_runtime_doctor_fails() -> Res {
    let d = dir("none")?;
    runtime(&d, "podman", false, true)?;
    runtime(&d, "docker", true, false)?;
    let out = doctor(&d, &[])?;
    let t = text(&out);
    assert_eq!(out.status.code(), Some(1), "{t}");
    assert!(t.contains("image: hyalos-sandbox:latest"), "{t}");
    assert!(
        t.contains("docker has no image hyalos-sandbox:latest"),
        "{t}"
    );
    assert!(t.contains("chosen: none"), "{t}");
    // Without the image there is nothing to probe with.
    assert!(!d.join("docker.runs").exists());
    assert!(t.contains("spend cap"), "{t}");
    Ok(())
}

#[test]
fn with_no_runtime_installed_doctor_fails() -> Res {
    let d = dir("empty")?;
    let out = doctor(&d, &[])?;
    let t = text(&out);
    assert_eq!(out.status.code(), Some(1), "{t}");
    assert!(t.contains("cannot run podman"), "{t}");
    assert!(t.contains("cannot run docker"), "{t}");
    Ok(())
}

/// With `microvm.conf` the microVM driver is asked first, under each runtime, and one that cannot
/// boot is reported and not chosen; a container is chosen instead.
#[test]
fn a_microvm_that_cannot_boot_is_reported_and_not_chosen() -> Res {
    let d = dir("microvm")?;
    runtime(&d, "docker", true, true)?;
    std::fs::create_dir_all(d.join("config"))?;
    std::fs::write(
        d.join("config/microvm.conf"),
        "firecracker = /nonexistent/firecracker\nkernel = /nonexistent/vmlinux\ninit = /nonexistent/init\n",
    )?;
    let out = doctor(&d, &["--image", "sandbox:1"])?;
    let t = text(&out);
    assert_eq!(out.status.code(), Some(0), "{t}");
    assert!(
        t.contains("substrate microvm.firecracker: not available: the kernel /nonexistent/vmlinux is not there"),
        "{t}"
    );
    assert!(t.contains("chosen: container.docker"), "{t}");
    assert!(!t.contains("microvm: not set up"), "{t}");
    // A file that is not three absolute paths is refused, not ignored.
    std::fs::write(d.join("config/microvm.conf"), "kernel = vmlinux\n")?;
    let out = doctor(&d, &[])?;
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("microvm.conf"), "{}", text(&out));
    Ok(())
}

#[test]
fn bad_usage_is_refused() -> Res {
    let d = dir("usage")?;
    for args in [&["--image"][..], &["--images", "x"], &["extra"]] {
        let out = doctor(&d, args)?;
        assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    }
    Ok(())
}
