//! The content gate on a push (ADR-0054, S3.14b).
//!
//! A push carries its commits and files as a pack: objects compressed with zlib, some stored as deltas
//! against others. The broker inflates every object, resolves every delta against its base in the same
//! pack, and checks what results before the push reaches the forge:
//! - every object, of any type, for a credential the broker holds (`content.credential`);
//! - commit and tag objects, which a reviewer reads as text, and the names in trees, for text that
//!   hides from its reader as well (`content.hidden_text`).
//!
//! File contents are not checked for hidden text: source files and binaries hold such bytes for good
//! reasons. A pack the broker cannot read whole is not checked, and the gate's answer is then unknown,
//! never allow: it is corrupt, too large to inflate, or holds a delta whose base is not in it.

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::inflate_flags::{
    TINFL_FLAG_COMPUTE_ADLER32, TINFL_FLAG_PARSE_ZLIB_HEADER,
    TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
};
use miniz_oxide::inflate::core::{DecompressorOxide, decompress};
use sha2::{Digest as _, Sha256};

use crate::content;

/// The most bytes a pack's objects may come to once inflated and resolved, deltas included.
pub const MAX_INFLATED: usize = 256 * 1024 * 1024;

/// What each object costs against `MAX_INFLATED` however small it inflates to, for what the broker
/// keeps of it and the work of reading it. Without it, millions of empty objects cost nothing
/// (#336).
pub const OBJECT_COST: usize = 64;

/// The most objects a pack may hold. A push of a job's own work has far fewer.
pub const MAX_OBJECTS: u32 = 1 << 20;

/// How many objects are read between looks at whether the job has ended.
const STOP_EVERY: u32 = 1024;

/// Why a pack was not passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// The pack carries something the gate refuses: the reason, and the SHA-256 of the object.
    Refused(&'static str, [u8; 32]),
    /// The pack could not be read whole, so nothing in it was checked: the reason.
    Unchecked(&'static str),
}

const UNREADABLE: Finding = Finding::Unchecked("content.pack_unreadable");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Commit,
    Tree,
    Blob,
    Tag,
}

/// Checks a pack whose object ids are `id_len` bytes (20 for SHA-1, 32 for SHA-256) against the
/// credentials in `secrets`. An empty pack, as a push of no new objects may send, passes.
pub fn check(pack: &[u8], id_len: usize, secrets: &[&str]) -> Result<(), Finding> {
    check_until(pack, id_len, secrets, &|| false)
}

