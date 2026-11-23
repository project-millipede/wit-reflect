# Developer guide

What you write, what wit-reflect generates from it, and what it leaves to you.
Every example is quoted from the `fixtures/session` crates, so it compiles and
the output shown is the committed output.

## The idea

You keep writing ordinary Rust. You mark what crosses the boundary, each item
states its own part of the contract, and you run a generator.

| You write | wit-reflect generates | Other tools generate |
| --- | --- | --- |
| Rust types and functions, as today | The WIT file | Rust bindings, by wit-bindgen from that WIT |
| `#[derive(WitType)]`, `#[export]` and rules on them | The bindings file: the contract's part of the `wit_bindgen::generate!` call | TypeScript bindings, by jco from the component |
| The package and world, in `Cargo.toml` | The forwarding file: the `impl Guest` blocks for those bindings | |
| | The conversion of every result, inside your derive | |

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
#[wit(display = "message", cases = "kind")]
pub enum InspectionFailure {
    Selection(Failure),
    Analysis(AnalysisError),
}
```

Rules can also be stated together above the item, which leaves its fields as
plain Rust:

```rust
#[derive(WitType)]
#[wit(display = "message", skip(cause))]
pub struct Failure {
    pub kind: FailureKind,
    pub cause: Option<Box<dyn Error + Send + Sync>>,
}
```

`display = "message"` adds a field holding the value's `Display` text: a
method cannot cross the boundary, so the text travels as data. `skip(cause)`
keeps a field out of the contract.

### 2. Mark the operations

```rust
/// A client that remembers the selected resource between calls.
#[derive(WitResource)]
#[wit(interface = "native")]
pub struct Session { /* fields are not exported */ }

#[exports(select, selection, inspect, progress)]
impl Session {
    /// Inspect `resource`. It must be the current selection.
    pub fn inspect(&mut self, resource: &str) -> InspectionOutcome { /* … */ }

    fn analyze(&self, resource: &str) -> Result<Report, InspectionFailure> { /* not exported */ }
}

/// Create a session without a selection.
#[export(interface = "native")]
pub fn create_session() -> Session { /* … */ }
```

`#[exports(…)]` names the functions of the block that are exported. It does
not change them: it reflects their signatures and registers them with their
resource. A name that is not a function of the block is a compile error.

What is exported is always stated. It does not follow from `pub` or from how
functions are grouped into blocks: a block may hold public functions that are
not in the contract, and the contract does not shape the Rust code.

A function can carry the mark itself instead, `#[export]`, in a block marked
with a plain `#[exports]`. A free function always does, and so does a function
without `self` that is a `static` function or a `constructor`.

### 3. State what the items cannot

Three things are left, and each is stated where it belongs.

**Which interface owns a type.** Only a type an interface has to own says so.
Everything it refers to is found through it.

```rust
#[derive(WitType)]
#[wit(interface = "binary")]
pub struct Report { /* … */ }
```

**What an instantiation of a generic type is called.** WIT has no generic
types: `Outcome<A>` and `Outcome<B>` are two definitions and need two names. A
type alias is where Rust names one, and the mark makes that name visible:

```rust
#[named]
pub type InspectionStepOutcome = StepOutcome<StepKind, Report, InspectionFailure>;
#[named]
pub type InspectionOutcome = Outcome<InspectionStepOutcome>;
```

**The package and the world.** No Rust item is a package. They are settings of
the component crate, in its `Cargo.toml`:

```toml
[package.metadata.wit-reflect]
package = "example:session@0.1.0"
world = "session"
exports = [{ interface = "native", name = "native" }, "binary"]
```

Five of the 17 definitions in the generated WIT carry such a mark: two
interface owners, two aliases and the resource. `Image`, `FileRange`, `Failure`
and nine more carry only the derive. They are found because the marked types
and operations refer to them.

### 4. Generate

The generator program declares nothing. It links the crate whose items are
marked and names the component crate's manifest:

```rust
use session_owner as _;

fn main() -> ExitCode {
    wit_reflect::tasks::run(COMPONENT_MANIFEST)
}
```

`use session_owner as _;` is required. Rust links a dependency only if the
program names it, and an unlinked crate registers nothing. The generator then
fails and says so.

The binary has two modes:

```bash
cargo run -p session-owner --bin generate
```

```bash
cargo run -p session-owner --bin generate -- --check
```

The first rewrites the three files. The second changes nothing and fails when
a committed file is stale.

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

The bindings file, `fixtures/session/component/src/bindings.rs`:

```rust
macro_rules! generate_bindings {
    ($($options:tt)*) => {
        ::wit_bindgen::generate!({
            world: "example:session/session@0.1.0",
            additional_derives: [::wit_reflect::FromParts],
            with: {
                "example:session/binary@0.1.0/macho-value": session_owner::MachoValue,
                "example:session/native@0.1.0/step-kind": session_owner::StepKind,
                "example:session/native@0.1.0/progress": session_owner::Progress,
            },
            $($options)*
        });
    };
}
```

Each `with` entry is a type that crosses unchanged; see
[Types that cross unchanged](#types-that-cross-unchanged).

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
include!("bindings.rs");
generate_bindings! { path: "wit" }

#[rustfmt::skip]
mod forwarding;
use forwarding::Component;

export!(Component);
```

`generate_bindings!` already knows the world, the types that cross unchanged
and the derive the forwarding needs. You pass what describes your crate: the
WIT directory, and any other `wit_bindgen::generate!` option except `world`.

## The marks and settings

| You write | Where | Effect |
| --- | --- | --- |
| `#[wit(interface = "…")]` | a derived type or resource | The interface defines it. |
| `#[export(interface = "…")]` | a free function | The interface exports it. |
| `#[exports(a, b)]` | an `impl` block | The named functions are exported from the resource. A plain `#[exports]` exports the functions marked `#[export]`. |
| `#[named]` | a type alias | The instantiation is named after the alias, in kebab case. |
| `package`, `world` | `[package.metadata.wit-reflect]` | The package and the world's name. |
| `imports`, `exports` | the same table | The world's items. `{ interface = "…", name = "…" }` gives an export its own name. |
| `wit`, `bindings`, `forwarding` | the same table | Output paths. They default to `wit/<world>.wit`, `src/bindings.rs` and `src/forwarding.rs`. |
| `[[package.metadata.wit-reflect.function]]` | the same file | A function the component crate implements itself: `interface`, `name`, `path`, and optionally `returns` and `docs`. |

A component crate compiles only after its files are generated, so it cannot
register anything. That is the one case for a `function` entry.

Generated code calls a registered operation at the root of its crate:
`session_owner::create_session`, `session_owner::Session::inspect`. If it is
exported elsewhere, say where: `#[export(interface = "…", path = owner::api::f)]`
or `#[exports(path = owner::api::Session)]`.

Registrations are applied in source order: by package name, then file and
line. Interfaces and definitions appear in that order in the WIT.

## Declaring with a schema program instead

The marks are a front end to `Schema`. A program can also declare a contract
call by call, for example to generate two contracts from the same crates.

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
  two definitions with the same default name. Name them with `#[named]` on an
  alias, or with `define_as`. No name is composed from the type arguments.
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
| `#[wit(widen)]` | `u64` for `usize`, `s64` for `isize` | only those two types; optional, they are widened without it |
| `#[wit(into = W)]` | `W`, from `W::from(field)` | `W: From<FieldType>` |
| `#[wit(into = W, with = f)]` | `W`, from `f(&self)` | `f: fn(&Self) -> W` |
| `#[wit(skip)]` | absent | — |
| `#[wit(skip(a, b))]` on the type | the named fields are absent | each name is a field without a rule of its own |
| `#[wit(display = "…")]` on the type | an extra leading `string` field with the value's `Display` text | the type is `Display` |
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
pub struct MachoValue { pub value: u32, pub name: Option<String> }

impl From<foreign::FileType> for MachoValue { /* into_inner(), name() */ }
```

These are the only wire-shaped types a person writes. If the foreign enum gains
a case, the `remote` declaration stops compiling.

## What is generated, and what is not

| Generated | By | When |
| --- | --- | --- |
| The WIT file | `ResolvedPackage: Display` | when you run the generator |
| The bindings file | `ResolvedPackage::bindings` | when you run the generator |
| The forwarding file | `ResolvedPackage::forwarding` | when you run the generator |
| The decision which types cross unchanged | `#[derive(WitType)]` and the compiler | when the owner crate compiles |
| The conversion of each result | `#[derive(WitType)]` | when the owner crate compiles |
| The assembly of each binding value | `#[derive(FromParts)]`, attached by wit-bindgen | when the component crate compiles |
| `From<Foreign>` for a `remote` declaration | `#[derive(WitType)]` | when the owner crate compiles |
| Rust bindings and the ABI | wit-bindgen | when the component crate compiles |
| TypeScript bindings | jco | in your build |

| Not generated: you write it | Size in the fixture |
| --- | --- |
| The marks that place and name: `interface`, `#[exports(…)]`, `#[named]` | 7 marks |
| The contract settings in `Cargo.toml` | 4 lines |
| The generator program | one `use` and one call to `wit_reflect::tasks::run` |
| The component crate's `include!`, bindings call, `mod` and `export!` | 6 lines |
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
| Binding tools other than wit-bindgen 0.63 | the bindings and forwarding files follow its options and naming; a mismatch is a compile error |
| Crossing unchanged for a generic type or an enum whose cases carry data | the type is converted |

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

### Types that cross unchanged

Wire parts are for types whose binding differs from the owner type. When
nothing differs, no conversion is needed: wit-bindgen's `with` option makes
the bindings use the owner type itself. wit-reflect generates those `with`
entries in the bindings file, and such a value is moved across as it is.

A struct or enum crosses unchanged when all of this holds:

| Condition | Why |
| --- | --- |
| It is `pub` and has no type parameters | The component crate must name it, and `with` accepts no generic arguments |
| It is `Clone` and `Debug` | The bindings clone and print the values they hold |
| No rule, computed field, `transparent`, `remote` or `name` on a member | The binding would differ in shape, value or name |
| A struct has only `pub` fields | The bindings read the fields directly |
| An enum has only unit cases without explicit discriminants | The bindings lower an enum by its discriminant |
| Every type inside it crosses unchanged | A binding field has exactly one type |

You declare nothing. The derive checks what is visible on the item, and the
compiler decides the rest from the field types. A type that does not qualify
is converted through wire parts. Both kinds mix freely:

| In the fixture | Crosses | Because |
| --- | --- | --- |
| `StepKind`, `Progress`, `MachoValue` | unchanged | every condition holds |
| `FailureKind` | converted | it is not `Clone` |
| `FileRange` | converted | its fields are private |
| `Report` | converted | it holds `Image`, which has rules |
| `StepOutcome<StepKind, …>` | converted | it is generic; the `StepKind` inside still crosses unchanged |

Two attributes on the type adjust the decision:

| Attribute | Use it when |
| --- | --- |
| `#[wit(convert)]` | the type must be converted although it qualifies |
| `#[wit(path = owner::module::Type)]` | the type is not exported at its crate's root, where the `with` entry names it by default |

A `with` path that does not resolve is a compile error in the component
crate. Inside a converted parent, a list or option of unchanged values is
still rebuilt element by element; the standard library usually reuses the
allocation, but that is not guaranteed.

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
error[E0277]: `std::time::Duration` has no WIT representation
   = note: derive `wit_reflect::WitType` on the struct or enum that owns this data

error: this field contains the type it belongs to; a WIT type cannot be recursive

error: a `&mut` parameter cannot be exported: changes to it would be lost
```

When the generator runs, all at once:

```text
no declaration is registered: the program links no crate with marked items;
name each such crate once, for example with `use owner_crate as _;`

the contract settings in fixtures/session/component/Cargo.toml are invalid:
- `world` is missing in the settings
- `exprots` in the settings is not a known key

the WIT schema is invalid:
- `…Session` exports functions but no interface of this package declares it
- interface `native` would define `outcome` more than once, for `…Outcome<…Report…>` and `…Outcome<…SelectionState…>`
- `…FileRange` is used by interfaces `headers`, `segments` but declared in none of them

the component's Rust files cannot be generated:
- function `configure` of resource `session` takes `options`, whose type cannot be passed to Rust yet
```

When a committed contract no longer matches the Rust types:

```text
error[E0277]: `u32` cannot be assembled from the wire parts `std::string::String`
   = note: they differ in field order, count or type, or the bindings define a type of their
           own where the owner type crosses unchanged; regenerate the WIT, bindings and
           forwarding files from the schema, then rebuild
```

## Checklist for a new contract

1. Derive `WitType` on each type an operation returns or takes; the compiler
   then points at every type it reaches that still needs it.
2. Add a rule where the compiler reports a field without a WIT representation.
3. Derive `WitResource` on each resource and name its exported functions in
   `#[exports(…)]` on their `impl` blocks.
4. State the interface on each type an interface has to own, on each resource
   and on each exported free function. Mark an alias of each generic
   instantiation with `#[named]`.
5. Add the settings to the component crate's `Cargo.toml` and a generator
   program that links the owner crates. Run it and fix what it reports.
6. Commit the WIT, the bindings file and the forwarding file. Add the check to
   your test run.
7. In the component crate, include the bindings file, call
   `generate_bindings!` with your WIT path and include the forwarding module.
