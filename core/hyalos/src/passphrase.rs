//! Reading the operator's passphrase (ADR-0070).
//!
//! It is read from the controlling terminal, `/dev/tty`, with echo off, so it is typed by the person
//! at the terminal and is never in an argument, the environment or a file. `--passphrase-stdin`
//! reads one line from standard input instead, for a script the operator runs and for tests. The
//! driving agent may run commands, so neither way is a secret channel in itself: what stops the
//! agent approving is that it does not know the passphrase.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};

use rustix::termios::{LocalModes, OptionalActions, SpecialCodeIndex, tcgetattr, tcsetattr};
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
    // Echo off, and keys read one at a time with no signals from them: Ctrl-C is a key that cancels
    // the read, so no signal can end the process while the terminal is silent, and nothing about
    // signals is changed for the rest of the command.
    let mut quiet = saved.clone();
    quiet
        .local_modes
        .remove(LocalModes::ECHO | LocalModes::ICANON | LocalModes::ISIG);
    quiet.special_codes[SpecialCodeIndex::VMIN] = 1;
    quiet.special_codes[SpecialCodeIndex::VTIME] = 0;
    (&tty)
        .write_all(prompt.as_bytes())
        .map_err(|e| format!("cannot write to the terminal: {e}"))?;
    let quieted = tcsetattr(&tty, OptionalActions::Flush, &quiet)
        .map_err(|e| format!("cannot turn the terminal's echo off: {e}"));
    let read = quieted.and_then(|()| keys(&tty));
    // Echo comes back whatever the read did.
    let restored = tcsetattr(&tty, OptionalActions::Now, &saved);
    let _ = (&tty).write_all(b"\n");
    restored.map_err(|e| format!("cannot turn the terminal's echo back on: {e}"))?;
    read
}

/// A passphrase typed key by key: Enter ends it, Backspace takes back a character, Ctrl-U clears
/// it, Ctrl-C, Ctrl-\\ and Ctrl-Z cancel, Ctrl-D cancels if nothing was typed. At most [`LIMIT`]
/// bytes.
fn keys(input: impl Read) -> Result<Zeroizing<String>, String> {
    // Full size at once, so it never moves and leaves an unwiped copy behind.
    let mut typed = Zeroizing::new(Vec::with_capacity(LIMIT as usize));
    let mut key = [0u8; 1];
    let mut input = input;
    loop {
        let n = input
            .read(&mut key)
            .map_err(|e| format!("cannot read the passphrase: {e}"))?;
        match (n, key[0]) {
            (0, _) | (_, b'\r' | b'\n') => break,
            (_, 0x03 | 0x1c | 0x1a) => return Err("cancelled".into()),
            (_, 0x04) if typed.is_empty() => return Err("cancelled".into()),
            (_, 0x04) => {}
            (_, 0x7f | 0x08) => {
                // Back to the start of the last character.
                while typed.pop().is_some_and(|b| b & 0xc0 == 0x80) {}
            }
            (_, 0x15) => typed.clear(),
            (_, b) if typed.len() < LIMIT as usize => typed.push(b),
            _ => {}
        }
    }
    let text =
        std::str::from_utf8(&typed).map_err(|_| "the passphrase is not UTF-8".to_string())?;
    Ok(Zeroizing::new(text.to_string()))
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

    #[test]
    fn keys_end_take_back_clear_and_cancel() {
        let typed = |b: &[u8]| keys(b).map(|t| t.as_str().to_string());
        assert_eq!(typed(b"secret\r").unwrap(), "secret");
        assert_eq!(typed(b"secrex\x7ft\n").unwrap(), "secret");
        // Backspace takes back a whole character, not one byte of it.
        assert_eq!(typed("caf\u{e9}\x7fe\r".as_bytes()).unwrap(), "cafe");
        assert_eq!(typed(b"wrong\x15right\r").unwrap(), "right");
        assert_eq!(typed(b"half").unwrap(), "half");
        for cancel in [&b"sec\x03ret\r"[..], b"\x1c", b"\x1a", b"\x04"] {
            assert_eq!(typed(cancel), Err("cancelled".to_string()));
        }
        assert_eq!(typed(b"ab\x04c\r").unwrap(), "abc");
        let long = vec![b'a'; 4096];
        assert_eq!(typed(&long).unwrap().len(), LIMIT as usize);
    }
}