/// [`check`], giving up as unchecked once `stop` says so, as when the job ends mid-check.
pub fn check_until(
    pack: &[u8],
    id_len: usize,
    secrets: &[&str],
    stop: &dyn Fn() -> bool,
) -> Result<(), Finding> {
    if pack.is_empty() {
        return Ok(());
    }
    if pack.get(..4) != Some(b"PACK") {
        return Err(UNREADABLE);
    }
    let word = |at: usize| -> Result<u32, Finding> {
        let bytes = pack.get(at..at + 4).ok_or(UNREADABLE)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    if !matches!(word(4)?, 2 | 3) {
        return Err(UNREADABLE);
    }
    let count = word(8)?;
    if count > MAX_OBJECTS {
        return Err(too_large());
    }
    let mut at = 12;
    // Every object read so far, by where it starts: what an offset delta's base is found by.
    let mut objects: Vec<(usize, Kind, Vec<u8>)> = Vec::new();
    let mut budget = MAX_INFLATED;
    for n in 0..count {
        if n % STOP_EVERY == 0 && stop() {
            return Err(Finding::Unchecked("content.job_ended"));
        }
        let start = at;
        let (kind, size) = object_header(pack, &mut at)?;
        let size = usize::try_from(size).map_err(|_| too_large())?;
        let base = match kind {
            1..=4 => None,
            6 => {
                let back = offset(pack, &mut at)?;
                let base = start.checked_sub(back).ok_or(UNREADABLE)?;
                let found = objects
                    .binary_search_by_key(&base, |(at, _, _)| *at)
                    .map_err(|_| UNREADABLE)?;
                Some(found)
            }
            // A delta against an object named by id. A push asked not to be thin has no reason to
            // send one, and the broker does not hash objects to find its base.
            7 => return Err(Finding::Unchecked("content.ref_delta")),
            _ => return Err(UNREADABLE),
        };
        budget = budget
            .checked_sub(size.max(OBJECT_COST))
            .ok_or_else(too_large)?;
        let (data, used) = inflate(pack.get(at..).ok_or(UNREADABLE)?, size)?;
        at += used;
        let (kind, data) = match base {
            None => (
                match kind {
                    1 => Kind::Commit,
                    2 => Kind::Tree,
                    3 => Kind::Blob,
                    _ => Kind::Tag,
                },
                data,
            ),
            Some(index) => {
                let (_, kind, ref base) = objects[index];
                (kind, apply(base, &data, &mut budget)?)
            }
        };
        scan(kind, &data, id_len, secrets)
            .map_err(|code| Finding::Refused(code, Sha256::digest(&data).into()))?;
        objects.push((start, kind, data));
    }
    // What is left is the pack's checksum, which the forge checks.
    if pack.len() - at != id_len {
        return Err(UNREADABLE);
    }
    Ok(())
}

fn too_large() -> Finding {
    Finding::Unchecked("content.pack_too_large")
}

/// An object's type and inflated size.
fn object_header(pack: &[u8], at: &mut usize) -> Result<(u8, u64), Finding> {
    let mut byte = *pack.get(*at).ok_or(UNREADABLE)?;
    *at += 1;
    let kind = (byte >> 4) & 7;
    let mut size = u64::from(byte & 15);
    let mut shift = 4;
    while byte & 0x80 != 0 {
        byte = *pack.get(*at).ok_or(UNREADABLE)?;
        *at += 1;
        if shift > 57 {
            return Err(too_large());
        }
        size |= u64::from(byte & 0x7f) << shift;
        shift += 7;
    }
    Ok((kind, size))
}

/// How far back an offset delta's base starts.
fn offset(pack: &[u8], at: &mut usize) -> Result<usize, Finding> {
    let mut byte = *pack.get(*at).ok_or(UNREADABLE)?;
    *at += 1;
    let mut back = usize::from(byte & 0x7f);
    while byte & 0x80 != 0 {
        byte = *pack.get(*at).ok_or(UNREADABLE)?;
        *at += 1;
        back = back
            .checked_add(1)
            .and_then(|b| b.checked_mul(128))
            .and_then(|b| b.checked_add(usize::from(byte & 0x7f)))
            .ok_or(UNREADABLE)?;
    }
    Ok(back)
}

/// Inflates one zlib stream at the start of `input` that must come to exactly `size` bytes. Returns
/// them and how many bytes of `input` the stream took.
fn inflate(input: &[u8], size: usize) -> Result<(Vec<u8>, usize), Finding> {
    let mut out = vec![0; size];
    let mut state = DecompressorOxide::new();
    let flags = TINFL_FLAG_PARSE_ZLIB_HEADER
        | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
        | TINFL_FLAG_COMPUTE_ADLER32;
    let (status, used, written) = decompress(&mut state, input, &mut out, 0, flags);
    if status != TINFLStatus::Done || written != size {
        return Err(UNREADABLE);
    }
    Ok((out, used))
}

/// A size at the start of a delta.
fn delta_size(delta: &[u8], at: &mut usize) -> Result<usize, Finding> {
    let mut size = 0usize;
    let mut shift = 0;
    loop {
        let byte = *delta.get(*at).ok_or(UNREADABLE)?;
        *at += 1;
        if shift > 63 {
            return Err(UNREADABLE);
        }
        size |= usize::from(byte & 0x7f)
            .checked_shl(shift)
            .ok_or(UNREADABLE)?;
        shift += 7;
        if byte & 0x80 == 0 {
            return Ok(size);
        }
    }
}

/// Builds the object a delta describes from its base.
fn apply(base: &[u8], delta: &[u8], budget: &mut usize) -> Result<Vec<u8>, Finding> {
    let mut at = 0;
    if delta_size(delta, &mut at)? != base.len() {
        return Err(UNREADABLE);
    }
    let size = delta_size(delta, &mut at)?;
    *budget = budget.checked_sub(size).ok_or_else(too_large)?;
    let mut out = Vec::with_capacity(size);
    while let Some(&op) = delta.get(at) {
        at += 1;
        if op & 0x80 != 0 {
            // Copy from the base: which bytes of the offset and size follow is in the low bits.
            let mut field = |bits: std::ops::Range<u8>| -> Result<usize, Finding> {
                let mut value = 0usize;
                for (i, bit) in bits.enumerate() {
                    if op & (1 << bit) != 0 {
                        value |= usize::from(*delta.get(at).ok_or(UNREADABLE)?) << (8 * i);
                        at += 1;
                    }
                }
                Ok(value)
            };
            let from = field(0..4)?;
            let length = match field(4..7)? {
                0 => 0x10000,
                n => n,
            };
            let part = from
                .checked_add(length)
                .and_then(|end| base.get(from..end))
                .ok_or(UNREADABLE)?;
            out.extend_from_slice(part);
        } else if op != 0 {
            let part = delta.get(at..at + usize::from(op)).ok_or(UNREADABLE)?;
            at += usize::from(op);
            out.extend_from_slice(part);
        } else {
            return Err(UNREADABLE);
        }
        if out.len() > size {
            return Err(UNREADABLE);
        }
    }
    if out.len() != size {
        return Err(UNREADABLE);
    }
    Ok(out)
}

/// Checks one whole object. `Err` is the reason it is refused.
fn scan(kind: Kind, data: &[u8], id_len: usize, secrets: &[&str]) -> Result<(), &'static str> {
    if secrets
        .iter()
        .any(|s| s.len() >= content::MIN_SECRET && contains(data, s.as_bytes()))
    {
        return Err("content.credential");
    }
    match kind {
        Kind::Blob => Ok(()),
        // A commit's or tag's message, author and the rest are read as text.
        Kind::Commit | Kind::Tag => {
            let text = std::str::from_utf8(data).map_err(|_| "content.hidden_text")?;
            content::check(text, secrets)
        }
        // A tree is entries of a mode, a space, a name, a NUL and an object id.
        Kind::Tree => {
            let mut rest = data;
            while !rest.is_empty() {
                let nul = rest
                    .iter()
                    .position(|b| *b == 0)
                    .ok_or("content.hidden_text")?;
                let entry = &rest[..nul];
                let space = entry
                    .iter()
                    .position(|b| *b == b' ')
                    .ok_or("content.hidden_text")?;
                let name =
                    std::str::from_utf8(&entry[space + 1..]).map_err(|_| "content.hidden_text")?;
                if name.chars().any(content::hides) {
                    return Err("content.hidden_text");
                }
                rest = rest.get(nul + 1 + id_len..).ok_or("content.hidden_text")?;
            }
            Ok(())
        }
    }
}

