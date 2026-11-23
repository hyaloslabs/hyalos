fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    println!("cargo:rustc-link-arg=-T{dir}/kernel.ld");
    println!("cargo:rerun-if-changed=kernel.ld");
}
