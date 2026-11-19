//! Repository checks for the rules in ADR-0017.
//!
//! * `cargo xtask layout` fails if the workspace breaks a dependency or placement rule.
//! * `cargo xtask lines` reports the trusted-core size and fails if it passes the budget.
//! * `cargo xtask docs` fails if a security page cites code or a test that is not there (#333).
//!
//! This crate is the control, so it is listed in `CODEOWNERS`. It is not trusted core and is not counted.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod refs;

/// The trusted-core budget from the PRD's success metrics.
const BUDGET: usize = 30_000;

/// Budgets for parts of the trusted core, each set by the story that planned the part. A path is a
/// directory under `core/`; its counted lines may not pass the number.
const PART_BUDGETS: &[(&str, usize)] = &[
    // The model protocols: both adapters, the relay, metering and the loopback transport (S3.8, ADR-0037).
    ("broker/src/model", 1_200),
    // The MCP protocol layer and its two transports (S5.1, ADR-0067). MCP revises yearly.
    ("hyalos/src/mcp", 1_000),
    // The plugin runtime: the Wasmtime host, install, Sigstore verification and update checks (S6.8,
    // ADR-0086). Set before any of it was written.
    ("plugin/src", 3_000),
];

/// For each crate in `core/`, the workspace crates it may depend on outside dev-dependencies.
/// A new core crate, or a new edge, is a change to this table and so is reviewed (ADR-0017).
const CORE: &[(&str, &[&str])] = &[
    ("hyalos-types", &[]),
    ("hyalos-log", &["hyalos-types"]),
    ("hyalos-policy", &["hyalos-types"]),
    ("hyalos-substrate", &["hyalos-types"]),
    (
        "hyalos-engine",
        &[
            "hyalos-types",
            "hyalos-log",
            "hyalos-policy",
            "hyalos-substrate",
        ],
    ),
    ("hyalos-broker", &["hyalos-types"]),
    ("hyalos-plugin", &["hyalos-types"]),
    (
        "hyalos",
        &[
            "hyalos-types",
            "hyalos-log",
            "hyalos-policy",
            "hyalos-substrate",
            "hyalos-engine",
            "hyalos-broker",
            "hyalos-plugin",
        ],
    ),
];

/// The crates whose code must be a pure function of its inputs: they get the determinism lint
/// (ADR-0013, ADR-0017) and may not add external dependencies freely.
const DETERMINISTIC: &[&str] = &[
    "hyalos-types",
    "hyalos-log",
    "hyalos-policy",
    "hyalos-engine",
];

/// External crates each deterministic crate may depend on. A crate that reads the clock or a
/// thread-local generator would get past the lint through a dependency, so each entry is a reviewed
/// decision with the reason it is deterministic. A transitive dependency is not checked here;
/// `cargo deny` and review cover it.
const EXTERNAL_ALLOWED: &[(&str, &[&str])] = &[
    (
        "hyalos-types",
        &[
            // Typed events and their JSON encoding (ADR-0019). Serialization is a pure function of
            // the value; neither crate reads the clock or an entropy source.
            "serde",
            "serde_json",
            // Ed25519 for the job manifest (ADR-0031), the same crate and settings as the log's
            // signatures. Signing is deterministic (RFC 8032); keys come in as bytes, and the
            // `rand_core` feature that could generate one is off.
            "ed25519-dalek",
        ],
    ),
    (
        "hyalos-log",
        &[
            // SQLite, bundled so the engine runs the version we tested. Storage, not time or randomness.
            "rusqlite",
            // Ed25519 signatures. Signing is deterministic (RFC 8032); keys come in as bytes, and the
            // `rand_core` feature that could generate one is off.
            "ed25519-dalek",
            // SHA-256 for the hash chain.
            "sha2",
        ],
    ),
    (
        "hyalos-policy",
        &[
            // The Cedar evaluator (ADR-0008). Evaluation is a pure function of the policies, the
            // entities and the request; with default features off the language has no extension
            // that reads the clock (the `datetime` feature parses values it is given and has no
            // `now`), and no code path of ours takes time or randomness from it. Its own
            // dependencies are checked by `cargo deny` and review, as for every crate here.
            "cedar-policy",
        ],
    ),
];

