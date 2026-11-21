//! Which owner types the bindings may use themselves.
//!
//! A type that crosses unchanged is mapped with wit-bindgen's `with`, so the
//! bindings read its fields and discriminants directly. A wrong decision is
//! therefore not always a compile error: an enum with explicit discriminants
//! would be lowered to another case.

#![allow(dead_code, reason = "fixture items are reflected, never used")]
#![allow(
    missing_docs,
    reason = "fixture items are public because only a public type can cross unchanged"
)]

use wit_reflect::WitType;
use wit_reflect::lower::{Lower, Passage};

#[derive(Clone, Copy, Debug, WitType)]
pub enum Kind {
    File,
    Directory,
}

#[derive(Clone, Debug, WitType)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub sizes: Vec<u64>,
    pub link: Option<Kind>,
}

// The binding lowers an enum by its discriminant, which here is not the case's
// position.
#[derive(Clone, Copy, Debug, WitType)]
pub enum Numbered {
    Seven = 7,
    Six = 6,
}

#[derive(Clone, Debug, WitType)]
pub struct Hidden {
    pub offset: u64,
    size: u64,
}

// Nothing on this type itself prevents the bindings from using it.
#[derive(Clone, Debug, WitType)]
pub struct HoldsHidden {
    pub range: Hidden,
}

#[derive(Clone, Debug, WitType)]
pub struct Ruled {
    #[wit(widen)]
    pub count: usize,
}

#[derive(Clone, Debug, WitType)]
pub struct Renamed {
    #[wit(name = "identifier")]
    pub id: u32,
}

#[derive(Clone, Debug, WitType)]
pub struct Carrier<Item> {
    pub items: Vec<Item>,
}

// The bindings clone and print the values they hold.
#[derive(WitType)]
pub struct NotCloned {
    pub value: u32,
}

#[derive(Clone, Debug, WitType)]
#[wit(convert)]
pub struct Declined {
    pub value: u32,
}

#[derive(Clone, Debug, WitType)]
pub enum WithPayload {
    Empty,
    Entry(Entry),
}

fn crosses_unchanged<Owner: Lower>() -> bool {
    <Owner::Passage as Passage>::UNCHANGED
}

#[test]
fn a_type_crosses_unchanged_only_if_the_bindings_can_use_it_as_it_is() {
    // Usable as it is, with everything inside.
    assert!(crosses_unchanged::<Kind>());
    assert!(crosses_unchanged::<Entry>());
    assert!(crosses_unchanged::<Vec<Option<Entry>>>());
    assert!(crosses_unchanged::<Result<Entry, Kind>>());

    // The discriminant is not the case's position.
    assert!(!crosses_unchanged::<Numbered>());
    // A field the bindings cannot read, and a type that only contains one.
    assert!(!crosses_unchanged::<Hidden>());
    assert!(!crosses_unchanged::<HoldsHidden>());
    assert!(!crosses_unchanged::<Vec<HoldsHidden>>());
    // A value or a name that differs from what is stored.
    assert!(!crosses_unchanged::<Ruled>());
    assert!(!crosses_unchanged::<Renamed>());
    // wit-bindgen cannot map a generic type, even one holding unchanged values.
    assert!(!crosses_unchanged::<Carrier<Entry>>());
    // Not `Clone` and `Debug`.
    assert!(!crosses_unchanged::<NotCloned>());
    // Declared with `convert`.
    assert!(!crosses_unchanged::<Declined>());
    // Cases with payloads are always converted.
    assert!(!crosses_unchanged::<WithPayload>());
    // The binding holds the content of a box, not the box.
    assert!(!crosses_unchanged::<Box<Entry>>());
}
