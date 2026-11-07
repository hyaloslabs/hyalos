//! Running a checked workflow's steps (ADR-0062).
//!
//! The engine runs a workflow only from what [`Workflow::parse`] checked: each step's call is built
//! from the step and the values earlier steps returned, and each step's manifest is minted with the
//! inputs the check derived ([`Checked::inputs`]), never with inputs written beside it.
//!
//! - A **reader** or **planner** is one model call the broker makes ([`ReaderCall`], ADR-0061). The
//!   engine names the issue by number and passes operator text and typed values as context; it gets
//!   back typed values, checked against its own copy of the step's output.
//! - The **actor** is the last step. Its prompt is an [`ActorPrompt`] of the step's instructions, the
//!   operator's text and the typed values it reads, so no text a model or the forge wrote reaches it.
//!
//! The fix-issue workflow ships in the binary ([`FIX_ISSUE`]); `hyalos run --issue-number` runs it.

use std::collections::BTreeMap;

use hyalos_types::wire::{Protocol, ReaderCall};
use hyalos_types::workflow::{Checked, Field, InputKind, Output, Step, TypedValue, WorkflowError};
use hyalos_types::{
    ActorPrompt, Choice, Choices, OperatorText, PluginText, Provenance, Role, Sourced, StepInputs,
    Workflow,
};

/// The shipped fix-issue workflow: a reader classifies the issue, a planner chooses an approach from
/// that, and an actor makes the change and opens a pull request.
pub const FIX_ISSUE: &str = include_str!("../workflows/fix_issue.json");

/// The fix-issue workflow, checked as any workflow is at install.
pub fn fix_issue() -> Result<Checked, WorkflowError> {
    Workflow::parse(FIX_ISSUE)
}

/// Why a checked workflow cannot run here. Each is a short code.
pub mod codes {
    /// A workflow whose steps are not readers and planners followed by one actor, the only shape
    /// this runner runs: an actor's result goes back to the operator, not to a later step
    /// (ADR-0064).
    pub const NOT_RUNNABLE: &str = "workflow.not_runnable";
    /// A reader or planner that reads plugin text. Those run in the broker, which is given only
    /// operator text and typed values; only the actor takes a skill's instructions here (ADR-0066).
    pub const PLUGIN_TEXT_IN_BROKER: &str = "workflow.plugin_text_in_broker";
    /// A workflow input the run did not supply.
    pub const INPUT_MISSING: &str = "workflow.input_missing";
    /// An actor's prompt read something its checked inputs do not hold.
    pub const INPUTS_MISMATCH: &str = "workflow.inputs_mismatch";
}

/// What the operator gave the run, for the workflow's inputs: the issue every `forge_issue` input
/// names, the text every `operator_text` input is, and each `plugin_text` input's skill text with its
/// grant, by input name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Supplied {
    pub issue: Option<u64>,
    pub task: Option<String>,
    pub plugins: BTreeMap<String, PluginText>,
}

/// A checked workflow this runner can run.
#[derive(Debug, Clone)]
pub struct Runnable {
    checked: Checked,
}

