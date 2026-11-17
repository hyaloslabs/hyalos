//! Running the plan: opening the log and the App, starting or resuming the task, and the jobs in
//! turn, with the steps every job shares (admitting, minting, ending).

use super::*;

/// Runs one job, or a workflow's jobs one after another under one task, and writes what happens to
/// `out`. `Err` is a refusal before anything ran; once a job is recorded, how it ended is in the
/// [`Summary`] (of the last job that ran) and in the log.
pub fn run_job(request: &Request, setup: Setup, out: &mut dyn Write) -> Result<Summary, String> {
    drive(request, setup, None, out)?.ok_or_else(|| "no job ran".to_string())
}

/// Carries on with `task`, which `request` started, from what its log records (ADR-0069). A job the
/// log shows running has no runner any more: it is ended `engine.restarted` and its step runs again in
/// a new job. Steps that returned their values are not run again. `Ok(None)` if nothing was left to
/// run; the task is then completed if it was not.
pub fn resume_job(
    request: &Request,
    task: TaskId,
    setup: Setup,
    out: &mut dyn Write,
) -> Result<Option<Summary>, String> {
    drive(request, setup, Some(task), out)
}

fn drive(
    request: &Request,
    setup: Setup,
    resume: Option<TaskId>,
    out: &mut dyn Write,
) -> Result<Option<Summary>, String> {
    // 1. A ready substrate, or a refusal that says why, before anything is written.
    let Setup {
        config_dir,
        log,
        runtime_dir,
        broker_exe,
        detect,
        detect_timeout,
        mut interrupt,
        on_task,
        operator,
    } = setup;
    // Beside the log, where the MCP server keeps its runs' files (ADR-0069).
    let runs_dir = log.parent().map_or_else(
        || PathBuf::from(crate::runs::RUNS_DIR),
        |d| d.join(crate::runs::RUNS_DIR),
    );
    let before_recorded = |interrupt: &Interrupt| {
        interrupt.signal().map(|signal| {
            format!("interrupted ({signal}) before the job was recorded, so nothing was written")
        })
    };
    if resume.is_some() && request.issue_number.is_none() {
        return Err("only a fix-issue run can be resumed".into());
    }
    // A held issue lives in the broker's memory, which a stopped runner took with it: the run is
    // ended rather than resumed (ADR-0078).
    if let Some(task) = resume.filter(|_| request.release_issue || request.await_release) {
        return close_lost(request, &config_dir, &log, &runs_dir, task, out);
    }
    let substrate = choose(&request.image, detect, detect_timeout, &interrupt)?;
    let chosen_name = substrate.chosen_name().ok_or("no substrate was chosen")?;
    let driver = substrate.chosen.ok_or("no substrate was chosen")?;
    let attestation = driver.attestation();
    let _ = writeln!(out, "substrate: {chosen_name}");
    let _ = writeln!(out, "attestation: {}", attestation_text(&attestation));

    // 2. Everything else the run needs, still before anything is written: the workflow, checked as
    // at install, and the key its manifests are signed with.
    let planned = plan(request)?;
    let app = GitHubApp::load(&config_dir).map_err(|e| e.to_string())?;
    let provider_keys = config_dir.join(paths::PROVIDER_KEYS_FILE);
    if !provider_keys.exists() {
        return Err(format!(
            "there is no {}: the harness could not call a model. See docs/operations/provider-keys.md",
            provider_keys.display()
        ));
    }
    let manifest_key = fresh_manifest_key().map_err(|e| format!("no manifest key: {e}"))?;
    let key = keys::load(&config_dir.join(paths::ENGINE_KEY_FILE)).map_err(|e| e.to_string())?;
    if request.release_issue && operator.is_none() {
        return Err(
            "--release-issue records your decision, and the run was not given your approval key"
                .into(),
        );
    }
    let mut engine = Engine::open(&log, key).map_err(|e| e.to_string())?;
    if let Some(operator) = operator {
        engine = engine.with_operator(operator);
    }
    // Other runs write the same log; each write follows the latest event, whoever wrote it.
    engine.stamp_forward();
    // The skills, read back from the store and checked against what the operator installed, still
    // before anything is written (ADR-0089).
    let skills = installed_skills(
        request,
        &engine,
        &Store::new(log.with_file_name(paths::PLUGIN_STORE_DIR)),
        &log.with_file_name(crate::plugin_trust::TRUST_DIR),
    )?;
    // Never before the log's last event, which another process may have written.
    let mut clock = Stamps(
        SystemClock.now().max(
            engine
                .state()
                .last_at()
                .unwrap_or(Timestamp::from_millis(0)),
        ),
    );
    if let Some(refusal) = before_recorded(&interrupt) {
        return Err(refusal);
    }
    let minutes = Duration::from_secs(request.minutes.saturating_mul(60));

    // 3. The task. Every job of the run belongs to it, one at a time, and ends by the run deadline.
    let mut engine = engine;
    let (task, deadline) = match resume {
        None => {
            let created = engine
                .submit(
                    clock.now(),
                    Origin::Engine,
                    Event::TaskCreated {
                        request: request_digest(request),
                    },
                )
                .map_err(|e| e.to_string())?;
            let task = TaskId::from_seq(created.seq);
            if let Some(on_task) = on_task
                && let Some(cancel) = on_task(task)?
            {
                interrupt.watch(cancel);
            }
            (task, clock.now().saturating_add(minutes))
        }
        Some(task) => {
            let recorded = engine
                .state()
                .tasks()
                .get(&task)
                .ok_or_else(|| format!("there is no task {task}"))?;
            if recorded.request != request_digest(request) {
                return Err(format!("task {task} was not started with this request"));
            }
            if recorded.status() != TaskStatus::Open {
                return Ok(None);
            }
            (task, recorded.created_at.saturating_add(minutes))
        }
    };
    let mut run = Run {
        request,
        engine,
        clock,
        task,
        app,
        provider_keys,
        runtime_dir,
        broker_exe,
        manifest_key,
        deadline,
        interrupt,
        broker: None,
        revoked: Ok(()),
        handle: None,
        awaiting: None,
        held_file: runs_dir.join(format!("task-{}.held", task.seq())),
        held_lock: None,
        skills,
    };
    let summary = match &planned {
        Planned::Workflow(flow, _) => {
            let done = match resume {
                None => {
                    let _ = writeln!(
                        out,
                        "workflow {}: {} step(s), task {}",
                        flow.checked().workflow().name,
                        flow.checked().workflow().steps.len(),
                        task.seq()
                    );
                    Done::default()
                }
                Some(_) => run.recover(flow, out)?,
            };
            run.workflow(&planned, done, (driver, chosen_name, attestation), out)
        }
        Planned::Actor => run
            .actor_job(
                &planned,
                &Values::default(),
                driver,
                chosen_name,
                attestation,
                out,
            )
            .map(Some),
    };
    run.shutdown_broker();
    run.forget_held();
    // The run is over unless it stopped held, which only a decision ends (ADR-0029). A refusal here
    // is printed: the jobs are recorded either way.
    if let Ok(Some(last)) = &summary
        && last.outcome != JobOutcome::Held
    {
        run.complete(out);
    }
    summary
}

