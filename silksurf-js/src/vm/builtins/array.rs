//! Array built-in methods.
//!
//! Arrays are objects with integer-indexed properties and a "length" property.
//! These methods are dispatched when accessing properties on array-like objects.

use std::cell::RefCell;
use std::rc::Rc;

use crate::vm::value::{NativeFunction, Object, PropertyKey, Value};

/// Check if an object looks like an array (has numeric "length").
pub fn is_array_like(obj: &Object) -> bool {
    matches!(obj.get_by_str("length"), Value::Number(n) if n >= 0.0)
}

/// Get the length of an array-like object.
fn array_length(obj: &Object) -> usize {
    match obj.get_by_str("length") {
        Value::Number(n) if n >= 0.0 => n as usize,
        _ => 0,
    }
}

/// Collect array elements into a Vec<Value>.
fn collect_elements(obj: &Object) -> Vec<Value> {
    let len = array_length(obj);
    let mut result = Vec::with_capacity(len);
    for i in 0..len {
        result.push(obj.get_by_key(&PropertyKey::Index(i as u32)));
    }
    result
}

/// Set array elements from a Vec<Value> and update length.
fn set_elements(obj: &mut Object, elements: &[Value]) {
    // Clear old numeric indices
    obj.properties
        .retain(|k, _| !matches!(k, PropertyKey::Index(_)));
    for (i, val) in elements.iter().enumerate() {
        obj.set_by_key(PropertyKey::Index(i as u32), val.clone());
    }
    obj.set_by_str("length", Value::Number(elements.len() as f64));
}

/// Create a new JS array from elements.
pub fn create_array(elements: Vec<Value>) -> Value {
    let obj = Object::new();
    let obj_rc = Rc::new(RefCell::new(obj));
    {
        let mut o = obj_rc.borrow_mut();
        for (i, val) in elements.iter().enumerate() {
            o.set_by_key(PropertyKey::Index(i as u32), val.clone());
        }
        o.set_by_str("length", Value::Number(elements.len() as f64));
    }
    Value::Object(obj_rc)
}

