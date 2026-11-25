//! hyalOS's line budget and its rules for `unsafe` (ADR-0133).
//!
//! hyalOS is its own workspace (ADR-0130), so its code is not part of the trusted core's count. It
//! has a budget of its own, counted by `count_rust`'s rules over its non-test code, and `unsafe` is
//! allowed only in the files on [`UNSAFE_FILES`], each use with a `SAFETY` comment.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::count_rust;

/// hyalOS's own non-test code may not pass this (ADR-0133). Raising it takes a new ADR.
pub const BUDGET: usize = 12_000;

/// The files under `hyalos-os/` that may use `unsafe`. A file joins this list in review, with a
/// reason; the lint table's `unsafe_code = "deny"` keeps it out of every other file.
pub const UNSAFE_FILES: &[&str] = &[
    // The guest's imports from the host and its fixed buffers.
    "guest/src/lib.rs",
    // The kernel image: its allocator, its entry point, page tables, `CPUID` and port I/O.
    "kernel-image/src/main.rs",
    // The monitor's KVM machine: guest memory and the KVM ioctls.
    "monitor/src/kvm.rs",
    // The monitor ending a live run's process group.
    "monitor/src/live.rs",
    // The machine runner: `close_range`, and in a `test-switches` build its hostile acts.
    "monitor/src/runner.rs",
    // The notary's system calls: a file lock, `getrandom`, `getrusage`.
    "attest/src/bin/notary.rs",
    // The canary's file lock, and its test's.
    "attest/src/bin/canary.rs",
    "attest/tests/canary.rs",
    // Tests that set the environment or confine a child before it runs: the standard library offers
    // no safe form of `env::set_var` or `pre_exec`.
    "audit/tests/no_stand_in.rs",
    "verify/tests/alone.rs",
    "verify/tests/real.rs",
];

/// How many lines above a use its `SAFETY` comment may be.
const SAFETY_REACH: usize = 3;

/// Whether a file under `hyalos-os/` is counted: Rust, outside any `tests/` directory, and not a
/// `*_tests.rs` file. `count_rust` then leaves out everything after `#[cfg(test)]`.
pub fn counted(rel: &str) -> bool {
    let mut parts: Vec<&str> = rel.split('/').collect();
    let Some(name) = parts.pop() else {
        return false;
    };
    name.ends_with(".rs") && !name.ends_with("_tests.rs") && !parts.contains(&"tests")
}

/// Counted lines per crate, the crate being the first directory under `hyalos-os/`.
pub fn per_crate(files: &[(String, String)]) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for (rel, text) in files {
        if counted(rel) {
            let krate = rel.split('/').next().unwrap_or_default().to_string();
            *out.entry(krate).or_default() += count_rust(text);
        }
    }
    out
}

/// Every line of code `count_rust` would not see because it follows a `#[cfg(test)]` item, as
/// `path:line: what`. The count stops at the first line starting `#[cfg(test)]`, so whatever comes
/// after it must be test modules too: further `#[cfg(test)]` lines, attributes, a `mod name;` or a
/// `mod name { ... }` closed by a `}` at the start of a line, blanks and comments. Anything else
/// would leave real code out of the count, so it fails the check.
pub fn hidden_findings(files: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (rel, text) in files {
        if !counted(rel) {
            continue;
        }
        if let Some(at) = hidden_after_test(text) {
            out.push(format!(
                "{rel}:{at}: code after #[cfg(test)] is not counted; move it above the tests"
            ));
        }
    }
    out
}

fn hidden_after_test(text: &str) -> Option<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.iter().position(|l| l.starts_with("#[cfg(test)]"))?;
    let mut pending = true;
    let mut in_block = false;
    for (i, line) in lines.iter().enumerate().skip(first + 1) {
        if in_block {
            in_block = !line.starts_with('}');
            continue;
        }
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        if line.starts_with("#[cfg(test)]") {
            pending = true;
            continue;
        }
        if line.starts_with("#[") {
            continue;
        }
        let module = ["mod ", "pub mod ", "pub(crate) mod ", "pub(super) mod "]
            .iter()
            .any(|m| line.starts_with(m));
        if pending && module {
            pending = false;
            in_block = !t.ends_with(';') && !t.ends_with('}');
            continue;
        }
        return Some(i + 1);
    }
    None
}