impl Run<'_> {
    pub(super) fn job_started(
        &mut self,
        substrate: SubstrateName,
        attestation: Attestation,
    ) -> Result<JobId, String> {
        let started = self
            .engine
            .submit(
                self.clock.now(),
                Origin::Engine,
                Event::JobStarted {
                    task: self.task,
                    substrate,
                    credentials: CredentialTier::BrokerHeld,
                    attestation,
                },
            )
            .map_err(|e| e.to_string())?;
        Ok(JobId::from_seq(started.seq))
    }

    /// Records how `job` ended. The run's [`Summary`] reports any revoke that failed, in this job or
    /// an earlier one.
    pub(super) fn end(
        &mut self,
        job: JobId,
        out: &mut dyn Write,
        outcome: JobOutcome,
        failure: Failure,
        revoked: Result<(), String>,
    ) -> Result<Summary, String> {
        self.end_values(job, out, outcome, failure, revoked, None)
    }

    /// [`Run::end`], recording in the same write the values the job returned (ADR-0069).
    pub(super) fn end_values(
        &mut self,
        job: JobId,
        out: &mut dyn Write,
        outcome: JobOutcome,
        failure: Failure,
        revoked: Result<(), String>,
        values: Option<Vec<TypedValue>>,
    ) -> Result<Summary, String> {
        let revoked = match (&self.revoked, revoked) {
            (Err(earlier), _) => Err(earlier.clone()),
            (Ok(()), now) => now,
        };
        self.revoked = revoked.clone();
        let mut ended = Ended {
            engine: &mut self.engine,
            clock: &mut self.clock,
            job,
        };
        ended.with(out, outcome, failure, revoked, values)
    }

    /// Records that the run is over (`task.completed`), however its last job ended.
    fn complete(&mut self, out: &mut dyn Write) {
        let task = self.task;
        let recorded = self.engine.submit(
            self.clock.now(),
            Origin::Engine,
            Event::TaskCompleted { task },
        );
        let _ = match recorded {
            Ok(head) => writeln!(out, "task {}: completed (event {})", task.seq(), head.seq),
            Err(e) => writeln!(
                out,
                "task {}: recording its completion failed: {e}",
                task.seq()
            ),
        };
    }

    /// What a resumed run's log shows done (ADR-0069). A job still running lost its runner, and with
    /// it its broker and sandbox: it is ended `engine.restarted` (or `job.deadline` past the run's
    /// deadline). The token it may have had was never written down, so it cannot be revoked here;
    /// GitHub expires it within an hour.
    fn recover(&mut self, flow: &Runnable, out: &mut dyn Write) -> Result<Done, String> {
        let task = self.task;
        let running = self
            .engine
            .state()
            .jobs()
            .iter()
            .find(|(_, job)| job.task == task && job.is_running())
            .map(|(id, _)| *id);
        if let Some(job) = running {
            let past = self.clock.now() >= self.deadline;
            let (outcome, reason) = if past {
                (JobOutcome::DeadlineExceeded, why::DEADLINE)
            } else {
                (JobOutcome::Failed, why::RESTARTED)
            };
            let _ = writeln!(
                out,
                "job {}: was running when its runner stopped; ending it. A GitHub token it held \
                 expires within an hour; a sandbox it ran stops at its deadline",
                job.seq()
            );
            self.end(
                job,
                out,
                outcome,
                Failure::new(reason, "its runner stopped"),
                Ok(()),
            )?;
        }
        let typed = flow.typed_steps();
        let total = typed.len() + 1;
        let mut done = Done::default();
        let jobs: Vec<_> = self
            .engine
            .state()
            .jobs()
            .iter()
            .filter(|(_, job)| job.task == task)
            .map(|(id, job)| (*id, job.clone()))
            .collect();
        for (id, job) in jobs {
            let Some((_, outcome)) = job.ended else {
                continue;
            };
            let last = Summary {
                job: id,
                outcome,
                revoked: Ok(()),
            };
            let restarted = job.reason.as_ref().map(ReasonCode::as_str) == Some(why::RESTARTED);
            match (outcome, &job.values) {
                (JobOutcome::Succeeded, Some(values)) if done.steps < total => {
                    let step = typed.get(done.steps).unwrap_or(flow.actor());
                    if !flow.accept(step, values, &mut done.values) {
                        return Err(format!("job {id}'s values do not fit step {}", step.id));
                    }
                    done.steps += 1;
                    done.over = (done.steps == total).then_some(last);
                }
                _ if restarted => done.over = None,
                _ => done.over = Some(last),
            }
        }
        let _ = writeln!(
            out,
            "task {}: resumed with {} of {total} step(s) done",
            task.seq(),
            done.steps
        );
        Ok(done)
    }

    /// The workflow's readers and planners, each a job the broker runs (ADR-0061, ADR-0062), from the
    /// first not done, then the actor in a sandbox.
    fn workflow(
        &mut self,
        planned: &Planned,
        done: Done,
        actor: (Box<dyn Substrate>, SubstrateName, Attestation),
        out: &mut dyn Write,
    ) -> Result<Option<Summary>, String> {
        if let Some(over) = done.over {
            return Ok(Some(over));
        }
        let Planned::Workflow(flow, supplied) = planned else {
            return Err("not a workflow".into());
        };
        let mut values = done.values;
        for step in flow.typed_steps().iter().skip(done.steps) {
            if let Some(stopped) = self.typed_step(flow, supplied, step, &mut values, out)? {
                return Ok(Some(stopped));
            }
        }
        let (driver, name, attestation) = actor;
        self.actor_job(planned, &values, driver, name, attestation, out)
            .map(Some)
    }

    pub(super) fn remaining(&mut self) -> Duration {
        let now = self.clock.now().as_millis();
        Duration::from_millis(self.deadline.as_millis().saturating_sub(now))
    }

    pub(super) fn revoke(
        &self,
        job: JobId,
        token: &ForgeToken,
        out: &mut dyn Write,
    ) -> Result<(), String> {
        let revoked = self.app.revoke(token).map_err(|e| e.to_string());
        let _ = match &revoked {
            Ok(()) => writeln!(out, "job {}: forge token revoked", job.seq()),
            Err(e) => writeln!(
                out,
                "job {}: REVOKING THE FORGE TOKEN FAILED: {e}. Revoke it on GitHub (the App's \
                 installation tokens expire within an hour); see docs/operations/github-app.md",
                job.seq()
            ),
        };
        revoked
    }

    /// Mints `manifest`'s forge token, if it asks for a forge action. `Ok(None)` if it asks for none.
    pub(super) fn token(
        &self,
        manifest: &Manifest,
        now: Timestamp,
    ) -> Result<Option<ForgeToken>, Failure> {
        match self.app.mint(manifest, now) {
            Ok(minted) => Ok(minted.map(|m| m.token)),
            Err(e) => Err(Failure::new(why::NO_TOKEN, format!("no forge token: {e}"))),
        }
    }

    /// Starts `job` in the broker (launching it for the first job) on a socket of its own.
    pub(super) fn admit(
        &mut self,
        job: JobId,
        role: Role,
        signed: &SignedManifest,
        token: Option<&ForgeToken>,
    ) -> Result<JobDir, Failure> {
        let not_started = |text: String| Failure::new(why::BROKER_NOT_STARTED, text);
        let manifest = signed.to_bytes().map_err(|e| not_started(e.to_string()))?;
        if self.broker.as_ref().is_none_or(|b| !b.is_open()) {
            paths::make_private_dir(&self.runtime_dir).map_err(not_started)?;
            let broker = BrokerProcess::launch_configured(
                &self.broker_exe,
                &self.runtime_dir,
                &self.manifest_key.public(),
                Some(&self.provider_keys),
                Some(self.app.forge_bases()),
                BROKER_WAIT,
            )
            .map_err(|e| not_started(format!("the broker did not start: {e}")))?;
            self.broker = Some(broker);
        }
        let broker = self
            .broker
            .as_mut()
            .ok_or_else(|| not_started("no broker".into()))?;
        let job_dir = JobDir::create(&self.runtime_dir)
            .map_err(|e| not_started(format!("no job directory: {e}")))?;
        broker
            .send(&ToBroker::StartJob {
                job,
                role,
                manifest,
                socket: job_dir.socket().to_string_lossy().into_owned(),
                forge_token: token.cloned(),
            })
            .map_err(|e| not_started(format!("the broker did not take the job: {e}")))?;
        match broker.recv() {
            Ok(Some(ToEngine::Admitted { job: admitted })) if admitted == job => Ok(job_dir),
            Ok(Some(ToEngine::Refused { reason, .. })) => Err(Failure {
                why: format!("the broker refused the job ({reason})"),
                reason,
            }),
            Ok(other) => Err(not_started(format!("the broker answered {other:?}"))),
            Err(e) => Err(not_started(format!("the broker did not answer: {e}"))),
        }
    }

    /// Ends `job` in the broker, if it is still running.
    pub(super) fn end_in_broker(&mut self, job: JobId) {
        if let Some(broker) = self.broker.as_mut().filter(|b| b.is_open()) {
            let _ = broker.send(&ToBroker::EndJob { job });
        }
    }

    pub(super) fn shutdown_broker(&mut self) {
        if let Some(mut broker) = self.broker.take()
            && broker.is_open()
        {
            let _ = broker.send(&ToBroker::Shutdown);
        }
    }
}

