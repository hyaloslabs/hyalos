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

pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// `events.db` in the data directory. Only the data directory is looked up.
pub fn log_file(env: Env<'_>) -> Result<PathBuf, String> {
    Ok(data_dir(env)?.join(LOG_FILE))
}

/// `engine.pub` in the config directory. Only the config directory is looked up, so a command given an
/// explicit key never fails because a directory it does not use cannot be found.
pub fn engine_public_key(env: Env<'_>) -> Result<PathBuf, String> {
    Ok(config_dir(env)?.join(ENGINE_PUBLIC_KEY_FILE))
}

fn data_dir(env: Env<'_>) -> Result<PathBuf, String> {
    dir(env, "HYALOS_DATA_DIR", "XDG_DATA_HOME", ".local/share")
}

fn config_dir(env: Env<'_>) -> Result<PathBuf, String> {
    dir(env, "HYALOS_CONFIG_DIR", "XDG_CONFIG_HOME", ".config")
}

fn dir(
    env: Env<'_>,
    override_var: &str,
    xdg_var: &str,
    home_relative: &str,
) -> Result<PathBuf, String> {
    // An empty value counts as unset.
    let get = |name: &str| env(name).filter(|v| !v.is_empty());
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
        let e = env(&[("HOME", "/home/o")]);
        assert_eq!(
            log_file(&e).unwrap(),
            PathBuf::from("/home/o/.local/share/hyalos/events.db")
        );
        assert_eq!(
            engine_public_key(&e).unwrap(),
            PathBuf::from("/home/o/.config/hyalos/engine.pub")
        );
    }

    #[test]
    fn xdg_directories_are_used_when_absolute() {
        let e = env(&[
            ("HOME", "/home/o"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_CONFIG_HOME", "/c"),
        ]);
        assert_eq!(log_file(&e).unwrap(), PathBuf::from("/d/hyalos/events.db"));
        assert_eq!(
            engine_public_key(&e).unwrap(),
            PathBuf::from("/c/hyalos/engine.pub")
        );
    }

    #[test]
    fn a_relative_xdg_value_is_ignored() {
        let e = env(&[("HOME", "/home/o"), ("XDG_CONFIG_HOME", "rel")]);
        assert_eq!(
            engine_public_key(&e).unwrap(),
            PathBuf::from("/home/o/.config/hyalos/engine.pub")
        );
    }

    #[test]
    fn the_hyalos_override_wins_and_is_used_as_given() {
        let e = env(&[
            ("HOME", "/home/o"),
            ("XDG_DATA_HOME", "/d"),
            ("HYALOS_DATA_DIR", "/srv/hyalos"),
            ("HYALOS_CONFIG_DIR", "/etc/hy"),
        ]);
        assert_eq!(
            log_file(&e).unwrap(),
            PathBuf::from("/srv/hyalos/events.db")
        );
        assert_eq!(
            engine_public_key(&e).unwrap(),
            PathBuf::from("/etc/hy/engine.pub")
        );
    }

    #[test]
    fn a_relative_override_is_an_error_not_a_guess() {
        let e = env(&[("HOME", "/h"), ("HYALOS_CONFIG_DIR", "here")]);
        let err = engine_public_key(&e).unwrap_err();
        assert!(
            err.contains("HYALOS_CONFIG_DIR must be an absolute path"),
            "{err}"
        );
    }

    #[test]
    fn without_a_home_or_an_override_it_says_what_to_set() {
        let e = env(&[]);
        assert!(log_file(&e).unwrap_err().contains("set HYALOS_DATA_DIR"));
        assert!(
            engine_public_key(&e)
                .unwrap_err()
                .contains("set HYALOS_CONFIG_DIR")
        );
    }

    #[test]
    fn each_directory_is_looked_up_on_its_own() {
        // Only the data directory is set: the log path resolves, and the key path is the one that fails.
        let e = env(&[("HYALOS_DATA_DIR", "/a")]);
        assert_eq!(log_file(&e).unwrap(), PathBuf::from("/a/events.db"));
        assert!(engine_public_key(&e).is_err());
    }

    #[test]
    fn empty_values_count_as_unset() {
        let e = env(&[
            ("HOME", "/h"),
            ("HYALOS_DATA_DIR", ""),
            ("XDG_DATA_HOME", ""),
        ]);
        assert_eq!(
            log_file(&e).unwrap(),
            PathBuf::from("/h/.local/share/hyalos/events.db")
        );
    }
}
