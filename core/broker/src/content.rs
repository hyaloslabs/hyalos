//! The content gate on outbound text (ADR-0004, ADR-0008, ADR-0048; the first slice of S4.5).
//!
//! Text a job sends to the forge in clear, such as a pull request's title and body, is checked byte by
//! byte before it leaves. It is refused if it holds a credential the broker holds, the job's forge
//! token or a provider key, or a character that hides text from the person reading it: bidirectional
//! controls, zero-width and other invisible characters, Unicode tags, and control characters other
//! than tab and line breaks. What it cannot catch is a secret the job has encoded or split; the gate
//! is a byte check, not a judge of intent.

/// Credentials shorter than this are not looked for: they would match ordinary text.
const MIN_SECRET: usize = 12;

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
}
