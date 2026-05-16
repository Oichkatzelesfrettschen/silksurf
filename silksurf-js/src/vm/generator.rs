/*
 * generator.rs -- ES6 generator function support (eager-evaluation strategy).
 *
 * WHY: ECMA-262 generator functions (`function*`) return an iterator object
 * whose `.next()` calls progressively resume the body until the next `yield`.
 * Proper spec-conformant suspension requires either (a) coroutine/fiber
 * support (stackful) or (b) full CPS / state-machine transformation of the
 * body (stackless).  Neither fits the current synchronous, single-threaded
 * register VM without a multi-week rewrite of the dispatch loop.
 *
 * WHAT: We implement the EAGER STRATEGY:
 *   1. Calling a generator function runs the WHOLE body to completion in a
 *      sub-execution context, collecting every `yield`ed value into a Vec.
 *   2. The returned Generator object is a simple iterator that pops from
 *      that Vec on each `.next()` call.
 *   3. The body's final return value (or `undefined`) becomes the final
 *      iterator step's `value` with `done: true`.
 *
 * HOW: A buffer stack is parked on the Vm (`generator_yield_stack`) so that
 * `op_yield` knows where to append.  When `op_call` dispatches a function
 * whose chunk is flagged `is_generator`, it allocates a fresh buffer, pushes
 * the existing one (re-entrant generator construction), runs the body in a
 * saved sub-context, pops the buffer back, and builds the iterator object.
 *
 * KNOWN LIMITATIONS (documented, deliberate):
 *   - Side effects in the body happen at construction time, not lazily per
 *     `.next()` call.  Most user code (for...of, `Array.from`, spread) is
 *     unaffected.  Code that relies on `yield` pausing observable side
 *     effects (e.g. an infinite generator, or one whose later iterations
 *     depend on state mutated between calls to .next() by the caller) will
 *     misbehave.
 *   - The argument to `.next(value)` is NOT plumbed back into the yielded
 *     expression (it always evaluates to `undefined` inside the body).
 *   - `yield*` (delegation) flattens an iterable's elements into the parent
 *     buffer eagerly; nested generators still respect eager semantics.
 *   - `.return()` and `.throw()` on the iterator only update the done flag;
 *     they do not retroactively re-run the body.
 *
 * These limitations are acceptable for the current test surface (for...of
 * over finite generators) and the failure mode is observable rather than UB.
 *
 * See: vm/mod.rs op_yield, op_yield_star, op_new_generator, op_call for the
 *      dispatch sites that read/write the generator state.
 */

use std::cell::RefCell;
use std::rc::Rc;

use super::value::{NativeFunction, Object, Value};
use super::{builtins, value};

/*
 * GeneratorBuffer -- shared yield-value sink for the currently-executing
 * generator body.
 *
 * WHY: `op_yield` runs deep inside `vm.execute()` and needs a way to deliver
 * the yielded value back to the enclosing `op_call`-style generator
 * constructor.  Rather than threading a return channel through the dispatch
 * loop, we park an `Rc<RefCell<Vec<Value>>>` on the Vm.  The constructor
 * pushes a fresh buffer before running the body and pops it afterwards; the
 * stack of saved buffers supports nested generator construction.
 */
pub(super) type GeneratorBuffer = Rc<RefCell<Vec<Value>>>;

/*
 * build_generator -- wrap a collected yield buffer + return value into a
 * JavaScript iterator object.
 *
 * The returned Value::Object exposes:
 *   - `next()`    -> { value, done } per ES iterator protocol
 *   - `return(v)` -> marks done immediately with v as value
 *   - `throw(e)`  -> marks done; surfaces e as the value (no rethrow in
 *                    eager mode since the body already ran)
 *   - `__done__`  (internal flag, read by op_iter_done for fast checks)
 *
 * On exhaustion the iterator produces `{ value: <return_value>, done: true }`
 * exactly once and `{ value: undefined, done: true }` thereafter, matching
 * the spec's "completion record" propagation for the final step.
 */
