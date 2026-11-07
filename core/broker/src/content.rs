//! The content gate on outbound text (ADR-0004, ADR-0008, ADR-0048, ADR-0065).
//!
//! Text a job sends to the forge in clear, such as a pull request's title and body, is checked byte by
//! byte before it leaves. [`check`] refuses a credential the broker holds, the job's forge token or a
//! provider key, and a character that hides text from the person reading it: bidirectional
//! controls, zero-width and other invisible characters, Unicode tags, and control characters other
//! than tab and line breaks.
//!
//! [`check_published`] is for text a tainted job publishes where people and their browsers render it
//! (ADR-0065). It also refuses a key of a known shape, a credential encoded or split, a link off the
//! forge or carrying a query, and a body longer than [`MAX_PUBLISHED`]. It is still a check of the
//! text, not a judge of intent: a secret written as prose passes.

/// Credentials shorter than this are not looked for: they would match ordinary text.
pub(crate) const MIN_SECRET: usize = 12;

/// Checks `text` against the credentials in `secrets`. `Err` is the reason code it is refused with.
pub fn check(text: &str, secrets: &[&str]) -> Result<(), &'static str> {
    if secrets
        .iter()
        .any(|s| s.len() >= MIN_SECRET && text.contains(s))
    {
        return Err("content.credential");
    }
    if text.chars().any(hides) {
        return Err("content.hidden_text");
    }
    Ok(())
}

/// The longest text a tainted job may publish in one field, in characters. Past it a body can carry a
/// payload no reviewer reads (ADR-0065).
pub const MAX_PUBLISHED: usize = 20_000;

/// Checks text a tainted job publishes on the forge (ADR-0065): what [`check`] refuses, then a body
/// longer than [`MAX_PUBLISHED`], a key of a known shape (`content.key_shape`), a credential or key
/// encoded, reversed or split (`content.encoded_credential`), and a link anywhere but `forge`, the
/// forge's web host, or one there with a query (`content.link`). `None` allows no link.
pub fn check_published(
    text: &str,
    secrets: &[&str],
    forge: Option<&str>,
) -> Result<(), &'static str> {
    check(text, secrets)?;
    if text.chars().count() > MAX_PUBLISHED {
        return Err("content.too_long");
    }
    if key_shape(text) {
        return Err("content.key_shape");
    }
    let held = |view: &str| {
        secrets
            .iter()
            .any(|s| s.len() >= MIN_SECRET && view.contains(s))
    };
    if views(text).iter().any(|v| held(v) || issued_key(v)) {
        return Err("content.encoded_credential");
    }
    // Markdown and HTML both decode character references, so `&#47;&#47;host` is a link too.
    if !links_allowed(text, forge) || !links_allowed(&entities_decoded(text), forge) {
        return Err("content.link");
    }
    Ok(())
}

/// The ways `text` could carry a key it does not show: with runs of short pieces joined (a key split
/// by spaces), reversed, percent-decoded, with character references decoded, and each run of base64
/// or hex decoded.
fn views(text: &str) -> Vec<String> {
    let mut views = vec![
        joined(text),
        text.chars().rev().collect(),
        percent_decoded(text),
        entities_decoded(text),
    ];
    let runs = |alphabet: fn(u8) -> bool| {
        text.as_bytes()
            .split(move |b| !alphabet(*b))
            .filter(|run| run.len() >= MIN_ENCODED)
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>()
    };
    for run in runs(|b| b.is_ascii_alphanumeric() || b"+/=-_".contains(&b)) {
        views.extend(base64_decoded(&run).map(|b| String::from_utf8_lossy(&b).into_owned()));
    }
    for run in runs(|b| b.is_ascii_hexdigit()) {
        views.extend(hex_decoded(&run).map(|b| String::from_utf8_lossy(&b).into_owned()));
    }
    views
}

/// Encoded runs shorter than this are not decoded: they would decode ordinary words.
const MIN_ENCODED: usize = 16;

