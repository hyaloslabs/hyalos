//! The plugin host, on generated grants, imports and calls (S8.3, ADR-0087, ADR-0109).

use std::collections::BTreeSet;

use hyalos_plugin::{CALLS, Host, Runtime, codes, interface};
use hyalos_props::{ALPHANUMERIC, SeededRng, forall, one_in, pick, seeds, subset, text, within};
use hyalos_types::{Grant, ReasonCode};

/// The broker's side, recording every call it is asked to make.
#[derive(Default)]
struct Broker {
    calls: Vec<(&'static str, String)>,
}

impl Host for Broker {
    fn egress_get(&mut self, host: &str, _: &str) -> Result<Vec<u8>, ReasonCode> {
        self.calls.push(("egress", host.into()));
        Ok(b"body".to_vec())
    }
    fn tool_call(&mut self, name: &str, _: &str) -> Result<String, ReasonCode> {
        self.calls.push(("tools", name.into()));
        Ok("ran".into())
    }
    fn forge_act(&mut self, action: &str, _: &str) -> Result<String, ReasonCode> {
        self.calls.push(("forge", action.into()));
        Ok("done".into())
    }
}

/// A capability interface: its name, its one function, that function's parameters and what it
/// answers, the grant field it is checked against, and what the broker records.
struct Capability {
    interface: &'static str,
    func: &'static str,
    params: (&'static str, &'static str),
    ok: &'static str,
    field: fn(&Grant) -> &BTreeSet<String>,
    field_mut: fn(&mut Grant) -> &mut BTreeSet<String>,
    recorded: &'static str,
}

const CAPABILITIES: [Capability; 3] = [
    Capability {
        interface: interface::EGRESS,
        func: "get",
        params: ("host", "path"),
        ok: "(list u8)",
        field: |g| &g.egress_hosts,
        field_mut: |g| &mut g.egress_hosts,
        recorded: "egress",
    },
    Capability {
        interface: interface::TOOLS,
        func: "call",
        params: ("name", "input"),
        ok: "string",
        field: |g| &g.tools,
        field_mut: |g| &mut g.tools,
        recorded: "tools",
    },
    Capability {
        interface: interface::FORGE,
        func: "act",
        params: ("action", "input"),
        ok: "string",
        field: |g| &g.actions,
        field_mut: |g| &mut g.actions,
        recorded: "forge",
    },
];

// A helper, not a test: text that does not parse is a bug in the suite.
#[allow(clippy::unwrap_used)]
fn component(text: &str) -> Vec<u8> {
    wat::parse_str(text).unwrap()
}

/// Linear memory and a bump allocator, instantiated first so imports can be lowered into it.
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

fn import(c: &Capability) -> String {
    let (a, b) = c.params;
    format!(
        r#"(import "{}" (instance (export "{}" (func (param "{a}" string) (param "{b}" string)
        (result (result {} (error string)))))))"#,
        c.interface, c.func, c.ok
    )
}

/// A plugin that imports each of `imports` and whose `run` does nothing.
fn importer(imports: &[&Capability]) -> Vec<u8> {
    let imports: String = imports.iter().map(|c| import(c)).collect();
    component(&format!(
        r#"(component
  {imports}
  {MEM}
  (core module $M
    (func (export "run") (param i32 i32) (result i32) (i32.const 64)))
  (core instance $m (instantiate $M))
  {ENTRY}
)"#
    ))
}

/// A plugin whose `run` calls `c`'s function `times` times with its input as the name, and answers
/// with the last answer.
fn caller(c: &Capability, times: u32) -> Vec<u8> {
    let (a, b) = c.params;
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
)"#,
        iface = c.interface,
        func = c.func,
        ok = c.ok,
    ))
}

/// A name the fields accept: lowercase letters, digits, dots and dashes.
fn name(rng: &mut SeededRng) -> String {
    let lower: Vec<char> = ALPHANUMERIC
        .iter()
        .copied()
        .filter(|c| !c.is_ascii_uppercase())
        .collect();
    format!(
        "{}{}",
        pick(rng, &lower[..26]),
        text(rng, &[&lower[..], &['.', '-']].concat(), 10)
    )
}

/// A grant with each capability's field empty or holding a few drawn names.
fn grant(rng: &mut SeededRng) -> Grant {
    let mut g = Grant::none();
    for c in &CAPABILITIES {
        if one_in(rng, 3) {
            continue;
        }
        *(c.field_mut)(&mut g) = (0..within(rng, 1..4)).map(|_| name(rng)).collect();
    }
    if one_in(rng, 2) {
        g.data_flows = ["job->web".to_string()].into();
    }
    g
}

/// A name close to `granted` but not in it: another case, a suffix, a prefix, a space, or empty.
fn near_miss(rng: &mut SeededRng, granted: &str) -> String {
    match rng.below(5) {
        0 => granted.to_ascii_uppercase(),
        1 => format!("{granted}.evil"),
        2 => format!("x{granted}"),
        3 => format!("{granted} "),
        _ => String::new(),
    }
}

