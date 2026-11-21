//! The committed artifacts of the session fixture.

use std::error::Error;

// The marked items register themselves; linking their crate is all it takes.
use session_owner as _;

/// The manifest of the component crate that includes the artifacts.
const COMPONENT_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../component/Cargo.toml");

/// The component is built from committed files, so they must be exactly what
/// the declarations generate. A difference means a Rust declaration or a
/// setting changed without regenerating, or the generated text changed
/// unreviewed.
///
/// Refresh the files with `cargo run -p session-owner --bin generate`.
#[test]
fn committed_artifacts_are_what_the_declarations_generate() -> Result<(), Box<dyn Error>> {
    wit_reflect::tasks::check(COMPONENT_MANIFEST)?;
    Ok(())
}
