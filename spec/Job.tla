-------------------------------- MODULE Job --------------------------------
(***************************************************************************)
(* The job state machine (S8.2, ADR-0013, ADR-0108).                       *)
(*                                                                         *)
(* Tasks, the jobs that run on them, the holds a gate's unknown answer     *)
(* opens, the manifest's deadline, the operator's merge approval and the   *)
(* pull request's head on the forge. The engine's state is a projection    *)
(* of its log (ADR-0003): every action below writes the events it records  *)
(* and updates the projection itself, and ProjectionIsReplay checks the    *)
(* two never disagree.                                                     *)
(*                                                                         *)
(* The four invariants of ADR-0013 are each also a property every          *)
(* simulated run is checked against (crates/world/src/spec.rs), under the  *)
(* same names:                                                             *)
(*   HeldTaskRunsNoJob, MergeOnlyAtApprovedHead,                           *)
(*   ExpiredManifestAuthorizesNothing, ProjectionIsReplay.                 *)
(*                                                                         *)
(* Mutant names one deliberate mistake, so a check can show that each      *)
(* invariant catches the mistake it is for. Job.cfg sets it to "none".     *)
(***************************************************************************)
EXTENDS Naturals, Sequences, TLC

CONSTANTS
    Tasks,    \* the tasks that may be created
    Shas,     \* the commits a pull request's head may be
    NoSha,    \* no commit: a model value, so it is compared only with commits
    MaxTime,  \* the clock stops here, which bounds every deadline
    MaxLog,   \* the state constraint on the log's length
    Mutant    \* "none", or the one mistake to make: see Mutants

Mutants == {"none", "split_hold", "stale_merge", "late_work", "unlogged_release"}

ASSUME Mutant \in Mutants
ASSUME MaxTime \in Nat /\ MaxLog \in Nat
ASSUME NoSha \notin Shas

\* The tasks are interchangeable, so the model checker explores one of each pair.
TaskSymmetry == Permutations(Tasks)
Kinds == {"actor", "merge"}
TaskStates == {"none", "open", "completed", "cancelled"}
JobStates == {"none", "running", "succeeded", "failed", "held", "deadline_exceeded"}

VARIABLES
    now,       \* the engine's clock
    task,      \* the projection: each task's state,
    kind,      \*   what it is for,
    job,       \*   its latest job's state,
    deadline,  \*   the deadline in that job's manifest,
    hold,      \*   whether it has an open hold,
    approval,  \*   and the head the operator approved for its merge
    head,      \* the pull request's head on the forge
    merges,    \* what the forge merged: [t, sha, head] at the moment of the merge
    work,      \* every step the broker admitted: [t, at, deadline]
    log        \* the events, in order

projection == <<task, kind, job, deadline, hold, approval>>
vars == <<now, task, kind, job, deadline, hold, approval, head, merges, work, log>>

-----------------------------------------------------------------------------
(* Replaying the log. This is written apart from the actions below, as     *)
(* replay is apart from the engine's writes, so the two can disagree.      *)

Empty == [task |-> [t \in Tasks |-> "none"],
          kind |-> [t \in Tasks |-> "none"],
          job |-> [t \in Tasks |-> "none"],
          deadline |-> [t \in Tasks |-> 0],
          hold |-> [t \in Tasks |-> FALSE],
          approval |-> [t \in Tasks |-> NoSha]]

Apply(s, e) ==
    CASE e.ev = "task.created"   -> [s EXCEPT !.task[e.t] = "open", !.kind[e.t] = e.kind]
      [] e.ev = "merge.approved" -> [s EXCEPT !.approval[e.t] = e.sha]
      [] e.ev = "job.started"    -> [s EXCEPT !.job[e.t] = "running", !.deadline[e.t] = e.deadline]
      [] e.ev = "job.ended"      -> [s EXCEPT !.job[e.t] = e.outcome]
      [] e.ev = "hold.opened"    -> [s EXCEPT !.hold[e.t] = TRUE]
      [] e.ev = "hold.resolved"  -> [s EXCEPT !.hold[e.t] = FALSE]
      [] e.ev = "task.completed" -> [s EXCEPT !.task[e.t] = "completed"]
      [] e.ev = "task.cancelled" -> [s EXCEPT !.task[e.t] = "cancelled", !.hold[e.t] = FALSE]
      [] OTHER                   -> s   \* gate.answered changes no state

RECURSIVE ReplayFrom(_, _)
ReplayFrom(s, l) == IF l = <<>> THEN s ELSE ReplayFrom(Apply(s, Head(l)), Tail(l))

Replay == ReplayFrom(Empty, log)

State == [task |-> task, kind |-> kind, job |-> job, deadline |-> deadline,
          hold |-> hold, approval |-> approval]

-----------------------------------------------------------------------------

Init ==
    /\ now = 0
    /\ task = [t \in Tasks |-> "none"]
    /\ kind = [t \in Tasks |-> "none"]
    /\ job = [t \in Tasks |-> "none"]
    /\ deadline = [t \in Tasks |-> 0]
    /\ hold = [t \in Tasks |-> FALSE]
    /\ approval = [t \in Tasks |-> NoSha]
    /\ head \in Shas
    /\ merges = {}
    /\ work = {}
    /\ log = <<>>

\* Time passes.
Tick ==
    /\ now < MaxTime
    /\ now' = now + 1
    /\ UNCHANGED <<task, kind, job, deadline, hold, approval, head, merges, work, log>>

CreateTask(t, k) ==
    /\ task[t] = "none"
    /\ task' = [task EXCEPT ![t] = "open"]
    /\ kind' = [kind EXCEPT ![t] = k]
    /\ log' = Append(log, [ev |-> "task.created", t |-> t, kind |-> k])
    /\ UNCHANGED <<now, job, deadline, hold, approval, head, merges, work>>

\* The operator names the commit they reviewed (ADR-0071). It need not be the
\* head now: they may have reviewed a commit a push has since replaced.
ApproveMerge(t, s) ==
    /\ task[t] = "open" /\ kind[t] = "merge"
    /\ job[t] = "none" /\ approval[t] = NoSha
    /\ approval' = [approval EXCEPT ![t] = s]
    /\ log' = Append(log, [ev |-> "merge.approved", t |-> t, sha |-> s])
    /\ UNCHANGED <<now, task, kind, job, deadline, hold, head, merges, work>>

\* A job starts with a manifest whose deadline is in the future. A task with an
\* open hold starts none (ADR-0009). A merge task runs one job, and only once
\* its merge is approved (ADR-0071).
StartJob(t, d) ==
    /\ task[t] = "open"
    /\ job[t] # "running"
    /\ ~hold[t]
    /\ kind[t] = "merge" => job[t] = "none" /\ approval[t] # NoSha
    /\ d \in (now + 1)..MaxTime
    /\ job' = [job EXCEPT ![t] = "running"]
    /\ deadline' = [deadline EXCEPT ![t] = d]
    /\ log' = Append(log, [ev |-> "job.started", t |-> t, deadline |-> d])
    /\ UNCHANGED <<now, task, kind, hold, approval, head, merges, work>>

\* The broker admits a step only while the manifest is in date: at the deadline
\* itself it still is, after it not (ADR-0004).
Admitted(t) ==
    /\ job[t] = "running"
    /\ (now <= deadline[t] \/ Mutant = "late_work")

Admit(t) == work' = work \cup {[t |-> t, at |-> now, deadline |-> deadline[t]]}

\* A gate allows a step, or denies it and the step is refused. Either way the job
\* goes on and no state changes, so the spec has one answer for both.
Gate(t) ==
    /\ Admitted(t)
    /\ Admit(t)
    /\ log' = Append(log, [ev |-> "gate.answered", t |-> t, answer |-> "allow"])
    /\ UNCHANGED <<now, task, kind, job, deadline, hold, approval, head, merges>>

\* A gate answers unknown. Ending the job and opening the hold are one write, so
\* no state has an open hold and a running job (ADR-0009, ADR-0020).
GateUnknown(t) ==
    /\ Admitted(t)
    /\ Admit(t)
    /\ hold' = [hold EXCEPT ![t] = TRUE]
    /\ IF Mutant = "split_hold"
         THEN /\ log' = log \o << [ev |-> "gate.answered", t |-> t, answer |-> "unknown"],
                                  [ev |-> "hold.opened", t |-> t] >>
              /\ UNCHANGED job
         ELSE /\ log' = log \o << [ev |-> "gate.answered", t |-> t, answer |-> "unknown"],
                                  [ev |-> "job.ended", t |-> t, outcome |-> "held"],
                                  [ev |-> "hold.opened", t |-> t] >>
              /\ job' = [job EXCEPT ![t] = "held"]
    /\ UNCHANGED <<now, task, kind, deadline, approval, head, merges>>

\* Only the split_hold mutant reaches this: the job is stopped a step later.
StopHeld(t) ==
    /\ Mutant = "split_hold"
    /\ hold[t] /\ job[t] = "running"
    /\ job' = [job EXCEPT ![t] = "held"]
    /\ log' = Append(log, [ev |-> "job.ended", t |-> t, outcome |-> "held"])
    /\ UNCHANGED <<now, task, kind, deadline, hold, approval, head, merges, work>>

\* An actor's job pushes to the pull request's branch, which moves its head.
Push(t, s) ==
    /\ kind[t] = "actor"
    /\ Admitted(t)
    /\ Admit(t)
    /\ head' = s
    /\ UNCHANGED <<now, task, kind, job, deadline, hold, approval, merges, log>>

\* An actor's sandbox exits.
Finish(t, outcome) ==
    /\ kind[t] = "actor" /\ job[t] = "running"
    /\ outcome \in {"succeeded", "failed"}
    /\ job' = [job EXCEPT ![t] = outcome]
    /\ log' = Append(log, [ev |-> "job.ended", t |-> t, outcome |-> outcome])
    /\ UNCHANGED <<now, task, kind, deadline, hold, approval, head, merges, work>>

\* The merge job asks the forge to merge at the approved head. The forge refuses
\* if the head has moved, so the binding holds at the moment of the merge.
Merge(t) ==
    /\ kind[t] = "merge"
    /\ Admitted(t)
    /\ Admit(t)
    /\ LET at == IF Mutant = "stale_merge" THEN head ELSE approval[t] IN
         IF at = head
           THEN /\ merges' = merges \cup {[t |-> t, sha |-> at, head |-> head]}
                /\ job' = [job EXCEPT ![t] = "succeeded"]
                /\ log' = Append(log, [ev |-> "job.ended", t |-> t, outcome |-> "succeeded"])
           ELSE /\ job' = [job EXCEPT ![t] = "failed"]   \* merge.head_moved
                /\ log' = Append(log, [ev |-> "job.ended", t |-> t, outcome |-> "failed"])
                /\ UNCHANGED merges
    /\ UNCHANGED <<now, task, kind, deadline, hold, approval, head>>

\* A step asked for after the deadline is refused, and the job ends.
Expire(t) ==
    /\ job[t] = "running" /\ now > deadline[t]
    /\ job' = [job EXCEPT ![t] = "deadline_exceeded"]
    /\ log' = Append(log, [ev |-> "job.ended", t |-> t, outcome |-> "deadline_exceeded"])
    /\ UNCHANGED <<now, task, kind, deadline, hold, approval, head, merges, work>>

\* A human releases the hold. The task may then run another job.
ReleaseHold(t) ==
    /\ hold[t]
    /\ hold' = [hold EXCEPT ![t] = FALSE]
    /\ IF Mutant = "unlogged_release"
         THEN UNCHANGED log
         ELSE log' = Append(log, [ev |-> "hold.resolved", t |-> t])
    /\ UNCHANGED <<now, task, kind, job, deadline, approval, head, merges, work>>

\* Only a task whose job succeeded, with no hold, completes.
Complete(t) ==
    /\ task[t] = "open" /\ job[t] = "succeeded" /\ ~hold[t]
    /\ task' = [task EXCEPT ![t] = "completed"]
    /\ log' = Append(log, [ev |-> "task.completed", t |-> t])
    /\ UNCHANGED <<now, kind, job, deadline, hold, approval, head, merges, work>>

\* A human cancels a task with no running job, and its holds with it (ADR-0029).
Cancel(t) ==
    /\ task[t] = "open" /\ job[t] # "running"
    /\ task' = [task EXCEPT ![t] = "cancelled"]
    /\ hold' = [hold EXCEPT ![t] = FALSE]
    /\ log' = Append(log, [ev |-> "task.cancelled", t |-> t])
    /\ UNCHANGED <<now, kind, job, deadline, approval, head, merges, work>>

Next ==
    \/ Tick
    \/ \E t \in Tasks :
         \/ \E k \in Kinds : CreateTask(t, k)
         \/ \E s \in Shas : ApproveMerge(t, s) \/ Push(t, s)
         \/ \E d \in 0..MaxTime : StartJob(t, d)
         \/ Gate(t)
         \/ GateUnknown(t) \/ StopHeld(t)
         \/ \E o \in {"succeeded", "failed"} : Finish(t, o)
         \/ Merge(t) \/ Expire(t) \/ ReleaseHold(t) \/ Complete(t) \/ Cancel(t)

Spec == Init /\ [][Next]_vars

\* The log is bounded so the model checker ends.
Bounded == Len(log) <= MaxLog

-----------------------------------------------------------------------------
(* The invariants.                                                         *)

TypeOK ==
    /\ now \in 0..MaxTime
    /\ task \in [Tasks -> TaskStates]
    /\ kind \in [Tasks -> Kinds \cup {"none"}]
    /\ job \in [Tasks -> JobStates]
    /\ deadline \in [Tasks -> 0..MaxTime]
    /\ hold \in [Tasks -> BOOLEAN]
    /\ approval \in [Tasks -> Shas \cup {NoSha}]
    /\ head \in Shas

\* A task with an open hold has no running job (ADR-0009).
HeldTaskRunsNoJob == \A t \in Tasks : hold[t] => job[t] # "running"

\* A merge happens only after an approval bound to the pull request's head
\* commit, and only while the head is still that commit (ADR-0004, ADR-0012).
MergeOnlyAtApprovedHead ==
    \A m \in merges : /\ kind[m.t] = "merge"
                      /\ approval[m.t] # NoSha
                      /\ m.sha = approval[m.t]
                      /\ m.head = m.sha

\* An expired manifest never authorizes work (ADR-0004).
ExpiredManifestAuthorizesNothing == \A w \in work : w.at <= w.deadline

\* The projection equals a replay of the log (ADR-0003).
ProjectionIsReplay == Replay = State

\* A completed or cancelled task has no open hold and no running job.
EndedTaskIsQuiet ==
    \A t \in Tasks : task[t] \in {"completed", "cancelled"} => ~hold[t] /\ job[t] # "running"

=============================================================================
