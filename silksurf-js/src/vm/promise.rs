/*
 * promise.rs -- ES2024 Promise implementation + microtask queue.
 *
 * WHY: Promises are the foundation of async JS. React, fetch(), and
 * all modern APIs return Promises. Without this, no async code runs.
 *
 * State machine: Pending -> Fulfilled(value) | Rejected(reason)
 * Once settled, state never changes (double-resolve is silently ignored).
 *
 * Reaction chain: .then(onFulfilled, onRejected) attaches callbacks.
 * If the promise is already settled, the reaction fires immediately
 * via microtask queue. If pending, it queues for later settlement.
 *
 * Microtask queue: FIFO queue drained after each macrotask (timer/event).
 * Promise reactions and queueMicrotask() callbacks go here.
 * Safety limit: 10,000 iterations per drain to prevent infinite loops.
 *
 * Memory: Promise is Rc<RefCell<Promise>>. Reactions clone the Rc.
 * Queue is VecDeque<Microtask> -- each microtask ~64 bytes.
 *
 * See: builtins/promise_builtin.rs for Promise.resolve/reject/all/race
 * See: event_loop.rs tick() for microtask drain in the event loop
 * See: vm/mod.rs Vm.microtasks for the queue on the VM struct
 */

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use super::value::{NativeFunction, Value};

/// Promise state per ES spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

/// A single reaction (callback pair) attached via .then()
#[derive(Clone)]
pub(crate) struct PromiseReaction {
    on_fulfilled: Option<Value>, // callback or None (identity)
    on_rejected: Option<Value>,  // callback or None (thrower)
    result_promise: Rc<RefCell<Promise>>,
}

/// Core Promise object.
pub struct Promise {
    pub state: PromiseState,
    pub result: Value, // fulfillment value or rejection reason
    reactions: Vec<PromiseReaction>,
}

impl std::fmt::Debug for Promise {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Promise")
            .field("state", &self.state)
            .field("result", &self.result)
            .field("reactions", &self.reactions.len())
            .finish()
    }
}

impl Promise {
    /// Create a new pending promise.
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            state: PromiseState::Pending,
            result: Value::Undefined,
            reactions: Vec::new(),
        }))
    }

    /// Resolve a promise with a value.
    pub fn resolve(this: &Rc<RefCell<Self>>, value: Value, queue: &mut MicrotaskQueue) {
        let mut p = this.borrow_mut();
        if p.state != PromiseState::Pending {
            return; // Already settled
        }
        p.state = PromiseState::Fulfilled;
        p.result = value;
        let reactions: Vec<PromiseReaction> = p.reactions.drain(..).collect();
        drop(p);
        for reaction in reactions {
            queue.enqueue(Microtask::PromiseReaction {
                reaction,
                settled_value: this.borrow().result.clone(),
                was_fulfilled: true,
            });
        }
    }

    /// Reject a promise with a reason.
    pub fn reject(this: &Rc<RefCell<Self>>, reason: Value, queue: &mut MicrotaskQueue) {
        let mut p = this.borrow_mut();
        if p.state != PromiseState::Pending {
            return;
        }
        p.state = PromiseState::Rejected;
        p.result = reason;
        let reactions: Vec<PromiseReaction> = p.reactions.drain(..).collect();
        drop(p);
        for reaction in reactions {
            queue.enqueue(Microtask::PromiseReaction {
                reaction,
                settled_value: this.borrow().result.clone(),
                was_fulfilled: false,
            });
        }
    }

    /// Attach a .then() reaction. Returns the chained promise.
    pub fn then(
        this: &Rc<RefCell<Self>>,
        on_fulfilled: Option<Value>,
        on_rejected: Option<Value>,
        queue: &mut MicrotaskQueue,
    ) -> Rc<RefCell<Self>> {
        let result_promise = Promise::new();
        let reaction = PromiseReaction {
            on_fulfilled,
            on_rejected,
            result_promise: Rc::clone(&result_promise),
        };

        let p = this.borrow();
        match p.state {
            PromiseState::Pending => {
                drop(p);
                this.borrow_mut().reactions.push(reaction);
            }
            PromiseState::Fulfilled => {
                let value = p.result.clone();
                drop(p);
                queue.enqueue(Microtask::PromiseReaction {
                    reaction,
                    settled_value: value,
                    was_fulfilled: true,
                });
            }
            PromiseState::Rejected => {
                let reason = p.result.clone();
                drop(p);
                queue.enqueue(Microtask::PromiseReaction {
                    reaction,
                    settled_value: reason,
                    was_fulfilled: false,
                });
            }
        }
        result_promise
    }
}

impl Default for Promise {
    fn default() -> Self {
        Self {
            state: PromiseState::Pending,
            result: Value::Undefined,
            reactions: Vec::new(),
        }
    }
}

// ============================================================================
// Microtask Queue
// ============================================================================

