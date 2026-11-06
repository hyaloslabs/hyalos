//! Workflows: steps with roles, typed reader schemas, and the check at install (ADR-0005, ADR-0060).
//!
//! A workflow is a JSON document the operator writes. It names its inputs and their kinds, and a list
//! of steps. Each step has a role, the values it reads, its instructions and, for a reader or a
//! planner, the typed fields it returns. [`Workflow::parse`] reads one and checks it, and a workflow
//! that fails the check does not install:
//! - Every reference resolves, to an input or a field of an earlier step.
//! - A field is a `bool`, a bounded integer or a `choice` among options the author wrote: the typed
//!   kinds of ADR-0059. There is no string field, so a reader's output cannot carry free text.
//! - **An actor or planner step that names raw tainted text as input is refused**
//!   (`workflow.raw_text_to_actor`, `workflow.raw_text_to_planner`). Only a reader reads it.
//!
//! The check also derives each step's inputs ([`Checked::inputs`]): what a step reads, by
//! provenance, with a field of a step that ran tainted being model output from tainted input. Those
//! are the `StepInputs` its manifest is minted with.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Role;
use crate::provenance::{Provenance, StepInputs};

/// The only workflow version this build reads.
pub const WORKFLOW_VERSION: u32 = 1;
/// The largest workflow document, in bytes.
pub const MAX_WORKFLOW_DOCUMENT: usize = 64 * 1024;
pub const MAX_STEPS: usize = 32;
pub const MAX_INPUTS: usize = 16;
pub const MAX_READS: usize = 32;
pub const MAX_FIELDS: usize = 16;
pub const MAX_OPTIONS: usize = 64;
/// The longest option text, in bytes.
pub const MAX_OPTION: usize = 128;
/// The longest instructions of one step, in bytes.
pub const MAX_INSTRUCTIONS: usize = 16 * 1024;
/// The longest name of a workflow, input, step or field, in bytes.
pub const MAX_NAME: usize = 32;

/// What a workflow input is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// An issue read from the forge: raw text, forge-authored.
    ForgeIssue,
    /// Text the operator gives when the workflow runs: raw text, the operator's.
    OperatorText,
}

impl InputKind {
    pub const fn provenance(self) -> Provenance {
        match self {
            InputKind::ForgeIssue => Provenance::ForgeAuthored,
            InputKind::OperatorText => Provenance::Operator,
        }
    }

    /// Raw text that taints whoever reads it, which only a reader may.
    pub const fn is_tainted_text(self) -> bool {
        self.provenance().taints()
    }
}

/// A field of a reader's or planner's output: one of the typed kinds of ADR-0059.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Bool,
    Integer(IntegerRange),
    /// One of these options, each written by the workflow's author.
    Choice(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegerRange {
    pub min: i64,
    pub max: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub role: Role,
    /// Inputs (`issue`) and fields of earlier steps (`triage.kind`).
    #[serde(default)]
    pub reads: Vec<String>,
    /// The author's instructions: operator text.
    pub instructions: String,
    /// The typed fields the step returns. Required for a reader and a planner.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "unique::optional"
    )]
    pub output: Option<BTreeMap<String, Field>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub version: u32,
    pub name: String,
    #[serde(default, deserialize_with = "unique::map")]
    pub inputs: BTreeMap<String, InputKind>,
    pub steps: Vec<Step>,
}

/// The document's maps, refusing a key named twice: what a reviewer reads first is what is checked.
/// Derived structs already refuse a repeated field.
mod unique {
    use std::collections::BTreeMap;
    use std::fmt;
    use std::marker::PhantomData;

    use serde::de::{Deserialize, Deserializer, Error, MapAccess, Visitor};

    struct Unique<V>(BTreeMap<String, V>);

