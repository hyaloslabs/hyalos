//! The actor's job: minting its token, starting the sandbox, recording what the broker reports while
//! it runs, and ending it.

use super::*;

impl Run<'_> {
    /// Runs the actor: a harness in a sandbox, given the operator's text and typed values only.
    pub(super) fn actor_job(
        &mut self,
        planned: &Planned,
        values: &Values,
        driver: Box<dyn Substrate>,
        substrate: SubstrateName,
        attestation: Attestation,
        out: &mut dyn Write,
    ) -> Result<Summary, String> {
        let request = self.request;
        let job = self.job_started(substrate, attestation)?;
        let _ = writeln!(out, "job {}: started (task {})", job.seq(), self.task.seq());
        if let Some(failure) = interrupted(&self.interrupt, job, out) {
            return self.end(job, out, JobOutcome::Failed, failure, Ok(()));
        }

        if let Err(failure) = self.await_release(job, out) {
            return self.end(job, out, JobOutcome::Failed, failure, Ok(()));
        }

        // The manifest, within policy; then the token, for that one repository. The prompt is planned
        // first, so the manifest's inputs come from it (ADR-0059).
        let branch = format!("hyalos/job-{}", job.seq());
        let handle = self.handle;
        let result = planned.result().cloned();
        let (prompt, inputs) =
            match actor_prompt(request, planned, values, handle.as_ref(), &branch) {
                Ok(planned) => planned,
                Err(failure) => return self.end(job, out, JobOutcome::Failed, failure, Ok(())),
            };
        let now = self.clock.now();
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            job,
            role: Role::Actor,
            tainted: true,
            repo: request.repo.clone(),
            branch_prefix: branch.clone(),
            pr_base: Some(request.base.clone()),
            actions: ACTOR_ACTIONS.map(String::from).into(),
            egress_hosts: request.egress_hosts.clone(),
            tools: BTreeSet::new(),
            // Its data goes to the model and the forge, and to the web only if it may reach a host
            // (ADR-0055).
            data_flows: [flow::MODEL, flow::FORGE]
                .into_iter()
                .chain((!request.egress_hosts.is_empty()).then_some(flow::WEB))
                .map(String::from)
                .collect(),
            budget: Budget {
                tokens: request.budget_tokens,
            },
            deadline: self.deadline,
            approval: None,
            handles: handle.into_iter().collect(),
            result: result.clone(),
            // The fix-issue run's own issue, from the request: its pull request closes it (ADR-0083).
            closes: request.issue_number,
        };
        // A skill in the prompt never widens the job: its grant cuts the manifest (ADR-0066).
        let manifest = inputs.bound(&manifest);
        let signed = match self
            .engine
            .mint_manifest(&manifest, &inputs, &self.manifest_key, now)
        {
            Ok(signed) => signed,
            Err(e) => {
                let failure = Failure::new(why::NO_MANIFEST, format!("no manifest: {e}"));
                return self.end(job, out, JobOutcome::Failed, failure, Ok(()));
            }
        };
        let token = match self.token(&manifest, now) {
            Ok(Some(token)) => token,
            Ok(None) => {
                let failure = Failure::new(
                    why::NO_TOKEN,
                    "no forge token: the job asks for no forge action",
                );
                return self.end(job, out, JobOutcome::Failed, failure, Ok(()));
            }
            Err(failure) => return self.end(job, out, JobOutcome::Failed, failure, Ok(())),
        };
        let _ = writeln!(out, "job {}: token minted for {}", job.seq(), request.repo);

        // The broker and the job's socket. From here every way out revokes the token, and a failed
        // revoke is reported the same way whatever ended the job.
        if let Some(failure) = interrupted(&self.interrupt, job, out) {
            let revoked = self.revoke(job, &token, out);
            return self.end(job, out, JobOutcome::Failed, failure, revoked);
        }
        let job_dir = match self.admit(job, Role::Actor, &signed, Some(&token)) {
            Ok(job_dir) => job_dir,
            Err(failure) => {
                let revoked = self.revoke(job, &token, out);
                return self.end(job, out, JobOutcome::Failed, failure, revoked);
            }
        };
        let _ = writeln!(out, "job {}: admitted by the broker", job.seq());
        // Ends the job in the broker and stops it, removes the job's socket, and revokes the token.
        let finish = |run: &mut Run<'_>, job_dir: JobDir, out: &mut dyn Write| {
            run.end_in_broker(job);
            run.shutdown_broker();
            drop(job_dir);
            run.revoke(job, &token, out)
        };
        if let Some(failure) = interrupted(&self.interrupt, job, out) {
            let revoked = finish(self, job_dir, out);
            return self.end(job, out, JobOutcome::Failed, failure, revoked);
        }

        // The sandbox, and the broker's reports while it runs.
        let mut spec = SandboxSpec::new(
            job,
            &request.image,
            request.harness.command(prompt),
            job_dir.socket(),
        );
        spec.deadline = self.remaining();
        spec.env = BTreeMap::from([
            ("HYALOS_REPO".into(), request.repo.clone()),
            ("HYALOS_BRANCH".into(), branch.clone()),
            ("HYALOS_BASE".into(), request.base.clone()),
            (
                "HYALOS_REMOTE".into(),
                format!("{FORWARDER}/git/{}.git", request.repo),
            ),
        ]);
        let mut sandbox = match driver.start(&spec) {
            Ok(sandbox) => sandbox,
            Err(e) => {
                let revoked = finish(self, job_dir, out);
                let failure = Failure::new(
                    why::SANDBOX_NOT_STARTED,
                    format!("the sandbox did not start: {e}"),
                );
                return self.end(job, out, JobOutcome::Failed, failure, revoked);
            }
        };
        let _ = writeln!(out, "job {}: sandbox {} running", job.seq(), sandbox.id());
        let stop = sandbox.stop_handle();
        let (done_tx, done) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done_tx.send(sandbox.wait());
        });
        // A waiter that stopped without an answer counts as a sandbox the runtime lost, so the job
        // still ends and its token is still revoked.
        let lost = || Outcome {
            ending: Ending::Lost,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        let Some(mut broker) = self.broker.take() else {
            stop.stop();
            let _ = done.recv();
            let revoked = finish(self, job_dir, out);
            let failure = Failure::new(why::BROKER_STOPPED, "the broker stopped");
            return self.end(job, out, JobOutcome::Failed, failure, revoked);
        };
        let mut state = Reported::default();
        let mut interruption = None;
        let mut stopping_held = false;
        let outcome = {
            let mut ended = Ended {
                engine: &mut self.engine,
                clock: &mut self.clock,
                job,
            };
            let outcome = loop {
                // A gate that could not answer ended the job held (ADR-0020): the sandbox has nothing
                // more to do, so it is stopped, and the loop goes on until the waiter says it has
                // ended.
                if state.held && !stopping_held {
                    stopping_held = true;
                    let _ = writeln!(out, "job {}: held; stopping the sandbox", job.seq());
                    stop.stop();
                }
                // An interrupt stops the sandbox; the loop then goes on until the waiter says it has
                // ended.
                if interruption.is_none() {
                    interruption = interrupted(&self.interrupt, job, out);
                    if interruption.is_some() {
                        stop.stop();
                    }
                }
                match done.try_recv() {
                    Ok(outcome) => break outcome,
                    Err(TryRecvError::Disconnected) => break lost(),
                    Err(TryRecvError::Empty) => {}
                }
                if state.broker_gone {
                    // Nothing more can be reported; the job's socket is gone, so the sandbox ends
                    // soon.
                    if interruption.is_none() {
                        interruption = interrupted(&self.interrupt, job, out);
                    }
                    if interruption.is_some() {
                        stop.stop();
                    }
                    break done.recv().unwrap_or_else(|_| lost());
                }
                state.take(&mut broker, POLL, &mut ended, out);
            };
            // What the broker reported just before the sandbox ended.
            while !state.broker_gone && state.take(&mut broker, POLL, &mut ended, out) {}
            // The harness's result, from the broker, once nothing else has failed the job
            // (ADR-0064). Reports that come first are recorded as any other.
            let finished = outcome.ending == Ending::Exited(0);
            if result.is_some() && finished && !state.held && state.refused.is_none() {
                if broker.send(&ToBroker::TakeResult { job }).is_err() {
                    state.broker_gone = true;
                }
                while !state.broker_gone
                    && state.values.is_none()
                    && state.refused.is_none()
                    && state.take(&mut broker, RESULT_WAIT, &mut ended, out)
                {}
            }
            outcome
        };
        self.broker = Some(broker);
        let _ = writeln!(
            out,
            "job {}: sandbox ended ({:?})",
            job.seq(),
            outcome.ending
        );
        // With a result to submit, what the harness printed is not its result: it is never read,
        // and the operator sees it only if the job failed (ADR-0064).
        let show = |out: &mut dyn Write| {
            let _ = writeln!(
                out,
                "--- harness output ---\n{}{}--- end ---",
                outcome.stdout_text(),
                String::from_utf8_lossy(&outcome.stderr)
            );
        };
        if result.is_none() {
            show(out);
        }
        // A harness that had finished before the stop took effect did its work: the job succeeded.
        // (The sandbox is in a process group of its own, so the operator's Ctrl-C never reached it.)
        // An interrupt that comes after the sandbox ended changes nothing: what follows is the
        // cleanup.
        if outcome.ending == Ending::Exited(0) {
            interruption = None;
        }

        // End the job in the broker, revoke the token, and record how it ended.
        let revoked = finish(self, job_dir, out);
        if state.held {
            if result.is_some() {
                show(out);
            }
            let _ = writeln!(out, "job {}: held; see the open hold in the log", job.seq());
            let revoked = match (&self.revoked, revoked) {
                (Err(earlier), _) => Err(earlier.clone()),
                (Ok(()), now) => now,
            };
            return Ok(Summary {
                job,
                outcome: JobOutcome::Held,
                revoked,
            });
        }
        let outcome_of = match (interruption, &state.refused, outcome.ending) {
            (Some(failure), _, _) => (JobOutcome::Failed, failure),
            (None, Some(reason), _) => (
                JobOutcome::Failed,
                Failure {
                    reason: reason.clone(),
                    why: format!("the broker refused ({reason})"),
                },
            ),
            (None, None, _) if state.broker_gone => (
                JobOutcome::Failed,
                Failure::new(why::BROKER_STOPPED, "the broker stopped"),
            ),
            (None, None, Ending::Exited(0)) => match (&result, &state.values) {
                (None, _) => (
                    JobOutcome::Succeeded,
                    Failure::new(why::FINISHED, "the harness finished"),
                ),
                (Some(output), Some(values)) if values_fit(output, values) => {
                    let _ = writeln!(
                        out,
                        "job {}: result: {}",
                        job.seq(),
                        shown_result(planned, values)
                    );
                    (
                        JobOutcome::Succeeded,
                        Failure::new(why::SUBMITTED, "the harness submitted its result"),
                    )
                }
                (Some(_), _) => (
                    JobOutcome::Failed,
                    Failure::new(why::NO_RESULT, "the broker gave no result that fits"),
                ),
            },
            (None, None, Ending::DeadlineExceeded) => (
                JobOutcome::DeadlineExceeded,
                Failure::new(why::DEADLINE, "the deadline passed"),
            ),
            (None, None, ending) => (
                JobOutcome::Failed,
                Failure::new(why::SANDBOX_FAILED, format!("the sandbox ended {ending:?}")),
            ),
        };
        if result.is_some() && outcome_of.0 != JobOutcome::Succeeded {
            show(out);
        }
        let (ended, failure) = outcome_of;
        // A submitted result is recorded with the ending, so the task's result is in the log.
        let values = state
            .values
            .filter(|_| failure.reason.as_str() == why::SUBMITTED);
        self.end_values(job, out, ended, failure, revoked, values)
    }
}
