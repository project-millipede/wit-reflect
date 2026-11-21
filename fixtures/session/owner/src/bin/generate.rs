//! Write or check the generated artifacts of the session fixture.
//!
//! - `generate` rewrites the committed WIT, bindings file and forwarding file.
//! - `generate --check` changes nothing and fails if a committed file differs
//!   from what the schema generates.
//!
//! Checking and refreshing are separate commands on purpose: a check must
//! never repair what it finds.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use session_owner::Artifacts;

/// The component crate that includes the artifacts.
const COMPONENT_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../component");

fn main() -> ExitCode {
    let check_only = match parse_arguments() {
        Ok(check_only) => check_only,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    match run(check_only) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Returns whether only a check was requested.
fn parse_arguments() -> Result<bool, String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [] => Ok(false),
        [flag] if flag == "--check" => Ok(true),
        _ => Err("usage: generate [--check]".to_owned()),
    }
}

/// Returns whether every artifact is current after the run.
fn run(check_only: bool) -> Result<bool, Box<dyn Error>> {
    let Artifacts {
        wit,
        bindings,
        forwarding,
    } = session_owner::generate()?;
    let component_directory = Path::new(COMPONENT_DIRECTORY);
    let generated_files = [
        (component_directory.join("wit/session.wit"), wit),
        (component_directory.join("src/bindings.rs"), bindings),
        (component_directory.join("src/forwarding.rs"), forwarding),
    ];

    let mut all_current = true;
    for (path, generated_contents) in generated_files {
        let is_current = is_current(&path, &generated_contents)?;
        if is_current {
            continue;
        }
        if check_only {
            eprintln!(
                "stale: {}\n  run `cargo run -p session-owner --bin generate` to refresh it",
                path.display()
            );
            all_current = false;
        } else {
            fs::write(&path, generated_contents)?;
            println!("wrote {}", path.display());
        }
    }
    Ok(all_current)
}

/// Whether the file at `path` already holds `generated_contents`.
///
/// A missing file is stale, not an error.
fn is_current(path: &PathBuf, generated_contents: &str) -> Result<bool, Box<dyn Error>> {
    let read_result = fs::read_to_string(path);
    match read_result {
        Ok(committed_contents) => Ok(committed_contents == generated_contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("cannot read {}: {error}", path.display()).into()),
    }
}
