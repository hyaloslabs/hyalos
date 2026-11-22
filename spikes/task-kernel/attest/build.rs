//! Takes the TLSNotary tag from the manifest, so the release identifier comes from the build.

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    let manifest = std::fs::read_to_string("Cargo.toml").expect("the manifest");
    let line = manifest.lines().find(|line| line.starts_with("tlsn = ")).expect("a tlsn dependency");
    let tag = line.split("tag = \"").nth(1).and_then(|rest| rest.split('"').next()).expect("a pinned tlsn tag");
    println!("cargo:rustc-env=ATTEST_TLSN_TAG={tag}");
}
