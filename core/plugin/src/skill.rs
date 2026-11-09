//! Agent Skills on disk (ADR-0006, ADR-0089): reading one from a directory, the digest that binds an
//! install to its exact files, and the store that keeps installed skills by that digest.
//!
//! Everything in a skill is plugin-authored: its file names, its `SKILL.md` and its scripts. So a
//! skill is read within limits, only regular files and directories are taken (never a link, which
//! could point anywhere on the operator's machine), and every name must be plain. An error is a
//! reason code and never names a file the skill holds.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hyalos_types::{Capabilities, CapabilityError, Digest, PluginName, ReasonCode};
use sha2::{Digest as _, Sha256};

/// The file every skill has, at its root.
pub const SKILL_FILE: &str = "SKILL.md";
/// The capabilities a skill asks for, at its root, as a capabilities document (ADR-0085). A skill
/// without one asks for none.
pub const CAPABILITIES_FILE: &str = "capabilities.json";
/// The most files a skill may hold.
pub const MAX_FILES: usize = 256;
/// The most bytes a skill's files may hold together.
pub const MAX_BYTES: u64 = 8 << 20;
/// How deep a skill's directories may go.
pub const MAX_DEPTH: usize = 8;

/// What the digest starts with, so it can never be mistaken for a digest of anything else.
const DOMAIN: &[u8] = b"hyalos.skill.v1\n";

/// One file of a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFile {
    pub bytes: Vec<u8>,
    /// Whether its owner may run it. Scripts keep this, so the bit is part of the digest.
    pub executable: bool,
}

/// A skill as read: its name, what it asks for and its files by relative path (`/`-separated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: PluginName,
    pub asks: Capabilities,
    pub files: BTreeMap<String, SkillFile>,
}

/// Why a skill cannot be read or used. Names nothing the skill holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillError {
    /// The path is not a directory that can be read.
    Unreadable,
    /// It has no `SKILL.md` at its root.
    NoSkillFile,
    /// `SKILL.md` has no front matter with a `name` that is a plugin name.
    NoName,
    /// A file or directory name that is not plain, a link, or something that is neither a file nor a
    /// directory.
    BadEntry,
    /// More files, bytes or depth than a skill may have.
    TooLarge,
    /// `capabilities.json` is not a document this build reads.
    Capabilities(CapabilityError),
    /// The stored files do not have the digest they were installed with, or another name.
    Altered,
    /// Writing to the store or a job's directory failed.
    Write,
}

impl SkillError {
    pub fn code(&self) -> ReasonCode {
        let code = match self {
            Self::Unreadable => "plugin.unreadable",
            Self::NoSkillFile => "plugin.no_skill_file",
            Self::NoName => "plugin.no_name",
            Self::BadEntry => "plugin.bad_entry",
            Self::TooLarge => "plugin.too_large",
            Self::Capabilities(_) => "plugin.bad_capabilities",
            Self::Altered => "plugin.altered",
            Self::Write => "plugin.write_failed",
        };
        ReasonCode::new(code).unwrap_or_else(ReasonCode::unavailable)
    }
}

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => write!(f, "not a directory that can be read"),
            Self::NoSkillFile => write!(f, "no {SKILL_FILE} at its root"),
            Self::NoName => write!(
                f,
                "{SKILL_FILE} has no front matter naming it in lowercase letters, digits and hyphens"
            ),
            Self::BadEntry => write!(
                f,
                "it holds a link, a special file, or a name that is not plain letters, digits, '.', '_' and '-'"
            ),
            Self::TooLarge => write!(
                f,
                "larger than a skill may be ({MAX_FILES} files, {MAX_BYTES} bytes, {MAX_DEPTH} levels)"
            ),
            Self::Capabilities(e) => write!(f, "{CAPABILITIES_FILE}: {e}"),
            Self::Altered => write!(f, "its stored files are not the ones that were installed"),
            Self::Write => write!(f, "cannot write its files"),
        }
    }
}

impl std::error::Error for SkillError {}

