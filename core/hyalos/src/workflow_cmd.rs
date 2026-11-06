//! `hyalos workflow check <file>`: whether a workflow installs (ADR-0005, ADR-0060).
//!
//! It reads and checks the document, writes nothing, and prints each step with its role, what it
//! reads by provenance, and whether it runs tainted. Installing a workflow comes with the fix-issue
//! workflow (S4.6).

use std::io::Read;
use std::process::ExitCode;

use hyalos_types::Workflow;
use hyalos_types::workflow::MAX_WORKFLOW_DOCUMENT;

const USAGE: &str = "usage: hyalos workflow check <file>
  check says whether <file> is a workflow that installs: every reference resolves, every output
  field is typed, and no actor or planner reads raw forge text. It writes nothing";

pub fn run(args: &[String]) -> ExitCode {
    match run_with(args) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err((code, message)) => {
            eprintln!("hyalos: {message}");
            ExitCode::from(code)
        }
    }
}

/// The command's output, or its exit code and message: 1 if the workflow does not install, 2 for
/// usage.
pub fn run_with(args: &[String]) -> Result<String, (u8, String)> {
    let [check, file] = args else {
        return Err((2, USAGE.to_string()));
    };
    if check != "check" {
        return Err((2, format!("unknown workflow command\n{USAGE}")));
    }
    let document = read(file).map_err(|e| (1, e))?;
    let checked = Workflow::parse(&document).map_err(|e| (1, format!("{file}: {e}")))?;
    let workflow = checked.workflow();
    let mut out = format!(
        "{file}: workflow {} installs, {} step(s)\n",
        workflow.name,
        workflow.steps.len()
    );
    for step in &workflow.steps {
        let reads: Vec<String> = checked
            .inputs(&step.id)
            .map(|i| i.reads.iter().map(|p| format!("{p:?}")).collect())
            .unwrap_or_default();
        let tainted = match checked.tainted(&step.id) {
            Some(true) => "tainted",
            _ => "untainted",
        };
        out.push_str(&format!(
            "  {} ({:?}): reads {}; runs {tainted}\n",
            step.id,
            step.role,
            reads.join(", ")
        ));
    }
    Ok(out)
}

fn read(file: &str) -> Result<String, String> {
    let handle = std::fs::File::open(file).map_err(|e| format!("cannot read {file}: {e}"))?;
    let mut bytes = Vec::new();
    handle
        .take(MAX_WORKFLOW_DOCUMENT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {file}: {e}"))?;
    if bytes.len() > MAX_WORKFLOW_DOCUMENT {
        return Err(format!(
            "{file} is larger than a workflow may be ({MAX_WORKFLOW_DOCUMENT} bytes)"
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("{file} is not text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
    }

    fn file(dir: &std::path::Path, body: &str) -> String {
        let path = dir.join("workflow.json");
        std::fs::write(&path, body).unwrap();
        path.display().to_string()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hyalos-workflow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const GOOD: &str = r#"{"version":1,"name":"w","inputs":{"issue":"forge_issue"},"steps":[
        {"id":"triage","role":"reader","reads":["issue"],"instructions":"","output":{"bug":"bool"}},
        {"id":"fix","role":"actor","reads":["triage.bug"],"instructions":""}]}"#;

    #[test]
    fn a_workflow_that_installs_is_described_step_by_step() {
        let dir = scratch("good");
        let out = run_with(&args(&["check", &file(&dir, GOOD)])).unwrap();
        assert!(out.contains("workflow w installs, 2 step(s)"), "{out}");
        assert!(
            out.contains("triage (Reader): reads Operator, ForgeAuthored; runs tainted"),
            "{out}"
        );
        assert!(
            out.contains("fix (Actor): reads Operator, ModelFromTainted; runs tainted"),
            "{out}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_actor_reading_the_issue_is_refused_with_its_code() {
        let dir = scratch("raw");
        let bad = GOOD.replace(r#""reads":["triage.bug"]"#, r#""reads":["issue"]"#);
        let (code, message) = run_with(&args(&["check", &file(&dir, &bad)])).unwrap_err();
        assert_eq!(code, 1);
        assert!(message.contains("workflow.raw_text_to_actor"), "{message}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn usage_and_unreadable_files() {
        assert_eq!(run_with(&args(&[])).unwrap_err().0, 2);
        assert_eq!(run_with(&args(&["install", "x"])).unwrap_err().0, 2);
        let dir = scratch("big");
        let big = " ".repeat(MAX_WORKFLOW_DOCUMENT + 1);
        let (code, message) = run_with(&args(&["check", &file(&dir, &big)])).unwrap_err();
        assert_eq!(code, 1);
        assert!(
            message.contains("larger than a workflow may be"),
            "{message}"
        );
        let missing = dir.join("missing.json").display().to_string();
        assert_eq!(run_with(&args(&["check", &missing])).unwrap_err().0, 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
