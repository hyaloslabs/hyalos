//! Issue digest guest for hyalOS kernel v0 (see ABI.md).
//!
//! Reads up to 200 records of `<issue number>\t<issue title>`, classifies each
//! title with `infer` (schema 0), and sends one digest through one `effect`.

#![no_std]

use core::arch::wasm32::unreachable;
use core::panic::PanicInfo;
use core::ptr::addr_of_mut;

#[link(wasm_import_module = "hyalos")]
extern "C" {
    #[link_name = "read"]
    fn host_read(index: i32, ptr: i32, cap: i32) -> i32;
    #[link_name = "infer"]
    fn host_infer(schema: i32, ptr: i32, len: i32) -> i32;
    #[link_name = "effect"]
    fn host_effect(kind: i32, ptr: i32, len: i32) -> i32;
    #[link_name = "return"]
    fn host_return(ptr: i32, len: i32);
}

const MAX_RECORDS: usize = 200;
/// Buffer for one input record (number, tab and title).
const REC_CAP: usize = 16384;
/// The infer question limit, also the most title text kept per security entry.
const TITLE_MAX: usize = 4096;
/// The digest limit, including the truncation line.
const DIGEST_MAX: usize = 8000;
const DIGEST_BUF: usize = 8192;
const TRUNC_LINE: &[u8] = b"...truncated\n";

const LABEL_BUG: i32 = 0;
const LABEL_FEATURE: i32 = 1;
const LABEL_SECURITY: i32 = 2;
const LABEL_OTHER: i32 = 3;

struct Entry {
    num: u32,
    len: usize,
    title: [u8; TITLE_MAX],
}

const EMPTY_ENTRY: Entry = Entry {
    num: 0,
    len: 0,
    title: [0; TITLE_MAX],
};

static mut REC: [u8; REC_CAP] = [0; REC_CAP];
static mut SEC: [Entry; MAX_RECORDS] = [EMPTY_ENTRY; MAX_RECORDS];
static mut DIG: [u8; DIGEST_BUF] = [0; DIGEST_BUF];

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unreachable()
}

/// Strict decimal parse; `None` for empty, non-digit or overflowing input.
fn parse_number(s: &[u8]) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((c - b'0') as u32)?;
    }
    Some(n)
}

/// Append bytes. Callers keep the total within the buffer.
fn put(buf: &mut [u8], len: &mut usize, bytes: &[u8]) {
    let end = *len + bytes.len();
    buf[*len..end].copy_from_slice(bytes);
    *len = end;
}

fn put_num(buf: &mut [u8], len: &mut usize, mut n: u32) {
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    put(buf, len, &tmp[i..]);
}

fn num_len(mut n: u32) -> usize {
    let mut d = 1;
    while n >= 10 {
        n /= 10;
        d += 1;
    }
    d
}

#[no_mangle]
pub extern "C" fn run() {
    let rec: &mut [u8] = unsafe { &mut *addr_of_mut!(REC) };
    let sec: &mut [Entry] = unsafe { &mut *addr_of_mut!(SEC) };
    let dig: &mut [u8] = unsafe { &mut *addr_of_mut!(DIG) };
    let rec_ptr = rec.as_ptr() as usize as i32;

    // counts indexed by label: bug, feature, security, other
    let mut counts = [0u32; 4];
    let mut total: u32 = 0;
    let mut nsec: usize = 0;

    for index in 0..MAX_RECORDS as i32 {
        let n = unsafe { host_read(index, rec_ptr, REC_CAP as i32) };
        if n == -1 {
            break;
        }
        total += 1;
        if n < 0 {
            // Too large for the buffer (-2) or otherwise unreadable: no title to ask about.
            counts[LABEL_OTHER as usize] += 1;
            continue;
        }
        let line = &rec[..core::cmp::min(n as usize, REC_CAP)];

        let label = match line.iter().position(|&b| b == b'\t') {
            None => LABEL_OTHER,
            Some(tab) => match parse_number(&line[..tab]) {
                None => LABEL_OTHER,
                Some(number) => {
                    let title = &line[tab + 1..];
                    let tl = core::cmp::min(title.len(), TITLE_MAX);
                    let answer =
                        unsafe { host_infer(0, title.as_ptr() as usize as i32, tl as i32) };
                    let label = if (LABEL_BUG..=LABEL_SECURITY).contains(&answer) {
                        answer
                    } else {
                        LABEL_OTHER
                    };
                    if label == LABEL_SECURITY && nsec < MAX_RECORDS {
                        let e = &mut sec[nsec];
                        e.num = number;
                        e.len = tl;
                        e.title[..tl].copy_from_slice(&title[..tl]);
                        nsec += 1;
                    }
                    label
                }
            },
        };
        counts[label as usize] += 1;
    }

    let mut len = 0usize;
    put(dig, &mut len, b"total ");
    put_num(dig, &mut len, total);
    put(dig, &mut len, b" bug ");
    put_num(dig, &mut len, counts[LABEL_BUG as usize]);
    put(dig, &mut len, b" feature ");
    put_num(dig, &mut len, counts[LABEL_FEATURE as usize]);
    put(dig, &mut len, b" security ");
    put_num(dig, &mut len, counts[LABEL_SECURITY as usize]);
    put(dig, &mut len, b" other ");
    put_num(dig, &mut len, counts[LABEL_OTHER as usize]);
    put(dig, &mut len, b"\n");

    for k in 0..nsec {
        let e = &sec[k];
        // "#" digits " " title "\n"
        let line_len = 1 + num_len(e.num) + 1 + e.len + 1;
        let last = k + 1 == nsec;
        // Keep room for the truncation line unless nothing follows this line.
        if len + line_len + TRUNC_LINE.len() <= DIGEST_MAX || (last && len + line_len <= DIGEST_MAX)
        {
            put(dig, &mut len, b"#");
            put_num(dig, &mut len, e.num);
            put(dig, &mut len, b" ");
            put(dig, &mut len, &e.title[..e.len]);
            put(dig, &mut len, b"\n");
        } else {
            put(dig, &mut len, TRUNC_LINE);
            break;
        }
    }

    let digest_ptr = dig.as_ptr() as usize as i32;
    unsafe {
        host_effect(0, digest_ptr, len as i32);
    }

    let mut result = [0u8; 16];
    let mut rlen = 0usize;
    put(&mut result, &mut rlen, b"ok ");
    put_num(&mut result, &mut rlen, total);
    unsafe {
        host_return(result.as_ptr() as usize as i32, rlen as i32);
        unreachable()
    }
}