/// Runs `detect` with a timeout, and refuses a host that cannot run a job with what the operator
/// needs to know. An interrupt while it runs refuses too.
fn choose(
    image: &str,
    detect: Detect,
    timeout: Duration,
    interrupt: &Interrupt,
) -> Result<Choice, String> {
    let (tx, rx) = mpsc::channel();
    let image_owned = image.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(detect(&image_owned));
    });
    let interrupted = |signal: &str| {
        format!("interrupted ({signal}) while finding a substrate, so no job was started")
    };
    let mut waited = Duration::ZERO;
    let detected = loop {
        if let Some(signal) = interrupt.signal() {
            return Err(interrupted(signal));
        }
        match rx.recv_timeout(POLL) {
            Ok(detected) => break detected,
            Err(mpsc::RecvTimeoutError::Timeout) if waited < timeout => waited += POLL,
            Err(_) => {
                return Err(format!(
                    "finding a substrate took longer than {} s, so a container runtime is not \
                     answering and no job was started. Check that `podman info` or `docker info` \
                     returns, then run hyalos doctor",
                    timeout.as_secs()
                ));
            }
        }
    };
    // A runtime the same Ctrl-C reached may have failed its check; say why it was.
    if let Some(signal) = interrupt.signal() {
        return Err(interrupted(signal));
    }
    match detected {
        Err(unsupported) => Err(unsupported.to_string()),
        Ok(choice) if choice.chosen.is_none() => {
            let checked = Ok(choice);
            let (report, _) = doctor_cmd::report(HOST_OS, &checked, image);
            Err(format!(
                "no substrate is ready, so no job was started. hyalos doctor says:\n{report}"
            ))
        }
        Ok(choice) => Ok(choice),
    }
}

