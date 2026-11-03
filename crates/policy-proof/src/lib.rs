//! Proofs about the shipped baseline, made in CI with `cedar-policy-symcc` and the cvc5 solver
//! (S3.6, ADR-0008, ADR-0033).
//!
//! The solver runs here and nowhere else: this crate is not part of the trusted core and the `hyalos`
//! binary does not link it. It needs a `cvc5` executable (the `CVC5` environment variable, or `cvc5` on
//! the `PATH`); `Prover::new` says so when there is none.
//!
//! **What is proved.** Let *A* be the baseline plus the most permissive operator policy there is,
//! `permit (principal, action, resource);`. A satisfied `forbid` overrides every `permit`, so whatever
//! *A* allows, every operator policy allows at most as much. Each property says *A* never allows a
//! forbidden region, for every action the schema has. **What is not proved** is how the engine fills in
//! the request's facts (ADR-0008); a separate check keeps *A* from being vacuous.

use std::str::FromStr;

use cedar_policy::{PolicySet, RequestEnv, Schema};
use cedar_policy_symcc::solver::LocalSolver;
use cedar_policy_symcc::{CedarSymCompiler, CompiledPolicy, CompiledPolicySet};
use hyalos_policy::{BASELINE, SCHEMA};

/// The most permissive operator policy.
pub const PERMIT_ALL: &str = "permit (principal, action, resource);";

/// A property of the baseline: *A* allows nothing in the region the `unless` describes.
pub struct Property {
    pub name: &'static str,
    /// A policy set that allows everything except the forbidden region.
    pub allowed_everywhere_but: &'static str,
}

/// The properties ADR-0008 lists, each as the region that must stay forbidden.
pub const PROPERTIES: &[Property] = &[
    Property {
        name: "no write outside the job's own repo",
        allowed_everywhere_but: r#"permit (principal, action, resource)
            unless { action in Action::"repo_write" && resource.repo != principal.repo };"#,
    },
    Property {
        name: "no push outside the job's branch prefix",
        allowed_everywhere_but: r#"permit (principal, action, resource)
            unless { action == Action::"push" && !resource.in_branch_prefix };"#,
    },
    Property {
        name: "a tainted job cannot merge",
        allowed_everywhere_but: r#"permit (principal, action, resource)
            unless { principal.tainted && action == Action::"merge" };"#,
    },
    Property {
        name: "a tainted job cannot use a subscription token",
        allowed_everywhere_but: r#"permit (principal, action, resource)
            unless { principal.tainted && context.subscription_token };"#,
    },
    Property {
        name: "no irreversible action without an approval",
        allowed_everywhere_but: r#"permit (principal, action, resource)
            unless { action in Action::"irreversible" && !context.approved };"#,
    },
];

/// What was found for one property in one request environment (one action).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub property: String,
    pub action: String,
    /// `None` if it holds; otherwise a request the policies allow that they should not.
    pub counterexample: Option<String>,
}

/// How a change to the baseline differs from the version before it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comparison {
    /// Requests the new version allows (with every operator permit) that the old one did not.
    pub allows_more: Vec<Difference>,
    /// Requests the old version allowed that the new one does not.
    pub allows_less: Vec<Difference>,
}

impl Comparison {
    pub fn unchanged(&self) -> bool {
        self.allows_more.is_empty() && self.allows_less.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub action: String,
    pub example: String,
}

/// A solver and the shipped schema.
pub struct Prover {
    schema: Schema,
    compiler: CedarSymCompiler<LocalSolver>,
    runtime: tokio::runtime::Runtime,
}

fn err<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> String + '_ {
    move |e| format!("{what}: {e}")
}

