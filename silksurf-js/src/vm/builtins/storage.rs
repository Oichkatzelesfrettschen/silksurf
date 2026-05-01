//! localStorage and sessionStorage stubs.
//!
//! Backed by in-memory HashMap. Data does not persist across sessions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::native_fn;
use crate::vm::value::{Object, PropertyKey, Value};

pub fn install(global: &mut Object) {
    global.set_by_str("localStorage", make_storage());
    global.set_by_str("sessionStorage", make_storage());
}

fn make_storage() -> Value {
    let store: Rc<RefCell<HashMap<String, String>>> = Rc::new(RefCell::new(HashMap::new()));

    let obj = Object::new();
    let obj_rc = Rc::new(RefCell::new(obj));

    let store_get = Rc::clone(&store);
    let get_item = Value::NativeFunction(Rc::new(crate::vm::value::NativeFunction::new(
        "getItem",
        move |args| {
            let key = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let s = store_get.borrow();
            match s.get(&key) {
                Some(val) => Value::string(val),
                None => Value::Null,
            }
        },
    )));

    let store_set = Rc::clone(&store);
    let set_item = Value::NativeFunction(Rc::new(crate::vm::value::NativeFunction::new(
        "setItem",
        move |args| {
            let key = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let val = args
                .get(1)
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            store_set.borrow_mut().insert(key, val);
            Value::Undefined
        },
    )));

    let store_remove = Rc::clone(&store);
    let remove_item = Value::NativeFunction(Rc::new(crate::vm::value::NativeFunction::new(
        "removeItem",
        move |args| {
            let key = args
                .first()
                .map(|v| {
                    let s = v.to_js_string();
                    s.as_str().unwrap_or("").to_string()
                })
                .unwrap_or_default();
            store_remove.borrow_mut().remove(&key);
            Value::Undefined
        },
    )));

    let store_clear = Rc::clone(&store);
    let clear = Value::NativeFunction(Rc::new(crate::vm::value::NativeFunction::new(
        "clear",
        move |_args| {
            store_clear.borrow_mut().clear();
            Value::Undefined
        },
    )));

    let store_len = Rc::clone(&store);
    let length = native_fn("length", move |_args| {
        Value::Number(store_len.borrow().len() as f64)
    });

    {
        let mut o = obj_rc.borrow_mut();
        o.set_by_key(PropertyKey::from_str("getItem"), get_item);
        o.set_by_key(PropertyKey::from_str("setItem"), set_item);
        o.set_by_key(PropertyKey::from_str("removeItem"), remove_item);
        o.set_by_key(PropertyKey::from_str("clear"), clear);
        o.set_by_key(PropertyKey::from_str("length"), length);
    }

    Value::Object(obj_rc)
}
