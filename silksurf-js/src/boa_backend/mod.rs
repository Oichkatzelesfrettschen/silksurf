/*
 * boa_backend/mod.rs -- Production JS runtime backed by boa_engine.
 *
 * boa_engine supplies the production ECMA-262 runtime. SilkContext installs
 * the browser host objects that silksurf needs and keeps callback scheduling
 * explicit so the GUI controls when deferred JS work runs.
 *
 * SilkContext wraps boa_engine::Context with pre-installed host objects:
 *   - console (via boa_runtime::Console)
 *   - setTimeout/clearTimeout/setInterval/clearInterval queued host callbacks
 *   - requestAnimationFrame/cancelAnimationFrame queued frame callbacks
 *   - document (stub by default; replaced with live DOM when with_dom() is used)
 *   - window / self (alias for globalThis)
 *   - location / navigator / performance / Web Storage host objects
 */

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};

use boa_engine::{
    Context, JsNativeError, JsObject, JsString, JsValue, Module, NativeFunction, Source,
    builtins::promise::PromiseState,
    js_string,
    module::MapModuleLoader,
    object::{
        FunctionObjectBuilder, ObjectInitializer,
        builtins::{JsArray, JsFunction, JsPromise},
    },
    property::Attribute,
};
use boa_runtime::Console;

mod dom_bridge;

const HOST_CALLBACKS_REGISTRY: &str = "__silksurfHostCallbacks";
const DEFAULT_HOST_CALLBACK_BUDGET: usize = 256;
const TRACE_HOST_CALLBACKS_ENV: &str = "SILKSURF_TRACE_HOST_CALLBACKS";

type HostSchedulerRef = Rc<RefCell<HostScheduler>>;

#[derive(Clone, Copy)]
enum HostCallbackQueue {
    Timeout,
    Interval,
    AnimationFrame,
}

impl HostCallbackQueue {
    fn label(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Interval => "interval",
            Self::AnimationFrame => "animation-frame",
        }
    }
}

#[derive(Clone, Copy)]
struct ScheduledHostCallback {
    id: u32,
    repeat: bool,
    queue: HostCallbackQueue,
}

#[derive(Default)]
struct HostScheduler {
    next_id: u32,
    timeout_callbacks: VecDeque<u32>,
    interval_callbacks: Vec<u32>,
    animation_frame_callbacks: Vec<u32>,
}

impl HostScheduler {
    fn new() -> Self {
        Self {
            next_id: 1,
            timeout_callbacks: VecDeque::new(),
            interval_callbacks: Vec::new(),
            animation_frame_callbacks: Vec::new(),
        }
    }

    fn schedule(&mut self, queue: HostCallbackQueue) -> u32 {
        let id = self.next_callback_id();
        match queue {
            HostCallbackQueue::Timeout => self.timeout_callbacks.push_back(id),
            HostCallbackQueue::Interval => self.interval_callbacks.push(id),
            HostCallbackQueue::AnimationFrame => self.animation_frame_callbacks.push(id),
        }
        id
    }

    fn cancel(&mut self, id: u32) {
        self.timeout_callbacks
            .retain(|callback_id| *callback_id != id);
        self.interval_callbacks
            .retain(|callback_id| *callback_id != id);
        self.animation_frame_callbacks
            .retain(|callback_id| *callback_id != id);
    }

    fn has_pending_callbacks(&self) -> bool {
        !self.timeout_callbacks.is_empty()
            || !self.interval_callbacks.is_empty()
            || !self.animation_frame_callbacks.is_empty()
    }

    fn take_timer_callbacks(&mut self, max_callbacks: usize) -> Vec<ScheduledHostCallback> {
        let mut callbacks = Vec::new();
        while callbacks.len() < max_callbacks {
            let Some(id) = self.timeout_callbacks.pop_front() else {
                break;
            };
            callbacks.push(ScheduledHostCallback {
                id,
                repeat: false,
                queue: HostCallbackQueue::Timeout,
            });
        }
        let remaining = max_callbacks.saturating_sub(callbacks.len());
        callbacks.extend(
            self.interval_callbacks
                .iter()
                .copied()
                .take(remaining)
                .map(|id| ScheduledHostCallback {
                    id,
                    repeat: true,
                    queue: HostCallbackQueue::Interval,
                }),
        );
        callbacks
    }

    fn take_animation_frame_callbacks(&mut self) -> Vec<ScheduledHostCallback> {
        self.animation_frame_callbacks
            .drain(..)
            .map(|id| ScheduledHostCallback {
                id,
                repeat: false,
                queue: HostCallbackQueue::AnimationFrame,
            })
            .collect()
    }

    fn next_callback_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = if id == u32::MAX { 1 } else { id + 1 };
        id
    }
}

type StorageMap = Rc<RefCell<HashMap<String, String>>>;
type CookieJar = Rc<RefCell<Vec<(String, String)>>>;

struct CookieAssignment {
    name: String,
    value: String,
    delete: bool,
}

fn install_storage_objects(ctx: &mut Context) {
    let local_storage = storage_object(ctx);
    let session_storage = storage_object(ctx);
    ctx.register_global_property(js_string!("localStorage"), local_storage, Attribute::all())
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("localStorage: install on fresh context cannot fail");
    ctx.register_global_property(
        js_string!("sessionStorage"),
        session_storage,
        Attribute::all(),
    )
    // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

    .expect("sessionStorage: install on fresh context cannot fail");
}

fn storage_object(ctx: &mut Context) -> JsObject {
    let storage = Rc::new(RefCell::new(HashMap::new()));
    let length_getter =
        FunctionObjectBuilder::new(ctx.realm(), storage_length_native(&storage)).build();

    ObjectInitializer::new(ctx)
        .accessor(
            js_string!("length"),
            Some(length_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .function(storage_get_item_native(&storage), js_string!("getItem"), 1)
        .function(storage_set_item_native(&storage), js_string!("setItem"), 2)
        .function(
            storage_remove_item_native(&storage),
            js_string!("removeItem"),
            1,
        )
        .function(storage_clear_native(&storage), js_string!("clear"), 0)
        .function(storage_key_native(&storage), js_string!("key"), 1)
        .build()
}

fn storage_string_arg(arg: Option<&JsValue>, ctx: &mut Context) -> boa_engine::JsResult<String> {
    match arg {
        Some(value) => Ok(value.to_string(ctx)?.to_std_string_lossy()),
        None => Ok(String::new()),
    }
}

fn storage_length_native(storage: &StorageMap) -> NativeFunction {
    let storage = Rc::clone(storage);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            Ok(JsValue::from(storage.borrow().len() as u32))
        })
    }
}

fn storage_get_item_native(storage: &StorageMap) -> NativeFunction {
    let storage = Rc::clone(storage);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let key = storage_string_arg(args.first(), ctx)?;
            Ok(storage
                .borrow()
                .get(&key)
                .map_or_else(JsValue::null, |value| {
                    JsValue::from(JsString::from(value.as_str()))
                }))
        })
    }
}

fn storage_set_item_native(storage: &StorageMap) -> NativeFunction {
    let storage = Rc::clone(storage);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let key = storage_string_arg(args.first(), ctx)?;
            let value = storage_string_arg(args.get(1), ctx)?;
            storage.borrow_mut().insert(key, value);
            Ok(JsValue::undefined())
        })
    }
}

fn storage_remove_item_native(storage: &StorageMap) -> NativeFunction {
    let storage = Rc::clone(storage);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let key = storage_string_arg(args.first(), ctx)?;
            storage.borrow_mut().remove(&key);
            Ok(JsValue::undefined())
        })
    }
}

