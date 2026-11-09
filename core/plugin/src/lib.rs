//! The plugin runtime (ADR-0006, ADR-0007, ADR-0086, ADR-0087): hosting a plugin's WASM component
//! in Wasmtime with its capabilities as host imports (S6.1), installing skills and plugins (S6.2,
//! S6.7), verifying a release's Sigstore bundle (S7.12) and checking for updates (S7.10).
//!
//! A plugin runs in the broker process (ADR-0087), which already parses untrusted input and enforces
//! every capability (ADR-0002). It gets no WASI: the world is reachable only through the
//! `hyalos:capability` interfaces, and only those its grant covers are linked. A component that
//! imports anything else fails to instantiate. Every call is checked against the granted names
//! before it reaches the [`Host`], the broker's gates, so a plugin can never do more than its grant
//! and the job's manifest both allow. What a plugin returns is plugin-authored text, carrying its
//! grant (ADR-0066). Errors are reason codes: nothing a plugin wrote reaches one.
//!
//! Layout rule (ADR-0086): this crate depends only on `hyalos-types` among the workspace crates, and
//! only the `hyalos` crate depends on it.

use std::collections::BTreeSet;

use hyalos_types::{Grant, PluginText, ReasonCode};
use wasmtime::component::{Component, Linker, TypedFunc};
use wasmtime::{Config, Engine, Store, StoreContextMut, StoreLimits, StoreLimitsBuilder, Trap};

/// The host imports, one interface per capability a plugin can call (ADR-0087). Versioned with the
/// capability vocabulary (ADR-0085).
pub mod interface {
    /// `get: func(host: string, path: string) -> result<list<u8>, string>`, for `egress_hosts`.
    pub const EGRESS: &str = "hyalos:capability/egress@1.0.0";
    /// `call: func(name: string, input: string) -> result<string, string>`, for `tools`.
    pub const TOOLS: &str = "hyalos:capability/tools@1.0.0";
    /// `act: func(action: string, input: string) -> result<string, string>`, for `actions`.
    pub const FORGE: &str = "hyalos:capability/forge@1.0.0";
    /// What a plugin exports: `run: func(input: string) -> result<string, string>`.
    pub const ENTRY: &str = "hyalos:plugin/run@1.0.0";
}

/// The fuel one instantiation or one call may burn before it is stopped: about a second of work.
pub const FUEL: u64 = 2_000_000_000;

/// The most linear memory an instance may hold.
pub const MEMORY: usize = 64 << 20;

/// The reason codes a plugin's run ends with or a call is refused with.
pub mod codes {
    use hyalos_types::ReasonCode;

    fn code(text: &str) -> ReasonCode {
        ReasonCode::new(text).unwrap_or_else(ReasonCode::unavailable)
    }

    /// The bytes are not a WASM component this runtime can compile.
    pub fn invalid() -> ReasonCode {
        code("plugin.invalid")
    }

    /// The component imports something its grant does not link: an interface for a capability it
    /// was not granted, or anything outside `hyalos:capability`.
    pub fn undeclared_import() -> ReasonCode {
        code("plugin.undeclared_import")
    }

    /// An import has the right name but not the interface's type.
    pub fn bad_import() -> ReasonCode {
        code("plugin.bad_import")
    }

    /// The component does not export `hyalos:plugin/run` with its type.
    pub fn no_entry() -> ReasonCode {
        code("plugin.no_entry")
    }

    /// The plugin used up its fuel.
    pub fn out_of_fuel() -> ReasonCode {
        code("plugin.out_of_fuel")
    }

    /// The plugin trapped, or the runtime failed it.
    pub fn trapped() -> ReasonCode {
        code("plugin.trapped")
    }

    /// The runtime could not be set up.
    pub fn runtime() -> ReasonCode {
        code("plugin.runtime")
    }

    /// The plugin called a capability with a name its grant does not hold. The plugin gets this
    /// code back as the call's error; the host is not called.
    pub fn not_granted() -> ReasonCode {
        code("capability.not_granted")
    }
}

/// What the broker does for a plugin's granted calls. Each argument is plugin-authored, and each
/// call has already been checked against the grant; the broker's own gates still apply.
pub trait Host: Send + 'static {
    fn egress_get(&mut self, host: &str, path: &str) -> Result<Vec<u8>, ReasonCode>;
    fn tool_call(&mut self, name: &str, input: &str) -> Result<String, ReasonCode>;
    fn forge_act(&mut self, action: &str, input: &str) -> Result<String, ReasonCode>;
}

/// The engine that compiles and runs plugins. One per broker.
pub struct Runtime {
    engine: Engine,
}

/// A compiled plugin, not yet given a grant.
pub struct Plugin(Component);

struct State<H> {
    grant: Grant,
    host: H,
    limits: StoreLimits,
}

/// A plugin instantiated with its grant, ready to run.
pub struct Running<H: Host> {
    store: Store<State<H>>,
    run: TypedFunc<(String,), (Result<String, String>,)>,
}

