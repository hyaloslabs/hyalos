//! Agent Skills on disk (ADR-0006, ADR-0089): reading one from a directory or from its archive
//! (ADR-0090), the digest that binds an install to its exact files, and the store that keeps
//! installed skills by that digest.
//!
//! Everything in a skill is plugin-authored: its file names, its `SKILL.md` and its scripts. So a
//! skill is read within limits, only regular files and directories are taken (never a link, which
//! could point anywhere on the operator's machine), and every name must be plain. An error is a
//! reason code and never names a file the skill holds.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hyalos_types::{Capabilities, CapabilityError, Digest, PluginName, PluginVersion, ReasonCode};
use sha2::{Digest as _, Sha256};

/// The file every skill has, at its root.
pub const SKILL_FILE: &str = "SKILL.md";
/// The capabilities a skill asks for, at its root, as a capabilities document (ADR-0085). A skill
/// without one asks for none.
pub const CAPABILITIES_FILE: &str = "capabilities.json";
/// The MCP server a skill runs, at its root, if it is one (ADR-0101): `{"command": .., "args": [..]}`.
pub const MCP_FILE: &str = "mcp.json";
/// The most arguments an MCP server's command may have.
pub const MCP_ARGS: usize = 32;
/// The longest an MCP server's command or one of its arguments may be.
pub const MCP_ARG_BYTES: usize = 512;
/// A plugin's WASM component, at its root, if it has one: what a deterministic step runs (ADR-0102).
pub const COMPONENT_FILE: &str = "component.wasm";
/// The most files a skill may hold.
pub const MAX_FILES: usize = 256;
/// The most bytes a skill's files may hold together.
pub const MAX_BYTES: u64 = 8 << 20;
/// How deep a skill's directories may go.
pub const MAX_DEPTH: usize = 8;

/// What a skill's archive, and so its digest, starts with, so it can never be mistaken for anything
/// else.
const DOMAIN: &[u8] = b"hyalos.skill.v1\n";
/// The longest path an archive may hold.
const MAX_PATH: usize = 1024;
/// The most bytes an archive may hold: the files' bytes, and each file's path and lengths.
pub const MAX_ARCHIVE: u64 = MAX_BYTES + (MAX_FILES * (MAX_PATH + 13)) as u64 + DOMAIN.len() as u64;

/// One file of a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFile {
    pub bytes: Vec<u8>,
    /// Whether its owner may run it. Scripts keep this, so the bit is part of the digest.
    pub executable: bool,
}

/// A skill as read: its name, optional declared version, what it asks for and its files by
/// relative path (`/`-separated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: PluginName,
    /// Optional `version` declared in `SKILL.md` front matter; when present, an index install must
    /// match it.
    pub version: Option<PluginVersion>,
    pub asks: Capabilities,
    /// The MCP server it runs in the job's sandbox, if it has an `mcp.json` (ADR-0101).
    pub mcp: Option<McpServer>,
    pub files: BTreeMap<String, SkillFile>,
}

/// Why a skill cannot be read or used. Names nothing the skill holds.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillError {
    /// The path is not a directory that can be read.
    #[error("not a directory that can be read")]
    Unreadable,
    /// It has no `SKILL.md` at its root.
    #[error("no {SKILL_FILE} at its root")]
    NoSkillFile,
    /// `SKILL.md` has no front matter with a `name` that is a plugin name.
    #[error("{SKILL_FILE} has no front matter naming it in lowercase letters, digits and hyphens")]
    NoName,
    /// `SKILL.md` front matter declares a `version` that is not a valid plugin version.
    #[error("{SKILL_FILE} front matter has an invalid version")]
    BadVersion,
    /// A file or directory name that is not plain, a link, or something that is neither a file nor a
    /// directory.
    #[error(
        "it holds a link, a special file, or a name that is not plain letters, digits, '.', '_' and '-'"
    )]
    BadEntry,
    /// More files, bytes or depth than a skill may have.
    #[error(
        "larger than a skill may be ({MAX_FILES} files, {MAX_BYTES} bytes, {MAX_DEPTH} levels)"
    )]
    TooLarge,
    /// `capabilities.json` is not a document this build reads.
    #[error("{CAPABILITIES_FILE}: {0}")]
    Capabilities(CapabilityError),
    /// `mcp.json` is not a command this build runs.
    #[error(
        "{MCP_FILE} is not a command of printable ASCII, with at most {MCP_ARGS} arguments, naming a program by its name or an executable file of the skill by ./path"
    )]
    Mcp,
    /// The stored files do not have the digest they were installed with, or another name.
    #[error("its stored files are not the ones that were installed")]
    Altered,
    /// Writing to the store or a job's directory failed.
    #[error("cannot write its files")]
    Write,
}