/// Look up an array method by name. Returns a NativeFunction Value if found.
pub fn get_array_method(obj_rc: &Rc<RefCell<Object>>, name: &str) -> Option<Value> {
    let method: Option<Box<dyn Fn(&[Value]) -> Value>> = match name {
        "push" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let mut o = arr.borrow_mut();
                let mut len = array_length(&o);
                for arg in args {
                    o.set_by_key(PropertyKey::Index(len as u32), arg.clone());
                    len += 1;
                }
                o.set_by_str("length", Value::Number(len as f64));
                Value::Number(len as f64)
            }))
        }
        "pop" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |_args: &[Value]| {
                let mut o = arr.borrow_mut();
                let len = array_length(&o);
                if len == 0 {
                    return Value::Undefined;
                }
                let last = o.get_by_key(&PropertyKey::Index((len - 1) as u32));
                o.properties.remove(&PropertyKey::Index((len - 1) as u32));
                o.set_by_str("length", Value::Number((len - 1) as f64));
                last
            }))
        }
        "indexOf" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let len = array_length(&o);
                for i in 0..len {
                    let elem = o.get_by_key(&PropertyKey::Index(i as u32));
                    if strict_equal(&elem, &target) {
                        return Value::Number(i as f64);
                    }
                }
                Value::Number(-1.0)
            }))
        }
        "includes" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let len = array_length(&o);
                for i in 0..len {
                    let elem = o.get_by_key(&PropertyKey::Index(i as u32));
                    if strict_equal(&elem, &target) {
                        return Value::Boolean(true);
                    }
                }
                Value::Boolean(false)
            }))
        }
        "join" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let sep = args
                    .first()
                    .map(|v| {
                        let s = v.to_js_string();
                        s.as_str().unwrap_or(",").to_string()
                    })
                    .unwrap_or_else(|| ",".to_string());
                let elements = collect_elements(&o);
                let parts: Vec<String> = elements
                    .iter()
                    .map(|v| {
                        if v.is_nullish() {
                            String::new()
                        } else {
                            let s = v.to_js_string();
                            s.as_str().unwrap_or("").to_string()
                        }
                    })
                    .collect();
                Value::string_owned(parts.join(&sep))
            }))
        }
        "slice" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let len = array_length(&o) as i64;
                let start = args.first().map_or(0, |v| {
                    let n = v.to_number() as i64;
                    if n < 0 { (len + n).max(0) } else { n.min(len) }
                });
                let end = args.get(1).map_or(len, |v| {
                    let n = v.to_number() as i64;
                    if n < 0 { (len + n).max(0) } else { n.min(len) }
                });
                let mut result = Vec::new();
                for i in start..end {
                    result.push(o.get_by_key(&PropertyKey::Index(i as u32)));
                }
                create_array(result)
            }))
        }
        "forEach" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                if let Value::NativeFunction(func) = &callback {
                    let elements = collect_elements(&o);
                    for (i, elem) in elements.iter().enumerate() {
                        func.call(&[elem.clone(), Value::Number(i as f64)]);
                    }
                }
                Value::Undefined
            }))
        }
        "map" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let elements = collect_elements(&o);
                if let Value::NativeFunction(func) = &callback {
                    let mapped: Vec<Value> = elements
                        .iter()
                        .enumerate()
                        .map(|(i, elem)| func.call(&[elem.clone(), Value::Number(i as f64)]))
                        .collect();
                    create_array(mapped)
                } else {
                    create_array(elements)
                }
            }))
        }
        "filter" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let elements = collect_elements(&o);
                if let Value::NativeFunction(func) = &callback {
                    let filtered: Vec<Value> = elements
                        .iter()
                        .enumerate()
                        .filter(|(i, elem)| {
                            func.call(&[(*elem).clone(), Value::Number(*i as f64)])
                                .is_truthy()
                        })
                        .map(|(_, elem)| elem.clone())
                        .collect();
                    create_array(filtered)
                } else {
                    create_array(vec![])
                }
            }))
        }
        "find" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let elements = collect_elements(&o);
                if let Value::NativeFunction(func) = &callback {
                    for (i, elem) in elements.iter().enumerate() {
                        if func
                            .call(&[elem.clone(), Value::Number(i as f64)])
                            .is_truthy()
                        {
                            return elem.clone();
                        }
                    }
                }
                Value::Undefined
            }))
        }
        "reduce" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let initial = args.get(1).cloned();
                let elements = collect_elements(&o);
                if let Value::NativeFunction(func) = &callback {
                    let mut iter = elements.iter().enumerate();
                    let mut acc = if let Some(init) = initial {
                        init
                    } else if let Some((_, first)) = iter.next() {
                        first.clone()
                    } else {
                        return Value::Undefined;
                    };
                    for (i, elem) in iter {
                        acc = func.call(&[acc, elem.clone(), Value::Number(i as f64)]);
                    }
                    acc
                } else {
                    Value::Undefined
                }
            }))
        }
        "concat" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let o = arr.borrow();
                let mut result = collect_elements(&o);
                for arg in args {
                    if let Value::Object(other) = arg {
                        let other_o = other.borrow();
                        if is_array_like(&other_o) {
                            result.extend(collect_elements(&other_o));
                            continue;
                        }
                    }
                    result.push(arg.clone());
                }
                create_array(result)
            }))
        }
        "reverse" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |_args: &[Value]| {
                let mut o = arr.borrow_mut();
                let mut elements = collect_elements(&o);
                elements.reverse();
                set_elements(&mut o, &elements);
                Value::Object(Rc::clone(&arr))
            }))
        }
        "length" => return Some(Value::Number(array_length(&obj_rc.borrow()) as f64)),
        _ => None,
    };

    method.map(|f| Value::NativeFunction(Rc::new(NativeFunction::new(name, f))))
}

/// Simplified strict equality for array method use.
fn strict_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        _ => false,
    }
}
