//! The GitHub App, and the per-job tokens the engine mints from it (ADR-0004, ADR-0044).
//!
//! The engine holds the App's private key, the long-lived secret, and the broker never does. For each
//! job the engine signs a short JWT as the App, finds the App's installation on the manifest's
//! repository, and asks for an installation token limited to that one repository and to the
//! permissions the manifest's actions need. It checks that GitHub granted no more, hands the token to
//! the broker with the manifest, and revokes it when the job ends. No personal access token is read or
//! configured anywhere.
//!
//! Settings are in `github.app` in the config directory (`app_id`, optional `api`), and the key the
//! App's settings page generates is `github-app.pem` beside it, the operator's alone (ADR-0030).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::Path;

use hyalos_broker::model::{HttpTransport, Upstream};
use hyalos_policy::Action;
use hyalos_types::wire::ForgeBases;
use hyalos_types::{ForgeToken, Manifest, Timestamp};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use rustls_pki_types::PrivateKeyDer;
use rustls_pki_types::pem::PemObject;
use serde_json::{Value, json};

use crate::keys::read_private;

/// The App's settings: its id, and where its API is.
pub const GITHUB_APP_FILE: &str = "github.app";
/// The App's private key, as GitHub generates it.
pub const GITHUB_KEY_FILE: &str = "github-app.pem";

const DEFAULT_API: &str = "https://api.github.com";
/// github.com's git host, which goes with [`DEFAULT_API`].
const DEFAULT_GIT: &str = "https://github.com";
/// A PEM private key is a few kilobytes; anything larger is not one.
const KEY_LIMIT: u64 = 16 * 1024;
/// GitHub's answers here are small; a larger one is refused rather than read.
const RESPONSE_LIMIT: u64 = 256 * 1024;
const API_VERSION: &str = "2022-11-28";

/// How much a token may do with one kind of thing on the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    Read,
    Write,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "read" => Some(Access::Read),
            "write" => Some(Access::Write),
            _ => None,
        }
    }
}

/// GitHub permissions by name, as the token request and response write them.
pub type Permissions = BTreeMap<&'static str, Access>;

/// The step a [`MintError::Refused`] names when GitHub refuses the token itself, rather than the App.
pub const MINT_STEP: &str = "mint an installation token";

/// The forge permissions a manifest's actions need (ADR-0044). An action that touches no forge needs
/// none. An action the engine never mints a token for, and any name that is not an action, is refused,
/// so a manifest cannot reach a permission this table does not name.
pub fn permissions_for(actions: &BTreeSet<String>) -> Result<Permissions, MintError> {
    let mut out = Permissions::new();
    for name in actions {
        let action = Action::ALL
            .into_iter()
            .find(|a| a.id() == name)
            .ok_or_else(|| MintError::Action(name.clone()))?;
        let needs: &[(&'static str, Access)] = match action {
            Action::Read => &[
                ("contents", Access::Read),
                ("issues", Access::Read),
                ("pull_requests", Access::Read),
            ],
            Action::Push => &[("contents", Access::Write)],
            Action::OpenPr => &[("pull_requests", Access::Write)],
            Action::Comment => &[("issues", Access::Write)],
            Action::Merge => &[
                ("contents", Access::Write),
                ("pull_requests", Access::Write),
            ],
            Action::UseTool | Action::DataFlow | Action::RequestHost | Action::SpendOverCap => &[],
            // A release is a decision about a task, never a job's (ADR-0103).
            Action::ChangePolicy
            | Action::Publish
            | Action::ExportCredentials
            | Action::ReleaseIssue => {
                return Err(MintError::Action(name.clone()));
            }
        };
        for &(permission, access) in needs {
            let slot = out.entry(permission).or_insert(access);
            *slot = (*slot).max(access);
        }
    }
    Ok(out)
}

/// Why no token was minted. Nothing here holds a token, a key or what GitHub said beyond its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintError {
    /// `github.app` is missing, or has a line that is not a setting this build knows.
    Settings(String),
    /// The private key could not be read or is not an RSA key.
    Key(String),
    /// The manifest asks for an action no token is minted for, or one that does not exist.
    Action(String),
    /// The manifest's repository is not `owner/name`.
    Repo,
    /// The App is not installed on the repository, or cannot see it.
    NotInstalled { repo: String },
    /// GitHub answered a step with this status.
    Refused { step: &'static str, status: u16 },
    /// GitHub could not be reached, or its answer could not be read.
    Transport(&'static str),
    /// GitHub's answer was not what the API documents.
    Response(&'static str),
    /// GitHub granted more than was asked; the token was revoked.
    Wider,
}

impl fmt::Display for MintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MintError::Settings(e) => write!(f, "the GitHub App settings: {e}"),
            MintError::Key(e) => write!(f, "the GitHub App key: {e}"),
            MintError::Action(a) => write!(
                f,
                "no forge token is minted for the action {a:?} (ADR-0044)"
            ),
            MintError::Repo => f.write_str("the manifest's repository is not owner/name"),
            MintError::NotInstalled { repo } => write!(
                f,
                "the GitHub App is not installed on {repo}, or cannot see it; install it on that repository (docs/operations/github-app.md)"
            ),
            MintError::Refused { step, status } => {
                write!(f, "GitHub refused to {step} (status {status})")?;
                if *status == 401 && *step != "revoke the token" {
                    f.write_str(": check app_id and that github-app.pem is this App's current key, and the clock")?;
                }
                Ok(())
            }
            MintError::Transport(code) => write!(f, "cannot reach GitHub: {code}"),
            MintError::Response(what) => write!(f, "GitHub's answer was not usable: {what}"),
            MintError::Wider => f.write_str(
                "GitHub granted more than was asked, so the token was revoked and not used",
            ),
        }
    }
}