impl SkillError {
    pub fn code(&self) -> ReasonCode {
        let code = match self {
            Self::Unreadable => "plugin.unreadable",
            Self::NoSkillFile => "plugin.no_skill_file",
            Self::NoName => "plugin.no_name",
            Self::BadVersion => "plugin.bad_version",
            Self::BadEntry => "plugin.bad_entry",
            Self::TooLarge => "plugin.too_large",
            Self::Capabilities(_) => "plugin.bad_capabilities",
            Self::Mcp => "plugin.bad_mcp",
            Self::Altered => "plugin.altered",
            Self::Write => "plugin.write_failed",
        };
        ReasonCode::new(code).unwrap_or_else(ReasonCode::unavailable)
    }
}

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
        Self::from_files(files)
    }

    fn from_files(files: BTreeMap<String, SkillFile>) -> Result<Skill, SkillError> {
        let skill_md = files.get(SKILL_FILE).ok_or(SkillError::NoSkillFile)?;
        let (name, version) = front_matter(&skill_md.bytes)?;
        let asks = match files.get(CAPABILITIES_FILE) {
            Some(file) => {
                Capabilities::from_document(&file.bytes).map_err(SkillError::Capabilities)?
            }
            None => Capabilities::none(),
        };
        let mcp = match files.get(MCP_FILE) {
            Some(file) => Some(McpServer::read(&file.bytes, &files).ok_or(SkillError::Mcp)?),
            None => None,
        };
        Ok(Skill {
            name,
            version,
            asks,
            mcp,
            files,
        })
    }

    /// The SHA-256 of the skill's archive: what an install records, so the decision covers exactly
    /// these files.
    pub fn digest(&self) -> Digest {
        digest_of(&self.archive())
    }

    /// The skill as one file (ADR-0090): a domain tag, then for each file in path order the path's
    /// length (4 bytes, big-endian), the path, 1 if it is executable or 0, the length of its bytes
    /// (8 bytes, big-endian) and the bytes. Its SHA-256 is the skill's digest.
    pub fn archive(&self) -> Vec<u8> {
        let mut out = DOMAIN.to_vec();
        for (path, file) in &self.files {
            out.extend_from_slice(&(path.len() as u32).to_be_bytes());
            out.extend_from_slice(path.as_bytes());
            out.push(u8::from(file.executable));
            out.extend_from_slice(&(file.bytes.len() as u64).to_be_bytes());
            out.extend_from_slice(&file.bytes);
        }
        out
    }

    /// Reads an archive with the limits and names a directory is read with. Paths must be in order,
    /// each once, and none may be a directory of another.
    pub fn from_archive(bytes: &[u8]) -> Result<Skill, SkillError> {
        if bytes.len() as u64 > MAX_ARCHIVE {
            return Err(SkillError::TooLarge);
        }
        let mut files: BTreeMap<String, SkillFile> = BTreeMap::new();
        let mut total = 0u64;
        let mut rest = bytes.strip_prefix(DOMAIN).ok_or(SkillError::Unreadable)?;
        while !rest.is_empty() {
            let (path, file, tail) = entry(rest)?;
            rest = tail;
            if files.len() >= MAX_FILES {
                return Err(SkillError::TooLarge);
            }
            total += file.bytes.len() as u64;
            if total > MAX_BYTES {
                return Err(SkillError::TooLarge);
            }
            let segments: Vec<&str> = path.split('/').collect();
            if segments.len() > MAX_DEPTH {
                return Err(SkillError::TooLarge);
            }
            let ordered = files.keys().next_back().is_none_or(|last| *last < path);
            let parent_is_file =
                (1..segments.len()).any(|n| files.contains_key(&segments[..n].join("/")));
            if !segments.iter().all(|s| plain(s)) || !ordered || parent_is_file {
                return Err(SkillError::BadEntry);
            }
            files.insert(path, file);
        }
        Self::from_files(files)
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

/// The stdio MCP server a skill runs (ADR-0101). It runs in the job's sandbox like the skill's
/// scripts, so it can do no more than the job, which the skill's grant has already cut.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServer {
    /// A program on the sandbox's `PATH` by its plain name, or `./path` to an executable file of
    /// the skill.
    pub command: String,
    /// Its arguments. One starting `./` names a file of the skill.
    #[serde(default)]
    pub args: Vec<String>,
}

