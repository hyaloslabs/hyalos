//! hyalOS's review scope and the advisory check on its upstreams (ADR-0135).
//!
//! `hyalos-os/review/scope.toml` gives every third-party component the security claim depends on a
//! stated position, row by row. `cargo xtask review-scope` holds it to the three hyalOS lockfiles, so
//! a version that moves fails until its row is read again. `cargo xtask advisories` matches the
//! published advisories of the upstreams RustSec may never see, `wasmi-labs/wasmi` and
//! `tlsnotary/tlsn`, against what those lockfiles pin. Both fail closed: a line, a feed or a range
//! they cannot read is a failure, never a pass.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// The review scope, relative to the repository root.
pub const SCOPE: &str = "hyalos-os/review/scope.toml";

/// hyalOS's lockfiles: the workspace's, and those of the kernel image and the guest, which are
/// workspaces of their own.
pub const LOCKFILES: &[&str] = &[
    "hyalos-os/Cargo.lock",
    "hyalos-os/kernel-image/Cargo.lock",
    "hyalos-os/guest/Cargo.lock",
];

/// The components ADR-005 and ADR-008 of `docs/plans/hyalos-kernel-core` split into a `use` row and
/// an `internals` row, with the position the `internals` row must hold.
const SPLIT: &[(&str, &str)] = &[
    ("wasmi", "covered"),
    ("tlsn", "excluded"),
    ("tlsn-core", "excluded"),
    ("tlsn-attestation", "excluded"),
    ("sha2", "excluded"),
    ("p256", "excluded"),
    ("k256", "excluded"),
    ("rustls", "excluded"),
    ("tokio-rustls", "excluded"),
    ("ring", "excluded"),
    ("rcgen", "excluded"),
];

/// The components ADR-005 covers "our use" of without splitting them: each needs a `covered` `use`
/// row. The `use` rows of `SPLIT`'s components must be `covered` too.
const USE_COVERED: &[&str] = &["kvm-ioctls", "kvm-bindings", "seccompiler", "landlock"];

/// The GitHub repositories whose own advisories are read, each with the crate an advisory that
/// names no package is taken to be about (`design.md`, **Modifier: integration**).
pub const FEEDS: &[(&str, &str)] = &[("wasmi-labs/wasmi", "wasmi"), ("tlsnotary/tlsn", "tlsn")];

const FIELDS: &[&str] = &[
    "name",
    "version",
    "commit",
    "part",
    "position",
    "upstream_audit",
    "reason",
];
const REQUIRED: &[&str] = &[
    "name",
    "version",
    "part",
    "position",
    "upstream_audit",
    "reason",
];

/// One `[[component]]` table of the scope, and the line it starts on.
#[derive(Debug)]
struct Row {
    line: usize,
    fields: BTreeMap<String, String>,
}

impl Row {
    fn get(&self, key: &str) -> &str {
        self.fields.get(key).map(String::as_str).unwrap_or_default()
    }
}

/// One `[[package]]` of a lockfile.
#[derive(Debug, Clone, PartialEq)]
pub struct Locked {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    deps: Vec<String>,
}

impl Locked {
    /// A git pin's tag and commit, from a source such as `git+URL?tag=v1#abc`.
    fn git(&self) -> Option<(Option<&str>, &str)> {
        let rest = self.source.as_deref()?.strip_prefix("git+")?;
        let (url, commit) = rest.split_once('#')?;
        let tag = url.split_once("?tag=").map(|(_, t)| t);
        Some((tag, commit))
    }

    /// The version an advisory's range is compared with: a git pin's tag without its `v`, or the
    /// locked version.
    fn pinned(&self) -> &str {
        match self.git() {
            Some((Some(tag), _)) => tag.strip_prefix('v').unwrap_or(tag),
            _ => &self.version,
        }
    }
}

/// A basic TOML string with no escapes: all the scope and a lockfile need. Anything else is refused.
fn string_value(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('"') && !inner.contains('\\')).then(|| inner.to_string())
}

/// Parses the scope's restricted TOML: `[[component]]` tables of `key = "string"` lines, comments and
/// blank lines. Every other line is a finding, so a field cannot hide in a form the check skips.
fn parse_scope(text: &str) -> (Vec<Row>, Vec<String>) {
    let mut rows: Vec<Row> = Vec::new();
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let at = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[component]]" {
            rows.push(Row {
                line: at,
                fields: BTreeMap::new(),
            });
            continue;
        }
        let parsed = line
            .split_once('=')
            .and_then(|(k, v)| Some((k.trim(), string_value(v.trim())?)));
        match (parsed, rows.last_mut()) {
            (Some((key, value)), Some(row)) if FIELDS.contains(&key) => {
                if row.fields.insert(key.to_string(), value).is_some() {
                    out.push(format!("{SCOPE}:{at}: `{key}` given twice in one row"));
                }
            }
            (Some((key, _)), Some(_)) => {
                out.push(format!("{SCOPE}:{at}: unknown field `{key}`"));
            }
            _ => out.push(format!(
                "{SCOPE}:{at}: not a `[[component]]` table or a `key = \"string\"` line in one"
            )),
        }
    }
    (rows, out)
}

