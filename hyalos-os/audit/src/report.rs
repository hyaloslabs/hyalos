//! What a passing audit states. The wording is a public contract, pinned by the golden reports.

use crate::{Attempt, Audited};

/// The fifth limit of S-04.5 (ADR-012): verify never sees the bytes a request hides.
pub const HIDDEN_BYTES_LIMIT: &str = "not proven: the request's hidden bytes are not shown, so the call may have used another credential or carried an added header";

/// What a passing attestation check reports. The limit is part of it, so it cannot be left out.
pub fn attested_report(answers: usize, test_settings: bool) -> String {
    // A check made against a stand-in says so, and so cannot be read as one about the real provider.
    let mark = if test_settings {
        "test trust settings: "
    } else {
        ""
    };
    format!(
        "{mark}attested: {answers} of {answers} model answers match what the notary saw\n{HIDDEN_BYTES_LIMIT}"
    )
}

/// What a passing audit proved, then what it did not.
pub fn audit_report(
    audited: &Audited,
    answers: usize,
    test_settings: bool,
    attempt: Option<&Attempt>,
    proofs: usize,
    oldest_pass: Option<u64>,
) -> String {
    let log = if audited.stand_in {
        "stand-in log"
    } else {
        "public log"
    };
    // Only a pass that checked an attempt budget may open with `audit passed` (ADR-011).
    let first = if audited.version != 1 && attempt.is_some() {
        "audit passed"
    } else {
        "legacy audit passed: no attempt budget"
    };
    let mut lines = vec![first.to_string()];
    if audited.stand_in {
        lines.push("test trust settings: the log here is a stand-in".to_string());
    }
    lines.push(
        "proven: every link's signature verifies under the operator's public key".to_string(),
    );
    lines.push(format!(
        "proven: every entry's inclusion proof and signed time verify under the {log}'s key"
    ));
    lines.push(format!("proven: the chain is unbroken and is the same list as the {log}'s entries under the operator's key"));
    lines.push(format!(
        "proven: this record is at link {} of {}, with one result for its run",
        audited.link, audited.links
    ));
    let times = if audited.runs == 1 { "time" } else { "times" };
    lines.push(format!(
        "the task was run {} {times} under this log key only, as the intent links name the task",
        audited.runs
    ));
    lines.push(format!(
        "the chain holds {} run(s) of any task under this log key only",
        audited.all_runs
    ));
    if test_settings {
        lines.push("test trust settings: the provider here is a stand-in".to_string());
    }
    lines.push(format!("proven: all {answers} answers were attested by the trusted notary for this run and position, inside the run's window, each with a sequence number equal to its position"));
    if let Some(held) = attempt {
        lines.push(format!("attempt {} of at most {} under this authorisation, {} authorised earlier, for this exact task on this notary", held.ticket + 1, held.budget, held.prior));
        if let Some(pass) = oldest_pass {
            lines.push(format!("provider canary: forged body refused at {} UTC, the oldest pass in this record, checked by the customer on this notary", utc(pass)));
        }
        // A count and nothing else: a proof's kind is the operator's text.
        lines.push(format!(
            "unchecked provider proofs: {proofs}, which prove nothing"
        ));
    }
    lines.push("not proven: the notary's independence is the customer's own".to_string());
    lines.push("not proven: the model's name is the provider's claim".to_string());
    lines.push("not proven: the answer's correctness is not judged".to_string());
    lines.push(
        "not proven: the log is trusted to have shown every entry under the operator's key"
            .to_string(),
    );
    lines.push(HIDDEN_BYTES_LIMIT.to_string());
    if attempt.is_some() {
        lines.push(
            "not proven: the operator may have asked the provider directly, away from the notary"
                .to_string(),
        );
        lines.push(
            "not proven: the customer did not authorise this task again after this attestation"
                .to_string(),
        );
    }
    if oldest_pass.is_some() {
        lines.push("not proven: the canary tested one forged shape, with the customer's credential, at the time stated".to_string());
    } else if audited.version == 2 {
        lines.push("not proven: this record has no provider canary".to_string());
    }
    lines.join("\n")
}

/// Unix seconds as `YYYY-MM-DD HH:MM:SS`, by the proleptic Gregorian calendar.
pub fn utc(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let of_era = shifted - era * 146_097;
    let year_of_era = (of_era - of_era / 1_460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}