    impl<'de, V: Deserialize<'de>> Deserialize<'de> for Unique<V> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct Keys<V>(PhantomData<V>);
            impl<'de, V: Deserialize<'de>> Visitor<'de> for Keys<V> {
                type Value = Unique<V>;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("an object")
                }

                fn visit_map<A: MapAccess<'de>>(
                    self,
                    mut access: A,
                ) -> Result<Unique<V>, A::Error> {
                    let mut map = BTreeMap::new();
                    while let Some((key, value)) = access.next_entry::<String, V>()? {
                        if map.contains_key(&key) {
                            let shown: String = key.chars().take(64).collect();
                            return Err(A::Error::custom(format!("{shown:?} is named twice")));
                        }
                        map.insert(key, value);
                    }
                    Ok(Unique(map))
                }
            }
            d.deserialize_map(Keys(PhantomData))
        }
    }

    pub fn map<'de, D: Deserializer<'de>, V: Deserialize<'de>>(
        d: D,
    ) -> Result<BTreeMap<String, V>, D::Error> {
        Unique::deserialize(d).map(|u| u.0)
    }

    pub fn optional<'de, D: Deserializer<'de>, V: Deserialize<'de>>(
        d: D,
    ) -> Result<Option<BTreeMap<String, V>>, D::Error> {
        Option::<Unique<V>>::deserialize(d).map(|o| o.map(|u| u.0))
    }
}

/// Why a workflow does not install. Each has a short code ([`WorkflowError::code`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowError {
    TooLarge,
    /// Not JSON of this shape; the parser's message.
    Malformed(String),
    UnknownVersion(u32),
    /// A name that is not 1 to 32 of `a-z`, `0-9` and `_`, starting with a letter.
    BadName(String),
    NoSteps,
    TooMany(&'static str),
    DuplicateName(String),
    /// Deterministic steps have nothing to run yet.
    RoleNotSupported {
        step: String,
    },
    UnknownReference {
        step: String,
        reference: String,
    },
    ForwardReference {
        step: String,
        reference: String,
    },
    OutputRequired {
        step: String,
    },
    BadField {
        step: String,
        field: String,
    },
    RawTextToActor {
        step: String,
        input: String,
    },
    RawTextToPlanner {
        step: String,
        input: String,
    },
    InstructionsTooLong {
        step: String,
    },
}

impl WorkflowError {
    pub fn code(&self) -> &'static str {
        match self {
            WorkflowError::TooLarge => "workflow.too_large",
            WorkflowError::Malformed(_) => "workflow.malformed",
            WorkflowError::UnknownVersion(_) => "workflow.unknown_version",
            WorkflowError::BadName(_) => "workflow.bad_name",
            WorkflowError::NoSteps => "workflow.no_steps",
            WorkflowError::TooMany(_) => "workflow.too_many",
            WorkflowError::DuplicateName(_) => "workflow.duplicate_name",
            WorkflowError::RoleNotSupported { .. } => "workflow.role_not_supported",
            WorkflowError::UnknownReference { .. } => "workflow.unknown_reference",
            WorkflowError::ForwardReference { .. } => "workflow.forward_reference",
            WorkflowError::OutputRequired { .. } => "workflow.output_required",
            WorkflowError::BadField { .. } => "workflow.bad_field",
            WorkflowError::RawTextToActor { .. } => "workflow.raw_text_to_actor",
            WorkflowError::RawTextToPlanner { .. } => "workflow.raw_text_to_planner",
            WorkflowError::InstructionsTooLong { .. } => "workflow.instructions_too_long",
        }
    }
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            WorkflowError::TooLarge => write!(f, "{code}: over {MAX_WORKFLOW_DOCUMENT} bytes"),
            WorkflowError::Malformed(why) => write!(f, "{code}: {why}"),
            WorkflowError::UnknownVersion(v) => {
                write!(
                    f,
                    "{code}: version {v}; this build reads {WORKFLOW_VERSION}"
                )
            }
            WorkflowError::BadName(name) => write!(f, "{code}: {name:?}"),
            WorkflowError::NoSteps => write!(f, "{code}: a workflow has at least one step"),
            WorkflowError::TooMany(what) => write!(f, "{code}: too many {what}"),
            WorkflowError::DuplicateName(name) => write!(f, "{code}: {name} is named twice"),
            WorkflowError::RoleNotSupported { step } => {
                write!(f, "{code}: step {step}: deterministic steps are not built")
            }
            WorkflowError::UnknownReference { step, reference } => {
                write!(
                    f,
                    "{code}: step {step} reads {reference}, which nothing defines"
                )
            }
            WorkflowError::ForwardReference { step, reference } => write!(
                f,
                "{code}: step {step} reads {reference}, which this step or a later one defines"
            ),
            WorkflowError::OutputRequired { step } => {
                write!(
                    f,
                    "{code}: step {step} is a reader or planner with no output"
                )
            }
            WorkflowError::BadField { step, field } => write!(
                f,
                "{code}: step {step} field {field}: an integer needs min <= max, a choice 1 to \
                 {MAX_OPTIONS} distinct non-empty options of at most {MAX_OPTION} bytes"
            ),
            WorkflowError::RawTextToActor { step, input } => write!(
                f,
                "{code}: actor step {step} reads {input}, raw forge text; only a reader may"
            ),
            WorkflowError::RawTextToPlanner { step, input } => write!(
                f,
                "{code}: planner step {step} reads {input}, raw forge text; only a reader may"
            ),
            WorkflowError::InstructionsTooLong { step } => {
                write!(f, "{code}: step {step}: over {MAX_INSTRUCTIONS} bytes")
            }
        }
    }
}

