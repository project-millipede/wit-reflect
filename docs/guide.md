# Developer guide

What you write, what wit-reflect generates from it, and what it leaves to you.
The worked example is taken from the `fixtures/session` crates, so it compiles
and the output shown is the committed output. Code is shortened where a comment
says so.

## The idea

You keep writing ordinary Rust. You mark what crosses the boundary, each item
states its own part of the contract, and you run a generator.

| You write | wit-reflect generates | Other tools generate |
| --- | --- | --- |
| Rust types and functions, as today | The WIT file | Rust bindings, by wit-bindgen from that WIT |
| A derive on each type, and a mark where an item has to state its part | The bindings file: the contract's part of the `wit_bindgen::generate!` call | TypeScript bindings, by jco from the component |
| One map for the types of other crates | The forwarding file: the `impl Guest` blocks for those bindings | |
| The package and world, in `Cargo.toml` | The conversion of every result, inside your derive | |

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

A field whose type belongs to another crate cannot cross as stored: no derive
can be placed on that type. How it is shown is a fact about the type, so the
crate states it once, in a map:

```rust
representations! {
    /// How the contract shows each type of the `foreign` module.
    pub Representations {
        foreign {
            foreign::Endianness => Endianness,     // a dependency's enum
            foreign::FileType => MachoValue with |file_type| MachoValue {
                value: file_type.into_inner(),     // wrapped number as { value, name }
                name: file_type.name().map(str::to_owned),
            },
            foreign::Version => DisplayText,       // its formatted text
            foreign::DecodeError => DisplayText,
        }
    }
}
```

One line in the crate's `Cargo.toml` names the map for every derive of the
crate:

```toml
[package.metadata.wit-reflect]
representations = "crate::Representations"
```

The fields then carry no rule:

```rust
#[derive(WitType)]
pub struct Image {
    pub endianness: foreign::Endianness,
    pub file_type: foreign::FileType,
    #[wit(into = MachoValue, with = Image::subtype_value)] // needs a sibling field
    pub cpu_subtype: foreign::CpuSubtype,
    pub minimum_os: foreign::Version,
    pub file_range: FileRange,                             // private fields are fine
    pub read_count: usize,                                 // shown as u64
}
```

- **An entry is `Foreign => Shown`:** the contract shows `Foreign` as the type
  `Shown`, converted with `From`. `DisplayText` is the library's type for a
  value's `Display` text; it appears as `string`.
- **An entry can state its conversion** after `with`, as `|value| expression`
  or as a function's path. No `From` is written then. Several foreign types
  separated by `|` share one entry and its conversion:
  `A | B => Shown with |value| …`. The conversion is expanded and checked for
  each type, so the types need no trait in common.
- **Entries stand in groups** named after where their types come from. An
  entry whose path does not begin with its group's name is a compile error.
- **A rule on a field still works** and wins over the map. `cpu_subtype` keeps
  one because its value depends on the field next to it, which is a fact about
  this field and not about its type.
- **`usize` needs nothing.** WIT has no platform-sized integer, so it is shown
  as `u64`, which holds every value.
- **A type the map does not name is not covered.** A foreign field without an
  entry and without a rule is a compile error at the field.

The compiler makes the choice for each field: the map's entry for its type, or
the type itself. Nothing is looked up when a value crosses.

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

/// What the component's environment supplies.
#[exports(interface = "native", open_session)]
pub trait SessionSource {
    /// Create a session with `resource` selected.
    fn open_session(resource: &str) -> Session;
}
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

`create_session` states its interface because it is an entry point. `Session`
states none: it belongs to `native` because that function returns it.

