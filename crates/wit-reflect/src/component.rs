//! Support for the component crate that hosts generated forwarding.

use std::cell::{Ref, RefCell, RefMut};

/// The type a component registers for a resource of type `T`.
///
/// A WIT method receives a shared borrow of its resource, while the ordinary
/// Rust method it forwards to may take `&mut self`. The cell provides that
/// access with a runtime borrow check. A component instance runs one call at a
/// time, so a borrow conflict means a method was re-entered while it was still
/// running.
#[derive(Debug)]
pub struct ResourceCell<T> {
    value: RefCell<T>,
}

impl<T> ResourceCell<T> {
    /// Wrap the value a constructor or factory produced.
    pub fn new(value: T) -> Self {
        Self {
            value: RefCell::new(value),
        }
    }

    /// Borrow the resource for a `&self` method.
    ///
    /// # Panics
    ///
    /// Panics if a `&mut self` method of this resource is still running.
    pub fn borrow(&self) -> Ref<'_, T> {
        self.value.borrow()
    }

    /// Borrow the resource for a `&mut self` method.
    ///
    /// # Panics
    ///
    /// Panics if another method of this resource is still running.
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        self.value.borrow_mut()
    }
}
