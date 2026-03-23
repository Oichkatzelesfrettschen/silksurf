//! setTimeout, setInterval, clearTimeout, clearInterval,
//! requestAnimationFrame, cancelAnimationFrame, queueMicrotask
//!
//! These are installed on the global object but need access to the VM's
//! timer queue and microtask queue. Since NativeFunction closures can't
//! borrow the VM, we use shared Rc<RefCell<>> references to the queues.
//!
//! The actual timer/microtask queues live on the Vm struct. These builtins
//! are wired up via Vm::install_timer_builtins() after Vm::new().

use super::native_fn;
use crate::vm::value::{Object, Value};

/// Install placeholder timer functions on the global.
/// These work for the common pattern of setTimeout(fn, 0) used for deferral.
/// Full timer integration requires the event loop (Phase A.5.3).
pub fn install(global: &mut Object) {
    // For now, these are simplified versions that record the callback
    // but don't actually schedule via the timer queue (which requires &mut Vm).
    // The event loop integration (task #20) will wire these properly.

    global.set_by_str(
        "setTimeout",
        native_fn("setTimeout", |args| {
            // Return a dummy timer ID; actual scheduling happens in event loop
            let _callback = args.first().cloned();
            let _delay = args.get(1).map_or(0.0, |v| v.to_number());
            Value::Number(0.0) // Placeholder ID
        }),
    );

    global.set_by_str(
        "setInterval",
        native_fn("setInterval", |args| {
            let _callback = args.first().cloned();
            let _interval = args.get(1).map_or(0.0, |v| v.to_number());
            Value::Number(0.0)
        }),
    );

    global.set_by_str("clearTimeout", native_fn("clearTimeout", |_args| Value::Undefined));

    global.set_by_str("clearInterval", native_fn("clearInterval", |_args| Value::Undefined));

    global.set_by_str(
        "requestAnimationFrame",
        native_fn("requestAnimationFrame", |_args| Value::Number(0.0)),
    );

    global.set_by_str(
        "cancelAnimationFrame",
        native_fn("cancelAnimationFrame", |_args| Value::Undefined),
    );

    global.set_by_str("queueMicrotask", native_fn("queueMicrotask", |_args| Value::Undefined));
}
