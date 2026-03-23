//! Built-in objects and functions for the JS runtime.
//!
//! Provides console, JSON, Math, Array/String prototypes,
//! and global functions (parseInt, parseFloat, isNaN, etc.).

pub mod array;
mod console;
mod error;
mod fetch_builtin;
mod globals;
pub mod json;
mod math;
mod promise_builtin;
mod storage;
pub mod string_proto;
mod timers_builtin;
pub mod window;

use std::cell::RefCell;
use std::rc::Rc;

use super::value::{NativeFunction, Object, PropertyKey, Value};

/// Install all built-in objects and functions onto the global object.
pub fn install_builtins(global: &Rc<RefCell<Object>>) {
    let mut g = global.borrow_mut();
    console::install(&mut g);
    json::install(&mut g);
    math::install(&mut g);
    globals::install(&mut g);
    promise_builtin::install(&mut g);
    error::install(&mut g);
    timers_builtin::install(&mut g);
    fetch_builtin::install(&mut g);
    storage::install(&mut g);
    window::install(&mut g);
    drop(g);
    window::install_window_self(global);
}

/// Helper: create a native function Value.
fn native_fn(name: &str, func: impl Fn(&[Value]) -> Value + 'static) -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(name, func)))
}

/// Helper: create an object with methods.
fn make_object_with_methods(methods: Vec<(&str, Value)>) -> Value {
    let obj = Object::new();
    let obj_rc = Rc::new(RefCell::new(obj));
    {
        let mut o = obj_rc.borrow_mut();
        for (name, value) in methods {
            o.set_by_key(PropertyKey::from_str(name), value);
        }
    }
    Value::Object(obj_rc)
}