/// The tables a Cargo lockfile holds. Any other table header is an error.
const LOCK_TABLES: &[&str] = &["[[package]]", "[[patch.unused]]", "[metadata]"];

/// Parses the `[[package]]` tables of a Cargo lockfile: their name, version, source and
/// dependencies. A package with no name or version, a line that is not a comment, a known table
/// header or `key = value`, and a lockfile with no workspace member are errors.
pub fn parse_lock(file: &str, text: &str) -> Result<Vec<Locked>, String> {
    let mut out: Vec<Locked> = Vec::new();
    let mut in_package = false;
    let mut in_deps = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let bad = || format!("{file}:{}: could not read `{line}`", i + 1);
        if in_deps {
            if line == "]" {
                in_deps = false;
            } else {
                let entry = string_value(line.trim_end_matches(',')).ok_or_else(bad)?;
                if let Some(p) = out.last_mut() {
                    p.deps.push(entry);
                }
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if !LOCK_TABLES.contains(&line) {
                return Err(bad());
            }
            in_package = line == "[[package]]";
            if in_package {
                out.push(Locked {
                    name: String::new(),
                    version: String::new(),
                    source: None,
                    deps: Vec::new(),
                });
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(bad());
        };
        let Some(p) = out.last_mut().filter(|_| in_package) else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "name" => p.name = string_value(value).ok_or_else(bad)?,
            "version" => p.version = string_value(value).ok_or_else(bad)?,
            "source" => p.source = Some(string_value(value).ok_or_else(bad)?),
            "dependencies" if value == "[" => in_deps = true,
            "dependencies" => {
                let inner = value
                    .strip_prefix('[')
                    .and_then(|v| v.strip_suffix(']'))
                    .ok_or_else(bad)?;
                for entry in inner.split(',').map(str::trim).filter(|e| !e.is_empty()) {
                    p.deps.push(string_value(entry).ok_or_else(bad)?);
                }
            }
            _ => {}
        }
    }
    if let Some(p) = out
        .iter()
        .find(|p| p.name.is_empty() || p.version.is_empty())
    {
        return Err(format!("{file}: a package with no name or version ({p:?})"));
    }
    if !out.iter().any(|p| p.source.is_none()) {
        return Err(format!(
            "{file}: no workspace member (a package with no `source`); the lockfile is empty or unread"
        ));
    }
    Ok(out)
}

/// The packages a lockfile's own members (those with no `source`) depend on directly, dev and build
/// dependencies included, other members left out. A dependency entry that names no unique package
/// is an error.
fn direct(file: &str, packages: &[Locked]) -> Result<Vec<Locked>, String> {
    let mut out = Vec::new();
    for member in packages.iter().filter(|p| p.source.is_none()) {
        for entry in &member.deps {
            let mut words = entry.split(' ');
            let name = words.next().unwrap_or_default();
            let version = words.next();
            let found: Vec<&Locked> = packages
                .iter()
                .filter(|p| p.name == name && version.is_none_or(|v| p.version == v))
                .collect();
            let [dep] = found[..] else {
                return Err(format!(
                    "{file}: {}'s dependency `{entry}` names {} packages",
                    member.name,
                    found.len()
                ));
            };
            if dep.source.is_some() && !out.contains(dep) {
                out.push(dep.clone());
            }
        }
    }
    Ok(out)
}

/// Whether `text` is a date written `YYYY-MM-DD`, with a month and a day in range.
fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        b[r.clone()].iter().all(u8::is_ascii_digit)
            && text[r].parse::<u32>().ok().is_some_and(|n| n > 0)
    };
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && digits(0..4)
        && digits(5..7)
        && digits(8..10)
        && text[5..7].parse::<u32>().is_ok_and(|m| m <= 12)
        && text[8..10].parse::<u32>().is_ok_and(|d| d <= 31)
}

/// Whether `audit` says when it was searched: `searched YYYY-MM-DD` somewhere in it.
fn has_search_date(audit: &str) -> bool {
    audit.match_indices("searched ").any(|(i, m)| {
        audit
            .get(i + m.len()..i + m.len() + 10)
            .is_some_and(is_date)
    })
}

