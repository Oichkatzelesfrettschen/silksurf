/*
 * builtins/mod.rs -- all built-in JS objects and global functions.
 *
 * WHY: JavaScript engines must provide a rich set of built-in objects
 * (console, JSON, Math, Error, Promise, Array, String, etc.) and
 * global functions (parseInt, parseFloat, isNaN, fetch, setTimeout).
 * These are installed on the global object at VM creation.
 *
 * Module map:
 *   console.rs       -- console.log/warn/error/info/debug
 *   json.rs          -- JSON.parse (serde_json) / JSON.stringify
 *   math.rs          -- Math.* (17 methods + 8 constants)
 *   error.rs         -- Error/TypeError/SyntaxError/RangeError/ReferenceError
 *   globals.rs       -- parseInt, parseFloat, isNaN, isFinite, encode/decodeURI
 *   promise_builtin.rs -- Promise.resolve/reject/all/race
 *   timers_builtin.rs  -- setTimeout, setInterval, rAF, queueMicrotask
 *   fetch_builtin.rs   -- fetch() -> Promise<Response>
 *   storage.rs         -- localStorage / sessionStorage (in-memory HashMap)
 *   window.rs          -- window/self/globalThis, performance.now(), navigator
 *   array.rs           -- Array.prototype (14 methods: push, map, filter, etc.)
 *   string_proto.rs    -- String.prototype (20 methods: split, replace, etc.)
 *
 * install_builtins() is called from Vm::new() to populate the global object.
 * The last step installs window/self/globalThis as self-referential pointers.
 *
 * See: vm/mod.rs Vm::new() for installation call
 * See: value.rs NativeFunction for how Rust closures become JS functions
 */

pub mod array;
mod console;
mod error;
mod fetch_builtin;
mod globals;
pub mod json;
pub mod map_set;
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
    map_set::install(&mut g);
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
            o.set_by_key(PropertyKey::string_key(name), value);
        }
    }
    Value::Object(obj_rc)
}
