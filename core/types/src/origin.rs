//! Where an event came from.
//!
//! The broker holds no signing key and never opens the log (ADR-0002, ADR-0003). It submits events to
//! the engine over the private channel, and the engine appends them with [`Origin::Broker`]. The log
//! therefore records what the broker said, not independent proof of it.

/// The process that produced an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    Engine,
    Broker,
}

impl Origin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Engine => "engine",
            Origin::Broker => "broker",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "engine" => Some(Origin::Engine),
            "broker" => Some(Origin::Broker),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_unknown_names_are_refused() {
        for o in [Origin::Engine, Origin::Broker] {
            assert_eq!(Origin::parse(o.as_str()), Some(o));
        }
        assert_eq!(Origin::parse("sandbox"), None);
        assert_eq!(Origin::parse("Engine"), None);
    }
}
