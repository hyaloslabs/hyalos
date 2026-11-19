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

use crate::provenance::{Provenance, StepInputs};
use crate::{PluginName, Role};

/// The newest workflow version this build reads. Version 2 adds a step's `plugin`, and so
/// deterministic steps (ADR-0102); version 1 documents read as before.
pub const WORKFLOW_VERSION: u32 = 2;
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
    /// An installed skill's or plugin's instructions: raw text, plugin-authored. An actor may read it
    /// untainted; the plugin's grant then bounds the job (ADR-0006, ADR-0066).
    PluginText,
}

impl InputKind {
    pub const fn provenance(self) -> Provenance {
        match self {
            InputKind::ForgeIssue => Provenance::ForgeAuthored,
            InputKind::OperatorText => Provenance::Operator,
            InputKind::PluginText => Provenance::PluginAuthored,
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
    /// An installed plugin the step uses (version 2, ADR-0102): the WASM component a deterministic
    /// step runs, which it must name, or a skill an actor is given. A reader or planner names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginName>,
    /// The typed fields the step returns. Required for a reader, a planner and a deterministic step.
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

    /// An object whose keys are each named once; also how a model's answer is read.
    pub(super) struct Unique<V>(pub(super) BTreeMap<String, V>);

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
    /// A deterministic step in a version 1 document, which has no way to say what it runs.
    RoleNotSupported {
        step: String,
    },
    /// A deterministic step that names no plugin to run.
    PluginRequired {
        step: String,
    },
    /// A plugin named on a reader or planner, or in a version 1 document.
    BadPlugin {
        step: String,
    },
    RawTextToDeterministic {
        step: String,
        input: String,
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
            WorkflowError::PluginRequired { .. } => "workflow.plugin_required",
            WorkflowError::BadPlugin { .. } => "workflow.bad_plugin",
            WorkflowError::RawTextToDeterministic { .. } => "workflow.raw_text_to_deterministic",
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
                    "{code}: version {v}; this build reads 1 to {WORKFLOW_VERSION}"
                )
            }
            WorkflowError::BadName(name) => write!(f, "{code}: {name:?}"),
            WorkflowError::NoSteps => write!(f, "{code}: a workflow has at least one step"),
            WorkflowError::TooMany(what) => write!(f, "{code}: too many {what}"),
            WorkflowError::DuplicateName(name) => write!(f, "{code}: {name} is named twice"),
            WorkflowError::RoleNotSupported { step } => {
                write!(
                    f,
                    "{code}: step {step}: a deterministic step needs version 2"
                )
            }
            WorkflowError::PluginRequired { step } => write!(
                f,
                "{code}: deterministic step {step} names no plugin to run"
            ),
            WorkflowError::BadPlugin { step } => write!(
                f,
                "{code}: step {step}: only a deterministic step or an actor names a plugin, in version 2"
            ),
            WorkflowError::RawTextToDeterministic { step, input } => write!(
                f,
                "{code}: deterministic step {step} reads {input}, raw forge text; only a reader may"
            ),
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
                    "{code}: step {step} is a reader, planner or deterministic step with no output"
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
    plugins: BTreeMap<String, BTreeSet<PluginName>>,
}

impl Checked {
    pub fn workflow(&self) -> &Workflow {
        &self.workflow
    }

    /// What step `id` reads, by provenance: the inputs its manifest is minted with.
    pub fn inputs(&self, id: &str) -> Option<&StepInputs> {
        self.inputs.get(id)
    }

