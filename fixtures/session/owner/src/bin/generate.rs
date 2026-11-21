//! Write or check the generated artifacts of the session fixture.
//!
//! - `generate` rewrites the committed WIT, bindings file and forwarding file.
//! - `generate --check` changes nothing and fails if a committed file differs
//!   from what the declarations generate.
//!
//! Checking and refreshing are separate commands on purpose: a check must
//! never repair what it finds.

use std::process::ExitCode;

// The marked items register themselves; linking their crate is all it takes.
use session_owner as _;

/// The manifest of the component crate that includes the artifacts. It holds
/// the contract settings.
const COMPONENT_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../component/Cargo.toml");

fn main() -> ExitCode {
    wit_reflect::tasks::run(COMPONENT_MANIFEST)
}
