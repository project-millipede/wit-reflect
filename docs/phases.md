# Definitions and phases

**Status: Phase 1 implemented, 2026-11-20. Phases 2–5 are planned, not started.**

The goal is the generation model required by the generator comparison:

```text
Existing Rust operations and types + declarative generation metadata
    -> Generated application WIT and required boundary adaptation
    -> wit-bindgen bindings, with compatible Rust type mappings
    -> jco / TypeScript bindings
```

wit-bindgen keeps the canonical ABI and jco the TypeScript side. This library
produces the WIT and, from Phase 4, the adaptation onto those bindings.

## Definitions

| Term | Meaning |
| --- | --- |
| Owner type | The struct or enum the ordinary Rust implementation already uses. The only place a field is edited. |
| Reflection | `WitType::wit_type`, implemented by the derive in the owner crate. Rust has already resolved aliases, re-exports, dependency crates and generic arguments. |
| Type graph | Every named definition reached from declared types, identified by `TypeId`, in dependency-first order. |
| Instantiation | One concrete use of a generic owner type. Always its own definition. |
| Schema | The declaration of a package: interfaces, the types each defines, instantiation names. |
| Placement | The rule giving each named type exactly one owning interface; other interfaces `use` it. |
| Representation rule | A declarative attribute for a real representation difference. It drives both the schema and the generated adaptation. (Phase 3) |
| Adaptation | Generated lowering from owner values to wit-bindgen binding values. (Phase 4) |
| Forwarding | Generated `Guest` and resource implementations that call the ordinary Rust operation. (Phase 4) |
| Resolved package | The validated result of a schema. Only this can be rendered. |

## What each generator contributed

| Capability | Source of the idea | What this library adds |
| --- | --- | --- |
| Compiler-resolved type reuse and generic carriers | Witty: derive plus trait registration | No layout or ABI bounds on the owner derive, only `T: WitType`. |
| Distinct instantiations | None of the three (first-wins, or arguments lost) | `TypeId` identity, a collision error, declared instantiation names. |
| Aliases, re-exports, dependency types, `cfg_attr` | None | A consequence of reflection instead of source scanning. |
| Keyword-safe names such as `%result` | WITGenstein, Fidius | One keyword table applied to every identifier position. |
| Struct-like case payload records | Fidius | A declared name, or one derived from the resolved variant name. |
| Cross-interface type use | None | The placement rule and generated `use` lines. |
| Failure detection | Fidius: errors for unsupported types | All diagnostics aggregated; compile-time rejection with tailored messages. |
| Resources with constructors | WITGenstein | Free factory and per-method selection. (Phase 2) |
| Generated conversions and guest glue | Fidius | Generated against owner types, not an author model. (Phase 4) |
| Semantic projections | None | Representation rules. (Phase 3) |

## Phases

Each phase ends with formatting, Clippy, tests, a status update here and a review.

### Phase 1 — Type reflection (done)

Owner types to validated WIT type definitions.

- `WitType` for primitives, `String`, `Vec`, `Option`, `Result`, `Box`, tuples, `()`.
- Derive: records, enums, variants, payload records, transparent newtypes,
  declared names, doc comments, generic parameters.
- `Schema`, placement, naming, aggregated `SchemaErrors`, deterministic rendering.

Evidence: `crates/wit-reflect/tests/type_reflection.rs`.

| Test | What it establishes |
| --- | --- |
| `generic_owner_types_render_the_handwritten_contract` | Two instantiations through Rust aliases, nested `%result`, payload record and cross-interface `use` equal the handwritten text. |
| `rendered_package_is_accepted_by_the_wit_parser` | That text is valid WIT for `wit-parser` 0.261. |
| `unnamed_instantiations_of_one_generic_type_are_rejected` | Instantiations are never merged. |
| `undeclared_type_shared_by_two_interfaces_is_rejected` | Placement is never arbitrary. |
| `recursive_types_are_reported` | Reflection terminates on recursive types. |

Against the acceptance criteria: criterion 1 (direct type reuse) is met for type
definitions; the type half of criteria 2 and 4 follows from it. Nothing has been
run against the workbench's real `Session::inspect` types yet.

### Phase 2 — Operations and worlds

Free functions; resources reflected from `impl` blocks with per-method selection;
constructor and free factory (`func() -> own<session>`); borrowed receivers and
parameters; worlds with exports and imports; references to pinned external WIT
such as `apple-code:filesystem/source.{read-error}`. Completes criterion 2.

### Phase 3 — Representation rules

`Display` to `string`; value-plus-name projection; checked integer widening
(`usize` to `u64`); declarations for foreign types; ordered diagnostics; field
skipping. Rules change the reflected wire shape. Schema side of criterion 3.

### Phase 4 — Adaptation and forwarding

Lowering generated by the derive inside the owner crate, where private fields are
accessible. The binding side attaches through wit-bindgen's `additional_derives`,
which covers records, variants and enums in wit-bindgen-rust 0.60. Generated
`Guest` and resource forwarding. The phase starts with a spike that confirms this
static approach before it is adopted.

### Phase 5 — Build integration and qualification

Generator entry point and wit-bindgen wiring. Then, on a workbench branch: the
five `Session::inspect` scenarios natively and through jco, the field-change
propagation proof (criterion 4), and removal of the handwritten WIT and adapters
(criterion 5).

## Open points for later phases

- Where generated WIT is written so that `wit_bindgen::generate!` can read it.
  The macro resolves paths against the manifest directory.
- Whether payload-free Rust enums used as inputs need lifting support.
- How rules address types this workspace does not own, such as `object` wrappers.