impl std::error::Error for MintError {}

/// What `github.app` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub app_id: u64,
    /// The API's base, such as `https://api.github.com`, or a GitHub Enterprise Server's
    /// `https://github.example.com/api/v3`.
    pub api: String,
    /// The git host jobs clone from and push to through the broker (ADR-0050), such as
    /// `https://github.com`. Needed when `api` is not api.github.com, since it cannot be guessed.
    pub git: String,
}

impl AppSettings {
    /// One `name = value` per line; `#` starts a comment. `app_id` is required; `api` and `git` are
    /// optional, and `git` is required with any `api` but api.github.com.
    /// Anything else is refused by line number, and a token is refused by name: Hyalos never takes
    /// a personal access token (ADR-0004).
    pub fn parse(text: &str) -> Result<Self, MintError> {
        let mut app_id = None;
        let mut api = None;
        let mut git = None;
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = || MintError::Settings(format!("line {} is not a setting", i + 1));
            let (name, value) = line
                .split_once('=')
                .map(|(n, v)| (n.trim(), v.trim()))
                .ok_or_else(bad)?;
            let slot = match name {
                "app_id" => &mut app_id,
                "api" => &mut api,
                "git" => &mut git,
                n if n.contains("token") || n == "pat" => {
                    return Err(MintError::Settings(format!(
                        "line {} names a token; Hyalos mints its own per job from the App and never takes a personal access token (ADR-0004)",
                        i + 1
                    )));
                }
                _ => return Err(bad()),
            };
            if value.is_empty() || slot.replace(value).is_some() {
                return Err(bad());
            }
        }
        let app_id = app_id
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|id| *id > 0)
            .ok_or_else(|| MintError::Settings("app_id must be the App's number".into()))?;
        let api = api.unwrap_or(DEFAULT_API).trim_end_matches('/').to_string();
        let git = match git {
            Some(git) => git.trim_end_matches('/').to_string(),
            None if api == DEFAULT_API => DEFAULT_GIT.to_string(),
            None => {
                return Err(MintError::Settings(
                    "git must name the git host when api is not api.github.com".into(),
                ));
            }
        };
        Ok(Self { app_id, api, git })
    }
}

/// A token minted for one job, and what it was minted against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Minted {
    pub token: ForgeToken,
    pub installation: u64,
    pub repo: String,
    pub permissions: Permissions,
}

/// The App, ready to mint.
pub struct GitHubApp {
    app_id: u64,
    bases: ForgeBases,
    key: RsaKeyPair,
    transport: HttpTransport,
    /// The API's path under its host, empty for api.github.com.
    prefix: String,
    rng: SystemRandom,
}

impl fmt::Debug for GitHubApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GitHubApp({}, {:?}, key ..)",
            self.app_id, self.transport
        )
    }
}

