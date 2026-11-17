//! A typed step: a reader, a planner or a deterministic step, a job the broker runs that returns
//! typed values (ADR-0062, ADR-0102), with no sandbox of its own.

use super::*;

impl Run<'_> {
    /// Runs reader, planner or deterministic `step` as a job of its own: minted from the workflow's checked inputs,
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
        // A deterministic step runs its plugin, and calls no model and reads no issue (ADR-0102).
        let deterministic = step.role == Role::Deterministic;
        let call = flow.call(step, supplied, values, protocol, &model);
        let call_issue = call.issue.filter(|_| !deterministic);
        let plugin = match self.plugin_call(flow, supplied, step, values) {
            Ok(plugin) => plugin,
            Err(failure) => {
                return self
                    .end(job, out, JobOutcome::Failed, failure, Ok(()))
                    .map(Some);
            }
        };
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
            actions: call_issue.map(|_| "read".to_string()).into_iter().collect(),
            egress_hosts: BTreeSet::new(),
            tools: BTreeSet::new(),
            data_flows: (!deterministic)
                .then(|| flow::MODEL.to_string())
                .into_iter()
                .collect(),
            budget: Budget {
                tokens: self.request.budget_tokens.min(STEP_BUDGET_TOKENS),
            },
            deadline: self.deadline,
            approval: None,
            handles: Default::default(),
            result: None,
            closes: None,
        };
        // The plugins that reached the step bound it by their grants (ADR-0066, ADR-0102).
        let Some(inputs) = flow
            .checked()
            .inputs(&step.id)
            .map(|i| i.clone().with_grants(self.grants(&flow.plugins(step))))
        else {
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
        let held = match call_issue.filter(|_| holds) {
            Some(number) if self.handle.is_none() && self.awaiting.is_none() => {
                self.hold(job, number, out)
            }
            _ => Ok(()),
        };
        let wait = self.remaining().min(STEP_WAIT);
        let answered = match (held, self.broker.as_mut()) {
            (Err(failure), _) => Err(failure),
            (Ok(()), Some(broker)) => match &plugin {
                Some(plugin) => broker.run_plugin(job, plugin, wait),
                None => broker.read(job, &call, wait),
            }
            .map_err(|reason| Failure {
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

impl Run<'_> {
    /// The grants of `names`, as installed. Every plugin the run names was loaded before it started.
    pub(super) fn grants(&self, names: &BTreeSet<PluginName>) -> Vec<Grant> {
        names
            .iter()
            .filter_map(|name| self.plugins.get(name).map(|(_, grant)| grant.clone()))
            .collect()
    }

    /// The call a deterministic step's plugin is run with (ADR-0102), or `None` for another step.
    fn plugin_call(
        &self,
        flow: &Runnable,
        supplied: &Supplied,
        step: &Step,
        values: &Values,
    ) -> Result<Option<PluginCall>, Failure> {
        let Some(name) = step
            .plugin
            .as_ref()
            .filter(|_| step.role == Role::Deterministic)
        else {
            return Ok(None);
        };
        let Some((skill, _)) = self.plugins.get(name) else {
            return Err(Failure::new(
                "plugin.not_installed",
                format!("no plugin {name}"),
            ));
        };
        let input = flow.plugin_input(step, supplied, values);
        if input.len() > MAX_PLUGIN_INPUT {
            return Err(Failure::new(
                "plugin.input_too_large",
                format!("step {}'s input is over {MAX_PLUGIN_INPUT} bytes", step.id),
            ));
        }
        Ok(Some(PluginCall {
            store: self.plugin_store.to_string_lossy().into_owned(),
            name: name.clone(),
            digest: skill.digest(),
            input,
            output: step.output.clone().unwrap_or_default(),
        }))
    }
}
