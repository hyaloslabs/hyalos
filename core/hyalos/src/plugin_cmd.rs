//! `hyalos plugin install|list|diff`: installing an Agent Skill and the capability diff an operator
//! reads first (ADR-0006, ADR-0085, ADR-0088, ADR-0089).
//!
//! `install` reads a skill from a directory, prints what it asks for as a diff against what its name
//! holds now, keeps its files in the plugin store by digest, and records the install as the
//! operator's signed decision. A skill that asks for a capability its name does not hold installs
//! only with `--grant`, after the operator has read the diff. `diff` reads capabilities documents and
//! writes nothing.
//!
//! A skill from a directory installs in local mode (S6.7, ADR-0006): it has no signature or
//! transparency-log entry, it is diffed against nothing unless its name was installed locally
//! before, and it is always the operator's decision, never policy's.
//!
//! An install or update from an index verifies the release's Sigstore bundle first (S7.12, ADR-0093),
//! against the trusted-root snapshot in force: nothing from an index is read without it. The snapshot
//! in force is the one this release embeds, or a later one it vouched for, imported with
//! `plugin snapshot import` or from the index location an install or update reads (S7.11, ADR-0097).

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use hyalos_engine::Decider;
use hyalos_plugin::index::{
    INDEX_BUNDLE, INDEX_FILE, Index, MAX_INDEX, SignedIndex, SignedIndexError, verify_signed,
};
use hyalos_plugin::revocation::RevocationError;
use hyalos_plugin::sigstore::{MAX_BUNDLE, Trust};
use hyalos_plugin::skill::{MAX_ARCHIVE, Skill, Store, digest_of};
use hyalos_plugin::snapshot::{Snapshot, SnapshotError};
use hyalos_types::capability::Field;
use hyalos_types::{
    Capabilities, Clock, Diff, Digest, Event, Grant, IndexLocation, InstallPolicy,
    MAX_CAPABILITY_DOCUMENT, Origin, PluginName, PluginSource, PluginVersion, ReasonCode,
    Timestamp,
};

use crate::clock::SystemClock;
use crate::mcp_cmd::EngineLog;
use crate::operator;
use crate::passphrase;
use crate::paths::{self, Env};
use crate::plugin_source;
use crate::plugin_trust::{self, NotImported};
use crate::policy_cmd;
use hyalos_types::plugin::{
    DEFAULT_MAX_REVOCATION_AGE_DAYS, DEFAULT_MAX_SNAPSHOT_AGE_DAYS, MAX_AGE_DAYS,
    MAX_POLICY_INDEXES,
};

