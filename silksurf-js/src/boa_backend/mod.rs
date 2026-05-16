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
 *   - document (stub by default; replaced with live DOM when with_dom() is used)
 *   - window / self (alias for globalThis)
 *   - location / navigator / performance / localStorage (browser-env stubs)
 *
 * HOW:
 *   // Without DOM access (scripts only):
 *   let mut ctx = SilkContext::new();
 *   ctx.eval(script_source)?;
 *   ctx.run_pending_jobs();
 *
 *   // With live DOM bridge:
 *   let dom_arc = Arc::new(Mutex::new(parse_html(html)));
 *   let mut ctx = SilkContext::with_dom(dom_arc);
 *   ctx.eval(script_source)?;
 */

use std::sync::{Arc, Mutex};

use boa_engine::{
    Context, JsNativeError, JsString, JsValue, NativeFunction, Source, js_string,
    object::{
        ObjectInitializer,
        builtins::{JsArray, JsPromise},
    },
    property::Attribute,
};
use boa_runtime::Console;

mod dom_bridge;

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

        // -- fetch() ----------------------------------------------------------
        // Synchronous execution: the HTTP request blocks the calling thread.
        // The returned JsPromise is pre-resolved (or pre-rejected), so .then()
        // chains and await both work correctly without an async event loop.
        ctx.register_global_callable(
            js_string!("fetch"),
            1,
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                let url = if let Some(v) = args.first() {
                    v.to_string(ctx)?.to_std_string_lossy()
                } else {
                    let err = JsNativeError::typ()
                        .with_message("fetch: URL argument is required");
                    return Ok(JsValue::from(JsPromise::from_result::<JsValue, JsNativeError>(Err(err), ctx)));
                };
                Ok(JsValue::from(fetch_sync(url.as_str(), ctx)))
            }),
        )
        // UNWRAP-OK: fresh Context cannot already have "fetch" defined.
        .expect("fetch: install on fresh context cannot fail");

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

        // -- location stub ---------------------------------------------------
        // href/origin/pathname/search/hash default to empty; assign/reload/replace
        // are no-ops. Scripts that only read location properties will not throw.
        let location = ObjectInitializer::new(&mut ctx)
            .property(js_string!("href"), js_string!(""), Attribute::all())
            .property(js_string!("origin"), js_string!(""), Attribute::all())
            .property(js_string!("pathname"), js_string!("/"), Attribute::all())
            .property(js_string!("search"), js_string!(""), Attribute::all())
            .property(js_string!("hash"), js_string!(""), Attribute::all())
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("assign"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("reload"),
                0,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("replace"),
                1,
            )
            .build();
        // UNWRAP-OK: fresh Context cannot already have a "location" property.
        ctx.register_global_property(js_string!("location"), location, Attribute::all())
            .expect("location: install on fresh context cannot fail");

        // -- navigator stub --------------------------------------------------
        // Minimal subset for feature-detection: userAgent, platform, language,
        // onLine (true), cookieEnabled (false).
        let navigator = ObjectInitializer::new(&mut ctx)
            .property(
                js_string!("userAgent"),
                js_string!("SilkSurf/0.1"),
                Attribute::all(),
            )
            .property(
                js_string!("platform"),
                js_string!("Linux"),
                Attribute::all(),
            )
            .property(js_string!("language"), js_string!("en"), Attribute::all())
            .property(js_string!("onLine"), true, Attribute::all())
            .property(js_string!("cookieEnabled"), false, Attribute::all())
            .build();
        // UNWRAP-OK: fresh Context cannot already have a "navigator" property.
        ctx.register_global_property(js_string!("navigator"), navigator, Attribute::all())
            .expect("navigator: install on fresh context cannot fail");

        // -- performance stub ------------------------------------------------
        // performance.now() returns 0.0; mark() and measure() are no-ops.
        // Scripts that measure relative durations will get zero but not crash.
        let performance = ObjectInitializer::new(&mut ctx)
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::from(0.0f64))),
                js_string!("now"),
                0,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("mark"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("measure"),
                1,
            )
            .build();
        // UNWRAP-OK: fresh Context cannot already have a "performance" property.
        ctx.register_global_property(js_string!("performance"), performance, Attribute::all())
            .expect("performance: install on fresh context cannot fail");

        // -- localStorage stub -----------------------------------------------
        // All writes are discarded; getItem/key return null; length is 0.
        // Scripts that guard on localStorage availability will not throw.
        let local_storage = ObjectInitializer::new(&mut ctx)
            .property(js_string!("length"), 0u32, Attribute::all())
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
                js_string!("getItem"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("setItem"),
                2,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("removeItem"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
                js_string!("clear"),
                0,
            )
            .function(
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::null())),
                js_string!("key"),
                1,
            )
            .build();
        // UNWRAP-OK: fresh Context cannot already have a "localStorage" property.
        ctx.register_global_property(js_string!("localStorage"), local_storage, Attribute::all())
            .expect("localStorage: install on fresh context cannot fail");

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

    /// Build a context with the full DOM bridge wired to `dom`.
    ///
    /// The returned context has all standard host objects from `new()` plus
    /// a live `document` object whose methods (`getElementById`, `querySelector`,
    /// `querySelectorAll`, `createElement`, `setAttribute`) read and write
    /// the supplied `Dom` through the shared `Arc<Mutex<Dom>>`.
    #[must_use]
    pub fn with_dom(dom: &Arc<Mutex<silksurf_dom::Dom>>) -> Self {
        let mut ctx = Self::new();
        dom_bridge::install_document(dom, &mut ctx.ctx);
        ctx
    }
}

