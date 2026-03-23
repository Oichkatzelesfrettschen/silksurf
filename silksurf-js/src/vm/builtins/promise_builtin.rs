//! Promise constructor and static methods installed on global.

use std::cell::RefCell;
use std::rc::Rc;

use super::{make_object_with_methods, native_fn};
use crate::vm::promise::{self, MicrotaskQueue, Promise};
use crate::vm::value::{Object, PropertyKey, Value};

pub fn install(global: &mut Object) {
    let promise_constructor = make_object_with_methods(vec![
        ("resolve", native_fn("Promise.resolve", promise_resolve)),
        ("reject", native_fn("Promise.reject", promise_reject)),
        ("all", native_fn("Promise.all", promise_all)),
        ("race", native_fn("Promise.race", promise_race)),
    ]);
    global.set_by_str("Promise", promise_constructor);
}

fn promise_resolve(args: &[Value]) -> Value {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let p = Promise::new();
    let mut queue = MicrotaskQueue::new();
    Promise::resolve(&p, value, &mut queue);
    queue.drain();
    promise::promise_to_value(&p)
}

fn promise_reject(args: &[Value]) -> Value {
    let reason = args.first().cloned().unwrap_or(Value::Undefined);
    let p = Promise::new();
    let mut queue = MicrotaskQueue::new();
    Promise::reject(&p, reason, &mut queue);
    queue.drain();
    promise::promise_to_value(&p)
}

fn promise_all(args: &[Value]) -> Value {
    // Simplified: expects an array-like object of promise values
    let result = Promise::new();
    let mut queue = MicrotaskQueue::new();

    let items = match args.first() {
        Some(Value::Object(obj)) => {
            let o = obj.borrow();
            let len = match o.get_by_str("length") {
                Value::Number(n) => n as usize,
                _ => 0,
            };
            let mut items = Vec::with_capacity(len);
            for i in 0..len {
                items.push(o.get_by_key(&PropertyKey::Index(i as u32)));
            }
            items
        }
        _ => vec![],
    };

    if items.is_empty() {
        Promise::resolve(&result, Value::Object(Rc::new(RefCell::new(Object::new()))), &mut queue);
        queue.drain();
        return promise::promise_to_value(&result);
    }

    // For now, resolve immediately with the array of values
    // (Real impl would wait for all promises to settle)
    let arr = crate::vm::builtins::array::create_array(items);
    Promise::resolve(&result, arr, &mut queue);
    queue.drain();
    promise::promise_to_value(&result)
}

fn promise_race(args: &[Value]) -> Value {
    // Simplified: resolve with first element
    let result = Promise::new();
    let mut queue = MicrotaskQueue::new();

    let first = match args.first() {
        Some(Value::Object(obj)) => {
            let o = obj.borrow();
            o.get_by_key(&PropertyKey::Index(0))
        }
        _ => Value::Undefined,
    };

    Promise::resolve(&result, first, &mut queue);
    queue.drain();
    promise::promise_to_value(&result)
}
