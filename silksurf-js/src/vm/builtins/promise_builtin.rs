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
    // Promise.withResolvers() -- ES2024. Returns {promise, resolve, reject}.
    // WHY: Modern JS (React 19, streaming APIs) uses withResolvers() to
    // create a Promise and store its settle callbacks for later use,
    // without wrapping everything in a `new Promise(executor)` constructor.
    if let Value::Object(ctor) = &promise_constructor {
        ctor.borrow_mut().set_by_str(
            "withResolvers",
            native_fn("Promise.withResolvers", promise_with_resolvers),
        );
    }
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
        Promise::resolve(
            &result,
            Value::Object(Rc::new(RefCell::new(Object::new()))),
            &mut queue,
        );
        queue.drain();
        return promise::promise_to_value(&result);
    }

    // For now, resolve immediately with the array of values
    // (Real impl would wait for all promises to settle)
    let arr = crate::vm::builtins::array::create_array(&items);
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

/*
 * promise_with_resolvers -- Promise.withResolvers() (ES2024).
 *
 * WHY: Modern libraries (React 19 streaming, web APIs) need to create a
 * Promise and store its resolve/reject callbacks for deferred settlement.
 * The traditional `new Promise(executor)` pattern requires the settlement
 * logic to be inline; withResolvers() externalizes it.
 *
 * Returns: { promise: Promise, resolve: Function, reject: Function }
 *
 * Implementation: Promise is created pending. resolve/reject closures
 * each capture a clone of the Rc<RefCell<Promise>> so they can settle
 * it from outside. Each creates a local MicrotaskQueue and drains it
 * immediately (same pattern as Promise.resolve/Promise.reject builtins).
 *
 * See: promise_resolve/promise_reject for the drain pattern
 * See: promise.rs Promise::resolve/reject for settlement mechanics
 */
fn promise_with_resolvers(_args: &[Value]) -> Value {
    use crate::vm::value::NativeFunction;

    let p = Promise::new();

    let p_resolve = Rc::clone(&p);
    let resolve_fn = Value::NativeFunction(Rc::new(NativeFunction::new(
        "resolve",
        move |args: &[Value]| {
            let value = args.first().cloned().unwrap_or(Value::Undefined);
            let mut queue = MicrotaskQueue::new();
            Promise::resolve(&p_resolve, value, &mut queue);
            queue.drain();
            Value::Undefined
        },
    )));

    let p_reject = Rc::clone(&p);
    let reject_fn = Value::NativeFunction(Rc::new(NativeFunction::new(
        "reject",
        move |args: &[Value]| {
            let reason = args.first().cloned().unwrap_or(Value::Undefined);
            let mut queue = MicrotaskQueue::new();
            Promise::reject(&p_reject, reason, &mut queue);
            queue.drain();
            Value::Undefined
        },
    )));

    let result = Object::new();
    let result_rc = Rc::new(RefCell::new(result));
    {
        let mut r = result_rc.borrow_mut();
        r.set_by_str("promise", promise::promise_to_value(&p));
        r.set_by_str("resolve", resolve_fn);
        r.set_by_str("reject", reject_fn);
    }
    Value::Object(result_rc)
}
