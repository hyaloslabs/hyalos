//! `hyalos plugin install|list|diff`: installing an Agent Skill and the capability diff an operator
//! reads first (ADR-0006, ADR-0085, ADR-0088, ADR-0089).
//!
//! `install` reads a skill from a directory, prints what it asks for as a diff against what its name
//! holds now, keeps its files in the plugin store by digest, and records the install as the
//! operator's signed decision. A skill that asks for a capability its name does not hold installs
//! only with `--grant`, after the operator has read the diff. `diff` reads capabilities documents and
//! writes nothing.

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use hyalos_engine::Decider;
use hyalos_plugin::index::{INDEX_FILE, Index, MAX_INDEX};
use hyalos_plugin::skill::{MAX_ARCHIVE, Skill, Store, digest_of};
use hyalos_types::capability::Field;
use hyalos_types::{
    Capabilities, Clock, Diff, Grant, IndexLocation, MAX_CAPABILITY_DOCUMENT, PluginName,
    PluginSource, PluginVersion, ReasonCode,
};

use crate::clock::SystemClock;
use crate::mcp_cmd::EngineLog;
use crate::operator;
use crate::passphrase;
use crate::paths::{self, Env};
use crate::plugin_source;

const USAGE: &str = "usage: hyalos plugin install <dir> [--grant] [--reason <code>] [--passphrase-stdin]
       hyalos plugin install --index <location> <name> [--version <v>] [--grant] [--reason <code>]
                             [--passphrase-stdin]
       hyalos plugin list
       hyalos plugin pack <dir> <file>
       hyalos plugin diff [<installed>] <new>
  install reads the Agent Skill in <dir> (SKILL.md, its scripts, and capabilities.json if it asks for
  any), prints what it asks for against what its name holds now, and installs it. If it asks for a
  capability its name does not hold, read the diff and run again with --grant to grant it. It is your
  decision, signed with your approval key: the passphrase is asked for at the terminal, or read from
  stdin with --passphrase-stdin. A job is given it with hyalos run --skill <name>
  With --index, it finds <name> in the plugin index at <location> (an https:// URL whose index.json
  and archives are beside each other, as a release's assets, or a directory holding the same files,
  as an offline bundle), takes its latest version or --version, and installs it only if its archive
  has the digest the index gives (ADR-0090)
  list shows every skill installed, its digest, where it came from and what it was granted
  pack writes the skill in <dir> as one archive to <file>, a new file, and prints its digest, as an
  index lists it
  diff prints what <new> adds to the capabilities in <installed>, what it takes away and what it
  keeps. With one file, it is a first install: everything <new> asks for is added. Both are
  capabilities documents ({\"version\": 1, ...}). It writes nothing";

pub fn run(args: &[String]) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    let result = match args.first().map(String::as_str) {
        Some("install" | "list" | "pack") => manage(args, &env),
        _ => run_with(args),
    };
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err((code, message)) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(code)
        }
    }
}

/// The command's output, or its exit code and message: 1 if a document cannot be read, 2 for usage.
pub fn run_with(args: &[String]) -> Result<String, (u8, String)> {
    let Some((command, files)) = args.split_first() else {
        return Err((2, USAGE.to_string()));
    };
    if command != "diff" {
        return Err((2, format!("unknown plugin command\n{USAGE}")));
    }
    let (installed, new) = match files {
        [new] => (Capabilities::none(), read(new)?),
        [installed, new] => (read(installed)?, read(new)?),
        _ => return Err((2, USAGE.to_string())),
    };
    Ok(render(&installed.diff(&new)))
}