impl McpServer {
    /// Reads `mcp.json`. Every string is printable ASCII within the limits, so it can be shown to the
    /// operator and passed to a harness as it is; a `./` path must be a file of the skill, and the
    /// command's must be executable.
    fn read(bytes: &[u8], files: &BTreeMap<String, SkillFile>) -> Option<McpServer> {
        let server: McpServer = serde_json::from_slice(bytes).ok()?;
        let printable = |s: &String| {
            (1..=MCP_ARG_BYTES).contains(&s.len()) && s.bytes().all(|b| (0x20..0x7f).contains(&b))
        };
        let file = |s: &str| s.strip_prefix("./").map(|path| files.get(path));
        let command = match file(&server.command) {
            Some(found) => found.is_some_and(|f| f.executable),
            None => plain(&server.command),
        };
        let args = server.args.len() <= MCP_ARGS
            && server.args.iter().all(printable)
            && server
                .args
                .iter()
                .all(|a| file(a).is_none_or(|f| f.is_some()));
        (command && args && printable(&server.command)).then_some(server)
    }

    /// The command and its arguments, with each `./` path made absolute under `root`, the skill's
    /// directory in the sandbox.
    pub fn launch(&self, root: &str) -> (String, Vec<String>) {
        let place = |s: &String| match s.strip_prefix("./") {
            Some(path) => format!("{root}/{path}"),
            None => s.clone(),
        };
        (place(&self.command), self.args.iter().map(place).collect())
    }
}

/// The SHA-256 of an archive, which is the digest of the skill in it.
pub fn digest_of(archive: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(archive).into())
}

/// One entry of an archive: its path, its file and what follows it.
fn entry(bytes: &[u8]) -> Result<(String, SkillFile, &[u8]), SkillError> {
    let short = SkillError::Unreadable;
    let (len, bytes) = bytes.split_first_chunk::<4>().ok_or(short.clone())?;
    let len = u32::from_be_bytes(*len) as usize;
    if len == 0 || len > MAX_PATH || bytes.len() < len {
        return Err(SkillError::BadEntry);
    }
    let (path, bytes) = bytes.split_at(len);
    let path = std::str::from_utf8(path).map_err(|_| SkillError::BadEntry)?;
    let (mode, bytes) = bytes.split_first().ok_or(short.clone())?;
    let executable = match mode {
        0 => false,
        1 => true,
        _ => return Err(SkillError::BadEntry),
    };
    let (size, bytes) = bytes.split_first_chunk::<8>().ok_or(short.clone())?;
    let size = u64::from_be_bytes(*size);
    if size > MAX_BYTES {
        return Err(SkillError::TooLarge);
    }
    let size = size as usize;
    if bytes.len() < size {
        return Err(short);
    }
    let (content, bytes) = bytes.split_at(size);
    Ok((
        path.to_string(),
        SkillFile {
            bytes: content.to_vec(),
            executable,
        },
        bytes,
    ))
}

