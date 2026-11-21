//! The plugin host (S6.1, ADR-0087): a component reaches only the host imports its grant links, and
//! each call is checked against the granted names before the broker is asked.

use std::collections::BTreeSet;

use hyalos_plugin::{Host, Runtime, codes, interface};
use hyalos_types::{Grant, PluginText, ReasonCode};

/// A broker stand-in that records what it was asked.
#[derive(Default)]
struct Calls {
    egress: Vec<(String, String)>,
    tools: Vec<String>,
}

impl Host for Calls {
    fn egress_get(&mut self, host: &str, path: &str) -> Result<Vec<u8>, ReasonCode> {
        self.egress.push((host.into(), path.into()));
        Ok(b"hello".to_vec())
    }
    fn tool_call(&mut self, name: &str, _: &str) -> Result<String, ReasonCode> {
        self.tools.push(name.into());
        Ok("ran".into())
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

/// A plugin whose `run` calls `func` of interface `iface` `times` times, with `input` as the first
/// argument and `/` as the second, and returns the last answer as its own. `params` and `ok` give
/// the function's type.
fn caller(iface: &str, func: &str, params: (&str, &str), ok: &str, times: u32) -> Vec<u8> {
    let (a, b) = params;
    component(&format!(
        r#"(component
  (import "{iface}" (instance $cap
    (export "{func}" (func (param "{a}" string) (param "{b}" string)
        (result (result {ok} (error string)))))))
  {MEM}
  (alias export $cap "{func}" (func $f))
  (core func $lowered (canon lower (func $f) (memory (core memory $mem "memory")) (realloc (core func $mem "realloc"))))
  (core module $M
    (import "host" "f" (func $f (param i32 i32 i32 i32 i32)))
    (import "mem" "memory" (memory 1))
    (data (i32.const 16) "/")
    (func (export "run") (param i32 i32) (result i32)
      (local $i i32)
      (loop $l
        (call $f (local.get 0) (local.get 1) (i32.const 16) (i32.const 1) (i32.const 64))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br_if $l (i32.lt_u (local.get $i) (i32.const {times}))))
      (i32.const 64)))
  (core instance $m (instantiate $M
    (with "host" (instance (export "f" (func $lowered))))
    (with "mem" (instance $mem))))
  {ENTRY}
)"#
    ))
}

/// A plugin that asks the egress capability for `input` as the host and `/` as the path, and
/// answers with the body or the error it was given.
fn fetcher() -> Vec<u8> {
    caller(interface::EGRESS, "get", ("host", "path"), "(list u8)", 1)
}