pub(super) fn build_generator(yielded: Vec<Value>, return_value: Value) -> Value {
    let elements = Rc::new(RefCell::new(yielded));
    let index = Rc::new(RefCell::new(0usize));
    let finished = Rc::new(RefCell::new(false));
    let final_value = Rc::new(RefCell::new(Some(return_value)));

    let iter_obj = Rc::new(RefCell::new(Object::new()));

    // next() closure -- captures the four Rc cells so each invocation steps
    // through the same buffer.  The closure is registered as a NativeFunction
    // so op_iter_next (and direct `.next()` calls in user code) dispatch
    // identically.
    {
        let elements_ref = Rc::clone(&elements);
        let index_ref = Rc::clone(&index);
        let finished_ref = Rc::clone(&finished);
        let final_value_ref = Rc::clone(&final_value);
        let next_fn =
            Value::NativeFunction(Rc::new(NativeFunction::new("__gen_next__", move |_args| {
                let result = Rc::new(RefCell::new(Object::new()));
                if *finished_ref.borrow() {
                    result.borrow_mut().set_by_str("value", Value::Undefined);
                    result.borrow_mut().set_by_str("done", Value::Boolean(true));
                    return Value::Object(result);
                }
                let current = *index_ref.borrow();
                let total = elements_ref.borrow().len();
                if current < total {
                    let value = elements_ref.borrow()[current].clone();
                    *index_ref.borrow_mut() = current + 1;
                    result.borrow_mut().set_by_str("value", value);
                    result
                        .borrow_mut()
                        .set_by_str("done", Value::Boolean(false));
                } else {
                    // Buffer exhausted -- deliver the final return value once
                    // with done=true, then transition to permanent done.
                    *finished_ref.borrow_mut() = true;
                    let final_v = final_value_ref
                        .borrow_mut()
                        .take()
                        .unwrap_or(Value::Undefined);
                    result.borrow_mut().set_by_str("value", final_v);
                    result.borrow_mut().set_by_str("done", Value::Boolean(true));
                }
                Value::Object(result)
            })));
        iter_obj.borrow_mut().set_by_str("next", next_fn);
    }

    // return(v) closure -- early-termination per iterator protocol.
    {
        let finished_ref = Rc::clone(&finished);
        let return_fn = Value::NativeFunction(Rc::new(NativeFunction::new(
            "__gen_return__",
            move |args| {
                *finished_ref.borrow_mut() = true;
                let v = args.first().cloned().unwrap_or(Value::Undefined);
                let result = Rc::new(RefCell::new(Object::new()));
                result.borrow_mut().set_by_str("value", v);
                result.borrow_mut().set_by_str("done", Value::Boolean(true));
                Value::Object(result)
            },
        )));
        iter_obj.borrow_mut().set_by_str("return", return_fn);
    }

    // throw(e) closure -- mirrors return() but surfaces the argument as the
    // value.  Eager mode cannot truly resume-and-throw the body, so we
    // report done immediately with the supplied reason.  Documented
    // limitation; see module header.
    {
        let finished_ref = Rc::clone(&finished);
        let throw_fn =
            Value::NativeFunction(Rc::new(NativeFunction::new("__gen_throw__", move |args| {
                *finished_ref.borrow_mut() = true;
                let v = args.first().cloned().unwrap_or(Value::Undefined);
                let result = Rc::new(RefCell::new(Object::new()));
                result.borrow_mut().set_by_str("value", v);
                result.borrow_mut().set_by_str("done", Value::Boolean(true));
                Value::Object(result)
            })));
        iter_obj.borrow_mut().set_by_str("throw", throw_fn);
    }

    // __done__ flag is read by op_iter_done as a fast path; we keep it as
    // Value::Boolean(false) initially.  op_iter_done already falls back to
    // reading the done flag from the {value, done} record returned by
    // op_iter_next so we don't need to keep this in sync per step.
    iter_obj
        .borrow_mut()
        .set_by_str("__done__", Value::Boolean(false));

    Value::Object(iter_obj)
}

/*
 * yield_star_flatten -- implement `yield* iterable` by draining the iterable
 * synchronously into the supplied buffer.
 *
 * WHY: `yield* x` is spec-defined to yield every value produced by x's
 * iterator one at a time.  In eager mode we expand the iterable in-place so
 * the parent generator's buffer ends up with the same final sequence a
 * spec-compliant implementation would have produced over many .next() calls.
 *
 * Supports: arrays / array-likes (via collect_elements_pub), strings (char
 * iteration), and objects exposing a `next` NativeFunction (which covers
 * generators built by build_generator).  Unknown values are silently
 * ignored to avoid throwing in the hot path; the compiler still emits a
 * runtime check in stricter mode if needed.
 *
 * The drain loop is bounded at 2^20 steps so a buggy iterator cannot lock
 * the VM.  Real-world iterables terminate well below that.
 */
pub(super) fn yield_star_flatten(buffer: &GeneratorBuffer, iterable: &Value) {
    match iterable {
        Value::Object(o) => {
            // Detect generator/iterator-like: has a `next` NativeFunction.
            let next_fn = o.borrow().get_by_str("next");
            if let Value::NativeFunction(next_native) = next_fn {
                let mut steps = 0usize;
                loop {
                    if steps >= 1 << 20 {
                        break;
                    }
                    steps += 1;
                    let step = next_native.call(&[]);
                    let (val, done) = if let Value::Object(record) = &step {
                        let v = record.borrow().get_by_str("value");
                        let d = matches!(record.borrow().get_by_str("done"), Value::Boolean(true));
                        (v, d)
                    } else {
                        (Value::Undefined, true)
                    };
                    if done {
                        break;
                    }
                    buffer.borrow_mut().push(val);
                }
                return;
            }
            // Fall back to array-like flattening.
            let o_borrow = o.borrow();
            if builtins::array::is_array_like(&o_borrow) {
                for el in builtins::array::collect_elements_pub(&o_borrow) {
                    buffer.borrow_mut().push(el);
                }
            }
        }
        Value::String(s) => {
            if let Some(text) = s.as_str() {
                for ch in text.chars() {
                    buffer
                        .borrow_mut()
                        .push(value::Value::string_owned(ch.to_string()));
                }
            }
        }
        _ => {
            // Unknown iterable: silently ignore (eager mode can't throw a
            // spec-correct TypeError without disrupting the buffer).
        }
    }
}
