//! Holding the issue as a handle and waiting for the operator's release of it (ADR-0063, ADR-0078).

use super::*;

/// How often the actor's job looks at the log while it waits for the operator's release.
const RELEASE_POLL: Duration = Duration::from_millis(500);

/// Ends a resumed run that held the issue (ADR-0078): a job its log shows running is ended
/// `handle.lost` and the task completed, since the issue's bytes went with the broker. The agent can
/// start the issue again; nothing is minted or run here.
pub(super) fn close_lost(
    request: &Request,
    config_dir: &std::path::Path,
    log: &std::path::Path,
    runs_dir: &std::path::Path,
    task: TaskId,
    out: &mut dyn Write,
) -> Result<Option<Summary>, String> {
    let key = keys::load(&config_dir.join(paths::ENGINE_KEY_FILE)).map_err(|e| e.to_string())?;
    let mut engine = Engine::open(log, key).map_err(|e| e.to_string())?;
    engine.stamp_forward();
    let mut clock = Stamps(
        SystemClock.now().max(
            engine
                .state()
                .last_at()
                .unwrap_or(Timestamp::from_millis(0)),
        ),
    );
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
    let running = engine
        .state()
        .jobs()
        .iter()
        .find(|(_, job)| job.task == task && job.is_running())
        .map(|(id, _)| *id);
    let mut summary = None;
    if let Some(job) = running {
        let _ = writeln!(
            out,
            "job {}: was running when its runner stopped, and the issue the broker held went with it; \
             ending the run. Start the issue again to run it anew",
            job.seq()
        );
        let mut ended = Ended {
            engine: &mut engine,
            clock: &mut clock,
            job,
        };
        let lost = Failure::new(
            why::LOST,
            "its runner stopped while the broker held the issue",
        );
        summary = Some(ended.with(out, JobOutcome::Failed, lost, Ok(()), None)?);
    }
    let head = clock.record(&mut engine, Event::TaskCompleted { task })?;
    let _ = writeln!(out, "task {}: completed (event {})", task.seq(), head.seq);
    let _ = std::fs::remove_file(runs_dir.join(format!("task-{}.held", task.seq())));
    Ok(summary)
}

/// Writes the awaited handle, the issue's repository and number where `hyalos release` reads them,
/// owner-only, and locks the file for as long as the returned file is kept.
fn write_held(
    file: &std::path::Path,
    handle: &Digest,
    repo: &str,
    number: u64,
) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(dir) = file.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let text = serde_json::json!({"handle": handle.to_string(), "repo": repo, "issue": number})
        .to_string();
    let mut held = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(file)?;
    // Locked before it says anything, so a release never reads a file no runner waits on.
    held.try_lock().map_err(std::io::Error::other)?;
    held.set_len(0)?;
    held.write_all(text.as_bytes())?;
    Ok(held)
}

