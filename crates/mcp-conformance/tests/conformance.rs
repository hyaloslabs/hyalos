//! The official MCP conformance suite against `hyalos::mcp` (S5.1, ADR-0067).
//!
//! It runs the pinned suite (`package.json` and its lockfile; install with
//! `npm ci --ignore-scripts --prefix crates/mcp-conformance`) on every scenario the 2026-07-28
//! revision requires, with `expected-failures.yml` naming the features the server does not offer.
//! The suite exits 0 only if nothing else fails and nothing listed passes. It skips without the
//! installed suite unless `HYALOS_REQUIRE_MCP_CONFORMANCE` is set, which the `mcp conformance` CI job
//! does.
//!
//! By hand: `cargo run -p hyalos-mcp-conformance` prints a URL, then
//! `crates/mcp-conformance/node_modules/.bin/conformance server --url <url> --requirements 2026-07-28 --expected-failures crates/mcp-conformance/expected-failures.yml`.

use std::path::Path;
use std::process::Command;

#[test]
fn the_conformance_suite_passes_every_required_scenario_but_the_features_not_offered() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let suite = dir.join("node_modules/.bin/conformance");
    if !suite.exists() {
        assert!(
            std::env::var_os("HYALOS_REQUIRE_MCP_CONFORMANCE").is_none(),
            "the conformance suite is not installed: npm ci --ignore-scripts --prefix crates/mcp-conformance"
        );
        eprintln!("skipped: the conformance suite is not installed");
        return;
    }
    let url = hyalos_mcp_conformance::serve_fixtures().expect("serve the fixtures");
    let results =
        std::env::temp_dir().join(format!("hyalos-mcp-conformance-{}", std::process::id()));
    let out = Command::new(&suite)
        .args([
            "server",
            "--url",
            &url,
            "--requirements",
            "2026-07-28",
            "--expected-failures",
        ])
        .arg(dir.join("expected-failures.yml"))
        .arg("--output-dir")
        .arg(&results)
        .output()
        .expect("run the conformance suite");
    let report = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&results);
    assert!(
        out.status.success(),
        "the conformance suite failed:\n{report}"
    );
    assert!(report.contains("Baseline check passed"), "{report}");
}
