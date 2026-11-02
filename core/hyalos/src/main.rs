//! The `hyalos` binary. The engine and broker run from this one executable (ADR-0002).

fn main() {
    println!("hyalos {}", env!("CARGO_PKG_VERSION"));
}