    /// The plugins whose grants bound step `id` (ADR-0102): its own, and every plugin whose step's
    /// fields reached it, however many steps between.
    pub fn plugins(&self, id: &str) -> Option<&BTreeSet<PluginName>> {
        self.plugins.get(id)
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
        if !(1..=WORKFLOW_VERSION).contains(&self.version) {
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

        // What each earlier step's fields carry: model or plugin output, tainted if the step ran
        // tainted, and the plugins that made it.
        type Made = (Provenance, BTreeSet<PluginName>);
        let mut fields: BTreeMap<(&str, &str), Made> = BTreeMap::new();
        let (mut inputs, mut plugins) = (BTreeMap::new(), BTreeMap::new());
        for (at, step) in self.steps.iter().enumerate() {
            let here = || step.id.clone();
            match (self.version, step.role, &step.plugin) {
                (1, Role::Deterministic, _) => {
                    return Err(WorkflowError::RoleNotSupported { step: here() });
                }
                (1, _, Some(_)) | (_, Role::Reader | Role::Planner, Some(_)) => {
                    return Err(WorkflowError::BadPlugin { step: here() });
                }
                (_, Role::Deterministic, None) => {
                    return Err(WorkflowError::PluginRequired { step: here() });
                }
                _ => {}
            }
            let mut used: BTreeSet<PluginName> = step.plugin.iter().cloned().collect();
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
                                // A plugin runs in the broker, given typed values only (ADR-0102).
                                Role::Deterministic => Err(WorkflowError::RawTextToDeterministic {
                                    step: here(),
                                    input: reference.clone(),
                                }),
                                Role::Reader => Ok(kind.provenance()),
                            }
                        } else {
                            Ok(kind.provenance())
                        }
                    }),
                    Some((from, field)) => fields.get(&(from, field)).map(|(made, by)| {
                        used.extend(by.iter().cloned());
                        Ok(*made)
                    }),
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
            // A plugin's instructions or output steer the step, so its grant bounds it (ADR-0066).
            if !used.is_empty() {
                reads.insert(Provenance::PluginAuthored);
            }
            let step_inputs = StepInputs::of(reads);
            let typed = matches!(
                step.role,
                Role::Reader | Role::Planner | Role::Deterministic
            );
            match &step.output {
                None if typed => {
                    return Err(WorkflowError::OutputRequired { step: here() });
                }
                None => {}
                Some(output) => {
                    if output.is_empty() && typed {
                        return Err(WorkflowError::OutputRequired { step: here() });
                    }
                    if output.len() > MAX_FIELDS {
                        return Err(WorkflowError::TooMany("fields"));
                    }
                    // What a plugin steered is the plugin's, whether it steered through a plugin's
                    // output or its text given as an input (#339).
                    let made = if step_inputs.tainted(step.role) {
                        Provenance::ModelFromTainted
                    } else if !used.is_empty()
                        || step_inputs.reads.contains(&Provenance::PluginAuthored)
                    {
                        Provenance::PluginAuthored
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
                        fields.insert((step.id.as_str(), field.as_str()), (made, used.clone()));
                    }
                }
            }
            inputs.insert(step.id.clone(), step_inputs);
            plugins.insert(step.id.clone(), used);
        }
        Ok(Checked {
            workflow: self,
            inputs,
            plugins,
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

/// What a reader or planner returns: named typed fields, in name order.
pub type Output = BTreeMap<String, Field>;

/// Whether `output` is one a step may be asked for: 1 to [`MAX_FIELDS`] well-named, well-formed
/// fields. The broker checks it again on a reader call, since it comes over the channel.
pub fn output_is_valid(output: &Output) -> bool {
    !output.is_empty()
        && output.len() <= MAX_FIELDS
        && output
            .iter()
            .all(|(field, kind)| name(field).is_ok() && well_formed(kind))
}

/// One value of an output as it crosses the channel (ADR-0061). It holds no text: a choice is the
/// index of its option, which the engine reads from its own copy of the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TypedValue {
    Bool(bool),
    Integer(i64),
    Choice(u32),
}

impl Field {
    /// Whether `value` is one this field may hold.
    pub fn admits(&self, value: &TypedValue) -> bool {
        match (self, value) {
            (Field::Bool, TypedValue::Bool(_)) => true,
            (Field::Integer(range), TypedValue::Integer(v)) => range.min <= *v && *v <= range.max,
            (Field::Choice(options), TypedValue::Choice(at)) => (*at as usize) < options.len(),
            _ => false,
        }
    }
}

/// Whether `values` are a value for each field of `output`, in its order. The engine checks what
/// the broker sends with this, as it checks every message.
pub fn values_fit(output: &Output, values: &[TypedValue]) -> bool {
    output.len() == values.len() && output.values().zip(values).all(|(f, v)| f.admits(v))
}

/// The JSON Schema of an answer to `output`, for a provider to constrain its output with (#273): an
/// object with exactly these fields, every one required, a choice as an `enum` of its options and a
/// bool as a `boolean`. An integer's range is said in its `description`, since not every provider
/// takes `minimum` and `maximum`. The schema only makes a good answer likely; [`read_answer`] is
/// still the check.
pub fn answer_schema(output: &Output) -> serde_json::Value {
    use serde_json::{Map, Value, json};
    let mut properties = Map::new();
    for (field, kind) in output {
        let schema = match kind {
            Field::Bool => json!({"type": "boolean"}),
            Field::Integer(range) => json!({
                "type": "integer",
                "description": format!("a whole number from {} to {}", range.min, range.max),
            }),
            Field::Choice(options) => json!({"type": "string", "enum": options}),
        };
        properties.insert(field.clone(), schema);
    }
    json!({
        "type": "object",
        "properties": Value::Object(properties),
        "required": output.keys().collect::<Vec<_>>(),
        "additionalProperties": false,
    })
}

/// The form of an answer to `output`, with a placeholder for each value rather than a value, so the
/// form steers no choice: `{"add_test": <true or false>, "approach": "<one of its options, exactly as written>"}`.
pub fn answer_form(output: &Output) -> String {
    let fields: Vec<String> = output
        .iter()
        .map(|(field, kind)| {
            let value = match kind {
                Field::Bool => "<true or false>".to_string(),
                Field::Integer(range) => {
                    format!("<a whole number from {} to {}>", range.min, range.max)
                }
                Field::Choice(_) => "\"<one of its options, exactly as written>\"".to_string(),
            };
            format!("\"{field}\": {value}")
        })
        .collect();
    format!("{{{}}}", fields.join(", "))
}

/// The first field of `output` that an answer leaves out or gives a bad value, in name order, if
/// `text` is a JSON object at all. Only the field's name, which the workflow's author wrote, never
/// anything from the answer: the broker says it when it refuses an answer, so the next failure can
/// be told apart (#273).
pub fn refused_field<'a>(output: &'a Output, text: &str) -> Option<&'a str> {
    let text = text.trim();
    let text = text
        .strip_prefix("```")
        .and_then(|f| f.split_once('\n'))
        .and_then(|(_, rest)| rest.strip_suffix("```"))
        .map_or(text, str::trim);
    let answer = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let answer = answer.as_object()?;
    output.iter().find_map(|(field, kind)| {
        let fits = answer.get(field).and_then(|given| value_of(kind, given));
        fits.is_none().then_some(field.as_str())
    })
}

