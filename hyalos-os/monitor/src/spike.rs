//! The Python guest spike's commands (ADR-0136, `docs/plans/hyalos-kernel-python`), compiled only
//! with the `spike-contract` feature. A spike run's input records are the contract, the SDK and the
//! script, each a whole file with its newlines, then the data file's lines as `run` reads them. The
//! audit reads the contract from record 0 and holds every call to it, on either backend.

use crate::{
    Backend, CURRENT_VERSION, Mode, load_backend, on_backend, read_inputs, sha, task_v4, unhex,
};
use hyalos_audit::Contract;
use hyalos_profile::Profile;
use std::{env, fs, time::Instant};

/// Names the spike profile a spike run uses, `hyalos.spike/1` when it is not set.
pub(crate) const PROFILE_VARIABLE: &str = "HYALOS_SPIKE_PROFILE";

const RUN_USAGE: &str =
    "usage: spike-run <guest.wasm> <contract> <sdk.py> <script.py> <data file> <record out> [seed]";
const TASK_USAGE: &str =
    "usage: spike-task <guest.wasm> <contract> <sdk.py> <script.py> <data file>";

/// The spike profile to run under. A name that is not a known spike profile is refused, since a
/// profile without a contract would run records 0 to 2 unchecked.
pub(crate) fn profile() -> Result<&'static Profile, String> {
    match env::var_os(PROFILE_VARIABLE) {
        None => Ok(&hyalos_profile::SPIKE_1),
        Some(name) => name
            .to_str()
            .and_then(hyalos_profile::lookup)
            .filter(|profile| profile.enforces_contract())
            .ok_or_else(|| format!("{PROFILE_VARIABLE} does not name a known spike profile")),
    }
}

/// A spike run's input records. An SDK, script or data file that cannot be read, or an empty SDK
/// or script, is a usage error. An unreadable contract is not: it is no contract, which the run
/// refuses with `BadContract` before its guest loads.
pub(crate) fn inputs(
    contract: &str,
    sdk: &str,
    script: &str,
    data: &str,
) -> Result<Vec<Vec<u8>>, String> {
    let whole = |path: &str, what: &str| -> Result<Vec<u8>, String> {
        let bytes = fs::read(path).map_err(|_| format!("cannot read the {what}"))?;
        if bytes.is_empty() {
            return Err(format!("the {what} is empty: a spike run needs one"));
        }
        Ok(bytes)
    };
    let sdk = whole(sdk, "SDK")?;
    let script = whole(script, "script")?;
    let data = read_inputs(data).map_err(|_| "cannot read the data file".to_string())?;
    let contract = fs::read(contract).unwrap_or_default();
    Ok([vec![contract, sdk, script], data].concat())
}

/// A seed of decimal digits only, as a `u64`.
fn seed(text: &str) -> Result<u64, String> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    digits
        .then(|| text.parse().ok())
        .flatten()
        .ok_or_else(|| "the seed is not a decimal number".to_string())
}

/// `spike-run`: one run under a spike profile, its record written as `run` writes one, then the
/// `ok:` line and the spike's timing, which is printed and never recorded.
pub(crate) fn run(args: &[String]) -> Result<bool, String> {
    let start = Instant::now();
    let [guest, contract, sdk, script, data, out, rest @ ..] = args else {
        return Err(RUN_USAGE.into());
    };
    let seed = match rest {
        [] => 0,
        [text] => seed(text)?,
        _ => return Err(RUN_USAGE.into()),
    };
    let profile = profile()?;
    let backend = load_backend()?;
    let guest = fs::read(guest).map_err(|_| "cannot read guest")?;
    let inputs = inputs(contract, sdk, script, data)?;
    let mut first_call = None;
    let t = hyalos_audit::execute_with(
        CURRENT_VERSION,
        Some(profile.name),
        &guest,
        inputs,
        Mode::Record { seed },
        |guest, host, profile| {
            let ran = on_backend(&backend, guest, host, profile);
            if let Ok((host, ..)) = &ran {
                first_call = host.first_call.get();
            }
            ran
        },
    )
    .map_err(|r| format!("{r:?}"))?;
    let run_time = start.elapsed();
    fs::write(out, serde_json::to_vec_pretty(&t).map_err(|_| "encode")?)
        .map_err(|_| "cannot write")?;
    println!(
        "ok: {} inputs, {} inferences, {} effect, fuel {}",
        t.inputs_hex.len(),
        t.answers.len(),
        t.effects.len(),
        t.fuel_used
    );
    // A guest that never called the host before it returned started when it ended.
    let cold_start = first_call.map_or(run_time, |at| at.duration_since(start));
    println!(
        "timing: cold_start {} ms, run_time {} ms, backend {}",
        cold_start.as_millis(),
        run_time.as_millis(),
        match backend {
            Backend::Process => "process",
            Backend::Kvm { .. } => "kvm",
        }
    );
    for e in &t.effects {
        println!(
            "{}",
            String::from_utf8_lossy(&unhex(&e.body_hex).unwrap_or_default())
        );
    }
    Ok(true)
}

/// `spike-task`: the task identity `spike-run` would record for these files, without running.
pub(crate) fn task(args: &[String]) -> Result<bool, String> {
    let [guest, contract, sdk, script, data] = args else {
        return Err(TASK_USAGE.into());
    };
    let profile = profile()?;
    let guest = fs::read(guest).map_err(|_| "cannot read guest")?;
    let inputs = inputs(contract, sdk, script, data)?;
    if inputs.first().and_then(|c| Contract::parse(c)).is_none() {
        return Err("BadContract".into());
    }
    println!(
        "{}",
        task_v4(profile.name, &sha(&guest), &inputs).ok_or("the guest's hash is unreadable")?
    );
    Ok(true)
}
