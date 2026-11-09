//! The plugin host (S6.1, ADR-0087): a component reaches only the host imports its grant links, and
//! each call is checked against the granted names before the broker is asked.

use std::collections::BTreeSet;

use hyalos_plugin::{Host, Runtime, codes, interface};
use hyalos_types::{Grant, PluginText, ReasonCode};

/// A broker stand-in that records what it was asked.
#[derive(Default)]
struct Calls {
    egress: Vec<(String, String)>,
}

impl Host for Calls {
    fn egress_get(&mut self, host: &str, path: &str) -> Result<Vec<u8>, ReasonCode> {
        self.egress.push((host.into(), path.into()));
        Ok(b"hello".to_vec())
    }
    fn tool_call(&mut self, _: &str, _: &str) -> Result<String, ReasonCode> {
        Err(codes::not_granted())
    }
    fn forge_act(&mut self, _: &str, _: &str) -> Result<String, ReasonCode> {
        Err(codes::not_granted())
    }
}

fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.map(String::from).into()
}

fn hosts<const N: usize>(names: [&str; N]) -> Grant {
    Grant {
        egress_hosts: set(names),
        ..Grant::none()
    }
}

// A helper, not a test, so clippy's allowance for tests does not reach it: text that does not
// parse is a bug in the test.
#[allow(clippy::unwrap_used)]
fn component(text: &str) -> Vec<u8> {
    wat::parse_str(text).unwrap()
}

/// Linear memory and a bump allocator, instantiated first so the import can be lowered into it.
const MEM: &str = r#"
  (core module $Mem
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $p i32)
      (local.set $p (i32.and
        (i32.add (global.get $bump) (i32.sub (local.get 2) (i32.const 1)))
        (i32.xor (i32.sub (local.get 2) (i32.const 1)) (i32.const -1))))
      (global.set $bump (i32.add (local.get $p) (local.get 3)))
      (local.get $p)))
  (core instance $mem (instantiate $Mem))"#;

/// The entry export, lifting core function `run` of instance `$m`.
const ENTRY: &str = r#"
  (func $run (param "input" string) (result (result string (error string)))
    (canon lift (core func $m "run") (memory (core memory $mem "memory")) (realloc (core func $mem "realloc"))))
  (instance $entry (export "run" (func $run)))
  (export "hyalos:plugin/run@1.0.0" (instance $entry))"#;

/// A plugin whose `run` asks the egress capability for `input` as the host and path `/`, and
/// returns the body as its answer, or the error it was given.
fn fetcher() -> Vec<u8> {
    component(&format!(
        r#"(component
  (import "{egress}" (instance $egress
    (export "get" (func (param "host" string) (param "path" string)
        (result (result (list u8) (error string)))))))
  {MEM}
  (alias export $egress "get" (func $get))
  (core func $get_lowered (canon lower (func $get) (memory (core memory $mem "memory")) (realloc (core func $mem "realloc"))))
  (core module $M
    (import "host" "get" (func $get (param i32 i32 i32 i32 i32)))
    (import "mem" "memory" (memory 1))
    (data (i32.const 16) "/")
    (func (export "run") (param i32 i32) (result i32)
      (call $get (local.get 0) (local.get 1) (i32.const 16) (i32.const 1) (i32.const 64))
      (i32.const 64)))
  (core instance $m (instantiate $M
    (with "host" (instance (export "get" (func $get_lowered))))
    (with "mem" (instance $mem))))
  {ENTRY}
)"#,
        egress = interface::EGRESS,
    ))
}

#[test]
fn a_component_importing_a_capability_it_was_not_granted_fails_to_instantiate() {
    let runtime = Runtime::new().unwrap();
    let plugin = runtime.load(&fetcher()).unwrap();
    // Granted nothing, or only other capabilities: egress is not linked, so the import is undeclared.
    for grant in [
        Grant::none(),
        Grant {
            tools: set(["search"]),
            actions: set(["read"]),
            data_flows: set(["job->web"]),
            ..Grant::none()
        },
    ] {
        assert_eq!(
            runtime.instantiate(&plugin, &grant, Calls::default()).err(),
            Some(codes::undeclared_import())
        );
    }
    // Granted a host, it instantiates.
    assert!(
        runtime
            .instantiate(&plugin, &hosts(["docs.rs"]), Calls::default())
            .is_ok()
    );
}

