//! `hyalos github check <owner/repo>`: shows the GitHub App is set up for a repository (ADR-0044).
//!
//! It mints, one at a time, a token for each permission a run's jobs need (`permissions_for` of
//! `run_cmd::ACTOR_ACTIONS`), each revoked at once, so a permission the App lacks, or that its
//! installation has not accepted, is named before a job fails on it (#256). It prints neither a token
//! nor anything from the key.

use std::process::ExitCode;

use hyalos_types::Clock;

use crate::clock::SystemClock;
use crate::github::{GitHubApp, MintError, Permissions, permissions_for};
use crate::paths;

const USAGE: &str = "usage: hyalos github check <owner/repo>
  mints a token for the repository from the GitHub App in github.app and github-app.pem (config
  directory) for each permission a job needs, one at a time, shows what GitHub granted, and revokes
  each. See docs/operations/github-app.md.
  Exit 0 if the App can mint every one for the repository, 1 if not, 2 for bad usage";

/// Where the operator accepts an App's changed permissions, which GitHub asks for separately.
const INSTALLATIONS: &str = "https://github.com/settings/installations (for an organization: https://github.com/organizations/<org>/settings/installations)";

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
    let actions = crate::run_cmd::ACTOR_ACTIONS.map(String::from).into();
    let needed = match permissions_for(&actions) {
        Ok(needed) => needed,
        Err(e) => {
            eprintln!("hyalos: {e}");
            return ExitCode::from(1);
        }
    };
    let mut ok = true;
    let mut installation = None;
    for (&permission, &access) in &needed {
        let asked = Permissions::from([(permission, access)]);
        let named = format!("{permission}: {}", access.as_str());
        let minted = match app.mint_for(repo, &asked, SystemClock.now()) {
            Ok(minted) => minted,
            // The installation is missing: no permission will mint, so say so once.
            Err(e @ MintError::NotInstalled { .. }) => {
                eprintln!("hyalos: {e}");
                return ExitCode::from(1);
            }
            Err(MintError::Refused { status: 422, .. }) => {
                eprintln!(
                    "hyalos: GitHub refused a token with {named} (status 422): the App does not have that permission, or its installation has not accepted it. Set it on the App's settings page, then accept the change at {INSTALLATIONS}"
                );
                ok = false;
                continue;
            }
            Err(e) => {
                eprintln!("hyalos: {named}: {e}");
                ok = false;
                continue;
            }
        };
        if installation.is_none() {
            println!(
                "GitHub App installation {} can mint for {}",
                minted.installation, minted.repo
            );
            installation = Some(minted.installation);
        }
        println!(
            "  minted a token with {named}, expiring at {} ms since 1970",
            minted.token.expires_at.as_millis()
        );
        match app.revoke(&minted.token) {
            Ok(()) => println!("  revoked it"),
            Err(e) => {
                eprintln!(
                    "hyalos: minted a token with {named} but could not revoke it ({e}); it expires on its own within an hour"
                );
                ok = false;
            }
        }
    }
    if ok {
        println!("every permission a job needs is granted");
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
