//! Host object interface for native-backed JS objects.
//!
//! `HostObject` allows Rust types (DOM nodes, Response objects, etc.)
//! to be exposed to JavaScript with custom property access and method calls.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use super::value::Value;

/// Trait for native objects accessible from JavaScript.
///
/// Implementors provide custom property get/set and method call behavior.
/// The JS VM dispatches to these methods via `op_get_prop/op_set_prop/op_call`.
pub trait HostObject: fmt::Debug {
    /// Get a property by name. Return `Value::Undefined` for missing properties.
    fn get_property(&self, name: &str) -> Value;

    /// Set a property by name. Return true if accepted.
    fn set_property(&mut self, name: &str, value: Value) -> bool;

    /// Get the class name (for typeof, toString, etc.)
    fn class_name(&self) -> &'static str {
        "Object"
    }

    /// Downcast to concrete type for internal use.
    fn as_any(&self) -> &dyn Any;

    /// Mutable downcast.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A reference-counted host object, shareable between JS values.
pub type HostObjectRef = Rc<RefCell<dyn HostObject>>;

/// Create a new `HostObjectRef` from a concrete `HostObject` implementor.
pub fn make_host_object<T: HostObject + 'static>(obj: T) -> HostObjectRef {
    Rc::new(RefCell::new(obj))
}