impl Runnable {
    /// Whether `checked` is readers and planners followed by exactly one actor, and every input it
    /// reads is supplied.
    pub fn new(checked: Checked, supplied: &Supplied) -> Result<Self, &'static str> {
        let steps = &checked.workflow().steps;
        let (actor, typed) = steps.split_last().ok_or(codes::NOT_RUNNABLE)?;
        if actor.role != Role::Actor
            || typed
                .iter()
                .any(|s| !matches!(s.role, Role::Reader | Role::Planner))
        {
            return Err(codes::NOT_RUNNABLE);
        }
        for step in steps {
            for reference in &step.reads {
                let given = match checked.workflow().inputs.get(reference) {
                    Some(InputKind::ForgeIssue) => supplied.issue.is_some(),
                    Some(InputKind::OperatorText) => supplied.task.is_some(),
                    Some(InputKind::PluginText) if step.role != Role::Actor => {
                        return Err(codes::PLUGIN_TEXT_IN_BROKER);
                    }
                    Some(InputKind::PluginText) => supplied.plugins.contains_key(reference),
                    None => true,
                };
                if !given {
                    return Err(codes::INPUT_MISSING);
                }
            }
        }
        Ok(Self { checked })
    }

    pub fn checked(&self) -> &Checked {
        &self.checked
    }

    /// The readers and planners, in order.
    pub fn typed_steps(&self) -> &[Step] {
        let steps = &self.checked.workflow().steps;
        &steps[..steps.len() - 1]
    }

    /// The actor, last.
    pub fn actor(&self) -> &Step {
        let steps = &self.checked.workflow().steps;
        &steps[steps.len() - 1]
    }

    /// Whether `step` runs tainted, as the check derived. A step the check did not see is tainted.
    pub fn tainted(&self, step: &Step) -> bool {
        self.checked.tainted(&step.id).unwrap_or(true)
    }

    /// The call the broker makes for reader or planner `step`.
    pub fn call(
        &self,
        step: &Step,
        supplied: &Supplied,
        values: &Values,
        protocol: Protocol,
        model: &str,
    ) -> ReaderCall {
        let mut context = Vec::new();
        let mut issue = None;
        let mut known = Vec::new();
        for reference in &step.reads {
            match self.checked.workflow().inputs.get(reference) {
                Some(InputKind::ForgeIssue) => issue = supplied.issue,
                // `Runnable::new` refuses a reader or planner that reads plugin text.
                Some(InputKind::PluginText) => {}
                Some(InputKind::OperatorText) => {
                    if let Some(task) = &supplied.task {
                        context.push(format!("The operator's task:\n{task}"));
                    }
                }
                None => {
                    if let Some(value) = values.get(reference) {
                        known.push(format!("{reference}: {}", value.render()));
                    }
                }
            }
        }
        if !known.is_empty() {
            context.push(format!("Values from earlier steps:\n{}", known.join("\n")));
        }
        ReaderCall {
            protocol,
            model: model.to_string(),
            instructions: step.instructions.clone(),
            context,
            issue,
            output: step.output.clone().unwrap_or_default(),
        }
    }

    /// Takes the values the broker returned for `step`, already checked against its output, as the
    /// engine's typed values. A field of a step that ran tainted is model output from tainted input.
    pub fn accept(&self, step: &Step, returned: &[TypedValue], values: &mut Values) -> bool {
        let Some(output) = &step.output else {
            return false;
        };
        if output.len() != returned.len() {
            return false;
        }
        let made = if self.tainted(step) {
            Provenance::ModelFromTainted
        } else {
            Provenance::Operator
        };
        let mut taken = Vec::new();
        for ((field, kind), value) in output.iter().zip(returned) {
            let value = match (kind, value) {
                (Field::Bool, TypedValue::Bool(b)) if kind.admits(value) => {
                    Value::Bool(Sourced::new(*b, made))
                }
                (Field::Integer(_), TypedValue::Integer(n)) if kind.admits(value) => {
                    Value::Integer(Sourced::new(*n, made))
                }
                (Field::Choice(options), TypedValue::Choice(at)) => {
                    // The options are the workflow's, written by its author, not the model.
                    let picked = options.get(*at as usize).and_then(|name| {
                        Choices::operator(options.iter().map(OperatorText::new))?.pick(name)
                    });
                    match picked {
                        Some(choice) => Value::Choice(Sourced::new(choice, made)),
                        None => return false,
                    }
                }
                _ => return false,
            };
            taken.push((format!("{}.{field}", step.id), value));
        }
        values.0.extend(taken);
        true
    }

    /// The actor's prompt: `before`, the step's instructions, the operator's task and the typed
    /// values it reads, then `after`. `Err` if what the prompt took is not within the inputs the
    /// check derived for the step, which mint the manifest.
    pub fn actor_prompt(
        &self,
        supplied: &Supplied,
        values: &Values,
        mut prompt: ActorPrompt,
        after: ActorPrompt,
    ) -> Result<(ActorPrompt, ActorPrompt), &'static str> {
        let step = self.actor();
        if let Some(number) = supplied.issue {
            prompt
                .literal("This work is for issue #")
                .value(&Sourced::new(number, Provenance::Operator))
                .literal(" of the repository.\n\n");
        }
        prompt.operator(&OperatorText::new(step.instructions.as_str()));
        let mut fields = Vec::new();
        for reference in &step.reads {
            match self.checked.workflow().inputs.get(reference) {
                Some(InputKind::OperatorText) => {
                    if let Some(task) = &supplied.task {
                        prompt
                            .literal("\n\nThe task:\n\n")
                            .operator(&OperatorText::new(task.trim_end()));
                    }
                }
                Some(InputKind::PluginText) => {
                    let text = supplied
                        .plugins
                        .get(reference)
                        .ok_or(codes::INPUTS_MISMATCH)?;
                    prompt
                        .literal("\n\nAn installed skill's instructions:\n\n")
                        .plugin(text);
                }
                // The check refuses an actor that reads raw forge text, so this is never reached.
                Some(InputKind::ForgeIssue) => return Err(codes::INPUTS_MISMATCH),
                None => fields.push(reference),
            }
        }
        if !fields.is_empty() {
            prompt.literal(
                "\n\nEarlier steps classified the issue and chose an approach. Each value below is one \
                 of a fixed set of options:\n",
            );
            for reference in fields {
                let Some(value) = values.get(reference) else {
                    return Err(codes::INPUTS_MISMATCH);
                };
                prompt
                    .literal("- ")
                    .operator(&OperatorText::new(reference.as_str()))
                    .literal(": ");
                value.push_to(&mut prompt);
                prompt.literal("\n");
            }
        }
        let checked = self
            .checked
            .inputs(&step.id)
            .ok_or(codes::INPUTS_MISMATCH)?;
        let mut took = prompt.inputs().reads;
        took.extend(after.inputs().reads);
        if !took.is_subset(&checked.reads) {
            return Err(codes::INPUTS_MISMATCH);
        }
        Ok((prompt, after))
    }

    /// What the actor's manifest is minted with: the inputs the check derived for it, and the grant
    /// of every plugin whose text `prompt` holds, which bound the manifest (ADR-0066).
    pub fn actor_inputs(&self, prompt: &ActorPrompt) -> Option<StepInputs> {
        let checked = self.checked.inputs(&self.actor().id)?.clone();
        Some(checked.with_grants(prompt.inputs().grants))
    }
}

