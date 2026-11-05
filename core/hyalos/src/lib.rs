//! The parts of the `hyalos` binary. The engine and broker run from this one executable (ADR-0002);
//! `main.rs` only picks the command. They are a library so tests can reach the launcher and the
//! channel without going through the command line.

pub mod broker_cmd;
pub mod channel;
pub mod clock;
pub mod doctor_cmd;
pub mod github;
pub mod github_cmd;
pub mod hardening;
pub mod init_cmd;
pub mod keys;
pub mod launch;
pub mod log_cmd;
pub mod paths;
pub mod policy_cmd;
pub mod run_cmd;
