//! Skills Hyalos drafts from runs that succeeded (S9.2, ADR-0113).
//!
//! A draft is written from what the log holds and the workflow says, never from text the forge or a
//! model wrote: the workflow's step names, roles and field names, the option each choice picked in
//! its author's words, and the run's request. [`files`] is a pure function of those, so the same
//! log always drafts the same bytes. A model chose the values after reading the issue, so a draft is
//! tainted, and it asks for no more than `Grant::read_only`: a job given it reads and talks to the
//! model, and nothing else.

use std::collections::BTreeMap;
use std::path::Path;

use hyalos_engine::State;
use hyalos_plugin::skill::{CAPABILITIES_FILE, SKILL_FILE, Skill, Store};
use hyalos_types::workflow::{Field, Workflow};
use hyalos_types::{Digest, Grant, PluginName, TaskId, TypedValue};

/// The run's request, as the log's `task.created` digests it, so a draft can be checked against
/// the log without the run's other files (ADR-0113).
pub const RUN_FILE: &str = "run.json";

/// What a draft of `task` is named.
pub fn name(task: TaskId) -> Option<PluginName> {
    PluginName::new(&format!("draft-{}", task.seq()))
}

/// The values each step of `task` returned, in order: one per job that returned any. A job that was
/// ended and run again returned none.
pub fn values(state: &State, task: TaskId) -> Vec<Vec<TypedValue>> {
    state
        .jobs()
        .values()
        .filter(|j| j.task == task)
        .filter_map(|j| j.values.clone())
        .collect()
}

/// The files of the skill drafted from `task`, a run of `workflow` whose steps returned `values`, in
/// order, and whose request is `request`. `None` unless every step with an output returned values
/// that fit it.
pub fn files(
    task: TaskId,
    workflow: &Workflow,
    values: &[Vec<TypedValue>],
    request: &[u8],
) -> Option<BTreeMap<String, Vec<u8>>> {
    let name = name(task)?;
    let steps: Vec<_> = workflow
        .steps
        .iter()
        .filter_map(|s| s.output.as_ref().map(|o| (s, o)))
        .collect();
    if steps.len() != values.len() {
        return None;
    }
    let mut lines = Vec::new();
    for ((step, output), values) in steps.iter().zip(values) {
        if output.len() != values.len() {
            return None;
        }
        let mut said = Vec::new();
        for ((field, kind), value) in output.iter().zip(values) {
            let shown = match (kind, value) {
                (Field::Choice(options), TypedValue::Choice(at)) => {
                    serde_json::to_string(options.get(*at as usize)?).ok()?
                }
                (Field::Bool, TypedValue::Bool(b)) => b.to_string(),
                (Field::Integer(_), TypedValue::Integer(n)) => n.to_string(),
                _ => return None,
            };
            said.push(format!("`{field}` = {shown}"));
        }
        let role = serde_json::to_value(step.role).ok()?;
        lines.push(format!(
            "- `{}` ({}): {}",
            step.id,
            role.as_str()?,
            said.join("; ")
        ));
    }
    let workflow_name = serde_json::to_string(&workflow.name).ok()?;
    let task = task.seq();
    let skill = format!(
        "---\nname: {name}\ndescription: What each step returned in task {task}, a run that succeeded. Drafted by Hyalos from the run's typed values; tainted.\n---\n\
         # What worked in task {task}\n\n\
         Hyalos drafted this skill from task {task}, a run of the workflow {workflow_name} that succeeded. \
         It holds only what each step returned, in the words of the workflow's author, and the run's request in `{RUN_FILE}`. \
         A model chose those values after reading text from the forge, so the draft is tainted: a job given it runs with read capabilities only.\n\n\
         ## What each step returned\n\n{}\n",
        lines.join("\n")
    );
    Some(BTreeMap::from([
        (SKILL_FILE.to_string(), skill.into_bytes()),
        (
            CAPABILITIES_FILE.to_string(),
            br#"{"version": 1, "actions": ["read"], "data_flows": ["job->model"]}"#.to_vec(),
        ),
        (RUN_FILE.to_string(), request.to_vec()),
    ]))
}

/// Keeps `files` in the plugin store at `store` and returns the skill's digest. The skill is read
/// back from the files as any skill is, so it is held to the same limits, and its capabilities must
/// be within `Grant::read_only`.
pub fn store(
    store: &Path,
    task: TaskId,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<Digest, String> {
    let partial = store.join(format!(".draft-{}-{}", task.seq(), std::process::id()));
    let _ = std::fs::remove_dir_all(&partial);
    let written = (|| {
        std::fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
        for (path, bytes) in files {
            std::fs::write(partial.join(path), bytes).map_err(|e| e.to_string())?;
        }
        let skill = Skill::read(&partial).map_err(|e| e.to_string())?;
        if !skill.asks.within(&Grant::read_only()) {
            return Err("a draft asks for more than read capabilities".to_string());
        }
        Store::new(store).put(&skill).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_dir_all(&partial);
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> Workflow {
        crate::workflow_run::fix_issue()
            .map(|c| c.workflow().clone())
            .unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn fix_issue_values() -> Vec<Vec<TypedValue>> {
        use TypedValue::{Bool, Choice};
        vec![
            vec![Bool(true), Bool(false), Choice(2), Choice(0)],
            vec![Bool(false), Choice(2)],
            vec![Choice(0), Bool(true)],
        ]
    }

    #[test]
    fn a_draft_is_written_from_the_workflows_words_and_the_typed_values_alone() {
        let task = TaskId::from_seq(12);
        let drafted = files(task, &flow(), &fix_issue_values(), b"{\"version\":1}").unwrap();
        let skill = String::from_utf8(drafted[SKILL_FILE].clone()).unwrap();
        assert!(skill.starts_with("---\nname: draft-12\n"), "{skill}");
        assert!(skill.contains("`outcome` = \"pr_opened\""), "{skill}");
        assert!(skill.contains("`tests_added` = true"), "{skill}");
        assert_eq!(drafted[RUN_FILE], b"{\"version\":1}");
        // The same log drafts the same bytes.
        assert_eq!(
            files(task, &flow(), &fix_issue_values(), b"{\"version\":1}"),
            Some(drafted.clone())
        );
        // It reads back as a skill named for its task, asking for read only.
        let dir = std::env::temp_dir().join(format!("hyalos-draft-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let digest = store(&dir, task, &drafted).unwrap();
        let read = Store::new(&dir)
            .load(&digest, &name(task).unwrap())
            .unwrap();
        assert_eq!(read.asks, Grant::read_only());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn values_that_do_not_fit_the_workflow_draft_nothing() {
        use TypedValue::{Bool, Choice};
        let task = TaskId::from_seq(12);
        let mut short = fix_issue_values();
        short.pop();
        let mut wrong = fix_issue_values();
        wrong[2] = vec![Choice(9), Bool(true)];
        let mut kind = fix_issue_values();
        kind[2] = vec![Bool(true), Bool(true)];
        for values in [short, wrong, kind] {
            assert_eq!(files(task, &flow(), &values, b"{}"), None, "{values:?}");
        }
    }
}
