//! The inputs file, `hyalos.inputs/2`: every record length-framed, so a record may hold any bytes
//! (Slice 2 of `docs/plans/hyalos-kernel-capsules`, its design §2.1, ADR-0138).
//!
//! ```text
//! magic   16 bytes   "hyalos.inputs/2\n"
//! count   u32 BE     number of records, 0..=4096
//! record  repeated count times: length u32 BE, 0..=RECORD_MAX, then that many bytes
//! end     the file ends exactly here
//! ```
//!
//! Every command reads this form only. `frame` writes it. A version 4 run of a default command
//! also holds each record to the `hyalos.abi/1` input rule (`v4_inputs`).

use crate::{Reason, kvm::RECORD_MAX};
use std::{fmt, fs};

const MAGIC: &[u8; 16] = b"hyalos.inputs/2\n";
/// The most records one file frames.
pub(crate) const MAX_RECORDS: usize = 4096;

/// Why an inputs file gave no records. Neither case carries an input byte.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadError {
    /// The file cannot be read: a usage error, with its fixed message.
    Usage(String),
    /// The file is not a well-formed framed file, or a record breaks the version 4 input rule.
    BadInputs,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Usage(message) => f.write_str(message),
            ReadError::BadInputs => write!(f, "{:?}", Reason::BadInputs),
        }
    }
}

impl From<ReadError> for String {
    fn from(error: ReadError) -> String {
        error.to_string()
    }
}

/// The records of a framed inputs file, every record whole, empty ones kept.
pub(crate) fn read_inputs(path: &str) -> Result<Vec<Vec<u8>>, ReadError> {
    let bytes = fs::read(path).map_err(|_| ReadError::Usage("cannot read inputs".into()))?;
    parse(&bytes).ok_or(ReadError::BadInputs)
}

/// The records of a framed file's bytes, or `None` for anything but exactly that layout.
pub(crate) fn parse(bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
    fn take_u32(bytes: &[u8]) -> Option<(usize, &[u8])> {
        let (head, rest) = bytes.split_first_chunk::<4>()?;
        Some((u32::from_be_bytes(*head) as usize, rest))
    }
    let (count, mut rest) = take_u32(bytes.strip_prefix(MAGIC)?)?;
    if count > MAX_RECORDS {
        return None;
    }
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let (len, after) = take_u32(rest)?;
        if len as u64 > RECORD_MAX || after.len() < len {
            return None;
        }
        let (record, after) = after.split_at(len);
        records.push(record.to_vec());
        rest = after;
    }
    rest.is_empty().then_some(records)
}

/// The framed bytes of these records. The caller keeps to `MAX_RECORDS` and `RECORD_MAX`.
pub(crate) fn frame(records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(records.len() as u32).to_be_bytes());
    for record in records {
        out.extend_from_slice(&(record.len() as u32).to_be_bytes());
        out.extend_from_slice(record);
    }
    out
}

/// A text file's non-empty lines, each one record: the form `frame --lines` reads.
pub(crate) fn lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// `frame <out> <file>... [--lines <file>]`: each positional file is one whole record, in order,
/// then each non-empty line of the `--lines` file. A missing file, too many records or a record
/// too long to frame is a usage error, and nothing is written.
pub(crate) fn command(args: &[String]) -> Result<bool, String> {
    const USAGE: &str = "usage: frame <out> <file>... [--lines <file>]";
    let (out, files) = args.split_first().ok_or(USAGE)?;
    let (whole, by_line) = match files.iter().position(|a| a == "--lines") {
        Some(at) if at + 2 == files.len() => (&files[..at], Some(&files[at + 1])),
        Some(_) => return Err(USAGE.into()),
        None => (files, None),
    };
    let read = |path: &String| fs::read(path).map_err(|_| "cannot read an input file".to_string());
    let mut records = whole.iter().map(read).collect::<Result<Vec<_>, _>>()?;
    if let Some(path) = by_line {
        records.extend(lines(&read(path)?));
    }
    if records.len() > MAX_RECORDS {
        return Err(format!("more than {MAX_RECORDS} records"));
    }
    if records.iter().any(|r| r.len() as u64 > RECORD_MAX) {
        return Err(format!("a record is over {RECORD_MAX} bytes"));
    }
    fs::write(out, frame(&records)).map_err(|_| "cannot write")?;
    println!("framed: {} records", records.len());
    Ok(true)
}

/// The records a version 4 run of a default command may start with: each one line of UTF-8, not
/// empty and with no newline, as `ABI.md`'s `hyalos.abi/1` section says. Anything else is
/// `BadInputs`, whatever form the file took (ADR-0138).
pub(crate) fn v4_inputs(records: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, ReadError> {
    let one_line = |r: &Vec<u8>| !r.is_empty() && !r.contains(&b'\n') && str::from_utf8(r).is_ok();
    if records.iter().all(one_line) {
        Ok(records)
    } else {
        Err(ReadError::BadInputs)
    }
}

/// A framed file read for a version 4 run of a default command.
pub(crate) fn read_v4_inputs(path: &str) -> Result<Vec<Vec<u8>>, ReadError> {
    v4_inputs(read_inputs(path)?)
}
