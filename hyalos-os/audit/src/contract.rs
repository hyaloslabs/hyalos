//! The Python guest spike's draft contract (ADR-0136, `docs/plans/hyalos-kernel-python`): the
//! question schemas a task may ask and the effect kinds it may request. Under a spike profile it is
//! input record 0, read here from the run's own inputs, so a replay enforces exactly the record's
//! contract. Compiled only with the `spike-contract` feature. Capsules replaces it.

use crate::record::NOTIFY;

/// The most bytes a contract may have.
pub const MAX_CONTRACT: usize = 4096;

/// What a contract grants. Each list is distinct, ascending and names only what the host supports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contract {
    pub asks: Vec<i32>,
    pub may_touch: Vec<i32>,
}

/// The schemas the host answers and the effect kinds it performs: a contract grants a subset.
const SCHEMAS: [i32; 1] = [0];
const KINDS: [i32; 1] = [NOTIFY];

impl Contract {
    /// Reads a contract from its canonical bytes: `{"asks":[…],"may_touch":[…]}` with exactly these
    /// keys in this order, no whitespace, each list of distinct ascending decimal integers the host
    /// supports. Anything else, another byte form of the same grants included, is no contract.
    pub fn parse(bytes: &[u8]) -> Option<Contract> {
        if bytes.len() > MAX_CONTRACT {
            return None;
        }
        let rest = bytes.strip_prefix(b"{\"asks\":")?;
        let (asks, rest) = list(rest, &SCHEMAS)?;
        let rest = rest.strip_prefix(b",\"may_touch\":")?;
        let (may_touch, rest) = list(rest, &KINDS)?;
        (rest == b"}").then_some(Contract { asks, may_touch })
    }
}

/// One `[…]` list at the start of `bytes`, and what follows it.
fn list<'a>(bytes: &'a [u8], supported: &[i32]) -> Option<(Vec<i32>, &'a [u8])> {
    let mut rest = bytes.strip_prefix(b"[")?;
    let mut values: Vec<i32> = Vec::new();
    if let Some(after) = rest.strip_prefix(b"]") {
        return Some((values, after));
    }
    loop {
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        let (number, after) = rest.split_at(digits);
        // Strict decimal: no sign, no leading zero, and small enough for an `i32`.
        if number.is_empty() || (number.len() > 1 && number[0] == b'0') || number.len() > 9 {
            return None;
        }
        let value: i32 = std::str::from_utf8(number).ok()?.parse().ok()?;
        if !supported.contains(&value) || values.last().is_some_and(|last| *last >= value) {
            return None;
        }
        values.push(value);
        match after.split_first()? {
            (b',', next) => rest = next,
            (b']', next) => return Some((values, next)),
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_the_canonical_contracts_parse() {
        let grants = |asks: &[i32], may_touch: &[i32]| {
            Some(Contract {
                asks: asks.to_vec(),
                may_touch: may_touch.to_vec(),
            })
        };
        assert_eq!(
            Contract::parse(br#"{"asks":[0],"may_touch":[0]}"#),
            grants(&[0], &[0])
        );
        assert_eq!(
            Contract::parse(br#"{"asks":[0],"may_touch":[]}"#),
            grants(&[0], &[])
        );
        assert_eq!(
            Contract::parse(br#"{"asks":[],"may_touch":[0]}"#),
            grants(&[], &[0])
        );
        assert_eq!(
            Contract::parse(br#"{"asks":[],"may_touch":[]}"#),
            grants(&[], &[])
        );
    }

    #[test]
    fn python_anything_else_is_no_contract() {
        let long = [br#"{"asks":[0],"may_touch":[0]}"#.as_slice(), &[b' '; 4096]].concat();
        for bad in [
            &b""[..],
            b"457\tfix a crash in read",
            br#"{"may_touch":[0],"asks":[0]}"#,
            br#"{"asks": [0],"may_touch":[0]}"#,
            br#"{"asks":[0],"may_touch":[0]} "#,
            br#"{"asks":[0],"may_touch":[0]}\n"#,
            br#"{"asks":[0],"may_touch":[0],"tools":[]}"#,
            br#"{"asks":[0,0],"may_touch":[0]}"#,
            br#"{"asks":[1],"may_touch":[0]}"#,
            br#"{"asks":[-0],"may_touch":[0]}"#,
            br#"{"asks":[00],"may_touch":[0]}"#,
            br#"{"asks":[0,],"may_touch":[0]}"#,
            br#"{"asks":[0],"may_touch":[7]}"#,
            br#"{"asks":[99999999999],"may_touch":[0]}"#,
            br#"{"asks":[0]}"#,
            b"{\"asks\":[0],\"may_touch\":[0]\xff}",
            &long,
        ] {
            assert_eq!(
                Contract::parse(bad),
                None,
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}