/// A plugin that makes `count` core instances, each with a memory of one page and a table of one
/// element, and grows each memory by `pages` and each table by `elements` when run.
fn grower(count: usize, pages: u32, elements: u32) -> Vec<u8> {
    let instances: String = (0..count)
        .map(|i| format!("(core instance $g{i} (instantiate $G))\n"))
        .collect();
    let imports: String = (0..count)
        .map(|i| format!("(import \"g{i}\" \"grow\" (func))\n"))
        .collect();
    let calls: String = (0..count).map(|i| format!("(call {i})")).collect();
    let withs: String = (0..count)
        .map(|i| format!("(with \"g{i}\" (instance $g{i}))"))
        .collect();
    component(&format!(
        r#"(component
  {MEM}
  (core module $G
    (memory 1)
    (table 1 funcref)
    (func (export "grow")
      (drop (memory.grow (i32.const {pages})))
      (drop (table.grow (ref.null func) (i32.const {elements})))))
  {instances}
  (core module $M
    {imports}
    (func (export "run") (param i32 i32) (result i32) {calls} (i32.const 64)))
  (core instance $m (instantiate $M {withs}))
  {ENTRY}
)"#
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
        // Semver-compatible, so the linker alone would accept it: the exact-name check refuses it.
        "hyalos:capability/egress@1.0.1",
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

    // The same holds for tools.
    let plugin = runtime
        .load(&caller(
            interface::TOOLS,
            "call",
            ("name", "input"),
            "string",
            1,
        ))
        .unwrap();
    let grant = Grant {
        tools: set(["search"]),
        ..Grant::none()
    };
    let mut running = runtime
        .instantiate(&plugin, &grant, Calls::default())
        .unwrap();
    assert_eq!(
        running.run("search").unwrap(),
        Ok(PluginText::granted("ran", grant.clone()))
    );
    assert_eq!(
        running.run("shell").unwrap(),
        Err(PluginText::granted("capability.not_granted", grant.clone()))
    );
    assert_eq!(running.host().tools, ["search"]);
}

#[test]
fn a_run_makes_a_bounded_number_of_host_calls() {
    let runtime = Runtime::new().unwrap();
    let plugin = runtime
        .load(&caller(
            interface::EGRESS,
            "get",
            ("host", "path"),
            "(list u8)",
            hyalos_plugin::CALLS + 44,
        ))
        .unwrap();
    let grant = hosts(["docs.rs"]);
    let mut running = runtime
        .instantiate(&plugin, &grant, Calls::default())
        .unwrap();
    // Past the cap a call is refused, and the host is never asked.
    assert_eq!(
        running.run("docs.rs").unwrap(),
        Err(PluginText::granted("plugin.too_many_calls", grant.clone()))
    );
    assert_eq!(running.host().egress.len(), hyalos_plugin::CALLS as usize);
    // The count starts again with each run.
    let _ = running.run("docs.rs");
    assert_eq!(
        running.host().egress.len(),
        2 * hyalos_plugin::CALLS as usize
    );
}

#[test]
fn an_argument_past_the_cap_traps_before_the_host_is_asked() {
    let runtime = Runtime::new().unwrap();
    let plugin = runtime.load(&fetcher()).unwrap();
    let mut running = runtime
        .instantiate(&plugin, &hosts(["docs.rs"]), Calls::default())
        .unwrap();
    let big = "a".repeat(hyalos_plugin::ARGUMENTS + 1);
    assert_eq!(running.run(&big).err(), Some(codes::trapped()));
    assert!(running.host().egress.is_empty());
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
fn a_plugin_s_memory_is_bounded_across_all_its_memories() {
    let runtime = Runtime::new().unwrap();
    let run = |bytes: Vec<u8>| {
        let plugin = runtime.load(&bytes).unwrap();
        runtime
            .instantiate(&plugin, &Grant::none(), Calls::default())
            .and_then(|mut running| running.run("x").map(|_| ()))
    };
    // One memory growing to about 38 MiB fits the 64 MiB a plugin may hold.
    assert_eq!(run(grower(1, 600, 0)), Ok(()));
    // Two of them do not, though each alone is under the cap.
    assert_eq!(run(grower(2, 600, 0)), Err(codes::trapped()));
    // One memory past the cap on its own.
    assert_eq!(run(grower(1, 2000, 0)), Err(codes::trapped()));
    // A table may grow a little, not without bound.
    assert_eq!(run(grower(1, 0, 100)), Ok(()));
    assert_eq!(run(grower(1, 0, 1 << 30)), Err(codes::trapped()));
    // And a plugin makes only a few instances.
    assert_eq!(run(grower(4, 0, 0)), Ok(()));
    assert_eq!(
        run(grower(hyalos_plugin::INSTANCES, 0, 0)),
        Err(codes::trapped())
    );
}

/// A plugin whose `run` answers `answer` (at most 255 bytes, no `\`), or fails with it.
fn answering(answer: &str, ok: bool) -> Vec<u8> {
    let (len, answer) = (answer.len(), answer.replace('"', "\\22"));
    let discriminant = if ok { r"\00" } else { r"\01" };
    component(&format!(
        r#"(component
  {MEM}
  (core module $M
    (import "mem" "memory" (memory 1))
    (data (i32.const 64) "{discriminant}\00\00\00\80\00\00\00\{len:02x}\00\00\00")
    (data (i32.const 128) "{answer}")
    (func (export "run") (param i32 i32) (result i32) (i32.const 64)))
  (core instance $m (instantiate $M (with "mem" (instance $mem))))
  {ENTRY}
)"#
    ))
}

// A helper, not a test: a name that does not parse is a bug in the test.
#[allow(clippy::unwrap_used)]
fn with_component(bytes: Vec<u8>) -> hyalos_plugin::skill::Skill {
    hyalos_plugin::skill::Skill {
        name: hyalos_types::PluginName::new("risk-score").unwrap(),
        version: None,
        asks: hyalos_types::Capabilities::none(),
        mcp: None,
        files: [(
            hyalos_plugin::skill::COMPONENT_FILE.to_string(),
            hyalos_plugin::skill::SkillFile {
                bytes,
                executable: false,
            },
        )]
        .into(),
    }
}

/// S6.5 (ADR-0102): a deterministic step runs the plugin's component with no capability, and only
/// typed values that fit the step's output come out of it.
#[test]
fn a_deterministic_step_runs_the_component_with_no_capability() {
    use hyalos_plugin::run_step;
    use hyalos_types::workflow::{Field, Output, TypedValue};
    let runtime = Runtime::new().unwrap();
    let output: Output = [("low".to_string(), Field::Bool)].into();
    let code = |c: &str| ReasonCode::new(c).unwrap();
    // Its answer, read as the step's fields.
    let skill = with_component(answering(r#"{"low": true}"#, true));
    assert_eq!(
        run_step(&runtime, &skill, "{}", &output),
        Ok(vec![TypedValue::Bool(true)])
    );
    // An answer that does not fit, or the plugin's own error, is a code and nothing else.
    let skill = with_component(answering(r#"{"low": 3}"#, true));
    assert_eq!(
        run_step(&runtime, &skill, "{}", &output),
        Err(code("plugin.answer_does_not_fit"))
    );
    let skill = with_component(answering("no", false));
    assert_eq!(
        run_step(&runtime, &skill, "{}", &output),
        Err(codes::failed())
    );
    // A component that imports a capability never instantiates here, whatever it was granted.
    assert_eq!(
        run_step(&runtime, &with_component(fetcher()), "{}", &output),
        Err(codes::undeclared_import())
    );
    // A plugin with no component cannot be a step.
    let mut none = with_component(Vec::new());
    none.files.clear();
    assert_eq!(
        run_step(&runtime, &none, "{}", &output),
        Err(codes::no_component())
    );
}