impl GitHubApp {
    /// Reads `github.app` and `github-app.pem` from `config_dir`. The key file must be the
    /// operator's alone, as `engine.key` is.
    pub fn load(config_dir: &Path) -> Result<Self, MintError> {
        let settings_path = config_dir.join(GITHUB_APP_FILE);
        let text = std::fs::read_to_string(&settings_path).map_err(|e| {
            MintError::Settings(format!("cannot read {}: {e}", settings_path.display()))
        })?;
        let settings = AppSettings::parse(&text)?;
        let pem = read_private(&config_dir.join(GITHUB_KEY_FILE), KEY_LIMIT)
            .map_err(|e| MintError::Key(e.to_string()))?;
        Self::new(&settings, &pem)
    }

    /// The App from its settings and the PEM text of its key (PKCS#1, as GitHub generates it, or
    /// PKCS#8).
    pub fn new(settings: &AppSettings, pem: &[u8]) -> Result<Self, MintError> {
        let not_rsa = || MintError::Key("not an RSA private key in PEM".into());
        let key = match PrivateKeyDer::from_pem_slice(pem).map_err(|_| not_rsa())? {
            PrivateKeyDer::Pkcs1(der) => RsaKeyPair::from_der(der.secret_pkcs1_der()),
            PrivateKeyDer::Pkcs8(der) => RsaKeyPair::from_pkcs8(der.secret_pkcs8_der()),
            _ => return Err(not_rsa()),
        }
        .map_err(|_| not_rsa())?;
        let (base, prefix) = split_api(&settings.api)?;
        let transport = HttpTransport::new(base)
            .map_err(|e| MintError::Settings(format!("api {:?}: {e}", settings.api)))?;
        Ok(Self {
            app_id: settings.app_id,
            bases: ForgeBases {
                api: settings.api.clone(),
                git: settings.git.clone(),
            },
            key,
            transport,
            prefix: prefix.to_string(),
            rng: SystemRandom::new(),
        })
    }

    /// Where the broker reaches this App's forge, for `Hello` (ADR-0050).
    pub fn forge_bases(&self) -> ForgeBases {
        self.bases.clone()
    }

