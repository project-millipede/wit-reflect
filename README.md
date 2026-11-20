# wit-reflect

Generate WIT directly from the Rust types an implementation already uses.

**Status: Phase 1 of 5 (type reflection) is implemented.** The library renders
validated WIT type definitions. Functions, resources, worlds, representation rules
and generated adapters are later phases; see [the phase plan](docs/phases.md).

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

## How it works

| Step | Owner | What happens |
| --- | --- | --- |
| Reflect | `#[derive(WitType)]` in the crate that owns the type | Fields are described as declared. The Rust compiler has already resolved aliases, re-exports, dependency crates and generic arguments. |
| Declare | `Schema` in a generator program | Interfaces, the types each one defines, and names for generic instantiations. |
| Resolve | `Schema::resolve` | Every type gets one owning interface and one name; all problems are reported together. |
| Render | `ResolvedPackage: Display` | Deterministic WIT text with keyword escaping and `use` statements. |

Rules worth knowing:

- **Instantiations are never merged.** `Outcome<A>` and `Outcome<B>` are two
  definitions. If both keep the default name, resolution fails and names both
  Rust types; give each a name with `define_as`.
- **Each type has one owning interface.** A declared type belongs where it is
  declared. An undeclared dependency belongs to the only interface that reaches
  it. A type reached from two interfaces must be declared in one of them.
- **Unsupported shapes fail early.** `usize`, borrowed data, unions and
  field-less records are compile errors at the owner type. Nothing is
  downgraded to a string or skipped with a warning.

To keep an owner crate's ordinary build free of this library, gate the derive:

```rust
#[cfg_attr(feature = "wit", derive(wit_reflect::WitType))]
pub struct BinaryReport { /* fields */ }
```

## Layout

```text
crates/wit-reflect/          reflection trait, schema, resolution, rendering
crates/wit-reflect-macros/   #[derive(WitType)]
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

Functions, resources and factories, worlds, references to external WIT packages,
representation rules (formatted values, projections, `usize` widening), generated
lowering and `Guest` forwarding. No Wasm target, wit-bindgen or jco run has been
performed with generated output yet.