/// `text` with each run of pieces of at most 4 characters joined into one word, so a key split into
/// short groups reads whole again while ordinary words stay apart.
fn joined(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut short = false;
    for piece in text.split_whitespace() {
        let is_short = piece.chars().count() <= 4;
        if !(short && is_short) && !out.is_empty() {
            out.push(' ');
        }
        out.push_str(piece);
        short = is_short;
    }
    out
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let pair = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], pair) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_decoded(run: &[u8]) -> Option<Vec<u8>> {
    run.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        })
        .collect()
}

/// `run` as standard or URL-safe base64, padded or not.
fn base64_decoded(run: &[u8]) -> Option<Vec<u8>> {
    let run = run
        .strip_suffix(b"==")
        .or(run.strip_suffix(b"="))
        .unwrap_or(run);
    let value = |b: u8| match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    if run.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(run.len() * 3 / 4);
    for chunk in run.chunks(4) {
        let mut n = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            n |= u32::from(value(*b)?) << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

/// The characters a key's body is made of.
fn key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Whether `text` holds a key of a shape a provider or service issues, or a credential written as an
/// assignment or inside a URL.
fn key_shape(text: &str) -> bool {
    if issued_key(text) {
        return true;
    }
    // A password in a URL: `scheme://user:password@host`.
    if text.match_indices("://").any(|(at, _)| {
        let authority = text[at + 3..]
            .split(|c: char| c.is_whitespace() || "/?#)]>\"'".contains(c))
            .next()
            .unwrap_or_default();
        authority
            .rsplit_once('@')
            .is_some_and(|(info, _)| info.contains(':'))
    }) {
        return true;
    }
    assignment(text)
}

/// Whether `text` holds a key in a form a provider or service issues: a known prefix that starts a
/// word and enough key characters after it, a private key block, or a JSON web token.
fn issued_key(text: &str) -> bool {
    /// A prefix, and how many key characters must follow it.
    const PREFIXES: [(&str, usize); 20] = [
        ("sk-ant-", 8),
        ("sk-proj-", 8),
        ("sk-svcacct-", 8),
        ("ghp_", 20),
        ("gho_", 20),
        ("ghu_", 20),
        ("ghs_", 20),
        ("ghr_", 20),
        ("github_pat_", 20),
        ("glpat-", 8),
        ("xoxa-", 8),
        ("xoxb-", 8),
        ("xoxp-", 8),
        ("xoxr-", 8),
        ("xoxs-", 8),
        ("xapp-", 8),
        ("sk_live_", 8),
        ("sk_test_", 8),
        ("rk_live_", 8),
        ("rk_test_", 8),
    ];
    let starts_word = |at: usize| !text[..at].chars().next_back().is_some_and(key_char);
    // Whether `need` characters of the kind `of` follow `at`; it reads no further than that.
    let tail = |at: usize, of: fn(char) -> bool, need: usize| {
        text[at..].chars().take(need).take_while(|c| of(*c)).count() >= need
    };
    if PREFIXES.iter().any(|(prefix, need)| {
        text.match_indices(prefix)
            .any(|(at, _)| starts_word(at) && tail(at + prefix.len(), key_char, *need))
    }) {
        return true;
    }
    // An AWS access key id: `AKIA` or `ASIA` and 16 capitals or digits.
    let aws = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit();
    if ["AKIA", "ASIA"].iter().any(|prefix| {
        text.match_indices(prefix)
            .any(|(at, _)| starts_word(at) && tail(at + 4, aws, 16))
    }) {
        return true;
    }
    if text.contains("PRIVATE KEY-----") {
        return true;
    }
    // A JSON web token: a word of base64url segments, the first two starting `eyJ`, joined by dots.
    text.split(|c: char| !(key_char(c) || c == '.')).any(|word| {
        let mut parts = word.split('.');
        matches!((parts.next(), parts.next()),
            (Some(head), Some(body)) if head.len() >= 8 && head.starts_with("eyJ") && body.starts_with("eyJ"))
    })
}

/// Whether `text` assigns a value of 8 key characters or more, one a digit, to a name that says it is
/// a credential: `API_TOKEN=...`, `password: ...`.
fn assignment(text: &str) -> bool {
    const NAMES: [&str; 8] = [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "APIKEY",
        "ACCESS_KEY",
        "CREDENTIAL",
    ];
    text.match_indices(['=', ':']).any(|(at, _)| {
        // `Token::new` is a path, not an assignment.
        if text[..at].ends_with(':') || text[at + 1..].starts_with(':') {
            return false;
        }
        // Each side is read only so far, so text of many `=` is checked in linear time.
        let name: String = text[..at]
            .trim_end()
            .chars()
            .rev()
            .take(64)
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let name: String = name.chars().rev().collect();
        if !NAMES.iter().any(|n| name.contains(n)) {
            return false;
        }
        let value: String = text[at + 1..]
            .trim_start()
            .trim_start_matches(['"', '\''])
            .chars()
            .take(256)
            .take_while(|c| !c.is_whitespace() && !"\"'".contains(*c))
            .collect();
        // A secret is made of key characters and has a digit: a number is a setting such as
        // `max_tokens: 16384`, and `token: Option<&str>` is a type.
        value.chars().count() >= 8
            && value.chars().all(|c| key_char(c) || "+/=.".contains(c))
            && value.chars().any(|c| c.is_ascii_digit())
            && !value.chars().all(|c| c.is_ascii_digit() || c == '_')
    })
}

/// `text` with HTML character references decoded: decimal and hexadecimal ones, and the named ones
/// a URL can be spelled with. Others are left as they are.
fn entities_decoded(text: &str) -> String {
    const NAMED: [(&str, char); 18] = [
        ("amp", '&'),
        ("lt", '<'),
        ("gt", '>'),
        ("quot", '"'),
        ("apos", '\''),
        ("colon", ':'),
        ("sol", '/'),
        ("bsol", '\\'),
        ("period", '.'),
        ("quest", '?'),
        ("equals", '='),
        ("num", '#'),
        ("commat", '@'),
        ("lpar", '('),
        ("rpar", ')'),
        ("lsqb", '['),
        ("rsqb", ']'),
        ("Tab", '\t'),
    ];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        // A numeric reference ends at its last digit, as a browser reads it: `&#x2Fwww` is `/www`.
        let (decoded, used) = match rest.strip_prefix('#') {
            Some(n) => {
                let (radix, skip) = if n.starts_with(['x', 'X']) {
                    (16, 2)
                } else {
                    (10, 1)
                };
                let digits: String = rest[skip..]
                    .chars()
                    .take(8)
                    .take_while(|c| c.is_digit(radix))
                    .collect();
                let c = u32::from_str_radix(&digits, radix)
                    .ok()
                    .and_then(char::from_u32);
                (c, skip + digits.len())
            }
            None => {
                let name: String = rest
                    .chars()
                    .take(10)
                    .take_while(char::is_ascii_alphanumeric)
                    .collect();
                let c = NAMED.iter().find(|(n, _)| *n == name).map(|(_, c)| *c);
                (c, name.len())
            }
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[used..];
                rest = rest.strip_prefix(';').unwrap_or(rest);
            }
            None => out.push('&'),
        }
    }
    out.push_str(rest);
    out
}

