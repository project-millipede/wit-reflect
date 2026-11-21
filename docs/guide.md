# Developer guide

What you write, what wit-reflect generates from it, and what it leaves to you.
Every example is quoted from the `fixtures/session` crates, so it compiles and
the output shown is the committed output.

## The idea

You keep writing ordinary Rust. You mark what crosses the boundary, declare the
contract once, and run a generator.

| You write | wit-reflect generates | Other tools generate |
| --- | --- | --- |
| Rust types and functions, as today | The WIT file | Rust bindings, by wit-bindgen from that WIT |
| `#[derive(WitType)]`, `#[export]` and rules on them | The forwarding file: the `impl Guest` blocks for those bindings | TypeScript bindings, by jco from the component |
| One schema function | The conversion of every result, inside your derive | |

There is no second data model. A field exists in one place: the Rust type.

## A complete example

### 1. Mark the types

A type that crosses the boundary derives `WitType`. None of its fields is
repeated anywhere.

```rust
#[derive(WitType)]
pub struct Report {
    pub byte_length: u64,
    pub images: Vec<Image>,
}
```

Where a field cannot cross as stored, a rule says how it is represented:

```rust
#[derive(WitType)]
pub struct Image {
    #[wit(into = Endianness)]                              // a dependency's enum
    pub endianness: foreign::Endianness,
    #[wit(into = MachoValue)]                              // wrapped number as { value, name }
    pub file_type: foreign::FileType,
    #[wit(into = MachoValue, with = Image::subtype_value)] // needs a sibling field
    pub cpu_subtype: foreign::CpuSubtype,
    #[wit(display)]                                        // formatted text
    pub minimum_os: foreign::Version,
    pub file_range: FileRange,                             // private fields are fine
    #[wit(widen)]                                          // usize as u64
    pub read_count: usize,
}
```

An error can carry its `Display` message next to its typed cases:

```rust
#[derive(WitType)]
#[wit(
    field(name = "message", into = String, with = ToString::to_string),
    cases = "kind"
)]
pub enum InspectionFailure {
    Selection(Failure),
    Analysis(AnalysisError),
}
```

### 2. Mark the operations

```rust
/// A client that remembers the selected resource between calls.
#[derive(WitResource)]
pub struct Session { /* fields are not exported */ }

impl Session {
    /// Inspect `resource`. It must be the current selection.
    #[export]
    pub fn inspect(&mut self, resource: &str) -> InspectionOutcome { /* … */ }
}

/// Create a session without a selection.
#[export]
pub fn create_session() -> Session { /* … */ }
```

`#[export]` does not change the function. It makes its signature available to
a schema. A marked function is not exported until a schema lists it.

### 3. Declare the contract

This is the only declaration. It says which types and operations each
interface exports, and nothing about fields or signatures.

```rust
pub fn schema() -> Result<Schema, Box<dyn Error>> {
    let package_name = "example:session@0.1.0".parse()?;
    let mut schema = Schema::new(package_name);
    schema
        .interface("binary")
        .define::<Report>()
        .define::<AnalysisError>();

    let mut native = schema.interface("native");
    native
        .define_as::<InspectionStepOutcome>("inspection-step-outcome")
        .define_as::<InspectionOutcome>("inspection-outcome");
    native
        .resource::<Session>()
        .method(operation!(session_owner::Session::select))
        .method(operation!(session_owner::Session::selection))
        .method(operation!(session_owner::Session::inspect));
    native.function(operation!(session_owner::create_session));

    schema
        .world("session")
        .export_as("native", "native")
        .export("binary");
    Ok(schema)
}
```

The schema lists 5 of the 16 definitions in the generated WIT. `Image`,
`FileRange`, `Failure` and eight more are not listed. They are found because
the listed types and operations refer to them.

### 4. Generate

```rust
let package = schema()?.resolve()?;               // validates; reports every problem at once
let wit = package.to_string();                    // the WIT file
let forwarding = package.forwarding("session")?;  // the Rust file for the component
```

The fixture wraps these three lines in a binary with two modes:

```bash
cargo run -p session-owner --bin generate
```

```bash
cargo run -p session-owner --bin generate -- --check
```

The first rewrites the two files. The second changes nothing and fails when a
committed file is stale.

### 5. What comes out

