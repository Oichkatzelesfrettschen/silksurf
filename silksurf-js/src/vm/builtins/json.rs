//! JSON object (parse, stringify)
//!
//! Uses `serde_json` for parsing/serialization -- battle-tested, SIMD-friendly
//! internal scanner, and zero-copy string handling where possible.

use std::cell::RefCell;
use std::rc::Rc;

use super::{make_object_with_methods, native_fn};
use crate::vm::value::{Object, PropertyKey, Value};

pub fn install(global: &mut Object) {
    let json = make_object_with_methods(vec![
        ("parse", native_fn("JSON.parse", json_parse)),
        ("stringify", native_fn("JSON.stringify", json_stringify)),
    ]);
    global.set_by_str("JSON", json);
}

fn json_parse(args: &[Value]) -> Value {
    let Some(text) = args.first() else {
        return Value::Undefined;
    };
    let s = text.to_js_string();
    let input = s.as_str().unwrap_or("");

    match serde_json::from_str::<serde_json::Value>(input) {
        Ok(parsed) => serde_to_js(&parsed),
        Err(_) => Value::Undefined, // Should throw SyntaxError in full impl
    }
}

fn json_stringify(args: &[Value]) -> Value {
    let Some(val) = args.first() else {
        return Value::Undefined;
    };
    // Optional: indent argument (args[2] for space parameter)
    let indent = args.get(2).and_then(|v| match v {
        Value::Number(n) => Some(*n as usize),
        _ => None,
    });

    let json_val = js_to_serde(val);
    let result = if let Some(spaces) = indent {
        let buf = Vec::new();
        let indent_bytes = b" ".repeat(spaces);
        let formatter = serde_json::ser::PrettyFormatter::with_indent(&indent_bytes);
        let mut ser = serde_json::Serializer::with_formatter(buf, formatter);
        if serde::Serialize::serialize(&json_val, &mut ser).is_ok() {
            String::from_utf8(ser.into_inner()).unwrap_or_default()
        } else {
            return Value::Undefined;
        }
    } else {
        match serde_json::to_string(&json_val) {
            Ok(s) => s,
            Err(_) => return Value::Undefined,
        }
    };
    Value::string_owned(result)
}

/// Convert `serde_json::Value` -> JS Value (public for use by fetch).
#[must_use]
pub fn serde_to_js_public(val: &serde_json::Value) -> Value {
    serde_to_js(val)
}

fn serde_to_js(val: &serde_json::Value) -> Value {
    match val {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(s) => Value::string(s),
        serde_json::Value::Array(arr) => {
            let obj = Object::new();
            let obj_rc = Rc::new(RefCell::new(obj));
            {
                let mut o = obj_rc.borrow_mut();
                for (i, item) in arr.iter().enumerate() {
                    o.set_by_key(PropertyKey::Index(i as u32), serde_to_js(item));
                }
                o.set_by_str("length", Value::Number(arr.len() as f64));
            }
            Value::Object(obj_rc)
        }
        serde_json::Value::Object(map) => {
            let obj = Object::new();
            let obj_rc = Rc::new(RefCell::new(obj));
            {
                let mut o = obj_rc.borrow_mut();
                for (key, val) in map {
                    o.set_by_key(PropertyKey::string_key(key), serde_to_js(val));
                }
            }
            Value::Object(obj_rc)
        }
    }
}

/// Convert JS Value -> `serde_json::Value`
fn js_to_serde(val: &Value) -> serde_json::Value {
    match val {
        Value::Undefined | Value::Function(_) | Value::NativeFunction(_) | Value::HostObject(_) => {
            serde_json::Value::Null
        }
        Value::Null => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => {
            if n.is_finite() {
                serde_json::Number::from_f64(*n)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number)
            } else {
                serde_json::Value::Null
            }
        }
        Value::String(s) => serde_json::Value::String(s.as_str().unwrap_or("").to_string()),
        Value::Object(obj) => {
            let o = obj.borrow();
            // Check if array-like (has numeric "length" property)
            let length = match o.get_by_str("length") {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => Some(n as usize),
                _ => None,
            };
            if let Some(len) = length {
                let mut arr = Vec::with_capacity(len);
                for i in 0..len {
                    arr.push(js_to_serde(&o.get_by_key(&PropertyKey::Index(i as u32))));
                }
                serde_json::Value::Array(arr)
            } else {
                let mut map = serde_json::Map::new();
                for (key, val) in &o.properties {
                    if let PropertyKey::String(s) = key
                        && let Some(name) = s.as_str()
                    {
                        map.insert(name.to_string(), js_to_serde(val));
                    }
                }
                serde_json::Value::Object(map)
            }
        }
    }
}
