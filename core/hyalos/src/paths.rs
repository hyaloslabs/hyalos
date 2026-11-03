//! Where Hyalos keeps its files (ADR-0018). Linux only (ADR-0011).
//!
//! * data directory, holding `events.db`: `$HYALOS_DATA_DIR`, else `$XDG_DATA_HOME/hyalos`, else
//!   `~/.local/share/hyalos`
//! * config directory, holding `engine.pub`: `$HYALOS_CONFIG_DIR`, else `$XDG_CONFIG_HOME/hyalos`, else
//!   `~/.config/hyalos`
//!
//! An override must be an absolute path. A relative XDG value is ignored, as the XDG specification says.

use std::path::{Path, PathBuf};

pub const LOG_FILE: &str = "events.db";
pub const ENGINE_PUBLIC_KEY_FILE: &str = "engine.pub";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub data: PathBuf,
    pub config: PathBuf,
}

impl Paths {
    pub fn log(&self) -> PathBuf {
        self.data.join(LOG_FILE)
    }

    pub fn engine_public_key(&self) -> PathBuf {
        self.config.join(ENGINE_PUBLIC_KEY_FILE)
    }
}

/// Resolves both directories from the environment. `env` looks a variable up; an empty value counts
/// as unset.
pub fn resolve(env: &dyn Fn(&str) -> Option<String>) -> Result<Paths, String> {
    let get = |name: &str| env(name).filter(|v| !v.is_empty());
    Ok(Paths {
        data: dir(&get, "HYALOS_DATA_DIR", "XDG_DATA_HOME", ".local/share")?,
        config: dir(&get, "HYALOS_CONFIG_DIR", "XDG_CONFIG_HOME", ".config")?,
    })
}

fn dir(
    get: &dyn Fn(&str) -> Option<String>,
    override_var: &str,
    xdg_var: &str,
    home_relative: &str,
) -> Result<PathBuf, String> {
    if let Some(path) = get(override_var) {
        return if Path::new(&path).is_absolute() {
            Ok(PathBuf::from(path))
        } else {
            Err(format!(
                "{override_var} must be an absolute path, not {path:?}"
            ))
        };
    }
    if let Some(base) = get(xdg_var).filter(|b| Path::new(b).is_absolute()) {
        return Ok(Path::new(&base).join("hyalos"));
    }
    match get("HOME").filter(|h| Path::new(h).is_absolute()) {
        Some(home) => Ok(Path::new(&home).join(home_relative).join("hyalos")),
        None => Err(format!(
            "cannot find a home directory; set {override_var} to an absolute path"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let pairs = pairs.to_vec();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn with_only_a_home_the_defaults_are_under_it() {
        let p = resolve(&env(&[("HOME", "/home/o")])).unwrap();
        assert_eq!(p.data, PathBuf::from("/home/o/.local/share/hyalos"));
        assert_eq!(p.config, PathBuf::from("/home/o/.config/hyalos"));
        assert_eq!(
            p.log(),
            PathBuf::from("/home/o/.local/share/hyalos/events.db")
        );
        assert_eq!(
            p.engine_public_key(),
            PathBuf::from("/home/o/.config/hyalos/engine.pub")
        );
    }

    #[test]
    fn xdg_directories_are_used_when_absolute() {
        let p = resolve(&env(&[
            ("HOME", "/home/o"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_CONFIG_HOME", "/c"),
        ]))
        .unwrap();
        assert_eq!(p.data, PathBuf::from("/d/hyalos"));
        assert_eq!(p.config, PathBuf::from("/c/hyalos"));
    }

    #[test]
    fn a_relative_xdg_value_is_ignored() {
        let p = resolve(&env(&[("HOME", "/home/o"), ("XDG_CONFIG_HOME", "rel")])).unwrap();
        assert_eq!(p.config, PathBuf::from("/home/o/.config/hyalos"));
    }

    #[test]
    fn the_hyalos_override_wins_and_is_used_as_given() {
        let p = resolve(&env(&[
            ("HOME", "/home/o"),
            ("XDG_DATA_HOME", "/d"),
            ("HYALOS_DATA_DIR", "/srv/hyalos"),
            ("HYALOS_CONFIG_DIR", "/etc/hy"),
        ]))
        .unwrap();
        assert_eq!(p.data, PathBuf::from("/srv/hyalos"));
        assert_eq!(p.config, PathBuf::from("/etc/hy"));
    }

    #[test]
    fn a_relative_override_is_an_error_not_a_guess() {
        let err = resolve(&env(&[("HOME", "/h"), ("HYALOS_CONFIG_DIR", "here")])).unwrap_err();
        assert!(
            err.contains("HYALOS_CONFIG_DIR must be an absolute path"),
            "{err}"
        );
    }

    #[test]
    fn without_a_home_or_an_override_it_says_what_to_set() {
        let err = resolve(&env(&[])).unwrap_err();
        assert!(err.contains("set HYALOS_DATA_DIR"), "{err}");
        assert!(
            resolve(&env(&[
                ("HYALOS_DATA_DIR", "/a"),
                ("HYALOS_CONFIG_DIR", "/b")
            ]))
            .is_ok()
        );
    }

    #[test]
    fn empty_values_count_as_unset() {
        let p = resolve(&env(&[
            ("HOME", "/h"),
            ("HYALOS_DATA_DIR", ""),
            ("XDG_DATA_HOME", ""),
        ]))
        .unwrap();
        assert_eq!(p.data, PathBuf::from("/h/.local/share/hyalos"));
    }
}
