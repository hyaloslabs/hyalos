//! `hyalos-mcp-fixtures`: serves the fixtures on a free loopback port and prints the URL, for running
//! the conformance suite by hand (see `tests/conformance.rs` for the command).

fn main() -> std::process::ExitCode {
    match hyalos_mcp_conformance::serve_fixtures() {
        Ok(url) => {
            println!("{url}");
            loop {
                std::thread::park();
            }
        }
        Err(e) => {
            eprintln!("hyalos-mcp-fixtures: {e}");
            std::process::ExitCode::from(1)
        }
    }
}
