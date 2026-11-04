//! `hyalos github check <owner/repo>`: shows the GitHub App is set up for a repository (ADR-0044).
//!
//! It mints the smallest token the App can give for the repository (read access to its contents),
//! says what GitHub granted and until when, and revokes it at once. It prints neither the token nor
//! anything from the key.

use std::process::ExitCode;

use hyalos_types::Clock;

use crate::clock::SystemClock;
use crate::github::{Access, GitHubApp, Permissions};
use crate::paths;

const USAGE: &str = "usage: hyalos github check <owner/repo>
  mints a read-only token for the repository from the GitHub App in github.app and github-app.pem
  (config directory), shows what GitHub granted, and revokes it. See docs/operations/github-app.md.
  Exit 0 if the App can mint for the repository, 1 if not, 2 for bad usage";

pub fn run(args: &[String]) -> ExitCode {
    let [check, repo] = args else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if check != "check" {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let env = |name: &str| std::env::var(name).ok();
    let config = match paths::config_dir(&env) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(2);
        }
    };
    let app = match GitHubApp::load(&config) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(1);
        }
    };
    let asked = Permissions::from([("contents", Access::Read)]);
    let minted = match app.mint_for(repo, &asked, SystemClock.now()) {
        Ok(minted) => minted,
        Err(e) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(1);
        }
    };
    println!(
        "GitHub App installation {} can mint for {}",
        minted.installation, minted.repo
    );
    println!(
        "  minted a token with contents: read, expiring at {} ms since 1970",
        minted.token.expires_at.as_millis()
    );
    match app.revoke(&minted.token) {
        Ok(()) => {
            println!("  revoked it");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!(
                "hyalos: minted a token but could not revoke it ({e}); it expires on its own within an hour"
            );
            ExitCode::from(1)
        }
    }
}
