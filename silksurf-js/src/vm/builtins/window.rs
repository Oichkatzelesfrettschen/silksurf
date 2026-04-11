//! window global object and performance API.
//!
//! `window` is a proxy to the global object (self-referential).
//! `performance.now()` returns high-res monotonic timestamp.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use super::{make_object_with_methods, native_fn};
use crate::vm::value::{Object, PropertyKey, Value};

// Shared start time for performance.now() (monotonic from VM creation).
thread_local! {
    static PERF_ORIGIN: Instant = Instant::now();
}

pub fn install(global: &mut Object) {
    // performance object with now()
    let perf =
        make_object_with_methods(vec![("now", native_fn("performance.now", performance_now))]);
    // Add performance.timeOrigin
    if let Value::Object(obj) = &perf {
        obj.borrow_mut()
            .set_by_str("timeOrigin", Value::Number(0.0));
    }
    global.set_by_str("performance", perf);

    // navigator object (minimal)
    let navigator = make_object_with_methods(vec![]);
    if let Value::Object(obj) = &navigator {
        let mut o = obj.borrow_mut();
        o.set_by_str("userAgent", Value::string("SilkSurf/0.1"));
        o.set_by_str("language", Value::string("en-US"));
        o.set_by_str("platform", Value::string("Linux x86_64"));
    }
    global.set_by_str("navigator", navigator);

    // location object (minimal)
    let location = make_object_with_methods(vec![]);
    if let Value::Object(obj) = &location {
        let mut o = obj.borrow_mut();
        o.set_by_str("href", Value::string("about:blank"));
        o.set_by_str("origin", Value::string("null"));
        o.set_by_str("protocol", Value::string("https:"));
    }
    global.set_by_str("location", location);

    /*
     * matchMedia -- returns a MediaQueryList for the given CSS media query.
     *
     * WHY: ChatGPT scripts check window.matchMedia('(prefers-color-scheme: dark)')
     * and similar queries to adapt to system preferences. Without this stub,
     * the property access returns undefined and subsequent .matches access throws.
     *
     * Implementation: always returns matches=false (no display to evaluate against).
     * addEventListener/addListener no-ops to absorb change listener registration.
     */
    let mql_factory = native_fn("matchMedia", |args| {
        let query = args
            .first()
            .map(|v| {
                let s = v.to_js_string();
                s.as_str().unwrap_or("").to_string()
            })
            .unwrap_or_default();

        use crate::vm::value::Object;
        let mql = Rc::new(RefCell::new(Object::new()));
        {
            let mut o = mql.borrow_mut();
            o.set_by_str("matches", Value::Boolean(false));
            o.set_by_str("media", Value::string_owned(query));
            o.set_by_str("onchange", Value::Null);
            o.set_by_str(
                "addEventListener",
                crate::vm::value::Value::NativeFunction(Rc::new(
                    crate::vm::value::NativeFunction::new("addEventListener", |_| Value::Undefined),
                )),
            );
            o.set_by_str(
                "removeEventListener",
                crate::vm::value::Value::NativeFunction(Rc::new(
                    crate::vm::value::NativeFunction::new("removeEventListener", |_| {
                        Value::Undefined
                    }),
                )),
            );
            // Legacy addListener/removeListener (deprecated but used by some scripts)
            o.set_by_str(
                "addListener",
                crate::vm::value::Value::NativeFunction(Rc::new(
                    crate::vm::value::NativeFunction::new("addListener", |_| Value::Undefined),
                )),
            );
            o.set_by_str(
                "removeListener",
                crate::vm::value::Value::NativeFunction(Rc::new(
                    crate::vm::value::NativeFunction::new("removeListener", |_| Value::Undefined),
                )),
            );
        }
        Value::Object(mql)
    });
    global.set_by_str("matchMedia", mql_factory);

    /*
     * addEventListener / removeEventListener / dispatchEvent stubs.
     *
     * WHY: Scripts call window.addEventListener('load', handler) and
     * document.addEventListener('DOMContentLoaded', handler) at module
     * init time. Without these, property access returns undefined and
     * the subsequent call throws a TypeError. The handlers themselves
     * never fire (no event loop dispatch), but absorbing the registration
     * prevents the TypeError that would abort the script.
     */
    global.set_by_str(
        "addEventListener",
        crate::vm::value::Value::NativeFunction(Rc::new(crate::vm::value::NativeFunction::new(
            "addEventListener",
            |_| Value::Undefined,
        ))),
    );
    global.set_by_str(
        "removeEventListener",
        crate::vm::value::Value::NativeFunction(Rc::new(crate::vm::value::NativeFunction::new(
            "removeEventListener",
            |_| Value::Undefined,
        ))),
    );
    global.set_by_str(
        "dispatchEvent",
        crate::vm::value::Value::NativeFunction(Rc::new(crate::vm::value::NativeFunction::new(
            "dispatchEvent",
            |_| Value::Boolean(true),
        ))),
    );

    // self = window = globalThis (all point to global)
    // These are set after global construction since they're self-referential.
    // The caller (install_builtins) handles this.
}

/// Install the self-referential window/self/globalThis properties.
/// Must be called with the global Rc after all other builtins are installed.
pub fn install_window_self(global: &Rc<RefCell<Object>>) {
    let window_ref = Value::Object(Rc::clone(global));
    let mut g = global.borrow_mut();
    g.set_by_key(PropertyKey::from_str("window"), window_ref.clone());
    g.set_by_key(PropertyKey::from_str("self"), window_ref.clone());
    g.set_by_key(PropertyKey::from_str("globalThis"), window_ref);
}

fn performance_now(_args: &[Value]) -> Value {
    PERF_ORIGIN.with(|origin| {
        let elapsed = origin.elapsed();
        // Return milliseconds with microsecond precision
        Value::Number(elapsed.as_secs_f64() * 1000.0)
    })
}
