//! How much a held action matters, and so who may release it (ADR-0012, ADR-0025).
//!
//! The engine decides an action's stakes from the action; nothing the broker or a sandbox reports can
//! lower them. A hold records them when it opens, and the state refuses a release by anyone not allowed
//! at that level.

use std::fmt;

use serde::de::{self, Deserializer};
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};

use crate::event::Resolution;

/// The three tiers of ADR-0012.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// A job within its manifest, an install within the operator's install policy: decided by policy.
    Routine,
    /// A single job that needs something outside its allowance: the verifier agent decides.
    Escalated,
    /// Changing policy, publishing publicly, merging to a protected branch, spending above a cap,
    /// exporting credentials: always a human.
    Irreversible,
}

/// The things a verifier agent can never approve (ADR-0009, ADR-0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitive {
    Credentials,
    Policy,
    Publishing,
    CapabilityGrant,
}

impl Sensitive {
    pub const ALL: [Sensitive; 4] = [
        Sensitive::Credentials,
        Sensitive::Policy,
        Sensitive::Publishing,
        Sensitive::CapabilityGrant,
    ];

    const fn bit(self) -> u8 {
        match self {
            Sensitive::Credentials => 1,
            Sensitive::Policy => 2,
            Sensitive::Publishing => 4,
            Sensitive::CapabilityGrant => 8,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Sensitive::Credentials => "credentials",
            Sensitive::Policy => "policy",
            Sensitive::Publishing => "publishing",
            Sensitive::CapabilityGrant => "capability_grant",
        }
    }
}

/// A set of [`Sensitive`] categories. Stored as a JSON array in a fixed order, so there is one
/// encoding for each set; a repeated or unknown name is refused.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Touches(u8);

impl Touches {
    pub const NONE: Touches = Touches(0);

    pub fn of(categories: &[Sensitive]) -> Self {
        categories.iter().fold(Self::NONE, |t, c| t.with(*c))
    }

    #[must_use]
    pub const fn with(self, category: Sensitive) -> Self {
        Touches(self.0 | category.bit())
    }

    pub const fn contains(self, category: Sensitive) -> bool {
        self.0 & category.bit() != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Sensitive> {
        Sensitive::ALL
            .into_iter()
            .filter(move |c| self.contains(*c))
    }
}

impl fmt::Debug for Touches {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl Serialize for Touches {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(None)?;
        for c in self.iter() {
            seq.serialize_element(&c)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Touches {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let names = Vec::<Sensitive>::deserialize(d)?;
        let set = Touches::of(&names);
        // Exactly the canonical list: no repeats, and in the fixed order.
        if names != set.iter().collect::<Vec<_>>() {
            return Err(de::Error::custom(
                "touches must list each category at most once, in the order credentials, policy, publishing, capability_grant",
            ));
        }
        Ok(set)
    }
}

/// How much a held action matters: its tier, and which sensitive categories it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stakes {
    pub tier: Tier,
    pub touches: Touches,
}

impl Stakes {
    pub const fn routine() -> Self {
        Stakes {
            tier: Tier::Routine,
            touches: Touches::NONE,
        }
    }

    pub const fn escalated() -> Self {
        Stakes {
            tier: Tier::Escalated,
            touches: Touches::NONE,
        }
    }

    /// The most cautious stakes: only a human releases it. This is what an old hold, or one nobody
    /// classified, is given.
    pub const fn irreversible() -> Self {
        Stakes {
            tier: Tier::Irreversible,
            touches: Touches::NONE,
        }
    }

    #[must_use]
    pub const fn touching(self, category: Sensitive) -> Self {
        Stakes {
            tier: self.tier,
            touches: self.touches.with(category),
        }
    }

    /// Whether `by` may release a hold with these stakes (ADR-0012, ADR-0025).
    ///
    /// A human may release any hold. Touching credentials, policy, publishing or a capability grant
    /// needs a human whatever the tier, so a verifier agent can never approve one. Otherwise each tier
    /// has its own decider: policy for routine, the verifier for escalated, and a human for
    /// irreversible. A fresh allow from the gate itself releases a routine or escalated hold.
    pub const fn may_be_resolved_by(self, by: Resolution) -> bool {
        match by {
            Resolution::Human => true,
            _ if !self.touches.is_empty() => false,
            Resolution::Policy => matches!(self.tier, Tier::Routine),
            Resolution::Verifier => matches!(self.tier, Tier::Escalated),
            Resolution::Reevaluated => !matches!(self.tier, Tier::Irreversible),
        }
    }