/// The longest answer text a model may give a reader, in bytes, once any code fence is removed.
pub const MAX_ANSWER_TEXT: usize = 16 * 1024;

/// A model's answer to a reader, read as `output` asks: a JSON object with exactly its fields, each
/// of its kind and within its range or options. Anything else is a short code, and fails the job.
pub fn read_answer(output: &Output, text: &str) -> Result<Vec<TypedValue>, &'static str> {
    let mut text = text.trim();
    // A model may fence its JSON. Only a whole-answer fence is removed, nothing inside it.
    if let Some(fenced) = text.strip_prefix("```") {
        let (_, rest) = fenced.split_once('\n').ok_or("reader.not_json")?;
        text = rest.strip_suffix("```").ok_or("reader.not_json")?.trim();
    }
    if text.len() > MAX_ANSWER_TEXT {
        return Err("reader.too_large");
    }
    typed(output, text).map_err(|wrong| match wrong {
        Wrong::NotJson => "reader.not_json",
        Wrong::Duplicate => "reader.duplicate_field",
        Wrong::Unknown => "reader.unknown_field",
        Wrong::Missing => "reader.missing_field",
        Wrong::BadValue => "reader.bad_value",
    })
}

/// A plugin's answer to a deterministic step, read as `output` asks: the object a reader answers
/// with, with no fence and no surrounding text (ADR-0102). Anything else is a short `plugin.*` code.
pub(crate) fn read_plugin_answer(
    output: &Output,
    text: &str,
) -> Result<Vec<TypedValue>, &'static str> {
    if text.len() > MAX_ANSWER_TEXT {
        return Err("plugin.answer_too_large");
    }
    typed(output, text.trim()).map_err(|wrong| match wrong {
        Wrong::NotJson => "plugin.answer_not_json",
        Wrong::Duplicate | Wrong::Unknown | Wrong::Missing | Wrong::BadValue => {
            "plugin.answer_does_not_fit"
        }
    })
}

