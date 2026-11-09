//! Creating the GitHub App from a manifest (ADR-0084). The setup page posts a fixed manifest to
//! GitHub from the operator's browser, on any computer. GitHub creates the App and sends the browser
//! back to the page with a one-time code. The operator types that code into their own terminal, and
//! this module exchanges it for the App's id and private key. So the key goes from GitHub straight
//! to this process, never through the page, the browser or the driving agent.
//!
//! The page is not trusted. What GitHub hands back is checked before anything is written: the App
//! must belong to the account the operator named, and have exactly the permissions Hyalos mints
//! from. Anything else is refused, and the operator is told to delete that App.
//!
//! Nothing here prints the code, the key or GitHub's other secrets. Their buffers are zeroed.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

use hyalos_broker::model::{HttpTransport, Upstream};
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::github::{Access, permissions_for};

/// The setup page, which posts the manifest and shows the code GitHub sends back.
pub const SETUP_PAGE: &str = "https://hyaloslabs.github.io/hyalos/setup/";

/// The App's homepage, and the address of its webhook, which is never active. A webhook anywhere
/// else is refused, so no one else learns where the App is installed.
pub const HOMEPAGE: &str = "https://hyaloslabs.github.io/hyalos/";

/// The API the code is exchanged with. The manifest flow is GitHub.com's; an App on GitHub
/// Enterprise Server is set up by hand.
pub const GITHUB_API: &str = "https://api.github.com";

/// The answer is a few kilobytes: the App, its key and its secrets.
const RESPONSE_LIMIT: u64 = 64 * 1024;

/// The actions Hyalos mints tokens for. The App needs what all of them need, and nothing more.
const MINTED: [&str; 5] = ["read", "push", "open_pr", "comment", "merge"];

/// The permissions the App must have, as GitHub names them: what every minted action needs, and
/// the read access to metadata GitHub gives every App.
pub fn permissions() -> BTreeMap<String, String> {
    let actions = MINTED.iter().map(|a| (*a).to_string()).collect();
    let mut out: BTreeMap<String, String> = permissions_for(&actions)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, access)| (name.to_string(), access.as_str().to_string()))
        .collect();
    out.insert("metadata".into(), Access::Read.as_str().into());
    out
}

/// The App GitHub created, checked.
pub struct Created {
    pub id: u64,
    pub slug: String,
    pub owner: String,
    /// The App's private key, PEM.
    pub pem: Zeroizing<Vec<u8>>,
}

impl Created {
    /// What to tell the operator when something fails after GitHub made the App: which App, and to
    /// delete it, since its key is not kept.
    pub fn made(&self, why: &str) -> String {
        format!(
            "GitHub made the App {} (App ID {}) for {}, but {why}, so nothing was kept. Delete it ({}), then run this again",
            self.slug,
            self.id,
            self.owner,
            settings_page(&self.owner, &self.slug)
        )
    }
}

