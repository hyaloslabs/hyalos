//! The customer's verifier: one program that checks a hyalOS record and exits 0 for a pass, 1 for a
//! refusal and 2 when the check could not finish. It holds no check of its own: arguments, bounded
//! reads and the exit code are here, and everything else is `hyalos-audit`.

#![forbid(unsafe_code)]

use hyalos_audit::{
    AuditInputs, FileError, FileKind, Files, LogSource, Verdict, Verifier, check, notary_key_text,
};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    path::Path,
    process::ExitCode,
};

const MIB: u64 = 1024 * 1024;

/// The most bytes each kind of input may hold.
fn limit(kind: FileKind) -> u64 {
    match kind {
        FileKind::Transcript | FileKind::Guest => 16 * MIB,
        FileKind::Presentation => MIB,
        FileKind::Link => 64 * 1024,
        FileKind::Key | FileKind::Salt => 4 * 1024,
    }
}

/// Reads a regular file, checking its size before reading and never taking more than its limit.
struct BoundedFiles;

impl Files for BoundedFiles {
    fn read(&self, path: &Path, kind: FileKind) -> Result<Vec<u8>, FileError> {
        let opened = |error: std::io::Error| match error.kind() {
            ErrorKind::NotFound => FileError::Missing,
            _ => FileError::Unreadable,
        };
        let file = File::open(path).map_err(opened)?;
        let meta = file.metadata().map_err(|_| FileError::Unreadable)?;
        if !meta.is_file() {
            return Err(FileError::Unreadable);
        }
        let most = limit(kind);
        if meta.len() > most {
            return Err(FileError::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(most + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| FileError::Unreadable)?;
        if bytes.len() as u64 > most {
            return Err(FileError::TooLarge);
        }
        Ok(bytes)
    }
}

/// What stopped the check before the audit began.
enum Stop {
    Refused(String),
    Inconclusive(String),
}

struct Command {
    record: String,
    inputs: AuditInputs,
}

fn inconclusive<T>(what: impl Into<String>) -> Result<T, Stop> {
    Err(Stop::Inconclusive(what.into()))
}

/// The trusted notary's key from its file. An unreadable file is inconclusive, and one over its
/// limit or holding no signing key is refused.
fn notary_key(path: &str) -> Result<String, Stop> {
    let what = "the notary's public key";
    let bytes = BoundedFiles
        .read(Path::new(path), FileKind::Key)
        .map_err(|error| match error {
            FileError::TooLarge => Stop::Refused(format!("{what} is over its size limit")),
            _ => Stop::Inconclusive(format!("{what} is not a readable file")),
        })?;
    let text = String::from_utf8(bytes)
        .map_err(|_| Stop::Refused(format!("{what} is not a signing public key")))?;
    notary_key_text(&text).map_err(Stop::Refused)
}

/// The verify program, resolved once so what runs is the file checked here.
fn verify_program(path: &str) -> Result<String, Stop> {
    let what = "the verify program cannot be launched";
    let Ok(resolved) = fs::canonicalize(path) else {
        return inconclusive(what);
    };
    if !fs::metadata(&resolved).is_ok_and(|m| m.is_file()) {
        return inconclusive(what);
    }
    match resolved.to_str() {
        Some(text) => Ok(text.to_string()),
        None => inconclusive(what),
    }
}

fn parse(args: &[String]) -> Result<Command, Stop> {
    let (mut positional, mut operator, mut notary, mut verify, mut guest) =
        (Vec::new(), None, None, None, None);
    let mut legacy = false;
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let mut value = |name: &str| {
            words
                .next()
                .cloned()
                .ok_or_else(|| Stop::Inconclusive(format!("{name} needs a value")))
        };
        match word.as_str() {
            "--operator-public" => operator = Some(value("--operator-public")?),
            "--notary-public" => notary = Some(value("--notary-public")?),
            "--verify" => verify = Some(value("--verify")?),
            "--guest" => guest = Some(value("--guest")?),
            "--legacy-v1" => legacy = true,
            flag if flag.starts_with('-') => return inconclusive("unknown option"),
            _ => positional.push(word.clone()),
        }
    }
    let [record, links] = <[String; 2]>::try_from(positional).map_err(|_| {
        Stop::Inconclusive("usage: hyalos-verify <record-file> <links-dir> ...".to_string())
    })?;
    let Some(operator) = operator else {
        return inconclusive("the operator's public key is not given: --operator-public <file>");
    };
    let Some(notary) = notary else {
        return inconclusive("the notary's public key is not given: --notary-public <file>");
    };
    let Some(verify) = verify else {
        return inconclusive("the verify program is not given: --verify <program you built>");
    };
    let notary = notary_key(&notary)?;
    let verify = verify_program(&verify)?;
    let inputs = AuditInputs {
        dir: Some(links),
        operator: Some(operator),
        notary,
        verify,
        legacy,
        guest,
    };
    Ok(Command { record, inputs })
}

fn version() -> String {
    let commit = option_env!("HYALOS_BUILD_COMMIT")
        .filter(|c| c.len() == 40 && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or("unknown");
    format!(
        "hyalos-verify {}\ncommit {commit}\nrecords 1-4\nprofiles hyalos.profile/1",
        env!("CARGO_PKG_VERSION")
    )
}

fn finish(verdict: Verdict) -> ExitCode {
    match verdict {
        Verdict::Passed(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Verdict::Refused(what) => {
            eprintln!("refused: {what}");
            ExitCode::from(1)
        }
        Verdict::Inconclusive(what) => {
            eprintln!("inconclusive: {what}");
            ExitCode::from(2)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = match std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect()
    {
        Ok(args) => args,
        Err(_) => return finish(Verdict::Inconclusive("an argument is not text".to_string())),
    };
    if args == ["--version"] {
        println!("{}", version());
        return ExitCode::SUCCESS;
    }
    let command = match parse(&args) {
        Ok(command) => command,
        Err(Stop::Refused(what)) => return finish(Verdict::Refused(what)),
        Err(Stop::Inconclusive(what)) => return finish(Verdict::Inconclusive(what)),
    };
    let log = match LogSource::from_env() {
        Ok(log) => log,
        Err(failure) => return finish(Verdict::Inconclusive(failure.message)),
    };
    let verifier = Verifier::from_env(&command.inputs.verify);
    finish(check(
        &command.record,
        &command.inputs,
        log,
        &verifier,
        &BoundedFiles,
    ))
}
