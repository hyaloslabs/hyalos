//! `hyalos doctor`: what this host can run, and what the operator must still do (ADR-0011, ADR-0043).
//!
//! It refuses a host that is not Linux, asks every substrate driver whether it is ready for the
//! sandbox image (for a container runtime that includes starting an init, ADR-0041), names the one a
//! job would run on, and reminds the operator to cap spending on the provider key at the provider. It
//! reads no key and writes nothing; the init probe starts and removes one short container.

use std::process::ExitCode;

use hyalos_substrate::{Choice, HOST_OS, Isolation, Unsupported, detect_with};

/// The sandbox image a job runs: as `images/sandbox/Containerfile` builds it (ADR-0040), or, in a
/// release binary, the image published with it, by digest (ADR-0081).
pub const DEFAULT_IMAGE: &str = match RELEASE_IMAGE {
    Some(image) => image,
    None => "hyalos-sandbox:latest",
};

/// The image a release was built with, set by the release workflow; `None` in a build from source.
pub const RELEASE_IMAGE: Option<&str> = match option_env!("HYALOS_RELEASE_IMAGE") {
    Some(image) if !image.is_empty() => Some(image),
    _ => None,
};

// A release names its image by digest, and a plain reference at that, or it does not build.
const _: () = if let Some(image) = RELEASE_IMAGE {
    assert!(
        by_digest(image),
        "HYALOS_RELEASE_IMAGE is not <name>@sha256:<64 hex>"
    );
};

/// Whether `image` is `<name>@sha256:<64 lowercase hex>`, with only the characters a plain image
/// reference has.
const fn by_digest(image: &str) -> bool {
    const AT: &[u8] = b"@sha256:";
    let b = image.as_bytes();
    if b.len() <= AT.len() + 64 || b.len() > 255 || b[0] == b'-' {
        return false;
    }
    let name = b.len() - AT.len() - 64;
    let mut i = 0;
    while i < name {
        let c = b[i];
        if !(c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'/' | b':' | b'-')) {
            return false;
        }
        i += 1;
    }
    while i < name + AT.len() {
        if b[i] != AT[i - name] {
            return false;
        }
        i += 1;
    }
    while i < b.len() {
        if !matches!(b[i], b'0'..=b'9' | b'a'..=b'f') {
            return false;
        }
        i += 1;
    }
    true
}

const USAGE: &str = "usage: hyalos doctor [--image <image>]
  checks that this host can run jobs: that it is Linux, which substrate a job would run on (each
  runtime must have the sandbox image and start an init), and reminds you to cap spending on the
  provider key. --image names the sandbox image (default hyalos-sandbox:latest, or the image a
  release was published with).
  Exit 0 if a job could run, 1 if not, 2 for bad usage";

/// The provider-side spend cap reminder (ADR-0038). A job's budget bounds what the broker spends; only
/// the provider can bound a key that has leaked.
pub const SPEND_CAP: &str = "spend cap: set a spend limit on the dedicated API key at the provider (the Anthropic Console's or OpenAI's usage limits). Hyalos holds each job to its budget, but a key cannot be scoped to one job, so the provider's cap is what bounds a loss if the key leaks. See docs/operations/provider-keys.md";

/// Said when there is no `microvm.conf`, since a job then never runs in a microVM (ADR-0058).
pub const MICROVM_ADVICE: &str = "microvm: not set up (no microvm.conf in the config directory); a microVM is the strongest boundary. See docs/operations/substrates.md\n";

pub fn run(args: &[String]) -> ExitCode {
    let image = match args {
        [] => DEFAULT_IMAGE.to_string(),
        [flag, image] if flag == "--image" => image.clone(),
        _ => {
            eprintln!("hyalos: doctor takes only --image <image>\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let env = |name: &str| std::env::var(name).ok();
    let microvm = match crate::microvm_conf::from_env(&env) {
        Ok(microvm) => microvm,
        Err(message) => {
            eprintln!("hyalos: {message}");
            return ExitCode::from(1);
        }
    };
    let none = microvm.is_none();
    let (mut text, ok) = report(HOST_OS, &detect_with(&image, microvm), &image);
    if none {
        text.push_str(MICROVM_ADVICE);
    }
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
            match (choice.chosen_name(), &choice.chosen) {
                (Some(name), Some(driver)) => {
                    out.push_str(&format!(
                        "chosen: {name}; each job records it on its job.started event\nattestation: {}; each job records this too (ADR-0057)\n",
                        crate::run_cmd::attestation_text(&driver.attestation())
                    ));
                    true
                }
                _ => {
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
    fn a_release_image_is_named_by_its_digest() {
        let digest = "0123456789abcdef".repeat(4);
        assert!(by_digest(&format!(
            "ghcr.io/hyaloslabs/hyalos-sandbox@sha256:{digest}"
        )));
        for bad in [
            "hyalos-sandbox:latest".to_string(),
            format!("@sha256:{digest}"),
            format!("-x@sha256:{digest}"),
            format!("a@b@sha256:{digest}"),
            format!("a@sha256:{}", &digest[1..]),
            format!("a@sha256:{}", digest.to_uppercase()),
            format!("a b@sha256:{digest}"),
            format!("a@sha512:{digest}"),
        ] {
            assert!(!by_digest(&bad), "{bad}");
        }
        // A build from source runs the image Containerfile builds.
        if RELEASE_IMAGE.is_none() {
            assert_eq!(DEFAULT_IMAGE, "hyalos-sandbox:latest");
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
        assert!(
            text.contains("attestation: none (attestation.no_hardware_root)"),
            "{text}"
        );
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
