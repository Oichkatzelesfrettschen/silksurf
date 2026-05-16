/*
 * boa_backend/mod.rs -- Production JS runtime backed by boa_engine.
 *
 * WHY: boa_engine is an ECMA-262 2024+ compliant engine with a register-based
 * bytecode VM, inline caches (ICs), and shape-based object optimization. It
 * supports try/catch/finally, async/await, generators, Promises, Proxy, and
 * the full test262 suite at ~90%+ pass rate -- coverage the hand-written VM
 * cannot reach in reasonable time. Using a crate's public API (NativeFunction,
 * NativeObject, HostHooks) is categorically different from copying its source.
 *
 * WHAT: SilkContext wraps boa_engine::Context with pre-installed host objects:
 *   - console (via boa_runtime::Console)
 *   - setTimeout/clearTimeout/setInterval/clearInterval (immediate-dispatch stubs)
 *   - requestAnimationFrame (no-op stub returning fake id)
 *   - document (stub object; full DOM bridge is future work)
 *   - window / self (alias for globalThis)
 *
 * HOW:
 *   let mut ctx = SilkContext::new();
 *   ctx.eval(script_source)?;
 *   ctx.run_pending_jobs();
 *
 * DOM BRIDGE NOTE: document.getElementById and friends return null. A full
 * silksurf_dom::Dom-backed bridge will be wired in a subsequent pass using
 * boa_engine's NativeObject trait to expose NodeId handles to JS.
 */

use boa_engine::{
    Context, JsValue, NativeFunction, Source, js_string, object::ObjectInitializer,
    property::Attribute,
};
use boa_runtime::Console;

/// Production JavaScript execution context backed by `boa_engine`.
///
/// Create with `SilkContext::new()`, then call `eval()` for each script chunk.
/// Call `run_pending_jobs()` after all scripts to drain Promise microtasks.
pub struct SilkContext {
    ctx: Context,
}

impl Default for SilkContext {
    fn default() -> Self {
        Self::new()
    }
}

impl SilkContext {
    /// Build a context with `SilkSurf` host objects pre-installed.
    ///
    /// Panics only if `boa_engine` itself is in an inconsistent state (should
    /// never happen on a freshly-constructed Context).
    #[must_use]
    pub fn new() -> Self {
        let mut ctx = Context::default();

        // -- Console ----------------------------------------------------------
        // boa_runtime provides the W3C-compatible console object.
        let console = Console::init(&mut ctx);
        // UNWRAP-OK: fresh Context cannot already have a "console" property.
        ctx.register_global_property(js_string!("console"), console, Attribute::all())
            .expect("console: install on fresh context cannot fail");

        // -- Timer stubs ------------------------------------------------------
        // Full scheduling is deferred; call the callback immediately for
        // setTimeout so that code patterns like `setTimeout(fn, 0)` work.
        ctx.register_global_callable(
            js_string!("setTimeout"),
            2,
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                if let Some(cb) = args.first()
                    && let Some(obj) = cb.as_object()
                    && obj.is_callable()
                {
                    // Best-effort: ignore the return value and any error.
                    let _ = obj.call(&JsValue::undefined(), &[], ctx);
                }
                Ok(JsValue::from(0u32))
            }),
        )
        // UNWRAP-OK: fresh Context cannot already have "setTimeout" defined.
        .expect("setTimeout: install on fresh context cannot fail");

        ctx.register_global_callable(
            js_string!("clearTimeout"),
            1,
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
        )
        // UNWRAP-OK: fresh Context cannot already have "clearTimeout" defined.
        .expect("clearTimeout: install on fresh context cannot fail");

        ctx.register_global_callable(
            js_string!("setInterval"),
            2,
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::from(0u32))),
        )
        // UNWRAP-OK: fresh Context cannot already have "setInterval" defined.
        .expect("setInterval: install on fresh context cannot fail");

        ctx.register_global_callable(
            js_string!("clearInterval"),
            1,
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
        )
        // UNWRAP-OK: fresh Context cannot already have "clearInterval" defined.
        .expect("clearInterval: install on fresh context cannot fail");

        ctx.register_global_callable(
            js_string!("requestAnimationFrame"),
            1,
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::from(0u32))),
        )
        // UNWRAP-OK: fresh Context cannot already have "requestAnimationFrame" defined.
        .expect("requestAnimationFrame: install on fresh context cannot fail");

        ctx.register_global_callable(
            js_string!("cancelAnimationFrame"),
            1,
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
        )
        // UNWRAP-OK: fresh Context cannot already have "cancelAnimationFrame" defined.
        .expect("cancelAnimationFrame: install on fresh context cannot fail");

        // -- document stub ----------------------------------------------------
        // getElementById / querySelector / querySelectorAll return null until
        // the full DOM bridge (NativeObject-backed NodeId handles) is wired in.
        let document = ObjectInitializer::new(&mut ctx)
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
                js_string!("getElementById"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
                js_string!("querySelector"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
                js_string!("querySelectorAll"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("addEventListener"),
                2,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("removeEventListener"),
                2,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("dispatchEvent"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
                js_string!("createElement"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
                js_string!("createElementNS"),
                2,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
                js_string!("createTextNode"),
                1,
            )
            .build();
        // UNWRAP-OK: fresh Context cannot already have a "document" property.
        ctx.register_global_property(js_string!("document"), document, Attribute::all())
            .expect("document: install on fresh context cannot fail");

        // -- window / self aliases -------------------------------------------
        // window and self are aliases for globalThis in a browser context.
        // Cloning JsObject only increments the GC reference count; no copy.
        let global_obj = ctx.global_object().clone();
        // UNWRAP-OK: fresh Context cannot already have "window" or "self" properties.
        ctx.register_global_property(js_string!("window"), global_obj.clone(), Attribute::all())
            .expect("window: install on fresh context cannot fail");
        ctx.register_global_property(js_string!("self"), global_obj, Attribute::all())
            .expect("self: install on fresh context cannot fail");

        Self { ctx }
    }

    /// Evaluate a JS source string and drain the microtask queue.
    ///
    /// Returns `Ok(())` on success. Returns `Err(message)` on parse or runtime
    /// error; the message is the `boa_engine` `JsError` Display string.
    pub fn eval(&mut self, script: &str) -> Result<(), String> {
        match self.ctx.eval(Source::from_bytes(script.as_bytes())) {
            Ok(_) => {
                // run_jobs() returns JsResult<()>; errors here are internal
                // scheduling faults, not script errors.  Discard safely.
                let _ = self.ctx.run_jobs();
                Ok(())
            }
            Err(e) => Err(format!("{e}")),
        }
    }

    /// Drain all pending microtasks and Promise reactions.
    ///
    /// Call this after a batch of `eval()` calls to ensure Promises settled
    /// during script execution have their `.then()` continuations run.
    pub fn run_pending_jobs(&mut self) {
        let _ = self.ctx.run_jobs();
    }
}
