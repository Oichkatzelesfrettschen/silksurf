//! Error, `TypeError`, `SyntaxError`, `RangeError`, `ReferenceError` constructors.

use std::cell::RefCell;
use std::rc::Rc;

use super::native_fn;
use crate::vm::value::{Object, PropertyKey, Value};

pub fn install(global: &mut Object) {
    global.set_by_str("Error", make_error_constructor("Error"));
    global.set_by_str("TypeError", make_error_constructor("TypeError"));
    global.set_by_str("SyntaxError", make_error_constructor("SyntaxError"));
    global.set_by_str("RangeError", make_error_constructor("RangeError"));
    global.set_by_str("ReferenceError", make_error_constructor("ReferenceError"));
    global.set_by_str("URIError", make_error_constructor("URIError"));
    global.set_by_str("EvalError", make_error_constructor("EvalError"));
}

fn make_error_constructor(name: &str) -> Value {
    let error_name = name.to_string();
    native_fn(name, move |args: &[Value]| {
        let message = args
            .first()
            .map(|v| {
                let s = v.to_js_string();
                s.as_str().unwrap_or("").to_string()
            })
            .unwrap_or_default();

        let obj = Object::new();
        let obj_rc = Rc::new(RefCell::new(obj));
        {
            let mut o = obj_rc.borrow_mut();
            o.set_by_key(PropertyKey::string_key("name"), Value::string(&error_name));
            o.set_by_key(PropertyKey::string_key("message"), Value::string(&message));
            o.set_by_key(
                PropertyKey::string_key("stack"),
                Value::string(&format!("{error_name}: {message}")),
            );
        }
        Value::Object(obj_rc)
    })
}
