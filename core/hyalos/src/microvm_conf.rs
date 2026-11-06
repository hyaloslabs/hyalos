//! `microvm.conf`: where the microVM driver finds Firecracker, the guest kernel and the init
//! (ADR-0058). Without the file no job runs in a microVM, and `hyalos doctor` says how to set one up.

use std::path::{Path, PathBuf};

use hyalos_substrate::MicroVmConfig;

/// The file's name in the configuration directory.
pub const FILE: &str = "microvm.conf";

/// Reads `microvm.conf` from `config_dir`, if there is one. Converted images are kept under the data
/// directory and each VM's sockets under the runtime directory.
pub fn load(
    config_dir: &Path,
    data_dir: &Path,
    runtime_dir: &Path,
) -> Result<Option<MicroVmConfig>, String> {
    let path = config_dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    parse(&text, data_dir.join("microvm"), runtime_dir.join("vm"))
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The configuration from the operator's directories, as `hyalos run` and `hyalos doctor` find them.
pub fn from_env(env: crate::paths::Env<'_>) -> Result<Option<MicroVmConfig>, String> {
    load(
        &crate::paths::config_dir(env)?,
        &crate::paths::data_dir(env)?,
        &crate::paths::runtime_dir(env)?,
    )
}

/// One `name = absolute path` per line for `firecracker`, `kernel` and `init`, all required; `#`
/// starts a comment. Anything else is refused by line number.
pub fn parse(text: &str, cache_dir: PathBuf, run_dir: PathBuf) -> Result<MicroVmConfig, String> {
    let (mut firecracker, mut kernel, mut init) = (None, None, None);
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || format!("line {} is not a setting", i + 1);
        let (name, value) = line
            .split_once('=')
            .map(|(n, v)| (n.trim(), v.trim()))
            .ok_or_else(bad)?;
        let slot = match name {
            "firecracker" => &mut firecracker,
            "kernel" => &mut kernel,
            "init" => &mut init,
            _ => return Err(bad()),
        };
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(format!("line {}: {name} must be an absolute path", i + 1));
        }
        if slot.replace(path).is_some() {
            return Err(format!("line {}: {name} is set twice", i + 1));
        }
    }
    let need = |v: Option<PathBuf>, name: &str| v.ok_or_else(|| format!("{name} is not set"));
    Ok(MicroVmConfig {
        firecracker: need(firecracker, "firecracker")?,
        kernel: need(kernel, "kernel")?,
        init: need(init, "init")?,
        cache_dir,
        run_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_absolute_paths_and_nothing_else() {
        let ok = "# the microVM driver\nfirecracker = /opt/fc/firecracker\nkernel=/opt/fc/vmlinux\n\ninit = /opt/fc/hyalos-vm-init\n";
        let c = parse(ok, "/c".into(), "/r".into()).unwrap();
        assert_eq!(c.firecracker, PathBuf::from("/opt/fc/firecracker"));
        assert_eq!(c.kernel, PathBuf::from("/opt/fc/vmlinux"));
        assert_eq!(c.init, PathBuf::from("/opt/fc/hyalos-vm-init"));
        assert_eq!((c.cache_dir, c.run_dir), ("/c".into(), "/r".into()));
        for bad in [
            "firecracker = /a\nkernel = /b",
            "firecracker = fc\nkernel = /b\ninit = /c",
            "firecracker = /a\nkernel = /b\ninit = /c\njailer = /d",
            "firecracker = /a\nfirecracker = /a\nkernel = /b\ninit = /c",
            "firecracker /a",
        ] {
            assert!(parse(bad, "/c".into(), "/r".into()).is_err(), "{bad}");
        }
    }

    #[test]
    fn no_file_is_no_microvm() {
        let dir = std::env::temp_dir().join(format!("hyalos-microvm-conf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir, &dir, &dir), Ok(None));
        std::fs::write(dir.join(FILE), "kernel = /k\n").unwrap();
        assert!(load(&dir, &dir, &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