#[test]
fn an_import_outside_the_capability_interfaces_is_never_linked() {
    let runtime = Runtime::new().unwrap();
    let every = Grant {
        actions: set(["read"]),
        egress_hosts: set(["docs.rs"]),
        tools: set(["search"]),
        data_flows: set(["job->web"]),
    };
    for name in [
        "wasi:filesystem/types@0.2.0",
        "hyalos:capability/shell@1.0.0",
        "hyalos:capability/egress@2.0.0",
    ] {
        let plugin = runtime
            .load(&component(&format!(
                r#"(component (import "{name}" (instance (export "f" (func)))))"#
            )))
            .unwrap();
        assert_eq!(
            runtime.instantiate(&plugin, &every, Calls::default()).err(),
            Some(codes::undeclared_import()),
            "{name}"
        );
    }
    // The right name with another type is refused too.
    let wrong = runtime
        .load(&component(&format!(
            r#"(component (import "{}" (instance (export "get" (func (param "x" u32))))))"#,
            interface::EGRESS
        )))
        .unwrap();
    assert_eq!(
        runtime.instantiate(&wrong, &every, Calls::default()).err(),
        Some(codes::bad_import())
    );
    // A component with no entry instantiates but cannot be run.
    let bare = runtime.load(&component("(component)")).unwrap();
    assert_eq!(
        runtime.instantiate(&bare, &every, Calls::default()).err(),
        Some(codes::no_entry())
    );
}

#[test]
fn a_call_reaches_the_host_only_for_a_granted_name() {
    let runtime = Runtime::new().unwrap();
    let plugin = runtime.load(&fetcher()).unwrap();
    let grant = hosts(["docs.rs"]);
    let mut running = runtime
        .instantiate(&plugin, &grant, Calls::default())
        .unwrap();
    let answer = running.run("docs.rs").unwrap();
    assert_eq!(answer, Ok(PluginText::granted("hello", grant.clone())));
    // A host outside the grant is refused to the plugin, and the broker never hears of it.
    let refused = running.run("exfil.example").unwrap();
    assert_eq!(
        refused,
        Err(PluginText::granted("capability.not_granted", grant.clone()))
    );
    assert_eq!(
        running.host().egress,
        [("docs.rs".to_string(), "/".to_string())]
    );
}

#[test]
fn a_plugin_that_never_stops_runs_out_of_fuel() {
    let runtime = Runtime::new().unwrap();
    let plugin = runtime
        .load(&component(&format!(
            r#"(component
  {MEM}
  (core module $M
    (func (export "run") (param i32 i32) (result i32)
      (loop $l (br $l))
      (unreachable)))
  (core instance $m (instantiate $M))
  {ENTRY}
)"#
        )))
        .unwrap();
    let mut running = runtime
        .instantiate(&plugin, &Grant::none(), Calls::default())
        .unwrap();
    assert_eq!(running.run("x").err(), Some(codes::out_of_fuel()));
}

#[test]
fn only_a_component_is_loaded() {
    let runtime = Runtime::new().unwrap();
    for bytes in [
        b"not wasm".to_vec(),
        Vec::new(),
        component("(module (func (export \"run\")))"),
    ] {
        assert_eq!(runtime.load(&bytes).err(), Some(codes::invalid()));
    }
}

#[test]
fn a_plugin_cannot_grow_past_its_memory() {
    let runtime = Runtime::new().unwrap();
    // 2,000 pages of 64 KiB is about 125 MiB, past the 64 MiB an instance may hold.
    let plugin = runtime
        .load(&component(&format!(
            r#"(component
  {MEM}
  (core module $M
    (import "mem" "memory" (memory 1))
    (func (export "run") (param i32 i32) (result i32)
      (drop (memory.grow (i32.const 2000)))
      (i32.const 64)))
  (core instance $m (instantiate $M (with "mem" (instance $mem))))
  {ENTRY}
)"#
        )))
        .unwrap();
    let mut running = runtime
        .instantiate(&plugin, &Grant::none(), Calls::default())
        .unwrap();
    assert_eq!(running.run("x").err(), Some(codes::trapped()));
}