impl fmt::Debug for Created {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Created")
            .field("id", &self.id)
            .field("slug", &self.slug)
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

/// GitHub's answer, read only for what is checked and kept. Its other secrets are never read.
#[derive(Deserialize)]
struct Conversion {
    id: u64,
    slug: String,
    owner: Owner,
    permissions: BTreeMap<String, String>,
    #[serde(default)]
    events: Vec<String>,
    pem: String,
}

#[derive(Deserialize)]
struct Owner {
    login: String,
}

/// Whether `code` has the shape of GitHub's one-time code, so nothing else is put in a URL.
pub fn plausible(code: &str) -> bool {
    (8..=128).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Exchanges `code` with GitHub at `api` for the App it was made for, and checks that `owner` owns
/// it and that it has exactly [`permissions`]. The errors say what to do, and never hold the code.
pub fn convert(api: &str, code: &str, owner: &str) -> Result<Created, String> {
    if !plausible(code) {
        return Err("that is not the code the setup page shows: letters and digits only".into());
    }
    let transport = HttpTransport::new(api).map_err(|e| format!("api {api:?}: {e}"))?;
    let response = transport
        .request(
            "POST",
            &Upstream {
                target: format!("/app-manifests/{code}/conversions"),
                headers: vec![
                    ("accept".into(), "application/vnd.github+json".into()),
                    ("user-agent".into(), "hyalos".into()),
                ],
                body: Vec::new(),
            },
        )
        .map_err(|e| format!("GitHub could not be reached ({})", e.0))?;
    let status = response.status;
    // Sized once, so the buffer holding the key never moves and leaves a copy behind.
    let mut raw = Zeroizing::new(Vec::with_capacity(RESPONSE_LIMIT as usize + 1));
    // Once GitHub has taken the code, the App may exist, and its key is lost with this answer.
    let maybe = |why: &str| {
        format!(
            "{why}. GitHub may have made the App: if so, delete it (github.com/settings/apps, or the organization's), then run this again"
        )
    };
    let read = response.body.take(RESPONSE_LIMIT + 1).read_to_end(&mut raw);
    match status {
        201 => {}
        404 | 422 => {
            return Err(
                "GitHub did not take the code. It works once, within an hour of creating the App. \
                 If the App was made, delete it (github.com/settings/apps), then run this again"
                    .into(),
            );
        }
        other => return Err(format!("GitHub answered {other} to the code")),
    }
    if read.is_err() {
        return Err(maybe("GitHub's answer could not be read"));
    }
    if raw.len() as u64 > RESPONSE_LIMIT {
        return Err(maybe("GitHub's answer was too large"));
    }
    let answer: Conversion = serde_json::from_slice(&raw)
        .map_err(|_| maybe("GitHub's answer was not the App it documents"))?;
    let pem = Zeroizing::new(answer.pem.into_bytes());
    let named =
        |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !named(&answer.slug) || !named(&answer.owner.login) {
        return Err(maybe("GitHub's answer named the App or its owner oddly"));
    }
    let delete = |why: &str| {
        format!(
            "{why}, so nothing was written. Delete that App ({}), then run this again",
            settings_page(&answer.owner.login, &answer.slug)
        )
    };
    if !answer.owner.login.eq_ignore_ascii_case(owner) {
        return Err(delete(&format!(
            "GitHub made the App for {}, not {owner}",
            answer.owner.login
        )));
    }
    if answer.permissions != permissions() || !answer.events.is_empty() {
        return Err(delete(
            "the App GitHub made has other permissions or events than Hyalos asks for",
        ));
    }
    Ok(Created {
        id: answer.id,
        slug: answer.slug,
        owner: answer.owner.login,
        pem,
    })
}

/// Where `owner` deletes `slug`: under their own settings, or their organization's.
fn settings_page(owner: &str, slug: &str) -> String {
    format!(
        "github.com/settings/apps/{slug}/advanced, or github.com/organizations/{owner}/settings/apps/{slug}/advanced for an organization"
    )
}

/// Where the operator installs `slug` on the repository the jobs work on.
pub fn install_page(slug: &str) -> String {
    format!("https://github.com/apps/{slug}/installations/new")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_needs_what_every_minted_action_needs() {
        let p = permissions();
        let want: BTreeMap<String, String> = [
            ("contents", "write"),
            ("issues", "write"),
            ("metadata", "read"),
            ("pull_requests", "write"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(p, want);
    }

    #[test]
    fn only_a_code_of_letters_and_digits_is_sent() {
        assert!(plausible("a180b1a3d263c81bc6441d7b990bae27d4c10679"));
        for bad in [
            "",
            "short",
            "a/../b12345",
            "abc def 123",
            "x".repeat(129).as_str(),
        ] {
            assert!(!plausible(bad), "{bad}");
        }
    }

    #[test]
    fn what_is_created_prints_no_key() {
        let created = Created {
            id: 7,
            slug: "hyalos-op".into(),
            owner: "op".into(),
            pem: Zeroizing::new(b"-----BEGIN RSA PRIVATE KEY-----secret".to_vec()),
        };
        let shown = format!("{created:?}");
        assert!(
            shown.contains("hyalos-op") && !shown.contains("secret"),
            "{shown}"
        );
    }
}
