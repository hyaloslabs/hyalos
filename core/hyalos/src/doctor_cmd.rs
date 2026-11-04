//! `hyalos doctor`: what this host can run, and what the operator must still do (ADR-0011, ADR-0043).
//!
//! It refuses a host that is not Linux, asks every substrate driver whether it is ready for the
//! sandbox image (for a container runtime that includes starting an init, ADR-0041), names the one a
//! job would run on, and reminds the operator to cap spending on the provider key at the provider. It
//! reads no key and writes nothing; the init probe starts and removes one short container.

use std::process::ExitCode;

use hyalos_substrate::{Choice, HOST_OS, Isolation, Unsupported, detect};

/// The sandbox image a job runs, as `images/sandbox/Containerfile` builds it (ADR-0040).
pub const DEFAULT_IMAGE: &str = "hyalos-sandbox:latest";

const USAGE: &str = "usage: hyalos doctor [--image <image>]
  checks that this host can run jobs: that it is Linux, which substrate a job would run on (each
  runtime must have the sandbox image and start an init), and reminds you to cap spending on the
  provider key. --image names the sandbox image (default hyalos-sandbox:latest).
  Exit 0 if a job could run, 1 if not, 2 for bad usage";

/// The provider-side spend cap reminder (ADR-0038). A job's budget bounds what the broker spends; only
/// the provider can bound a key that has leaked.
pub const SPEND_CAP: &str = "spend cap: set a spend limit on the dedicated API key at the provider (the Anthropic Console's or OpenAI's usage limits). Hyalos holds each job to its budget, but a key cannot be scoped to one job, so the provider's cap is what bounds a loss if the key leaks. See docs/operations/provider-keys.md";

pub fn run(args: &[String]) -> ExitCode {
    let image = match args {
        [] => DEFAULT_IMAGE.to_string(),
        [flag, image] if flag == "--image" => image.clone(),
        _ => {
            eprintln!("hyalos: doctor takes only --image <image>\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (text, ok) = report(HOST_OS, &detect(&image), &image);
    print!("{text}");
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn isolation(i: Isolation) -> &'static str {
    match i {
        Isolation::Container => "container",
        Isolation::UserspaceKernel => "userspace kernel",
        Isolation::MicroVm => "micro-VM",
    }
}

/// The report for a host and what detection found, and whether a job could run.
pub fn report(os: &str, detected: &Result<Choice, Unsupported>, image: &str) -> (String, bool) {
    let mut out = String::new();
    let ok = match detected {
        Err(unsupported) => {
            out.push_str(&format!("host: {os}: no\n  {unsupported}\n"));
            false
        }
        Ok(choice) => {
            out.push_str(&format!("host: {os}: ok\nimage: {image}\n"));
            for tried in &choice.tried {
                match &tried.result {
                    Ok(()) => out.push_str(&format!(
                        "substrate {}: ready ({} isolation)\n",
                        tried.name,
                        isolation(tried.isolation)
                    )),
                    Err(why) => {
                        out.push_str(&format!("substrate {}: {why}\n", tried.name));
                    }
                }
            }
            match choice.chosen_name() {
                Some(name) => {
                    out.push_str(&format!(
                        "chosen: {name}; each job records it on its job.started event\n"
                    ));
                    true
                }
                None => {
                    out.push_str(
                        "chosen: none; no job can run until one substrate above is ready\n",
                    );
                    false
                }
            }
        }
    };
    out.push_str(SPEND_CAP);
    out.push('\n');
    (out, ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyalos_substrate::{ContainerSubstrate, Tried, Unavailable};
    use hyalos_types::SubstrateName;

    fn tried(name: &str, result: Result<(), Unavailable>) -> Tried {
        Tried {
            name: SubstrateName::new(name).unwrap(),
            isolation: Isolation::Container,
            result,
        }
    }

    #[test]
    fn a_host_that_is_not_linux_is_refused_and_says_why() {
        let (text, ok) = report(
            "macos",
            &Err(Unsupported { os: "macos".into() }),
            DEFAULT_IMAGE,
        );
        assert!(!ok);
        assert!(text.contains("host: macos: no"), "{text}");
        assert!(text.contains("only on Linux"), "{text}");
        assert!(text.contains("spend cap"), "{text}");
    }

    #[test]
    fn the_chosen_substrate_and_every_refusal_are_named() {
        let choice = Choice {
            tried: vec![
                tried(
                    "container.podman",
                    Err(Unavailable("install catatonit".into())),
                ),
                tried("container.docker", Ok(())),
            ],
            chosen: Some(Box::new(ContainerSubstrate::new("docker"))),
        };
        let (text, ok) = report("linux", &Ok(choice), "img:1");
        assert!(ok);
        assert!(text.contains("image: img:1"), "{text}");
        assert!(
            text.contains("substrate container.podman: not available: install catatonit"),
            "{text}"
        );
        assert!(
            text.contains("substrate container.docker: ready (container isolation)"),
            "{text}"
        );
        assert!(text.contains("chosen: container.docker"), "{text}");
        assert!(text.ends_with(&format!("{SPEND_CAP}\n")), "{text}");
    }

    #[test]
    fn no_ready_substrate_is_a_failure() {
        let choice = Choice {
            tried: vec![tried("container.docker", Err(Unavailable("gone".into())))],
            chosen: None,
        };
        let (text, ok) = report("linux", &Ok(choice), DEFAULT_IMAGE);
        assert!(!ok);
        assert!(text.contains("chosen: none"), "{text}");
        assert!(text.contains("spend cap"), "{text}");
    }
}
