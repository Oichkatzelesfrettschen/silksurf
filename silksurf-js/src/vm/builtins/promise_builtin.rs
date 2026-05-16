//! Promise constructor and static methods installed on global.

use std::cell::RefCell;
use std::rc::Rc;

use super::{make_object_with_methods, native_fn};
use crate::vm::promise::{self, MicrotaskQueue, Promise, PromiseState};
use crate::vm::value::{Object, PropertyKey, Value};

pub fn install(global: &mut Object) {
    let promise_constructor = make_object_with_methods(vec![
        ("resolve", native_fn("Promise.resolve", promise_resolve)),
        ("reject", native_fn("Promise.reject", promise_reject)),
        ("all", native_fn("Promise.all", promise_all)),
        ("race", native_fn("Promise.race", promise_race)),
        (
            "allSettled",
            native_fn("Promise.allSettled", promise_all_settled),
        ),
        ("any", native_fn("Promise.any", promise_any)),
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

/*
 * extract_iterable -- pull items out of an array-like first argument.
 *
 * WHY: Promise.all/race/allSettled/any all accept an iterable. In this
 * engine we only support array-like objects (have "length" + indexed
 * properties); generators/Set iterators are out of scope here. Returns
 * an empty Vec when the argument is not array-like, which matches the
 * "empty iterable" path of each combinator (per spec, that path yields
 * Promise.resolve([]) for all/allSettled, a forever-pending promise for
 * race, and immediate rejection for any).
 *
 * Centralising this keeps the four combinators short and prevents the
 * RefCell borrow window from straddling the microtask drain, which would
 * panic if any reaction needed mutable access to the same object.
 */
fn extract_iterable(args: &[Value]) -> Vec<Value> {
    let Some(Value::Object(obj)) = args.first() else {
        return Vec::new();
    };
    let o = obj.borrow();
    let len = match o.get_by_str("length") {
        Value::Number(n) if n >= 0.0 => n as usize,
        _ => 0,
    };
    let mut items = Vec::with_capacity(len);
    for i in 0..len {
        items.push(o.get_by_key(&PropertyKey::Index(i as u32)));
    }
    items
}

/*
 * settle_each -- drain microtasks, then peek the (state, value) of every
 * item. Non-Promise values count as already Fulfilled with themselves
 * (matches ECMAScript "Resolve(item)" treating non-thenables as values).
 *
 * Synchronous-settle limitation: this only observes states reachable by
 * draining the microtask queue once. A pending promise that requires VM
 * resumption to settle stays Pending; the caller is responsible for
 * deciding what to do in that case. For the supported workflows
 * (Promise.resolve / Promise.reject inputs) this is enough.
 */
fn settle_each(items: &[Value]) -> Vec<(PromiseState, Value)> {
    let mut queue = MicrotaskQueue::new();
    queue.drain();
    items
        .iter()
        .map(|item| {
            promise::as_settled_promise(item)
                .unwrap_or_else(|| (PromiseState::Fulfilled, item.clone()))
        })
        .collect()
}

/*
 * promise_all -- Promise.all([p1, p2, ...]) per ECMAScript 27.2.4.1.
 *
 * WHY: React render() / Suspense / parallel fetch() patterns all rely on
 * Promise.all to wait for a fan-out of promises. Returning the raw input
 * array (as the old stub did) broke any code that iterated the resolved
 * value expecting fulfillment values rather than Promise wrappers.
 *
 * Semantics:
 *   - All fulfill -> result fulfilled with an array of fulfillment values
 *     in input order.
 *   - Any reject  -> result rejected with the first rejection reason
 *     (input order; the spec calls this "first to reject" but with the
 *     synchronous-settle model "first" is well-defined as lowest index).
 *   - Empty input -> result fulfilled with [] immediately.
 *   - Any pending -> result stays pending; under the synchronous-settle
 *     model this means the test would observe a Pending wrapper. True
 *     async coordination requires resumable VM frames (out of scope).
 */
fn promise_all(args: &[Value]) -> Value {
    let result = Promise::new();
    let mut queue = MicrotaskQueue::new();
    let items = extract_iterable(args);

    if items.is_empty() {
        Promise::resolve(
            &result,
            crate::vm::builtins::array::create_array(&[]),
            &mut queue,
        );
        queue.drain();
        return promise::promise_to_value(&result);
    }

    let settled = settle_each(&items);

    // First scan for any rejection so we can fail fast (spec: first reject wins).
    if let Some((_, reason)) = settled
        .iter()
        .find(|(state, _)| *state == PromiseState::Rejected)
    {
        Promise::reject(&result, reason.clone(), &mut queue);
        queue.drain();
        return promise::promise_to_value(&result);
    }

    // If anything is still pending we cannot complete synchronously; leave the
    // result pending. Callers using only Promise.resolve/Promise.reject inputs
    // (the supported pattern) will never hit this branch.
    if settled
        .iter()
        .any(|(state, _)| *state == PromiseState::Pending)
    {
        return promise::promise_to_value(&result);
    }

    let values: Vec<Value> = settled.into_iter().map(|(_, value)| value).collect();
    let arr = crate::vm::builtins::array::create_array(&values);
    Promise::resolve(&result, arr, &mut queue);
    queue.drain();
    promise::promise_to_value(&result)
}

/*
 * promise_race -- Promise.race([p1, p2, ...]) per ECMAScript 27.2.4.5.
 *
 * WHY: Race is the cancellation/timeout primitive (Promise.race([work,
 * timeout])). The old stub returned the first input value verbatim,
 * which broke timeout patterns because the timeout Promise's eventual
 * rejection was never observed.
 *
 * Semantics: settle with the first settled input (fulfilled or rejected).
 * Non-Promise values count as already fulfilled with themselves, so they
 * take precedence over any later promise. Empty input leaves the result
 * pending forever, matching spec behaviour.
 *
 * Synchronous-settle note: "first to settle" reduces to "lowest index
 * that is not Pending after the microtask drain". For Promise.resolve /
 * Promise.reject inputs every item is settled, so element 0 wins.
 */
fn promise_race(args: &[Value]) -> Value {
    let result = Promise::new();
    let mut queue = MicrotaskQueue::new();
    let items = extract_iterable(args);

    if items.is_empty() {
        return promise::promise_to_value(&result);
    }

    let settled = settle_each(&items);
    for (state, value) in settled {
        match state {
            PromiseState::Fulfilled => {
                Promise::resolve(&result, value, &mut queue);
                queue.drain();
                return promise::promise_to_value(&result);
            }
            PromiseState::Rejected => {
                Promise::reject(&result, value, &mut queue);
                queue.drain();
                return promise::promise_to_value(&result);
            }
            PromiseState::Pending => {
                // Skip: cannot settle synchronously. Continue scanning so a
                // later already-settled item still wins the race.
            }
        }
    }
    // Nothing was settled -- leave the result pending.
    promise::promise_to_value(&result)
}

/*
 * make_settled_descriptor -- build the per-element object returned by
 * Promise.allSettled: { status: "fulfilled", value } or
 *                     { status: "rejected",  reason }.
 *
 * Centralised so the shape stays consistent and there is one place to
 * change if/when allSettled grows additional metadata.
 */
fn make_settled_descriptor(state: &PromiseState, value: Value) -> Value {
    let obj = Object::new();
    let obj_rc = Rc::new(RefCell::new(obj));
    {
        let mut o = obj_rc.borrow_mut();
        match state {
            PromiseState::Fulfilled => {
                o.set_by_str("status", Value::string_owned("fulfilled".to_string()));
                o.set_by_str("value", value);
            }
            PromiseState::Rejected => {
                o.set_by_str("status", Value::string_owned("rejected".to_string()));
                o.set_by_str("reason", value);
            }
            PromiseState::Pending => {
                // Should not occur: allSettled only descriptors settled items.
                // Defensive: tag as pending so callers can detect the bug.
                o.set_by_str("status", Value::string_owned("pending".to_string()));
            }
        }
    }
    Value::Object(obj_rc)
}

/*
 * promise_all_settled -- Promise.allSettled (ES2020, 27.2.4.2).
 *
 * WHY: Unlike Promise.all, allSettled never short-circuits on rejection.
 * It is the right tool when callers need to see the outcome of every
 * input (e.g. batch operations that should report partial success).
 *
 * Semantics: resolves with an array of descriptor objects, one per input,
 * in input order. Each descriptor is { status, value } or { status,
 * reason } -- see make_settled_descriptor. Empty input resolves with [].
 *
 * Synchronous-settle limitation matches promise_all: any input still
 * pending after a microtask drain leaves the overall result pending,
 * because we cannot describe an unsettled outcome and the spec forbids
 * resolving allSettled before every input has settled.
 */
fn promise_all_settled(args: &[Value]) -> Value {
    let result = Promise::new();
    let mut queue = MicrotaskQueue::new();
    let items = extract_iterable(args);

    if items.is_empty() {
        Promise::resolve(
            &result,
            crate::vm::builtins::array::create_array(&[]),
            &mut queue,
        );
        queue.drain();
        return promise::promise_to_value(&result);
    }

    let settled = settle_each(&items);
    if settled
        .iter()
        .any(|(state, _)| *state == PromiseState::Pending)
    {
        // Cannot describe pending items; leave the result pending.
        return promise::promise_to_value(&result);
    }

    let descriptors: Vec<Value> = settled
        .iter()
        .map(|(state, value)| make_settled_descriptor(state, value.clone()))
        .collect();
    let arr = crate::vm::builtins::array::create_array(&descriptors);
    Promise::resolve(&result, arr, &mut queue);
    queue.drain();
    promise::promise_to_value(&result)
}

/*
 * promise_any -- Promise.any (ES2021, 27.2.4.3).
 *
 * WHY: "First fulfilled wins; if everyone rejects, fail." Useful for
 * redundant data sources (try mirrors in parallel, use whichever returns
 * first). The proper rejection value is an AggregateError carrying the
 * rejection reasons; this engine has no AggregateError prototype, so we
 * reject with a sentinel string and document the deviation. Upgrading to
 * a real AggregateError-shaped object is a follow-up once Error class
 * subclassing is wired into the constructor table.
 *
 * Semantics:
 *   - Any fulfilled -> result fulfilled with the first fulfillment value.
 *   - All rejected  -> result rejected with the sentinel string
 *                      "All promises were rejected".
 *   - Empty input   -> rejected immediately (matches spec: AggregateError
 *                      with empty errors list).
 *   - Any pending   -> see Promise.all note; leave result pending if we
 *                      have not seen a fulfilment yet.
 */
fn promise_any(args: &[Value]) -> Value {
    let result = Promise::new();
    let mut queue = MicrotaskQueue::new();
    let items = extract_iterable(args);

    if items.is_empty() {
        Promise::reject(
            &result,
            Value::string_owned("All promises were rejected".to_string()),
            &mut queue,
        );
        queue.drain();
        return promise::promise_to_value(&result);
    }

    let settled = settle_each(&items);

    // First fulfilled wins.
    if let Some((_, value)) = settled
        .iter()
        .find(|(state, _)| *state == PromiseState::Fulfilled)
    {
        Promise::resolve(&result, value.clone(), &mut queue);
        queue.drain();
        return promise::promise_to_value(&result);
    }

    // If anything is still pending we cannot conclude "all rejected" yet.
    if settled
        .iter()
        .any(|(state, _)| *state == PromiseState::Pending)
    {
        return promise::promise_to_value(&result);
    }

    // Every input rejected.
    Promise::reject(
        &result,
        Value::string_owned("All promises were rejected".to_string()),
        &mut queue,
    );
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