fn storage_clear_native(storage: &StorageMap) -> NativeFunction {
    let storage = Rc::clone(storage);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            storage.borrow_mut().clear();
            Ok(JsValue::undefined())
        })
    }
}

fn storage_key_native(storage: &StorageMap) -> NativeFunction {
    let storage = Rc::clone(storage);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let index = args
                .first()
                .map(|value| value.to_u32(ctx))
                .transpose()?
                .unwrap_or(0) as usize;
            let mut keys: Vec<String> = storage.borrow().keys().cloned().collect();
            keys.sort_unstable();
            Ok(keys.get(index).map_or_else(JsValue::null, |key| {
                JsValue::from(JsString::from(key.as_str()))
            }))
        })
    }
}

fn new_cookie_jar() -> CookieJar {
    Rc::new(RefCell::new(Vec::new()))
}

fn document_cookie_getter(ctx: &mut Context, jar: &CookieJar) -> JsFunction {
    FunctionObjectBuilder::new(ctx.realm(), document_cookie_get_native(jar)).build()
}

fn document_cookie_setter(ctx: &mut Context, jar: &CookieJar) -> JsFunction {
    FunctionObjectBuilder::new(ctx.realm(), document_cookie_set_native(jar)).build()
}

fn document_cookie_get_native(jar: &CookieJar) -> NativeFunction {
    let jar = Rc::clone(jar);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            Ok(JsValue::from(JsString::from(
                cookie_header_value(&jar).as_str(),
            )))
        })
    }
}

fn document_cookie_set_native(jar: &CookieJar) -> NativeFunction {
    let jar = Rc::clone(jar);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let value = storage_string_arg(args.first(), ctx)?;
            if let Some(assignment) = parse_cookie_assignment(value.as_str()) {
                apply_cookie_assignment(&jar, assignment);
            }
            Ok(JsValue::undefined())
        })
    }
}

fn cookie_header_value(jar: &CookieJar) -> String {
    let jar = jar.borrow();
    let byte_len = jar
        .iter()
        .map(|(name, value)| name.len() + value.len() + 2)
        .sum();
    let mut header = String::with_capacity(byte_len);
    for (index, (name, value)) in jar.iter().enumerate() {
        if index > 0 {
            header.push_str("; ");
        }
        header.push_str(name);
        header.push('=');
        header.push_str(value);
    }
    header
}

fn parse_cookie_assignment(input: &str) -> Option<CookieAssignment> {
    let mut parts = input.split(';');
    let pair = parts.next()?.trim();
    let (name, value) = pair.split_once('=')?;
    let name = name.trim();
    if name.is_empty() || name.starts_with('$') {
        return None;
    }
    let delete = parts.any(cookie_attribute_deletes);
    Some(CookieAssignment {
        name: name.to_string(),
        value: value.trim().to_string(),
        delete,
    })
}

fn cookie_attribute_deletes(attribute: &str) -> bool {
    let attr = attribute.trim();
    attr.eq_ignore_ascii_case("max-age=0")
        || attr.eq_ignore_ascii_case("max-age=-1")
        || attr.eq_ignore_ascii_case("expires=thu, 01 jan 1970 00:00:00 gmt")
}

fn apply_cookie_assignment(jar: &CookieJar, assignment: CookieAssignment) {
    let mut jar = jar.borrow_mut();
    if let Some(index) = jar.iter().position(|(name, _)| name == &assignment.name) {
        if assignment.delete {
            jar.remove(index);
        } else {
            jar[index].1 = assignment.value;
        }
        return;
    }
    if !assignment.delete {
        jar.push((assignment.name, assignment.value));
    }
}