impl std::error::Error for WorkflowError {}

/// A workflow that passed the check, and what each step reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    workflow: Workflow,
    inputs: BTreeMap<String, StepInputs>,
}

impl Checked {
    pub fn workflow(&self) -> &Workflow {
        &self.workflow
    }

    /// What step `id` reads, by provenance: the inputs its manifest is minted with.
    pub fn inputs(&self, id: &str) -> Option<&StepInputs> {
        self.inputs.get(id)
    }

    /// Whether step `id` runs tainted (ADR-0047).
    pub fn tainted(&self, id: &str) -> Option<bool> {
        let step = self.workflow.steps.iter().find(|s| s.id == id)?;
        Some(self.inputs.get(id)?.tainted(step.role))
    }
}

impl Workflow {
    /// Reads and checks a workflow document. Nothing that fails installs.
    pub fn parse(document: &str) -> Result<Checked, WorkflowError> {
        if document.len() > MAX_WORKFLOW_DOCUMENT {
            return Err(WorkflowError::TooLarge);
        }
        let workflow: Workflow =
            serde_json::from_str(document).map_err(|e| WorkflowError::Malformed(e.to_string()))?;
        workflow.check()
    }

    /// Checks a workflow (see the module documentation).
    pub fn check(self) -> Result<Checked, WorkflowError> {
        if self.version != WORKFLOW_VERSION {
            return Err(WorkflowError::UnknownVersion(self.version));
        }
        name(&self.name)?;
        if self.steps.is_empty() {
            return Err(WorkflowError::NoSteps);
        }
        if self.steps.len() > MAX_STEPS {
            return Err(WorkflowError::TooMany("steps"));
        }
        if self.inputs.len() > MAX_INPUTS {
            return Err(WorkflowError::TooMany("inputs"));
        }
        for input in self.inputs.keys() {
            name(input)?;
        }
        let mut ids = BTreeSet::new();
        for step in &self.steps {
            name(&step.id)?;
            if self.inputs.contains_key(&step.id) || !ids.insert(step.id.as_str()) {
                return Err(WorkflowError::DuplicateName(step.id.clone()));
            }
        }

        // What each earlier step's fields carry: model output, tainted if the step ran tainted.
        let mut fields: BTreeMap<(&str, &str), Provenance> = BTreeMap::new();
        let mut inputs = BTreeMap::new();
        for (at, step) in self.steps.iter().enumerate() {
            let here = || step.id.clone();
            if step.role == Role::Deterministic {
                return Err(WorkflowError::RoleNotSupported { step: here() });
            }
            if step.instructions.len() > MAX_INSTRUCTIONS {
                return Err(WorkflowError::InstructionsTooLong { step: here() });
            }
            if step.reads.len() > MAX_READS {
                return Err(WorkflowError::TooMany("reads"));
            }
            let mut reads = BTreeSet::from([Provenance::Operator]);
            for reference in &step.reads {
                let found = match reference.split_once('.') {
                    None => self.inputs.get(reference).map(|kind| {
                        if kind.is_tainted_text() {
                            match step.role {
                                Role::Actor => Err(WorkflowError::RawTextToActor {
                                    step: here(),
                                    input: reference.clone(),
                                }),
                                Role::Planner => Err(WorkflowError::RawTextToPlanner {
                                    step: here(),
                                    input: reference.clone(),
                                }),
                                Role::Reader | Role::Deterministic => Ok(kind.provenance()),
                            }
                        } else {
                            Ok(kind.provenance())
                        }
                    }),
                    Some((from, field)) => fields.get(&(from, field)).copied().map(Ok),
                };
                match found {
                    Some(provenance) => {
                        reads.insert(provenance?);
                    }
                    None => {
                        let later = reference
                            .split_once('.')
                            .is_some_and(|(from, _)| self.steps[at..].iter().any(|s| s.id == from));
                        let (step, reference) = (here(), reference.clone());
                        return Err(if later {
                            WorkflowError::ForwardReference { step, reference }
                        } else {
                            WorkflowError::UnknownReference { step, reference }
                        });
                    }
                }
            }
            let step_inputs = StepInputs::of(reads);
            match (&step.output, step.role) {
                (None, Role::Reader | Role::Planner) => {
                    return Err(WorkflowError::OutputRequired { step: here() });
                }
                (None, _) => {}
                (Some(output), _) => {
                    if output.is_empty() && matches!(step.role, Role::Reader | Role::Planner) {
                        return Err(WorkflowError::OutputRequired { step: here() });
                    }
                    if output.len() > MAX_FIELDS {
                        return Err(WorkflowError::TooMany("fields"));
                    }
                    let made = if step_inputs.tainted(step.role) {
                        Provenance::ModelFromTainted
                    } else {
                        Provenance::Operator
                    };
                    for (field, kind) in output {
                        name(field)?;
                        if !well_formed(kind) {
                            return Err(WorkflowError::BadField {
                                step: here(),
                                field: field.clone(),
                            });
                        }
                        fields.insert((step.id.as_str(), field.as_str()), made);
                    }
                }
            }
            inputs.insert(step.id.clone(), step_inputs);
        }
        Ok(Checked {
            workflow: self,
            inputs,
        })
    }
}