The WIT, `fixtures/session/component/wit/session.wit` (excerpt):

```wit
package example:session@0.1.0;

interface binary {
  record image {
    endianness: endianness,
    file-type: macho-value,
    cpu-subtype: macho-value,
    minimum-os: string,
    file-range: file-range,
    read-count: u64,
  }
  // …
}

interface native {
  use binary.{analysis-error, report};

  variant inspection-failure-kind {
    selection(failure),
    analysis(analysis-error),
  }

  record inspection-failure {
    message: string,
    kind: inspection-failure-kind,
  }

  /// A client that remembers the selected resource between calls.
  resource session {
    /// Inspect `resource`. It must be the current selection.
    inspect: func(%resource: string) -> inspection-outcome;
    // …
  }

  /// Create a session without a selection.
  create-session: func() -> own<session>;
}

world session {
  export native: native;
  export binary;
}
```

The forwarding file, `fixtures/session/component/src/forwarding.rs` (excerpt):

```rust
pub struct Component;

impl crate::exports::native::Guest for Component {
    type Session = ::wit_reflect::component::ResourceCell<session_owner::Session>;

    fn create_session() -> crate::exports::native::Session {
        let created = session_owner::create_session();
        crate::exports::native::Session::new(::wit_reflect::component::ResourceCell::new(created))
    }
}

impl crate::exports::native::GuestSession for ::wit_reflect::component::ResourceCell<session_owner::Session> {
    fn inspect(&self, resource: ::std::string::String) -> crate::exports::native::InspectionOutcome {
        let returned = session_owner::Session::inspect(&mut self.borrow_mut(), &resource);
        ::wit_reflect::lower::assemble(returned)
    }
    // …
}
```

### 6. The component crate

This is all of its handwritten code:

```rust
wit_bindgen::generate!({
    path: "wit",
    world: "example:session/session@0.1.0",
    additional_derives: [wit_reflect::FromParts],
});

#[rustfmt::skip]
mod forwarding;
use forwarding::Component;

export!(Component);
```

## The schema, call by call

| Call | Meaning | Effect in the WIT |
| --- | --- | --- |
| `Schema::new(name)` | The package | `package example:session@0.1.0;` |
| `schema.interface("binary")` | Declare an interface, or continue it | `interface binary { … }` |
| `.define::<T>()` | `T` belongs to this interface | `T` and everything it refers to is defined |
| `.define_as::<T>("name")` | The same, under a chosen name | Needed to tell generic instantiations apart |
| `.resource::<R>()` | `R` is a resource of this interface | `resource r { … }` |
| `.method(operation!(R::f))` | Export this marked function of `R` | One line inside the resource |
| `.function(operation!(path::f))` | Export a marked free function | `f: func(…) -> …;` |
| `schema.external_interface(path).define::<T>()` | A dependency already defines `T` | `use path.{t};` where it is referenced; never defined here |
| `schema.world("w").import(t).export(t)` | A world's imports and exports | `world w { … }` |
| `.export_as("label", t)` | A named export | `export label: t;` |

Three rules decide what the declaration cannot leave open:

- **Each type has one owning interface.** A declared type belongs where it is
  declared. An undeclared type belongs to the only interface that reaches it.
  A type reached from two interfaces must be declared in one; the other gets a
  `use`. Above, `report` is declared in `binary`, so `native` imports it.
- **Each instantiation needs its own name.** `Outcome<A>` and `Outcome<B>` are
  two definitions with the same default name. Name them with `define_as`. A
  Rust type alias names the same instantiation as its expansion.
- **Operation paths are absolute.** `operation!` records the path as written,
  and the forwarding file calls the function by it from another crate. Start
  it with a crate name or `crate`.

## Default names

| Rust | WIT |
| --- | --- |
| `struct FileRange` | `record file-range` |
| field `byte_length` | `byte-length` |
| field or parameter `resource`, `r#type` | `%resource`, `%type` |
| `enum StepKind { QuerySelection, Inspect }` | `enum step-kind { query-selection, inspect }` |
| an enum with data | `variant` |
| a struct-like case `Truncated { .. }` of `ErrorKind` | the record `error-kind-truncated`, or the name in `#[wit(payload = "…")]` |
| `fn create_session` | `create-session` |
| `///` comments | `///` comments |

