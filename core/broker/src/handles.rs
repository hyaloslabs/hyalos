//! Gated handles (ADR-0005, ADR-0063): raw forge text the broker holds and the engine refers to by
//! digest only.
//!
//! The engine asks the broker to hold an issue (`HoldIssue`). The broker reads it with the job's
//! token, keeps the bytes for the rest of its run, and answers only their SHA-256, the handle. A human
//! releases a handle to a task (`handle.released`, in the engine), and the engine then mints a job's
//! manifest naming it. The broker serves `GET /handle/<digest>` on that job's socket only for a handle
//! the signed manifest names and the broker holds; the bytes it serves are the ones whose digest the
//! decision named, whatever the forge says now.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use hyalos_types::{Digest, ForgeIssue, Manifest};
use sha2::{Digest as _, Sha256};

use crate::egress::{self, ProxyRequest, Reports};
use crate::model::Response;

/// Where handles are served on a job's socket.
pub const PREFIX: &str = "/handle/";

/// The most handles one broker holds. A run holds one per issue it releases.
pub const MAX_HANDLES: usize = 16;

/// Why a handle was not served.
pub const NOT_RELEASED: &str = "handle.not_released";

/// The bytes the broker holds, by handle.
pub type Held = BTreeMap<Digest, Arc<[u8]>>;

/// The bytes the broker holds for `issue`, and their digest, which is its handle. The bytes are the
/// issue as JSON: its fields are fixed and capped (ADR-0047), so one issue has one encoding.
pub fn hold(held: &mut Held, issue: &ForgeIssue) -> Result<Digest, &'static str> {
    let bytes = serde_json::to_vec(issue).map_err(|_| "handle.unencodable")?;
    let handle = Digest::from_bytes(Sha256::digest(&bytes).into());
    if !held.contains_key(&handle) && held.len() >= MAX_HANDLES {
        return Err("handle.too_many");
    }
    held.insert(handle, bytes.into());
    Ok(handle)
}

/// The handles a manifest names, if the broker holds every one; `None` if it holds any of them not.
pub fn for_manifest(held: &Held, manifest: &Manifest) -> Option<Held> {
    manifest
        .handles
        .iter()
        .map(|h| held.get(h).map(|bytes| (*h, Arc::clone(bytes))))
        .collect()
}

/// Answers a request under [`PREFIX`] for the job `manifest` is for, from the handles it was given
/// at admission. A handle the manifest does not name is refused and reported as the manifest gate's
/// `deny`, with the SHA-256 of the handle asked for.
pub(crate) fn answer(
    head: &ProxyRequest,
    manifest: &Manifest,
    held: &Held,
    reports: &Reports,
    ended: &Arc<AtomicBool>,
) -> Response {
    let asked = head.target.strip_prefix(PREFIX).unwrap_or_default();
    let Some(handle) = Digest::parse(asked).filter(|_| head.method == "GET") else {
        return egress::refused(404, "handle.route_refused");
    };
    match held
        .get(&handle)
        .filter(|_| manifest.handles.contains(&handle))
    {
        Some(bytes) => Response::bytes(200, "application/json", bytes.to_vec()),
        None => {
            reports.deny(NOT_RELEASED, egress::inputs_digest(asked), ended);
            egress::refused(403, NOT_RELEASED)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(body: &str) -> ForgeIssue {
        serde_json::from_value(serde_json::json!({
            "number": 7, "state": "open", "title": "t", "body": body, "author": null, "labels": []
        }))
        .unwrap()
    }

    #[test]
    fn a_handle_is_the_digest_of_the_bytes_held_and_the_same_issue_is_one_handle() {
        let mut held = Held::new();
        let a = hold(&mut held, &issue("one")).unwrap();
        assert_eq!(hold(&mut held, &issue("one")).unwrap(), a);
        let b = hold(&mut held, &issue("two")).unwrap();
        assert_ne!(a, b);
        let bytes = &held[&a];
        assert_eq!(Digest::from_bytes(Sha256::digest(bytes).into()), a);
        assert_eq!(held.len(), 2);
    }

    #[test]
    fn a_broker_holds_a_bounded_number_of_handles() {
        let mut held = Held::new();
        for n in 0..MAX_HANDLES {
            hold(&mut held, &issue(&n.to_string())).unwrap();
        }
        assert_eq!(hold(&mut held, &issue("more")), Err("handle.too_many"));
        // One already held is not a new one.
        assert!(hold(&mut held, &issue("0")).is_ok());
    }

    #[test]
    fn a_manifest_is_given_only_handles_the_broker_holds() {
        let mut held = Held::new();
        let a = hold(&mut held, &issue("one")).unwrap();
        let manifest = |handles: &[Digest]| Manifest {
            version: hyalos_types::MANIFEST_VERSION,
            job: hyalos_types::JobId::from_seq(1),
            role: hyalos_types::Role::Actor,
            tainted: true,
            repo: "o/r".into(),
            branch_prefix: String::new(),
            pr_base: None,
            actions: Default::default(),
            egress_hosts: Default::default(),
            tools: Default::default(),
            data_flows: Default::default(),
            budget: hyalos_types::Budget { tokens: 1 },
            deadline: hyalos_types::Timestamp::from_millis(1),
            approval: None,
            handles: handles.iter().copied().collect(),
            result: None,
            closes: None,
        };
        assert_eq!(
            for_manifest(&held, &manifest(&[a])).map(|h| h.len()),
            Some(1)
        );
        assert_eq!(
            for_manifest(&held, &manifest(&[])).map(|h| h.len()),
            Some(0)
        );
        let missing = Digest::from_bytes([1; 32]);
        assert!(for_manifest(&held, &manifest(&[a, missing])).is_none());
    }
}
