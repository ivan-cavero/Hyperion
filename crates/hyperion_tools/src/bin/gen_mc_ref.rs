//! Run all mc-ref offline generators in order.
//!
//! ```text
//! cargo run -p hyperion_tools --bin gen-mc-ref
//! ```

use std::process::{Command, ExitCode, Stdio};

fn main() -> ExitCode {
    let bins = ["gen-join-data", "gen-registry-nbt", "gen-block-states"];
    for bin in bins {
        println!("==> cargo run -p hyperion_tools --bin {bin}");
        let status = Command::new(env!("CARGO"))
            .args(["run", "-p", "hyperion_tools", "--bin", bin])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("gen-mc-ref: {bin} failed with {s}");
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("gen-mc-ref: failed to spawn cargo for {bin}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    println!("gen-mc-ref: all generators finished");
    ExitCode::SUCCESS
}