impl Run<'_> {
    /// Waits, before anything is minted, until the log shows the operator released the awaited
    /// handle to this task with `hyalos release` (ADR-0078). Interrupts and a cancel end the wait as
    /// they end a job; so does the run's deadline. Nothing to wait for passes at once.
    pub(super) fn await_release(&mut self, job: JobId, out: &mut dyn Write) -> Result<(), Failure> {
        let waited = self.wait_for_release(job, out);
        if waited.is_err() {
            self.forget_held();
        }
        waited
    }

    /// The handle file is the waiting runner's alone: once it waits no more, nothing may release.
    pub(super) fn forget_held(&mut self) {
        if self.held_lock.take().is_some() {
            let _ = std::fs::remove_file(&self.held_file);
        }
    }

    fn wait_for_release(&mut self, job: JobId, out: &mut dyn Write) -> Result<(), Failure> {
        let Some((handle, number)) = self.awaiting else {
            return Ok(());
        };
        let task = self.task.seq();
        let _ = writeln!(
            out,
            "job {}: waiting for you to release issue #{number} to task {task}: run hyalos release {task} in a terminal of your own",
            job.seq()
        );
        loop {
            if let Some(failure) = interrupted(&self.interrupt, job, out) {
                return Err(failure);
            }
            self.engine.catch_up().map_err(|e| {
                Failure::new(why::HANDLE_REFUSED, format!("cannot read the log: {e}"))
            })?;
            if self.engine.state().handle_is_released(self.task, &handle) {
                let _ = writeln!(
                    out,
                    "job {}: issue #{number} released to the task by you",
                    job.seq()
                );
                self.forget_held();
                self.awaiting = None;
                self.handle = Some(handle);
                return Ok(());
            }
            let left = self.remaining();
            if left.is_zero() {
                return Err(Failure::new(
                    why::UNRELEASED,
                    format!("issue #{number} was not released before the run's deadline"),
                ));
            }
            std::thread::sleep(left.min(RELEASE_POLL));
        }
    }

    /// Has the broker hold issue `number` for `job`, then either records the operator's release of it
    /// to this run's task, bound to the digest the broker answered (ADR-0063), or keeps the digest for
    /// the operator to release while the actor waits (ADR-0078).
    pub(super) fn hold(
        &mut self,
        job: JobId,
        number: u64,
        out: &mut dyn Write,
    ) -> Result<(), Failure> {
        let wait = self.remaining().min(STEP_WAIT);
        let (handle, writers) = match self.broker.as_mut() {
            Some(broker) => broker.hold_issue(job, number, wait),
            None => Err(code("handle.broker_unavailable")),
        }
        .map_err(|reason| Failure {
            why: format!("the broker did not hold issue #{number} ({reason})"),
            reason,
        })?;
        if self.request.await_release {
            // The operator's policy may release an issue only the repository's writers wrote
            // (ADR-0103); anything else waits for the operator, as before.
            let now = self.clock.now();
            let by_policy = self.engine.release_issue_by_policy(
                now,
                self.task,
                handle,
                &self.request.repo,
                self.request.budget_tokens,
                writers,
            );
            match by_policy {
                Ok(_) => {
                    let _ = writeln!(
                        out,
                        "job {}: issue #{number} held as handle {handle}, released to the task by your policy (handle.policy_released)",
                        job.seq()
                    );
                    self.handle = Some(handle);
                    return Ok(());
                }
                Err(reason) => {
                    let _ = writeln!(
                        out,
                        "job {}: your policy did not release issue #{number} ({reason})",
                        job.seq()
                    );
                }
            }
            let lock =
                write_held(&self.held_file, &handle, &self.request.repo, number).map_err(|e| {
                    Failure::new(
                        why::HANDLE_REFUSED,
                        format!("cannot write {}: {e}", self.held_file.display()),
                    )
                })?;
            self.held_lock = Some(lock);
            let _ = writeln!(
                out,
                "job {}: issue #{number} held as handle {handle}, for you to release to the task",
                job.seq()
            );
            self.awaiting = Some((handle, number));
            return Ok(());
        }
        self.release(job, number, handle, out)
    }

    /// Records the operator's release of `handle` to this run's task. It is the operator's decision,
    /// made with `--release-issue` and signed with the key unlocked before the run (ADR-0070).
    fn release(
        &mut self,
        job: JobId,
        number: u64,
        handle: Digest,
        out: &mut dyn Write,
    ) -> Result<(), Failure> {
        let now = self.clock.now();
        self.engine
            .release_handle(
                now,
                self.task,
                handle,
                Decider::Human,
                code("handle.operator_flag"),
            )
            .map_err(|e| Failure::new(why::HANDLE_REFUSED, format!("the release refused: {e}")))?;
        let _ = writeln!(
            out,
            "job {}: issue #{number} held as handle {handle}, released to the task by --release-issue",
            job.seq()
        );
        self.handle = Some(handle);
        Ok(())
    }
}
