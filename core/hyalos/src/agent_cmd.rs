//! `hyalos agent issue|revoke|list`: the driving agent's credential (ADR-0010, ADR-0068).
//!
//! The operator issues a credential at this terminal and gives it to the agent once; the MCP server
//! admits only a caller presenting one in force. The log records the credential's SHA-256 and the
//! name, never the secret, so a backup or an audit of the log reveals none. Issuing writes an event
//! signed with the engine's key, which the MCP server never opens. The credential is a bearer secret,
//! not a key: it signs nothing, so it cannot stand for an approval.

use std::process::ExitCode;

use hyalos_engine::Decider;
use hyalos_types::{AgentId, AgentName, Digest, ReasonCode};
use zeroize::Zeroizing;

use crate::clock::SystemClock;
use crate::mcp_cmd::{CREDENTIAL_VAR, EngineLog};
use crate::operator;
use crate::passphrase;
use crate::paths::{self, Env};

/// What every credential starts with, so one found somewhere is recognised.
pub const CREDENTIAL_PREFIX: &str = "hyalos-agent-";

const USAGE: &str = "usage: hyalos agent issue <name> [--reason <code>] [--passphrase-stdin]
       hyalos agent revoke <id> [--reason <code>] [--passphrase-stdin]
       hyalos agent list
  issue records a new credential for a driving agent, issued by you, and prints it once: the log keeps
  only its SHA-256. <name> is 1 to 64 lowercase letters, digits, '.', '_' and '-', such as claude-code
  revoke stops the MCP server admitting agent <id> from its next call
  list shows every credential issued, by id and name, and whether it is revoked
  --reason is a short code (default agent.issued or agent.revoked)
  issue and revoke are your decisions, signed with your approval key (hyalos operator init): the
  passphrase is asked for at the terminal, or read from stdin with --passphrase-stdin";

pub fn run(args: &[String]) -> ExitCode {
    let env = |name: &str| std::env::var(name).ok();
    match run_with(args, &env) {
        Ok(Output { out, note }) => {
            print!("{}", out.as_str());
            eprint!("{note}");
            ExitCode::SUCCESS
        }
        Err((code, message)) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(code)
        }
    }
}

/// What the command prints: `out` on stdout (the credential alone, for `issue`), `note` on stderr.
/// `out` is wiped when dropped, since it may hold the credential.
#[derive(Debug, Default)]
pub struct Output {
    pub out: Zeroizing<String>,
    pub note: String,
}

/// The command's output, or its exit code and message.
pub fn run_with(args: &[String], env: Env<'_>) -> Result<Output, (u8, String)> {
    let (args, source) = passphrase::source(args);
    let usage = |why: &str| (2, format!("{why}\n{USAGE}"));
    let failed = |e: String| (1, e);
    let reason = |rest: &[String], default: &str| -> Result<ReasonCode, (u8, String)> {
        let text = match rest {
            [] => default,
            [flag, code] if flag == "--reason" => code,
            _ => return Err(usage("this takes only --reason <code>")),
        };
        ReasonCode::new(text).ok_or_else(|| usage("a reason is a short code, such as agent.issued"))
    };
    match args.as_slice() {
        [issue, name, rest @ ..] if issue == "issue" => {
            let name = AgentName::new(name).ok_or_else(|| {
                usage("a name is 1 to 64 lowercase letters, digits, '.', '_' and '-'")
            })?;
            let reason = reason(rest, "agent.issued")?;
            let credential = fresh_credential().map_err(failed)?;
            let mut engine = operator::engine(env, source).map_err(failed)?;
            let at = hyalos_types::Clock::now(&SystemClock);
            let head = engine
                .issue_agent(
                    at,
                    name.clone(),
                    credential_digest(&credential),
                    Decider::Human,
                    reason,
                )
                .map_err(|e| failed(e.to_string()))?;
            Ok(Output {
                out: {
                    let mut out = credential;
                    out.push('\n');
                    out
                },
                note: format!(
                    "issued agent {} ({name}). The credential above is shown only now; the log keeps its SHA-256.\nGive it to the agent: {CREDENTIAL_VAR} for hyalos mcp on stdio, or Authorization: Bearer on HTTP.\n",
                    head.seq
                ),
            })
        }
        [revoke, id, rest @ ..] if revoke == "revoke" => {
            let agent = id
                .parse::<u64>()
                .map(AgentId::from_seq)
                .map_err(|_| usage("an agent's id is the number hyalos agent list shows"))?;
            let reason = reason(rest, "agent.revoked")?;
            let mut engine = operator::engine(env, source).map_err(failed)?;
            let at = hyalos_types::Clock::now(&SystemClock);
            let head = engine
                .revoke_agent(at, agent, Decider::Human, reason)
                .map_err(|e| failed(e.to_string()))?;
            Ok(Output {
                out: Zeroizing::new(format!(
                    "revoked agent {agent} at event {}: the MCP server refuses it from its next call\n",
                    head.seq
                )),
                note: String::new(),
            })
        }
        [list] if list == "list" => {
            let log = EngineLog {
                log: paths::log_file(env).map_err(failed)?,
                public_key: paths::engine_public_key(env).map_err(failed)?,
            };
            let state = log.read().map_err(failed)?;
            if state.agents().is_empty() {
                return Ok(Output {
                    out: Zeroizing::new("no credential has been issued\n".into()),
                    note: String::new(),
                });
            }
            let mut out = String::new();
            for (id, agent) in state.agents() {
                let status = match &agent.revoked {
                    None => "in force".to_string(),
                    Some(r) => format!("revoked at {} ms ({})", r.at.as_millis(), r.reason),
                };
                out.push_str(&format!(
                    "{id}\t{}\tissued at {} ms\t{status}\n",
                    agent.name,
                    agent.at.as_millis()
                ));
            }
            Ok(Output {
                out: Zeroizing::new(out),
                note: String::new(),
            })
        }
        _ => Err(usage("unknown agent command")),
    }
}

/// A new credential: the prefix and 32 bytes from the operating system, in hex.
pub fn fresh_credential() -> Result<Zeroizing<String>, String> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    getrandom::fill(&mut *bytes).map_err(|e| format!("the system gave no randomness: {e}"))?;
    let mut credential = Zeroizing::new(String::with_capacity(CREDENTIAL_PREFIX.len() + 64));
    credential.push_str(CREDENTIAL_PREFIX);
    for b in bytes.iter() {
        credential.push_str(&format!("{b:02x}"));
    }
    Ok(credential)
}

/// The SHA-256 the log records for `credential`, and what a presented one is compared by.
pub fn credential_digest(credential: &str) -> Digest {
    let hash = ring::digest::digest(&ring::digest::SHA256, credential.as_bytes());
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(hash.as_ref());
    Digest::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_is_the_prefix_and_64_hex_digits_and_never_the_same_twice() {
        let a = fresh_credential().unwrap();
        let b = fresh_credential().unwrap();
        assert_ne!(*a, *b);
        let hex = a.strip_prefix(CREDENTIAL_PREFIX).unwrap();
        assert_eq!(hex.len(), 64);
        assert!(
            hex.bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn the_digest_is_sha_256_of_the_text() {
        // SHA-256("abc"), FIPS 180-2.
        assert_eq!(
            credential_digest("abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