/// Each skill `request` names, as installed: its files from `store`, checked against the digest the
/// install recorded, and the grant a human gave it. A skill not installed, revoked (ADR-0098), or
/// whose files changed since, refuses the run.
fn installed_skills(
    request: &Request,
    engine: &Engine,
    store: &Store,
    trust: &std::path::Path,
) -> Result<Vec<(Skill, Grant)>, String> {
    // The list in force, not only the marks it left: an import cut short before it marked every
    // release still keeps them from jobs, and a list that cannot be read keeps every skill out.
    let list = if request.skills.is_empty() {
        None
    } else {
        crate::plugin_trust::revocations_kept_in(trust, Some(engine.state()))?
    };
    request
        .skills
        .iter()
        .map(|name| {
            let installed = engine.state().plugins().get(name).ok_or_else(|| {
                format!("there is no skill {name} installed; install it with hyalos plugin install")
            })?;
            let reason = engine
                .state()
                .revoked_plugins()
                .get(name)
                .map(|r| &r.reason)
                .or_else(|| list.as_ref().and_then(|l| l.reason(&installed.digest)));
            if let Some(reason) = reason {
                return Err(format!(
                    "skill {name} is revoked ({reason}) and disabled, so no job is given it; install a release that is not revoked (plugin.revoked)"
                ));
            }
            let skill = store
                .load(&installed.digest, name)
                .map_err(|e| format!("skill {name}: {e} ({})", e.code()))?;
            Ok((skill, installed.grant.clone()))
        })
        .collect()
}