impl Prover {
    /// Starts a solver. Fails with a message if there is no `cvc5`.
    pub fn new() -> Result<Self, String> {
        let (schema, _) = Schema::from_cedarschema_str(SCHEMA).map_err(err("the schema"))?;
        let solver = LocalSolver::cvc5().map_err(err(
            "no cvc5: set CVC5 to its path, or put it on the PATH (cvc5 1.3.1)",
        ))?;
        let compiler = CedarSymCompiler::new(solver).map_err(err("starting the solver"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(err("the async runtime"))?;
        Ok(Self {
            schema,
            compiler,
            runtime,
        })
    }

    fn envs(&self) -> Vec<RequestEnv> {
        let mut envs: Vec<RequestEnv> = self.schema.request_envs().collect();
        envs.sort_by_key(|e| e.action().to_string());
        envs
    }

    fn set(&self, text: &str) -> Result<PolicySet, String> {
        PolicySet::from_str(text).map_err(err("a policy set"))
    }

    /// The baseline with every operator permit: *A*.
    fn open(&self, baseline: &str) -> Result<PolicySet, String> {
        self.set(&format!("{baseline}\n{PERMIT_ALL}\n"))
    }

    /// Every property, for every action. All `counterexample`s are `None` if the baseline is sound.
    pub fn prove_properties(&mut self) -> Result<Vec<Finding>, String> {
        let a = self.open(BASELINE)?;
        let mut findings = Vec::new();
        for env in self.envs() {
            let action = env.action().to_string();
            let a_compiled = CompiledPolicySet::compile(&a, &env, &self.schema)
                .map_err(err("compiling the baseline"))?;
            for property in PROPERTIES {
                let b = self.set(property.allowed_everywhere_but)?;
                let b_compiled = CompiledPolicySet::compile(&b, &env, &self.schema)
                    .map_err(err("compiling a property"))?;
                let found = self
                    .runtime
                    .block_on(
                        self.compiler
                            .check_implies_with_counterexample_opt(&a_compiled, &b_compiled),
                    )
                    .map_err(err("the solver"))?;
                findings.push(Finding {
                    property: property.name.to_string(),
                    action: action.clone(),
                    counterexample: found.map(|c| c.to_string()),
                });
            }
        }
        Ok(findings)
    }

    /// The baseline with a permit is not vacuous: in every action there is a request it allows. If it
    /// denied everything, every property above would hold for nothing. Returns the actions where it
    /// allows nothing.
    pub fn actions_that_allow_nothing(&mut self) -> Result<Vec<String>, String> {
        let a = self.open(BASELINE)?;
        let mut dead = Vec::new();
        for env in self.envs() {
            let compiled = CompiledPolicySet::compile(&a, &env, &self.schema)
                .map_err(err("compiling the baseline"))?;
            let denies_all = self
                .runtime
                .block_on(self.compiler.check_always_denies_opt(&compiled))
                .map_err(err("the solver"))?;
            if denies_all {
                dead.push(env.action().to_string());
            }
        }
        Ok(dead)
    }

    /// Baseline policies that can error on a well-formed request, as `(policy id, action)`. An error
    /// in a `forbid` stops it forbidding (ADR-0008), so the answer must be empty.
    pub fn policies_that_can_error(&mut self) -> Result<Vec<(String, String)>, String> {
        let baseline = self.set(BASELINE)?;
        let mut errors = Vec::new();
        for env in self.envs() {
            for policy in baseline.policies() {
                let compiled = CompiledPolicy::compile(policy, &env, &self.schema)
                    .map_err(err("compiling a baseline policy"))?;
                let never = self
                    .runtime
                    .block_on(self.compiler.check_never_errors_opt(&compiled))
                    .map_err(err("the solver"))?;
                if !never {
                    errors.push((
                        policy.annotation("id").unwrap_or("?").to_string(),
                        env.action().to_string(),
                    ));
                }
            }
        }
        Ok(errors)
    }

    /// Compares two baselines, each with every operator permit, on every action.
    pub fn compare(&mut self, old: &str, new: &str) -> Result<Comparison, String> {
        let (old, new) = (self.open(old)?, self.open(new)?);
        let mut out = Comparison::default();
        for env in self.envs() {
            let action = env.action().to_string();
            let o = CompiledPolicySet::compile(&old, &env, &self.schema)
                .map_err(err("compiling the old baseline"))?;
            let n = CompiledPolicySet::compile(&new, &env, &self.schema)
                .map_err(err("compiling the new baseline"))?;
            // New allows something old does not: new does not imply old.
            if let Some(example) = self
                .runtime
                .block_on(self.compiler.check_implies_with_counterexample_opt(&n, &o))
                .map_err(err("the solver"))?
            {
                out.allows_more.push(Difference {
                    action: action.clone(),
                    example: example.to_string(),
                });
            }
            if let Some(example) = self
                .runtime
                .block_on(self.compiler.check_implies_with_counterexample_opt(&o, &n))
                .map_err(err("the solver"))?
            {
                out.allows_less.push(Difference {
                    action,
                    example: example.to_string(),
                });
            }
        }
        Ok(out)
    }
}

/// A comparison as the Markdown a pull request shows.
pub fn markdown(comparison: &Comparison) -> String {
    let mut out = String::from("### Baseline policy change\n\n");
    if comparison.unchanged() {
        out.push_str("The baseline allows exactly what it did before (checked with every operator permit, for every action).\n");
        return out;
    }
    if !comparison.allows_more.is_empty() {
        out.push_str("**This change allows more than before.** With every operator permit, these actions now allow requests they did not:\n\n");
        for d in &comparison.allows_more {
            out.push_str(&format!(
                "- `{}`, for example:\n\n```\n{}\n```\n",
                d.action, d.example
            ));
        }
    }
    if !comparison.allows_less.is_empty() {
        out.push_str("\nThis change allows less than before for these actions:\n\n");
        for d in &comparison.allows_less {
            out.push_str(&format!("- `{}`\n", d.action));
        }
    }
    out
}
