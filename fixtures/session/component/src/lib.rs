//! Fixture: a WebAssembly component without handwritten boundary code.
//!
//! Everything this crate adds to `session-owner` is generated:
//!
//! - `wit/session.wit`, the contract;
//! - `src/bindings.rs`, the contract's part of the `wit_bindgen::generate!` call;
//! - `src/forwarding.rs`, the implementation of the binding traits.
//!
//! The three files are written by `cargo run -p session-owner --bin generate`.

// Defines `generate_bindings!`, which knows the world, the types that cross
// unchanged and the derive that forwarding needs.
include!("bindings.rs");

// Only the location of the WIT files is this crate's own.
generate_bindings! { path: "wit" }

// Generated text is not formatted; it is compared byte for byte instead.
#[rustfmt::skip]
mod forwarding;

// `export!` takes the type by name, not by path.
use forwarding::Component;

export!(Component);

#[cfg(test)]
mod tests {
    use session_owner::{MachoValue, Progress, Session, StepKind};
    use wit_reflect::component::ResourceCell;

    use crate::exports::example::session::binary::{Endianness, ErrorKind};
    use crate::exports::native::{GuestSession, InspectionFailureKind};

    /// The operations run through the generated forwarding and return the
    /// values the bindings pass on. Each assertion is one rule, one entry of
    /// the owner's representation map or one piece of structure that lowering
    /// and assembly must agree on.
    ///
    /// Where a binding is annotated with a `session_owner` type, the bindings
    /// use that owner type itself, so the annotation only compiles while the
    /// type crosses unchanged.
    ///
    /// Resource handles exist only inside a Wasm instance, so the session is
    /// placed in its cell directly instead of through the factory.
    #[test]
    fn operations_return_lowered_values_through_generated_forwarding() {
        // 1. `&mut self` and `&self` methods reach the same retained session.
        let session: ResourceCell<Session> = ResourceCell::new(session_owner::create_session());
        session.select("application".to_owned());
        let selection = session.selection();

        // 2. A successful inspection, lowered through nested generic carriers.
        let first_outcome = session.inspect("application".to_owned());
        let second_outcome = session.inspect("application".to_owned());

        // 3. Failures: a stage with a skipped cause, a struct-like case and a
        //    foreign error exported as text.
        let unselected_outcome = session.inspect("other".to_owned());
        session.select("truncated".to_owned());
        let truncated_outcome = session.inspect("truncated".to_owned());
        session.select("undecodable".to_owned());
        let undecodable_outcome = session.inspect("undecodable".to_owned());

        // 4. An unchanged record, returned as the owner type.
        let progress: Progress = session.progress();

        assert_eq!(selection.as_deref(), Some("application"));

        let first_step = &first_outcome.steps[0];
        // An unchanged enum held by a converted generic carrier.
        let first_kind: StepKind = first_step.kind;
        assert_eq!(first_kind, StepKind::Inspect);
        let report = first_step
            .result
            .as_ref()
            .expect("the selection is inspected");
        assert_eq!(report.byte_length, 4096);
        let image = &report.images[0];
        // a map entry naming a `remote` enum
        assert!(matches!(image.endianness, Endianness::Little));
        // a map entry converted through `From`, producing an unchanged value
        let file_type: &MachoValue = &image.file_type;
        assert_eq!(
            (file_type.value, file_type.name.as_deref()),
            (2, Some("MH_EXECUTE"))
        );
        // `with`: the name depends on the sibling file type
        assert_eq!(
            (image.cpu_subtype.value, image.cpu_subtype.name.as_deref()),
            (2, Some("ARM64_ALL"))
        );
        // a map entry shown as `DisplayText`
        assert_eq!(image.minimum_os, "14.4");
        // private fields, read by the derive
        assert_eq!((image.file_range.offset, image.file_range.size), (0, 4096));
        // a `usize` shown as `u64`, and state kept across `&mut self` calls
        assert_eq!(image.read_count, 1);
        let second_report = second_outcome.steps[0]
            .result
            .as_ref()
            .expect("inspected again");
        assert_eq!(second_report.images[0].read_count, 2);

        // A computed message on an enum, and `skip` leaving the cause out.
        let unselected = unselected_outcome.steps[0]
            .result
            .as_ref()
            .expect_err("not selected");
        assert_eq!(
            unselected.message,
            "cannot validate selection: filesystem operation failed: `other` is not selected"
        );
        let InspectionFailureKind::Selection(failure) = &unselected.kind else {
            panic!("expected the selection case, got {:?}", unselected.kind);
        };
        assert_eq!(
            failure.message,
            "filesystem operation failed: `other` is not selected"
        );

        // A struct-like case as its declared payload record.
        let truncated = truncated_outcome.steps[0]
            .result
            .as_ref()
            .expect_err("truncated");
        let InspectionFailureKind::Analysis(analysis) = &truncated.kind else {
            panic!("expected the analysis case, got {:?}", truncated.kind);
        };
        assert_eq!(analysis.message, "requested 32 bytes, 8 available");
        assert!(matches!(
            &analysis.kind,
            ErrorKind::Truncated(range) if range.requested_size == 32 && range.available_size == 8
        ));

        // A map entry on a case payload.
        let undecodable = undecodable_outcome.steps[0]
            .result
            .as_ref()
            .expect_err("undecodable");
        let InspectionFailureKind::Analysis(analysis) = &undecodable.kind else {
            panic!("expected the analysis case, got {:?}", undecodable.kind);
        };
        assert!(matches!(&analysis.kind, ErrorKind::Decode(text) if text == "invalid magic"));

        // Unchanged values arrive intact, also inside an option.
        assert_eq!(
            (progress.last_step, progress.inspections),
            (Some(StepKind::Inspect), 5)
        );
    }
}