/// Refuses a count over the budget, naming the budget.
pub fn within_budget(total: usize) -> Result<(), String> {
    if total > BUDGET {
        Err(format!(
            "hyalOS's own code is {} lines over its budget of {BUDGET} (ADR-0133)",
            total - BUDGET
        ))
    } else {
        Ok(())
    }
}

/// Every breach of the `unsafe` rules, as `path:line: what`. Paths are relative to `hyalos-os/`.
pub fn unsafe_findings(files: &[(String, String)], listed: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for want in listed {
        if !files.iter().any(|(rel, _)| rel == want) {
            out.push(format!("{want}: on the unsafe list but not found"));
        }
    }
    for (rel, text) in files {
        if !rel.ends_with(".rs") {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !uses_unsafe(line) {
                continue;
            }
            let at = i + 1;
            if !listed.contains(&rel.as_str()) {
                out.push(format!("{rel}:{at}: unsafe in a file not on the list"));
            } else if !explained(&lines, i) {
                out.push(format!(
                    "{rel}:{at}: unsafe with no SAFETY comment within {SAFETY_REACH} lines above"
                ));
            }
        }
    }
    out
}

/// Whether line `i`, or one of the [`SAFETY_REACH`] lines above it, holds a `// SAFETY` comment or
/// a line of one: a comment that goes on over several lines still counts from its last line.
fn explained(lines: &[&str], i: usize) -> bool {
    let comment = |l: &str| l.trim_start().starts_with("//");
    (i.saturating_sub(SAFETY_REACH)..=i).any(|j| {
        if lines[j].contains("// SAFETY") {
            return true;
        }
        let mut k = j;
        while comment(lines[k]) {
            if lines[k].contains("// SAFETY") {
                return true;
            }
            match k.checked_sub(1) {
                Some(up) => k = up,
                None => break,
            }
        }
        false
    })
}

/// Whether a line uses the keyword: `unsafe` as a whole word, on a line that is not a comment.
/// Whatever follows it counts, so `unsafe /* a note */ {` or a form the design does not name is a
/// use too: the check errs towards failing. The lint's name, `unsafe_code`, is not a whole word.
fn uses_unsafe(line: &str) -> bool {
    if line.trim_start().starts_with("//") {
        return false;
    }
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(found) = line[from..].find("unsafe") {
        let start = from + found;
        let end = start + "unsafe".len();
        from = end;
        if !line[..start].chars().next_back().is_some_and(word)
            && !line[end..].chars().next().is_some_and(word)
        {
            return true;
        }
    }
    false
}