    /// A JWT that says this is the App, valid from a minute ago (for clock drift) to nine minutes
    /// from `now`, within GitHub's ten.
    pub fn jwt(&self, now: Timestamp) -> Result<String, MintError> {
        let now = now.as_millis() / 1000;
        let header = base64url(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = json!({
            "iat": now.saturating_sub(60),
            "exp": now + 540,
            // The App's id as a number, as GitHub documents it for App-id JWTs.
            "iss": self.app_id,
        });
        let signed = format!("{header}.{}", base64url(claims.to_string().as_bytes()));
        let mut signature = vec![0; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &self.rng,
                signed.as_bytes(),
                &mut signature,
            )
            .map_err(|_| MintError::Key("signing failed".into()))?;
        Ok(format!("{signed}.{}", base64url(&signature)))
    }

    /// Mints the token for `manifest`'s job: its repository only, and the permissions its actions
    /// need. `None` when the actions need no forge access, so a job that does not touch the forge
    /// holds no token at all.
    pub fn mint(&self, manifest: &Manifest, now: Timestamp) -> Result<Option<Minted>, MintError> {
        let permissions = permissions_for(&manifest.actions)?;
        if permissions.is_empty() {
            return Ok(None);
        }
        self.mint_for(&manifest.repo, &permissions, now).map(Some)
    }

    /// Mints a token for `repo` with exactly `permissions`.
    pub fn mint_for(
        &self,
        repo: &str,
        permissions: &Permissions,
        now: Timestamp,
    ) -> Result<Minted, MintError> {
        let (owner, name) = split_repo(repo).ok_or(MintError::Repo)?;
        let jwt = self.jwt(now)?;
        let found = self.call(
            "GET",
            &format!("/repos/{owner}/{name}/installation"),
            &jwt,
            None,
        )?;
        let installation = match found {
            (200, body) => body
                .get("id")
                .and_then(Value::as_u64)
                .ok_or(MintError::Response("no installation id"))?,
            (404, _) => {
                return Err(MintError::NotInstalled {
                    repo: repo.to_string(),
                });
            }
            (status, _) => {
                return Err(MintError::Refused {
                    step: "find the App's installation",
                    status,
                });
            }
        };
        let asked: serde_json::Map<String, Value> = permissions
            .iter()
            .map(|(p, a)| (p.to_string(), Value::from(a.as_str())))
            .collect();
        let request = json!({ "repositories": [name], "permissions": asked });
        let (status, body) = self.call(
            "POST",
            &format!("/app/installations/{installation}/access_tokens"),
            &jwt,
            Some(&request),
        )?;
        if status != 201 {
            return Err(MintError::Refused {
                step: MINT_STEP,
                status,
            });
        }
        // From here on GitHub has issued a token, so every refusal revokes it: one this build cannot
        // use must not stay alive until it expires.
        let raw = body
            .get("token")
            .and_then(Value::as_str)
            .ok_or(MintError::Response("no token"))?;
        let Some(token) = body
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(parse_time)
            .and_then(|expires_at| ForgeToken::new(raw, expires_at))
        else {
            let _ = self.revoke_raw(raw);
            return Err(MintError::Response(
                "a token or expiry this build cannot use",
            ));
        };
        let expires_at = token.expires_at;
        if !granted_within(&body, repo, permissions) {
            // Never used: it could do more than the manifest allows.
            let _ = self.revoke(&token);
            return Err(MintError::Wider);
        }
        if expires_at <= now {
            let _ = self.revoke(&token);
            return Err(MintError::Response("the token had already expired"));
        }
        Ok(Minted {
            token,
            installation,
            repo: repo.to_string(),
            permissions: permissions.clone(),
        })
    }

    /// Revokes `token` at GitHub, so it stops working when its job ends and not only when it
    /// expires.
    pub fn revoke(&self, token: &ForgeToken) -> Result<(), MintError> {
        self.revoke_raw(token.expose())
    }

    fn revoke_raw(&self, token: &str) -> Result<(), MintError> {
        if token.bytes().any(|b| !b.is_ascii_graphic()) {
            return Err(MintError::Response("a token that cannot go in a header"));
        }
        match self.call("DELETE", "/installation/token", token, None)? {
            (204, _) => Ok(()),
            (status, _) => Err(MintError::Refused {
                step: "revoke the token",
                status,
            }),
        }
    }

    /// Where GitHub delivers this App's webhook, `None` when it has none (ADR-0084). Any other
    /// answer than the config or a 404 is a refusal.
    pub fn hook_url(&self, now: Timestamp) -> Result<Option<String>, MintError> {
        let jwt = self.jwt(now)?;
        let (status, body) = self.call("GET", "/app/hook/config", &jwt, None)?;
        match status {
            200 => Ok(body
                .get("url")
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
                .map(str::to_string)),
            404 => Ok(None),
            status => Err(MintError::Refused {
                step: "read the App's webhook",
                status,
            }),
        }
    }

    /// One API call, with `bearer` as its credential. Returns the status and the JSON body (`null`
    /// when there is none).
    fn call(
        &self,
        method: &str,
        path: &str,
        bearer: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), MintError> {
        let mut headers = vec![
            ("authorization".to_string(), format!("Bearer {bearer}")),
            (
                "accept".to_string(),
                "application/vnd.github+json".to_string(),
            ),
            ("user-agent".to_string(), "hyalos".to_string()),
            ("x-github-api-version".to_string(), API_VERSION.to_string()),
        ];
        let body = match body {
            Some(value) => {
                headers.push(("content-type".into(), "application/json".into()));
                value.to_string().into_bytes()
            }
            None => Vec::new(),
        };
        let response = self
            .transport
            .request(
                method,
                &Upstream {
                    target: format!("{}{path}", self.prefix),
                    headers,
                    body,
                },
            )
            .map_err(|e| MintError::Transport(e.0))?;
        let mut raw = Vec::new();
        response
            .body
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut raw)
            .map_err(|_| MintError::Transport("transport.bad_response"))?;
        if raw.len() as u64 > RESPONSE_LIMIT {
            return Err(MintError::Response("too large"));
        }
        let value = if raw.iter().all(u8::is_ascii_whitespace) {
            Value::Null
        } else {
            serde_json::from_slice(&raw).map_err(|_| MintError::Response("not JSON"))?
        };
        Ok((response.status, value))
    }
}

