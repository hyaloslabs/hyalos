//! Reading the operator's passphrase (ADR-0070).
//!
//! It is read from the controlling terminal, `/dev/tty`, with echo off, so it is typed by the person
//! at the terminal and is never in an argument, the environment or a file. `--passphrase-stdin`
//! reads one line from standard input instead, for a script the operator runs and for tests. The
//! driving agent may run commands, so neither way is a secret channel in itself: what stops the
//! agent approving is that it does not know the passphrase.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};

use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;
use zeroize::Zeroizing;

/// The flag that reads the passphrase from standard input.
pub const STDIN_FLAG: &str = "--passphrase-stdin";

/// No passphrase is longer than this; reading stops here.
const LIMIT: u64 = 1024;

/// Where the passphrase is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Terminal,
    Stdin,
}

/// `args` without `--passphrase-stdin`, and where the passphrase is to be read from.
pub fn source(args: &[String]) -> (Vec<String>, Source) {
    let rest: Vec<String> = args.iter().filter(|a| *a != STDIN_FLAG).cloned().collect();
    let source = if rest.len() == args.len() {
        Source::Terminal
    } else {
        Source::Stdin
    };
    (rest, source)
}

/// Reads one passphrase, prompting with `prompt` on the terminal.
pub fn read(source: Source, prompt: &str) -> Result<Zeroizing<String>, String> {
    let line = match source {
        Source::Stdin => line(std::io::stdin().lock())?,
        Source::Terminal => from_terminal(prompt)?,
    };
    if line.is_empty() {
        return Err("no passphrase was given".into());
    }
    Ok(line)
}

fn from_terminal(prompt: &str) -> Result<Zeroizing<String>, String> {
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| {
            format!("cannot open the terminal to ask for the passphrase ({e}); run this at a terminal, or pass {STDIN_FLAG}")
        })?;
    let saved = tcgetattr(&tty).map_err(|e| format!("cannot read the terminal's settings: {e}"))?;
    let mut quiet = saved.clone();
    quiet.local_modes.remove(LocalModes::ECHO);
    (&tty)
        .write_all(prompt.as_bytes())
        .map_err(|e| format!("cannot write to the terminal: {e}"))?;
    // A signal while echo is off would leave the terminal silent: take them for the read, put the
    // terminal back, then exit as the signal would have.
    let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP, SIGQUIT])
        .map_err(|e| format!("cannot take signals while reading the passphrase: {e}"))?;
    let handle = signals.handle();
    let (back, settings) = (
        tty.try_clone()
            .map_err(|e| format!("cannot use the terminal: {e}"))?,
        saved.clone(),
    );
    let watcher = std::thread::spawn(move || {
        if signals.forever().next().is_some() {
            let _ = tcsetattr(&back, OptionalActions::Now, &settings);
            let _ = (&back).write_all(b"\n");
            std::process::exit(130);
        }
    });
    let quieted = tcsetattr(&tty, OptionalActions::Flush, &quiet)
        .map_err(|e| format!("cannot turn the terminal's echo off: {e}"));
    let read = quieted.and_then(|()| line(&tty));
    // Echo comes back whatever the read did.
    let restored = tcsetattr(&tty, OptionalActions::Now, &saved);
    handle.close();
    let _ = watcher.join();
    let _ = (&tty).write_all(b"\n");
    restored.map_err(|e| format!("cannot turn the terminal's echo back on: {e}"))?;
    read
}

/// One line, without its line ending, at most [`LIMIT`] bytes.
fn line(input: impl Read) -> Result<Zeroizing<String>, String> {
    let mut text = Zeroizing::new(String::new());
    BufReader::new(input.take(LIMIT))
        .read_line(&mut text)
        .map_err(|e| format!("cannot read the passphrase: {e}"))?;
    let end = text.trim_end_matches(['\n', '\r']).len();
    text.truncate(end);
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_is_taken_out_wherever_it_is() {
        let args: Vec<String> = ["issue", "--passphrase-stdin", "claude"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            source(&args),
            (
                vec!["issue".to_string(), "claude".to_string()],
                Source::Stdin
            )
        );
        assert_eq!(source(&args[..1]).1, Source::Terminal);
    }

    #[test]
    fn a_line_is_read_without_its_ending_and_no_further() {
        assert_eq!(
            line(&b"secret words\r\nmore"[..]).unwrap().as_str(),
            "secret words"
        );
        assert_eq!(line(&b"last"[..]).unwrap().as_str(), "last");
        let long = vec![b'a'; 4096];
        assert_eq!(line(&long[..]).unwrap().len(), LIMIT as usize);
    }
}