/// How the actor is told to return `output` with `hyalos-submit` (ADR-0064): each field and what it
/// may hold, and an example. The field names and options are the workflow author's.
pub fn submission(output: &Output) -> String {
    let mut text = String::from(
        "When you are done, report the result by running hyalos-submit with one JSON object that \
         has exactly these fields:\n",
    );
    let mut example = serde_json::Map::new();
    for (field, kind) in output {
        let (holds, value) = match kind {
            Field::Bool => ("true or false".to_string(), serde_json::Value::Bool(true)),
            Field::Integer(range) => (
                format!("a whole number from {} to {}", range.min, range.max),
                serde_json::Value::from(range.min),
            ),
            Field::Choice(options) => (
                format!(
                    "one of {}",
                    options
                        .iter()
                        .map(|o| serde_json::Value::from(o.as_str()).to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                options
                    .first()
                    .map_or(serde_json::Value::Null, |o| o.as_str().into()),
            ),
        };
        text.push_str(&format!("- {field}: {holds}\n"));
        example.insert(field.clone(), value);
    }
    text.push_str(&format!(
        "For example: hyalos-submit '{}'\n\
         Only this result is read, and only once; nothing you print is. A result that is not \
         exactly this fails the job.\n",
        serde_json::Value::Object(example)
    ));
    text
}

/// One value a step returned, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bool(Sourced<bool>),
    Integer(Sourced<i64>),
    Choice(Sourced<Choice>),
}

impl Value {
    /// The value as a prompt shows it: `true`, a number, or the option's own text.
    pub fn render(&self) -> String {
        use hyalos_types::Typed;
        match self {
            Value::Bool(v) => v.value().render(),
            Value::Integer(v) => v.value().render(),
            Value::Choice(v) => v.value().render(),
        }
    }

    pub fn provenance(&self) -> Provenance {
        match self {
            Value::Bool(v) => v.provenance(),
            Value::Integer(v) => v.provenance(),
            Value::Choice(v) => v.provenance(),
        }
    }

    fn push_to(&self, prompt: &mut ActorPrompt) {
        match self {
            Value::Bool(v) => prompt.value(v),
            Value::Integer(v) => prompt.value(v),
            Value::Choice(v) => prompt.value(v),
        };
    }
}

/// The values steps have returned, by `step.field`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values(BTreeMap<String, Value>);

impl Values {
    pub fn get(&self, reference: &str) -> Option<&Value> {
        self.0.get(reference)
    }

    /// `field=value` for each value of `step`, for the operator's terminal. The values are options
    /// and numbers the workflow defines, never text a model wrote.
    pub fn shown(&self, step: &str) -> String {
        let prefix = format!("{step}.");
        self.0
            .iter()
            .filter_map(|(k, v)| {
                k.strip_prefix(&prefix)
                    .map(|field| format!("{field}={}", v.render()))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn supplied() -> Supplied {
        Supplied {
            issue: Some(7),
            task: Some("Add a CHANGES file.".into()),
            ..Supplied::default()
        }
    }

    #[test]
    fn the_shipped_workflow_installs_and_passes_the_role_checks() {
        let checked = fix_issue().unwrap();
        let roles: Vec<Role> = checked.workflow().steps.iter().map(|s| s.role).collect();
        assert_eq!(roles, [Role::Reader, Role::Planner, Role::Actor]);
        for step in &checked.workflow().steps {
            assert_eq!(checked.tainted(&step.id), Some(true), "{}", step.id);
        }
        // Only the reader reads the issue; the actor reads the operator's task and typed values.
        assert_eq!(
            checked.inputs("triage").unwrap().reads,
            BTreeSet::from([Provenance::Operator, Provenance::ForgeAuthored])
        );
        for id in ["plan", "fix"] {
            assert_eq!(
                checked.inputs(id).unwrap().reads,
                BTreeSet::from([Provenance::Operator, Provenance::ModelFromTainted]),
                "{id}"
            );
        }
        assert!(Runnable::new(checked, &supplied()).is_ok());
    }

    #[test]
    fn the_shipped_actor_reports_a_typed_result_and_is_told_how() {
        let checked = fix_issue().unwrap();
        let actor = checked
            .workflow()
            .steps
            .iter()
            .find(|s| s.role == Role::Actor)
            .unwrap();
        let output = actor.output.as_ref().unwrap();
        assert!(hyalos_types::workflow::output_is_valid(output));
        assert_eq!(
            output.keys().map(String::as_str).collect::<Vec<_>>(),
            ["outcome", "tests_added"]
        );
        let told = submission(
            &[
                (
                    "n".to_string(),
                    Field::Integer(hyalos_types::workflow::IntegerRange { min: -1, max: 9 }),
                ),
                ("ok".to_string(), Field::Bool),
            ]
            .into(),
        );
        assert!(
            told.contains("- n: a whole number from -1 to 9\n"),
            "{told}"
        );
        assert!(told.contains("- ok: true or false\n"), "{told}");
        assert!(
            told.contains(r#"For example: hyalos-submit '{"n":-1,"ok":true}'"#),
            "{told}"
        );
    }

    #[test]
    fn a_copy_edited_so_the_issue_reaches_the_actor_does_not_install() {
        let reads =
            r#""reads": ["task", "triage.kind", "triage.size", "plan.approach", "plan.add_test"]"#;
        assert_eq!(FIX_ISSUE.matches(reads).count(), 1);
        let edited = FIX_ISSUE.replacen(reads, r#""reads": ["task", "issue"]"#, 1);
        let refused = Workflow::parse(&edited).unwrap_err();
        assert_eq!(refused.code(), "workflow.raw_text_to_actor");
        // Through the planner instead: refused too.
        let planner =
            r#""reads": ["triage.kind", "triage.size", "triage.clear", "triage.has_repro"]"#;
        let edited = FIX_ISSUE.replacen(planner, r#""reads": ["issue"]"#, 1);
        assert_eq!(
            Workflow::parse(&edited).unwrap_err().code(),
            "workflow.raw_text_to_planner"
        );
    }

    #[test]
    fn only_readers_and_planners_then_one_actor_run_with_their_inputs() {
        let shapes = [
            r#"{"version":1,"name":"w","steps":[{"id":"a","role":"actor","instructions":""},
                {"id":"b","role":"actor","instructions":""}]}"#,
            r#"{"version":1,"name":"w","steps":[
                {"id":"r","role":"reader","instructions":"","output":{"k":"bool"}}]}"#,
        ];
        for shape in shapes {
            let checked = Workflow::parse(shape).unwrap();
            assert_eq!(
                Runnable::new(checked, &supplied()).err(),
                Some(codes::NOT_RUNNABLE)
            );
        }
        let no_task = Supplied {
            issue: Some(7),
            task: None,
            ..Supplied::default()
        };
        assert_eq!(
            Runnable::new(fix_issue().unwrap(), &no_task).err(),
            Some(codes::INPUT_MISSING)
        );
    }

    #[test]
    fn each_step_gets_only_what_it_reads_and_the_actor_gets_typed_values() {
        let run = Runnable::new(fix_issue().unwrap(), &supplied()).unwrap();
        let mut values = Values::default();
        let [triage, plan] = run.typed_steps() else {
            panic!("two typed steps");
        };
        let call = run.call(triage, &supplied(), &values, Protocol::OpenAi, "m");
        assert_eq!(call.issue, Some(7));
        assert!(call.context.is_empty(), "{:?}", call.context);
        assert_eq!(call.output.len(), 4);
        // kind, size, clear, has_repro in field order: clear, has_repro, kind, size.
        assert!(!run.accept(triage, &[TypedValue::Bool(true)], &mut values));
        assert!(!run.accept(
            triage,
            &[
                TypedValue::Bool(true),
                TypedValue::Bool(false),
                TypedValue::Choice(9),
                TypedValue::Choice(0)
            ],
            &mut values
        ));
        assert!(run.accept(
            triage,
            &[
                TypedValue::Bool(true),
                TypedValue::Bool(false),
                TypedValue::Choice(2),
                TypedValue::Choice(0)
            ],
            &mut values
        ));
        assert_eq!(
            values.shown("triage"),
            "clear=true, has_repro=false, kind=docs, size=small"
        );
        assert_eq!(
            values.get("triage.kind").unwrap().provenance(),
            Provenance::ModelFromTainted
        );

        let call = run.call(plan, &supplied(), &values, Protocol::OpenAi, "m");
        assert_eq!(call.issue, None);
        assert_eq!(
            call.context,
            [
                "Values from earlier steps:\ntriage.kind: docs\ntriage.size: small\n\
              triage.clear: true\ntriage.has_repro: false"
            ]
        );
        assert!(run.accept(
            plan,
            &[TypedValue::Bool(false), TypedValue::Choice(2)],
            &mut values
        ));

        let mut before = ActorPrompt::new();
        before.literal("Repository work.\n\n");
        let mut after = ActorPrompt::new();
        after.literal("\n\nSteps.");
        let (prompt, after) = run
            .actor_prompt(&supplied(), &values, before, after)
            .unwrap();
        let inputs = {
            let mut reads = prompt.inputs().reads;
            reads.extend(after.inputs().reads);
            reads
        };
        assert_eq!(
            inputs,
            BTreeSet::from([Provenance::Operator, Provenance::ModelFromTainted])
        );
        let text = prompt.into_text();
        assert!(
            text.starts_with("Repository work.\n\nThis work is for issue #7"),
            "{text}"
        );
        assert!(text.contains("The task:\n\nAdd a CHANGES file."), "{text}");
        assert!(text.contains("- triage.kind: docs\n"), "{text}");
        assert!(text.contains("- plan.approach: docs_only\n"), "{text}");
        assert!(text.contains("- plan.add_test: false\n"), "{text}");
        assert!(
            !text.contains("triage.clear"),
            "the actor reads only its own reads"
        );
    }

    #[test]
    fn an_actor_prompt_missing_a_value_is_refused() {
        let run = Runnable::new(fix_issue().unwrap(), &supplied()).unwrap();
        assert_eq!(
            run.actor_prompt(
                &supplied(),
                &Values::default(),
                ActorPrompt::new(),
                ActorPrompt::new()
            )
            .err(),
            Some(codes::INPUTS_MISMATCH)
        );
    }

    #[test]
    fn a_skill_reaches_the_actor_and_its_grant_bounds_the_manifest() {
        let checked = Workflow::parse(
            r#"{"version":1,"name":"w","inputs":{"skill":"plugin_text","task":"operator_text"},
                "steps":[{"id":"a","role":"actor","reads":["task","skill"],"instructions":"Work."}]}"#,
        )
        .unwrap();
        let grant = hyalos_types::Grant {
            actions: ["read", "push", "publish"].map(String::from).into(),
            ..Default::default()
        };
        let mut given = Supplied {
            task: Some("Add a CHANGES file.".into()),
            ..Supplied::default()
        };
        // Not supplied, it is missing; supplied, it runs.
        assert_eq!(
            Runnable::new(checked.clone(), &given).err(),
            Some(codes::INPUT_MISSING)
        );
        given.plugins.insert(
            "skill".into(),
            PluginText::granted("Run the linter before you push.", grant.clone()),
        );
        let run = Runnable::new(checked, &given).unwrap();
        let (prompt, _) = run
            .actor_prompt(
                &given,
                &Values::default(),
                ActorPrompt::new(),
                ActorPrompt::new(),
            )
            .unwrap();
        let inputs = run.actor_inputs(&prompt).unwrap();
        assert!(inputs.reads.contains(&Provenance::PluginAuthored));
        assert_eq!(inputs.grants, vec![grant]);
        assert!(
            prompt
                .into_text()
                .contains("An installed skill's instructions:\n\nRun the linter before you push.")
        );
        // The grant cuts what the actor may do to what both it and the job have.
        let manifest = hyalos_types::Manifest {
            version: hyalos_types::MANIFEST_VERSION,
            job: hyalos_types::JobId::from_seq(3),
            role: Role::Actor,
            tainted: true,
            repo: "octo/repo".into(),
            branch_prefix: "hyalos/job-3".into(),
            pr_base: Some("main".into()),
            actions: ["read", "push", "open_pr"].map(String::from).into(),
            egress_hosts: BTreeSet::new(),
            tools: BTreeSet::new(),
            data_flows: ["job->model"].map(String::from).into(),
            budget: hyalos_types::Budget { tokens: 10 },
            deadline: hyalos_types::Timestamp::from_millis(9),
            approval: None,
            handles: BTreeSet::new(),
            result: None,
        };
        let bounded = inputs.bound(&manifest);
        assert_eq!(bounded.actions, ["push", "read"].map(String::from).into());
        assert!(bounded.data_flows.is_empty());
    }

    #[test]
    fn a_reader_or_planner_that_reads_a_skill_does_not_run_here() {
        let checked = Workflow::parse(
            r#"{"version":1,"name":"w","inputs":{"skill":"plugin_text"},"steps":[
                {"id":"p","role":"planner","reads":["skill"],"instructions":"","output":{"k":"bool"}},
                {"id":"a","role":"actor","reads":["p.k"],"instructions":""}]}"#,
        )
        .unwrap();
        let mut given = Supplied::default();
        given.plugins.insert(
            "skill".into(),
            PluginText::granted("x", hyalos_types::Grant::none()),
        );
        assert_eq!(
            Runnable::new(checked, &given).err(),
            Some(codes::PLUGIN_TEXT_IN_BROKER)
        );
    }
}