const USAGE: &str = "usage: hyalos plugin install <dir> [--grant] [--reason <code>] [--passphrase-stdin]
       hyalos plugin install --index <location> <name> [--version <v>] [--grant] [--reason <code>]
                             [--passphrase-stdin]
       hyalos plugin list
       hyalos plugin policy set --ceiling <file> [--index <location>]... [--max-snapshot-age-days <n>]
                                [--max-revocation-age-days <n>] [--reason <code>] [--passphrase-stdin]
       hyalos plugin policy show
       hyalos plugin update [--every <hours>]
       hyalos plugin snapshot import <location>
       hyalos plugin snapshot show
       hyalos plugin pack <dir> <file>
       hyalos plugin diff [<installed>] <new>
  install reads the Agent Skill in <dir> (SKILL.md, its scripts, and capabilities.json if it asks for
  any), prints what it asks for against what its name holds now, and installs it. If it asks for a
  capability its name does not hold, read the diff and run again with --grant to grant it. A skill
  from a directory is local: not signed or logged, so it starts from no capability unless the same
  name was installed locally before, and no install policy covers it. It is your decision, signed
  with your approval key: the passphrase is asked for at the terminal, or read from
  stdin with --passphrase-stdin. A job is given it with hyalos run --skill <name>
  With --index, it finds <name> in the plugin index at <location> (an https:// URL whose index.json
  and archives are beside each other, as a release's assets, or a directory holding the same files,
  as an offline bundle), takes its latest version or --version, and installs it only if its archive
  has the digest the index gives (ADR-0090) and its Sigstore bundle shows the Hyalos marketplace
  pipeline signed it, verified offline (ADR-0093). If your install policy trusts that index and the
  skill asks for nothing beyond its ceiling, it installs without your passphrase (ADR-0091)
  policy set is your decision, signed with your approval key: a plugin from one of the indexes,
  asking for no more than the capabilities document <file>, then installs without you. Anything else
  stays your decision each time. The ages (default 90 and 30 days) bound how old a trusted root and a
  revocation list may be (ADR-0007). policy show prints the policy in force
  update looks up each skill installed from an index there again. A newer version your install
  policy covers is installed; one that asks for more is recorded as waiting for you, and the
  installed version keeps running until you install it. With --every, it checks again on that
  schedule until stopped (ADR-0092). Exit 1 if a check failed
  snapshot import reads trusted-root.json and its Sigstore bundle at <location> (an index location, as
  for install --index) and makes it the trusted root releases are checked against, only if the one in
  force verifies it as the Hyalos marketplace pipeline's and it is newer (ADR-0097). It then reads
  revocations.json the same way, under the snapshot now in force, and disables every installed
  release it revokes (ADR-0098). install --index and update import both from their location the same
  way first, and nothing the list in force revokes is installed. snapshot show prints both in force
  list shows every skill installed, its digest, where it came from, what it was granted, whether it
  is revoked, and any update waiting for you
  pack writes the skill in <dir> as one archive to <file>, a new file, and prints its digest, as an
  index lists it
  diff prints what <new> adds to the capabilities in <installed>, what it takes away and what it
  keeps. With one file, it is a first install: everything <new> asks for is added. Both are
  capabilities documents ({\"version\": 1, ...}). It writes nothing";

pub fn run(args: &[String]) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    let result = match args.first().map(String::as_str) {
        Some("install" | "list" | "pack" | "policy" | "snapshot" | "update") => manage(args, &env),
        _ => run_with(args),
    };
    crate::operator_cmd::finish(result)
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

/// `install` and `list`, which read the log and the store. Exit 1 if refused, 2 for usage. A release
/// from an index is verified against the snapshot in force, which starts from the one this release
/// embeds.
pub fn manage(args: &[String], env: Env<'_>) -> Result<String, (u8, String)> {
    manage_with(args, env, None, None)
}

/// As [`manage`], starting from the snapshot `base` rather than the embedded one, and with the
/// operator's passphrase given rather than read where the arguments say. The binary always calls
/// [`manage`]; this is for tests, which sign releases and snapshots with a Sigstore of their own.
pub fn manage_with(
    args: &[String],
    env: Env<'_>,
    base: Option<&Snapshot>,
    passphrase: Option<&str>,
) -> Result<String, (u8, String)> {
    // Verified only by the commands that check against it, so the others never depend on it.
    let base = || -> Result<Snapshot, String> {
        match base {
            Some(base) => Ok(base.clone()),
            None => Snapshot::embedded().map_err(|e| {
                format!(
                    "the trusted-root snapshot this release embeds cannot be used: {e} ({})",
                    e.code()
                )
            }),
        }
    };
    let (args, source_of_passphrase) = passphrase::source(args);
    let usage = |why: &str| (2, format!("{why}\n{USAGE}"));
    let failed = |e: String| (1, e);
    let unlock = || match passphrase {
        Some(passphrase) => operator::engine_with(env, passphrase),
        None => operator::engine(env, source_of_passphrase),
    };
    let log = EngineLog::from_env(env).map_err(failed)?;
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
            // Everything that can be checked without the passphrase is checked first.
            let state = log.read_if_any().map_err(failed)?;
            let mut notes = String::new();
            // The index, once it is proven the pipeline's current one (ADR-0128).
            let mut signed_index: Option<(IndexLocation, SignedIndex)> = None;
            // Said before the passphrase is asked for, so the operator signs knowing it.
            let mut unverified = String::new();
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
                    let current =
                        plugin_trust::in_force(env, state.as_ref(), &base().map_err(failed)?)
                            .map_err(failed)?;
                    let snapshot = match policy_cmd::open(env) {
                        Ok(mut engine) => {
                            refreshed(env, &mut engine, &location, current, &mut notes)
                        }
                        Err(e) => {
                            notes.push_str(&format!(
                                "the trusted-root snapshot there was not looked at: {e}\n"
                            ));
                            current
                        }
                    };
                    // Too old a snapshot or list refuses a new install (ADR-0099).
                    let state = log.read().map_err(failed)?;
                    let list = plugin_trust::revocations_in_force(env, Some(&state))
                        .map_err(|e| failed(format!("{notes}{e}")))?;
                    plugin_trust::fresh(
                        &snapshot,
                        list.as_ref(),
                        state.install_policy().map(|p| &p.policy),
                        SystemClock.now(),
                    )
                    .map_err(|e| failed(format!("{notes}{e}")))?;
                    let trust = trust_of(&snapshot).map_err(failed)?;
                    let (bytes, bundle) = fetch_index(&location).map_err(failed)?;
                    let last = state
                        .accepted_index(&location)
                        .map(|a| (a.digest, a.signed_at));
                    // A signature that fails is refused. An index merely unsigned, older or too old
                    // is the operator's to install from, and never policy's (ADR-0128).
                    match verify_signed(&trust, &bytes, bundle.as_deref(), last, SystemClock.now())
                    {
                        Ok(checked) => signed_index = Some((location.clone(), checked)),
                        Err(
                            e @ (SignedIndexError::Unverifiable(_) | SignedIndexError::NotAnIndex),
                        ) => {
                            return Err(failed(format!("{notes}{location}: {e} ({})", e.code())));
                        }
                        Err(e) => {
                            let line = format!(
                                "the version below is unverified, so only you can install it: {e} ({})\n",
                                e.code()
                            );
                            notes.push_str(&line);
                            unverified = line;
                        }
                    }
                    from_index(&bytes, location, &name, version.as_ref(), &trust).map_err(failed)?
                }
            };
            // What the list in force revokes is never installed (ADR-0098), from an index or a
            // directory. The state is read again: the location may just have brought a newer list.
            let state = log.read_if_any().map_err(failed)?;
            let list = plugin_trust::revocations_in_force(env, state.as_ref())
                .map_err(|e| failed(format!("{notes}{e}")))?;
            plugin_trust::refuse_revoked(list.as_ref(), &skill.digest())
                .map_err(|e| failed(format!("{notes}{}: {e}", skill.name)))?;
            // A local install starts from nothing (ADR-0006): what an index release was granted
            // was granted to that release, not to whatever is in a directory under the same name.
            // A local update is diffed against the local install before it.
            let held = state
                .as_ref()
                .and_then(|s| s.plugins().get(&skill.name))
                .filter(|p| source != PluginSource::Local || p.source == PluginSource::Local)
                .map(|p| p.grant.clone())
                .unwrap_or_default();
            // Within the install policy the operator signed, it installs without them (ADR-0091).
            // A release not proven later than the one installed is theirs to sign (ADR-0127).
            let by_policy = signed_index.is_some()
                && state.as_ref().is_some_and(|s| {
                    s.install_policy()
                        .is_some_and(|p| p.policy.covers(&source, &skill.asks))
                        && s.plugins()
                            .get(&skill.name)
                            .is_none_or(|p| source.policy_may_replace(&p.source))
                });
            let diff = held.diff(&skill.asks);
            // The command an MCP server runs in each job given it, printable ASCII (ADR-0101).
            let mcp = skill.mcp.as_ref().map_or(String::new(), |m| {
                let words: Vec<&str> = std::iter::once(&m.command)
                    .chain(&m.args)
                    .map(String::as_str)
                    .collect();
                format!("runs an MCP server: {}\n", words.join(" "))
            });
            let mut out = format!(
                "{notes}skill {} ({} files), digest {}{}\n{mcp}{}",
                skill.name,
                skill.files.len(),
                skill.digest(),
                from(&source),
                render(&diff)
            );
            if diff.widens() && !grant && !by_policy {
                return Err(failed(format!(
                    "{out}it was not installed: read what it asks for above, and run again with --grant to grant it"
                )));
            }
            // Unlocked before anything is kept, so a wrong passphrase leaves nothing behind.
            let (mut engine, decider) = if by_policy {
                (policy_cmd::open(env).map_err(failed)?, Decider::Policy)
            } else {
                eprint!("{unverified}");
                (
                    unlock().map_err(|e| failed(format!("{unverified}{e}")))?,
                    Decider::Human,
                )
            };
            // Again against the engine about to write it: a list may have been imported since.
            let list =
                plugin_trust::revocations_in_force(env, Some(engine.state())).map_err(failed)?;
            plugin_trust::refuse_revoked(list.as_ref(), &skill.digest())
                .map_err(|e| failed(format!("{}: {e}", skill.name)))?;
            if let Some((at, checked)) = signed_index.filter(|(_, c)| c.is_new) {
                engine
                    .accept_index(SystemClock.now(), at, checked.digest, checked.signed_at)
                    .map_err(|e| failed(e.to_string()))?;
            }
            let head = keep_and_install(
                env,
                &mut engine,
                skill.name.clone(),
                &skill,
                source,
                decider,
                reason,
            )
            .map_err(failed)?;
            let how = if by_policy {
                "within your install policy"
            } else {
                "signed by you"
            };
            out.push_str(&format!(
                "installed {} at event {}, {how}; give it to a job with hyalos run --skill {}\n",
                skill.name, head.seq, skill.name
            ));
            Ok(out)
        }
        [policy, show] if policy == "policy" && show == "show" => {
            let set = log
                .read_if_any()
                .map_err(failed)?
                .and_then(|state| state.install_policy().cloned());
            Ok(match set {
                None => "no install policy is set: every install is your decision\n".into(),
                Some(set) => format!(
                    "install policy set at event {}\n{}",
                    set.seq,
                    describe(&set.policy)
                ),
            })
        }
        [policy, set, rest @ ..] if policy == "policy" && set == "set" => {
            let (mut ceiling, mut indexes, mut reason) =
                (None, BTreeSet::new(), "plugin.policy_set");
            let (mut snapshot, mut revocation) = (
                DEFAULT_MAX_SNAPSHOT_AGE_DAYS,
                DEFAULT_MAX_REVOCATION_AGE_DAYS,
            );
            let mut rest = rest.iter();
            while let Some(arg) = rest.next() {
                let mut value = || {
                    rest.next()
                        .ok_or_else(|| usage(&format!("{arg} needs a value")))
                };
                let days = |text: &str| {
                    text.parse::<u32>()
                        .ok()
                        .filter(|d| (1..=MAX_AGE_DAYS).contains(d))
                        .ok_or_else(|| usage(&format!("an age is 1 to {MAX_AGE_DAYS} days")))
                };
                match arg.as_str() {
                    "--ceiling" => ceiling = Some(value()?),
                    "--index" => {
                        indexes.insert(location(value()?).ok_or_else(|| {
                            usage("--index is an https:// URL, an http:// URL on loopback, or a directory")
                        })?);
                    }
                    "--max-snapshot-age-days" => snapshot = days(value()?)?,
                    "--max-revocation-age-days" => revocation = days(value()?)?,
                    "--reason" => reason = value()?,
                    _ => {
                        return Err(usage(
                            "policy set takes --ceiling <file>, --index <location>..., --max-snapshot-age-days <n>, --max-revocation-age-days <n> and --reason <code>",
                        ));
                    }
                }
            }
            let ceiling = ceiling.ok_or_else(|| {
                usage("policy set needs --ceiling <file>, a capabilities document")
            })?;
            let reason = ReasonCode::new(reason)
                .ok_or_else(|| usage("a reason is a short code, such as plugin.policy_set"))?;
            let new = InstallPolicy {
                ceiling: read(ceiling)?,
                indexes,
                max_snapshot_age_days: snapshot,
                max_revocation_age_days: revocation,
            };
            if !new.is_valid() {
                return Err(usage(&format!(
                    "a policy trusts at most {MAX_POLICY_INDEXES} indexes"
                )));
            }
            let old = log
                .read_if_any()
                .map_err(failed)?
                .map_or_else(Grant::none, |state| {
                    state
                        .install_policy()
                        .map(|p| p.policy.ceiling.clone())
                        .unwrap_or_default()
                });
            let mut out = format!(
                "the ceiling, against the policy in force:\n{}{}",
                render(&old.diff(&new.ceiling)),
                describe(&new)
            );
            let mut engine = unlock().map_err(failed)?;
            let head = engine
                .set_install_policy(SystemClock.now(), new, Decider::Human, reason)
                .map_err(|e| failed(e.to_string()))?;
            out.push_str(&format!(
                "set at event {}, signed by you: a plugin from those indexes asking for no more than the ceiling now installs without you\n",
                head.seq
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
            if state.plugins().is_empty() && state.drafts().is_empty() {
                return Ok("no skill is installed\n".into());
            }
            let list = plugin_trust::revocations_in_force(env, Some(&state)).map_err(failed)?;
            let mut out = String::new();
            for (name, p) in state.plugins() {
                out.push_str(&format!(
                    "{name}\t{}\tinstalled at event {}{}\tgranted {}\n",
                    p.digest,
                    p.seq,
                    from(&p.source),
                    summary(&p.grant)
                ));
                if let Some(r) = state.revoked_plugins().get(name) {
                    out.push_str(&format!(
                        "  revoked ({}) since event {}: disabled, no job is given it; install a release that is not revoked\n",
                        r.reason, r.seq
                    ));
                }
                if let Some(u) = state.pending_updates().get(name) {
                    if let Some(reason) = list.as_ref().and_then(|l| l.reason(&u.digest)) {
                        out.push_str(&format!(
                            "  version {} found at event {} is revoked ({reason}) and will not install\n",
                            u.version, u.seq
                        ));
                        continue;
                    }
                    out.push_str(&format!(
                        "  version {} waits for you since event {}: it asks for {}; install it with hyalos plugin install --index {} {name}\n",
                        u.version,
                        u.seq,
                        summary(&u.asks),
                        u.location
                    ));
                }
            }
            // The skills Hyalos drafted, which no one installed (ADR-0113, ADR-0114).
            for (name, d) in state.drafts() {
                let trust = match d.trusted {
                    Some(seq) => format!("trusted after replay at event {seq}"),
                    None => "tainted: not replayed, only --skill gives it to a job".into(),
                };
                out.push_str(&format!(
                    "{name}\t{}\tdrafted from task {} at event {}\tgranted {}\t{trust}\n",
                    d.digest,
                    d.task.seq(),
                    d.seq,
                    summary(&d.grant)
                ));
            }
            Ok(out)
        }
        [update, rest @ ..] if update == "update" => {
            let every = match rest {
                [] => None,
                [flag, hours] if flag == "--every" => Some(
                    hours
                        .parse::<u64>()
                        .ok()
                        .filter(|h| (1..=MAX_EVERY_HOURS).contains(h))
                        .ok_or_else(|| {
                            usage(&format!("--every is 1 to {MAX_EVERY_HOURS} hours"))
                        })?,
                ),
                _ => return Err(usage("update takes only --every <hours>")),
            };
            let Some(hours) = every else {
                let base = base().map_err(failed)?;
                let (out, clean) = check_updates(env, &log, &base);
                return if clean { Ok(out) } else { Err((1, out)) };
            };
            // On a schedule: a failed check is reported and the next one still runs.
            let base = base().map_err(failed)?;
            loop {
                let (out, _) = check_updates(env, &log, &base);
                print!("{out}");
                std::thread::sleep(std::time::Duration::from_secs(hours * 3600));
            }
        }
        [snapshot, import, at] if snapshot == "snapshot" && import == "import" => {
            let location = location(at).ok_or_else(|| {
                usage("a location is an https:// URL, an http:// URL on loopback, or a directory")
            })?;
            let mut engine = policy_cmd::open(env).map_err(failed)?;
            let base = base().map_err(failed)?;
            let current = match plugin_trust::in_force(env, Some(engine.state()), &base) {
                Ok(current) => current,
                // A kept snapshot lost to a crash or a slip can be put back from a copy of it.
                Err(e) => {
                    if !plugin_trust::restore(env, engine.state(), &location).map_err(failed)? {
                        return Err(failed(e));
                    }
                    let current =
                        plugin_trust::in_force(env, Some(engine.state()), &base).map_err(failed)?;
                    return Ok(format!(
                        "put back the kept snapshot the log recorded, from {location}\n{}",
                        plugin_trust::describe(
                            &current,
                            engine.state().trusted_root().map(|t| t.seq)
                        )
                    ));
                }
            };
            let mut out = String::new();
            // A kept list lost the same way is put back the same way.
            if plugin_trust::revocations_in_force(env, Some(engine.state())).is_err()
                && plugin_trust::restore_revocations(env, engine.state(), &location)
                    .map_err(failed)?
            {
                out.push_str(&format!(
                    "put back the kept revocation list the log recorded, from {location}\n"
                ));
            }
            let mut snapshot_refused = None;
            let (snapshot, mut imported) =
                match plugin_trust::import(env, &mut engine, &location, &current) {
                    Ok((next, seq)) => {
                        out.push_str(&format!(
                            "{}releases are now checked against it\n",
                            plugin_trust::describe(&next, Some(seq))
                        ));
                        (next, true)
                    }
                    Err(NotImported::Absent) => (current, false),
                    Err(NotImported::Refused(e)) if e == SnapshotError::NotNewer => {
                        out.push_str(&format!("the trusted-root snapshot there: {e}\n"));
                        (current, false)
                    }
                    Err(e) => {
                        // The list there is still read, under the snapshot in force, as an install
                        // does.
                        snapshot_refused = Some(format!("it was not imported: {}", e.why()));
                        (current, false)
                    }
                };
            match plugin_trust::import_revocations(env, &mut engine, &location, &snapshot) {
                Ok((list, seq, disabled)) => {
                    imported = true;
                    out.push_str(&plugin_trust::describe_revocations(Some(&list), Some(seq)));
                    out.push_str(&disabled);
                }
                Err(NotImported::Absent) => {
                    out.push_str(&plugin_trust::apply_in_force(env, &mut engine).map_err(failed)?);
                }
                Err(NotImported::RefusedList(e)) if e == RevocationError::NotNewer => {
                    out.push_str(&format!("the revocation list there: {e}\n"));
                    out.push_str(&plugin_trust::apply_in_force(env, &mut engine).map_err(failed)?);
                }
                Err(e) => {
                    return Err(failed(format!(
                        "{out}{}the revocation list was not imported: {}",
                        snapshot_refused
                            .as_ref()
                            .map(|why| format!("the trusted-root snapshot {why}\n"))
                            .unwrap_or_default(),
                        e.why()
                    )));
                }
            }
            if let Some(why) = snapshot_refused {
                return Err(failed(format!("{out}{why}")));
            }
            if !imported && out.is_empty() {
                return Err(failed(format!(
                    "{location}: there is no {} or {} there (snapshot.absent)",
                    hyalos_plugin::snapshot::FILE,
                    hyalos_plugin::revocation::FILE
                )));
            }
            Ok(out)
        }
        [snapshot, show] if snapshot == "snapshot" && show == "show" => {
            let state = log.read_if_any().map_err(failed)?;
            let current = plugin_trust::in_force(env, state.as_ref(), &base().map_err(failed)?)
                .map_err(failed)?;
            let imported = state
                .as_ref()
                .and_then(|s| s.trusted_root())
                .filter(|t| t.digest == current.digest())
                .map(|t| t.seq);
            let list = plugin_trust::revocations_in_force(env, state.as_ref()).map_err(failed)?;
            Ok(format!(
                "{}{}",
                plugin_trust::describe(&current, imported),
                plugin_trust::describe_revocations(
                    list.as_ref(),
                    state.as_ref().and_then(|s| s.revocations()).map(|r| r.seq)
                )
            ))
        }
        _ => Err(usage("unknown plugin command")),
    }
}