/// The longest result a harness may submit, in bytes (ADR-0064).
pub const MAX_SUBMISSION: usize = 16 * 1024;

/// A result a harness submitted through `hyalos-submit`, read as the manifest's `result` asks: the
/// same object a reader answers with, with no fence and no surrounding text (ADR-0064). Anything
/// else is a short `submit.*` code, and fails the job.
pub fn read_submission(output: &Output, bytes: &[u8]) -> Result<Vec<TypedValue>, &'static str> {
    if bytes.len() > MAX_SUBMISSION {
        return Err("submit.too_large");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "submit.not_json")?;
    typed(output, text.trim()).map_err(|wrong| match wrong {
        Wrong::NotJson => "submit.not_json",
        Wrong::Duplicate => "submit.duplicate_field",
        Wrong::Unknown => "submit.unknown_field",
        Wrong::Missing => "submit.missing_field",
        Wrong::BadValue => "submit.bad_value",
    })
}

/// Why an object is not a value of an output.
enum Wrong {
    NotJson,
    Duplicate,
    Unknown,
    Missing,
    BadValue,
}

/// `text` read as one JSON object with exactly the fields of `output`, each of its kind and within
/// its range or options.
fn typed(output: &Output, text: &str) -> Result<Vec<TypedValue>, Wrong> {
    if !text.starts_with('{') {
        return Err(Wrong::NotJson);
    }
    let unique::Unique(answer) = serde_json::from_str::<unique::Unique<serde_json::Value>>(text)
        .map_err(|e| {
            if e.to_string().contains("named twice") {
                Wrong::Duplicate
            } else {
                Wrong::NotJson
            }
        })?;
    if answer.keys().any(|key| !output.contains_key(key)) {
        return Err(Wrong::Unknown);
    }
    output
        .iter()
        .map(|(field, kind)| {
            let given = answer.get(field).ok_or(Wrong::Missing)?;
            value_of(kind, given).ok_or(Wrong::BadValue)
        })
        .collect()
}

