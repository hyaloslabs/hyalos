//! Identifiers for tasks, jobs and holds.
//!
//! An id is the sequence number of the event that created the thing: a task's id is the sequence of
//! its `task.created` event. Ids therefore need no randomness, cannot collide, and are the same on
//! every replay of the log (ADR-0013).

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub const fn from_seq(seq: u64) -> Self {
                Self(seq)
            }

            pub const fn seq(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id!(
    /// A unit of work an operator or agent asked for.
    TaskId
);
id!(
    /// One run of one step, under one signed manifest (ADR-0004).
    JobId
);
id!(
    /// An open question that stops a task until it is answered (ADR-0009).
    HoldId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_plain_numbers_in_json_and_keep_their_sequence() {
        let t = TaskId::from_seq(7);
        assert_eq!(serde_json::to_string(&t).unwrap(), "7");
        assert_eq!(serde_json::from_str::<TaskId>("7").unwrap(), t);
        assert_eq!(t.seq(), 7);
        assert_eq!(t.to_string(), "7");
        assert!(serde_json::from_str::<TaskId>("-1").is_err());
    }
}