/// `current`, or the later snapshot at `location` it vouches for, imported, with the later revocation
/// list there imported under it; a line in `notes` says why either was not, and which installed
/// releases the list disabled.
fn refreshed(
    env: Env<'_>,
    engine: &mut hyalos_engine::Engine,
    location: &IndexLocation,
    current: Snapshot,
    notes: &mut String,
) -> Snapshot {
    let snapshot = refreshed_snapshot(env, engine, location, current, notes);
    match plugin_trust::import_revocations(env, engine, location, &snapshot) {
        Ok((list, seq, disabled)) => {
            notes.push_str(&format!(
                "imported a newer revocation list from there at event {seq}, issued {}\n{disabled}",
                crate::mcp::rfc3339(list.signed_at().as_millis())
            ));
        }
        Err(why) => {
            if let Some(note) = why.note() {
                notes.push_str(&note);
            }
            match plugin_trust::apply_in_force(env, engine) {
                Ok(disabled) => notes.push_str(&disabled),
                Err(e) => notes.push_str(&format!("{e}\n")),
            }
        }
    }
    snapshot
}

fn refreshed_snapshot(
    env: Env<'_>,
    engine: &mut hyalos_engine::Engine,
    location: &IndexLocation,
    current: Snapshot,
    notes: &mut String,
) -> Snapshot {
    match plugin_trust::import(env, engine, location, &current) {
        Ok((next, seq)) => {
            notes.push_str(&format!(
                "imported a newer trusted-root snapshot from there at event {seq}, issued {}\n",
                crate::mcp::rfc3339(next.signed_at().as_millis())
            ));
            next
        }
        Err(why) => {
            if let Some(note) = why.note() {
                notes.push_str(&note);
            }
            current
        }
    }
}