fn contains(data: &[u8], needle: &[u8]) -> bool {
    let Some((&first, _)) = needle.split_first() else {
        return false;
    };
    let mut from = 0;
    while let Some(i) = data[from..].iter().position(|b| *b == first) {
        let at = from + i;
        if data[at..].starts_with(needle) {
            return true;
        }
        from = at + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniz_oxide::deflate::compress_to_vec_zlib;

    const TOKEN: &str = "ghs_0123456789abcdef";

    fn header(kind: u8, size: usize) -> Vec<u8> {
        let mut out = vec![(kind << 4) | (size & 15) as u8];
        let mut rest = size >> 4;
        while rest > 0 {
            *out.last_mut().unwrap() |= 0x80;
            out.push((rest & 0x7f) as u8);
            rest >>= 7;
        }
        out
    }

    fn object(kind: u8, data: &[u8]) -> Vec<u8> {
        let mut out = header(kind, data.len());
        out.extend(compress_to_vec_zlib(data, 6));
        out
    }

    fn pack(objects: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"PACK\0\0\0\x02".to_vec();
        out.extend((objects.len() as u32).to_be_bytes());
        for o in objects {
            out.extend(o);
        }
        out.extend([0; 20]);
        out
    }

    #[test]
    fn a_pack_of_too_many_objects_is_refused_before_one_is_read() {
        // A pack that says it holds more objects than any push of a job's work needs is refused
        // before a single one is read (#336).
        let mut many = b"PACK\0\0\0\x02".to_vec();
        many.extend((MAX_OBJECTS + 1).to_be_bytes());
        assert_eq!(check(&many, 20, &[]), Err(too_large()));
        // Empty objects still pass, each charged OBJECT_COST against the budget.
        let empty = object(3, b"");
        assert_eq!(check(&pack(&[empty.clone(), empty]), 20, &[]), Ok(()));
    }

    #[test]
    fn a_check_stops_unchecked_once_the_job_ends() {
        let blob = object(3, b"fine");
        let objects = vec![blob; 3];
        assert_eq!(
            check_until(&pack(&objects), 20, &[], &|| true),
            Err(Finding::Unchecked("content.job_ended"))
        );
        assert_eq!(check_until(&pack(&objects), 20, &[], &|| false), Ok(()));
    }

    /// An offset delta `back` bytes before it.
    fn ofs_delta(back: usize, delta: &[u8]) -> Vec<u8> {
        let mut out = header(6, delta.len());
        let mut bytes = vec![(back & 0x7f) as u8];
        let mut rest = back >> 7;
        while rest > 0 {
            rest -= 1;
            bytes.push(0x80 | (rest & 0x7f) as u8);
            rest >>= 7;
        }
        bytes.reverse();
        out.extend(bytes);
        out.extend(compress_to_vec_zlib(delta, 6));
        out
    }

    fn tree(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, id) in entries {
            out.extend(b"100644 ");
            out.extend(name.as_bytes());
            out.push(0);
            out.extend(*id);
        }
        out
    }

    #[test]
    fn ordinary_objects_pass() {
        let commit =
            b"tree 0000\nauthor A <a@b> 1 +0000\n\nFix the parser\n\n\xe4\xbf\xae\xe5\xa4\x8d\n";
        let p = pack(&[
            object(
                3,
                b"binary \0\x1b\x7f bytes \xe2\x80\xae are fine in a file",
            ),
            object(2, &tree(&[("README", &[1; 20]), ("src", &[2; 20])])),
            object(1, commit),
            object(3, b""),
        ]);
        assert_eq!(check(&p, 20, &[TOKEN]), Ok(()));
        assert_eq!(check(&[], 20, &[TOKEN]), Ok(()));
        assert_eq!(check(&pack(&[]), 20, &[]), Ok(()));
    }

    #[test]
    fn a_credential_anywhere_is_refused() {
        let file = format!("let key = \"{TOKEN}\";\n");
        for p in [
            pack(&[object(3, file.as_bytes())]),
            pack(&[object(1, format!("tree x\n\nmsg {TOKEN}\n").as_bytes())]),
            pack(&[object(2, &tree(&[(TOKEN, &[1; 20])]))]),
        ] {
            assert_eq!(
                check(&p, 20, &[TOKEN]),
                Err(Finding::Refused(
                    "content.credential",
                    Sha256::digest(inflated_last(&p)).into()
                ))
            );
        }
    }

    /// The last object of a pack of whole objects, inflated.
    fn inflated_last(p: &[u8]) -> Vec<u8> {
        let mut at = 12;
        let mut last = Vec::new();
        while at < p.len() - 20 {
            let (_, size) = object_header(p, &mut at).unwrap();
            let (data, used) = inflate(&p[at..], size as usize).unwrap();
            at += used;
            last = data;
        }
        last
    }

    #[test]
    fn a_credential_a_delta_builds_from_its_base_is_refused() {
        // The base holds the first half; the delta copies it and inserts the rest.
        let (head, tail) = TOKEN.split_at(8);
        let base = format!("first line\n{head}");
        let base_object = object(3, base.as_bytes());
        let mut delta = vec![base.len() as u8, (base.len() + tail.len()) as u8];
        delta.extend([0x80 | 0x10, base.len() as u8]);
        delta.push(tail.len() as u8);
        delta.extend(tail.as_bytes());
        let p = pack(&[base_object.clone(), ofs_delta(base_object.len(), &delta)]);
        let built = format!("{base}{tail}");
        assert_eq!(
            check(&p, 20, &[TOKEN]),
            Err(Finding::Refused(
                "content.credential",
                Sha256::digest(built.as_bytes()).into()
            ))
        );
        // The same delta with an ordinary insert passes.
        let mut delta = vec![base.len() as u8, (base.len() + 3) as u8];
        delta.extend([0x80 | 0x10, base.len() as u8, 3]);
        delta.extend(b"abc");
        let p = pack(&[base_object.clone(), ofs_delta(base_object.len(), &delta)]);
        assert_eq!(check(&p, 20, &[TOKEN]), Ok(()));
    }

    #[test]
    fn hidden_text_in_a_message_or_a_name_is_refused() {
        for p in [
            pack(&[object(1, "tree x\n\nlooks fine\u{202e}\n".as_bytes())]),
            pack(&[object(4, "object x\n\nv1\u{e0041}\n".as_bytes())]),
            pack(&[object(1, b"tree x\n\nnot utf-8 \xff\n")]),
            pack(&[object(
                2,
                &tree(&[("README", &[1; 20]), ("a\u{200b}b", &[2; 20])]),
            )]),
        ] {
            assert!(
                matches!(
                    check(&p, 20, &[]),
                    Err(Finding::Refused("content.hidden_text", _))
                ),
                "{:?}",
                check(&p, 20, &[])
            );
        }
    }

    #[test]
    fn a_pack_that_cannot_be_read_whole_is_unchecked() {
        let good = pack(&[object(3, b"hello")]);
        let mut short = good.clone();
        short.pop();
        let mut extra = good.clone();
        extra.push(0);
        let mut corrupt = good.clone();
        corrupt[15] ^= 0xff;
        let mut lying = good.clone();
        lying[12] = (3 << 4) | 6; // says 6 bytes, inflates to 5
        let mut ref_delta = header(7, 5);
        ref_delta.extend([1; 20]);
        ref_delta.extend(compress_to_vec_zlib(b"\x05\x05\x05abcde", 6));
        let mut base_missing = good[..12].to_vec();
        base_missing[11] = 1;
        base_missing.extend(ofs_delta(1, b"\x00\x01\x01a"));
        base_missing.extend([0; 20]);
        for (name, p, reason) in [
            (
                "not a pack",
                b"PAKC\0\0\0\x02\0\0\0\0".to_vec(),
                "content.pack_unreadable",
            ),
            ("short", short, "content.pack_unreadable"),
            ("extra", extra, "content.pack_unreadable"),
            ("corrupt", corrupt, "content.pack_unreadable"),
            ("lying", lying, "content.pack_unreadable"),
            ("ref delta", pack(&[ref_delta]), "content.ref_delta"),
            ("base missing", base_missing, "content.pack_unreadable"),
            (
                "huge",
                pack(&[header(3, MAX_INFLATED + 1)]),
                "content.pack_too_large",
            ),
        ] {
            assert_eq!(
                check(&p, 20, &[]),
                Err(Finding::Unchecked(reason)),
                "{name}"
            );
        }
    }

    #[test]
    fn a_delta_is_held_to_its_base_and_its_size() {
        let base = b"0123456789";
        for delta in [
            &b"\x09\x02\x90\x0a"[..],     // base size wrong
            &b"\x0a\x02\x91\x08\x05"[..], // copies past the base
            &b"\x0a\x02\x00"[..],         // op 0
            &b"\x0a\x02\x03abc"[..],      // more than it said
            &b"\x0a\x05\x02ab"[..],       // less than it said
        ] {
            let mut budget = usize::MAX;
            assert_eq!(
                apply(base, delta, &mut budget),
                Err(UNREADABLE),
                "{delta:?}"
            );
        }
        assert_eq!(
            apply(base, b"\x0a\x05\x91\x02\x03\x02xy", &mut 100),
            Ok(b"234xy".to_vec())
        );
        assert_eq!(apply(base, b"\x0a\x05\x02ab", &mut 2), Err(too_large()));
    }
}