/// A name: 1 to 32 of `a-z`, `0-9` and `_`, starting with a letter.
fn name(text: &str) -> Result<(), WorkflowError> {
    let ok = text.len() <= MAX_NAME
        && text.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(WorkflowError::BadName(text.chars().take(64).collect()))
    }
}

fn well_formed(field: &Field) -> bool {
    match field {
        Field::Bool => true,
        Field::Integer(range) => range.min <= range.max,
        Field::Choice(options) => {
            let distinct: BTreeSet<&String> = options.iter().collect();
            !options.is_empty()
                && options.len() <= MAX_OPTIONS
                && distinct.len() == options.len()
                && options
                    .iter()
                    .all(|o| !o.is_empty() && o.len() <= MAX_OPTION)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of the fix-issue workflow (S4.6): a reader classifies the issue, a planner picks an
    /// approach from that, and the actor works from the operator's task and the typed values.
    const FIX_ISSUE: &str = r#"{
        "version": 1,
        "name": "fix_issue",
        "inputs": { "issue": "forge_issue", "task": "operator_text" },
        "steps": [
            {
                "id": "triage",
                "role": "reader",
                "reads": ["issue"],
                "instructions": "Classify the issue.",
                "output": {
                    "kind": { "choice": ["bug", "feature", "docs"] },
                    "severity": { "integer": { "min": 1, "max": 5 } },
                    "has_repro": "bool"
                }
            },
            {
                "id": "plan",
                "role": "planner",
                "reads": ["triage.kind", "triage.severity"],
                "instructions": "Pick an approach.",
                "output": { "approach": { "choice": ["small_fix", "refactor"] } }
            },
            {
                "id": "fix",
                "role": "actor",
                "reads": ["task", "triage.kind", "plan.approach"],
                "instructions": "Make the change and open a pull request."
            }
        ]
    }"#;

    fn edited(from: &str, to: &str) -> Result<Checked, WorkflowError> {
        assert_eq!(FIX_ISSUE.matches(from).count(), 1, "{from}");
        Workflow::parse(&FIX_ISSUE.replacen(from, to, 1))
    }

    #[test]
    fn the_fix_issue_shape_installs_and_taint_follows_the_issue() {
        let checked = Workflow::parse(FIX_ISSUE).unwrap();
        let reads = |id| checked.inputs(id).unwrap().reads.clone();
        assert_eq!(
            reads("triage"),
            BTreeSet::from([Provenance::Operator, Provenance::ForgeAuthored])
        );
        // The reader read the issue, so what it returns is model output from tainted input, and so
        // is what the planner made from that.
        assert_eq!(
            reads("plan"),
            BTreeSet::from([Provenance::Operator, Provenance::ModelFromTainted])
        );
        assert_eq!(
            reads("fix"),
            BTreeSet::from([Provenance::Operator, Provenance::ModelFromTainted])
        );
        for id in ["triage", "plan", "fix"] {
            assert_eq!(checked.tainted(id), Some(true), "{id}");
        }
        assert_eq!(checked.tainted("nope"), None);
    }

    #[test]
    fn an_actor_step_that_names_raw_text_does_not_install() {
        let refused = edited(
            r#""reads": ["task", "triage.kind", "plan.approach"]"#,
            r#""reads": ["task", "issue"]"#,
        );
        assert_eq!(
            refused,
            Err(WorkflowError::RawTextToActor {
                step: "fix".into(),
                input: "issue".into()
            })
        );
        assert_eq!(refused.unwrap_err().code(), "workflow.raw_text_to_actor");
    }

    #[test]
    fn a_planner_that_names_raw_text_does_not_install() {
        let refused = edited(
            r#""reads": ["triage.kind", "triage.severity"]"#,
            r#""reads": ["issue"]"#,
        );
        assert_eq!(
            refused,
            Err(WorkflowError::RawTextToPlanner {
                step: "plan".into(),
                input: "issue".into()
            })
        );
    }

    #[test]
    fn operator_text_and_typed_values_reach_an_actor_untainted_until_the_issue() {
        let checked = Workflow::parse(
            r#"{"version":1,"name":"w","inputs":{"task":"operator_text"},"steps":[
                {"id":"r","role":"reader","reads":["task"],"instructions":"","output":{"k":"bool"}},
                {"id":"p","role":"planner","reads":["r.k"],"instructions":"","output":{"k":"bool"}},
                {"id":"a","role":"actor","reads":["task","p.k"],"instructions":""}]}"#,
        )
        .unwrap();
        assert_eq!(checked.tainted("r"), Some(false));
        assert_eq!(checked.tainted("p"), Some(false));
        assert_eq!(
            checked.inputs("a").unwrap().reads,
            BTreeSet::from([Provenance::Operator])
        );
        assert_eq!(
            checked.tainted("a"),
            Some(true),
            "an actor reads its repository"
        );
    }

    #[test]
    fn a_reference_must_name_an_input_or_an_earlier_field() {
        let unknown = edited(r#""triage.kind", "triage.severity""#, r#""triage.colour""#);
        assert_eq!(unknown.unwrap_err().code(), "workflow.unknown_reference");
        let later = edited(r#""triage.kind", "triage.severity""#, r#""plan.approach""#);
        assert_eq!(later.unwrap_err().code(), "workflow.forward_reference");
        let whole_step = edited(r#""triage.kind", "triage.severity""#, r#""triage""#);
        assert_eq!(whole_step.unwrap_err().code(), "workflow.unknown_reference");
    }

    #[test]
    fn a_field_is_a_typed_kind_and_free_text_is_not_one() {
        let string = edited(r#""has_repro": "bool""#, r#""summary": "string""#);
        assert_eq!(string.unwrap_err().code(), "workflow.malformed");
        let text = edited(r#""has_repro": "bool""#, r#""summary": {"text": 100}"#);
        assert_eq!(text.unwrap_err().code(), "workflow.malformed");
        let range = edited(r#""min": 1, "max": 5"#, r#""min": 5, "max": 1"#);
        assert_eq!(range.unwrap_err().code(), "workflow.bad_field");
        for options in [
            "[]".to_string(),
            r#"["bug", "bug"]"#.to_string(),
            r#"["bug", ""]"#.to_string(),
            format!(r#"["{}"]"#, "a".repeat(MAX_OPTION + 1)),
        ] {
            let bad = edited(r#"["small_fix", "refactor"]"#, &options);
            assert_eq!(bad.unwrap_err().code(), "workflow.bad_field", "{options}");
        }
    }

    #[test]
    fn readers_and_planners_return_typed_fields() {
        let none = edited(
            r#""output": { "approach": { "choice": ["small_fix", "refactor"] } }"#,
            r#""output": null"#,
        );
        assert_eq!(none.unwrap_err().code(), "workflow.output_required");
        let empty = edited(
            r#""output": { "approach": { "choice": ["small_fix", "refactor"] } }"#,
            r#""output": {}"#,
        );
        assert_eq!(empty.unwrap_err().code(), "workflow.output_required");
    }

    #[test]
    fn the_document_is_closed_and_capped() {
        let extra = edited(
            r#""name": "fix_issue","#,
            r#""name": "fix_issue", "tools": [],"#,
        );
        assert_eq!(extra.unwrap_err().code(), "workflow.malformed");
        let step_extra = edited(
            r#""role": "actor","#,
            r#""role": "actor", "tools": ["bash"],"#,
        );
        assert_eq!(step_extra.unwrap_err().code(), "workflow.malformed");
        let version = edited(r#""version": 1"#, r#""version": 2"#);
        assert_eq!(version.unwrap_err().code(), "workflow.unknown_version");
        let kind = edited(r#""forge_issue""#, r#""forge_comment""#);
        assert_eq!(kind.unwrap_err().code(), "workflow.malformed");
        let role = edited(r#""role": "planner""#, r#""role": "deterministic""#);
        assert_eq!(role.unwrap_err().code(), "workflow.role_not_supported");
        let big = format!("{FIX_ISSUE}{}", " ".repeat(MAX_WORKFLOW_DOCUMENT));
        assert_eq!(Workflow::parse(&big), Err(WorkflowError::TooLarge));
        let long = edited(
            r#""Pick an approach.""#,
            &format!(r#""{}""#, "a".repeat(MAX_INSTRUCTIONS + 1)),
        );
        assert_eq!(long.unwrap_err().code(), "workflow.instructions_too_long");
    }

    #[test]
    fn a_key_named_twice_is_refused_in_every_object() {
        // A map: what a reviewer reads first would not be what was checked.
        let input = edited(
            r#""issue": "forge_issue""#,
            r#""issue": "forge_issue", "issue": "operator_text""#,
        );
        assert_eq!(input.unwrap_err().code(), "workflow.malformed");
        let field = edited(
            r#""has_repro": "bool""#,
            r#""has_repro": "bool", "has_repro": "bool""#,
        );
        assert_eq!(field.unwrap_err().code(), "workflow.malformed");
        // A step, and the document: derived structs already refuse a repeated field.
        let step = edited(
            r#""role": "actor","#,
            r#""role": "actor", "role": "reader","#,
        );
        assert_eq!(step.unwrap_err().code(), "workflow.malformed");
        let document = edited(r#""version": 1,"#, r#""version": 1, "version": 1,"#);
        assert_eq!(document.unwrap_err().code(), "workflow.malformed");
    }

    #[test]
    fn names_are_short_codes_and_unique() {
        for bad in ["Fix", "1st", "", "a-b", "a.b", &"a".repeat(MAX_NAME + 1)] {
            let refused = edited(r#""id": "plan""#, &format!(r#""id": "{bad}""#));
            assert_eq!(refused.unwrap_err().code(), "workflow.bad_name", "{bad:?}");
        }
        let twice = edited(r#""id": "plan""#, r#""id": "triage""#);
        assert_eq!(twice.unwrap_err().code(), "workflow.duplicate_name");
        let like_input = edited(r#""id": "plan""#, r#""id": "issue""#);
        assert_eq!(like_input.unwrap_err().code(), "workflow.duplicate_name");
        let none = Workflow::parse(r#"{"version":1,"name":"w","steps":[]}"#);
        assert_eq!(none, Err(WorkflowError::NoSteps));
    }

    #[test]
    fn every_code_is_a_reason_code() {
        let all = [
            WorkflowError::TooLarge,
            WorkflowError::Malformed(String::new()),
            WorkflowError::UnknownVersion(2),
            WorkflowError::BadName(String::new()),
            WorkflowError::NoSteps,
            WorkflowError::TooMany("steps"),
            WorkflowError::DuplicateName(String::new()),
            WorkflowError::RoleNotSupported { step: "s".into() },
            WorkflowError::UnknownReference {
                step: "s".into(),
                reference: "r".into(),
            },
            WorkflowError::ForwardReference {
                step: "s".into(),
                reference: "r".into(),
            },
            WorkflowError::OutputRequired { step: "s".into() },
            WorkflowError::BadField {
                step: "s".into(),
                field: "f".into(),
            },
            WorkflowError::RawTextToActor {
                step: "s".into(),
                input: "i".into(),
            },
            WorkflowError::RawTextToPlanner {
                step: "s".into(),
                input: "i".into(),
            },
            WorkflowError::InstructionsTooLong { step: "s".into() },
        ];
        for e in all {
            assert!(crate::ReasonCode::new(e.code()).is_some(), "{}", e.code());
            assert!(e.to_string().starts_with(e.code()), "{e}");
        }
    }
}