fn install_crypto(ctx: &mut Context) {
    let crypto = ObjectInitializer::new(ctx)
        .function(
            NativeFunction::from_fn_ptr(crypto_get_random_values),
            js_string!("getRandomValues"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(crypto_random_uuid),
            js_string!("randomUUID"),
            0,
        )
        .build();
    ctx.register_global_property(js_string!("crypto"), crypto, Attribute::all())
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("crypto: install on fresh context cannot fail");
}

fn crypto_get_random_values(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let target = args
        .first()
        .and_then(JsValue::as_object)
        .ok_or_else(|| JsNativeError::typ().with_message("getRandomValues: object is required"))?;
    let length = target.get(js_string!("length"), ctx)?.to_u32(ctx)? as usize;
    if length > 65_536 {
        return Err(JsNativeError::typ()
            .with_message("getRandomValues: quota exceeded")
            .into());
    }
    let mut bytes = vec![0_u8; length];
    fill_random_bytes(&mut bytes)?;
    for (index, byte) in bytes.into_iter().enumerate() {
        target.set(index, u32::from(byte), false, ctx)?;
    }
    Ok(JsValue::from(target))
}

fn crypto_random_uuid(
    _this: &JsValue,
    _args: &[JsValue],
    _ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let mut bytes = [0_u8; 16];
    fill_random_bytes(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(JsValue::from(JsString::from(
        format_uuid_v4(bytes).as_str(),
    )))
}

fn fill_random_bytes(bytes: &mut [u8]) -> boa_engine::JsResult<()> {
    getrandom::fill(bytes).map_err(|err| {
        JsNativeError::typ()
            .with_message(format!("crypto: OS randomness failed: {err}"))
            .into()
    })
}

fn format_uuid_v4(bytes: [u8; 16]) -> String {
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        push_hex_byte(&mut out, *byte);
    }
    out
}

fn push_hex_byte(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(char::from(HEX[(byte >> 4) as usize]));
    out.push(char::from(HEX[(byte & 0x0f) as usize]));
}

fn install_abort_api(ctx: &mut Context) {
    let abort_signal = FunctionObjectBuilder::new(
        ctx.realm(),
        NativeFunction::from_fn_ptr(abort_signal_constructor),
    )
    .name(js_string!("AbortSignal"))
    .length(0)
    .constructor(true)
    .build();
    let abort_signal_object: JsObject = abort_signal.clone().into();
    let abort_signal_abort = FunctionObjectBuilder::new(
        ctx.realm(),
        NativeFunction::from_fn_ptr(abort_signal_abort_static),
    )
    .name(js_string!("abort"))
    .length(1)
    .build();
    abort_signal_object
        .set(js_string!("abort"), abort_signal_abort, false, ctx)
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("AbortSignal.abort: install on fresh function cannot fail");
    ctx.register_global_property(js_string!("AbortSignal"), abort_signal, Attribute::all())
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("AbortSignal: install on fresh context cannot fail");

    let abort_controller = FunctionObjectBuilder::new(
        ctx.realm(),
        NativeFunction::from_fn_ptr(abort_controller_constructor),
    )
    .name(js_string!("AbortController"))
    .length(0)
    .constructor(true)
    .build();
    ctx.register_global_property(
        js_string!("AbortController"),
        abort_controller,
        Attribute::all(),
    )
    // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

    .expect("AbortController: install on fresh context cannot fail");
}

fn abort_controller_constructor(
    _this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let signal = new_abort_signal(false, JsValue::undefined(), ctx);
    let controller = ObjectInitializer::new(ctx)
        .property(js_string!("signal"), signal, Attribute::all())
        .function(
            NativeFunction::from_fn_ptr(abort_controller_abort),
            js_string!("abort"),
            1,
        )
        .build();
    Ok(controller.into())
}

fn abort_signal_constructor(
    _this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    Ok(new_abort_signal(false, JsValue::undefined(), ctx))
}

fn abort_signal_abort_static(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let reason = abort_reason(args.first(), ctx);
    Ok(new_abort_signal(true, reason, ctx))
}

fn new_abort_signal(aborted: bool, reason: JsValue, ctx: &mut Context) -> JsValue {
    ObjectInitializer::new(ctx)
        .property(js_string!("aborted"), aborted, Attribute::all())
        .property(js_string!("reason"), reason, Attribute::all())
        .property(js_string!("onabort"), JsValue::null(), Attribute::all())
        .function(
            NativeFunction::from_fn_ptr(abort_signal_add_event_listener),
            js_string!("addEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(abort_signal_remove_event_listener),
            js_string!("removeEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(abort_signal_dispatch_event),
            js_string!("dispatchEvent"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(abort_signal_throw_if_aborted),
            js_string!("throwIfAborted"),
            0,
        )
        .build()
        .into()
}

fn abort_controller_abort(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(controller) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    let signal = controller.get(js_string!("signal"), ctx)?;
    let Some(signal) = signal.as_object() else {
        return Ok(JsValue::undefined());
    };
    if signal_is_aborted(&signal, ctx)? {
        return Ok(JsValue::undefined());
    }
    signal.set(js_string!("aborted"), true, false, ctx)?;
    signal.set(
        js_string!("reason"),
        abort_reason(args.first(), ctx),
        false,
        ctx,
    )?;
    call_abort_listener(&signal, ctx)?;
    Ok(JsValue::undefined())
}

fn abort_signal_add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    if !event_type_is_abort(args.first(), ctx)? {
        return Ok(JsValue::undefined());
    }
    let Some(signal) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    if let Some(callback) = args.get(1).and_then(JsValue::as_object)
        && callback.is_callable()
    {
        signal.set(js_string!("onabort"), callback.clone(), false, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn abort_signal_remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    if !event_type_is_abort(args.first(), ctx)? {
        return Ok(JsValue::undefined());
    }
    let Some(signal) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    signal.set(js_string!("onabort"), JsValue::null(), false, ctx)?;
    Ok(JsValue::undefined())
}

fn abort_signal_dispatch_event(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(signal) = this.as_object() else {
        return Ok(false.into());
    };
    let Some(event) = args.first().and_then(JsValue::as_object) else {
        return Ok(false.into());
    };
    let event_type = event
        .get(js_string!("type"), ctx)?
        .to_string(ctx)?
        .to_std_string_lossy();
    if event_type == "abort" {
        call_abort_listener(&signal, ctx)?;
        return Ok(true.into());
    }
    Ok(false.into())
}

fn abort_signal_throw_if_aborted(
    this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(signal) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    if signal_is_aborted(&signal, ctx)? {
        return Err(JsNativeError::error()
            .with_message("AbortSignal: operation aborted")
            .into());
    }
    Ok(JsValue::undefined())
}

fn abort_reason(reason: Option<&JsValue>, ctx: &mut Context) -> JsValue {
    if let Some(reason) = reason
        && !reason.is_undefined()
    {
        return reason.clone();
    }
    ObjectInitializer::new(ctx)
        .property(
            js_string!("name"),
            js_string!("AbortError"),
            Attribute::all(),
        )
        .property(
            js_string!("message"),
            js_string!("The operation was aborted."),
            Attribute::all(),
        )
        .build()
        .into()
}

fn event_type_is_abort(
    event_type: Option<&JsValue>,
    ctx: &mut Context,
) -> boa_engine::JsResult<bool> {
    let Some(event_type) = event_type else {
        return Ok(false);
    };
    Ok(event_type.to_string(ctx)?.to_std_string_lossy() == "abort")
}

fn signal_is_aborted(signal: &JsObject, ctx: &mut Context) -> boa_engine::JsResult<bool> {
    Ok(signal
        .get(js_string!("aborted"), ctx)?
        .as_boolean()
        .unwrap_or(false))
}

fn call_abort_listener(signal: &JsObject, ctx: &mut Context) -> boa_engine::JsResult<()> {
    let listener = signal.get(js_string!("onabort"), ctx)?;
    let Some(listener) = listener
        .as_object()
        .filter(boa_engine::JsObject::is_callable)
    else {
        return Ok(());
    };
    let event = ObjectInitializer::new(ctx)
        .property(js_string!("type"), js_string!("abort"), Attribute::all())
        .property(js_string!("target"), signal.clone(), Attribute::all())
        .build();
    listener.call(&JsValue::from(signal.clone()), &[event.into()], ctx)?;
    Ok(())
}

/// Production JavaScript execution context backed by `boa_engine`.
///
/// Create with `SilkContext::new()`, then call `eval()` for each script chunk.
/// Call `run_pending_jobs()` after all scripts to drain Promise microtasks.
pub struct SilkContext {
    ctx: Context,
    module_loader: Rc<MapModuleLoader>,
    scheduler: HostSchedulerRef,
    start_time: Instant,
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
        let module_loader = Rc::new(MapModuleLoader::default());
        let mut ctx = Context::builder()
            .module_loader(Rc::clone(&module_loader))
            .build()
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("Boa Context builder succeeds with MapModuleLoader");
        let scheduler = Rc::new(RefCell::new(HostScheduler::new()));
        install_host_scheduler(&mut ctx, &scheduler);

        // -- Console ----------------------------------------------------------
        // boa_runtime provides the W3C-compatible console object.
        let console = Console::init(&mut ctx);
        // UNWRAP-OK: fresh Context cannot already have a "console" property.
        ctx.register_global_property(js_string!("console"), console, Attribute::all())
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("console: install on fresh context cannot fail");

        // -- fetch() ----------------------------------------------------------
        // Synchronous execution: the HTTP request blocks the calling thread.
        // The returned JsPromise is pre-resolved (or pre-rejected), so .then()
        // chains and await both work correctly without an async event loop.
        ctx.register_global_callable(
            js_string!("fetch"),
            1,
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                let Some(input) = args.first() else {
                    let err = JsNativeError::typ()
                        .with_message("fetch: URL argument is required");
                    return Ok(JsValue::from(JsPromise::from_result::<
                        JsValue,
                        JsNativeError,
                    >(Err(err), ctx)));
                };
                if fetch_signal_is_aborted(args, ctx)? {
                    let err = JsNativeError::error().with_message("fetch: signal is aborted");
                    return Ok(JsValue::from(JsPromise::from_result::<
                        JsValue,
                        JsNativeError,
                    >(Err(err), ctx)));
                }
                let url = fetch_input_url(input, ctx)?;
                Ok(JsValue::from(fetch_sync(url.as_str(), ctx)))
            }),
        )
        // UNWRAP-OK: fresh Context cannot already have "fetch" defined.
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("fetch: install on fresh context cannot fail");
        install_websocket(&mut ctx);
        install_stream_constructors(&mut ctx);
        install_crypto(&mut ctx);
        install_abort_api(&mut ctx);

        // -- document stub ----------------------------------------------------
        // getElementById / querySelector / querySelectorAll return null until
        // the full DOM bridge (NativeObject-backed NodeId handles) is wired in.
        let cookie_jar = new_cookie_jar();
        let cookie_getter = document_cookie_getter(&mut ctx, &cookie_jar);
        let cookie_setter = document_cookie_setter(&mut ctx, &cookie_jar);
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
            .accessor(
                js_string!("cookie"),
                Some(cookie_getter),
                Some(cookie_setter),
                Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
            )
            .build();
        // UNWRAP-OK: fresh Context cannot already have a "document" property.
        ctx.register_global_property(js_string!("document"), document, Attribute::all())
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("document: install on fresh context cannot fail");

        // -- window / self aliases -------------------------------------------
        // window and self are aliases for globalThis in a browser context.
        // Cloning JsObject only increments the GC reference count; no copy.
        let global_obj = ctx.global_object().clone();
        // UNWRAP-OK: fresh Context cannot already have "window" or "self" properties.
        ctx.register_global_property(js_string!("window"), global_obj.clone(), Attribute::all())
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("window: install on fresh context cannot fail");
        ctx.register_global_property(js_string!("self"), global_obj, Attribute::all())
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

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
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("location: install on fresh context cannot fail");

        // -- navigator stub --------------------------------------------------
        // Minimal subset for feature-detection: userAgent, platform, language,
        // onLine (true), cookieEnabled (true).
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
            .property(js_string!("cookieEnabled"), true, Attribute::all())
            .build();
        // UNWRAP-OK: fresh Context cannot already have a "navigator" property.
        ctx.register_global_property(js_string!("navigator"), navigator, Attribute::all())
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

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
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("performance: install on fresh context cannot fail");

        install_storage_objects(&mut ctx);

        Self {
            ctx,
            module_loader,
            scheduler,
            start_time: Instant::now(),
        }
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

    /// Parse, link, and evaluate a bounded module graph already fetched by the browser.
    pub fn eval_module_graph(
        &mut self,
        root_path: &str,
        modules: &[(String, String)],
    ) -> Result<(), String> {
        self.module_loader.clear();
        let mut root_module = None;
        for (module_path, source_text) in modules {
            let path = PathBuf::from(module_path);
            let source = Source::from_bytes(source_text.as_bytes()).with_path(path.as_path());
            let module = Module::parse(source, None, &mut self.ctx)
                .map_err(|err| format!("module parse {module_path}: {err}"))?;
            self.module_loader.insert(module_path, module.clone());
            if module_path == root_path {
                root_module = Some(module);
            }
        }

        let module =
            root_module.ok_or_else(|| format!("module root {root_path} was not fetched"))?;
        let promise = module.load_link_evaluate(&mut self.ctx);
        let _ = self.ctx.run_jobs();
        match promise.state() {
            PromiseState::Fulfilled(_) => Ok(()),
            PromiseState::Rejected(reason) => {
                Err(format!("module evaluation rejected: {reason:?}"))
            }
            PromiseState::Pending => Err("module evaluation stayed pending".to_string()),
        }
    }

    /// Drain all pending microtasks and Promise reactions.
    ///
    /// Call this after a batch of `eval()` calls to ensure Promises settled
    /// during script execution have their `.then()` continuations run.
    pub fn run_pending_jobs(&mut self) {
        let _ = self.ctx.run_jobs();
    }

    /// Return true when host timer or frame callbacks are ready to run.
    #[must_use]
    pub fn has_pending_host_callbacks(&self) -> bool {
        self.scheduler.borrow().has_pending_callbacks()
    }

    /// Run queued setTimeout, setInterval, and requestAnimationFrame callbacks.
    ///
    /// The caller supplies the tick cadence. `max_timer_callbacks` bounds timer
    /// work per tick; rAF callbacks drain once per call.
    pub fn run_host_callbacks(&mut self, max_timer_callbacks: usize) -> Result<usize, String> {
        let budget = max_timer_callbacks.max(1);
        let trace_callbacks = trace_host_callbacks_enabled();
        let mut ran = 0;
        let timer_callbacks = self.scheduler.borrow_mut().take_timer_callbacks(budget);
        for callback in timer_callbacks {
            let callback_start = Instant::now();
            if self.call_registered_callback(callback, &[])? {
                ran += 1;
            }
            trace_host_callback(trace_callbacks, callback, callback_start.elapsed());
        }

        let frame_timestamp = JsValue::from(self.frame_timestamp_ms());
        let frame_callbacks = self.scheduler.borrow_mut().take_animation_frame_callbacks();
        for callback in frame_callbacks {
            let callback_start = Instant::now();
            if self.call_registered_callback(callback, std::slice::from_ref(&frame_timestamp))? {
                ran += 1;
            }
            trace_host_callback(trace_callbacks, callback, callback_start.elapsed());
        }
        let jobs_start = Instant::now();
        self.run_pending_jobs();
        trace_host_callback_jobs(trace_callbacks, jobs_start.elapsed());
        Ok(ran)
    }

    /// Run queued host callbacks with the default per-tick timer budget.
    pub fn run_ready_host_callbacks(&mut self) -> Result<usize, String> {
        self.run_host_callbacks(DEFAULT_HOST_CALLBACK_BUDGET)
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

    fn call_registered_callback(
        &mut self,
        scheduled: ScheduledHostCallback,
        args: &[JsValue],
    ) -> Result<bool, String> {
        let Some(callback) = self.registered_callback(scheduled.id)? else {
            return Ok(false);
        };
        if !scheduled.repeat {
            self.clear_registered_callback(scheduled.id)?;
        }
        callback
            .call(&JsValue::undefined(), args, &mut self.ctx)
            .map_err(|err| format!("{err}"))?;
        Ok(true)
    }

    fn registered_callback(&mut self, id: u32) -> Result<Option<JsObject>, String> {
        let registry = host_callback_registry(&mut self.ctx).map_err(|err| format!("{err}"))?;
        let value = registry
            .get(id, &mut self.ctx)
            .map_err(|err| format!("{err}"))?;
        Ok(value.as_object().filter(boa_engine::JsObject::is_callable))
    }

    fn clear_registered_callback(&mut self, id: u32) -> Result<(), String> {
        let registry = host_callback_registry(&mut self.ctx).map_err(|err| format!("{err}"))?;
        registry
            .set(id, JsValue::undefined(), false, &mut self.ctx)
            .map_err(|err| format!("{err}"))?;
        Ok(())
    }

    fn frame_timestamp_ms(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64() * 1000.0
    }
}

fn install_host_scheduler(ctx: &mut Context, scheduler: &HostSchedulerRef) {
    let _ = host_callback_registry(ctx);

    let timeout_scheduler = Rc::clone(scheduler);
    // SAFETY: Boa stores the native closure with owned scheduler captures for the JS function lifetime.
    let set_timeout = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            register_host_callback(
                ctx,
                &timeout_scheduler,
                HostCallbackQueue::Timeout,
                args.first(),
            )
        })
    };
    ctx.register_global_callable(js_string!("setTimeout"), 2, set_timeout)
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

    .expect("setTimeout: install on fresh context cannot fail");

    let interval_scheduler = Rc::clone(scheduler);
    // SAFETY: Boa stores the native closure with owned scheduler captures for the JS function lifetime.
    let set_interval = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            register_host_callback(
                ctx,
                &interval_scheduler,
                HostCallbackQueue::Interval,
                args.first(),
            )
        })
    };
    ctx.register_global_callable(js_string!("setInterval"), 2, set_interval)
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("setInterval: install on fresh context cannot fail");

    let frame_scheduler = Rc::clone(scheduler);
    // SAFETY: Boa stores the native closure with owned scheduler captures for the JS function lifetime.
    let request_animation_frame = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            register_host_callback(
                ctx,
                &frame_scheduler,
                HostCallbackQueue::AnimationFrame,
                args.first(),
            )
        })
    };
    ctx.register_global_callable(
        js_string!("requestAnimationFrame"),
        1,
        request_animation_frame,
    )
    // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

    .expect("requestAnimationFrame: install on fresh context cannot fail");

    let clear_timeout_scheduler = Rc::clone(scheduler);
    // SAFETY: Boa stores the native closure with owned scheduler captures for the JS function lifetime.
    let clear_timeout = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            clear_host_callback(ctx, &clear_timeout_scheduler, args.first())
        })
    };
    ctx.register_global_callable(js_string!("clearTimeout"), 1, clear_timeout)
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("clearTimeout: install on fresh context cannot fail");

    let clear_interval_scheduler = Rc::clone(scheduler);
    // SAFETY: Boa stores the native closure with owned scheduler captures for the JS function lifetime.
    let clear_interval = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            clear_host_callback(ctx, &clear_interval_scheduler, args.first())
        })
    };
    ctx.register_global_callable(js_string!("clearInterval"), 1, clear_interval)
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("clearInterval: install on fresh context cannot fail");

    let cancel_frame_scheduler = Rc::clone(scheduler);
    // SAFETY: Boa stores the native closure with owned scheduler captures for the JS function lifetime.
    let cancel_animation_frame = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            clear_host_callback(ctx, &cancel_frame_scheduler, args.first())
        })
    };
    ctx.register_global_callable(
        js_string!("cancelAnimationFrame"),
        1,
        cancel_animation_frame,
    )
    // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

    .expect("cancelAnimationFrame: install on fresh context cannot fail");
}

