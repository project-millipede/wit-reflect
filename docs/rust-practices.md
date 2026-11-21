# Rust practices in this workspace

This workspace follows the Apple Code Workbench guidance: its development
guidelines, Rust implementation practices, operation structure, integer
conventions, error-handling contract and testing policy. Those documents remain
the authority and are not repeated here.

The practices below are not stated there. They matter for a code generator and a
proc-macro, and they are applied in this workspace. Each is a candidate to add to
the workbench Rust guide.

## 1. Generated output is deterministic

**Rule:** identical declarations produce byte-identical output.

Iteration order of `HashMap` and `HashSet`, and the ordering of `TypeId`, differ
between runs or builds. Use them for lookup only. Anything that reaches output or
a diagnostic iterates a `Vec` in declaration order or a `BTreeMap`/`BTreeSet`.

Example: `TypeGraph` stores definitions in a `Vec` and uses its `HashMap` only to
find a position.

## 2. Proc-macros report, they do not panic

**Rule:** a derive rejects unsupported input with `syn::Error` at the offending
tokens, combines all errors of one item, and never panics.

- Use fully qualified paths (`::core::option::Option::Some`) in expansions, so a
  user's local names cannot change their meaning.
- Offer a `crate = path` attribute for re-exported runtimes.
- Keep rules in the runtime crate. The macro translates declared shape into
  runtime calls, so derived and handwritten implementations are validated by the
  same code.

[Rust Reference: procedural macros](https://doc.rust-lang.org/reference/procedural-macros.html)

## 3. Public types can evolve

**Rule:** decide for each public type what may change later.

| Situation | Choice |
| --- | --- |
| An enum that will gain variants | `#[non_exhaustive]` |
| A struct with invariants | Private fields and constructors |
| A builder or a pure constructor | `#[must_use]` |
| A dependency's type in a signature | Avoid it unless that dependency is part of the contract |

Example: `PackageNameError` stores the version parser's message as text, so
`semver` is not part of this crate's public API.

[Rust API Guidelines: future proofing](https://rust-lang.github.io/api-guidelines/future-proofing.html)

## 4. Common traits and conversions are implemented deliberately

**Default:** data types derive `Debug`, `Clone`, `PartialEq` and `Eq` where those
are meaningful. Text forms come in pairs (`FromStr` with `Display`). Every error
implements `std::error::Error`.

Error messages are lowercase, have no trailing punctuation and read as part of a
sentence, because callers embed them. An aggregate error is the exception: it
prints one line per problem.

A hand-written `PartialEq` or `Hash` states which fields define identity.
`TypeKey` compares its `TypeId` only; the type name is descriptive.

[Rust API Guidelines: interoperability](https://rust-lang.github.io/api-guidelines/interoperability.html)

## 5. Trait misuse gets a tailored compiler message

**Default:** a trait that users are expected to hit accidentally carries
`#[diagnostic::on_unimplemented]` explaining the usual cause and the fix.

Example: a `usize` field in a derived type reports that target-dependent integers
are not wire types, instead of only "trait bound not satisfied".

[Rust Reference: the `diagnostic` namespace](https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-diagnostic-tool-attribute-namespace)

## 6. Lints and toolchain are declared once

**Rule:** shared lints live in `[workspace.lints]`; each crate opts in with
`[lints] workspace = true`. `unsafe_code` is forbidden, `missing_docs` warns, and
Clippy's pedantic group is enabled. An allowed lint has a comment stating why.
The workspace declares its `rust-version`.

[Cargo Book: the lints table](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section)

## 7. Libraries declare ranges; lockfiles pin

**Rule:** a library states compatible version ranges and commits `Cargo.lock`.

An exact `=` requirement in a library conflicts with any consumer workspace that
pins the same crate to another patch release, because Cargo allows one version
per compatible range. Exact pins belong to applications and executables.

## 8. Optional integration is feature-gated at the owner

**Rule:** an owner crate enables generation metadata behind a default-off
feature (`cfg_attr(feature = "wit", derive(...))`). Its ordinary build then
compiles no generator code. Features stay additive.

## 9. Examples are executable

**Default:** a public item's example is a doctest that asserts its result and
uses `?` for fallible steps. An example that cannot compile in some feature
configuration is gated explicitly rather than marked `ignore` everywhere.

## 10. Generated files are checked, never silently repaired

**Rule:** a file generated from source and committed has two commands: one that
rewrites it and one that fails when it is stale. The check changes nothing.

A check that also repaired would hide that a contract changed. Committing the
output makes such a change visible in review, and a test that compares the
committed file with the generator's output keeps the two from drifting.

Example: `cargo run -p session-owner --bin generate` rewrites the fixture's WIT
bindings file and forwarding file; `-- --check` reports stale files and exits
with an error.

## 11. One plan, several outputs

**Rule:** when two generated outputs must agree, derive both from one parsed
representation instead of parsing the input twice.

Example: the `WitType` derive plans an item once and generates the schema
reflection and the lowering from that plan, so a rule cannot apply to one and
not the other.