/// Whether the token GitHub describes in `body` is limited to `repo` and to `asked`. GitHub always
/// adds read access to the repository's metadata.
fn granted_within(body: &Value, repo: &str, asked: &Permissions) -> bool {
    let Some(granted) = body.get("permissions").and_then(Value::as_object) else {
        return false;
    };
    let permissions_ok = granted.iter().all(|(name, level)| {
        let level = level.as_str().and_then(Access::parse);
        match (name.as_str(), level) {
            ("metadata", Some(Access::Read)) => true,
            (name, Some(level)) => asked.get(name).is_some_and(|a| level <= *a),
            (_, None) => false,
        }
    });
    let selected = body.get("repository_selection").and_then(Value::as_str) == Some("selected");
    let repos_ok = match body.get("repositories").and_then(Value::as_array) {
        Some(list) => {
            !list.is_empty()
                && list.iter().all(|r| {
                    r.get("full_name")
                        .and_then(Value::as_str)
                        .is_some_and(|n| n.eq_ignore_ascii_case(repo))
                })
        }
        None => false,
    };
    permissions_ok && selected && repos_ok
}

/// `owner/name`, each 1 to 100 of the characters GitHub allows in them, so either is safe in a path.
fn split_repo(repo: &str) -> Option<(&str, &str)> {
    let (owner, name) = repo.split_once('/')?;
    let ok = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    (ok(owner) && ok(name)).then_some((owner, name))
}

/// Splits `https://host[:port][/path]` into the base the transport takes and the path the API is
/// under.
fn split_api(api: &str) -> Result<(&str, &str), MintError> {
    let bad = || MintError::Settings(format!("api {api:?} is not an http(s) URL"));
    let (scheme, rest) = api.split_once("://").ok_or_else(bad)?;
    let at = rest.find('/').unwrap_or(rest.len());
    let prefix = &rest[at..];
    if prefix.contains(['?', '#', ' ']) {
        return Err(bad());
    }
    Ok((&api[..scheme.len() + 3 + at], prefix))
}