/// Whether `audit` is exactly the utility form, `not searched (utility, YYYY-MM-DD)`.
fn is_utility_form(audit: &str) -> bool {
    audit
        .strip_prefix("not searched (utility, ")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(is_date)
}

/// Every way the scope breaks its rules or disagrees with the lockfiles, as one line each. The
/// lockfiles are `(path, text)`.
pub fn findings(scope: &str, locks: &[(String, String)]) -> Vec<String> {
    let (rows, mut out) = parse_scope(scope);
    if rows.is_empty() && out.is_empty() {
        out.push(format!("{SCOPE}: no `[[component]]` rows"));
    }
    let mut locked: Vec<Locked> = Vec::new();
    let mut wanted: Vec<Locked> = Vec::new();
    for (file, text) in locks {
        match parse_lock(file, text).and_then(|p| Ok((direct(file, &p)?, p))) {
            Ok((d, p)) => {
                wanted.extend(d);
                locked.extend(p);
            }
            Err(e) => out.push(e),
        }
    }
    let mut seen = BTreeSet::new();
    for row in &rows {
        out.extend(row_findings(row, &locked));
        let key = (row.get("name"), row.get("version"), row.get("part"));
        if !seen.insert(key) {
            out.push(format!(
                "{SCOPE}:{}: {} {} has two `{}` rows",
                row.line,
                row.get("name"),
                row.get("version"),
                row.get("part")
            ));
        }
    }
    // Every locked version of a listed component has a row, so a second copy cannot slip in beside
    // the one that was reviewed.
    let listed: BTreeSet<&str> = rows.iter().map(|r| r.get("name")).collect();
    for p in &locked {
        let label = scope_version(p);
        if listed.contains(p.name.as_str())
            && !rows
                .iter()
                .any(|r| r.get("name") == p.name && r.get("version") == label)
        {
            out.push(format!(
                "{} {label} is locked but has no row; its rows name another version, so read them again",
                p.name
            ));
        }
    }
    for p in &wanted {
        if !listed.contains(p.name.as_str()) {
            out.push(format!(
                "{} {} is a direct dependency of a hyalOS crate but has no row; a utility crate gets an `excluded` row with its reason",
                p.name,
                scope_version(p)
            ));
        }
    }
    for (name, internals) in SPLIT {
        for part in ["use", "internals"] {
            match rows
                .iter()
                .find(|r| r.get("name") == *name && r.get("part") == part)
            {
                None => out.push(format!(
                    "{name} needs a `{part}` row (ADR-005 and ADR-008 split it)"
                )),
                Some(r) if part == "internals" && r.get("position") != *internals => {
                    out.push(format!(
                        "{SCOPE}:{}: {name}'s internals must be `{internals}` (ADR-005, ADR-008)",
                        r.line
                    ))
                }
                Some(_) => {}
            }
        }
    }
    // ADR-005 and ADR-008 cover our use of every component they name.
    for name in SPLIT
        .iter()
        .map(|(n, _)| *n)
        .chain(USE_COVERED.iter().copied())
    {
        let uses: Vec<&Row> = rows
            .iter()
            .filter(|r| r.get("name") == name && r.get("part") == "use")
            .collect();
        if uses.is_empty() && USE_COVERED.contains(&name) {
            out.push(format!("{name} needs a `use` row (ADR-005)"));
        }
        for r in uses.iter().filter(|r| r.get("position") != "covered") {
            out.push(format!(
                "{SCOPE}:{}: {name}'s use must be `covered` (ADR-005, ADR-008)",
                r.line
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The version a scope row gives a package: a git pin's tag, or the locked version.
fn scope_version(p: &Locked) -> String {
    match p.git() {
        Some((Some(tag), _)) => tag.to_string(),
        _ => p.version.clone(),
    }
}

fn row_findings(row: &Row, locked: &[Locked]) -> Vec<String> {
    let at = |what: String| format!("{SCOPE}:{}: {} {what}", row.line, row.get("name"));
    let mut out = Vec::new();
    for key in REQUIRED {
        if row.get(key).trim().is_empty() {
            out.push(at(format!("has no `{key}`")));
        }
    }
    if !out.is_empty() {
        return out;
    }
    let part = row.get("part");
    if !["use", "internals"].contains(&part) {
        out.push(at(format!("has part `{part}`; it is `use` or `internals`")));
    }
    let position = row.get("position");
    if !["covered", "excluded"].contains(&position) {
        out.push(at(format!(
            "has position `{position}`; it is `covered` or `excluded`"
        )));
    }
    // A citation or `none found` says `searched YYYY-MM-DD`. Only an excluded row may say it was
    // not searched, and only in the one form, so no spelling of it slips past.
    let audit = row.get("upstream_audit");
    if audit.to_ascii_lowercase().contains("not searched") {
        if !is_utility_form(audit) {
            out.push(at(
                "has an `upstream_audit` that is not searched but not written `not searched (utility, YYYY-MM-DD)`".into(),
            ));
        }
        if position != "excluded" {
            out.push(at(
                "is `not searched` but not `excluded`; a covered row needs a real search".into(),
            ));
        }
    } else if !has_search_date(audit) {
        out.push(at(
            "has an `upstream_audit` with no date searched (`searched YYYY-MM-DD`)".into(),
        ));
    }
    let same: Vec<&Locked> = locked
        .iter()
        .filter(|p| p.name == row.get("name"))
        .collect();
    if same.is_empty() {
        out.push(at("is in no hyalOS lockfile".into()));
        return out;
    }
    let version = row.get("version");
    let commit = row.get("commit");
    let matches = same.iter().any(|p| match p.git() {
        Some((tag, locked_commit)) => tag == Some(version) && locked_commit == commit,
        None => p.version == version && commit.is_empty(),
    });
    if !matches {
        let have: Vec<String> = same
            .iter()
            .map(|p| match p.git() {
                Some((tag, c)) => format!("tag {} commit {c}", tag.unwrap_or("(none)")),
                None => p.version.clone(),
            })
            .collect();
        let given = if commit.is_empty() {
            version.to_string()
        } else {
            format!("tag {version} commit {commit}")
        };
        out.push(at(format!(
            "row says {given}, the lockfiles pin {}; read the row again before changing it",
            have.join(", ")
        )));
    }
    out
}

/// Reads the scope and the lockfiles under `root`.
fn read_inputs(root: &Path) -> Result<(String, Vec<(String, String)>), String> {
    let scope = fs::read_to_string(root.join(SCOPE)).map_err(|e| format!("{SCOPE}: {e}"))?;
    let mut locks = Vec::new();
    for file in LOCKFILES {
        let text = fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
        locks.push((file.to_string(), text));
    }
    Ok((scope, locks))
}

/// `cargo xtask review-scope`.
pub fn check(root: &Path) -> Result<(), String> {
    let (scope, locks) = read_inputs(root)?;
    let found = findings(&scope, &locks);
    if found.is_empty() {
        let (rows, _) = parse_scope(&scope);
        println!(
            "hyalOS review scope: {} rows agree with {} lockfiles",
            rows.len(),
            locks.len()
        );
        Ok(())
    } else {
        Err(format!(
            "{}\nthe review scope must name every pinned component it lists, and every direct \
             dependency of a hyalOS crate, at its locked version (ADR-0135)",
            found.join("\n")
        ))
    }
}

// ---- advisories ---------------------------------------------------------------------------------

/// A version as SemVer orders it: the numbers, then the pre-release identifiers (none sorts last).
#[derive(Debug, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = text.split('+').next()?;
        let (nums, pre) = match text.split_once('-') {
            Some((n, p)) => (n, p.split('.').map(str::to_string).collect()),
            None => (text, Vec::new()),
        };
        let mut core = [0u64; 3];
        let parts: Vec<&str> = nums.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        for (slot, part) in core.iter_mut().zip(&parts) {
            *slot = part.parse().ok()?;
        }
        let pre_ok = pre.iter().all(|p: &String| {
            !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
        pre_ok.then_some(Self { core, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        self.core
            .cmp(&other.core)
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&other.pre) {
                        let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Whether `version` is in an advisory's affected range, written as GitHub writes them: constraints
/// such as `>= 2.0.0-beta.4, <= 2.0.2` or `>= 0.41.0 and <= 1.0.0`, every one of which must hold. An
/// empty range affects every version. A constraint that cannot be read is an error.
fn in_range(version: &str, range: &str) -> Result<bool, String> {
    let v = Version::parse(version).ok_or_else(|| format!("cannot read version `{version}`"))?;
    let range = range.replace(" and ", ",").replace("&&", ",");
    let mut all = true;
    for c in range.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        let (op, rest) = ["<=", ">=", "==", "<", ">", "="]
            .iter()
            .find_map(|op| c.strip_prefix(op).map(|r| (*op, r)))
            .unwrap_or(("=", c));
        let bound =
            Version::parse(rest).ok_or_else(|| format!("cannot read range `{range}` at `{c}`"))?;
        all &= match op {
            "<=" => v <= bound,
            ">=" => v >= bound,
            "<" => v < bound,
            ">" => v > bound,
            _ => v == bound,
        };
    }
    Ok(all)
}

/// The advisories in `feeds` (each a repository and the JSON GitHub's REST API returned for its
/// security advisories) whose affected range includes a version the lockfiles pin, one line each,
/// naming the advisory. A feed that is missing or cannot be read is an error.
pub fn advisory_findings(
    feeds: &[(String, String)],
    locked: &[Locked],
) -> Result<Vec<String>, String> {
    for (repo, _) in FEEDS {
        if !feeds.iter().any(|(r, _)| r == repo) {
            return Err(format!("no feed was read for {repo}"));
        }
    }
    let mut out = Vec::new();
    for (repo, json) in feeds {
        let default = FEEDS
            .iter()
            .find(|(r, _)| r == repo)
            .map(|(_, c)| *c)
            .ok_or_else(|| format!("{repo} is not one of the feeds this check reads"))?;
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| format!("{repo}: the feed is not JSON ({e})"))?;
        let serde_json::Value::Array(items) = value else {
            return Err(format!("{repo}: the feed is not a list of advisories"));
        };
        // `gh api --paginate --slurp` wraps each page in a list of its own.
        let mut advisories = Vec::new();
        for item in items {
            match item {
                serde_json::Value::Array(page) => advisories.extend(page),
                other => advisories.push(other),
            }
        }
        for a in &advisories {
            out.extend(one_advisory(repo, default, a, locked)?);
        }
    }
    // A crate locked alike in two lockfiles is one finding.
    out.sort();
    out.dedup();
    Ok(out)
}

fn one_advisory(
    repo: &str,
    default: &str,
    a: &serde_json::Value,
    locked: &[Locked],
) -> Result<Vec<String>, String> {
    let id = a["ghsa_id"]
        .as_str()
        .ok_or_else(|| format!("{repo}: an advisory with no `ghsa_id`"))?;
    let state = a["state"].as_str().unwrap_or("published");
    if matches!(state, "withdrawn" | "closed") || !a["withdrawn_at"].is_null() {
        return Ok(Vec::new());
    }
    let empty = Vec::new();
    let vulns = a["vulnerabilities"].as_array().unwrap_or(&empty);
    // An advisory that names no package or range is taken to affect every version of the
    // repository's crate.
    let unnamed = serde_json::json!({});
    let vulns: Vec<&serde_json::Value> = if vulns.is_empty() {
        vec![&unnamed]
    } else {
        vulns.iter().collect()
    };
    let mut out = Vec::new();
    for v in vulns {
        let ecosystem = v["package"]["ecosystem"].as_str().unwrap_or("rust");
        if !ecosystem.eq_ignore_ascii_case("rust") {
            continue;
        }
        let name = v["package"]["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .unwrap_or(default);
        let range = v["vulnerable_version_range"].as_str().unwrap_or_default();
        // Cargo treats `-` and `_` in a crate's name alike, so an advisory may spell it either way.
        let same = |locked: &str| locked.replace('-', "_") == name.replace('-', "_");
        for p in locked.iter().filter(|p| same(&p.name)) {
            if in_range(p.pinned(), range).map_err(|e| format!("{repo}: {id}: {e}"))? {
                out.push(format!(
                    "{id} ({repo}): {name} {} is in its affected range `{range}`",
                    p.pinned()
                ));
            }
        }
    }
    Ok(out)
}

/// `cargo xtask advisories <owner/repo>=<file> ...`: fails, naming each advisory, when a saved feed
/// holds one whose range includes a pinned version.
pub fn check_advisories(root: &Path, args: &[String]) -> Result<(), String> {
    let mut feeds = Vec::new();
    for arg in args {
        let (repo, file) = arg
            .split_once('=')
            .ok_or_else(|| format!("`{arg}`: expected <owner/repo>=<file>"))?;
        let json = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        feeds.push((repo.to_string(), json));
    }
    let (_, locks) = read_inputs(root)?;
    let mut locked = Vec::new();
    for (file, text) in &locks {
        locked.extend(parse_lock(file, text)?);
    }
    let found = advisory_findings(&feeds, &locked)?;
    if found.is_empty() {
        println!(
            "hyalOS: no published advisory of {} affects a pinned version",
            FEEDS
                .iter()
                .map(|(r, _)| *r)
                .collect::<Vec<_>>()
                .join(" or ")
        );
        Ok(())
    } else {
        Err(format!(
            "{}\nan upstream advisory affects a version hyalOS pins. This check has no exception \
             list: move the pin, or take the advisory to the developer (ADR-0135)",
            found.join("\n")
        ))
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