impl Skill {
    /// Reads the skill in `dir`. Refuses a link anywhere, an entry that is neither a file nor a
    /// directory, a name that is not plain, and anything past the limits.
    pub fn read(dir: &Path) -> Result<Skill, SkillError> {
        let meta = fs::symlink_metadata(dir).map_err(|_| SkillError::Unreadable)?;
        if !meta.is_dir() {
            return Err(SkillError::Unreadable);
        }
        let mut files = BTreeMap::new();
        let mut total = 0;
        walk(dir, "", 0, &mut files, &mut total)?;
        let skill_md = files.get(SKILL_FILE).ok_or(SkillError::NoSkillFile)?;
        let name = name_of(&skill_md.bytes).ok_or(SkillError::NoName)?;
        let asks = match files.get(CAPABILITIES_FILE) {
            Some(file) => {
                Capabilities::from_document(&file.bytes).map_err(SkillError::Capabilities)?
            }
            None => Capabilities::none(),
        };
        Ok(Skill { name, asks, files })
    }

    /// The SHA-256 of every file's path, mode and bytes, in path order: what an install records, so
    /// the decision covers exactly these files.
    pub fn digest(&self) -> Digest {
        let mut hash = Sha256::new();
        hash.update(DOMAIN);
        for (path, file) in &self.files {
            hash.update((path.len() as u32).to_be_bytes());
            hash.update(path.as_bytes());
            hash.update([u8::from(file.executable)]);
            hash.update((file.bytes.len() as u64).to_be_bytes());
            hash.update(&file.bytes);
        }
        Digest::from_bytes(hash.finalize().into())
    }

    /// Writes the files under `dir`, which must not exist yet, readable by anyone and writable by
    /// no one, so the unprivileged user a sandbox runs as can read them and nothing can change them.
    pub fn write_to(&self, dir: &Path) -> Result<(), SkillError> {
        fs::create_dir(dir).map_err(|_| SkillError::Write)?;
        for (path, file) in &self.files {
            let target = dir.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|_| SkillError::Write)?;
            }
            fs::write(&target, &file.bytes).map_err(|_| SkillError::Write)?;
            let mode = if file.executable { 0o555 } else { 0o444 };
            fs::set_permissions(&target, fs::Permissions::from_mode(mode))
                .map_err(|_| SkillError::Write)?;
        }
        Ok(())
    }
}

fn plain(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn walk(
    dir: &Path,
    prefix: &str,
    depth: usize,
    files: &mut BTreeMap<String, SkillFile>,
    total: &mut u64,
) -> Result<(), SkillError> {
    if depth >= MAX_DEPTH {
        return Err(SkillError::TooLarge);
    }
    let entries = fs::read_dir(dir).map_err(|_| SkillError::Unreadable)?;
    for entry in entries {
        let entry = entry.map_err(|_| SkillError::Unreadable)?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .filter(|n| plain(n))
            .ok_or(SkillError::BadEntry)?;
        let path = format!("{prefix}{name}");
        // Not following links: the type of the entry itself.
        let kind = entry.file_type().map_err(|_| SkillError::Unreadable)?;
        if kind.is_dir() {
            walk(&entry.path(), &format!("{path}/"), depth + 1, files, total)?;
        } else if kind.is_file() {
            if files.len() >= MAX_FILES {
                return Err(SkillError::TooLarge);
            }
            let meta = entry.metadata().map_err(|_| SkillError::Unreadable)?;
            let handle = fs::File::open(entry.path()).map_err(|_| SkillError::Unreadable)?;
            let mut bytes = Vec::new();
            handle
                .take(MAX_BYTES - *total + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| SkillError::Unreadable)?;
            *total += bytes.len() as u64;
            if *total > MAX_BYTES {
                return Err(SkillError::TooLarge);
            }
            let executable = meta.permissions().mode() & 0o100 != 0;
            files.insert(path, SkillFile { bytes, executable });
        } else {
            return Err(SkillError::BadEntry);
        }
    }
    Ok(())
}

/// The `name` in `SKILL.md`'s front matter: the lines between a first line of `---` and the next.
fn name_of(skill_md: &[u8]) -> Option<PluginName> {
    let text = std::str::from_utf8(skill_md).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            return None;
        }
        if let Some(value) = line.strip_prefix("name:") {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            return PluginName::new(value);
        }
    }
    None
}

