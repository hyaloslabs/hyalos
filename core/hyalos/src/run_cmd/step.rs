//! A typed step: a reader or a planner, a job the broker runs that returns typed values (ADR-0062),
//! with no sandbox of its own.

use super::*;

impl Run<'_> {
    /// Runs reader or planner `step` as a job of its own: minted from the workflow's checked inputs,
    /// answered by the broker, and ended. `Ok(Some(summary))` if it failed, which stops the run.
    pub(super) fn typed_step(
        &mut self,
        flow: &Runnable,
        supplied: &Supplied,
        step: &Step,
        values: &mut Values,
        out: &mut dyn Write,
    ) -> Result<Option<Summary>, String> {
        let job = self.job_started(broker_substrate(), no_sandbox())?;
        let _ = writeln!(
            out,
            "job {}: step {} ({:?}) started",
            job.seq(),
            step.id,
            step.role
        );
        if let Some(failure) = interrupted(&self.interrupt, job, out) {
            return self
                .end(job, out, JobOutcome::Failed, failure, Ok(()))
                .map(Some);
        }
        let (protocol, model) = step_model(self.request);
        let call = flow.call(step, supplied, values, protocol, &model);
        let now = self.clock.now();
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            job,
            role: step.role,
            tainted: flow.tainted(step),
            repo: self.request.repo.clone(),
            // Nothing to push and no pull request: a reader or planner has no sandbox.
            branch_prefix: String::new(),
            pr_base: None,
            actions: call.issue.map(|_| "read".to_string()).into_iter().collect(),
            egress_hosts: BTreeSet::new(),
            tools: BTreeSet::new(),
            data_flows: [flow::MODEL.to_string()].into(),
            budget: Budget {
                tokens: self.request.budget_tokens.min(STEP_BUDGET_TOKENS),
            },
            deadline: self.deadline,
            approval: None,
            handles: Default::default(),
            result: None,
            closes: None,
        };
        let Some(inputs) = flow.checked().inputs(&step.id).cloned() else {
            let failure = Failure::new(workflow_run::codes::INPUTS_MISMATCH, "no inputs");
            return self
                .end(job, out, JobOutcome::Failed, failure, Ok(()))
                .map(Some);
        };
        let signed = match self
            .engine
            .mint_manifest(&manifest, &inputs, &self.manifest_key, now)
        {
            Ok(signed) => signed,
            Err(e) => {
                let failure = Failure::new(why::NO_MANIFEST, format!("no manifest: {e}"));
                return self
                    .end(job, out, JobOutcome::Failed, failure, Ok(()))
                    .map(Some);
            }
        };
        let token = match self.token(&manifest, now) {
            Ok(token) => token,
            Err(failure) => {
                return self
                    .end(job, out, JobOutcome::Failed, failure, Ok(()))
                    .map(Some);
            }
        };
        let revoke = |run: &Run<'_>, out: &mut dyn Write| {
            token
                .as_ref()
                .map_or(Ok(()), |token| run.revoke(job, token, out))
        };
        let job_dir = match self.admit(job, step.role, &signed, token.as_ref()) {
            Ok(job_dir) => job_dir,
            Err(failure) => {
                let revoked = revoke(self, out);
                return self
                    .end(job, out, JobOutcome::Failed, failure, revoked)
                    .map(Some);
            }
        };
        // The issue the step reads is held as a handle first, if the operator releases it to the
        // actor: now, with --release-issue, or later, while the actor waits (ADR-0078).
        let holds = self.request.release_issue || self.request.await_release;
        let held = match call.issue.filter(|_| holds) {
            Some(number) if self.handle.is_none() && self.awaiting.is_none() => {
                self.hold(job, number, out)
            }
            _ => Ok(()),
        };
        let wait = self.remaining().min(STEP_WAIT);
        let answered = match (held, self.broker.as_mut()) {
            (Err(failure), _) => Err(failure),
            (Ok(()), Some(broker)) => broker.read(job, &call, wait).map_err(|reason| Failure {
                why: format!("step {} did not answer ({reason})", step.id),
                reason,
            }),
            (Ok(()), None) => Err(Failure::new(
                "reader.broker_unavailable",
                format!("step {} did not answer (no broker)", step.id),
            )),
        };
        self.end_in_broker(job);
        drop(job_dir);
        let revoked = revoke(self, out);
        match answered {
            Ok(returned) if flow.accept(step, &returned, values) => {
                let _ = writeln!(
                    out,
                    "job {}: step {} answered: {}",
                    job.seq(),
                    step.id,
                    values.shown(&step.id)
                );
                let answered = Failure::new(why::ANSWERED, "the step returned its values");
                self.end_values(
                    job,
                    out,
                    JobOutcome::Succeeded,
                    answered,
                    revoked,
                    Some(returned),
                )?;
                if self.revoked.is_err() {
                    // A token that may still be live stops the run rather than minting another.
                    return Ok(Some(Summary {
                        job,
                        outcome: JobOutcome::Succeeded,
                        revoked: self.revoked.clone(),
                    }));
                }
                Ok(None)
            }
            Ok(_) => {
                let failure = Failure::new("reader.bad_answer", "the values do not fit the step");
                self.end(job, out, JobOutcome::Failed, failure, revoked)
                    .map(Some)
            }
            Err(failure) => self
                .end(job, out, JobOutcome::Failed, failure, revoked)
                .map(Some),
        }
    }
}

/// Where a reader or planner job runs: in the broker, with no sandbox (ADR-0062).
pub(super) fn broker_substrate() -> SubstrateName {
    SubstrateName::new("broker").unwrap_or_else(SubstrateName::unrecorded)
}

/// A job with no sandbox has nothing to attest.
fn no_sandbox() -> Attestation {
    Attestation::None(code("attestation.no_sandbox"))
}