/// `install` and `list`, which read the log and the store. Exit 1 if refused, 2 for usage.
pub fn manage(args: &[String], env: Env<'_>) -> Result<String, (u8, String)> {
    let (args, source_of_passphrase) = passphrase::source(args);
    let usage = |why: &str| (2, format!("{why}\n{USAGE}"));
    let failed = |e: String| (1, e);
    let log = EngineLog {
        log: paths::log_file(env).map_err(failed)?,
        public_key: paths::engine_public_key(env).map_err(failed)?,
    };
    match args.as_slice() {
        [install, rest @ ..] if install == "install" => {
            let (mut grant, mut reason, mut index, mut version, mut target) =
                (false, "plugin.installed", None, None, None);
            let mut rest = rest.iter();
            while let Some(arg) = rest.next() {
                let mut value = || {
                    rest.next()
                        .ok_or_else(|| usage(&format!("{arg} needs a value")))
                };
                match arg.as_str() {
                    "--grant" => grant = true,
                    "--reason" => reason = value()?,
                    "--index" => index = Some(location(value()?).ok_or_else(|| {
                        usage("--index is an https:// URL, an http:// URL on loopback, or a directory")
                    })?),
                    "--version" => {
                        version = Some(PluginVersion::new(value()?).ok_or_else(|| {
                            usage("--version is letters, digits, '.', '+' and '-'")
                        })?)
                    }
                    other if !other.starts_with("--") && target.is_none() => target = Some(other),
                    _ => {
                        return Err(usage(
                            "install takes a directory, or --index <location> and a name, and only --version <v>, --grant and --reason <code>",
                        ));
                    }
                }
            }
            let target =
                target.ok_or_else(|| usage("install needs a directory, or a name with --index"))?;
            let reason = ReasonCode::new(reason)
                .ok_or_else(|| usage("a reason is a short code, such as plugin.installed"))?;
            let (skill, source) = match index {
                None if version.is_some() => {
                    return Err(usage("--version picks a version from --index"));
                }
                None => {
                    let skill = Skill::read(Path::new(target))
                        .map_err(|e| failed(format!("{target}: {e}")))?;
                    (skill, PluginSource::Local)
                }
                Some(location) => {
                    let name = PluginName::new(target).ok_or_else(|| {
                        usage("with --index, install takes a plugin's name: lowercase letters, digits and hyphens")
                    })?;
                    from_index(location, &name, version.as_ref()).map_err(failed)?
                }
            };
            // Everything that can be checked without the passphrase is checked first.
            let held = if log.log.exists() {
                log.read()
                    .map_err(failed)?
                    .plugins()
                    .get(&skill.name)
                    .map(|p| p.grant.clone())
                    .unwrap_or_default()
            } else {
                Grant::none()
            };
            let diff = held.diff(&skill.asks);
            let mut out = format!(
                "skill {} ({} files), digest {}{}\n{}",
                skill.name,
                skill.files.len(),
                skill.digest(),
                from(&source),
                render(&diff)
            );
            if diff.widens() && !grant {
                return Err(failed(format!(
                    "{out}it was not installed: read what it asks for above, and run again with --grant to grant it"
                )));
            }
            // Unlocked before anything is kept, so a wrong passphrase leaves nothing behind.
            let mut engine = operator::engine(env, source_of_passphrase).map_err(failed)?;
            let store = paths::plugin_store(env).map_err(failed)?;
            paths::make_private_dir(&store).map_err(failed)?;
            let digest = Store::new(store)
                .put(&skill)
                .map_err(|e| failed(format!("cannot keep the skill: {e}")))?;
            let head = engine
                .install_plugin(
                    SystemClock.now(),
                    skill.name.clone(),
                    digest,
                    skill.asks.clone(),
                    source,
                    Decider::Human,
                    reason,
                )
                .map_err(|e| failed(e.to_string()))?;
            out.push_str(&format!(
                "installed {} at event {}, signed by you; give it to a job with hyalos run --skill {}\n",
                skill.name, head.seq, skill.name
            ));
            Ok(out)
        }
        [pack, dir, file] if pack == "pack" => {
            let skill = Skill::read(Path::new(dir)).map_err(|e| failed(format!("{dir}: {e}")))?;
            let archive = skill.archive();
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(file)
                .and_then(|mut f| std::io::Write::write_all(&mut f, &archive))
                .map_err(|e| failed(format!("{file}: {e}")))?;
            Ok(format!(
                "wrote {} ({} files, {} bytes), digest {}\n",
                skill.name,
                skill.files.len(),
                archive.len(),
                skill.digest()
            ))
        }
        [list] if list == "list" => {
            if !log.log.exists() {
                return Ok("no skill is installed\n".into());
            }
            let state = log.read().map_err(failed)?;
            if state.plugins().is_empty() {
                return Ok("no skill is installed\n".into());
            }
            let mut out = String::new();
            for (name, p) in state.plugins() {
                out.push_str(&format!(
                    "{name}\t{}\tinstalled at event {}{}\tgranted {}\n",
                    p.digest,
                    p.seq,
                    from(&p.source),
                    summary(&p.grant)
                ));
            }
            Ok(out)
        }
        _ => Err(usage("unknown plugin command")),
    }
}

