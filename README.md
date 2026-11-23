# wit-reflect

Generate a WIT contract, and the code behind it, from the Rust types and
functions an implementation already has.

**Status: Phases 1 to 11 are implemented.** From ordinary Rust the library
generates the WIT contract, the bindings setup, the conversion of results and
the code that forwards each exported function to its Rust operation. The
contract is declared on the items themselves; no schema program is needed. A
fixture component built this way runs under jco, and one production component
is generated with it. See [the phase plan](docs/phases.md) for the evidence and
the limits.

New here? Read [the developer guide](docs/guide.md): what you write, what is
generated, what is not, and a complete worked example.

## Why

The generator comparison in the Apple Code Workbench repository
(`docs/libraries/rust-to-wit-generators/README.md`) found that Linera Witty,
WITGenstein and Fidius each needed a handwritten copy of the data model before
they could produce a contract. This library removes that copy: the owner types
are the only definition, and each item states its own part of the contract.

It depends on none of the three.

## Install

```toml
[dependencies]
wit-reflect = "0.1"
```

A crate that should build without this library puts it behind a feature of
its own; see [Keeping an owner crate independent](#keeping-an-owner-crate-independent).

## Example

### 1. Types and operations, in the crate that owns them

```rust
use wit_reflect::{WitResource, WitType, export, exports, named};

#[derive(WitType)]
#[wit(interface = "binary")]              // an interface has to own this type
pub struct Report {
    pub byte_length: u64,
    pub images: Vec<Image>,               // `Image` is found through this field
}

#[derive(WitType)]
pub struct Image {
    pub endianness: object::Endianness,   // a type of another crate, see step 2
    pub file_range: FileRange,            // private fields are fine
    pub read_count: usize,                // shown as u64
}

#[derive(WitType)]
pub struct Outcome<Step> {
    pub steps: Vec<Step>,
}

#[named]                                  // WIT has no generics: names this instantiation
pub type InspectionOutcome = Outcome<Report>;

#[derive(WitResource)]
pub struct Session { /* fields are not exported */ }

#[exports(inspect)]                       // the functions of this block that are exported
impl Session {
    pub fn inspect(&mut self, resource: &str) -> InspectionOutcome { /* … */ }

    pub fn read_at(&mut self, offset: u64) -> Vec<u8> { /* public, not exported */ }
}

#[export(interface = "native")]           // an entry point states its interface
pub fn create_session() -> Session { /* … */ }
```

Only `Report` and `create_session` name an interface. `Session` belongs to
`native` because that function returns it, and every other type is found
through the fields and signatures that refer to it.

### 2. Types of other crates, once

A type of another crate cannot carry a derive. The crate states once how each
one is shown, and fields of those types then need no rule:

```rust
#[derive(WitType)]
#[wit(remote = object::Endianness)]       // checked against the foreign enum
pub enum Endianness { Little, Big }

wit_reflect::representations! {
    pub Representations {
        object {
            object::Endianness => Endianness,          // a local enum
            object::macho::CpuType | object::macho::FileType => MachoValue
                with |value| MachoValue::new(value.into_inner(), value.name()),
            object::macho::Version => DisplayText,     // its `Display` text
        }
        std {
            std::io::Error => DisplayText,
        }
    }
}
```

```toml
# Cargo.toml of the crate that has the map
[package.metadata.wit-reflect]
representations = "crate::Representations"
```

An entry is `Foreign => Shown`. It converts through `impl From<Foreign> for
Shown`, or through the conversion it states after `with`. Several foreign types
separated by `|` share an entry, so one conversion is written once; the types
need no trait in common. `MachoValue` is a local record `{ value, name }`, and
`DisplayText` is the library's type for a value's `Display` text. Entries stand
in groups named after where their types come from.

### 3. Rules, where a representation differs from what is stored

```rust
#[derive(WitType)]
#[wit(display = "message", skip(cause))]  // its text as a field; one field left out
pub struct Failure {
    pub kind: FailureKind,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

#[derive(WitType)]
pub struct CpuIdentity {
    pub cpu_type: macho::CpuType,
    #[wit(into = MachoValue, with = CpuIdentity::subtype_value)]  // depends on a sibling
    pub cpu_subtype: macho::CpuSubtype,
}
```

```wit
record failure {
  message: string,
  kind: failure-kind,
}
```

### 4. Package and world, in the component crate's `Cargo.toml`

```toml
[package.metadata.wit-reflect]
package = "example:session@0.1.0"
world = "session"
exports = [{ interface = "native", name = "native" }, "binary"]
```

### 5. The generator program

```rust
use session_owner as _;   // links the crate whose items are marked

fn main() -> std::process::ExitCode {
    wit_reflect::tasks::run("fixtures/session/component/Cargo.toml")
}
```

It writes three files into the component crate. With `--check` it changes
nothing and fails if one of them is stale.

### 6. What is generated

```wit
// wit/session.wit
package example:session@0.1.0;

interface native {
  use binary.{report};

  record inspection-outcome {
    steps: list<report>,
  }

  resource session {
    inspect: func(%resource: string) -> inspection-outcome;
  }

  create-session: func() -> own<session>;
}

world session {
  export native: native;
  export binary;
}
```

```rust
// src/bindings.rs (one of its three `with` entries left out)
macro_rules! generate_bindings {
    ($($options:tt)*) => {
        ::wit_bindgen::generate!({
            world: "example:session/session@0.1.0",
            additional_derives: [::wit_reflect::FromParts],
            with: {
                "example:session/native@0.1.0/step-kind": session_owner::StepKind,
                "example:session/native@0.1.0/progress": session_owner::Progress,
            },
            $($options)*
        });
    };
}
```

```rust
// src/forwarding.rs
impl crate::exports::native::GuestSession for ResourceCell<session_owner::Session> {
    fn inspect(&self, resource: String) -> crate::exports::native::InspectionOutcome {
        let returned = session_owner::Session::inspect(&mut self.borrow_mut(), &resource);
        ::wit_reflect::lower::assemble(returned)
    }
}
```

The component crate then contains nothing else written by hand:

```rust
include!("bindings.rs");
generate_bindings! { path: "wit" }

#[rustfmt::skip]
mod forwarding;
use forwarding::Component;

export!(Component);
```

The snippets above are simplified. `fixtures/session` is a complete, compiling
example of the same shape: `owner` holds the ordinary Rust with its marks and
its map, `component` holds the contract settings and the three generated
files. The bindings and forwarding shown here are taken from it.

### Types that cross unchanged

Each `with` entry above is a type the bindings use as it is: no second type is
generated for it and its values are moved, not converted. Nothing declares
this. A struct or enum crosses unchanged when:

- it is `pub`, has no type parameters and is `Clone` and `Debug`;
- no `#[wit(...)]` attribute changes its shape, a value or a member's name;
- a struct has only `pub` fields, an enum only unit cases without explicit
  discriminants;
- no field is a `usize`, an `isize` or a type the crate's map shows as another
  type;
- every type inside it crosses unchanged too.

Every other type is converted, as `InspectionOutcome` is in the forwarding
above. `#[wit(convert)]` forces conversion, and `#[wit(path = …)]` names a type
that is not exported at its crate's root.

## What you write

| You write | On | It states |
| --- | --- | --- |
| `#[derive(WitType)]` | a struct or enum | the type is part of the contract |
| `#[derive(WitResource)]` | a struct | the type is a resource |
| `#[wit(interface = "…")]` | a derived type or resource | the interface that owns it, where nothing else settles that |
| `#[exports(a, b)]` | an `impl` block | the functions of the block that are exported |
| `#[exports(interface = "…", a)]` | a trait | functions of an interface that the component crate implements |
| `#[export(interface = "…")]` | a free function | the interface that exports it |
| `#[named]` | a type alias | the name of that instantiation of a generic type |
| `representations! { … }` | once in a crate | how each type of another crate is shown |
| `#[wit(display = "…")]`, `#[wit(skip(a, b))]` | a type | a text field; fields left out |
| `#[wit(into = W, with = f)]`, `#[wit(display)]`, `#[wit(skip)]` | a field | a representation for that one field |
| `[package.metadata.wit-reflect]` | the component crate's `Cargo.toml` | package, world, imports, exports |

## How it works

| Step | Owner | What happens |
| --- | --- | --- |
| Reflect | The derives and marks, in the crate that owns the item | Fields and signatures are described as declared. The Rust compiler has already resolved aliases, re-exports, dependency crates and generic arguments. |
| Declare | The same marks, collected at link time; settings in the component crate's `Cargo.toml` | Interfaces, the types and operations each one exports, names for generic instantiations, and the world. |
| Resolve | `Schema::resolve` | Every type gets one owning interface and one name; all problems are reported together. |
| Render | `wit_reflect::tasks` | Deterministic WIT text, the contract's part of the `wit_bindgen::generate!` call, and the Rust file that implements wit-bindgen's traits for the world. |
| Convert | `Lower`, derived with `WitType`; `FromParts`, derived on the bindings | A result is taken apart in declaration order with its rules applied, and the binding value is assembled by position. Neither side names the other's types. A type the bindings can use as it is crosses unchanged instead. |

Rules worth knowing:

- **The contract does not shape the code.** Whether a function or field is
  exported never follows from `pub`, from how an `impl` block is laid out or
  from the crate a type is in. It is stated, and every field crosses unless a
  rule skips it.
- **Operations are exported deliberately.** A method joins the contract when
  `#[exports(…)]` on its `impl` block names it. Methods from several blocks
  combine. A block may hold public functions that are not exported.
- **Only a root states an interface.** A function of an interface is a root;
  so is a type that names its interface. Everything else, a resource
  included, belongs to the only interface that reaches it. An item reached
  from two interfaces, or from none, must name one.
- **A function the owner crate cannot implement is declared on a trait.**
  `#[exports(interface = "…", f)]` on the trait exports the function, and
  the component crate implements the trait for its generated `Component`
  type. The implementation carries no mark, and a missing one does not
  compile.
- **Instantiations are never merged.** `Outcome<A>` and `Outcome<B>` are two
  definitions. Each needs a name: mark an alias of it with `#[named]`. No name
  is composed from the type arguments.
- **A foreign type is described once.** For every field without a rule the
  compiler picks the map's entry for its type, or the type itself. A rule on a
  field wins, and a type the map does not name is not covered.
- **Platform-sized integers are widened.** WIT has no `usize`: it is 64 bits
  natively and 32 bits in a component. `usize` and `isize` are exported as
  `u64` and `s64`, which hold every value. No rule is needed.
- **Rules are checked where they are written.** `into` needs a `From` impl,
  `display` needs `Display`, a `remote` declaration is matched exhaustively
  against the foreign type. A rule or entry that no longer holds stops
  compiling.
- **Registrations need their crate linked.** Rust links a dependency only if
  the program names it. A generator writes `use owner as _;`; with nothing
  registered it fails instead of generating an empty contract.
- **Generated artifacts are committed and checked.** One command rewrites the
  WIT, the bindings file and the forwarding file; another fails when they are
  stale and changes nothing.
- **A stale contract does not compile.** If the bindings and an owner type
  disagree, the compiler names the binding type and the parts it was given.
- **Unchanged types are decided by the compiler.** The derive sees one type;
  whether everything inside it also qualifies is computed from the field types.
- **Handles follow the Rust signature.** A resource passed or returned by value
  is `own<r>`; behind a reference it is `borrow<r>`.
- **Unsupported shapes fail early.** Borrowed data, unions, field-less
  records, `async fn`, generic functions and `&mut` parameters are compile
  errors at the declaration. Nothing is downgraded to a string or skipped with
  a warning.

## Keeping an owner crate independent

Gate the macros behind a default-off feature, so the crate's ordinary build
does not compile this library. One `cfg_attr` takes several attributes, and
rules can be stated on the type, so a gated type needs one block above it and
nothing on its fields:

```rust
#[cfg_attr(
    feature = "wit",
    derive(wit_reflect::WitType),
    wit(display = "message", skip(diagnostic, cause))
)]
pub struct Failure { /* plain fields */ }

#[cfg_attr(feature = "wit", wit_reflect::exports(inspect))]
impl Session { /* plain functions */ }
```

| Feature of wit-reflect | Default | For |
| --- | --- | --- |
| `derive` | yes | the derives and the other macros |
| `tasks` | no | a generator program: reads the contract settings, writes or checks the files |
| `representations` | no | a crate that names its map in `Cargo.toml` |

The registry, the tasks and the map setting exist only on non-Wasm targets. A
component is built from the generated files and carries none of them.

## Declaring with a schema program instead

The marks are a front end to `Schema`. A program can also declare a contract
call by call, for example to generate two contracts from the same crates, or
to place a type in an interface of another package:

```rust
use wit_reflect::{Schema, operation};

let mut schema = Schema::new("example:session@0.1.0".parse()?);
schema.interface("binary").define::<Report>();

let mut native = schema.interface("native");
native.define_as::<Outcome<Report>>("inspection-outcome");
native
    .resource::<Session>()
    .method(operation!(session_owner::Session::inspect));
schema.world("session").export_as("native", "native").export("binary");

let package = schema.resolve()?;
let wit = package.to_string();
let bindings = package.bindings("session")?;
let forwarding = package.forwarding("session")?;
```

A function listed this way carries `#[export]`, which reflects its signature.

## Layout

```text
crates/wit-reflect/          reflection, schema, registry, resolution, rendering, lowering, tasks
crates/wit-reflect-macros/   the derives, #[export], #[exports], #[named], representations! and operation!
fixtures/session/owner/      example library with its marks and its map, and the generate binary
fixtures/session/component/  example component: contract settings and generated artifacts
docs/guide.md                the developer guide with the worked example
docs/issues/                 one document per open design question
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

```bash
cargo run -p session-owner --bin generate -- --check
```

## Not yet supported

- Parameters that are records, variants or enums, owned resource parameters,
  and `usize` or `isize` parameters. Plain values and borrowed resources are
  passed.
- A resource handle nested inside a result. A function may return one directly.
- `with` on fields inside enum cases, and `remote` declarations for foreign
  types with private fields or `#[non_exhaustive]` enums.
- `async fn`, methods that consume `self`, and operations that do not map to
  exactly one Rust function.
- Binding tools other than wit-bindgen 0.63. The bindings and forwarding files
  follow its options and naming; a mismatch is a compile error.
- A representation map for generic items, and for containers other than
  `Option` and `Vec`. A field there takes a rule.
- Crossing unchanged for generic types and for enums whose cases carry data.
  They are converted.
- Comparing a type placed in an external interface with that interface's WIT.
- More than one contract per generator program when it is declared by marks,
  and types of external interfaces declared by marks. Both work with a schema
  program.

## License

Licensed under the [MIT license](LICENSE-MIT).