/// RFC 4648 base64url without padding, as JWTs use.
fn base64url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// `YYYY-MM-DDTHH:MM:SSZ`, the form GitHub gives expiry times in, as a timestamp.
fn parse_time(text: &str) -> Option<Timestamp> {
    let b = text.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let num = |from: usize, to: usize| text.get(from..to)?.parse::<u64>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = u64::try_from(era * 146_097 + doe - 719_468).ok()?;
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second;
    Some(Timestamp::from_millis(seconds * 1000))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn each_action_needs_only_the_permissions_in_the_table() {
        assert_eq!(
            permissions_for(&actions(&["read"])).unwrap(),
            Permissions::from([
                ("contents", Access::Read),
                ("issues", Access::Read),
                ("pull_requests", Access::Read),
            ])
        );
        // The higher level wins where actions overlap.
        assert_eq!(
            permissions_for(&actions(&["read", "push", "open_pr", "comment"])).unwrap(),
            Permissions::from([
                ("contents", Access::Write),
                ("issues", Access::Write),
                ("pull_requests", Access::Write),
            ])
        );
        assert!(
            permissions_for(&actions(&["use_tool", "data_flow", "request_host"]))
                .unwrap()
                .is_empty()
        );
        for refused in [
            "publish",
            "export_credentials",
            "change_policy",
            "admin",
            "Read",
        ] {
            assert_eq!(
                permissions_for(&actions(&["read", refused])),
                Err(MintError::Action(refused.into())),
                "{refused}"
            );
        }
    }

    #[test]
    fn settings_need_an_app_id_and_never_take_a_token() {
        assert_eq!(
            AppSettings::parse("# the App\napp_id = 12345\n").unwrap(),
            AppSettings {
                app_id: 12345,
                api: DEFAULT_API.into(),
                git: DEFAULT_GIT.into(),
            }
        );
        assert_eq!(
            AppSettings::parse(
                "app_id=7\napi = https://ghe.example.com/api/v3\ngit = https://ghe.example.com/\n"
            )
            .unwrap()
            .git,
            "https://ghe.example.com"
        );
        assert_eq!(
            AppSettings::parse(
                "app_id=7\napi = https://ghe.example.com/api/v3/\ngit = https://ghe.example.com\n"
            )
            .unwrap()
            .api,
            "https://ghe.example.com/api/v3"
        );
        for bad in [
            "",
            "app_id = abc",
            "app_id = 0",
            "app_id = 1\napp_id = 2",
            "app_id = 1\ninstallation = 3",
            "app_id 1",
            // Another API's git host cannot be guessed.
            "app_id = 1\napi = https://ghe.example.com/api/v3",
        ] {
            assert!(
                matches!(AppSettings::parse(bad), Err(MintError::Settings(_))),
                "{bad:?}"
            );
        }
        for token in ["token = ghp_x", "github.token = ghp_x", "pat = ghp_x"] {
            let Err(MintError::Settings(message)) =
                AppSettings::parse(&format!("app_id = 1\n{token}"))
            else {
                panic!("{token} was accepted");
            };
            assert!(
                message.contains("never takes a personal access token"),
                "{message}"
            );
            assert!(!message.contains("ghp_x"), "{message}");
        }
    }

    #[test]
    fn repositories_and_api_bases_are_checked() {
        assert_eq!(split_repo("octo/repo.js"), Some(("octo", "repo.js")));
        for bad in [
            "octo",
            "octo/",
            "/repo",
            "a/b/c",
            "octo/..",
            "octo/re po",
            "o?x/r",
        ] {
            assert_eq!(split_repo(bad), None, "{bad}");
        }
        assert_eq!(
            split_api("https://api.github.com").unwrap(),
            ("https://api.github.com", "")
        );
        assert_eq!(
            split_api("https://ghe.example.com/api/v3").unwrap(),
            ("https://ghe.example.com", "/api/v3")
        );
        assert!(split_api("api.github.com").is_err());
    }

    #[test]
    fn base64url_and_times_are_right() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        // The same as the encoder written out by hand that it replaced, at every length up to a
        // few blocks and over every byte value.
        fn by_hand(bytes: &[u8]) -> String {
            const ALPHABET: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut out = String::new();
            for chunk in bytes.chunks(3) {
                let n = chunk
                    .iter()
                    .enumerate()
                    .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
                for i in 0..=chunk.len() {
                    out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
                }
            }
            out
        }
        let bytes: Vec<u8> = (0..=255u8).chain((0..=255u8).rev()).collect();
        for len in 0..bytes.len() {
            for start in [0, 1, 2, 85, 170, 255]
                .into_iter()
                .filter(|s| s + len <= bytes.len())
            {
                let slice = &bytes[start..start + len];
                assert_eq!(base64url(slice), by_hand(slice), "{slice:?}");
            }
        }
        assert_eq!(
            parse_time("1970-01-01T00:00:00Z"),
            Some(Timestamp::from_millis(0))
        );
        assert_eq!(
            parse_time("2016-07-11T22:14:10Z"),
            Some(Timestamp::from_millis(1_468_275_250_000))
        );
        assert_eq!(
            parse_time("2024-02-29T12:00:00Z"),
            Some(Timestamp::from_millis(1_709_208_000_000))
        );
        for bad in [
            "2016-07-11 22:14:10Z",
            "2016-13-11T22:14:10Z",
            "2016-07-11T22:14:10+01:00",
        ] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_grant_wider_than_asked_is_caught() {
        let asked = Permissions::from([("contents", Access::Write)]);
        let body = |permissions: Value, repos: Value, selection: &str| json!({ "permissions": permissions, "repositories": repos, "repository_selection": selection });
        let one = json!([{ "full_name": "Octo/Repo" }]);
        assert!(granted_within(
            &body(
                json!({"contents": "write", "metadata": "read"}),
                one.clone(),
                "selected"
            ),
            "octo/repo",
            &asked
        ));
        assert!(granted_within(
            &body(json!({"contents": "read"}), one.clone(), "selected"),
            "octo/repo",
            &asked
        ));
        for (permissions, repos, selection) in [
            (
                json!({"contents": "write", "issues": "read"}),
                one.clone(),
                "selected",
            ),
            (json!({"contents": "admin"}), one.clone(), "selected"),
            (json!({"metadata": "write"}), one.clone(), "selected"),
            (json!({"contents": "write"}), one.clone(), "all"),
            (
                json!({"contents": "write"}),
                json!([{ "full_name": "octo/other" }]),
                "selected",
            ),
            (json!({"contents": "write"}), json!([]), "selected"),
        ] {
            assert!(
                !granted_within(
                    &body(permissions.clone(), repos, selection),
                    "octo/repo",
                    &asked
                ),
                "{permissions}"
            );
        }
        assert!(!granted_within(&json!({}), "octo/repo", &asked));
    }
}
