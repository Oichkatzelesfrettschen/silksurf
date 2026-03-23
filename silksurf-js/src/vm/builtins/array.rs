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

/// Collect array elements into a Vec<Value> (pub for use from vm/mod.rs static dispatch).
pub fn collect_elements_pub(obj: &Object) -> Vec<Value> {
    collect_elements(obj)
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
        "some" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                let elements = collect_elements(&arr.borrow());
                for (i, el) in elements.iter().enumerate() {
                    let result = call_value(&cb, &[el.clone(), Value::Number(i as f64)]);
                    if result.is_truthy() {
                        return Value::Boolean(true);
                    }
                }
                Value::Boolean(false)
            }))
        }
        "every" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                let elements = collect_elements(&arr.borrow());
                for (i, el) in elements.iter().enumerate() {
                    let result = call_value(&cb, &[el.clone(), Value::Number(i as f64)]);
                    if !result.is_truthy() {
                        return Value::Boolean(false);
                    }
                }
                Value::Boolean(true)
            }))
        }
        "findIndex" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                let elements = collect_elements(&arr.borrow());
                for (i, el) in elements.iter().enumerate() {
                    let result = call_value(&cb, &[el.clone(), Value::Number(i as f64)]);
                    if result.is_truthy() {
                        return Value::Number(i as f64);
                    }
                }
                Value::Number(-1.0)
            }))
        }
        "at" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let idx = args.first().map_or(0, |v| v.to_number() as i64);
                let elements = collect_elements(&arr.borrow());
                let len = elements.len() as i64;
                let real = if idx < 0 { len + idx } else { idx };
                if real < 0 || real >= len {
                    Value::Undefined
                } else {
                    elements[real as usize].clone()
                }
            }))
        }
        "flat" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let depth = args.first().map_or(1, |v| v.to_number() as u32);
                let elements = collect_elements(&arr.borrow());
                let mut result = Vec::new();
                flatten_array(&elements, depth, &mut result);
                create_array(result)
            }))
        }
        "flatMap" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let cb = args.first().cloned().unwrap_or(Value::Undefined);
                let elements = collect_elements(&arr.borrow());
                let mut result = Vec::new();
                for (i, el) in elements.iter().enumerate() {
                    let mapped = call_value(&cb, &[el.clone(), Value::Number(i as f64)]);
                    if let Value::Object(ref o) = mapped {
                        let o_borrow = o.borrow();
                        if is_array_like(&o_borrow) {
                            result.extend(collect_elements(&o_borrow));
                            continue;
                        }
                    }
                    result.push(mapped);
                }
                create_array(result)
            }))
        }
        "sort" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let cb = args.first().cloned();
                let mut elements = collect_elements(&arr.borrow_mut());
                if let Some(cb_val) = cb {
                    // Simplified sort with comparator -- may not be perfectly stable
                    elements.sort_by(|a, b| {
                        let r = call_value(&cb_val, &[a.clone(), b.clone()]);
                        let n = r.to_number();
                        if n < 0.0 { std::cmp::Ordering::Less }
                        else if n > 0.0 { std::cmp::Ordering::Greater }
                        else { std::cmp::Ordering::Equal }
                    });
                } else {
                    // Default: sort by string representation
                    elements.sort_by(|a, b| {
                        let sa = a.to_js_string();
                        let sb = b.to_js_string();
                        sa.as_str().unwrap_or("").cmp(sb.as_str().unwrap_or(""))
                    });
                }
                let mut o = arr.borrow_mut();
                set_elements(&mut o, &elements);
                Value::Object(Rc::clone(&arr))
            }))
        }
        "fill" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let value = args.first().cloned().unwrap_or(Value::Undefined);
                let mut o = arr.borrow_mut();
                let len = array_length(&o);
                let start = args.get(1).map_or(0, |v| {
                    let n = v.to_number() as i64;
                    if n < 0 { (len as i64 + n).max(0) as usize } else { (n as usize).min(len) }
                });
                let end = args.get(2).map_or(len, |v| {
                    let n = v.to_number() as i64;
                    if n < 0 { (len as i64 + n).max(0) as usize } else { (n as usize).min(len) }
                });
                for i in start..end {
                    o.set_by_key(crate::vm::value::PropertyKey::Index(i as u32), value.clone());
                }
                Value::Object(Rc::clone(&arr))
            }))
        }
        "splice" => {
            let arr = Rc::clone(obj_rc);
            Some(Box::new(move |args: &[Value]| {
                let mut elements = collect_elements(&arr.borrow());
                let len = elements.len() as i64;
                let start = args.first().map_or(0, |v| {
                    let n = v.to_number() as i64;
                    if n < 0 { (len + n).max(0) as usize } else { (n as usize).min(len as usize) }
                });
                let delete_count = args.get(1).map_or(len as usize - start, |v| {
                    (v.to_number() as usize).min(elements.len() - start)
                });
                let removed: Vec<Value> = elements.drain(start..start + delete_count).collect();
                let new_items: Vec<Value> = args.iter().skip(2).cloned().collect();
                for (i, item) in new_items.into_iter().enumerate() {
                    elements.insert(start + i, item);
                }
                let mut o = arr.borrow_mut();
                set_elements(&mut o, &elements);
                create_array(removed)
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

/*
 * call_value -- invoke a Value as a function with the given arguments.
 *
 * WHY: Array higher-order methods (some, every, findIndex, flatMap, sort)
 * receive a callback Value that may be a NativeFunction. Rather than
 * matching on Value::NativeFunction at each call site, this helper
 * centralises dispatch. Returns Undefined if the value is not callable.
 *
 * See: NativeFunction::call (value.rs) for the underlying dispatch.
 */
fn call_value(func: &Value, args: &[Value]) -> Value {
    match func {
        Value::NativeFunction(f) => f.call(args),
        _ => Value::Undefined,
    }
}

/*
 * flatten_array -- recursively flatten array elements up to `depth` levels.
 *
 * WHY: Array.prototype.flat() requires recursive descent into nested arrays.
 * Each level of nesting checks is_array_like; if true and depth > 0, recurse;
 * otherwise push the value as-is. This mirrors the ECMAScript FlattenIntoArray
 * abstract operation (ECMA-262 Section 23.1.3.13).
 *
 * depth=0: no flattening (copy elements verbatim)
 * depth=1: flatten one level (default for Array.prototype.flat())
 * depth=Infinity: represented as u32::MAX
 */
fn flatten_array(elements: &[Value], depth: u32, out: &mut Vec<Value>) {
    for el in elements {
        if depth > 0 {
            if let Value::Object(o) = el {
                let o_borrow = o.borrow();
                if is_array_like(&o_borrow) {
                    let inner = collect_elements(&o_borrow);
                    drop(o_borrow);
                    flatten_array(&inner, depth - 1, out);
                    continue;
                }
            }
        }
        out.push(el.clone());
    }
}