// ---- fetch() implementation ------------------------------------------------

/// Execute a single HTTP GET synchronously and return a pre-resolved `JsPromise`.
///
/// The Promise is pre-resolved (or pre-rejected), so `.then()` chains and
/// `await` both work without an async event loop.  Only GET is supported when
/// called without an options argument.
fn fetch_sync(url: &str, ctx: &mut Context) -> JsPromise {
    use silksurf_net::{BasicClient, HttpMethod, HttpRequest, NetClient};

    let request = HttpRequest {
        method: HttpMethod::Get,
        url: url.to_owned(),
        headers: vec![("Accept".to_owned(), "*/*".to_owned())],
        body: Vec::new(),
    };

    match BasicClient::new().fetch(&request) {
        Ok(response) => {
            let response_val = build_response_object(response, ctx);
            JsPromise::from_result::<JsValue, JsNativeError>(Ok(response_val), ctx)
        }
        Err(err) => {
            let js_err = JsNativeError::error().with_message(err.message.clone());
            JsPromise::from_result::<JsValue, JsNativeError>(Err(js_err), ctx)
        }
    }
}

/// Build a plain JS Response-like object from an HTTP response.
///
/// Exposes: status (u32), ok (bool), statusText (string),
/// `text()` -> `Promise<string>`, `json()` -> `Promise<object>`.
fn build_response_object(response: silksurf_net::HttpResponse, ctx: &mut Context) -> JsValue {
    let status = response.status;
    let body = response.body;

    let status_text = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "",
    };

    // Build headers JsArray of [name, value] pairs.
    let headers_arr = JsArray::new(ctx);
    for (name, value) in &response.headers {
        let pair = JsArray::new(ctx);
        let _ = pair.push(JsValue::from(JsString::from(name.as_str())), ctx);
        let _ = pair.push(JsValue::from(JsString::from(value.as_str())), ctx);
        let _ = headers_arr.push(JsValue::from(pair), ctx);
    }

    // text() closure
    // SAFETY: body_text is Vec<u8>, which is not a GC-traced type.
    let body_text = body.clone();
    let text_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let s = String::from_utf8_lossy(&body_text).to_string();
            let p = JsPromise::from_result::<JsValue, JsNativeError>(
                Ok(JsValue::from(JsString::from(s.as_str()))),
                ctx,
            );
            Ok(JsValue::from(p))
        })
    };

    // json() closure
    // SAFETY: body_json is Vec<u8>, which is not a GC-traced type.
    let body_json = body;
    let json_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let text = String::from_utf8_lossy(&body_json);
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(parsed) => {
                    let js_val = serde_json_to_js(&parsed, ctx);
                    let p = JsPromise::from_result::<JsValue, JsNativeError>(Ok(js_val), ctx);
                    Ok(JsValue::from(p))
                }
                Err(e) => {
                    let err =
                        JsNativeError::syntax().with_message(format!("JSON parse error: {e}"));
                    let p = JsPromise::from_result::<JsValue, JsNativeError>(Err(err), ctx);
                    Ok(JsValue::from(p))
                }
            }
        })
    };

    ObjectInitializer::new(ctx)
        .property(js_string!("status"), status, Attribute::all())
        .property(
            js_string!("ok"),
            (200..300).contains(&status),
            Attribute::all(),
        )
        .property(
            js_string!("statusText"),
            JsString::from(status_text),
            Attribute::all(),
        )
        .property(js_string!("headers"), headers_arr, Attribute::all())
        .function(text_fn, js_string!("text"), 0)
        .function(json_fn, js_string!("json"), 0)
        .build()
        .into()
}

/// Recursively convert a `serde_json::Value` into a `JsValue`.
fn serde_json_to_js(val: &serde_json::Value, ctx: &mut Context) -> JsValue {
    match val {
        serde_json::Value::Null => JsValue::null(),
        serde_json::Value::Bool(b) => JsValue::from(*b),
        serde_json::Value::Number(n) => JsValue::from(n.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(s) => JsValue::from(JsString::from(s.as_str())),
        serde_json::Value::Array(arr) => {
            let js_arr = JsArray::new(ctx);
            for item in arr {
                let v = serde_json_to_js(item, ctx);
                let _ = js_arr.push(v, ctx);
            }
            JsValue::from(js_arr)
        }
        serde_json::Value::Object(map) => {
            let pairs: Vec<(String, JsValue)> = map
                .iter()
                .map(|(k, v)| (k.clone(), serde_json_to_js(v, ctx)))
                .collect();
            let mut init = ObjectInitializer::new(ctx);
            for (key, val) in pairs {
                init.property(js_string!(key.as_str()), val, Attribute::all());
            }
            init.build().into()
        }
    }
}
