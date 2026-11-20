//! The graph of named definitions reached while reflecting declared types.

use std::collections::HashMap;

use crate::types::{Type, TypeDef, TypeDefKind, TypeKey};

fn reference_to(key: TypeKey, definition: &TypeDef) -> Type {
    match definition.kind() {
        TypeDefKind::Resource => Type::Own(key),
        TypeDefKind::Record(_) | TypeDefKind::Variant(_) | TypeDefKind::Enum(_) => Type::Named(key),
    }
}

/// Named definitions collected while reflecting types.
///
/// A definition is identified by its Rust type, not by its WIT name. Each
/// instantiation of a generic type is therefore recorded separately, and a
/// type reached through an alias, a re-export or several fields is recorded
/// once.
///
/// Definitions are kept in completion order: a definition is stored after the
/// definitions its fields refer to. That order depends only on the order of
/// declarations and fields, so generated output is reproducible.
#[derive(Debug, Default)]
pub struct TypeGraph {
    definitions: Vec<(TypeKey, TypeDef)>,
    /// Position in `definitions`. Used for lookup only, never iterated.
    positions: HashMap<TypeKey, usize>,
    /// Definitions whose `describe` closure is currently running.
    in_progress: Vec<TypeKey>,
    cycles: Vec<Vec<&'static str>>,
}

impl TypeGraph {
    /// Record the named definition of `T` and return a reference to it.
    ///
    /// `describe` runs only the first time `T` is reached. It reflects the
    /// field types through the graph it receives, which is how dependencies
    /// are discovered.
    ///
    /// The reference to a resource is an owned handle, [`Type::Own`]. Every
    /// other definition is referred to by [`Type::Named`].
    ///
    /// A type that reaches itself has no WIT representation. The cycle is
    /// recorded and reported by [`Schema::resolve`](crate::Schema::resolve);
    /// reflection still terminates.
    pub fn define<T: ?Sized + 'static>(
        &mut self,
        describe: impl FnOnce(&mut Self) -> TypeDef,
    ) -> Type {
        let key = TypeKey::of::<T>();

        if let Some(position) = self.positions.get(&key) {
            let (_, definition) = &self.definitions[*position];
            return reference_to(key, definition);
        }
        let enclosing_position = self
            .in_progress
            .iter()
            .position(|in_progress| *in_progress == key);
        if let Some(enclosing_position) = enclosing_position {
            // Only a definition with members can contain itself, and those
            // are referred to by name.
            let cycle = self.cycle_from(enclosing_position, key);
            self.cycles.push(cycle);
            return Type::Named(key);
        }

        self.in_progress.push(key);
        let definition = describe(self);
        self.in_progress.pop();

        let reference = reference_to(key, &definition);
        let position = self.definitions.len();
        self.definitions.push((key, definition));
        self.positions.insert(key, position);
        reference
    }

    /// The Rust type names from the enclosing definition back to itself.
    fn cycle_from(&self, enclosing_position: usize, repeated: TypeKey) -> Vec<&'static str> {
        let mut cycle: Vec<&'static str> = self.in_progress[enclosing_position..]
            .iter()
            .map(TypeKey::rust_name)
            .collect();
        cycle.push(repeated.rust_name());
        cycle
    }

    /// All definitions in completion order.
    pub(crate) fn definitions(&self) -> &[(TypeKey, TypeDef)] {
        &self.definitions
    }

    /// The definition a [`Type::Named`] reference points to.
    ///
    /// # Panics
    ///
    /// Panics if `key` was created by another graph.
    pub(crate) fn definition(&self, key: TypeKey) -> &TypeDef {
        let position = *self
            .positions
            .get(&key)
            .expect("keys are created only by `define`, which records their definition");
        let (_, definition) = &self.definitions[position];
        definition
    }

    pub(crate) fn cycles(&self) -> &[Vec<&'static str>] {
        &self.cycles
    }
}