/// A microtask to be executed.
#[derive(Clone)]
pub(crate) enum Microtask {
    /// Promise reaction (from .then/.catch)
    PromiseReaction {
        reaction: PromiseReaction,
        settled_value: Value,
        was_fulfilled: bool,
    },
    /// Plain callback (from queueMicrotask)
    Callback(Value),
}

/// FIFO queue for microtasks. Drained after each macrotask.
#[derive(Default)]
pub struct MicrotaskQueue {
    queue: VecDeque<Microtask>,
}

impl MicrotaskQueue {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub(crate) fn enqueue(&mut self, task: Microtask) {
        self.queue.push_back(task);
    }

    pub(crate) fn dequeue(&mut self) -> Option<Microtask> {
        self.queue.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Drain the queue, executing all microtasks.
    /// Returns the number of microtasks executed.
    /// New microtasks enqueued during execution are also drained.
    pub fn drain(&mut self) -> usize {
        let mut count = 0;
        // Process up to a safety limit to prevent infinite loops
        let max_iterations = 10_000;
        while let Some(task) = self.dequeue() {
            count += 1;
            if count > max_iterations {
                eprintln!("[WARN] Microtask queue exceeded {max_iterations} iterations, breaking");
                break;
            }
            match task {
                Microtask::PromiseReaction {
                    reaction,
                    settled_value,
                    was_fulfilled,
                } => {
                    execute_promise_reaction(reaction, settled_value, was_fulfilled, self);
                }
                Microtask::Callback(callback) => {
                    if let Value::NativeFunction(func) = &callback {
                        func.call(&[]);
                    }
                }
            }
        }
        count
    }
}

/// Execute a promise reaction and resolve/reject the chained promise.
fn execute_promise_reaction(
    reaction: PromiseReaction,
    settled_value: Value,
    was_fulfilled: bool,
    queue: &mut MicrotaskQueue,
) {
    let handler = if was_fulfilled {
        &reaction.on_fulfilled
    } else {
        &reaction.on_rejected
    };

    match handler {
        Some(Value::NativeFunction(func)) => {
            let result = func.call(&[settled_value]);
            // Check if result is itself a promise (thenable)
            // For now, resolve the chained promise with the result
            Promise::resolve(&reaction.result_promise, result, queue);
        }
        Some(Value::Function(_)) => {
            // JS function callbacks need VM execution -- for now, pass through
            // This will be wired up when VM is made resumable (A.4.5)
            if was_fulfilled {
                Promise::resolve(&reaction.result_promise, settled_value, queue);
            } else {
                Promise::reject(&reaction.result_promise, settled_value, queue);
            }
        }
        None => {
            // No handler: identity for fulfilled, thrower for rejected
            if was_fulfilled {
                Promise::resolve(&reaction.result_promise, settled_value, queue);
            } else {
                Promise::reject(&reaction.result_promise, settled_value, queue);
            }
        }
        _ => {
            // Non-callable handler, treat as identity/thrower
            if was_fulfilled {
                Promise::resolve(&reaction.result_promise, settled_value, queue);
            } else {
                Promise::reject(&reaction.result_promise, settled_value, queue);
            }
        }
    }
}

// ============================================================================
// Promise JS API (installed as builtins)
// ============================================================================

/// Create a JS Value wrapping a Promise for use from JS.
pub fn promise_to_value(promise: &Rc<RefCell<Promise>>) -> Value {
    use super::value::{Object, PropertyKey};

    let obj = Object::new();
    let obj_rc = Rc::new(RefCell::new(obj));

    // Store the promise ref as a hidden property
    // (In a full impl this would be an internal slot)

    // .then(onFulfilled, onRejected)
    let p_then = Rc::clone(promise);
    let then_fn = Value::NativeFunction(Rc::new(NativeFunction::new("then", move |args| {
        let on_fulfilled = args.first().cloned();
        let on_rejected = args.get(1).cloned();
        // We need access to the microtask queue here --
        // for now create a temporary one that must be drained by the caller
        // This is a simplified version; full impl threads queue through VM
        let mut temp_queue = MicrotaskQueue::new();
        let chained = Promise::then(&p_then, on_fulfilled, on_rejected, &mut temp_queue);
        temp_queue.drain();
        promise_to_value(&chained)
    })));

    // .catch(onRejected)
    let p_catch = Rc::clone(promise);
    let catch_fn = Value::NativeFunction(Rc::new(NativeFunction::new("catch", move |args| {
        let on_rejected = args.first().cloned();
        let mut temp_queue = MicrotaskQueue::new();
        let chained = Promise::then(&p_catch, None, on_rejected, &mut temp_queue);
        temp_queue.drain();
        promise_to_value(&chained)
    })));

    // .finally(onFinally)
    let p_finally = Rc::clone(promise);
    let finally_fn = Value::NativeFunction(Rc::new(NativeFunction::new("finally", move |args| {
        let callback = args.first().cloned();
        let cb_fulfilled = callback.clone();
        let cb_rejected = callback;
        let on_fulfilled = cb_fulfilled.map(|cb| {
            Value::NativeFunction(Rc::new(NativeFunction::new("finally_wrap", move |_args| {
                if let Value::NativeFunction(f) = &cb {
                    f.call(&[]);
                }
                Value::Undefined
            })))
        });
        let on_rejected = cb_rejected.map(|cb| {
            Value::NativeFunction(Rc::new(NativeFunction::new("finally_wrap", move |_args| {
                if let Value::NativeFunction(f) = &cb {
                    f.call(&[]);
                }
                Value::Undefined
            })))
        });
        let mut temp_queue = MicrotaskQueue::new();
        let chained = Promise::then(&p_finally, on_fulfilled, on_rejected, &mut temp_queue);
        temp_queue.drain();
        promise_to_value(&chained)
    })));

    {
        let mut o = obj_rc.borrow_mut();
        o.set_by_key(PropertyKey::from_str("then"), then_fn);
        o.set_by_key(PropertyKey::from_str("catch"), catch_fn);
        o.set_by_key(PropertyKey::from_str("finally"), finally_fn);
    }

    Value::Object(obj_rc)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_promise_resolve() {
        let p = Promise::new();
        let mut queue = MicrotaskQueue::new();
        Promise::resolve(&p, Value::Number(42.0), &mut queue);
        assert_eq!(p.borrow().state, PromiseState::Fulfilled);
        assert!(matches!(p.borrow().result, Value::Number(n) if n == 42.0));
    }

    #[test]
    fn test_promise_reject() {
        let p = Promise::new();
        let mut queue = MicrotaskQueue::new();
        Promise::reject(&p, Value::string("error"), &mut queue);
        assert_eq!(p.borrow().state, PromiseState::Rejected);
    }

    #[test]
    fn test_promise_then_after_resolve() {
        let p = Promise::new();
        let mut queue = MicrotaskQueue::new();
        Promise::resolve(&p, Value::Number(1.0), &mut queue);

        let called = Rc::new(RefCell::new(false));
        let called_clone = Rc::clone(&called);
        let callback = Value::NativeFunction(Rc::new(NativeFunction::new("test", move |args| {
            *called_clone.borrow_mut() = true;
            assert!(matches!(args.first(), Some(Value::Number(n)) if *n == 1.0));
            Value::Number(2.0)
        })));

        let _chained = Promise::then(&p, Some(callback), None, &mut queue);
        queue.drain();
        assert!(*called.borrow());
    }

    #[test]
    fn test_promise_then_before_resolve() {
        let p = Promise::new();
        let mut queue = MicrotaskQueue::new();

        let called = Rc::new(RefCell::new(false));
        let called_clone = Rc::clone(&called);
        let callback = Value::NativeFunction(Rc::new(NativeFunction::new("test", move |_args| {
            *called_clone.borrow_mut() = true;
            Value::Undefined
        })));

        let _chained = Promise::then(&p, Some(callback), None, &mut queue);
        assert!(!*called.borrow()); // Not called yet

        Promise::resolve(&p, Value::Number(1.0), &mut queue);
        queue.drain();
        assert!(*called.borrow()); // Now called
    }

    #[test]
    fn test_promise_chain() {
        let p = Promise::new();
        let mut queue = MicrotaskQueue::new();

        let double = Value::NativeFunction(Rc::new(NativeFunction::new("double", |args| {
            let n = args.first().map_or(0.0, |v| v.to_number());
            Value::Number(n * 2.0)
        })));

        let p2 = Promise::then(&p, Some(double), None, &mut queue);

        Promise::resolve(&p, Value::Number(5.0), &mut queue);
        queue.drain();

        assert_eq!(p2.borrow().state, PromiseState::Fulfilled);
        assert!(matches!(p2.borrow().result, Value::Number(n) if n == 10.0));
    }

    #[test]
    fn test_microtask_queue_drain() {
        let mut queue = MicrotaskQueue::new();
        let counter = Rc::new(RefCell::new(0));

        for _ in 0..5 {
            let c = Rc::clone(&counter);
            queue.enqueue(Microtask::Callback(Value::NativeFunction(Rc::new(
                NativeFunction::new("inc", move |_| {
                    *c.borrow_mut() += 1;
                    Value::Undefined
                }),
            ))));
        }

        let count = queue.drain();
        assert_eq!(count, 5);
        assert_eq!(*counter.borrow(), 5);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_double_resolve_ignored() {
        let p = Promise::new();
        let mut queue = MicrotaskQueue::new();
        Promise::resolve(&p, Value::Number(1.0), &mut queue);
        Promise::resolve(&p, Value::Number(2.0), &mut queue); // Ignored
        assert!(matches!(p.borrow().result, Value::Number(n) if n == 1.0));
    }
}
