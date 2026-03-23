//! Event loop integration for the SilkSurf JS runtime.
//!
//! The event loop orchestrates:
//! 1. XCB event polling (mouse, keyboard, window events)
//! 2. Expired timer callbacks (setTimeout, setInterval)
//! 3. Microtask queue draining (Promise reactions, queueMicrotask)
//! 4. requestAnimationFrame callbacks
//! 5. DOM mutation -> re-layout -> re-render
//!
//! Architecture: single-threaded cooperative scheduling.
//! Each "tick" of the event loop runs one macrotask, then drains all microtasks.

use super::promise::MicrotaskQueue;
use super::timers::TimerQueue;
use super::value::Value;

/// Result of a single event loop tick.
#[derive(Debug, PartialEq, Eq)]
pub enum TickResult {
    /// Work was done (timers fired, microtasks drained, etc.)
    Active,
    /// No work to do; caller may sleep or poll for external events.
    Idle,
    /// The loop should terminate.
    Exit,
}

/// Run one tick of the event loop.
///
/// Returns what happened so the caller (webview) can decide whether
/// to render a frame or sleep.
pub fn tick(timers: &mut TimerQueue, microtasks: &mut MicrotaskQueue) -> TickResult {
    let mut did_work = false;

    // 1. Drain expired timers (macrotasks)
    let timer_callbacks = timers.drain_expired();
    for callback in &timer_callbacks {
        execute_callback(callback);
    }
    if !timer_callbacks.is_empty() {
        did_work = true;
    }

    // 2. Drain microtask queue (after each macrotask)
    if !microtasks.is_empty() {
        microtasks.drain();
        did_work = true;
    }

    // 3. Drain rAF callbacks (once per frame)
    let raf_callbacks = timers.drain_raf();
    for callback in &raf_callbacks {
        // rAF callback receives a timestamp argument
        let timestamp = Value::Number(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0),
        );
        execute_callback_with_args(callback, &[timestamp]);
    }
    if !raf_callbacks.is_empty() {
        did_work = true;
    }

    // 4. Drain any microtasks enqueued by rAF callbacks
    if !microtasks.is_empty() {
        microtasks.drain();
        did_work = true;
    }

    if did_work {
        TickResult::Active
    } else {
        TickResult::Idle
    }
}

/// Execute a callback value (NativeFunction only for now).
fn execute_callback(callback: &Value) {
    if let Value::NativeFunction(func) = callback {
        func.call(&[]);
    }
}

/// Execute a callback with arguments.
fn execute_callback_with_args(callback: &Value, args: &[Value]) {
    if let Value::NativeFunction(func) = callback {
        func.call(args);
    }
}

/// Check if the event loop has any pending work.
pub fn has_pending_work(timers: &TimerQueue, microtasks: &MicrotaskQueue) -> bool {
    timers.has_pending() || !microtasks.is_empty()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::vm::value::NativeFunction;

    #[test]
    fn test_tick_idle() {
        let mut timers = TimerQueue::new();
        let mut microtasks = MicrotaskQueue::new();
        assert_eq!(tick(&mut timers, &mut microtasks), TickResult::Idle);
    }

    #[test]
    fn test_tick_with_expired_timer() {
        let mut timers = TimerQueue::new();
        let mut microtasks = MicrotaskQueue::new();

        let called = Rc::new(RefCell::new(false));
        let called_clone = Rc::clone(&called);
        timers.set_timeout(
            Value::NativeFunction(Rc::new(NativeFunction::new("test", move |_| {
                *called_clone.borrow_mut() = true;
                Value::Undefined
            }))),
            0,
        );

        std::thread::sleep(std::time::Duration::from_millis(1));
        assert_eq!(tick(&mut timers, &mut microtasks), TickResult::Active);
        assert!(*called.borrow());
    }

    #[test]
    fn test_tick_with_microtask() {
        use super::super::promise::Microtask;

        let mut timers = TimerQueue::new();
        let mut microtasks = MicrotaskQueue::new();

        let called = Rc::new(RefCell::new(false));
        let called_clone = Rc::clone(&called);
        microtasks.enqueue(Microtask::Callback(Value::NativeFunction(Rc::new(
            NativeFunction::new("test", move |_| {
                *called_clone.borrow_mut() = true;
                Value::Undefined
            }),
        ))));

        assert_eq!(tick(&mut timers, &mut microtasks), TickResult::Active);
        assert!(*called.borrow());
    }

    #[test]
    fn test_has_pending_work() {
        let mut timers = TimerQueue::new();
        let microtasks = MicrotaskQueue::new();
        assert!(!has_pending_work(&timers, &microtasks));

        timers.set_timeout(Value::Number(1.0), 1000);
        assert!(has_pending_work(&timers, &microtasks));
    }
}