    /// Whether `by` may cancel a task held at these stakes (ADR-0029). The same deciders as for
    /// releasing it, so a lower decider cannot end work that a higher one is to decide, except that a
    /// gate answering again is never a decision to end a task.
    pub const fn may_be_cancelled_by(self, by: Resolution) -> bool {
        !matches!(by, Resolution::Reevaluated) && self.may_be_resolved_by(by)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_BY: [Resolution; 4] = [
        Resolution::Reevaluated,
        Resolution::Policy,
        Resolution::Verifier,
        Resolution::Human,
    ];

    fn allowed(stakes: Stakes) -> Vec<Resolution> {
        ALL_BY
            .into_iter()
            .filter(|by| stakes.may_be_resolved_by(*by))
            .collect()
    }

    #[test]
    fn who_may_release_a_hold_at_each_tier() {
        // The whole matrix of ADR-0025, written out.
        assert_eq!(
            allowed(Stakes::routine()),
            vec![
                Resolution::Reevaluated,
                Resolution::Policy,
                Resolution::Human
            ]
        );
        assert_eq!(
            allowed(Stakes::escalated()),
            vec![
                Resolution::Reevaluated,
                Resolution::Verifier,
                Resolution::Human
            ]
        );
        assert_eq!(allowed(Stakes::irreversible()), vec![Resolution::Human]);
    }

    #[test]
    fn touching_anything_sensitive_needs_a_human_at_every_tier() {
        for tier in [Tier::Routine, Tier::Escalated, Tier::Irreversible] {
            for category in Sensitive::ALL {
                let stakes = Stakes {
                    tier,
                    touches: Touches::NONE.with(category),
                };
                assert_eq!(
                    allowed(stakes),
                    vec![Resolution::Human],
                    "{tier:?} touching {}",
                    category.as_str()
                );
            }
        }
    }

    fn may_cancel(stakes: Stakes) -> Vec<Resolution> {
        ALL_BY
            .into_iter()
            .filter(|by| stakes.may_be_cancelled_by(*by))
            .collect()
    }

    #[test]
    fn who_may_cancel_a_held_task_is_who_may_decide_it_and_never_a_gate_answering_again() {
        // Release and cancel differ in one column only: a fresh answer from the gate.
        assert_eq!(
            may_cancel(Stakes::routine()),
            vec![Resolution::Policy, Resolution::Human]
        );
        assert_eq!(
            may_cancel(Stakes::escalated()),
            vec![Resolution::Verifier, Resolution::Human]
        );
        assert_eq!(may_cancel(Stakes::irreversible()), vec![Resolution::Human]);
        for tier in [Tier::Routine, Tier::Escalated, Tier::Irreversible] {
            for category in Sensitive::ALL {
                let stakes = Stakes {
                    tier,
                    touches: Touches::NONE.with(category),
                };
                assert_eq!(
                    may_cancel(stakes),
                    vec![Resolution::Human],
                    "{tier:?} touching {}",
                    category.as_str()
                );
            }
        }
        for stakes in [
            Stakes::routine(),
            Stakes::escalated(),
            Stakes::irreversible(),
        ] {
            let release: Vec<_> = allowed(stakes)
                .into_iter()
                .filter(|by| *by != Resolution::Reevaluated)
                .collect();
            assert_eq!(may_cancel(stakes), release);
        }
    }

    #[test]
    fn touches_is_a_canonical_array() {
        let all = Touches::of(&Sensitive::ALL);
        assert_eq!(
            serde_json::to_string(&all).unwrap(),
            r#"["credentials","policy","publishing","capability_grant"]"#
        );
        assert_eq!(serde_json::to_string(&Touches::NONE).unwrap(), "[]");
        // Every subset round-trips.
        for bits in 0u8..16 {
            let set = Sensitive::ALL
                .into_iter()
                .filter(|c| bits & c.bit() != 0)
                .fold(Touches::NONE, Touches::with);
            let json = serde_json::to_string(&set).unwrap();
            assert_eq!(
                serde_json::from_str::<Touches>(&json).unwrap(),
                set,
                "{json}"
            );
        }
    }

    #[test]
    fn a_non_canonical_or_unknown_list_is_refused() {
        for bad in [
            r#"["credentials","credentials"]"#,
            r#"["policy","credentials"]"#,
            r#"["capability_grant","policy"]"#,
            r#"["unknown"]"#,
            r#""credentials""#,
            "null",
            "{}",
        ] {
            assert!(serde_json::from_str::<Touches>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn stakes_refuse_unknown_fields_and_missing_ones() {
        assert!(serde_json::from_str::<Stakes>(r#"{"tier":"routine","touches":[]}"#).is_ok());
        for bad in [
            r#"{"tier":"routine"}"#,
            r#"{"touches":[]}"#,
            r#"{"tier":"routine","touches":[],"extra":1}"#,
            r#"{"tier":"urgent","touches":[]}"#,
        ] {
            assert!(serde_json::from_str::<Stakes>(bad).is_err(), "{bad}");
        }
    }
}
