//! Choosing the substrate a job runs on (ADR-0011, ADR-0043).
//!
//! v1 runs jobs on Linux hosts only. On Linux every driver is asked whether it is ready for the job's
//! image, and the one with the strongest isolation is chosen; among equals, the first asked.

use std::fmt;

use hyalos_types::SubstrateName;

use crate::{ContainerSubstrate, Isolation, Substrate, Unavailable};

/// The operating system this build runs on, as `std::env::consts::OS` names it.
pub const HOST_OS: &str = std::env::consts::OS;

/// The host is not one v1 runs jobs on (ADR-0011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    pub os: String,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "this host runs {}, and Hyalos v1 starts jobs only on Linux (ADR-0011): no sandbox driver has been tested anywhere else, so no job will start. On Windows, run Hyalos inside WSL2, which counts as Linux",
            self.os
        )
    }
}

impl std::error::Error for Unsupported {}

/// Refuses every host but Linux. WSL2 reports itself as Linux and is treated as Linux.
pub fn check_host(os: &str) -> Result<(), Unsupported> {
    if os == "linux" {
        Ok(())
    } else {
        Err(Unsupported { os: os.to_string() })
    }
}

/// One driver that was asked, and its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tried {
    pub name: SubstrateName,
    pub isolation: Isolation,
    pub result: Result<(), Unavailable>,
}

/// Every driver asked, in order, and the one chosen, if any was ready.
pub struct Choice {
    pub tried: Vec<Tried>,
    pub chosen: Option<Box<dyn Substrate>>,
}

impl Choice {
    /// What a job run on the chosen driver records on `job.started`.
    pub fn chosen_name(&self) -> Option<SubstrateName> {
        self.chosen.as_ref().map(|d| d.recorded_as())
    }
}

/// Asks every driver, in order, whether it is ready for `image`, and chooses the ready one with the
/// strongest isolation. On a host that is not Linux no driver is asked.
pub fn choose(
    os: &str,
    drivers: Vec<Box<dyn Substrate>>,
    image: &str,
) -> Result<Choice, Unsupported> {
    check_host(os)?;
    let mut tried = Vec::new();
    let mut chosen: Option<Box<dyn Substrate>> = None;
    for driver in drivers {
        let result = driver.ready(image);
        let ready = result.is_ok();
        tried.push(Tried {
            name: driver.recorded_as(),
            isolation: driver.isolation(),
            result,
        });
        if ready
            && chosen
                .as_ref()
                .is_none_or(|c| driver.isolation() > c.isolation())
        {
            chosen = Some(driver);
        }
    }
    Ok(Choice { tried, chosen })
}

/// [`choose`] on this host among the drivers this build has: Podman, then Docker.
pub fn detect(image: &str) -> Result<Choice, Unsupported> {
    let drivers = ContainerSubstrate::candidates()
        .into_iter()
        .map(|d| Box::new(d) as Box<dyn Substrate>)
        .collect();
    choose(HOST_OS, drivers, image)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::{Sandbox, SandboxSpec, StartError};

    struct Fake {
        name: &'static str,
        isolation: Isolation,
        ready: bool,
        asked: Arc<AtomicUsize>,
    }

    impl Substrate for Fake {
        fn name(&self) -> &'static str {
            self.name
        }
        fn isolation(&self) -> Isolation {
            self.isolation
        }
        fn available(&self) -> Result<(), Unavailable> {
            Ok(())
        }
        fn ready(&self, _: &str) -> Result<(), Unavailable> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            if self.ready {
                Ok(())
            } else {
                Err(Unavailable(format!("{} has no init", self.name)))
            }
        }
        fn start(&self, _: &SandboxSpec) -> Result<Box<dyn Sandbox>, StartError> {
            Err(StartError::Refused("fake".into()))
        }
        fn existing(&self) -> Result<Vec<String>, Unavailable> {
            Ok(Vec::new())
        }
    }

    fn fakes(
        list: &[(&'static str, Isolation, bool)],
        asked: &Arc<AtomicUsize>,
    ) -> Vec<Box<dyn Substrate>> {
        list.iter()
            .map(|&(name, isolation, ready)| {
                Box::new(Fake {
                    name,
                    isolation,
                    ready,
                    asked: asked.clone(),
                }) as Box<dyn Substrate>
            })
            .collect()
    }

    fn name(s: &str) -> SubstrateName {
        SubstrateName::new(s).unwrap()
    }

    #[test]
    fn a_host_that_is_not_linux_is_refused_before_any_driver_is_asked() {
        let asked = Arc::new(AtomicUsize::new(0));
        for os in ["macos", "windows", "freebsd"] {
            let drivers = fakes(&[("one", Isolation::Container, true)], &asked);
            let err = choose(os, drivers, "image").err().unwrap();
            assert_eq!(err, Unsupported { os: os.into() });
            let message = err.to_string();
            assert!(
                message.contains(os) && message.contains("only on Linux"),
                "{message}"
            );
        }
        assert_eq!(asked.load(Ordering::SeqCst), 0);
        assert_eq!(check_host("linux"), Ok(()));
    }

    #[test]
    fn the_strongest_ready_driver_is_chosen_and_every_driver_is_reported() {
        let asked = Arc::new(AtomicUsize::new(0));
        let drivers = fakes(
            &[
                ("first", Isolation::Container, true),
                ("vm", Isolation::MicroVm, false),
                ("gvisor", Isolation::UserspaceKernel, true),
                ("second", Isolation::Container, true),
            ],
            &asked,
        );
        let choice = choose("linux", drivers, "image").unwrap();
        assert_eq!(choice.chosen_name(), Some(name("gvisor")));
        assert_eq!(asked.load(Ordering::SeqCst), 4);
        let names: Vec<_> = choice.tried.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["first", "vm", "gvisor", "second"]);
        assert_eq!(
            choice.tried[1].result,
            Err(Unavailable("vm has no init".into()))
        );
    }

    #[test]
    fn among_equals_the_first_is_chosen_and_an_unready_one_never_is() {
        let asked = Arc::new(AtomicUsize::new(0));
        let drivers = fakes(
            &[
                ("podman", Isolation::Container, false),
                ("docker", Isolation::Container, true),
                ("other", Isolation::Container, true),
            ],
            &asked,
        );
        let choice = choose("linux", drivers, "image").unwrap();
        assert_eq!(choice.chosen_name(), Some(name("docker")));

        let none = choose(
            "linux",
            fakes(&[("podman", Isolation::Container, false)], &asked),
            "image",
        )
        .unwrap();
        assert!(none.chosen.is_none());
        assert_eq!(none.tried.len(), 1);
    }

    #[test]
    fn a_container_driver_records_its_runtime() {
        for (runtime, recorded) in [
            ("podman", "container.podman"),
            ("/usr/bin/docker", "container.docker"),
            ("/opt/nerdctl", "container"),
        ] {
            assert_eq!(
                ContainerSubstrate::new(runtime).recorded_as(),
                name(recorded)
            );
        }
        let names: Vec<_> = ContainerSubstrate::candidates()
            .iter()
            .map(|d| d.recorded_as())
            .collect();
        assert_eq!(names, [name("container.podman"), name("container.docker")]);
    }
}
