//! The parts of the `hyalos` binary. The engine and broker run from this one executable (ADR-0002);
//! `main.rs` only picks the command. They are a library so tests can reach the launcher and the
//! channel without going through the command line.

pub mod agent_cmd;
pub mod broker_cmd;
pub mod channel;
pub mod clock;
pub mod doctor_cmd;
pub mod github;
pub mod github_cmd;
pub mod hardening;
pub mod init_cmd;
pub mod interrupt;
pub mod keys;
pub mod launch;
pub mod log_cmd;
pub mod mcp;
pub mod mcp_cmd;
pub mod merge_cmd;
pub mod microvm_conf;
pub mod operator;
pub mod operator_cmd;
pub mod passphrase;
pub mod paths;
pub mod policy_cmd;
pub mod run_cmd;
pub mod runs;
pub mod setup_ask;
pub mod setup_cmd;
pub mod stack;
pub mod tools_cmd;
pub mod workflow_cmd;
pub mod workflow_run;