/// Whether a character ends a URL's host.
fn ends_host(c: char) -> bool {
    c.is_whitespace() || "/\\?#)]>\"'<".contains(c)
}

/// Whether every link in `text` is to `forge` with no query. A link is any of:
/// - a URL with a scheme and a slash or backslash after it, or `http:`/`https:` with neither, as
///   browsers read `https:/host` and `https:\\host` (only `http` and `https` can pass);
/// - a scheme-relative link, two or more slashes or backslashes where a link begins, before a host
///   name (`//host.name`, `///host.name`, `\\\\host.name`);
/// - a `data:` URI;
/// - a bare host name that starts `www.` or carries a query (`exfil.example/c?d=...`).
///
/// A `// comment` and a bare `Node.js/path` are not links. A reader's browser or the forge's image
/// proxy fetches what an image link names, so a link off the forge, or a query to it, carries data
/// out of the job (ADR-0065).
fn links_allowed(text: &str, forge: Option<&str>) -> bool {
    // `rest` starts after the scheme's colon, or at the slashes of a scheme-relative link.
    let to_forge = |rest: &str| {
        let rest = rest.trim_start_matches(['/', '\\']);
        let end = rest.find(ends_host).unwrap_or(rest.len());
        let authority = &rest[..end];
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host.split(':').next().unwrap_or_default();
        let query = rest[end..]
            .split(|c: char| c.is_whitespace() || ")]>\"'<".contains(c))
            .next()
            .unwrap_or_default()
            .contains('?');
        forge.is_some_and(|f| host.eq_ignore_ascii_case(f)) && !query
    };
    let slash = |c: char| c == '/' || c == '\\';
    for (at, _) in text.match_indices(':') {
        let scheme: String = text[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || "+.-".contains(*c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let after = &text[at + 1..];
        let web = scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http");
        // A scheme starts with a letter; one letter alone is a drive, as in `C:\\Users`.
        if scheme.len() < 2 || !scheme.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        if scheme.eq_ignore_ascii_case("data") && !after.starts_with(char::is_whitespace) {
            return false;
        }
        if after.starts_with(slash) && !web {
            return false;
        }
        // `https:host` too; `https: ` in prose names no host.
        if web && !after.starts_with(char::is_whitespace) && !after.is_empty() && !to_forge(after) {
            return false;
        }
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, (at, c)) in chars.iter().enumerate() {
        let starts_link = match i.checked_sub(1).map(|j| chars[j].1) {
            None => true,
            Some(b) => b.is_whitespace() || "(\"'=<[".contains(b),
        };
        if !slash(*c) || !starts_link || !text[*at + 1..].starts_with(slash) {
            continue;
        }
        let rest = &text[*at..];
        let host_named = rest
            .trim_start_matches(slash)
            .split(ends_host)
            .next()
            .is_some_and(|host| host.contains('.'));
        // A `// comment` is not a link.
        if host_named && !to_forge(rest) {
            return false;
        }
    }
    for word in text.split(|c: char| c.is_whitespace() || "()[]<>\"'".contains(c)) {
        if word.contains(':') {
            continue;
        }
        // A bare host name followed by a query, or one that starts `www.`.
        let end = word.find(['/', '\\', '?']).unwrap_or(word.len());
        let host = &word[..end];
        let labels: Vec<&str> = host.split('.').collect();
        let named = labels.len() >= 2
            && labels.iter().all(|l| {
                !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            && labels
                .last()
                .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()));
        let carries = word[end..].contains('?') || host.to_ascii_lowercase().starts_with("www.");
        if named && carries && !to_forge(word) {
            return false;
        }
    }
    true
}

/// Whether `c` can hide or reorder what a reader sees.
pub fn hides(c: char) -> bool {
    matches!(c,
        // Control characters, but tab and line breaks.
        '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}'..='\u{9f}'
        // Soft hyphen, combining grapheme joiner, Hangul and Khmer fillers, Mongolian separator.
        | '\u{ad}' | '\u{34f}' | '\u{115f}' | '\u{1160}' | '\u{17b4}' | '\u{17b5}' | '\u{180e}'
        // Zero-width spaces and joiners, and the left-to-right and right-to-left marks.
        | '\u{200b}'..='\u{200f}'
        // Line and paragraph separators, and the bidirectional embeddings and overrides.
        | '\u{2028}'..='\u{202e}'
        // Word joiner, invisible operators, bidirectional isolates, deprecated format characters.
        | '\u{2060}'..='\u{206f}'
        | '\u{3164}' | '\u{feff}' | '\u{ffa0}'
        // Interlinear annotation and object replacement characters.
        | '\u{fff9}'..='\u{fffc}'
        // Unicode tags, which can carry a whole hidden message.
        | '\u{e0000}'..='\u{e007f}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "ghs_0123456789abcdef";
    const KEY: &str = "sk-ant-api03-secret-key";

    #[test]
    fn ordinary_text_passes_in_any_language() {
        for text in [
            "Fix the parser\n\nCloses #12.\r\n\t- one\n",
            "修复解析器 · Исправить · إصلاح المحلل · 🙂 café",
            "",
        ] {
            assert_eq!(check(text, &[TOKEN, KEY]), Ok(()), "{text:?}");
        }
    }

    #[test]
    fn a_credential_the_broker_holds_is_refused() {
        assert_eq!(
            check(&format!("see {TOKEN} here"), &[TOKEN]),
            Err("content.credential")
        );
        assert_eq!(check(KEY, &[TOKEN, KEY]), Err("content.credential"));
        // Too short to look for without refusing ordinary words.
        assert_eq!(check("the key", &["key"]), Ok(()));
    }

    #[test]
    fn text_that_hides_from_its_reader_is_refused() {
        for c in [
            '\u{0}',
            '\u{1b}',
            '\u{7f}',
            '\u{85}',
            '\u{ad}',
            '\u{200b}',
            '\u{200d}',
            '\u{200e}',
            '\u{202e}',
            '\u{2066}',
            '\u{2069}',
            '\u{2060}',
            '\u{feff}',
            '\u{e0041}',
            '\u{e007f}',
        ] {
            assert_eq!(
                check(&format!("looks fine{c}"), &[]),
                Err("content.hidden_text"),
                "U+{:04X}",
                c as u32
            );
        }
    }

    const FORGE: Option<&str> = Some("github.com");

    #[test]
    fn an_ordinary_pull_request_passes_the_published_check() {
        let body = "Fixes the retry backoff in `src/retry.rs` (closes #12).\n\n\
            - `ForgeToken::new` now takes the expiry; `max_tokens: 16384` is unchanged.\n\
            - Tested on Node.js/Deno and with `cargo test`, e.g. the `risk_test_cases` suite.\n\
            - See https://github.com/octo/repo/issues/12 and \
            [the docs](https://github.com/octo/repo/blob/main/README.md#retry).\n\n\
            ```rust\n// a comment, not a link\nlet token: Option<&str> = None;\n```\n\
            GitHub App tokens start with ghs_ and expire after an hour. Commit 0123456789abcdef0123456789abcdef01234567.";
        assert_eq!(check_published(body, &[TOKEN, KEY], FORGE), Ok(()));
        assert_eq!(check_published("修复解析器 · café", &[], None), Ok(()));
    }

    #[test]
    fn what_check_refuses_is_refused_first() {
        assert_eq!(
            check_published(&format!("see {TOKEN}"), &[TOKEN], FORGE),
            Err("content.credential")
        );
        assert_eq!(
            check_published("a\u{200b}b", &[], FORGE),
            Err("content.hidden_text")
        );
    }

    #[test]
    fn a_body_past_the_cap_is_refused() {
        let at = "a".repeat(MAX_PUBLISHED);
        assert_eq!(check_published(&at, &[], FORGE), Ok(()));
        assert_eq!(
            check_published(&format!("{at}a"), &[], FORGE),
            Err("content.too_long")
        );
    }

    #[test]
    fn keys_of_known_shapes_are_refused() {
        let pem = ["-----BEGIN OPENSSH ", "PRIVATE KEY-----"].concat();
        for key in [
            ["sk-", "ant-api03-abcdefgh"].concat(),
            ["gh", "p_", &"a".repeat(36)].concat(),
            ["gl", "pat-abcdefghij"].concat(),
            ["AK", "IAABCDEFGHIJKLMNOP"].concat(),
            pem,
            ["xo", "xb-1234-abcdefgh"].concat(),
            ["s", "k_live_abcdefghij"].concat(),
            ["ey", "JhbGciOiJI.", "ey", "JzdWIiOi.sig"].concat(),
            "https://bot:hunter2hunter2@git.example/x.git".into(),
            "API_TOKEN=abcdef1234".into(),
            "password: \"hunter2hunter2\"".into(),
        ] {
            assert_eq!(
                check_published(&format!("key {key} here"), &[], FORGE),
                Err("content.key_shape"),
                "{key}"
            );
        }
        // Inside a word, or with too short a tail, a prefix is ordinary text.
        for text in [
            "disk_test_harness_runs",
            "the ghs_ prefix",
            "ASIAN_PACIFIC_REGIONS",
        ] {
            assert_eq!(check_published(text, &[], FORGE), Ok(()), "{text}");
        }
    }

    #[test]
    fn a_credential_encoded_reversed_or_split_is_refused() {
        let held = "held-credential-0123";
        let b64 = "aGVsZC1jcmVkZW50aWFsLTAxMjM=";
        let hex: String = held.bytes().map(|b| format!("{b:02x}")).collect();
        let pct: String = held.bytes().map(|b| format!("%{b:02X}")).collect();
        let reversed: String = held.chars().rev().collect();
        let split = "hel d-c red ent ial -01 23";
        for text in [b64, &hex, &pct, &reversed, split] {
            assert_eq!(
                check_published(&format!("ref {text}"), &[held], FORGE),
                Err("content.encoded_credential"),
                "{text}"
            );
        }
        // A key of a known shape, encoded, is refused without the broker holding it.
        let key = ["sk-", "ant-api03-abcdefgh"].concat();
        let hex: String = key.bytes().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            check_published(&hex, &[], FORGE),
            Err("content.encoded_credential")
        );
    }

    #[test]
    fn links_off_the_forge_or_with_a_query_are_refused() {
        for text in [
            "![s](https://evil.example/p.png)",
            "https://exfil.example/c?d=1",
            "[r](//exfil.example/c)",
            "<img src=\"//exfil.example/p.png\">",
            "https://github.com.exfil.example/x",
            "ftp://exfil.example/x",
            "<https://exfil.example/a>",
            "exfil.example/c?d=1",
            "[1]: exfil.example/c?d=1",
            "www.exfil.example/x",
            "https://github.com/octo/repo/issues?q=1",
            "![x](data:text/plain;base64,aGk=)",
            "HTTPS://EXFIL.EXAMPLE/",
            "![s](///exfil.example/p.png)",
            "![s](\\\\exfil.example/p.png)",
            "https:\\\\exfil.example/p",
            "https:/exfil.example/p",
            "https:exfil.example/p",
            "<img src=\"&#47;&#47;exfil.example/p.png\">",
            "[r](https&colon;//exfil.example/x)",
            "[r](&#x68;ttps://exfil.example/x)",
            "[r](&#x2F;&#x2Fwww.exfil.example/x)",
            "[r](&#47&#47exfil.example/x)",
            "https://github.com@exfil.example/x",
            "DATA:text/plain,hi",
            "javascript://exfil.example/%0aalert(1)",
        ] {
            assert_eq!(
                check_published(text, &[], FORGE),
                Err("content.link"),
                "{text}"
            );
        }
        // With no forge host known, no link is allowed.
        assert_eq!(
            check_published("https://github.com/octo/repo", &[], None),
            Err("content.link")
        );
        for text in [
            "https://GitHub.com/octo/repo/pull/1#x",
            "use https: always, and see the note: it is short",
            "Fixes `Ord::cmp` in a::b::c, ratio 1:2, at 10:30.",
            "R&amp;D, &lt;T&gt; and &#169; 2026",
            "Built on C:\\Users\\ci and D:/cache.",
        ] {
            assert_eq!(check_published(text, &[], FORGE), Ok(()), "{text}");
        }
    }

    #[test]
    fn text_built_to_be_slow_is_checked_in_time() {
        let started = std::time::Instant::now();
        for text in [
            "/".repeat(MAX_PUBLISHED),
            ":".repeat(MAX_PUBLISHED),
            "&#".repeat(MAX_PUBLISHED / 2),
            "a=".repeat(MAX_PUBLISHED / 2),
            "https:".repeat(MAX_PUBLISHED / 6),
            "eyJ".repeat(MAX_PUBLISHED / 3),
            "ab ".repeat(MAX_PUBLISHED / 3),
            "TOKEN=".repeat(MAX_PUBLISHED / 6),
            "TOKEN:a".repeat(MAX_PUBLISHED / 7),
            format!(
                "{}{}",
                " ".repeat(MAX_PUBLISHED / 2),
                ": ".repeat(MAX_PUBLISHED / 4)
            ),
            format!("{}:", " ".repeat(MAX_PUBLISHED - 1)),
        ] {
            let _ = check_published(&text, &[TOKEN, KEY], FORGE);
        }
        // Linear work, even unoptimised; a quadratic scan of 20,000 characters takes far longer.
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
