/// The commit a build names: forty hexadecimal digits, or `unknown`, never a guess.
pub fn named(value: Option<&str>) -> &str {
    value
        .filter(|commit| commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or("unknown")
}