#[derive(Debug, Clone)]
struct Package {
    name: String,
    /// Directory relative to the repository root, with forward slashes, such as `core/engine`.
    dir: String,
    /// Workspace crates this one depends on.
    deps: Vec<Dep>,
    /// Everything else it depends on: registry, git and path dependencies outside the workspace.
    external: Vec<Dep>,
}

#[derive(Debug, Clone)]
struct Dep {
    name: String,
    dev: bool,
}

fn main() -> ExitCode {
    let root = repo_root();
    let result = match std::env::args().nth(1).as_deref() {
        Some("layout") => layout(&root),
        Some("lines") => lines(&root),
        Some("docs") => refs::check(&root),
        _ => {
            eprintln!("usage: cargo xtask <layout|lines|docs>");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn repo_root() -> PathBuf {
    // crates/xtask -> repository root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

// ---- layout ---------------------------------------------------------------------------------

fn layout(root: &Path) -> Result<(), String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("could not run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let packages = parse_metadata(&output.stdout, root)?;
    let mut problems = violations(&packages);
    let configs: Vec<(String, Option<String>)> = packages
        .iter()
        .filter(|p| DETERMINISTIC.contains(&p.name.as_str()))
        .map(|p| {
            let text = fs::read_to_string(root.join(&p.dir).join("clippy.toml")).ok();
            (p.name.clone(), text)
        })
        .collect();
    problems.extend(config_violations(&configs));
    if problems.is_empty() {
        println!("layout ok: {} workspace crates", packages.len());
        Ok(())
    } else {
        Err(format!(
            "layout violations (ADR-0017):\n  - {}",
            problems.join("\n  - ")
        ))
    }
}

fn parse_metadata(json: &[u8], root: &Path) -> Result<Vec<Package>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| format!("bad cargo metadata: {e}"))?;
    let raw = value["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;
    let names: BTreeSet<&str> = raw.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut packages = Vec::new();
    for p in raw {
        let name = p["name"].as_str().ok_or("package without a name")?;
        let manifest = p["manifest_path"]
            .as_str()
            .ok_or("package without a path")?;
        let dir = Path::new(manifest)
            .parent()
            .and_then(|d| d.strip_prefix(root).ok())
            .map(|d| d.to_string_lossy().replace('\\', "/"))
            .ok_or_else(|| format!("{name} is outside the repository"))?;
        let mut deps = Vec::new();
        let mut external = Vec::new();
        for d in p["dependencies"].as_array().into_iter().flatten() {
            let dep_name = d["name"].as_str().unwrap_or_default();
            let dep = Dep {
                name: dep_name.to_string(),
                dev: d["kind"].as_str() == Some("dev"),
            };
            // A workspace dependency is a path dependency on another workspace member.
            if d["path"].is_string() && names.contains(dep_name) {
                deps.push(dep);
            } else {
                external.push(dep);
            }
        }
        packages.push(Package {
            name: name.to_string(),
            dir,
            deps,
            external,
        });
    }
    Ok(packages)
}

fn violations(packages: &[Package]) -> Vec<String> {
    let allowed: BTreeMap<&str, BTreeSet<&str>> = CORE
        .iter()
        .map(|(name, deps)| (*name, deps.iter().copied().collect()))
        .collect();
    let dir_of: BTreeMap<&str, &str> = packages
        .iter()
        .map(|p| (p.name.as_str(), p.dir.as_str()))
        .collect();
    let mut out = Vec::new();

    for p in packages {
        let in_core = p.dir.starts_with("core/");
        if !in_core && !p.dir.starts_with("crates/") {
            out.push(format!(
                "{} is in `{}`; workspace crates live in `core/` or `crates/`",
                p.name, p.dir
            ));
            continue;
        }
        if !in_core {
            continue;
        }
        let Some(permitted) = allowed.get(p.name.as_str()) else {
            out.push(format!(
                "{} is a new crate in `core/`; add it to the table in crates/xtask and to ADR-0017",
                p.name
            ));
            continue;
        };
        for d in p.deps.iter().filter(|d| !d.dev) {
            let target_dir = dir_of.get(d.name.as_str()).copied().unwrap_or("");
            if target_dir.starts_with("crates/") {
                out.push(format!(
                    "{} depends on {} from `crates/`; the trusted core never depends on it except as a dev-dependency",
                    p.name, d.name
                ));
            } else if !permitted.contains(d.name.as_str()) {
                out.push(format!("{} must not depend on {}", p.name, d.name));
            }
        }
    }
    for p in packages
        .iter()
        .filter(|p| DETERMINISTIC.contains(&p.name.as_str()))
    {
        for d in p.external.iter().filter(|d| !d.dev) {
            let allowed = EXTERNAL_ALLOWED
                .iter()
                .any(|(krate, deps)| *krate == p.name && deps.contains(&d.name.as_str()));
            if !allowed {
                out.push(format!(
                    "{} adds the external dependency {}; deterministic crates take none without a reviewed entry for that crate in EXTERNAL_ALLOWED (ADR-0013)",
                    p.name, d.name
                ));
            }
        }
    }
    for (name, _) in CORE {
        if !dir_of.contains_key(name) {
            out.push(format!(
                "{name} is in the layout table but missing from the workspace"
            ));
        }
    }
    out
}

/// Each deterministic crate must carry the same `clippy.toml`, so the lint cannot be weakened in one
/// place. `files` pairs a crate name with its file's content, or `None` when the file is missing.
fn config_violations(files: &[(String, Option<String>)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut first: Option<(&str, &str)> = None;
    for (name, content) in files {
        match (content, first) {
            (None, _) => out.push(format!("{name} has no clippy.toml; deterministic crates need the determinism lint")),
            (Some(c), None) => first = Some((name, c)),
            (Some(c), Some((owner, expected))) if c != expected => out.push(format!(
                "{name}/clippy.toml differs from {owner}/clippy.toml; the determinism lint must be identical in every deterministic crate"
            )),
            _ => {}
        }
    }
    out
}

// ---- lines ----------------------------------------------------------------------------------

fn lines(root: &Path) -> Result<(), String> {
    let core = root.join("core");
    let mut per_crate: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_part: BTreeMap<&str, usize> = BTreeMap::new();
    let mut files = Vec::new();
    collect(&core, &mut files).map_err(|e| format!("could not read core/: {e}"))?;
    for file in files {
        let rel = file.strip_prefix(&core).map_err(|e| e.to_string())?;
        let mut parts = rel.components();
        let Some(krate) = parts
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
        else {
            continue;
        };
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        let in_src = parts.next().is_some_and(|c| c.as_os_str() == "src");
        // Only what is counted is read, so a binary test fixture under core/ is not an error.
        let read = || fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()));
        let n = match ext {
            "rs" if in_src => count_rust(&read()?),
            "cedar" | "cedarschema" => count_cedar(&read()?),
            _ => 0,
        };
        *per_crate.entry(krate).or_default() += n;
        let rel = rel.to_string_lossy().replace('\\', "/");
        for (part, _) in PART_BUDGETS {
            if rel.starts_with(&format!("{part}/")) {
                *per_part.entry(part).or_default() += n;
            }
        }
    }
    let total: usize = per_crate.values().sum();
    let mut over = Vec::new();
    for (part, budget) in PART_BUDGETS {
        let n = per_part.get(part).copied().unwrap_or(0);
        println!("  part core/{part}: {n} / {budget}");
        if n > *budget {
            over.push(format!(
                "core/{part} is {} lines over its budget of {budget}",
                n - budget
            ));
        }
    }
    if !over.is_empty() {
        return Err(over.join("\n"));
    }
    println!(
        "trusted core: {total} / {BUDGET} lines ({:.1}%)",
        total as f64 * 100.0 / BUDGET as f64
    );
    for (name, n) in &per_crate {
        println!("  {name:<12} {n}");
    }
    if total > BUDGET {
        Err(format!(
            "the trusted core is {} lines over its budget of {BUDGET} (PRD success metrics)",
            total - BUDGET
        ))
    } else {
        Ok(())
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Non-blank, non-comment lines up to the file's `#[cfg(test)]` module, which clippy requires to come last.
fn count_rust(source: &str) -> usize {
    let mut n = 0;
    for line in source.lines() {
        if line.starts_with("#[cfg(test)]") {
            break;
        }
        let t = line.trim();
        if !t.is_empty() && !t.starts_with("//") {
            n += 1;
        }
    }
    n
}

fn count_cedar(source: &str) -> usize {
    source
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.starts_with("//"))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, dir: &str, deps: &[(&str, bool)]) -> Package {
        Package {
            name: name.into(),
            dir: dir.into(),
            deps: deps
                .iter()
                .map(|(n, dev)| Dep {
                    name: (*n).into(),
                    dev: *dev,
                })
                .collect(),
            external: Vec::new(),
        }
    }

    fn with_external(mut v: Vec<Package>, name: &str, external: &[(&str, bool)]) -> Vec<Package> {
        let p = v.iter_mut().find(|p| p.name == name).unwrap();
        p.external = external
            .iter()
            .map(|(n, dev)| Dep {
                name: (*n).into(),
                dev: *dev,
            })
            .collect();
        v
    }

    fn cfg(name: &str, text: Option<&str>) -> (String, Option<String>) {
        (name.into(), text.map(Into::into))
    }

    /// The layout the ADR describes, with each crate's real dependencies.
    fn good() -> Vec<Package> {
        let mut v = Vec::new();
        for (name, deps) in CORE {
            let dir = if *name == "hyalos" {
                "core/hyalos".to_string()
            } else {
                format!("core/{}", name.trim_start_matches("hyalos-"))
            };
            let deps: Vec<(&str, bool)> = deps.iter().map(|d| (*d, false)).collect();
            v.push(pkg(name, &dir, &deps));
        }
        v.push(pkg("xtask", "crates/xtask", &[]));
        v
    }

    fn replace(v: &mut [Package], name: &str, deps: &[(&str, bool)]) {
        let dir = v
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.dir.clone())
            .unwrap();
        *v.iter_mut().find(|p| p.name == name).unwrap() = pkg(name, &dir, deps);
    }

    #[test]
    fn the_described_layout_passes() {
        assert_eq!(violations(&good()), Vec::<String>::new());
    }

    #[test]
    fn broker_may_not_depend_on_the_engine() {
        let mut v = good();
        replace(
            &mut v,
            "hyalos-broker",
            &[("hyalos-types", false), ("hyalos-engine", false)],
        );
        assert_eq!(
            violations(&v),
            ["hyalos-broker must not depend on hyalos-engine"]
        );
    }

    #[test]
    fn broker_may_not_link_the_log_or_substrate() {
        let mut v = good();
        replace(
            &mut v,
            "hyalos-broker",
            &[
                ("hyalos-types", false),
                ("hyalos-log", false),
                ("hyalos-substrate", false),
            ],
        );
        assert_eq!(violations(&v).len(), 2);
    }

    /// Only the `hyalos` crate may link the plugin runtime: the broker and engine crates may not, and
    /// the runtime links no other workspace crate but the types (ADR-0086).
    #[test]
    fn only_the_binary_may_depend_on_the_plugin_runtime() {
        let mut v = good();
        replace(
            &mut v,
            "hyalos-broker",
            &[("hyalos-types", false), ("hyalos-plugin", false)],
        );
        replace(
            &mut v,
            "hyalos-engine",
            &[("hyalos-types", false), ("hyalos-plugin", false)],
        );
        replace(
            &mut v,
            "hyalos-plugin",
            &[("hyalos-types", false), ("hyalos-engine", false)],
        );
        assert_eq!(
            violations(&v),
            [
                "hyalos-engine must not depend on hyalos-plugin",
                "hyalos-broker must not depend on hyalos-plugin",
                "hyalos-plugin must not depend on hyalos-engine",
            ]
        );
    }

    #[test]
    fn engine_may_not_depend_on_the_broker() {
        let mut v = good();
        replace(
            &mut v,
            "hyalos-engine",
            &[("hyalos-types", false), ("hyalos-broker", false)],
        );
        assert_eq!(
            violations(&v),
            ["hyalos-engine must not depend on hyalos-broker"]
        );
    }

    #[test]
    fn types_is_a_leaf() {
        let mut v = good();
        replace(&mut v, "hyalos-types", &[("hyalos-log", false)]);
        assert_eq!(
            violations(&v),
            ["hyalos-types must not depend on hyalos-log"]
        );
    }

    #[test]
    fn core_may_use_crates_only_as_a_dev_dependency() {
        let mut v = good();
        replace(
            &mut v,
            "hyalos-log",
            &[("hyalos-types", false), ("xtask", true)],
        );
        assert!(violations(&v).is_empty());
        replace(
            &mut v,
            "hyalos-log",
            &[("hyalos-types", false), ("xtask", false)],
        );
        assert_eq!(violations(&v).len(), 1);
    }

    #[test]
    fn a_new_core_crate_must_be_added_to_the_table() {
        let mut v = good();
        v.push(pkg("hyalos-new", "core/new", &[]));
        assert_eq!(violations(&v).len(), 1);
    }

    #[test]
    fn crates_outside_core_and_crates_are_refused() {
        let mut v = good();
        v.push(pkg("stray", "tools/stray", &[]));
        assert_eq!(violations(&v).len(), 1);
    }

    #[test]
    fn a_missing_core_crate_is_reported() {
        let v: Vec<Package> = good()
            .into_iter()
            .filter(|p| p.name != "hyalos-log")
            .collect();
        assert!(
            violations(&v)
                .iter()
                .any(|m| m.contains("hyalos-log is in the layout table"))
        );
    }

    #[test]
    fn a_deterministic_crate_may_not_add_an_external_dependency() {
        let v = with_external(good(), "hyalos-engine", &[("rand", false)]);
        let problems = violations(&v);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("hyalos-engine adds the external dependency rand"));
    }

    #[test]
    fn a_reviewed_dependency_is_allowed_only_in_the_crate_it_was_reviewed_for() {
        let v = with_external(
            good(),
            "hyalos-log",
            &[("rusqlite", false), ("sha2", false)],
        );
        assert!(violations(&v).is_empty());
        let v = with_external(good(), "hyalos-engine", &[("rusqlite", false)]);
        assert_eq!(violations(&v).len(), 1);
        let v = with_external(good(), "hyalos-log", &[("rand", false)]);
        assert_eq!(violations(&v).len(), 1);
    }

    #[test]
    fn a_dev_dependency_or_a_non_deterministic_crate_is_not_restricted() {
        let v = with_external(good(), "hyalos-types", &[("proptest", true)]);
        let v = with_external(v, "hyalos-broker", &[("rand", false)]);
        assert!(violations(&v).is_empty());
    }

    #[test]
    fn identical_determinism_configs_pass() {
        let files = [cfg("hyalos-types", Some("a")), cfg("hyalos-log", Some("a"))];
        assert!(config_violations(&files).is_empty());
    }

    #[test]
    fn a_differing_or_missing_determinism_config_is_reported() {
        let files = [
            cfg("hyalos-types", Some("a")),
            cfg("hyalos-log", Some("weaker")),
            cfg("hyalos-policy", None),
        ];
        let problems = config_violations(&files);
        assert_eq!(problems.len(), 2);
        assert!(
            problems[0].contains("hyalos-log/clippy.toml differs from hyalos-types/clippy.toml")
        );
        assert!(problems[1].contains("hyalos-policy has no clippy.toml"));
    }

    #[test]
    fn rust_lines_skip_blanks_comments_and_the_test_module() {
        let src = "//! doc\n\nfn a() {}\n    // note\nfn b() {}\n\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        assert_eq!(count_rust(src), 2);
    }

    #[test]
    fn cedar_lines_skip_blanks_and_comments() {
        assert_eq!(
            count_cedar("// why\npermit (principal, action, resource);\n\n"),
            1
        );
    }
}
