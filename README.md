# wit-reflect

Generate WIT directly from the Rust types an implementation already uses.

**Status: Phases 1 and 2 of 5 are implemented.** The library renders a validated
WIT contract: types, functions, resources and worlds. Representation rules and
the generated adapters that connect the contract to the Rust implementation are
later phases; see [the phase plan](docs/phases.md).

## Why

The generator comparison in the Apple Code Workbench repository
(`docs/libraries/rust-to-wit-generators/README.md`) found that Linera Witty,
WITGenstein and Fidius each needed a handwritten copy of the data model before
they could produce a contract. This library removes that
copy: the owner types are the only definition, and declarative metadata selects
names and placement.

It depends on none of the three. Their checkouts next to this folder are read-only
references.

## Example

```rust
use wit_reflect::{PackageName, Schema, WitType};

/// Completed steps in execution order.
#[derive(WitType)]
struct Outcome<Step> {
    steps: Vec<Step>,
}

#[derive(WitType)]
struct Report {
    byte_length: u64,
}

let package_name: PackageName = "example:analysis@0.1.0".parse()?;
let mut schema = Schema::new(package_name);
schema
    .interface("inspection")
    .define_as::<Outcome<Report>>("report-outcome");

print!("{}", schema.resolve()?);
```

```wit
package example:analysis@0.1.0;

interface inspection {
  record report {
    byte-length: u64,
  }

  /// Completed steps in execution order.
  record report-outcome {
    steps: list<report>,
  }
}
```

Operations are marked where they are declared and selected where the contract
is declared:

```rust
use wit_reflect::{WitResource, export, operation};

#[derive(WitResource)]
struct Session { /* fields are not exported */ }

impl Session {
    #[export]
    fn select(&mut self, resource: &str) { /* … */ }
}

#[export]
fn create_session() -> Session { /* … */ }

let mut native = schema.interface("native");
native.resource::<Session>().method(operation!(Session::select));
native.function(operation!(create_session));
schema.world("files").export("native");
```

```wit
interface native {
  resource session {
    select: func(%resource: string);
  }

  create-session: func() -> own<session>;
}

world files {
  export native;
}
```

## How it works

| Step | Owner | What happens |
| --- | --- | --- |
| Reflect | `#[derive(WitType)]`, `#[derive(WitResource)]` and `#[export]` in the crate that owns the item | Fields and signatures are described as declared. The Rust compiler has already resolved aliases, re-exports, dependency crates and generic arguments. |
| Declare | `Schema` in a generator program | Interfaces, the types and operations each one exports, names for generic instantiations, external interfaces and worlds. |
| Resolve | `Schema::resolve` | Every type gets one owning interface and one name; all problems are reported together. |
| Render | `ResolvedPackage: Display` | Deterministic WIT text with keyword escaping and `use` statements. |

Rules worth knowing:

- **Instantiations are never merged.** `Outcome<A>` and `Outcome<B>` are two
  definitions. If both keep the default name, resolution fails and names both
  Rust types; give each a name with `define_as`.
- **Each type has one owning interface.** A declared type belongs where it is
  declared. An undeclared dependency belongs to the only interface that reaches
  it. A type reached from two interfaces must be declared in one of them.
- **Operations are selected, never collected.** `#[export]` only reflects a
  signature. A method is part of the contract when the schema lists it, so
  methods from several `impl` blocks combine and unlisted ones stay private.
- **Handles follow the Rust signature.** A resource passed or returned by value
  is `own<r>`; behind a reference it is `borrow<r>`.
- **Dependency types are imported.** A type placed with
  `Schema::external_interface` is brought in with `use` and never redefined.
- **Unsupported shapes fail early.** `usize`, borrowed data, unions, field-less
  records, `async fn`, generic functions and `&mut` parameters are compile
  errors at the declaration. Nothing is downgraded to a string or skipped with
  a warning.

To keep an owner crate's ordinary build free of this library, gate the macros:

```rust
#[cfg_attr(feature = "wit", derive(wit_reflect::WitType))]
pub struct BinaryReport { /* fields */ }

impl Session {
    #[cfg_attr(feature = "wit", wit_reflect::export)]
    pub fn inspect(&mut self, resource: &str) -> InspectionOutcome { /* … */ }
}
```

## Layout

```text
crates/wit-reflect/          reflection traits, schema, resolution, rendering
crates/wit-reflect-macros/   derives, #[export] and operation!
docs/phases.md               definitions, phases and their acceptance evidence
docs/rust-practices.md       Rust practices applied in this workspace
```

## Checks

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

## Not yet supported

- Representation rules: formatted values, projections, `usize` widening.
- Generated lowering and `Guest` forwarding. Until then the generated WIT has no
  generated implementation behind it.
- `async fn`, methods that consume `self`, and operations that do not map to
  exactly one Rust function.
- Comparing a type placed in an external interface with that interface's WIT.

No Wasm target, wit-bindgen or jco run has been performed with generated output
yet.
