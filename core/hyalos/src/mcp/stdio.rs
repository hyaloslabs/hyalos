//! stdio (ADR-0067): one JSON-RPC message per line in, one per line out, until the input ends. This
//! is how a client that starts `hyalos mcp` as a command talks to it. Only messages go to the output;
//! anything else the command prints goes to stderr.

use std::io::{self, BufRead, Read, Write};

use serde_json::{Value, json};

use super::{MAX_MESSAGE, Server, Tools, codes};

/// Serves messages from `input` until it ends. A line longer than [`MAX_MESSAGE`] is answered with an
/// invalid-request error and skipped.
pub fn serve<T: Tools>(
    server: &Server<T>,
    mut input: impl BufRead,
    mut output: impl Write,
) -> io::Result<()> {
    loop {
        let mut line = Vec::new();
        let read = Read::by_ref(&mut input)
            .take(MAX_MESSAGE as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(());
        }
        if line.len() > MAX_MESSAGE {
            skip_line(&mut input)?;
            let too_long = json!({"jsonrpc": "2.0", "error": {"code": codes::INVALID_REQUEST, "message": "Message too large"}});
            write_line(&mut output, &too_long)?;
            continue;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let mut failed = None;
        let reply = server.handle(&line, None, &mut |note| {
            if failed.is_none() {
                failed = write_line(&mut output, &note).err();
            }
        });
        if let Some(e) = failed {
            return Err(e);
        }
        if let Some(body) = reply.body {
            write_line(&mut output, &body)?;
        }
    }
}

fn skip_line(input: &mut impl BufRead) -> io::Result<()> {
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(());
        }
        match buf.iter().position(|b| *b == b'\n') {
            Some(i) => {
                input.consume(i + 1);
                return Ok(());
            }
            None => {
                let n = buf.len();
                input.consume(n);
            }
        }
    }
}

/// Compact JSON escapes every line break inside a string, so a message is always one line.
fn write_line(output: &mut impl Write, message: &Value) -> io::Result<()> {
    output.write_all(format!("{message}\n").as_bytes())?;
    output.flush()
}