#[test]
fn a_component_instantiates_exactly_when_its_grant_covers_every_interface_it_imports()
-> Result<(), String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    let (mut linked, mut refused) = (0, 0);
    forall("plugin.linking", seeds(150)?, |rng| {
        let imports: Vec<&Capability> = subset(rng, &[0, 1, 2])
            .into_iter()
            .map(|i| &CAPABILITIES[i])
            .collect();
        let g = grant(rng);
        let plugin = runtime
            .load(&importer(&imports))
            .map_err(|e| e.to_string())?;
        let covered = imports.iter().all(|c| !(c.field)(&g).is_empty());
        let names: Vec<&str> = imports.iter().map(|c| c.interface).collect();
        match (runtime.instantiate(&plugin, &g, Broker::default()), covered) {
            (Ok(_), true) => linked += 1,
            (Err(code), false) if code == codes::undeclared_import() => refused += 1,
            (Ok(_), false) => return Err(format!("imports {names:?} instantiated under {g:?}")),
            (Err(code), _) => {
                return Err(format!("imports {names:?} under {g:?}: refused {code}"));
            }
        }
        Ok(())
    })?;
    assert!(
        linked > 0 && refused > 0,
        "{linked} linked, {refused} refused"
    );
    Ok(())
}

#[test]
fn a_call_reaches_the_broker_exactly_when_its_name_is_granted() -> Result<(), String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    let plugins = CAPABILITIES
        .iter()
        .map(|c| runtime.load(&caller(c, 1)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let (mut reached, mut refused) = (0, 0);
    forall("plugin.calls", seeds(300)?, |rng| {
        let which = rng.below(3) as usize;
        let c = &CAPABILITIES[which];
        let mut g = grant(rng);
        if (c.field)(&g).is_empty() {
            *(c.field_mut)(&mut g) = [name(rng)].into();
        }
        let granted: Vec<String> = (c.field)(&g).iter().cloned().collect();
        let mut running = runtime
            .instantiate(&plugins[which], &g, Broker::default())
            .map_err(|e| e.to_string())?;
        // Several calls on one instance: granted names, near misses and names from other fields.
        let mut expected: Vec<(&'static str, String)> = Vec::new();
        for _ in 0..within(rng, 1..6) {
            let target = pick(rng, &granted).clone();
            let input = match rng.below(3) {
                0 => target,
                1 => near_miss(rng, &target),
                _ => name(rng),
            };
            let allowed = (c.field)(&g).contains(&input);
            let answer = running.run(&input).map_err(|e| e.to_string())?;
            match (answer, allowed) {
                (Ok(_), true) => {
                    expected.push((c.recorded, input));
                    reached += 1;
                }
                (Err(text), false) => {
                    let said = format!("{text:?}");
                    if !said.contains(codes::not_granted().as_str()) {
                        return Err(format!("{input:?} on {}: answered {said}", c.interface));
                    }
                    refused += 1;
                }
                (got, _) => {
                    return Err(format!(
                        "{input:?} on {} under {granted:?}: answered {got:?}",
                        c.interface
                    ));
                }
            }
        }
        if running.host().calls != expected {
            return Err(format!(
                "the broker was asked {:?}, expected {expected:?}",
                running.host().calls
            ));
        }
        Ok(())
    })?;
    assert!(
        reached > 0 && refused > 0,
        "{reached} reached, {refused} refused"
    );
    Ok(())
}

#[test]
fn one_run_asks_the_broker_at_most_the_call_limit() -> Result<(), String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    forall("plugin.call_limit", seeds(12)?, |rng| {
        let which = rng.below(3) as usize;
        let c = &CAPABILITIES[which];
        let times = within(rng, 1..u64::from(CALLS) * 2) as u32;
        let named = name(rng);
        let mut g = Grant::none();
        *(c.field_mut)(&mut g) = [named.clone()].into();
        let plugin = runtime
            .load(&caller(c, times))
            .map_err(|e| e.to_string())?;
        let mut running = runtime
            .instantiate(&plugin, &g, Broker::default())
            .map_err(|e| e.to_string())?;
        // Two runs: the limit is per run, not per instance.
        for run in 0..2 {
            let before = running.host().calls.len();
            let answer = running.run(&named).map_err(|e| e.to_string())?;
            let asked = running.host().calls.len() - before;
            if asked != times.min(CALLS) as usize {
                return Err(format!(
                    "run {run} of {times} calls asked the broker {asked} times"
                ));
            }
            let over = times > CALLS;
            let refused = matches!(&answer, Err(t) if format!("{t:?}").contains(codes::too_many_calls().as_str()));
            if over != refused {
                return Err(format!("run {run} of {times} calls answered {answer:?}"));
            }
        }
        Ok(())
    })
    .map(|_| ())
}