/// `given` as a value of `kind`: of its kind and within its range or options, or none.
fn value_of(kind: &Field, given: &serde_json::Value) -> Option<TypedValue> {
    let value = match (kind, given) {
        (Field::Bool, serde_json::Value::Bool(b)) => Some(TypedValue::Bool(*b)),
        (Field::Integer(_), serde_json::Value::Number(n)) => n.as_i64().map(TypedValue::Integer),
        (Field::Choice(options), serde_json::Value::String(s)) => options
            .iter()
            .position(|o| o == s)
            .and_then(|at| u32::try_from(at).ok())
            .map(TypedValue::Choice),
        _ => None,
    };
    value.filter(|v| kind.admits(v))
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
    fn plugin_text_reaches_an_actor_and_tainted_text_still_does_not() {
        let checked = Workflow::parse(
            r#"{"version":1,"name":"w","inputs":{"skill":"plugin_text","issue":"forge_issue"},"steps":[
                {"id":"r","role":"reader","reads":["issue","skill"],"instructions":"","output":{"k":"bool"}},
                {"id":"p","role":"planner","reads":["skill","r.k"],"instructions":"","output":{"k":"bool"}},
                {"id":"a","role":"actor","reads":["skill","p.k"],"instructions":""}]}"#,
        )
        .unwrap();
        // Plugin text does not taint: a planner reads it, and the actor gets it as plugin-authored.
        assert_eq!(
            checked.tainted("p"),
            Some(true),
            "p reads the tainted reader's field"
        );
        assert_eq!(
            checked.inputs("a").unwrap().reads,
            BTreeSet::from([
                Provenance::Operator,
                Provenance::PluginAuthored,
                Provenance::ModelFromTainted
            ])
        );
        let untainted = Workflow::parse(
            r#"{"version":1,"name":"w","inputs":{"skill":"plugin_text"},"steps":[
                {"id":"p","role":"planner","reads":["skill"],"instructions":"","output":{"k":"bool"}}]}"#,
        )
        .unwrap();
        assert_eq!(untainted.tainted("p"), Some(false));
        // What the planner made from the skill's text is the plugin's, not the operator's (#339).
        let next = Workflow::parse(
            r#"{"version":1,"name":"w","inputs":{"skill":"plugin_text"},"steps":[
                {"id":"p","role":"planner","reads":["skill"],"instructions":"","output":{"k":"bool"}},
                {"id":"a","role":"actor","reads":["p.k"],"instructions":""}]}"#,
        )
        .unwrap();
        assert!(
            next.inputs("a")
                .unwrap()
                .reads
                .contains(&Provenance::PluginAuthored)
        );
        // The same workflow with the issue given to the actor does not install.
        let raw = Workflow::parse(
            r#"{"version":1,"name":"w","inputs":{"skill":"plugin_text","issue":"forge_issue"},"steps":[
                {"id":"a","role":"actor","reads":["skill","issue"],"instructions":""}]}"#,
        );
        assert_eq!(raw.unwrap_err().code(), "workflow.raw_text_to_actor");
    }

    /// S6.5 (#40, ADR-0102): one workflow composes a reader, a deterministic step that runs an
    /// installed component, a planner and an actor given a skill. It installs, the role checks of
    /// E4 still hold, and each step knows the plugins whose grants bound it.
    const COMPOSED: &str = r#"{"version":2,"name":"composed",
        "inputs":{"issue":"forge_issue","task":"operator_text"},
        "steps":[
          {"id":"triage","role":"reader","reads":["issue"],"instructions":"Classify.",
           "output":{"kind":{"choice":["bug","docs"]}}},
          {"id":"score","role":"deterministic","plugin":"risk-score","reads":["task"],"instructions":"",
           "output":{"risk":{"integer":{"min":1,"max":5}}}},
          {"id":"plan","role":"planner","reads":["task","score.risk"],"instructions":"Choose.",
           "output":{"approach":{"choice":["small","large"]}}},
          {"id":"fix","role":"actor","plugin":"style-guide","reads":["task","triage.kind","plan.approach"],
           "instructions":"Fix it."}]}"#;

    #[test]
    fn a_workflow_composes_skills_components_and_deterministic_steps() {
        let checked = Workflow::parse(COMPOSED).unwrap();
        let names = |id: &str| -> Vec<&str> {
            checked
                .plugins(id)
                .unwrap()
                .iter()
                .map(PluginName::as_str)
                .collect()
        };
        // The component bounds its own step and every step its output reaches; the skill bounds
        // the actor. The reader reads no plugin.
        assert_eq!(names("triage"), Vec::<&str>::new());
        assert_eq!(names("score"), ["risk-score"]);
        assert_eq!(names("plan"), ["risk-score"]);
        assert_eq!(names("fix"), ["risk-score", "style-guide"]);
        let reads = |id: &str| checked.inputs(id).unwrap().reads.clone();
        assert!(reads("score").contains(&Provenance::PluginAuthored));
        assert!(reads("plan").contains(&Provenance::PluginAuthored));
        assert!(reads("fix").contains(&Provenance::PluginAuthored));
        assert!(!reads("triage").contains(&Provenance::PluginAuthored));
        // Taint still follows the issue to the actor, and nowhere the issue does not reach.
        assert_eq!(checked.tainted("triage"), Some(true));
        assert_eq!(checked.tainted("score"), Some(false));
        assert_eq!(checked.tainted("plan"), Some(false));
        assert!(reads("fix").contains(&Provenance::ModelFromTainted));

        // The role checks of E4 hold in a composed workflow.
        let refused = |from: &str, to: &str| {
            let doc = COMPOSED.replacen(from, to, 1);
            Workflow::parse(&doc).unwrap_err().code()
        };
        assert_eq!(
            refused(
                r#""reads":["task","triage.kind""#,
                r#""reads":["issue","triage.kind""#
            ),
            "workflow.raw_text_to_actor"
        );
        assert_eq!(
            refused(r#""reads":["task","score.risk"]"#, r#""reads":["issue"]"#),
            "workflow.raw_text_to_planner"
        );
        assert_eq!(
            refused(
                r#""plugin":"risk-score","reads":["task"]"#,
                r#""plugin":"risk-score","reads":["issue"]"#
            ),
            "workflow.raw_text_to_deterministic"
        );
        // A deterministic step names its component and returns typed fields; only it and an actor
        // name a plugin, and only in version 2.
        assert_eq!(
            refused(r#""plugin":"risk-score","#, ""),
            "workflow.plugin_required"
        );
        assert_eq!(
            refused(
                r#""role":"reader","reads""#,
                r#""role":"reader","plugin":"risk-score","reads""#
            ),
            "workflow.bad_plugin"
        );
        assert_eq!(
            refused(
                r#""output":{"risk":{"integer":{"min":1,"max":5}}}"#,
                r#""output":null"#
            ),
            "workflow.output_required"
        );
        assert_eq!(
            refused(r#""plugin":"risk-score""#, r#""plugin":"Risk""#),
            "workflow.malformed"
        );
        assert_eq!(
            refused(r#""version":2"#, r#""version":1"#),
            "workflow.role_not_supported"
        );
        let v1_plugin = COMPOSED
            .replacen(r#""version":2"#, r#""version":1"#, 1)
            .replacen(r#""role":"deterministic""#, r#""role":"planner""#, 1);
        assert_eq!(
            Workflow::parse(&v1_plugin).unwrap_err().code(),
            "workflow.bad_plugin"
        );
    }

    #[test]
    fn a_plugins_answer_is_read_as_typed_values_only() {
        let output: Output = [(
            "risk".to_string(),
            Field::Integer(IntegerRange { min: 1, max: 5 }),
        )]
        .into();
        let answer =
            |text: &str| crate::PluginText::granted(text, crate::Grant::none()).answer(&output);
        assert_eq!(answer(r#"{"risk": 3}"#), Ok(vec![TypedValue::Integer(3)]));
        assert_eq!(answer(r#" {"risk": 3} "#), Ok(vec![TypedValue::Integer(3)]));
        assert_eq!(
            answer("```\n{\"risk\": 3}\n```"),
            Err("plugin.answer_not_json")
        );
        assert_eq!(answer(r#"{"risk": 9}"#), Err("plugin.answer_does_not_fit"));
        assert_eq!(
            answer(r#"{"risk": 3, "why": "x"}"#),
            Err("plugin.answer_does_not_fit")
        );
        let long = format!(r#"{{"risk": 3}}{}"#, " ".repeat(MAX_ANSWER_TEXT));
        assert_eq!(answer(&long), Err("plugin.answer_too_large"));
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
        let version = edited(r#""version": 1"#, r#""version": 3"#);
        assert_eq!(version.unwrap_err().code(), "workflow.unknown_version");
        let version = edited(r#""version": 1"#, r#""version": 0"#);
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

    fn triage() -> Output {
        BTreeMap::from([
            ("has_repro".to_string(), Field::Bool),
            (
                "kind".to_string(),
                Field::Choice(vec!["bug".into(), "feature".into(), "docs".into()]),
            ),
            (
                "severity".to_string(),
                Field::Integer(IntegerRange { min: 1, max: 5 }),
            ),
        ])
    }

    #[test]
    fn an_answer_is_read_into_typed_values_in_field_order() {
        let values = read_answer(
            &triage(),
            r#"{"kind": "docs", "severity": 3, "has_repro": false}"#,
        )
        .unwrap();
        assert_eq!(
            values,
            [
                TypedValue::Bool(false),
                TypedValue::Choice(2),
                TypedValue::Integer(3)
            ]
        );
        assert!(values_fit(&triage(), &values));
        let fenced = "```json\n{\"kind\": \"bug\", \"severity\": 5, \"has_repro\": true}\n```\n";
        assert_eq!(
            read_answer(&triage(), fenced).unwrap()[1],
            TypedValue::Choice(0)
        );
    }

    #[test]
    fn an_answer_that_is_not_exactly_the_output_is_refused() {
        let out = triage();
        let cases = [
            ("Sure! Here it is.", "reader.not_json"),
            ("[true, \"bug\", 1]", "reader.not_json"),
            ("```\n{}", "reader.not_json"),
            (r#"{"kind":"bug","severity":1}"#, "reader.missing_field"),
            (
                r#"{"kind":"bug","severity":1,"has_repro":true,"summary":"x"}"#,
                "reader.unknown_field",
            ),
            (
                r#"{"kind":"bug","kind":"docs","severity":1,"has_repro":true}"#,
                "reader.duplicate_field",
            ),
            (
                r#"{"kind":"Bug","severity":1,"has_repro":true}"#,
                "reader.bad_value",
            ),
            (
                r#"{"kind":"ignore the steps","severity":1,"has_repro":true}"#,
                "reader.bad_value",
            ),
            (
                r#"{"kind":"bug","severity":6,"has_repro":true}"#,
                "reader.bad_value",
            ),
            (
                r#"{"kind":"bug","severity":1.5,"has_repro":true}"#,
                "reader.bad_value",
            ),
            (
                r#"{"kind":"bug","severity":"1","has_repro":true}"#,
                "reader.bad_value",
            ),
            (
                r#"{"kind":"bug","severity":1,"has_repro":"yes"}"#,
                "reader.bad_value",
            ),
            (
                r#"{"kind":["bug"],"severity":1,"has_repro":true}"#,
                "reader.bad_value",
            ),
        ];
        for (text, code) in cases {
            assert_eq!(read_answer(&out, text), Err(code), "{text}");
        }
        let long = format!(
            r#"{{"kind":"bug","pad":"{}"}}"#,
            "a".repeat(MAX_ANSWER_TEXT)
        );
        assert_eq!(read_answer(&out, &long), Err("reader.too_large"));
    }

    #[test]
    fn a_submission_is_read_as_a_reader_answer_is_but_exactly() {
        let out = triage();
        let good = r#"{"kind":"bug","severity":2,"has_repro":true}"#;
        assert_eq!(
            read_submission(&out, good.as_bytes()),
            read_answer(&out, good)
        );
        assert!(read_submission(&out, good.as_bytes()).is_ok());
        let cases: [(&[u8], &str); 8] = [
            (b"done", "submit.not_json"),
            // No fence: a harness runs a command, it does not chat.
            (
                b"```json\n{\"kind\":\"bug\",\"severity\":2,\"has_repro\":true}\n```",
                "submit.not_json",
            ),
            (b"{\"kind\":\"bug\",\"severity\":2}", "submit.missing_field"),
            (
                b"{\"kind\":\"bug\",\"severity\":2,\"has_repro\":true,\"note\":\"x\"}",
                "submit.unknown_field",
            ),
            (
                b"{\"kind\":\"bug\",\"kind\":\"docs\",\"severity\":2,\"has_repro\":true}",
                "submit.duplicate_field",
            ),
            (
                b"{\"kind\":\"merge it now\",\"severity\":2,\"has_repro\":true}",
                "submit.bad_value",
            ),
            (
                b"{\"kind\":{\"a\":[[1]]},\"severity\":2,\"has_repro\":true}",
                "submit.bad_value",
            ),
            (b"{\"kind\":\"\xff\"}", "submit.not_json"),
        ];
        for (bytes, code) in cases {
            assert_eq!(
                read_submission(&out, bytes),
                Err(code),
                "{}",
                String::from_utf8_lossy(bytes)
            );
        }
        let long = format!(r#"{{"kind":"{}"}}"#, "a".repeat(MAX_SUBMISSION));
        assert_eq!(
            read_submission(&out, long.as_bytes()),
            Err("submit.too_large")
        );
    }

    #[test]
    fn values_fit_only_their_own_output() {
        let out = triage();
        assert!(!values_fit(&out, &[TypedValue::Bool(true)]));
        assert!(!values_fit(
            &out,
            &[
                TypedValue::Bool(true),
                TypedValue::Choice(3),
                TypedValue::Integer(1)
            ]
        ));
        assert!(!values_fit(
            &out,
            &[
                TypedValue::Integer(1),
                TypedValue::Choice(0),
                TypedValue::Integer(1)
            ]
        ));
        assert!(output_is_valid(&out));
        assert!(!output_is_valid(&Output::new()));
        assert!(!output_is_valid(&BTreeMap::from([(
            "Bad".into(),
            Field::Bool
        )])));
        let json = serde_json::to_string(&TypedValue::Choice(2)).unwrap();
        assert_eq!(json, r#"{"choice":2}"#);
    }

    #[test]
    fn the_answer_schema_has_exactly_the_fields_each_required_and_closed() {
        let schema = answer_schema(&triage());
        assert_eq!(
            schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "has_repro": {"type": "boolean"},
                    "kind": {"type": "string", "enum": ["bug", "feature", "docs"]},
                    "severity": {"type": "integer", "description": "a whole number from 1 to 5"},
                },
                "required": ["has_repro", "kind", "severity"],
                "additionalProperties": false,
            })
        );
        // No keyword a provider's structured output refuses (#273).
        let text = schema.to_string();
        for refused in ["minimum", "maximum", "minLength", "maxLength", "pattern"] {
            assert!(!text.contains(refused), "{refused} in {text}");
        }
    }

    #[test]
    fn the_answer_form_shows_placeholders_and_no_option() {
        let form = answer_form(&triage());
        assert_eq!(
            form,
            r#"{"has_repro": <true or false>, "kind": "<one of its options, exactly as written>", "severity": <a whole number from 1 to 5>}"#
        );
        // An option in the form could steer the choice.
        for option in ["bug", "feature", "docs"] {
            assert!(!form.contains(option), "{option} in {form}");
        }
    }

    #[test]
    fn a_refused_answer_names_its_first_bad_field_and_nothing_from_the_answer() {
        let out = triage();
        let cases = [
            // The answers a small model gives when it mirrors the shape or quotes a bool (#273).
            (
                r#"{"has_repro":"false","kind":"bug","severity":1}"#,
                Some("has_repro"),
            ),
            (
                r#"{"has_repro":true,"kind":{"choice":"bug"},"severity":1}"#,
                Some("kind"),
            ),
            (
                r#"{"has_repro":true,"kind":"Bug","severity":1}"#,
                Some("kind"),
            ),
            (
                r#"{"has_repro":true,"kind":"bug","severity":9}"#,
                Some("severity"),
            ),
            (r#"{"has_repro":true,"kind":"bug"}"#, Some("severity")),
            (
                "```json\n{\"has_repro\":true,\"kind\":\"docs-only\",\"severity\":1}\n```",
                Some("kind"),
            ),
            (r#"{"has_repro":true,"kind":"bug","severity":1}"#, None),
            ("not json", None),
            ("[1, 2]", None),
        ];
        for (text, field) in cases {
            assert_eq!(refused_field(&out, text), field, "{text}");
            if field.is_some() {
                assert_eq!(read_answer(&out, text).ok(), None, "{text}");
            }
        }
    }
}
