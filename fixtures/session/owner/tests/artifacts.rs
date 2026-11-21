//! The committed artifacts of the session fixture.

use std::error::Error;

/// The files the component crate compiles, as they are committed.
const COMMITTED_WIT: &str = include_str!("../../component/wit/session.wit");
const COMMITTED_FORWARDING: &str = include_str!("../../component/src/forwarding.rs");

/// The component is built from committed files, so they must be exactly what
/// the schema generates. A difference means a Rust declaration or the schema
/// changed without regenerating, or the generated text changed unreviewed.
///
/// Refresh the files with `cargo run -p session-owner --bin generate`.
#[test]
fn committed_artifacts_are_what_the_schema_generates() -> Result<(), Box<dyn Error>> {
    let generated = session_owner::generate()?;

    assert_eq!(generated.wit, COMMITTED_WIT, "wit/session.wit is stale");
    assert_eq!(
        generated.forwarding, COMMITTED_FORWARDING,
        "src/forwarding.rs is stale"
    );
    Ok(())
}