/// Where installed skills are kept: one directory per digest. A skill is used only after its files
/// are read back and found to have the digest it was installed with.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn dir(&self, digest: &Digest) -> PathBuf {
        self.root.join(digest.to_string())
    }

    /// Keeps `skill` under its digest and returns the digest. Storing the same files again changes
    /// nothing; a directory already there with other files is refused.
    pub fn put(&self, skill: &Skill) -> Result<Digest, SkillError> {
        let digest = skill.digest();
        let dir = self.dir(&digest);
        if dir.exists() {
            return self.load(&digest, &skill.name).map(|_| digest);
        }
        fs::create_dir_all(&self.root).map_err(|_| SkillError::Write)?;
        // Written beside it and renamed, so a half-written skill is never found under its digest.
        let partial = self
            .root
            .join(format!(".partial-{digest}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&partial);
        skill.write_to(&partial)?;
        fs::rename(&partial, &dir).map_err(|_| SkillError::Write)?;
        Ok(digest)
    }

    /// The skill stored under `digest`, which must be named `name`: read back and checked, so files
    /// changed after the install are refused, never used.
    pub fn load(&self, digest: &Digest, name: &PluginName) -> Result<Skill, SkillError> {
        let skill = Skill::read(&self.dir(digest)).map_err(|e| match e {
            SkillError::Unreadable => SkillError::Unreadable,
            _ => SkillError::Altered,
        })?;
        if skill.digest() != *digest || skill.name != *name {
            return Err(SkillError::Altered);
        }
        Ok(skill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hyalos-skill-{name}-{}", std::process::id()));
        let _ = make_writable(&dir);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_writable(dir: &Path) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
            if path.is_dir() {
                make_writable(&path)?;
            }
        }
        Ok(())
    }

    fn skill_dir(root: &Path) -> PathBuf {
        let dir = root.join("src");
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(
            dir.join(SKILL_FILE),
            "---\nname: pdf-tools\ndescription: Reads PDFs\n---\n# PDF\nRun scripts/extract.sh\n",
        )
        .unwrap();
        fs::write(
            dir.join(CAPABILITIES_FILE),
            r#"{"version":1,"egress_hosts":["docs.rs"]}"#,
        )
        .unwrap();
        let script = dir.join("scripts/extract.sh");
        fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }

    #[test]
    fn a_skill_is_read_with_its_name_its_asks_and_its_files() {
        let root = scratch("read");
        let skill = Skill::read(&skill_dir(&root)).unwrap();
        assert_eq!(skill.name.as_str(), "pdf-tools");
        assert_eq!(skill.asks.egress_hosts, ["docs.rs".to_string()].into());
        let paths: Vec<&str> = skill.files.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            ["SKILL.md", "capabilities.json", "scripts/extract.sh"]
        );
        assert!(skill.files["scripts/extract.sh"].executable);
        assert!(!skill.files["SKILL.md"].executable);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_digest_covers_every_path_mode_and_byte() {
        let root = scratch("digest");
        let skill = Skill::read(&skill_dir(&root)).unwrap();
        let base = skill.digest();
        assert_eq!(Skill::read(&skill_dir(&root)).unwrap().digest(), base);
        let mut other = skill.clone();
        other
            .files
            .get_mut("scripts/extract.sh")
            .unwrap()
            .executable = false;
        assert_ne!(other.digest(), base);
        let mut other = skill.clone();
        other.files.get_mut("SKILL.md").unwrap().bytes.push(b'\n');
        assert_ne!(other.digest(), base);
        let mut other = skill.clone();
        let file = other.files.remove("scripts/extract.sh").unwrap();
        other.files.insert("scripts/extract.sh2".into(), file);
        assert_ne!(other.digest(), base);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_link_a_hidden_name_or_too_much_is_refused() {
        let root = scratch("refuse");
        let dir = skill_dir(&root);
        std::os::unix::fs::symlink("/etc/passwd", dir.join("passwd")).unwrap();
        assert_eq!(Skill::read(&dir), Err(SkillError::BadEntry));
        fs::remove_file(dir.join("passwd")).unwrap();
        fs::write(dir.join(".env"), "x").unwrap();
        assert_eq!(Skill::read(&dir), Err(SkillError::BadEntry));
        fs::remove_file(dir.join(".env")).unwrap();
        fs::write(dir.join("big"), vec![0u8; MAX_BYTES as usize + 1]).unwrap();
        assert_eq!(Skill::read(&dir), Err(SkillError::TooLarge));
        fs::remove_file(dir.join("big")).unwrap();
        let mut deep = dir.clone();
        for _ in 0..MAX_DEPTH {
            deep = deep.join("d");
        }
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(Skill::read(&dir), Err(SkillError::TooLarge));
        fs::remove_dir_all(dir.join("d")).unwrap();
        assert!(Skill::read(&dir).is_ok());
        assert_eq!(
            Skill::read(&dir.join("SKILL.md")),
            Err(SkillError::Unreadable)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_skill_needs_a_skill_file_a_name_and_readable_capabilities() {
        let root = scratch("name");
        let dir = skill_dir(&root);
        for (text, want) in [
            ("# no front matter\n", Some(SkillError::NoName)),
            (
                "---\ndescription: x\n---\nname: late\n",
                Some(SkillError::NoName),
            ),
            ("---\nname: Bad_Name\n---\n", Some(SkillError::NoName)),
            ("---\nname: \"quoted\"\n---\n", None),
        ] {
            fs::write(dir.join(SKILL_FILE), text).unwrap();
            assert_eq!(Skill::read(&dir).err(), want, "{text}");
        }
        fs::write(
            dir.join(CAPABILITIES_FILE),
            r#"{"version":1,"egress_hosts":["*.x"]}"#,
        )
        .unwrap();
        assert!(matches!(
            Skill::read(&dir),
            Err(SkillError::Capabilities(CapabilityError::Unknown(_)))
        ));
        fs::remove_file(dir.join(CAPABILITIES_FILE)).unwrap();
        assert!(Skill::read(&dir).unwrap().asks.is_empty());
        fs::remove_file(dir.join(SKILL_FILE)).unwrap();
        assert_eq!(Skill::read(&dir), Err(SkillError::NoSkillFile));
        // No error names what the skill holds.
        for e in [
            SkillError::BadEntry,
            SkillError::NoName,
            SkillError::Altered,
        ] {
            assert!(!e.to_string().contains("extract"), "{e}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_store_keeps_a_skill_by_digest_and_refuses_one_changed_after() {
        let root = scratch("store");
        let skill = Skill::read(&skill_dir(&root)).unwrap();
        let store = Store::new(root.join("store"));
        let digest = store.put(&skill).unwrap();
        assert_eq!(digest, skill.digest());
        assert_eq!(
            store.put(&skill).unwrap(),
            digest,
            "storing again changes nothing"
        );
        assert_eq!(store.load(&digest, &skill.name).unwrap(), skill);
        let other = PluginName::new("other").unwrap();
        assert_eq!(store.load(&digest, &other), Err(SkillError::Altered));
        // A file changed in the store after the install is caught on the next load.
        let script = root
            .join("store")
            .join(digest.to_string())
            .join("scripts/extract.sh");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(&script, "#!/bin/sh\ncurl evil\n").unwrap();
        assert_eq!(store.load(&digest, &skill.name), Err(SkillError::Altered));
        let missing = Digest::from_bytes([0; 32]);
        assert_eq!(
            store.load(&missing, &skill.name),
            Err(SkillError::Unreadable)
        );
        let _ = make_writable(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn written_files_are_read_only_and_keep_their_mode() {
        let root = scratch("write");
        let skill = Skill::read(&skill_dir(&root)).unwrap();
        let out = root.join("out");
        skill.write_to(&out).unwrap();
        let mode = |p: &str| fs::metadata(out.join(p)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode("SKILL.md"), 0o444);
        assert_eq!(mode("scripts/extract.sh"), 0o555);
        assert_eq!(Skill::read(&out).unwrap().digest(), skill.digest());
        assert_eq!(
            skill.write_to(&out),
            Err(SkillError::Write),
            "never into an existing directory"
        );
        let _ = make_writable(&root);
        let _ = fs::remove_dir_all(root);
    }
}
