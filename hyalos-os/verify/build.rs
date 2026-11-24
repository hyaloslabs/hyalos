//! Gives `--version` the commit the build script names. Without one it says `unknown`.

include!("src/commit.rs");

fn main() {
    println!("cargo:rerun-if-env-changed=HYALOS_BUILD_COMMIT");
    println!("cargo:rerun-if-changed=src/commit.rs");
    let commit = std::env::var("HYALOS_BUILD_COMMIT").ok();
    println!(
        "cargo:rustc-env=HYALOS_VERIFY_COMMIT={}",
        named(commit.as_deref())
    );
}