/// The trust releases are checked with under `snapshot`.
fn trust_of(snapshot: &Snapshot) -> Result<Trust, String> {
    snapshot.trust().map_err(|e| {
        format!(
            "the trusted-root snapshot in force cannot be used: {e} ({})",
            e.code()
        )
    })
}

/// The longest interval `update --every` takes: a week.
const MAX_EVERY_HOURS: u64 = 168;

/// One update check (ADR-0092): each plugin installed from an index is looked up there again. A
/// newer version the install policy covers is installed by policy. One beyond it is recorded as
/// waiting for the operator, once, and the installed version keeps running. Returns what it did,
/// one line a plugin, and whether every check went through.
fn check_updates(env: Env<'_>, log: &EngineLog, base: &Snapshot) -> (String, bool) {
    if !log.log.exists() {
        return ("no skill is installed\n".into(), true);
    }
    let mut engine = match policy_cmd::open(env) {
        Ok(engine) => engine,
        Err(e) => return (format!("{e}\n"), false),
    };
    let installed: Vec<(PluginName, Digest, IndexLocation, Option<Timestamp>)> = engine
        .state()
        .plugins()
        .iter()
        .filter_map(|(name, p)| match &p.source {
            PluginSource::Index {
                location,
                signed_at,
                ..
            } => Some((name.clone(), p.digest, location.clone(), *signed_at)),
            PluginSource::Local => None,
        })
        .collect();
    if installed.is_empty() {
        return ("no skill is installed from an index\n".into(), true);
    }
    let (mut out, mut clean) = (String::new(), true);
    let mut snapshot = match plugin_trust::in_force(env, Some(engine.state()), base) {
        Ok(snapshot) => snapshot,
        Err(e) => return (format!("{e}\n"), false),
    };
    for (name, digest, location, installed_signed_at) in installed {
        snapshot = refreshed(env, &mut engine, &location, snapshot, &mut out);
        // Too old a snapshot or list refuses the update; the installed version keeps running.
        let stale =
            plugin_trust::revocations_in_force(env, Some(engine.state())).and_then(|list| {
                plugin_trust::fresh(
                    &snapshot,
                    list.as_ref(),
                    engine.state().install_policy().map(|p| &p.policy),
                    SystemClock.now(),
                )
            });
        if let Err(e) = stale {
            clean = false;
            out.push_str(&format!(
                "{name}: not updated, and the installed version keeps running: {e}\n"
            ));
            continue;
        }
        let trust = match trust_of(&snapshot) {
            Ok(trust) => trust,
            Err(e) => return (format!("{out}{e}\n"), false),
        };
        match update_one(
            env,
            &mut engine,
            &name,
            digest,
            installed_signed_at,
            location,
            &trust,
        ) {
            Ok(line) => out.push_str(&format!("{name}: {line}\n")),
            Err(e) => {
                clean = false;
                out.push_str(&format!("{name}: {e}\n"));
            }
        }
    }
    (out, clean)
}