`SessionSource` declares a function the owner crate cannot implement. The
component crate implements the trait; see
[Where a function is exported from](#where-a-function-is-exported-from).

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

  /// Create a session with `resource` selected.
  open-session: func(%resource: string) -> own<session>;
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

    fn open_session(resource: ::std::string::String) -> crate::exports::native::Session {
        let created = <Component as session_owner::SessionSource>::open_session(&resource);
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

// The one function the owner crate declares and this crate implements.
impl session_owner::SessionSource for Component {
    fn open_session(resource: &str) -> session_owner::Session {
        let mut session = session_owner::create_session();
        session.select(resource);
        session
    }
}
```

The `impl` block carries no mark. The trait in the owner crate declares the
function; this crate supplies its body.

`generate_bindings!` already knows the world, the types that cross unchanged
and the derive the forwarding needs. You pass what describes your crate: the
WIT directory, and any other `wit_bindgen::generate!` option except `world`.

## The marks and settings

| You write | Where | Effect |
| --- | --- | --- |
| `#[wit(interface = "…")]` | a derived type or resource | The interface defines it. Without the mark, the item belongs to the only interface that reaches it. |
| `#[export(interface = "…")]` | a free function | The interface exports it. |
| `#[exports(a, b)]` | an `impl` block | The named functions are exported from the resource. A plain `#[exports]` exports the functions marked `#[export]`. |
| `#[exports(interface = "…", a)]` | a trait | The named functions are exported from the interface. The component crate implements the trait. |
| `#[named]` | a type alias | The instantiation is named after the alias, in kebab case. |
| `representations! { … }` | once in a crate | The map: how each type of another crate is shown. |
| `representations = "crate::…"` | `[package.metadata.wit-reflect]` of the crate that has the map | Every derive of that crate's library uses the map. Needs the `representations` feature of wit-reflect. |
| `#[wit(map = path)]` | a derived type | That item uses the named map. For a crate without the setting, or for another map. |
| `package`, `world` | `[package.metadata.wit-reflect]` | The package and the world's name. |
| `imports`, `exports` | the same table | The world's items. `{ interface = "…", name = "…" }` gives an export its own name. |
| `wit`, `bindings`, `forwarding` | the same table | Output paths. They default to `wit/<world>.wit`, `src/bindings.rs` and `src/forwarding.rs`. |
| `[[package.metadata.wit-reflect.function]]` | the same file | A function the component crate implements itself: `interface`, `name`, `path`, and optionally `returns` and `docs`. |

A component crate compiles only after its files are generated, so it cannot
register anything. Declare a function it implements on a trait in the crate
that owns the function's types. A `function` entry states the same in the
manifest, for a function that has no such crate.

Generated code calls a registered operation at the root of its crate:
`session_owner::create_session`, `session_owner::Session::inspect`. If it is
exported elsewhere, say where: `#[export(interface = "…", path = owner::api::f)]`
or `#[exports(path = owner::api::Session)]`.

Registrations are applied in source order: by package name, then file and
line. Interfaces and definitions appear in that order in the WIT.

## Where a function is exported from

A function joins the contract in exactly one way: a mark names it. The mark
stands on the block that holds the function, and there are three kinds of
block. Which one you use follows from two questions:

1. Does the function work on a resource, or does it stand alone?
2. Who can write its body: the crate that owns the types, or only the
   component?

| The function | Its body is written in | Form |
| --- | --- | --- |
| works on a resource | the owner crate | 1. An `impl` block |
| stands alone | the owner crate | 2. A free function |
| stands alone | the component crate | 3. A trait |

The examples below build one contract. A program stores named counters; a
caller creates one, or opens one that the component's environment has stored,
and then adds to it.

### Form 1: an `impl` block, for the functions of a resource

```rust
#[derive(WitResource)]
pub struct Counter {
    value: u32,
}

#[exports(add, value)]
impl Counter {
    /// Add `amount` and return the new value.
    pub fn add(&mut self, amount: u32) -> u32 {
        self.value += amount;
        self.value
    }

    /// The current value.
    pub fn value(&self) -> u32 {
        self.value
    }

    // Public, but not in the contract: it is not listed.
    pub fn reset(&mut self) {
        self.value = 0;
    }
}
```

What it implies:

- **The functions belong to the resource.** They appear inside
  `resource counter { … }` and take the resource as their receiver.
- **No interface is stated.** A resource's functions are exported from the
  interface the resource is in.
- **`&self` and `&mut self` are both methods.** The generated code borrows the
  resource shared or exclusively, as the signature says. A function that
  consumes `self` cannot be exported.
- **A function without `self` must say what it is:** `#[export(static)]` or
  `#[export(constructor)]` on that function. Without it the block reports an
  error.
- **Only listed functions are exported.** `reset` stays an ordinary Rust
  function. Visibility decides nothing.
- **Several blocks combine.** Each `impl Counter` block lists its own exported
  functions, in whichever file it stands.

### Form 2: a free function, implemented where the types are

```rust
/// Create a counter that starts at zero.
#[export(interface = "counters")]
pub fn new_counter() -> Counter {
    Counter { value: 0 }
}
```

What it implies:

- **The function states its interface.** It is an entry point, and an
  interface is a named group of entry points.
- **It places what it mentions.** `Counter` is returned by a function of
  `counters` and by nothing else, so `Counter` belongs to `counters` without a
  mark of its own. The same holds for every type in the signature.
- **The body is ordinary Rust in the owner crate.** A native program calls
  `new_counter()` directly; no component, no WIT and no Wasm runtime is
  involved.
- **The generated code calls it by path.** The function must be reachable at
  its crate's root, or state `path = …`.

### Form 3: a trait, for a function only the component can implement

Some functions need something the owner crate does not have: a connection, a
store, a clock that the component's environment supplies. The owner crate then
declares the function and the component implements it.

```rust
// Owner crate: the declaration.
#[exports(interface = "counters", open_counter)]
pub trait CounterStore {
    /// Open the counter stored under `name`.
    fn open_counter(name: &str) -> Counter;
}
```

```rust
// Component crate: the implementation. No attribute.
impl CounterStore for Component {
    fn open_counter(name: &str) -> Counter {
        storage::load(name)
    }
}
```

What it implies:

- **The contract is still declared in Rust, in the owner crate.** The name,
  the parameters, the result and the documentation come from the trait, as
  they do for any other function.
- **The component crate adds only the body.** It needs no mark and no setting.
  It could not carry a mark anyway: it compiles only once the contract is
  generated, so a generator never sees it.
- **The compiler checks the implementation.** A missing or different
  implementation does not compile; the error names the trait.
- **The implementing type is `Component`,** the type the generated forwarding
  file defines. A trait function therefore takes no `self`.
- **The trait is ordinary Rust.** Another environment can implement it for a
  type of its own. Nothing about it requires a component.
- **Only listed functions are exported,** as on an `impl` block.

### What the three forms generate

```wit
interface counters {
  resource counter {
    /// Add `amount` and return the new value.
    add: func(amount: u32) -> u32;

    /// The current value.
    value: func() -> u32;
  }

  /// Create a counter that starts at zero.
  new-counter: func() -> own<counter>;

  /// Open the counter stored under `name`.
  open-counter: func(name: string) -> own<counter>;
}
```

Forms 2 and 3 are indistinguishable in the contract. They differ in what the
generated forwarding calls:

| Form | The generated call |
| --- | --- |
| 1 | `owner::Counter::add(&mut self.borrow_mut(), amount)` |
| 2 | `owner::new_counter()` |
| 3 | `<Component as owner::CounterStore>::open_counter(&name)` |

### Where the interface comes from

Only a root states an interface. A root is where an interface begins: a
function of form 2 or 3, or a type that an interface must own although no
function of that interface leads to it. Everything else follows the root that
mentions it.

| Item | States an interface? |
| --- | --- |
| A free function, or a trait's functions | yes: it is a root |
| A resource that such a function returns or takes | no: it follows the function |
| The functions of a resource | no: they follow the resource |
| A type in a signature or in another type | no: it follows what refers to it |
| A resource or type that nothing leads to, or that two interfaces lead to | yes: `#[wit(interface = "…")]` on the item |

### What happens when

| Situation | Result |
| --- | --- |
| A function is not listed | It is not exported and stays ordinary Rust. |
| A listed name does not exist in the block | Compile error at the name. |
| `impl` block: a function without `self` and without `static` or `constructor` | Compile error at the function. |
| `impl` block: `interface = …` is given | Compile error: the resource's interface is used. |
| The mark stands on `impl Trait for Type` | Compile error: mark the trait's declaration. |
| Trait: a function takes `self` | Compile error at the receiver. |
| Trait: no `interface = …`, or no function listed | Compile error at the trait. |
| Trait: the component does not implement it | Compile error in the component, naming the trait. |
| A signature has a shape the contract cannot carry | Compile error at the function: `async fn`, a generic function, a `&mut` parameter, a returned reference, a `usize` parameter. |

### What no form does

- Nothing is exported because it is `pub`, or because of where it stands in a
  file or a crate. A mark names it.
- No form changes the function. Marks add a hidden description next to it; the
  function compiles and behaves as it does without them.
- No form reaches into the component's `Cargo.toml`. The manifest holds the
  settings of the contract as a whole: its package, its world, and what the
  world imports and exports.

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
| `Schema::from_registry(name)` | Start from everything the marks registered | what the marks declare |
| `schema.name_instantiation::<T>("name")` | Name `T` wherever it is defined | what `#[named]` registers |
| `schema.resource_method::<R>(operation!(R::f))` | Export a function of `R`, whichever interface it belongs to | what `#[exports]` registers |

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

A rule states a representation that differs from what is stored. Most fields
need none: a type of another crate is covered by the crate's map, and `usize`
is widened by itself. A rule is for one field, or for the type as a whole.

| Rule | The field becomes | Checked by the compiler |
| --- | --- | --- |
| `#[wit(display)]` | `string`, from `to_string()`; the same as `into = DisplayText` | the type is `Display` |
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

No derive can be placed on a dependency's type, so its shape is stated once,
as a type of your own. The map then names that type for the foreign one.

```rust
// An enum: a local declaration that the compiler checks against the real one.
#[derive(WitType)]
#[wit(remote = foreign::Endianness)]
pub enum Endianness { Little, Big }

// A wrapped number: one wire record. The map entry states the conversion,
// once for every foreign type that is shown this way.
#[derive(WitType)]
pub struct MachoValue { pub value: u32, pub name: Option<String> }
```

These are the only wire-shaped types a person writes. If the foreign enum gains
a case, the `remote` declaration stops compiling.

Three limits of the map:

- A generic item cannot use it, because the choice is made per concrete field
  type. A foreign field inside a generic struct takes a rule.
- An entry for `T` covers `Option<T>` and `Vec<T>`, not other containers.
- The setting applies to the package's library. A binary, example or
  integration test of the same package is a crate of its own.

## What is generated, and what is not

| Generated | By | When |
| --- | --- | --- |
| The WIT file | `wit_reflect::tasks` | when you run the generator |
| The bindings file | `wit_reflect::tasks` | when you run the generator |
| The forwarding file | `wit_reflect::tasks` | when you run the generator |
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
| The representation map and its setting | 1 declaration with 4 entries, 1 line in `Cargo.toml` |
| Cargo wiring: dependencies and, if wanted, a default-off feature for the macros | a few lines |
| The implementation of a world's imports | — |
| The jco configuration, including which exports are asynchronous | — |
| Everything on the TypeScript side | — |

| Not supported | What happens |
| --- | --- |
| A record, variant or enum as a parameter | error when the forwarding file is generated |
| A resource handle nested in a result | error when the forwarding file is generated |
| `async fn`, `unsafe fn`, generic functions, `impl Trait`, `&mut` parameters, `usize` parameters, consuming `self` | compile error at the function |
| A foreign field inside a generic struct | the map does not reach it; the field takes a rule |
| A type that contains itself | compile error at the field |
| One WIT function backed by anything but exactly one Rust function | write an ordinary Rust function for it first |
| Checking a type placed in an external interface against that interface's WIT | not checked; a mismatch appears when the bindings compile |
| Binding tools other than wit-bindgen 0.63 | the bindings and forwarding files follow its options and naming; a mismatch is a compile error |
| Crossing unchanged for a generic type or an enum whose cases carry data | the type is converted |

## How a value crosses the boundary: wire parts

**Wire parts are the neutral in-between form of a Rust value on its way to the
bindings: the value taken apart field by field, in declaration order, with
every rule and map entry already applied, using only plain Rust types.**

They exist because two crates each know half of the picture. The owner crate
knows the real type, its private fields, its rules and its map, but cannot name
wit-bindgen's types, which are generated later in the component crate. The
component crate has those types but cannot read private fields or know the
rules. Wire parts are what is handed across.

### A record

```rust
// Three of the fixture's six fields.
#[derive(WitType)]
pub struct Image {
    pub minimum_os: foreign::Version,  // the map shows it as text
    pub file_range: FileRange,         // private fields `offset`, `size`
    pub read_count: usize,             // shown as u64
}
```

For `minimum_os` 14.4, a range of offset 0 and size 4096, and a count of 1,
the wire parts are:

```rust
Fields((
    "14.4".to_string(),               // the map's entry applied
    (Fields((0_u64, (4096_u64, ()))), // the nested record, taken apart too
    (1_u64,                           // widened
    ()))
))
```

- Every representation is already applied: no `Version` and no `usize` is left.
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
    Decode(foreign::DecodeError),                            // case 3, text by the map
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
| No field is a `usize`, or a type the map shows as another type | The binding holds the shown type, not the stored one |
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
| `Report` | converted | it holds `Image`, whose fields are shown as other types |
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
   = note: for a type of another crate, put a rule on the field or add the type to the
           crate's `wit_reflect::representations!` map

error[E0277]: the trait bound `MachoValue: From<Platform>` is not satisfied
              (at the entry `Platform => MachoValue` of the map)

error: this type's path starts with `std`, but the entry stands in the group `object`

error: this block has no function of that name
       (at a name in `#[exports(…)]`)

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
           forwarding files, then rebuild
```

## Checklist for a new contract

1. Derive `WitType` on each type an operation returns or takes; the compiler
   then points at every type it reaches that still needs it.
2. Where the compiler reports a field without a WIT representation, add its
   type to the crate's map if it belongs to another crate, or a rule if the
   value depends on that one field.
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