impl Runtime {
    pub fn new() -> Result<Self, ReasonCode> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(|_| codes::runtime())?;
        Ok(Self { engine })
    }

    /// Compiles `bytes`, which must be a component.
    pub fn load(&self, bytes: &[u8]) -> Result<Plugin, ReasonCode> {
        Component::from_binary(&self.engine, bytes)
            .map(Plugin)
            .map_err(|_| codes::invalid())
    }

    /// Instantiates `plugin` with only the host imports `grant` covers. Refused with
    /// `plugin.undeclared_import` when the component imports anything else.
    pub fn instantiate<H: Host>(
        &self,
        plugin: &Plugin,
        grant: &Grant,
        host: H,
    ) -> Result<Running<H>, ReasonCode> {
        let mut linker = Linker::new(&self.engine);
        let linked = link(&mut linker, grant).map_err(|_| codes::runtime())?;
        let ty = plugin.0.component_type();
        if !ty
            .imports(&self.engine)
            .all(|(name, _)| linked.contains(name))
        {
            return Err(codes::undeclared_import());
        }
        let state = State {
            grant: grant.clone(),
            host,
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY)
                .trap_on_grow_failure(true)
                .build(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(FUEL).map_err(|_| codes::runtime())?;
        let pre = linker
            .instantiate_pre(&plugin.0)
            .map_err(|_| codes::bad_import())?;
        let instance = pre.instantiate(&mut store).map_err(|e| failure(&e))?;
        let entry = instance
            .get_export_index(&mut store, None, interface::ENTRY)
            .and_then(|i| instance.get_export_index(&mut store, Some(&i), "run"))
            .ok_or_else(codes::no_entry)?;
        let run = instance
            .get_typed_func(&mut store, entry)
            .map_err(|_| codes::no_entry())?;
        Ok(Running { store, run })
    }
}

impl<H: Host> Running<H> {
    /// Runs the plugin's entry on `input`, with fresh fuel. Its answer, either way, is
    /// plugin-authored text bounded by its grant.
    pub fn run(&mut self, input: &str) -> Result<Result<PluginText, PluginText>, ReasonCode> {
        self.store.set_fuel(FUEL).map_err(|_| codes::runtime())?;
        let (answer,) = self
            .run
            .call(&mut self.store, (input.to_string(),))
            .map_err(|e| failure(&e))?;
        let grant = self.store.data().grant.clone();
        Ok(answer
            .map(|t| PluginText::granted(t, grant.clone()))
            .map_err(|t| PluginText::granted(t, grant)))
    }

    /// The host, for the broker to read what its calls did.
    pub fn host(&self) -> &H {
        &self.store.data().host
    }
}

/// The code for a failed instantiation or call. Wasmtime's message is never kept: it can name what
/// the plugin wrote.
fn failure(error: &wasmtime::Error) -> ReasonCode {
    match error.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => codes::out_of_fuel(),
        _ => codes::trapped(),
    }
}

/// Links the interfaces `grant` covers and returns their names. A call names what it acts on, and a
/// name the grant does not hold is answered `capability.not_granted` without calling the host.
fn link<H: Host>(
    linker: &mut Linker<State<H>>,
    grant: &Grant,
) -> wasmtime::Result<BTreeSet<&'static str>> {
    fn answer<V>(out: Result<V, ReasonCode>) -> wasmtime::Result<(Result<V, String>,)> {
        Ok((out.map_err(|r| r.as_str().to_string()),))
    }
    let mut linked = BTreeSet::new();
    if !grant.egress_hosts.is_empty() {
        linker.instance(interface::EGRESS)?.func_wrap(
            "get",
            |mut cx: StoreContextMut<'_, State<H>>, (host, path): (String, String)| {
                let s = cx.data_mut();
                answer(match s.grant.egress_hosts.contains(&host) {
                    true => s.host.egress_get(&host, &path),
                    false => Err(codes::not_granted()),
                })
            },
        )?;
        linked.insert(interface::EGRESS);
    }
    if !grant.tools.is_empty() {
        linker.instance(interface::TOOLS)?.func_wrap(
            "call",
            |mut cx: StoreContextMut<'_, State<H>>, (name, input): (String, String)| {
                let s = cx.data_mut();
                answer(match s.grant.tools.contains(&name) {
                    true => s.host.tool_call(&name, &input),
                    false => Err(codes::not_granted()),
                })
            },
        )?;
        linked.insert(interface::TOOLS);
    }
    if !grant.actions.is_empty() {
        linker.instance(interface::FORGE)?.func_wrap(
            "act",
            |mut cx: StoreContextMut<'_, State<H>>, (action, input): (String, String)| {
                let s = cx.data_mut();
                answer(match s.grant.actions.contains(&action) {
                    true => s.host.forge_act(&action, &input),
                    false => Err(codes::not_granted()),
                })
            },
        )?;
        linked.insert(interface::FORGE);
    }
    Ok(linked)
}
