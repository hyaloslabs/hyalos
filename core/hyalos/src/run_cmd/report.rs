//! What the broker reports while a job runs, the events that end it, and how an attestation reads.

use super::*;

/// Writes the job's last events.
pub(super) struct Ended<'a> {
    pub(super) engine: &'a mut Engine,
    pub(super) clock: &'a mut Stamps,
    pub(super) job: JobId,
}

impl Ended<'_> {
    pub(super) fn with(
        &mut self,
        out: &mut dyn Write,
        outcome: JobOutcome,
        failure: Failure,
        revoked: Result<(), String>,
        values: Option<Vec<TypedValue>>,
    ) -> Result<Summary, String> {
        let job = self.job;
        let Failure { reason, why } = failure;
        let mut events: Vec<(Origin, Event)> = values
            .map(|values| (Origin::Engine, Event::JobValues { job, values }))
            .into_iter()
            .collect();
        events.push((
            Origin::Engine,
            Event::JobEnded {
                job,
                outcome,
                reason: reason.clone(),
            },
        ));
        let recorded = self.engine.submit_all(self.clock.now(), &events);
        match &recorded {
            Ok(head) => writeln!(
                out,
                "job {}: {outcome:?} ({reason}): {why} (event {})",
                job.seq(),
                head.seq
            ),
            Err(e) => writeln!(
                out,
                "job {}: {outcome:?} ({reason}): {why}; recording that failed: {e}",
                job.seq()
            ),
        }
        .map_err(|e| e.to_string())?;
        recorded.map_err(|e| e.to_string())?;
        Ok(Summary {
            job,
            outcome,
            revoked,
        })
    }
}

/// What the broker has said about the job.
#[derive(Default)]
pub(super) struct Reported {
    pub(super) refused: Option<ReasonCode>,
    /// The job's submitted result, answering `TakeResult` (ADR-0064).
    pub(super) values: Option<Vec<TypedValue>>,
    pub(super) held: bool,
    pub(super) broker_gone: bool,
}

impl Reported {
    /// Handles the broker's next message, if one comes within `wait`. False if none did.
    pub(super) fn take(
        &mut self,
        broker: &mut BrokerProcess,
        wait: Duration,
        ended: &mut Ended<'_>,
        out: &mut dyn Write,
    ) -> bool {
        let job = ended.job;
        let message = match broker.poll(wait) {
            Ok(Incoming::Message(message)) => message,
            Ok(Incoming::Nothing) => return false,
            Ok(Incoming::Closed) | Err(_) => {
                let _ = writeln!(out, "job {}: the broker closed its channel", job.seq());
                self.broker_gone = true;
                return false;
            }
        };
        match message {
            ToEngine::Gate {
                job: reported,
                gate,
                inputs,
                answer,
                reason,
            } if reported == job => {
                let at = ended.clock.now();
                let recorded = if answer == Answer::Unknown {
                    // A gate that could not answer, such as the content gate on a push it could
                    // not read (ADR-0054): the job holds as ADR-0020 says, and only a human can
                    // release it.
                    let request = GateRequest {
                        gate,
                        inputs,
                        stakes: Stakes::irreversible(),
                    };
                    let outcome = GateOutcome::Hold {
                        reason: reason.clone(),
                        why: HoldReason::Unknown,
                    };
                    ended
                        .engine
                        .rule_gate(at, Origin::Broker, job, &request, outcome)
                        .map(|ruling| {
                            self.held = matches!(ruling, Ruling::Held(_));
                            ended
                                .engine
                                .head()
                                .ok()
                                .flatten()
                                .map(|h| h.seq)
                                .unwrap_or_default()
                        })
                } else {
                    ended
                        .engine
                        .submit(
                            at,
                            Origin::Broker,
                            Event::GateAnswered {
                                job,
                                gate,
                                inputs,
                                answer,
                                reason: reason.clone(),
                            },
                        )
                        .map(|head| head.seq)
                };
                let _ = match recorded {
                    Ok(seq) => writeln!(
                        out,
                        "job {}: gate {gate:?} answered {answer:?} ({reason}), recorded as event {seq}",
                        job.seq()
                    ),
                    Err(e) => writeln!(
                        out,
                        "job {}: gate {gate:?} answered {answer:?} ({reason}), not recorded: {e}",
                        job.seq()
                    ),
                };
            }
            ToEngine::Refused {
                job: reported,
                reason,
            } if reported == job => {
                let _ = writeln!(
                    out,
                    "job {}: the broker ended the job ({reason})",
                    job.seq()
                );
                self.refused = Some(reason);
            }
            ToEngine::Values {
                job: reported,
                values,
            } if reported == job => {
                self.values = Some(values);
            }
            other => {
                let _ = writeln!(out, "job {}: ignored from the broker: {other:?}", job.seq());
            }
        }
        true
    }
}

/// How a job's attestation reads in the run's output and `hyalos doctor` (ADR-0057).
pub fn attestation_text(attestation: &hyalos_types::Attestation) -> String {
    match attestation {
        hyalos_types::Attestation::None(reason) => format!("none ({reason})"),
        hyalos_types::Attestation::Unrecorded => "unrecorded".to_string(),
    }
}
