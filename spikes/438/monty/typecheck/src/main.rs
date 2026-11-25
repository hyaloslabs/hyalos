//! Checks a script with Monty's own type checker (`monty-type-checking`, which embeds `ty`) against
//! a stub of module `hyalos`, as Monty's sessions check code against host module stubs.
//!
//!   hyalos-monty-typecheck <stub.pyi> <script.py>
//!
//! Prints `rejected` and the diagnostics when the checker reports any, `accepted` when it reports
//! none, or `unusable: <why>` when it cannot check the script at all. It exits 0 in every case: the
//! result is recorded, not judged (S-05.3).

use monty_type_checking::{SourceFile, TypeCheckContext, TypeChecker};
use monty_types::{ModuleStub, TypeCheckingConfig};
use std::{env, fs};

fn main() {
    match check() {
        Ok(None) => println!("accepted"),
        Ok(Some(diagnostics)) => println!("rejected\n{diagnostics}"),
        Err(why) => println!("unusable: {}", why.replace('\n', " ")),
    }
}

fn check() -> Result<Option<String>, String> {
    let args: Vec<String> = env::args().collect();
    let [_, stub, script] = args.as_slice() else {
        return Err("usage: hyalos-monty-typecheck <stub.pyi> <script.py>".into());
    };
    let stub = fs::read_to_string(stub).map_err(|e| format!("cannot read the stub: {e}"))?;
    let script = fs::read_to_string(script).map_err(|e| format!("cannot read the script: {e}"))?;
    let stubs = [ModuleStub::new("hyalos", stub).map_err(|e| e.to_string())?];
    let context = TypeCheckContext {
        module_stubs: &stubs,
        ..TypeCheckContext::default()
    };
    let mut checker = TypeChecker::default();
    let diagnostics = checker.run_with(
        &SourceFile::new(&script, "main.py"),
        &context,
        TypeCheckingConfig::default(),
    )?;
    Ok(diagnostics.map(|d| d.to_string()))
}