/// The Rust files under `hyalos-os/`, relative to it, with their text. Build output (`target/`, or
/// any directory Cargo tagged with `CACHEDIR.TAG`) and hidden directories are not hyalOS's code.
pub fn read(root: &Path) -> Result<Vec<(String, String)>, String> {
    let base = root.join("hyalos-os");
    let mut paths = Vec::new();
    walk(&base, &mut paths).map_err(|e| format!("could not read hyalos-os/: {e}"))?;
    let mut out = Vec::new();
    for path in paths {
        let rel = path.strip_prefix(&base).map_err(|e| e.to_string())?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        let text = fs::read_to_string(&path).map_err(|e| format!("hyalos-os/{rel}: {e}"))?;
        out.push((rel, text));
    }
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // Cargo marks every build directory with a `CACHEDIR.TAG`, whatever its name: the
            // monitor's tests build the kernel image into `kernel-image/target-selftest/`.
            let build = name == "target" || path.join("CACHEDIR.TAG").exists();
            if !build && !name.starts_with('.') {
                walk(&path, out)?;
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// `cargo xtask lines`' hyalOS half: reports the count against the budget and fails above it.
pub fn lines(root: &Path) -> Result<(), String> {
    let files = read(root)?;
    let counts = per_crate(&files);
    let total: usize = counts.values().sum();
    println!(
        "hyalOS: {total} / {BUDGET} lines ({:.1}%)",
        total as f64 * 100.0 / BUDGET as f64
    );
    for (name, n) in &counts {
        println!("  {name:<12} {n}");
    }
    let hidden = hidden_findings(&files);
    if !hidden.is_empty() {
        return Err(format!("{}\n(ADR-0133)", hidden.join("\n")));
    }
    within_budget(total)
}

/// `cargo xtask hyalos-unsafe`.
pub fn check_unsafe(root: &Path) -> Result<(), String> {
    let findings = unsafe_findings(&read(root)?, UNSAFE_FILES);
    if findings.is_empty() {
        println!(
            "hyalOS: unsafe only in the {} listed files, each use with a SAFETY comment",
            UNSAFE_FILES.len()
        );
        Ok(())
    } else {
        Err(format!(
            "{}\nunsafe is allowed only in the files on crates/xtask/src/hyalos.rs's list, each use \
             with a `// SAFETY` comment within {SAFETY_REACH} lines above (ADR-0133)",
            findings.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(rel: &str, text: &str) -> (String, String) {
        (rel.into(), text.into())
    }

    fn lines_of(n: usize) -> String {
        "fn a() {}\n".repeat(n)
    }

    #[test]
    fn core_s_05_1_tests_and_build_output_are_not_counted() {
        assert!(counted("monitor/src/kvm.rs"));
        assert!(counted("attest/build.rs"));
        assert!(!counted("monitor/src/runner_tests.rs"));
        assert!(!counted("verify/tests/common/mod.rs"));
        assert!(!counted("attest/tests/canary.rs"));
        assert!(!counted("ABI.md"));
        let files = [
            file(
                "monitor/src/a.rs",
                "fn a() {}\n// note\n\n#[cfg(test)]\nmod t {}\n",
            ),
            file("monitor/src/a_tests.rs", &lines_of(50)),
            file("guest/src/lib.rs", &lines_of(2)),
        ];
        let counts = per_crate(&files);
        assert_eq!(counts.get("monitor"), Some(&1));
        assert_eq!(counts.get("guest"), Some(&2));
    }

    #[test]
    fn core_s_05_1_code_after_a_test_module_fails() {
        let ok = "fn a() {}\n\n#[cfg(test)]\nmod a_tests;\n\n#[cfg(test)]\n#[path = \"x.rs\"]\n\
                  mod x;\n// note\n#[cfg(test)]\nmod t {\n    fn b() {}\n}\n#[cfg(test)]\nmod u {}\n";
        assert_eq!(
            hidden_findings(&[file("monitor/src/main.rs", ok)]),
            Vec::<String>::new()
        );
        for (text, at) in [
            ("#[cfg(test)]\nmod a_tests;\n\nfn real() {}\n", 4),
            (
                "fn a() {}\n#[cfg(test)]\nmod t {\n}\n#[inline]\nfn real() {}\n",
                6,
            ),
            ("#[cfg(test)]\nmod t;\nmod real;\n", 3),
            ("#[cfg(test)]\nfn helper() {}\n", 2),
        ] {
            let findings = hidden_findings(&[file("monitor/src/main.rs", text)]);
            assert_eq!(
                findings,
                [format!(
                    "monitor/src/main.rs:{at}: code after #[cfg(test)] is not counted; move it above the tests"
                )],
                "{text}"
            );
        }
        // A test file is not counted, so what follows its tests is not checked.
        assert_eq!(
            hidden_findings(&[file(
                "monitor/src/a_tests.rs",
                "#[cfg(test)]\nmod t;\nfn b() {}\n"
            )]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn core_s_05_1_hyalos_os_hides_no_code_after_its_tests() {
        assert_eq!(
            hidden_findings(&read(&crate::repo_root()).unwrap()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn core_s_05_1_e1_code_within_the_budget_passes() {
        let total: usize = per_crate(&[file("monitor/src/main.rs", &lines_of(BUDGET))])
            .values()
            .sum();
        assert_eq!(total, BUDGET);
        assert_eq!(within_budget(total), Ok(()));
    }

    #[test]
    fn core_s_05_1_e2_code_one_line_over_fails_naming_the_budget() {
        let total: usize = per_crate(&[
            file("monitor/src/main.rs", &lines_of(BUDGET)),
            file("guest/src/lib.rs", &lines_of(1)),
        ])
        .values()
        .sum();
        let err = within_budget(total).unwrap_err();
        assert!(
            err.contains("1 lines over") && err.contains("12000"),
            "{err}"
        );
    }

    #[test]
    fn core_s_05_1_hyalos_os_is_within_its_budget() {
        let files = read(&crate::repo_root()).unwrap();
        let total: usize = per_crate(&files).values().sum();
        assert!(total > 0, "no hyalOS code was found");
        assert_eq!(within_budget(total), Ok(()));
    }

    #[test]
    fn core_s_05_2_e1_unsafe_in_a_listed_file_with_a_safety_comment_passes() {
        let text = "// SAFETY: the pointer is valid.\nlet a = unsafe { *p };\n\
                    // SAFETY: the kernel copies it.\n\nunsafe impl Send for Ram {}\n\
                    #[unsafe(no_mangle)] // SAFETY: the one entry point.\n\
                    let ok = unsafe_code; // the lint's name is not a use\n\
                    // an unsafe { comment } is not a use\n";
        assert_eq!(
            unsafe_findings(&[file("monitor/src/kvm.rs", text)], &["monitor/src/kvm.rs"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn core_s_05_2_e2_unsafe_in_an_unlisted_file_fails() {
        for code in [
            "unsafe { x() }",
            "unsafe fn f() {}",
            "unsafe impl Send for T {}",
            "unsafe trait T {}",
            "unsafe extern \"C\" {}",
            "#[unsafe(no_mangle)]",
            "let a = unsafe{ x() };",
            "let a = unsafe\n{ x() };",
            "let a = unsafe /* a note */ { x() };",
            "type B = unsafe<'a> &'a u8;",
            "pub(crate) unsafe static X: u8;",
        ] {
            let text = format!("// SAFETY: a comment does not admit a file.\n{code}\n");
            let files = [
                file("monitor/src/kvm.rs", ""),
                file("monitor/src/ledger.rs", &text),
            ];
            let findings = unsafe_findings(&files, &["monitor/src/kvm.rs"]);
            assert_eq!(findings.len(), 1, "{code}: {findings:?}");
            assert!(findings[0].starts_with("monitor/src/ledger.rs:2: unsafe in a file not on"));
        }
    }

    #[test]
    fn core_s_05_2_unsafe_with_no_safety_comment_fails() {
        let text = "// SAFETY: too far.\n\n\n\nlet a = unsafe { *p };\n";
        let findings =
            unsafe_findings(&[file("monitor/src/kvm.rs", text)], &["monitor/src/kvm.rs"]);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].starts_with("monitor/src/kvm.rs:5: unsafe with no SAFETY"));
    }

    #[test]
    fn core_s_05_2_a_safety_comment_may_go_on_over_several_lines() {
        let text = "// SAFETY: a reason that\n// takes three lines\n// to give.\nunsafe impl A for B {}\n\
                    unsafe fn f() {}\n";
        assert_eq!(
            unsafe_findings(&[file("monitor/src/kvm.rs", text)], &["monitor/src/kvm.rs"]),
            Vec::<String>::new()
        );
        let text = "// SAFETY: a reason.\nfn a() {}\nfn b() {}\nfn c() {}\n// a note, not a reason\nunsafe { x() };\n";
        assert_eq!(
            unsafe_findings(&[file("monitor/src/kvm.rs", text)], &["monitor/src/kvm.rs"]).len(),
            1
        );
    }

    #[test]
    fn core_s_05_2_a_stale_list_entry_fails() {
        let findings = unsafe_findings(&[], &["monitor/src/gone.rs"]);
        assert_eq!(
            findings,
            ["monitor/src/gone.rs: on the unsafe list but not found"]
        );
    }

    #[test]
    fn core_s_05_2_hyalos_os_keeps_the_unsafe_rules() {
        let findings = unsafe_findings(&read(&crate::repo_root()).unwrap(), UNSAFE_FILES);
        assert_eq!(findings, Vec::<String>::new());
    }
}