/// Whether `name` is a plain file or directory name: letters, digits, `.`, `_` and `-`, starting with
/// a letter, a digit or `_`, so it is never hidden, `.` or `..`.
pub fn plain(name: &str) -> bool {
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

/// The `name` and optional `version` in `SKILL.md`'s front matter: the lines between a first line
/// of `---` and the next.
fn front_matter(skill_md: &[u8]) -> Result<(PluginName, Option<PluginVersion>), SkillError> {
    let text = std::str::from_utf8(skill_md).map_err(|_| SkillError::NoName)?;
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(SkillError::NoName);
    }
    fn unquote(raw: &str) -> &str {
        let value = raw.trim();
        value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value)
    }
    let (mut name, mut version) = (None, None);
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            break;
        }
        if name.is_none()
            && let Some(value) = line.strip_prefix("name:")
        {
            name = Some(PluginName::new(unquote(value)).ok_or(SkillError::NoName)?);
        } else if version.is_none()
            && let Some(value) = line.strip_prefix("version:")
        {
            version = Some(PluginVersion::new(unquote(value)).ok_or(SkillError::BadVersion)?);
        }
    }
    Ok((name.ok_or(SkillError::NoName)?, version))
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
    fn an_mcp_server_is_read_and_placed_under_the_skills_directory() {
        let root = scratch("mcp");
        let dir = skill_dir(&root);
        assert_eq!(Skill::read(&dir).unwrap().mcp, None);
        let server = |json: &str| {
            fs::write(dir.join(MCP_FILE), json).unwrap();
            Skill::read(&dir).map(|s| s.mcp.unwrap())
        };
        let read =
            server(r#"{"command":"./scripts/extract.sh","args":["--port","./SKILL.md"]}"#).unwrap();
        assert_eq!(
            read.launch("/opt/hyalos/skills/pdf-tools"),
            (
                "/opt/hyalos/skills/pdf-tools/scripts/extract.sh".to_string(),
                vec![
                    "--port".to_string(),
                    "/opt/hyalos/skills/pdf-tools/SKILL.md".to_string()
                ]
            )
        );
        assert_eq!(
            server(r#"{"command":"node"}"#).unwrap().args,
            Vec::<String>::new()
        );
        // Not a program by name, not an executable file of the skill, a file it lacks, text that
        // is not printable ASCII, too many arguments or a field this build does not know.
        let too_many = format!(
            r#"{{"command":"node","args":{:?}}}"#,
            vec!["a"; MCP_ARGS + 1]
        );
        for bad in [
            r#"{"command":"/bin/sh"}"#,
            r#"{"command":"../sh"}"#,
            r#"{"command":"./SKILL.md"}"#,
            r#"{"command":"./missing"}"#,
            r#"{"command":"node","args":["./missing"]}"#,
            r#"{"command":"node","args":["a[2J"]}"#,
            r#"{"command":"node","args":[""]}"#,
            r#"{"command":"node","env":{"A":"b"}}"#,
            r#"{"args":[]}"#,
            "not json",
            too_many.as_str(),
        ] {
            assert_eq!(server(bad), Err(SkillError::Mcp), "{bad}");
        }
        assert_eq!(SkillError::Mcp.code().as_str(), "plugin.bad_mcp");
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
    fn an_archive_reads_back_to_the_same_skill_and_hashes_to_its_digest() {
        let root = scratch("archive");
        let skill = Skill::read(&skill_dir(&root)).unwrap();
        let archive = skill.archive();
        assert_eq!(digest_of(&archive), skill.digest());
        assert_eq!(Skill::from_archive(&archive).unwrap(), skill);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_archive_out_of_order_with_a_bad_name_or_cut_short_is_refused() {
        fn archive(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
            let mut out = DOMAIN.to_vec();
            for (path, mode, bytes) in entries {
                out.extend_from_slice(&(path.len() as u32).to_be_bytes());
                out.extend_from_slice(path.as_bytes());
                out.push(*mode);
                out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
                out.extend_from_slice(bytes);
            }
            out
        }
        let md: &[u8] = b"---\nname: pdf\n---\n";
        assert!(Skill::from_archive(&archive(&[("SKILL.md", 0, md)])).is_ok());
        for (entries, want) in [
            (
                vec![("SKILL.md", 0, md), ("A", 0, b"x" as &[u8])],
                SkillError::BadEntry,
            ),
            (
                vec![("SKILL.md", 0, md), ("SKILL.md", 0, md)],
                SkillError::BadEntry,
            ),
            (
                vec![("../x", 0, b"x" as &[u8]), ("SKILL.md", 0, md)],
                SkillError::BadEntry,
            ),
            (
                vec![(".env", 0, b"x" as &[u8]), ("SKILL.md", 0, md)],
                SkillError::BadEntry,
            ),
            (
                vec![("SKILL.md", 0, md), ("a", 0, b"x"), ("a/b", 0, b"x")],
                SkillError::BadEntry,
            ),
            (vec![("SKILL.md", 2, md)], SkillError::BadEntry),
            (
                vec![("a/b/c/d/e/f/g/h/i", 0, b"x" as &[u8])],
                SkillError::TooLarge,
            ),
            (vec![("README", 0, b"x" as &[u8])], SkillError::NoSkillFile),
        ] {
            assert_eq!(
                Skill::from_archive(&archive(&entries)),
                Err(want),
                "{entries:?}"
            );
        }
        let whole = archive(&[("SKILL.md", 0, md)]);
        assert_eq!(
            Skill::from_archive(&whole[..whole.len() - 1]),
            Err(SkillError::Unreadable)
        );
        assert_eq!(Skill::from_archive(b"tar"), Err(SkillError::Unreadable));
        let mut huge = DOMAIN.to_vec();
        huge.extend_from_slice(&8u32.to_be_bytes());
        huge.extend_from_slice(b"SKILL.md\0");
        huge.extend_from_slice(&u64::MAX.to_be_bytes());
        assert_eq!(Skill::from_archive(&huge), Err(SkillError::TooLarge));
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

    #[test]
    fn front_matter_parses_optional_version_and_refuses_bad_one() {
        let from_md = |md: &[u8]| {
            Skill::from_files(BTreeMap::from([(
                SKILL_FILE.to_string(),
                SkillFile {
                    bytes: md.to_vec(),
                    executable: false,
                },
            )]))
        };
        let skill = from_md(b"---\nname: pdf-tools\nversion: \"1.2.0\"\n---\nBody.\n").unwrap();
        assert_eq!(skill.version.as_ref().map(|v| v.as_str()), Some("1.2.0"));

        let without = from_md(b"---\nname: pdf-tools\n---\nBody.\n").unwrap();
        assert_eq!(without.version, None);

        let bad = from_md(b"---\nname: pdf-tools\nversion: ../bad\n---\nBody.\n");
        assert_eq!(bad, Err(SkillError::BadVersion));
    }
}