/// `location` as an index location, a relative directory made absolute.
fn location(text: &str) -> Option<IndexLocation> {
    if text.contains("://") {
        return IndexLocation::new(text);
    }
    let dir = std::path::absolute(text).ok()?;
    IndexLocation::new(dir.to_str()?)
}

/// Resolves `name` in the index at `location`, fetches its archive and checks it has the digest the
/// index gave before reading it (ADR-0090). The index's Sigstore bundle is not verified yet (S7.12),
/// so the install is the operator's alone, as a local one is.
fn from_index(
    location: IndexLocation,
    name: &PluginName,
    version: Option<&PluginVersion>,
) -> Result<(Skill, PluginSource), String> {
    let at = location.as_str();
    let index = plugin_source::fetch(&location, INDEX_FILE, MAX_INDEX)?;
    let index = Index::parse(&index).map_err(|e| format!("{at}: {e} ({})", e.code()))?;
    let entry = index
        .resolve(name, version)
        .map_err(|e| format!("{at}: {e} ({})", e.code()))?;
    let archive = plugin_source::fetch(&location, &entry.artifact, MAX_ARCHIVE)?;
    if digest_of(&archive) != entry.digest {
        return Err(format!(
            "{}: its SHA-256 is not the digest the index gives, so it was not read (plugin.digest_mismatch)",
            entry.artifact
        ));
    }
    let skill = Skill::from_archive(&archive)
        .map_err(|e| format!("{}: {e} ({})", entry.artifact, e.code()))?;
    if skill.name != *name {
        return Err(format!(
            "{}: the skill in it is not named {name} (plugin.wrong_name)",
            entry.artifact
        ));
    }
    let source = PluginSource::Index {
        location,
        version: entry.version.clone(),
    };
    Ok((skill, source))
}

/// Where a plugin came from, for a line about it: nothing for a local install.
fn from(source: &PluginSource) -> String {
    match source {
        PluginSource::Local => String::new(),
        PluginSource::Index { location, version } => format!(", version {version} from {location}"),
    }
}

/// A grant on one line, field by field, or `nothing`.
fn summary(grant: &Grant) -> String {
    let fields: Vec<String> = Field::ALL
        .iter()
        .filter(|f| !grant.field(**f).is_empty())
        .map(|f| {
            let names: Vec<&str> = grant.field(*f).iter().map(String::as_str).collect();
            format!("{}: {}", f.name(), names.join(", "))
        })
        .collect();
    if fields.is_empty() {
        "nothing".into()
    } else {
        fields.join("; ")
    }
}

/// The diff as an operator reads it: a count, then each field with its names marked `+` (added),
/// `-` (removed) or ` ` (unchanged), then whether it needs consent. Every name has passed the
/// vocabulary's check, so none can hold a control character or anything but its field's syntax.
pub fn render(diff: &Diff) -> String {
    let count = |c: &Capabilities| Field::ALL.iter().map(|f| c.field(*f).len()).sum::<usize>();
    let mut out = format!(
        "capabilities: {} added, {} removed, {} unchanged\n",
        count(&diff.added),
        count(&diff.removed),
        count(&diff.unchanged)
    );
    for field in Field::ALL {
        out.push_str(field.name());
        out.push('\n');
        let mut lines: Vec<(&String, char)> = Vec::new();
        for (set, mark) in [
            (&diff.added, '+'),
            (&diff.removed, '-'),
            (&diff.unchanged, ' '),
        ] {
            lines.extend(set.field(field).iter().map(|name| (name, mark)));
        }
        if lines.is_empty() {
            out.push_str("    (none)\n");
        }
        lines.sort();
        for (name, mark) in lines {
            out.push_str(&format!("  {mark} {name}\n"));
        }
    }
    out.push_str(if diff.widens() {
        "it adds capabilities: a human must grant each one before it installs (ADR-0012)\n"
    } else {
        "it adds no capability\n"
    });
    out
}