`#[wit(name = "…")]` on a type, field or case and `#[export(name = "…")]` on a
function replace the default.

## Rules

| Rule | The field becomes | Checked by the compiler |
| --- | --- | --- |
| `#[wit(display)]` | `string`, from `to_string()` | the type is `Display` |
| `#[wit(widen)]` | `u64` for `usize`, `s64` for `isize` | only those two types |
| `#[wit(into = W)]` | `W`, from `W::from(field)` | `W: From<FieldType>` |
| `#[wit(into = W, with = f)]` | `W`, from `f(&self)` | `f: fn(&Self) -> W` |
| `#[wit(skip)]` | absent | — |
| `#[wit(field(name = "…", into = W, with = f))]` on the type | an extra leading field | as above |
| `#[wit(cases = "…")]` on an enum with such a field | a record holding the cases | — |
| `#[wit(transparent)]` on a single-field struct | the field's own representation | — |

### Types of other crates

No derive can be placed on a dependency's type, so its shape is stated once.

```rust
// An enum: a local declaration that the compiler checks against the real one.
#[derive(WitType)]
#[wit(remote = foreign::Endianness)]
pub enum Endianness { Little, Big }

// A wrapped number: one wire record and a conversion per foreign type.
#[derive(WitType)]
#[wit(name = "macho-value")]
pub struct MachoValue { pub value: u32, pub name: Option<String> }

impl From<foreign::FileType> for MachoValue { /* into_inner(), name() */ }
```

These are the only wire-shaped types a person writes. If the foreign enum gains
a case, the `remote` declaration stops compiling.

## What is generated, and what is not

| Generated | By | When |
| --- | --- | --- |
| The WIT file | `ResolvedPackage: Display` | when you run the generator |
| The forwarding file | `ResolvedPackage::forwarding` | when you run the generator |
| The conversion of each result | `#[derive(WitType)]` | when the owner crate compiles |
| The assembly of each binding value | `#[derive(FromParts)]`, attached by wit-bindgen | when the component crate compiles |
| `From<Foreign>` for a `remote` declaration | `#[derive(WitType)]` | when the owner crate compiles |
| Rust bindings and the ABI | wit-bindgen | when the component crate compiles |
| TypeScript bindings | jco | in your build |

| Not generated: you write it | Size in the fixture |
| --- | --- |
| The schema function | 25 lines |
| The program that writes and checks the two files | about 90 lines; the library has no ready-made command yet |
| The component crate's bindings call, `mod` and `export!` | 9 lines |
| Conversions and wire records for types of other crates | a few lines per foreign type |
| Cargo wiring: dependencies and, if wanted, a default-off feature for the macros | a few lines |
| The implementation of a world's imports | — |
| The jco configuration, including which exports are asynchronous | — |
| Everything on the TypeScript side | — |

| Not supported | What happens |
| --- | --- |
| A record, variant or enum as a parameter | error when the forwarding file is generated |
| A resource handle nested in a result | error when the forwarding file is generated |
| `async fn`, `unsafe fn`, generic functions, `impl Trait`, `&mut` parameters, consuming `self` | compile error at the function |
| A type that contains itself | compile error at the field |
| One WIT function backed by anything but exactly one Rust function | write an ordinary Rust function for it first |
| Checking a type placed in an external interface against that interface's WIT | not checked; a mismatch appears when the bindings compile |
| Binding tools other than wit-bindgen 0.60 | the forwarding file follows its naming; a mismatch is a compile error |

## How a value crosses the boundary: wire parts

**Wire parts are the neutral in-between form of a Rust value on its way to the
bindings: the value taken apart field by field, in declaration order, with
every rule already applied, using only plain Rust types.**

They exist because two crates each know half of the picture. The owner crate
knows the real type, its private fields and its rules, but cannot name
wit-bindgen's types, which are generated later in the component crate. The
component crate has those types but cannot read private fields or know the
rules. Wire parts are what is handed across.

### A record

```rust
#[derive(WitType)]
pub struct Image {
    #[wit(display)]
    pub minimum_os: Version,    // exported as text
    pub file_range: FileRange,  // private fields `offset`, `size`
    #[wit(widen)]
    pub read_count: usize,      // exported as u64
}
```

For `minimum_os` 14.4, a range of offset 0 and size 4096, and a count of 1,
the wire parts are:

```rust
Fields((
    "14.4".to_string(),               // display applied
    (Fields((0_u64, (4096_u64, ()))), // the nested record, taken apart too
    (1_u64,                           // widen applied
    ()))
))
```

- The rules are already applied: no `Version` and no `usize` is left.
- Field names are gone. Only the order remains.
- The shape is a nested pair list, `(a, (b, (c, ())))`, so a record may have
  any number of fields.

The binding side takes the first part for its first field, the second for its
second, and so on:

| Position | Wire part | Binding field |
| --- | --- | --- |
| 1 | `"14.4"` | `minimum_os: String` |
| 2 | `Fields((0, (4096, ())))` | `file_range: FileRange`, assembled the same way |
| 3 | `1` | `read_count: u64` |

The positions agree because both sides come from one source: the derive wrote
the WIT fields in declaration order, and wit-bindgen generated its struct from
that WIT.

### An enum

The parts say which case was chosen, by position, with that case's payload.

```rust
#[derive(WitType)]
pub enum ErrorKind {
    Truncated { requested_size: u64, available_size: u64 },  // case 1
    UnsupportedImage,                                        // case 2
    Decode(#[wit(display)] DecodeError),                     // case 3
}
```

| Value | Wire parts |
| --- | --- |
| `Truncated { requested_size: 32, available_size: 8 }` | `Cases(Either::This(Fields((32, (8, ())))))` |
| `UnsupportedImage` | `Cases(Either::Next(Either::This(())))` |
| `Decode(error)` | `Cases(Either::Next(Either::Next(Either::This("invalid magic".to_string()))))` |

Each `Next` means "a later case". `This` holds the payload; a unit case
carries `()`.

### If the fields are public

Nothing changes. Private fields are only the case where no other approach
works. Public fields would also allow copying field by field in the component
crate, or, when no field needs a rule, wit-bindgen's `with` option, which uses
the owner type as the binding type and converts nothing.

wit-reflect does not use `with`. Every result goes through wire parts, even
when the owner type and the binding type are identical. That costs a move of
each field and a rebuild of each list, and has not been measured.

### Are wire parts DTOs?

| At the boundary | Written by | A named type? | Maintained by a person? |
| --- | --- | --- | --- |
| Wire parts | the derive | no, anonymous nested tuples | no |
| wit-bindgen's binding types | wit-bindgen, from the generated WIT | yes | no, regenerated |
| A wire record such as `MachoValue` | a person | yes | yes |
| A `remote` declaration | a person | yes | yes, and compiler-checked |

Nobody declares or names wire parts, and no `ImageParts` type exists. During
one call a value is moved into a tuple and then into the binding value: an
intermediate value, not an intermediate model. The two handwritten rows are
limited to types of other crates.

## Errors you will meet

At compile time, at the declaration:

```text
error[E0277]: `usize` has no WIT representation
   = note: `usize` and `isize` depend on the target; use a fixed-width integer for wire data

error: this field contains the type it belongs to; a WIT type cannot be recursive

error: a `&mut` parameter cannot be exported: changes to it would be lost
```

When the generator runs, all at once:

```text
the WIT schema is invalid:
- interface `native` would define `outcome` more than once, for `…Outcome<…Report…>` and `…Outcome<…SelectionState…>`
- `…FileRange` is used by interfaces `headers`, `segments` but declared in none of them

the forwarding file cannot be generated:
- function `configure` of resource `session` takes `options`, whose type cannot be passed to Rust yet
```

When a committed contract no longer matches the Rust types:

```text
error[E0277]: `u32` cannot be assembled from the wire parts `std::string::String`
   = note: they differ in field order, count or type; regenerate the WIT and forwarding file
           from the schema, then rebuild
```

## Checklist for a new contract

1. Derive `WitType` on each type you name in the schema; the compiler then
   points at every type it reaches that still needs it.
2. Add a rule where the compiler reports a field without a WIT representation.
3. Derive `WitResource` on each resource and mark its functions with `#[export]`.
4. Write the schema function. Run the generator and fix what it reports.
5. Commit the WIT and the forwarding file. Add the check to your test run.
6. In the component crate, call `wit_bindgen::generate!` with
   `additional_derives: [wit_reflect::FromParts]` and include the forwarding
   module.