/// Keeps `skill` in the plugin store, under its digest, then records it installed as `name`.
fn keep_and_install(
    env: Env<'_>,
    engine: &mut hyalos_engine::Engine,
    name: PluginName,
    skill: &Skill,
    source: PluginSource,
    decider: Decider,
    reason: ReasonCode,
) -> Result<hyalos_log::Head, String> {
    let store = paths::plugin_store(env)?;
    paths::make_private_dir(&store)?;
    let digest = Store::new(store)
        .put(skill)
        .map_err(|e| format!("cannot keep the skill: {e}"))?;
    engine
        .install_plugin(
            SystemClock.now(),
            name,
            digest,
            skill.asks.clone(),
            source,
            decider,
            reason,
        )
        .map_err(|e| e.to_string())
}

fn update_one(
    env: Env<'_>,
    engine: &mut hyalos_engine::Engine,
    name: &PluginName,
    installed: Digest,
    installed_signed_at: Option<Timestamp>,
    location: IndexLocation,
    trust: &Trust,
) -> Result<String, String> {
    // An update by policy trusts only the pipeline's current index (ADR-0128): signed, no older than
    // the last accepted from this location, and not frozen.
    let (index, bundle) = fetch_index(&location)?;
    let last = engine
        .state()
        .accepted_index(&location)
        .map(|a| (a.digest, a.signed_at));
    let checked = verify_signed(trust, &index, bundle.as_deref(), last, SystemClock.now())
        .map_err(|e| format!("{location}: {e} ({})", e.code()))?;
    if checked.is_new {
        engine
            .accept_index(
                SystemClock.now(),
                location.clone(),
                checked.digest,
                checked.signed_at,
            )
            .map_err(|e| e.to_string())?;
    }
    let (skill, source) = from_index(&index, location.clone(), name, None, trust)?;
    let PluginSource::Index {
        version, signed_at, ..
    } = &source
    else {
        return Err("not from an index".into());
    };
    let version = version.clone();
    let digest = skill.digest();
    if digest == installed {
        return Ok(format!("up to date at version {version}"));
    }
    if let (Some(prev), Some(next)) = (installed_signed_at, *signed_at)
        && next <= prev
    {
        return Err(format!(
            "version {version} is not newer than the installed release (plugin.not_newer)"
        ));
    }
    let list = plugin_trust::revocations_in_force(env, Some(engine.state()))?;
    if let Some(reason) = list.as_ref().and_then(|l| l.reason(&digest)) {
        return Ok(format!(
            "version {version} is revoked ({reason}), so it is not offered (plugin.revoked)"
        ));
    }
    let state = engine.state();
    let covered = state
        .install_policy()
        .is_some_and(|p| p.policy.covers(&source, &skill.asks));
    if covered {
        let head = keep_and_install(
            env,
            engine,
            name.clone(),
            &skill,
            source,
            Decider::Policy,
            ReasonCode::new("plugin.updated").unwrap_or_else(ReasonCode::unavailable),
        )?;
        return Ok(format!(
            "updated to version {version} at event {}, within your install policy",
            head.seq
        ));
    }
    if state
        .pending_updates()
        .get(name)
        .is_some_and(|u| u.digest == digest)
    {
        return Ok(format!("version {version} still waits for you"));
    }
    let head = engine
        .submit(
            SystemClock.now(),
            Origin::Engine,
            Event::PluginUpdatePending {
                name: name.clone(),
                version: version.clone(),
                digest,
                asks: skill.asks.clone(),
                location: location.clone(),
            },
        )
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "version {version} waits for you (event {}): it asks for more than your install policy lets install without you. The installed version keeps running. Read it and decide with hyalos plugin install --index {location} {name}",
        head.seq
    ))
}

