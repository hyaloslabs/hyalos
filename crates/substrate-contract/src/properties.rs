//! The contract's generated half (S8.3, ADR-0109): properties that hold for every spec a generator
//! draws, not only the cases written out in `checks`. Each case is named by its seed and replays with
//! `HYALOS_PROP_SEED=<seed>`; `HYALOS_PROP_CASES` runs more.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use hyalos_props::{AWKWARD, SeededRng, forall, one_in, pick, seeds, text, within};
use hyalos_substrate::{Ending, SandboxSpec, StartError, Substrate};

use crate::checks::{Ctx, run_to_end};

/// Cases per run of the round trip. Each starts a sandbox, so this is kept small for the drivers
/// that boot one.
const ROUND_TRIPS: u64 = 12;

/// Cases per run of the bad specs. A refused spec starts nothing, so these are cheap.
const BAD_SPECS: u64 = 200;

const UPPER: &[char] = &[
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '_',
];

/// A generated spec's arguments, environment and exit code reach the sandbox exactly: every argument
/// verbatim and unread by any shell on the way, every variable with its value, and the code it exits
/// with. The script prints each variable and then each argument on a line of its own.
pub(crate) fn round_trip(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    forall("substrate.round_trip", seeds(ROUND_TRIPS)?, |rng| {
        let code = rng.below(256) as i32;
        let mut env = BTreeMap::new();
        for i in 0..within(rng, 0..5) {
            let name = format!("HYALOS_P{i}_{}", text(rng, UPPER, 8));
            env.insert(name, text(rng, AWKWARD, 24));
        }
        let args: Vec<String> = (0..within(rng, 0..5))
            .map(|_| text(rng, AWKWARD, 24))
            .collect();

        let mut script = String::new();
        for name in env.keys() {
            script.push_str(&format!("printf '%s\\n' \"${name}\"; "));
        }
        script.push_str(&format!(
            "for a in \"$@\"; do printf '%s\\n' \"$a\"; done; exit {code}"
        ));
        let mut command = vec!["sh", "-c", &script, "sh"];
        command.extend(args.iter().map(String::as_str));
        let mut spec = c.spec(&command);
        spec.env = env.clone();

        let mut expected = String::new();
        for line in env.values().chain(args.iter()) {
            expected.push_str(line);
            expected.push('\n');
        }
        let out = run_to_end(s, &spec)?;
        if out.ending != Ending::Exited(code) || out.stdout != expected.as_bytes() {
            return Err(format!(
                "drew env {env:?}, args {args:?} and exit {code}; got {:?} with stdout {:?} and stderr {:?}",
                out.ending,
                out.stdout_text(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    })
    .map(|_| ())
}

/// A spec with one field made bad, at a place and in a way the generator draws, is refused as a
/// spec, and nothing is left behind.
pub(crate) fn bad_specs(s: &dyn Substrate, c: &Ctx) -> Result<(), String> {
    let before = s.existing().map_err(|e| e.to_string())?;
    forall("substrate.bad_specs", seeds(BAD_SPECS)?, |rng| {
        let (what, spec) = spoiled(rng, c.sh("exit 0"));
        match s.start(&spec) {
            Err(StartError::Spec(_)) => Ok(()),
            Err(other) => Err(format!("{what}: {other}, expected a refused spec")),
            Ok(_) => Err(format!("{what}: a bad spec was started")),
        }
    })?;
    if s.existing().map_err(|e| e.to_string())? != before {
        return Err("a refused spec left something behind".into());
    }
    Ok(())
}

/// `text` with `bad` put in at a drawn place.
fn with(rng: &mut SeededRng, text: &str, bad: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at = rng.below(chars.len() as u64 + 1) as usize;
    let mut out: String = chars[..at].iter().collect();
    out.push_str(bad);
    out.extend(&chars[at..]);
    out
}

/// A good path for a socket or for skills, made bad in one drawn way.
fn bad_path(rng: &mut SeededRng) -> PathBuf {
    let good = format!("/tmp/{}", text(rng, &['a', 'b', 'c'], 6));
    PathBuf::from(match rng.below(5) {
        0 => String::new(),
        1 => good.trim_start_matches('/').to_string(),
        2 => with(rng, &good, ","),
        3 => with(rng, &good, ":"),
        _ => with(rng, &good, "\n"),
    })
}

/// `spec` with one field made bad, and which.
fn spoiled(rng: &mut SeededRng, mut spec: SandboxSpec) -> (&'static str, SandboxSpec) {
    let what = match rng.below(9) {
        0 => {
            let bad = *pick(rng, &[" ", "\n", "\0", ";", "\t", "$", "`"]);
            spec.image = if one_in(rng, 4) {
                format!("-{}", spec.image)
            } else {
                with(rng, &spec.image, bad)
            };
            "image"
        }
        1 => {
            spec.command.clear();
            "empty command"
        }
        2 => {
            let at = rng.below(spec.command.len() as u64) as usize;
            spec.command[at] = with(rng, &spec.command[at], "\0");
            "NUL in an argument"
        }
        3 => {
            let good = format!("A{}", text(rng, UPPER, 6));
            let name = match rng.below(4) {
                0 => String::new(),
                1 => format!("{}{good}", rng.below(10)),
                _ => {
                    let bad = *pick(rng, &["=", " ", "-", ".", "é", "\0"]);
                    with(rng, &good, bad)
                }
            };
            spec.env.insert(name, "v".into());
            "environment name"
        }
        4 => {
            let good = text(rng, AWKWARD, 12);
            let value = with(rng, &good, "\0");
            spec.env.insert("A".into(), value);
            "NUL in a value"
        }
        5 => {
            spec.broker_socket = bad_path(rng);
            "socket path"
        }
        6 => {
            spec.skills = Some(bad_path(rng));
            "skills path"
        }
        7 => {
            spec.deadline = Duration::ZERO;
            "zero deadline"
        }
        _ => {
            if one_in(rng, 2) {
                spec.memory_bytes = 0;
            } else {
                spec.max_processes = 0;
            }
            "zero limit"
        }
    };
    (what, spec)
}