fn read(file: &str) -> Result<Capabilities, (u8, String)> {
    let fail = |e: String| (1, e);
    let handle = std::fs::File::open(file).map_err(|e| fail(format!("cannot read {file}: {e}")))?;
    let mut bytes = Vec::new();
    handle
        .take(MAX_CAPABILITY_DOCUMENT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| fail(format!("cannot read {file}: {e}")))?;
    // The error names the field, never what the document said (ADR-0085).
    Capabilities::from_document(&bytes).map_err(|e| fail(format!("{file}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hyalos-plugin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file(dir: &std::path::Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path.display().to_string()
    }

    #[test]
    fn an_update_lists_added_removed_and_unchanged_capabilities() {
        let dir = scratch("update");
        let old = file(
            &dir,
            "old.json",
            r#"{"version":1,"actions":["read","push"],"egress_hosts":["docs.rs"]}"#,
        );
        let new = file(
            &dir,
            "new.json",
            r#"{"version":1,"actions":["read","open_pr"],"egress_hosts":["docs.rs"],"tools":["search"]}"#,
        );
        let out = run_with(&args(&["diff", &old, &new])).unwrap();
        assert_eq!(
            out,
            "capabilities: 2 added, 1 removed, 2 unchanged\n\
             actions\n  + open_pr\n  - push\n    read\n\
             egress_hosts\n    docs.rs\n\
             tools\n  + search\n\
             data_flows\n    (none)\n\
             it adds capabilities: a human must grant each one before it installs (ADR-0012)\n"
        );
        // An update that only takes away adds nothing to consent to.
        let narrower = file(&dir, "narrower.json", r#"{"version":1,"actions":["read"]}"#);
        let back = run_with(&args(&["diff", &new, &narrower])).unwrap();
        assert!(
            back.starts_with("capabilities: 0 added, 3 removed, 1 unchanged\n"),
            "{back}"
        );
        assert!(back.ends_with("it adds no capability\n"), "{back}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_first_install_adds_everything_it_asks_for() {
        let dir = scratch("first");
        let new = file(
            &dir,
            "new.json",
            r#"{"version":1,"data_flows":["job->web"]}"#,
        );
        let out = run_with(&args(&["diff", &new])).unwrap();
        assert!(
            out.starts_with("capabilities: 1 added, 0 removed, 0 unchanged\n"),
            "{out}"
        );
        assert!(out.contains("data_flows\n  + job->web\n"), "{out}");
        assert!(out.contains("a human must grant each one"), "{out}");
        // Asking for nothing adds nothing.
        let none = file(&dir, "none.json", r#"{"version":1}"#);
        assert!(
            run_with(&args(&["diff", &none]))
                .unwrap()
                .ends_with("it adds no capability\n")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_document_is_refused_without_echoing_it() {
        let dir = scratch("bad");
        let good = file(&dir, "good.json", r#"{"version":1}"#);
        let bad = file(
            &dir,
            "bad.json",
            "{\"version\":1,\"egress_hosts\":[\"evil\\u001b[2J.example\"]}",
        );
        let (code, message) = run_with(&args(&["diff", &good, &bad])).unwrap_err();
        assert_eq!(code, 1);
        assert!(
            message.contains("egress_hosts") && !message.contains("evil"),
            "{message}"
        );
        let version = file(&dir, "v2.json", r#"{"version":2}"#);
        assert_eq!(run_with(&args(&["diff", &version])).unwrap_err().0, 1);
        let big = file(&dir, "big.json", &" ".repeat(MAX_CAPABILITY_DOCUMENT + 1));
        let (code, message) = run_with(&args(&["diff", &big])).unwrap_err();
        assert_eq!(code, 1);
        assert!(
            message.contains("larger than a capabilities document may be"),
            "{message}"
        );
        let missing = dir.join("missing.json").display().to_string();
        assert_eq!(run_with(&args(&["diff", &missing])).unwrap_err().0, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn usage() {
        assert_eq!(run_with(&args(&[])).unwrap_err().0, 2);
        assert_eq!(run_with(&args(&["install", "x"])).unwrap_err().0, 2);
        assert_eq!(run_with(&args(&["diff"])).unwrap_err().0, 2);
        assert_eq!(run_with(&args(&["diff", "a", "b", "c"])).unwrap_err().0, 2);
    }
}