/// `location` as an index location, a relative directory made absolute.
fn location(text: &str) -> Option<IndexLocation> {
    if text.contains("://") {
        return IndexLocation::new(text);
    }
    let dir = std::path::absolute(text).ok()?;
    IndexLocation::new(dir.to_str()?)
}

/// The index at `location`, with its Sigstore bundle if it has one.
fn fetch_index(location: &IndexLocation) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let index = plugin_source::fetch(location, INDEX_FILE, MAX_INDEX)?;
    let bundle = plugin_source::fetch_optional(location, INDEX_BUNDLE, MAX_BUNDLE)?;
    Ok((index, bundle))
}

/// Resolves `name` in `index`, the bytes read from `location` and already checked by the caller, so
/// that what was checked is what is used. Fetches the archive, and reads it only once it has the
/// digest the index gave (ADR-0090) and its Sigstore bundle verifies against `trust` (ADR-0093). A
/// release with no bundle, or one that does not verify, is refused.
fn from_index(
    index: &[u8],
    location: IndexLocation,
    name: &PluginName,
    version: Option<&PluginVersion>,
    trust: &Trust,
) -> Result<(Skill, PluginSource), String> {
    let at = location.as_str();
    let index = Index::parse(index).map_err(|e| format!("{at}: {e} ({})", e.code()))?;
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
    let bundle = plugin_source::fetch(&location, &entry.bundle, MAX_BUNDLE)?;
    let signed_at = trust.verify_signed(&archive, &bundle).map_err(|e| {
        format!(
            "{}: {e}, so {} was not read ({})",
            entry.bundle,
            entry.artifact,
            e.code()
        )
    })?;
    let skill = Skill::from_archive(&archive)
        .map_err(|e| format!("{}: {e} ({})", entry.artifact, e.code()))?;
    if skill.name != *name {
        return Err(format!(
            "{}: the skill in it is not named {name} (plugin.wrong_name)",
            entry.artifact
        ));
    }
    // Only an index release must declare a well-formed version, if it declares one (ADR-0127).
    let declared = skill
        .declared_version()
        .map_err(|e| format!("{}: {e} ({})", entry.artifact, e.code()))?;
    if let Some(version) = declared
        && version != entry.version
    {
        return Err(format!(
            "{}: the skill in it is version {version}, not {} (plugin.wrong_version)",
            entry.artifact, entry.version
        ));
    }
    let source = PluginSource::Index {
        location,
        version: entry.version.clone(),
        signed_at: Some(signed_at),
    };
    Ok((skill, source))
}

/// A policy as the operator reads it: the ceiling, the indexes and the ages.
fn describe(policy: &InstallPolicy) -> String {
    let indexes: Vec<&str> = policy.indexes.iter().map(IndexLocation::as_str).collect();
    format!(
        "ceiling: {}\nindexes: {}\nmaximum ages: trusted root {} days, revocation list {} days\n",
        summary(&policy.ceiling),
        if indexes.is_empty() {
            "none, so nothing installs without you".to_string()
        } else {
            indexes.join(", ")
        },
        policy.max_snapshot_age_days,
        policy.max_revocation_age_days
    )
}

/// Where a plugin came from, for a line about it: nothing for a local install.
fn from(source: &PluginSource) -> String {
    match source {
        PluginSource::Local => ", local: no signature or transparency-log entry".into(),
        PluginSource::Index {
            location, version, ..
        } => format!(", version {version} from {location}"),
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