fn register_host_callback(
    ctx: &mut Context,
    scheduler: &HostSchedulerRef,
    queue: HostCallbackQueue,
    callback: Option<&JsValue>,
) -> boa_engine::JsResult<JsValue> {
    let Some(callback) = callback.and_then(JsValue::as_object) else {
        return Ok(JsValue::from(0_u32));
    };
    if !callback.is_callable() {
        return Ok(JsValue::from(0_u32));
    }

    let id = scheduler.borrow_mut().schedule(queue);
    host_callback_registry(ctx)?.set(id, callback.clone(), false, ctx)?;
    Ok(JsValue::from(id))
}

fn clear_host_callback(
    ctx: &mut Context,
    scheduler: &HostSchedulerRef,
    callback_id: Option<&JsValue>,
) -> boa_engine::JsResult<JsValue> {
    let id = callback_id.and_then(JsValue::as_number).unwrap_or_default() as u32;
    if id != 0 {
        scheduler.borrow_mut().cancel(id);
        host_callback_registry(ctx)?.set(id, JsValue::undefined(), false, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn host_callback_registry(ctx: &mut Context) -> boa_engine::JsResult<JsObject> {
    let key = js_string!(HOST_CALLBACKS_REGISTRY);
    let global = ctx.global_object().clone();
    let existing = global.get(key.clone(), ctx)?;
    if let Some(registry) = existing.as_object() {
        return Ok(registry.clone());
    }

    let registry = ObjectInitializer::new(ctx).build();
    global.set(key, registry.clone(), false, ctx)?;
    Ok(registry)
}

fn trace_host_callbacks_enabled() -> bool {
    std::env::var_os(TRACE_HOST_CALLBACKS_ENV).is_some()
}

fn trace_host_callback(
    enabled: bool,
    callback: ScheduledHostCallback,
    elapsed: std::time::Duration,
) {
    if enabled {
        eprintln!(
            "[SilkSurf] Host callback {} id={} repeat={} elapsed={:?}",
            callback.queue.label(),
            callback.id,
            callback.repeat,
            elapsed
        );
    }
}

fn trace_host_callback_jobs(enabled: bool, elapsed: std::time::Duration) {
    if enabled {
        eprintln!("[SilkSurf] Host callback jobs elapsed={elapsed:?}");
    }
}

fn install_websocket(ctx: &mut Context) {
    let websocket = FunctionObjectBuilder::new(
        ctx.realm(),
        NativeFunction::from_fn_ptr(websocket_constructor),
    )
    .name(js_string!("WebSocket"))
    .length(1)
    .constructor(true)
    .build();

    ctx.register_global_property(js_string!("WebSocket"), websocket, Attribute::all())
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("WebSocket: install on fresh context cannot fail");
}

fn install_stream_constructors(ctx: &mut Context) {
    register_constructor(ctx, "ReadableStream", readable_stream_constructor, 1);
    register_constructor(ctx, "WritableStream", writable_stream_constructor, 1);
    register_constructor(ctx, "TransformStream", transform_stream_constructor, 1);
    register_constructor(ctx, "TextEncoderStream", transform_stream_constructor, 0);
    register_constructor(ctx, "TextDecoderStream", transform_stream_constructor, 0);
}

fn register_constructor(
    ctx: &mut Context,
    name: &'static str,
    constructor: fn(&JsValue, &[JsValue], &mut Context) -> boa_engine::JsResult<JsValue>,
    length: usize,
) {
    let function =
        FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(constructor))
            .name(JsString::from(name))
            .length(length)
            .constructor(true)
            .build();

    ctx.register_global_property(JsString::from(name), function, Attribute::all())
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("stream constructor install on fresh context cannot fail");
}

fn readable_stream_constructor(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let controller = stream_controller(ctx);
    if let Some(source) = args.first().and_then(JsValue::as_object) {
        call_optional_method(
            &source,
            js_string!("start"),
            std::slice::from_ref(&controller),
            ctx,
        )?;
    }
    let stream = readable_stream_object(controller, ctx);
    Ok(stream.into())
}

fn writable_stream_constructor(
    _this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    Ok(writable_stream_object(ctx).into())
}

fn transform_stream_constructor(
    _this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let readable = readable_stream_object(stream_controller(ctx), ctx);
    let writable = writable_stream_object(ctx);
    let transform = ObjectInitializer::new(ctx)
        .property(js_string!("readable"), readable, Attribute::all())
        .property(js_string!("writable"), writable, Attribute::all())
        .build();
    Ok(transform.into())
}

fn stream_controller(ctx: &mut Context) -> JsValue {
    ObjectInitializer::new(ctx)
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("enqueue"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("close"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            js_string!("error"),
            1,
        )
        .build()
        .into()
}

fn readable_stream_object(controller: JsValue, ctx: &mut Context) -> JsObject {
    ObjectInitializer::new(ctx)
        .property(js_string!("locked"), false, Attribute::all())
        .property(js_string!("controller"), controller, Attribute::all())
        .function(
            NativeFunction::from_fn_ptr(stream_pipe_through),
            js_string!("pipeThrough"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(resolved_undefined_promise),
            js_string!("pipeTo"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(stream_get_reader),
            js_string!("getReader"),
            0,
        )
        .build()
}

fn writable_stream_object(ctx: &mut Context) -> JsObject {
    ObjectInitializer::new(ctx)
        .property(js_string!("locked"), false, Attribute::all())
        .function(
            NativeFunction::from_fn_ptr(resolved_undefined_promise),
            js_string!("close"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(resolved_undefined_promise),
            js_string!("abort"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(resolved_undefined_promise),
            js_string!("write"),
            1,
        )
        .build()
}

fn stream_pipe_through(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    if let Some(transform) = args.first().and_then(JsValue::as_object) {
        let readable = transform.get(js_string!("readable"), ctx)?;
        if readable.as_object().is_some() {
            return Ok(readable);
        }
    }
    Ok(this.clone())
}

fn stream_get_reader(
    _this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let reader = ObjectInitializer::new(ctx)
        .function(
            NativeFunction::from_fn_ptr(stream_reader_read),
            js_string!("read"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(resolved_undefined_promise),
            js_string!("releaseLock"),
            0,
        )
        .build();
    Ok(reader.into())
}

fn stream_reader_read(
    _this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let chunk = ObjectInitializer::new(ctx)
        .property(js_string!("done"), true, Attribute::all())
        .property(js_string!("value"), JsValue::undefined(), Attribute::all())
        .build();
    Ok(JsValue::from(JsPromise::from_result::<
        JsValue,
        JsNativeError,
    >(Ok(chunk.into()), ctx)))
}

fn resolved_undefined_promise(
    _this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    Ok(JsValue::from(JsPromise::from_result::<
        JsValue,
        JsNativeError,
    >(Ok(JsValue::undefined()), ctx)))
}

fn call_optional_method(
    object: &JsObject,
    name: JsString,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<()> {
    let method = object.get(name, ctx)?;
    let Some(callback) = method.as_object().filter(boa_engine::JsObject::is_callable) else {
        return Ok(());
    };
    callback.call(&JsValue::from(object.clone()), args, ctx)?;
    Ok(())
}

fn websocket_constructor(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(url_value) = args.first() else {
        return Err(JsNativeError::typ()
            .with_message("WebSocket: URL argument is required")
            .into());
    };
    let url = url_value.to_string(ctx)?.to_std_string_lossy();

    let socket = ObjectInitializer::new(ctx)
        .property(
            js_string!("url"),
            JsString::from(url.as_str()),
            Attribute::all(),
        )
        .property(js_string!("readyState"), 1_u32, Attribute::all())
        .property(js_string!("lastMessage"), js_string!(""), Attribute::all())
        .property(js_string!("lastError"), js_string!(""), Attribute::all())
        .property(js_string!("onopen"), JsValue::null(), Attribute::all())
        .property(js_string!("onmessage"), JsValue::null(), Attribute::all())
        .property(js_string!("onerror"), JsValue::null(), Attribute::all())
        .property(js_string!("onclose"), JsValue::null(), Attribute::all())
        .function(
            NativeFunction::from_fn_ptr(websocket_send),
            js_string!("send"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(websocket_close),
            js_string!("close"),
            0,
        )
        .build();

    Ok(JsValue::from(socket))
}

fn websocket_send(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let socket = websocket_receiver(this)?;
    let url = socket
        .get(js_string!("url"), ctx)?
        .to_string(ctx)?
        .to_std_string_lossy();
    let payload = args
        .first()
        .map(|value| value.to_string(ctx))
        .transpose()?
        .map(|value| value.to_std_string_lossy())
        .unwrap_or_default();

    match silksurf_net::websocket_text_roundtrip(url.as_str(), payload.as_str()) {
        Ok(reply) => {
            let message = websocket_reply_text(reply);
            socket.set(
                js_string!("lastMessage"),
                JsString::from(message.as_str()),
                false,
                ctx,
            )?;
            call_websocket_handler(
                &socket,
                js_string!("onmessage"),
                websocket_message_event(message.as_str(), ctx),
                ctx,
            )?;
        }
        Err(err) => {
            socket.set(js_string!("readyState"), 3_u32, false, ctx)?;
            socket.set(
                js_string!("lastError"),
                JsString::from(err.message.as_str()),
                false,
                ctx,
            )?;
            call_websocket_handler(
                &socket,
                js_string!("onerror"),
                websocket_error_event(err.message.as_str(), ctx),
                ctx,
            )?;
        }
    }

    Ok(JsValue::undefined())
}

fn websocket_close(
    this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let socket = websocket_receiver(this)?;
    socket.set(js_string!("readyState"), 3_u32, false, ctx)?;
    let close_event = ObjectInitializer::new(ctx)
        .property(js_string!("type"), js_string!("close"), Attribute::all())
        .build();
    call_websocket_handler(&socket, js_string!("onclose"), close_event.into(), ctx)?;
    Ok(JsValue::undefined())
}

fn websocket_receiver(this: &JsValue) -> boa_engine::JsResult<JsObject> {
    this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WebSocket method requires a WebSocket object")
            .into()
    })
}

fn websocket_reply_text(reply: silksurf_net::WebSocketReply) -> String {
    match reply {
        silksurf_net::WebSocketReply::Text(text) => text,
        silksurf_net::WebSocketReply::Binary(bytes) => {
            String::from_utf8_lossy(bytes.as_slice()).to_string()
        }
        silksurf_net::WebSocketReply::Close => String::new(),
    }
}

fn websocket_message_event(message_data: &str, ctx: &mut Context) -> JsValue {
    ObjectInitializer::new(ctx)
        .property(js_string!("type"), js_string!("message"), Attribute::all())
        .property(
            js_string!("data"),
            JsString::from(message_data),
            Attribute::all(),
        )
        .build()
        .into()
}

fn websocket_error_event(message: &str, ctx: &mut Context) -> JsValue {
    ObjectInitializer::new(ctx)
        .property(js_string!("type"), js_string!("error"), Attribute::all())
        .property(
            js_string!("message"),
            JsString::from(message),
            Attribute::all(),
        )
        .build()
        .into()
}

fn call_websocket_handler(
    socket: &JsObject,
    handler_name: JsString,
    event: JsValue,
    ctx: &mut Context,
) -> boa_engine::JsResult<()> {
    let handler = socket.get(handler_name, ctx)?;
    let Some(callback) = handler
        .as_object()
        .filter(boa_engine::JsObject::is_callable)
    else {
        return Ok(());
    };
    callback.call(&JsValue::from(socket.clone()), &[event], ctx)?;
    Ok(())
}

// ---- fetch() implementation ------------------------------------------------

fn fetch_input_url(input: &JsValue, ctx: &mut Context) -> boa_engine::JsResult<String> {
    if let Some(object) = input.as_object() {
        let url = object.get(js_string!("url"), ctx)?;
        if !url.is_undefined() {
            return Ok(url.to_string(ctx)?.to_std_string_lossy());
        }
    }
    Ok(input.to_string(ctx)?.to_std_string_lossy())
}

fn fetch_signal_is_aborted(args: &[JsValue], ctx: &mut Context) -> boa_engine::JsResult<bool> {
    let init_signal = args
        .get(1)
        .and_then(JsValue::as_object)
        .map(|init| init.get(js_string!("signal"), ctx))
        .transpose()?;
    let input_signal = args
        .first()
        .and_then(JsValue::as_object)
        .map(|input| input.get(js_string!("signal"), ctx))
        .transpose()?;

    for signal in [init_signal, input_signal] {
        let Some(signal) = signal.and_then(|value| value.as_object()) else {
            continue;
        };
        if signal_is_aborted(&signal, ctx)? {
            return Ok(true);
        }
    }
    Ok(false)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn global_bool(ctx: &mut SilkContext, name: &str) -> bool {
        global_value(ctx, name).as_boolean().unwrap_or(false)
    }

    fn global_number(ctx: &mut SilkContext, name: &str) -> f64 {
        global_value(ctx, name).as_number().unwrap_or(f64::NAN)
    }

    fn assert_number_eq(ctx: &mut SilkContext, name: &str, expected: f64) {
        let actual = global_number(ctx, name);
        assert!(
            (actual - expected).abs() <= f64::EPSILON,
            "{name}: actual={actual}, expected={expected}"
        );
    }

    fn global_string(ctx: &mut SilkContext, name: &str) -> String {
        global_value(ctx, name)
            .to_string(&mut ctx.ctx)
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("global property converts to string")
            .to_std_string_lossy()
    }

    fn global_value(ctx: &mut SilkContext, name: &str) -> JsValue {
        let global = ctx.ctx.global_object().clone();
        global
            .get(JsString::from(name), &mut ctx.ctx)
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("global property read succeeds")
    }

    fn minimal_document() -> (Arc<Mutex<silksurf_dom::Dom>>, silksurf_dom::NodeId) {
        let mut dom = silksurf_dom::Dom::new();
        let document = dom.create_document();
        let html = dom.create_element("html");
        let head = dom.create_element("head");
        let body = dom.create_element("body");
        dom.append_child(document, html).expect("html attaches");
        dom.append_child(html, head).expect("head attaches");
        dom.append_child(html, body).expect("body attaches");
        (Arc::new(Mutex::new(dom)), document)
    }

    fn start_websocket_echo_server() -> (String, std::thread::JoinHandle<()>) {
        use futures_util::{SinkExt, StreamExt};
        use std::net::TcpListener;
        use tokio::runtime::Builder;
        use tokio_tungstenite::accept_async;

        let listener = TcpListener::bind("127.0.0.1:0").expect("echo server binds");
        let addr = listener.local_addr().expect("echo server has local addr");
        listener
            .set_nonblocking(true)
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("echo server socket enters nonblocking mode");
        let handle = std::thread::spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_io()
                .build()
                // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

                .expect("echo server runtime builds");
            runtime.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(listener).expect("tokio listener imports");
                let (stream, _) = listener.accept().await.expect("echo server accepts");
                let mut socket = accept_async(stream)
                    .await
                    // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

                    .expect("websocket handshake works");
                if let Some(Ok(message)) = socket.next().await {
                    socket.send(message).await.expect("echo server replies");
                }
            });
        });

        (format!("ws://{addr}"), handle)
    }

    #[test]
    fn local_storage_tracks_items_and_length() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "localStorage.setItem('a', '1'); \
             localStorage.setItem('b', '2'); \
             globalThis.firstValue = localStorage.getItem('a'); \
             globalThis.initialLength = localStorage.length; \
             globalThis.firstKey = localStorage.key(0); \
             localStorage.removeItem('a'); \
             globalThis.missingValue = localStorage.getItem('a') === null; \
             globalThis.finalLength = localStorage.length;",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script mutates localStorage");

        assert_eq!(global_string(&mut ctx, "firstValue"), "1");
        assert_number_eq(&mut ctx, "initialLength", 2.0);
        assert_eq!(global_string(&mut ctx, "firstKey"), "a");
        assert!(global_bool(&mut ctx, "missingValue"));
        assert_number_eq(&mut ctx, "finalLength", 1.0);
    }

    #[test]
    fn dynamic_script_element_preserves_src_and_inner_html() {
        let (dom, document) = minimal_document();
        let mut ctx = SilkContext::with_dom(&dom);

        ctx.eval(
            "var script = document.createElement('script'); \
             script.src = '/cdn/dynamic.js'; \
             script.innerHTML = 'globalThis.dynamicScriptRan = true;'; \
             document.head.appendChild(script); \
             globalThis.dynamicScriptNode = script.src;",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script appends dynamic script element");

        let dom = dom
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let scripts = dom
            .children(document)
            // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

            .expect("document has children")
            .iter()
            .copied()
            .flat_map(|node| collect_script_nodes_for_test(&dom, node))
            .collect::<Vec<_>>();
        let script = scripts.first().copied().expect("dynamic script exists");
        let attrs = dom.attributes(script).expect("script has attributes");
        let src = attrs
            .iter()
            .find(|attr| attr.name.as_str() == "src")
            .map(|attr| attr.value.as_str())
            .unwrap_or_default();

        assert_eq!(
            global_string(&mut ctx, "dynamicScriptNode"),
            "/cdn/dynamic.js"
        );
        assert_eq!(src, "/cdn/dynamic.js");
        assert_eq!(
            dom.children(script)
                // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

                .expect("script has text child")
                .iter()
                .filter_map(|child| match dom.node(*child).ok()?.kind() {
                    silksurf_dom::NodeKind::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>(),
            "globalThis.dynamicScriptRan = true;"
        );
    }

    fn collect_script_nodes_for_test(
        dom: &silksurf_dom::Dom,
        node: silksurf_dom::NodeId,
    ) -> Vec<silksurf_dom::NodeId> {
        let mut nodes = Vec::new();
        if dom
            .element_name(node)
            .ok()
            .flatten()
            .is_some_and(|name| name == "script")
        {
            nodes.push(node);
        }
        if let Ok(children) = dom.children(node) {
            for &child in children {
                nodes.extend(collect_script_nodes_for_test(dom, child));
            }
        }
        nodes
    }

    #[test]
    fn session_storage_is_separate_from_local_storage() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "localStorage.setItem('token', 'local'); \
             sessionStorage.setItem('token', 'session'); \
             globalThis.localValue = localStorage.getItem('token'); \
             globalThis.sessionValue = sessionStorage.getItem('token'); \
             globalThis.localLength = localStorage.length; \
             globalThis.sessionLength = sessionStorage.length; \
             sessionStorage.clear(); \
             globalThis.sessionCleared = sessionStorage.length === 0; \
             globalThis.localStillPresent = localStorage.getItem('token');",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script mutates localStorage and sessionStorage");

        assert_eq!(global_string(&mut ctx, "localValue"), "local");
        assert_eq!(global_string(&mut ctx, "sessionValue"), "session");
        assert_number_eq(&mut ctx, "localLength", 1.0);
        assert_number_eq(&mut ctx, "sessionLength", 1.0);
        assert!(global_bool(&mut ctx, "sessionCleared"));
        assert_eq!(global_string(&mut ctx, "localStillPresent"), "local");
    }

    #[test]
    fn document_cookie_tracks_name_value_pairs() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "document.cookie = 'sid=abc; Path=/; SameSite=Lax'; \
             document.cookie = 'theme=dark'; \
             document.cookie = 'sid=def'; \
             globalThis.cookiesEnabled = navigator.cookieEnabled; \
             globalThis.cookieText = document.cookie; \
             document.cookie = 'theme=gone; Max-Age=0'; \
             globalThis.afterDelete = document.cookie;",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script mutates document.cookie");

        assert!(global_bool(&mut ctx, "cookiesEnabled"));
        assert_eq!(global_string(&mut ctx, "cookieText"), "sid=def; theme=dark");
        assert_eq!(global_string(&mut ctx, "afterDelete"), "sid=def");
    }

    #[test]
    fn crypto_get_random_values_fills_typed_array() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "var bytes = new Uint8Array(16); \
             var returned = crypto.getRandomValues(bytes); \
             globalThis.sameObject = returned === bytes; \
             globalThis.lengthOk = bytes.length === 16; \
             globalThis.sum = Array.from(bytes).reduce(function (acc, byte) { return acc + byte; }, 0);",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script fills typed array with random bytes");

        assert!(global_bool(&mut ctx, "sameObject"));
        assert!(global_bool(&mut ctx, "lengthOk"));
        assert!(global_number(&mut ctx, "sum") > 0.0);
    }

    #[test]
    fn crypto_random_uuid_returns_v4_shape() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "var id = crypto.randomUUID(); \
             globalThis.uuidText = id; \
             globalThis.uuidShape = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(id);",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script creates random UUID");

        assert_eq!(global_string(&mut ctx, "uuidText").len(), 36);
        assert!(global_bool(&mut ctx, "uuidShape"));
    }

    #[test]
    fn abort_controller_updates_signal_and_listener() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "var controller = new AbortController(); \
             globalThis.initialAbort = controller.signal.aborted; \
             globalThis.abortHit = false; \
             globalThis.abortEventType = ''; \
             controller.signal.addEventListener('abort', function (event) { \
               globalThis.abortHit = true; \
               globalThis.abortEventType = event.type; \
             }); \
             controller.abort('stop'); \
             globalThis.finalAbort = controller.signal.aborted; \
             globalThis.abortReason = controller.signal.reason;",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script aborts controller");

        assert!(!global_bool(&mut ctx, "initialAbort"));
        assert!(global_bool(&mut ctx, "finalAbort"));
        assert!(global_bool(&mut ctx, "abortHit"));
        assert_eq!(global_string(&mut ctx, "abortEventType"), "abort");
        assert_eq!(global_string(&mut ctx, "abortReason"), "stop");
    }

    #[test]
    fn abort_signal_static_abort_returns_aborted_signal() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "var signal = AbortSignal.abort('done'); \
             globalThis.staticAbort = signal.aborted; \
             globalThis.staticReason = signal.reason; \
             try { signal.throwIfAborted(); } catch (err) { globalThis.throwHit = true; }",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script creates aborted signal");

        assert!(global_bool(&mut ctx, "staticAbort"));
        assert_eq!(global_string(&mut ctx, "staticReason"), "done");
        assert!(global_bool(&mut ctx, "throwHit"));
    }

    #[test]
    fn fetch_rejects_pre_aborted_signal() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "var controller = new AbortController(); \
             controller.abort(); \
             globalThis.fetchRejected = false; \
             fetch('http://127.0.0.1:1/', { signal: controller.signal }) \
               .then(function () { globalThis.fetchResolved = true; }) \
               .catch(function (err) { \
                 globalThis.fetchRejected = true; \
                 globalThis.fetchError = String(err); \
               });",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script rejects aborted fetch");

        assert!(global_bool(&mut ctx, "fetchRejected"));
        assert!(global_string(&mut ctx, "fetchError").contains("aborted"));
    }

    #[test]
    fn set_timeout_defers_until_host_tick() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "globalThis.hit = false; \
             globalThis.timerId = setTimeout(function () { globalThis.hit = true; }, 0);",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script schedules timeout");

        assert!(!global_bool(&mut ctx, "hit"));
        assert!(global_number(&mut ctx, "timerId") > 0.0);
        assert!(ctx.has_pending_host_callbacks());
        assert_eq!(ctx.run_ready_host_callbacks().unwrap(), 1);
        assert!(global_bool(&mut ctx, "hit"));
        assert!(!ctx.has_pending_host_callbacks());
    }

    #[test]
    fn request_animation_frame_runs_once_with_timestamp() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "globalThis.frames = 0; \
             globalThis.frameTimestamp = -1; \
             requestAnimationFrame(function (timestamp) { \
               globalThis.frames += 1; \
               globalThis.frameTimestamp = timestamp; \
             });",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script schedules frame callback");

        assert_eq!(ctx.run_ready_host_callbacks().unwrap(), 1);
        assert_number_eq(&mut ctx, "frames", 1.0);
        assert!(global_number(&mut ctx, "frameTimestamp") >= 0.0);
        assert_eq!(ctx.run_ready_host_callbacks().unwrap(), 0);
    }

    #[test]
    fn canceled_animation_frame_does_not_run() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "globalThis.hit = false; \
             var id = requestAnimationFrame(function () { globalThis.hit = true; }); \
             cancelAnimationFrame(id);",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script schedules and cancels frame callback");

        assert!(!ctx.has_pending_host_callbacks());
        assert_eq!(ctx.run_ready_host_callbacks().unwrap(), 0);
        assert!(!global_bool(&mut ctx, "hit"));
    }

    #[test]
    fn interval_repeats_until_cleared() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "globalThis.count = 0; \
             var id = setInterval(function () { \
               globalThis.count += 1; \
               if (globalThis.count === 2) { clearInterval(id); } \
             }, 1);",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script schedules interval");

        assert_eq!(ctx.run_ready_host_callbacks().unwrap(), 1);
        assert_number_eq(&mut ctx, "count", 1.0);
        assert!(ctx.has_pending_host_callbacks());
        assert_eq!(ctx.run_ready_host_callbacks().unwrap(), 1);
        assert_number_eq(&mut ctx, "count", 2.0);
        assert!(!ctx.has_pending_host_callbacks());
    }

    #[test]
    fn websocket_send_invokes_onmessage() {
        let (url, server) = start_websocket_echo_server();
        let mut ctx = SilkContext::new();
        ctx.eval(
            format!(
                "globalThis.wsHit = false; \
                 globalThis.wsData = ''; \
                 var ws = new WebSocket('{url}'); \
                 ws.onmessage = function (event) {{ \
                   globalThis.wsHit = true; \
                   globalThis.wsData = event.data; \
                 }}; \
                 ws.send('hello-ai'); \
                 globalThis.wsLast = ws.lastMessage; \
                 globalThis.wsReady = ws.readyState;"
            )
            .as_str(),
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script sends websocket message");

        server.join().expect("echo server exits");
        assert!(global_bool(&mut ctx, "wsHit"));
        assert_eq!(global_string(&mut ctx, "wsData"), "hello-ai");
        assert_eq!(global_string(&mut ctx, "wsLast"), "hello-ai");
        assert_number_eq(&mut ctx, "wsReady", 1.0);
    }

    #[test]
    fn websocket_send_invokes_onerror_for_invalid_url() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "globalThis.wsErrorHit = false; \
             globalThis.wsErrorMessage = ''; \
             var ws = new WebSocket('ws://127.0.0.1:1'); \
             ws.onerror = function (event) { \
               globalThis.wsErrorHit = true; \
               globalThis.wsErrorMessage = event.message; \
             }; \
             ws.send('hello'); \
             globalThis.wsReady = ws.readyState; \
             globalThis.wsLastError = ws.lastError;",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("script handles websocket error");

        assert!(global_bool(&mut ctx, "wsErrorHit"));
        assert!(!global_string(&mut ctx, "wsErrorMessage").is_empty());
        assert!(!global_string(&mut ctx, "wsLastError").is_empty());
        assert_number_eq(&mut ctx, "wsReady", 3.0);
    }

    #[test]
    fn readable_stream_invokes_start_with_controller() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "globalThis.streamStarted = false; \
             globalThis.controllerHasEnqueue = false; \
             var stream = new ReadableStream({ \
               start: function (controller) { \
                 globalThis.streamStarted = true; \
                 globalThis.controllerHasEnqueue = typeof controller.enqueue === 'function'; \
                 controller.enqueue('chunk'); \
                 controller.close(); \
               } \
             }); \
             globalThis.streamHasReader = typeof stream.getReader === 'function';",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("readable stream constructor runs start");

        assert!(global_bool(&mut ctx, "streamStarted"));
        assert!(global_bool(&mut ctx, "controllerHasEnqueue"));
        assert!(global_bool(&mut ctx, "streamHasReader"));
    }

    #[test]
    fn transform_stream_pipe_through_returns_readable() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "var input = new ReadableStream(); \
             var encoded = input.pipeThrough(new TextEncoderStream()); \
             globalThis.hasPipeTo = typeof encoded.pipeTo === 'function'; \
             globalThis.hasReader = typeof encoded.getReader === 'function';",
        )
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("stream pipeThrough returns readable side");

        assert!(global_bool(&mut ctx, "hasPipeTo"));
        assert!(global_bool(&mut ctx, "hasReader"));
    }
}
